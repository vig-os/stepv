//! Measurements between B-rep entities (#33): distances and angles on the
//! exact shapes, from a long-lived, sandboxed `stepv-occt --serve`.
//!
//! Single-entity facts (a radius, an area) come from `--topology`; anything
//! between two entities needs the kernel (`BRepExtrema_DistShapeShape`).
//! [`Server`] keeps one kernel process per file, reading it once and then
//! answering newline-JSON queries (protocol in `kernel/measure.h`).
//!
//! The kernel is untrusted: a file can make it hang, balloon or crash. Every
//! query gets the run's limits (wall clock, memory footprint); past either,
//! or when the process dies, it is killed, restarted on the next query, and
//! the query reports what happened. The caller never hangs on it.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::occt::{Limits, footprint, full_sandbox};

/// A B-rep entity, numbered as `--topology` numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entity {
    Face { part: u32, face: u32 },
    Edge { part: u32, edge: u32 },
}

impl Entity {
    fn to_json(self) -> Value {
        match self {
            Self::Face { part, face } => json!({ "part": part, "face": face }),
            Self::Edge { part, edge } => json!({ "part": part, "edge": edge }),
        }
    }
}

/// A question for the kernel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Query {
    /// The minimum distance between two entities, with its witness points,
    /// and the distance between their axes when both have one.
    Distance(Entity, Entity),
    /// The angle between two entities' axes (planes: outward normals).
    Angle(Entity, Entity),
    /// The point of an entity nearest `near`, and a face's normal there.
    Point(Entity, [f64; 3]),
}

/// The kernel's answer to a [`Query`], all in millimetres and model
/// coordinates.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Answer {
    pub distance: Option<f64>,
    /// The witness points of [`Self::distance`]: on the first entity, then
    /// on the second.
    pub points: Option<[[f64; 3]; 2]>,
    pub axis_distance: Option<f64>,
    pub angle_deg: Option<f64>,
    pub point: Option<[f64; 3]>,
    pub normal: Option<[f64; 3]>,
}

/// Why a query has no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The kernel could not read the file at all.
    Load(String),
    /// The kernel answered, but no: "no such entity", an angle between two
    /// spheres, …
    Refused(String),
    /// Past the time limit: killed; the next query restarts it.
    Timeout,
    /// Past the memory limit: killed; the next query restarts it.
    MemoryCap,
    /// The kernel died or spoke nonsense: restarted on the next query.
    Crashed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load(e) => write!(f, "the kernel cannot read the file: {e}"),
            Self::Refused(e) => write!(f, "{e}"),
            Self::Timeout => write!(f, "the measurement took too long; the kernel was restarted"),
            Self::MemoryCap => write!(
                f,
                "the measurement used too much memory; the kernel was restarted"
            ),
            Self::Crashed(e) => write!(f, "the kernel crashed ({e}); it will be restarted"),
        }
    }
}

impl std::error::Error for Error {}

/// One running kernel.
struct Process {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    /// The sandbox it reported.
    sandbox: String,
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A measurement server for one file. Starts the kernel on first use and
/// again after anything kills it.
pub struct Server {
    kernel: PathBuf,
    input: PathBuf,
    limits: Limits,
    process: Option<Process>,
    next_id: u64,
    /// Extra environment for the kernel (tests: STEPV_OCCT_TEST_HOOKS).
    env: Vec<(String, String)>,
    /// How many times the kernel had to be (re)started.
    pub starts: u32,
}

impl Server {
    /// A server for `input`, run by `kernel` under `limits` per query (and
    /// per load). Starts nothing yet.
    #[must_use]
    pub fn new(kernel: &Path, input: &Path, limits: Limits) -> Self {
        Self {
            kernel: kernel.to_owned(),
            input: input.to_owned(),
            limits,
            process: None,
            next_id: 1,
            starts: 0,
            env: Vec::new(),
        }
    }

    /// Sets an environment variable for the kernels this server starts.
    #[doc(hidden)]
    #[must_use]
    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_owned(), value.to_owned()));
        self
    }

    /// The sandbox the running kernel reported, once started.
    #[must_use]
    pub fn sandbox(&self) -> Option<&str> {
        self.process.as_ref().map(|p| p.sandbox.as_str())
    }

    /// Whether the running kernel is fully sandboxed.
    #[must_use]
    pub fn sandboxed(&self) -> bool {
        self.sandbox().is_some_and(full_sandbox)
    }

    /// The kernel's pid, while one runs (tests kill it).
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.process.as_ref().map(|p| p.child.id())
    }

    fn start(&mut self) -> Result<(), Error> {
        let mut cmd = Command::new(&self.kernel);
        cmd.envs(self.env.iter().map(|(k, v)| (k, v)));
        cmd.env_remove("LD_LIBRARY_PATH")
            .arg(&self.input)
            .arg("--serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|e| Error::Load(format!("cannot start the kernel: {e}")))?;
        self.starts += 1;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut p = Process {
            child,
            stdin,
            lines,
            sandbox: String::new(),
        };
        // The ready line, under the same limits as a query: reading the
        // file is where a hostile one does its worst.
        let ready = wait(&mut p, self.limits)?;
        let v: Value = serde_json::from_str(&ready)
            .map_err(|_| Error::Crashed(format!("not a ready line: {ready}")))?;
        p.sandbox = v["sandbox"].as_str().unwrap_or_default().to_owned();
        if v["ready"] != true {
            return Err(Error::Load(
                v["error"].as_str().unwrap_or("unknown error").to_owned(),
            ));
        }
        self.process = Some(p);
        Ok(())
    }

    /// Asks `query`, starting the kernel if it is not running.
    ///
    /// # Errors
    /// See [`Error`]. After a timeout, a memory cap or a crash, the kernel is
    /// gone, and the next call starts a new one.
    pub fn ask(&mut self, query: &Query) -> Result<Answer, Error> {
        let id = self.next_id;
        self.next_id += 1;
        let line = match *query {
            Query::Distance(a, b) => {
                json!({ "id": id, "op": "distance", "a": a.to_json(), "b": b.to_json() })
            }
            Query::Angle(a, b) => {
                json!({ "id": id, "op": "angle", "a": a.to_json(), "b": b.to_json() })
            }
            Query::Point(a, near) => {
                json!({ "id": id, "op": "point", "a": a.to_json(), "near": near })
            }
        };
        self.raw(&line)
    }

    /// Sends one query object as is (tests use it for the test hooks).
    ///
    /// # Errors
    /// See [`Server::ask`].
    #[doc(hidden)]
    pub fn raw(&mut self, query: &Value) -> Result<Answer, Error> {
        if self.process.is_none() {
            self.start()?;
        }
        let p = self.process.as_mut().expect("started above");
        let sent = writeln!(p.stdin, "{query}").and_then(|()| p.stdin.flush());
        if let Err(e) = sent {
            self.process = None;
            return Err(Error::Crashed(format!("cannot write the query: {e}")));
        }
        let reply = match wait(p, self.limits) {
            Ok(r) => r,
            Err(e) => {
                // Killed or dead: drop it (Drop kills and reaps).
                self.process = None;
                return Err(e);
            }
        };
        let v: Value = match serde_json::from_str(&reply) {
            Ok(v) => v,
            Err(_) => {
                self.process = None;
                return Err(Error::Crashed(format!("not an answer: {reply}")));
            }
        };
        if v["id"] != query["id"] {
            self.process = None;
            return Err(Error::Crashed(format!(
                "an answer to another query: {reply}"
            )));
        }
        if v["ok"] != true {
            return Err(Error::Refused(
                v["error"].as_str().unwrap_or("refused").to_owned(),
            ));
        }
        serde_json::from_value(v).map_err(|e| Error::Crashed(e.to_string()))
    }
}

/// The next line from `p`, within `limits`: the wall clock, and the
/// memory footprint polled meanwhile. Kills the kernel past either.
fn wait(p: &mut Process, limits: Limits) -> Result<String, Error> {
    let start = Instant::now();
    loop {
        match p.lines.recv_timeout(Duration::from_millis(5)) {
            Ok(line) => return Ok(line),
            Err(RecvTimeoutError::Disconnected) => {
                let status = p.child.wait().map(|s| s.to_string()).unwrap_or_default();
                return Err(Error::Crashed(format!("exited: {status}")));
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if start.elapsed() >= limits.timeout {
            let _ = p.child.kill();
            return Err(Error::Timeout);
        }
        if let Some(cap) = limits.memory
            && footprint(p.child.id()).is_some_and(|b| b > cap)
        {
            let _ = p.child.kill();
            return Err(Error::MemoryCap);
        }
    }
}

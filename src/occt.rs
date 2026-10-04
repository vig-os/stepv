//! The Plan B kernel: native OCCT in a subprocess (`kernel/stepv-occt`).
//!
//! The C++ side does the OCCT work and nothing else; this module runs it,
//! enforces the wall-clock cap by killing it, and turns its output into a
//! [`Scene`]. Running OCCT out of process is deliberate: a file that crashes
//! the kernel crashes a child, and the caller still gets an honest
//! [`Outcome::Crashed`] instead of a dead previewer. That is the isolation the
//! WASM sandbox was going to provide under Plan A (`plan.md` §3).
//!
//! The mesh file format is specified at the top of `kernel/stepv-occt.cpp`;
//! [`read_mesh`] is its only reader.

use std::io::Read;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::{BBox, Color, Deflection, FaceStatus, LineKind, Lines, Mesh, Part, Scene};

/// The kernel's JSON summary, one line on its stdout. Present on success AND
/// on a clean failure (exit 3) — the header-level facts survive a failed
/// tessellation, which is what the front-ends' honest degradation rests on.
#[derive(Debug, Clone, Deserialize)]
pub struct Summary {
    pub ok: bool,
    /// The stage reached, or the one that failed: `read`, `transfer`,
    /// `walk`, `mesh`, `extract`, `done`.
    pub stage: String,
    pub error: Option<String>,
    pub format: String,
    pub bbox: Option<[f64; 6]>,
    pub diagonal: f64,
    pub linear_abs: f64,
    pub parts: usize,
    pub parts_named: usize,
    /// Parts with a part-level colour, their own or inherited from an
    /// assembly or instance.
    pub parts_colored: usize,
    /// Parts with no part-level colour but colours on individual faces.
    pub parts_face_colored: usize,
    pub prototypes: usize,
    pub prototypes_mesh_failed: usize,
    pub faces: usize,
    /// Unique faces the FIRST meshing pass left without triangles. Each then
    /// went down the recovery ladder and landed in one of the seven below.
    pub faces_unmeshed: usize,
    pub faces_remeshed: usize,
    pub faces_healed: usize,
    pub faces_refined: usize,
    pub faces_coarse: usize,
    pub faces_degenerate: usize,
    pub faces_approx: usize,
    pub faces_missing: usize,
    /// Faceless parts in a file with no faces at all: drawn as curves.
    pub sketch_parts: usize,
    /// Faceless parts beside solids: construction geometry.
    pub construction_parts: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub segments: usize,
    pub t_read_ms: f64,
    pub t_transfer_ms: f64,
    pub t_mesh_ms: f64,
    pub t_extract_ms: f64,
    pub peak_rss_bytes: u64,
}

/// How a kernel run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Exit 0: geometry produced.
    Ok,
    /// Exit 3: a clean, reported failure. The summary is still valid.
    Failed,
    /// Killed by a signal, or an exit code outside the contract.
    Crashed {
        signal: Option<i32>,
        code: Option<i32>,
    },
    /// Exceeded the wall-clock cap and was killed.
    Timeout,
}

/// One kernel invocation.
#[derive(Debug)]
pub struct Run {
    pub outcome: Outcome,
    /// `None` when the kernel crashed or timed out before printing.
    pub summary: Option<Summary>,
    /// Parent-side wall-clock, process spawn included: what a user waits for.
    pub wall: Duration,
}

/// Where the kernel binary is: `$STEPV_OCCT`, else beside the current
/// executable, else this checkout's `target/kernel/` (`just kernel`).
#[must_use]
pub fn kernel_path() -> PathBuf {
    if let Some(p) = std::env::var_os("STEPV_OCCT") {
        return PathBuf::from(p);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        let beside = dir.join("stepv-occt");
        if beside.is_file() {
            return beside;
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("target/kernel/stepv-occt")
}

/// Runs the kernel on `input`, writing mesh buffers to `mesh_out` if given.
///
/// # Errors
/// Only when the kernel cannot be spawned at all. Every failure of the kernel
/// itself is reported through [`Run::outcome`].
pub fn run(
    kernel: &Path,
    input: &Path,
    deflection: Deflection,
    timeout: Duration,
    mesh_out: Option<&Path>,
) -> std::io::Result<Run> {
    let mut cmd = Command::new(kernel);
    cmd.arg(input)
        .arg("--linear-rel")
        .arg(deflection.linear_rel.to_string())
        .arg("--angular-deg")
        .arg(deflection.angular_deg.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(out) = mesh_out {
        cmd.arg("--mesh").arg(out);
    }

    let start = Instant::now();
    let mut child = cmd.spawn()?;
    // Drain stdout on a thread so a chatty kernel can never fill the pipe and
    // deadlock against our wait loop.
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        s
    });

    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let wall = start.elapsed();
    let text = reader.join().unwrap_or_default();
    let summary = text
        .lines()
        .next()
        .and_then(|l| serde_json::from_str(l).ok());

    let outcome = match status {
        None => Outcome::Timeout,
        Some(s) => match s.code() {
            Some(0) => Outcome::Ok,
            Some(3) => Outcome::Failed,
            code => Outcome::Crashed {
                signal: s.signal(),
                code,
            },
        },
    };
    Ok(Run {
        outcome,
        summary,
        wall,
    })
}

/// A malformed mesh file. The kernel wrote it, so any of these is a bug on
/// one side of the format, never bad user input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeshError {
    BadMagic,
    UnsupportedVersion(u32),
    /// A face-status or line-kind byte outside the contract.
    BadEnum(u8),
    Truncated,
    TrailingBytes(usize),
}

impl std::fmt::Display for MeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadMagic => write!(f, "not a STEPVMSH file"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported STEPVMSH version {v}"),
            Self::BadEnum(b) => write!(f, "invalid face-status or line-kind byte {b}"),
            Self::Truncated => write!(f, "STEPVMSH file is truncated"),
            Self::TrailingBytes(n) => write!(f, "{n} unexpected trailing bytes"),
        }
    }
}

impl std::error::Error for MeshError {}

struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], MeshError> {
        if self.0.len() < n {
            return Err(MeshError::Truncated);
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, MeshError> {
        Ok(self.take(1)?[0])
    }

    fn enums<T>(&mut self, count: usize, f: fn(u8) -> Option<T>) -> Result<Vec<T>, MeshError> {
        self.take(count)?
            .iter()
            .map(|&b| f(b).ok_or(MeshError::BadEnum(b)))
            .collect()
    }

    fn u32(&mut self) -> Result<u32, MeshError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    fn f32(&mut self) -> Result<f32, MeshError> {
        Ok(f32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    fn f64(&mut self) -> Result<f64, MeshError> {
        Ok(f64::from_le_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }

    /// `count` items of 4 bytes each. Sized against what remains BEFORE
    /// allocating, so a corrupt count cannot ask for gigabytes.
    fn vec4<T>(&mut self, count: usize, f: fn([u8; 4]) -> T) -> Result<Vec<T>, MeshError> {
        let bytes = self.take(count.checked_mul(4).ok_or(MeshError::Truncated)?)?;
        Ok(bytes
            .chunks_exact(4)
            .map(|c| f(c.try_into().expect("4 bytes")))
            .collect())
    }
}

const fn line_kind(b: u8) -> Option<LineKind> {
    match b {
        0 => Some(LineKind::Sketch),
        1 => Some(LineKind::MissingOutline),
        2 => Some(LineKind::Construction),
        _ => None,
    }
}

/// Decodes a `STEPVMSH` file into a [`Scene`].
///
/// # Errors
/// See [`MeshError`].
pub fn read_mesh(bytes: &[u8]) -> Result<Scene, MeshError> {
    let mut c = Cursor(bytes);
    if c.take(8).map_err(|_| MeshError::BadMagic)? != b"STEPVMSH" {
        return Err(MeshError::BadMagic);
    }
    let version = c.u32()?;
    if version != 2 {
        return Err(MeshError::UnsupportedVersion(version));
    }
    let mut b = [0.0; 6];
    for v in &mut b {
        *v = c.f64()?;
    }
    let bbox = BBox {
        min: [b[0], b[1], b[2]],
        max: [b[3], b[4], b[5]],
    };

    let part_count = c.u32()? as usize;
    // Every part is at least 33 bytes, so a count beyond that is corruption.
    if part_count > c.0.len() / 33 {
        return Err(MeshError::Truncated);
    }
    let mut parts = Vec::with_capacity(part_count);
    for _ in 0..part_count {
        let name_len = c.u32()? as usize;
        let name = c.take(name_len)?;
        let has_color = c.u8()? != 0;
        let (r, g, b) = (c.f32()?, c.f32()?, c.f32()?);
        let face_count = c.u32()? as usize;
        let faces = c.enums(face_count, FaceStatus::from_u8)?;
        let vertices = c.u32()? as usize;
        let triangles = c.u32()? as usize;
        let mesh = Mesh {
            positions: c.vec4(vertices * 3, f32::from_le_bytes)?,
            normals: c.vec4(vertices * 3, f32::from_le_bytes)?,
            indices: c.vec4(triangles * 3, u32::from_le_bytes)?,
            face_ids: c.vec4(triangles, u32::from_le_bytes)?,
        };
        let segments = c.u32()? as usize;
        let lines = Lines {
            positions: c.vec4(segments * 6, f32::from_le_bytes)?,
            kinds: c.enums(segments, line_kind)?,
        };
        parts.push(Part {
            name: (name_len > 0).then(|| String::from_utf8_lossy(name).into_owned()),
            color: has_color.then_some(Color { r, g, b }),
            mesh,
            faces,
            lines,
        });
    }
    if !c.0.is_empty() {
        return Err(MeshError::TrailingBytes(c.0.len()));
    }
    Ok(Scene { bbox, parts })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One-triangle, one-segment file, built the way the kernel writes it.
    fn one_triangle(name: &str) -> Vec<u8> {
        let mut v = b"STEPVMSH".to_vec();
        v.extend(2u32.to_le_bytes());
        for x in [0.0f64, 0.0, 0.0, 1.0, 1.0, 0.0] {
            v.extend(x.to_le_bytes());
        }
        v.extend(1u32.to_le_bytes());
        v.extend((name.len() as u32).to_le_bytes());
        v.extend(name.as_bytes());
        v.push(1);
        for x in [0.5f32, 0.25, 1.0] {
            v.extend(x.to_le_bytes());
        }
        v.extend(2u32.to_le_bytes()); // faces
        v.extend([FaceStatus::Ok as u8, FaceStatus::Missing as u8]);
        v.extend(3u32.to_le_bytes());
        v.extend(1u32.to_le_bytes());
        for x in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            v.extend(x.to_le_bytes());
        }
        for x in [0.0f32, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0] {
            v.extend(x.to_le_bytes());
        }
        for i in [0u32, 1, 2, 0] {
            v.extend(i.to_le_bytes());
        }
        v.extend(1u32.to_le_bytes()); // segments
        for x in [0.0f32, 0.0, 0.0, 1.0, 1.0, 0.0] {
            v.extend(x.to_le_bytes());
        }
        v.push(LineKind::MissingOutline as u8);
        v
    }

    #[test]
    fn decodes_a_well_formed_file() {
        let scene = read_mesh(&one_triangle("bolt")).unwrap();
        assert_eq!(scene.parts.len(), 1);
        let p = &scene.parts[0];
        assert_eq!(p.name.as_deref(), Some("bolt"));
        assert_eq!(
            p.color,
            Some(Color {
                r: 0.5,
                g: 0.25,
                b: 1.0
            })
        );
        assert!(p.is_well_formed());
        assert_eq!(p.faces, [FaceStatus::Ok, FaceStatus::Missing]);
        assert_eq!(p.lines.kinds, [LineKind::MissingOutline]);
        assert_eq!(scene.worst_face(), Some(FaceStatus::Missing));
        assert_eq!(scene.triangle_count(), 1);
        assert_eq!(scene.segment_count(), 1);
        assert!((scene.bbox.diagonal() - 2f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn empty_name_decodes_as_none() {
        assert_eq!(read_mesh(&one_triangle("")).unwrap().parts[0].name, None);
    }

    #[test]
    fn every_truncation_is_an_error_not_a_panic() {
        let full = one_triangle("bolt");
        for n in 0..full.len() {
            assert!(
                read_mesh(&full[..n]).is_err(),
                "prefix of {n} bytes decoded"
            );
        }
    }

    #[test]
    fn out_of_contract_status_byte_is_rejected() {
        let mut v = one_triangle("bolt");
        // magic 8 + version 4 + bbox 48 + count 4 + name 4+4 + colour 1+12 + faces 4
        v[89] = 9;
        assert_eq!(read_mesh(&v).unwrap_err(), MeshError::BadEnum(9));
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut v = one_triangle("bolt");
        v.push(0);
        assert_eq!(read_mesh(&v).unwrap_err(), MeshError::TrailingBytes(1));
    }

    #[test]
    fn absurd_part_count_does_not_allocate() {
        let mut v = one_triangle("bolt");
        v[60..64].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(read_mesh(&v).unwrap_err(), MeshError::Truncated);
    }
}

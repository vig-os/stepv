//! Spike B (#27): the #21 viewer as a three.js page in the browser.
//!
//!   viewer-web <file.step> [--bench 300]
//!
//! Runs the sandboxed kernel, then serves the page, three.js (vendored) and
//! the mesh + topology on 127.0.0.1 behind a random token, and opens the
//! default browser. std only: no async runtime, no HTTP crate. The page
//! POSTs /bench when benchmarking; the server prints it and exits.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use stepv::occt::{self, Limits};
use stepv::topology::Topology;
use stepv::{Deflection, Scene};

const INDEX: &str = include_str!("../static/index.html");
const THREE: &[u8] = include_bytes!("../static/three.module.min.js");
const THREE_CORE: &[u8] = include_bytes!("../static/three.core.min.js");
const ORBIT: &[u8] = include_bytes!("../static/OrbitControls.js");

/// Per part: u32 vertices, u32 triangles, then positions, normals, colours
/// (3 x f32 each per vertex), indices (3 x u32 per triangle), face ids (u32
/// per triangle). Prefixed by u32 part count. Little-endian.
fn pack(scene: &Scene) -> Vec<u8> {
    let mut out = Vec::new();
    let put = |out: &mut Vec<u8>, b: &[u8]| out.extend_from_slice(b);
    put(&mut out, &(scene.parts.len() as u32).to_le_bytes());
    for p in &scene.parts {
        let n = p.mesh.positions.len() / 3;
        put(&mut out, &(n as u32).to_le_bytes());
        put(&mut out, &((p.mesh.indices.len() / 3) as u32).to_le_bytes());
        for f in p.mesh.positions.iter().chain(&p.mesh.normals) {
            put(&mut out, &f.to_le_bytes());
        }
        let mut face_of = vec![0u32; n];
        for (t, tri) in p.mesh.indices.chunks_exact(3).enumerate() {
            for &v in tri {
                face_of[v as usize] = p.mesh.face_ids[t];
            }
        }
        for &f in &face_of {
            let c = p.faces.get(f as usize).and_then(|f| f.color).or(p.color);
            let c = c.map_or([0.62, 0.66, 0.72], |c| [c.r, c.g, c.b]);
            for x in c {
                put(&mut out, &x.to_le_bytes());
            }
        }
        for i in p.mesh.indices.iter().chain(&p.mesh.face_ids) {
            put(&mut out, &i.to_le_bytes());
        }
    }
    out
}

fn respond(s: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         Content-Security-Policy: default-src 'self'; script-src 'self' 'unsafe-inline'\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

fn main() {
    let t0 = Instant::now();
    let args: Vec<String> = std::env::args().collect();
    let input = args.get(1).expect("usage: viewer-web <file> [--bench N]");
    let bench = args.iter().position(|a| a == "--bench").and_then(|i| args.get(i + 1)).cloned();
    let dir = std::env::temp_dir();
    let (mesh, topo) = (dir.join("spike-b.msh"), dir.join("spike-b.json"));
    let limits = Limits { timeout: Duration::from_secs(300), memory: None };
    let run = occt::run(&occt::kernel_path(), input.as_ref(), Deflection::PREVIEW, limits, Some(&mesh), Some(&topo))
        .expect("kernel");
    assert_eq!(run.outcome, occt::Outcome::Ok, "{:?}", run.summary.and_then(|s| s.error));
    let scene = occt::read_mesh(&std::fs::read(&mesh).unwrap()).unwrap();
    let topo_bytes = std::fs::read(&topo).unwrap();
    Topology::parse(&topo_bytes).unwrap().check_against(&scene).unwrap();
    let packed = pack(&scene);
    eprintln!("spike-b: kernel + pack {:.2} s, {} MB to the browser", t0.elapsed().as_secs_f64(), packed.len() >> 20);

    // Loopback only, an OS-chosen port, and a token no other page knows.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let mut seed = [0u8; 16];
    std::fs::File::open("/dev/urandom").unwrap().read_exact(&mut seed).unwrap();
    let token: String = seed.iter().map(|b| format!("{b:02x}")).collect();
    let host = format!("127.0.0.1:{port}");
    let url = format!("http://{host}/?t={token}{}", bench.as_ref().map_or(String::new(), |n| format!("&bench={n}")));
    eprintln!("spike-b: {url}");
    let _ = std::process::Command::new(if cfg!(target_os = "macos") { "open" } else { "xdg-open" }).arg(&url).spawn();
    let opened = Instant::now();

    for stream in listener.incoming() {
        let Ok(mut s) = stream else { continue };
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut line = String::new();
        if r.read_line(&mut line).is_err() {
            continue;
        }
        let (mut host_ok, mut len) = (false, 0usize);
        loop {
            let mut h = String::new();
            if r.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                break;
            }
            let lower = h.to_ascii_lowercase();
            // DNS rebinding: a page on evil.example resolved to 127.0.0.1
            // sends its own Host.
            host_ok |= lower.trim_end() == format!("host: {host}");
            if let Some(v) = lower.strip_prefix("content-length:") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
        let mut parts = line.split_whitespace();
        let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let (route, query) = path.split_once('?').unwrap_or((path, ""));
        let authed = query.split('&').any(|kv| kv == format!("t={token}"));
        if !host_ok {
            respond(&mut s, "403 Forbidden", "text/plain", b"bad host");
            continue;
        }
        match (method, route) {
            // Static code needs no token; data does.
            ("GET", "/three.module.min.js") => respond(&mut s, "200 OK", "text/javascript", THREE),
            ("GET", "/three.core.js") => respond(&mut s, "200 OK", "text/javascript", THREE_CORE),
            ("GET", "/OrbitControls.js") => respond(&mut s, "200 OK", "text/javascript", ORBIT),
            ("GET", "/") if authed => respond(&mut s, "200 OK", "text/html; charset=utf-8", INDEX.as_bytes()),
            ("GET", "/mesh") if authed => respond(&mut s, "200 OK", "application/octet-stream", &packed),
            ("GET", "/topology") if authed => respond(&mut s, "200 OK", "application/json", &topo_bytes),
            ("POST", "/bench") if authed => {
                let mut body = vec![0; len.min(1 << 16)];
                let _ = r.read_exact(&mut body);
                println!(
                    "bench: kernel+pack {:.2} s; browser (from open) {:.2} s; {}",
                    (opened - t0).as_secs_f64(),
                    opened.elapsed().as_secs_f64(),
                    String::from_utf8_lossy(&body)
                );
                respond(&mut s, "200 OK", "text/plain", b"ok");
                return;
            }
            _ => respond(&mut s, "404 Not Found", "text/plain", b"no"),
        }
    }
}

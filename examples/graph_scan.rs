//! Scans STEP files into the graph's entity table and reports the time and
//! the heap against ADR-0001 §2.7 (`docs/adr/0001-graph-core.md`).
//!
//! ```text
//! cargo run --release --example graph_scan -- FILE...
//! ```
//!
//! One line per file: the entity count, distinct types, duplicate ids, the
//! scan time, and the heap the table holds beyond the map, in total and per
//! entity, with the peak during the scan. A refused file prints its error.
//! Exits 1 when any file was refused.

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Instant;

use stepv::graph::Graph;

/// The system allocator, counting the bytes live and their peak.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is forwarded to `System` unchanged; only counts are added.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            PEAK.fetch_max(LIVE.fetch_add(l.size(), Relaxed) + l.size(), Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, size) };
        if !q.is_null() {
            let live = LIVE.fetch_add(size, Relaxed) + size;
            PEAK.fetch_max(live, Relaxed);
            LIVE.fetch_sub(l.size(), Relaxed);
        }
        q
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("usage: graph_scan FILE...");
        std::process::exit(2);
    }
    let mut refused = false;
    for f in files {
        let before = LIVE.load(Relaxed);
        PEAK.store(before, Relaxed);
        let t = Instant::now();
        match Graph::open(Path::new(&f)) {
            Ok(g) => {
                let secs = t.elapsed().as_secs_f64();
                let heap = LIVE.load(Relaxed) - before;
                let peak = PEAK.load(Relaxed) - before;
                let types: std::collections::HashSet<_> =
                    g.nodes().map(|n| g.type_name(n)).collect();
                println!(
                    "{f}: {} entities, {} types, {} duplicate ids, scan {secs:.3} s, \
                     heap {:.1} MB ({:.1} B/entity), peak {:.1} MB",
                    g.len(),
                    types.len(),
                    g.duplicate_ids(),
                    heap as f64 / 1e6,
                    heap as f64 / g.len().max(1) as f64,
                    peak as f64 / 1e6,
                );
            }
            Err(e) => {
                refused = true;
                println!("{f}: refused: {e}");
            }
        }
    }
    std::process::exit(i32::from(refused));
}

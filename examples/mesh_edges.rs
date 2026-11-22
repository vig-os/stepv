//! Builds every `EDGE_CURVE`'s polyline ([`stepv::mesh::Polylines`],
//! ADR-0001 §2.6, M1a) at the preview deflection of the graph's model
//! diagonal, single-threaded, and reports the counts and the time.
//!
//! ```text
//! cargo run --release --example mesh_edges -- [--diagonal MM] FILE...
//! ```
//!
//! `--diagonal` replaces the graph's model diagonal, for instance with the
//! kernel's (its summary's `diagonal`), since the graph's box is
//! conservative and can be far larger (§2.6).
//!
//! One line per file: the edges, the polylines built, their points, the
//! edges routed by reason, and the build time (the graph's scan, structure
//! and boxes before it are timed apart). Every edge is built at the unit of
//! the file's first product; the line says when the products' units differ.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use stepv::Deflection;
use stepv::graph::{self, Bound, Graph};
use stepv::mesh::{Polylines, Tolerance};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut given = None;
    if args.first().is_some_and(|a| a == "--diagonal") && args.len() > 1 {
        given = Some(args[1].parse::<f64>().expect("--diagonal MM"));
        args.drain(..2);
    }
    for arg in args {
        let t = Instant::now();
        let g = match Graph::open(Path::new(&arg)) {
            Ok(g) => g,
            Err(e) => {
                println!("{arg}: refused: {e}");
                continue;
            }
        };
        let Ok(s) = graph::structure(&g) else {
            println!("{arg}: no structure");
            continue;
        };
        let diag = match (given, graph::bounds(&g, &s).model()) {
            (Some(d), _) => d,
            (None, Bound::Box(b)) => b.diagonal(),
            (None, Bound::Unknown(r)) => {
                println!("{arg}: no model box: {r}");
                continue;
            }
        };
        let prep = t.elapsed();
        let units: Vec<f64> = s.products.iter().map(|p| p.mm_per_unit).collect();
        let unit = units.first().copied().unwrap_or(1.);
        let mixed = units.iter().any(|&u| u != unit);
        let edges: Vec<_> = g.of_type("EDGE_CURVE").collect();
        let tol = Tolerance::new(Deflection::PREVIEW, diag).expect("a positive diagonal");
        let t = Instant::now();
        let p = Polylines::build(&g, edges.iter().copied(), unit, tol);
        let build = t.elapsed();
        let mut routed = BTreeMap::new();
        for (_, r) in p.routed() {
            *routed.entry(r.0).or_insert(0usize) += 1;
        }
        println!(
            "{arg}: {} edges, {} polylines, {} points, routed {routed:?}; d {:.4} mm; \
             build {:.1} ms (scan + structure + boxes {:.2} s){}",
            edges.len(),
            p.edges().count(),
            p.point_count(),
            tol.linear(),
            build.as_secs_f64() * 1e3,
            prep.as_secs_f64(),
            if mixed { "; mixed units" } else { "" }
        );
    }
}

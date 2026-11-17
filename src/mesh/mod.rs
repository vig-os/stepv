//! The Rust mesher for analytic products (ADR-0001 §2.6), straight from the
//! graph, without OCCT.
//!
//! It is edge-first: [`edges`] turns every `EDGE_CURVE` into one canonical
//! polyline, shared by every face that uses it, so neighbouring faces share
//! their boundary points by construction. Faces (M2/M3) refer to those
//! points as [`edges::EdgePoint`]s.

pub mod edges;

pub use edges::{EdgePoint, Polyline, Polylines, Tolerance};

//! The Rust mesher for analytic products (ADR-0001 §2.6), straight from the
//! graph, without OCCT.
//!
//! It is edge-first: [`edges`] turns every `EDGE_CURVE` into one canonical
//! polyline, shared by every face that uses it, so neighbouring faces share
//! their boundary points by construction. Faces refer to those points as
//! [`edges::EdgePoint`]s; [`plane`] meshes planar faces (M2) and
//! [`analytic`] cylinders, cones, spheres and tori (M3), both through
//! [`face`]'s shared triangulation and [`Context`].

pub mod analytic;
pub mod edges;
pub mod face;
pub mod plane;

pub use edges::{EdgePoint, Polyline, Polylines, Tolerance};
pub use face::{Context, FaceMesh, Route};

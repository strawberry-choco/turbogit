//! TurboGit domain layer: the data model and the shared error type.
//!
//! Leaf crate — depends only on `serde`, `chrono`, and `thiserror`. Every
//! other crate in the workspace may depend on this one; this crate depends
//! on none of them.

pub mod error;
pub mod model;

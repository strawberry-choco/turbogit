//! TurboGit domain layer: the data model, the shared error type, and git's
//! C-style path-quoting grammar.
//!
//! Leaf crate — depends only on `serde`, `chrono`, and `thiserror`. Every
//! other crate in the workspace may depend on this one; this crate depends
//! on none of them.

pub mod error;
pub mod git_path_quoting;
pub mod model;

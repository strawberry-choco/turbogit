//! Optional read-only acceleration via `gix` (feature `gix-reader`).
//!
//! Placeholder module: the read path still runs through
//! [`crate::engine::cli::CliExecutor`]. Implementations land behind this
//! module when the fast-read ticket executes; the file exists so tooling
//! (`cargo fmt`) can resolve the feature-gated `pub mod gix_reader;`
//! declaration in [`crate::engine`].

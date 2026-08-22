//! Optional read-only acceleration via `gix` (feature `gix-reader`).
//!
//! Placeholder module: the in-process read path has not landed yet, so with
//! the feature enabled every read still falls through to the CLI executor.
//! The module exists so `cargo fmt` / `cargo clippy --all-targets` can
//! resolve `mod gix_reader` regardless of the active feature set (issue #13
//! tooling unblock; see `engine/mod.rs`).

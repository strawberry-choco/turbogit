//! Typed error type for TurboGit.
//!
//! All git-facing layers return [`TgResult`]; the UI surfaces `TgError` via a
//! [`egui::Window`]/toast. The `Gix` variant is only compiled when the optional
//! `gix-reader` feature is enabled (reads-only path).

use thiserror::Error;

/// The single error type used across engine + core + ui.
#[derive(Error, Debug)]
pub enum TgError {
    /// Wraps [`std::io::Error`] (file IO, process spawn, …).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A `git` CLI invocation exited non-zero. Captures the exit code and stderr.
    #[error("git exited with code {code}:\n{stderr}")]
    Cli { code: i32, stderr: String },

    /// Porcelain / output parsing failure.
    #[error("parse error: {0}")]
    Parse(String),

    /// RON (de)serialization failure (`.turbogit/` state / shelves).
    #[error("serialization error: {0}")]
    Serde(#[from] ron::Error),

    /// A git operation was attempted on a path that is not a git repository.
    #[error("not a git repository: {0}")]
    NotARepo(String),

    /// Errors from the optional `gix` reader (feature `gix-reader` only).
    #[cfg(feature = "gix-reader")]
    #[error("gix error: {0}")]
    Gix(#[from] gix::Error),

    /// Anything else.
    #[error("{0}")]
    Other(String),
}

/// Convenience alias used everywhere.
pub type TgResult<T> = Result<T, TgError>;

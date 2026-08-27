//! Typed error type for TurboGit.
//!
//! All git-facing layers return [`TgResult`]; the UI surfaces `TgError` via a
//! [`egui::Window`]/toast.

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

    /// RON (de)serialization failure (`.turbogit/` state / shelves). Carries
    /// the serializer's message as a plain `String` so the domain layer never
    /// depends on a concrete serializer.
    #[error("serialization error: {0}")]
    Serde(String),

    /// A git operation was attempted on a path that is not a git repository.
    #[error("not a git repository: {0}")]
    NotARepo(String),

    /// Anything else.
    #[error("{0}")]
    Other(String),
}

/// Convenience alias used everywhere.
pub type TgResult<T> = Result<T, TgError>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The user-visible message for a serialization failure keeps the exact
    /// `serialization error: <detail>` shape after the variant stops holding
    /// a `ron::Error` and carries a plain `String` instead.
    #[test]
    fn serde_variant_displays_unchanged_message() {
        let err = TgError::Serde("expected `(`".to_string());
        assert_eq!(err.to_string(), "serialization error: expected `(`");
    }
}

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

    /// A protected-branch policy refused the action before git was touched.
    /// `consequence` is the clause naming the refused action ("rebasing it is
    /// not allowed"), so the message stays whatever each guard says while the
    /// refusal itself remains a fact a consumer can match: settlement records
    /// it per root, and a refusal this codebase wrote should not have to be
    /// re-read as prose to be recognised.
    #[error("'{branch}' is a protected branch — {consequence}")]
    ProtectedBranch {
        branch: String,
        consequence: &'static str,
    },

    /// A read that names `HEAD` in a repository nobody has committed in yet.
    /// This is a variant and not `Other` holding a sentence because a consumer
    /// must be able to tell "this root has no commits" from every other failed
    /// read without re-reading prose: the keyed read's per-key failure store
    /// shows the text, and a later surface that wants to offer a first commit
    /// instead of a dead sentence has to match the condition, not quote it.
    /// `consequence` names the refused read, so each producer owns its own
    /// clause while the refusal stays one matchable fact, exactly as
    /// [`Self::ProtectedBranch`] does.
    #[error("this repository has no commits yet: {consequence}")]
    NoCommitsYet { consequence: &'static str },

    /// Anything else.
    #[error("{0}")]
    Other(String),
}

/// Convenience alias used everywhere.
pub type TgResult<T> = Result<T, TgError>;

impl Clone for TgError {
    /// `std::io::Error` is not `Clone`; the copy reconstructs it from its
    /// `ErrorKind` and message so the displayed text is preserved. Needed by
    /// the cascade engine (issue 10), which reports one outcome both through
    /// the event stream and the pass result.
    fn clone(&self) -> Self {
        match self {
            TgError::Io(e) => TgError::Io(std::io::Error::new(e.kind(), e.to_string())),
            TgError::Cli { code, stderr } => TgError::Cli {
                code: *code,
                stderr: stderr.clone(),
            },
            TgError::Parse(s) => TgError::Parse(s.clone()),
            TgError::Serde(s) => TgError::Serde(s.clone()),
            TgError::NotARepo(s) => TgError::NotARepo(s.clone()),
            TgError::ProtectedBranch {
                branch,
                consequence,
            } => TgError::ProtectedBranch {
                branch: branch.clone(),
                consequence,
            },
            TgError::NoCommitsYet { consequence } => TgError::NoCommitsYet { consequence },
            TgError::Other(s) => TgError::Other(s.clone()),
        }
    }
}

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

    /// A protected-branch refusal displays the guard's own sentence: the
    /// variant carries the refused action as `consequence`, so each guard's
    /// wording is exactly what it was before the refusal had a type.
    #[test]
    fn protected_branch_variant_displays_the_guard_wording() {
        let err = TgError::ProtectedBranch {
            branch: "main".to_string(),
            consequence: "rebasing it is not allowed",
        };
        assert_eq!(
            err.to_string(),
            "'main' is a protected branch — rebasing it is not allowed"
        );
    }

    /// The no-commits refusal displays the producer's own clause after the
    /// condition, and the condition part is fixed text so a surface can build
    /// on it without holding the variant.
    #[test]
    fn no_commits_yet_variant_displays_the_refused_read() {
        let err = TgError::NoCommitsYet {
            consequence: "the changes cannot be compared against a first commit",
        };
        assert_eq!(
            err.to_string(),
            "this repository has no commits yet: the changes cannot be compared \
             against a first commit"
        );
    }
}

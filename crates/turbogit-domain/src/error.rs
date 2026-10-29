use thiserror::Error;

#[derive(Error, Debug)]
pub enum TgError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A `git` CLI invocation exited non-zero. Captures the exit code and stderr.
    #[error("git exited with code {code}:\n{stderr}")]
    Cli { code: i32, stderr: String },

    #[error("parse error: {0}")]
    Parse(String),

    /// RON (de)serialization failure (`.turbogit/` state / shelves). Carries
    /// the serializer's message as a plain `String` so the domain layer never
    /// depends on a concrete serializer.
    #[error("serialization error: {0}")]
    Serde(String),

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

    #[error("{0}")]
    Other(String),
}

pub type TgResult<T> = Result<T, TgError>;

impl Clone for TgError {
    /// `std::io::Error` is not `Clone`; the copy reconstructs it from its
    /// `ErrorKind` and message so the displayed text is preserved. Needed by
    /// the cascade engine, which reports one outcome both through
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

    #[test]
    fn serde_variant_displays_unchanged_message() {
        let err = TgError::Serde("expected `(`".to_string());
        assert_eq!(err.to_string(), "serialization error: expected `(`");
    }

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

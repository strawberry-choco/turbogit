//! Git engine adapters: the CLI executor, the composed libgit2-over-CLI
//! executor, and the one factory over them (ADR-0001, as corrected by ADR-0022
//! and ADR-0030).
//!
//! The port these adapters implement lives in `turbogit-engine-api`; this
//! crate is the only place (besides the composition root) that may name the
//! concrete executors. The in-memory fake executor ships behind the
//! `test-util` cargo feature so consumers opt in through dev-dependencies
//! and release builds never compile it.

pub mod cli;
pub mod git2_exec;

#[cfg(feature = "test-util")]
pub mod fake;

pub use turbogit_engine_api::{ApplyDirection, GitExecutor};

use turbogit_domain::error::TgError;
use turbogit_domain::model::{VcsSettings, git_binary};

/// Construct the engine behind the seam (ADR-0001, as corrected by ADR-0022
/// and ADR-0030): the composed executor — the reads `git2` answers
/// in-process, the CLI adapter for the rest — is the one engine, and the
/// factory builds nothing else. The retired `backend` key in an older
/// `state.ron` is an ignored field, not a choice (ADR-0030). Callers rebuild
/// this whenever settings change (e.g. the settings modal's Apply), exactly
/// like they rebuilt a bare [`cli::CliExecutor`] before.
pub fn build_executor(settings: &VcsSettings) -> std::sync::Arc<dyn GitExecutor> {
    let cli = cli::CliExecutor {
        settings: settings.clone(),
    };
    std::sync::Arc::new(git2_exec::Git2Executor::new(cli))
}

/// Resolve the git version behind `settings` (issue #26): runs the resolved
/// `<git> --version` — settings override, else `git` on PATH — and returns
/// the parsed version string (e.g. `2.47.1`). Errors when the executable
/// cannot be spawned or answers in an unexpected shape; the settings modal
/// renders this as the live version badge on the Git-executable row.
pub fn resolve_git_version(settings: &VcsSettings) -> turbogit_domain::error::TgResult<String> {
    let out = std::process::Command::new(git_binary(settings))
        .arg("--version")
        .output()
        .map_err(|e| TgError::Io(std::io::Error::new(e.kind(), e.to_string())))?;
    if !out.status.success() {
        return Err(TgError::Cli {
            code: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let version = stdout
        .trim()
        .strip_prefix("git version")
        .map(str::trim)
        .unwrap_or(stdout.trim());
    if version.is_empty() {
        return Err(TgError::Parse(format!(
            "unrecognized `git --version` output: {stdout:?}"
        )));
    }
    Ok(version.to_string())
}

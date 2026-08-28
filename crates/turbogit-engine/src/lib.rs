//! Git engine adapters: the CLI executor, the libgit2 executor, and the
//! backend-selection factory (ADR-0001).
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

use turbogit_domain::model::{GitBackend, VcsSettings};

/// Construct the engine for `settings` behind the seam (ADR-0001): selects
/// the libgit2-backed executor when `settings.backend` asks for it,
/// otherwise the plain CLI executor (library-migration plan Phase L2).
/// Callers rebuild this whenever settings change (e.g. the settings modal's
/// Apply), exactly like they rebuilt a bare [`cli::CliExecutor`] before.
pub fn build_executor(settings: &VcsSettings) -> std::sync::Arc<dyn GitExecutor> {
    let cli = cli::CliExecutor {
        settings: settings.clone(),
    };
    match settings.backend {
        GitBackend::Libgit2 => std::sync::Arc::new(git2_exec::Git2Executor::new(cli)),
        GitBackend::Cli => std::sync::Arc::new(cli),
    }
}

//! Git engine layer.
//!
//! The port lives in `turbogit-engine-api` and the adapters plus the
//! backend-selection factory live in `turbogit-engine` (DDD split issue 06).
//! This module keeps re-export shims so every existing `engine::X` path
//! resolves untouched; the event types below moved to the app crate in
//! issue 08 (the port no longer references them).

pub use turbogit_engine::{build_executor, cli, git2_exec};
pub use turbogit_engine_api::{ApplyDirection, GitExecutor};

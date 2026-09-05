//! Pure domain/application services: every operation in the engine vocabulary
//! expressed in terms of the [`turbogit_engine_api::GitExecutor`] port and
//! the value types in `turbogit-domain`. No egui, no I/O outside the engine
//! seam — the stateful granular staging protocol is the one exception and
//! stays in the composition root until issue 08.
//!
//! Library-migration plan Phase L1 (CLI parity) and Phase L3 (the two-way
//! merge rewriter) live here; see `product-spec.md` §3 and the parity
//! suites under `tests/`.

pub mod branch_service;
pub mod bulk_ops;
pub mod bulk_run;
pub mod changes;
pub mod cherry_across;
pub mod commit_across;
pub mod conflict;
pub mod conflict_propagation;
pub mod diff_engine;
pub mod history_editor;
pub mod history_service;
pub mod hunk_stats;
pub mod integrate_service;
pub mod multi_root;
pub mod partial;
pub mod remote_service;
pub mod shelve_stash;
pub mod sync_service;
pub mod tag_service;

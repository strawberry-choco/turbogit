//! Application layer: egui-free state, persistence, recents, root caches,
//! the event types the workers post to the UI thread, and the stateful
//! granular staging protocol. The eframe entry point (`app.rs`) stays in
//! the composition root and composes this crate with the UI crate.

/// Engine re-export for the layers above: the Settings modal's live git
/// version check (issue #26) runs against the draft settings, but the UI
/// crate only depends on this crate — the engine stays below the seam.
pub use turbogit_engine::resolve_git_version;

/// The service modules the presentation crate paints with, re-exported so that
/// `turbogit-ui` can hold no dependency on `turbogit-services`: a modal's row
/// type, a scoped push target and a couple of pure classifiers are data the
/// Shell is handed, not reach. None of it is a way to git, because every
/// function in these modules that reaches the port takes a `&dyn GitExecutor`
/// and that handle is `pub(crate)` here — re-exporting the module hands over the
/// vocabulary, never the port. The write side of the same rule is
/// [`shell_writes`]; a verb that *does* git is named there, not borrowed from
/// a module path.
pub use turbogit_services::{
    bulk_ops, bulk_run, cherry_across, conflict, diff_engine, history_editor, hunk_stats,
    integrate_service, patch_apply, patch_create, remote_service, reselection, shelve_stash,
    sync_service, tag_service,
};

/// The in-flight operation's owner, the one engine-api type the Shell paints:
/// the status bar's announcement and the rewrite gate's refusals both name it,
/// and `integrate_service` re-exports the same type, so both paths converge
/// here rather than leaving the UI to pick a crate for one enum.
pub use turbogit_engine_api::OwnedOperation;

pub mod activity;
pub mod apply_patch;
pub mod banner;
pub mod bulk_history;
pub mod bulk_run_view;
pub mod cherry_run_view;
pub mod confirm;
pub mod create_patch;
pub mod diff_data;
pub mod diff_load;
pub mod diff_model;
pub mod events;
pub mod fetch_admission;
pub mod granular;
pub mod keyed_read;
pub mod log_display;
pub mod log_states;
pub mod operation;
pub mod persistence;
pub mod pinned_views;
pub mod polling;
pub mod recents;
pub mod root_caches;
pub mod settle;
pub mod shell_reads;
pub mod shell_writes;
pub mod smart_rules;
pub mod state;
pub mod worktree_lifecycle;

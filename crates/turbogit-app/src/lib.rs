//! Application layer: egui-free state, persistence, recents, root caches,
//! the event types the workers post to the UI thread, and the stateful
//! granular staging protocol. The eframe entry point (`app.rs`) stays in
//! the composition root and composes this crate with the UI crate.

/// Engine re-export for the layers above: the Settings modal's live git
/// version check (issue #26) runs against the draft settings, but the UI
/// crate only depends on this crate — the engine stays below the seam.
pub use turbogit_engine::resolve_git_version;

pub mod activity;
pub mod banner;
pub mod bulk_history;
pub mod bulk_run_view;
pub mod cherry_run_view;
pub mod diff_data;
pub mod events;
pub mod granular;
pub mod persistence;
pub mod pinned_views;
pub mod polling;
pub mod recents;
pub mod root_caches;
pub mod smart_rules;
pub mod state;

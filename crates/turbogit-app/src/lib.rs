pub use turbogit_engine::resolve_git_version;

pub use turbogit_services::{
    bulk_ops, bulk_run, cherry_across, conflict, diff_engine, history_editor, integrate_service,
    patch_apply, patch_create, remote_service, reselection, shelve_stash, sync_service,
    tag_service,
};

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
pub mod staged_lines;
pub mod state;
pub mod worktree_lifecycle;

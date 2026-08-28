//! Domain/application services.
//!
//! Every pure service moved to the `turbogit-services` crate (DDD split
//! issue 07); this module keeps re-export shims so every existing
//! `turbogit::core::X` path keeps resolving without modification.

// Re-export every pure service so existing paths keep resolving.
pub use turbogit_services::{
    branch_service, changes, conflict, diff_engine, history_editor, history_service,
    integrate_service, multi_root, partial, shelve_stash, sync_service,
};

//! TurboGit library crate — shared by the `turbogit` binary and the
//! integration tests. Module layout mirrors `execution-plan.md` §4.

pub mod app;
pub mod core;
pub mod engine;
pub use turbogit_domain::error;
pub use turbogit_domain::model;
pub mod persistence;
pub mod recents;
pub mod root_caches;
pub mod state;
pub mod theme;
pub mod ui;

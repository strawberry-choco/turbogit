//! TurboGit library crate — shared by the `turbogit` binary and the
//! integration tests. Module layout mirrors `execution-plan.md` §4.

pub mod app;
pub mod core;
pub use turbogit_app::{diff_data, events, granular, persistence, recents, root_caches, state};
pub mod engine;
pub use turbogit_domain::error;
pub use turbogit_domain::model;
pub mod theme;
pub mod ui;

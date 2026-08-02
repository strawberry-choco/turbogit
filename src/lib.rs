//! TurboGit library crate — shared by the `turbogit` binary and the
//! integration tests. Module layout mirrors `execution-plan.md` §4.

#![allow(dead_code)]

pub mod app;
pub mod core;
pub mod engine;
pub mod error;
pub mod model;
pub mod persistence;
pub mod state;
pub mod theme;
pub mod ui;

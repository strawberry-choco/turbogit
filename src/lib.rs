//! TurboGit composition root. Everything except the eframe wiring lives in
//! the workspace crates; this library exposes only the root-owned
//! `app` module so the binary can `use turbogit::app::TurbogitApp`.
//!
//! No re-export shims: every caller names its real crate
//! (`turbogit_domain`, `turbogit_app`, `turbogit_ui`, `turbogit_services`,
//! `turbogit_engine_api`).

pub mod app;

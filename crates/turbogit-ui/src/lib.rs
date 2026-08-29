//! Presentation layer: the egui theme token set and the IDE shell with its
//! floating surfaces (DDD split issue 09).
//!
//! The composition root re-exports both modules so the historical
//! `turbogit::ui` / `turbogit::theme` paths keep resolving.

pub mod theme;
pub mod ui;
pub use ui::render;

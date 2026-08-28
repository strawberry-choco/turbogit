//! Application layer: egui-free state, persistence, recents, root caches,
//! the event types the workers post to the UI thread, and the stateful
//! granular staging protocol. The eframe entry point (`app.rs`) stays in
//! the composition root and composes this crate with the UI crate.

pub mod diff_data;
pub mod events;
pub mod granular;
pub mod persistence;
pub mod recents;
pub mod root_caches;
pub mod state;

//! Crate-internal presentation kits for shared UI grammars.
//!
//! The conflict-pane kit is deliberately presentation-only: it owns the
//! shared pane grammar, while parsing, composition, interaction, resolution,
//! and apply behavior stay with each surface. The inline surface keeps its
//! read-only Result composition; the dedicated resolver keeps its free-text
//! Result editor and resolver-only controls.
//!
//! The existing component, widget, and icon modules keep their direct public
//! paths; this namespace only groups the new conflict-specific presentation
//! primitives without moving or generalizing those APIs.

pub(crate) mod conflict_pane;

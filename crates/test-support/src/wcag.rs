//! WCAG legibility maths — the one place a contrast number in the test suites
//! is computed.
//!
//! Three suites under `turbogit-ui` each carried their own copy of the same
//! relative-luminance formula: `design_tokens.rs`, `git_log.rs` and
//! `welcome.rs`. `design_tokens.rs` had already collapsed two *earlier* copies
//! within itself (a nested `fn` and a nested closure) down to a single
//! module-level pair, and states the standard in its own header comment —
//! "there is exactly one place a contrast number in this suite is computed" —
//! which was true of the file and not of the tree. This module is that place,
//! so the sentence holds across files as well.
//!
//! Gated behind `harness` because the argument is an `egui::Color32`: the gate
//! is what keeps `turbogit-app`'s and `turbogit-services'` suites off the egui
//! stack entirely, and the suites that measure legibility are the egui ones.
//!
//! Alpha is ignored on purpose. Every caller passes a palette token or a
//! colour read off an opaque painted rect, and WCAG's formula is defined on
//! opaque colours; compositing is the caller's decision, not this module's.

use egui::Color32;

/// Relative luminance (WCAG) — the measure that says which of two greys reads
/// as the *stronger* line against a dark surface.
pub fn luminance(color: Color32) -> f64 {
    let linear = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
}

/// WCAG contrast ratio between two opaque colours: `1.0` for identical
/// colours, `21.0` for black on white.
pub fn contrast(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

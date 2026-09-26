//! Non-text pane assembly: routing a diff section to the text, image or
//! binary renderer, and the pane's GPU texture cache (spec R8, ADR-0015).
//!
//! Which sides a pane's bytes come from, what the pane is keyed by, and when a
//! fetch is admitted are the read's business ([`turbogit_app::keyed_read`]);
//! this module asks for a [`PaneTarget`] and paints the verdict it is given.

use super::model::{FileMeta, ROW_H};
use crate::theme::Palette;
use crate::ui::widgets;
use egui::{Align, Layout, Sense, TextureOptions, Ui, Vec2};
use std::cell::RefCell;
use std::collections::HashMap;
use turbogit_app::diff_data::{PaneEntry, PaneSide};
use turbogit_app::events::DecodedImage;
use turbogit_app::keyed_read::{DiffTarget, PaneFlavour, PaneTarget, Read};
use turbogit_app::state::AppState;

/// Resolved byte lengths for the binary caption, when both sides resolved.
pub(super) fn pane_byte_lens(entry: &PaneEntry) -> Option<(u64, u64)> {
    Some((entry.old.as_ref()?.byte_len, entry.new.as_ref()?.byte_len))
}

/// Human-readable byte size ("0 B", "512 B", "1.2 KB", "12 MB") — decimal
/// units; one decimal below 10 of a unit, whole numbers from there.
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    // Rounding must never print "1000.0 KB" — promote once more instead.
    if value.round() >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    let name = UNITS[unit];
    if unit == 0 || value >= 10.0 {
        format!("{value:.0} {name}")
    } else {
        format!("{value:.1} {name}")
    }
}
// --- non-text pane bytes & textures (R8) -------------------------------------

// The plain-data pane cache ([`PaneCache`], [`PaneEntry`], [`PaneSide`]) lives
// in [`turbogit_app::diff_data`] beside the app state — the UI imports it back up
// (DDD split issue 04). Re-exported here so the historical `ui::diff` paths
// keep resolving.

// GPU textures for image panes are a UI-layer concern (DDD split issue 04): the
// plain-data pane cache holds decoded bytes only, and textures are uploaded
// lazily at first paint and cached here. Two invalidation rules mirror the
// plain-data cache exactly: a generation change (wholesale root refresh)
// discards everything, and cap eviction drops any texture whose tag the app no
// longer answers for — asked through [`AppState::pane_is_cached`], so this is
// the last of the reads' stores that presentation code does not reach into
// (ADR-0021). Browsing many image files therefore never pins more GPU memory
// than the live entries.
/// (pane generation, (pane tag, side index) → uploaded texture).
type PaneTextureCache = (u64, HashMap<(String, usize), egui::TextureHandle>);

thread_local! {
    static PANE_TEXTURES: RefCell<PaneTextureCache> = RefCell::new((0, HashMap::new()));
}

/// Align the UI-local texture cache with the app state before painting: clear
/// on a generation change, otherwise prune to exactly the panes still cached.
fn sync_pane_textures(state: &AppState) {
    let generation = state.ui.pane_generation;
    PANE_TEXTURES.with(|slot| {
        let mut cache = slot.borrow_mut();
        if cache.0 != generation {
            cache.1.clear();
            cache.0 = generation;
            return;
        }
        cache.1.retain(|(tag, _), _| state.pane_is_cached(tag));
    });
}

/// Texture for one pane side: uploaded once per (pane tag, side index) and
/// cached so re-showing a file never re-uploads. Call [`sync_pane_textures`]
/// first each frame.
fn pane_texture(
    ui: &Ui,
    pane_key: &str,
    side_index: usize,
    image: &DecodedImage,
) -> egui::TextureHandle {
    PANE_TEXTURES.with(|slot| {
        slot.borrow_mut()
            .1
            .entry((pane_key.to_owned(), side_index))
            .or_insert_with(|| {
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [image.width as usize, image.height as usize],
                    &image.rgba,
                );
                ui.ctx().load_texture(
                    format!("diff-pane-{pane_key}-{side_index}"),
                    color,
                    TextureOptions::LINEAR,
                )
            })
            .clone()
    })
}

/// Fit `tex` inside `max`, preserving aspect ratio, never upscaling past
/// the natural pixel size (crisp beats blurry).
fn fitted(tex: Vec2, max: Vec2) -> Vec2 {
    if tex.x <= 0.0 || tex.y <= 0.0 {
        return Vec2::ZERO;
    }
    let scale = (max.x / tex.x).min(max.y / tex.y).min(1.0);
    tex * scale
}

/// Per-side caption under an image pane: `1920×1080 · 1.2 MB`.
fn image_caption(side: &PaneSide) -> String {
    match &side.image {
        Some(img) => format!(
            "{}×{} · {}",
            img.width,
            img.height,
            human_size(side.byte_len)
        ),
        None => human_size(side.byte_len),
    }
}

/// One image cell: the texture fitted into `max`, caption centered below.
fn image_cell(ui: &mut Ui, side: &PaneSide, tex: &egui::TextureHandle, max: Vec2) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space(6.0);
        ui.add(egui::Image::new(tex).fit_to_exact_size(fitted(tex.size_vec2(), max)));
        ui.add_space(2.0);
        ui.colored_label(Palette::INK_2, image_caption(side));
    });
}

/// Image-diff pane (CONTEXT.md "Image diff", ADR-0015): the two decoded
/// versions side by side with dimension/size captions, replacing the row
/// view entirely — the mode toggle has no second layout to switch to and
/// no hunk affordances exist. Bytes fetch off-frame; while loading a
/// lightweight note stands in. Over-cap / undecodable / unreadable sides
/// fall back to the binary-change rendering with whatever sizes resolved;
/// a new/deleted file renders its single existing side.
pub(super) fn render_image_pane(
    ui: &mut Ui,
    state: &mut AppState,
    target: &DiffTarget,
    meta: &FileMeta,
) {
    let pane = PaneTarget::new(target.clone(), PaneFlavour::Image, meta.clone(), true);
    // The tag addresses this pane in the UI-local texture cache below; the
    // read derives it, and nothing here builds or compares one.
    let tag = pane.texture_tag();
    let entry = match state.read(pane) {
        Read::Fresh(entry) => entry,
        _ => {
            centered_note(ui, "Loading image…");
            return;
        }
    };
    // Lazy GPU upload: decoding happened on the worker; each texture is
    // built once and kept in the UI-local cache — the plain-data pane
    // entry never holds an egui type (DDD split issue 04).
    sync_pane_textures(state);
    let present: Vec<&PaneSide> = [entry.old.as_ref(), entry.new.as_ref()]
        .into_iter()
        .flatten()
        .collect();
    if present.is_empty() || present.iter().any(|s| s.image.is_none()) {
        // Nothing usable decoded: the binary-change fallback (sizes that
        // did resolve still show).
        let sizes = match (&entry.old, &entry.new) {
            (Some(o), Some(n)) => Some((o.byte_len, n.byte_len)),
            _ => None,
        };
        binary_placeholder(ui, sizes);
        return;
    }
    let sides = [entry.old.as_ref(), entry.new.as_ref()];
    let textures: [Option<egui::TextureHandle>; 2] = [
        sides[0]
            .and_then(|s| s.image.as_ref())
            .map(|img| pane_texture(ui, &tag, 0, img)),
        sides[1]
            .and_then(|s| s.image.as_ref())
            .map(|img| pane_texture(ui, &tag, 1, img)),
    ];

    const CAPTION_H: f32 = 24.0;
    let avail_h = (ui.available_height() - CAPTION_H).max(ROW_H * 2.0);
    match (entry.old.as_ref(), entry.new.as_ref()) {
        (Some(old), Some(new)) => {
            let (t_old, t_new) = (
                textures[0].as_ref().expect("image present"),
                textures[1].as_ref().expect("image present"),
            );
            ui.columns(2, |cols| {
                let w0 = cols[0].available_width();
                let w1 = cols[1].available_width();
                image_cell(&mut cols[0], old, t_old, Vec2::new(w0, avail_h));
                image_cell(&mut cols[1], new, t_new, Vec2::new(w1, avail_h));
            });
        }
        // Single-sided (new / deleted file): the lone image, centered.
        // Covers (None, None) too — the empty-entry fallback above already
        // returned.
        (side, None) | (None, side) => {
            if let Some(side) = side {
                let tex = textures.iter().flatten().next().expect("image present");
                let width = ui.available_width();
                image_cell(ui, side, tex, Vec2::new(width, avail_h));
            }
        }
    }
}
/// Centered muted one-line note filling the remaining pane height.
fn centered_note(ui: &mut Ui, text: &str) {
    let width = ui.available_width();
    let height = ui.available_height().max(ROW_H * 3.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    widgets::paint_centered_text(
        ui.painter(),
        rect,
        text,
        crate::theme::chrome_font(crate::theme::TYPE_BODY),
        Palette::INK_2,
    );
}

/// Binary-change placeholder (CONTEXT.md "Binary change", ADR-0015):
/// replaces the row-based view for a single binary file section. `sizes`
/// carries the resolved `(before, after)` byte lengths from the same
/// off-frame sourcing path the image pane uses; `None` (still loading, or
/// a side unreadable) renders the bare description.
pub(super) fn binary_placeholder(ui: &mut Ui, sizes: Option<(u64, u64)>) {
    let text = match sizes {
        Some((before, after)) => format!(
            "Binary file changed · {} → {}",
            human_size(before),
            human_size(after)
        ),
        None => "Binary file changed".to_owned(),
    };
    centered_note(ui, &text);
}
#[cfg(test)]
mod tests {
    use super::*;
    use turbogit_app::diff_data::PaneSide;
    use turbogit_app::events::DecodedImage;

    #[test]
    fn diff_human_size_formats_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1000), "1.0 KB");
        assert_eq!(human_size(1500), "1.5 KB");
        // Rounding promotes instead of printing "1000.0 KB".
        assert_eq!(human_size(999_999), "1.0 MB");
        assert_eq!(human_size(900_000), "900 KB");
        assert_eq!(human_size(1_200_000), "1.2 MB");
        assert_eq!(human_size(12_000_000), "12 MB");
    }

    #[test]
    fn diff_image_caption_formats_dimensions() {
        let side = PaneSide {
            byte_len: 5,
            image: Some(DecodedImage {
                width: 1920,
                height: 1080,
                rgba: Vec::new(),
            }),
        };
        assert_eq!(image_caption(&side), "1920×1080 · 5 B");
    }
}

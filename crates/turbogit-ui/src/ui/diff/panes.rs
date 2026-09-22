//! Non-text pane assembly: routing a diff section to the text, image or
//! binary renderer, the pane's texture cache, and the request building for the
//! app layer's off-frame loads (spec R8, ADR-0015). The loads themselves are
//! [`AppState::ensure_diff`] and [`AppState::ensure_pane_bytes`] — the Shell
//! never holds the Git engine or an event sender.

use super::actions::paint_centered;
use super::model::{FileMeta, ROW_H};
use crate::theme::Palette;
use egui::{Align, Layout, Sense, TextureOptions, Ui, Vec2};
use std::cell::RefCell;
use std::collections::HashMap;
use turbogit_app::diff_data::PaneSide;
use turbogit_app::diff_load::{PaneSideRequest, SideSpec};
use turbogit_app::events::DecodedImage;
use turbogit_app::state::AppState;

/// The two byte sources for one file section. New files have no old side;
/// deleted files no new side — those render as the single-image case. Paths
/// become repo-relative and slash-separated here, because that is the form
/// `git show` takes and the metadata carries git's `a/`/`b/` prefixes.
pub(super) fn pane_side_requests(
    root: &std::path::Path,
    left: &Option<String>,
    right: &Option<String>,
    staged: bool,
    meta: &FileMeta,
) -> (PaneSideRequest, PaneSideRequest) {
    let rel = |p: &Option<String>| {
        p.as_deref()
            .map(super::model::repo_rel_path)
            .unwrap_or_default()
            .to_owned()
    };
    let old = if meta.new_file {
        SideSpec::Missing
    } else {
        match left {
            Some(l) => SideSpec::Rev(l.clone()),
            None if staged => SideSpec::Rev("HEAD".to_owned()),
            None => SideSpec::Rev(":0".to_owned()),
        }
    };
    let new = if meta.deleted_file {
        SideSpec::Missing
    } else {
        match right {
            Some(r) => SideSpec::Rev(r.clone()),
            None if staged => SideSpec::Rev(":0".to_owned()),
            None => SideSpec::Worktree,
        }
    };
    (
        PaneSideRequest {
            root: root.to_path_buf(),
            spec: old,
            path: rel(&meta.old_path),
        },
        PaneSideRequest {
            root: root.to_path_buf(),
            spec: new,
            path: rel(&meta.new_path),
        },
    )
}

/// Resolved byte lengths for the binary caption, when both sides resolved.
pub(super) fn pane_byte_lens(state: &AppState, pane_key: &str) -> Option<(u64, u64)> {
    let entry = state.ui.pane_bytes.get(pane_key)?;
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

// GPU textures for image panes are a UI-layer concern (DDD split issue 04):
// the plain-data pane cache holds decoded bytes only, and textures are
// uploaded lazily at first paint and cached here. Two invalidation rules
// mirror the plain-data cache exactly: a generation change (wholesale root
// refresh) discards everything, and cap eviction drops any texture whose
// pane key the [`PaneCache`] no longer holds — so browsing many image files
// never pins more GPU memory than the live entries.
/// (pane generation, (pane key, side index) → uploaded texture).
type PaneTextureCache = (u64, HashMap<(String, usize), egui::TextureHandle>);

thread_local! {
    static PANE_TEXTURES: RefCell<PaneTextureCache> = RefCell::new((0, HashMap::new()));
}

/// Align the UI-local texture cache with the app state before painting:
/// clear on a generation change, otherwise prune to exactly the pane keys
/// still cached in `turbogit_app::diff_data::PaneCache`.
fn sync_pane_textures(generation: u64, live_keys: impl IntoIterator<Item = String>) {
    PANE_TEXTURES.with(|slot| {
        let mut cache = slot.borrow_mut();
        if cache.0 != generation {
            cache.1.clear();
            cache.0 = generation;
            return;
        }
        let live: std::collections::HashSet<String> = live_keys.into_iter().collect();
        cache.1.retain(|(key, _), _| live.contains(key));
    });
}

/// Texture for one pane side: uploaded once per (pane key, side index) and
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
#[allow(clippy::too_many_arguments)]
pub(super) fn render_image_pane(
    ui: &mut Ui,
    state: &mut AppState,
    pane_key: &str,
    root: &std::path::Path,
    left: &Option<String>,
    right: &Option<String>,
    staged: bool,
    meta: &FileMeta,
) {
    let (old, new) = pane_side_requests(root, left, right, staged, meta);
    if !state.ensure_pane_bytes(pane_key.to_owned(), old, new, true) {
        centered_note(ui, "Loading image…");
        return;
    }
    let generation = state.ui.pane_generation;
    let live_keys: Vec<String> = state.ui.pane_bytes.keys().map(str::to_owned).collect();
    let entry = state
        .ui
        .pane_bytes
        .get(pane_key)
        .expect("entry cached above");
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
    // Lazy GPU upload: decoding happened on the worker; each texture is
    // built once and kept in the UI-local cache — the plain-data pane
    // entry never holds an egui type (DDD split issue 04).
    sync_pane_textures(generation, live_keys);
    let sides = [entry.old.as_ref(), entry.new.as_ref()];
    let textures: [Option<egui::TextureHandle>; 2] = [
        sides[0]
            .and_then(|s| s.image.as_ref())
            .map(|img| pane_texture(ui, pane_key, 0, img)),
        sides[1]
            .and_then(|s| s.image.as_ref())
            .map(|img| pane_texture(ui, pane_key, 1, img)),
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
    paint_centered(
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
    use crate::ui::diff::model::FileMeta;
    use turbogit_app::diff_data::PaneSide;
    use turbogit_app::events::DecodedImage;

    #[test]
    fn pane_side_requests_mirror_the_diff_invocation() {
        let root = std::path::Path::new("/repo");
        let ask = |left: Option<&str>, right: Option<&str>, staged: bool, meta: &FileMeta| {
            pane_side_requests(
                root,
                &left.map(str::to_owned),
                &right.map(str::to_owned),
                staged,
                meta,
            )
        };
        let plain = FileMeta::default();
        let added = FileMeta {
            new_file: true,
            ..FileMeta::default()
        };
        let deleted = FileMeta {
            deleted_file: true,
            ..FileMeta::default()
        };

        // Repo chip (HEAD↔worktree): `git diff HEAD`.
        assert_eq!(
            ask(Some("HEAD"), None, false, &plain),
            (
                side(root, SideSpec::Rev("HEAD".to_owned()), ""),
                side(root, SideSpec::Worktree, ""),
            )
        );
        // Staged chip (HEAD↔index): `git diff --cached`; index via stage-0.
        assert_eq!(
            ask(None, None, true, &plain),
            (
                side(root, SideSpec::Rev("HEAD".to_owned()), ""),
                side(root, SideSpec::Rev(":0".to_owned()), ""),
            )
        );
        // Local chip (index↔worktree): plain `git diff`.
        assert_eq!(
            ask(None, None, false, &plain),
            (
                side(root, SideSpec::Rev(":0".to_owned()), ""),
                side(root, SideSpec::Worktree, ""),
            )
        );
        // Explicit commit-to-commit targets pass their revs through.
        assert_eq!(
            ask(Some("abc123"), Some("def456"), false, &plain),
            (
                side(root, SideSpec::Rev("abc123".to_owned()), ""),
                side(root, SideSpec::Rev("def456".to_owned()), ""),
            )
        );
        // New files have no old side; deleted files no new side.
        assert_eq!(ask(None, None, false, &added).0.spec, SideSpec::Missing);
        assert_eq!(
            ask(Some("HEAD"), None, false, &deleted).1.spec,
            SideSpec::Missing
        );

        // Paths arrive repo-relative and slash-separated, stripped of git's
        // `a/`/`b/` prefixes, because that is the form `git show` takes.
        let renamed = FileMeta {
            old_path: Some("a/old/dir/Art.png".into()),
            new_path: Some("b/new/dir/Art.png".into()),
            ..FileMeta::default()
        };
        let (old, new) = ask(None, None, false, &renamed);
        assert_eq!(
            (old.path.as_str(), new.path.as_str()),
            ("old/dir/Art.png", "new/dir/Art.png")
        );
    }

    fn side(root: &std::path::Path, spec: SideSpec, path: &str) -> PaneSideRequest {
        PaneSideRequest {
            root: root.to_path_buf(),
            spec,
            path: path.to_owned(),
        }
    }

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

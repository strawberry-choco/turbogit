//! Non-text pane assembly and async loading: image/binary pane kind
//! routing, off-frame byte fetches, texture upload, and the diff loader
//! (`ensure_diff`, spec R8, ADR-0015).

use super::actions::{paint_centered, preview_status};
use super::model::{FileMeta, IMAGE_CAP_BYTES, IMAGE_MAX_PIXELS, ROW_H, repo_rel_path};
use crate::theme::Palette;
use egui::{Align, FontFamily, FontId, Layout, Sense, TextureOptions, Ui, Vec2};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use turbogit_app::diff_data::PaneSide;
use turbogit_app::events::{AppEvent, DecodedImage, FetchedBlob};
use turbogit_app::granular::{self, diff_key};
use turbogit_app::state::AppState;
use turbogit_domain::model::{ChangeStatus, DiffOpts};
use turbogit_engine_api::GitExecutor;
use turbogit_services::diff_engine;

/// Where one side's bytes come from — mirroring exactly how the viewer's
/// diff text is requested ([`turbogit_app::granular::comparison_triple`] + `CliExecutor::diff`):
///
/// | comparison       | old side   | new side     |
/// |------------------|------------|--------------|
/// | Repo (HEAD↔wt)   | `HEAD`     | worktree fs  |
/// | Staged (HEAD↔ix) | `HEAD`     | index `:0`   |
/// | Local (ix↔wt)    | index `:0` | worktree fs  |
/// | explicit l..r    | `<left>`   | `<right>`    |
///
/// Index revs use git's stage syntax (`:<n>:<path>`), the same style the
/// conflict reader uses for `:1`/`:2`/`:3`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SideSpec {
    Rev(String),
    Worktree,
    Missing,
}

/// Byte sources for `(old, new)` of one file section. New files have no old
/// side; deleted files no new side — those render as the single-image case.
fn byte_side_specs(
    left: &Option<String>,
    right: &Option<String>,
    staged: bool,
    meta: &FileMeta,
) -> (SideSpec, SideSpec) {
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
    (old, new)
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

/// Decode raw bytes into a [`DecodedImage`] when they are an in-cap image.
/// SVG can never reach this — the extension sniff gates decoding — and a
/// blob over [`IMAGE_CAP_BYTES`] or with more than [`IMAGE_MAX_PIXELS`]
/// counts as undecodable so the pane falls back to the binary change.
fn decode_image(bytes: &[u8]) -> Option<DecodedImage> {
    if bytes.len() as u64 > IMAGE_CAP_BYTES {
        return None;
    }
    let img = image::load_from_memory(bytes).ok()?;
    let (width, height) = (img.width(), img.height());
    if width as u64 * height as u64 > IMAGE_MAX_PIXELS {
        return None;
    }
    Some(DecodedImage {
        width,
        height,
        rgba: img.to_rgba8().into_raw(),
    })
}

/// Fetch one side's bytes (worker-thread only): engine blob for rev specs,
/// filesystem for the worktree — metadata-only when no decode is needed,
/// since the binary caption wants lengths, not content.
fn fetch_side(
    exec: &dyn GitExecutor,
    root: &std::path::Path,
    spec: &SideSpec,
    rel: &str,
    decode: bool,
) -> Option<FetchedBlob> {
    let bytes = match spec {
        SideSpec::Missing => return None,
        SideSpec::Worktree => {
            let full = root.join(rel);
            if !decode {
                let len = std::fs::metadata(full).ok()?.len();
                return Some(FetchedBlob {
                    byte_len: len,
                    decoded: None,
                });
            }
            std::fs::read(full).ok()?
        }
        SideSpec::Rev(rev) => exec
            .show_file_bytes(root, rev, std::path::Path::new(rel))
            .ok()?,
    };
    let byte_len = bytes.len() as u64;
    let decoded = decode.then(|| decode_image(&bytes)).flatten();
    Some(FetchedBlob { byte_len, decoded })
}

/// Dispatch the off-frame byte load for a non-text pane when nothing is in
/// flight and the result is not cached — one in-flight load per target,
/// mirroring `ensure_diff`. Returns whether an entry for `pane_key` is
/// already cached.
#[allow(clippy::too_many_arguments)]
pub(super) fn ensure_pane_bytes(
    state: &mut AppState,
    pane_key: String,
    root: &std::path::Path,
    left: &Option<String>,
    right: &Option<String>,
    staged: bool,
    meta: &FileMeta,
    decode: bool,
) -> bool {
    if state.ui.pane_bytes.get(&pane_key).is_some() {
        return true;
    }
    if state.ui.pane_bytes_loading.is_none() {
        let (old_spec, new_spec) = byte_side_specs(left, right, staged, meta);
        // Owned: the worker closure is 'static, the metadata borrow is not.
        let old_rel = meta
            .old_path
            .as_deref()
            .map(|p| repo_rel_path(p).to_owned())
            .unwrap_or_default();
        let new_rel = meta
            .new_path
            .as_deref()
            .map(|p| repo_rel_path(p).to_owned())
            .unwrap_or_default();
        let root = root.to_path_buf();
        let executor = state.executor.clone();
        let tx = state.tx.clone();
        state.ui.pane_bytes_loading = Some(pane_key.clone());
        std::thread::spawn(move || {
            let old = fetch_side(executor.as_ref(), &root, &old_spec, &old_rel, decode);
            let new = fetch_side(executor.as_ref(), &root, &new_spec, &new_rel, decode);
            let _ = tx.send(AppEvent::FileBytesReady {
                key: pane_key,
                old,
                new,
            });
        });
    }
    false
}

/// Resolved byte lengths for the binary caption, when both sides resolved.
pub(super) fn pane_byte_lens(state: &AppState, pane_key: &str) -> Option<(u64, u64)> {
    let entry = state.ui.pane_bytes.get(pane_key)?;
    Some((entry.old.as_ref()?.byte_len, entry.new.as_ref()?.byte_len))
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
    if !ensure_pane_bytes(
        state,
        pane_key.to_owned(),
        root,
        left,
        right,
        staged,
        meta,
        true,
    ) {
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
// --- engine access -----------------------------------------------------------

/// Synthesize a creation unified-diff for an untracked preview (spec R2):
/// `git diff` cannot see untracked files, so the worktree content is dressed
/// as a whole-file addition and fed through the normal cache/parse path —
/// rows, gutters, hover tracking, and hunk nav all behave unchanged. This is
/// the exact shape `partial_stage_cli.rs` proves appliable. `None` falls back
/// to engine behavior (unreadable, binary, or empty files).
fn synthetic_untracked_diff(root: &std::path::Path, rel: &std::path::Path) -> Option<String> {
    let bytes = std::fs::read(root.join(rel)).ok()?;
    // Binary (NUL byte) or empty content has no meaningful granular diff.
    if bytes.is_empty() || bytes.contains(&0) {
        return None;
    }
    let content = String::from_utf8(bytes).ok()?;
    // Patch headers need slash-separated repo-relative paths.
    let display = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let n = content.lines().count();
    let mut out = format!(
        "diff --git a/{display} b/{display}\n\
         new file mode 100644\n\
         --- /dev/null\n\
         +++ b/{display}\n\
         @@ -0,0 +1,{n} @@\n"
    );
    for line in content.lines() {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    if !content.ends_with('\n') {
        out.push_str("\\ No newline at end of file\n");
    }
    Some(out)
}

/// Trigger an async diff load if the cache is missing/stale and not already
/// loading. Resets hunk navigation whenever a fresh load starts so the ‹n/N›
/// counter always describes the content being displayed.
pub(super) fn ensure_diff(
    state: &mut AppState,
    root: &std::path::Path,
    left: &Option<String>,
    right: &Option<String>,
    staged: bool,
    ignore_whitespace: bool,
    path: &Option<std::path::PathBuf>,
) {
    let key = diff_key(root, left, right, staged, ignore_whitespace, path);
    let stale = state
        .ui
        .diff_cache
        .as_ref()
        .map(|(k, _)| k != &key)
        .unwrap_or(true);
    if stale && !state.ui.diff_loading {
        state.ui.diff_error = None;
        state.ui.diff_current_hunk = 0;
        // The sub-hunk line selections refer to the outgoing content; the
        // granular module drops them with the rest of the per-diff
        // navigation state (spec R2, story 3) — the current hunk was reset
        // to the first hunk above.
        granular::on_diff_changed(state, path.as_deref());

        // Untracked previews never reach `git diff`; synthesize a creation
        // diff from worktree content synchronously and populate the cache
        // under the same key — no worker, no loading spinner (spec R2).
        let untracked = preview_status(state, path.as_deref()) == ChangeStatus::Unversioned;
        if untracked
            && let Some(rel) = path
            && let Some(text) = synthetic_untracked_diff(root, rel)
        {
            state.ui.diff_cache = Some((key, text));
            return;
        }

        state.ui.diff_loading = true;
        let executor: Arc<dyn turbogit_engine_api::GitExecutor> = state.executor.clone();
        let tx = state.tx.clone();
        let root = root.to_path_buf();
        // Phase L1: when the setting asks for in-process diffs, the worker
        // computes the patch with `similar`; `diff_text` itself falls back to
        // this same CLI call whenever the in-process path cannot produce it
        // (multi-file targets, unreadable sides, non-UTF-8 content).
        let in_process = state.settings.in_process_diffs;
        let opts = DiffOpts {
            staged,
            ignore_whitespace,
            left: left.clone(),
            right: right.clone(),
            path: path.clone(),
            ..DiffOpts::default()
        };
        std::thread::spawn(move || {
            let res = if in_process {
                diff_engine::diff_text(executor.as_ref(), &root, &opts)
            } else {
                executor.diff(&root, &opts)
            };
            let _ = tx.send(AppEvent::DiffReady { key, result: res });
        });
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
        FontId::new(12.0, FontFamily::Proportional),
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
    use crate::ui::diff::model::{FileMeta, IMAGE_CAP_BYTES};
    use turbogit_app::diff_data::PaneSide;
    use turbogit_app::events::DecodedImage;

    #[test]
    fn diff_byte_side_specs_mirror_diff_invocation() {
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
            byte_side_specs(&Some("HEAD".to_owned()), &None, false, &plain),
            (SideSpec::Rev("HEAD".to_owned()), SideSpec::Worktree)
        );
        // Staged chip (HEAD↔index): `git diff --cached`; index via stage-0.
        assert_eq!(
            byte_side_specs(&None, &None, true, &plain),
            (
                SideSpec::Rev("HEAD".to_owned()),
                SideSpec::Rev(":0".to_owned())
            )
        );
        // Local chip (index↔worktree): plain `git diff`.
        assert_eq!(
            byte_side_specs(&None, &None, false, &plain),
            (SideSpec::Rev(":0".to_owned()), SideSpec::Worktree)
        );
        // Explicit commit-to-commit targets pass their revs through.
        assert_eq!(
            byte_side_specs(
                &Some("abc123".to_owned()),
                &Some("def456".to_owned()),
                false,
                &plain
            ),
            (
                SideSpec::Rev("abc123".to_owned()),
                SideSpec::Rev("def456".to_owned())
            )
        );
        // New files have no old side; deleted files no new side.
        assert_eq!(
            byte_side_specs(&None, &None, false, &added).0,
            SideSpec::Missing
        );
        assert_eq!(
            byte_side_specs(&Some("HEAD".to_owned()), &None, false, &deleted).1,
            SideSpec::Missing
        );
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

    #[test]
    fn diff_decode_image_rejects_garbage_and_over_cap() {
        assert!(decode_image(b"not an image").is_none());
        let over_cap = vec![0u8; IMAGE_CAP_BYTES as usize + 1];
        assert!(decode_image(&over_cap).is_none());
    }

    #[test]
    fn diff_fetch_side_reads_rev_blob_and_worktree_file() {
        let png = {
            let img = image::DynamicImage::new_rgb8(2, 3);
            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
            buf.into_inner()
        };
        let exec = turbogit_engine::fake::FakeExecutor::new();
        exec.files_bytes
            .lock()
            .unwrap()
            .insert(std::path::PathBuf::from("art.png"), png.clone());

        // Rev side through the engine seam (`<rev>:<path>`).
        let blob = fetch_side(
            &exec,
            std::path::Path::new("/irrelevant"),
            &SideSpec::Rev("HEAD".to_owned()),
            "art.png",
            true,
        )
        .expect("rev side readable");
        assert_eq!(blob.byte_len, png.len() as u64);
        let decoded = blob.decoded.expect("png decodes");
        assert_eq!((decoded.width, decoded.height), (2, 3));

        // Worktree side reads the file when decoding…
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("art.png"), &png).unwrap();
        let blob = fetch_side(&exec, dir.path(), &SideSpec::Worktree, "art.png", true)
            .expect("worktree side readable");
        assert_eq!(blob.decoded.as_ref().map(|d| d.width), Some(2));

        // …and uses fs metadata only when lengths suffice (binary caption).
        let blob = fetch_side(&exec, dir.path(), &SideSpec::Worktree, "art.png", false)
            .expect("worktree side measurable");
        assert_eq!(blob.byte_len, png.len() as u64);
        assert!(blob.decoded.is_none());

        // A missing side (new/deleted file) yields nothing.
        assert!(fetch_side(&exec, dir.path(), &SideSpec::Missing, "art.png", true).is_none());
    }
}

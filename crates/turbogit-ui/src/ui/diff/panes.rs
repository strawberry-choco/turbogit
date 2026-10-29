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

pub(super) fn pane_byte_lens(entry: &PaneEntry) -> Option<(u64, u64)> {
    Some((entry.old.as_ref()?.byte_len, entry.new.as_ref()?.byte_len))
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
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

type PaneTextureCache = (u64, HashMap<(String, usize), egui::TextureHandle>);

thread_local! {
    static PANE_TEXTURES: RefCell<PaneTextureCache> = RefCell::new((0, HashMap::new()));
}

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

fn fitted(tex: Vec2, max: Vec2) -> Vec2 {
    if tex.x <= 0.0 || tex.y <= 0.0 {
        return Vec2::ZERO;
    }
    let scale = (max.x / tex.x).min(max.y / tex.y).min(1.0);
    tex * scale
}

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

fn image_cell(ui: &mut Ui, side: &PaneSide, tex: &egui::TextureHandle, max: Vec2) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space(6.0);
        ui.add(egui::Image::new(tex).fit_to_exact_size(fitted(tex.size_vec2(), max)));
        ui.add_space(2.0);
        ui.colored_label(Palette::INK_2, image_caption(side));
    });
}

pub(super) fn render_image_pane(
    ui: &mut Ui,
    state: &mut AppState,
    target: &DiffTarget,
    meta: &FileMeta,
) {
    let pane = PaneTarget::new(target.clone(), PaneFlavour::Image, meta.clone(), true);
    let tag = pane.texture_tag();
    let entry = match state.read(pane) {
        Read::Fresh(entry) => entry,
        _ => {
            centered_note(ui, "Loading image…");
            return;
        }
    };
    sync_pane_textures(state);
    let present: Vec<&PaneSide> = [entry.old.as_ref(), entry.new.as_ref()]
        .into_iter()
        .flatten()
        .collect();
    if present.is_empty() || present.iter().any(|s| s.image.is_none()) {
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
        (side, None) | (None, side) => {
            if let Some(side) = side {
                let tex = textures.iter().flatten().next().expect("image present");
                let width = ui.available_width();
                image_cell(ui, side, tex, Vec2::new(width, avail_h));
            }
        }
    }
}
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

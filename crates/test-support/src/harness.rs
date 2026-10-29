use egui::epaint::TextShape;
use egui::{Color32, FontFamily, Pos2, Rect, Shape, Ui, Vec2};
use egui_kittest::Harness;
use turbogit_app::state::AppState;
use turbogit_ui::theme::{configure_style, install_fonts};

pub fn shell_harness_over(state: AppState, size: Vec2) -> Harness<'static, AppState> {
    shell_harness_built(state, size, Tempo::KITTEST_DEFAULT, true)
}

pub fn shell_harness_over_animated(state: AppState, size: Vec2) -> Harness<'static, AppState> {
    shell_harness_built(state, size, Tempo::SIXTY_FPS, true)
}

/// `max_steps` is required rather than defaulted because kittest's default is **4**
/// and `Harness::run`, unlike `Harness::step`, **panics** once a run exceeds it. A
/// shell with an operation in flight never settles, so those suites need hundreds of
/// steps; a hidden default would restore the very 4 this exists to remove.
pub fn shell_harness_over_unstyled(
    state: AppState,
    size: Vec2,
    max_steps: u64,
) -> Harness<'static, AppState> {
    shell_harness_built(
        state,
        size,
        Tempo {
            max_steps,
            ..Tempo::KITTEST_DEFAULT
        },
        false,
    )
}

pub const KITTEST_DEFAULT_BOX: Vec2 = Vec2::new(800.0, 600.0);

#[derive(Clone, Copy)]
struct Tempo {
    max_steps: u64,
    step_dt: f32,
}

impl Tempo {
    const KITTEST_DEFAULT: Self = Self {
        max_steps: 4,
        step_dt: 1.0 / 4.0,
    };
    const SIXTY_FPS: Self = Self {
        step_dt: 1.0 / 60.0,
        ..Self::KITTEST_DEFAULT
    };
}

fn shell_harness_built(
    state: AppState,
    size: Vec2,
    tempo: Tempo,
    styled: bool,
) -> Harness<'static, AppState> {
    let mut fonts_installed = false;
    let mut harness = Harness::builder()
        .with_max_steps(tempo.max_steps)
        .with_step_dt(tempo.step_dt)
        .build_ui_state(
            move |ui, state| {
                if styled {
                    configure_style(ui.ctx());
                    if !fonts_installed {
                        install_fonts(ui.ctx());
                        fonts_installed = true;
                    }
                }
                state.drain_events();
                turbogit_ui::render(ui, state);
            },
            state,
        );
    harness.set_size(size);
    harness
}

pub fn widget_harness(size: Vec2, body: impl Fn(&mut Ui) + 'static) -> Harness<'static, ()> {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| body(ui));
        },
        (),
    );
    harness.set_size(size);
    harness
}

fn leaked_temp_dir() -> std::path::PathBuf {
    let dir = tempfile::tempdir().expect("temp config dir");
    let path = dir.path().to_path_buf();
    std::mem::forget(dir);
    path
}

/// The project dir is an empty temp directory: zero roots are discovered, so
/// the render is deterministic, no background git workers are spawned, and,
/// per the Welcome-vs-shell model (spec §9.2), the central body shows the
/// Welcome placeholder while every shell region still renders.
pub fn shell_harness() -> (Harness<'static, AppState>, tempfile::TempDir) {
    let project = tempfile::tempdir().expect("temp project dir");
    let state = AppState::launch_in(Some(project.path().to_path_buf()), Some(leaked_temp_dir()));
    assert!(
        state.multi.roots.is_empty(),
        "test project must discover no roots"
    );
    (shell_harness_over(state, Vec2::new(1024.0, 768.0)), project)
}

pub fn repo_shell_harness() -> (Harness<'static, AppState>, tempfile::TempDir) {
    let project = tempfile::tempdir().expect("temp project dir");
    let repo = crate::git_seed::repo_with_one_commit(project.path(), "repo");
    let state = AppState::launch_in(Some(repo), Some(leaked_temp_dir()));
    assert!(
        !state.multi.roots.is_empty(),
        "seeded repo root must be discovered"
    );
    (shell_harness_over(state, Vec2::new(1024.0, 768.0)), project)
}

pub fn painted_text<S>(harness: &Harness<'_, S>) -> Vec<String> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

#[track_caller]
pub fn assert_painted<S>(harness: &Harness<'_, S>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

#[track_caller]
pub fn assert_not_painted<S>(harness: &Harness<'_, S>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        !texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was unexpectedly painted; painted text:\n{texts:#?}"
    );
}

pub fn galley_origin<S>(harness: &Harness<'_, S>, text: &str) -> Option<Pos2> {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text() == text => Some(shape.pos),
            _ => None,
        })
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaintedGalley {
    pub text: String,
    pub pos: Pos2,
    pub rect: Rect,
    /// The ink this string was **painted** with.
    ///
    /// NOT simply `job.sections[0].format.color`, which is the obvious-looking
    /// one-liner and the wrong one. A galley keeps the colour it was *laid out*
    /// with, and this codebase lays text out in `Color32::WHITE`, then applies
    /// the real ink at paint time by overriding the text colour
    /// (`Painter::galley_with_override_text_color`). Reading the layout colour
    /// alone therefore reports `WHITE` for every shared button, kit button,
    /// sidebar row, menu item and segmented option, a test built on it passes
    /// or fails for the wrong reason. [`paint_ink`] is the one place that
    /// resolves paint-time override first and laid-out colour second.
    pub color: Color32,
    pub family: FontFamily,
}

fn paint_ink(shape: &TextShape) -> Color32 {
    shape
        .override_text_color
        .or_else(|| shape.galley.job.sections.first().map(|s| s.format.color))
        .unwrap_or(Color32::TRANSPARENT)
}

pub fn painted_galleys<S>(harness: &Harness<'_, S>) -> Vec<PaintedGalley> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(shape) => {
                let section = shape.galley.job.sections.first();
                Some(PaintedGalley {
                    text: shape.galley.text().to_owned(),
                    pos: shape.pos,
                    rect: Rect::from_min_size(shape.pos, shape.galley.size()),
                    color: paint_ink(shape),
                    family: section
                        .map(|s| s.format.font_id.family.clone())
                        .unwrap_or(FontFamily::Proportional),
                })
            }
            _ => None,
        })
        .collect()
}

pub fn painted_ink<S>(harness: &Harness<'_, S>, needle: &str) -> Option<Color32> {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == needle => Some(paint_ink(text)),
            _ => None,
        })
}

const ITEM_SPACING_Y: f32 = 3.0;

#[track_caller]
pub fn assert_matrix_geometry<S>(
    harness: &Harness<'_, S>,
    anchor: &str,
    columns: &[(&str, f32)],
    rows: &[&str],
    header_h: f32,
    row_h: f32,
) {
    let galleys = painted_galleys(harness);
    let same_line = |a: f32, b: f32| (a - b).abs() < 1.0;
    let same_x = |a: f32, b: f32| (a - b).abs() < 0.01;

    let anchor_galley = galleys
        .iter()
        .find(|g| g.text == anchor)
        .unwrap_or_else(|| panic!("`{anchor}` is not painted: no matrix header row"));
    let header_cy = anchor_galley.rect.center().y;
    let origins: Vec<f32> = columns
        .iter()
        .map(|(label, _)| {
            galleys
                .iter()
                .find(|g| g.text == *label && same_line(g.rect.center().y, header_cy))
                .unwrap_or_else(|| panic!("`{label}` is not painted on the matrix's header row"))
                .pos
                .x
        })
        .collect();

    let left = origins[0];
    for ((label, offset), origin) in columns.iter().zip(&origins) {
        assert!(
            (origin - (left + offset)).abs() < 0.01,
            "the `{label}` header label starts at {origin}, not at its column offset {offset} \
             from the matrix's own left edge {left}"
        );
    }

    let row_cys: Vec<f32> = rows
        .iter()
        .map(|name| {
            let mut ys: Vec<f32> = galleys
                .iter()
                .filter(|g| {
                    g.text == *name && same_x(g.pos.x, origins[0]) && g.rect.center().y > header_cy
                })
                .map(|g| g.rect.center().y)
                .collect();
            ys.sort_by(|a, b| a.partial_cmp(b).expect("a finite y centre"));
            ys.into_iter().next().unwrap_or_else(|| {
                panic!("the matrix's first column has no `{name}` cell under its header")
            })
        })
        .collect();

    for ((label, _), origin) in columns.iter().zip(&origins) {
        for cy in &row_cys {
            let hits = galleys
                .iter()
                .filter(|g| same_line(g.rect.center().y, *cy) && same_x(g.pos.x, *origin))
                .count();
            assert_eq!(
                hits, 1,
                "the `{label}` column starts {hits} galleys on the row at y {cy} at the \
                 header's own x {origin}: a column narrow in the header and wide in the rows \
                 moves pixels nothing was watching"
            );
        }
    }

    assert!(
        (row_cys[0] - header_cy - (header_h / 2.0 + row_h / 2.0 + ITEM_SPACING_Y)).abs() < 0.01,
        "the header band is not {header_h} tall: its centre sits {:.3} from the first row's, \
         expected {:.3}",
        row_cys[0] - header_cy,
        header_h / 2.0 + row_h / 2.0 + ITEM_SPACING_Y,
    );
    if let (Some(first), Some(second)) = (row_cys.first(), row_cys.get(1)) {
        assert!(
            (second - first - (row_h + ITEM_SPACING_Y)).abs() < 0.01,
            "the row band is not {row_h} tall: consecutive rows are {:.3} apart, expected \
             {:.3}",
            second - first,
            row_h + ITEM_SPACING_Y,
        );
    }

    assert_eq!(
        anchor_galley.color,
        turbogit_ui::theme::Palette::INK_3,
        "the header label's ink is not the muted step"
    );
    assert_eq!(
        anchor_galley.family,
        egui::FontFamily::Proportional,
        "the header label is not in the chrome face"
    );
    let first_cell = galleys
        .iter()
        .find(|g| same_line(g.rect.center().y, row_cys[0]) && same_x(g.pos.x, origins[0]))
        .expect("the first row's first cell is painted");
    assert_eq!(
        first_cell.color,
        turbogit_ui::theme::Palette::INK,
        "the first cell's ink is not the primary step"
    );
    assert_eq!(
        first_cell.family,
        egui::FontFamily::Proportional,
        "the first cell is not in the chrome face"
    );
}

pub fn filled_rects<S>(harness: &Harness<'_, S>) -> Vec<(Rect, Color32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rect_shape) if rect_shape.fill != Color32::TRANSPARENT => {
                Some((rect_shape.rect, rect_shape.fill))
            }
            _ => None,
        })
        .collect()
}

pub fn stroked_rects<S>(harness: &Harness<'_, S>) -> Vec<(Rect, Color32, f32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rect_shape)
                if rect_shape.stroke.color != Color32::TRANSPARENT
                    && rect_shape.stroke.width > 0.0 =>
            {
                Some((
                    rect_shape.rect,
                    rect_shape.stroke.color,
                    rect_shape.stroke.width,
                ))
            }
            _ => None,
        })
        .collect()
}

pub fn filled_circles<S>(harness: &Harness<'_, S>) -> Vec<(Pos2, f32, Color32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Circle(circle) if circle.fill != Color32::TRANSPARENT => {
                Some((circle.center, circle.radius, circle.fill))
            }
            _ => None,
        })
        .collect()
}

pub fn painted_paths<S>(harness: &Harness<'_, S>) -> Vec<(Rect, Color32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Path(path) => match &path.stroke.color {
                egui::epaint::ColorMode::Solid(color) => {
                    Some((path.visual_bounding_rect(), *color))
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

pub fn right_click_row<S>(harness: &mut Harness<'_, S>, needle: &str) {
    use egui_kittest::kittest::{NodeT as _, Queryable as _};
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|label| label.contains(needle))
        })
        .unwrap_or_else(|| panic!("no row whose label contains {needle}"))
        .click_secondary();
    harness.step();
    harness.step();
}

/// Primary-button click on the node labelled exactly `label`, with `modifiers`
/// held, then a full run so the frame's state is readable when this returns.
/// The `modifiers` field of `egui::Event::PointerButton` is decoration: egui
/// only ever sets `InputState::modifiers` from `ModifiersChanged` (or from
/// kittest's own `RawInput::modifiers`), so a test that wants a widget to read
/// a held key must go through `click_modifiers`, which presses the keys,
/// clicks, and releases them as separate frames.
#[track_caller]
pub fn click_with_modifiers<S>(
    harness: &mut Harness<'_, S>,
    label: &str,
    modifiers: egui::Modifiers,
) {
    use egui_kittest::kittest::Queryable as _;
    harness.get_by_label(label).click_modifiers(modifiers);
    harness.run();
}

/// Click one item of the context menu that is open.
/// `sentinel` is a label unique to THAT menu, used to find its left edge: the
/// command palette lists its own "Pull", "Push…" and "Checkout" actions
/// elsewhere, and only the menu's rows all share one left edge. A disabled item
/// is still found, the menu's convention is that a blocked action stays
/// visible.
pub fn click_menu_item<S>(harness: &mut Harness<'_, S>, sentinel: &str, label: &str) {
    use egui_kittest::kittest::{NodeT as _, Queryable as _};
    harness.remove_cursor();
    harness.step();
    let column = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(sentinel.to_string()))
        .unwrap_or_else(|| panic!("the menu is open (no {sentinel:?} item)"))
        .rect();
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| {
            n.accesskit_node().label() == Some(label.to_string())
                && (n.rect().min.x - column.min.x).abs() < 2.0
        })
        .unwrap_or_else(|| panic!("menu item {label} inside the open menu"))
        .click();
    harness.step();
}

#[track_caller]
pub fn assert_menu_item_gated<S>(harness: &mut Harness<'_, S>, sentinel: &str, label: &str) {
    use egui_kittest::kittest::{NodeT as _, Queryable as _};
    harness.remove_cursor();
    harness.step();
    let column = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(sentinel.to_string()))
        .unwrap_or_else(|| panic!("the menu is open (no {sentinel:?} item)"))
        .rect();
    let item = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| {
            n.accesskit_node().label() == Some(label.to_string())
                && (n.rect().min.x - column.min.x).abs() < 2.0
        })
        .unwrap_or_else(|| panic!("menu item {label} inside the open menu"));
    assert!(
        item.accesskit_node().is_disabled(),
        "{label} must be gated, not hidden"
    );
}

/// Step frames until the painted output stabilizes.
/// The first frames after startup relayout (embedded fonts take effect at
/// pass 2), so queries and clicks must only happen on a settled frame,
/// mirroring a user clicking an already-rendered shell.
pub fn settle<S>(harness: &mut Harness<'_, S>) {
    let mut prev = String::new();
    for _ in 0..10 {
        harness.step();
        let fingerprint = format!("{:?}", painted_text(harness));
        if fingerprint == prev {
            return;
        }
        prev = fingerprint;
    }
    panic!("shell layout did not settle within 10 frames");
}

/// Step frames until the painted text is byte-identical for **three consecutive**
/// frames, up to 300, sleeping 10ms between frames so a worker can post between them.
/// Kept separate from [`settle`] on purpose: that one answers "has the layout stopped
/// relaying out?" off a single repeat, which a shell with background work reaches
/// *between* worker repaints, so a suite that then clicks acts on stale coordinates.
pub fn settle_quiet<S>(harness: &mut Harness<'_, S>) {
    let mut stable = 0;
    let mut prev = String::new();
    for _ in 0..300 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let fingerprint = format!("{:?}", painted_text(harness));
        if fingerprint == prev {
            stable += 1;
            if stable >= 3 {
                return;
            }
        } else {
            stable = 0;
            prev = fingerprint;
        }
    }
    panic!("shell layout did not settle within 300 frames; last painted:\n{prev}");
}

/// Step frames until the painted output stabilizes **and no keyed read is
/// pending**, diff or blame, the gate does not say which.
///
/// Neither [`settle`] nor [`settle_quiet`] fits a surface whose content arrives
/// through the keyed read: both ask only whether the layout stopped relaying
/// out, so a suite could read a "Computing…" placeholder and call it a result.
pub fn settle_reads(harness: &mut Harness<'_, AppState>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut prev = String::new();
    while std::time::Instant::now() < deadline {
        harness.step();
        let fingerprint = format!(
            "{:?}|read={}",
            painted_text(harness),
            harness.state().read_pending(),
        );
        if fingerprint == prev && !harness.state().read_pending() {
            return;
        }
        prev = fingerprint;
    }
    panic!("the keyed-read surface did not settle within 15s");
}

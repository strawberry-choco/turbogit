use std::cell::Cell;
use std::rc::Rc;

use egui::{Color32, Shape};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use test_support::git_seed::git;
use test_support::harness::{
    assert_painted, painted_galleys, painted_paths, painted_text, settle, widget_harness,
};
use test_support::srcscan;
use turbogit_ui::theme::{BUTTON_PADDING, PILL_RADIUS, Palette, RAIL_WIDTH};
use turbogit_ui::ui::icons::Icon;
use turbogit_ui::ui::widgets::*;

fn ui_src_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui")
}

#[test]
fn palette_tokens_required_by_widgets_match_the_spec_hexes() {
    assert_eq!(Palette::BG, Color32::from_rgb(0x1e, 0x1f, 0x22));
    assert_eq!(Palette::SURFACE, Color32::from_rgb(0x2b, 0x2d, 0x30));
    assert_eq!(Palette::SURFACE_2, Color32::from_rgb(0x31, 0x34, 0x38));
    assert_eq!(Palette::SURFACE_3, Color32::from_rgb(0x3c, 0x3f, 0x41));
    assert_eq!(Palette::LINE, Color32::from_rgb(0x4e, 0x51, 0x57));
    assert_eq!(Palette::LINE_SUBTLE, Color32::from_rgb(0x36, 0x38, 0x3c));

    assert_eq!(Palette::INK, Palette::T_PRIMARY);
    assert_eq!(Palette::INK_2, Palette::T_SECONDARY);
    assert_eq!(Palette::INK_3, Palette::T_MUTED);
    assert_eq!(Palette::INK_4, Palette::T_DIM);
    assert_eq!(Palette::T_MUTED, Color32::from_rgb(0x8a, 0x8e, 0x96));
    assert_eq!(Palette::T_DIM, Color32::from_rgb(0x6e, 0x72, 0x7a));

    assert_eq!(Palette::BRAND, Color32::from_rgb(0x35, 0x74, 0xf0));
    assert_eq!(Palette::BRAND_INK, Color32::WHITE);

    assert_eq!(Palette::STATE_SUCCESS, Color32::from_rgb(0x4c, 0xaf, 0x50));
    assert_eq!(Palette::STATE_WARNING, Color32::from_rgb(0xf9, 0xa8, 0x25));
    assert_eq!(Palette::STATE_ERROR, Color32::from_rgb(0xef, 0x53, 0x50));
}

#[test]
fn surface_ladder_is_strictly_ordered() {
    let lum = |c: Color32| c.r() as u32 + c.g() as u32 + c.b() as u32;
    assert!(lum(Palette::BG) < lum(Palette::SURFACE));
    assert!(lum(Palette::SURFACE) < lum(Palette::SURFACE_2));
    assert!(lum(Palette::SURFACE_2) < lum(Palette::SURFACE_3));
    assert_ne!(Palette::SURFACE_2, Palette::BG);
    assert_ne!(Palette::SURFACE_3, Palette::SURFACE_2);
}

#[test]
fn badge_kind_maps_to_the_spec_status_colors() {
    assert_eq!(BadgeKind::Added.accent(), Palette::STATE_SUCCESS);
    assert_eq!(BadgeKind::Modified.accent(), Palette::STATE_WARNING);
    assert_eq!(BadgeKind::Deleted.accent(), Palette::STATE_ERROR);
    assert_eq!(BadgeKind::Neutral.accent(), Palette::INK_2);

    assert_eq!(BadgeKind::Neutral.colors().bg, Palette::SURFACE_3);
    for kind in [BadgeKind::Added, BadgeKind::Modified, BadgeKind::Deleted] {
        let colors = kind.colors();
        assert_eq!(
            colors.bg,
            tint_over_bg(kind.accent(), BADGE_TINT),
            "{kind:?} bg must be its accent tinted over BG"
        );
        assert_ne!(colors.bg, Color32::TRANSPARENT);
    }
}

#[test]
fn ref_kind_accent_maps_to_brand_success_warning() {
    assert_eq!(RefKind::Branch.accent(), Palette::BRAND);
    assert_eq!(RefKind::Remote.accent(), Palette::STATE_SUCCESS);
    assert_eq!(RefKind::Tag.accent(), Palette::STATE_WARNING);
}

#[test]
fn ghost_button_states_follow_the_widget_state_table() {
    use ButtonVariant::Ghost;
    use WidgetState::{Active, Disabled, Hovered, Idle};

    assert_eq!(Ghost.fill(Idle), Color32::TRANSPARENT);
    assert_eq!(Ghost.fill(Hovered), Palette::SURFACE_2);
    assert_eq!(Ghost.fill(Active), Palette::SURFACE_3);

    assert_eq!(Ghost.text(Idle), Palette::INK_2);
    assert_eq!(Ghost.text(Hovered), Palette::INK);
    assert_eq!(Ghost.text(Active), Palette::INK);
    assert_eq!(Ghost.text(Disabled), Palette::INK_3);

    assert_eq!(Ghost.fill(Disabled), Ghost.fill(Idle));
}

#[test]
fn primary_button_brightens_instead_of_surface_hover() {
    use ButtonVariant::Primary;
    use WidgetState::{Active, Disabled, Hovered, Idle};

    assert_eq!(Primary.fill(Idle), Palette::BRAND);
    assert_ne!(Primary.fill(Hovered), Palette::SURFACE_2);
    assert_ne!(Primary.fill(Active), Palette::SURFACE_3);

    let brighter =
        |a: Color32, b: Color32| a.r() >= b.r() && a.g() >= b.g() && a.b() >= b.b() && a != b;
    assert!(
        brighter(Primary.fill(Hovered), Primary.fill(Idle)),
        "hover must brighten the brand fill"
    );
    assert!(
        brighter(Primary.fill(Active), Primary.fill(Hovered)),
        "active must brighten past hover"
    );

    assert_eq!(Primary.text(Idle), Palette::BRAND_INK);
    assert_eq!(Primary.text(Hovered), Palette::BRAND_INK);
    assert_eq!(Primary.text(Active), Palette::BRAND_INK);
    assert_eq!(Primary.text(Disabled), Palette::INK_3);
    assert_eq!(Primary.fill(Disabled), Primary.fill(Idle));
}

#[test]
fn icon_only_and_icon_label_buttons_centre_their_glyph_on_both_axes() {
    let mut harness = widget_harness(egui::vec2(320.0, 120.0), |ui| {
        icon_button(ui, Icon::ALIGN_JUSTIFY);
        ghost_button(ui, Some(Icon::ALIGN_JUSTIFY), "Refresh");
    });
    harness.step();
    harness.step();

    let glyph_of = |label: &str| -> (egui::Rect, egui::Rect) {
        let frame = harness.get_by_label(label).rect();
        let rects: Vec<egui::Rect> = painted_paths(&harness)
            .into_iter()
            .filter(|(r, _)| frame.contains(r.center()))
            .map(|(r, _)| r)
            .collect();
        assert!(
            !rects.is_empty(),
            "`{label}` painted no glyph inside {frame:?}"
        );
        (
            frame,
            rects.into_iter().reduce(|a, b| a.union(b)).expect("union"),
        )
    };

    let (frame, glyph) = glyph_of("align-justify");
    assert!(
        (glyph.center().x - frame.center().x).abs() < 0.01,
        "an icon-only button's glyph is {} px from the frame's horizontal centre \
         (glyph at {}, frame at {}): the content measure must count the icon's own width",
        glyph.center().x - frame.center().x,
        glyph.center().x,
        frame.center().x,
    );
    assert!(
        (glyph.center().y - frame.center().y).abs() < 0.01,
        "an icon-only button's glyph is {} px from the frame's vertical centre",
        glyph.center().y - frame.center().y,
    );

    let pad_x = BUTTON_PADDING.x;
    let (frame, glyph) = glyph_of("Refresh");
    let label = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "Refresh")
        .expect("the labelled button paints its label")
        .rect;
    assert!(
        (label.max.x - (frame.max.x - pad_x)).abs() < 0.01,
        "a labelled button's group must end at its padding edge: label ends at {}, \
         expected {}",
        label.max.x,
        frame.max.x - pad_x,
    );
    assert!(
        glyph.min.x >= frame.min.x + pad_x - 0.01,
        "the glyph spills left of the button's padding edge: {} vs {}",
        glyph.min.x,
        frame.min.x + pad_x,
    );
    assert!(
        glyph.center().x < frame.center().x && label.center().x > frame.center().x,
        "a labelled button must sit its glyph left of centre and its label right, \
         got glyph at {} and label at {} around frame centre {}",
        glyph.center().x,
        label.center().x,
        frame.center().x,
    );
}

#[test]
fn compact_and_icon_variants_share_ghost_color_decisions() {
    for state in [
        WidgetState::Idle,
        WidgetState::Hovered,
        WidgetState::Active,
        WidgetState::Disabled,
    ] {
        assert_eq!(
            ButtonVariant::Compact.fill(state),
            ButtonVariant::Ghost.fill(state)
        );
        assert_eq!(
            ButtonVariant::Icon.fill(state),
            ButtonVariant::Ghost.fill(state)
        );
        assert_eq!(
            ButtonVariant::Compact.text(state),
            ButtonVariant::Ghost.text(state)
        );
        assert_eq!(
            ButtonVariant::Icon.text(state),
            ButtonVariant::Ghost.text(state)
        );
    }
}

#[test]
fn a_compact_primary_shares_the_primary_colour_ladder_and_fits_the_pane_header_band() {
    for state in [
        WidgetState::Idle,
        WidgetState::Hovered,
        WidgetState::Active,
        WidgetState::Disabled,
    ] {
        assert_eq!(
            ButtonVariant::CompactPrimary.fill(state),
            ButtonVariant::Primary.fill(state),
            "the compact primary is the primary: R1's brand fill may not have a \
             second value just because a button is shorter"
        );
        assert_eq!(
            ButtonVariant::CompactPrimary.text(state),
            ButtonVariant::Primary.text(state),
            "…and the on-brand ink is the same step in both"
        );
    }
    assert_eq!(
        ButtonVariant::CompactPrimary.fill(WidgetState::Idle),
        Palette::BRAND
    );
    assert_ne!(
        ButtonVariant::CompactPrimary,
        ButtonVariant::Primary,
        "a band-height primary is a separate member of the vocabulary, not a \
         spelling of the 32px one: the cross-pane header ratchet needs the height \
         to be a property a caller can ask for"
    );

    let mut band = widget_harness(egui::vec2(560.0, 200.0), move |ui| {
        pane_header(ui, "BAND", None, |ui| {
            compact_primary_button(ui, "Do the thing");
        });
    });
    settle(&mut band);

    let primary = band.get_by_label("Do the thing").rect();
    let rule = the_header_rule(&band);
    assert_eq!(
        primary.height(),
        PANE_HEADER_HEIGHT,
        "the shared compact primary is exactly the pane header's band height: a \
         taller control grows the band, and one pane's header being taller than \
         every other pane's is what the cross-pane geometry ratchet catches"
    );
    assert!(
        rule.top() - PANE_HEADER_HEIGHT <= primary.top() + 0.01
            && primary.bottom() <= rule.top() + 0.01,
        "and it fits *inside* its band rather than pushing the band's hairline \
         down: primary {primary:?}, hairline {rule:?}"
    );
}

type ClickFlag = Rc<Cell<bool>>;

fn widgets_harness(
    ghost_clicked: ClickFlag,
    compact_clicked: ClickFlag,
) -> (Harness<'static, ()>, tempfile::TempDir) {
    let search_buf = std::cell::RefCell::new(String::new());
    let name_buf = std::cell::RefCell::new(String::new());

    let harness = widget_harness(egui::vec2(800.0, 600.0), move |ui| {
        group_title(ui, "Recent");
        pane_header(ui, "CHANGED FILES", None, |_ui| {});

        if ghost_button(ui, None, "Ghost action").clicked() {
            ghost_clicked.set(true);
        }
        if primary_button(ui, Some(Icon::CHECK), "Primary action").clicked() {
            // Counted via painted assertion only.
        }
        if compact_button(ui, "Compact action").clicked() {
            compact_clicked.set(true);
        }
        icon_button(ui, Icon::X);

        badge(ui, "+3", BadgeKind::Added);
        badge(ui, "M", BadgeKind::Modified);
        badge(ui, "D", BadgeKind::Deleted);

        search_input(ui, "Search commits", &mut search_buf.borrow_mut());
        text_input(ui, "Branch name", &mut name_buf.borrow_mut());

        dialog_footer(ui, |ui| {
            primary_button(ui, None, "Footer OK");
        });
    });
    (harness, tempfile::tempdir().expect("tempdir"))
}

#[test]
fn inline_error_composes_visible_text_with_semantic_error_ink() {
    let mut harness = widget_harness(egui::vec2(480.0, 120.0), move |ui| {
        ui.label("page context");
        inline_error(ui, "the operation could not finish");
    });

    settle(&mut harness);
    assert_painted(&harness, "the operation could not finish");
    assert_eq!(
        painted_galleys(&harness)
            .into_iter()
            .find(|g| g.text.contains("the operation could not finish"))
            .map(|g| g.color),
        Some(Palette::STATE_ERROR)
    );
}

#[test]
fn keyed_read_presenter_renders_waiting_and_failure_from_display_inputs() {
    let mut harness = widget_harness(egui::vec2(480.0, 120.0), move |ui| {
        keyed_read_presentation(ui, KeyedReadPresentation::Waiting("Computing preview…"));
        keyed_read_presentation(
            ui,
            KeyedReadPresentation::Failed("Could not read the preview"),
        );
    });

    settle(&mut harness);
    assert_painted(&harness, "Computing preview…");
    assert_painted(&harness, "Could not read the preview");
    assert_eq!(
        painted_galleys(&harness)
            .into_iter()
            .find(|g| g.text.contains("Could not read the preview"))
            .map(|g| g.color),
        Some(Palette::STATE_ERROR)
    );
}

#[test]
fn shared_note_and_alert_feedback_keep_distinct_rendered_roles() {
    let mut harness = widget_harness(egui::vec2(360.0, 180.0), move |ui| {
        note(ui, None, |ui| {
            ui.label("context note");
        });
        alert_box(ui, "contained alert");
    });
    settle(&mut harness);
    assert_painted(&harness, "context note");
    assert_painted(&harness, "contained alert");
}

#[test]
fn chip_geometry_owns_radius_padding_measurement_and_text_placement() {
    assert_eq!(CHIP_GEOMETRY.height, CHIP_HEIGHT);
    assert_eq!(CHIP_GEOMETRY.pad_x, CHIP_PAD_X);
    assert_eq!(CHIP_GEOMETRY.radius, f32::from(PILL_RADIUS));
    assert_eq!(chip_radius(), egui::CornerRadius::same(PILL_RADIUS));

    let mut harness = widget_harness(egui::vec2(240.0, 80.0), move |ui| {
        badge(ui, "main", BadgeKind::Neutral);
    });
    harness.step();
    let galley = harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == "main" => Some(text.galley.clone()),
            _ => None,
        })
        .expect("badge should paint a main label");
    let size = CHIP_GEOMETRY.size(&galley);
    assert_eq!(size.y, CHIP_HEIGHT);
    assert_eq!(size.x, galley.size().x + 2.0 * CHIP_PAD_X);

    let rect = egui::Rect::from_min_size(egui::pos2(30.0, 40.0), size);
    let origin = chip_text_origin(rect, &galley);
    assert!((origin.x - (rect.center().x - galley.size().x / 2.0)).abs() < 0.01);
    assert!((origin.y - (rect.center().y - galley.size().y / 2.0)).abs() < 0.01);
}

#[test]
fn paint_centered_text_centres_one_galley_on_both_axes() {
    const INK: Color32 = Color32::from_rgb(0x11, 0x22, 0x33);
    let rect = egui::Rect::from_min_size(egui::pos2(12.0, 30.0), egui::vec2(240.0, 72.0));
    let mut harness = widget_harness(egui::vec2(360.0, 180.0), move |ui| {
        paint_centered_text(
            ui.painter(),
            rect,
            "centered",
            turbogit_ui::theme::chrome_font(turbogit_ui::theme::TYPE_BODY),
            INK,
        );
    });
    settle(&mut harness);

    let painted = painted_galleys(&harness);
    assert_eq!(
        painted.len(),
        1,
        "one string paints one galley: {painted:#?}"
    );
    let galley = &painted[0];
    assert_eq!(galley.text, "centered");
    assert_eq!(
        galley.color, INK,
        "the label is laid out and painted in `INK`"
    );
    assert!(
        (rect.width() - rect.height()).abs() > 1.0,
        "the test rect must not be square"
    );
    assert!(
        (galley.rect.width() - galley.rect.height()).abs() > 1.0,
        "the test galley must not be square, got {:?}",
        galley.rect.size()
    );
    let want_x = rect.center().x - galley.rect.width() / 2.0;
    let want_y = rect.center().y - galley.rect.height() / 2.0;
    assert!(
        (galley.pos.x - want_x).abs() < 0.01,
        "x: painted at {:?}, wanted {want_x}",
        galley.pos.x
    );
    assert!(
        (galley.pos.y - want_y).abs() < 0.01,
        "y: painted at {:?}, wanted {want_y}",
        galley.pos.y
    );
}

#[test]
fn multiple_shared_chips_coexist_without_overlap_or_geometry_regression() {
    let mut harness = widget_harness(egui::vec2(400.0, 80.0), move |ui| {
        ui.horizontal(|ui| {
            badge(ui, "added", BadgeKind::Added);
            badge(ui, "main", BadgeKind::Neutral);
            badge(ui, "gone", BadgeKind::Deleted);
        });
    });
    settle(&mut harness);

    let added = harness.get_by_label("added").rect();
    let neutral = harness.get_by_label("main").rect();
    let third = harness.get_by_label("gone").rect();
    for rect in [added, neutral, third] {
        assert_eq!(rect.height(), CHIP_HEIGHT);
    }
    assert!(added.right() <= neutral.left());
    assert!(neutral.right() <= third.left());
    assert_eq!(added.top(), neutral.top());
    assert_eq!(neutral.top(), third.top());
}

#[test]
fn smoke_render_paints_the_widget_vocabulary_together() {
    let (mut harness, _dir) = widgets_harness(Rc::default(), Rc::default());
    settle(&mut harness);

    assert_painted(&harness, "Ghost action");
    assert_painted(&harness, "Primary action");
    assert_painted(&harness, "Compact action");

    assert_painted(&harness, "+3");
    assert_painted(&harness, "M");
    assert_painted(&harness, "D");

    assert_painted(&harness, "Search commits");
    assert_painted(&harness, "Branch name");

    assert_painted(&harness, "RECENT");
    assert_painted(&harness, "CHANGED FILES");
}

#[test]
fn ghost_and_compact_buttons_click_through_the_accessibility_tree() {
    let ghost = Rc::new(Cell::new(false));
    let compact = Rc::new(Cell::new(false));
    let (mut harness, _dir) = widgets_harness(ghost.clone(), compact.clone());
    settle(&mut harness);

    harness.get_by_label("Ghost action").click();
    harness.get_by_label("Compact action").click();
    settle(&mut harness);

    assert!(ghost.get(), "ghost button click must register");
    assert!(compact.get(), "compact button click must register");
}

fn segmented_harness() -> (Harness<'static, ()>, tempfile::TempDir) {
    let harness = widget_harness(egui::vec2(400.0, 80.0), move |ui| {
        segmented_control(ui, &["One", "Two", "Three"], 1);
    });
    (harness, tempfile::tempdir().expect("tempdir"))
}

fn short_commit_ref_harness(reference: &'static str) -> (Harness<'static, ()>, tempfile::TempDir) {
    let harness = widget_harness(egui::vec2(240.0, 40.0), move |ui| {
        ui.label(short_commit_ref(reference));
    });
    (harness, tempfile::tempdir().expect("tempdir"))
}

#[test]
fn short_commit_reference_renders_seven_characters_by_default() {
    let (mut harness, _dir) = short_commit_ref_harness("0123456789abcdef");
    settle(&mut harness);

    assert_painted(&harness, "0123456");
    assert!(
        !painted_text(&harness)
            .iter()
            .any(|text| text.contains("01234567"))
    );
}

#[test]
fn already_short_commit_reference_renders_intact() {
    let (mut harness, _dir) = short_commit_ref_harness("abc1234");
    settle(&mut harness);

    assert_painted(&harness, "abc1234");
}

#[test]
fn non_ascii_commit_reference_renders_whole_characters() {
    let (mut harness, _dir) = short_commit_ref_harness("界界界界界界界界尾");
    settle(&mut harness);

    assert_painted(&harness, "界界界界界界界");
    assert!(
        !painted_text(&harness)
            .iter()
            .any(|text| text.contains('界') && text.chars().count() > 7)
    );
}

#[test]
fn segmented_control_lives_in_widgets_and_clicks_through_the_tree() {
    let (mut harness, _dir) = segmented_harness();
    settle(&mut harness);

    for label in ["One", "Two", "Three"] {
        assert_painted(&harness, label);
    }

    let mut saw_track = false;
    let mut saw_selected = false;
    for clipped in harness.output().shapes.iter() {
        if let Shape::Rect(rs) = &clipped.shape {
            if rs.fill == Palette::SURFACE_2 {
                saw_track = true;
            }
            if rs.fill == Palette::SURFACE_3 {
                saw_selected = true;
            }
        }
    }
    assert!(saw_track, "segmented track must paint SURFACE_2");
    assert!(saw_selected, "selected segment must paint SURFACE_3");

    harness.get_by_label("Three").click();
    settle(&mut harness);
    assert_painted(&harness, "Three");
}

fn painted_ink<S>(harness: &Harness<'_, S>, needle: &str) -> Color32 {
    test_support::harness::painted_ink(harness, needle)
        .unwrap_or_else(|| panic!("`{needle}` was not painted"))
}

fn rects_with_fill(harness: &Harness<'_, ()>) -> Vec<Color32> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rs) => Some(rs.fill),
            _ => None,
        })
        .collect()
}

#[derive(Debug, Default)]
struct ScopeProbe {
    inside_enabled: Vec<bool>,
    returned: Vec<u32>,
    caller_enabled: Vec<bool>,
    width_before: Vec<f32>,
    width_after: Vec<f32>,
    max_rect_before: Vec<egui::Rect>,
    max_rect_after: Vec<egui::Rect>,
    top_before: Vec<f32>,
    top_after: Vec<f32>,
}

#[test]
fn disabled_child_scope_dim_inside_and_leaves_the_caller_intact() {
    let probe = Rc::new(std::cell::RefCell::new(ScopeProbe::default()));
    let probe_ui = probe.clone();

    let mut harness = widget_harness(egui::vec2(420.0, 320.0), move |ui| {
        for (enabled, label) in [(false, "Dimmed"), (true, "Lit")] {
            let mut p = probe_ui.borrow_mut();
            p.width_before.push(ui.available_width());
            p.max_rect_before.push(ui.max_rect());
            p.top_before.push(ui.available_rect_before_wrap().top());

            let seq = p.returned.len() as u32 + 1;
            let mut inside = None;
            let value = disabled_child_scope(ui, enabled, |child| {
                inside = Some(child.is_enabled());
                primary_button(child, None, label);
                seq
            });

            p.inside_enabled
                .push(inside.expect("the closure must run in either state"));
            p.returned.push(value);
            p.caller_enabled.push(ui.is_enabled());
            p.width_after.push(ui.available_width());
            p.max_rect_after.push(ui.max_rect());
            p.top_after.push(ui.available_rect_before_wrap().top());
        }
        primary_button(ui, None, "After");
    });
    harness.step();

    let p = probe.borrow();
    assert!(
        p.inside_enabled.len() >= 2,
        "both the disabled and the enabled scope must run"
    );
    assert_eq!(
        &p.inside_enabled[..2],
        &[false, true],
        "only the disabled child scope may report !is_enabled()"
    );
    assert_eq!(
        &p.returned[..2],
        &[1, 2],
        "the closure's return value must survive the scope in both states"
    );
    assert_eq!(
        &p.caller_enabled[..2],
        &[true, true],
        "the caller stays enabled"
    );
    for i in 0..p.returned.len() {
        assert_eq!(
            p.width_before[i], p.width_after[i],
            "the child scope must not resize the caller"
        );
        assert_eq!(
            p.max_rect_before[i], p.max_rect_after[i],
            "the child scope must not re-lay-out the caller"
        );
        assert!(
            p.top_after[i] > p.top_before[i],
            "the caller's cursor must advance past the child ({} -> {})",
            p.top_before[i],
            p.top_after[i]
        );
    }
    drop(p);

    // (d) No-leak: the disabled scope dimmed its own control and nothing else.
    //     egui dims a disabled scope by multiplying that scope's painter
    //     opacity, so "dimmed" reads as a lower-alpha paint of the same token
    //     rather than a different token; the accessibility node is the
    //     token-level statement.
    let ink = |needle: &str| painted_ink(&harness, needle);
    let luma = |c: Color32| c.r() as u32 + c.g() as u32 + c.b() as u32;
    assert_eq!(
        ink("Lit"),
        Palette::BRAND_INK,
        "an enabled scope must leave the control at its enabled ink"
    );
    assert_eq!(
        ink("After"),
        Palette::BRAND_INK,
        "the control after a disabled scope must not inherit its dimmed ink"
    );
    assert!(
        luma(ink("Dimmed")) < luma(ink("Lit")),
        "a control inside a disabled scope must paint dimmed: {:?} vs {:?}",
        ink("Dimmed"),
        ink("Lit")
    );
    assert!(
        rects_with_fill(&harness).contains(&Palette::BRAND),
        "a disabled scope must not dim the caller's next control's fill either"
    );

    assert!(
        harness
            .get_by_label("Dimmed")
            .accesskit_node()
            .is_disabled(),
        "a control inside a disabled scope reports itself disabled"
    );
    assert!(
        !harness.get_by_label("After").accesskit_node().is_disabled(),
        "a control after a disabled scope must not be reported disabled"
    );
}

fn painted_rects<S>(
    harness: &Harness<'_, S>,
) -> Vec<(egui::Rect, Color32, egui::Stroke, egui::CornerRadius)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rs) => Some((rs.rect, rs.fill, rs.stroke, rs.corner_radius)),
            _ => None,
        })
        .collect()
}

fn default_card_harness() -> Harness<'static, ()> {
    widget_harness(egui::vec2(360.0, 400.0), move |ui| {
        card(ui, CardFrame::default(), |ui| {
            ui.label("default body");
        });
    })
}

fn galley_x<S>(harness: &Harness<'_, S>, text: &str) -> f32 {
    painted_galleys(harness)
        .into_iter()
        .find(|g| g.text == text)
        .map(|g| g.pos.x)
        .unwrap_or_else(|| panic!("`{text}` was not painted"))
}

fn card_rects(
    harness: &Harness<'_, ()>,
) -> Vec<(egui::Rect, Color32, egui::Stroke, egui::CornerRadius)> {
    painted_rects(harness)
        .into_iter()
        .filter(|(_, fill, _, radius)| {
            matches!(*fill, Palette::CONTENT_BG | Palette::SURFACE)
                && *radius == egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS)
        })
        .filter(|(rect, _, _, _)| rect.height() > 20.0)
        .collect()
}

#[test]
fn card_frame_owns_its_shape_and_honours_pad_surface_and_width() {
    let panel_width = Rc::new(Cell::new(0.0_f32));
    let panel_width_ui = panel_width.clone();

    let mut harness = widget_harness(egui::vec2(360.0, 400.0), move |ui| {
        panel_width_ui.set(ui.available_width());
        card(ui, CardFrame::default(), |ui| {
            ui.label("default body");
        });
        card(ui, CardFrame::default().raised().padded(24), |ui| {
            ui.label("raised body");
        });
        card(ui, CardFrame::default().padded(16), |ui| {
            ui.label("padded body");
        });
        card(
            ui,
            CardFrame::default().raised().padded(20).min_width(420.0),
            |ui| {
                ui.label("pinned body");
            },
        );
    });
    settle(&mut harness);

    let rects = painted_rects(&harness);
    const EPS: f32 = 0.01;

    let (default_rect, default_radius) = rects
        .iter()
        .find(|(rect, fill, _, _)| *fill == Palette::CONTENT_BG && rect.height() > 20.0)
        .map(|(rect, _, _, radius)| (*rect, *radius))
        .expect("the default card must paint a CONTENT_BG frame");
    assert_eq!(
        default_radius,
        egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS),
        "the card radius is the shared one, not a per-call-site value"
    );

    let panel = panel_width.get();
    assert!(
        (default_rect.width() - panel).abs() <= EPS,
        "the default card must stretch: {} vs available {panel}",
        default_rect.width()
    );
    assert!(
        (galley_x(&harness, "default body")
            - default_rect.left()
            - turbogit_ui::theme::PANEL_PADDING)
            .abs()
            <= EPS,
        "the default card must pad by exactly PANEL_PADDING, got {}",
        galley_x(&harness, "default body") - default_rect.left()
    );

    let surface: Vec<egui::Rect> = rects
        .iter()
        .filter(|(rect, fill, _, _)| *fill == Palette::SURFACE && rect.height() > 20.0)
        .map(|(rect, _, _, _)| *rect)
        .collect();
    assert_eq!(
        surface.len(),
        2,
        "both `raised()` cards must paint, got {surface:?}"
    );
    let raised_rect = *surface
        .iter()
        .find(|rect| (rect.width() - panel).abs() <= EPS)
        .expect("the padded raised card must still stretch to the pane");
    let pinned_rect = *surface
        .iter()
        .find(|rect| (rect.width() - (420.0 + 2.0 * 20.0)).abs() <= EPS)
        .unwrap_or_else(|| {
            panic!("the pinned card must honour its 420 min width, got {surface:?}")
        });

    let body_x = |text: &str| galley_x(&harness, text);
    assert!(
        (body_x("raised body") - body_x("default body") - 12.0).abs() <= EPS,
        "`.padded(24)` must inset the body 12 further than `.padded(12)`, got {}",
        body_x("raised body") - body_x("default body")
    );
    assert!(
        (body_x("padded body") - body_x("default body") - 4.0).abs() <= EPS,
        "`.padded(16)` must inset the body 4 further than `.padded(12)`, got {}",
        body_x("padded body") - body_x("default body")
    );
    assert!(
        (body_x("pinned body") - body_x("default body") - 8.0).abs() <= EPS,
        "the pinned card must inset the body 8 further than `.padded(12)`, got {}",
        body_x("pinned body") - body_x("default body")
    );

    assert!(
        (pinned_rect.width() - panel).abs() > EPS,
        "a MinWidth card must not stretch to the pane ({})",
        pinned_rect.width()
    );
    assert!(
        raised_rect.width() < pinned_rect.width(),
        "a stretched card must be narrower than one pinned past the pane"
    );
}

#[test]
fn card_paints_no_stroke_where_a_default_card_is() {
    let mut harness = widget_harness(egui::vec2(360.0, 400.0), move |ui| {
        card(ui, CardFrame::default(), |ui| {
            ui.label("default body");
        });
        card(ui, CardFrame::default().padded(16), |ui| {
            ui.label("padded body");
        });
        card(ui, CardFrame::default().raised().padded(16), |ui| {
            ui.label("raised body");
        });
    });
    settle(&mut harness);

    let rects = painted_rects(&harness);
    let cards: Vec<egui::Rect> = card_rects(&harness)
        .into_iter()
        .map(|(rect, _, _, _)| rect)
        .collect();
    assert_eq!(
        cards.len(),
        3,
        "all three default cards must paint, got {cards:?}"
    );

    for card in &cards {
        let strokes_over_card: Vec<&(egui::Rect, Color32, egui::Stroke, egui::CornerRadius)> =
            rects
                .iter()
                .filter(|(rect, _, stroke, _)| {
                    stroke.color != Color32::TRANSPARENT
                        && stroke.width > 0.0
                        && rect.intersects(*card)
                })
                .collect();
        assert!(
            strokes_over_card.is_empty(),
            "a card is a surface, so nothing may stroke it — the card at {card:?} is \
             covered by {strokes_over_card:?}"
        );
    }

    let any_stroke = rects
        .iter()
        .find(|(_, _, stroke, _)| stroke.color != Color32::TRANSPARENT && stroke.width > 0.0);
    assert!(
        any_stroke.is_none(),
        "a frame of default cards must paint no stroke anywhere, found {any_stroke:?}"
    );
}

#[test]
fn the_bordered_card_is_the_one_surface_allowed_to_stroke() {
    let mut harness = widget_harness(egui::vec2(360.0, 400.0), move |ui| {
        card(ui, CardFrame::default(), |ui| {
            ui.label("default body");
        });
        card(
            ui,
            CardFrame::default()
                .raised()
                .bordered()
                .padded(20)
                .min_width(420.0),
            |ui| {
                ui.label("pinned body");
            },
        );
    });
    settle(&mut harness);

    let rects = painted_rects(&harness);
    let (bordered_rect, bordered_stroke, bordered_radius) = rects
        .iter()
        .find(|(rect, fill, stroke, _)| {
            *fill == Palette::SURFACE && stroke.width > 0.0 && rect.height() > 20.0
        })
        .map(|(rect, _, stroke, radius)| (*rect, *stroke, *radius))
        .expect("`.bordered()` must paint a stroked SURFACE card");
    assert_eq!(
        bordered_stroke,
        egui::Stroke::new(1.0, Palette::LINE),
        "a bordered card is a 1px `LINE` hairline — the one stroke the app allows"
    );
    assert_eq!(
        bordered_radius,
        egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS),
        "a bordered card keeps the card radius; the border is not its own shape"
    );
    // The border's cost, named rather than tolerated: egui folds
    // `Frame::stroke.width` into the frame's inner margin, so a bordered card is
    // one point wider on each side than the same card unbordered. 462 = 420 of
    // pinned body + 2×20 of padding + 2×1 of hairline.
    assert!(
        (bordered_rect.width() - (420.0 + 2.0 * 20.0 + 2.0 * 1.0)).abs() <= 0.01,
        "a bordered pinned card is its 460 plus the 1pt hairline on each side, got {}",
        bordered_rect.width()
    );
    assert!(
        (galley_x(&harness, "pinned body") - bordered_rect.left() - (20.0 + 1.0)).abs() <= 0.01,
        "a bordered card's body sits one point inside its padding, got {}",
        galley_x(&harness, "pinned body") - bordered_rect.left()
    );

    let (default_rect, default_stroke) = rects
        .iter()
        .find(|(rect, fill, _, _)| *fill == Palette::CONTENT_BG && rect.height() > 20.0)
        .map(|(rect, _, stroke, _)| (*rect, *stroke))
        .expect("the default card must paint a CONTENT_BG frame");
    assert_eq!(
        default_stroke,
        egui::Stroke::NONE,
        "the default card next to a bordered one still strokes nothing, got {default_stroke:?} at {default_rect:?}"
    );
}

#[test]
fn dropping_the_cards_hairline_moved_nothing_but_the_hairlines_own_pixel() {
    let mut harness = default_card_harness();
    settle(&mut harness);

    const EPS: f32 = 0.01;
    const BODY_INSET_WITH_HAIRLINE: f32 = 13.0;
    const BOX_HEIGHT_WITH_HAIRLINE: f32 = 40.0;

    let rects = painted_rects(&harness);
    let (card_rect, radius) = rects
        .iter()
        .find(|(rect, fill, _, _)| *fill == Palette::CONTENT_BG && rect.height() > 20.0)
        .map(|(rect, _, _, radius)| (*rect, *radius))
        .expect("the default card must paint a CONTENT_BG frame");
    assert_eq!(
        radius,
        egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS)
    );

    let body_x = galley_x(&harness, "default body");
    let inset = body_x - card_rect.left();
    assert!(
        (card_rect.width() - 328.0).abs() <= EPS,
        "the card must still claim the whole pane, got {}",
        card_rect.width()
    );
    assert!(
        (inset - turbogit_ui::theme::PANEL_PADDING).abs() <= EPS,
        "the body must sit exactly PANEL_PADDING inside the card, got {inset} \
         (it sat {BODY_INSET_WITH_HAIRLINE} while the card stroked)"
    );
    assert!(
        (card_rect.height() - (BOX_HEIGHT_WITH_HAIRLINE - 2.0)).abs() <= EPS,
        "the card must lose exactly the 2pt the two hairlines cost, got {} vs {}",
        card_rect.height(),
        BOX_HEIGHT_WITH_HAIRLINE - 2.0
    );
    assert!(
        (BODY_INSET_WITH_HAIRLINE - inset - 1.0).abs() <= EPS,
        "dropping the hairline must recover exactly its own 1pt of padding, \
         recovered {}",
        BODY_INSET_WITH_HAIRLINE - inset
    );
}

const CHIPS_SRC: &str = include_str!("../src/ui/widgets/chips.rs");

const CONTAINERS_SRC: &str = include_str!("../src/ui/widgets/containers.rs");
const CONFLICT_PANE_SRC: &str = include_str!("../src/ui/kit/conflict_pane.rs");
const WIDGETS_MOD_SRC: &str = include_str!("../src/ui/widgets/mod.rs");

const REF_NAME: &str = "feature/multi-root";
const CURRENT_NAME: &str = "main";
const COUNT_TEXT: &str = "12";

fn three_chips_harness() -> Harness<'static, ()> {
    widget_harness(egui::vec2(460.0, 80.0), move |ui| {
        ui.horizontal(|ui| {
            ref_chip(ui, REF_NAME);
            current_chip(ui, CURRENT_NAME);
            count_chip(ui, COUNT_TEXT);
        });
    })
}

fn chip_sized_rects<S>(harness: &Harness<'_, S>) -> Vec<(egui::Rect, Color32, egui::CornerRadius)> {
    painted_rects(harness)
        .into_iter()
        .filter(|(rect, _, _, _)| (rect.height() - CHIP_HEIGHT).abs() < 0.01)
        .map(|(rect, fill, _, radius)| (rect, fill, radius))
        .collect()
}

fn the_chip_rect_with<S>(
    harness: &Harness<'_, S>,
    fill: Color32,
    who: &str,
) -> (egui::Rect, egui::CornerRadius) {
    let hits: Vec<_> = chip_sized_rects(harness)
        .into_iter()
        .filter(|(_, painted, _)| *painted == fill)
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one {who} at chip height in {fill:?}, found {hits:#?}"
    );
    let (rect, _, radius) = hits.into_iter().next().expect("one chip rect");
    (rect, radius)
}

fn painted_font<S>(harness: &Harness<'_, S>, needle: &str) -> egui::FontId {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == needle => text
                .galley
                .job
                .sections
                .first()
                .map(|s| s.format.font_id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("`{needle}` was not painted"))
}

/// The number of laid-out rows in the single galley painting exactly `needle`.
/// The wrapping claim, taken from geometry: the galley's *text* accessor returns
/// the unwrapped input, so a chip that wrapped looks identical through it.
fn painted_rows(harness: &Harness<'_, ()>, needle: &str) -> usize {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(text) if text.galley.text() == needle => Some(text.galley.rows.len()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("`{needle}` was not painted"))
}

fn the_only_galley(
    harness: &Harness<'_, ()>,
    needle: &str,
) -> test_support::harness::PaintedGalley {
    let hits: Vec<_> = painted_galleys(harness)
        .into_iter()
        .filter(|g| g.text == needle)
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "`{needle}` must paint exactly once in the chip frame, found {hits:#?}"
    );
    hits.into_iter().next().expect("one galley")
}

#[test]
fn exactly_one_function_produces_each_of_the_three_chips() {
    assert_eq!(
        srcscan::items_naming(CHIPS_SRC, "Palette::RAISED_ON_CARD"),
        ["REF_CHIP_COLORS"],
        "the ref chip's fill is defined in exactly one place"
    );
    assert_eq!(
        srcscan::items_naming(CHIPS_SRC, "Palette::ROW_SELECTED"),
        ["CURRENT_CHIP_COLORS"],
        "the current chip's fill is defined in exactly one place"
    );
    assert_eq!(
        srcscan::items_naming(CHIPS_SRC, "Palette::RAISED"),
        ["COUNT_CHIP_COLORS"],
        "the count chip's fill is defined in exactly one place"
    );
    assert_eq!(
        srcscan::fns_using(CHIPS_SRC, "REF_CHIP_COLORS"),
        ["ref_chip"]
    );
    assert_eq!(
        srcscan::fns_using(CHIPS_SRC, "CURRENT_CHIP_COLORS"),
        ["current_chip"]
    );
    assert_eq!(
        srcscan::fns_using(CHIPS_SRC, "COUNT_CHIP_COLORS"),
        ["count_chip"]
    );

    assert_eq!(
        srcscan::fns_using(CHIPS_SRC, "chip_with"),
        ["badge", "count_chip", "current_chip", "ref_chip"],
        "`chip_with` is the only thing that paints a chip body: a fifth caller is \
         a fourth chip, or a second badge, and it has to be named here first"
    );
    assert_eq!(
        srcscan::fns_using(CHIPS_SRC, "COMPACT_CHIP_GEOMETRY"),
        [
            "compact_chip_radius",
            "count_chip",
            "current_chip",
            "ref_chip"
        ],
        "the chip geometry is read by the three constructors and by the radius \
         accessor beside them — nobody else, so the chip set stays closed"
    );
    assert_eq!(
        srcscan::fns_using(CHIPS_SRC, "CHIP_GEOMETRY"),
        [
            "badge",
            "chip_radius",
            "chip_rect_right",
            "chip_text_origin",
            "paint_chip",
        ],
        "the pill slot keeps its own callers: the badge, the four public \
         geometry helpers, and no chip"
    );

    assert!(
        srcscan::items_naming(CHIPS_SRC, "Sense::click").is_empty(),
        "no chip may gain a click plane: a chip that is a label inherits \
         `hash_chip`'s hover-only sense, and one that is a control carries its \
         own node"
    );
}

#[test]
fn no_chip_names_the_reserved_counter_or_a_repository_state_colour() {
    for reserved in [
        "Palette::COUNTER",
        "Palette::AHEAD",
        "Palette::STATUS_DIVERGED",
    ] {
        assert_eq!(
            srcscan::items_naming(CHIPS_SRC, reserved),
            Vec::<String>::new(),
            "`{reserved}` means dirt / unpushed / diverged and is a mark, never \
             a chip fill: a state behind a fill stops reading as state"
        );
    }
    assert_eq!(COUNT_CHIP_COLORS.bg, Palette::RAISED);
    assert_eq!(COUNT_CHIP_COLORS.fg, Palette::INK_2);
}

#[test]
fn the_hash_chip_paints_the_hash_token_and_not_the_action_accent() {
    const HASH: &str = "a1b2c3d";
    let mut harness = widget_harness(egui::vec2(800.0, 600.0), move |ui| {
        hash_chip(ui, HASH, "");
    });
    settle(&mut harness);

    assert_eq!(
        painted_ink(&harness, HASH),
        Palette::LINK,
        "the hash chip's ink is the hash token"
    );
    assert_ne!(
        painted_ink(&harness, HASH),
        Palette::BRAND,
        "…and not the action accent: a chip that answers no press may not look \
         like one, and the accent is unreadable on this fill anyway"
    );
    assert!(
        rects_with_fill(&harness).contains(&Palette::SURFACE_3),
        "the hash chip keeps its SURFACE_3 fill; only its ink moved off the accent"
    );
}

#[test]
fn the_ref_chip_paints_the_raised_on_card_fill_at_the_chip_radius_in_the_data_face() {
    let mut harness = three_chips_harness();
    settle(&mut harness);

    let (rect, radius) = the_chip_rect_with(&harness, Palette::RAISED_ON_CARD, "the ref chip");
    assert_eq!(
        radius,
        egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS),
        "the ref chip is the compact chip, not the pill"
    );

    assert_eq!(painted_ink(&harness, REF_NAME), Palette::INK_2);

    let font = painted_font(&harness, REF_NAME);
    assert_eq!(
        font.family,
        egui::FontFamily::Monospace,
        "a ref chip holds a ref name, which is data"
    );
    assert_eq!(
        font.size,
        turbogit_ui::theme::TYPE_CHIP,
        "a chip's type size is the chip type size"
    );
    let galley = the_only_galley(&harness, REF_NAME);
    assert_eq!(galley.family, egui::FontFamily::Monospace);

    assert_eq!(
        painted_rows(&harness, REF_NAME),
        1,
        "a chip never wraps: the second row would be clipped by an 18px body"
    );
    assert!(
        (rect.width() - (galley.rect.width() + 2.0 * CHIP_PAD_X)).abs() < 0.01,
        "the chip reserved one row of text plus the shared inset on each side, \
         got {} for a {}-wide row",
        rect.width(),
        galley.rect.width()
    );

    // No click plane, at the render seam too. The chip answers as a **Label**:
    // the ref name is findable by name, a screen reader can read a branch off
    // the row, without the node claiming a verb. A clickable chip would answer
    // Click, which is the ADR-0024 failure: an unnamed button where there is no
    // button. (Asserted positively rather than by the absence of a button node:
    // `get_all_by_role` panics when nothing matches, so "no button" is not a
    // shape this harness can express.)
    let nodes: Vec<_> = harness.get_all_by_label(REF_NAME).collect();
    assert_eq!(
        nodes.len(),
        1,
        "the ref chip answers under its own name, once: {nodes:?}"
    );
    assert_eq!(
        nodes[0].accesskit_node().role(),
        egui::accesskit::Role::Label,
        "a ref chip is a label, not a button"
    );
    for text in [CURRENT_NAME, COUNT_TEXT] {
        let node = harness
            .get_all_by_label(text)
            .next()
            .unwrap_or_else(|| panic!("`{text}` answers under its own name"));
        assert_eq!(
            node.accesskit_node().role(),
            egui::accesskit::Role::Label,
            "`{text}` is a chip label too, not a button"
        );
    }
}

#[test]
fn the_current_chip_is_the_only_chip_allowed_near_the_brand() {
    let mut harness = three_chips_harness();
    settle(&mut harness);

    let (rect, radius) = the_chip_rect_with(&harness, Palette::ROW_SELECTED, "the current chip");
    assert_eq!(rect.height(), CHIP_HEIGHT);
    assert_eq!(
        radius,
        egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS)
    );
    assert_eq!(painted_ink(&harness, CURRENT_NAME), Palette::ACCENT_TEXT);

    let chips = [
        ("ref chip", REF_CHIP_COLORS),
        ("current chip", CURRENT_CHIP_COLORS),
        ("count chip", COUNT_CHIP_COLORS),
    ];
    assert_eq!(
        Palette::ACCENT,
        Palette::BRAND,
        "the accent and the brand are one token, so the negative below covers \
         both spellings"
    );
    for (who, colors) in chips {
        assert_ne!(
            colors.bg,
            Palette::BRAND,
            "the {who} must not fill the brand token: a brand fill behind a \
             running string is a button's shape"
        );
    }
    let accented: Vec<&str> = chips
        .iter()
        .filter(|(_, colors)| matches!(colors.fg, Palette::ACCENT_TEXT | Palette::BRAND))
        .map(|(who, _)| *who)
        .collect();
    assert_eq!(
        accented,
        ["current chip"],
        "exactly one chip may wear an accent, and it is the one stating a fact \
         about the current ref rather than asking for something"
    );
    assert_ne!(
        CURRENT_CHIP_COLORS.fg,
        Palette::BRAND_INK,
        "a solid brand fill is gone, so its on-brand ink has nothing to sit on"
    );

    let brand_fills: Vec<egui::Rect> = painted_rects(&harness)
        .into_iter()
        .filter(|(_, fill, _, _)| *fill == Palette::BRAND)
        .map(|(rect, _, _, _)| rect)
        .collect();
    assert!(
        brand_fills.is_empty(),
        "no chip fills the brand token: {brand_fills:?}"
    );
    let brand_ink: Vec<String> = painted_galleys(&harness)
        .into_iter()
        .filter(|g| g.color == Palette::BRAND)
        .map(|g| g.text.clone())
        .collect();
    assert!(
        brand_ink.is_empty(),
        "no chip inks the brand token: {brand_ink:?}"
    );
}

#[test]
fn the_count_chip_paints_the_raised_fill_with_secondary_mono_ink() {
    let mut harness = three_chips_harness();
    settle(&mut harness);

    let (rect, radius) = the_chip_rect_with(&harness, Palette::RAISED, "the count chip");
    assert_eq!(
        radius,
        egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS)
    );
    assert_eq!(rect.height(), CHIP_HEIGHT);

    assert_eq!(painted_ink(&harness, COUNT_TEXT), Palette::INK_2);
    let font = painted_font(&harness, COUNT_TEXT);
    assert_eq!(font.family, egui::FontFamily::Monospace);
    assert_eq!(font.size, turbogit_ui::theme::TYPE_CHIP);
    assert_eq!(painted_rows(&harness, COUNT_TEXT), 1);
    assert_ne!(COUNT_CHIP_COLORS.bg, Palette::COUNTER);
    assert_ne!(COUNT_CHIP_COLORS.fg, Palette::COUNTER);
}

#[test]
fn a_frame_of_three_chips_paints_exactly_three_chips() {
    let mut harness = three_chips_harness();
    settle(&mut harness);

    let rects = chip_sized_rects(&harness);
    let mut fills: Vec<Color32> = rects.iter().map(|(_, fill, _)| *fill).collect();
    fills.sort_by_key(|c| (c.r(), c.g(), c.b()));
    let mut expected = vec![
        Palette::RAISED,
        Palette::RAISED_ON_CARD,
        Palette::ROW_SELECTED,
    ];
    expected.sort_by_key(|c| (c.r(), c.g(), c.b()));
    assert_eq!(
        fills, expected,
        "the chip set is closed: every chip-sized rect in the frame is one of \
         the three chips, and there is no fourth"
    );
    assert!(
        !fills.contains(&BadgeKind::Neutral.colors().bg),
        "the status badge keeps its own slot and is not a chip"
    );
}

#[test]
fn the_status_badge_keeps_the_pill_radius_and_the_two_radii_stay_distinct() {
    assert_eq!(
        turbogit_ui::theme::CHIP_RADIUS,
        3,
        "the chip radius is the compact one"
    );
    assert_eq!(
        turbogit_ui::theme::PILL_RADIUS,
        9,
        "the pill radius is the full-height wrap"
    );
    assert_ne!(
        turbogit_ui::theme::CHIP_RADIUS,
        turbogit_ui::theme::PILL_RADIUS,
        "the two radii are different shapes for different jobs"
    );
    assert_eq!(
        COMPACT_CHIP_GEOMETRY.radius,
        f32::from(turbogit_ui::theme::CHIP_RADIUS)
    );
    assert_eq!(
        CHIP_GEOMETRY.radius,
        f32::from(turbogit_ui::theme::PILL_RADIUS)
    );
    assert_eq!(
        compact_chip_radius(),
        egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS)
    );
    assert_eq!(
        chip_radius(),
        egui::CornerRadius::same(turbogit_ui::theme::PILL_RADIUS)
    );
    assert_ne!(compact_chip_radius(), chip_radius());

    let mut harness = widget_harness(egui::vec2(520.0, 80.0), move |ui| {
        ui.horizontal(|ui| {
            badge(ui, "A", BadgeKind::Added);
            ref_chip(ui, REF_NAME);
            current_chip(ui, CURRENT_NAME);
            count_chip(ui, COUNT_TEXT);
        });
    });
    settle(&mut harness);

    let badge_rect = painted_rects(&harness)
        .into_iter()
        .find(|(_, fill, _, _)| *fill == BadgeKind::Added.colors().bg)
        .map(|(rect, _, _, radius)| (rect, radius))
        .expect("the badge paints its own tinted fill");
    assert_eq!(
        badge_rect.1,
        egui::CornerRadius::same(turbogit_ui::theme::PILL_RADIUS),
        "the status badge keeps the pill radius: it is a status, not a ref"
    );
    for (who, fill) in [
        ("ref chip", Palette::RAISED_ON_CARD),
        ("current chip", Palette::ROW_SELECTED),
        ("count chip", Palette::RAISED),
    ] {
        let radius = chip_sized_rects(&harness)
            .into_iter()
            .find(|(_, painted, _)| *painted == fill)
            .map(|(_, _, radius)| radius)
            .unwrap_or_else(|| panic!("the {who} paints at chip height"));
        assert_eq!(
            radius,
            egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS),
            "the {who} is the compact chip, not the pill"
        );
    }
}

#[test]
fn the_tool_window_header_is_retired_from_the_shared_widget_surface() {
    let retired: Vec<String> = srcscan::sources_under(ui_src_root())
        .iter()
        .filter(|(_, src)| {
            srcscan::items_naming(src, "toolwindow_header")
                .iter()
                .any(|n| n == "toolwindow_header")
        })
        .map(|(rel, _)| rel.clone())
        .collect();
    assert!(
        retired.is_empty(),
        "`toolwindow_header` is declared again in {retired:?}: there is exactly one \
         pane header (`widgets::pane_header`), and a second implementation is not a \
         second spelling of it — it is a header the user can tell apart from the first"
    );

    for name in ["toolwindow_header", "TOOLWINDOW_HEADER_HEIGHT"] {
        assert!(
            srcscan::word_offsets(WIDGETS_MOD_SRC, name).is_empty(),
            "`{name}` is back on the shared widget module's public surface; the \
             retirement is a removal from `widgets::mod.rs`, not a rename"
        );
    }

    let kit: Vec<String> = srcscan::top_level_items(CONFLICT_PANE_SRC)
        .into_iter()
        .filter(|item| item.name == "pane_header")
        .map(|item| item.name)
        .collect();
    assert!(
        kit.is_empty(),
        "the conflict kit declares its own `pane_header` again; it must delegate to \
         `widgets::pane_header` so the grammar's three columns wear the same header \
         as every tool pane (a second implementation is precisely what R7 forbids)"
    );
    assert!(
        !srcscan::word_offsets(CONFLICT_PANE_SRC, "widgets::pane_header").is_empty(),
        "the conflict kit must reach the shared pane header, not merely have deleted \
         its own"
    );
}

#[test]
fn one_band_height_and_one_title_treatment_own_every_pane_header() {
    assert_eq!(
        srcscan::items_naming(CONTAINERS_SRC, "PANE_HEADER_HEIGHT"),
        ["PANE_HEADER_HEIGHT", "pane_header_inset"],
        "the pane header's band height is declared once and read by the one function \
         that paints a band; a second reader is a second header"
    );
    assert_eq!(
        srcscan::fns_using(CONTAINERS_SRC, "PANE_HEADER_HEIGHT"),
        ["pane_header_inset"],
        "only the one pane header may size a band"
    );
    assert_eq!(
        srcscan::fns_using(CONTAINERS_SRC, "pane_title"),
        ["column_header", "pane_header_inset"],
        "a pane title and a column label are the same tracked mark: the one \
         `pane_title()` decides the face and the ink, exactly these two read it, \
         and a third reader is a fourth thing wearing the type"
    );
    let elsewhere: Vec<String> = srcscan::sources_under(ui_src_root())
        .iter()
        .filter(|(rel, _)| {
            !matches!(
                rel.as_str(),
                "widgets/containers.rs" | "widgets/mod.rs" | "widgets/controls.rs"
            )
        })
        .filter(|(_, src)| {
            !srcscan::word_offsets(src, "PANE_HEADER_HEIGHT").is_empty()
                || !srcscan::word_offsets(src, "pane_title").is_empty()
        })
        .map(|(rel, _)| rel.clone())
        .collect();
    assert!(
        elsewhere.is_empty(),
        "{elsewhere:?} name a pane header's private vocabulary; a pane asks for the \
         shared header rather than borrowing its numbers"
    );
    for (rel, src) in srcscan::sources_under(ui_src_root()) {
        if rel == "widgets/containers.rs" {
            continue;
        }
        assert_eq!(
            srcscan::fns_using(&src, "PANE_HEADER_HEIGHT"),
            Vec::<String>::new(),
            "`{rel}` reads the pane header's band height inside a function: the \
             band is one number owned by `widgets/containers`, and a control may \
             not size itself against it at paint time. A static assertion that the \
             two numbers agree is a different thing, and is what the compact \
             primary does."
        );
    }
    assert_eq!(
        srcscan::items_naming(WIDGETS_MOD_SRC, "PANE_HEADER_HEIGHT"),
        Vec::<String>::new(),
        "the façade re-exports the band height; a second mention would be a second \
         place deciding what a header is"
    );
    assert_eq!(
        turbogit_ui::theme::RAIL_WIDTH,
        2.0,
        "sanity: the token layer still owns the rail width, which the conflict \
         grammar's focus marker now reads"
    );
}

fn structural_rules<S>(harness: &Harness<'_, S>) -> Vec<egui::Rect> {
    painted_rects(harness)
        .into_iter()
        .filter(|(rect, fill, _, _)| {
            *fill == Palette::RULE_STRUCTURAL && (rect.height() - 1.0).abs() < 0.01
        })
        .map(|(rect, _, _, _)| rect)
        .collect()
}

fn the_header_rule<S>(harness: &Harness<'_, S>) -> egui::Rect {
    let mut rules = structural_rules(harness);
    rules.sort_by_key(|r| (r.top() * 100.0) as i64);
    *rules
        .first()
        .unwrap_or_else(|| panic!("a pane header must paint a structural hairline"))
}

fn the_pane_title_galley<S>(harness: &Harness<'_, S>) -> test_support::harness::PaintedGalley {
    painted_galleys(harness)
        .into_iter()
        .min_by(|a, b| {
            a.pos
                .x
                .partial_cmp(&b.pos.x)
                .expect("finite x")
                .then(a.pos.y.partial_cmp(&b.pos.y).expect("finite y"))
        })
        .unwrap_or_else(|| panic!("a pane header must paint a title"))
}

fn pane_header_harness(count: Option<&'static str>) -> (Harness<'static, ()>, Rc<Cell<f32>>) {
    let content_top = Rc::new(Cell::new(0.0));
    let content_top_ui = content_top.clone();
    let harness = widget_harness(egui::vec2(560.0, 200.0), move |ui| {
        content_top_ui.set(ui.available_rect_before_wrap().top());
        pane_header(ui, "PANES", count, |ui| {
            compact_button(ui, "Do the thing");
        });
        ui.label("pane body");
    });
    (harness, content_top)
}

#[test]
fn the_pane_header_paints_title_count_actions_one_hairline_then_content() {
    let (mut harness, content_top) = pane_header_harness(Some("12"));
    settle(&mut harness);

    let title = the_pane_title_galley(&harness);
    assert_eq!(title.text, "PANES");
    assert_eq!(painted_ink(&harness, "PANES"), Palette::INK_3);
    let font = painted_font(&harness, "PANES");
    assert_eq!(font.size, turbogit_ui::theme::TYPE_SECTION);
    assert_eq!(font.family, egui::FontFamily::Proportional);

    let (chip, radius) = the_chip_rect_with(&harness, Palette::RAISED, "the header's count chip");
    assert_eq!(
        radius,
        egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS)
    );
    assert_eq!(painted_ink(&harness, "12"), Palette::INK_2);

    let action = harness.get_by_label("Do the thing").rect();
    assert!(
        title.rect.right() < chip.left(),
        "the count chip sits beside the title, not over it: title {:?}, chip {chip:?}",
        title.rect
    );
    assert!(
        chip.right() < action.left(),
        "the action slot is the rightmost thing in the band: chip {chip:?}, action {action:?}"
    );
    assert!(
        (action.right() - title.rect.right()).abs() < 560.0,
        "the action slot must be reachable at all"
    );
    let rule = the_header_rule(&harness);
    assert!(
        (action.right() - rule.right()).abs() < 0.01,
        "the action slot is right-aligned to the header's own width, not floating: \
         action {:?}, rule {rule:?}",
        action
    );

    let rules = structural_rules(&harness);
    assert_eq!(
        rules.len(),
        1,
        "a pane header paints ONE hairline. A second rule between a header and its \
         content is the nested-boxes problem this migration exists to remove, and it \
         reappears quietly — a caller wanting air adds `ui.add_space`, not a rule. \
         Found {rules:?}"
    );
    assert!(
        (rule.left() - title.rect.left()).abs() < 0.01,
        "the hairline spans the header's own width, starting where the title starts: \
         rule {rule:?}, title {:?}",
        title.rect
    );
    assert!(
        (rule.top() - content_top.get() - PANE_HEADER_HEIGHT).abs() < 0.01,
        "the header band is `PANE_HEADER_HEIGHT` tall and the hairline is its last \
         pixel: got {} for a band of {}",
        rule.top() - content_top.get(),
        PANE_HEADER_HEIGHT
    );
    let body = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "pane body")
        .expect("the pane's content paints");
    assert!(
        body.pos.y > rule.bottom(),
        "content comes after the hairline, not over it: body {:?}, rule {rule:?}",
        body.pos
    );
}

#[test]
fn a_pane_header_without_a_count_is_the_same_header() {
    let (mut with_count, top_with) = pane_header_harness(Some("7"));
    settle(&mut with_count);
    let (mut without, top_without) = pane_header_harness(None);
    settle(&mut without);

    let a = the_header_rule(&with_count);
    let b = the_header_rule(&without);
    assert_eq!(
        (a.top() - top_with.get(), a.height()),
        (b.top() - top_without.get(), b.height()),
        "dropping the count chip must not resize the band"
    );
    assert_eq!(
        painted_galleys(&without)
            .into_iter()
            .find(|g| g.text == "7")
            .map(|g| g.text),
        None,
        "no count chip means no count painted"
    );
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ChromePane {
    Worktrees,
    Submodules,
    LogBranches,
    Changes,
}

impl ChromePane {
    fn title(self) -> &'static str {
        match self {
            Self::Worktrees => "WORKTREES",
            Self::Submodules => "SUBMODULES",
            Self::LogBranches => "BRANCHES",
            Self::Changes => "CHANGES",
        }
    }

    fn harness(
        self,
    ) -> (
        Harness<'static, turbogit_app::state::AppState>,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).expect("repo dir");
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "user.name", "Test"]);
        let repo = repo.canonicalize().expect("canonical repo");
        let state = turbogit_app::state::AppState::for_roots(dir.path(), &[repo]);
        let mut fonts_installed = false;
        let harness = Harness::builder()
            .with_size(egui::vec2(760.0, 620.0))
            .build_ui_state(
                move |ui, state| {
                    turbogit_ui::theme::configure_style(ui.ctx());
                    if !fonts_installed {
                        turbogit_ui::theme::install_fonts(ui.ctx());
                        fonts_installed = true;
                    }
                    state.drain_events();
                    egui::CentralPanel::default().show(ui, |ui| match self {
                        Self::Worktrees => turbogit_ui::ui::worktrees::show(ui, state),
                        Self::Submodules => turbogit_ui::ui::submodules::show(ui, state),
                        Self::LogBranches => turbogit_ui::ui::log_window::branches_pane(ui, state),
                        Self::Changes => turbogit_ui::ui::commit_window::show(ui, state),
                    });
                },
                state,
            );
        (harness, dir)
    }
}

#[derive(Debug)]
struct PaintedHeader {
    pane: ChromePane,
    rule: egui::Rect,
    title_offset: egui::Pos2,
    title_x: f32,
    title: String,
    size: f32,
    family: egui::FontFamily,
    ink: Color32,
}

fn measure_pane_header(pane: ChromePane) -> PaintedHeader {
    let (mut harness, _dir) = pane.harness();
    settle(&mut harness);
    let rule = the_header_rule(&harness);
    let galley = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == pane.title() && g.pos.x < rule.right())
        .unwrap_or_else(|| {
            panic!(
                "the {} pane must open with its own title `{}` inside its own \
                 header band (rule {rule:?})",
                format!("{pane:?}").to_lowercase(),
                pane.title(),
            )
        });
    let font = painted_font(&harness, pane.title());
    PaintedHeader {
        pane,
        rule,
        title_offset: egui::pos2(galley.pos.x - rule.left(), galley.pos.y - rule.top()),
        title_x: galley.pos.x,
        title: galley.text,
        size: font.size,
        family: font.family,
        ink: painted_ink(&harness, pane.title()),
    }
}

#[test]
fn every_named_pane_produces_identical_header_geometry() {
    const PANES_NAMED_HERE: [ChromePane; 4] = [
        ChromePane::Worktrees,
        ChromePane::Submodules,
        ChromePane::LogBranches,
        ChromePane::Changes,
    ];
    const FULL_BLEED: [ChromePane; 3] = [
        ChromePane::Worktrees,
        ChromePane::Submodules,
        ChromePane::LogBranches,
    ];

    let measured: Vec<PaintedHeader> = PANES_NAMED_HERE
        .iter()
        .copied()
        .map(measure_pane_header)
        .collect();

    let first = &measured[0];
    let titles: Vec<&str> = measured.iter().map(|h| h.title.as_str()).collect();
    assert_eq!(
        titles,
        ["WORKTREES", "SUBMODULES", "BRANCHES", "CHANGES"],
        "each named pane must have rendered its own title, or the comparison below \
         is four renders of one pane"
    );

    for header in &measured {
        assert!(
            (header.rule.height() - 1.0).abs() < 0.01,
            "the {:?} pane's header hairline is not one pixel: {:?}",
            header.pane,
            header.rule
        );
        assert!(
            (header.rule.left() - header.title_x).abs() < 0.01,
            "the {:?} pane's hairline must start where its title starts, so it is \
             the header's own rule spanning the header's own width: rule {:?}, \
             title x={}",
            header.pane,
            header.rule,
            header.title_x
        );
    }

    for other in &measured[1..] {
        assert_eq!(
            (other.title_offset.x, other.title_offset.y),
            (first.title_offset.x, first.title_offset.y),
            "the {:?} pane's title sits somewhere else inside its header: {:?} vs {:?}. \
             Two headers is not two spellings of one decision; it is a pane the user \
             has to re-read.",
            other.pane,
            other.title_offset,
            first.title_offset
        );
        assert_eq!(
            (other.size, other.family.clone(), other.ink),
            (first.size, first.family.clone(), first.ink),
            "the {:?} pane's title is set in a different type than the {:?} pane's: \
             {:?}/{:?}/{:?} vs {:?}/{:?}/{:?}",
            other.pane,
            first.pane,
            other.size,
            other.family,
            other.ink,
            first.size,
            first.family,
            first.ink
        );
    }

    for other in measured.iter().filter(|h| FULL_BLEED.contains(&h.pane)) {
        assert_eq!(
            (
                other.rule.left(),
                other.rule.top(),
                other.rule.width(),
                other.rule.height()
            ),
            (
                first.rule.left(),
                first.rule.top(),
                first.rule.width(),
                first.rule.height()
            ),
            "the {:?} pane's header is not the header the {:?} pane wears: {:?} vs {:?}. \
             Two headers is not two spellings of one decision; it is a pane the user \
             has to re-read.",
            other.pane,
            first.pane,
            other.rule,
            first.rule
        );
    }

    assert_eq!(first.size, turbogit_ui::theme::TYPE_SECTION);
    assert_eq!(first.ink, Palette::INK_3);
    assert!(
        (first.rule.height() - 1.0).abs() < 0.01,
        "the header's hairline is one pixel"
    );
    assert!(
        first.rule.top() > first.title_offset.y,
        "the title is inside the band, above its hairline"
    );
}

#[test]
fn every_pane_title_renders_in_the_same_case() {
    for pane in [
        ChromePane::Worktrees,
        ChromePane::Submodules,
        ChromePane::LogBranches,
    ] {
        let (mut harness, _dir) = pane.harness();
        settle(&mut harness);
        let title = the_pane_title_galley(&harness).text;
        assert_eq!(
            title,
            title.to_uppercase(),
            "the {pane:?} pane opens with `{title}`: a pane title is the app's one \
             case, upper, and a pane that spells its own title differently is a band \
             the eye has to re-read"
        );
        assert_eq!(title, pane.title());
        let painted: Vec<String> = painted_text(&harness);
        for lower in ["Worktrees", "Submodules", "Branches", "Blame"] {
            assert!(
                !painted.contains(&lower.to_owned()),
                "the {pane:?} pane still paints the title-case `{lower}`: {painted:?}"
            );
        }
    }
}

const TEST_COLUMNS: [PaneColumn; 4] = [
    PaneColumn::start("HASH", 26.0),
    PaneColumn::start("AUTHOR", 84.0),
    PaneColumn::start("MESSAGE", 164.0),
    PaneColumn::end("DATE", 0.0),
];

fn column_chrome_harness() -> Harness<'static, ()> {
    widget_harness(egui::vec2(700.0, 160.0), move |ui| {
        let row = ui.available_rect_before_wrap();
        column_header(ui, row, &TEST_COLUMNS);
        ui.horizontal(|ui| {
            for (i, cell) in ["a1b2c3d", "Ada", "subject"].into_iter().enumerate() {
                column_cell(ui, &TEST_COLUMNS, i, |ui| {
                    ui.label(cell);
                });
            }
            let galley = ui.painter().layout_no_wrap(
                "2026-01-01".to_owned(),
                egui::FontId::proportional(12.0),
                Palette::INK,
            );
            ui.painter().galley(
                egui::pos2(
                    TEST_COLUMNS[3].origin(row) - galley.size().x,
                    ui.cursor().top(),
                ),
                galley,
                Palette::INK,
            );
        });
    })
}

#[test]
fn the_column_header_and_its_rows_measure_from_the_same_offsets() {
    let mut harness = column_chrome_harness();
    settle(&mut harness);

    let pairs = [
        ("HASH", "a1b2c3d"),
        ("AUTHOR", "Ada"),
        ("MESSAGE", "subject"),
    ];
    for (label, cell) in pairs {
        let label_x = galley_x(&harness, label);
        let cell_x = galley_x(&harness, cell);
        assert!(
            (label_x - cell_x).abs() < 0.01,
            "the `{label}` header label is not over its cell: label at {label_x}, cell \
             at {cell_x}. The header and the rows must read the same column table."
        );
    }
    let date_label = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "DATE")
        .expect("the trailing column is labelled");
    let date_cell = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "2026-01-01")
        .expect("the trailing cell paints");
    assert!(
        (date_label.rect.right() - date_cell.rect.right()).abs() < 0.01,
        "a trailing column's label ends where its cell ends: {:?} vs {:?}",
        date_label.rect,
        date_cell.rect
    );
}

#[test]
fn a_column_header_is_ink_3_and_never_the_dim_ink() {
    let mut harness = column_chrome_harness();
    settle(&mut harness);

    assert_eq!(
        painted_ink(&harness, "HASH"),
        Palette::INK_3,
        "a column header is a label the user reads to know what a bare number or \
         timestamp means"
    );
    assert_eq!(
        painted_ink(&harness, "HASH"),
        Palette::T_MUTED,
        "`INK_3` and `T_MUTED` are one step under two names, and the token layer is \
         where the value lives"
    );
    assert_ne!(
        painted_ink(&harness, "HASH"),
        Palette::INK_4,
        "9px is normal-size text. `INK_4` is 3.2:1 and is reserved for placeholders, \
         dim path suffixes and hatches — never the only rendering of something the \
         user needs. The target frames were drawn with the dim value before this was \
         checked, which is exactly why this assertion exists."
    );
    assert!(
        Palette::INK_3 != Palette::INK_4,
        "if the muted and dim steps were ever merged, this ratchet would stop \
         meaning anything"
    );

    let font = painted_font(&harness, "HASH");
    assert_eq!(font.size, turbogit_ui::theme::TYPE_SECTION);
    assert_eq!(font.family, egui::FontFamily::Proportional);
    let (mut pane, _top) = pane_header_harness(None);
    settle(&mut pane);
    let pane_font = painted_font(&pane, "PANES");
    assert_eq!(
        (pane_font.size, pane_font.family),
        (font.size, font.family),
        "a pane title and a column label are the same tracked mark"
    );
    assert_eq!(painted_ink(&pane, "PANES"), painted_ink(&harness, "HASH"));
}

#[test]
fn the_column_header_row_underlines_itself_once_in_the_structural_tone() {
    let table_width = Rc::new(Cell::new(0.0));
    let table_width_ui = table_width.clone();
    let mut harness = widget_harness(egui::vec2(700.0, 160.0), move |ui| {
        let row = ui.available_rect_before_wrap();
        table_width_ui.set(row.width());
        column_header(ui, row, &TEST_COLUMNS);
        ui.label("the first data row");
    });
    settle(&mut harness);

    let rules = structural_rules(&harness);
    assert_eq!(
        rules.len(),
        1,
        "the column-header row underlines itself once: {rules:?}"
    );
    let band_top = rules[0].top() - COLUMN_HEADER_HEIGHT;
    for column in TEST_COLUMNS {
        let label = painted_galleys(&harness)
            .into_iter()
            .find(|g| g.text == column.label)
            .unwrap_or_else(|| panic!("`{}` paints", column.label));
        let expected = band_top + COLUMN_HEADER_HEIGHT / 2.0;
        assert!(
            (label.rect.center().y - expected).abs() <= 1.0,
            "a column label is centred in the band above the underline: \
             `{}` is centred at {} but the band's centre is {expected} \
             (rule {:?}, label {:?})",
            column.label,
            label.rect.center().y,
            rules[0],
            label.rect,
        );
        assert!(
            rules[0].top() >= label.rect.bottom(),
            "the column header's underline is UNDER its labels, not through them: \
             rule {:?}, `{}` label {:?}",
            rules[0],
            column.label,
            label.rect,
        );
    }
    assert!(
        (rules[0].width() - table_width.get()).abs() < 0.01,
        "the underline spans the table's width, got {} for a table of {}",
        rules[0].width(),
        table_width.get()
    );
    let body = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "the first data row")
        .expect("the row paints");
    assert!(
        body.pos.y > rules[0].bottom(),
        "the first data row is below the header's underline: {:?} vs {:?}",
        body.pos,
        rules[0]
    );
}

const PANE_PRIMARY_ACTION: [(ChromePane, Option<&str>); 4] = [
    (ChromePane::Worktrees, Some("Add worktree")),
    (ChromePane::Submodules, None),
    (ChromePane::LogBranches, None),
    (ChromePane::Changes, None),
];

#[test]
fn a_tool_pane_paints_at_most_one_brand_fill_and_it_is_its_primary_action() {
    for (pane, expected_label) in PANE_PRIMARY_ACTION {
        let (mut harness, _dir) = pane.harness();
        settle(&mut harness);
        let rule = the_header_rule(&harness);

        let brand: Vec<egui::Rect> = painted_rects(&harness)
            .into_iter()
            .filter(|(_, fill, _, _)| *fill == Palette::BRAND)
            .map(|(rect, _, _, _)| rect)
            .collect();
        let (rails, rest): (Vec<egui::Rect>, Vec<egui::Rect>) = brand
            .iter()
            .copied()
            .partition(|rect| (rect.width() - RAIL_WIDTH).abs() < 0.01);
        for rail in &rails {
            assert!(
                rail.height() <= 40.0,
                "a brand-filled rect {rail:?} is a rail's width but is {}-tall: the \
                 accent fills a row's leading edge, and nothing else at that width",
                rail.height()
            );
        }
        let (tab_rules, buttons): (Vec<egui::Rect>, Vec<egui::Rect>) = rest
            .iter()
            .copied()
            .partition(|rect| (rect.height() - RAIL_WIDTH).abs() < 0.01);
        assert!(
            tab_rules.len() <= 1,
            "one active tab paints one underline, so a pane carries at most one: \
             {tab_rules:?}"
        );
        for rule in &tab_rules {
            let band_top = rule.bottom() - TAB_STRIP_HEIGHT;
            let spans: Vec<_> = painted_galleys(&harness)
                .into_iter()
                .filter(|g| {
                    g.rect.center().y >= band_top
                        && g.rect.center().y <= rule.bottom()
                        && g.rect.center().x >= rule.left()
                        && g.rect.center().x <= rule.right()
                })
                .collect();
            assert_eq!(
                spans.len(),
                1,
                "a brand rule {rule:?} reads as a tab underline only over the one \
                 tab label it closes; found {spans:?} in its band"
            );
        }

        assert_eq!(
            buttons.len(),
            usize::from(expected_label.is_some()),
            "the {pane:?} pane painted {} brand-filled control(s) at {buttons:?} \
             (plus {} rail(s)). R1 gives the accent three jobs and a primary \
             action button is the only one a tool pane may spend it on: a second \
             blue here is the blue soup this migration removed.",
            buttons.len(),
            rails.len()
        );

        let Some(label) = expected_label else {
            continue;
        };

        let primary = harness.get_by_label(label).rect();
        assert_eq!(
            buttons,
            vec![primary],
            "the {pane:?} pane's one brand fill is its `{label}` action, painted \
             at the button's own rect: {buttons:?} vs {primary:?}"
        );
        assert!(
            primary.bottom() <= rule.top() + 0.01,
            "the {pane:?} pane's primary action belongs in the header's action \
             slot, above the hairline: {primary:?} vs the band rule {rule:?}"
        );
    }
}

const STROKE_SITES: [(&str, usize, &str); 7] = [
    (
        "widgets/containers.rs",
        2,
        "the one bordered card (a card inside an `egui::Area` — the only caller \
         is the Welcome changelog overlay) and `note`'s severity ring, which \
         outlines an *inset* in warning ink rather than a region's edge",
    ),
    ("components.rs", 1, "the kit button's own focus ring"),
    (
        "widgets/controls.rs",
        2,
        "the shared `focus_ring` helper every custom-drawn control reaches for, \
         and the vocabulary button's own",
    ),
    (
        "widgets/inputs.rs",
        2,
        "the text field's resting edge (a control's own border) and its focus \
         ring",
    ),
    (
        "commit_window.rs",
        3,
        "the tri-state partial checkbox's border, the message editor's focus \
         ring, and the commit/rollback divider rule",
    ),
    ("sidebar.rs", 1, "the tri-state checkbox's own border"),
    (
        "kit/conflict_pane.rs",
        1,
        "the 2px `BRAND` ring around a focused conflict header or result cell: a \
         focus marker, and the geometry difference between a ring and a rail is \
         the whole reason it is not routed through the rail painter",
    ),
];

#[test]
fn every_stroke_in_the_ui_layer_is_a_floating_surface_a_focus_ring_or_a_rule() {
    let mut found: Vec<(String, usize)> = Vec::new();
    for (rel, src) in srcscan::sources_under(ui_src_root()) {
        let mut statement = String::new();
        let mut count = 0usize;
        for line in src.lines() {
            statement.push_str(line);
            statement.push('\n');
            if !line.trim_end().ends_with(';') && !line.trim_end().ends_with('}') {
                continue;
            }
            if statement.contains("rect_stroke(") || statement.contains(".stroke(") {
                count += 1;
            }
            statement.clear();
        }
        if count > 0 {
            found.push((rel, count));
        }
    }
    found.sort();

    let mut expected: Vec<(String, usize)> = STROKE_SITES
        .iter()
        .map(|(file, count, _)| ((*file).to_owned(), *count))
        .collect();
    expected.sort();
    assert_eq!(
        found, expected,
        "the set of stroke sites in `src/ui` is not the one this table \
         classifies. A stroke means one of three things — this floats, this \
         control has focus, or this is a rule — and never 'this is a content \
         region'. Add the site with its category, or remove the stroke."
    );
    assert_eq!(
        found, expected,
        "the set of stroke sites in `src/ui` is not the one this table \
         classifies. A stroke means one of three things — this floats, this \
         control has focus, or this is a rule — and never 'this is a content \
         region'. Add the site with its category, or remove the stroke."
    );

    for (file, count, why) in STROKE_SITES {
        assert!(
            !file.is_empty() && count > 0 && !why.trim().is_empty(),
            "every stroke site is recorded with the file, how many sites it \
             accounts for, and why none of them is a content region's edge"
        );
    }

    let containers = srcscan::sources_under(ui_src_root())
        .into_iter()
        .find(|(rel, _)| rel == "widgets/containers.rs")
        .map(|(_, src)| src)
        .expect("the container module is under src/ui");
    assert_eq!(
        srcscan::items_naming(&containers, "BORDER_HAIRLINE_WIDTH"),
        vec!["BORDER_HAIRLINE_WIDTH".to_owned(), "card".to_owned()],
        "the card's hairline is one opt-in read inside the one card function: a \
         second reader is a second surface deciding to box itself"
    );
    assert!(
        !srcscan::items_naming(&containers, "RULE_CONTENT")
            .iter()
            .any(|name| name == "card"),
        "a card separates its regions with surface tone and at most one hairline, \
         so it does not stroke itself with the content-divider role"
    );
}

#[test]
fn the_ref_chip_is_produced_by_one_function_in_one_file_and_no_screen_paints_one() {
    const NAMED_IN: [(&str, &[&str]); 2] = [
        (
            "REF_CHIP_COLORS",
            &["widgets/chips.rs", "components.rs", "widgets/mod.rs"],
        ),
        (
            "compact_chip_radius",
            &["widgets/chips.rs", "components.rs", "widgets/mod.rs"],
        ),
    ];

    const SCREEN_GEOMETRY_USE: [(&str, &str); 2] = [
        (
            "sidebar.rs",
            "hand-places the **count** chip into caller-allocated rects (the \
             shared `count_chip` allocates its own slot in a flow, which a \
             measured-offset rail row cannot use)",
        ),
        (
            "branch_tree_view.rs",
            "reserves the chip's inset to line a row's columns up, and names no \
             chip colour at all — the pair it paints with comes from \
             `components::ref_chip_with_ink`",
        ),
    ];

    let files: Vec<(String, String)> = srcscan::sources_under(ui_src_root())
        .into_iter()
        .map(|(rel, src)| (rel, srcscan::code_only(&src)))
        .collect();
    let chips = files
        .iter()
        .find(|(rel, _)| rel == "widgets/chips.rs")
        .map(|(_, src)| src.as_str())
        .expect("the chip module is under src/ui");

    for (value, allowed) in NAMED_IN {
        assert!(
            chips.contains(value),
            "the chip module must still name `{value}`: the whole-tree ratchet \
             below is only a claim about the app while the chip module really \
             does define the role"
        );
        let mut naming: Vec<String> = files
            .iter()
            .filter(|(_, src)| src.contains(value))
            .map(|(rel, _)| rel.clone())
            .collect();
        naming.sort();
        let mut expected: Vec<String> = allowed.iter().map(|a| (*a).to_owned()).collect();
        expected.sort();
        assert_eq!(
            naming, expected,
            "`{value}` is named by a module that is not one of {expected:?}. A \
             second module that builds a ref chip — or a second function inside a \
             screen that paints one with a hand-spelled fill — is the \
             duplication the closed chip vocabulary exists to prevent, and \
             `exactly_one_function_produces_each_of_the_three_chips` cannot see \
             it because it only reads `widgets/chips.rs`."
        );
    }

    assert_eq!(
        turbogit_ui::ui::components::ref_chip_with_ink_fill(),
        Palette::RAISED_ON_CARD,
        "the branch row's ref chip keeps the one ref-chip fill; a caller-chosen \
         *ink* is allowed, a caller-chosen fill is a fourth chip"
    );

    let mut geometry_in_screens: Vec<&str> = files
        .iter()
        .filter(|(rel, src)| {
            src.contains("COMPACT_CHIP_GEOMETRY")
                && !matches!(
                    rel.as_str(),
                    "widgets/chips.rs" | "components.rs" | "widgets/mod.rs"
                )
        })
        .map(|(rel, _)| rel.as_str())
        .collect();
    geometry_in_screens.sort();
    let mut expected_screens: Vec<&str> = SCREEN_GEOMETRY_USE.iter().map(|(f, _)| *f).collect();
    expected_screens.sort();
    assert_eq!(
        geometry_in_screens, expected_screens,
        "the set of screen modules that name the compact chip geometry is not the \
         one this table explains. The shape is the chips' shared shape role, so a \
         screen may use it for a chip of its own — but it has to say which chip, \
         and the only one a screen may build by hand is the count chip."
    );
    for (rel, why) in SCREEN_GEOMETRY_USE {
        let src = &files
            .iter()
            .find(|(name, _)| name == rel)
            .expect("a named screen module")
            .1;
        assert!(
            !why.trim().is_empty(),
            "every screen that names the chip geometry records why it is not a \
             fourth chip"
        );
        assert!(
            !src.contains("REF_CHIP_COLORS"),
            "`{rel}` names the compact chip geometry and must not name the ref \
             chip's colours: the ref chip is produced by exactly one function, and \
             a screen that paints the fill itself is that function's second copy"
        );
        if src.contains("COUNT_CHIP_COLORS") {
            assert!(
                !why.contains("names no chip colour"),
                "`{rel}` places the count chip by hand, so it names that chip's \
                 pair — the reason recorded above has to say so"
            );
        }
    }

    for (rel, src) in &files {
        if rel == "widgets/chips.rs" || rel == "widgets/mod.rs" {
            continue;
        }
        for declared in ["fn ref_chip(", "fn ref_chip_with_ink("] {
            assert!(
                srcscan::word_offsets(src, declared).is_empty(),
                "`{rel}` declares `{declared}`: exactly one function produces a \
                 ref chip, and a second declaration of the name — however it \
                 colours itself — is a second producer, not a second caller."
            );
        }
    }
}

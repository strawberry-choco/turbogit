use turbogit_domain::model::{ChangeRef, ProjectState, RootId};

const LEGACY_HIGH_CONTRAST: &str = r#"
(
    mappings: [],
    settings: (
        git_executable: "",
        staging_area: false,
        synchronous_branches: false,
        update_method: Rebase,
        clean_tree_method: Stash,
        incoming_check: Auto,
        protected_branch_patterns: ["main"],
        warn_crlf: true,
        warn_detached: true,
        commit_template: "",
        restore_workspace: false,
        gutter_markers: true,
        date_format: Iso,
        no_commit_hooks: false,
        theme: HighContrast,
    ),
)
"#;

#[test]
fn legacy_state_with_removed_theme_mode_still_loads() {
    let legacy_light = LEGACY_HIGH_CONTRAST.replace("HighContrast", "Light");

    let hc: ProjectState =
        ron::from_str(LEGACY_HIGH_CONTRAST).expect("legacy HighContrast state must load");
    let light: ProjectState = ron::from_str(&legacy_light).expect("legacy Light state must load");

    assert_eq!(hc.settings, light.settings);
}

#[test]
fn p0_shell_defaults_use_the_branch_type_ramp() {
    use egui::TextStyle;
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let style = ctx.style_of(egui::Theme::Dark);
    for (role, font) in [
        (TextStyle::Body, theme::chrome_font(theme::TYPE_BODY)),
        (TextStyle::Button, theme::chrome_font(theme::TYPE_CONTROL)),
        (TextStyle::Small, theme::chrome_font(theme::TYPE_CONTROL)),
        (
            TextStyle::Heading,
            theme::chrome_font(theme::TYPE_DETAIL_TITLE),
        ),
        (TextStyle::Monospace, theme::data_font(theme::TYPE_BODY)),
    ] {
        assert_eq!(style.text_styles[&role], font, "{role:?}");
    }
}

const AUDITED_SURFACES: [(&str, Color32); 8] = [
    ("app background (BG)", Palette::BG),
    ("content surface (CONTENT_BG)", Palette::CONTENT_BG),
    ("sidebar (SIDEBAR)", Palette::SIDEBAR),
    ("raised surface (SURFACE)", Palette::SURFACE),
    ("hover / raised-on-card (SURFACE_2)", Palette::SURFACE_2),
    ("highest raised surface (SURFACE_3)", Palette::SURFACE_3),
    ("selection token (SELECTION)", Palette::SELECTION),
    ("selected list row (ROW_SELECTED)", Palette::ROW_SELECTED),
];

#[derive(Clone, Copy)]
struct InkStep {
    name: &'static str,
    ink: Color32,
    legal: &'static [&'static str],
}

const ALL_AUDITED: &[&str] = &[
    "app background (BG)",
    "content surface (CONTENT_BG)",
    "sidebar (SIDEBAR)",
    "raised surface (SURFACE)",
    "hover / raised-on-card (SURFACE_2)",
    "highest raised surface (SURFACE_3)",
    "selection token (SELECTION)",
    "selected list row (ROW_SELECTED)",
];

const MUTED_LEGAL: &[&str] = &[
    "app background (BG)",
    "content surface (CONTENT_BG)",
    "sidebar (SIDEBAR)",
];

const MUTED_LEGAL_NAMED: [&str; 3] = [
    "app background (BG)",
    "content surface (CONTENT_BG)",
    "sidebar (SIDEBAR)",
];

const INK_RAMP: [InkStep; 4] = [
    InkStep {
        name: "INK",
        ink: Palette::INK,
        legal: ALL_AUDITED,
    },
    InkStep {
        name: "INK_2",
        ink: Palette::INK_2,
        legal: ALL_AUDITED,
    },
    InkStep {
        name: "INK_3",
        ink: Palette::INK_3,
        legal: MUTED_LEGAL,
    },
    InkStep {
        name: "INK_4",
        ink: Palette::INK_4,
        legal: &[],
    },
];

#[test]
fn the_ink_sweep_is_driven_by_each_steps_written_down_legality_set() {
    assert_eq!(Palette::INK, Palette::T_PRIMARY, "INK aliases T_PRIMARY");
    assert_eq!(
        Palette::INK_2,
        Palette::T_SECONDARY,
        "INK_2 aliases T_SECONDARY"
    );
    assert_eq!(Palette::INK_3, Palette::T_MUTED, "INK_3 aliases T_MUTED");
    assert_eq!(Palette::INK_4, Palette::T_DIM, "INK_4 aliases T_DIM");

    let mut steps: Vec<InkStep> = vec![InkStep {
        name: "ACCENT_TEXT",
        ink: Palette::ACCENT_TEXT,
        legal: ALL_AUDITED,
    }];
    steps.extend(INK_RAMP.iter().copied());
    assert_eq!(INK_RAMP.len(), 4, "the ramp has four steps");

    for step in &steps {
        for (surface_name, surface) in AUDITED_SURFACES {
            let ratio = contrast(step.ink, surface);
            let legal = step.legal.contains(&surface_name);
            println!(
                "{:12} on {:34} {ratio:6.3}:1  {}",
                step.name,
                surface_name,
                if legal { "legal" } else { "NOT legal" }
            );
            if legal {
                assert!(
                    ratio >= 4.5,
                    "{} is legal on {surface_name} and must clear AA: {ratio:.3}:1",
                    step.name
                );
            } else {
                assert!(
                    ratio < 4.5,
                    "{} is not legal on {surface_name}, so it must actually be \
                     sub-AA there — it measures {ratio:.3}:1. Either add the \
                     surface to the step's legality set on purpose, or the \
                     value moved.",
                    step.name
                );
            }
        }
        for declared in step.legal {
            assert!(
                AUDITED_SURFACES.iter().any(|(n, _)| n == declared),
                "{} declares an un-audited surface `{declared}`",
                step.name
            );
        }
    }
}

#[test]
fn ink_3_clears_aa_on_the_app_content_and_sidebar_surfaces() {
    for (surface_name, surface) in [
        ("app background", Palette::BG),
        ("content surface", Palette::CONTENT_BG),
        ("sidebar", Palette::SIDEBAR),
    ] {
        let ratio = contrast(Palette::INK_3, surface);
        println!("INK_3 on {surface_name}: {ratio:.3}:1");
        assert!(
            ratio >= 4.5,
            "INK_3 on {surface_name} must clear AA: {ratio:.3}:1"
        );
    }
    for (surface_name, surface) in [
        ("raised surface", Palette::SURFACE),
        ("hover / raised-on-card", Palette::SURFACE_2),
        ("highest raised surface", Palette::SURFACE_3),
        ("selection token", Palette::SELECTION),
        ("selected list row", Palette::ROW_SELECTED),
    ] {
        let ratio = contrast(Palette::INK_3, surface);
        println!("INK_3 on {surface_name}: {ratio:.3}:1 (not legal)");
        assert!(
            ratio < 4.5,
            "INK_3 is not legal on {surface_name} and must be measurably \
             sub-AA there, not borderline: {ratio:.3}:1"
        );
    }
}

#[test]
fn ink_2_clears_every_surface_the_muted_step_gave_up() {
    for (surface_name, surface) in AUDITED_SURFACES {
        let ratio = contrast(Palette::INK_2, surface);
        println!("INK_2 on {surface_name}: {ratio:.3}:1");
        assert!(
            ratio >= 4.5,
            "the muted step's caller steps up to INK_2 on {surface_name}, \
             which must clear AA: {ratio:.3}:1"
        );
    }
}

#[test]
fn every_adjacent_ink_step_is_separated_by_at_least_1_4_to_1_on_the_content_surface() {
    let surface = Palette::CONTENT_BG;
    let pairs: Vec<(InkStep, InkStep)> = INK_RAMP.windows(2).map(|w| (w[0], w[1])).collect();
    assert_eq!(pairs.len(), 3, "four steps give three adjacent pairs");
    for (brighter, dimmer) in pairs {
        let brighter_ratio = contrast(brighter.ink, surface);
        let dimmer_ratio = contrast(dimmer.ink, surface);
        let separation = brighter_ratio / dimmer_ratio;
        println!(
            "{} ({brighter_ratio:.3}:1) vs {} ({dimmer_ratio:.3}:1) \
             on the content surface: {separation:.3}:1",
            brighter.name, dimmer.name
        );
        assert_ne!(
            brighter.ink, dimmer.ink,
            "{} and {} are the same colour wearing two names",
            brighter.name, dimmer.name
        );
        assert!(
            brighter_ratio > dimmer_ratio,
            "{} must read stronger than {} on the content surface \
             ({brighter_ratio:.3}:1 vs {dimmer_ratio:.3}:1)",
            brighter.name,
            dimmer.name
        );
        assert!(
            separation >= 1.4,
            "{} → {} is only {separation:.3}:1 on the content surface; the \
             ramp needs 1.4:1 for the step to exist",
            brighter.name,
            dimmer.name
        );
    }
}
#[test]
fn ink_4_is_the_only_ink_that_is_sub_aa_everywhere_and_ink_3_the_only_conditional_one() {
    let below_on_content: Vec<&str> = INK_RAMP
        .iter()
        .filter(|s| contrast(s.ink, Palette::CONTENT_BG) < 4.5)
        .map(|s| s.name)
        .collect();
    assert_eq!(
        below_on_content,
        ["INK_4"],
        "INK_4 must be the only ink below 4.5:1 on the content surface"
    );

    let sub_aa_everywhere: Vec<&str> = INK_RAMP
        .iter()
        .filter(|s| {
            AUDITED_SURFACES
                .iter()
                .all(|(_, surface)| contrast(s.ink, *surface) < 4.5)
        })
        .map(|s| s.name)
        .collect();
    assert_eq!(
        sub_aa_everywhere,
        ["INK_4"],
        "INK_4 must be the only ink that is sub-AA on every audited surface"
    );

    let conditional: Vec<&str> = INK_RAMP
        .iter()
        .filter(|s| {
            let ratios: Vec<f64> = AUDITED_SURFACES
                .iter()
                .map(|(_, surface)| contrast(s.ink, *surface))
                .collect();
            let sub_aa = ratios.iter().filter(|r| **r < 4.5).count();
            sub_aa > 0 && sub_aa < ratios.len()
        })
        .map(|s| s.name)
        .collect();
    assert_eq!(
        conditional,
        ["INK_3"],
        "INK_3 must be the only ink that is sub-AA on some audited surfaces \
         and clear on others"
    );

    assert!(
        INK_RAMP[3].legal.is_empty(),
        "INK_4 has no legal surface in the palette"
    );
    for named in MUTED_LEGAL_NAMED {
        assert!(
            INK_RAMP[2].legal.contains(&named),
            "INK_3 must stay legal on {named}"
        );
    }
    for (surface_name, _) in RAISED_OR_SELECTED {
        assert!(
            !INK_RAMP[2].legal.contains(&surface_name),
            "{surface_name} is a raised/selected surface: INK_3 must not be \
             declared legal there"
        );
    }
    assert_eq!(INK_RAMP[3].name, "INK_4");
    assert!(
        INK_RAMP[3].ink != INK_RAMP[2].ink && INK_RAMP[3].ink != INK_RAMP[1].ink,
        "the dim step must stay its own value, one clear step below the muted one"
    );
}

#[test]
fn the_added_accent_clears_aa_on_the_content_surface_at_its_new_value() {
    let ratio = contrast(Palette::STATUS_ADDED, Palette::CONTENT_BG);
    println!("added accent on the content surface: {ratio:.3}:1");
    assert!(
        ratio >= 4.5,
        "the added accent must clear AA on the content surface: {ratio:.3}:1"
    );
    assert!(
        ratio >= 5.0,
        "the added accent should clear AA with room to spare, not scrape it: \
         {ratio:.3}:1"
    );

    assert_eq!(
        Palette::DIFF_ADD_ACCENT,
        Palette::STATUS_ADDED,
        "the added-file letter and the added line's accent are ONE token"
    );
    assert_eq!(Palette::STATUS_ADDED, Color32::from_rgb(0x6f, 0xae, 0x75));

    assert_eq!(Palette::AHEAD, Color32::from_rgb(0x5f, 0xa8, 0x6c));
    let ahead = contrast(Palette::AHEAD, Palette::CONTENT_BG);
    println!("AHEAD on the content surface: {ahead:.3}:1 (unchanged)");
    assert!(
        ahead >= 4.5,
        "AHEAD is unchanged and must still clear AA where it is used: {ahead:.3}:1"
    );
    assert_eq!(Palette::ACCENT_TEXT, Color32::from_rgb(0x8b, 0xb5, 0xf5));
    for (surface_name, surface) in AUDITED_SURFACES {
        let ratio = contrast(Palette::ACCENT_TEXT, surface);
        assert!(ratio >= 4.5, "ACCENT_TEXT on {surface_name}: {ratio:.3}:1");
    }
}

#[test]
fn the_diff_vocabulary_is_untouched_except_the_one_token_that_is_also_the_added_file_accent() {
    assert_eq!(Palette::DIFF_ADD_BG, Color32::from_rgb(0x34, 0x4f, 0x3e));
    assert_eq!(Palette::DIFF_ADD_TEXT, Color32::from_rgb(0x85, 0xe8, 0x9d));
    assert_eq!(Palette::DIFF_DEL_BG, Color32::from_rgb(0x5a, 0x3a, 0x3a));
    assert_eq!(Palette::DIFF_DEL_TEXT, Color32::from_rgb(0xff, 0x9a, 0x9a));
    assert_eq!(
        Palette::DIFF_DEL_ACCENT,
        Color32::from_rgb(0xf7, 0x54, 0x64)
    );
    assert_ne!(Palette::DIFF_ADD_ACCENT, Palette::DIFF_ADD_BG);
    assert_ne!(Palette::DIFF_DEL_ACCENT, Palette::DIFF_DEL_BG);
    assert_eq!(
        Palette::selection_bg(),
        Color32::from_rgba_premultiplied(0x0d, 0x1d, 0x3c, 0x40)
    );

    assert_eq!(Palette::DIFF_ADD_ACCENT, Palette::STATUS_ADDED);
    assert_ne!(
        Palette::DIFF_ADD_ACCENT,
        Color32::from_rgb(0x57, 0x96, 0x5c)
    );

    let accent_on_add_bg = contrast(Palette::DIFF_ADD_ACCENT, Palette::DIFF_ADD_BG);
    println!("added accent on the added-line background: {accent_on_add_bg:.3}:1");
    assert!(
        accent_on_add_bg >= 3.0,
        "the moved accent must still read as a graphical object on the \
         added-line background: {accent_on_add_bg:.3}:1"
    );
    assert_ne!(Palette::DIFF_ADD_TEXT, Palette::DIFF_ADD_ACCENT);
}

const RAISED_LADDER: [(&str, Color32, Color32); 2] = [
    ("app background (BG)", Palette::BG, Palette::RAISED),
    (
        "content-surface card (CONTENT_BG)",
        Palette::CONTENT_BG,
        Palette::RAISED_ON_CARD,
    ),
];

#[test]
fn the_selected_row_fill_is_one_opaque_list_row_value_and_neither_of_the_other_two_selections() {
    assert_eq!(Palette::ROW_SELECTED, Color32::from_rgb(0x24, 0x34, 0x56));
    assert!(
        Palette::ROW_SELECTED.is_opaque(),
        "a list row's fill is one value whatever it sits on — not a composite"
    );

    assert_ne!(
        Palette::ROW_SELECTED,
        Palette::SELECTION,
        "the current-ref band is not a row fill: no list row resolves to SELECTION"
    );
    assert_ne!(
        Palette::ROW_SELECTED,
        Palette::selection_bg(),
        "the opaque list-row fill is not the translucent focus composite"
    );
    assert_ne!(
        Palette::ROW_SELECTED,
        Palette::BRAND,
        "a solid brand band behind running text is what this token replaces"
    );

    assert_eq!(
        Palette::selection_bg(),
        Color32::from_rgba_premultiplied(0x0d, 0x1d, 0x3c, 0x40)
    );

    let step = contrast(Palette::ROW_SELECTED, Palette::CONTENT_BG);
    println!("selected-row fill on the content surface: {step:.3}:1");
    assert!(
        (1.2..1.3).contains(&step),
        "the selected-row fill must read as a step above the content surface \
         without becoming a saturated fill: {step:.3}:1"
    );

    for (ink_name, ink) in [
        ("INK", Palette::INK),
        ("INK_2", Palette::INK_2),
        ("ACCENT_TEXT", Palette::ACCENT_TEXT),
    ] {
        let ratio = contrast(ink, Palette::ROW_SELECTED);
        println!("{ink_name} on the selected list row: {ratio:.3}:1");
        assert!(
            ratio >= 4.5,
            "{ink_name} on a selected list row must clear AA: {ratio:.3}:1"
        );
    }
    let muted = contrast(Palette::INK_3, Palette::ROW_SELECTED);
    println!("INK_3 on the selected list row: {muted:.3}:1 (not legal)");
    assert!(
        muted < 4.5,
        "INK_3 is not legal on a selected list row and must be measurably \
         sub-AA there, not borderline: {muted:.3}:1"
    );
    assert!(
        is_raised_or_selected(Palette::ROW_SELECTED),
        "a selected list row is a *selected* surface for the ink rule"
    );

    let accent_on_content = contrast(Palette::BRAND, Palette::CONTENT_BG);
    println!("accent on the content surface: {accent_on_content:.3}:1");
    assert!(
        accent_on_content < 4.5,
        "the accent may not be a fill behind running text: {accent_on_content:.3}:1"
    );
    assert!(
        accent_on_content >= 3.0,
        "…and it is above the 3:1 a non-text graphical object needs, which is \
         why a 2px rail is allowed where the fill is not: {accent_on_content:.3}:1"
    );
}

#[test]
fn the_raised_ladder_is_strictly_ordered_and_each_rung_steps_up_from_its_own_host() {
    let ladder = [
        ("BG", Palette::BG),
        ("CONTENT_BG", Palette::CONTENT_BG),
        ("SURFACE (RAISED)", Palette::RAISED),
        ("SURFACE_2 (RAISED_ON_CARD)", Palette::RAISED_ON_CARD),
        ("SURFACE_3", Palette::SURFACE_3),
    ];
    for pair in ladder.windows(2) {
        let (lower_name, lower) = pair[0];
        let (upper_name, upper) = pair[1];
        println!(
            "{upper_name} ({:.5}) over {lower_name} ({:.5}): {:.3}:1",
            luminance(upper),
            luminance(lower),
            contrast(upper, lower)
        );
        assert!(
            luminance(upper) > luminance(lower),
            "{upper_name} must be strictly above {lower_name} in the ladder \
             ({:.5} vs {:.5})",
            luminance(upper),
            luminance(lower)
        );
    }

    assert_eq!(Palette::RAISED_ON_CARD, Palette::SURFACE_2);
    assert_eq!(Palette::RAISED, Palette::SURFACE);
    assert_ne!(
        Palette::RAISED,
        Palette::RAISED_ON_CARD,
        "the two rungs are different values: one tone cannot serve both hosts"
    );

    for (host_name, host, raised) in RAISED_LADDER {
        let ratio = contrast(raised, host);
        println!("raised on the {host_name}: {ratio:.3}:1");
        assert!(
            luminance(raised) > luminance(host),
            "the raised fill for the {host_name} must sit above it"
        );
        assert!(
            ratio >= 1.1,
            "a control raised on the {host_name} must be visibly above it: \
             {ratio:.3}:1"
        );
    }

    assert_eq!(RAISED_LADDER[0].2, Palette::SURFACE);
    assert_eq!(RAISED_LADDER[1].2, Palette::RAISED_ON_CARD);
    assert_ne!(RAISED_LADDER[1].2, Palette::SURFACE);
}

const RAIL_PAINTERS: [(&str, &str, &str); 1] = [(
    "ui/sidebar.rs",
    include_str!("../src/ui/sidebar.rs"),
    "paint_active_band",
)];

const RAIL_PAINTER_SRC: &str = include_str!("../src/ui/components.rs");

#[test]
fn the_rail_width_is_a_named_dimension_defined_exactly_once_in_the_token_layer() {
    assert_eq!(theme::RAIL_WIDTH, 2.0, "the rail width dimension");

    let theme_src = include_str!("../src/theme.rs");
    assert_eq!(
        theme_src.matches("pub const RAIL_WIDTH").count(),
        1,
        "the rail width is defined in the token layer and nowhere else"
    );
    assert_eq!(
        theme_src.matches("pub const MARK_RADIUS").count(),
        1,
        "the 2px radius role stays its own declaration"
    );
    assert_eq!(
        theme::MARK_RADIUS,
        2,
        "MARK_RADIUS is the 2px *radius* role and is not the rail width"
    );
    const {
        assert!(
            theme::INDENT > theme::RAIL_WIDTH,
            "the leading indent is a layout role and must not be confused with \
             the rail width"
        )
    };

    for (file, src, painter) in RAIL_PAINTERS {
        assert!(
            src.contains(painter),
            "{file} no longer has `{painter}`: the rail enumeration follows the \
             painter, not the file name"
        );
        assert!(
            src.contains("paint_rail("),
            "{file}::{painter} no longer routes its rail through the one rail \
             painter: every rail takes its width from theme::RAIL_WIDTH, so a \
             second width in this file is a drift"
        );
    }
    assert!(
        RAIL_PAINTER_SRC.contains("pub fn paint_rail"),
        "the one rail painter is `ui::components::paint_rail`"
    );
    assert!(
        RAIL_PAINTER_SRC.contains("crate::theme::RAIL_WIDTH"),
        "the rail painter must take its width from theme::RAIL_WIDTH — that is \
         what 'defined exactly once' means at the paint site"
    );
    let brand_strokes: Vec<&str> = [
        ("ui/sidebar.rs", include_str!("../src/ui/sidebar.rs")),
        (
            "ui/diff/actions.rs",
            include_str!("../src/ui/diff/actions.rs"),
        ),
    ]
    .into_iter()
    .filter(|(_, src)| src.contains("Stroke::new(2.0, Palette::BRAND)"))
    .map(|(file, _)| file)
    .collect();
    assert!(
        brand_strokes.is_empty(),
        "a rail must not be spelled as its own 2px brand stroke any more; the \
         one rail painter owns it: {brand_strokes:?}"
    );
}

const RAISED_OR_SELECTED: [(&str, Color32); 5] = [
    ("raised surface", Palette::SURFACE),
    ("hover / raised-on-card", Palette::SURFACE_2),
    ("highest raised surface", Palette::SURFACE_3),
    ("selection token", Palette::SELECTION),
    ("selected list row", Palette::ROW_SELECTED),
];

fn is_raised_or_selected(fill: Color32) -> bool {
    RAISED_OR_SELECTED.iter().any(|(_, c)| *c == fill)
}

fn is_a_raised_or_selected_band(fill: Color32) -> bool {
    is_raised_or_selected(fill) || fill == Palette::selection_bg()
}

fn assert_ink_legal_on_fill(site: &str, what: &str, fill: Color32, ink: Color32) {
    if is_a_raised_or_selected_band(fill) {
        assert_ne!(
            ink,
            Palette::INK_3,
            "{site}: {what} paints the muted ink on a raised/selected fill \
             ({fill:?}). INK_3 is not legal there — step up to INK_2."
        );
    }
    if fill != Color32::TRANSPARENT && AUDITED_SURFACES.iter().any(|(_, surface)| *surface == fill)
    {
        let ratio = contrast(ink, fill);
        println!("{site}: {what} ink {ratio:.3}:1 on {fill:?}");
        assert!(
            ratio >= 4.5,
            "{site}: {what} must clear AA on its own fill {fill:?}: {ratio:.3}:1"
        );
    }
}

#[test]
fn the_row_fill_decision_and_the_row_shell_never_put_the_muted_ink_on_a_raised_or_selected_band() {
    use turbogit_ui::ui::components::{RowState, current_row_fill, row_fill, row_ink};

    let brand_rest = turbogit_ui::ui::widgets::tint_over_bg(Palette::BRAND, 0.16);
    let brand_hover = turbogit_ui::ui::widgets::tint_over_bg(Palette::BRAND, 0.24);
    assert_eq!(row_fill(RowState::Default), Color32::TRANSPARENT);
    assert_eq!(row_fill(RowState::Hover), Palette::RAISED_ON_CARD);
    assert_eq!(row_fill(RowState::RowSelected), Palette::ROW_SELECTED);
    assert_eq!(row_fill(RowState::FocusSelected), Palette::selection_bg());
    assert_eq!(current_row_fill(RowState::Default), brand_rest);
    assert_eq!(current_row_fill(RowState::Hover), brand_hover);
    assert_eq!(current_row_fill(RowState::RowSelected), Palette::SELECTION);

    let name_ink = row_ink(false);
    let band_inks = [
        ("hover", row_fill(RowState::Hover), name_ink),
        ("selected", row_fill(RowState::RowSelected), name_ink),
        (
            "focus selected",
            row_fill(RowState::FocusSelected),
            name_ink,
        ),
        (
            "current at rest",
            current_row_fill(RowState::Default),
            name_ink,
        ),
        (
            "current hovered",
            current_row_fill(RowState::Hover),
            name_ink,
        ),
        (
            "current selected",
            current_row_fill(RowState::RowSelected),
            name_ink,
        ),
    ];
    for (name, fill, _ink) in band_inks {
        println!("row band {name}: {fill:?}");
        assert!(
            fill != row_fill(RowState::Default),
            "row band {name} must be a painted band, not the transparent rest"
        );
    }
    for (name, fill, ink) in band_inks {
        println!("row band {name}: {fill:?} with {ink:?}");
        assert_ink_legal_on_fill("row shell", name, fill, ink);
        assert_eq!(ink, name_ink, "row band {name} wears the row's own ink");
        let ratio = contrast(ink, fill);
        assert!(
            ratio >= 4.5,
            "row shell: a row on the {name} band ({fill:?}) must read clearly: \
             {ratio:.3}:1"
        );
    }

    assert_eq!(name_ink, Palette::T_PRIMARY, "a fresh row reads at primary");
    let dim_ink = row_ink(true);
    assert_eq!(
        dim_ink,
        Palette::T_MUTED,
        "a stale row dims to the muted step"
    );
    assert_ink_legal_on_fill(
        "row shell",
        "dimmed resting row",
        Palette::CONTENT_BG,
        dim_ink,
    );
    assert!(
        !is_raised_or_selected(Palette::CONTENT_BG),
        "a resting row's surface must not itself be a raised/selected surface, \
         or the dim role would be illegal everywhere"
    );
}

#[test]
fn the_chip_constructors_never_put_the_muted_ink_on_a_raised_or_selected_fill() {
    use turbogit_ui::ui::widgets::{
        BadgeKind, ButtonVariant, COUNT_CHIP_COLORS, CURRENT_CHIP_COLORS, REF_CHIP_COLORS,
        WidgetState,
    };

    for (who, colors) in [
        ("ref chip", REF_CHIP_COLORS),
        ("current chip", CURRENT_CHIP_COLORS),
        ("count chip", COUNT_CHIP_COLORS),
    ] {
        assert_ink_legal_on_fill("chip", who, colors.bg, colors.fg);
        assert!(
            is_raised_or_selected(colors.bg),
            "{who} rests on a raised or selected surface, which is exactly why \
             its ink has to be the step-up one: {colors:?}"
        );
    }
    assert_eq!(REF_CHIP_COLORS.bg, Palette::RAISED_ON_CARD);
    assert_eq!(REF_CHIP_COLORS.fg, Palette::INK_2);
    assert_eq!(CURRENT_CHIP_COLORS.bg, Palette::ROW_SELECTED);
    assert_eq!(CURRENT_CHIP_COLORS.fg, Palette::ACCENT_TEXT);
    assert_eq!(COUNT_CHIP_COLORS.bg, Palette::RAISED);
    assert_eq!(COUNT_CHIP_COLORS.fg, Palette::INK_2);
    assert!(
        !is_raised_or_selected(Palette::INK_3),
        "INK_3 is the ink this rule exists to keep off a raised/selected fill"
    );

    for kind in [
        BadgeKind::Neutral,
        BadgeKind::Added,
        BadgeKind::Modified,
        BadgeKind::Deleted,
    ] {
        let colors = kind.colors();
        assert_eq!(colors.fg, kind.accent(), "{kind:?}: accent is the ink");
        assert_ink_legal_on_fill("badge", &format!("{kind:?}"), colors.bg, colors.fg);
    }
    let neutral = BadgeKind::Neutral.colors();
    assert_eq!(neutral.bg, Palette::SURFACE_3);
    assert_eq!(neutral.fg, Palette::INK_2);
    assert!(
        !is_raised_or_selected(Palette::INK_2),
        "the step-up ink is not itself a surface"
    );

    for variant in [
        ButtonVariant::Ghost,
        ButtonVariant::Compact,
        ButtonVariant::Icon,
    ] {
        for state in [
            WidgetState::Idle,
            WidgetState::Hovered,
            WidgetState::Active,
            WidgetState::Disabled,
        ] {
            let fill = variant.fill(state);
            assert_ink_legal_on_fill(
                "ghost button",
                &format!("{variant:?}/{state:?}"),
                fill,
                variant.text(state),
            );
        }
    }
    assert_eq!(
        ButtonVariant::Ghost.fill(WidgetState::Disabled),
        Color32::TRANSPARENT
    );
    assert_eq!(
        ButtonVariant::Ghost.text(WidgetState::Disabled),
        Palette::INK_3
    );
    assert_eq!(
        ButtonVariant::Primary.fill(WidgetState::Disabled),
        Palette::BRAND
    );
    assert_eq!(
        ButtonVariant::Primary.text(WidgetState::Disabled),
        Palette::INK_3
    );
    assert!(
        !is_raised_or_selected(Palette::BRAND),
        "the brand fill is not a raised or selected surface, so the muted step \
         on it is outside this ticket's rule"
    );
}

#[test]
fn the_two_new_chip_fills_carry_ink_that_clears_aa() {
    use turbogit_ui::ui::widgets::{COUNT_CHIP_COLORS, CURRENT_CHIP_COLORS, REF_CHIP_COLORS};

    const AA: f64 = 4.5;

    let ref_ratio = contrast(REF_CHIP_COLORS.fg, REF_CHIP_COLORS.bg);
    let muted_on_ref_fill = contrast(Palette::INK_3, REF_CHIP_COLORS.bg);
    println!(
        "ref chip:    INK_2 {ref_ratio:.3}:1 on {:?}  (INK_3 would be {muted_on_ref_fill:.3}:1)",
        REF_CHIP_COLORS.bg
    );
    assert_eq!(REF_CHIP_COLORS.bg, Palette::RAISED_ON_CARD);
    assert_eq!(REF_CHIP_COLORS.fg, Palette::INK_2);
    assert!(
        ref_ratio >= AA,
        "the ref chip's ink must clear AA on its own fill: {ref_ratio:.3}:1"
    );
    assert!(
        muted_on_ref_fill < AA,
        "…and the step up from the muted ink has to be worth making: the muted \
         step measures {muted_on_ref_fill:.3}:1 there"
    );

    let current_ratio = contrast(CURRENT_CHIP_COLORS.fg, CURRENT_CHIP_COLORS.bg);
    let muted_on_current_fill = contrast(Palette::INK_3, CURRENT_CHIP_COLORS.bg);
    println!(
        "current chip: ACCENT_TEXT {current_ratio:.3}:1 on {:?}  (INK_3 would \
         be {muted_on_current_fill:.3}:1)",
        CURRENT_CHIP_COLORS.bg
    );
    assert_eq!(CURRENT_CHIP_COLORS.bg, Palette::ROW_SELECTED);
    assert_eq!(CURRENT_CHIP_COLORS.fg, Palette::ACCENT_TEXT);
    assert!(
        current_ratio >= AA,
        "the current chip's ink must clear AA on its own fill: {current_ratio:.3}:1"
    );
    assert!(
        muted_on_current_fill < AA,
        "…and the selected row is no more forgiving than the ref chip's fill: \
         the muted step measures {muted_on_current_fill:.3}:1 there"
    );

    let count_ratio = contrast(COUNT_CHIP_COLORS.fg, COUNT_CHIP_COLORS.bg);
    println!(
        "count chip:  INK_2 {count_ratio:.3}:1 on {:?}",
        COUNT_CHIP_COLORS.bg
    );
    assert_eq!(COUNT_CHIP_COLORS.bg, Palette::RAISED);
    assert!(
        count_ratio >= AA,
        "the count chip's ink must clear AA on its own fill: {count_ratio:.3}:1"
    );
}

#[test]
fn the_current_chip_is_the_only_chip_allowed_to_carry_the_accent() {
    use turbogit_ui::ui::widgets::{COUNT_CHIP_COLORS, CURRENT_CHIP_COLORS, REF_CHIP_COLORS};

    assert_eq!(Palette::ACCENT, Palette::BRAND);

    let chips = [
        ("ref chip", REF_CHIP_COLORS),
        ("current chip", CURRENT_CHIP_COLORS),
        ("count chip", COUNT_CHIP_COLORS),
    ];
    for (who, colors) in chips {
        assert_ne!(
            colors.bg,
            Palette::BRAND,
            "no chip fills the brand token, and the {who} just did: a brand fill \
             behind a running string is a button's shape, and a button's shape \
             is the blue soup this vocabulary exists to remove"
        );
    }
    let accented: Vec<&str> = chips
        .iter()
        .filter(|(_, colors)| colors.fg == Palette::ACCENT_TEXT)
        .map(|(who, _)| *who)
        .collect();
    assert_eq!(
        accented,
        ["current chip"],
        "exactly one chip wears the readable accent, and it is the one stating a \
         fact about the current ref rather than asking for something"
    );
    for (who, colors) in chips {
        assert_ne!(
            colors.fg,
            Palette::BRAND_INK,
            "the {who} must not wear the on-brand ink: there is no solid brand \
             fill left for it to sit on"
        );
    }
    let state_colors: [(&str, Color32); 4] = [
        ("COUNTER", Palette::COUNTER),
        ("AHEAD", Palette::AHEAD),
        ("STATUS_DIVERGED", Palette::STATUS_DIVERGED),
        (
            "INK_3 (the uninitialised arm — work owed, not a severity)",
            Palette::INK_3,
        ),
    ];
    for (who, colors) in chips {
        for (token, state_color) in state_colors {
            assert_ne!(
                colors.bg, state_color,
                "the {who} must not fill a repository-state colour ({token}): a \
                 state is coloured text or a leading dot from `RepoState::color`, \
                 never a filled pill"
            );
            assert_ne!(
                colors.fg, state_color,
                "the {who} must not ink itself a repository-state colour \
                 ({token}) either — the ink says what the text is, not what \
                 state the repository is in"
            );
        }
    }
    for state in [
        turbogit_ui::theme::RepoState::Clean,
        turbogit_ui::theme::RepoState::Dirty,
        turbogit_ui::theme::RepoState::Conflict,
        turbogit_ui::theme::RepoState::Diverged,
        turbogit_ui::theme::RepoState::Unpushed,
        turbogit_ui::theme::RepoState::Unpulled,
        turbogit_ui::theme::RepoState::Uninitialized,
    ] {
        assert!(
            state_colors.iter().any(|(_, c)| *c == state.color()),
            "{state:?} resolves to {:?}, which is not in the list of state \
             colours above — name it there or this loop stops covering the map",
            state.color()
        );
    }
}

#[test]
fn p0_repo_state_shared_mapping_is_app_wide() {
    use turbogit_ui::theme::RepoState;
    let expected = [
        (RepoState::Clean, Palette::AHEAD),
        (RepoState::Unpushed, Palette::AHEAD),
        (RepoState::Dirty, Palette::COUNTER),
        (RepoState::Unpulled, Palette::COUNTER),
        (RepoState::Conflict, Palette::STATUS_DIVERGED),
        (RepoState::Diverged, Palette::STATUS_DIVERGED),
    ];
    for (state, color) in expected {
        assert_eq!(state.color(), color, "{state:?}");
    }
    assert_eq!(
        turbogit_ui::ui::sidebar::dot_color(RepoState::Dirty),
        Palette::COUNTER
    );
    assert_eq!(
        turbogit_ui::ui::sidebar::dot_color(RepoState::Diverged),
        Palette::STATUS_DIVERGED
    );
    assert_eq!(RepoState::Clean.words(), "");
    assert_eq!(RepoState::Dirty.words(), "dirty");
    assert_eq!(RepoState::Conflict.words(), "has conflicts");
    assert_eq!(RepoState::Diverged.words(), "diverged");
    assert_eq!(RepoState::Unpushed.words(), "unpushed commits");
    assert_eq!(RepoState::Unpulled.words(), "unpulled commits");
    assert_eq!(
        turbogit_ui::ui::components::sync_ink(turbogit_ui::ui::components::SyncKind::Diverged),
        RepoState::Diverged.color()
    );
    assert_eq!(
        turbogit_ui::ui::components::sync_ink(turbogit_ui::ui::components::SyncKind::Behind),
        RepoState::Unpulled.color()
    );
    assert_eq!(
        turbogit_ui::ui::components::sync_ink(turbogit_ui::ui::components::SyncKind::Ahead),
        RepoState::Unpushed.color()
    );
    assert_eq!(RepoState::Diverged.color(), Palette::STATUS_DIVERGED);
    assert_eq!(RepoState::Unpulled.color(), Palette::COUNTER);

    assert_eq!(RepoState::Uninitialized.color(), Palette::INK_3);
    assert_eq!(RepoState::Uninitialized.words(), "not initialized");
    assert_ne!(
        RepoState::Uninitialized.color(),
        RepoState::Clean.color(),
        "a clean repository and an uninitialised submodule must never look alike"
    );
    assert_ne!(
        RepoState::Uninitialized.color(),
        RepoState::Diverged.color(),
        "…nor may it borrow the divergence red"
    );
}

#[test]
fn one_authoritative_primary_ink_across_shell_and_tool_windows() {
    assert_eq!(
        Palette::INK,
        Palette::T_PRIMARY,
        "IK must be an alias of the authoritative primary"
    );
    assert_eq!(Palette::INK_2, Palette::T_SECONDARY);
    assert_eq!(Palette::INK_3, Palette::T_MUTED);
    assert_eq!(Palette::INK_4, Palette::T_DIM);

    for (brighter, dimmer) in INK_RAMP.windows(2).map(|w| (w[0], w[1])) {
        assert_ne!(
            brighter.ink, dimmer.ink,
            "{} and {} must be distinct levels, not two names for one grey",
            brighter.name, dimmer.name
        );
    }
}

#[test]
fn secondary_and_muted_keep_readable_distinct_roles() {
    let ctx = egui::Context::default();
    configure_style(&ctx);

    assert_eq!(Palette::T_SECONDARY, Palette::INK_2);
    assert_eq!(Palette::T_MUTED, Palette::INK_3);

    let ratio = contrast(Palette::T_SECONDARY, Palette::SELECTION);
    println!("T_SECONDARY on selection: {ratio:.3}:1");
    assert!(
        ratio >= 4.5,
        "INK_2 on selection must stay ≥4.5:1, got {ratio:.3}:1"
    );

    let muted = contrast(Palette::T_MUTED, Palette::SELECTION);
    println!("T_MUTED on selection: {muted:.3}:1 (not legal — step up to INK_2)");
    assert!(
        muted < 4.5,
        "T_MUTED is not legal on the selection fill; if this has become \
         readable, the step-up rule in theme.rs is stale and must be rewritten: \
         {muted:.3}:1"
    );
    let content = contrast(Palette::T_MUTED, Palette::CONTENT_BG);
    assert!(
        content >= 4.5,
        "T_MUTED on the content surface must still clear AA: {content:.3}:1"
    );
}

#[test]
fn selection_uses_one_canonical_opaque_fill() {
    assert!(
        Palette::SELECTION.is_opaque(),
        "the canonical opaque selection fill is solid, not a composite"
    );
    assert_ne!(
        Palette::SELECTION,
        Palette::selection_bg(),
        "opaque selection must stay distinct from the translucent focus fill"
    );
}

use egui::Color32;
use test_support::wcag::{contrast, luminance};
use turbogit_ui::theme::{self, Palette, configure_style};

const BG: Color32 = Color32::from_rgb(0x1e, 0x1f, 0x22);
const SURFACE: Color32 = Color32::from_rgb(0x2b, 0x2d, 0x30);
const SURFACE_2: Color32 = Color32::from_rgb(0x31, 0x34, 0x38);
const SURFACE_3: Color32 = Color32::from_rgb(0x3c, 0x3f, 0x41);
const INK: Color32 = Color32::from_rgb(0xdf, 0xe1, 0xe5);
const BRAND: Color32 = Color32::from_rgb(0x35, 0x74, 0xf0);
const STATE_WARNING: Color32 = Color32::from_rgb(0xf9, 0xa8, 0x25);
const STATE_ERROR: Color32 = Color32::from_rgb(0xef, 0x53, 0x50);

#[test]
fn dark_visuals_map_the_spec_tokens() {
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let v = &ctx.style_of(egui::Theme::Dark).visuals;

    assert_eq!(v.panel_fill, BG, "panel_fill");
    assert_eq!(v.window_fill, SURFACE, "window_fill");
    assert_eq!(v.extreme_bg_color, BG, "extreme_bg_color");
    assert_eq!(v.override_text_color, Some(INK), "override_text_color");
    assert_eq!(v.faint_bg_color, SURFACE_2, "faint_bg_color");
    assert_eq!(v.code_bg_color, SURFACE_3, "code_bg_color");
    assert_eq!(
        v.hyperlink_color,
        Palette::ACCENT_TEXT,
        "hyperlink_color (B2: readable accent ink)"
    );
    assert_eq!(v.warn_fg_color, STATE_WARNING, "warn_fg_color");
    assert_eq!(v.error_fg_color, STATE_ERROR, "error_fg_color");
    assert!(v.dark_mode, "dark-only app must stay in dark mode");
}

#[test]
fn widget_states_follow_the_surface_mapping() {
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let w = &ctx.style_of(egui::Theme::Dark).visuals.widgets;

    assert_eq!(w.noninteractive.bg_fill, SURFACE, "noninteractive.bg_fill");
    assert_eq!(
        w.noninteractive.fg_stroke.color,
        Palette::T_SECONDARY,
        "noninteractive.fg (B2 lift)"
    );
    assert_eq!(w.inactive.bg_fill, SURFACE_2, "inactive.bg_fill");
    assert_eq!(
        w.inactive.fg_stroke.color,
        Palette::T_SECONDARY,
        "inactive.fg (B2 lift)"
    );
    assert_eq!(w.hovered.bg_fill, SURFACE_2, "hovered.bg_fill");
    assert_eq!(w.hovered.fg_stroke.color, INK, "hovered.fg");
    assert_eq!(w.active.bg_fill, SURFACE_3, "active.bg_fill");
    assert_eq!(w.active.fg_stroke.color, INK, "active.fg");
    assert_eq!(w.open.bg_fill, SURFACE_3, "open.bg_fill");
    assert_eq!(w.open.fg_stroke.color, INK, "open.fg");
}

#[test]
fn the_window_stroke_is_the_one_stroke_the_floating_surfaces_wear() {
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let v = &ctx.style_of(egui::Theme::Dark).visuals;
    assert_eq!(
        v.window_stroke,
        egui::Stroke::new(1.0, Palette::LINE),
        "a window — a popover, a dialog, a menu, a toast — wears one 1px hairline \
         in the line tone, and that is the only stroke in the app that means \
         'this floats'"
    );
    assert_eq!(v.window_stroke.color, Palette::LINE);
    assert_ne!(
        v.window_stroke, v.widgets.inactive.fg_stroke,
        "the floating edge is a surface boundary, not a control's own stroke"
    );
    assert!(
        (v.window_stroke.width - 1.0).abs() < f32::EPSILON,
        "and it is one pixel: a 2px window edge is a frame, not a hairline"
    );
}

#[test]
fn selection_uses_brand_with_brand_stroke() {
    let ctx = egui::Context::default();
    configure_style(&ctx);
    let sel = &ctx.style_of(egui::Theme::Dark).visuals.selection;

    assert_eq!(sel.stroke.color, BRAND, "selection.stroke");
    assert_eq!(sel.stroke.width, 1.0);
    assert_eq!(
        sel.bg_fill,
        Color32::from_rgba_premultiplied(0x0d, 0x1d, 0x3c, 0x40),
        "selection.bg_fill"
    );
}

use turbogit_ui::theme::font_definitions;

#[test]
fn proportional_family_leads_with_the_ui_sans() {
    let defs = font_definitions();
    let fam = &defs.families.get(&egui::FontFamily::Proportional).unwrap();
    assert_eq!(fam[0], "Ubuntu-Light", "primary chrome font");
    assert!(
        fam.iter()
            .any(|f| f == "emoji-icon-font" || f == "NotoEmoji-Regular"),
        "egui's built-in glyph fallbacks stay behind the sans so chrome text \
         outside its coverage still resolves"
    );
    // The Segoe UI fallback is registered only when the face is actually
    // present on this machine (Windows); it degrades gracefully by
    // omission elsewhere (ADR-0002).
    if cfg!(windows) {
        assert!(
            fam.iter().any(|f| f == "Segoe UI"),
            "Segoe UI fallback required"
        );
    } else {
        assert!(
            !fam.iter().any(|f| f == "Segoe UI"),
            "absent system face must be omitted"
        );
    }
}

#[test]
fn monospace_family_is_jetbrains_mono_with_consolas_fallback() {
    let defs = font_definitions();
    let fam = &defs.families.get(&egui::FontFamily::Monospace).unwrap();
    assert_eq!(fam[0], "jetbrains-mono-regular", "primary mono font");
    if cfg!(windows) {
        assert!(
            fam.iter().any(|f| f == "Consolas"),
            "Consolas fallback required"
        );
    } else {
        assert!(
            !fam.iter().any(|f| f == "Consolas"),
            "absent system face must be omitted"
        );
    }
}

#[test]
fn bold_weight_is_embedded_as_its_own_family() {
    let defs = font_definitions();
    let bold_family = egui::FontFamily::Name("jetbrains-mono-bold".into());
    let fam = &defs
        .families
        .get(&bold_family)
        .expect("bold family registered");
    assert_eq!(fam[0], "jetbrains-mono-bold");

    let data = &defs
        .font_data
        .get("jetbrains-mono-bold")
        .expect("bold font data");
    assert!(data.font.len() > 100_000, "bold ttf should be ~160KB");
}

#[test]
fn embedded_font_data_is_valid_truetype() {
    let defs = font_definitions();
    for key in ["jetbrains-mono-regular", "jetbrains-mono-bold"] {
        let data = defs
            .font_data
            .get(key)
            .unwrap_or_else(|| panic!("{key} missing"));
        // sfnt magic: 0x00010000 (TrueType) or 'OTTO' (CFF).
        let magic = &data.font[0..4];
        assert!(
            magic == [0x00, 0x01, 0x00, 0x00] || magic == b"OTTO",
            "{key} is not a valid sfnt font"
        );
    }
}

use turbogit_ui::theme::install_fonts;

#[test]
fn install_fonts_applies_the_embedded_stack_to_the_context() {
    let ctx = egui::Context::default();
    install_fonts(&ctx);

    // Font definitions take effect at the next pass begin.
    let mut full = ctx.run_ui(egui::RawInput::default(), |_ui| {});
    full.textures_delta.clear();

    ctx.fonts(|f| {
        let defs = f.definitions();
        let fam = defs
            .families
            .get(&egui::FontFamily::Proportional)
            .expect("proportional family registered");
        assert_eq!(fam[0], "Ubuntu-Light", "chrome text renders in the UI sans");
        let bold = egui::FontFamily::Name("jetbrains-mono-bold".into());
        assert!(defs.families.contains_key(&bold), "bold family available");
    });
}

#[test]
fn chrome_seam_is_proportional_while_the_data_seam_is_monospaced() {
    let ctx = egui::Context::default();
    install_fonts(&ctx);

    // Font definitions take effect at the next pass begin.
    let mut full = ctx.run_ui(egui::RawInput::default(), |_ui| {});
    full.textures_delta.clear();

    let chrome = theme::chrome_font(12.0);
    let data = theme::data_font(12.0);
    let chrome_probe = "TurboGit LOCAL REMOTE TAGS origin/main 26 · … \"-+/_ .";
    let (chrome_narrow, chrome_wide, data_narrow, data_wide, chrome_covers) = ctx.fonts_mut(|f| {
        (
            f.glyph_width(&chrome, 'i'),
            f.glyph_width(&chrome, 'W'),
            f.glyph_width(&data, 'i'),
            f.glyph_width(&data, 'W'),
            f.has_glyphs(&chrome, chrome_probe),
        )
    });

    assert!(
        chrome_covers,
        "the chrome family cannot render some chrome text: {chrome_probe}"
    );
    assert!(
        chrome_narrow > 0.0 && chrome_wide > 0.0,
        "the chrome face resolves no glyph at all ({chrome_narrow}, {chrome_wide})"
    );
    assert!(
        chrome_narrow < chrome_wide,
        "chrome text is a proportional face: `i` ({chrome_narrow}) must be \
         narrower than `W` ({chrome_wide})"
    );
    assert!(
        (data_narrow - data_wide).abs() < f32::EPSILON,
        "data text stays monospaced: `i` ({data_narrow}) and `W` ({data_wide}) \
         must share one advance"
    );
}

#[test]
fn accent_accessor_returns_brand() {
    assert_eq!(theme::accent(), Palette::BRAND, "accent() must alias BRAND");
}

#[test]
fn the_diverged_status_names_the_error_red_and_the_other_statuses_do_not_borrow_a_name() {
    use turbogit_ui::theme::RepoState;

    assert_eq!(Palette::STATUS_DIVERGED, Palette::STATE_ERROR);
    assert_ne!(Palette::STATUS_DIVERGED, Palette::STATE_SUCCESS);
    assert_ne!(Palette::STATUS_DIVERGED, Palette::STATE_WARNING);
    assert_ne!(Palette::STATUS_DIVERGED, Palette::STATE_INFO);

    for state in [
        RepoState::Clean,
        RepoState::Dirty,
        RepoState::Conflict,
        RepoState::Diverged,
        RepoState::Unpushed,
        RepoState::Unpulled,
        RepoState::Uninitialized,
    ] {
        assert_eq!(
            state.color(),
            match state {
                RepoState::Clean | RepoState::Unpushed => Palette::AHEAD,
                RepoState::Dirty | RepoState::Unpulled => Palette::COUNTER,
                RepoState::Conflict | RepoState::Diverged => Palette::STATUS_DIVERGED,
                RepoState::Uninitialized => Palette::INK_3,
            },
            "{state:?} must paint a real colour, never a fallback"
        );
        assert!(
            state.color().is_opaque(),
            "{state:?} paints a solid mark, not a composite"
        );
    }
}

#[test]
fn the_sidebar_surface_is_a_dedicated_painted_fill_and_not_an_alias() {
    let sidebar = Palette::SIDEBAR;
    let bg = Palette::BG;
    println!(
        "sidebar over the app background: {:.3}:1",
        contrast(sidebar, bg)
    );
    assert!(
        luminance(sidebar) < luminance(bg),
        "the rail must be darker than the app background it sits beside"
    );
    assert!(
        contrast(sidebar, bg) > 1.0,
        "…by a visible step rather than a hair, or the rail is not a surface"
    );
    for shell_surface in [
        Palette::BG,
        Palette::SURFACE,
        Palette::SURFACE_2,
        Palette::SURFACE_3,
    ] {
        assert_ne!(
            sidebar, shell_surface,
            "sidebar must be a distinct surface token, not an alias of {shell_surface:?}"
        );
    }
    for (role, fill) in [
        ("SURFACE", Palette::SURFACE),
        ("SURFACE_2", Palette::SURFACE_2),
        ("SURFACE_3", Palette::SURFACE_3),
    ] {
        assert!(
            luminance(fill) > luminance(sidebar),
            "{role} is painted inside the rail and must read above it"
        );
    }
    assert_eq!(Palette::BG, Color32::from_rgb(0x1e, 0x1f, 0x22));
    assert_eq!(Palette::SURFACE, Color32::from_rgb(0x2b, 0x2d, 0x30));
}

#[test]
fn the_selection_fill_is_solid_and_the_focus_composite_is_not_it() {
    assert!(
        Palette::SELECTION.is_opaque(),
        "selection fill is solid, not a composite"
    );

    assert_ne!(
        Palette::SELECTION,
        Palette::selection_bg(),
        "solid selection must not alias the translucent focus fill"
    );
    assert_eq!(
        Palette::selection_bg(),
        Color32::from_rgba_premultiplied(0x0d, 0x1d, 0x3c, 0x40),
        "selection_bg() must remain the translucent brand blend"
    );
}

#[test]
fn status_letter_tokens_match_the_mockup_modified_added_unversioned() {
    assert_eq!(
        Palette::STATUS_MODIFIED,
        Color32::from_rgb(0xa8, 0xc0, 0xe8),
        "M must render in the mockup's modified blue"
    );
    assert_eq!(
        Palette::STATUS_ADDED,
        Color32::from_rgb(0x6f, 0xae, 0x75),
        "A must render in the added green, lifted to clear AA on the content surface"
    );
    assert_eq!(
        Palette::STATUS_UNVERSIONED,
        Color32::from_rgb(0xb5, 0xb3, 0x7e),
        "U must render in the mockup's unversioned olive"
    );

    let letters = [
        Palette::STATUS_MODIFIED,
        Palette::STATUS_ADDED,
        Palette::STATUS_UNVERSIONED,
    ];
    for (i, a) in letters.iter().enumerate() {
        for b in letters.iter().skip(i + 1) {
            assert_ne!(a, b, "status-letter tokens must be pairwise distinct");
        }
    }
}

#[test]
fn redesign_diff_tokens_match_the_mockup_and_stay_distinct_from_legacy() {
    assert_eq!(
        Palette::DIFF_ADD_ACCENT,
        Color32::from_rgb(0x6f, 0xae, 0x75),
        "added accent — lifted with the added-file green, the one diff token this \
         migration moves"
    );
    assert_eq!(
        Palette::DIFF_DEL_ACCENT,
        Color32::from_rgb(0xf7, 0x54, 0x64),
        "removed accent"
    );

    assert_eq!(Palette::DIFF_ADD_ACCENT, Palette::STATUS_ADDED);
    assert_ne!(Palette::DIFF_ADD_ACCENT, Palette::DIFF_ADD_BG);
    assert_ne!(Palette::DIFF_DEL_ACCENT, Palette::DIFF_DEL_BG);
    assert_ne!(Palette::DIFF_ADD_ACCENT, Palette::DIFF_DEL_ACCENT);
    assert_ne!(Palette::DIFF_ADD_BG, Palette::DIFF_DEL_BG);

    assert_ne!(Palette::DIFF_ADD_ACCENT, Palette::DIFF_ADD_TEXT);
    assert_ne!(Palette::DIFF_DEL_ACCENT, Palette::DIFF_DEL_TEXT);
}

#[test]
fn counter_token_is_reserved_orange_distinct_from_every_accent() {
    assert_eq!(
        Palette::COUNTER,
        Color32::from_rgb(0xe0, 0x88, 0x3c),
        "counter orange must match the design"
    );
    assert!(
        Palette::COUNTER.is_opaque(),
        "counter orange is a solid fill"
    );

    assert_ne!(Palette::COUNTER, Palette::BRAND, "not the general accent");
    assert_ne!(
        Palette::COUNTER,
        Palette::STATE_WARNING,
        "not the generic warning"
    );
    assert_ne!(Palette::COUNTER, Palette::STATE_ERROR, "not the error red");
}

#[test]
fn spacing_scale_tokens_match_the_redesign_spec() {
    assert_eq!(theme::FILE_ROW_HEIGHT, 24.0, "file-row height");
    assert_eq!(theme::GROUP_ROW_HEIGHT, 26.0, "group-row height");
    assert_eq!(theme::PANEL_PADDING, 12.0, "panel padding (12–14 px scale)");

    const GRID: i32 = 4;
    for (name, value) in [
        ("file-row height", theme::FILE_ROW_HEIGHT),
        ("panel padding", theme::PANEL_PADDING),
    ] {
        assert_eq!(
            value as i32 % GRID,
            0,
            "{name} must sit on the 4 px grid the design mandates"
        );
    }
    assert_eq!(
        theme::GROUP_ROW_HEIGHT as i32 % GRID,
        2,
        "the group row is the stated exception — a hair taller than the file \
         rows it heads, so it is a whole-pixel height rather than a grid one"
    );
}

#[test]
fn shared_spacing_roles_match_the_configured_defaults() {
    assert_eq!(theme::ITEM_SPACING, egui::vec2(8.0, 6.0));
    assert_eq!(theme::WINDOW_MARGIN, 10);
    assert_eq!(theme::BUTTON_PADDING, egui::vec2(10.0, 5.0));
    assert_eq!(theme::INDENT, 14.0);

    let ctx = egui::Context::default();
    configure_style(&ctx);
    let spacing = &ctx.style_of(egui::Theme::Dark).spacing;
    assert_eq!(spacing.item_spacing, theme::ITEM_SPACING);
    assert_eq!(
        spacing.window_margin,
        egui::Margin::same(theme::WINDOW_MARGIN)
    );
    assert_eq!(spacing.button_padding, theme::BUTTON_PADDING);
    assert_eq!(spacing.indent, theme::INDENT);
}

#[test]
fn named_density_variants_cover_shell_and_status_bar() {
    assert_eq!(theme::DENSITY_COMPACT_BUTTON, egui::vec2(8.0, 4.0));
    assert_eq!(theme::DENSITY_DENSE_BUTTON, egui::vec2(6.0, 2.0));
    const { assert!(theme::DENSITY_COMPACT_BUTTON.y < theme::BUTTON_PADDING.y) };
    const { assert!(theme::DENSITY_DENSE_BUTTON.y < theme::DENSITY_COMPACT_BUTTON.y) };
    assert_ne!(theme::WINDOW_RADIUS, theme::CHIP_RADIUS);
}

#[test]
fn compressed_list_density_is_a_named_role_not_a_call_site_literal() {
    assert_eq!(theme::DENSITY_COMMAND_BUTTON, egui::vec2(8.0, 1.0));
    assert_eq!(theme::DENSITY_COMMAND_ROW_HEIGHT, 14.0);
    assert_eq!(theme::DENSITY_LIST_GAP, 2.0);
    const { assert!(theme::DENSITY_COMMAND_BUTTON.y < theme::DENSITY_DENSE_BUTTON.y) };
    const { assert!(theme::DENSITY_COMMAND_BUTTON.x < theme::BUTTON_PADDING.x) };
    const { assert!(theme::DENSITY_LIST_GAP < theme::ITEM_SPACING.y) };
}

#[test]
fn chip_radii_are_shared_variants_not_local_contracts() {
    assert_eq!(theme::CHIP_RADIUS, 3);
    assert_eq!(theme::CONTROL_RADIUS, 4);
    assert_eq!(theme::PILL_RADIUS, 9);
    assert_ne!(theme::CHIP_RADIUS, theme::PILL_RADIUS);
    assert_ne!(theme::CONTROL_RADIUS, theme::PILL_RADIUS);
}

#[test]
fn governance_consolidated_role_contract_is_internally_consistent() {
    assert_eq!(Palette::INK, Palette::T_PRIMARY);
    assert_eq!(Palette::INK_2, Palette::T_SECONDARY);
    assert_eq!(Palette::INK_3, Palette::T_MUTED);
    assert_eq!(Palette::INK_4, Palette::T_DIM);
    assert_ne!(Palette::T_SECONDARY, Palette::T_MUTED);
    assert_ne!(Palette::T_MUTED, Palette::T_DIM);
    assert!(Palette::SELECTION.is_opaque());
    assert_ne!(Palette::SELECTION, Palette::selection_bg());
    assert_eq!(Palette::RULE_CONTENT, Palette::RAISED);
    assert_ne!(Palette::RULE_CONTENT, Palette::LINE);
    assert_ne!(theme::WINDOW_RADIUS, theme::CHIP_RADIUS);
    const { assert!(theme::DENSITY_DENSE_BUTTON.y < theme::DENSITY_COMPACT_BUTTON.y) };
    assert_ne!(Palette::STATE_INFO, Palette::STATE_WARNING);
    assert_ne!(Palette::STATE_WARNING, Palette::STATE_ERROR);
}

#[test]
fn the_three_hairline_roles_are_named_and_resolve_to_their_tones() {
    assert_eq!(Palette::RULE_CONTENT, Palette::RAISED);
    assert_ne!(Palette::RULE_CONTENT, Palette::LINE);

    assert_eq!(Palette::RULE_FOOTER, Palette::LINE);
    assert_ne!(
        Palette::RULE_FOOTER,
        Palette::RULE_CONTENT,
        "a modal's action slot is a boundary, not a division"
    );
    assert!(
        luminance(Palette::RULE_FOOTER) > luminance(Palette::RULE_CONTENT),
        "the footer rule must actually read stronger than the content divider: \
         {} vs {}",
        luminance(Palette::RULE_FOOTER),
        luminance(Palette::RULE_CONTENT)
    );

    assert_eq!(Palette::RULE_STRUCTURAL, Palette::LINE_SUBTLE);
    assert_ne!(Palette::RULE_STRUCTURAL, Palette::RULE_CONTENT);
    assert_ne!(Palette::RULE_STRUCTURAL, Palette::RULE_FOOTER);

    assert_eq!(Palette::LINE, Color32::from_rgb(0x4e, 0x51, 0x57));
    assert_eq!(Palette::LINE_SUBTLE, Color32::from_rgb(0x36, 0x38, 0x3c));
    assert_eq!(Palette::RULE_CONTENT, Color32::from_rgb(0x2b, 0x2d, 0x30));

    for (weaker_name, weaker, stronger_name, stronger) in [
        (
            "RULE_CONTENT",
            Palette::RULE_CONTENT,
            "RULE_STRUCTURAL",
            Palette::RULE_STRUCTURAL,
        ),
        (
            "RULE_STRUCTURAL",
            Palette::RULE_STRUCTURAL,
            "RULE_FOOTER",
            Palette::RULE_FOOTER,
        ),
    ] {
        assert!(
            luminance(weaker) < luminance(stronger),
            "{weaker_name} ({:.5}) must read weaker than {stronger_name} ({:.5})",
            luminance(weaker),
            luminance(stronger)
        );
    }
}

const TOKEN_INVENTORY: &[&str] = &[
    "BG",
    "SURFACE",
    "SURFACE_2",
    "SURFACE_3",
    "SURFACE_WARNING",
    "SURFACE_INFLIGHT_ABORT",
    "LINE",
    "LINE_SUBTLE",
    "INK",
    "INK_2",
    "INK_3",
    "INK_4",
    "BRAND",
    "BRAND_INK",
    "STATE_SUCCESS",
    "STATE_WARNING",
    "STATE_ERROR",
    "STATE_INFO",
    "STATUS_DIVERGED",
    "CONTENT_BG",
    "RAISED",
    "RAISED_ON_CARD",
    "SELECTION",
    "ROW_SELECTED",
    "SECTION_BG",
    "RULE_CONTENT",
    "RULE_FOOTER",
    "RULE_STRUCTURAL",
    "ACCENT",
    "AHEAD",
    "DANGER",
    "LINK",
    "T_PRIMARY",
    "T_SECONDARY",
    "T_MUTED",
    "T_DIM",
    "ACCENT_TEXT",
    "COUNTER",
    "STATUS_MODIFIED",
    "STATUS_ADDED",
    "STATUS_UNVERSIONED",
    "STATUS_DELETED",
    "DIFF_ADD_BG",
    "DIFF_ADD_TEXT",
    "DIFF_DEL_BG",
    "DIFF_DEL_TEXT",
    "DIFF_ADD_ACCENT",
    "DIFF_DEL_ACCENT",
    "DIFF_ADD_UNSTAGED_BG",
    "DIFF_ADD_UNSTAGED_TEXT",
    "DIFF_DEL_UNSTAGED_BG",
    "DIFF_DEL_UNSTAGED_TEXT",
    "SIDEBAR",
    "SECTION_TINT",
    "MARKER_TINT",
    "FILE_ROW_HEIGHT",
    "GROUP_ROW_HEIGHT",
    "PANEL_PADDING",
    "ITEM_SPACING",
    "WINDOW_MARGIN",
    "BUTTON_PADDING",
    "INDENT",
    "CELL_GAP",
    "DENSITY_COMPACT_BUTTON",
    "DENSITY_DENSE_BUTTON",
    "DENSITY_COMMAND_BUTTON",
    "DENSITY_COMMAND_ROW_HEIGHT",
    "DENSITY_LIST_GAP",
    "STATUS_INFLIGHT_ABORT_GAP",
    "CHIP_RADIUS",
    "CONTROL_RADIUS",
    "WINDOW_RADIUS",
    "MENU_RADIUS",
    "CARD_RADIUS",
    "PILL_RADIUS",
    "MARK_RADIUS",
    "CASCADE_ACCENT",
    "RAIL_WIDTH",
    "CARET_WIDTH",
    "TYPE_SECTION",
    "TYPE_CHIP",
    "TYPE_CONTROL",
    "TYPE_BODY",
    "TYPE_DETAIL_TITLE",
    "TYPE_GROUP_NAME",
    "TYPE_PANE_TITLE",
    "TYPE_WORDMARK",
    "TYPE_TAGLINE",
    "TYPE_STATISTIC",
    "INDENT_STEPS_PER_LEVEL",
    "JETBRAINS_MONO_REGULAR",
    "JETBRAINS_MONO_BOLD",
    "NOTO_SANS_SC_REGULAR",
];

fn declared_theme_tokens() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/theme.rs");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut out = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        if line.starts_with("//") {
            continue;
        }
        let Some(rest) = line.strip_prefix("pub const ") else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
            .collect();
        assert!(
            !name.is_empty(),
            "unreadable `pub const` in theme.rs: {line}"
        );
        out.push(name);
    }
    out
}

#[test]
fn the_token_inventory_is_closed_over_the_palette() {
    let declared = declared_theme_tokens();

    let mut declared_sorted = declared.clone();
    declared_sorted.sort();
    let declared_total = declared_sorted.len();
    declared_sorted.dedup();
    assert_eq!(
        declared_total,
        declared_sorted.len(),
        "theme.rs declares a token name twice; every name is one token: {declared:?}"
    );

    let mut listed = TOKEN_INVENTORY.to_vec();
    listed.sort();
    let before = listed.len();
    listed.dedup();
    assert_eq!(
        before,
        listed.len(),
        "the inventory lists a name twice: {TOKEN_INVENTORY:?}"
    );

    let undeclared: Vec<&&str> = TOKEN_INVENTORY
        .iter()
        .filter(|name| !declared.iter().any(|d| d == *name))
        .collect();
    assert!(
        undeclared.is_empty(),
        "the inventory names tokens theme.rs no longer declares: {undeclared:?}. \
         A token the sweep deleted goes from the palette and this list in the \
         same change."
    );

    let uninventoried: Vec<&String> = declared
        .iter()
        .filter(|name| !TOKEN_INVENTORY.contains(&name.as_str()))
        .collect();
    assert!(
        uninventoried.is_empty(),
        "theme.rs declares tokens the inventory does not list: {uninventoried:?}. \
         Adding a token is a deliberate edit to both, and this assertion is what \
         makes it one — a token that arrives without a consumer lands here first."
    );

    assert_eq!(
        declared.len(),
        TOKEN_INVENTORY.len(),
        "the inventory holds {} tokens and theme.rs declares {}",
        TOKEN_INVENTORY.len(),
        declared.len()
    );
}

#[test]
fn consumer_presence_is_review_gated_and_this_suite_does_not_scan_for_it() {
    assert_eq!(
        declared_theme_tokens().len(),
        TOKEN_INVENTORY.len(),
        "the inventory is the suite's half of the sweep; the consumer half is \
         reviewed against it, and adding a token without a caller is the one \
         failure this suite reports by name"
    );
    assert_eq!(
        (
            TOKEN_INVENTORY.contains(&"INK_4"),
            TOKEN_INVENTORY.contains(&"T_DIM"),
            TOKEN_INVENTORY.contains(&"RULE_CONTENT")
        ),
        (true, true, true),
        "the two standing exceptions stay inventoried: each is a rung of a \
         ladder the palette measures, so deleting one would delete the \
         measurement rather than fix a dead token"
    );
}

fn diff_fixture_repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    use test_support::git_seed::git;
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    let file = repo.join("file.txt");
    std::fs::write(
        &file,
        "alpha\nbeta\ndelta\nepsilon\nzeta\neta\ntheta\ngamma\niota\n",
    )
    .expect("seed file");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "c1"]);
    std::fs::write(
        &file,
        "alpha\nBETA\ndelta\nepsilon\nzeta\neta\ntheta\ngamma\niota\n",
    )
    .expect("staged edit");
    git(&repo, &["add", "file.txt"]);
    std::fs::write(
        &file,
        "alpha\nBETA\ndelta\nepsilon\nzeta\neta\ntheta\nGAMMA\niota\n",
    )
    .expect("unstaged edit");
    (tmp, repo)
}

fn diff_shell(
    repo: &std::path::Path,
) -> egui_kittest::Harness<'static, turbogit_app::state::AppState> {
    use turbogit_app::state::AppState;
    let state = AppState::new(repo.to_path_buf());
    assert!(
        !state.multi.roots.is_empty(),
        "the repository root must be discovered, or the diff pane never renders"
    );
    test_support::harness::shell_harness_over(state, egui::vec2(1024.0, 900.0))
}

/// Step until two consecutive frames agree *and* no read is outstanding, budgeted
/// by wall clock rather than by frame count so a contended `git` subprocess cannot
/// starve it into a flake.
///
/// Not either shared settle, which fingerprint painted text alone: the diff pane
/// paints its header and band before the `diff_patch` worker has answered, so a
/// text-only fingerprint reports a settled frame over a pane about to fill in.
fn settle_diff_shell(harness: &mut egui_kittest::Harness<'static, turbogit_app::state::AppState>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut previous = String::new();
    while std::time::Instant::now() < deadline {
        harness.step();
        let fingerprint = format!(
            "{:?}|read={}",
            test_support::harness::painted_galleys(harness)
                .into_iter()
                .map(|g| g.text)
                .collect::<Vec<_>>(),
            harness.state().read_pending()
        );
        if fingerprint == previous && !harness.state().read_pending() {
            return;
        }
        previous = fingerprint;
    }
    panic!("the diff pane did not settle within 15s");
}

#[test]
fn the_diff_colour_pair_is_untouched() {
    assert_eq!(Palette::DIFF_ADD_BG, Color32::from_rgb(0x34, 0x4f, 0x3e));
    assert_eq!(Palette::DIFF_DEL_BG, Color32::from_rgb(0x5a, 0x3a, 0x3a));
    assert_eq!(Palette::DIFF_ADD_TEXT, Color32::from_rgb(0x85, 0xe8, 0x9d));
    assert_eq!(Palette::DIFF_DEL_TEXT, Color32::from_rgb(0xff, 0x9a, 0x9a));
    assert_eq!(
        Palette::DIFF_ADD_ACCENT,
        Color32::from_rgb(0x6f, 0xae, 0x75)
    );
    assert_eq!(
        Palette::DIFF_DEL_ACCENT,
        Color32::from_rgb(0xf7, 0x54, 0x64)
    );
    for (who, text, band) in [
        ("added", Palette::DIFF_ADD_TEXT, Palette::DIFF_ADD_BG),
        ("removed", Palette::DIFF_DEL_TEXT, Palette::DIFF_DEL_BG),
    ] {
        let ratio = contrast(text, band);
        println!("{who} text on the {who} band: {ratio:.2}:1");
        assert!(
            ratio >= 4.5,
            "{who} text on the {who} band must clear AA: {ratio:.2}:1"
        );
    }
    assert_ne!(Palette::DIFF_ADD_ACCENT, Palette::DIFF_ADD_BG);
    assert_ne!(Palette::DIFF_DEL_ACCENT, Palette::DIFF_DEL_BG);

    use test_support::harness::{filled_rects, painted_galleys, painted_ink};
    let (_tmp, repo) = diff_fixture_repo();
    let mut harness = diff_shell(&repo);
    settle_diff_shell(&mut harness);
    harness.state_mut().ui.preview_change = Some(ChangeRef::new(
        RootId(repo.to_path_buf().into()),
        repo.join("file.txt"),
    ));
    settle_diff_shell(&mut harness);

    let origin_of = |needle: &str| {
        painted_galleys(&harness)
            .into_iter()
            .find(|g| g.text == needle)
            .map(|g| g.pos)
            .unwrap_or_else(|| {
                panic!(
                    "`{needle}` painted nothing; the frame painted {:?}",
                    painted_galleys(&harness)
                        .into_iter()
                        .map(|g| g.text)
                        .collect::<Vec<_>>()
                )
            })
    };
    let band_under = |at: egui::Pos2| {
        filled_rects(&harness)
            .into_iter()
            .filter(|(rect, _)| rect.contains(at))
            .map(|(_, fill)| fill)
            .collect::<Vec<_>>()
    };
    for (who, band, ink) in [
        ("BETA", Palette::DIFF_ADD_BG, Palette::DIFF_ADD_TEXT),
        ("beta", Palette::DIFF_DEL_BG, Palette::DIFF_DEL_TEXT),
        (
            "GAMMA",
            Palette::DIFF_ADD_UNSTAGED_BG,
            Palette::DIFF_ADD_UNSTAGED_TEXT,
        ),
        (
            "gamma",
            Palette::DIFF_DEL_UNSTAGED_BG,
            Palette::DIFF_DEL_UNSTAGED_TEXT,
        ),
    ] {
        let at = origin_of(who);
        let painted = band_under(at);
        assert!(
            painted.contains(&band),
            "`{who}` is a row whose band is {band:?}, so the token is a colour the \
             pane really uses rather than one nothing reads; painted {painted:?}"
        );
        assert_eq!(
            painted_ink(&harness, who),
            Some(ink),
            "`{who}` is painted in {ink:?}: the staged rows take the full pair and \
             the unstaged rows take the muted one"
        );
    }

    let staged_add = origin_of("BETA");
    let unstaged_add = origin_of("GAMMA");
    assert_ne!(
        band_under(staged_add),
        band_under(unstaged_add),
        "a staged row and an unstaged row of the same kind must not paint the same \
         bands, or the pane still cannot tell the user they toggled a line"
    );
    assert_ne!(
        painted_ink(&harness, "BETA"),
        painted_ink(&harness, "GAMMA"),
        "and their inks differ too, which is the half a screenshot at a glance reads"
    );
}

const DIFF_ROW_ROLES: [(&str, Color32, Color32); 4] = [
    ("staged added", Palette::DIFF_ADD_BG, Palette::DIFF_ADD_TEXT),
    (
        "staged removed",
        Palette::DIFF_DEL_BG,
        Palette::DIFF_DEL_TEXT,
    ),
    (
        "unstaged added",
        Palette::DIFF_ADD_UNSTAGED_BG,
        Palette::DIFF_ADD_UNSTAGED_TEXT,
    ),
    (
        "unstaged removed",
        Palette::DIFF_DEL_UNSTAGED_BG,
        Palette::DIFF_DEL_UNSTAGED_TEXT,
    ),
];

#[test]
fn the_unstaged_row_pair_clears_aa_and_the_staged_pair_keeps_the_full_treatment() {
    assert_eq!(
        Palette::DIFF_ADD_UNSTAGED_BG,
        Color32::from_rgb(0x2a, 0x36, 0x31)
    );
    assert_eq!(
        Palette::DIFF_DEL_UNSTAGED_BG,
        Color32::from_rgb(0x39, 0x2d, 0x30)
    );
    assert_eq!(
        Palette::DIFF_DEL_UNSTAGED_TEXT,
        Color32::from_rgb(0xd9, 0x83, 0x83)
    );
    assert_eq!(
        Palette::DIFF_ADD_UNSTAGED_TEXT,
        Palette::DIFF_ADD_ACCENT,
        "the muted added row's ink is the added accent used in a second role, the \
         same one-green-two-names identity `STATUS_ADDED` already carries"
    );
    assert_eq!(Palette::DIFF_ADD_BG, Color32::from_rgb(0x34, 0x4f, 0x3e));
    assert_eq!(Palette::DIFF_DEL_BG, Color32::from_rgb(0x5a, 0x3a, 0x3a));

    for (who, band, ink) in DIFF_ROW_ROLES {
        let ratio = contrast(ink, band);
        println!("{who}: {ratio:.2}:1");
        assert!(
            ratio >= 4.5,
            "{who} row ink on its own band must clear AA for running text: {ratio:.2}:1"
        );
    }

    for (side, staged_band, staged_ink, muted_band, muted_ink) in [
        (
            "added",
            Palette::DIFF_ADD_BG,
            Palette::DIFF_ADD_TEXT,
            Palette::DIFF_ADD_UNSTAGED_BG,
            Palette::DIFF_ADD_UNSTAGED_TEXT,
        ),
        (
            "removed",
            Palette::DIFF_DEL_BG,
            Palette::DIFF_DEL_TEXT,
            Palette::DIFF_DEL_UNSTAGED_BG,
            Palette::DIFF_DEL_UNSTAGED_TEXT,
        ),
    ] {
        assert_ne!(staged_band, muted_band, "the {side} band quiets");
        assert_ne!(staged_ink, muted_ink, "the {side} ink quiets");
        assert!(
            contrast(staged_ink, muted_ink) >= 1.3,
            "the {side} row's two inks must read as two steps, not as rounding: \
             {:.2}:1 apart",
            contrast(staged_ink, muted_ink)
        );
        let staged_step = contrast(staged_band, Palette::CONTENT_BG);
        let muted_step = contrast(muted_band, Palette::CONTENT_BG);
        assert!(
            muted_step < staged_step && muted_step >= 1.1,
            "the {side} row's muted band must sit between the content surface and \
             its own staged band: {muted_step:.2}:1 against a staged {staged_step:.2}:1"
        );
    }

    for (who, band) in [
        ("staged added", Palette::DIFF_ADD_BG),
        ("unstaged added", Palette::DIFF_ADD_UNSTAGED_BG),
        ("staged removed", Palette::DIFF_DEL_BG),
        ("unstaged removed", Palette::DIFF_DEL_UNSTAGED_BG),
    ] {
        assert!(
            contrast(Palette::ACCENT, band) >= contrast(Palette::ACCENT, Palette::DIFF_ADD_BG),
            "the caret must not read weaker on {who} than it does on the vivid band"
        );
        assert!(
            contrast(Palette::INK_2, band) / contrast(Palette::INK_3, band) >= 1.4,
            "{who} carries the hover step from INK_3 to INK_2, so the two gutter \
             inks have to be a visible step apart on this band too"
        );
    }

    let already_shipped = [
        Palette::BG,
        Palette::SURFACE,
        Palette::SURFACE_2,
        Palette::SURFACE_3,
        Palette::CONTENT_BG,
        Palette::SECTION_BG,
        Palette::ROW_SELECTED,
        Palette::SELECTION,
        Palette::LINE,
        Palette::LINE_SUBTLE,
        Palette::DIFF_ADD_BG,
        Palette::DIFF_DEL_BG,
        Palette::DIFF_ADD_TEXT,
        Palette::DIFF_DEL_TEXT,
        Palette::DIFF_DEL_ACCENT,
        Palette::AHEAD,
        Palette::STATE_SUCCESS,
        Palette::T_MUTED,
    ];
    for fresh in [
        ("DIFF_ADD_UNSTAGED_BG", Palette::DIFF_ADD_UNSTAGED_BG),
        ("DIFF_DEL_UNSTAGED_BG", Palette::DIFF_DEL_UNSTAGED_BG),
        ("DIFF_DEL_UNSTAGED_TEXT", Palette::DIFF_DEL_UNSTAGED_TEXT),
    ] {
        assert!(
            !already_shipped.contains(&fresh.1),
            "{} repeats a value the palette already ships, so it is a second name \
             for it and not a role",
            fresh.0
        );
    }
}

const RAISED_ON_CARD_SITES: [(&str, &str, Color32); 4] = [
    (
        "components::row_fill(Hover) — a list row's hover",
        "a list row on the content surface",
        Palette::RAISED_ON_CARD,
    ),
    (
        "widgets::REF_CHIP_COLORS — a ref chip",
        "a ref name, which mostly sits on a CONTENT_BG card",
        Palette::RAISED_ON_CARD,
    ),
    (
        "widgets::ButtonVariant::Ghost.fill(Hovered) — a ghost control",
        "whatever the caller painted it over; the ladder is host-agnostic, so the \
         rung it carries must be the card's",
        Palette::RAISED_ON_CARD,
    ),
    (
        "widgets::ButtonVariant::Ghost.fill(Active) — a ghost control, pressed",
        "the same host, one step further up the ladder",
        Palette::SURFACE_3,
    ),
];

#[test]
fn no_raised_control_on_a_content_card_takes_the_raised_on_background_fill() {
    use turbogit_ui::ui::components::{RowState, row_fill};
    use turbogit_ui::ui::widgets::{ButtonVariant, REF_CHIP_COLORS, WidgetState};

    let actual: [Color32; 4] = [
        row_fill(RowState::Hover),
        REF_CHIP_COLORS.bg,
        ButtonVariant::Ghost.fill(WidgetState::Hovered),
        ButtonVariant::Ghost.fill(WidgetState::Active),
    ];

    for ((site, host, expected), actual_fill) in RAISED_ON_CARD_SITES.iter().zip(&actual) {
        assert_eq!(
            *expected, *actual_fill,
            "{site} is painted on {host}, so it takes the raised-on-card rung \
             ({expected:?}); it resolves to {actual_fill:?}"
        );
        assert_ne!(
            *actual_fill,
            Palette::RAISED,
            "{site} sits on {host} and must not take the raised-on-background \
             fill: a control that does not step up from the surface it actually \
             sits on is invisible, and `RAISED` on `CONTENT_BG` is 1.1:1 — the \
             floor, not a step."
        );
        assert_ne!(
            *actual_fill,
            Palette::SURFACE,
            "…and `RAISED` is an alias of `SURFACE`, so both spellings of the \
             wrong rung are excluded"
        );
        assert!(
            is_raised_or_selected(*actual_fill),
            "{site}'s fill {actual_fill:?} is on the raised/selected list, so its \
             ink has to be the step-up one — enforced at the chip constructors and \
             the row-fill decision in `design_tokens.rs`"
        );
    }
    assert_eq!(
        RAISED_ON_CARD_SITES.len(),
        actual.len(),
        "every site in the R5 table is measured by this test, and every site this \
         test measures is in the table"
    );
}

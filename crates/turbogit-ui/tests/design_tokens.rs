//! Issue #3 — dark-only design tokens and embedded fonts.
//!
//! ADR-0003: `ThemeMode::{Light, HighContrast}` are deleted outright. A
//! legacy `state.ron` carrying a removed theme preference must still load
//! — the stale key is ignored and the app renders the designed dark
//! experience regardless.

use turbogit_domain::model::ProjectState;

/// Complete pre-redesign `state.ron` document whose theme is a removed
/// mode (`HighContrast`; `Light` is exercised via the same shape).
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

    // The stale theme key is ignored: both legacy documents load to the
    // same clean (dark-only) settings.
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

// --- The ink ramp and where each step is legal (ticket 02) --------------------
//
// Everything here is one measurement helper and one list of audited surfaces.
// There used to be three copies of the WCAG luminance maths in this file (a
// nested `fn`, a nested closure and the module-level `fn` further down); the
// two nested copies are gone, so there is exactly one place a contrast number
// in this suite is computed.

/// The surfaces the palette audits, in ladder order. This list *is* the decision
/// record's surface axis, and every contrast claim in this file is measured
/// against exactly these — a surface that is not here is not audited, and
/// adding one is a decision rather than an accident.
///
/// `SIDEBAR` is a row for the same reason every other one is: it is the left
/// rail's own painted fill (`ui::sidebar::show`), and it is the darkest surface
/// in the app, so it is the one surface no ink can go sub-AA on by accident.
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

/// One ink step and the exact set of audited surfaces it is legal on. The
/// legality set is **data**, not a comment: the sweep below is driven by it,
/// and a step that quietly starts clearing AA somewhere it was not declared
/// legal fails rather than silently widening its own contract.
#[derive(Clone, Copy)]
struct InkStep {
    name: &'static str,
    ink: Color32,
    legal: &'static [&'static str],
}

/// Every audited surface's name, for a step whose legality set is "all of them".
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

/// The surfaces the muted step is legal on, by name. Written down here because
/// this list *is* the narrowed contract: `INK_3` was legal on every audited
/// surface, and it no longer is.
///
/// `SIDEBAR` rides along because it is the left rail's own fill and is darker
/// than the app background, so any ink legal on the app background is legal
/// there by arithmetic rather than by a fourth decision. It is a corollary of
/// the two named below, not a separate case.
const MUTED_LEGAL: &[&str] = &[
    "app background (BG)",
    "content surface (CONTENT_BG)",
    "sidebar (SIDEBAR)",
];

/// The three surfaces the ticket names as `INK_3`'s legal set, asserted
/// separately from [`MUTED_LEGAL`] so the criterion is stated on its own terms
/// rather than inherited from whatever the sweep happens to cover.
const MUTED_LEGAL_NAMED: [&str; 3] = [
    "app background (BG)",
    "content surface (CONTENT_BG)",
    "sidebar (SIDEBAR)",
];

/// The four ramp steps, brightest first. `INK_4`'s legality set is empty and
/// that is the point: it is the one ink with no legal surface in the palette.
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

/// The ink ramp audited on every audited surface, with each step's declared
/// legality set. Prints the whole grid, so a failure's `println!` output is the
/// decision record rather than a single disputed number.
#[test]
fn the_ink_sweep_is_driven_by_each_steps_written_down_legality_set() {
    // The shell-facing names alias the authoritative ramp (C3) — the sweep
    // reads through the aliases precisely so a re-pointed alias fails here.
    assert_eq!(Palette::INK, Palette::T_PRIMARY, "INK aliases T_PRIMARY");
    assert_eq!(
        Palette::INK_2,
        Palette::T_SECONDARY,
        "INK_2 aliases T_SECONDARY"
    );
    assert_eq!(Palette::INK_3, Palette::T_MUTED, "INK_3 aliases T_MUTED");
    assert_eq!(Palette::INK_4, Palette::T_DIM, "INK_4 aliases T_DIM");

    // The readable accent ink is not a ramp step, but it is an ink and it
    // carries its own coverage: it is the ink on a brand link, so it must
    // clear AA everywhere the palette audits. **The four ramp steps themselves
    // are read from [`INK_RAMP`] rather than restated**: this file used to carry
    // a second, local copy of the table here, and a copy is a second place to
    // edit — the exact drift the ramp is meant to prevent, one level up. One
    // table, read by the sweep and by the distinctness test below.
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
                // "Not legal" has to be a real measurement, not a shrug: if an
                // ink outside its declared set is comfortably clear, the set is
                // stale fiction and the contract is quietly wrong.
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
        // And the set names real surfaces, so a typo cannot quietly empty a
        // step's contract.
        for declared in step.legal {
            assert!(
                AUDITED_SURFACES.iter().any(|(n, _)| n == declared),
                "{} declares an un-audited surface `{declared}`",
                step.name
            );
        }
    }
}

/// `INK_3` clearing AA on its three legal surfaces is the half of the narrowed
/// contract that is load-bearing for the surfaces most of the app is: a branch
/// list, a panel, the left rail. Stated on its own so the reason the value exists
/// is readable without the rest of the grid.
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
    // And it is not legal on the rest — asserted as a negative so the rule is
    // "these three", not "at least these three".
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

/// The narrowing is safe *because* `INK_2` clears every surface `INK_3` gave
/// up. Asserted as the measured statement it is, so "step up to `INK_2`" is a
/// promise the suite keeps rather than advice.
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

/// Four genuinely distinguishable steps. A bare `assert_ne!` is not the
/// criterion: two inks that differ by one unit per channel are *distinct tokens
/// and useless steps*, which is the bug the ramp exists to fix. The margin is
/// therefore measured, as a ratio of the two steps' contrast **on the surface a
/// list is read on**.
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
        // Distinctness first: the margin is meaningless on two identical values.
        assert_ne!(
            brighter.ink, dimmer.ink,
            "{} and {} are the same colour wearing two names",
            brighter.name, dimmer.name
        );
        // Ordered: the ramp is brightest-first, so a pair that reads the other
        // way round is a ramp bug, not a tolerance question.
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
/// The two restrictions are different restrictions, and this test keeps them
/// that way. `INK_4` is sub-AA **everywhere**; `INK_3` is sub-AA **only on
/// raised and selected surfaces**. An implementer who reads one as the other
/// either brightens `INK_4` until it is legal (and loses the ramp) or stops
/// trusting `INK_3` on the three surfaces where it is still fine.
#[test]
fn ink_4_is_the_only_ink_that_is_sub_aa_everywhere_and_ink_3_the_only_conditional_one() {
    // (a) On the content surface, exactly one ink is below AA — the dim step.
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

    // (b) Across the whole audited set, exactly one ink is sub-AA on *every*
    // surface. This is the distinct claim: it is not "the dimmest", it is
    // "nowhere legal".
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

    // (c) And exactly one ink is *conditionally* sub-AA — sub-AA on some audited
    // surfaces but not all of them. That is `INK_3`, and it is the only ink for
    // which the surface it sits on decides legality. ("Some but not all", not
    // "some": the dim step is sub-AA everywhere, which is claim (b), and
    // conflating the two is precisely the confusion this test exists to stop.)
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

    // (d) The two are not the same restriction, stated as the distinction
    // rather than inferred from (b) and (c): `INK_4` is nowhere legal, and
    // `INK_3` is legal on three surfaces.
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
    // Every surface it gave up is still on the audited list, so "not legal"
    // is a statement about a surface somebody can actually paint on.
    for (surface_name, _) in RAISED_OR_SELECTED {
        assert!(
            !INK_RAMP[2].legal.contains(&surface_name),
            "{surface_name} is a raised/selected surface: INK_3 must not be \
             declared legal there"
        );
    }
    // Its permitted uses are the reason it exists at all, and they are all
    // "furniture around the answer", never the answer.
    assert_eq!(INK_RAMP[3].name, "INK_4");
    assert!(
        INK_RAMP[3].ink != INK_RAMP[2].ink && INK_RAMP[3].ink != INK_RAMP[1].ink,
        "the dim step must stay its own value, one clear step below the muted one"
    );
}

// --- The added accent, and the diff it is allowed to move with (ticket 02) ---

/// The added accent is one token under two names — the `A` letter on an added
/// file row and the added line's marker in the diff — and it is the one
/// contrast fix this ticket makes. It moved because the added file's letter
/// fails AA on the content surface where the file list is read, not because the
/// diff looked wrong.
#[test]
fn the_added_accent_clears_aa_on_the_content_surface_at_its_new_value() {
    // The criterion first, in its own words: a contrast measurement. The value
    // pins follow, so a regression reports *why* it matters before it reports
    // which literal moved.
    let ratio = contrast(Palette::STATUS_ADDED, Palette::CONTENT_BG);
    println!("added accent on the content surface: {ratio:.3}:1");
    assert!(
        ratio >= 4.5,
        "the added accent must clear AA on the content surface: {ratio:.3}:1"
    );
    // It was 4.33:1 there at `#57965C`; the fix is not a rounding accident, so
    // assert the value moved by a real amount rather than a token amount.
    assert!(
        ratio >= 5.0,
        "the added accent should clear AA with room to spare, not scrape it: \
         {ratio:.3}:1"
    );

    // One token, two spellings — asserted rather than assumed, because a
    // future "just darken the diff's green" edit is exactly the drift this
    // catches, and it is what the original migration plan invites.
    assert_eq!(
        Palette::DIFF_ADD_ACCENT,
        Palette::STATUS_ADDED,
        "the added-file letter and the added line's accent are ONE token"
    );
    // Neither spelling may be reintroduced as a near-duplicate: the value is
    // the only thing that holds the fix.
    assert_eq!(Palette::STATUS_ADDED, Color32::from_rgb(0x6f, 0xae, 0x75));

    // The sweep also covered the meaning-colour neighbours this ticket
    // deliberately does NOT touch, and records what it found so "we only
    // changed one accent" is checkable rather than asserted by vibes:
    //
    // - `AHEAD` is unchanged, and clears AA where it is used (ahead counts,
    //   the current-branch icon and "in sync" all sit on the content surface).
    //   It is *not* clear on every audited surface (3.69:1 on the highest
    //   raised one), so this is not a blanket promise and the suite does not
    //   make one.
    // - `ACCENT_TEXT` is unchanged and does clear AA on every audited surface.
    // - `BRAND`/`ACCENT` and `DANGER` are *fills*, not inks, and are out of
    //   scope for this ticket entirely — asserting a sweep here would be
    //   inventing a guarantee the migration has not made.
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

/// The honest, cheap statement that the diff was left alone. The added accent
/// is the *only* diff token this ticket moves, and it moves because it is
/// another name for the added-file accent — not because the diff's vocabulary
/// was reopened. Everything else the diff paints is pinned here so a widening of
/// this ticket shows up as a failing assertion instead of a design change
/// smuggled in under "one accent".
#[test]
fn the_diff_vocabulary_is_untouched_except_the_one_token_that_is_also_the_added_file_accent() {
    // The background/text pair, exactly as before.
    assert_eq!(Palette::DIFF_ADD_BG, Color32::from_rgb(0x34, 0x4f, 0x3e));
    assert_eq!(Palette::DIFF_ADD_TEXT, Color32::from_rgb(0x85, 0xe8, 0x9d));
    assert_eq!(Palette::DIFF_DEL_BG, Color32::from_rgb(0x5a, 0x3a, 0x3a));
    assert_eq!(Palette::DIFF_DEL_TEXT, Color32::from_rgb(0xff, 0x9a, 0x9a));
    // The removed accent.
    assert_eq!(
        Palette::DIFF_DEL_ACCENT,
        Color32::from_rgb(0xf7, 0x54, 0x64)
    );
    // A row's fill is its line background, so the accent is the whole
    // added/removed treatment over it and the two must stay distinguishable.
    assert_ne!(Palette::DIFF_ADD_ACCENT, Palette::DIFF_ADD_BG);
    assert_ne!(Palette::DIFF_DEL_ACCENT, Palette::DIFF_DEL_BG);
    // The focus/selection fill, which shares a src literal with the row
    // decision: untouched here on purpose (its opaque neighbour arrives with
    // the selected-row token, not with an ink change).
    assert_eq!(
        Palette::selection_bg(),
        Color32::from_rgba_premultiplied(0x0d, 0x1d, 0x3c, 0x40)
    );

    // The one that moved, and the reason: it is the added-file token.
    assert_eq!(Palette::DIFF_ADD_ACCENT, Palette::STATUS_ADDED);
    assert_ne!(
        Palette::DIFF_ADD_ACCENT,
        Color32::from_rgb(0x57, 0x96, 0x5c)
    );

    // And the move cost the diff's added line nothing: the accent reads better
    // on the diff's own (unchanged) added-line background than it did, so the
    // added line is strictly more legible after this ticket than before.
    let accent_on_add_bg = contrast(Palette::DIFF_ADD_ACCENT, Palette::DIFF_ADD_BG);
    println!("added accent on the added-line background: {accent_on_add_bg:.3}:1");
    assert!(
        accent_on_add_bg >= 3.0,
        "the moved accent must still read as a graphical object on the \
         added-line background: {accent_on_add_bg:.3}:1"
    );
    // The diff's own text token is the diff's ink, and it is not the accent —
    // a merge of the two would be the widening this ticket must not do.
    assert_ne!(Palette::DIFF_ADD_TEXT, Palette::DIFF_ADD_ACCENT);
}

// --- The selected-row fill, the raised ladder, the rail width (ticket 03) ---
//
// Three tokens, and each one is a role rather than a value: the fill a chosen
// list row takes, the second rung of the raised ladder, and the one width an
// accent rail is allowed to be. The measurements below extend the ink sweep
// above rather than re-implementing it — the contrast helper is the same one
// the ratchet uses.

/// The two rungs of the raised ladder, as data: the surface a control sits on,
/// and the raised fill that is correct *there*.
///
/// Stated as a table so "step up from your own surface" is a claim about a
/// specific pair of surfaces, not a phrase: the app background takes the first
/// rung, and a content-surface card gets the second.
const RAISED_LADDER: [(&str, Color32, Color32); 2] = [
    ("app background (BG)", Palette::BG, Palette::RAISED),
    (
        "content-surface card (CONTENT_BG)",
        Palette::CONTENT_BG,
        Palette::RAISED_ON_CARD,
    ),
];

/// The selected-row fill: one opaque value, documented as a *list row's* fill,
/// and three distinguishable things rather than three names for one pixel.
#[test]
fn the_selected_row_fill_is_one_opaque_list_row_value_and_neither_of_the_other_two_selections() {
    // The criterion first: the value, and that it is a solid fill.
    assert_eq!(Palette::ROW_SELECTED, Color32::from_rgb(0x24, 0x34, 0x56));
    assert!(
        Palette::ROW_SELECTED.is_opaque(),
        "a list row's fill is one value whatever it sits on — not a composite"
    );

    // What it is not. Each of these is a *different* selection the app already
    // had, and collapsing any pair of the three is the failure this ticket
    // exists to prevent: the current-ref band, the translucent focus composite,
    // and the commit-inclusion fill the alias used to name.
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

    // And the composite's literal, pinned here so this file says plainly that
    // the opaque fill and the translucent composite are two things. Ticket 03
    // must not move it.
    assert_eq!(
        Palette::selection_bg(),
        Color32::from_rgba_premultiplied(0x0d, 0x1d, 0x3c, 0x40)
    );

    // A visible step above the surface a list is read on — enough to see, not
    // so much that the row becomes a slab. 1.24:1, measured.
    let step = contrast(Palette::ROW_SELECTED, Palette::CONTENT_BG);
    println!("selected-row fill on the content surface: {step:.3}:1");
    assert!(
        (1.2..1.3).contains(&step),
        "the selected-row fill must read as a step above the content surface \
         without becoming a saturated fill: {step:.3}:1"
    );

    // The ink contract on the new surface, from the one measurement helper: the
    // step-up holds, and the muted step is measurably not legal — which is why
    // the fill is in RAISED_OR_SELECTED and a caller dims nothing on it.
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

    // And the reason the accent is a 2px rail at this row's leading edge and
    // never its fill: it is a graphical-object colour, not a text colour. Both
    // halves are asserted, because "it is fine as a rail" and "it is not fine
    // as a fill" are the same measurement read against two thresholds.
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

/// The ladder is strictly ordered, and the raised-on-card role is the rung the
/// rule names — asserted as arithmetic, not as the comment in `theme.rs`.
#[test]
fn the_raised_ladder_is_strictly_ordered_and_each_rung_steps_up_from_its_own_host() {
    // The whole surface ladder, bottom to top. A rung that stops stepping up
    // is a rung whose control vanishes into the surface it was dropped on, so
    // the ordering is the criterion and it is measured.
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
        // Strictly ordered, measured. This is an ordering claim and not a
        // visibility one: adjacent rungs sit close together (the app
        // background and the content surface measure 1.04:1), which is why the
        // *raised* rungs are asserted separately below rather than here.
        assert!(
            luminance(upper) > luminance(lower),
            "{upper_name} must be strictly above {lower_name} in the ladder \
             ({:.5} vs {:.5})",
            luminance(upper),
            luminance(lower)
        );
    }

    // The identity the rule names, and the two rungs' distinctness: equal-valued
    // aliases, so no value moved, but a control on a content-surface card is
    // still named differently from one on the app background.
    assert_eq!(Palette::RAISED_ON_CARD, Palette::SURFACE_2);
    assert_eq!(Palette::RAISED, Palette::SURFACE);
    assert_ne!(
        Palette::RAISED,
        Palette::RAISED_ON_CARD,
        "the two rungs are different values: one tone cannot serve both hosts"
    );

    // Per rung: the raised fill for that host really is a step up from *that*
    // host. The table is what makes "relative to the surface it actually sits
    // on" a checkable statement rather than a phrase.
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

    // The rule's own wording, pinned: raised on the app background is
    // `SURFACE`, raised on a content-surface card is the raised-on-card role.
    assert_eq!(RAISED_LADDER[0].2, Palette::SURFACE);
    assert_eq!(RAISED_LADDER[1].2, Palette::RAISED_ON_CARD);
    assert_ne!(RAISED_LADDER[1].2, Palette::SURFACE);
}

/// Every accent rail in the tree, and where its width is written *today*.
///
/// The width is one dimension — [`theme::RAIL_WIDTH`]. This table is the list
/// of painters that paint a rail, and every one of them now reaches the width
/// through the **one** rail painter, `ui::components::paint_rail`. That is what
/// makes "every rail is 2 px" checkable instead of assumed, and it is what
/// forces a *fourth* rail to be declared here rather than appearing quietly with
/// its own literal: a new entry that did not route through the one painter fails
/// on the delegation check, and a painter that grows a 2px brand stroke of its
/// own fails on the enumeration of every such stroke below.
///
/// This table previously held each painter's **literal** width expression. The
/// two pre-existing rails it listed (the sidebar's `paint_active_band` and the
/// diff's `paint_selection_bar`) are the ones ticket 07's one rail painter
/// absorbs, and both now delegate — so the width column is gone and the
/// delegation is the claim.
const RAIL_PAINTERS: [(&str, &str, &str); 2] = [
    (
        "ui/sidebar.rs",
        include_str!("../src/ui/sidebar.rs"),
        "paint_active_band",
    ),
    (
        "ui/diff/actions.rs",
        include_str!("../src/ui/diff/actions.rs"),
        "paint_selection_bar",
    ),
];

/// The one painter that turns a row's rect into a rail. The width lives here and
/// nowhere else, which is the whole content of "defined exactly once".
const RAIL_PAINTER_SRC: &str = include_str!("../src/ui/components.rs");

#[test]
fn the_rail_width_is_a_named_dimension_defined_exactly_once_in_the_token_layer() {
    // The value.
    assert_eq!(theme::RAIL_WIDTH, 2.0, "the rail width dimension");

    // "Written in exactly one place", scoped to the *rail-width dimension*
    // rather than to the bare literal `2.0`. A test that banned the literal
    // would also ban four things that are not rails: the two 2px BRAND rings
    // around a focused conflict-pane header and result cell
    // (`ui/kit/conflict_pane.rs`), the 2px dash inside the tri-state partial
    // checkbox (`ui/sidebar.rs`), and the icon stroke weight
    // (`ui/icons.rs`). Counting the *definition* is the honest version of the
    // criterion, and it needs a source scan because a definition is not a value
    // anything else can observe.
    let theme_src = include_str!("../src/theme.rs");
    assert_eq!(
        theme_src.matches("pub const RAIL_WIDTH").count(),
        1,
        "the rail width is defined in the token layer and nowhere else"
    );
    // A width, not a radius. The two 2px roles coexist, the numbers match, and
    // that coincidence is the trap: a caller that rounds a chip with a rail
    // width gets a 2px pill. Two declarations, asserted as two.
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
    // Paint, not layout: the rail is never a padding role, and the tree's
    // leading-indent role is a different number for a different job. (Static
    // property, so it is asserted in a const block.)
    const {
        assert!(
            theme::INDENT > theme::RAIL_WIDTH,
            "the leading indent is a layout role and must not be confused with \
             the rail width"
        )
    };

    // Every rail the tree paints goes through the one painter, and the one
    // painter reads the width off the token. Each painter is checked for both
    // its existence and its delegation, so a rail that quietly becomes 3px, or
    // grows a stroke of its own, fails here with the site named.
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
    // The one painter is the one reader of the width.
    assert!(
        RAIL_PAINTER_SRC.contains("pub fn paint_rail"),
        "the one rail painter is `ui::components::paint_rail`"
    );
    assert!(
        RAIL_PAINTER_SRC.contains("crate::theme::RAIL_WIDTH"),
        "the rail painter must take its width from theme::RAIL_WIDTH — that is \
         what 'defined exactly once' means at the paint site"
    );
    // And nothing paints a 2px brand stroke of its own any more: the one painter
    // is the only place a rail and the brand colour appear together.
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

// --- Construction sites: the muted ink never lands on a raised/selected fill --
//
// The narrowed contract is a *rule for callers*, and the only thing that
// catches a violation is an assertion at the site that constructs the
// (fill, ink) pair. A render seam can only prove this at the pairs a test
// enumerates, which is exactly what the table below is: each row names a
// construction site, the fill that site puts down, and the ink that site puts
// on it. Scanning source would break on a rename and pass on a stub.

/// A fill the muted ink may never be painted on: raised, hover/pressed, or
/// selected. Named once, so "raised or selected" is a set and not a phrase.
///
/// The selected list row joins the selection token here for the same reason it
/// joined [`AUDITED_SURFACES`]: it *is* a selected surface, and the muted step
/// is measurably sub-AA on it (3.75:1), so a caller painting metadata in
/// `INK_3` on a selected row breaks the step-up rule. The row shell itself steps
/// up to `INK_2`; the branch row's tracking line does not, and that call site is
/// enumerated in the report rather than fixed here.
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

/// Whether a fill is one of the selection or raised roles a caller must step up
/// to `INK_2` on. The translucent focus band is a *selected* role too — it is
/// `selection_bg()`, composited over whatever it sits on — so it belongs to the
/// rule by identity. Its composited colour is deliberately not measured here:
/// naming an opaque selected-row token is the selected-row token's own work,
/// and until then a contrast ratio against an un-composited translucent fill
/// would be a number about nothing.
fn is_a_raised_or_selected_band(fill: Color32) -> bool {
    is_raised_or_selected(fill) || fill == Palette::selection_bg()
}

/// The construction-site rule, stated once and applied to every enumerated
/// pair. Two things have to hold for a pair to be legal:
///
/// 1. the muted ink is never the ink on a raised or selected fill — that is the
///    narrowed contract, and the caller steps *up* to `INK_2` instead. This
///    half applies to *every* fill, including the solid brand and tinted-accent
///    ones, because it is a claim about the token rather than a measurement.
/// 2. the ink clears AA on the fill it is painted on — asserted for the pairs
///    whose fill is a surface the palette audits. Accent tints and solid brand
///    fills are governed by their own, already-pinned contracts, so measuring
///    them here would quietly extend this ticket's guarantee to tokens the
///    migration has deliberately not swept.
///
/// Rule 1 alone would accept any bright colour; rule 2 alone is the surface
/// sweep's job and says nothing about which construction sites exist.
fn assert_ink_legal_on_fill(site: &str, what: &str, fill: Color32, ink: Color32) {
    if is_a_raised_or_selected_band(fill) {
        assert_ne!(
            ink,
            Palette::INK_3,
            "{site}: {what} paints the muted ink on a raised/selected fill \
             ({fill:?}). INK_3 is not legal there — step up to INK_2."
        );
    }
    // A transparent fill means "whatever the host surface is", which the
    // construction site does not own; it is skipped here so the rule is not
    // silently weakened into a no-op, and the caller asserts the ink against
    // the host's resting surface instead.
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

/// Site 1 — the row-fill decision, and site 2 — the row shell's ink. The two
/// are pinned together because the shell's name ink is what actually lands on
/// the band the fill decision put down, and the shell's *dim* role is a
/// resting-row role: it is legal on the content surface a resting row sits on
/// and nowhere else.
#[test]
fn the_row_fill_decision_and_the_row_shell_never_put_the_muted_ink_on_a_raised_or_selected_band() {
    use turbogit_ui::ui::components::{RowState, current_row_fill, row_fill, row_ink};

    // The bands are a *closed* set, pinned by identity rather than by
    // "recognised as one of ours": a row shell that grows a new band has to say
    // here whether the muted ink is legal on it.
    let brand_rest = turbogit_ui::ui::widgets::tint_over_bg(Palette::BRAND, 0.16);
    let brand_hover = turbogit_ui::ui::widgets::tint_over_bg(Palette::BRAND, 0.24);
    assert_eq!(row_fill(RowState::Default), Color32::TRANSPARENT);
    assert_eq!(row_fill(RowState::Hover), Palette::RAISED_ON_CARD);
    assert_eq!(row_fill(RowState::RowSelected), Palette::ROW_SELECTED);
    assert_eq!(row_fill(RowState::FocusSelected), Palette::selection_bg());
    assert_eq!(current_row_fill(RowState::Default), brand_rest);
    assert_eq!(current_row_fill(RowState::Hover), brand_hover);
    assert_eq!(current_row_fill(RowState::RowSelected), Palette::SELECTION);

    // The shell's ink is the same for every band — the selected list row
    // included, which is the criterion this ticket adds: selection does not
    // invert ink. The solid brand band that used to be a row state (with
    // `BRAND_INK` on it) is gone, so no row band is an inverting one any more.
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
        // Every band carries the row's own name ink, and that ink has to read on
        // it. The solid brand band that had to be skipped here (it is not a
        // surface, it is a fill, and it inverted to `BRAND_INK`) is gone: the
        // list row's selection is a surface now, so there is no exception left
        // to make and no band a row can be in that is not readable.
        assert_eq!(ink, name_ink, "row band {name} wears the row's own ink");
        let ratio = contrast(ink, fill);
        assert!(
            ratio >= 4.5,
            "row shell: a row on the {name} band ({fill:?}) must read clearly: \
             {ratio:.3}:1"
        );
    }

    // The shell's *dim* role — a stale row never hides (§14.1) — is legal on
    // the resting row's surface and only there. A dimmed row that is also
    // hovered or selected is a call-site concern, not this token's: the rule
    // is that whoever paints a dimmed row on a band steps up to `INK_2`.
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
    // The negative, as a negative: the dim step is exactly the ink the
    // raised/selected rule forbids, so this table is what would have to change
    // for a dimmed row to be painted on a band.
    assert!(
        !is_raised_or_selected(Palette::CONTENT_BG),
        "a resting row's surface must not itself be a raised/selected surface, \
         or the dim role would be illegal everywhere"
    );
}

/// Site 2 — the chip constructors. A chip is a filled shape with text on it,
/// so its ink is chosen in the same breath as its fill and is exactly where a
/// muted-ink-on-a-raised-fill drift would land.
///
/// The vocabulary added by ticket 05 is swept here by the same helper and the
/// same rule, which is the point: a fourth chip in the set, or a chip whose ink
/// is retuned, is caught by the row that already caught the neutral badge —
/// there is no second, weaker copy of this rule to keep in step.
#[test]
fn the_chip_constructors_never_put_the_muted_ink_on_a_raised_or_selected_fill() {
    use turbogit_ui::ui::widgets::{
        BadgeKind, ButtonVariant, COUNT_CHIP_COLORS, CURRENT_CHIP_COLORS, REF_CHIP_COLORS,
        WidgetState,
    };

    // The closed three-chip set, in the order the vocabulary names them. Every
    // fill is a *raised* or *selected* surface, so every one of these rows is
    // the step-up case and not the resting-content case.
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
    // The two new fills, spelled out. Both were measured in ticket 03; saying
    // them here keeps the choice of *ink* next to the choice of *fill*.
    assert_eq!(REF_CHIP_COLORS.bg, Palette::RAISED_ON_CARD);
    assert_eq!(REF_CHIP_COLORS.fg, Palette::INK_2);
    assert_eq!(CURRENT_CHIP_COLORS.bg, Palette::ROW_SELECTED);
    assert_eq!(CURRENT_CHIP_COLORS.fg, Palette::ACCENT_TEXT);
    assert_eq!(COUNT_CHIP_COLORS.bg, Palette::RAISED);
    assert_eq!(COUNT_CHIP_COLORS.fg, Palette::INK_2);
    // The muted step is the exact ink the rule forbids on both of these, so the
    // mutation "one word in the constructor" is caught here rather than by
    // reading a comment. (The numbers: `INK_3` measures 3.81:1 on the ref chip's
    // fill and 3.75:1 on the current chip's, both sub-AA.)
    assert!(
        !is_raised_or_selected(Palette::INK_3),
        "INK_3 is the ink this rule exists to keep off a raised/selected fill"
    );

    // The badge family: every kind states its own (fill, ink) pair.
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
    // The neutral badge is the one chip that rests on a raised surface, and it
    // steps *up* to INK_2 rather than reaching for the muted step. This is the
    // concrete shape of the rule, and the mutation that breaks it is a one-word
    // change in the constructor.
    let neutral = BadgeKind::Neutral.colors();
    assert_eq!(neutral.bg, Palette::SURFACE_3);
    assert_eq!(neutral.fg, Palette::INK_2);
    assert!(
        !is_raised_or_selected(Palette::INK_2),
        "the step-up ink is not itself a surface"
    );

    // The button family: ghost/compact/icon share one ladder, so pin the shared
    // one once and the primary separately (it wears brand ink on a brand fill).
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
    // The disabled state is the one that reaches for the muted step, and it is
    // only safe because a disabled ghost keeps the *host's* surface (a
    // transparent fill) rather than taking a raised one. Pinned so "disabled
    // dims" can never quietly become "disabled dims onto a hover fill".
    assert_eq!(
        ButtonVariant::Ghost.fill(WidgetState::Disabled),
        Color32::TRANSPARENT
    );
    assert_eq!(
        ButtonVariant::Ghost.text(WidgetState::Disabled),
        Palette::INK_3
    );
    // The primary disabled keeps its solid brand fill, so the muted step lands
    // on a brand fill rather than a raised one. Whether that pairing is
    // readable is a `BRAND` question and this ticket does not sweep `BRAND` —
    // what is asserted here is only that the pair is not a raised/selected
    // surface, which is the rule this site is enumerated for.
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

/// --- The three-chip vocabulary's own rows (ticket 05) -----------------------
//
// Two claims that are the chip set's rather than the badge family's, and both
// are measurements rather than a comparison against a literal:
//
// 1. **Ink is legible on the two new fills.** The ref chip's fill
//    (`RAISED_ON_CARD` = `#313438`) and the current chip's (`ROW_SELECTED` =
//    `#243456`) are new pairings, and a new pairing is exactly where a
//    "token-legal, still unreadable" ink hides. Both clear AA, and the numbers
//    are printed so the record is the measurement rather than a disputed claim.
// 2. **The current chip is the only chip allowed near the brand.** The negative
//    is asserted at the constructors, because that is the only seam that can
//    prove one: a render test sees the chips a given frame happened to paint.
//
// The ref chip and the current chip both rest on a *new* fill, so both inks
// are checked as arithmetic rather than assumed from the ramp: 5.96:1 and
// 5.90:1 respectively, against a 4.5 floor.
#[test]
fn the_two_new_chip_fills_carry_ink_that_clears_aa() {
    use turbogit_ui::ui::widgets::{COUNT_CHIP_COLORS, CURRENT_CHIP_COLORS, REF_CHIP_COLORS};

    // The floor this suite measures everything against, stated once so a
    // failure names the number it missed rather than a symbol.
    const AA: f64 = 4.5;

    // The ref chip: a raised-on-card fill, and the step-up ink. The muted step
    // is measured alongside it rather than left as a claim, because "not legal
    // there" is only a real rule if the number is actually sub-AA.
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

    // The current chip: the selected-row fill with the readable accent ink. The
    // accent itself is the one ink that is *not* a ramp step, so it carries its
    // own coverage and is measured here on the fill it is actually used on.
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

    // The count chip, for completeness: the plain raised surface with the same
    // secondary ink. It is not one of the two *new* pairings, but it is a chip
    // and a chip that fails to read is not a vocabulary.
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

/// The current chip is permitted to carry the accent because "this is the
/// current ref" is a fact about the ref rather than a call to action. The
/// permission is only meaningful if nothing else in the set has it, so the
/// negative is stated here: **no chip fills the brand token**, and exactly one
/// wears an accent.
///
/// The mutation this exists to catch is the current chip's own fill becoming
/// `BRAND` — the branches screen's blue soup, arrived at through a chip that
/// somebody reused because it was right there.
#[test]
fn the_current_chip_is_the_only_chip_allowed_to_carry_the_accent() {
    use turbogit_ui::ui::widgets::{COUNT_CHIP_COLORS, CURRENT_CHIP_COLORS, REF_CHIP_COLORS};

    // `ACCENT` and `BRAND` are one colour under two names, and both are named
    // in the token layer for branch chips — so asserting the alias first is
    // what lets one inequality cover both spellings.
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
    // The permission itself: the accent *ink* belongs to the current chip and
    // to no other, which is the form the current chip actually ships in.
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
    // And nothing in the set reaches for the *solid* brand as ink either: the
    // on-brand ink is `WHITE`, and the solid brand fill it was built for is
    // gone, so a chip wearing it would be painting the inversion of a button
    // on top of a button's own colour.
    for (who, colors) in chips {
        assert_ne!(
            colors.fg,
            Palette::BRAND_INK,
            "the {who} must not wear the on-brand ink: there is no solid brand \
             fill left for it to sit on"
        );
    }
    // Semantic state is not a chip, and this is where that rule is measured at
    // the token level: none of the three fills or inks is a repository-state
    // colour. A state behind a fill stops reading as a state and starts reading
    // as a category, and this is the specific mistake the branches screen made
    // twice. The reserved counter orange is in the same list — it means dirt and
    // unpushed, and the count chip is the thing that stops a third number
    // borrowing it.
    //
    // **The muted ink is in the list too, and that is the edit this variant
    // forced.** The list above used to be the whole map by coincidence — six
    // states, three colours — and a coincidence is not a ratchet: the moment
    // `RepoState` grew an arm whose colour is none of the three tones, the loop
    // would have kept passing while checking a colour the map no longer
    // produces, and a chip that put the muted ink behind a status would have
    // walked straight through it. So the list is now derived from the map itself
    // rather than transcribed, which is the only form in which "every colour the
    // map can produce" is actually the claim.
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
    // …and the list really is the whole map: every state the one map can produce
    // resolves to a colour in it. Without this the list could drift from the map
    // again the same way it did the first time, and the loop above would be
    // checking a set the map no longer produces.
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
    // One status→colour map: dots, smart groups, badges and headers agree.
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
    // The same value that colours the dot names the state in words, so the two
    // can never disagree (§13; wording reused from the sidebar's smart groups).
    assert_eq!(RepoState::Clean.words(), "in sync");
    assert_eq!(RepoState::Dirty.words(), "dirty worktree");
    assert_eq!(RepoState::Conflict.words(), "has conflicts");
    assert_eq!(RepoState::Diverged.words(), "diverged");
    assert_eq!(RepoState::Unpushed.words(), "unpushed commits");
    assert_eq!(RepoState::Unpulled.words(), "unpulled commits");
    // Sync badge vocabulary matches the same map.
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
    // Status-bar counters use the same family (diverged red, dirty/unpulled orange).
    assert_eq!(RepoState::Diverged.color(), Palette::STATUS_DIVERGED);
    assert_eq!(RepoState::Unpulled.color(), Palette::COUNTER);

    // A submodule with no working copy is not a repository state this map already
    // had: it is not clean, not a conflict and not a divergence, and none of those
    // is a severity it should wear. It is work the user is owed.
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

// --- C3/C4: one authoritative text hierarchy and selection model (ticket 04) ---

#[test]
fn one_authoritative_primary_ink_across_shell_and_tool_windows() {
    // C3: the shell default text and the §13 body text are ONE primary role —
    // no competing INK-vs-T_PRIMARY definitions for the same role.
    assert_eq!(
        Palette::INK,
        Palette::T_PRIMARY,
        "IK must be an alias of the authoritative primary"
    );
    // The secondary/muted levels also keep their single-role aliases.
    assert_eq!(Palette::INK_2, Palette::T_SECONDARY);
    assert_eq!(Palette::INK_3, Palette::T_MUTED);
    assert_eq!(Palette::INK_4, Palette::T_DIM);

    // The four levels are distinct tokens so hierarchy is never accidental.
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
    // The hierarchy is explicit, and each step carries its own legal surface
    // set rather than one blanket promise. Before this ticket the muted level
    // was promised ≥4.5:1 on every audited surface; it is now promised that on
    // the app, panel and content surfaces only, and the selected surface is
    // explicitly a caller's cue to step *up* to `INK_2`.
    let ctx = egui::Context::default();
    configure_style(&ctx);

    assert_eq!(Palette::T_SECONDARY, Palette::INK_2);
    assert_eq!(Palette::T_MUTED, Palette::INK_3);

    // `INK_2` keeps the promise the muted step used to carry: it clears AA on
    // the darkest audited surface (SELECTION, whose commit-inclusion alias
    // shares this value), so it is the step-up ink the rule names.
    let ratio = contrast(Palette::T_SECONDARY, Palette::SELECTION);
    println!("T_SECONDARY on selection: {ratio:.3}:1");
    assert!(
        ratio >= 4.5,
        "INK_2 on selection must stay ≥4.5:1, got {ratio:.3}:1"
    );

    // The muted level is NOT promised there any more, and the promise is now a
    // measured negative rather than an absent one.
    let muted = contrast(Palette::T_MUTED, Palette::SELECTION);
    println!("T_MUTED on selection: {muted:.3}:1 (not legal — step up to INK_2)");
    assert!(
        muted < 4.5,
        "T_MUTED is not legal on the selection fill; if this has become \
         readable, the step-up rule in theme.rs is stale and must be rewritten: \
         {muted:.3}:1"
    );
    // ... but it still clears AA on the surfaces it is legal on, or the whole
    // narrowed contract is worthless.
    let content = contrast(Palette::T_MUTED, Palette::CONTENT_BG);
    assert!(
        content >= 4.5,
        "T_MUTED on the content surface must still clear AA: {content:.3}:1"
    );
}

#[test]
fn selection_uses_one_canonical_opaque_fill() {
    // C4: commit inclusion and navigation selection share ONE opaque fill —
    // the two near-duplicate literals (#2E4369 / #2E436E) collapsed into the
    // canonical SELECTION token, and the alias that spelled it a second way
    // went with them (ticket 08). The translucent focus fill stays separate
    // and is documented against its compositing background (ticket 03).
    assert!(
        Palette::SELECTION.is_opaque(),
        "the canonical opaque selection fill is solid, not a composite"
    );
    // The translucent focus treatment is a distinct, documented variant.
    assert_ne!(
        Palette::SELECTION,
        Palette::selection_bg(),
        "opaque selection must stay distinct from the translucent focus fill"
    );
}

// --- Cycle 2: dark-only Visuals derive from the central token set (spec §2.5) ---
use egui::Color32;
use turbogit_ui::theme::{self, Palette, configure_style};

const BG: Color32 = Color32::from_rgb(0x1e, 0x1f, 0x22);
const SURFACE: Color32 = Color32::from_rgb(0x2b, 0x2d, 0x30);
const SURFACE_2: Color32 = Color32::from_rgb(0x31, 0x34, 0x38);
const SURFACE_3: Color32 = Color32::from_rgb(0x3c, 0x3f, 0x41);
const INK: Color32 = Color32::from_rgb(0xdf, 0xe1, 0xe5); // unified primary (C3)
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
    // R2's positive half. "A 1px stroke means *this floats*" is only a rule if
    // the floating surfaces actually wear one, and the one every popup, dialog,
    // menu and toast inherits is egui's `visuals.window_stroke` — set on the
    // style, not painted from a module, which is why it is pinned here with the
    // rest of the token contract and why the stroke-site table in
    // `tests/widget_library.rs` (which walks `src/ui` and so cannot see it) names
    // this as the other half of its claim.
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
    // It is a *hairline* and not a box: the same value the content divider would
    // have been before R2 narrowed the divider to a fill, and not the footer
    // rule's stronger tone.
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
    // BRAND at ~25% premultiplied alpha: 0.25 * channel.
    assert_eq!(
        sel.bg_fill,
        Color32::from_rgba_premultiplied(0x0d, 0x1d, 0x3c, 0x40),
        "selection.bg_fill"
    );
}

// --- Cycle 3: embedded JetBrains Mono with system fallbacks (ADR-0002, spec §3.1) ---

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

    // The bold weight is really embedded (non-empty TrueType data).
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

// --- Cycle 4: the app installs the stack into its context (ADR-0002) ---

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

/// The chrome/data typeface split has to be physically real, not nominal: both
/// seams resolve to their own face. Measured on glyph advances through the
/// public [`theme::chrome_font`] / [`theme::data_font`] seam rather than on a
/// registered family name, because a family label can name the wrong face.
#[test]
fn chrome_seam_is_proportional_while_the_data_seam_is_monospaced() {
    let ctx = egui::Context::default();
    install_fonts(&ctx);

    // Font definitions take effect at the next pass begin.
    let mut full = ctx.run_ui(egui::RawInput::default(), |_ui| {});
    full.textures_delta.clear();

    let chrome = theme::chrome_font(12.0);
    let data = theme::data_font(12.0);
    // Every character the Branches view paints in chrome type. Chrome text
    // moved onto a face this suite never relied on for Latin, so a gap here is
    // tofu on screen rather than a wrong number.
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

// --- Cycle 5: accent, risk, and status semantics (issue #01) ---
#[test]
fn accent_accessor_returns_brand() {
    assert_eq!(theme::accent(), Palette::BRAND, "accent() must alias BRAND");
}

#[test]
fn the_diverged_status_names_the_error_red_and_the_other_statuses_do_not_borrow_a_name() {
    use turbogit_ui::theme::RepoState;

    // Diverged is the one status that needed a second name: it shares the error
    // red with a conflict, and three different surfaces say "diverged" through
    // it. So the alias stays and is asserted here.
    assert_eq!(Palette::STATUS_DIVERGED, Palette::STATE_ERROR);
    assert_ne!(Palette::STATUS_DIVERGED, Palette::STATE_SUCCESS);
    assert_ne!(Palette::STATUS_DIVERGED, Palette::STATE_WARNING);
    assert_ne!(Palette::STATUS_DIVERGED, Palette::STATE_INFO);

    // And the map it belongs to still decides every status colour, so "no
    // status ever falls back" is a claim about one function rather than four
    // aliases nothing reads. The three states with no alias route through the
    // ahead green and the reserved counter orange, deliberately. The seventh is
    // the one that is not a severity at all and wears the muted ink for that
    // reason — see `p0_repo_state_shared_mapping_is_app_wide`, which pins it.
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

// --- Cycle 6: Local Changes redesign token set (issue 01, ticket 01) ---
//
// Purely additive: every token below is new and nothing existing re-renders
// differently — later tickets opt in by switching call sites. Sources:
// `docs/ui-local-changes-visual-changes.md` §7–8 and the redesign mockup SVG.

#[test]
fn the_sidebar_surface_is_a_dedicated_painted_fill_and_not_an_alias() {
    // The redesign gives the sidebar rail its own surface (#1B1C1E), darker
    // than the app background and every raised fill. It is a step, not a
    // different colour: 1.03:1 against `BG`, which is what makes the left rail
    // read as a surface the content is *beside*.
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
    // Every raised fill inside the rail still steps *up* from it, so the
    // `RAISED_ON_CARD` ladder rule holds against the rail too — this is the
    // claim the token only earns once the rail actually paints it.
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
    // The window/panel tokens themselves stay untouched by the redesign
    // (they already matched the design doc §7 surfaces).
    assert_eq!(Palette::BG, Color32::from_rgb(0x1e, 0x1f, 0x22));
    assert_eq!(Palette::SURFACE, Color32::from_rgb(0x2b, 0x2d, 0x30));
}

#[test]
fn the_selection_fill_is_solid_and_the_focus_composite_is_not_it() {
    // C4: the commit-inclusion fill is the canonical opaque SELECTION token
    // (their previously near-duplicate #2E4369/#2E436E values are unified),
    // and it never aliases the translucent shell focus fill.
    assert!(
        Palette::SELECTION.is_opaque(),
        "selection fill is solid, not a composite"
    );

    // The translucent focus treatment stays a distinct, documented variant
    // composited over the surface it sits on (ticket 03).
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
    // File rows (issue 05) colour the status letter and the filename by state:
    // M modified blue, A added green, U unversioned olive — values straight
    // from the redesign mockup.
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

    // The three states are pairwise distinct so a row is never ambiguous
    // about which status letter it carries.
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
    // The diff preview (issue 06) paints added/deleted lines as accent color
    // over the tinted background pair — values straight from the design doc §7.
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

    // Added shares its green with the added-file status letter — one hue for
    // 'added' across rows and diff.
    assert_eq!(Palette::DIFF_ADD_ACCENT, Palette::STATUS_ADDED);
    // Accent and background of each pair are distinct, and the two pairs never
    // cross: a row's fill is its line background, so the accent is the whole
    // added/removed treatment and has to be readable on top of it.
    assert_ne!(Palette::DIFF_ADD_ACCENT, Palette::DIFF_ADD_BG);
    assert_ne!(Palette::DIFF_DEL_ACCENT, Palette::DIFF_DEL_BG);
    assert_ne!(Palette::DIFF_ADD_ACCENT, Palette::DIFF_DEL_ACCENT);
    assert_ne!(Palette::DIFF_ADD_BG, Palette::DIFF_DEL_BG);

    // Purely additive: the legacy diff view tokens are untouched — the
    // redesign pair is a separate token set, not a re-paint of DIFF_*_BG/TEXT.
    assert_ne!(Palette::DIFF_ADD_ACCENT, Palette::DIFF_ADD_TEXT);
    assert_ne!(Palette::DIFF_DEL_ACCENT, Palette::DIFF_DEL_TEXT);
}

#[test]
fn counter_token_is_reserved_orange_distinct_from_every_accent() {
    // #E0883C paints dirt/unpulled counters only (design doc §7) — it is
    // semantically reserved, never intended as a general accent or warning.
    assert_eq!(
        Palette::COUNTER,
        Color32::from_rgb(0xe0, 0x88, 0x3c),
        "counter orange must match the design"
    );
    assert!(
        Palette::COUNTER.is_opaque(),
        "counter orange is a solid fill"
    );

    // Reserved: it must not alias the brand accent, the generic warning
    // orange, or the error red — so a counter can never be confused with an
    // action, a warning state, or a diverged state.
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
    // Design doc §8: one consistent row height per row kind, gaps in multiples
    // of 4, panel padding 12–14 px.
    assert_eq!(theme::FILE_ROW_HEIGHT, 24.0, "file-row height");
    assert_eq!(theme::GROUP_ROW_HEIGHT, 26.0, "group-row height");
    assert_eq!(theme::PANEL_PADDING, 12.0, "panel padding (12–14 px scale)");

    // The 4 px grid covers the *gaps and the padding*, and is a property of
    // those values rather than a token of its own. It never covered the row
    // heights: `GROUP_ROW_HEIGHT` is 26 px, a file row's group label sitting a
    // hair taller than the rows it heads. The grid unit that used to be named
    // here — and whose doc comment claimed it described row heights — went with
    // the sweep, and with it the claim it was quietly making false.
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

// --- S1/S2: named density and spacing roles (ticket 06) ---------------------

#[test]
fn shared_spacing_roles_match_the_configured_defaults() {
    // S1: the shell's configured defaults are the shared roles — one source
    // for item gaps, window margins, control padding and indentation; the
    // false "every gap is a multiple of 4" claim is superseded here.
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
    // S2: compact shell controls and the dense status bar are named roles,
    // not regional literals — central defaults plus explicit exceptions.
    assert_eq!(theme::DENSITY_COMPACT_BUTTON, egui::vec2(8.0, 4.0));
    assert_eq!(theme::DENSITY_DENSE_BUTTON, egui::vec2(6.0, 2.0));
    // Compact/dense are genuinely tighter than the default control padding
    // (static property — the values are compile-time constants).
    const { assert!(theme::DENSITY_COMPACT_BUTTON.y < theme::BUTTON_PADDING.y) };
    const { assert!(theme::DENSITY_DENSE_BUTTON.y < theme::DENSITY_COMPACT_BUTTON.y) };
    // The default window radius stays distinct from chip/control radii
    // (S3 guards chip shapes, not window chrome).
    assert_ne!(theme::WINDOW_RADIUS, theme::CHIP_RADIUS);
}

#[test]
fn chip_radii_are_shared_variants_not_local_contracts() {
    // S3: compact chips and the welcome pill are explicit shared shape
    // variants — the pill radius equals the welcome chip's half-height, and
    // chip/control radii stay distinct from window/menu surface roles.
    assert_eq!(theme::CHIP_RADIUS, 3);
    assert_eq!(theme::CONTROL_RADIUS, 4);
    assert_eq!(theme::PILL_RADIUS, 9);
    assert_ne!(theme::CHIP_RADIUS, theme::PILL_RADIUS);
    assert_ne!(theme::CONTROL_RADIUS, theme::PILL_RADIUS);
}

// --- G1: the consolidated role contract agrees with its consumers (ticket 07) --

#[test]
fn governance_consolidated_role_contract_is_internally_consistent() {
    // One pass over the whole reconciled contract (docs/design-system-roles.md):
    // each shared role resolves through exactly one authoritative source and
    // stays distinct where semantics differ.
    // C3 — one primary ink, a real four-step ramp below it, distinct levels.
    assert_eq!(Palette::INK, Palette::T_PRIMARY);
    assert_eq!(Palette::INK_2, Palette::T_SECONDARY);
    assert_eq!(Palette::INK_3, Palette::T_MUTED);
    assert_eq!(Palette::INK_4, Palette::T_DIM);
    assert_ne!(Palette::T_SECONDARY, Palette::T_MUTED);
    assert_ne!(Palette::T_MUTED, Palette::T_DIM);
    // C4 — one opaque selection fill; translucent focus is a separate variant.
    assert!(Palette::SELECTION.is_opaque());
    assert_ne!(Palette::SELECTION, Palette::selection_bg());
    // C5 — the 1px separator reuses the raised surface, not the LINE border.
    assert_eq!(Palette::RULE_CONTENT, Palette::RAISED);
    assert_ne!(Palette::RULE_CONTENT, Palette::LINE);
    // S3 — chip/control radii are shared variants, distinct from window/menu.
    assert_ne!(theme::WINDOW_RADIUS, theme::CHIP_RADIUS);
    // S2 — density roles are genuinely tighter than the default control
    // (static property: the values are compile-time constants).
    const { assert!(theme::DENSITY_DENSE_BUTTON.y < theme::DENSITY_COMPACT_BUTTON.y) };
    // Severity family keeps its three meanings distinct (C2 semantics).
    assert_ne!(Palette::STATE_INFO, Palette::STATE_WARNING);
    assert_ne!(Palette::STATE_WARNING, Palette::STATE_ERROR);
}

// --- H1/H2/H3: the three hairline roles are named (ticket 09) ----------------

/// Relative luminance (WCAG) — the measure that says which of two greys reads
/// as the *stronger* line against a dark surface.
fn luminance(color: Color32) -> f64 {
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

/// WCAG contrast ratio between two opaque colours. The one place this file
/// computes contrast — the two nested copies that used to sit inside
/// individual tests are gone, so a change to the maths lands in one place.
fn contrast(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[test]
fn the_three_hairline_roles_are_named_and_resolve_to_their_tones() {
    // H1: a 1px line is picked by the ROLE it plays, not by whichever token
    // happened to be nearest. Three roles, three names.
    //
    // H3: the content divider is the weakest of the three — it separates
    // sibling content regions inside one surface without bounding them. It
    // stays an alias of the raised surface, so the C5 contract reads
    // `RULE_CONTENT == RAISED`, `RULE_CONTENT != LINE`. The second name for
    // that same value (`DIVIDER`) went with the sweep; one tone, one spelling.
    assert_eq!(Palette::RULE_CONTENT, Palette::RAISED);
    assert_ne!(Palette::RULE_CONTENT, Palette::LINE);

    // H2: the footer rule separates a modal body from its action slot, and an
    // action slot is a *boundary* rather than a division — so it is
    // deliberately the stronger of the two. This is intent, not drift, and the
    // only thing keeping it that way is that the two tones stay distinct.
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

    // The structural rule — table header underlines, tree indent guides, panel
    // edge rules: the chrome that gives a surface its shape — is its own third
    // tone. It is never a stand-in for the content divider, and it is the
    // weakest-adjacent of the two chrome roles, not a fourth name for `LINE`.
    assert_eq!(Palette::RULE_STRUCTURAL, Palette::LINE_SUBTLE);
    assert_ne!(Palette::RULE_STRUCTURAL, Palette::RULE_CONTENT);
    assert_ne!(Palette::RULE_STRUCTURAL, Palette::RULE_FOOTER);

    // Purely additive naming: the three roles are aliases, so no existing token
    // value moved and the pre-existing assertions above are unaffected.
    assert_eq!(Palette::LINE, Color32::from_rgb(0x4e, 0x51, 0x57));
    assert_eq!(Palette::LINE_SUBTLE, Color32::from_rgb(0x36, 0x38, 0x3c));
    assert_eq!(Palette::RULE_CONTENT, Color32::from_rgb(0x2b, 0x2d, 0x30));

    // All three roles are distinct tones, ordered by how hard each one reads.
    // The footer rule is the strongest because a boundary reads harder than a
    // division; the structural rule sits between them, above the content
    // divider it must never be confused with. Asserting the order is what stops
    // a re-point from quietly collapsing the ladder into two steps.
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

// --- The token inventory, held closed (ticket 08) ----------------------------
//
// The sweep's other half. A palette of constants cannot be walked by
// reflection from a test, so "every token has a consumer outside the test tree"
// is a review-gated fact about the tree, not something a suite can prove — and
// this file does not pretend otherwise. What *is* mechanical, and what this
// section holds, is the inventory itself: every token `theme.rs` declares,
// written out as one explicit list. Adding a token is then a deliberate edit in
// two places rather than a declaration that appears and quietly has no caller.
//
// The closure ratchet below reads `theme.rs` and compares its declared `pub
// const` names against the list. That is a check on *declaration*, never on
// consumers, and it is deliberately the weaker of the two claims.

/// Every token `crates/turbogit-ui/src/theme.rs` declares, in declaration
/// order: the `Palette` colour consts, then the module-level dimension, shape,
/// tint and type roles, then the two embedded font blobs.
///
/// Nothing here is checked for a consumer by this suite — see above. What the
/// list buys is that a token cannot be *added* silently, and that a token which
/// the sweep deleted is gone from the palette and from this list in the same
/// change rather than surviving on the strength of an assertion.
const TOKEN_INVENTORY: &[&str] = &[
    // --- Palette: surfaces & lines
    "BG",
    "SURFACE",
    "SURFACE_2",
    "SURFACE_3",
    "SURFACE_WARNING",
    "LINE",
    "LINE_SUBTLE",
    // --- Palette: the ink ramp
    "INK",
    "INK_2",
    "INK_3",
    "INK_4",
    // --- Palette: brand
    "BRAND",
    "BRAND_INK",
    // --- Palette: status
    "STATE_SUCCESS",
    "STATE_WARNING",
    "STATE_ERROR",
    "STATE_INFO",
    "STATUS_DIVERGED",
    // --- Palette: §13 surface / selection vocabulary
    "CONTENT_BG",
    "RAISED",
    "RAISED_ON_CARD",
    "SELECTION",
    "ROW_SELECTED",
    "SECTION_BG",
    // --- Palette: the three hairline roles
    "RULE_CONTENT",
    "RULE_FOOTER",
    "RULE_STRUCTURAL",
    // --- Palette: meaning colours
    "ACCENT",
    "AHEAD",
    "DANGER",
    "LINK",
    // --- Palette: the text ramp
    "T_PRIMARY",
    "T_SECONDARY",
    "T_MUTED",
    "T_DIM",
    "ACCENT_TEXT",
    // --- Palette: reserved + status letters
    "COUNTER",
    "STATUS_MODIFIED",
    "STATUS_ADDED",
    "STATUS_UNVERSIONED",
    // --- Palette: diff
    "DIFF_ADD_BG",
    "DIFF_ADD_TEXT",
    "DIFF_DEL_BG",
    "DIFF_DEL_TEXT",
    "DIFF_ADD_ACCENT",
    "DIFF_DEL_ACCENT",
    // --- Palette: the left rail's own surface
    "SIDEBAR",
    // --- Module-level: tint fractions
    "SECTION_TINT",
    "MARKER_TINT",
    // --- Module-level: spacing
    "FILE_ROW_HEIGHT",
    "GROUP_ROW_HEIGHT",
    "PANEL_PADDING",
    "ITEM_SPACING",
    "WINDOW_MARGIN",
    "BUTTON_PADDING",
    "INDENT",
    "DENSITY_COMPACT_BUTTON",
    "DENSITY_DENSE_BUTTON",
    // --- Module-level: shape
    "CHIP_RADIUS",
    "CONTROL_RADIUS",
    "WINDOW_RADIUS",
    "MENU_RADIUS",
    "CARD_RADIUS",
    "PILL_RADIUS",
    "MARK_RADIUS",
    // --- Module-level: accents & marks
    "CASCADE_ACCENT",
    "RAIL_WIDTH",
    // --- Module-level: type ramp
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
    // --- Module-level: embedded font binaries
    "JETBRAINS_MONO_REGULAR",
    "JETBRAINS_MONO_BOLD",
];

/// Every `pub const NAME` declared in `theme.rs`, in source order.
fn declared_theme_tokens() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/theme.rs");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut out = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        // A comment is not a declaration, and no `pub const` in this file is
        // written across lines, so the leading token is the whole name.
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

/// The inventory is closed over what `theme.rs` declares: nothing is declared
/// without being inventoried, and nothing is inventoried without existing.
///
/// The failing direction that matters is the first. A token that arrives without
/// a consumer lands here rather than in a comment — the exact failure this
/// ticket exists to end — and the message says so by name.
#[test]
fn the_token_inventory_is_closed_over_the_palette() {
    let declared = declared_theme_tokens();

    // No token is declared twice under one name, in the file or the list.
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

    // The count is pinned so the two lists cannot quietly agree while shrinking.
    assert_eq!(
        declared.len(),
        TOKEN_INVENTORY.len(),
        "the inventory holds {} tokens and theme.rs declares {}",
        TOKEN_INVENTORY.len(),
        declared.len()
    );
}

/// The sweep's *other* half, stated as the one thing the suite will not
/// pretend: a token's having a caller is a fact about the tree a reviewer
/// checks, and this file deliberately does not scan `src/` for one.
///
/// The reason is not effort. A scan that looks for a token's name passes just
/// as happily on a call site that does nothing, fails on a rename, and says
/// nothing about whether the call site *paints* anything — so it would convert
/// a review-gated fact into a green tick, which is the trade this migration
/// refuses. The inventory above is the part a suite can honestly own.
#[test]
fn consumer_presence_is_review_gated_and_this_suite_does_not_scan_for_it() {
    assert_eq!(
        declared_theme_tokens().len(),
        TOKEN_INVENTORY.len(),
        "the inventory is the suite's half of the sweep; the consumer half is \
         reviewed against it, and adding a token without a caller is the one \
         failure this suite reports by name"
    );
    // Named here so the standing exceptions are stated where the inventory is,
    // not only in `theme.rs`. Two tokens in the list have no `src` consumer
    // today, both kept deliberately and both owed a caller:
    // `INK_4`/`T_DIM` is the fourth ramp step the legality rule is written in
    // terms of, and `RULE_CONTENT` is the third rung of an ordered hairline
    // ladder whose other two are named and used.
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

// --- The diff pane's own colour pair, and the pane that paints it (ticket 12,
//     moved here by ticket 21) -------------------------------------------------
//
// This section used to live in `tests/diff_viewer.rs`, which is where the diff's
// behaviour is tested and not where the token contract is. It is here for the
// same reason every other section in this file is: **the diff's six values are
// tokens**, they are pinned by hex, their two text-on-band pairs are measured
// against AA with the one [`contrast`] helper above, and a suite that pins a
// palette without pinning what the palette does to a pane is half a contract.
//
// Nothing is weakened by the move. The values, the two contrast floors and the
// three painted assertions all came with it; what changed is that the harness
// they need (a real repository, a real preview) now lives beside the numbers it
// is measuring, and the diff suite keeps its own coverage where it belongs.

/// The repository the painted half renders: one file with a staged edit on line 2
/// and an unstaged edit on line 8, far enough apart that the *Local* comparison's
/// diff carries the unstaged one without the staged edit leaking into the
/// context around it.
fn diff_fixture_repo() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .expect("git invocation");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);
    let file = repo.join("file.txt");
    std::fs::write(
        &file,
        "alpha\nbeta\ndelta\nepsilon\nzeta\neta\ntheta\ngamma\niota\n",
    )
    .expect("seed file");
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "c1"]);
    std::fs::write(
        &file,
        "alpha\nBETA\ndelta\nepsilon\nzeta\neta\ntheta\ngamma\niota\n",
    )
    .expect("staged edit");
    git(&["add", "file.txt"]);
    std::fs::write(
        &file,
        "alpha\nBETA\ndelta\nepsilon\nzeta\neta\ntheta\nGAMMA\niota\n",
    )
    .expect("unstaged edit");
    (tmp, repo)
}

/// The shell over `repo`, configured exactly as production configures it, stepped
/// until painted output and the diff read's verdict both stabilise.
fn diff_shell(
    repo: &std::path::Path,
) -> egui_kittest::Harness<'static, turbogit_app::state::AppState> {
    use turbogit_app::state::AppState;
    let state = AppState::new(repo.to_path_buf());
    assert!(
        !state.multi.roots.is_empty(),
        "the repository root must be discovered, or the diff pane never renders"
    );
    let mut fonts_installed = false;
    let mut harness = egui_kittest::Harness::new_ui_state(
        move |ui, state: &mut AppState| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1024.0, 900.0));
    harness
}

/// Step until two consecutive frames agree *and* no read is outstanding, budgeted
/// by wall clock rather than by frame count so a contended `git` subprocess cannot
/// starve it into a flake.
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

/// **The diff pane's own colour pair, by value, by measurement, and by paint.**
///
/// Moved here verbatim from `tests/diff_viewer.rs` (ticket 12 wrote it there
/// because the file was another agent's partition; ticket 21 is the one that
/// brings it home). The claims:
///
/// 1. the six diff tokens are exactly the values the redesign left alone — the
///    only one of them that moved is the added accent, and it moved because it is
///    another name for the added-file accent, which this file already pins;
/// 2. both text-on-band pairs clear AA, so "the diff's palette is unchanged" is
///    also "the diff's palette is still readable";
/// 3. the pane **paints** those tokens: the added line's band is
///    `DIFF_ADD_BG`, the removed line's is `DIFF_DEL_BG`, and the inks are
///    `DIFF_ADD_TEXT` / `DIFF_DEL_TEXT`. A token that exists and is never painted
///    is a token nothing is reading, and the third claim is what turns the first
///    two from a description into a contract.
///
/// The contrast maths is the one helper at the top of this file, over the token
/// values — never over painted pixels, because rendering shifts a colour by a
/// rounding step and the token is the contract.
#[test]
fn the_diff_colour_pair_is_untouched() {
    // The values.
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
    // The pair still reads: the text on its own band, in both directions.
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
    // …and the accent is not one of the pair it sits on, in either direction: a
    // row's fill is its line background, so the accent is the whole added/removed
    // treatment over it.
    assert_ne!(Palette::DIFF_ADD_ACCENT, Palette::DIFF_ADD_BG);
    assert_ne!(Palette::DIFF_DEL_ACCENT, Palette::DIFF_DEL_BG);

    // And the pane paints exactly those: the add/remove rows' bands and text
    // inks are the tokens above, not a neighbour of them.
    use test_support::harness::{filled_rects, painted_galleys, painted_ink};
    let (_tmp, repo) = diff_fixture_repo();
    let mut harness = diff_shell(&repo);
    settle_diff_shell(&mut harness);
    harness.state_mut().ui.preview_change = Some(repo.join("file.txt"));
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
    let added = origin_of("GAMMA");
    let removed = origin_of("gamma");
    for (band, at, who) in [
        (Palette::DIFF_ADD_BG, added, "added"),
        (Palette::DIFF_DEL_BG, removed, "removed"),
    ] {
        assert!(
            filled_rects(&harness)
                .iter()
                .any(|(rect, fill)| *fill == band && rect.contains(at)),
            "the {who} line paints the {band:?} band behind it, so the token is a \
             colour the pane really uses rather than one nothing reads"
        );
    }
    assert_eq!(
        painted_ink(&harness, "GAMMA"),
        Some(Palette::DIFF_ADD_TEXT),
        "the added line's ink is the diff's added-text token"
    );
    assert_eq!(
        painted_ink(&harness, "gamma"),
        Some(Palette::DIFF_DEL_TEXT),
        "and the removed line's is the removed-text token"
    );
}

// --- R5: a raised control steps up from the surface it sits on ----------------

/// Every raised control that is painted **on a `CONTENT_BG` card**, with the host
/// it is painted on and the raised fill that is correct there.
///
/// R5's table is two rows — `SURFACE` on the app/panel background, the
/// raised-on-card role on a content card — and the *value* half of it is already
/// asserted as arithmetic by
/// `the_raised_ladder_is_strictly_ordered_and_each_rung_steps_up_from_its_own_host`.
/// What a token maths test cannot see is a **caller** reaching for the wrong rung:
/// the app/panel fill dropped onto a card is a control that vanishes into the
/// surface it was dropped on, and no contrast number complains because the number
/// is computed on the tokens and not on the call site.
///
/// So the negative is pinned at the construction sites, the way the ticket says it
/// has to be: a render seam can only prove it at the frames a test enumerates,
/// and a source scan for a token's name passes on a stub. Each row below is a
/// decision a real function makes, so re-pointing one fails here.
///
/// Two named non-members, so the table is a decision rather than an omission:
///
/// - **The count chip and the neutral badge** take the app/panel and top-raised
///   fills (`RAISED`, `SURFACE_3`). A badge is a *mark*, not a control: its fill
///   is chosen for the contrast of the ink on it, and both values clear the R5
///   visibility floor by a wide margin on a content card (1.9:1 and 2.2:1 against
///   `CONTENT_BG`, where the card rung's own 1.1:1 is the floor the ladder test
///   asserts). R5 governs *controls the user has to find*; re-filling the badge
///   family is a value change in the token layer, which this migration's closed
///   token delta does not make. The observation is recorded here rather than left
///   for a reader to make.
/// - **The ghost button ladder** (`SURFACE_2` hover / `SURFACE_3` press) is
///   host-agnostic by construction: `ButtonVariant::fill` is not told what it is
///   painted over, which is R5's own documented reason for the role being a name
///   the caller hands in. So its hover rung *is* the card rung, and the row below
///   asserts exactly that — a ghost control on a content card never takes the
///   app/panel rung. The converse (a ghost on the app background is one rung
///   brighter than the table says) is a known, accepted difference of a
///   single-value ladder, and it is named rather than asserted away.
///
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

/// **R5: no raised control on a content-surface parent takes the
/// raised-on-background fill.** The rule as a *negative*, at the sites that make
/// the decision.
///
/// The failure this catches is a one-token edit — `RAISED_ON_CARD` becomes
/// `RAISED` in a constructor — and it is invisible in a screenshot until someone
/// reports that a button stopped showing up. So each row is compared against the
/// card rung explicitly *and* against the app rung explicitly, and the message
/// names the site and the host rather than the token.
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
        // The negative, said directly. This is the half a value test cannot make:
        // `SURFACE` is a perfectly good raised fill for a control on the app
        // background, and exactly the wrong one here.
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
        // And the ink on it is the step-up ink, not the muted step: R5's
        // principle applied to ink, which is a separate claim with its own
        // construction-site ratchet (`design_tokens.rs`) and is named here so a
        // reader knows where it is enforced rather than assuming it is not.
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

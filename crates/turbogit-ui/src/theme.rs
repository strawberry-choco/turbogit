//! Central dark-only design token set (ADR-0003, issue #3).
//!
//! [`Palette`] below is the single central token set for the *shared*
//! presentation roles — surfaces, ink hierarchy, selection, severity,
//! status/diff families, spacing, density and chip shapes — mirroring the
//! mockups' `colors_and_type.css`. There is exactly one palette and widgets
//! never branch on a theme mode because no other mode exists.
//!
//! Ownership boundary (G1): not every presentation value in the app is
//! centralized here — screen- or component-specific vocabularies kept local
//! to their consumers (e.g. the log graph palette and the cascade accent)
//! remain where they are used, and Review-only C6/T3 families are out of
//! scope. The shared roles below ARE the authoritative contract: changing one
//! of these tokens reaches every consumer that uses the role. Enforcement
//! lives in the existing design-token, widget-library and branch-component
//! suites (`tests/design_tokens.rs`, `tests/widget_library.rs`,
//! `tests/branch_component_kit.rs`).
//!
//! One call to [`configure_style`] maps the tokens into egui `Visuals`;
//! [`install_fonts`] applies the embedded type stack.

use egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Ui, Vec2, Visuals,
};

/// The single central token set (spec §2, Darcula-derived dark).
pub struct Palette;

impl Palette {
    // Core surfaces & lines.
    /// App background, panel fill (`--tg-bg`).
    pub const BG: Color32 = Color32::from_rgb(0x1e, 0x1f, 0x22);
    /// Topbar, headers, dialogs (`--tg-surface`).
    pub const SURFACE: Color32 = Color32::from_rgb(0x2b, 0x2d, 0x30);
    /// Hover fills, secondary buttons (`--tg-surface-2`). Also the *value* of
    /// the raised-on-card role ([`Self::RAISED_ON_CARD`]) — ask for that name
    /// when the control sits on a content-surface card, so "step up from your
    /// own surface" is a rule rather than a per-call-site tone comparison.
    pub const SURFACE_2: Color32 = Color32::from_rgb(0x31, 0x34, 0x38);
    /// Inputs, popovers (`--tg-surface-3`).
    pub const SURFACE_3: Color32 = Color32::from_rgb(0x3c, 0x3f, 0x41);
    /// Contained warning surface — the guardrail alert (logs-panels redesign
    /// issue 03). `STATE_WARNING` at 12% over `SURFACE`, the same derivation
    /// the badge tints use, so an alert can never read as a foreign hue.
    pub const SURFACE_WARNING: Color32 = Color32::from_rgb(0x44, 0x3c, 0x2f);
    /// Primary borders (`--tg-line`).
    pub const LINE: Color32 = Color32::from_rgb(0x4e, 0x51, 0x57);
    /// Subtle row separators (`--tg-line-subtle`).
    pub const LINE_SUBTLE: Color32 = Color32::from_rgb(0x36, 0x38, 0x3c);

    // --- Ink (text): a four-step ramp, with a per-step legality set ---------
    // The ink ramp is a *reading order*, not four names for one grey. Steps 1
    // and 2 are unchanged. Step 3 used to be `#AEB2BA`, which differs from
    // step 2 by two, two and one per channel and measures 1.01:1 against it —
    // so section labels, field labels, filenames, refs and metadata all landed
    // on a single value and a list had no reading order to speak of. Step 3
    // is now `#8A8E96` and step 4 is a new `#6E727A`, and those two values are
    // the whole point: every adjacent pair is separated by at least 1.4:1
    // measured as contrast against the content surface (1.60, 1.57, 1.47).
    //
    // **`INK_3`'s legality narrowed, and that is a real reduction of a
    // previously-held guarantee — not a regression that was always there.**
    // The old contract was "≥4.5:1 on every surface the palette audits", and
    // the old value held it on all of them. That contract is
    // *incompatible with having a step 3 at all*: the selection token is dark
    // enough that the brightest ink which still clears 4.5:1 on it measures
    // 1.04:1 against `INK_2` — the same collision this block exists to remove.
    // So the value does not move; `INK_3` stops being legal everywhere:
    //
    // - **legal** on the app background, the content surface and the sidebar
    //   surface (5.02:1, 4.67:1, 5.19:1) — the sidebar is the left rail's own
    //   fill, which `ui::sidebar::show` paints, and it is darker than the app
    //   background, so it is legal by arithmetic rather than by a fourth
    //   decision;
    // - **not legal** on a raised surface (`SURFACE`, `SURFACE_2`,
    //   `SURFACE_3`) or a selected one (`SELECTION`, `ROW_SELECTED`) — 4.20,
    //   3.81, 3.23, 3.01 and 3.75. There the caller steps *up* to `INK_2`,
    //   which clears every one of them (6.58, 5.96, 5.06, 4.72, 5.87). That is
    //   what makes the narrowing safe.
    //
    // Do not "fix" this by raising `INK_3` to chase the raised surfaces: that
    // lands it near `#ACB0B0`, whose contrast to `INK_2` is 1.04:1 — precisely
    // the bug the ramp exists to fix. Do not fix it by deleting a row from the
    // surface sweep either; the sweep is the decision record.
    //
    // `INK_4`'s restriction is a *different* restriction, and the difference is
    // load-bearing: `INK_4` is sub-AA on **every** surface the palette audits
    // (3.53:1 down to 2.05:1), while `INK_3` is sub-AA *only* on raised and
    // selected surfaces. An implementer who conflates the two will either
    // re-brighten `INK_4` until it is legal and destroy the ramp, or stop
    // trusting `INK_3` on the surfaces where it is still fine.
    //
    // `tests/design_tokens.rs` pins the legal/illegal surface sets per step,
    // the measured margins, and the negative — that no chip or row construction
    // site wears the muted ink on a raised or selected fill — at those
    // construction sites rather than by scanning source.
    ///
    // **The sweep's one standing exception, named.** `INK_4`/`T_DIM` is the
    // only token in `theme.rs` with no `src` consumer. The sweep kept it
    // rather than deleting it because the step *is* the legality rule this
    // block argues from: a ramp of three cannot state "exactly one ink may sit
    // below 4.5:1", and the spec introduces the step to carry exactly that
    // restriction. Its call sites are owed by the view phase (tickets 09–19),
    // and its natural consumer is named here so the next sweep does not have
    // to re-derive one: `configure_style` leaves
    // `Visuals::weak_text_color` unset, so every placeholder in the app —
    // every `TextEdit::hint_text`, the search filter included — is painted
    // egui's derived `text_color().gamma_multiply(0.6)` rather than a token.
    // That is the dim-ink job. The sweep flagged it and did not do it, because
    // it repaints every placeholder in the app and that is a visual decision,
    /// not a token decision.
    /// Primary text (`--tg-ink`). Equal-value alias of the authoritative
    /// `T_PRIMARY` body ink — one primary role across shell defaults and
    /// tool-window content (C3), so a central ramp change reaches both.
    pub const INK: Color32 = Self::T_PRIMARY;
    /// Secondary text (`--tg-ink-2`) — body text and secondary labels. Legal on
    /// every audited surface, which is what makes it the step-up ink wherever
    /// [`Self::INK_3`] is not legal.
    pub const INK_2: Color32 = Self::T_SECONDARY;
    /// Muted/hint text (`--tg-ink-3`) — section labels, column headers,
    /// metadata, refs. **Not legal on a raised or selected surface**; see the
    /// ramp block above for the measured argument and the step-up rule.
    pub const INK_3: Color32 = Self::T_MUTED;
    /// Dim ink (`--tg-ink-4`) — the fourth ramp step, and the only ink with no
    /// legal surface in the palette. Equal-value alias of [`Self::T_DIM`]; see
    /// the ramp block above for its permitted uses.
    pub const INK_4: Color32 = Self::T_DIM;

    // Brand.
    /// Primary actions, selection, links (`--tg-brand`).
    pub const BRAND: Color32 = Color32::from_rgb(0x35, 0x74, 0xf0);
    /// Text on brand-colored fills (`--tg-brand-ink`).
    pub const BRAND_INK: Color32 = Color32::WHITE;

    // Status colors.
    /// Success (`--tg-state-success`).
    pub const STATE_SUCCESS: Color32 = Color32::from_rgb(0x4c, 0xaf, 0x50);
    /// Warning (`--tg-state-warning`).
    pub const STATE_WARNING: Color32 = Color32::from_rgb(0xf9, 0xa8, 0x25);
    /// Error (`--tg-state-error`).
    pub const STATE_ERROR: Color32 = Color32::from_rgb(0xef, 0x53, 0x50);
    /// Info (`--tg-state-info`).
    pub const STATE_INFO: Color32 = Color32::from_rgb(0x42, 0xa5, 0xf5);

    // Status semantics (issue #01): the one status the state family needed a
    // second name for. Clean, dirty and stale all resolved onto the same three
    // state tones with no caller ever asking for them by that name, and the
    // sweep deleted the three aliases rather than the map. The map itself is
    // unchanged and still has a real consumer: `RepoState::color` routes
    // through it, and that map's own vocabulary is deliberate — clean and
    // unpushed are the *ahead* green, dirty and unpulled the reserved counter
    // orange, and only a conflict or a divergence reaches the error red. So
    // "every status chip paints a real colour, never a fallback" holds at the
    // one place a status colour is decided, and diverged is the one status
    // that needed a distinct name to say so.
    /// Local branch has diverged from upstream — issue #01. Shares the error
    /// red with a conflict, and is named for that reason: a sidebar row, a
    /// counter chip and a branch-tree row all have to say *diverged* without
    /// borrowing a name that would read as a generic failure.
    pub const STATUS_DIVERGED: Color32 = Self::STATE_ERROR;

    // --- Branches screen token set (design doc §13) ------------------------
    // The Branches screen adds its own surface/meaning/text vocabulary beside
    // the core palette. Values are the design reference's own; where a §13
    // surface carries the same value as an existing token the existing token
    // is declared as the value's source so the whole app keeps one definition
    // (RAISED aliases SURFACE, RAISED_ON_CARD aliases SURFACE_2 — the two
    // rungs of the raised ladder, one per host surface — and RULE_CONTENT
    // aliases RAISED: the weakest 1px separator uses the raised-surface tone,
    // NOT the stronger LINE border).
    //
    // Supported surfaces and their intended compositing backgrounds (C5):
    // all fills here are fully opaque and composite directly onto the app
    // background; translucent treatments (e.g. the rgba selection fill) are
    // defined against the specific surface they sit on and documented with it.
    // The one opaque selection fill, `ROW_SELECTED`, is defined the same way:
    // it is opaque *because* a list row's fill has to be one value, whatever
    // the row happens to sit on.

    /// Content (`#232529`): branch list, breadcrumb strip, tab strip.
    pub const CONTENT_BG: Color32 = Color32::from_rgb(0x23, 0x25, 0x29);
    /// Raised control (`#2B2D30`): secondary buttons, badges, chips, active tab.
    /// This is the raised fill **on the app background or a panel background** —
    /// the first rung of the surface ladder. The second rung is
    /// [`Self::RAISED_ON_CARD`], and which one a control takes depends on the
    /// surface it actually sits on, not on the control.
    pub const RAISED: Color32 = Self::SURFACE;
    /// Raised control **on a content-surface card** — the second rung of the
    /// surface ladder, an equal-value alias of [`Self::SURFACE_2`].
    ///
    /// | Surface the control sits on | Raised fill is |
    /// |---|---|
    /// | app background | [`Self::RAISED`] (`#2B2D30`) |
    /// | a content-surface card | this role (`#313438`) |
    ///
    /// A raised control steps up *relative to the surface it actually sits on*,
    /// so one tone cannot serve both rungs: a chip dropped on a content-surface
    /// card in the app's own `SURFACE` is invisible against it, and the fix is
    /// not a darker chip but the rung that matches the host. Two aliases, no new
    /// values — the ladder `BG < CONTENT_BG < SURFACE < SURFACE_2 < SURFACE_3`
    /// is untouched, and `tests/design_tokens.rs` asserts both the ordering and
    /// this identity as arithmetic rather than as this comment.
    pub const RAISED_ON_CARD: Color32 = Self::SURFACE_2;
    /// Selection (`#2E4369`): the **current-ref** band — the heavier "this is
    /// where HEAD points" treatment, and only that.
    ///
    /// Its documented scope is one construction site: the selected band
    /// `ui::components::current_row_fill` paints for the row carrying the
    /// current ref. **No list row resolves to this token.** A chosen row in a
    /// tree or list takes [`Self::ROW_SELECTED`]; the log table, the sidebar
    /// tree and blame take the translucent [`Self::selection_bg`] focus band.
    /// The row-fill decision states the same rule where the fills are actually
    /// chosen, and `tests/branch_component_kit.rs` pins it there.
    ///
    /// It is not a row fill because a *solid* band behind running text is a fill
    /// the accent may not be: the accent measures 3.6:1 on the content surface —
    /// enough for a [`crate::theme::RAIL_WIDTH`] rail, never enough to sit
    /// behind a branch name. What a selected row needs is a visible step that
    /// leaves the text readable, which is [`Self::ROW_SELECTED`].
    pub const SELECTION: Color32 = Color32::from_rgb(0x2e, 0x43, 0x69);
    /// Selected list row (`#243456`) — the fill a **chosen row in a tree or
    /// list** takes. This is the shared tree/list selection vocabulary: the one
    /// value `ui::components::RowState::RowSelected` resolves to, and the one
    /// every hand-painted row reaches through `RowState::from_flags`.
    ///
    /// Opaque on purpose. The app's other selection treatment is the translucent
    /// [`Self::selection_bg`] composite, which is *composited* against whatever
    /// it happens to sit on and therefore measures differently on every surface;
    /// a list row's fill has to be one value, or the same list reads as selected
    /// at two strengths in two windows. It is deliberately neither
    /// [`Self::SELECTION`] (the current-ref band — no list row resolves to that)
    /// nor that composite, and it is not the brand fill either: a solid brand
    /// band behind running text is precisely what this token replaces.
    ///
    /// The measurements that make it a fill rather than a wash: 1.24:1 against
    /// the content surface — a visible step, not a saturated fill. On it
    /// [`Self::INK_2`] and [`Self::ACCENT_TEXT`] clear AA (5.87:1, 5.89:1) while
    /// [`Self::INK_3`] does not (3.75:1), which is why the muted step is not
    /// legal on a selected row and its caller steps up. The accent itself
    /// measures 3.6:1 on the content surface, which is why it may be the
    /// [`crate::theme::RAIL_WIDTH`] rail at this row's leading edge and may not
    /// be its fill.
    pub const ROW_SELECTED: Color32 = Color32::from_rgb(0x24, 0x34, 0x56);
    /// Group-label band (`#1F232C`): the scaffolding strip a `LOCAL` / `REMOTE`
    /// header sits on. A faint cool lift where the repo header, the hover fill
    /// and the raised surfaces are all neutral grey, and far weaker than a
    /// current row's band — so group labels never read as a row state.
    pub const SECTION_BG: Color32 = Color32::from_rgb(0x1f, 0x23, 0x2c);

    // --- Hairline roles (ticket 09) ---------------------------------------
    // A one-pixel line used to be picked by whichever of the three line tokens
    // was nearest, and several genuinely different uses of it shared a look
    // they had no reason to share. These name the three roles the app
    // actually has, so a caller asks for the role it wants rather than for the
    // tone that happened to render correctly. They are pure aliases: no token
    // value moved, and the C5 contract above (`RULE_CONTENT == RAISED`,
    // `RULE_CONTENT != LINE`) is untouched. The weakest of the three used to be
    // spelled `DIVIDER` as a second name for `RAISED`; the sweep removed that
    // duplicate and the role is the surviving name, so there is one spelling
    // per tone.
    //
    // A FOURTH tone is deliberately left unowned here: egui's default
    // `ui.separator()` paints `visuals.noninteractive.bg_stroke`, and
    // `configure_style` never assigns that field — it only sets
    // `noninteractive.fg_stroke` to `INK_2`, which `Separator` does not read.
    // So the separator keeps egui's stock dark `Widgets::default()` value
    // (`from_gray(60)`, `#3C3C3C`). It is a real hairline role with no name,
    // and sweeping its ~37 call sites onto these three is follow-up work — each
    // site needs its own role judgement, and several are deliberately not
    // hairlines at all. See `docs/design-system-roles.md`.

    // `RULE_FOOTER` and `RULE_STRUCTURAL` have real callers — the dialog
    // footer in `ui::widgets::containers` and the menu's group rule in
    // `ui::widgets::menu`. `RULE_CONTENT` is the sweep's second standing
    // exception: no site asks for "the weakest hairline inside one surface"
    // today, and it is kept because it is the third rung of an ordered
    // ladder whose other two are named and used, and because the separator
    // sweep below is where its first caller arrives. It was not deleted with
    // its duplicate for that reason, and it is named here so the next sweep
    // sees an exception rather than a gap.

    /// Hairline role — the content divider: a 1px rule between sibling content
    /// regions inside one surface. The weakest of the three: it separates, it
    /// does not bound. Aliases the raised surface, so the C5 contract reads
    /// `RULE_CONTENT == RAISED`, `RULE_CONTENT != LINE`.
    pub const RULE_CONTENT: Color32 = Self::RAISED;
    /// Hairline role — the footer rule: the 1px rule separating a modal body from
    /// its action slot. Deliberately a stronger tone than [`Self::RULE_CONTENT`]
    /// — a modal's action slot is a boundary, not a division.
    pub const RULE_FOOTER: Color32 = Self::LINE;
    /// Hairline role — the structural rule: table header underlines, tree indent
    /// guides, and panel edge rules — the chrome that gives a surface its
    /// structure, rather than dividing its content.
    pub const RULE_STRUCTURAL: Color32 = Self::LINE_SUBTLE;

    /// Accent / primary action (`#3574F0`): New Branch, Checkout, branch chips.
    pub const ACCENT: Color32 = Self::BRAND;
    /// Ahead (`#5FA86C`): ahead counts, current-branch icon, "in sync".
    pub const AHEAD: Color32 = Color32::from_rgb(0x5f, 0xa8, 0x6c);
    /// Danger (`#DB5C5C`): Delete only.
    pub const DANGER: Color32 = Color32::from_rgb(0xdb, 0x5c, 0x5c);
    /// Commit hash ink (`#74A3E8`). Consumer: `widgets::hash_chip`, the only
    /// place in the app that renders a hash as itself.
    ///
    /// Not a link and not the accent. A hash chip is a *label* — hover sense
    /// only, no click plane, no focus ring (ADR-0024), because copying the hash
    /// lives in the commit's context menu — so it may not wear the action
    /// accent, and the accent it used to wear failed to read anyway: `BRAND` on
    /// the chip's own `SURFACE_3` fill measures 2.48:1, under the 3:1 floor for
    /// a non-text graphical object and far under AA. This measures 4.12:1 there.
    /// The value is unchanged by that fix — the chip was the thing that was
    /// wrong about it.
    pub const LINK: Color32 = Color32::from_rgb(0x74, 0xa3, 0xe8);

    /// Text ramp — primary (`#DFE1E5`): branch names, body text, the row being
    /// read. 11.7:1 on the content surface.
    pub const T_PRIMARY: Color32 = Color32::from_rgb(0xdf, 0xe1, 0xe5);
    /// Secondary text, including on selected rows and raised controls. 7.3:1 on
    /// the content surface and ≥4.7:1 on every audited surface, which is exactly
    /// why it is the step-up ink wherever [`Self::T_MUTED`] is not legal.
    pub const T_SECONDARY: Color32 = Color32::from_rgb(0xb0, 0xb3, 0xbb);
    /// Muted text (`#8A8E96`): section labels, column headers, metadata, refs.
    /// 4.7:1 on the content surface, and ≥4.5:1 on the app, content and sidebar
    /// surfaces **only** — not on a raised surface, and not on a selected one,
    /// where the caller steps up to [`Self::T_SECONDARY`]. Read the ink-ramp
    /// block at [`Self::INK_3`] for the full measured argument.
    pub const T_MUTED: Color32 = Color32::from_rgb(0x8a, 0x8e, 0x96);
    /// Dim ink (`#6E727A`), 3.2:1 on the content surface — the only ink in the
    /// palette that is sub-AA on *every* audited surface, which is precisely
    /// why it is restricted to **placeholders, dim path suffixes and hatches**.
    /// Never the only rendering of something the user needs: if the text is the
    /// answer rather than the furniture around it, it takes [`Self::T_MUTED`] or
    /// above. Carries the sweep's one standing exception — see the ink-ramp block
    /// at [`Self::INK_3`] for why and for the consumer it is owed.
    pub const T_DIM: Color32 = Color32::from_rgb(0x6e, 0x72, 0x7a);
    /// Readable accent ink. Action fills continue to use ACCENT/BRAND.
    pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0x8b, 0xb5, 0xf5);

    // Reserved counter orange (redesign issue 01): `#E0883C` is reserved for
    // dirt/unpulled count badges only. Never a general accent, action, or
    // warning — keep it out of any non-counter call site.
    /// Dirty/unpulled counter orange.
    pub const COUNTER: Color32 = Color32::from_rgb(0xe0, 0x88, 0x3c);

    // Local Changes status letters (redesign issue 01): M / A / U colour the
    // status letter and the filename in a file row (mockup values).
    /// Modified file letter (`M`) — light blue.
    pub const STATUS_MODIFIED: Color32 = Color32::from_rgb(0xa8, 0xc0, 0xe8);
    /// Added file letter (`A`) (`#6FAE75`) — the diff-added green, and the one
    /// added-accent contrast fix: 4.33:1 on the content surface at the old
    /// `#57965C`, 5.84:1 here. It is the same token as
    /// [`Self::DIFF_ADD_ACCENT`], which is why the diff's added line moves with
    /// the added file's letter and nothing else in the diff vocabulary does.
    pub const STATUS_ADDED: Color32 = Color32::from_rgb(0x6f, 0xae, 0x75);
    /// Unversioned file letter (`U`) — olive.
    pub const STATUS_UNVERSIONED: Color32 = Color32::from_rgb(0xb5, 0xb3, 0x7e);

    // Diff colors.
    /// Added-line background (`--tg-diff-add`).
    pub const DIFF_ADD_BG: Color32 = Color32::from_rgb(0x34, 0x4f, 0x3e);
    /// Added-line text (`--tg-diff-add-text`).
    pub const DIFF_ADD_TEXT: Color32 = Color32::from_rgb(0x85, 0xe8, 0x9d);
    /// Deleted-line background (`--tg-diff-del`).
    pub const DIFF_DEL_BG: Color32 = Color32::from_rgb(0x5a, 0x3a, 0x3a);
    /// Deleted-line text (`--tg-diff-del-text`).
    pub const DIFF_DEL_TEXT: Color32 = Color32::from_rgb(0xff, 0x9a, 0x9a);

    // Redesign diff accents (issue 01): the line text/marker colour for the new
    // diff preview — distinct from the legacy DIFF_*_BG/TEXT pair above, which
    // the old diff view keeps using. `DIFF_{ADD,DEL}_BG` are the
    // line-background roles; a row's *block* fill is the line background, so the
    // accent over it is the whole added/removed treatment.
    /// Added-line accent (`#6FAE75`) — shares the added-file green, and is the
    /// *only* token in the diff vocabulary that moved: 4.33:1 on the content
    /// surface at `#57965C`, 5.84:1 here. The diff's own background/text pair
    /// and every diff row fill are untouched by that move.
    pub const DIFF_ADD_ACCENT: Color32 = Color32::from_rgb(0x6f, 0xae, 0x75);
    /// Removed-line accent (`#F75464`).
    pub const DIFF_DEL_ACCENT: Color32 = Color32::from_rgb(0xf7, 0x54, 0x64);

    /// Selected-row fill: BRAND at ~25% premultiplied alpha over BG.
    pub fn selection_bg() -> Color32 {
        Color32::from_rgba_premultiplied(0x0d, 0x1d, 0x3c, 0x40)
    }

    /// Sidebar surface (`#1B1C1E`): the left rail's own fill, painted by
    /// `ui::sidebar::show`. Deliberately darker than the app background it sits
    /// beside (1.03:1), so the rail reads as a distinct surface rather than as
    /// an unlabelled gap in the content — and darker still than any raised
    /// surface, so every hover fill and selection band inside it steps *up* from
    /// the rail, which is what makes the `RAISED_ON_CARD` ladder rule hold here
    /// too. It is the one audited surface the muted ink is legal on by
    /// arithmetic rather than by decision; see the ink-ramp block at
    /// [`Self::INK_3`].
    pub const SIDEBAR: Color32 = Color32::from_rgb(0x1b, 0x1c, 0x1e);
}

// --- Accent tint fractions -------------------------------------------------
// How strongly an accent colour is mixed over a surface, as a role rather than a
// per-call-site decimal. `tint_over_bg` (ui::widgets) applies them; each value
// here is the amount of accent in the result. The third member of the family,
// a chip's 0.18, lives as `ui::widgets::BADGE_TINT` — a home for it is a
// conformance issue 18 question, not a reason to duplicate the value here.
/// A band behind one section of a multi-part panel — a "yours" / "theirs" block
/// in the conflict screens. Weaker than a chip's fill, because it sits under many
/// lines of text rather than behind one word.
pub const SECTION_TINT: f32 = 0.12;
/// A marker strip: a small object that has to read as *a thing you can act on*
/// rather than as background. One step above [`SECTION_TINT`], which is what
/// keeps a conflict's marker strip distinct from the section it marks.
pub const MARKER_TINT: f32 = 0.15;

/// Shared repository-state vocabulary for dots, badges and summaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RepoState {
    Clean,
    Dirty,
    Conflict,
    Diverged,
    Unpushed,
    Unpulled,
    /// A submodule registered but not initialised: no working copy checked out.
    /// Not a severity — it is work the user is owed — so it takes the muted ink and
    /// not one of the three state tones.
    Uninitialized,
}

impl RepoState {
    /// The state in words, beside [`RepoState::color`] so a dot and the summary
    /// next to it can never disagree. The wording is the sidebar's smart-group
    /// vocabulary where the states overlap; `in sync` is new, and is what a
    /// repository with nothing to report says.
    pub fn words(self) -> &'static str {
        match self {
            Self::Clean => "in sync",
            Self::Dirty => "dirty worktree",
            Self::Conflict => "has conflicts",
            Self::Diverged => "diverged",
            Self::Unpushed => "unpushed commits",
            Self::Unpulled => "unpulled commits",
            Self::Uninitialized => "not initialized",
        }
    }

    /// The one state→colour map, and every state a caller can reach.
    ///
    /// Six of the seven states are severities and route to one of the three
    /// state tones. [`Self::Uninitialized`] is **not** a severity — the recorded
    /// commit is in the index and no working copy has been checked out, which is
    /// work owed rather than something wrong — so it takes [`Palette::INK_3`]
    /// and the app never has to invent a fourth tone.
    ///
    /// The muted step is the one ink that can say this, and the ramp's own
    /// legality set is why the call sites are not free about it: `INK_3` clears
    /// AA on the app background (5.01:1), the content surface (4.67:1) and the
    /// sidebar surface (5.19:1) and is **not** legal on a raised or selected
    /// fill. The submodules pane is a `Ui::new_child` of the central panel, so
    /// its uninitialised dot lands on `panel_fill` — the app background — which
    /// is the legal set's brightest member. A caller that puts this state on a
    /// raised or selected surface must step up to `INK_2` rather than weaken
    /// the ramp's contract.
    pub fn color(self) -> Color32 {
        match self {
            Self::Clean | Self::Unpushed => Palette::AHEAD,
            Self::Dirty | Self::Unpulled => Palette::COUNTER,
            Self::Conflict | Self::Diverged => Palette::STATUS_DIVERGED,
            Self::Uninitialized => Palette::INK_3,
        }
    }

    /// Conflicts and local edits take precedence over upstream drift.
    pub fn from_root(root: &turbogit_domain::model::Root, ahead: usize, behind: usize) -> Self {
        if !root.status.conflicted.is_empty() {
            Self::Conflict
        } else if root.status.modified() + root.status.unversioned() > 0 {
            Self::Dirty
        } else if ahead > 0 && behind > 0 {
            Self::Diverged
        } else if behind > 0 {
            Self::Unpulled
        } else if ahead > 0 {
            Self::Unpushed
        } else {
            Self::Clean
        }
    }
}

// --- Spacing scale (Local Changes redesign, design doc §8) ---
// Dimension tokens, kept beside the color palette as part of the single
// central token set. Rows keep one consistent height per kind; the default
// gaps/padding/margins below ARE the shared layout contract (S1) — the
// 4 px grid covers the gaps and the panel padding, not the row heights
// (`GROUP_ROW_HEIGHT` is 26 px, a hair taller than the file rows it heads) and
// not every gap, since the default item spacing and control padding are named
// roles of their own. The grid is a property of the values below rather than a
// token, so the suite asserts the relation against them.
/// File-row height in the changes tree (24 px). This is the single-line
/// height: a row grows one text line per extra line of text, because a
/// filename too long for its column wraps rather than clipping.
pub const FILE_ROW_HEIGHT: f32 = 24.0;
/// Group-row height in the changes tree (26 px).
pub const GROUP_ROW_HEIGHT: f32 = 26.0;
/// Panel padding (12 px; the spec allows 12–14 px).
pub const PANEL_PADDING: f32 = 12.0;

// Shared layout roles (S1): the one source for the shell defaults.
/// Default item gap between widgets (8×6) — installed by [`configure_style`].
pub const ITEM_SPACING: Vec2 = Vec2::new(8.0, 6.0);
/// Default window/panel margin (10 px) — installed by [`configure_style`].
pub const WINDOW_MARGIN: i8 = 10;
/// Default control padding (10×5) — installed by [`configure_style`].
pub const BUTTON_PADDING: Vec2 = Vec2::new(10.0, 5.0);
/// Default list/tree indent (14 px) — installed by [`configure_style`].
pub const INDENT: f32 = 14.0;
/// Air between two adjacent cells' columns in a column-oriented tool pane, so a
/// long cell in one column does not read as belonging to the next.
///
/// **A gap, not a column offset:** a `PaneColumn`'s `x` places a cell; this is
/// the clearance left after one, so it never moves a column.
pub const CELL_GAP: f32 = 8.0;

// Density variants (S2): named exceptions to the defaults above, so compact
// and dense regions stop carrying regional literals.
/// Compact control padding (8×4) for dense shell/toolbar regions.
pub const DENSITY_COMPACT_BUTTON: Vec2 = Vec2::new(8.0, 4.0);
/// Dense control padding (6×2) for the status bar.
pub const DENSITY_DENSE_BUTTON: Vec2 = Vec2::new(6.0, 2.0);

// Shape variants (S3): chip/control radii are explicit shared roles; the
// pill variant is the full-height wrap the welcome chips and badges use.
/// Compact chip corner radius (3 px).
pub const CHIP_RADIUS: u8 = 3;
/// Control corner radius (4 px).
pub const CONTROL_RADIUS: u8 = 4;
/// Window corner radius (8 px).
pub const WINDOW_RADIUS: u8 = 8;
/// Menu corner radius (6 px).
pub const MENU_RADIUS: u8 = 6;
/// Card corner radius (8 px) — the bordered content regions of the Local
/// Changes redesign, one step above the 4 px control radius so a card reads
/// as a container rather than as a large control.
pub const CARD_RADIUS: u8 = 8;
/// Pill corner radius (9 px) — the full-height wrap the welcome chips and
/// badges use. Shared chip geometry derives the full pill radius from this
/// token: `widgets::CHIP_GEOMETRY.radius` is `PILL_RADIUS as f32` and
/// `widgets::chip_radius` rounds chips with `CornerRadius::same(PILL_RADIUS)`.
pub const PILL_RADIUS: u8 = 9;
/// Cascade-operation accent, distinct from the shared status colors.
pub const CASCADE_ACCENT: Color32 = Color32::from_rgb(0xa7, 0x8b, 0xfa);
/// Mark corner radius (2 px) — the rounding on something too small to take
/// [`CONTROL_RADIUS`]: a 3 px kind-colored accent bar, a 10 px repository dot,
/// a 2 px focus stroke drawn *inside* a pane's own edge, a character-level diff
/// highlight. Anything larger is a control or a chip and takes its radius.
pub const MARK_RADIUS: u8 = 2;
/// Accent-rail width (2 px) — the selection rail a list row carries at its
/// leading edge, and the **only** place in the tree that width is written down
/// as a dimension.
///
/// **Paint, not layout.** The rail is drawn over the row's own fill at the
/// row's leading edge, so a selected row's text origin *is* an unselected row's
/// text origin. Reserving this width as padding instead shifts every row's
/// content 2 px the moment it is selected, which is the drift the painted-geometry
/// ratchet in `tests/branch_component_kit.rs` exists to catch.
///
/// Deliberately not [`MARK_RADIUS`]: that is a 2 px **radius**, this is a 2 px
/// **width**. The numbers coincide and the roles do not, which is exactly why
/// they are two declarations rather than one token wearing two names — a caller
/// that rounds a chip with a rail width gets a 2 px pill.
///
/// The two pre-existing rails — the sidebar's stroked segment and the diff pane's
/// filled rect — were absorbed into this one painter in ticket 07, so
/// `ui::components::paint_rail` is the single consumer and the width is
/// written down exactly once in the tree. It paints the sidebar's segment
/// and the diff pane's rect, and a selected list row's leading edge.
pub const RAIL_WIDTH: f32 = 2.0;

/// Accent (selection / primary action) color — the brand token.
pub fn accent() -> Color32 {
    Palette::BRAND
}

/// Muted icon/foreground tint that reads well on the dark palette.
pub fn icon_color() -> Color32 {
    Palette::INK_2
}

// --- Branches-screen type ramp (design doc §13) -------------------------
// Five sizes are enough for the whole screen: 9 section labels, 10 chips,
// 11 controls/metadata, 12 branch names/body, 13 the detail title.
/// Section labels — 9px, uppercase, letter-spaced (rendered via `section_label`).
pub const TYPE_SECTION: f32 = 9.0;
/// Chips and badges — 10px.
pub const TYPE_CHIP: f32 = 10.0;
/// Controls, key labels, metadata — 11px.
pub const TYPE_CONTROL: f32 = 11.0;
/// Branch names and body text — 12px.
pub const TYPE_BODY: f32 = 12.0;
/// Detail-panel title — 13px.
pub const TYPE_DETAIL_TITLE: f32 = 13.0;

// Two steps the §13 ramp does not cover, each found by the conformance sweep as
// a literal with no role to point at. Both have exactly one consumer today, so
// each is recorded with it — a third call site has to be a deliberate choice to
// reuse the role, not a coincidence of pixels.
/// Repository-group header name in the commit window's changes tree (12.5 px).
/// Sits one hair above [`TYPE_BODY`], which the file rows it groups render at,
/// so a group header reads as the parent without jumping to a title. Consumer:
/// `ui::commit_window::repo_group`.
pub const TYPE_GROUP_NAME: f32 = 12.5;
/// A full-window pane's own title (16 px) — the heading a tool window, dialog or
/// summary pane writes above its content, as opposed to a label inside it.
/// Consumers: `ui::multi_selection::show_summary`, the settings page header, and
/// the changelog dialog's title in `ui::welcome`.
pub const TYPE_PANE_TITLE: f32 = 16.0;

// Display roles (T2): distinct sizes for prominent display-only text. Named
// so central type changes reach the consumers, without reducing them to body.
/// Welcome wordmark — 42px (welcome.rs brand header).
pub const TYPE_WORDMARK: f32 = 42.0;
/// The strapline directly under [`TYPE_WORDMARK`] (14 px) — one step above the
/// §13 ramp's largest role, and only ever set beside the wordmark. Consumer:
/// `ui::welcome::brand_header`.
pub const TYPE_TAGLINE: f32 = 14.0;
/// Multi-repo statistics — 22px (multi_selection.rs stats strip).
pub const TYPE_STATISTIC: f32 = 22.0;

/// Data font face — JetBrains Mono (branch names, hashes, paths, upstreams,
/// counts, timestamps).
pub fn data_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

/// Chrome font face — the embedded UI sans, so interface labels (repo names,
/// section labels, buttons, counts) read in a proportional face while data
/// (branch and remote names, hashes, upstreams) stays monospaced. Which face
/// sits here is decided once, in [`font_definitions`]; a different chrome
/// family upgrades every call site at once.
///
/// Bold chrome has no bold member in this family, so emphasised labels keep
/// rendering through the embedded mono bold (`ui::widgets`'s bold-font path)
/// rather than a synthesised weight of the sans.
pub fn chrome_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Horizontal indent per tree depth — the width of two whitespace characters
/// in the data face. Shared by the branch tree and the sidebar project tree so
/// both hang at the same rate, off the type ramp rather than a fixed pixel.
pub fn two_space_indent(ui: &Ui) -> f32 {
    ui.painter()
        .layout_no_wrap("  ".to_owned(), data_font(TYPE_BODY), Color32::WHITE)
        .size()
        .x
}

/// How many [`two_space_indent`] steps a tree level hangs its children on. Two,
/// so a directory subgroup and the branches under it are not read as siblings.
pub const INDENT_STEPS_PER_LEVEL: f32 = 2.0;

/// The horizontal indent of one tree level — twice the two-space width, off the
/// data face.
pub fn indent_step(ui: &Ui) -> f32 {
    two_space_indent(ui) * INDENT_STEPS_PER_LEVEL
}

fn dark_visuals() -> Visuals {
    let mut v = Visuals::dark();
    v.dark_mode = true;
    v.window_fill = Palette::SURFACE;
    v.panel_fill = Palette::BG;
    v.extreme_bg_color = Palette::BG;
    v.override_text_color = Some(Palette::INK);
    v.faint_bg_color = Palette::SURFACE_2;
    v.code_bg_color = Palette::SURFACE_3;
    v.hyperlink_color = Palette::ACCENT_TEXT;
    v.warn_fg_color = Palette::STATE_WARNING;
    v.error_fg_color = Palette::STATE_ERROR;
    v.selection.bg_fill = Palette::selection_bg();
    v.selection.stroke = Stroke::new(1.0, Palette::BRAND);
    v.widgets.noninteractive.bg_fill = Palette::SURFACE;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Palette::INK_2);
    v.widgets.inactive.bg_fill = Palette::SURFACE_2;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, Palette::INK_2);
    v.widgets.hovered.bg_fill = Palette::SURFACE_2;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Palette::INK);
    v.widgets.active.bg_fill = Palette::SURFACE_3;
    v.widgets.active.fg_stroke = Stroke::new(1.0, Palette::INK);
    v.widgets.open.bg_fill = Palette::SURFACE_3;
    v.widgets.open.fg_stroke = Stroke::new(1.0, Palette::INK);
    // Active window headers blend with the SURFACE window fill (issue #22).
    v.widgets.open.weak_bg_fill = Palette::SURFACE;
    v.window_corner_radius = CornerRadius::same(WINDOW_RADIUS); // radius-lg
    v.menu_corner_radius = CornerRadius::same(MENU_RADIUS); // radius-md
    // Popup chrome (issue #22, spec §10): every floating surface (dialogs,
    // popups, palette, toast) gets the LINE border stroke over its SURFACE
    // fill; radius + fill are already mapped above.
    v.window_stroke = Stroke::new(1.0, Palette::LINE);
    v
}

/// Apply the dark-only design `Visuals` plus a shared spacing / typography
/// scale. Idempotent; called from the app loop.
pub fn configure_style(ctx: &Context) {
    ctx.all_styles_mut(|style| {
        style.visuals = dark_visuals();

        // Shared layout roles (S1): the defaults below ARE the contract.
        style.spacing.item_spacing = ITEM_SPACING;
        style.spacing.window_margin = egui::Margin::same(WINDOW_MARGIN);
        style.spacing.button_padding = BUTTON_PADDING;
        style.spacing.indent = INDENT;

        // Shell defaults consume the same §13 ramp as the Branches view.
        style
            .text_styles
            .insert(TextStyle::Body, chrome_font(TYPE_BODY));
        style
            .text_styles
            .insert(TextStyle::Button, chrome_font(TYPE_CONTROL));
        style
            .text_styles
            .insert(TextStyle::Heading, chrome_font(TYPE_DETAIL_TITLE));
        style
            .text_styles
            .insert(TextStyle::Monospace, data_font(TYPE_BODY));
        style
            .text_styles
            .insert(TextStyle::Small, chrome_font(TYPE_CONTROL));

        style.animation_time = 0.12;
    });
}

// --- Cycle 3 implementation: embedded fonts (ADR-0002) ---

use egui::epaint::text::{FontData, FontDefinitions, FontTweak};

/// Embedded JetBrains Mono Regular (OFL-licensed; see `assets/fonts/OFL.txt`).
pub const JETBRAINS_MONO_REGULAR: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");
/// Embedded JetBrains Mono Bold.
pub const JETBRAINS_MONO_BOLD: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf");

/// Registry keys used in [`font_definitions`].
const JBM_REGULAR_KEY: &str = "jetbrains-mono-regular";
const JBM_BOLD_KEY: &str = "jetbrains-mono-bold";
/// egui's built-in proportional face, already carried in
/// `FontDefinitions::default()`'s font data. Leads the chrome chain.
const UI_SANS_KEY: &str = "Ubuntu-Light";

/// Load a system font file, if present on this machine. System faces are
/// fallback-only chain entries (ADR-0002): when the file cannot be found the
/// face is omitted from the chains entirely — epaint panics on family members
/// without font data, so absent names must never be listed.
fn system_font_data(file_names: &[&str]) -> Option<std::borrow::Cow<'static, [u8]>> {
    let mut roots = Vec::new();
    if let Some(windir) = std::env::var_os("WINDIR") {
        roots.push(std::path::PathBuf::from(windir).join("Fonts"));
    }
    roots.push(std::path::PathBuf::from("C:\\Windows\\Fonts"));

    for root in &roots {
        for name in file_names {
            let path = root.join(name);
            if let Ok(data) = std::fs::read(&path) {
                return Some(std::borrow::Cow::Owned(data));
            }
        }
    }
    None
}

/// Build the design font stack: the Monospace (data) family leads with embedded
/// JetBrains Mono Regular+Bold, the Proportional (chrome) family leads with
/// egui's embedded UI sans, and both keep egui's built-in glyph fallbacks plus —
/// when present on this machine — the Windows system faces (`Segoe UI`,
/// `Consolas`). System faces are consulted only for glyphs missing from the
/// leading faces and degrade gracefully by omission when absent.
///
/// Per ADR-0002 every leading face is embedded binary so text metrics are
/// deterministic across machines; system lookup would vary layout.
pub fn font_definitions() -> FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    defs.font_data.insert(
        JBM_REGULAR_KEY.into(),
        std::sync::Arc::new(FontData {
            font: std::borrow::Cow::Borrowed(JETBRAINS_MONO_REGULAR),
            index: 0,
            tweak: FontTweak::default(),
        }),
    );
    defs.font_data.insert(
        JBM_BOLD_KEY.into(),
        std::sync::Arc::new(FontData {
            font: std::borrow::Cow::Borrowed(JETBRAINS_MONO_BOLD),
            index: 0,
            tweak: FontTweak::default(),
        }),
    );

    // Optional system fallbacks: registered only when actually loadable.
    let segoe_ui_available = system_font_data(&["segoeui.ttf"]);
    if let Some(data) = segoe_ui_available.clone() {
        defs.font_data.insert(
            "Segoe UI".into(),
            std::sync::Arc::new(FontData {
                font: data,
                index: 0,
                tweak: FontTweak::default(),
            }),
        );
    }
    let consolas_available = system_font_data(&["consola.ttf"]);
    if let Some(data) = consolas_available.clone() {
        defs.font_data.insert(
            "Consolas".into(),
            std::sync::Arc::new(FontData {
                font: data,
                index: 0,
                tweak: FontTweak::default(),
            }),
        );
    }

    // Proportional = the embedded UI sans, so chrome labels stop painting in
    // the data face. Ubuntu-Light ships inside egui (`epaint_default_fonts`),
    // which keeps ADR-0002's rule intact: an embedded binary leads the chain,
    // never a system lookup, so metrics are identical across machines.
    // egui's built-in glyph fallbacks stay behind it, so a chrome label
    // carrying a character the sans lacks still resolves to an embedded face.
    let mut proportional = vec![UI_SANS_KEY.to_owned()];
    proportional.extend(
        defs.families
            .get(&egui::FontFamily::Proportional)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|face| face != UI_SANS_KEY),
    );
    if segoe_ui_available.is_some() {
        proportional.push("Segoe UI".to_owned());
    }
    defs.families
        .insert(egui::FontFamily::Proportional, proportional);

    // Monospace = JBM with optional Consolas fallback behind the defaults.
    let mut monospace = vec![JBM_REGULAR_KEY.to_owned()];
    monospace.extend(
        defs.families
            .get(&egui::FontFamily::Monospace)
            .cloned()
            .unwrap_or_default(),
    );
    if consolas_available.is_some() {
        monospace.push("Consolas".to_owned());
    }
    defs.families.insert(egui::FontFamily::Monospace, monospace);

    // Named bold family for real bold rendering via FontId.
    defs.families.insert(
        egui::FontFamily::Name("jetbrains-mono-bold".into()),
        vec![JBM_BOLD_KEY.into()],
    );

    defs
}

/// Install the embedded design font stack into the context (ADR-0002).
/// Call once at startup, before the first frame; takes effect at the
/// next pass begin.
pub fn install_fonts(ctx: &Context) {
    ctx.set_fonts(font_definitions());
}

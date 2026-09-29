# TurboGit design-system component roles

**Status:** authoritative role contract for the current UI crate

This document is the boundary contract for `crates/turbogit-ui`. It describes
what the current modules mean, which surfaces may reuse them, and which
similar-looking roles must remain separate. It is not a proposal to redesign a
surface. A caller may adopt a shared component only when its semantic role
matches; visual similarity is not enough.

**Design system v2 is decided and landing.** The shared-role sections below state
`ADR-0027`'s seven rules — R1 through R7 — and the decisions v2 makes about names
that are retired, narrowed or reversed, each with the rule that decided it and the
ticket that lands it. That means a rule and a call site can legitimately disagree
for a while: **the rule is current, the call site is v1 until its ticket lands**, and
each rule below names the ticket so a reader can tell which is which. The rules
*amend* the contract this document already states — dark-only, the 24px click-target
floor, no shadows, monospace-for-data — and they do not replace it.

The source of truth for this contract is the code in `crates/turbogit-ui/src`.
The role descriptions below use the implemented module and function names.
`ui::widgets` is the public compatibility façade over focused private modules
for controls, chips, rows, inputs, feedback, containers, and text utilities.
`ui::icons` remains the separate public owner of icon primitives, while
`ui::kit::conflict_pane` remains an internal specialized kit. These boundaries
and import paths are part of the contract.

## Non-negotiable boundaries

1. **`theme` owns shared tokens.** `turbogit_ui::theme` is the sole authority for
   shared colors, semantic status colors, surface roles, selection roles,
   typography, spacing, density, radii, and style/font installation. A component
   may derive a value from a token (for example, `tint_over_bg`) or use a
   documented theme alias, but it must not introduce a second shared palette,
   a page-local copy of a shared token, or a literal that claims a new shared
   role. Screen-specific geometry and explicitly specialized accents remain
   local only when their ownership is documented.
2. **A general widget has no feature policy.** General widgets own layout,
   interaction, accessibility metadata, and token-driven visual decisions. They
   do not call Git, read application state, dispatch operations, or decide
   branch, diff, conflict, or dialog policy.
3. **A feature component may own feature vocabulary.** Branch, diff, conflict,
   dialog, popup, and other screen-level components may know their own data and
   events. They must not become the home for unrelated general primitives.
4. **A specialized kit is not a general widget.** A kit may share a grammar
   among a small set of surfaces while retaining a specialized meaning. Its
   boundary is part of its contract; do not widen it merely to reuse a painter
   or a row fill.
5. **Do not collapse roles through a configuration-heavy universal widget.**
   A generic button is not automatically a branch button; a title is not
   automatically a section header; a chip is not a row; and an alert is not a
   toast. Preserve the semantic distinction even when two roles share
   geometry.

### The seven rules, and what owns each

Design system v2 (`ADR-0027`) adds seven rules to the contract above. They are
decided, and they land in the token and widget layer **before** any surface adopts
them, because a view cannot conform to a value that does not exist yet. Each rule
names the shared thing that owns it — owning it is what makes the rule a rule rather
than a per-call-site judgement — and the ticket that lands it.

| Rule | Stated as | Owned by | Lands in |
| --- | --- | --- | --- |
| **R1 — One accent** | `Palette::BRAND` fills only the view's single primary action, the active tab underline, and a selected row's 2px rail. Never behind running text, never a chip, never a state. | the token layer; asserted at the chip constructors and the row-fill decision | 05, 07, then every view |
| **R2 — Cards are surfaces, not boxes** | A content region is `CONTENT_BG` at `CARD_RADIUS` with **no stroke**; regions inside it divide by spacing and at most one `RULE_CONTENT` hairline; a 1px stroke means *this floats*. | the shared card. Card padding is already the panel padding and does not change, which is why losing the stroke does not collapse the region | 04, 19 |
| **R3 — Ink is a four-step ramp** | `INK` for names, values and the row being read; `INK_2` for body and secondary labels; `INK_3` for section labels, column headers, metadata and refs; `INK_4` for placeholders, dim path suffixes and hatches — and never as the only rendering of something the user needs. | the token layer, asserted as contrast maths over the values rather than as a comment | 02, then every view |
| **R4 — Selection is a composite** | A selected list row is `ROW_SELECTED` plus a 2px `BRAND` rail at its leading edge. Ink does not invert. The rail is paint, so a selected row's text origin equals an unselected row's. | the row-fill decision and one rail painter | 03, 07, 14 |
| **R5 — Raised controls step up** | Raised on the app background is `SURFACE`; raised on a `CONTENT_BG` card is the raised-on-card role, an alias of `SURFACE_2`. | the token layer's named raised-on-card role | 03, 04 |
| **R6 — Ref chips are neutral, state is coloured** | One `ref_chip()` — raised-on-card fill, `INK_2` text, `CHIP_RADIUS`, `TYPE_CHIP`, monospaced. The chip set is closed at ref, current and count. Semantic state is coloured text or a leading dot, never a filled pill. | the shared chips module, and the one `RepoState::color()` map for state colour | 05, 07, 16, 17, 18 |
| **R7 — One pane-chrome vocabulary** | Every tool pane: a 9px tracked `INK_3` title, an optional count chip, a right-aligned action slot, one hairline, then content. Column-oriented panes add one shared column-header row over a `RULE_STRUCTURAL` underline, with the header and the data rows deriving their offsets from the same constants. | one pane-header implementation, one column-header row, one row shell | 06, 07, 13, 14, 16, 17, 18, 19 |

Two things about those rows that are easy to get wrong:

- **`INK_3` is the only existing token whose documented contract narrows.** It stays
  legal on the app background, the panel background and the content surface, and it
  is **not** legal on a raised or selected surface, where a caller steps up to
  `INK_2`. That is R5's principle applied to ink instead of fills. `INK_4` is
  separate and stronger: it is sub-AA everywhere, which is why it is its own name
  rather than a darker `INK_3`.
- **`Palette::SELECTION` keeps its value and loses its scope.** It is the
  current-ref treatment, and no list row resolves to it — stated on the token and
  asserted at the row-fill decision, which is the only seam that can prove a
  negative. `Palette::ROW_SELECTED` is a *new* token, not a rename.

`widgets::card`'s scope narrows under R2 the same way: the default card paints no
stroke, and a bordered variant survives for surfaces that genuinely float. A view
that wants a border back is asking for a different surface, not for a different card.

### How each rule is ratcheted

A rule nobody can fail is a comment. Each rule below has at least one assertion, at
one of three altitudes, and every assertion names the rule it guards in its failure
message. Three suites carry the contract, and the negative rules are pinned at their
**construction sites** rather than at a render seam, because a render seam can only
see the sites a test enumerates.

| Rule | Assertion | Suite | Altitude |
| --- | --- | --- | --- |
| R1 | No chip fills the brand token, and exactly one chip wears an accent | `design_tokens.rs`, `widget_library.rs` | construction site + painted |
| R1 | A tool pane's one brand fill is its named primary action; everything else brand-filled is a rail | `widget_library.rs` (four panes), `branch_component_kit.rs` (the branch tree) | painted |
| R2 | A default card paints no stroked rect at all | `widget_library.rs` | painted |
| R2 | Every stroke in `src/ui` is a floating surface, a control's focus ring, or a rule — never a content region's edge (closed table) | `widget_library.rs` | construction site |
| R2 | The floating surfaces themselves wear the one window hairline | `design_tokens.rs` (`visuals.window_stroke`) | token maths |
| R3 | The four ink steps are distinct, ordered and ≥1.4:1 apart on the content surface | `design_tokens.rs` | token maths |
| R3 | `INK_3` clears 4.5:1 on the app / panel / content surfaces and is the only step that is sub-AA on *some* audited surfaces; `INK_4` is the only one sub-AA on *all* of them | `design_tokens.rs` | token maths |
| R3 | No chip fill, row fill or row-shell fill puts the muted ink on a raised or selected fill | `design_tokens.rs` | construction site |
| R3 | A column header and a section label are ramp steps and never the dim step | `widget_library.rs`, `branch_component_kit.rs` | painted |
| R4 | No list row resolves to the current-ref token | `branch_component_kit.rs` | construction site |
| R4 | A selected list row paints `ROW_SELECTED`, and its first text origin equals an unselected row's | `branch_component_kit.rs` | painted |
| R4 | Every list row's resting, hover and selected fill comes from the one row-fill decision | `branch_component_kit.rs` | painted |
| R5 | The raised ladder is strictly ordered and each rung steps up from its own host; `RAISED_ON_CARD == SURFACE_2` | `design_tokens.rs` | token maths |
| R5 | No raised control on a content card takes the raised-on-background fill (closed table of sites) | `design_tokens.rs` | construction site |
| R6 | The ref chip's fill is the raised-on-card role, at the chip radius, in the data face | `widget_library.rs` | painted |
| R6 | No chip fills or inks any colour `RepoState::color()` can produce, for **every** state | `design_tokens.rs`, `branch_component_kit.rs` | construction site |
| R6 | The state mark pair is a circle and words, with no chip geometry and no state vocabulary of its own | `branch_component_kit.rs` | construction site + painted |
| R6 | Exactly one function produces a ref chip, in one file, named in no screen module | `widget_library.rs` (tree), `git_log.rs` (the log's half) | construction site |
| R7 | Four panes produce identical header geometry from their own entry points | `widget_library.rs` | painted |
| R7 | `toolwindow_header` is declared nowhere and off the façade; one band height and one title treatment | `widget_library.rs` | construction site |
| R7 | Every column-oriented pane's header labels and row cells start at the same offsets | `worktrees_submodules_tabs.rs` | painted |
| R7 | The rail width is defined once and read by exactly three files: the token layer, the rail painter, the tab underline | `branch_component_kit.rs` | construction site |
| Purity | The branch view builder names no egui type, imports no styling module, and reaches for no git | `branch_component_kit.rs` | source, deliberately |

Two mechanisms in that table are worth naming because they are how the negative
rules are kept honest without a render seam: **closed tables** (the stroke sites,
the `RAIL_WIDTH` readers, the `RepoState` variants) fail when the tree grows and
name what grew, and every row of such a table carries the reason that row is
allowed; and **derived sets** (the state colours, the chip geometry's screen users)
are read from the source of truth rather than transcribed, because a transcribed
list is not exhaustiveness-checked and this migration has already been bitten by
one — the sixth `RepoState` compiled silently and two ratchets stopped covering it.

### Retirements and narrowings v2 makes

Recorded here as decisions, each with what it replaces, because a retirement left
only as a comment explaining an absence is a question nobody answered.

- **The shared row shell stays retired.** `widgets::tree_row` and the private row
  implementation it solely owned went in the dead-API sweep, and that decision is
  not revisited. *Row roles and selection states* below carries the rationale and
  the real cost. What replaces it is not the same name: **one row shell** owns row
  height per row kind, leading padding, the hover fill, the radius, and the selected
  fill plus rail, with one rail painter for the 2px accent — landed by ticket 07,
  extended for the virtualised log by ticket 14. The log takes a variant that
  accepts an already-allocated rect, because routing the hottest path in the app
  through a row shell that re-measures per row is a rewrite, not a sweep.
- **`widgets::toolwindow_header` is retired** in favour of one pane header — R7's
  title, optional count chip, right-aligned action slot, one hairline. It has five
  call sites today (worktrees, submodules, the log's branches strip, blame, and the
  widget-library suite's own harness) and the branches pane's differently-cased
  title is a fifth variant of the same idea, so standardising on it as it stands
  would enshrine a shape that was never drawn at this size. The conflict panes'
  private header is absorbed rather than left behind: nothing flags it as unused, so
  nothing would tell you it survived. Landed by ticket 06, which is also the ticket
  that updates the three places this section does not own, where the name is still
  described as live — the *Similar names with different roles* catalogue's "Titles
  and headers" entry, the `toolwindow_header` bullet under *General controls, inputs,
  and containers*, and the row in the weakly-adopted table below. They are left
  intact on purpose: the catalogue's job is to keep two similar names apart, and
  that job does not depend on either name still existing, so its entries are not
  reworded to match a symbol that has not been deleted yet.
- **The selection row states narrow to the three that are real.** The three today
  are the quiet `SELECTION` band, the solid `BRAND` band, and the translucent focus
  band. They become, in order: the **list row**, which is `ROW_SELECTED` plus the
  rail and is what `RowState::from_flags(selected, hovered)` resolves `selected` to
  from now on, instead of the solid brand band it hands every hand-painted row today
  — the loudest blue in the app; the **current ref**, which is `Palette::SELECTION`;
  and the **focus band** the log table, the sidebar tree and blame still use. One
  *meaning* goes with them: "the quiet tool-window active row" is not a role distinct
  from the current-ref band, so it disappears into it, and a current branch stays a
  repository fact rather than a pointer state. Landed by ticket 03.

## Ownership and visibility

| Family | Current owner and path | Boundary | Public or internal |
| --- | --- | --- | --- |
| Design tokens and style | `turbogit_ui::theme` | The only authority for shared presentation tokens, semantic mappings, typography, spacing, radii, and egui style/font setup. It does not own component behavior. | Public token and style API. |
| General widget vocabulary | `turbogit_ui::ui::widgets` (public façade) over private `widgets::{controls, chips, rows, inputs, feedback, containers, text}` modules | Reusable, policy-free controls and presentation primitives. The focused modules own implementation details; the façade explicitly re-exports the established names and signatures. | Public façade; implementation modules are private. |
| Branch component kit | `turbogit_ui::ui::components` (`KitButton`, `RowState`, branch fills, `section_header`, `pill`, sync helpers, and branch geometry) | Branch-screen grammar: fixed branch geometry, branch rows, sync relationships, current/count pills, collapsible branch sections, and branch actions. It is not a general button or title library. | Public branch-kit path, with a specialized role. |
| General icon primitive | `turbogit_ui::ui::icons` | Centered icon painting and icon lookup/painting primitives. It does not decide what an icon means. | Public shared primitive. |
| Branch feature components | `ui::branch_widget`, `ui::branches`, `ui::branches_tree`, and `ui::branch_tree_view` | Branch popup, branch status indicator, branch view model, and branch-tree rendering. The branch tree returns events and consumes caller-owned tree state; it does not perform Git operations. These remain feature components, not generic widgets. | Public feature modules with feature-specific APIs. |
| Diff feature component | `turbogit_ui::ui::diff` (`render_diff` and its diff data/actions) | Diff-specific data, panes, actions, and rendering. Diff rows and diff state must not be generalized merely because they share a row or chip primitive. The `actions`, `model`, `panes`, and `view` submodules are implementation details. | Public specialized feature component; internal submodules are not a general façade. |
| Dialogs, popups, and floating surfaces | `ui::dialogs`, `ui::popups`, `ui::push_dialog`, `ui::remotes_dialog`, `ui::interactive_rebase`, `ui::conflict_resolver`, `ui::bulk_monitor`, and related surface modules | Feature flows and their composition. Their lists, previews, status messages, and actions remain owned by the flow unless the role is genuinely generic. | Public feature modules where currently public; not part of the general widget vocabulary. |
| Internal specialized kits | `ui::kit::conflict_pane` under crate-private `ui::kit` | Shared three-pane conflict presentation grammar only. It owns pane geometry, headers, marker/side sections, and read-only result cells; parsing, resolution policy, editor state, and apply behavior stay in the conflict surfaces. | `pub(crate)` / internal. It must not become a second stateful editor or a public generic kit. |
| Application shell and feedback hosts | `turbogit_ui::ui::render` and `ui::mod` floating-surface orchestration | Composes shell, banners, dialogs, confirmation, and toast. The host owns placement and lifetime; individual feedback roles remain distinct. | `ui::render` is also re-exported at the crate root as `turbogit_ui::render`. |

The word “public” describes Rust visibility and the compatibility surface; it
does not mean that every public item is a general component. A public branch or
diff module is still a feature component.

## Similar names with different roles

### Buttons

- `widgets::ButtonVariant`, `ghost_button`, `primary_button`, `compact_button`,
  `icon_button`, and `compact_primary_button` are the **general button
  vocabulary**. They are sized and styled for reusable shell, dialog, toolbar,
  and action contexts. A general button chooses its own label and may optionally
  carry an icon.
- **`ButtonVariant::CompactPrimary` / `widgets::compact_primary_button` is the
  brand primary at the pane-header band height (28 px).** It is `Primary` and
  nothing else — the same fill and ink ladders arm for arm, asserted — because
  R1's primary is the one thing in the app that must not have two spellings. It
  exists because the vocabulary had a 32 px primary and a 28 px **ghost** and no
  brand button at the header band's height, and the gap had a cost: a 32 px
  primary in the header's action slot grows the band, which breaks the cross-pane
  header-geometry ratchet, so the one call site that needed it (the worktrees
  pane's "Add worktree") borrowed the branch kit's `KitButton::Primary` instead.
  A static assertion in the button module ties the compact height to
  `widgets::PANE_HEADER_HEIGHT`, so the two numbers are one decision rather than
  two that happen to agree. `ButtonVariant::Compact` (the 28 px ghost) and
  `CompactPrimary` are different *roles* at the same height: one asks no fill, one
  spends the accent.
- `widgets::compact_button_enabled` is a specialized branch-popup action gate;
  `widgets::action_button` is the specialized full-width stacked
  primary/secondary treatment. Neither replaces the general vocabulary.
  `action_button` has no production caller today: the log commit-details
  Actions section it was built for is gone (ADR-0024), and commit actions now
  live in the commit row's context menu. It is retained as a kit role for a
  surface that needs a stacked action block, and its own suite
  (`widget_library.rs`) keeps the treatment from drifting while it waits.
- `components::KitButton` and `components::kit_button` /
  `kit_button_at` are the **branch-kit button role**. They use the branch
  screen's compact 28px kit height, minimum target rules, four branch action
  variants (`Primary`, `Secondary`, `Quiet`, and `Danger`). `kit_button_at` is
  the explicit-width form: `kit_button` measures a label and calls it, and a
  caller that needs a column of equal widths passes its own. Since ADR-0023 no
  branch screen lays out such a column, so `kit_button_at` has no caller
  outside the kit. `Danger` is a branch/destructive action treatment, not a
  general `ButtonVariant` replacement.
- The two families may share theme tokens and state math. They must not be
  merged into one API until a separate design decision proves that their
  geometry, semantics, and call-site contracts are equivalent. A generic button
  must not be forced to carry branch width, branch action gating, or branch
  screen typography merely to make the APIs symmetrical.
- `components::kit_button_at` was deliberately **not** consolidated onto
  `widgets::paint_centered_text`, even though the two do the same two-axis
  centring arithmetic. It lays its galley out in `Color32::WHITE` so the width
  can be measured once and reused for the natural-width measure, then applies
  the state's ink at paint time with `galley_with_override_text_color`.
  `Painter::galley`'s colour argument is only a fallback for `PLACEHOLDER`
  vertices, so a white-laid galley painted through it stays white — folding the
  call in would put **every kit-button label on `WHITE`** regardless of variant
  or state. The site carries a comment saying so. This is a case where two
  similar-looking calls are not interchangeable, not a missed consolidation.

### Titles and headers

- `widgets::group_title` is a plain uppercase micro-header for a dialog, panel,
  or list. It paints text only: no band, no count, no chevron, and no toggle.
- `components::section_header` is a branch group **band** for `Local`, `Remote`,
  or another branch/smart group. It has `SECTION_BG`, a chevron, a live count
  pill, an expand/collapse hit target, and an optional trailing action. The
  strip toggle and a trailing action have separate interaction precedence.
- `widgets::toolwindow_header` is the tool-window title/action strip, not either
  of the above. The branch kit's `detail_panel_header` left with the detail
  panel it headed (ADR-0023), so a header for a future detail surface starts by
  establishing its own role.
- A surface-specific region band (for example, a repo block or a diff pane
  header) is not automatically a title widget. Its geometry belongs to that
  feature unless a shared role is explicitly established.

### Feedback and messages

These roles are intentionally different. Do not make a caller choose among
them solely because their text is a warning, error, or status.

| Role | Meaning and current implementation seam | Must not become |
| --- | --- | --- |
| Inline error | A short, one-line error associated with a field, control, or immediate operation. It is adjacent feedback, not a container; its error ink and wrapping policy are independent of multi-line failures. | A dialog error list, alert well, banner, or toast. |
| Cautions rail | `widgets::cautions_rail` is the titled count plus one warning-ink line per `RebaseCaution` a history rewrite computed. SHARED by every surface that rewrites history — the plan editor's right rail and the drop preflight — because "what is likely to go wrong here" is one set of facts with one wording. A clean plan paints nothing at all: an empty titled section is a section to read past. | A general `note`, an `alert_box` well, a banner, or a toast: it is a titled list of counts, not a contained message. |
| Recovery note | `widgets::recovery_note` states that a rewrite writes a backup ref before it moves anything, and names that ref. Shared with the cautions rail for the same reason — the promise that a rewrite can be undone, and the name of what undoes it, are one wording. The restore ACTION is not part of it: only a surface running a replay long enough for "abort" to mean anything owns that button. | A persistent warning that stays on screen, or a per-surface restatement of the same sentence. |
| Note | `widgets::note` is an inset `SURFACE_2` well for a preview, summary, or explanatory note. It has optional severity emphasis through the caller's stroke, but the default note is not an error surface. | An always-warning alert or a global banner. |
| Alert | `widgets::alert_box` is a contained warning/guardrail surface with `SURFACE_WARNING`, a warning icon, and warning ink. It is for a warning that needs a visible boundary. | A bare inline error, a general note, or a transient toast. |
| Banner | `ui::banner` renders a severity-tinted horizontal strip with a state-owned message and optional deep-link actions. `banner::maybe_show` is the app-wide host: the single entry point, called once from `ui::render`, and it owns whether a banner is showing at all. | A contained alert or a toast. It is anchored to a surface/application feedback state and may carry actions. |
| Toast | The private `render_toast` host in `ui::mod` renders a transient, bottom-right notice with a kind-colored accent/icon, a dismiss action, and the current four-second lifetime. | A persistent banner, inline error, or dialog list. Toast placement, lifetime, and dismissal are part of its role. |
| Dialog/conflict list | A dialog's error, warning, target, or conflict list is content composed by that dialog flow. A conflict list additionally uses the specialized three-pane conflict grammar in `kit::conflict_pane`: local/incoming sides, raw markers, and composed result cells. | A generic status chip, a toast, or a generic note. List order, count, selection, and resolution actions are feature semantics. |

A keyed read may share waiting/failure presentation when the behavior is
identical, but its empty state remains surface-owned. The shared contract is
about the waiting/failure treatment, not a universal replacement for diff,
blame, branch, or dialog content.

#### Feedback adoption audit (ticket 09)

The ticket-09 audit of note and alert consumers found that the existing
production consumers of `widgets::note` and `widgets::alert_box` already use
the shared contracts — rebase preview/results and dialog summaries use `note`,
and the commit-details guardrail warning used `alert_box` until ADR-0024 removed
that surface along with the actions it warned about — and that no
remaining raw or ad-hoc frame was semantically equivalent to either role enough
to migrate: the candidates were toast, banner, result-list, and conflict
feedback, all of which carry placement, lifetime, action, selection, or
resolution semantics that a note/alert well does not own. Those surfaces
therefore stayed specialized and unchanged, and no additional production
note/alert migration was appropriate.

## Chips, pills, rows, and selection

### Chip roles

The shared geometry is centralized in the public `ui::widgets` façade and its
private `widgets::chips` implementation module: `ChipGeometry`, `CHIP_GEOMETRY`,
`CHIP_HEIGHT`, `CHIP_PAD_X`, `chip_radius`, `chip_rect_right`,
`chip_text_origin`, and `paint_chip` (with the layout-level `chip` implementation
behind the public wrappers). That geometry is shared; the semantic wrapper is
not.

- `widgets::badge` with `BadgeKind` states a file status (`Neutral`, `Added`,
  `Modified`, `Deleted`).
- `widgets::RefKind` is the surviving **colour vocabulary** half of a retired
  role — see *The `RefKind` asymmetry* below. `RefKind::accent` is live: it maps
  `Branch`→`BRAND`, `Remote`→`STATE_SUCCESS`, `Tag`→`STATE_WARNING`.
- `widgets::hash_chip` is a commit reference with monospace content. It is a
  **label, not a control**: it senses hover only and carries no click, because
  copying a hash is the commit menu's Copy hash item, which states the same short
  reference (ADR-0024). It is a specialized reference chip, not an interactive
  reference and not a generic label.
- `components::pill` with `PillKind::Current` and `PillKind::Count` **no longer
  exists.** It was the branch-kit fact vocabulary; the branches screen was its
  only caller and both variants were replaced by the closed chip set (the current
  marker is `widgets::current_chip` / the word beside the row's band, a group's
  count is `widgets::count_chip`). The type, `pill` and `pill_width` were deleted
  once the last thing that constructed them — the "one brand-filled chip still to
  migrate" exception record in `tests/widget_library.rs`, which ticket 16's
  migration emptied — was deleted in the same edit. A second chip vocabulary
  nobody paints is how a vocabulary rots, and the v1 names below are the
  migration map, not live API.
- Branch-tree sync chips, current-branch chips, and any similarly local chip
  remain branch-feature roles even when they reuse the shared chip geometry.
  Do not replace a semantic wrapper with a color parameter and lose the meaning.

A chip is a compact fact marker. A pill is a branch-kit fact marker. A row is an
interactive or data-list unit. A selection state is a row-state decision. None
is a generic synonym for another. The shared chip vocabulary paints an
unstroked fill, so a chip that needs a border is a different role with no
shared equivalent and paints its own stroke.

#### The three chips, and the state that is not one (R6)

v2 closes the chip vocabulary at three, because at least four different things are
called a chip today and look like four different things. One function produces
each, so there is nowhere for a second opinion to come from.

| Chip | Treatment | Carries |
| --- | --- | --- |
| **Ref chip** | raised-on-card fill, `INK_2` text, `CHIP_RADIUS` (3), `TYPE_CHIP` (10), monospaced face | a ref name: branch names, remote refs, worktree branches, the log's ref pills |
| **Current chip** | the selected-row fill with accent text | "this is the current ref" — the **only** chip permitted to carry the accent, and only because that is a fact about a ref rather than a call to action |
| **Count chip** | the raised surface fill, secondary monospaced ink | an ad-hoc counter, including every counter that was borrowing the reserved counter orange for something that was not a dirt or unpushed count |

The two neutral chips take **the raised role for the surface they sit on** (R5). The
ref chip's named treatment is the raised-on-card role because it mostly sits on a
`CONTENT_BG` card; the count chip's is the raised surface because it usually sits in a
chrome row that is already raised. That is the same decision a button on a card makes,
for the same reason: a control that does not step up from the surface it is on is
invisible.

**Semantic state is not a chip.** `dirty`, `↓ 2 behind`, `Up to date` and
`Needs update` render as **coloured text or a leading dot**, coloured from the one
`RepoState::color()` map that already exists and is already asserted to be app-wide.
A dot is a *mark*: a filled circle of the dot's radius, with no fill rect, no radius,
no chip geometry and no node of its own. This is what removes the blue soup from the
branches screen, and it is the rule the submodules status label already follows —
see the reversals below.

**What "a state colour" means, settled (the seventh variant).** The map has seven
states and only **three colours** that name a severity, so "no chip may wear a
state colour" had to be answered twice: once for the severities and once for the
state that is not one. `RepoState::Uninitialized` — a submodule registered but never
checked out — is **work the user is owed**, not something that went wrong, so it
wears `Palette::INK_3` and the app invents no fourth tone. The decision recorded
here is that the *severity table* stays at three (`COUNTER`, `AHEAD`,
`STATUS_DIVERGED`) and `INK_3` is **not** added to it, because:

- the table's role is severity, and the seventh variant exists precisely to draw a
  line between the two kinds of thing. A fourth row called "state colour" would
  erase it on the next state that arrives;
- the rule that actually protects the chip vocabulary is stronger and already
  written: *no chip fills or inks any colour `RepoState::color()` can produce*. That
  form covers `INK_3` without calling it a severity, and both contract suites assert
  it over the whole map rather than over a transcribed list of colours;
- `INK_3` on a chip is not an accessibility question but a *shape* question — an ink
  step behind a fill is a mark wearing a control's geometry, which is the thing R6
  rules out — and the shape question is answered without the table.

The consequence for callers is unchanged and is the one the token layer documents:
`INK_3` is legal on the app, panel and content surfaces, which is where a
submodule's state and a repository row's state actually sit; a caller that puts a
state on a raised or selected surface steps up to `INK_2`, exactly as any other
muted text does.

Three consequences worth stating because they are what a later sweep gets wrong:

- **`widgets::badge` with `BadgeKind` is not one of the three chips.** A file-status
  letter keeps the pill radius (9) and its own slot; the ref chip's compact radius
  (3) is a different shape for a different job. Both radii stay distinct and both stay
  pinned — they are not merged, and the badge is not reclassified as a chip to tidy the
  vocabulary.
- **A ref chip that is a label does not gain a click plane.** `widgets::hash_chip` is
  a label by decision — copying a hash is the commit menu's item, per `ADR-0024` —
  and a ref chip inherits that. A chip that *is* a control is a different thing and
  carries its own accessibility node; the one chip in the app that is also a control
  is the branches scope chip, and the reversals below say why.
- **Where the v1 names mapped, and where they went.** `components::pill`'s
  `PillKind::Current` became the current chip and `PillKind::Count` became the
  count chip; the branch tree's sync chips and the welcome roundel keep their own
  roles for the reasons in the per-site verdicts below and are not swept onto the
  ref chip. Landed by ticket 05, and the v1 names themselves were **deleted** in
  the same change that retired the last exception record that constructed them
  (ticket 21) — so this row is a migration map, not a live vocabulary.

**The log's ref markers are settled, and they are not ref chips.** The question
this section used to leave open — `RefKind::accent` is a colour vocabulary where
`Branch`→`BRAND` and R6 makes the ref chip neutral, and the log's own markers had
to be placed somewhere — is answered by the log's ticket: the collapsed marker
carries **no ref name** (the names are in the hover tooltip), so spending the
compact chip radius on it would make that radius mean two things. It stays the
**neutral badge in the pill slot**, which is a different shape for a different
job, and R6's one-function-per-chip property survives intact:
`widgets::ref_chip` is the only thing in the app that produces a ref chip. The
log's half is pinned from paint and from source in `tests/git_log.rs`; the
whole-tree half — `REF_CHIP_COLORS` and the compact radius named in the chip
module, the branch row's `ref_chip_with_ink` variant and the façade's re-export
list, and in **no screen module** — is in `tests/widget_library.rs`. The
`RefKind` asymmetry below is otherwise untouched by v2 and must not be "tidied"
in the meantime.

#### The `RefKind` asymmetry (intentional)

The dead-API sweep retired the ref-chip **render function** (`widgets::ref_label`)
and deliberately **kept** the ref-kind **type and its accent mapping**
(`widgets::RefKind`, `RefKind::accent`). That is not an oversight and must not
be "tidied" in a later sweep.

- `ui::log_window` owns a private adapter that maps its own `GitRefKind` onto
  the shared `RefKind::accent` vocabulary to colour log references, and it
  **never renders a ref label** — the log surface paints its own hand-painted
  label pill (see *Log surface pills* below). So `RefKind` is a live *colour*
  consumer, which is exactly why an adoption count reads it as dead and is
  wrong.
- What went with the render function is `RefKind::colors` — the solid-pill
  `{bg, fg}` colour pair whose only caller was `ref_label`. Once the function
  was gone the pair was orphaned, and leaving it would have failed the
  workspace's warnings-as-errors lint gate. It is a derivation, not a role, and
  it can come back the day a surface genuinely paints a solid ref pill.

If a future surface does need a ref-label chip, the honest move is to add the
render function back **and** `RefKind::colors`, and to migrate a real caller
onto it in the same change — not to keep a colour-pair helper alive on
speculation.

#### Per-site chip verdicts (ticket 08)

Five surfaces in the app look like the shared chip without being the same role.
None of them was migrated. Each was adjudicated individually and is recorded
here with the reasoning, not only the outcome, so the next maintainer inherits
the judgement instead of re-deriving it. The verdict vocabulary is **migrate**,
**keep with a named token**, and **keep because the role differs**.

`ADR-0027` then changes one of these five verdicts and confirms another, so the
table is the v1 audit and the two rows it touches carry the current decision. Both
are argued in full under *Two decisions v2 reverses, and one it ratifies* below:
the branches scope indicator becomes a real clickable chip, and the submodules
status indicator stays coloured text.

**No site in this table was marked _migrate_.** The one chip-like surface that
did move — the log file-row status pill — already had, and is recorded under
*Log surface pills* below. Every other candidate is a case where adopting the
shared chip would cost an icon, a fixed numeral, or a nested button; those are
design losses, not refactoring debt.

| Site | Where | Verdict | Reasoning |
| --- | --- | --- | --- |
| Branch-tree sync chip | `ui::branch_tree_view::sync_chip` | keep with a named token; the role also differs | It carries a 10px status icon and monospaced `TYPE_CHIP` type inside its fill, which is the branch tree's sync-relationship vocabulary. The shared chip geometry has no icon slot and no mono variant, so adopting it would mean either dropping the icon or growing a config flag on the shared primitive — both of which breach non-negotiable boundary 5. Its radius is already the named `theme::CHIP_RADIUS`, and its fill already resolves through the shared `components::sync_bg` (which is itself now written as `tint_over_bg(sync_ink(kind), BADGE_TINT)` rather than a repeated `0.18`). |
| Welcome step indicator | `ui::welcome::step_pill` | keep with a named token; the role also differs | A fixed 20×20 `SURFACE_3` numeral roundel, not a variable-width fact marker: its width is its height because the content is a single digit, and it is numbered 1–5 by position in a list. It already takes its radius from the named `theme::PILL_RADIUS` (which is itself defined as the welcome roundel's half-height, so the two can never disagree) and its centring from the shared two-axis text helper. The roundel's 20px edge is screen-specific geometry and stays a local `STEP_PILL` constant. |
| Branches scope indicator | `ui::branches::scope_label` | **reversed by `ADR-0027`: becomes a chip** | The v1 verdict was "not a chip — coloured text with no background, no fill, no radius and no padding, renamed from `scope_chip` to say what it is". R7 overrides that, and the argument it overrides is answered rather than overruled. See *Two decisions v2 reverses, and one it ratifies* below; landed by ticket 16, which also rewrites the comment this row vouches for. |
| Submodules status indicator | `ui::submodules::status_label` | keep because the role differs — **and R6 ratifies it** | **Still not a chip.** It paints coloured text with no background, no fill, no radius, and no padding. Renamed from `status_chip` to say what it is. The leading state dot ticket 18 adds to that row is a *mark*, not a chip, so the comment stays true of the code it describes. See the same subsection below. |
| Settings removable pattern chip | `ui::settings_modal::pattern_chip` | **genuine gap, no shared equivalent** | A `SURFACE_2` frame carrying a label *and* a `×` remove control that is a real button with its own accessibility node (`Remove <pattern>`). The shared chip vocabulary has no slot for a nested interactive control, so there is no equivalent to adopt. Its radius is the named `theme::CONTROL_RADIUS`. **Deferred:** whether to add a shared "removable chip" primitive or to document this as a page-local frame is a decision for its own ticket. |

**The two shell chip sites left with the repo header.** `shell::branch_pill`
and `shell::dirty_badge` were adjudicated here while that header existed.
Deleting the header deleted both functions, so their rows are removed rather
than reworded, the same way the retired `status_badge` row below was. The one
judgement among them that was not site-specific is now a rule of the shared
chip vocabulary, in *Chip roles* above: a bordered chip has no shared
equivalent.

#### Two decisions v2 reverses, and one it ratifies

Two code comments in the tree read, near-word-for-word: *"Not a chip, and
deliberately not one: it paints **coloured text with no background at all**, so it
shares no geometry, no radius and no fill with the shared chip vocabulary. Turning
it into a real chip would introduce a background where none exists and would newly
register an accessibility node for a piece of status text — a design change that
needs its own ticket, not a consolidation. See `docs/design-system-roles.md`."* They
sit on `ui::branches::scope_label` and on `ui::submodules::status_label`, they point
at **this** document by name, and they are indistinguishable to anyone who has not
read to here. That is exactly why the answer lives here: one of them is reversed, one
of them is confirmed, and a rule that disagrees with a comment but is not written
anywhere else is a rule the tree will contradict twice.

The general rule both comments are instances of survives unchanged: **a chip is a
filled, bounded fact marker; a coloured label is not one, whatever shape it
resembles.** What changes is whether the branches scope indicator is a label.

**1. The branches scope label — REVERSED. R7 overrode the earlier decision, and here
is why that decision was right and is still being replaced.**

The earlier decision was made by the comment above, under the rationale that giving
the scope indicator a background would (a) introduce a filled surface where there is
none and (b) newly register an accessibility node for what is status text. Neither
objection is dismissed; the first is answered by the chip's own treatment and the
second by a change in what the thing *is*.

- **Which rule overrode it: R7**, one pane-chrome vocabulary. R7 gives every tool
  pane one chrome row — title, optional count chip, right-aligned action slot, one
  hairline — and the Branches pane's scope is a **filter**. A pane that tells you
  which repositories it is showing, in a row that exists to be acted on, and then
  offers the filter in a separate quiet button beside it, splits one decision into
  two affordances. Under R7 the scope indicator is not status text at all: it is the
  pane's scope selector, and the click is what changes the repository filter.
- **The accessibility argument answered, not overruled.** The objection was that a
  chip would "newly register an accessibility node for a piece of status text". The
  objection holds for a *label* and does not hold for a *control*. Once the scope
  indicator is something you press to change what the list below is filtered to, the
  node it registers is the node for a control with a state and an action, and a
  screen-reader user gains the filter rather than losing status text. The scope
  indicator is the one chip in the app that is also a control, and it must carry its
  state in its own accessible label — the repository count, or the repository it is
  filtered to — so the node is self-describing rather than announcing a bare word.
- **The visual-weight objection answered by R6, not waved away.** A filled surface
  does appear, and that is a real change to the row's visual weight against its
  neighbours. What keeps it from competing is R6: the scope chip is one of that
  rule's closed set of three and takes the **count chip's** treatment — the raised
  fill for the surface it sits on, with secondary ink — so it is a neutral fact
  marker, and the one blue object left in the pane is the New Branch button. R1 is
  what forbids this from becoming another blue pill. The three treatments are the
  table under *The three chips, and the state that is not one* above, and if ticket
  16 finds that a fourth shape is needed, **R6 is the rule to reopen** — in writing,
  here, not silently in a call site.
- **What is being replaced, precisely.** The comment's claim that a chip is "a design
  change that needs its own ticket, not a consolidation" is the sentence that stays
  true: this *is* its own ticket, it is ticket 16, and it is a design change rather
  than a refactor. The comment is wrong only about which way the design went.
- **What happens to the comment.** Ticket 16 implements the chip **and deletes or
  rewrites that doc comment in the same change.** Until it lands, this paragraph and
  the comment it reverses disagree, and this paragraph is the one that says which is
  current. Recording the reversal here is not permission to leave the stale comment
  sitting under working code: a later reader who finds the comment and not this
  subsection has been handed the wrong answer with equal confidence.

**2. The submodules status label — KEPT. No rule overrode it; R6 is the rule that
confirms it.**

The comment is the same and its reasoning is the same, and the reasoning is correct.
**R6 says semantic state renders as coloured text or a dot and never as a filled
pill**, so v2 agrees with the decision that was already made rather than reversing
it — and R6 is a rule rather than a preference, which is what turns a comment into a
contract. `Up to date`, `Needs update`, `Uninitialized` and `Conflicts` stay coloured
text with no background, no fill, no radius and no padding, taken from the one
repository-state colour map.

- **The leading state dot added to that row is a mark, not a chip.** A filled circle
  of the dot's radius beside the status word, so a column of statuses is scannable;
  no fill rect behind the text, no radius, no chip geometry, no accessibility node of
  its own. A chip is a bounded container; a dot is a mark. The distinction is the
  reason the comment stays true of the code it is attached to rather than becoming
  half-stale.
- **The action beside it does change, and that is not a chip change.** On an
  uninitialised submodule the action is labelled `Init` rather than `Update`, because
  the code already passes `init: true` and the operation is really an init. That is a
  label fix about what pressing the control will do, and it has nothing to do with
  whether the status word is a chip.
- **Landed by ticket 18, which keeps the comment** and keeps it accurate. It does not
  delete this comment the way ticket 16 deletes its twin, and the asymmetry between
  the two sites is the decision — not an oversight in either direction.

The trap this subsection exists to close: the two comments are near-identical and only
one of them is reversed. Copying the branches treatment onto the submodules status word
because the comment looks the same is the mistake; so is deleting the submodules
comment because the branches one went. If you find yourself justifying either text
label as a chip, or deleting either comment, you are past a decision that was made.

### Log surface pills

`ui::log_window` paints two pills, and they are not the same problem.

- The **status pill** (the file-status letter on a decorated commit row) now
  uses the shared `widgets::ChipGeometry` through a named local
  `STATUS_PILL: widgets::ChipGeometry`, built from the shared `CHIP_HEIGHT` and
  `CHIP_PAD_X` and pinning `radius` to `theme::CONTROL_RADIUS` — the radius the
  row had before, so adopting `ChipGeometry::paint` moved no pixel. A
  `ChipGeometry` *value* rather than the `CHIP_GEOMETRY` constant is the point:
  the log row is a status pill wearing the control radius, and naming it says so
  instead of implying it is the default chip shape.
- **`paint_label_pill`** — the icon-only tag pill — deliberately stays
  hand-painted. It already sits on the shared chip tokens (`CHIP_HEIGHT`,
  `CHIP_PAD_X`, `chip_radius()`, `BadgeKind::Neutral`), and it cannot use
  `ChipGeometry::paint` at all because that function takes a galley, which an
  icon-only pill does not have; its content is centred icon arithmetic, not text
  centring. The site carries a comment saying so.

### Row roles and selection states

The row layer is **one painter and one state grammar**, not a row widget — and under
v2, one row shell and one rail painter on top of them.

- `widgets::paint_row(ui, rect, state)` is the general row fill: it paints the
  `row_fill(state)` colour into a rect the caller has already allocated, with the
  `CONTROL_RADIUS` corner every such row has always used. It is the shared
  form for the hand-painted tables — the settings category rail, the rebase
  todo, the commit window's file rows, the multi-selection body, the welcome
  and log rows — that allocate a rect, paint behind their content and want the
  one row-state decision. A surface that rounds its rows differently (the
  sidebar's full-bleed band, blame's dense rows) keeps its own corner; that is a
  real geometry difference, not a second row role. It survives v2 as the fill
  primitive; what v2 adds is the geometry above it and the rail beside it.
- **`widgets::tree_row` no longer exists, and that decision is not revisited.**
  The fixed-height tree/list row wrapper was retired by the dead-API sweep together
  with the private row implementation it solely owned, because nothing outside its own
  characterization test ever called it. It is *not* a universal row for commits,
  files, diff lines, or conflict panes, and it was never going to become one. The
  retired function's interaction target (the row owning hover and click across its
  full rect) is the reason a caller that needs one now lays its own row out and calls
  `paint_row`; that is a real cost of the retirement, recorded here rather than
  hidden. `ADR-0027` keeps the retirement — see *Retirements and narrowings v2 makes*
  under *Non-negotiable boundaries* — and puts something narrower in its place rather
  than the same name back.
- **One row shell, one rail painter (R4, R7).** Rows stop hand-rolling their own
  geometry: one row shell owns row height per row kind, leading padding, the hover
  fill, the radius, and the selected fill plus rail; one rail painter owns the 2px
  accent and absorbs the two independent rail sites that exist today (the sidebar's
  stroked segment and the diff pane's filled rect), so every row that grows a rail
  places it identically. This is what makes "adding a list does not mean inventing a
  selection" true rather than aspirational. Landed by ticket 07.
- **The log is the one documented exception, and the design accounts for it.** The
  log is virtualised — its own row allocation, its own measured pitch, its own
  eliding, its own paint helpers that bypass the shared vocabulary entirely — so the
  row shell takes a **variant that accepts an already-allocated rect**: the log keeps
  its pitch and its virtualisation and shares the fills, the rail and the ink ramp.
  Routing it through a shell that re-measures per row would be a rewrite of the
  hottest path in the app, and `ADR-0026`'s frame budget is why that is not a
  symmetry worth buying. Log conformance lands last in the view phase for the same
  reason. Landed by ticket 14.
- `components::RowState`, `row_fill`, and `current_row_fill` belong to the
  branch/shared row-state grammar. The branch row and repo
  section add branch-specific geometry,
  current-branch facts, sync markers, stale ink, and operation labels on top of
  that grammar.
- Commit rows, file rows, diff rows, blame rows, log rows, rebase rows, and
  conflict result cells remain feature/page rows. They may use `row_fill` or
  `paint_row` for a compatible fill, but their content, interaction, and state
  models stay specialized.

The selection roles in `components::RowState` are not aliases. v2 leaves `Default` and
`Hover` alone and narrows the three selection roles to the three that are real:

- `RowState::Default` is the resting, transparent row.
- `RowState::Hover` is the shared `SURFACE_2` interaction band.
- **The list row** is `Palette::ROW_SELECTED` plus a 2px `Palette::BRAND` rail at the
  leading edge. Ink does not invert: a selected row's text is the colour an
  unselected row's text is, so selecting a row does not cost the user the ability to
  read it. The rail is **paint, not layout** — reserving its width as padding would
  shift every row's text origin, and *a selected row's first text origin equals an
  unselected row's* is a criterion of the rule, not a detail. This is the role
  `RowState::BrandSelected` becomes: the solid `Palette::BRAND` fill with `BRAND_INK`
  content, which `RowState::from_flags(selected, hovered)` — the boolean
  `(selected, hovered)` mapping the retired `tree_row` API handed out — resolves
  `selected` to today, at five production call sites (settings modal, interactive
  rebase, log window, welcome, multi-selection). The loudest blue in the app is
  currently the default selection for hand-painted rows; this is the change that
  stops it being one.
- **The current ref** is `Palette::SELECTION #2E4369`, and it is a selection role
  only for "this is the current ref". It is the old `RowState::Selected` band with
  its scope narrowed: the *quiet tool-window active row* is not a role distinct from
  the current-ref band, so that meaning goes with the rename and the value stays.
  **No list row resolves to it** — stated on the token and asserted at the row-fill
  decision, which is the only seam that can prove a negative.
- `RowState::FocusSelected` is the translucent `Palette::selection_bg()`
  focus band used by the log table, sidebar tree, and blame surfaces. It is not
  the opaque `Palette::SELECTION` fill, and it is one of
  the three survivors rather than a duplicate of any of them. **The left rail's
  active row is one of its consumers, deliberately** — R4's "a selected list row is
  `ROW_SELECTED` plus a rail" is about a row *of a list*, and what the rail paints
  is a full-bleed tree selection spanning a workspace tree (zero corner radius, the
  rail's own edges), which is a *region* that is chosen rather than a row picked out
  of many. The two fills are also the same colour to the eye: composited over the app
  background the focus band is `#233455` against the list row's `#243456`, and over
  the left rail's own (darker) surface `#213252` — inside a rounding step either
  way, so the swap would change which *role* the band claims and nothing the user can
  see. `tests/branch_component_kit.rs` pins both halves, so revisiting it starts
  from the numbers rather than from a preference.
- A current branch is a repository fact, not a pointer state.
  `components::current_row_fill` keeps its resting brand tint distinct from
  hover and selection, and keeps selection visible when the current row is also
  selected.
- Callers that need a selection role other than these three must construct it
  explicitly. Selecting one of these states is a visible decision. Do not collapse
  them to a single boolean or infer that “selected” means the same thing in every
  list.

#### The retirement sweep was name-aware

The sweep that removed the four dead widget APIs was **not** a blind name
search, and it could not have been: three live sites contain a retired API name
as a **substring**, and a substring match would have deleted working code and
working coverage.

| Site | Contains | What it actually is |
| --- | --- | --- |
| `crates/turbogit-ui/src/ui/worktrees.rs` — `fn worktree_row` | `tree_row` | A page-local hand-laid row for one worktree. |
| `crates/turbogit-ui/tests/git_log.rs` — `fn ref_labels_collapse_into_a_single_pill_revealed_on_hover` | `ref_label` | Live coverage of the log surface's own hand-painted label pill. |
| `crates/turbogit-ui/tests/git_log.rs` — `fn changed_files_pane_lists_selected_commit_files_with_status_badges` | `status_badge` | Live coverage of the log pane's file-status letter chips. |

Each of those three sites now carries a comment saying so. Two further names
were checked and are **not** collisions, so they were left alone:
`tests/commit_window.rs`'s `diff_header_shows_path_status_chip_stats_and_nav_on_selection`
contains `status_chip` (a diff-header role, never `status_badge`), and
`tests/branch_tree_view.rs`'s `a_current_row_keeps_its_status_chips` contains
`status_chips` for the same reason. Run a word-boundary search, or read the
site, before deleting anything by name.

## Hairline roles

A one-pixel line used to be picked by whichever of the app's three line tokens
was nearest, so several genuinely different uses of a hairline shared a look
they had no reason to share. The three roles the app actually has are now named
in `theme`, and they are named by **role**, not by appearance.

| Role | Token | Alias of | What it is |
| --- | --- | --- | --- |
| Content divider | `Palette::RULE_CONTENT` | `RAISED` = `SURFACE` | A 1px rule between sibling content regions *inside* one surface. The weakest of the three: it separates, it does not bound. |
| Footer rule | `Palette::RULE_FOOTER` | `Palette::LINE` | The 1px rule separating a modal body from its action slot. |
| Structural rule | `Palette::RULE_STRUCTURAL` | `Palette::LINE_SUBTLE` | Table header underlines, tree indent guides, and panel edge rules — the chrome that gives a surface its structure, rather than dividing its content. |

All three are **pure aliases**: no token value moved, and the pre-existing
contract (`DIVIDER == RAISED`, `DIVIDER != LINE`) is untouched.

### Which hairline a pane is allowed, and how many (R2, R7)

v2 does not add a hairline role. It decides **where a line may appear and which of
the three names it takes**, which is what turns "picked whichever token was nearest"
into a rule a test can pin.

- **R2 — a card's interior is divided by spacing and at most one hairline**, and that
  one is the **content divider**: a 1px rule between sibling content regions inside
  one surface. A region is carried by its fill and its padding; the hairline is
  available for a division that spacing cannot express, once. "At most one" is a
  negative rule, so it is pinned at the sites a suite enumerates rather than by
  scanning for strokes.
- **R7 — a pane's chrome row is a title, an optional count chip, a right-aligned
  action slot, one hairline, then content.** That hairline is the **content divider**
  again — it divides the chrome from the content inside one surface — and it is *one*.
  Two rules between a header and its rows is the nested-boxes problem this migration
  exists to fix, and it reappears quietly, which is why the count is stated rather than
  implied.
- **A column header's underline is the structural rule.** It is not a divider between
  content regions; it is the chrome that gives a table its structure, sitting under a
  header row that is itself chrome. Structural is the right name for it for the same
  reason it is the right name for a tree indent guide.
- **A modal's footer rule is the footer rule**, and it stays the strongest of the
  three for the reason below. R2's stroke-means-it-floats rule is why a dialog keeps
  its 1px border at all: it is a floating surface.

The useful consequence is arithmetic rather than aesthetic: a pane can now be
*counted*. Title, count, action slot, one divider, then content, plus at most one
interior divider and one structural underline for a column header — so "one hairline,
then content" is a claim about a number of rules rather than an opinion about a
screenshot, and "one hairline" is the phrase a reviewer can check. The mismatches
recorded in the rest of this section — the shell's panel edge rules wearing the
footer-rule value for a structural role, and egui's unowned fourth tone — are
unchanged by v2 and stay the recorded follow-ups they already are. What v2 changes is
that a sweep against them now has a list of allowed positions to sweep onto.

**The footer rule being stronger than the content divider is intent, not
drift.** A modal's action slot is a *boundary* — it is where a decision is taken
— while a content divider merely divides one region from another. `RULE_FOOTER`
is therefore `#4E5157` and `RULE_CONTENT` is `#2B2D30`, and the
design-token suite asserts both that they differ and that the footer rule's
relative luminance is genuinely higher, so the distinction cannot quietly
collapse into "both are grey lines".

The shared painter for the footer rule is `widgets::footer_rule` (crate-private,
re-exported to sibling surface modules through `ui::widgets`), which
`widgets::dialog_footer` delegates to. The one rendered change in this work is
the interactive rebase editor's body-to-footer rule, which moved off egui's
default separator onto that shared treatment so it matches every other modal
footer. The reserved band around it is unchanged — see
`crates/turbogit-ui/tests/interactive_rebase_editor.rs`, which pins the rule's
tone *and* its 1px height, full-content-width span, and position relative to the
tab-strip rule and the footer row, because no acceptance capture covers that
page.

`RULE_STRUCTURAL`'s current users were checked to be genuinely structural
rather than dividers, and one genuine mismatch was found and left for the
follow-up sweep below: the commit window's tree indent guide
(`commit_window::indent_guide`), the multi-selection summary's header rule
(`multi_selection`), and the sidebar's selection-bar top edge (`sidebar`) all
wear `LINE_SUBTLE` and are all chrome; but the shell's own panel edge rules
(`shell::paint_edge_line_at` — the tab-strip and status-bar edges) wear `LINE`,
the *footer-rule* value, even though their role is structural. Their role is
identified here; their value is not re-pointed, because this ticket is about
naming roles and not re-sweeping call sites.

### A fourth hairline tone that no token owns

egui's default `ui.separator()` paints
`visuals.widgets.noninteractive.bg_stroke`, and `theme::dark_visuals` **never
assigns that field**. It starts from `Visuals::dark()` and overrides fills and
`fg_stroke`, so `bg_stroke` keeps egui's stock `Widgets::default()` (==
`Widgets::dark()`) value: `Stroke::new(1.0, Color32::from_gray(60))`, i.e.
`#3C3C3C`. `dark_visuals` *does* set `noninteractive.fg_stroke` to
`Stroke::new(1.0, Palette::INK_2)` (`#B0B3BB`), but a `Separator` does not read
it — an earlier version of this paragraph credited the separator's tone to
`INK_2`, which is wrong.

The finding itself is unchanged and the value is worth stating: `#3C3C3C` is
distinct from all three named roles — `RULE_CONTENT` `#2B2D30`, `RULE_FOOTER`
`#4E5157`, `RULE_STRUCTURAL` `#36383C` — so this is a real, fourth hairline role
that no design token owns, and it is recorded here as a finding. (It is
nonetheless close to `RULE_STRUCTURAL`; the follow-up sweep below should expect
several of these sites to resolve onto the structural role rather than needing a
fourth token.)

Migrating its ~37 call sites onto the three named roles is **follow-up work**,
deliberately left undone, because each site needs its own role judgement and
several are deliberately not hairlines at all. Two families are already settled
and must not be swept:

- **The diff toolbar's inline separators** (`ui::diff::view`, the four
  `ui.separator()` calls at lines 72, 76, 79 and 81 between the mode control,
  comparison chips, granularity toggle, hunk navigation, and the whitespace
  checkbox) **stay inline dividers**. They sit inside a `horizontal_wrapped`
  toolbar row, so egui paints them as *vertical* rules between neighbouring
  controls (`is_horizontal_line` is false whenever the layout's main direction is
  horizontal). A blanket migration that replaced them with a full-width
  horizontal section rule would turn a control group into a stack of full-width
  sections — the exact failure boundary 5 warns about.
- **The command palette's search-field separator** (`ui::popups`, line 295, in
  `command_palette`) is recorded as **questionable**. The action list beneath it
  deliberately compresses its rhythm — `item_spacing.y = 2.0`,
  `interact_size.y = 16.0` — and a full-width 1px rule, with its own 6px reserved
  band and 6px item spacing on either side, fights that compression. It is a
  candidate for removal rather than for a named role, and that call belongs to
  the follow-up.

## General controls, inputs, and containers

The following are general roles in `ui::widgets`, subject to the same
token-only rule:

- `ButtonVariant` and its button functions own reusable interaction states,
  focus rings, labels, and hit targets.
- `segmented_control` is a general token-exact two- or three-way picker. It is
  not a branch group header and not a selectable row.
- `text_input` and `search_input` own input framing, hints, and focus rings;
  search adds the leading search icon. Input validation and the surface's
  surrounding inline error remain separate.
- `dialog_footer` owns the shared modal action separator/alignment. The rule
  itself is `widgets::footer_rule`'s — a full-width 1px `RULE_FOOTER` band, so
  a modal body ruled off from its action slot can adopt the same treatment
  without owning a footer. `dialog_footer` keeps the 6px gap below the rule and
  the right-aligned action slot. It does not own the dialog's action policy or
  its error/list content.
- `toolwindow_header` owns the shared tool-window title/action strip. It does
  not own a feature's body or state.
- `card`, `card_header`, `note`, and `alert_box` are container roles with the
  distinct meanings described above. A page that needs a different containment
  may keep a local frame, but should not call that frame `note`, `alert`, or
  `card` merely because it is visually similar.
- `focus_ring`, `mix`, and `tint_over_bg` are shared presentation helpers, not
  general interaction components. `focus_ring` preserves keyboard focus for
  hand-painted controls; origin-based icon positioning and centered icon
  painting remain separate operations.

### General primitives added by the consolidation

These are part of the boundary now and are recorded here so the document stays
authoritative. All are exported from the `ui::widgets` façade except
`footer_rule`, which is deliberately crate-private (see below) and therefore
never part of the public compatibility surface.

- `accent_bar` is the narrow tinted strip that leads a feedback container down
  its leading edge. It is the *general* form: it takes a colour the caller has
  already resolved and owns no severity vocabulary, so a banner and a toast get
  the same width, height, and rounding without either one deciding what "error"
  means.
- `ghost_icon_button` owns the fill, ink, corner radius, accessibility node, and
  the single brand focus ring for an icon-only ghost control. The rect is a
  *parameter*: a host either allocates a standard square, or allocates a dense,
  deliberately-shaped hit rect of its own (a diff gutter cell) and passes that in.
  `paint_content` receives the resolved `WidgetState`, so a host takes its ink
  from `ButtonVariant::Ghost.text(state)` instead of deriving a second ladder.
  It is deliberately not a configurable universal button.
- `disabled_child_scope` disables a subtree and turns its clicks into no-ops
  without leaking `disabled` styling into the widgets beside it. Whether a given
  control delegates at all stays the caller's decision, and both shapes — one
  that always builds and paints the control, one that short-circuits on the
  enabled path — share this scope.
- `paint_centered_text` is the shared two-axis text centring, for a galley
  centred in a rect it is handed. The *vertical-only* centring family — an x
  from a layout column, a right-aligned edge, or an icon-plus-label group — is a
  different calculation and is intentionally not swept into it. Unifying it is
  its own change.
- `CardFrame` / `CardSurface` / `CardSizing` are how `card` is parameterised.
  `CardSurface` chooses the content or raised tone (a card on a `CONTENT_BG`
  surface asks for `.raised()`, because a `CONTENT_BG` fill there would be
  invisible); `CardSizing::Stretch` versus `CardSizing::MinWidth(w)` is the one
  thing a call site genuinely varies, so a card either spans its pane or floats
  above it. A call site that needs a different fill or a different corner radius
  has a different role and keeps its own frame.
- The `RULE_CONTENT` / `RULE_FOOTER` / `RULE_STRUCTURAL` hairline roles and the
  crate-private `widgets::footer_rule` painter that owns the modal footer rule —
  see *Hairline roles* above.

Feature surfaces may compose these roles, but a feature surface must not
silently redefine their geometry or colors with local literals.

## Feature-component contracts

### Branch features

The branch popup/status indicator (`ui::branch_widget`) and the branch tree
(`ui::branch_tree_view`) are reusable feature components. They are not generic
buttons, rows, or lists merely because they use `ui::widgets` primitives.
They retain branch-specific vocabulary: current branch, favorite, ahead/behind,
gone upstream, stale age, protected branch, local/remote/tag grouping,
collapsible groups, checkout/rename/delete gating, and compare/worktree actions.

The branch tree's current contract is data and events oriented: its rendering
receives the tree data and caller-owned state and returns events. It does not
call Git or dispatch operations. Any change that would move Git policy,
application dispatch, or editor state into a generic branch-kit primitive is
outside the component boundary.

### Diff features

`ui::diff` is a specialized diff feature component. Its diff model, panes,
actions, and rendering own diff-specific meaning such as added/removed lines,
file/hunk/line granularity, side-by-side/unified view, and diff selection.
Its internal modules are not a second general widget API. Shared chips, rows,
buttons, and icons may be used where the semantic roles match; diff rows and
diff actions remain diff roles.

### Dialogs, conflicts, and floating surfaces

Dialog and popup modules are feature composition surfaces. They may own their
specific list, loading, empty, error, and action content. The conflict pane
kit is intentionally narrower: it supplies shared visual grammar for the
three-pane conflict view but does not parse files, choose resolution, hold an
editor, or apply a result. The inline conflict surface and dedicated resolver
may differ in their Result editor while sharing the read-only conflict
presentation primitives.

## Weakly adopted APIs: final dispositions

Production adoption now settles the role of the previously weakly adopted
public APIs. These classifications guide future adoption; they do not authorize
removal, deprecation, or a breaking import change.

| API | Disposition | Production basis and boundary |
| --- | --- | --- |
| `ghost_button`, `primary_button`, `compact_button`, `icon_button` | **supported general** | Actively used across dialogs, settings, navigation, feature actions, and shell-adjacent surfaces. They remain the reusable shell/action vocabulary and remain distinct from branch-kit buttons. |
| `compact_button_enabled` | **specialized** | Retained for the branch popup's gated row actions. Its disabled child scope and no-op click behavior serve that feature's action-gating contract; it does not replace `components::KitButton` or become the branch button API. |
| `action_button` | **specialized** | The full-width stacked primary/secondary treatment. Its log commit-details consumer went with ADR-0024, which moved every commit action into the commit menu, so it has no production caller today and is retained as a kit role for a surface that needs a stacked action block. It is not required to absorb branch, dialog, or other feature action policy. |
| `dialog_footer` | **supported general** | Used by push, new-branch, tag, settings, smart-rule, and remote-management dialogs. It owns only the separator and right-aligned action slot, never dialog policy. The separator itself is now `footer_rule`'s, shared with modal bodies that rule themselves off from an action slot without owning a footer. |
| `toolwindow_header` | **supported general** | Used by Log branches, Blame, Worktrees, and Submodules tool-window headers. It owns only the title/action strip. |
| `card`, `card_header` | **supported general** | Used by the Commit tool window's local, staged, and changes regions. These are general containment roles, not aliases for branch detail panels or feature rows. |
| `note` | **supported general** | Used by rebase previews/results and multiple dialog summaries and warnings. Optional severity emphasis does not turn it into `alert_box`. |
| `alert_box` | **specialized** | The contained warning surface it defines. Its commit-details guardrail consumer went with ADR-0024 along with the buttons it explained, and it has no production caller today; a future surface that needs a warning with a visible boundary is what it is for. It is not the general `note`, inline-error, banner, or toast role. |
| `segmented_control` | **supported general** | Used for settings choices, diff side-by-side/unified mode, and file/hunk/line granularity. Each caller still owns its option list and state transition. |
| `hash_chip`, `avatar_initials`, `churn_bar` | **specialized** | Retained as named commit-detail primitives used by the Log commit-details surface. `hash_chip` is a non-interactive reference chip (see *Chip roles*); the other two are the identity and statistics roles they have always been. They are not promoted to generic reference, identity, or statistics widgets. |

The only final dispositions are **supported general** and **specialized**.
Branch and
conflict APIs retain their specialized semantics regardless of whether a
general primitive shares geometry with them.

**This table no longer contains a *candidate for retirement*.** The one
candidate it used to carry, `status_badge`, has been **retired**, along with
`ref_label`, `CountDirection`, and `tree_row` (see *The `RefKind` asymmetry*
and *The retirement sweep was name-aware* above). A code-only retirement that
left the row here would have made this document assert that a retired API is
still public and supported, so the row was removed rather than reworded. There
is no "candidate" disposition left to award: an API with no production consumer
now goes through the dead-API sweep instead of waiting for a review.

## Compatibility and public import contract

The following paths are the current compatibility surface and must continue to
resolve for existing consumers and tests:

- `turbogit_ui::theme::*` for shared tokens and style helpers;
- `turbogit_ui::ui::widgets::*` for the general public façade;
- `turbogit_ui::ui::components::*` for the branch component kit;
- `turbogit_ui::ui::icons::*` for shared icon primitives; and
- the existing public feature paths, including `turbogit_ui::ui::branch_widget`,
  `ui::branch_tree_view`, `ui::branches`, `ui::branches_tree`, `ui::diff`,
  `ui::dialogs`, and `ui::popups`.

`turbogit_ui::render` is a compatibility re-export of `ui::render`, and
`turbogit_ui::ui::banner::{AppBanner, BannerAction, AppSeverity}` are
compatibility aliases for the app-owned banner types. These aliases are part of
the boundary: a UI presentation module may render the values, but the data
model remains owned by `turbogit_app`.

The implemented `ui::widgets/` directory is a public façade plus private focused
implementation modules, not a second import path for callers. The focused
modules cannot require a flag-day migration because the established names are
explicitly re-exported from `ui::widgets`. The separate `ui::components` path
remains the public branch component kit, `ui::icons` remains the public icon
owner, and `ui::kit::conflict_pane` remains internal. Moving implementation
helpers must not move that conflict kit to a public path or accidentally turn
an internal helper into a feature component API.

Later component-library tickets added public primitives while preserving the
compatibility paths described above. The work remains behavior-preserving with
exactly one deliberate exception, recorded in *Hairline roles* above: the
interactive rebase editor's body-to-footer rule moved onto the shared footer
treatment, which is a tone change from egui's unowned default-separator tone to
`RULE_FOOTER`. Any later change to a role, boundary, or public path must update
this document and retain the corresponding compatibility re-export.

**The dead-API sweep is the one change that removed public paths, and it is
recorded here rather than being left to a changelog.** Four names left the
`ui::widgets` façade — `ref_label`, `status_badge` (and its `StatusBadge` /
`CountDirection` vocabulary), and `tree_row` — because none had a production
caller and each survived only on its own characterization test. No rendered
pixel moved, so the visual compatibility of every surface below is untouched;
what changed is the *advertised* surface. The five frozen compatibility import
paths listed above all still resolve:

| Path | Declared at |
| --- | --- |
| `turbogit_ui::render` (re-export of `ui::render`) | `crates/turbogit-ui/src/lib.rs:6` |
| `turbogit_ui::ui::banner::{AppBanner, BannerAction, AppSeverity}` (aliases for the app-owned banner types) | `crates/turbogit-ui/src/ui/banner.rs:12` |
| `turbogit_ui::ui::diff::{PaneCache, PaneEntry, PaneSide}` | `crates/turbogit-ui/src/ui/diff/mod.rs:50` |
| `turbogit_ui::ui::hunk_nav::{Dir, EDGE_WINDOW}` | `crates/turbogit-ui/src/ui/hunk_nav.rs:18` |
| `turbogit_ui::ui::widgets::CASCADE_ACCENT` (token's home is `theme`, re-exported so the widget path keeps resolving) | `crates/turbogit-ui/src/ui/widgets/mod.rs:33` |

`CASCADE_ACCENT` is the trap in that table: with `StatusBadge::accent` gone it
has **no reader inside the crate**, which is exactly what a "delete the unused
re-export" sweep would flag. It is frozen public surface; the site carries a
comment saying so.

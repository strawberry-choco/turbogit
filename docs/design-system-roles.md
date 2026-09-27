# TurboGit design-system component roles

**Status:** authoritative role contract for the current UI crate

This document is the boundary contract for `crates/turbogit-ui`. It describes
what the current modules mean, which surfaces may reuse them, and which
similar-looking roles must remain separate. It is not a proposal to redesign a
surface. A caller may adopt a shared component only when its semantic role
matches; visual similarity is not enough.

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
  and `icon_button` are the **general button vocabulary**. They are sized and
  styled for reusable shell, dialog, toolbar, and action contexts. A general
  button chooses its own label and may optionally carry an icon.
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
- `components::pill` with `PillKind::Current` and `PillKind::Count` is the
  branch-kit fact vocabulary. `Current` identifies the current branch; `Count`
  identifies the number of items in a group and has its own smaller dimensions.
  It is not a generic status badge and not a row.
- Branch-tree sync chips, current-branch chips, and any similarly local chip
  remain branch-feature roles even when they reuse the shared chip geometry.
  Do not replace a semantic wrapper with a color parameter and lose the meaning.

A chip is a compact fact marker. A pill is a branch-kit fact marker. A row is an
interactive or data-list unit. A selection state is a row-state decision. None
is a generic synonym for another.

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

Seven surfaces in the app look like the shared chip without being the same role.
None of them was migrated. Each was adjudicated individually and is recorded
here with the reasoning, not only the outcome, so the next maintainer inherits
the judgement instead of re-deriving it. The verdict vocabulary is **migrate**,
**keep with a named token**, and **keep because the role differs**.

**No site in this table was marked _migrate_.** The one chip-like surface that
did move — the log file-row status pill — already had, and is recorded under
*Log surface pills* below. Every other candidate is a case where adopting the
shared chip would cost a border, an icon, a fixed numeral, or a nested button;
those are design losses, not refactoring debt.

| Site | Where | Verdict | Reasoning |
| --- | --- | --- | --- |
| Shell repository-header chip | `ui::shell::branch_pill` | keep with a named token; the role also differs | It is a *bordered* chip: a `SURFACE_2` fill with a `LINE_SUBTLE` **inside** stroke around it. The shared chip vocabulary paints an unstroked fill, so migrating it would drop the border that is the whole point of the treatment — it is what makes the repository's current branch read as a named object rather than as a word floating in the header. Its radius already comes from the named `theme::CONTROL_RADIUS` token rather than a local literal, so the "named token" half of the verdict is already satisfied and there is nothing to name. |
| Shell dirty badge | `ui::shell::dirty_badge` | keep with a named token; the role also differs | A counter, not a label: it is the focused root's uncommitted *count* in the reserved counter orange. It already derives its fill from the shared `widgets::BADGE_TINT` and its radius from `theme::CONTROL_RADIUS`, and it already reuses the shared `CHIP_HEIGHT`/`CHIP_PAD_X`. The shared `components::pill` count role is a branch-kit fact marker with its own smaller dimensions; adopting it here would resize the badge. |
| Branch-tree sync chip | `ui::branch_tree_view::sync_chip` | keep with a named token; the role also differs | It carries a 10px status icon and monospaced `TYPE_CHIP` type inside its fill, which is the branch tree's sync-relationship vocabulary. The shared chip geometry has no icon slot and no mono variant, so adopting it would mean either dropping the icon or growing a config flag on the shared primitive — both of which breach non-negotiable boundary 5. Its radius is already the named `theme::CHIP_RADIUS`, and its fill already resolves through the shared `components::sync_bg` (which is itself now written as `tint_over_bg(sync_ink(kind), BADGE_TINT)` rather than a repeated `0.18`). |
| Welcome step indicator | `ui::welcome::step_pill` | keep with a named token; the role also differs | A fixed 20×20 `SURFACE_3` numeral roundel, not a variable-width fact marker: its width is its height because the content is a single digit, and it is numbered 1–5 by position in a list. It already takes its radius from the named `theme::PILL_RADIUS` (which is itself defined as the welcome roundel's half-height, so the two can never disagree) and its centring from the shared two-axis text helper. The roundel's 20px edge is screen-specific geometry and stays a local `STEP_PILL` constant. |
| Branches scope indicator | `ui::branches::scope_label` | keep because the role differs | **Not a chip.** It paints coloured text with no background, no fill, no radius, and no padding. Renamed from `scope_chip` to say what it is. |
| Submodules status indicator | `ui::submodules::status_label` | keep because the role differs | **Not a chip.** It paints coloured text with no background, no fill, no radius, and no padding. Renamed from `status_chip` to say what it is. |
| Settings removable pattern chip | `ui::settings_modal::pattern_chip` | **genuine gap, no shared equivalent** | A `SURFACE_2` frame carrying a label *and* a `×` remove control that is a real button with its own accessibility node (`Remove <pattern>`). The shared chip vocabulary has no slot for a nested interactive control, so there is no equivalent to adopt. Its radius is the named `theme::CONTROL_RADIUS`. **Deferred:** whether to add a shared "removable chip" primitive or to document this as a page-local frame is a decision for its own ticket. |

**Turning either coloured-text surface into a real chip is a design change
requiring its own ticket, not a consolidation.** Giving `branches::scope_label`
or `submodules::status_label` a background would (a) introduce a filled surface
where today there is none, changing the visual weight of both list rows against
their neighbours, and (b) newly register an accessibility node for a piece of
status text that is currently plain content text inside its row. Both are
user-visible; neither follows from the role contract. Until a design ticket says
otherwise, the rule is: **a chip is a filled, bounded fact marker; a coloured
label is not one, whatever shape it resembles.**

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

The row layer is **one painter and one state grammar**, not a row widget.

- `widgets::paint_row(ui, rect, state)` is the general row fill: it paints the
  `row_fill(state)` colour into a rect the caller has already allocated, with
  the `CONTROL_RADIUS` corner every such row has always used. It is the shared
  form for the hand-painted tables — the settings category rail, the rebase
  todo, the commit window's file rows, the multi-selection body, the welcome
  and log rows — that allocate a rect, paint behind their content and want the
  one row-state decision. A surface that rounds its rows differently (the
  sidebar's full-bleed band, blame's dense rows) keeps its own corner; that is a
  real geometry difference, not a second row role.
- **`widgets::tree_row` no longer exists.** The fixed-height tree/list row
  wrapper was retired by the dead-API sweep together with the private row
  implementation it solely owned, because nothing outside its own
  characterization test ever called it. It is *not* a universal row for
  commits, files, diff lines, or conflict panes, and it was never going to
  become one. The retired function's interaction target (the row owning hover
  and click across its full rect) is the reason a caller that needs one now
  lays its own row out and calls `paint_row`; that is a real cost of the
  retirement, recorded here rather than hidden.
- `components::RowState`, `row_fill`, and `current_row_fill` belong to the
  branch/shared row-state grammar and are unchanged. The branch row and repo
  section add branch-specific geometry,
  current-branch facts, sync markers, stale ink, and operation labels on top of
  that grammar.
- Commit rows, file rows, diff rows, blame rows, log rows, rebase rows, and
  conflict result cells remain feature/page rows. They may use `row_fill` or
  `paint_row` for a compatible fill, but their content, interaction, and state
  models stay specialized.

The selection roles in `components::RowState` are not aliases:

- `RowState::Default` is the resting, transparent row.
- `RowState::Hover` is the shared `SURFACE_2` interaction band.
- `RowState::Selected` is the quiet tool-window active row using
  `Palette::SELECTION`; it is a selection role, not the generic tree selected
  role.
- `RowState::BrandSelected` is the solid `Palette::BRAND` fill with
  `BRAND_INK` content, reached through `RowState::from_flags(selected, hovered)`
  — the boolean `(selected, hovered)` mapping the retired `tree_row` API handed
  out. `from_flags` outlived its caller and still has five production call
  sites (settings modal, interactive rebase, log window, welcome,
  multi-selection), so the mapping is live even though the wrapper is gone.
  Callers that need another selection role must construct it explicitly.
- `RowState::FocusSelected` is the translucent `Palette::selection_bg()`
  focus band used by the log table, sidebar tree, and blame surfaces. It is not
  the opaque `Palette::SELECTION` or `Palette::SELECTION_BG` fill.
- A current branch is a repository fact, not a pointer state.
  `components::current_row_fill` keeps its resting brand tint distinct from
  hover and selection, and keeps selection visible when the current row is also
  selected.
Selecting one of these states is a visible decision. Do not collapse them to a
single boolean or infer that “selected” means the same thing in every list.

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
| Content divider | `Palette::RULE_CONTENT` | `Palette::DIVIDER` (= `RAISED` = `SURFACE`) | A 1px rule between sibling content regions *inside* one surface. The weakest of the three: it separates, it does not bound. |
| Footer rule | `Palette::RULE_FOOTER` | `Palette::LINE` | The 1px rule separating a modal body from its action slot. |
| Structural rule | `Palette::RULE_STRUCTURAL` | `Palette::LINE_SUBTLE` | Table header underlines, tree indent guides, and panel edge rules — the chrome that gives a surface its structure, rather than dividing its content. |

All three are **pure aliases**: no token value moved, and the pre-existing
contract (`DIVIDER == RAISED`, `DIVIDER != LINE`) is untouched.

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
(`multi_selection`), the sidebar's selection-bar top edge (`sidebar`), and the
shell repository-header chip's border (`shell::branch_pill`) all wear
`LINE_SUBTLE` and are all chrome; but the shell's own panel edge rules
(`shell::paint_edge_line_at` — the topbar, toolbar, tab-strip and status-bar
edges) wear `LINE`, the *footer-rule* value, even though their role is
structural. Their role is identified here; their value is not re-pointed,
because this ticket is about naming roles and not re-sweeping call sites.

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

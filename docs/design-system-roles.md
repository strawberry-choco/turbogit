# TurboGit design-system component roles

**Status:** authoritative role contract for the current UI crate

This document is the boundary contract for `crates/turbogit-ui`. It describes
what the current modules mean, which surfaces may reuse them, and which
similar-looking roles must remain separate. It is not a proposal to redesign a
surface. A caller may adopt a shared component only when its semantic role
matches; visual similarity is not enough.

The source of truth for this contract is the code in `crates/turbogit-ui/src`.
The role descriptions below use the implemented module and function names. The
seven rules design system v2 decided (`ADR-0027`, R1 through R7) are stated in
full there, each with the alternative it was chosen over; the map below records
only which thing in the tree **owns** each one, because owning it is what makes
a rule a rule rather than a per-call-site judgement. The retirements and
narrowings v2 made are recorded at `ui/widgets/mod.rs:7-23`, and the pane-chrome
collapse at `ui/widgets/containers.rs:403-411`.

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

| Rule | Owned by | Stated in |
| --- | --- | --- |
| **R1 — One accent** | the token layer; asserted at the chip constructors and the row-fill decision | `ADR-0027` §R1 |
| **R2 — Cards are surfaces, not boxes** | the shared card. Card padding is already the panel padding and does not change, which is why losing the stroke does not collapse the region | §R2 |
| **R3 — Ink is a four-step ramp** | the token layer, asserted as contrast maths over the values rather than as a comment | §R3 |
| **R4 — Selection is a composite** | the row-fill decision and one rail painter | §R4 |
| **R5 — Raised controls step up** | the token layer's named raised-on-card role | §R5 |
| **R6 — Ref chips are neutral, state is coloured** | the shared chips module, and the one `RepoState::color()` map for state colour | §R6 |
| **R7 — One pane-chrome vocabulary** | one pane-header implementation, one column-header row, one row shell | §R7 |

### How each rule is ratcheted

A rule nobody can fail is a comment. Each rule has at least one assertion, in
one of four suites — `design_tokens.rs` (token maths, and closed tables pinned
at construction sites), `widget_library.rs` (painted output),
`branch_component_kit.rs` (painted rows and the row-fill decision),
`worktrees_submodules_tabs.rs` (painted column geometry) — at one of three
altitudes, and every assertion names the rule it guards in its failure message.
**The suites are the index for the rules; this document does not restate them.**

Two mechanisms keep the negative rules honest without a render seam: **closed
tables** (the stroke sites, the `RAIL_WIDTH` readers, the `RepoState` variants)
fail when the tree grows and name what grew, and every row of such a table
carries the reason that row is allowed; and **derived sets** (the state colours,
the chip geometry's screen users) are read from the source of truth rather than
transcribed, because a transcribed list is not exhaustiveness-checked and this
migration has already been bitten by one — the sixth `RepoState` compiled
silently and two ratchets stopped covering it.

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
  vocabulary**, sized and styled for reusable shell, dialog, toolbar, and action
  contexts. A general button chooses its own label and may optionally carry an
  icon.
- **`ButtonVariant::CompactPrimary` / `widgets::compact_primary_button` is the
  brand primary at the pane-header band height (28 px)** — `Primary` and nothing
  else, because R1's primary is the one thing in the app that must not have two
  spellings. `ButtonVariant::Compact` (the 28 px ghost) and `CompactPrimary` are
  different *roles* at the same height: one asks no fill, one spends the accent.
- `widgets::compact_button_enabled` is a specialized branch-popup action gate;
  `widgets::action_button` is the specialized full-width stacked
  primary/secondary treatment. Neither replaces the general vocabulary.
- `components::KitButton` and `components::kit_button` /
  `kit_button_at` are the **branch-kit button role**: the branch screen's compact
  28px kit height, minimum target rules, and four branch action variants
  (`Primary`, `Secondary`, `Quiet`, and `Danger`). `kit_button_at` is the
  explicit-width form, and since ADR-0023 no branch screen lays out a column of
  equal widths, so it has no caller outside the kit. `Danger` is a
  branch/destructive action treatment, not a general `ButtonVariant` replacement.
- The two families may share theme tokens and state math. They must not be
  merged into one API until a separate design decision proves that their
  geometry, semantics, and call-site contracts are equivalent. A generic button
  must not be forced to carry branch width, branch action gating, or branch
  screen typography merely to make the APIs symmetrical.

### Titles and headers

- `widgets::group_title` is a plain uppercase micro-header for a dialog, panel,
  or list. It paints text only: no band, no count, no chevron, and no toggle.
- `components::section_header` is a branch group **band** for `Local`, `Remote`,
  or another branch/smart group. It has `SECTION_BG`, a chevron, a live count
  pill, an expand/collapse hit target, and an optional trailing action. The
  strip toggle and a trailing action have separate interaction precedence.
- `widgets::pane_header` is the tool-window title/action strip — the one pane
  chrome row — and is not either of the above. The name it replaced,
  `widgets::toolwindow_header`, is **retired and deleted, in any spelling**; the
  reason it must not come back is that a second pane-header implementation is
  not a second spelling of the first, it is a header the user can tell apart
  from it. The branch kit's `detail_panel_header` left with the detail panel it
  headed (ADR-0023), so a header for a future detail surface starts by
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
| Cautions rail | `widgets::cautions_rail` is the titled count plus one warning-ink line per `RebaseCaution` a history rewrite computed. | A general `note`, an `alert_box` well, a banner, or a toast: it is a titled list of counts, not a contained message. |
| Recovery note | `widgets::recovery_note` states that a rewrite writes a backup ref before it moves anything, and names that ref. The restore ACTION is not part of it: only a surface running a replay long enough for "abort" to mean anything owns that button. | A persistent warning that stays on screen, or a per-surface restatement of the same sentence. |
| Note | `widgets::note` is an inset `SURFACE_2` well for a preview, summary, or explanatory note. It has optional severity emphasis through the caller's stroke, but the default note is not an error surface. | An always-warning alert or a global banner. |
| Alert | `widgets::alert_box` is a contained warning/guardrail surface with `SURFACE_WARNING`, a warning icon, and warning ink. It is for a warning that needs a visible boundary. | A bare inline error, a general note, or a transient toast. |
| Banner | `ui::banner` renders a severity-tinted horizontal strip with a state-owned message and optional deep-link actions. `banner::maybe_show` is the app-wide host: the single entry point, called once from `ui::render`, and it owns whether a banner is showing at all. | A contained alert or a toast. It is anchored to a surface/application feedback state and may carry actions. |
| Toast | The private `render_toast` host in `ui::mod` renders a transient, bottom-right notice with a kind-colored accent/icon, a dismiss action, and the current four-second lifetime. | A persistent banner, inline error, or dialog list. Toast placement, lifetime, and dismissal are part of its role. |
| Dialog/conflict list | A dialog's error, warning, target, or conflict list is content composed by that dialog flow. A conflict list additionally uses the specialized three-pane conflict grammar in `kit::conflict_pane`: local/incoming sides, raw markers, and composed result cells. | A generic status chip, a toast, or a generic note. List order, count, selection, and resolution actions are feature semantics. |

A keyed read may share waiting/failure presentation when the behavior is
identical, but its empty state remains surface-owned. The shared contract is
about the waiting/failure treatment, not a universal replacement for diff,
blame, branch, or dialog content.

## Chips, pills, rows, and selection

### Chip roles

`widgets/chips.rs` owns the shared chip geometry — `ChipGeometry`,
`CHIP_GEOMETRY`, `CHIP_HEIGHT`, `CHIP_PAD_X`, `chip_radius`,
`chip_rect_right`, `chip_text_origin`, `paint_chip` — and is re-exported on the
`ui::widgets` façade. That geometry is shared; the semantic wrapper is not, and
the wrappers are what the following contrasts are about:

- `widgets::badge` with `BadgeKind` states a file status (`Neutral`, `Added`,
  `Modified`, `Deleted`). It is **not** one of the three chips: it keeps the pill
  radius (9) and its own slot, and the ref chip's compact radius (3) is a
  different shape for a different job. Both radii stay distinct and both stay
  pinned; neither is merged, and the badge is not reclassified as a chip to tidy
  the vocabulary.
- `widgets::RefKind` is the surviving **colour vocabulary** half of a retired
  role — see *The `RefKind` asymmetry* below. `RefKind::accent` is live: it maps
  `Branch`→`BRAND`, `Remote`→`STATE_SUCCESS`, `Tag`→`STATE_WARNING`.
- `widgets::hash_chip` is a commit reference with monospace content, and a
  **label, not a control**: it senses hover only and carries no click, because
  copying a hash is the commit menu's Copy hash item (ADR-0024). It is a
  specialized reference chip, not an interactive reference and not a generic
  label.
- `components::pill` with `PillKind::Current` and `PillKind::Count` **no longer
  exists**, the branches screen having been their only caller, and both variants
  were replaced by the closed chip set (the current marker is
  `widgets::current_chip` / the word beside the row's band, a group's count is
  `widgets::count_chip`). A second chip vocabulary nobody paints is how a
  vocabulary rots.
- Branch-tree sync chips, current-branch chips, and any similarly local chip
  remain branch-feature roles even when they reuse the shared chip geometry.
  Do not replace a semantic wrapper with a color parameter and lose the meaning.

A chip is a compact fact marker. A pill is a branch-kit fact marker. A row is an
interactive or data-list unit. A selection state is a row-state decision. None
is a generic synonym for another. The shared chip vocabulary paints an
unstroked fill, so a chip that needs a border is a different role with no
shared equivalent and paints its own stroke.

#### The three chips, and the state that is not one (R6)

`widgets/chips.rs:1-49` is the owner: it states the closed set of three — one
function each, with their fills, inks and what each carries — that the badge and
`hash_chip` are deliberately outside it, and that **semantic state is not a
chip**: `dirty`, `↓ 2 behind`, `Up to date` and `Needs update` render as coloured
text or a leading dot from the one `RepoState::color()` map, because a state
behind a fill stops reading as state and starts reading as a category. What
that page does not record is the decision that closed the question of "what does
a state colour mean". The map has seven states and only **three colours** that
name a severity, so "no chip may wear a state colour" had to be answered twice:
once for the severities, and once for `RepoState::Uninitialized` — a submodule
registered but never checked out, which is **work the user is owed** rather than
something that went wrong, so it wears `Palette::INK_3` and the app invents no
fourth tone. The *severity table* stays at three (`COUNTER`, `AHEAD`,
`STATUS_DIVERGED`) and `INK_3` is **not** added to it: a fourth row called
"state colour" would erase the very distinction it drew, on the next state that
arrives. The rule that actually protects the vocabulary is stronger and already
written — *no chip fills or inks any colour `RepoState::color()` can produce* —
and that form covers `INK_3` without calling it a severity, with both contract
suites asserting it over the whole map rather than over a transcribed list of
colours.

#### The `RefKind` asymmetry (intentional)

The dead-API sweep retired the ref-chip **render function** (`widgets::ref_label`)
and deliberately **kept** the ref-kind **type and its accent mapping**
(`widgets::RefKind`, `RefKind::accent`). That is not an oversight and must not
be "tidied" in a later sweep: `ui::log_window` owns a private adapter that maps
its own `GitRefKind` onto the shared vocabulary to colour log references and
**never renders a ref label**, which is exactly why an adoption count reads
`RefKind` as dead and is wrong.

What went with the render function is `RefKind::colors` — the solid-pill
`{bg, fg}` colour pair whose only caller was `ref_label`. Once the function was
gone the pair was orphaned, and leaving it would have failed the workspace's
warnings-as-errors lint gate. It is a derivation, not a role. If a future surface
does need a ref-label chip, the honest move is to add the render function back
**and** `RefKind::colors`, and to migrate a real caller onto it in the same
change — not to keep a colour-pair helper alive on speculation.

#### Per-site chip verdicts

Five surfaces in the app looked like the shared chip without being the same role.
None was migrated; each was adjudicated individually with the reasoning recorded,
so the next maintainer inherits the judgement instead of re-deriving it. The
verdict vocabulary is **migrate**, **keep with a named token**, and **keep
because the role differs**. **No site was marked _migrate_.** Every candidate
would cost an icon, a fixed numeral, or a nested button; those are design
losses, not refactoring debt.

| Site | Verdict | Reasoning |
| --- | --- | --- |
| `ui::welcome::step_pill` | keep with a named token; the role also differs | A fixed 20×20 `SURFACE_3` numeral roundel, not a variable-width fact marker: its width is its height because the content is a single digit, and it is numbered 1–5 by position in a list. It already takes its radius from the named `theme::PILL_RADIUS` and its centring from the shared two-axis text helper. The roundel's 20px edge is screen-specific geometry and stays a local `STEP_PILL` constant. |
| `ui::settings_modal::pattern_chip` | **genuine gap, no shared equivalent** | A `SURFACE_2` frame carrying a label *and* a `×` remove control that is a real button with its own accessibility node (`Remove <pattern>`). The shared chip vocabulary has no slot for a nested interactive control, so there is no equivalent to adopt. Its radius is the named `theme::CONTROL_RADIUS`. **Deferred:** whether to add a shared "removable chip" primitive or to document this as a page-local frame is a decision for its own ticket. |

Two further verdicts are recorded at their sites, because both sites are now
carrying their own resolution: the submodules status indicator stays coloured
text (`ui/submodules.rs:188-210`, ratified by R6) and the branches scope
indicator became a real chip (`ui/branches.rs:398-422`, reversed by R7). Two
rows are absent because their sites are gone: `shell::branch_pill` and
`shell::dirty_badge` were adjudicated and have since been deleted. The one
judgement among them that was not site-specific is now a rule of the shared chip
vocabulary above: a bordered chip has no shared equivalent.

#### Two decisions v2 reverses, and one it ratifies

Two code comments in the tree read, near-word-for-word: *"Not a chip, and
deliberately not one: it paints **coloured text with no background at all**, so it
shares no geometry, no radius and no fill with the shared chip vocabulary. Turning
it into a real chip would introduce a background where none exists and would newly
register an accessibility node for a piece of status text — a design change that
needs its own ticket, not a consolidation."* One sat on `ui::branches::scope_chip`
and one on `ui::submodules::status_mark`; they pointed at **this** document by
name, and they were indistinguishable to anyone who had not read to here. That
is exactly why the answer is recorded in one place: one was reversed, one
confirmed, and a rule that disagrees with a comment but is written nowhere else
is a rule the tree will contradict twice.

The general rule both comments are instances of survives unchanged: **a chip is a
filled, bounded fact marker; a coloured label is not one, whatever shape it
resembles.** What changed is whether the branches scope indicator is a label. Both
resolutions, and the argument each one overrules or confirms, are now carried at
the sites themselves: `ui/branches.rs:398-422` (reversed — the scope indicator
is the pane's **scope selector**, a control, so the accessibility objection holds
for a label and not for a control, and a screen-reader user gains the filter) and
`ui/submodules.rs:188-210` (ratified by R6 — a state renders as coloured text or
a mark, and a chip is a bounded container while a dot is a mark). Neither comment
may be deleted, and copying the branches treatment onto the submodules status
word because the two comments look the same is the mistake this paragraph exists
to prevent.

### Log surface pills

`ui::log_window` paints two pills that look alike and are not the same problem.
The **status pill** (the file-status letter on a decorated commit row) uses a
named local `STATUS_PILL: widgets::ChipGeometry` built from the shared
`CHIP_HEIGHT` and `CHIP_PAD_X`, pinning `radius` to `theme::CONTROL_RADIUS` — the
radius the row had before, so adopting `ChipGeometry::paint` moved no pixel; a
`ChipGeometry` *value* rather than the `CHIP_GEOMETRY` constant is the point,
because the row is a status pill wearing the control radius and naming it says
so. **`paint_label_pill`**, the icon-only tag pill, deliberately stays
hand-painted on the shared chip tokens and cannot use `ChipGeometry::paint` at
all, because that function takes a galley and an icon-only pill has none; its
content is centred icon arithmetic, not text centring.

### Row roles and selection states

The row layer is **one painter and one state grammar**, not a row widget — and
under v2, one row shell and one rail painter on top of them.

- `widgets::paint_row(ui, rect, state)` is the general row fill: it paints the
  `row_fill(state)` colour into a rect the caller has already allocated, with the
  `CONTROL_RADIUS` corner every such row has always used. It is the shared form
  for the hand-painted tables that allocate a rect, paint behind their content
  and want the one row-state decision. A surface that rounds its rows differently
  (the sidebar's full-bleed band, blame's dense rows) keeps its own corner; that
  is a real geometry difference, not a second row role.
- **`widgets::tree_row` no longer exists, and that decision is not revisited.**
  The fixed-height tree/list row wrapper was retired by the dead-API sweep
  together with the private row implementation it solely owned, because nothing
  outside its own characterization test ever called it. It is *not* a universal
  row for commits, files, diff lines, or conflict panes, and it was never going
  to become one. The real cost of the retirement is that the retired function's
  interaction target — the row owning hover and click across its full rect — is
  not something `paint_row` gives back, so a caller that needs one now lays its
  own row out.
- `components::RowState`, `row_fill`, and `current_row_fill` belong to the
  branch/shared row-state grammar. The branch row and repo
  section add branch-specific geometry,
  current-branch facts, sync markers, stale ink, and operation labels on top of
  that grammar.
- Commit rows, file rows, diff rows, blame rows, log rows, rebase rows, and
  conflict result cells remain feature/page rows. They may use `row_fill` or
  `paint_row` for a compatible fill, but their content, interaction, and state
  models stay specialized.

The selection roles in `components::RowState` are not aliases, and the suites
assert what each one is. v2 leaves `Default` (resting, transparent) and `Hover`
(the shared `SURFACE_2` interaction band) alone and narrows the three selection
roles to the three that are real:

- **The list row** is `Palette::ROW_SELECTED` plus a 2px `Palette::BRAND` rail at
  the leading edge. Ink does not invert: a selected row's text is the colour an
  unselected row's text is, so selecting a row does not cost the user the ability
  to read it. The rail is **paint, not layout** — reserving its width as padding
  would shift every row's text origin, and *a selected row's first text origin
  equals an unselected row's* is a criterion of the rule, not a detail. This is
  the role `RowState::BrandSelected` becomes; `RowState::from_flags(selected,
  hovered)` — the boolean `(selected, hovered)` mapping the retired `tree_row` API
  handed out — resolves `selected` to it today, at five production call sites
  (settings modal, interactive rebase, log window, welcome, multi-selection).
  The loudest blue in the app is currently the default selection for
  hand-painted rows; this is the change that stops it being one.
- **The current ref** is `Palette::SELECTION #2E4369`, and it is a selection role
  only for "this is the current ref". It is the old `RowState::Selected` band with
  its scope narrowed: the *quiet tool-window active row* is not a role distinct
  from the current-ref band, so that meaning goes with the rename and the value
  stays. **No list row resolves to it** — stated on the token and asserted at the
  row-fill decision, which is the only seam that can prove a negative.
- `RowState::FocusSelected` is the translucent `Palette::selection_bg()` focus
  band used by the log table, sidebar tree, and blame surfaces — one of the three
  survivors rather than a duplicate of any of them. **The left rail's active row
  is one of its consumers, deliberately**: R4's "a selected list row is
  `ROW_SELECTED` plus a rail" is about a row *of a list*, and what the rail paints
  is a full-bleed tree selection spanning a workspace tree, which is a *region*
  that is chosen rather than a row picked out of many. The two fills are also the
  same colour to the eye (`ui/sidebar.rs:69-75`, pinned by
  `tests/branch_component_kit.rs`), so the swap would change which *role* the
  band claims and nothing the user can see.
- A current branch is a repository fact, not a pointer state.
  `components::current_row_fill` keeps its resting brand tint distinct from
  hover and selection, and keeps selection visible when the current row is also
  selected.
- Callers that need a selection role other than these three must construct it
  explicitly. Selecting one of these states is a visible decision. Do not collapse
  them to a single boolean or infer that “selected” means the same thing in every
  list.

## Hairline roles

A one-pixel line was once picked by whichever of the app's three line tokens was
nearest, which is why `theme.rs:250-294` names the three real roles there — by
**role**, not by appearance — as pure aliases whose values and the
`RULE_CONTENT == RAISED`, `RULE_CONTENT != LINE` contract are untouched. That
paragraph is the owner; what the roles document adds is **where a line may appear
and which of the three names it takes**, which is what turns "picked whichever
token was nearest" into a rule a test can pin.

- **R2 — a card's interior is divided by spacing and at most one hairline**, and
  that one is the **content divider**. A region is carried by its fill and its
  padding; the hairline is available for a division that spacing cannot express,
  once. "At most one" is a negative rule, so it is pinned at the sites a suite
  enumerates rather than by scanning for strokes.
- **R7 — a pane's chrome row is a title, an optional count chip, a right-aligned
  action slot, one hairline, then content.** That hairline is the **content
  divider** again — it divides the chrome from the content inside one surface —
  and it is *one*. Two rules between a header and its rows is the nested-boxes
  problem this migration exists to fix, and it reappears quietly, which is why
  the count is stated rather than implied.
- **A column header's underline is the structural rule.** It is not a divider
  between content regions; it is the chrome that gives a table its structure,
  sitting under a header row that is itself chrome. Structural is the right name
  for it for the same reason it is the right name for a tree indent guide.
- **A modal's footer rule is the footer rule**, and it stays the strongest of the
  three. R2's stroke-means-it-floats rule is why a dialog keeps its 1px border at
  all: it is a floating surface.

The useful consequence is arithmetic rather than aesthetic: a pane can now be
*counted*. Title, count, action slot, one divider, then content, plus at most one
interior divider and one structural underline for a column header — so "one
hairline, then content" is a claim about a number of rules rather than an opinion
about a screenshot, and "one hairline" is the phrase a reviewer can check.

`widgets::footer_rule` is the shared painter for the footer rule (crate-private,
re-exported to sibling surface modules through `ui::widgets`), which
`widgets::dialog_footer` delegates to, and which the interactive rebase editor's
body-to-footer rule now wears — pinned by
`crates/turbogit-ui/tests/interactive_rebase_editor.rs`, which pins the rule's tone
*and* its 1px height, span and position, because no acceptance capture covers that
page.

### One structural edge wearing the wrong tone, and the unowned fourth tone

`RULE_STRUCTURAL`'s users were checked to be genuinely structural rather than
dividers. The commit window's tree indent guide (`commit_window::indent_guide`),
the multi-selection summary's header rule (`multi_selection`), and the sidebar's
selection-bar top edge (`sidebar`) all wear `LINE_SUBTLE` and are all chrome;
but the shell's own panel edge rules (`shell::paint_edge_line` at
`shell.rs:426-430`, and the tab strip's underline at `shell.rs:481`) wear `LINE`,
the *footer-rule* value, even though their role is structural. Their role is
identified here; their value is not re-pointed. `widgets::edge_rule` says so at
its own definition — do not quietly correct it there.

egui's default `ui.separator()` paints a tone **no design token owns**:
`theme::dark_visuals` starts from `Visuals::dark()` and never assigns
`noninteractive.bg_stroke`, so a `Separator` keeps egui's stock
`Stroke::new(1.0, Color32::from_gray(60))`. `theme.rs:262-270` records the
finding, the fourth unowned role it makes, and why sweeping those call sites is
follow-up work; `widgets::menu` records the site that resolved onto the structural
role instead.

Two families inside that follow-up are already settled and must not be swept:

- **The command palette's search-field separator** (`ui/popups.rs:291`, in
  `command_palette`) is recorded as **questionable**. The action list beneath it
  deliberately compresses its rhythm — `item_spacing.y = 2.0`,
  `interact_size.y = 16.0` — and a full-width 1px rule, with its own 6px reserved
  band and 6px item spacing on either side, fights that compression. It is a
  candidate for removal rather than for a named role, and that call belongs to
  the follow-up. The same file's second `ui.separator()` (`ui/popups.rs:438`,
  under the workspace picker) separates the recents list from its two
  open-folder actions and is a plain modal-body divider.

## General controls, inputs, and containers

Each of the following is a general role in `ui::widgets`, subject to the same
token-only rule, and each states its own framing at its definition: `ButtonVariant`
and its button functions (reusable interaction states, focus rings, labels, hit
targets); `text_input` and `search_input` (input framing, hints and focus rings,
search adding the leading search icon, with validation and the surface's inline
error staying separate); `dialog_footer` (the right-aligned action slot and the
6px gap below the rule, never the dialog's action policy or its error/list
content); `pane_header` (the shared tool-window title/action strip, which does
not own a feature's body or state, and is the only one — the name that preceded
it is retired); `card`, `card_header`, `note` and `alert_box` (the container roles
described above, where a page needing different containment may keep a local frame
but must not call it `note`, `alert` or `card` merely because it looks similar);
and the presentation helpers `focus_ring`, `mix`, `tint_over_bg` and
`paint_centered_text`, which own no interaction. Two distinctions the widget layer
cannot state about itself, because they are about what these names are *not*:

- **`segmented_control` is not a branch group header and not a selectable row.**
  It is a general token-exact two- or three-way picker; each caller still owns its
  option list and its state transition.
- **`paint_centered_text` is the two-axis centring only.** The *vertical-only*
  family — an x from a layout column, a right-aligned edge, or an
  icon-plus-label group — is a different calculation and is intentionally not
  swept into it. For the same reason `components::kit_button_at` was deliberately
  *not* consolidated onto it, even though the two do the same arithmetic, for the
  hazard recorded at `components.rs:772-780`.

### General primitives added by the consolidation

These are part of the boundary now. All are exported from the `ui::widgets` façade
except two: `footer_rule`, which is deliberately crate-private, and
`ui::column_table`, which is a crate-private `ui::` module rather than a
`widgets` one — it is a *pane* skeleton, not a primitive, and it is not part of
the public compatibility surface at all. Each states its own contract, boundary
and rejected alternative at its definition: `accent_bar`, `ghost_icon_button`,
`disabled_child_scope`, `button_enabled` and `CardFrame` / `CardSurface` /
`CardSizing` in `ui/widgets/{feedback,controls,containers}.rs`, `edge_rule` in
`ui/widgets/containers.rs`, and `ui::column_table` in its own module header. Two
points of the boundary are visible only from the role contract:

- **`edge_rule` absorbed four hand-rolled copies, one of which had no half-pixel
  offset** — the branch-multi-selection bar at `multi_selection.rs`, whose
  hairline moved by half a pixel when it was folded in. That was the one pixel
  change the extraction made.
- **`empty_state(ui, text)` settled the ink for a role the tree had seven
  conventions for**, and it is the one place a contrast audit will flag a
  sub-AA step that was accepted deliberately. It paints one "there is nothing
  here" sentence at `INK_3` and `TYPE_BODY`; the tree carried **seven**
  conventions for that one role, and one of them was a raw `Color32::GRAY` at
  **3.50:1 on `SURFACE` — below the AA floor**. The rationale is that an empty
  state is never what the reader opened the pane for, and `INK_3` is the step
  their neighbouring column headers already use.

  **The ratified tradeoff, with its measured cost.** `INK_3` is **5.01:1 on
  `BG`** but **4.20:1 on `SURFACE`** — and empty states inside panes sit on
  `SURFACE`, so the role is *marginally under* the 4.5:1 AA floor there. This was
  reviewed and accepted deliberately (the alternative, `INK_2`, is 7.86:1 on `BG`
  and 6.58:1 on `SURFACE` and reads louder). **Do not "correct" this to `INK_2`
  or back to `INK` without raising it as a design decision** — the numbers above
  are the reason, and the nine sites that were previously `INK` at 10.55:1 are
  the visible consequence. If a contrast audit flags `empty_state` as sub-AA,
  that is this entry, already considered.

  Two sites are deliberately outside the helper. `commit_window.rs` stays a
  *string producer* — the empty-state text is built by one surface and painted by
  another there, and moving both at once would change which module owns the
  string. `branch_tree_view::no_branches_state` is a different role (a call to
  action: a primary-ink sentence *plus* a primary button), not a report that
  there is nothing.

Feature surfaces may compose these roles, but a feature surface must not
silently redefine their geometry or colors with local literals.

## Feature-component contracts

The branch popup/status indicator (`ui::branch_widget`) and the branch tree
(`ui::branch_tree_view`) are reusable feature components. They are not generic
buttons, rows, or lists merely because they use `ui::widgets` primitives, and
they retain branch-specific vocabulary: current branch, favorite, ahead/behind,
gone upstream, stale age, protected branch, local/remote/tag grouping,
collapsible groups, checkout/rename/delete gating, and compare/worktree actions.
The branch tree's current contract is data and events oriented — it receives the
tree data and caller-owned state, returns events, and does not call Git or
dispatch operations — so any change that would move Git policy, application
dispatch, or editor state into a generic branch-kit primitive is outside the
component boundary.

`ui::diff` is a specialized diff feature component whose model, panes, actions and
rendering own diff-specific meaning: added/removed lines, file/hunk/line
granularity, side-by-side/unified view, and diff selection. Its internal modules
are not a second general widget API. Shared chips, rows, buttons and icons may be
used where the semantic roles match; diff rows and diff actions remain diff roles.

Dialog and popup modules are feature composition surfaces that may own their
specific list, loading, empty, error and action content. The conflict pane kit is
intentionally narrower — shared visual grammar for the three-pane conflict view,
but it does not parse files, choose resolution, hold an editor, or apply a result.
The inline conflict surface and dedicated resolver may differ in their Result
editor while sharing the read-only conflict presentation primitives.

## Weakly adopted APIs: final dispositions

Production adoption now settles the role of the previously weakly adopted public
APIs. These classifications guide future adoption; they do not authorize removal,
deprecation, or a breaking import change.

| API | Disposition | Production basis and boundary |
| --- | --- | --- |
| `ghost_button`, `primary_button`, `compact_button`, `icon_button` | **supported general** | Actively used across dialogs, settings, navigation, feature actions, and shell-adjacent surfaces. They remain the reusable shell/action vocabulary and remain distinct from branch-kit buttons. |
| `compact_button_enabled` | **specialized** | Retained for the branch popup's gated row actions. Its disabled child scope and no-op click behavior serve that feature's action-gating contract; it does not replace `components::KitButton` or become the branch button API. |
| `action_button` | **specialized** | The full-width stacked primary/secondary treatment. Its log commit-details consumer went with ADR-0024, which moved every commit action into the commit menu, so it has no production caller today and is retained as a kit role for a surface that needs a stacked action block. It is not required to absorb branch, dialog, or other feature action policy. |
| `dialog_footer` | **supported general** | Used by push, new-branch, tag, settings, smart-rule, and remote-management dialogs. It owns only the separator and right-aligned action slot, never dialog policy. The separator itself is now `footer_rule`'s, shared with modal bodies that rule themselves off from an action slot without owning a footer. |
| `card`, `card_header` | **supported general** | Used by the Commit tool window's local, staged, and changes regions. These are general containment roles, not aliases for branch detail panels or feature rows. |
| `note` | **supported general** | Used by rebase previews/results and multiple dialog summaries and warnings. Optional severity emphasis does not turn it into `alert_box`. |
| `alert_box` | **specialized** | The contained warning surface it defines. Its commit-details guardrail consumer went with ADR-0024 along with the buttons it explained, and it has no production caller today; a future surface that needs a warning with a visible boundary is what it is for. It is not the general `note`, inline-error, banner, or toast role. |
| `segmented_control` | **supported general** | Used for settings choices, diff side-by-side/unified mode, and file/hunk/line granularity. Each caller still owns its option list and state transition. |
| `hash_chip`, `avatar_initials`, `churn_bar` | **specialized** | Retained as named commit-detail primitives used by the Log commit-details surface. `hash_chip` is a non-interactive reference chip (see *Chip roles*); the other two are the identity and statistics roles they have always been. They are not promoted to generic reference, identity, or statistics widgets. |

The only final dispositions are **supported general** and **specialized**. Branch
and conflict APIs retain their specialized semantics regardless of whether a
general primitive shares geometry with them. There is no "candidate" disposition
left to award: the one it used to carry, `status_badge`, went with `ref_label`,
`CountDirection` and `tree_row` in the dead-API sweep, and an API with no
production consumer now goes through that sweep rather than waiting for a review.

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
the boundary: a UI presentation module may render the values, but the data model
remains owned by `turbogit_app`.

The implemented `ui::widgets/` directory is a public façade plus private focused
implementation modules, not a second import path for callers, and the focused
modules cannot require a flag-day migration because the established names are
explicitly re-exported from `ui::widgets`. Moving implementation helpers must not
move `ui::kit::conflict_pane` to a public path or accidentally turn an internal
helper into a feature component API. Any later change to a role, boundary or
public path must update this document and retain the corresponding compatibility
re-export.

Two sweeps changed the *advertised* surface without changing the source of truth
for it: the dead-API sweep took `ref_label`, `status_badge` (and its
`StatusBadge` / `CountDirection` vocabulary) and `tree_row` off the `ui::widgets`
façade, and v2 retired `toolwindow_header` in favour of `widgets::pane_header` and
renamed `ui::branches::scope_label` → `scope_chip` and
`ui::submodules::status_label` → `status_mark`. The first moved no pixel. The
second does not hold for the tree as it stands: R1–R7 repaint the accent, the ink
ramp, card fills, selection and pane chrome, so any comparison against a pre-v2
capture is invalid by construction, and the tracked goldens under
`crates/turbogit-ui/tests/snapshots/` are the only valid baseline.

The five frozen compatibility import paths listed above all still resolve:

| Path | Declared at |
| --- | --- |
| `turbogit_ui::render` (re-export of `ui::render`) | `crates/turbogit-ui/src/lib.rs:6` |
| `turbogit_ui::ui::banner::{AppBanner, BannerAction, AppSeverity}` (aliases for the app-owned banner types) | `crates/turbogit-ui/src/ui/banner.rs:12` |
| `turbogit_ui::ui::diff::{PaneCache, PaneEntry, PaneSide}` | `crates/turbogit-ui/src/ui/diff/mod.rs:59` |
| `turbogit_ui::ui::hunk_nav::{Dir, EDGE_WINDOW}` | `crates/turbogit-ui/src/ui/hunk_nav.rs:18` |
| `turbogit_ui::ui::widgets::CASCADE_ACCENT` (token's home is `theme`, re-exported so the widget path keeps resolving) | `crates/turbogit-ui/src/ui/widgets/mod.rs:56` |

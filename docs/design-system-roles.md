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
| Branch component kit | `turbogit_ui::ui::components` (`KitButton`, `RowState`, branch fills, `section_header`, `pill`, sync helpers, branch geometry, and branch detail primitives) | Branch-screen grammar: fixed branch geometry, branch rows, sync relationships, current/count pills, collapsible branch sections, and branch actions. It is not a general button or title library. | Public branch-kit path, with a specialized role. |
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
  `widgets::action_button` is the specialized full-width treatment used by the
  log commit-detail Actions section. Neither replaces the general vocabulary.
- `components::KitButton` and `components::kit_button` /
  `kit_button_at` are the **branch-kit button role**. They use the branch
  screen's compact 28px kit height, minimum target rules, four branch action
  variants (`Primary`, `Secondary`, `Quiet`, and `Danger`), and—when needed—an
  explicit width so row actions line up as a column. `Danger` is a branch/destructive
  action treatment, not a general `ButtonVariant` replacement.
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
- `widgets::toolwindow_header` is the tool-window title/action strip, not either
  of the above. `components::detail_panel_header` is the branch detail-panel
  header, not a generic group title.
- A surface-specific region band (for example, a repo block or a diff pane
  header) is not automatically a title widget. Its geometry belongs to that
  feature unless a shared role is explicitly established.

### Feedback and messages

These roles are intentionally different. Do not make a caller choose among
them solely because their text is a warning, error, or status.

| Role | Meaning and current implementation seam | Must not become |
| --- | --- | --- |
| Inline error | A short, one-line error associated with a field, control, or immediate operation. It is adjacent feedback, not a container; its error ink and wrapping policy are independent of multi-line failures. | A dialog error list, alert well, banner, or toast. |
| Note | `widgets::note` is an inset `SURFACE_2` well for a preview, summary, or explanatory note. It has optional severity emphasis through the caller's stroke, but the default note is not an error surface. | An always-warning alert or a global banner. |
| Alert | `widgets::alert_box` is a contained warning/guardrail surface with `SURFACE_WARNING`, a warning icon, and warning ink. It is for a warning that needs a visible boundary. | A bare inline error, a general note, or a transient toast. |
| Banner | `ui::banner` renders a severity-tinted horizontal strip with a state-owned message and optional deep-link actions. `banner::maybe_show` is the app-wide host; `banner::show` is the surface-scoped entry point. | A contained alert or a toast. It is anchored to a surface/application feedback state and may carry actions. |
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
and the commit-details guardrail warning uses `alert_box` — and that no
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
- `widgets::ref_label` with `RefKind` states a Git ref (`Branch`, `Remote`,
  `Tag`) using solid semantic ref colors.
- `widgets::status_badge` with `StatusBadge` states an application/status fact
  such as ahead/behind count, lock, stale age, focused, or cascade. Its
  direction and status semantics are not file-status or Git-ref semantics.
- `widgets::hash_chip` is a clickable commit reference with monospace content
  and copy/hover behavior. It is a specialized interactive reference, not a
  generic label.
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

### Row roles and selection states

- `widgets::tree_row` is the general fixed-height tree/list row. It lays out a
  caller's contents, owns the row interaction target, and uses
  `RowState::from_flags`; the common painter-level form is `widgets::paint_row`.
  It is not a universal row for commits, files, diff lines, or conflict panes.
- `components::RowState`, `row_fill`, and `current_row_fill` belong to the
  branch/shared row-state grammar. The branch row, repo section, and branch
  detail surfaces add branch-specific geometry, current-branch facts, sync
  markers, stale ink, and operation labels on top of that grammar.
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
- `RowState::BrandSelected` is the shared tree/list chosen row: a solid
  `Palette::BRAND` fill with `BRAND_INK` content. `RowState::from_flags`
  intentionally maps the old boolean `(selected, hovered)` tree API to this
  role; callers that need another selection role must construct it explicitly.
- `RowState::FocusSelected` is the translucent `Palette::selection_bg()`
  focus band used by the log table, sidebar tree, and blame surfaces. It is not
  the opaque `Palette::SELECTION` or `Palette::SELECTION_BG` fill.
- A current branch is a repository fact, not a pointer state.
  `components::current_row_fill` keeps its resting brand tint distinct from
  hover and selection, and keeps selection visible when the current row is also
  selected.

Selecting one of these states is a visible decision. Do not collapse them to
a single boolean or infer that “selected” means the same thing in every list.

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
- `dialog_footer` owns the shared modal action separator/alignment. It does not
  own the dialog's action policy or its error/list content.
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
| `action_button` | **specialized** | Retained for the log commit-detail Actions section, where the full-width stacked primary/secondary treatment is meaningful. It is not required to absorb branch, dialog, or other feature action policy. |
| `dialog_footer` | **supported general** | Used by push, new-branch, tag, settings, smart-rule, and remote-management dialogs. It owns only the separator and right-aligned action slot, never dialog policy. |
| `toolwindow_header` | **supported general** | Used by Log branches, Blame, Worktrees, and Submodules tool-window headers. It owns only the title/action strip. |
| `card`, `card_header` | **supported general** | Used by the Commit tool window's local, staged, and changes regions. These are general containment roles, not aliases for branch detail panels or feature rows. |
| `note` | **supported general** | Used by rebase previews/results and multiple dialog summaries and warnings. Optional severity emphasis does not turn it into `alert_box`. |
| `alert_box` | **specialized** | Retained for the contained guardrail warning in commit details. It is not the general `note`, inline-error, banner, or toast role. |
| `segmented_control` | **supported general** | Used for settings choices, diff side-by-side/unified mode, and file/hunk/line granularity. Each caller still owns its option list and state transition. |
| `status_badge` | **candidate for retirement** | The semantic status vocabulary has no current production consumer outside characterization coverage. Keep the public API available unchanged, but do not add adoption solely to justify it; revisit it in a future removal review. |
| `hash_chip`, `avatar_initials`, `churn_bar` | **specialized** | Retained as named commit-detail primitives used by the Log commit-details surface. They are not promoted to generic reference, identity, or statistics widgets. |

The only final dispositions are **supported general**, **specialized**, and
**candidate for retirement**. A candidate remains public and supported by its
existing compatibility export; the disposition is not a deprecation. Branch and
conflict APIs retain their specialized semantics regardless of whether a
general primitive shares geometry with them.

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
compatibility paths described above; the role/architecture work itself remains
behavior-preserving. Any later change to a role, boundary, or public path must
update this document and retain the corresponding compatibility re-export.

# TurboGit — UI Redesign Specification (v1)

> **Purpose.** This document is the single source of truth for applying the new design system (defined by the HTML mockups in `C:/projects/turbogit/turbogit-screens/`) to the TurboGit egui application. It translates every visual token, layout structure, and page composition from the mockups into concrete, actionable requirements for the Rust/egui codebase. It is structured to be consumed directly by a code-generation step.
>
> **Source of truth for visuals:** the mockups in `C:/projects/turbogit/turbogit-screens/pages/*.html` and their shared stylesheet `colors_and_type.css`. Where this document and the mockups disagree, **the mockups win** — update this spec to match, never silently diverge.
>
> **Status:** v1 — initial translation from the 8-page mockup set (validated: 8/8 pages pass).

---

## Table of Contents

1. [Design System Overview](#1-design-system-overview)
2. [Color Token Map](#2-color-token-map)
3. [Typography](#3-typography)
4. [Spacing & Metrics](#4-spacing--metrics)
5. [Iconography](#5-iconography)
6. [App Shell Layout](#6-app-shell-layout)
7. [Shared Widget Library](#7-shared-widget-library)
8. [Page Specifications](#8-page-specifications)
9. [State Model Changes](#9-state-model-changes)
10. [Implementation Plan](#10-implementation-plan)
11. [Verification & Acceptance](#11-verification--acceptance)
12. [Open Questions](#12-open-questions)

---

## 1. Design System Overview

The new design is an IntelliJ IDEA-inspired IDE shell with a Darcula-derived dark palette.

**Key characteristics:**

- Dark-first: all 8 pages ship dark-only. Light and High Contrast modes are out of scope for v1 and must remain functional but visually unchanged until a light palette is defined.
- Flat surfaces with 1px borders instead of shadows (shadows reserved for floating layers only).
- Blue primary action color (`#3574f0`) used sparingly: one primary button per surface, selected states, focus rings, hash text.
- Semantic status colors (success/warning/error/info) applied consistently across badges, labels, and diff/conflict highlights.
- Monospace font for code/diff/graph content; proportional font everywhere else.
- Dense layout: small paddings (4–8px), tight line heights (1.4–1.6), compact row heights (24–32px).

---

## 2. Color Token Map

Every CSS custom property in `colors_and_type.css` maps to a Rust constant or egui `Visuals` field. Define these centrally in `src/theme.rs`.

### 2.1 Core palette

| CSS variable | Hex | Rust constant | Usage |
|---|---|---|---|
| `--tg-bg` / `--tg-background` | `#1e1f22` | `BG` | App background, panel fill |
| `--tg-surface` / `--tg-card` | `#2b2d30` | `SURFACE` | Topbar, tool window headers, table headers, dialogs |
| `--tg-surface-2` / `--tg-muted` | `#313438` | `SURFACE_2` | Hover fills, secondary buttons, inactive tabs |
| `--tg-surface-3` / `--tg-popover` / `--tg-input` | `#3c3f41` | `SURFACE_3` | Inputs, popover backgrounds, badge backgrounds |
| `--tg-line` / `--tg-border` | `#4e5157` | `LINE` | Primary borders/separators |
| `--tg-line-subtle` | `#36383c` | `LINE_SUBTLE` | Row separators, subtle dividers |
| `--tg-ink` / `--tg-foreground` | `#bcbec4` | `INK` | Primary text |
| `--tg-ink-2` | `#a0a3ab` | `INK_2` | Secondary text, toolbar buttons |
| `--tg-ink-3` / `--tg-muted-foreground` | `#808080` | `INK_3` | Muted/hint text, section headers, gutters |
| `--tg-brand` / `--tg-primary` / `--tg-ring` / `--tg-focus` | `#3574f0` | `BRAND` | Primary actions, selection, links, hash text |
| `--tg-brand-ink` / `--tg-primary-foreground` | `#ffffff` | `BRAND_INK` | Text on brand-colored fills |

### 2.2 Status colors

| Variable | Hex | Constant | Usage |
|---|---|---|---|
| `--tg-state-success` | `#4caf50` | `STATE_SUCCESS` | Added badge, remote label, success toast |
| `--tg-state-warning` | `#f9a825` | `STATE_WARNING` | Modified badge, tag label, conflict marker, star |
| `--tg-state-error` | `#ef5350` | `STATE_ERROR` | Deleted badge, error text/toast, conflict-theirs border |
| `--tg-state-info` | `#42a5f5` | `STATE_INFO` | Info toast, conflict-yours border |

### 2.3 Diff colors

| Variable | Hex | Constant | Usage |
|---|---|---|---|
| `--tg-diff-add` | `#344f3e` | `DIFF_ADD_BG` | Added-line background |
| `--tg-diff-add-text` | `#85e89d` | `DIFF_ADD_TEXT` | Added-line text and `+` gutter |
| `--tg-diff-del` | `#5a3a3a` | `DIFF_DEL_BG` | Deleted-line background |
| `--tg-diff-del-text` | `#ff9a9a` | `DIFF_DEL_TEXT` | Deleted-line text and `-` gutter |
| `--tg-diff-hunk` | `#2b2d30` (= SURFACE) | `DIFF_HUNK_BG` | Hunk-header background |

### 2.4 Derived colors (not in CSS, computed at runtime)

| Name | Derivation | Usage |
|---|---|---|
| `SELECTION_BG` | BRAND at ~15% alpha over BG | Selected log table rows (CSS `color-mix(in srgb, var(--tg-primary) 15%, transparent)`) |
| `CONFLICT_YOURS_BG` | STATE_INFO at ~12% alpha over BG | Conflict "yours" block background |
| `CONFLICT_THEIRS_BG` | STATE_ERROR at ~12% alpha over BG | Conflict "theirs" block background |
| `CONFLICT_MARKER_BG` | STATE_WARNING at ~15% alpha over BG | Conflict marker strip background |
| `BACKDROP` | Black at 65% alpha | Modal/popup dim overlay |
| Focus ring stroke | BRAND, 1px + spread approximation | Input focus (approximates CSS `box-shadow: 0 0 0 2px rgba(53,116,240,.25)` with egui stroke) |

### 2.5 Mapping into egui Visuals

Apply via `theme::configure_style(ctx, mode)` when `mode == ThemeMode::Dark`. Required changes from current implementation:

| egui field | New value |
|---|---|
| `visuals.panel_fill` | `BG` (#1e1f22) |
| `visuals.window_fill` | `SURFACE` (#2b2d30) |
| `visuals.extreme_bg_color` | `BG` (#1e1f22) |
| `visuals.override_text_color` | `INK` (#bcbec4) |
| `visuals.faint_bg_color` | `SURFACE_2` (#313438) |
| `visuals.code_bg_color` | `SURFACE_3` (#3c3f41) |
| `visuals.hyperlink_color` | `BRAND` |
| `visuals.warn_fg_color` | `STATE_WARNING` |
| `visuals.error_fg_color` | `STATE_ERROR` |
| `widgets.*.bg_fill` | per-widget-state mapping below |
| `widgets.*.fg_stroke` | INK_2 default, INK on hover/active |
| `selection.bg_fill` | BRAND at ~25% premultiplied alpha |
| `selection.stroke` | 1px BRAND |
| `window_corner_radius` | 8 (`radius-lg`) |
| `menu_corner_radius` | 6 (`radius-md`) |

Widget state → surface mapping:

| State | bg_fill | fg_stroke |
|---|---|---|
| noninteractive | SURFACE | INK_2 |
| inactive | SURFACE_2 | INK_2 |
| hovered | SURFACE_2 | INK |
| active | SURFACE_3 | INK |
| open | SURFACE_3 | INK |

Corner radius scale (CSS `--tg-radius-*`): sm = 4, md = 6, lg = 8, full = pill (use `CornerRadius::same()`; pill approximated by height/2).

---
## 3. Typography

### 3.1 Font families

| Role | Design font | egui strategy |
|---|---|---|
| Body/UI | JetBrains Mono → SF Mono → Segoe UI → system-ui | Load JetBrains Mono at startup; proportional fallback = Segoe UI (Windows) |
| Code/Diff/Graph | Same mono stack, explicit | `TextStyle::Monospace` = JetBrains Mono |
| CJK support | Noto Sans SC / PingFang SC / Microsoft YaHei listed in stack | Add system CJK fallback fonts if available; not required for v1 correctness |

Load fonts once at startup via `ctx.set_fonts(FontDefinitions)`:

1. Include JetBrains Mono Regular + Bold as binary includes (`include_bytes!`) or load from the OS font directory.
2. Set `Proportional` family = JetBrains Mono, then Segoe UI as fallback entry (matches the mockups' mono-everything look).
3. Set `Monospace` family = JetBrains Mono, Consolas fallback.

If bundling is rejected (§12.2), fall back to `Segoe UI` for Proportional and `Consolas` for Monospace — spacing shifts slightly but all layouts in §4 must still fit.

### 3.2 Font sizes

| Style | CSS reference | Size | Weight | egui TextStyle |
|---|---|---|---|---|
| Body/UI text | base 13px, line-height 1.5 | 13–14px | 400 | `Body`, `Button` |
| Small/muted | `.tg-menubar`, `.tg-toolbar-btn` 12px | 12px | 400 | `Small` |
| Section header | `.tg-toolwindow-header` 11px uppercase | 11px | 600 | `Small` + `RichText::strong()` |
| Dialog title | `.tg-dialog-header` 14px semibold | 14px | 600 | helper fn returning styled `RichText` |
| Welcome brand | `.welcome-brand` 42px bold | 42px | 700 | custom `FontId` |
| Code | `.tg-merge-code` 12px, line-height 1.6 | 12–13px | 400 | `Monospace` |

Implement section-header and dialog-title styling as helper functions returning styled `RichText` (e.g. `theme::section_header("COMMIT")`, `theme::dialog_title("Push")`) rather than proliferating custom `TextStyle` keys.

### 3.3 Uppercase micro-headers

All `.tg-toolwindow-header` and section-title elements use `text-transform: uppercase; letter-spacing: 0.03em`. In egui, uppercase the string; acceptable to skip letter-spacing (egui lacks native tracking). Never skip the uppercase transform.

---

## 4. Spacing & Metrics

### 4.1 Global spacing (matches current implementation — confirm, no change)

| Property | Value | egui location |
|---|---|---|
| item_spacing | 8 × 6 px | `style.spacing.item_spacing` |
| window_margin | 10 px | `style.spacing.window_margin` |
| button_padding | 10 × 5 px | `style.spacing.button_padding` |
| indent | 14 px | `style.spacing.indent` |

### 4.2 Fixed heights (from mockup measurements)

| Element | Height |
|---|---|
| Top menubar (`.tg-topbar`) | 38px |
| Toolbar (`.tg-toolbar`) | 34px |
| Sidebar rail (`.tg-sidebar`) | 48px wide |
| Sidebar buttons | 36×36px, icon 18×18 |
| Toolbar buttons | 26px tall |
| Tab strip (`.tg-tabs`) | 32px |
| Tab item | 31px tall |
| Tool window header (`.tg-toolwindow-header`) | 28px |
| Tree row (`.tg-tree-row`) | 24px |
| Log branch row | 26px |
| Log file row / detail line | ~22–24px |
| Branch popup row | 28px |
| Dialog header / footer | 40px / ~44px |
| Standard input / button (`.tg-input`, `.tg-btn`) | 32px tall (compact variants 26–28px) |
| Badge / label chip | 18px pill |
| Status bar | ~24px (keep current) |

### 4.3 Fixed widths

| Element | Width |
|---|---|
| Commit changelist pane | 320px |
| Log branches pane | 210px |
| Log details pane | 320px |
| Merge gutter | 44px |
| Settings category list | 176px |
| Branch popup | 420px |
| Push dialog | ~520px |
| Settings dialog | ~768px |

---
## 5. Iconography

The design uses **Lucide icons** exclusively (56 unique names across all pages).

### 5.1 Approach

Embed Lucide SVG path data as inline constants rendered via `egui::Painter`. No runtime asset loading or image crates for v1.

### 5.2 Required icon set (complete list extracted from mockups)

```
alert-circle, alert-triangle, align-justify, archive, arrow-down, arrow-down-circle,
arrow-left, arrow-right, arrow-right-left, arrow-up, bell, book-open, bug, check,
check-square, chevron-down, chevron-left, chevron-right, chevron-up, clock, columns,
download, eye-off, file, file-code, file-minus, file-plus, file-warning, files, filter,
folder, folder-git, folder-open, git-branch, git-commit, git-compare, git-merge,
keyboard, laptop, layers, layout, menu, monitor, more-horizontal, play, plus,
plus-circle, refresh-cw, search, settings, star, tag, trash-2, undo, upload, x
```

### 5.3 Rendering rules

- Sizes by context: 14×14 (toolbar/tree/badge), 16×16 (buttons), 18×18 (sidebar rail), 22×22 (welcome cards).
- Color = contextual text color: INK_3 muted, INK_2 normal, BRAND active/primary, STATE_WARNING stars.
- Implement as `pub fn icon(ui: &mut Ui, name: Icon, size: f32, color: Color32)` in new `src/ui/icons.rs`.
- Lucide icons use a 24×24 viewBox with 2px strokes; scale = size / 24.
- Missing icon: render nothing, never panic; `debug_assert!` + log at debug level.

---

## 6. App Shell Layout

All full-screen pages share one shell. Rebuild `src/ui/mod.rs::render()` to compose nested egui panels matching `.tg-shell`.

### 6.1 Structure (top to bottom)

```
+-----------------------------------------------------------+
| Topbar (38px, SURFACE, bottom border LINE)                |
|   File Edit View Navigate Code Git Window Help            |
+-----------------------------------------------------------+
| Toolbar (34px, BG, bottom border LINE)                    |
|   Run Debug Search [Commit] Update ...         [gear]     |
+--+--------------------------------------------------------+
|S | Tab Strip (32px, BG, bottom border LINE)                |
|i +--------------------------------------------------------+
|d | Active tool window content                              |
|e |                                                         |
|b |                                                         |
+--+--------------------------------------------------------+
| Status bar (~24px)                                        |
+-----------------------------------------------------------+
```

### 6.2 Component details

**Topbar** — `TopBottomPanel::top("topbar")`, exact_height(38), SURFACE fill, bottom stroke LINE. Content: menu items (File, Edit, View, Navigate, Code, Git, Window, Help) as clickable 12px INK_2 labels, 4×8 padding each, hover → SURFACE_2 rounded-sm. Menu dropdowns are out of scope for v1 except File/Git/View (§12.1); other items render inert.

**Toolbar** — `TopBottomPanel::top("toolbar")` added after topbar, exact_height(34), BG fill, bottom stroke LINE. Left-to-right: Run (play), Debug (bug), Search (search) as ghost buttons; **Commit (git-commit)** as the single PRIMARY variant (BRAND fill, white ink); Update Project (refresh-cw), Pull (arrow-down), Fetch (download), Push (upload), Branches (git-branch), Tags (tag); spacer; right-aligned Settings gear (settings). Each button: 26px tall, 0×8 padding, icon 14×14 + label, INK_2 text, transparent bg → SURFACE_2 on hover, radius 4. Primary variant hover: brighter BRAND tint.

**Sidebar rail** — `SidePanel::left("rail")`, exact_width(48), SURFACE fill, right stroke LINE. Vertical column of 36×36 icon buttons (icon 18×18), radius-md: Project (folder), Commit (git-commit), Git Log (git-branch), Search (search), then optionally Run/Debug per §12.4. Active tab's button: BRAND icon + SURFACE_2 bg. Clicking Commit ↔ switches to Commit tab; Git Log ↔ Log tab; others inert in v1.

**Tab strip** — part of central panel, above tool window content: horizontal tabs, 31px tall each, 0×12 padding, icon 14×14 + label, INK_3 text; active tab: INK text, SURFACE bg, LINE border top/left/right only, top corner radius 4. Tabs map to `state::Tab` (§9).

**Tool window content** — the active page body per §8.

### 6.3 Z-ordering (floating layers)

Rendered after all panels, bottom to top of stack order: popups (branches, VCS ops, command palette) → modal dialogs → confirm prompts → toasts. All modals get BACKDROP dim behind them. Popup chrome: SURFACE fill, LINE border stroke, corner_radius 8 (egui has no blur shadows; approximate `.tg-dialog` shadow with slightly darker outer stroke).

---
## 7. Shared Widget Library

New files `src/ui/widgets.rs` and `src/ui/icons.rs`. Every page uses these instead of ad-hoc painting.

### 7.1 Functions to implement

```rust
// Buttons
pub fn ghost_button(ui, icon: Option<Icon>, label: &str) -> Response;    // .tg-toolbar-btn / .tg-btn
pub fn primary_button(ui, icon: Option<Icon>, label: &str) -> Response;  // .tg-btn-primary
pub fn compact_button(ui, label: &str) -> Response;                      // h-7 px-3 text-xs variants
pub fn icon_button(ui, icon: Icon) -> Response;                          // square ghost, e.g. dialog close X

// Chips
pub fn badge(ui, text: &str, kind: BadgeKind) -> Response;               // .tg-badge: Neutral/Added/Modified/Deleted
pub fn ref_label(ui, text: &str, kind: RefKind) -> Response;             // .tg-label: Branch=brand, Remote=success, Tag=warning

// Trees & lists
pub fn tree_row(ui, selected: bool, contents) -> Response;               // 24px, hover SURFACE_2, selected = BRAND bg + white ink
pub fn selectable_row(ui, contents) -> Response;                         // generic row w/ hover, no persistent selection

// Inputs
pub fn search_input(ui, placeholder: &str, buf: &mut String) -> Response; // 26–32px, SURFACE_3 bg, LINE border, focus ring
pub fn text_input(ui, placeholder: &str, buf: &mut String) -> Response;

// Dialog chrome
pub fn dialog_header(ui, title: &str) -> InnerResponse;                  // 40px, title left, X right
pub fn dialog_footer(ui, buttons) ;                                      // right-aligned row, top border LINE

// Section chrome
pub fn toolwindow_header(ui, title: &str, actions: impl FnOnce(&mut Ui)); // 28px, 11px uppercase muted + right actions
pub fn group_title(ui, title: &str);                                      // 11px uppercase INK_3 ("RECENT", "GETTING STARTED")
```

### 7.2 Behavioral rules

- Hover: interactive elements get SURFACE_2 fill unless already solid-filled (primary buttons brighten instead).
- Selected: BRAND fill + BRAND_INK text for tree/list rows; SELECTION_BG (translucent brand) for table rows where full saturation is too loud (log graph).
- Disabled: INK_3 text/fg_stroke, no hover change.
- Focus ring: BRAND 1px stroke on focused inputs/buttons (approximate CSS box-shadow spread).
- Scrollbars: 10px wide, thumb LINE → INK_3 on hover, transparent track (`Visuals::scroll` styling).

---

## 8. Page Specifications

Each section maps mockup → module → required changes. Feature behavior is unchanged; these are visual/layout specs. Feature IDs reference `product-spec.md`.

### 8.1 Welcome / Onboarding (NEW)

**Mockup:** `pages/welcome.html` · **Module:** new `src/ui/welcome.rs` · **Features:** A1, A2, A8

Shown when no project/root is open. Full-window, scrollable, centered content (max-width 980px).

Layout:
1. Brand header: logo icon (38×38, BRAND) + "TurboGit" 42px bold + tagline "A fast, keyboard-friendly Git client for your desktop." (14px INK_3, centered)
2. Two-column grid (left flexible, right fixed 260px):
   - **Left — action cards** (3-up grid):
     - *Clone from URL / Provider* (book-open): opens clone flow (A2)
     - *Open Project* (folder-open): directory picker (rfd)
     - *Initialize Repository* (folder-git): `git init` flow (A1)
     Card style: SURFACE bg, LINE border, radius-md, 18px padding, icon 22px BRAND, title 13px semibold, body 12px INK_3. Hover: SURFACE_2 bg + BRAND border.
   - **Left — clone box**: inline clone form below cards: URL input (full width) + Clone primary button + "Shallow clone" checkbox row
   - **Right — recent projects**: "RECENT PROJECTS" group-title + count badge; rows: project name (13px INK), path (12px INK_3), last-opened meta (11px INK_3), branch indicator. Hover SURFACE_2. Click opens that root.
3. Getting Started hints: 5 numbered tips (stage → commit → push → pull → view log), each icon + one line, INK_2.

State/persistence: add `RecentProject { path, name, last_opened, branch_snapshot }` to `UiState`, persisted via `persistence.rs`. Welcome visible when `roots.is_empty() || ui.welcome_visible`.

### 8.2 Main Commit Window

**Mockup:** `pages/main-commit.html` · **Module:** `src/ui/commit_window.rs` · **Features:** C1–C14

Shell context: sidebar "Commit" active; tabs: Local Changes (active) · Unversioned Files · Shelf · Stash (Shelf/Stash visibility per §12.6).

Layout inside tool window body:
1. Tool window header: "COMMIT" + settings/collapse icon buttons right-aligned
2. Horizontal split: left pane 320px fixed, right pane flexible
3. **Changelist pane** (left):
   - Collapsible changelist groups: header (name 12px semibold + count badge + chevron): "Default Changelist" (4), "Merge Conflicts" (1)
   - Root sub-groups (multi-root): root name row (folder icon, semibold) + modified-count badge
   - File rows: 24px, checkbox (14×14) + status icon (file / file-plus / file-minus / file-warning) + filename + trailing M/A/C badge. Checked = included in commit. Selected row = BRAND bg.
   - Root rows have select-all checkboxes for their subtree.
4. **Diff preview pane** (right): file breadcrumb ("turbogit-cli / src/main.rs | modified"), then unified diff: hunk headers (SURFACE bg, INK_3), del lines DIFF_DEL_BG/TEXT, add lines DIFF_ADD_BG/TEXT, gutter line numbers INK_3.
5. **Message editor** (bottom of right pane): "COMMIT MESSAGE" group title, multiline input (SURFACE_3, LINE border, radius-sm); "Advanced options..." disclosure in BRAND text below.
6. **Action row**: Amend checkbox · "Commit and Push..." ghost · Commit primary. Commit disabled when message empty or nothing checked.

### 8.3 Git Log

**Mockup:** `pages/git-log.html` · **Module:** `src/ui/log_window.rs` · **Features:** H1–H4, B3/B11

Shell context: sidebar "Git" active; toolbar shows log actions: Update Project, Commit..., Push, Pull, Fetch, Branches, Tags, Incoming (badge count).

Four-pane workspace:

1. **Branches pane** (left, 210px): header "BRANCHES"; search input (28px, 8px margins); groups LOCAL (star toggle + name), REMOTE (muted, origin/ prefix), TAGS; bottom "ROOTS:" filter (All roots / per-root radio rows). Row states: selected BRAND bg; star STATE_WARNING at 40%/100% opacity.
2. **Graph pane** (center): filter toolbar (SURFACE bg): search + filter dropdowns; root-stripe chip row (11px INK_3); commit table columns Graph | Hash | Author | Date | Message. Graph cell mono colored lanes (keep existing assign_colors lane algorithm). Hash cell BRAND mono. Message cell wraps with ref chips inline (.tg-label.branch BRAND pill / .remote SUCCESS pill / .tag WARNING pill). Selected row = SELECTION_BG translucent brand, not solid.
3. **Changed files pane** (right-top, 320px): "CHANGED FILES (n)" header; file rows path + M/A badge; click loads diff.
4. **Commit details pane** (right-bottom, 200px fixed height, SURFACE bg): key-value lines (Hash:, Author:, Date:, Parents:) INK_3 labels / INK values, hash mono; full message paragraph; file summary badges ("2 modified · 1 added").

### 8.4 Diff Viewer

**Mockup:** `pages/diff.html` · **Module:** `src/ui/diff.rs` · **Features:** K1–K5

Entry-point chrome: breadcrumb of file path; segmented control Side-by-Side | Unified; hunk nav ‹ n/N ›; Ignore whitespace checkbox; revision chips (Repo/Staged/Local ↔ Before/After working tree).

Side-by-side layout:
- Pane headers (28px, SURFACE): "Before index 4a20283..." / "After working tree" + Modified badge
- Panes: line-number gutter (44px, SURFACE, right-aligned INK_3, right border LINE) + code area (mono 12px, BG)
- Del lines DIFF_DEL_BG + DIFF_DEL_TEXT; add lines DIFF_ADD_BG + DIFF_ADD_TEXT; context plain INK/BG; hunk separators "..." SURFACE bg INK_3
- Unified mode: single pane, combined +/- gutter markers

Preserve existing interactions: file-click → diff load, hunk keyboard nav, whitespace toggle.

---
### 8.5 Branches Popup

**Mockup:** `pages/branches.html` · **Module:** `src/ui/branch_widget.rs::branches_popup` · **Features:** E1–E10, B5/B6

Floating popup: 420px max width, max-height viewport − 48px, BACKDROP behind.

Structure:
1. Header: "Branches" (dialog title) + search input (flex fill) + close X (24×24 ghost)
2. Body (scrollable), sections divided by LINE_SUBTLE:
   - RECENT — starred rows first
   - LOCAL — current branch pinned top with check icon + emphasized row; others plain
   - REMOTE — muted rows, `origin/` prefix INK_3
   - TAGS — tag icon + name
   - Multi-root sync notice at bottom: info strip "Synchronous branch operations across N repositories"
3. Row actions (context menu or hover affordances): New Branch..., Checkout, Rename, Delete, Compare..., New Worktree from...

Row anatomy: 28px tall, 0×16 padding, 8px gap: [status icon] [name] ... [star] [remote-label chip]. Selected = BRAND bg; star = STATE_WARNING.

Behavior: live filter across sections; Enter checks out highlighted row; Esc closes.

### 8.6 Push Dialog

**Mockup:** `pages/push.html` · **Module:** `src/ui/dialogs.rs` (`Dialog::Push`) · **Features:** D4–D7, B4/B8

Modal ~520px wide, BACKDROP behind, radius-lg, SURFACE bg.

Structure:
1. Header: "Push" + close X
2. Commits tree (scrollable):
   - Root node: "<project> (all roots)" (selected style)
   - Per-root nodes: "<root> — N commits ahead" (name semibold + count INK_3)
   - Commit rows under each root: short-hash (mono BRAND) + subject (INK) + author + relative time (INK_3), indented
3. "Changed files in <hash>" expandable section under selected commit: path + M/A badges
4. Options checkboxes: Push tags · Force push (--force-with-lease) · Push current branch only
5. Force-push warning strip: alert-triangle icon + STATE_WARNING text "Force push rewrites history on the remote. Verify the commits above before continuing."
6. Footer right-aligned: Cancel (ghost) · Preview (ghost) · Push (primary)

Multi-root: tree aggregates outgoing commits across roots; selecting a root node filters the file list; Push executes batch push (B4) unless "current branch only" narrows scope. Checking force-push doubles as acknowledgment of the warning.

### 8.7 3-Way Merge Editor

**Mockup:** `pages/merge.html` · **Module:** `src/ui/conflicts.rs` · **Features:** G1–G9

Full-window layout inside the shell during conflict resolution (NOT a modal dialog).

Three equal-width panes side-by-side (Local | Result | Incoming), LINE borders between:

1. **Pane headers** (28px, SURFACE): label semibold ("Local (Yours)" / "Result" / "Server / Incoming (Theirs)") + branch chip 11px INK_3 (main / merged-result / origin/feature). Result pane outlined 2px BRAND (focus indicator).
2. **Code areas**: mono 12px, line-height 1.6; per-pane gutter 44px (SURFACE bg, INK_3 numbers).
3. **Conflict blocks** in Result pane (replacing raw marker text): bordered rounded-sm blocks with:
   - Marker strips: CONFLICT_MARKER_BG + STATE_WARNING text for `<<<<<<< HEAD`, `=======`, `>>>>>>> origin/feature`
   - Yours section: CONFLICT_YOURS_BG + 3px STATE_INFO left border + "Accept Yours" compact button
   - Theirs section: CONFLICT_THEIRS_BG + 3px STATE_ERROR left border + "Accept Theirs" compact button
   - Actions row: Accept Yours · Accept Theirs · Ignore
4. **Toolbar**: breadcrumb "src/conflict.rs" + "Apply Non-Conflicting Changes" ghost button
5. **Pane footer** (28px): "N conflicts remaining" (INK_2) · "Ln X" position (INK_3)

Resolving updates Result immediately and decrements remaining count; Apply (G4) enables at zero.

### 8.8 Settings Dialog

**Mockup:** `pages/settings.html` · **Module:** `src/ui/mod.rs::settings_window` · **Features:** Q1+

Large modal (~768px), centered, BACKDROP behind.

Structure:
1. Header: "Settings" + help (?) + close (X) icon buttons (24×24 ghosts)
2. Body horizontal split: left category list (176px, LINE right border); right settings panel (flexible, 20px padding, scrollable)
3. Categories: Version Control > Appearance, Version Control, Git, Notifications, Keymap (sub-items indented). Selected = tree-row selected style.
4. Settings rows — each row: label 13px INK + optional description 12px INK_3 below + control right-aligned:
   - Git executable path (text input + folder-browse button)
   - Staging mode checkbox ("Use staging area instead of classic commit")
   - Sync branch operations across roots toggle
   - Update method dropdown: Rebase | Merge | Fast-forward-only
   - Clean-tree method dropdown: Stash | Discard
   - Protected branches pattern list (chips + add input)
   - Commit checks: Run hooks / Check commit message / Sign-off checkboxes
   - CRLF handling radio group: Convert to LF on commit | Convert to CRLF on checkout | No conversion
   - Date format dropdown: System default | ISO 8601 | RFC 2822
   - Manage Remotes button (opens remotes manager A6)
5. Footer: Reset (ghost) | Cancel (ghost) | Apply (ghost, disabled until dirty) | OK (primary)

Controls bind to existing `VcsSettings`; Apply persists via `persistence.rs`.

---
## 9. State Model Changes

Minimal additions to `src/state.rs`; extend existing enums rather than replace.

### 9.1 Tab enum

```rust
pub enum Tab {
    #[default]
    Commit,
    Log,
    History,   // kept for compatibility; hidden from tab strip in v1
    Settings,  // moves out of tab strip; becomes modal-only (§8.8)
}
```

Tab strip renders: Local Changes (Commit) · Git Log (Log). History/Settings remain valid internal states but are not shown as tabs — History is reachable via log context menus, Settings via the toolbar gear.

### 9.2 New UI state

```rust
pub struct UiState {
    // ... existing fields ...
    pub welcome_visible: bool,        // true when no root open -> show welcome instead of shell
    pub show_toolbar: bool,           // user-toggleable, persisted
}

pub struct RecentProject {
    pub path: String,
    pub name: String,
    pub last_opened: i64,             // unix timestamp
    pub branch_snapshot: String,
}
```

`UiState` gains `recent_projects: Vec<RecentProject>`, persisted alongside other UI state in `persistence.rs`. Welcome visibility derives from `state.multi.roots.is_empty() || state.ui.welcome_visible`; reopening via File → Welcome or closing all roots sets `welcome_visible = true`.

### 9.3 Dialog enum

No changes required. Existing `Dialog::{Push, Merge, Rebase, InteractiveRebase, NewBranch, Tag, Shelve, Stash}` covers §8.6/§8.8 flows. The merge editor (§8.7) is not a `Dialog` — it replaces central panel content when conflicts exist (current behavior), now with the three-pane layout.

### 9.4 ThemeMode

Keep `ThemeMode::{Dark, Light, HighContrast}`. Only Dark receives the new palette; Light/HighContrast keep current palettes unchanged. Add a `palette(mode)` accessor returning the token set so widgets never hardcode hex values — they call `theme::palette().brand`, `theme::palette().surface_2`, etc. This keeps a future light palette a drop-in addition.

---

## 10. Implementation Plan

Ordered phases, each independently shippable.

### Phase R0 — Theme tokens + fonts (foundation)

| Step | Detail | Files |
|---|---|---|
| R0.1 | Add all §2 color constants + `Palette` struct + `palette(mode)` accessor | `src/theme.rs` |
| R0.2 | Rewrite `dark_visuals()` to consume Palette (mapping §2.5) | `src/theme.rs` |
| R0.3 | Font loading per §3.1 (JetBrains Mono embedded or system, fallback chains) | `src/theme.rs` or new `src/fonts.rs` |
| R0.4 | Section-header / dialog-title helper functions | `src/theme.rs` |
| R0.5 | Verify dark theme against token sheet; light/high-contrast unaffected | manual screenshots |

### Phase R1 — Shared widget library + icons

| Step | Detail | Files |
|---|---|---|
| R1.1 | Implement icons.rs with all §5.2 icons as painter primitives | `src/ui/icons.rs` |
| R1.2 | Implement widget library per §7 | `src/ui/widgets.rs` |
| R1.3 | Refactor one existing surface (toolbar buttons) to widgets as smoke test | `src/ui/mod.rs` |
| R1.4 | Unit tests: badge kind → color mapping; tree_row selection logic | `src/ui/widgets.rs` |

### Phase R2 — Shell layout

| Step | Detail | Files |
|---|---|---|
| R2.1 | Add topbar + toolbar panels to render() | `src/ui/mod.rs` |
| R2.2 | Add sidebar rail; wire clicks to Tab switching | `src/ui/mod.rs` |
| R2.3 | Render tab strip in central panel mapped to Tab | `src/ui/mod.rs` |
| R2.4 | Restyle status bar (24px, SURFACE bg, LINE top border) | `src/ui/mod.rs` |
| R2.5 | Verify all windows/popups still open/close on top of shell | manual |

### Phase R3 — Page implementations (order recommended below)

| Step | Page | Files touched |
|---|---|---|
| R3.1 | Commit window restyle | `src/ui/commit_window.rs` |
| R3.2 | Git log four-pane restyle | `src/ui/log_window.rs` |
| R3.3 | Diff viewer side-by-side | `src/ui/diff.rs` |
| R3.4 | Branches popup restyle | `src/ui/branch_widget.rs` |
| R3.5 | Push dialog restyle | `src/ui/dialogs.rs` |
| R3.6 | Merge editor three-pane | `src/ui/conflicts.rs` |
| R3.7 | Welcome screen (needs state/persistence additions) | `src/ui/welcome.rs` (new), `src/state.rs`, `src/persistence.rs` |
| R3.8 | Settings dialog restyle | `src/ui/mod.rs` |

### Phase R4 — Polish & cross-cutting

| Step | Detail | Files |
|---|---|---|
| R4.1 | Toast semantic colors + icon (success/warning/error/info) | `src/ui/mod.rs` |
| R4.2 | Confirm prompt restyle | `src/ui/mod.rs` |
| R4.3 | VCS ops popup + command palette to popup chrome | `src/ui/popups.rs` |
| R4.4 | Keyboard nav audit: tab order, Esc handling, visible focus rings | global |
| R4.5 | Perf pass: no per-frame allocations in graph/diff hot paths | profiling |

Dependency order: R0 → R1 → R2 → R3 → R4. Within R3 steps are independent; recommended order puts highest-traffic surfaces first (commit → log → diff), then popups/dialogs, then welcome (state changes), settings last.

---

## 11. Verification & Acceptance

### 11.1 Automated

- `cargo check` green after every phase.
- `cargo clippy --all-targets -- -D warnings` green.
- Existing integration tests (`cargo test`) pass — especially window/dialog wiring after the shell restructure.
- New unit tests: widget color mapping, badge/ref-kind rendering decisions, palette completeness.

### 11.2 Manual verification matrix (per page in §8, vs its mockup)

- [ ] Layout structure matches (panes/widths/heights within ±2px)
- [ ] Colors match §2 hex tokens (pixel-sample check)
- [ ] Typography matches (sizes, weights, uppercase headers)
- [ ] Interactive states present: normal, hover, selected, disabled, focus
- [ ] Floating layers have backdrop dim
- [ ] Icons render at correct size/color; no blanks
- [ ] Shortcuts still work: Ctrl+K commit, Alt+` VCS popup, Ctrl+T refresh, Ctrl+Shift+K push, Ctrl+Shift+A find
- [ ] Light/HighContrast themes still function unchanged
- [ ] Window resize degrades gracefully (min pane sizes, scrollbars appear)

### 11.3 Screenshots

Per repo guidelines: before/after screenshots or GIFs attached to PRs for each visible change (R2 onward).

---

## 12. Open Questions

1. **Topbar menus** — mockups show full IDE menubar labels but define no dropdown contents. Proposal: v1 ships File/Git/View functional (File → Open/Clone/Init/Welcome; Git → fetch/pull/push/branch ops; View → toggle toolbar/status); remaining items inert chrome. Defer full menu trees.
2. **JetBrains Mono bundling** — OFL license permits redistribution (~160KB/weight). Recommend embedding Regular + Bold only; alternative is system-font lookup with graceful fallback.
3. **Light theme palette** — not defined by mockups. Keep current light theme unchanged for v1 (recommended); deriving a light palette from the same token structure is future work and does not block v1.
4. **Sidebar rail Run/Debug buttons** — no equivalent in a pure Git client. Recommend omitting Run/Debug (misleading) while keeping Project/Search inert-but-present to match mockup geometry; final call open.
5. **Dockable tool windows** — spec assumes fixed pane splits with optional resizable dividers. If `egui_dock` integration comes later, pane definitions here map onto dock nodes cleanly.
6. **Shelf/Stash tabs** — features are Phase-J scoped. Recommend hiding until implemented rather than shipping disabled tabs.

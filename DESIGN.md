# TurboGit design system (nsh)

The design system for `crates/turbogit-ui-nsh`. It has two token layers: the
**chrome tokens** in `crates/turbogit-ui-nsh/theme.json`, which
`nsh_base::theme()` reads (surfaces, ink, the primary, the accent), and the
**data tokens** in `crates/turbogit-ui-nsh/src/theme.rs` (diff color, file
status, state, repository state, lanes, the type scale, density, radii). A
call site names a token; it never names a color or a radius. The role
vocabulary is `docs/design-system-roles.md`'s; the nsh controls serve the
same roles.

## Chrome roles (`theme.json`)

Each role has a light and a dark value. The dark values carry the egui
palette's values; the light values are their honest lighter counterparts.

| Role | Dark | Light | Owns |
| --- | --- | --- | --- |
| `background` | `#1e1f22` (BG) | `#f4f5f7` | The app surface |
| `card` | `#2b2d30` (SURFACE) | `#ffffff` | Cards; RAISED wears it too |
| `popover` | `#2b2d30` (RAISED) | `#ffffff` | Menus, dialogs, popovers, tooltips |
| `foreground` | `#dfe1e5` (INK) | `#24272c` | Text and icons |
| `muted-foreground` | `#8a8e96` (INK_3) | `#5d636d` | Hints, captions, placeholders |
| `primary` | `#3574f0` (BRAND) | `#2b64d6` | The one action; light deepens for its white ink |
| `primary-foreground` | `#ffffff` (BRAND_INK) | `#ffffff` | Ink on the primary |
| `secondary` | `#313438` (SURFACE_2) | `#e8eaee` | Secondary actions |
| `muted` | `#313438` (SURFACE_2) | `#eceef1` | Hover bands, tracks |
| `accent` | `#2e4369` (SELECTION) | `#d6e2f5` | What is hovered, focused, or selected |
| `destructive` | `#ef5350` (STATE_ERROR) | `#e04b48` | Destructive actions and errors |
| `border` | `#4e5157` (LINE) | `#d0d4da` | Borders and dividers |
| `input` | `#4e5157` (LINE) | `#c4c9d0` | Input borders |
| `ring` | `#3574f0` (BRAND) | `#2b64d6` | Focus rings |
| `sidebar` | `#1f232c` (SECTION_BG) | `#eef0f3` | Section bands |
| `chart-1..5` | lane and accent hues | darker hues | Plot series |

The one accepted sub-AA step: white ink on the dark `primary` measures
4.279:1. The brand value is the one the role contract pins; the number is
asserted in `tests/tokens.rs` so a value move flags an audit. Light deepens
the primary to `#2b64d6`, whose white ink clears AA (5.389:1).

## Data roles (`src/theme.rs`)

Diff pairs (staged is the foreground pair, unstaged the muted one, each band
and ink asserted at AA in both appearances), file status ink, state color
(success, warning, error, info), the repository-state map (three severities
plus the muted ink — no chip wears a state color), ref accents
(`CASCADE_ACCENT`, `COUNTER`, `LINK`, `ACCENT_TEXT`, `AHEAD`, `DANGER`),
and the four graph lane rails.

## Type scale

Points, applied with `text_size(px(..))`. The chrome face is the system's
sans; data text may set the theme's mono family.

| Step | Size | Reads |
| --- | --- | --- |
| `TYPE_SECTION` | 9 | Section headers |
| `TYPE_CHIP` | 10 | Chips, tooltips |
| `TYPE_CONTROL` | 11 | Buttons, tabs, menu items |
| `TYPE_BODY` | 12 | Body text, inputs |
| `TYPE_GROUP_NAME` | 12.5 | Group names |
| `TYPE_DETAIL_TITLE` | 13 | Card headers, large controls |
| `TYPE_TAGLINE` | 14 | Welcome tagline |
| `TYPE_PANE_TITLE` | 16 | Pane headers, dialog titles |
| `TYPE_STATISTIC` | 22 | Welcome statistics |
| `TYPE_WORDMARK` | 42 | The welcome wordmark |

## Density

`PANEL_PADDING` 12, spacing 8x6, button padding 10x5, `INDENT` 14,
`CELL_GAP` 8; the compact (8x4), dense (6x2), and command (8x1) button
paddings; `FILE_ROW_HEIGHT` 24 and `GROUP_ROW_HEIGHT` 26; the list gap 2.

## Radii per role

Chip 3, control 4, menu 6, card and window 8, pill 9, mark 2. The chip keeps
its compact corner and the badge its pill; neither merges. `RAIL_WIDTH` 2 is
the selection rail's paint (not layout); `CARET_WIDTH` 2 is the text caret's,
kept a separate token from the rail.

## Controls and their states

Every control takes variant, size, state props, and slots; the test id
sits on the press target itself (`src/test_ids.rs` spells the vocabulary).

| Control | States | Slots |
| --- | --- | --- |
| Button | default, hover (darker fill), pressed (darkest), focused (ring), disabled (dim), selected | label, leading icon |
| Icon button | default, hover, pressed, focused, disabled, selected | icon glyph, label |
| Input | default, focused (ring), disabled, read-only, masked | placeholder, value |
| Text area | default, focused, disabled | placeholder, value |
| Checkbox | unchecked, checked, indeterminate, hover, pressed, focused, disabled | label |
| Select | default, hover, pressed, focused, disabled, open | trigger label, chevron, items |
| Combobox | default, open, multiple, disabled | placeholder, items |
| Chip | neutral, raised, accent | label |
| Tab | default, selected, hover, focused | label |
| Toolbar | item focus stops, whole-bar disabled | items, groups, separators |
| Pane header | title, count chip, actions, one hairline | title, count, actions |
| Row | default, hover, selected (accent fill + brand rail, ink does not invert) | cells |
| Card | surface, radius, panel padding, optional header | header, body |
| Data table | header muted ink, row, row hover, row selected | columns, rows |
| Tree | rows, selected accent band, hover band, expand/collapse | items |
| Menu | popup surface, item accent band, disabled item, separator | trigger, items |
| Context menu | desktop popup at the pointer | host, items |
| Dialog | backdrop press, Escape, footer rule, action slot | title, body, actions |
| Alert dialog | platform alert (NSAlert on macOS) | title, description, actions |
| Popover | trigger-anchored popup, Escape and outside press | trigger, content |
| Toast | bottom-center stack, dismiss, five-second lifetime | message |
| Tooltip | popover bubble after rest or focus | trigger, label |
| Progress | determinate share, indeterminate | value, label |
| Scrollbar | scrolling, hover, always; thumb at foreground 30 percent | target, axis |

## Motion inventory

Every state change of every control, and what animates it. Reduced motion
(the environment's `reduce_motion`) removes the motion and keeps the state
change; the nsh controls that carry their own motion honor it natively.

| Control | State change | Motion |
| --- | --- | --- |
| Button | hover, pressed | Fill steps with the pointer, no timed transition |
| Button | focus | Ring appears with the keyboard focus, no fade |
| Checkbox | checked, indeterminate | Fill and mark switch at once |
| Tabs | selected | The indicator travels natively under the selected tab (off under reduced motion) |
| Tree | expand, collapse | `reveal` is off by default; rows come and go at once |
| Select / Combobox | open, close | The popup's native entrance, per the platform; Escape closes with no fade |
| Dialog | open, close | The modal's native presentation; backdrop switches at once |
| Popover | open, close | The popup's native entrance and exit |
| Toast | push, expire | The stack's native sonner motion; reduced motion keeps the push and the dismiss without travel |
| Tooltip | show, hide | Appears after the delay; hides at once |
| Progress | determinate | The indicator's width follows the value with no spring |
| Scrollbar | show, hide | The bar's fade while scrolling, per `ScrollbarMotion` |

## What the review judges

The redesign is intentional: nsh-native values, not pixel parity with the
egui snapshots. The light values, the select trigger, and the dialog's
centered card are the visible choices this PR asks the operator to judge.

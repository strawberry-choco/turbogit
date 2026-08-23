# UI redesign audit — R4 polish pass (issue #23)

Findings and resolutions from the keyboard/focus, scroll, shortcut, and
performance audit across every redesigned surface (shell, welcome, commit +
subtabs, log + path history, diff, branches popup, merge editor, settings
modal, push dialog + safety, feedback chrome).

Verification harness: `tests/redesign_polish.rs` (headless egui_kittest over
`turbogit::ui::render`; painted output + accessibility tree + public state).
All four passes are covered there; `cargo test` green at time of writing.

## Pass 1 — Keyboard focus (spec §7.2, R4.4)

Token contract: focused inputs/buttons/selectable rows paint a 1px `BRAND`
stroke just outside the widget rect (approximating the mockups' CSS
box-shadow spread). The shared vocabulary already did this for buttons
(`widgets::button_response_sized`) and inputs (`widgets::input_frame`);
custom-drawn controls did not paint anything when focused.

Gap → fix: added `widgets::focus_ring(ui, &response)` and applied it to every
custom-drawn interactive control:

| Surface | Controls fixed |
| --- | --- |
| Shell | sidebar rail buttons, tab-strip items |
| Git Log | commit rows, changed-file rows, ROOTS filter rows |
| Diff preview | segmented control segments, Repo/Staged/Local chips, hunk nav buttons |
| Welcome | action cards, recent-project rows |
| Settings modal | category list rows |
| Shared | `tree_row` / `selectable_row` (`widgets::row_impl`) |

A guardrail test asserts exactly ONE ring is painted while any widget holds
focus, so keyboard focus can never be ambiguous.

## Pass 2 — Scroll & minimum sizes at small windows

| Finding | Resolution |
| --- | --- |
| Log window's fixed panes (210px branches / 320px files / 200px details) left the graph pane near-zero width below ~600px window width, clipping commit rows irrecoverably | Panes shrink proportionally with floors (branches ≥140px, files ≥180px); the details pane yields to short windows (`min(200, column−80)` ≥96px) so the files pane never collapses |
| Settings modal body was a fixed 420px; on short viewports the footer was clipped away entirely (widgets culled, unreachable) | Body height yields to the viewport (`min(420, view−170)` ≥240px); viewport height is sensed from the shell-level clip rect because headless harnesses do not populate `RawInput::screen_rect` |
| Welcome two-column grid could overflow narrow viewports (fixed floors summed above available width), pushing the recents column out of reach | Split is spacing-aware and both columns are width-pinned (`min_width` == `max_width`), so left + gap + recents ≤ available by construction. The pin also terminates a latent runaway-width feedback loop: `input_frame` sizes its edit from `available_width()`, and an uncapped column lets that availability feed back into itself frame-over-frame |
| Toolbar content min-width (~918px) pushed the right-aligned settings gear past the viewport edge on narrow windows — the click landed outside the screen and the modal silently never opened | Gear is pinned to the right edge (first child of a right-to-left row); only the action cluster scrolls horizontally. The 34px spec height is untouched |
| Branches popup reserved ≥240px of list height, overflowing short viewports | Floor lowered to 160px |

## Pass 3 — Shortcut audit (ADR-0009)

Result: **pass — no conflicts introduced by the redesign.**

- All five frozen combos dispatch unchanged from `shell::render`, before any
  widget runs, reading raw input without consuming events:
  Ctrl+K commit · Ctrl+Shift+K push · Ctrl+T refresh · Ctrl+Shift+A palette ·
  Alt+` VCS operations.
- Complete inventory of other keyboard consumers found no new grabs:
  - Branches popup: Esc close, ↑/↓ highlight, Enter checkout — scoped to the
    popup, disjoint from the five.
  - Command palette / dialogs / settings: plain widget text-edit keys only.
- Regression suite covers all five end-to-end, plus: shortcuts fire while a
  text field holds focus (dispatch-first contract), unmodified keys never
  trigger them, and Alt+` coexists with Ctrl+Shift+K.

## Pass 4 — Performance: heaviest redraw paths

Static analysis of the per-frame work in the two hottest surfaces. Findings
are recorded here; fixes are deliberately deferred (minimal-diff polish pass)
except where noted.

### Diff preview (`src/ui/diff.rs`)

1. **Re-parse per frame (highest cost).** `render_diff` calls `parse(t)` on
   the entire cached diff text every frame (~line 250), allocating a
   `Vec<Row>` plus one `String` per line each frame. Cost scales with diff
   size × frame rate. *Recommendation:* cache parsed rows next to
   `UiState::diff_cache`, keyed by the same `diff_key`.
2. **Whole-diff string clone per frame.** `state.ui.diff_cache.clone()`
   duplicates the full diff text every frame (~line 249). *Recommendation:*
   store `Arc<str>` in the cache and clone the handle.
3. Per-cell galley layout (`paint_cell` → `layout_no_wrap`) is mitigated by
   egui's font LRU; residual cost is a hash lookup per visible cell. Fine.
4. Good patterns already present: async engine access behind the seam,
   hunk-scroll dedup via temp memory prevents perpetual repaints.

### Log rendering (`src/ui/log_window.rs`)

1. **Full commit-list rebuild per frame (highest cost).**
   `visible_commits()` clones every visible root's commits, sorts, and
   live-filters them each frame (~lines 234–259); `commits_for()` adds up to
   two more whole-Vec clones per frame (files-pane parent lookup,
   details-pane lookup). *Recommendation:* cache the merged/sorted/filtered
   snapshot keyed by (log-cache version, root filter, path scope, filter).
2. **Lane-color DAG walk per frame.** `assign_colors()` rebuilds its
   `HashMap<String, usize>` with per-commit id clones every frame
   (~lines 113–160). *Recommendation:* memoize against the commit-list
   identity (first/last hash + length).
3. Commit rows paint 4–5 galleys per row per frame — LRU-cached by egui
   fonts; acceptable, and `ScrollArea` culls off-screen painting.
4. Good patterns already present: ref decorations / files / scoped logs are
   lazily fetched once through the executor seam and cached.

### Adjacent finding

- `push_dialog::changed_files_preview` runs synchronous
  `executor.commit_files` calls inside the frame loop for uncached
  (root, commit) pairs — first-open jank risk on busy branches.
  *Recommendation:* prefetch on dialog open, as `ensure_outgoing` already does.

### Repaint policy

`app.rs` requests repaints only when worker events drain; no continuous or
timer-driven repaint exists anywhere in the redesign. Welcome branch
indicators are TTL-cached (5s). No issues found.

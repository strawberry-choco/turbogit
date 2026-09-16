# Switching workspaces happens from the topbar picker, reachable from the palette

The topbar's workspace selector shipped as a stub that looked live and
discarded every click; the only route to another workspace was a three-step
detour (Ctrl+Shift+A → Open Welcome → a Welcome card). Issue #34 replaces the
stub with a real picker. The decisions below are the ones a future reader
would otherwise re-litigate.

## D1 — a state-driven `egui::Window`, not a response-bound `Popup::menu`

`Popup::menu(&selector)` is the local precedent (commit window's branch menu)
and needs zero state, but it is mouse-only: its open state derives from a
click on a response that only exists during the topbar's own render, so the
command palette could never open it. ADR-0011 makes the palette the
keyboard-accessibility fallback for everything the redesign relocates or
turns inert; shipping a mouse-only workspace switch would reintroduce exactly
the problem that ADR exists to prevent. Every other floating surface
(`command_palette`, `vcs_operations`, `branches_popup`) is already a
state-driven window painted after the shell in `ui::render`, so a click
recorded during the topbar's render paints the picker in the same frame.

## D2 — the palette entry is palette-only; `Action::all()` is untouched

`Action::all()` is the VCS operations popup's frozen action set, pinned by
`feedback_chrome.rs`. The palette set (`palette_actions()`) may grow:
ADR-0011 says the sets do not shrink and the palette additionally gains
shell-navigation actions (`GoToLog` / `OpenWelcome` / `StageHunk` are the
precedent `SwitchWorkspace` copies verbatim).

## D3 — no persisted state; the anchor is plain `f32` pairs

Two session-only `UiState` fields (`workspace_picker_open`,
`workspace_picker_anchor`) sit beside `vcs_popup` / `command_palette` /
`branches_popup`, and neither enters `persistence::UiStateData`. The anchor
is the selector's bottom-left captured at click time so the dropdown sits
under the chevron without hardcoding a topbar x-offset; `None` (palette
route) falls back to a fixed position. It is stored as a plain `(f32, f32)`
rather than `egui::Rect` because `turbogit-app` is egui-free by design —
adding an egui type to store two numbers would trade the crate's boundary
for nothing. The UI layer owns the `Pos2` conversion.

## D4 — dispatch is shared with the Welcome screen, not duplicated

`welcome.rs` already contained the kind dispatch (a `Workspace` recent
deep-scans, a `Project` recent bounded-scans). It is extracted into
`AppState::open_recent` and both sites call it. Two copies of "Workspace
means deep scan" is exactly the kind of thing that drifts the first time a
third `RecentKind` appears.

## D5 — the current workspace is always a row, and it is inert

`launch_in(Some(dir))` registers roots but does not record a recent, so a
workspace opened from the CLI would be missing from its own picker. The
picker synthesizes a current row from `project_dir` + `multi.roots.len()`
and marks it `current`. Clicking it is inert (close only): re-dispatching
would reset `selected_root` and drop every cache for no user-visible gain.

## D6 — a missing path is caught before dispatch

`open_project` on a deleted directory rescans to zero roots and
`show_welcome()` then returns true — the user is silently dumped on Welcome
with no explanation. `open_recent` pre-checks `is_dir()` and surfaces
`Toast::error` instead of dispatching.

## Dismissal note

Esc and click-outside are hand-rolled (a title-bar-less window gives
neither for free). The click-outside path follows egui's own dropdown idiom
(`containers/popup.rs`): a click only dismisses once the surface was visible
on the *previous* frame, so the selector click that opens the picker cannot
also close it in the same frame.

# Switching workspaces happens from the topbar picker, reachable from the palette

**Amended (2026-09-28): the topbar's trigger is gone and the sidebar's
workspace header is now the mouse trigger.** The picker, its state, and this
file's D1–D6 stand; only *which* surface opens it changed, and it is recorded
here rather than in a new ADR because nothing about the decision moved. See
the amendment note at the end.

The topbar's workspace selector shipped as a stub that looked live and
discarded every click; the only route to another workspace was a three-step
detour (Ctrl+Shift+A → Open Welcome → a Welcome card). Issue #34 replaces the
stub with a real picker. The decisions below are the ones a future reader
would otherwise re-litigate.

## D1 — a state-driven `egui::Window`, not a response-bound `Popup::menu`

`Popup::menu(&selector)` is the local precedent (commit window's branch menu)
and needs zero state, but it is mouse-only: its open state derives from a
click on a response that only exists during the trigger's own render, so
neither the command palette nor any later frame could open it. ADR-0011 makes
the palette the keyboard-accessibility fallback for everything the redesign
relocates or turns inert; shipping a mouse-only workspace switch would
reintroduce exactly the problem that ADR exists to prevent. Every other
floating surface (`command_palette`, `vcs_operations`, `branches_popup`) is
already a state-driven window painted after the shell in `ui::render`, so a
click recorded during the trigger's render paints the picker in the same
frame.

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
is the trigger's bottom-left captured at click time so the dropdown sits
under the row without hardcoding a trigger x-offset; `None` (palette route)
falls back to a fixed position. It is stored as a plain `(f32, f32)`
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
on the *previous* frame, so the trigger click that opens the picker cannot
also close it in the same frame.

## Amendment — the trigger is the sidebar's workspace header

The topbar never got its selector back; it was deleted, which left the picker
reachable only by keyboard (the palette) and no mouse affordance at all. The
sidebar's workspace header row — the folder icon, project basename, repo count
and the chevron that had always looked like a dropdown — is now the trigger:
the whole band is a `ui.interact(…, Sense::click())` with a stable id, a hand
cursor and a "Switch workspace" tooltip on hover.

D1 still decides, and the same reason decides it a second time. The reason was
never "the topbar is gone" — it was that a `Popup::menu` bound to one render's
response can be opened *only* by that click. The header's response has exactly
the same lifetime, so the same conclusion holds: the click records
`workspace_picker_open` and the window paints later in the same frame. Making
the header a `Popup::menu` would trade the palette's keyboard route for a
dropdown that can only ever be opened by the one response it is bound to. D2
(unchanged), D3 (the anchor is now the header row's bottom-left) and D4–D6
(unchanged) are untouched by this.

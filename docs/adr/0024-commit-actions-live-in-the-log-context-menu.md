# Commit actions live in the log context menu, and nowhere else

The Git Log's commit-details pane performed six actions in three places: an
ACTIONS section with `Cherry-pick to…`, `Cherry-pick across…`, `Revert commit`
and `Create branch here`; a `Copy hash` button in the pane header; and a hash
chip that also copied, captioned "click to copy full hash". Two of the three
guardrail reasons were stated in a separate `alert_box` above the buttons.
Meanwhile commit rows had no context menu at all, and the file rows beside them
used a third menu dialect — stock egui `response.context_menu` with raw
`ui.button` items, no shared frame, no disabled-with-reason.

## Decision

The right-click context menu on a commit row is the single place a commit
action lives: **eleven** items in one order, gated by one function and
dispatched by one function, in a new `ui/commit_menu.rs`. The ACTIONS section,
its group title, the header `Copy hash` button, the hash chip's click-to-copy
and the guardrail `alert_box` are deleted. The pane keeps everything it says
about a commit and loses everything it did to one.

The eleven items in four groups — `Copy hash`, `Copy commit message`,
`Create patch`, `Cherry-pick to…`, `Cherry-pick across…`; then `Checkout`; then
`Revert commit`, `Drop commit`, `Reword commit`; then `New branch`, `New tag`.
No item is `Primary`: a commit has no single dominant verb, the first slot
belongs to `Copy hash`, and brand ink on a copy action would be absurd. Only
`Drop commit` is `Danger` — it is the one item here that can lose work outright,
and severity in this kit is per-item rather than positional.

The pane's two guards were enforced by greying buttons and naming the reasons
*elsewhere*, in the alert box. The menu's convention is the opposite: a blocked
item stays rendered and states its reason on hover. The alert box is deleted
and both reasons move onto the items they block, extending one rule for "why
can't I do this" across both surfaces.

Right-click **selects**. The menu targets the row that was right-clicked *and*
writes it to the selection, so the pane and the menu never describe different
commits.

## Status

Accepted, and implemented. `.scratch/commit-actions-into-log-menu/` holds
the tickets and their per-ticket evidence. The shared menu host is ticket 01,
the eleven-item menu and its gates ticket 03, the menu replacing the details
pane's actions ticket 06, the changed-file rows joining the same host ticket
09, and the verbs it could not reach until later — Checkout at a commit 07,
Create patch 08, Drop commit 10, Reword commit 11.

## Consequences

- Eleven items is ~290px plus three rules. The app enforces a 1000×680
  minimum window (`src/main.rs`), so it fits — the same argument ADR-0023 made
  for ten, and the reason no scroll container is added.
- The menu is reachable only by a gesture with no on-screen affordance, and
  the pane that used to be the discoverable path to four of these actions no
  longer performs any. Someone who never right-clicks can no longer cherry-pick,
  revert, or create a branch from the log. This is ADR-0023's accepted
  consequence, repeated deliberately rather than by oversight.
- `DETAILS_HEIGHT` (440px) was already at its documented ceiling with a
  short-window guardrail. Removing the action grid relieves that pressure, and
  the two reasons that lived in the alert box now live in the menu.
- `DetailAction` loses `CopyHash`, `CherryPick`, `CherryPickAcross`, `Revert`
  and `NewBranchHere`, keeping `SelectParent` — the parent-hash links stay in
  the meta grid, because they are navigation inside the metadata rather than an
  action, and no menu item replaces them.
- Both menu hosts are replaced by one extracted component. The open/close
  lifecycle — anchor in egui memory, `egui::Area`, the `was_open` trick that
  stops the opening right-click from also dismissing, Esc — is currently
  inlined in `branches.rs`, and the log needs all of it again.
- Cherry-pick surviving as a menu item is what keeps `Dialog::CherryPickTarget`
  and `Dialog::CherryPickAcross` from losing their only entry point.

## Why not the alternatives

- **Keeping the action grid as well as the menu.** Rejected: two lists stay
  free to drift, which is ADR-0023's first rejected alternative, verbatim.
- **Migrating only the new menu and leaving the file rows on stock egui
  menus.** Rejected: a second menu dialect surviving one file over from the
  first, with none of the disabled-with-reason convention.
- **Right-click targeting the row without selecting it** — ADR-0023's rule,
  carried over unchanged. Rejected: it leaves the menu and the pane describing
  different commits, and the pane jumps under the cursor on the left click that
  closes the menu.
- **`Cherry-pick across…` hidden in single-root projects.** Rejected: the
  menu's convention is that an item states why it is blocked rather than
  vanishing. It stays rendered, disabled, with the reason on hover.

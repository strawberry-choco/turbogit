# Commit tool window sub-tabs render with placeholder panes

The commit tool window shows three sub-tabs: Local Changes, Shelf, Stash. Only
Local Changes has a backing feature; Shelf/Stash are Phase-J scope. We decided
to render all three as clickable tabs whose unimplemented ones show a labeled
placeholder pane ("Shelf arrives in a later phase") instead of hiding them.
Amended 2026-09-30: the premise was the shell as it stood before Branches,
Worktrees and Submodules became real tool windows of their own, and before
issue 04 merged the Unversioned Files sub-tab into the Local Changes tree.
Unimplemented scope in this window is Shelf and Stash only, and the same rule
still covers them.

This follows ADR-0016: the mockup is the visual truth; missing behavior is made
explicit on screen rather than hidden by removing controls. Rejected:
disabled-looking tabs (dishonest affordance) and omission (mockup divergence).
The existing Shelf/Stash dialogs remain reachable through the command palette
(ADR-0011) until their features land.

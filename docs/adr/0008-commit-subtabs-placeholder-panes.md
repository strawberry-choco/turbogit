# Commit tool window sub-tabs render with placeholder panes

The commit tool window shows four sub-tabs: Local Changes, Unversioned Files,
Shelf, Stash. Only Local Changes and Unversioned Files have backing features;
Shelf/Stash are Phase-J scope. We decided to render all four as clickable tabs
whose unimplemented ones show a labeled placeholder pane ("Shelf arrives in a
later phase") instead of hiding them.

This follows ADR-0016: the mockup is the visual truth; missing behavior is made
explicit on screen rather than hidden by removing controls. Rejected:
disabled-looking tabs (dishonest affordance) and omission (mockup divergence).
The existing Shelf/Stash dialogs remain reachable through the command palette
(ADR-0011) until their features land.

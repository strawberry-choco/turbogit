# Branch popup row actions render inert until their flows exist

The redesigned branches popup shows per-row actions (New Branch…, Checkout,
Rename, Delete, Compare…, New Worktree from…). The engine already backs
create/checkout/rename/delete/compare/worktree-add, but the UI has complete
flows only for New Branch and Checkout today; rename, compare, and
worktree-from-popup have engine support but no dialog or context-menu flow.
We decided to render all actions visually and wire only what has a complete UI
flow; the rest are visible but inert, with scope recorded in the spec —
consistent with ADR-0016 (mockups win; behavior gaps are made explicit rather
than hidden by removing controls).

Rejected: omitting unwired actions (diverges from approved mockup) and
half-wiring them to placeholder dialogs (worse than honest inertness).

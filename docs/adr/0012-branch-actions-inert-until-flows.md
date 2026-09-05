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

## Status: superseded for the wired flows (issue 32)

Issue 32 gave the remaining popup actions their flows, so they are no longer
inert: Delete routes through the issue-02 rich confirmation
(`PendingConfirm::DeleteLocalBranch` / `DeleteRemoteBranch`), Rename opens
`Dialog::RenameBranch`, and Compare… opens `Dialog::CompareBranches` (spec
E9's commit list with Swap). New Worktree… stays visibly inert until its flow
exists. The inertness rule itself still holds for any control without a flow
— it is the mockup-wins / explicit-gap principle (ADR-0016), not a licence to
render dead buttons forever.

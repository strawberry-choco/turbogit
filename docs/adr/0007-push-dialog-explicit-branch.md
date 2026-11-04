# Push dialog keeps explicit remote/branch fields alongside the commit tree

The redesigned push dialog renders an aggregated commit tree but does not show
remote/branch pickers. The existing dialog exposes editable Remote and Branch
fields, and protected-branch force-push blocking keys off the branch name. We
decided to keep both fields in the redesigned dialog (as compact inputs above
the options section) rather than silently deriving remote from each root's
tracking config at push time.

Reasons: (1) force-push protection checks the target branch explicitly,
deriving it invisibly would hide the thing the guard guards; (2) upstream
fallbacks differ per root in multi-root pushes and an invisible per-root
resolution makes failures undiagnosable; (3) the mockup predates this codebase's
protected-branch feature and cannot express the constraint.

**Amendment (2026-11-03, `ADR-0032`): all three reasons stand, and the mechanism
moves from one global pair to one pair per repository row.** Reason (2) was
describing a live defect rather than a risk. The batch path *was* silently
deriving each root's remote at push time — `push_roots` called `push_remote` and
`root.current_branch` per root while the dialog printed one global Remote and
Branch pair, and `settle.rs` re-derived a third time to word the toast. One
resolver now produces the `PushTarget` that the row displays and the batch
executes, which satisfies this ADR's reasons more exactly than the compact global
inputs did. `Set upstream` moves onto the row with them because it is an argument
of the `PushTargetState` the row prints, and a batch-level box would paint a
target-state chip that lies about rows it was not asked about.

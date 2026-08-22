# Push dialog keeps explicit remote/branch fields alongside the commit tree

The redesigned push dialog renders an aggregated commit tree but does not show
remote/branch pickers. The existing dialog exposes editable Remote and Branch
fields, and protected-branch force-push blocking keys off the branch name. We
decided to keep both fields in the redesigned dialog (as compact inputs above
the options section) rather than silently deriving remote from each root's
tracking config at push time.

Reasons: (1) force-push protection checks the target branch explicitly —
deriving it invisibly would hide the thing the guard guards; (2) upstream
fallbacks differ per root in multi-root pushes and an invisible per-root
resolution makes failures undiagnosable; (3) the mockup predates this codebase's
protected-branch feature and cannot express the constraint.

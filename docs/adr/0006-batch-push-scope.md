# Batch push dialog aggregates all roots; per-root scoping is a filter, not a mode

**Superseded by `ADR-0032` (2026-11-03).** Its decision, that selecting a root node filters the
preview and never narrows the push, and its rejection of per-root selective push, are both
replaced: a repository row now owns a checkbox and that checkbox is the only push filter. The
rejection's stated ground was that "the mockup gives root nodes no checkbox semantics", and the
reference product gives every repository row a checkbox and selects only the current repository by
default, so the ground is gone rather than the decision having been wrong for its time. Its
remaining worry survives and `ADR-0032` answers it: selection-driven scope was ambiguous because
the *button* named the scope. The button now says `Push` and each repository owns its own yes.

The push dialog shows a tree of "<project> (all roots)" with per-root nodes
listing outgoing commits. We decided Push always executes the batch push
(B4) across all roots with upstreams; selecting a root node only filters the
changed-files preview, it does not narrow the push. "Push current branch
only" is the sole scope-narrowing option, per the approved mockup.

Rejected: per-root selective push via node selection, the mockup gives root
nodes no checkbox semantics, and inventing selection-driven push scope would
make the dialog's primary action ambiguous. Selective push remains possible
via per-root flows outside this dialog. The dialog keeps explicit Remote and
Branch fields (see ADR-0007) because protected-branch force-push blocking keys
off the branch name; the tree aggregates outgoing commits across roots, which
the current count-only ahead/behind display cannot yet list, listing outgoing
commit SHAs is a required engine addition for R3.5.

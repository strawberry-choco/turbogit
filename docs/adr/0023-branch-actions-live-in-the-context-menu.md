# Branch actions live in the context menu, and nowhere else

The Branches surface offered the same branch actions through three affordances,
and no two of them offered the same set. `Merge`, `Rebase`, `Compare` and
`Delete` were unreachable from right-click; `Pull`, `Push`, `New branch from`
and `Checkout and pull` were unreachable from the detail panel; hovering a row
replaced its status badges with a `Checkout` button that was already menu item
zero. `Checkout` and `Rename` existed in two places each with different labels
and different selection side-effects. Three lists, no shared source of truth
for the union, and a per-row `⋯` button whose menu was a third copy of a
subset.

## Decision

The right-click context menu is the single place a branch action lives:
**ten** items in one order, gated by one function (`branch_menu_items`) and
dispatched by one function (`apply_branch_action`) in
`crates/turbogit-ui/src/ui/branch_menu.rs` and `ui/branches.rs`. Right-click is
the only trigger. The hover `Checkout` button, the per-row `⋯` overflow, and
the 280px right-hand detail panel are deleted.

The union is the ten items of the plan's order table — `Checkout`, `New branch
from`, `Checkout and pull`, then `Merge into «current»`, `Rebase onto
«current»`, `Compare with «current»`, then `Pull`, `Push`, then `Rename
branch`, `Delete branch`. Six menu items plus six panel items minus the two
they shared is ten, not the twelve the plan's prose said; the order table and
the gate table in it both list ten rows, and the count was the prose's error.

The panel enforced three of its gates by *omission* — it hid `Merge`, `Rebase`,
`Compare` and `Delete` for rows they could not apply to. The menu's own
convention is the opposite: a blocked item stays rendered and states its
reason on hover. Those four became disabled items with reasons ("this is the
current branch", "remote branches cannot be merged", "the current branch
cannot be deleted"), which extends one rule for "why can't I do this" across
the surface.

## Status

Accepted, and implemented. `.scratch/branch-actions-into-context-menu/` holds
the six tickets and their per-ticket evidence.

## Consequences

- The menu is reachable only by a gesture with no on-screen affordance. That is
  the point of the decision — one trigger, one list — but it means a person who
  never right-clicks sees no branch actions at all.
- The detail panel's informational content went with it: the relationship line
  ("2 ahead · 1 behind · tracks origin/main") and the latest-commit block are
  no longer shown anywhere in the Branches view. The relationship the line
  stated is still on the row as sync badges; the commit is not.
- `Branch.tip` is now carried by the view model and painted by no surface.
  `branches_tree.rs`'s `leaf_carries_its_tip_commit` keeps it honest as domain
  data; a future commit column is the reason to fill it.
- `DETAIL_W` and `components::detail_panel_header` are deleted: a 280px token
  and a header with no consumer name a panel that no longer exists.
- `components::overflow_button` survives with no production caller. It is a
  published kit primitive with a `section_header` action slot that can use it;
  deleting a public kit piece is a separate decision from this one.
- The menu is ~285px tall at ten items. No scroll container was added: the app
  enforces a 1000×680 minimum window (`src/main.rs`), so the menu always fits,
  and the plan's ScrollArea ticket rested on the twelve-item count.
- Widening the rows by the `⋯` column's ~32px exposed a pre-existing layout
  flaw in the inline rename editor: `widgets::text_input` claims every pixel
  the row has left, which pushed Apply and Cancel past the row's edge. The
  editor now measures the buttons first. The flaw was documented as accepted
  behavior in `branch_tree_view`'s tests before this change.

## Why not the alternatives

- **A curated short `⋯` list next to the menu.** Rejected: two lists stay free
  to drift, which is the exact failure `branch_menu.rs` was extracted to
  prevent.
- **One shared action enum painted by two components.** Rejected for the same
  reason — the duplication was never the enum, it was the second painter and
  its second copy of the gates.
- **Keeping the panel and adding its four actions to the menu.** Rejected: the
  panel would then be a read-only side of information the row already states,
  paid for with 280px of the list's width.

## ADR-0012 is not in the way

`ADR-0012`'s `Rejected: omitting unwired actions` reads as a mandate against
removing action controls, so say plainly that it is not. That ADR is scoped to
the branches *popup* (`ui/branch_widget.rs`, pinned by
`tests/branches_popup.rs`), which this change leaves untouched, and it is
already marked superseded for the wired flows. The distinction is the verb:
nothing is *omitted* here. Every branch action is reachable from the menu; it
just lives in one place now.

# Changed-file rows take a context menu, and the menu answers to the row's repository

The Changes list was the last surface with row-level verbs and no row-level menu. Its three
selection verbs were already named in `CONTEXT.md` (Discard Changes, Shelve, Create Patch), and
two of them, `Create Patch from Local Changes` and `Copy as Patch to Clipboard`, existed end to
end with the command palette as their only entry point. The list had a per-row staging checkbox,
a header `Rollback` icon, and nothing else: no `secondary_clicked` handler anywhere in
`commit_window.rs`.

## Decision

The changed-file row gets a hosted context menu, `ui/changes_menu.rs`, built the way the branch
and commit menus are: one `ChangesMenuAction` enum, the designed order as a `const` array, one
pure gate function returning `[MenuItemState; N]`, one dispatcher, and `widgets::menu_host` for
the frame. Two orders, not one, because a Merge-conflicts row is a different domain object: it
has no index state to toggle and it never joins the selection, so it answers with `Resolve…`,
`Take ours`, `Take theirs`, then the two verbs a blocked row still owes a reason for.

**Right-click selects, and keeps a set it is already inside.** ADR-0024's rule says the menu
targets the row it was opened on and writes that row to the selection. This list is the first
menu surface with a real multi-selection, and applying the rule literally would throw away the
rows the selection-scoped verbs exist to act on. So: a right-click on a row outside the set
replaces the set with that row, and a right-click on a row already in the set leaves the set
alone. Either way the row becomes the previewed change, which is the part of ADR-0024 that keeps
the pane and the menu from describing different things.

**The row's repository travels with the verb.** `PendingConfirm::Discard` now carries a `root`,
the same correction the delete confirms already record: `AppState::selected_root` is the workspace
focus, and the Changes tree paints every root, so a name-less confirm acts in whichever
repository the focus last visited. The Shelve dialog takes its root when it opens rather than
when its button is pressed, and the two patch verbs gained `_in` routes that read the selection
of the row's own root.

## Status

Accepted, and implemented.

## Consequences

- The header's `Rollback` button survives next to the menu's `Discard changes…`, which is the
  shape ADR-0023 argued against. It is not the same case. ADR-0023 deleted a *panel* whose whole
  job was offering actions; this is a header band that already owns refresh and expand-all, and
  the staging checkbox is the row's own state control rather than a second route to a verb. Both
  routes call one function, `discard_selected_changes`, so the pair cannot drift the way three
  branch lists could. If a future pass deletes the button, the menu is complete without it.
- `Discard` no longer hands an untracked path to `git checkout -- <paths>`. Before this, a
  selection holding one untracked file made the whole discard fail with a pathspec error; now
  `discard_selected_changes` filters those rows out and the confirm names only what it will undo.
- A right-click that lands on a row hidden behind an already-open menu does not retarget it,
  because the click belongs to the menu's area. That is `menu_host`'s existing behaviour and the
  branches and log menus share it.
- The shelf still parks the whole repository. `Operation::Shelve` runs a tree-wide `git stash
  push` and the selection only reaches the shelf's own record, so a shelf can list two paths
  while holding every change in that repository. `CONTEXT.md`'s claim that Shelve acts on the
  selection is true of the record and false of the bytes. This ADR does not fix that, and the row
  menu inherits the dialog's wording rather than promising more.

## Why not the alternatives

- **Nine dimmed rows on a conflicted row**, rejected. ADR-0023's disabled-with-reason rule answers
  "why can't I do this to *this* object", not "this menu has no meaning for that object". Five
  greyed items a user cannot want is noise, and the reason they cannot want them is already stated
  by `changes_selection`'s guard.
- **Adding `Delete untracked file` and `Add to .gitignore`**, rejected. Neither exists at any
  layer: `GitExecutor` has no unlink and `exclude_dir`'s own doc comment forbids reading it as
  gitignore editing. They are engine work wearing a menu item.
- **`Stage all` and `Unstage all` in the menu**, rejected. Their removal from the header is pinned
  by a test, and a menu is where a deleted decision gets quietly reinstated.
- **Reusing the selection as the menu's target with no per-row root**, rejected: it is the defect
  this ADR's third decision exists to close.

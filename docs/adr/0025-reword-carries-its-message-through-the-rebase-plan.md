# A reworded commit carries its message through the rebase plan

`RebaseAction::Reword` has been in the plan enum since the interactive-rebase
editor shipped, and `history_editor` parses and renders the `reword` verb. But
the CLI adapter runs the rebase with `GIT_EDITOR=true`, so git never opens an
editor and the message never changes: **reword was a no-op**. It stayed
invisible because nothing could reach it. ADR-0024's commit menu makes it
reachable, and it has to actually reword.

## Decision

A `RebasePlanEntry` grows an optional message. A targeted reword builds a plan
over the target commit's first parent, sets `Reword` on that entry with the new
message, and executes it through the existing
`history_editor::execute_with_backup` — the same machinery a targeted drop uses,
one call apart. In the CLI adapter the message is written to a temp file and
`GIT_EDITOR` is set to `cp <file>`, mirroring the `GIT_SEQUENCE_EDITOR=cp <todo>`
trick already used a few lines above it. One reworded commit per execution; the
existing multi-reword editor keeps today's behaviour rather than gaining a
message it has no way to key.

## Status

Accepted, and implemented. Ticket 02 of
`.scratch/commit-actions-into-log-menu/` made the plan carry the message and
the CLI adapter hand it to git; ticket 11 made the verb reachable from the
commit menu.

## Consequences

- A rendered plan containing a reword no longer round-trips through
  `parse_todo` with its message intact: git's todo format has no slot for a
  replacement message, so `render_todo` must not emit one. The editor's buffer
  is a rebase todo, not a message store.
- `plan_preview` counts a reword as surviving verbatim and cannot show the
  replacement text. The log's reword preflight shows the affected commit set
  and the cautions, not the new message — the message is confirmed in its own
  editor before the plan is built.
- The message is carried on the plan, so it is visible to every adapter, but
  only the CLI adapter supplies it to git. The other three
  (`git2_exec`, `fake`, test-support) are unchanged and their behaviour is
  whatever it already was.

## Why not the alternatives

- **A new `GitExecutor::reword_commit(root, commit, message)` plus a bespoke
  service.** Rejected: a second history-rewriting path, introduced in the same
  change that removed the second branch-action path. Reword and Drop now share
  one mechanism, and they should keep sharing it.
- **Leaving `GIT_EDITOR=true` and letting the reword open the user's
  `$EDITOR`.** Rejected: a GUI action that shells out to a terminal editor is
  worse than no action at all.
- **Landing Reword as an inert menu item this round.** Rejected: it would ship
  the new menu with a dead verb in it, and `Inert control` is for controls with
  no behaviour in v1 — not for one that is a single engine change away.

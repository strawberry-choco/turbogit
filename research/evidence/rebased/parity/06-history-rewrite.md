# Rewrite history

> Surface `history-rewrite`. Filled by the agent pass from installed-bundle
> evidence plus verified documentation. Bundle wins on names/ids/keystrokes; docs
> win on UX and flow. See `research/evidence/rebased/DISTILL.md`.

## Sources

- https://www.jetbrains.com/help/idea/edit-project-history.html — verified 200, 5355 chars → `docs-text/edit-project-history.md`
- `actions-labeled.tsv`, `keymap-bindings.txt`
- `strings/GitBundle.properties` — the `rebase.log.*`, `rebase.entry.*`, `rebase.interactive.*`, `rebase.option.*`, `rebase.dialog.*` families
- Cross-read: `docs-text/apply-changes-from-one-branch-to-another.md` (the interactive-rebase walkthrough lives there, not in `edit-project-history.md`)
- Recovered 2026-10-05: `docs-text/undo-changes.md` (found via `undo-the-last-commit` → 404 → topic search). Supplies the four-mode Reset enumeration (Soft / Mixed / Hard / **Keep**), the `Select Target Changelist` dialog behind Undo Commit, the multi-commit-revert N-commits note, and the drop-of-pushed-work recovery tip — see spec §7.1a

Docs resolve 1/1. This is the highest-density surface per byte of documentation:
`edit-project-history.md` is the shortest page in the set, and the bundle's
`rebase.log.*` family (≈45 keys) supplies nearly all of the UX — including **a
complete undo lifecycle for three separate verbs**, which no other surface
documents this precisely.

## UI — what exists

### Action inventory (bundle)

10 actions harvested from the installed bundle.

| Action id | Label | Key |
| --- | --- | --- |
| `Git.Drop.Commits` | Drop Commits | — |
| `Git.Fixup.To.Commit` | Fixup… | — |
| `Git.Interactive.Rebase` | Interactively Rebase from Here… | — |
| `Git.Reset` | Reset HEAD… | — |
| `Git.Reset.In.Log` | Reset Current Branch to Here… | — |
| `Git.Revert.In.Log` | Revert Commit | — |
| `Git.Reword.Commit` | Edit Commit Message… | F2, ⇧F6 |
| `Git.Squash.Commits` | Squash Commits… | — |
| `Git.Squash.Into.Commit` | Squash Into… | — |
| `Git.Uncommit` | Undo Commit… | — |

Two more in `actions-labeled.tsv` that belong to this surface's grammar:
`Git.InMemory.Extract.Selected.Changes` = "Extract Selected Changes to Separate
Commit…" and `Git.Drop.Selected.Changes` = "Drop Selected Changes" (the
*file-level* counterpart to `Git.Drop.Commits`, reachable from the Changed Files
pane — log-tab.md:413).

Descriptions from the bundle state the destructive classification:

- `Git.Uncommit.description` = "Undo last commit and put its changes into selected changelist"
- `Git.Interactive.Rebase.description` = "Show the dialog for interactive rebase which allows to squash, fixup, reorder, remove and reword commits"
- `Git.Reword.Commit.description` = "Change commit message via reword option of git rebase or amend"
- `Git.Squash.Commits.description` = "Rebase interactively to combine selected commits into one"
- `Git.Squash.Into.Commit.description` = "Commit local changes with the intent to squash them into the selected commit"
- `Git.Fixup.To.Commit.description` = "Create a fixup commit"
- `Git.InMemory.Extract.Selected.Changes.description` = "Create a new commit from the selected changes, placing it immediately after the original commit in history"
- `Git.Revert.In.Log.description` = "Produce a new commit(s), which reverts the changes made in the original commit(s)"

**Fixup and Squash-Into are the pair that folds *staged local work* into an
*earlier* commit; Squash-Commits is the pair that combines *selected* commits.**
Conflating them loses half the surface — and the descriptions make the distinction
explicit.

### Topology: two surfaces plus per-verb dialogs

**A. The plan editor** (`Interactively Rebase from Here…`). Dialog title
**`Rebasing Commits`** (`rebase.interactive.dialog.title`; the doc calls it "the
Rebasing Commits dialog", apply-changes:157). Regions:

1. **Plan table** with **three named columns** — `Action`
   (`rebase.interactive.dialog.git.commands.column.action`), `Hash` (`…column.hash`),
   `Subject` (`…column.subject`).
2. **Seven plan verbs**, from `rebase.entry.action.name.*` (GitBundle:212-219):

   | Verb | Bundle key |
   | --- | --- |
   | `Pick` | `rebase.entry.action.name.pick` |
   | `Edit` | `rebase.entry.action.name.edit` |
   | `Reword` | `rebase.entry.action.name.reword` |
   | `Squash` | `rebase.entry.action.name.squash` |
   | `Fixup` | `rebase.entry.action.name.fixup` |
   | `Drop` | `rebase.entry.action.name.drop` |
   | `Update Ref` | `rebase.entry.action.name.update.ref` |
   | `Unknown` | `rebase.entry.action.name.unknown` |

   **Seven verbs, not five.** TurboGit's plan editor has five chips
   (`pick`, `reword`, `squash`, `fixup`, `drop` —
   `crates/turbogit-ui/src/ui/interactive_rebase.rs`) and is **missing `Edit` and
   `Update Ref`**, even though `RebaseAction::Edit` exists in the domain enum and is
   parsed from `REBASE-TODO`. A concrete, evidence-backed gap. `Unknown` is the
   parser's fallback for an unrecognised todo verb, so the plan editor must render
   a state it did not create.

3. **Reorder controls** — "use the ↑ and ↓ buttons to move commits up and down the
   list" (apply-changes:136).
4. **`Reset` link** — "click Reset to discard all actions you've applied to the
   commits" (line 156); bundle `rebase.interactive.dialog.reset.link.text=Reset`
   (GitBundle:229). **The whole plan is undoable in one click, before running it.**
5. **`View Git Commands` toggle** — `rebase.interactive.dialog.view.git.commands.text`
   (GitBundle:234), group `Git Commands` (GitBundle:228), reword hint
   `Press {0} to apply changes` (GitBundle:230). The editor can show the **raw git
   todo it will run** — the plan is inspectable in git's own syntax.
6. **Footer** — `Start Rebasing` (`rebase.interactive.dialog.start.rebase`,
   GitBundle:231), `Cancel Rebase` (GitBundle:221).
7. **Graph preview** — "the Rebasing Commits dialog shows a **graph** illustrating
   all actions you've applied to commits in your branch so that you can review them
   before starting the rebase" (apply-changes:157-158). The plan has a *visual*
   preview, not just rows.

**B. Per-verb dialogs** — one per verb, same shape:

| Verb | Dialog | Bundle |
| --- | --- | --- |
| Reword | "enter a new commit message and click OK" (edit:20) | mini-editor title `Git Commit Message` (`rebase.interactive.edit.commit.message.dialog.title`, GitBundle:236) |
| Squash N commits | "edit the commit message (by default, it contains the messages from both commits) and click OK" (edit:59) | title `Squashed Commit Message`, label `Edit message for squashed commit` (GitBundle:286-287) |
| Drop | confirm | title `Drop {1 Commit\|2 Commits}` (GitBundle:261) |
| Initial commit | extra confirm | title `Editing Initial Commit`, body "Are you sure you want to edit the initial commit?" (GitBundle:248-249) |

**C. The rebase options dialog** (`Git | Rebase…`, title `Rebase`,
`rebase.dialog.title`, GitBundle:195). Fields, from the bundle key names — which are
unusually self-documenting:

| Bundle key | Meaning |
| --- | --- |
| `rebase.dialog.target` = "branch or hash" | the upstream to rebase onto |
| `rebase.dialog.new.base` = "new base (branch or hash)" | `--onto` new base |
| `rebase.dialog.old.base` = "old base (branch or hash)" | `--onto` cutoff |
| `rebase.dialog.branch.field` = "source branch" | the branch to rebase |

`--onto` therefore has **two ordered fields** (new base, then old base), matching
the doc: "In the new base field, enter the branch name or commit hash on which you
want to rebase… In the old base field, enter the branch name or commit hash that
marks the cutoff point" (apply-changes:177-178).

Per-field validation, which is the useful detail:

- `rebase.dialog.error.upstream.not.selected=Select an upstream`
- `rebase.dialog.error.base.not.selected=Select a new base`
- `rebase.dialog.error.branch.or.tag.not.exist=There is no such branch or tag`
- `rebase.dialog.error.branch.not.selected=Select a branch to switch to`
- `rebase.dialog.error.branch.invalid=Branch ''{0}'' does not exist`
- `rebase.dialog.error.rebase.in.progress=Interactive rebase is already in progress for this Git root`
- `rebase.dialog.root.invalid.label.text=(invalid)` — a per-root invalid marker
- `rebase.dialog.progress.loading.tags=Loading tags…`

### State matrix

| State | Which region | What it shows | Source |
| --- | --- | --- | --- |
| default | plan row | `Pick` — "this is the default state for all commits" (apply-changes:138) | apply-changes:138 |
| default | plan row | clicking `Pick` on a modified row **undoes that row**: "If you need to undo an action you've already taken on a commit, click Pick so that this commit is applied as is" (line 138) | apply-changes:138 |
| hover | plan row | `View Git Commands` shows the equivalent git todo line (GitBundle:234) | GitBundle:234 |
| pressed | `Stop to Edit` | marks the row to stop at; `rebase.interactive.dialog.stop.to.edit.text=Stop to Edit` (GitBundle:232) | GitBundle:232 |
| disabled | `Interactively Rebase from Here` | three reasons, **explained in the status bar on hover** (apply-changes:124-132): multiple parents, not in current branch, pushed to protected branch. Bundled: `Selected commit has {0} parents` (GitBundle:250), `Commit {0} is not in the current branch` (273), `The commit is already pushed to protected branch ''{0}''` (247) | apply-changes:124-132; GitBundle:247-250 |
| disabled | any rewrite verb | **blocked by repository state**, four distinct reasons: `Cannot {0} during cherry-pick` / `during merge` / `during rebase` / `during revert` (GitBundle:254-257), plus bare `Cannot {0}` (258) | GitBundle:254-258 |
| disabled | any rewrite verb | `Selected {N commits} {is\|are} from different repositories` / `from external repository` (GitBundle:251-252) | GitBundle:251-252 |
| focus | reword mini-editor | `rebase.interactive.dialog.reword.hint.text=Press {0} to apply changes` (GitBundle:230); its OK is `Continue Rebasing` (GitBundle:237) | GitBundle:230, 237 |
| focus | reword mini-editor | cancelling warns first: title `Cancel Reword`, body "Entered commit message will be lost", choices `Cancel Reword` / `Keep Editing` (GitBundle:238-241) | GitBundle:238-241 |
| selected | commit row | enables the verb set; `Commit Details` is the verification surface — "Use the information in the Commit Details area to make sure these are the changes you want" (apply-changes:242) | apply-changes:242 |
| progress | plan editor | `Searching for branches containing the selected commit…` (GitBundle:253), `Checking if it is allowed to edit selected commits…` (272), `Loading {N commit\|N commits} details…` (284) | GitBundle:253, 272, 284 |
| progress | drop | `Dropping {N commit\|N commits}…` (GitBundle:266) | GitBundle:266 |
| progress | squash | `Squashing…` (GitBundle:288) | GitBundle:288 |
| error | reword load | `Could not load changes of {N commit\|N commits}` (GitBundle:279) | GitBundle:279 |
| error | failure titles | `Could not Drop Commits` (265), `Could not Edit Commit Message` (278), `Could not Squash Commits` (285), `Cannot Create Fixup Commit` (275), `Cannot Create Squash Commit` (276), `Cannot Start Rebase` (277) — **one per verb, all distinct** | GitBundle:275-278, 285 |
| error | head lookup | `Could not find HEAD branch in repository which contains selected {N commits}` (GitBundle:271) | GitBundle:271 |

**The plural discipline is the surface's strongest signal.** Every count-bearing
string is ICU-pluralised — `{0,choice,1#commit|2#commits}`,
`{0,choice,1#is|2#are}`, `{0,choice,1#file|2#files}`. A port that writes
"Dropping 1 commit…" from one format string is visibly wrong. One string is
pluralised *twice in a single sentence* (GitBundle:252: `Selected
{0,choice,1#commit|2#commits} {0,choice,1#is|2#are} from different repositories`).

## UX — how it behaves

### How is it reached, and what hides behind a secondary affordance?

Every verb is on the Log commit row's context menu (apply-changes:18, 46, 57, 67,
122; edit:18, 46, 57, 67). `Edit Commit Message` also has `F2` / `⇧F6`
(`actions-labeled.tsv`) — the only verb here with a keystroke.

Hidden behind secondary affordances:

- **The whole plan grammar** is behind one menu item. `Interactively Rebase from
  Here…` leads to a dialog holding seven verbs, reordering, a graph preview, a
  raw-git view, and a full reset (GitBundle:212-234).
- **`Squash` vs `Fixup` share one control**: "click Squash **or the arrow next to
  the Squash button** and then Fixup" (apply-changes:147). A split control, not two
  buttons.
- **The reword message editor is a mini-editor inside the dialog**: "click Reword
  or double-click a commit and edit the text in the mini-editor that opens"
  (apply-changes:145). Double-click is the second route.
- **All of `--onto` / `--rebase-merges` / `--keep-empty` / `--root` /
  `--update-refs` / `--autosquash`** hide behind `Modify options`
  (apply-changes:76, 84, 171, 179). The default rebase path shows one field.
- **Fixup/Squash-Into require staged changes first** — the flow is *stage in the
  Commit tool window, then choose the verb in the Log* (edit:44-46). A cross-surface
  dependency, stated from both sides.
- **The disabled reason hides behind a hover**: "To identify the reason, hover over
  the action in the context menu and look for the message in the status bar"
  (apply-changes:131-132) — and even the status bar must be enabled: "right-click in
  the lower-right corner of the navigation bar to open the context menu and make
  sure that the **Status Text widget** is enabled" (line 132).

### Which verbs are destructive, and what do they confirm or preflight?

**All are destructive**, and confirmations are per-verb with distinct copy:

| Verb | Confirmation | Evidence |
| --- | --- | --- |
| Drop Commits | **Yes.** Title `Drop {1 Commit\|2 Commits}`; body "Do you want to drop {N} {commits} from the **`<branch>`** branch?"; and a **separate detached-HEAD variant**: "The repository is in the detached HEAD state. Do you want to drop {N} {commits}?" (GitBundle:261-263) | GitBundle:261-263 |
| Drop / rewrite the initial commit | **Yes, an extra confirm.** `Editing Initial Commit` / "Are you sure you want to edit the initial commit?" (GitBundle:248-249) | GitBundle:248-249 |
| Undo Commit | no confirm documented; it *is* the recovery | `Git.Uncommit` |
| Reword (Edit Commit Message) | the mini-editor is the confirmation | edit:20 |
| Reword via the plan editor | no extra confirm — the plan + graph + `Reset` is the preflight | apply-changes:157-159 |
| Squash Commits | the message dialog is the confirmation; "by default, it contains the messages from both commits" (edit:59) | edit:59 |
| Fixup / Squash Into | no dialog documented — `Commit and Rebase…` from the Commit button's arrow (edit:51) | edit:51 |
| Extract Selected Changes | "In the dialog that opens, enter the new commit message if necessary" (edit:77) | edit:77 |
| Revert Commit | no confirm documented | log-tab.md:288 |
| Reset Current Branch to Here | the Reset dialog asks for the mode: "In the Git Reset dialog that opens, select the mode in which the working tree will be updated" (log-tab.md:286) | log-tab.md:286 |
| Plan editor discard | **Yes.** "Discard changes and cancel rebase?" with `Discard` / `Continue Rebasing` / `Cancel Rebase` (GitBundle:221-224) | GitBundle:221-224 |
| Plan editor reword cancel | **Yes.** `Cancel Reword` / "Entered commit message will be lost" / `Keep Editing` (GitBundle:238-241) | GitBundle:238-241 |

Three design details worth copying by intent:

- **The drop confirmation names the branch in bold** (`from the <b>{1}</b>
  branch`), because the operation's scope is "this branch's history", not "this
  commit".
- **The detached-HEAD case gets its own sentence**, not a parenthetical. The
  confirmation text changes when the scope is ambiguous.
- **Gating is a state machine, not a boolean.** Four `Cannot {0} during
  {cherry-pick|merge|rebase|revert}` strings (GitBundle:254-257) plus a bare
  `Cannot {0}` (258) mean the app knows which operation owns the repository and
  refuses rewrites inside it.

**Preflight beyond confirmation**: the plan editor's graph preview *is* the
preflight (apply-changes:157-158), and `View Git Commands` shows the literal git
todo.

### What can be undone, from where, and for how long?

This is where the bundle is most valuable. **Three verbs have a full, explicit undo
lifecycle, each with its own success, in-progress, impossible and failed state.**

**Drop Commits — six states:**

| State | String | Key |
| --- | --- | --- |
| progress | `Dropping {N commit\|N commits}…` | GitBundle:266 |
| success | `Dropped {N} {commits}` | GitBundle:267 |
| undo in progress | `Undoing drop…` | GitBundle:270 |
| undo impossible | `Cannot undo drop` | GitBundle:269 |
| undo failed | `Undo drop failed` | GitBundle:268 |
| failure | `Could not Drop Commits` | GitBundle:265 |

**Reword — four states, including a *repository-changed* guard:**

| State | String | Key |
| --- | --- | --- |
| success | `Commit message changed` | GitBundle:280 |
| undo impossible (repo moved) | `Cannot undo commit message edit` + "**Repository has already been changed**" | GitBundle:282-283 |
| undo failed | `Undo commit message edit failed` | GitBundle:281 |
| failure | `Could not Edit Commit Message` | GitBundle:278 |

**The "Repository has already been changed" guard is the important one.** Reword
is a history rewrite, so a concurrent edit invalidates the undo. The undo is
offered *unless* the repository moved — i.e. **the undo's validity is checked at
undo time, not at reword time.** Stronger than TurboGit's current model.

**Squash:**

| State | String | Key |
| --- | --- | --- |
| progress | `Squashing…` | GitBundle:288 |
| success | `Commits Squashed` | GitBundle:289 |
| failure | `Could not Squash Commits` | GitBundle:285 |

**Coverage is asymmetric**: drop and reword are undoable by name, squash is not.
That asymmetry is itself a finding — a client should not assume "rewrite ⇒ undo".

**Hard boundaries:**

- **Unpushed** is the boundary for `Edit Commit Message` (log-tab.md:295) and for
  the reword undo.
- **Protected branch** is a hard wall, not a warning:
  `rebase.log.undo.impossible.pushed.to.protected.branch.notification.text` =
  "Commits from current branch have already been pushed to protected branch {0}"
  (GitBundle:259); and "You cannot modify the history for protected branches where
  `git push --force` is not allowed" (edit:11). A GitHub-protected branch is
  auto-detected as protected on checkout (edit:11).
- **Contributors**: "Avoid modifying the history for a remote branch with multiple
  contributors unless absolutely necessary" (edit:9) — advice, not a gate.
- **Pushing a rewritten history is rejected**: "Pushing modifications that rewrite a
  branch history to the remote repository will be rejected … so you will have to use
  force push" (edit:10).
- **Not undoable**: `Edit` (stop-to-edit) — once the rebase stops, the plan is
  history; the escape is `Abort Rebase`, which "restore[s] the original branch and
  stop[s] rebasing" (GitBundle:383/1329).
- **Reset is scoped**: "You cannot perform actions that modify a branch history for
  commits that are not contained in the branch currently checked out" (edit:12).

### Which options are on by default, and what is remembered between uses?

| Option | Default | Evidence |
| --- | --- | --- |
| Plan verb | `Pick` for every commit | apply-changes:138 |
| `--onto` | off; the basic rebase shows one target field | apply-changes:76 |
| `Select another branch to rebase` | off | `rebase.option.switch.to.branch` |
| `--rebase-merges` | off — and it carries a warning: "any resolved merge conflicts or manual amendments in these merge commits will have to be resolved or reapplied manually" (apply-changes:87) | apply-changes:87 |
| `--keep-empty` | off; label `Do not remove empty commits during rebase` (GitBundle:206) | GitBundle:206 |
| `--root` | off; label `Rebase all commits in the branch` (GitBundle:207) | GitBundle:207 |
| `--autosquash` | off; label `Auto-squash commits marked for fixup or squash` (GitBundle:208) | GitBundle:208 |
| `--update-refs` | off; **also a per-commit plan verb** `Update Ref` (GitBundle:215) | GitBundle:205, 215 |
| Squash message | **pre-filled with both messages** — "by default, it contains the messages from both commits" (edit:59) | edit:59 |
| Fixup message | **discarded** — "the commit message of the fixup commit will be discarded, so this change will be invisible in the branch history" (apply-changes:149) | apply-changes:149 |
| Ask for confirmation to drop commits | present as a setting — `settings.show.rebase.log.drop.confirmation=Ask for confirmation to drop commits` | settings tree |
| Add `cherry-picked from <hash>` suffix when picking commits pushed to protected branches | present as a setting | `settings.add.suffix` |
| Load branch protection rules from GitHub | present as a setting | `settings.synchronize.branch.protection.rules` |
| Use Safe Force Push | present as a setting | `advanced.setting.git.use.push.force.with.lease` |

Remembered between uses:

- **`rebase.autoSquash` git config is the authority, not the app**: "automatic
  processing of `fixup!` and `squash!` commits is affected by `rebase.autoSquash`
  Git config setting" (apply-changes:151). The app defers to git's own config and
  offers `--autosquash` for a one-off ("If you want to use `--autosquash` for this
  particular rebase without changing Git config", line 152). **Worth copying:
  honour git's config, don't duplicate it.**
- Plan-editor `Start Rebasing` / `Reset` state is per-dialog, not persisted.
- The `Revert` verb's scope ("Produce a new commit(s)", `Git.Revert.In.Log.description`)
  is decided at invocation.

### What feedback appears, and where does focus land?

Progress strings, one per operation (GitBundle:253, 266, 272, 284, 288):

- `Searching for branches containing the selected commit…`
- `Checking if it is allowed to edit selected commits…`
- `Loading {N commit\|N commits} details…`
- `Dropping {N commit\|N commits}…`
- `Squashing…`

Success notifications, each a verb-specific **past-tense title**:

- `Dropped {N} {commits}` (267), `Commit message changed` (280),
  `Commits Squashed` (289)

**Notification actions carry the undo.** That is the mechanism: the success
notification is where undo lives, and the two "undo impossible" strings (269, 283)
are the states where the action is withheld.

Rebase-in-progress feedback:

- "When rebase is stopped at a commit, a notification pops up in the bottom-right
  corner … letting you continue or abort the rebase" (apply-changes:141-142).
- "If you've closed the notification, from the main menu choose `Git | Continue
  rebase` to resume it" (line 143). The recovery is a **menu item, always
  present**, not a notification-only affordance.
- `rebase.interactive.edit.commit.message.ok.action.title=Continue Rebasing`
  (GitBundle:237) — the reword editor's OK *is* "continue the rebase", so the two
  dialogs are one flow.

**Optimistic paint: no.** Every verb reports after completion with a distinct title.

Focus after the action:

- `Start Rebasing` → the plan editor closes; if a row was `Stop to Edit`, focus
  lands in the stopped commit's editor (apply-changes:141).
- `Cancel Reword` → `Keep Editing` returns focus to the mini-editor (GitBundle:241).
- `Discard changes and cancel rebase?` → `Continue Rebasing` returns focus to the
  plan editor (GitBundle:222).
- After a successful drop or squash the list repaints and focus presumably moves to
  a neighbour — **UNVERIFIED**, no source states it. Re-tested against
  undo-changes/edit-project-history/apply-changes-from-one-branch-to-another
  2026-10-06: all three describe the notification and the repaint, none names a
  focus target → needs runtime capture (R3).

## Flow — how tasks chain

### Entry points

1. Log commit row context menu — the origin of every verb (apply-changes:18, 46,
   57, 67, 122; log-tab.md:264-338).
2. `F2` / `⇧F6` — `Edit Commit Message` only.
3. The success notification after a commit — `Edit Commit Message` (tutorial:67).
4. Commit tool window gutter toolbar — `Amend`, and the arrow to `Commit and
   Rebase…` (edit:51; commit-and-push-changes.md:124).
5. `Git | Rebase…` for the options dialog; `Git | Continue rebase` / `Abort rebase`
   for an in-flight rebase (apply-changes:143).
6. Changed Files pane context menu — `Drop Selected Changes`,
   `Extract Selected Changes to Separate Commit` (log-tab.md:410-415).

### Internal sequence, with dialog chaining

**Reword one unpushed commit (the shortest journey):**

```
Log → commit → Edit Commit Message…  (F2)
  → mini-editor "Git Commit Message"  [dialog]
      (cancelling → Cancel Reword warn: "Entered commit message will be lost")  [chained]
  → OK  (label: Continue Rebasing)
  → notification "Commit message changed"
        ├── Undo  (withheld if "Repository has already been changed")
        └── or Git | Rebase → Modify options → --interactive → plan editor  [chained]
```

**Interactive rebase from a commit:**

```
Log → oldest commit of the series → Interactively Rebase from Here…
  → Rebasing Commits  [dialog]
      Action | Hash | Subject table
      per-row: Pick | Edit | Reword | Squash ▾Fixup | Drop | Update Ref
      ↑ ↓ reorder
      [View Git Commands] → the literal git todo
      [graph preview of the resulting history]
      [Reset] → discards every plan change
  → Start Rebasing
      → (Stop to Edit row reached) rebase pauses → notification: Continue / Abort
      → (Reword row reached)  mini-editor opens inline → Continue Rebasing
      → (conflict)  Conflicts dialog                                    [chained]
  → (Complete or Abort)
```

(apply-chances:118-159, 194-221.)

**Fixup / Squash-Into — the cross-surface chain:**

```
Commit tool window: select the files whose changes should be appended
  → (leave them staged)
Log → target the EARLIER commit → Fixup…  or  Squash Into…
  → (squash only) "Edit message for squashed commit"  [dialog], pre-filled with both messages
  → Commit tool window → Commit ▾ → Commit and Rebase…
```

(edit:44-51.) **The only flow here that requires two surfaces in sequence**, with
work staged in one and the verb chosen in the other. The staging is the input; the
log row is the target.

**Amend a local commit (no rebase):**

```
Commit tool window: select the files to add
  → select Amend → choose the local commit to modify   (⌥M toggles the mode)
  → Amend Commit
```

(edit:28-32.) No rebase is needed because the commit is local and unpushed, so its
hash is not yet shared.

**Undo the last unpushed commit:**

```
Log → commit → Undo Commit…
  → "Undo last commit and put its changes into selected changelist"
  → the commit's files reappear in the Commit tool window
```

(tutorial:122; `Git.Uncommit.description`.)

**Extract to split a commit:**

```
Log → select the local branch, then the commit to split
  → Changed Files pane → select files → Extract Selected Changes to Separate Commit…
  → [dialog] enter the new commit message if necessary
  → "there will be two commits with new hashes"
```

(edit:73-78.)

**Push a rewritten history — the tail of every flow here:**

```
… rewrite …
  → Push → (rejected: non-fast-forward)
  → Push Rejected dialog                                            [chained]
      Rebase | Merge
  → (or) Push ▾ → Force push  (--force-with-lease)
      (unavailable on a protected branch)
```

(commit-and-push-changes.md:231-250; edit:10.)

### What happens next on success

- **Drop**: the commit and its descendants are replaced; notification with undo
  (GitBundle:267-270).
- **Reword**: the commit's hash changes; notification with a conditional undo
  (GitBundle:280-283).
- **Squash**: N commits become 1; notification (GitBundle:289). Then "Push ⌃⇧K the
  changes to the remote branch" (edit:61) — **the flow does not stop at the
  rewrite; it names the push as the next step.**
- **Interactive rebase**: the plan editor closes; if a row was `Stop to Edit`, the
  rebase pauses and waits (apply-changes:141).
- **Undo Commit**: files return to the Commit tool window (tutorial:122).

### What happens next on failure, and how the user recovers

| Failure | Evidence | Recovery |
| --- | --- | --- |
| Commit not in current branch | `Commit {0} is not in the current branch` (GitBundle:273); disabled with a status-bar reason (apply-changes:128) | pick a commit in the current branch |
| Commit is a merge | `Commit {0} has {1} parents` (GitBundle:274) / `Selected commit has {0} parents` (250) | disabled; no recovery |
| Pushed to a protected branch | `The commit is already pushed to protected branch ''{0}''` (GitBundle:247); undo notification GitBundle:259 | **unrecoverable in-app** — the doc's answer is force push, which the protection forbids (edit:11) |
| Selection spans repositories | `Selected {N commits} {is\|are} from different repositories` (GitBundle:252) | narrow the selection |
| Selection is external | `…from external repository` (GitBundle:251) | narrow the selection |
| A rebase is already running | `Interactive rebase is already in progress for this Git root` (GitBundle:190) | finish or abort the running rebase |
| During cherry-pick / merge / rebase / revert | `Cannot {0} during {…}` (GitBundle:254-257) | finish or abort that operation first |
| No HEAD branch in the repo | `Could not find HEAD branch in repository which contains selected {N commits}` (GitBundle:271) | **UNVERIFIED** — re-tested against undo-changes/edit-project-history 2026-10-06: both silent on this failure → needs runtime capture (R3) |
| Could not load commit message | `Could not load changes of {N commits}` (GitBundle:279) | **UNVERIFIED** — re-tested against undo-changes/edit-project-history 2026-10-06: silent → needs runtime capture (R3) |
| Verb failed | six distinct titles, GitBundle:275-278, 285 | **UNVERIFIED** — no bundled per-verb recovery action; re-tested against undo-changes/edit-project-history 2026-10-06: the pages give recovery only for the push-rejected tail, not per-verb failure → needs runtime capture (R3) |
| Undo no longer valid | `Repository has already been changed` (GitBundle:282); `Cannot undo drop` (269) | the undo is withheld; **UNVERIFIED** whether anything else is offered. Re-tested against undo-changes 2026-10-06: the page documents recovery for a dropped *pushed* change (force push, or update first then push the merge commit) but says nothing about what the withheld-undo notification offers → needs runtime capture (R3) |
| Push of a rewritten history rejected | edit:10 | force push, unavailable on protected branches |
| Discard plan on cancel | "Discard changes and cancel rebase?" (GitBundle:224) | `Continue Rebasing` / `Cancel Rebase` (GitBundle:221-222) |

### Escape / dismiss semantics

- The plan editor's cancel is a **three-way** choice — `Discard` / `Continue
  Rebasing` / `Cancel Rebase` (GitBundle:221-223) — separating "abandon my plan
  edits" from "leave the dialog".
- The reword mini-editor refuses a bare cancel: `Cancel Reword` warns that "Entered
  commit message will be lost" and offers `Keep Editing` (GitBundle:238-241).
- `Abort Rebase` "restore[s] the original branch and stop[s] rebasing", offered from
  a notification *and* the main menu (GitBundle:383/1329; apply-changes:143).
- `Reset` inside the plan editor is the local undo: it discards plan edits without
  leaving (apply-chances:156).

## Divergences (intentional)

| Dimension | Rebased behaviour | TurboGit behaviour | Why |
| --- | --- | --- | --- |
| UI | Seven plan verbs: Pick, Edit, Reword, Squash, Fixup, Drop, **Update Ref**, plus an `Unknown` fallback (GitBundle:212-219) | Five chips: `pick`, `reword`, `squash`, `fixup`, `drop` (`crates/turbogit-ui/src/ui/interactive_rebase.rs`) | **Open gap, not a settled divergence.** `RebaseAction::Edit` already exists in the domain enum and is parsed from `REBASE-TODO` — it simply has no chip and no shortcut. `Update Ref` maps to the existing `--update-refs` dialog flag, so surfacing it per-commit is a rendering change. See ticket 08. |
| UX | `Pick` on a modified row **resets that row** (apply-chances:138) | Row verbs are chips; reordering has `⇧↑`/`⇧↓`; no per-row reset binding (`interactive_rebase.rs:281-295`) | Small, cheap, and it removes a foot-gun: a user who has half-edited a plan currently has no one-click per-row way back, only the global `Reset`. **Open gap.** |
| UX | `Update Ref` available as a **per-commit plan verb** (GitBundle:215) | `--update-refs` is a dialog-level checkbox only | Per-commit is strictly more expressive, and the plan is already the right place for it. **Open gap.** |
| UI | Plan editor shows a **graph** of the resulting history plus a `View Git Commands` raw-todo view (apply-chances:157; GitBundle:234) | `RebaseEditorTab::{Plan, Preview, Log}` — Preview and Log already provide both (`state.rs:358-368`) | No divergence. Recorded because it confirms TurboGit's three-tab shape is the right match. |
| UX | Drop/reword/squash each get a success notification with an **undo action**, and each undo has named impossible/failed states (GitBundle:265-289) | `Operation::DropCommit` / `RewordCommit` with `RewritePreflight` before, and a backup ref (`refs/turbogit/preflight-backup`); **no post-hoc undo notification** | TurboGit's model is *pre*-flight (disclose before) where Rebased's is *post*-flight (undo after). Both defensible; Rebased's covers the case TurboGit's cannot — a mistake discovered after the fact. The mechanism already exists. **Open gap**; see ticket 09. |
| UX | Reword's undo is **invalidated if the repository changed** ("Repository has already been changed", GitBundle:282) | No such check | Reword is a history rewrite, so a concurrent edit genuinely invalidates the undo. Validating at undo time beats assuming. **Open gap**, cheap to add. |
| UX | `--onto` is two ordered fields — new base, then old base (GitBundle:196-197; apply-changes:177-178) | `RebaseKind::Standard { onto }` — a single target, no `--onto` (`operation.rs`) | `--onto` is the "move only this sequence of commits" tool, genuinely useful for stack surgery. **Open gap**; the field pair and both validation strings are specified. |
| UX | A rebase already in flight blocks another: `Interactive rebase is already in progress for this Git root` (GitBundle:190) | Not established | **Open gap** — and the *per-root* qualifier is the interesting part, since TurboGit is multi-root. |
| UX | State-machine gating: `Cannot {0} during cherry-pick / merge / rebase / revert` (GitBundle:254-257) | Protected-branch guard exists in `Operation::Rebase`; no in-flight-operation state machine | **Open gap.** The four-reason enumeration is fully specified and testable. |
| UX | Squash is not undoable while drop and reword are (asymmetric coverage in the bundle) | Uniform preflight for both Drop and Reword (`Dialog::RewritePreflight` takes a `HistoryVerb`) | TurboGit's uniformity is *better* here. Deliberate: `HistoryVerb` parameterises one preflight rather than forking per verb (`state.rs:513-528`). Uniformity costs nothing the user can observe. |
| UI | Every count-bearing string is ICU-pluralised, one twice in a sentence (GitBundle:252) | Rust `format!` with hand-written conditionals | Pluralisation must be one shared helper, not per-call-site conditionals, or the same sentence drifts. Recorded as an implementation constraint. |
| UX | Notices that its own rewrites will be rejected on push and names force push as the consequence (edit:10) | Protected-branch guard blocks the rewrite up front | TurboGit refuses earlier, so it never has to explain a later rejection. Strictly better; the doc's warning becomes unnecessary. Recorded so the difference is understood, not accidental. |

## Needs runtime capture

1. **The plan editor's visual layout** — column widths, whether the graph sits
   above or beside the table, and whether `View Git Commands` replaces the table
   or opens a second view (GitBundle:228, 234).
2. **Default state of `Ask for confirmation to drop commits`** — the setting exists
   (`settings.show.rebase.log.drop.confirmation`) but the drop confirmation
   (GitBundle:261) is unconditional in the bundle, so whether the setting suppresses
   it is unclear.
3. **Focus target after a successful drop or squash** — which row takes focus when
   the selected commit ceases to exist.
4. **Whether `Update Ref` appears per-row always, or only when `--update-refs` is
   enabled** — the two are separate strings and their interaction is undocumented.
5. **The `Stop to Edit` pause UX** — what the stopped state looks like, and which
   actions the paused commit accepts ("You can modify this commit using the context
   actions (such as Revert, Undo, Amend, and so on) before continuing",
   apply-changes:142).
6. **Whether the reword mini-editor's OK reads `Continue Rebasing` in the
   `Edit Commit Message…` path too**, or only inside the interactive dialog — the
   key is `rebase.interactive.edit.commit.message.ok.action.title`, so its scope is
   implied but not stated.

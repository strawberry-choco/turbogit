# Conflict resolution

> Surface `conflicts`. Filled by the agent pass from installed-bundle evidence
> plus verified documentation. Bundle wins on names/ids/keystrokes; docs win on
> UX and flow. See `research/evidence/rebased/DISTILL.md`.

## Sources

- https://www.jetbrains.com/help/idea/resolve-conflicts.html — verified 200, 8274 chars → `docs-text/resolve-conflicts.md`
- https://www.jetbrains.com/help/idea/working-with-git-tutorial.html — verified 200, 15473 chars → `docs-text/working-with-git-tutorial.md`
- `actions-labeled.tsv` — action ids, labels, default keys
- `strings/VcsBundle.properties` — the `multiple.file.iterative.merge.*` family (the Conflicts dialog chrome)
- `strings/GitBundle.properties` — the `conflicts.*` family, the `smart.operation.*` family, `apply.changes.*`

Docs resolve 2/2 for this surface, so the flow half is fully evidenced.

## UI — what exists

### Action inventory (bundle)

7 actions harvested from the installed bundle.

| Action id | Label | Key |
| --- | --- | --- |
| `Diff.ApplyLeftSide` | — | ⌘⌥R |
| `Diff.ApplyNonConflicts` | — | — |
| `Diff.ApplyNonConflicts.Left` | — | — |
| `Diff.ApplyNonConflicts.Right` | — | — |
| `Diff.ApplyRightSide` | — | ⌘⌥A |
| `Git.ResolveConflicts` | Resolve Conflicts… | — |
| `Git.RevertResolved` | Revert Resolved | — |

Three further ids exist in the bundle that the surface prefix did not catch and
that belong here — **Accept Yours / Accept Theirs are two distinct actions, not
one toggle** (`actions-labeled.tsv`):

| Action id | Label | Key |
| --- | --- | --- |
| `Git.ChangesView.AcceptYours` | (no text in the action table) | — |
| `Git.ChangesView.AcceptTheirs` | (no text in the action table) | — |

The bundle *does* carry their labels, in
`strings/GitBundle.properties:1521-1522` — `conflicts.accept.yours.action.text=Accept Yours`
and `conflicts.accept.theirs.action.text=Accept Theirs`. So the labels are
established; only their action-table text column is empty. Both are also
reachable from the `Git.Stage.*` mirror set (`Git.Stage.AcceptYours`,
`Git.Stage.AcceptTheirs`), i.e. once per staging mode.

### Topology

Two surfaces, not one. This is the single most important structural fact.

**A. The Conflicts dialog** — a *file list*, one row per conflicted file, with
the per-file verb trio. This is a list you clear, not a place you work.

Regions, top to bottom:

1. **File list** — one row per conflicted path. Each row carries a status word
   for the kind of clash, from `strings/GitBundle.properties:1509-1515`:
   `both deleted`, `both added`, `both modified`, `deleted by you`,
   `deleted by them`, `added by you`, `added by them`. Seven named kinds, so
   "Modified line / Deleted lines / Newly added lines / Conflicting lines"
   (docs, `resolve-conflicts.md` lines 28-34) is the *editor's* grammar, not the
   list's — do not conflate them.
2. **Per-row actions** — three, per `resolve-conflicts.md` line 11-17:
   - `Accept Yours` — "keep the changes from the currently checked-out branch and discard the changes from the branch you are merging"
   - `Accept Theirs` — "accept the changes from the branch you are merging and discard the changes from the currently checked-out branch"
   - manual resolution — "review each conflict and choose which changes to keep"
3. **Toolbar** — the bulk-apply cluster, `actions-labeled.tsv`:
   - `Diff.ApplyNonConflicts` — apply all
   - `Diff.ApplyNonConflicts.Left` / `.Right` — one side only
   - the "Resolve All Simple Conflicts" control, `strings/VcsBundle.properties:590`
4. **Footer** — progress counter `multiple.file.iterative.merge.files.resolved.changes.count={0}/{1}`
   (VcsBundle:573) and the finish control `Accept and Finish`
   (VcsBundle:594), disabled until the list empties.
5. **Top bar** — `Resolve Conflicts` and `Abort Merge`
   (`resolve-conflicts.md` line 19).

**B. The three-pane merge tool** — double-clicking a conflicted file
(`resolve-conflicts.md` line 20). Pane topology, per lines 22-26:

- **Left** — "the read-only local copy"
- **Centre** — "a fully-functional editor where the results of resolving conflicts are displayed. Initially, the contents of this pane are the same as the base revision of the file"
- **Right** — "the read-only version checked in to the repository"

Three panes, and the centre *starts at base*, not at either side. That is the
detail most likely to be got wrong: the result pane is seeded from the common
ancestor, so an unedited resolve is not "yours" or "theirs".

The tutorial names the same three panes for a merge, with the branches
substituted: "The left pane called Changes from main shows the read-only changes
from the local copy", "The right pane called Changes from new_feature shows the
read-only incoming changes", "The central pane called Result is a
fully-functional editor" (`working-with-git-tutorial.md` lines 163-167). So the
pane *titles* are per-operation, and the roles are fixed.

Per-conflict controls inside the centre pane:

- gutter checkboxes / chevrons to accept from a side (`resolve-conflicts.md`
  line 44)
- right-click context menu with `Resolve using Left` and `Resolve using Right`
  (line 45), which "provide a shortcut to accepting changes from one side and
  ignoring them from the other side"
- a gutter toolbar that appears **on hover over a change marker**, drawn
  together with "a frame showing the previous contents of the modified line"
  (line 61), carrying `Revert` for that hunk (line 62)
- `Resolve simple conflicts` for one-click merges, enabled only when a simple
  conflict exists (line 46)
- the version-list button (line 50), which is also where
  `Diff.ApplyLeftSide` / `Diff.ApplyRightSide` live
- `Diff.FocusOppositePane` at `⌃⇧⇥` (`actions-labeled.tsv`), "Switch between the
  panes of the Diff Viewer. The active pane has the caret"
  (`differences-viewer.md` line 107)

### State matrix

| State | Which region | What it shows | Source |
| --- | --- | --- | --- |
| default | file list | every conflicted file listed, none resolved; counter at `0/N` | VcsBundle:573 |
| default | merge tool centre | base revision content | resolve-conflicts.md:26 |
| default | finish control | `Accept and Finish` **disabled** — "If there are no remaining conflicts, the Conflicts dialog enables the Accept and Finish option" | resolve-conflicts.md:56 |
| hover | gutter marker | the marker toolbar + previous-contents frame appear | resolve-conflicts.md:61 |
| hover | branch name | star outline appears on the left, to favourite | manage-branches.md:52 |
| pressed | accept chevron | `Ctrl` changes its behaviour to "append changes" rather than replace | differences-viewer.md:116 |
| disabled | `Resolve All Simple Conflicts` | "There are no simple conflicts to resolve" (the tooltip *is* the reason) | VcsBundle:588 |
| disabled | `Apply Non-Conflicting Changes` (auto) | "Such conflicts are not resolved with the Apply non-conflicting changes actions since you have to make sure that they are resolved properly" — so a simple conflict is *exempt* from the bulk action | resolve-conflicts.md:47, differences-viewer.md:136 |
| focus | centre pane | caret-bearing pane; `⌃⇧⇥` moves between panes | differences-viewer.md:107 |
| selected | file row | drives the centre pane and the right-hand diff preview | log-tab.md:18 (same shape) |
| progress | whole dialog | `Resolving {0,choice,1#Conflict|2#Conflicts}` — count-pluralised, so 1 vs N reads differently | GitBundle:1518 |
| progress | loading | `Loading merge conflicts…` | GitBundle:1519 |
| error | resolution | `Cannot Resolve Conflict` / `Cannot find file for {0}` | GitBundle:1516-1517 |

### Naming: the bundle contradicts the docs on two buttons

The docs name the merge tool's commit buttons `Apply Changes` and
`Save and Close` (`resolve-conflicts.md` lines 52-53). **Neither string exists in
the shipped bundles.** What the bundle actually carries is:

- `multiple.file.iterative.merge.review.changes=Review Changes` (VcsBundle:575)
- `multiple.file.iterative.merge.accept.finish=Accept and Finish` (VcsBundle:594)
- a *close* confirmation, not a close button: title `Discard Changes?`
  (VcsBundle:583), body "Closing the dialog will discard your changes in
  partially resolved files" (VcsBundle:584), with the two choices
  `Discard Changes` / **`Continue Merge`** (VcsBundle:585-586)

So the installed build resolves a file with **Review Changes** (→ next file) and
**closes** via a confirm that offers to continue merging. The doc's
`Save and Close` is a paraphrase of the *close* path, not a button label. Per the
source-precedence rule, take the bundle: use `Review Changes` and
`Accept and Finish` as button names, and treat "save and close" as the
dismiss-with-loss path.

## UX — how it behaves

### How is it reached, and what hides behind a secondary affordance?

Automatic first, manual second.

- Automatic: "The Conflicts dialog is triggered automatically when a conflict is
  detected on the Git level" (`resolve-conflicts.md` line 10). The triggering
  operations are enumerated at line 9: "pull, merge, rebase, cherry-pick,
  unstash changes or apply a patch". **Unstash and apply-patch are in that list**,
  which is the sharpest edge here — a conflict can arrive from a *recovery*
  action, not only from an integration one.
- Resumable: "If you close the Conflicts dialog or run a Git operation from the
  command line that results in merge conflicts, a Merge Conflicts node appears in
  the Changes view of the Commit tool window with a link to resolve them"
  (line 18). The node label is in the bundle: `Merge Conflicts` (GitBundle, via
  `conflicts` family). The link text is `Resolve Conflicts…`
  (`actions-labeled.tsv`, `Git.ResolveConflicts`).
- If conflicts remain, the operation is *not* silently dropped. Bundled wording:
  "You need to resolve all merge conflicts before {0}" (GitBundle:1427, 1433).

Behind secondary affordances:

- **The bulk-apply cluster is on a toolbar, not on the file rows.** Three of the
  seven harvested actions (`Diff.ApplyNonConflicts`, `.Left`, `.Right`) are
  toolbar-only.
- **`Resolve All Simple Conflicts` is a separate control from
  `Apply Non-Conflicting Changes`**, and this is not a distinction without a
  difference: the bulk-apply *deliberately skips* simple conflicts
  (`resolve-conflicts.md` line 47), while the Resolve-All control handles them
  (VcsBundle:589, "Apply all non-conflicting changes and resolve simple conflicts
  (ones that can be resolved through static analysis) across all files"). Two
  buttons, different scopes, and a user who reaches for the wrong one gets a
  partial result.
- **Undoing a resolution is per-file and in the list's context menu**:
  "right-click the file in the Conflicts dialog and select Revert conflict
  resolution" (line 54); bundle label `Revert Conflict Resolution`
  (VcsBundle:576) and action `Git.RevertResolved` = "Revert Resolved"
  (`actions-labeled.tsv`).

### Which verbs are destructive, and what do they confirm or preflight?

Every verb here is destructive, and the build confirms three of them.

| Verb | Destructive? | Confirmation | Evidence |
| --- | --- | --- | --- |
| Accept Yours / Theirs | yes — discards one side | **Yes, if already merged.** Title `Overwrite Changes`; "This file already contains merged changes. Accepting changes from '{1}' will discard and overwrite them."; the affirmative is `Discard and Accept` (VcsBundle:580-582) | VcsBundle:580-582 |
| `Resolve All Simple Conflicts` | yes, in bulk | **No dialog.** It is one click and reports afterwards | resolve-conflicts.md:38; VcsBundle:590 |
| Apply non-conflicting changes | yes | No dialog, no preflight | differences-viewer.md:126 |
| Revert conflict resolution | yes — discards the merge result | **Yes.** Title `Confirm Revert`; "All changes will be reverted. The file will return to its original conflicted state." (VcsBundle:578-579) | resolve-conflicts.md:54; VcsBundle:578-579 |
| Closing with partial work | yes — discards edits | **Yes.** Title `Discard Changes?`; "Closing the dialog will discard your changes in partially resolved files"; the safe choice is `Continue Merge` (VcsBundle:583-586) | VcsBundle:583-586 |
| `Abort Merge` | yes — ends the merge | no dialog evidence found | resolve-conflicts.md:19 |

Two design details worth copying by intent:

- The destructive confirmation's **affirmative button is the dangerous sentence**
  (`Discard and Accept`), and its **negative button is the safe one**
  (`Continue Merge`). Naming, not ordering, carries the safety.
- `Discard Changes?` fires on *close*, so the loss is prevented by default: the
  dialog's default action is to stay and finish.

### What can be undone, from where, and for how long?

- **Revert Conflict Resolution** — per file, from the Conflicts list context
  menu, restores the *conflicted* state (not the pre-merge state). Available for
  as long as the file is still in the list, i.e. until `Accept and Finish`.
- **Accept and Finish** is the point of no return. Nothing in either source
  offers an undo past it; the merge commit is the record. **UNVERIFIED** whether
  an undo affordance exists in the notification tray after finishing — neither
  `resolve-conflicts.md` nor the bundle strings mention one. Re-tested
  2026-10-06 against resolve-conflicts/undo-changes/working-with-git-tutorial:
  `undo-changes` documents `Revert Commit` as the post-hoc recovery for a
  completed (pushed) merge commit, but no notification-tray undo is stated
  anywhere → needs runtime capture (R3).
- **The `edit` state is its own undo boundary**: "If you need to revert the merge
  result, right-click the file in the Conflicts dialog and select Revert conflict
  resolution" is the only documented reversal, and it is scoped to a file.
- A conflicted operation can be abandoned wholesale via `Abort Merge`
  (`resolve-conflicts.md` line 19) — that is the escape hatch, not an undo.

### Which options are on by default, and what is remembered between uses?

Defaults, from the bundle's settings tree:

| Option | Default | Evidence |
| --- | --- | --- |
| Automatically apply non-conflicting changes | **off** — you are told to it from the dialog | resolve-conflicts.md:60 |
| Warn if CRLF line separators are about to be committed | present as an option; enable state UNVERIFIED (re-tested 2026-10-06 against resolve-conflicts.md:69 — "Enable the … option", instructive only — and settings-version-control-git — "Select this option to enable…", no default stated → R3) | resolve-conflicts.md:69 |
| Whitespace handling in the diff viewer | `None` — "white spaces are important, and all the differences are highlighted. This option is selected by default." | differences-viewer.md:67, shelving-and-unshelving-changes.md:110 |
| Shelve base revisions of files under DVCS | present; **on** by implication — "If it is disabled, IntelliJ IDEA will look for the base revision in the project history, which may take a while" | shelving-and-unshelving-changes.md:226-227 |

Remembered between uses:

- `Automatically apply non-conflicting changes` is a persisted setting on
  "Tools | Diff Merge settings page" (resolve-conflicts.md:60) — once ticked,
  the dialog stops asking. This is the one place the conflict surface carries
  state forward.
- The `core.autocrlf` value suggested by **Fix and Commit** is written to git
  config, so it outlives the app (resolve-conflicts.md:75).
- **Not remembered:** pane layout, last-opened side, the simple-conflict
  auto-resolve state. Each session re-derives them.

### What feedback appears, and where does focus land?

Progress:

- `Loading merge conflicts…` on entry (GitBundle:1519)
- `Resolving {0,choice,1#Conflict|2#Conflicts}` during a bulk accept
  (GitBundle:1518) — note the ICU plural, so the singular and plural are
  different strings and a naive port that hardcodes "conflicts" is wrong.
- a `0/N` → `N/N` counter in the dialog footer (VcsBundle:573)

Result reporting, all bundled:

- `All conflicts have been resolved` (VcsBundle:595)
- `All conflicts were resolved automatically` (VcsBundle:592)
- `No conflicts were resolved automatically` (VcsBundle:591) — a *negative* result
  is reported, not left silent
- partial: `{0} conflicts resolved. {1} conflicts in {2} files still require attention` (VcsBundle:593)
- `Cannot Resolve Conflict` (GitBundle:1516)

Notification-level, for the *operation* rather than the dialog:

- `There are unresolved conflicts in the working tree.` with two actions —
  `Resolve…` and `Abort {0}` (GitBundle:25, 26-27)
- `{0} was performed with conflicts` (GitBundle:32)
- `Merge conflicts detected. Resolve them before continuing rebase.` /
  `… before continuing update.` — two operation-specific variants
  (GitBundle:385, 392)
- `File has unresolved merge conflicts` with `Resolve conflicts…`
  (GitBundle:1561-1562)
- `Resolving merge conflicts is in progress` with `Show resolve conflicts window`
  and `Cancel resolve` (GitBundle:1558-1560) — a background resolve stays
  cancellable and re-focusable from a notification.
- `Cannot check the working tree for unmerged files because of an error. {0}`
  (GitBundle:1517 area / :61) — the scan itself can fail, and it says so.

**Optimistic paint: no.** Every bundled string is a completion or a count, and
the list is a list of files that are still unresolved. TurboGit should not
optimistically clear a row.

Focus after the action:

- `Review Changes` closes the file and opens the next conflicted file
  (resolve-conflicts.md:52) — focus advances **into the next file's merge tool**,
  not back to the list.
- Closing returns "to the list of remaining conflicts" (line 53) — focus lands on
  the list.
- `Accept and Finish` completes the merge; no documented focus target. **The
  bundle is silent here — UNVERIFIED.** Re-tested 2026-10-06 against
  resolve-conflicts/working-with-git-tutorial/undo-changes: all silent on where
  focus lands after the merge completes → needs runtime capture (R3).

## Flow — how tasks chain

### Entry points

1. Automatic, on conflict detection, for any of: pull, merge, rebase,
   cherry-pick, unstash, apply patch (resolve-conflicts.md:9-10).
2. `Merge Conflicts` node in the Commit tool window → `Resolve Conflicts…`
   (resolve-conflicts.md:18; `Git.ResolveConflicts`).
3. Notification action `Resolve…` (GitBundle:26).
4. Notification action `Show resolve conflicts window` when a resolve is already
   running (GitBundle:1559).
5. `Abort Merge` from the toolbar, to leave instead (resolve-conflicts.md:19).

### Internal sequence, with dialog chaining

The canonical journey — a conflicted merge:

```
merge (or rebase / cherry-pick / pull / unstash / apply patch)
  → Conflicts dialog (file list)                        [auto]
  → Resolve Manually → 3-pane merge tool (one file)     [dialog]
  → Review Changes → next conflicted file's merge tool  [loops]
  → (list empty) → Accept and Finish → merge commit
```

Bulk-first variant:

```
merge
  → Conflicts dialog (file list)
  → Resolve All Simple Conflicts          → status: All conflicts were resolved automatically
  → (remaining rows) per-file Accept Yours / Theirs | Resolve Manually
  → Accept and Finish
```

Abandon variant:

```
merge → conflict
  → Abort Merge  → back to pre-merge state
```

Recovery variant — the one that makes this a *flow* surface rather than a dialog:

```
operation fails / dialog closed mid-way
  → Merge Conflicts node in Commit tool window ("with a link to resolve them")
  → Resolve Conflicts…  → Conflicts dialog  → resume exactly as above
```

### What happens next on success

- All rows resolved → `Accept and Finish` becomes enabled → the merge completes
  and the merge commit appears in the Log (`working-with-git-tutorial.md` line
  180: "IntelliJ IDEA will merge the new_feature branch to the main branch";
  `apply-changes-from-one-branch-to-another.md` line 56 for the clean case).
- Notifications resolve in stages: `All conflicts have been resolved` in-dialog,
  then a commit-level notification for the operation.
- After a conflicted merge, a new post-hoc affordance exists for *review*:
  "If, at a later time, you need to review how exactly conflicts were resolved
  during a merge, you can locate the required merge commit in the Log tab …
  select a file with conflicts in the Commit Details pane … and press `⌘D`"
  (resolve-conflicts.md:76). This is the flow's tail — the merge is auditable
  afterwards via a three-panel diff (`investigate-changes.md:153`).

### What happens next on failure, and how the user recovers

| Failure | Evidence | Recovery |
| --- | --- | --- |
| Operation performed but conflicts remain | `{0} was performed with conflicts` (GitBundle:32) | `Merge Conflicts` node stays; `Resolve…` in the notification |
| Scan for unmerged files fails | `Cannot check the working tree for unmerged files because of an error. {0}` (GitBundle:61) | no documented recovery; **UNVERIFIED** — re-tested 2026-10-06 against resolve-conflicts/undo-changes: silent → R3 |
| File missing during resolve | `Cannot find file for {0}` (GitBundle:1517) | no documented recovery; **UNVERIFIED** — re-tested 2026-10-06 against resolve-conflicts/undo-changes: silent → R3 |
| Rebase stopped by conflicts | `Merge conflicts detected. Resolve them before continuing rebase.` (GitBundle:385) | resolve, then Continue Rebase — the analogous rebase-specific message is at `apply-changes-from-one-branch-to-another.md:99` |
| Bulk resolve partially succeeds | `{0} conflicts resolved. {1} conflicts in {2} files still require attention` (VcsBundle:593) | the remaining files are named by count; work through the list |
| Some repositories succeeded | `However, checkout has succeeded for the following {0,choice,1#repository|2#repositories}` (GitBundle:1347) | multi-root partial state; the same `However, … succeeded for` pattern appears for branch deletion (GitBundle:1357) |

The multi-root pattern is worth naming as a family: this build reports partial
success **per repository** with a count-pluralised sentence, in at least checkout
(GitBundle:1347), branch deletion (GitBundle:1357), and apply-changes
(GitBundle:33).

### Escape / dismiss semantics

- Closing the dialog with partial work is **not silent**: `Discard Changes?` →
  `Discard Changes` / `Continue Merge` (VcsBundle:583-586). The safe option is to
  stay.
- `Revert conflict resolution` re-arms a file rather than closing the dialog
  (resolve-conflicts.md:54).
- `Abort Merge` is the exit that discards the whole operation
  (resolve-conflicts.md:19; `apply-changes-from-one-branch-to-another.md:59` —
  "You can cancel an unfinished merge operation by selecting the Abort action
  from the VCS widget").
- A background resolve is cancellable from its notification
  (GitBundle:1560, `Cancel resolve`).

## Divergences (intentional)

| Dimension | Rebased behaviour | TurboGit behaviour | Why |
| --- | --- | --- | --- |
| UI | Three equal panes, centre seeded from the **base** revision (resolve-conflicts.md:26) | Three equal panes `Local \| Result \| Incoming`; the Result pane is a composition the user edits, not a base copy (`crates/turbogit-ui/src/ui/conflict_resolver.rs`) | The result pane must be writable and must show *something* resolved; seeding from base and asking the user to type the resolution is a worse default than presenting ours/theirs/both as explicit per-conflict choices. Same topology, better default. |
| UX | Exactly two resolution choices per file: Accept Yours / Accept Theirs (resolve-conflicts.md:13-15) | Four: `Take ours` / `Take theirs` / `Take both` (ours-first) / `Take both` (theirs-first) | "Take both" is the common real case for non-overlapping edits that git still marked conflicted, and the *ordering* variant matters for import lists and struct literals. Same item count as the user's mental model, strictly more expressive. |
| UX | Two separate bulk buttons with deliberately different scopes: `Apply Non-Conflicting Changes` (skips simple conflicts) vs `Resolve All Simple Conflicts` (handles them) (resolve-conflicts.md:47; VcsBundle:588-590) | One `Apply to all N remaining` control | Two adjacent bulk buttons that differ only in whether they touch one conflict class is a mis-click generator. TurboGit folds the simple-conflict class into the single bulk action and says so. |
| UX | `Accept Yours`/`Theirs` confirm with a dialog titled `Overwrite Changes` when the file was already merged (VcsBundle:580-582) | No such pre-check | TurboGit's resolver tracks per-conflict resolution state already, so the "already merged" case is representable; **this is a gap to close, not a settled divergence** — see ticket 01. Recorded here so the divergence table is honest about what is still open. |
| Flow | A conflicted operation is resumable from a `Merge Conflicts` node in the Commit tool window after the dialog is closed (resolve-conflicts.md:18) | Conflicts appear as a `Merge conflicts` group inside the Commit window changes list (`crates/turbogit-ui/src/ui/commit_window.rs`) | Structurally the same promise — a persistent, clickable reminder in the commit surface — with TurboGit's existing bucket instead of a new node type. No new affordance needed. |
| UI | Toolbar-button glyphs for accept/ignore, drawn as chevrons whose behaviour changes under a modifier (differences-viewer.md:116) | TurboGit's own icon set + `painted_paths` assertions | Icon parity is explicitly out of scope per `DISTILL.md`. |
| UI | The Conflicts dialog is a *window* with its own file list, a `0/N` counter, and its own footer buttons (VcsBundle:572-595) | A dedicated conflict-resolver tool window plus an inline per-file editor (`conflict_resolver.rs`, `conflicts.rs`) | egui has no native sub-window with a modal footer; our chrome is a full-bleed surface with the same regions. Topology (list → per-file three-pane → finish) is preserved. |
| UX | Line-ending conflict handling: a pre-commit warning plus a `Fix and Commit` action that writes `core.autocrlf` (resolve-conflicts.md:68-75) | Not implemented | Deliberately deferred: it mutates global git config as a side effect of a commit, and TurboGit's stance is to show the discrepancy in the diff and let the user decide. Recorded as **intentionally absent** in the gap list, not forgotten. |

## Needs runtime capture

Neither source establishes the following. Each needs Rebased launched.

1. **Exact conflict-dialog button order** — left-to-right placement of
   `Resolve All Simple Conflicts` / `Review Changes` / `Accept and Finish`, and
   whether `Accept and Finish` is right- or centre-aligned. The bundle proves the
   strings exist; nothing proves the layout. `SOURCES.md` Tier 2 records that
   menu ordering is unreadable from public source.
2. **Focus target after `Accept and Finish`** — does focus land on the commit
   list, the log, or nowhere? Nothing in `VcsBundle.properties` or the docs says.
3. **Whether an undo affordance survives in the notification tray after
   finishing** — the delete-branch flow documents exactly that behaviour
   (manage-branches.md:167) but no equivalent is documented for merge completion.
4. **Recovery for `Cannot find file for {0}`** and for a failed unmerged-file
   scan (GitBundle:61, 1517) — no documented user-facing route.
5. **The file-list row's secondary affordance** — whether each row has an
   inline preview toggle, a diff-preview split, or nothing beyond the context
   menu.
6. **Greyed-vs-hidden rule for `Accept and Finish` and the bulk-apply cluster** —
   the docs say "enables", which does not distinguish a disabled button from a
   hidden one.

# Log tool window

> Surface `log-tool-window`. Filled by the agent pass from installed-bundle evidence
> plus verified documentation. Bundle wins on names/ids/keystrokes; docs win on
> UX and flow. See `research/evidence/rebased/DISTILL.md`.

## Sources

- https://www.jetbrains.com/help/idea/log-tab.html — verified 200, 18170 chars → `docs-text/log-tab.md`
- https://www.jetbrains.com/help/idea/investigate-changes.html — verified 200, 14274 chars → `docs-text/investigate-changes.md`
- `actions-labeled.tsv`, `keymap-bindings.txt`
- `strings/VcsLogBundle.properties` — the `vcs.log.*`, `file.history.*` families (**the log's entire state machine lives here**)
- `strings/VcsBundle.properties` — the `action.Vcs.Log.*` presentation settings

Docs resolve 2/2, and `VcsLogBundle.properties` is the best state-machine
evidence in the whole harvest: loading, five distinct error states, two distinct
empty states, and filter recovery are **all enumerated as strings**. This surface
can be fully specified for its states without runtime capture.

## UI — what exists

### Action inventory (bundle)

10 actions harvested from the installed bundle.

| Action id | Label | Key |
| --- | --- | --- |
| `Git.Log.Branches.Change.Branch.Filter` | Update Branch Filter in Log | — |
| `Git.Log.Branches.Change.Branch.Filter.On.Selection` | Update Branch Filter | — |
| `Git.Log.Branches.Navigate.Log.To.Branch.On.Selection` | Navigate Log to Branch Head | — |
| `Git.Log.Branches.Navigate.Log.To.Selected.Branch` | Navigate Log to Selected Branch Head | — |
| `Git.Log.Hide.Branches` | Hide Git Branches | — |
| `Git.Log.HighlightCherryPickedCommits` | Not Cherry-Picked Commits | — |
| `Git.Log.Show.Command` | Show Git Log for Command | — |
| `Vcs.CopyCommitSubjectAction` | — | — |
| `Vcs.CopyCurrentBranchName` | — | — |
| `Vcs.CopyRevisionNumberAction` | — | — |

The three `Vcs.Copy*` actions matter more than their empty label columns suggest:
**copy-to-clipboard is a first-class verb family with three members** — revision
number, commit subject, current branch name. All three are in the docs' context
menus (log-tab.md:267, 268; manage-branches.md:46) and all three are *read-only*
operations on this surface, which is why a log with no mutating verbs is still a
surface.

The presentation settings are equally substantive — `action.Vcs.Log.*` in the
bundle, all in the settings tree:

| Action | Label | Why it matters |
| --- | --- | --- |
| `Vcs.Log.CompactReferencesView` | Compact References View | "branch references for a single commit are displayed in a collapsed view" (log-tab.md:228) |
| `Vcs.Log.ShowTagNames` | Tag Names | tag names beside the icon (log-tab.md:232) |
| `Vcs.Log.ShowLongEdges` | Long Edges | "If this option is disabled (**by default**), long branches are replaced with a down arrow" (log-tab.md:236) |
| `Vcs.Log.PreferCommitDate` | Commit Timestamp | commit time vs author time (log-tab.md:238) |
| `Vcs.Log.AlignLabels` | References on the Left | refs move to the left of the message (log-tab.md:240) |
| `Vcs.Log.ShowRootsColumnAction` | Root Names | expand the Roots column (log-tab.md:242) |
| `Vcs.Log.EnableFilterByRegexAction` | Regex | regex search (log-tab.md:172) |
| `Vcs.Log.MatchCaseAction` | Match Case | (log-tab.md:173) |
| `Vcs.Log.GoToRef` | Go To Hash/Branch/Tag | `⌃F` (log-tab.md:257) |
| `Vcs.Log.FocusTextFilter` | Focus Text Filter | `⌃L` (log-tab.md:164) |
| `Vcs.Log.ShowDetailsAction` | Show Details | (log-tab.md:370) |
| `Vcs.Log.ShowDiffPreview` | Show Diff Preview | (log-tab.md:372) |
| `Vcs.Log.MoveDiffPreviewToRight` / `.ToBottom` | Right / Bottom | diff-preview location (log-tab.md:368) |
| `Vcs.Log.ShowOnlyAffectedChanges` | Show Only Affected Changes | restricts to the Paths filter |
| `Vcs.Log.CompareRevisions` | Compare Versions | two-commit compare (investigate:47) |
| `Vcs.Log.ShowAllAffected` | Show All Affected Files | `⌥⇧A` (investigate:82) |
| `Vcs.Log.ShowChangesFromParents` | Show Changes to Parents | the merge-review 3-panel diff (log-tab.md:426) |
| `Vcs.Log.GoToParent` / `.GoToChild` | Go to Parent/Child Commit | `←`/`→` (log-tab.md:331-335) |
| `Vcs.Log.OpenRepositoryVersion` | Show File at Revision | (log-tab.md:430) |
| `Vcs.Log.AnnotateRevisionAction` | Annotate | blame (investigate:228) |
| `Vcs.Log.ResumeIndexing` | Resume Indexing | "Indexing was paused as it took longer than expected. Resume." |
| `Vcs.Log.SelectInLog` | Show in VCS Log | cross-surface jump |

Plus `action.description.collapse.linear.branches` / `expand.linear.branches`
(= "Collapse Linear Branches" / "Expand Linear Branches", log-tab.md:200-201) and
`group.Vcs.Log.ToggleColumns.text=Columns` — the column set is user-selectable
(author, date, hash, CI checks; log-tab.md:244).

### Topology: four panes, and the tab is not one tab

From log-tab.md:10-18, the four panes and their exact positions:

| Pane | Position | Contents |
| --- | --- | --- |
| **Branches** | left | "all local and remote branches" — and all branch operations |
| **Commits** | centre | "the commits to all or to the selected branches" |
| **Changed Files** | right, upper | files modified within the selected commit |
| **Commit Details** | right, **under** Changed Files | "the details of the selected commit" |

**The right side is a vertical split, not a tab pair.** That is the topology fact
most likely to be mis-built.

**The Log tab is one of several tabs in the same tool window** (`⌥9`,
`ActivateVersionControlToolWindow Opt+9`), and the tabs are *per-query*:

- `Log` — the main tab (`vcs.log.tab.name=Log`)
- `Update Info` — opens automatically after an update (sync:78)
- `History` — file history (`file.history.diff.preview.editor.tab.name=History: {0}`)
- `Console`
- `Local changes` — the Commit tool window can become this tab
  (commit-and-push-changes.md:151)
- `Repository Diff` — the diff-preview editor tab
  (`vcs.log.diff.preview.editor.tab.name=Repository Diff: {0}`)

**Tabs are created by filters, and that is the key interaction**: "To avoid setting
filters back and forth, click on the toolbar to open a new tab matching your
filters" (investigate:29). Bundle: `vcs.log.action.open.new.tab.with.log=Open New
{0} Log Tab` — and a **guard against duplicates**:
`vcs.log.duplicated.tab.id.error=VCS Log tab cannot be opened twice` (VcsLog:156).

**Per-commit row grammar** (log-tab.md:133-146):

- message, author, timestamp
- the branch name label on each branch's latest commit
- colour-coded refs: **yellow = current branch head**, **green = local branches**,
  **violet = remote branches** (log-tab.md:135-139)
- **background** distinguishes membership: "Commits to the current branch are
  displayed against the light blue background, while commits to all other branches
  are shown against the white background" (line 140)
- **bold** for the user's own commits (line 141)
- an **asterisk** where author ≠ committer (line 142)
- in multi-root: "the colored stripe on the left indicates which root the selected
  commit belongs to … Hover over the colored stripe to invoke a tip that shows the
  root path" (line 145)

Branch and ref interaction affordances, all with tooltips in the bundle:

- `vcs.log.graph.arrow.tooltip.jump.to.subject.author.date.time=Jump to {0} by {1} on {2} at {3}` (VcsLog:129)
- `vcs.log.graph.arrow.tooltip.jump.to.hash=Jump to commit {0}` (130)
- `vcs.log.graph.arrow.tooltip.jump.to.hash.in.root=Jump to commit {0} in {1}` (131) — a
  **per-root variant**, so the tooltip changes shape in multi-root
- `vcs.log.references.more.tooltip=... {0} more in details pane` (132) — a **cap with
  a count**, matching "If the selected commit is contained in more than six
  branches, only the first six are displayed, and the `Show All` link appears"
  (log-tab.md:432)
- `vcs.log.click.to.collapse.paths.column.tooltip=Click to collapse` (127)

### State matrix — the log's state machine is fully enumerated

This is the most completely evidenced state matrix in the harvest. All
`VcsLogBundle.properties`.

| State | String | Key | Recovery offered |
| --- | --- | --- | --- |
| loading (first paint) | `Loading VCS Log…` | `vcs.log.is.loading` | — |
| loading (commits) | `Loading commits…` | `vcs.log.loading.status` | — |
| loading (process) | `Loading commits…` | `vcs.log.creating.process` | — |
| loading (details) | `Loading selected details` | `vcs.log.loading.selected.details.process` | — |
| loading (filters) | `Applying Filters…` | `vcs.log.applying.filters.process` | — |
| **error: commits** | `Error loading commits` | `vcs.log.error.loading.commits.status` | `Refresh` (`vcs.log.refresh.status.action`) |
| **error: details** | `Error loading commit details` | `vcs.log.error.loading.details.status` | `Refresh` |
| **error: changes** | `Error loading changes` | `vcs.log.error.loading.changes.status` | `Refresh` |
| **error: filtering** | `Error filtering commits` | `vcs.log.error.filtering.status` | `Reset filters` (`vcs.log.reset.filters.status.action`) |
| **empty: nothing committed** | `No changes committed.` | `vcs.log.no.commits.status` | **`Commit local changes`** (`vcs.log.commit.status.action`) — a *call to action*, not just a message |
| **empty: filters excluded everything** | `No commits matching filters` | `vcs.log.no.commits.matching.status` | **`Reset filters`** |
| empty: file history | `File history` | `file.history.empty.status` | `Reset filters` |
| error: file history | `Error calculating file history` | `file.history.error.status` | `Reset filters` |
| default (nothing special) | `Changes log` | `vcs.log.default.status` | — |
| indexing | `{0} Log Indexing…` | `vcs.log.status.bar.indexing` | cancel tooltip: "Cancel indexing. Indexing could be restarted manually from {0} Log toolbar." |
| indexing cancelled | `Cancelling` | `vcs.log.status.bar.indexing.cancelling` | `Resume Indexing` |
| progress: jump | `Searching commit {0} in the log` | `vcs.log.show.commit.in.log.process` | — |
| progress: file-history jump | `Searching commit {0} in file history` | `file.history.show.commit.in.history.process` | — |
| error: bad hash | `'{0}' does not look like a commit hash` | `vcs.log.string.is.not.a.hash` | — |
| error: not found | `{0} not found` | `vcs.log.commit.not.found` | — |
| **error: filtered out** | `{0} doesn't match the filters` | `vcs.log.commit.does.not.match` | **two links**: `View and reset filters` and `View in new tab` (`vcs.log.commit.does.not.match.view.and.reset.link`, `…view.in.tab.link`) |
| error: not in branch | `{0} does not exist in history for {1} in selected branches` | `file.history.commit.not.found.in.branch` | `View and show all branches` / `View in Log` |
| error: duplicate tab | `VCS Log tab cannot be opened twice` | `vcs.log.duplicated.tab.id.error` | — |

**The pattern to copy: every error state names the operation that failed
(`commits` / `commit details` / `changes` / `filtering`), and every non-terminal
error state carries its own recovery action.** A log that says "something went
wrong" and offers only Refresh is strictly worse than one that says "Error
filtering commits · Reset filters".

Also note the two empty states are **different sentences with different actions**:
no history at all offers `Commit local changes`; filters excluded everything offer
`Reset filters`. Conflating them into one "no results" row loses the recovery.

## UX — how it behaves

### How is it reached, and what hides behind a secondary affordance?

- `⌥9` / `View | Tool Windows | Git` (tutorial:30); `Git.Log` = "Show Git
  Repository Log…"; `Vcs.Show.Log` = "Show VCS Log" (both `actions-labeled.tsv`).
- **The Branches pane can be hidden**: `Git.Log.Hide.Branches` = "Hide Git Branches"
  — a first-class action, so the pane's presence is a user choice, and TurboGit's
  fixed four-pane layout will not match this affordance.
- **Per-query tabs** — the main reason the log is a tab surface rather than a
  single view (investigate:29).

Behind secondary affordances:

- **Everything.** The Commits pane toolbar carries: search field (with history and
  clear), `.*` / `Cc` text-filter toggles, `Branch`, `User`, `Date`, `Paths`
  filters, `Graph Options`, `Enable Git Log Indexing`, `Refresh` (`⌃F5`),
  `Cherry-pick`, `Presentation Settings`, `Go to Hash/Branch/Tag` (`⌃F`)
  (log-tab.md:160-259). **A dozen controls, of which only Refresh and Cherry-pick
  are single clicks.**
- **Graph Options** is a submenu: sort By commit date / Topologically; `Show First
  Parent` / `No Merges`; `Collapse Linear Branches` / `Expand Linear Branches`
  (log-tab.md:194-201).
- **Presentation Settings** is a two-part submenu — `Show:` (six options),
  `Highlight:` (four options), `Columns:` (log-tab.md:225-244).
- **The Branches pane's own single-click behaviour** is a setting
  (log-tab.md:68-75) — see `02-branches-popup`.
- **Issue-tracker links** in commit messages require prior configuration: "This
  functionality is available if: The pattern of the bug tracking system is specified
  in the Issue Navigation Settings … The corresponding issue number is mentioned in
  the commit message" (log-tab.md:147-152). **Conditional feature, silently absent
  when unconfigured.**

### Which verbs are destructive, and what do they confirm or preflight?

**Almost nothing on this surface is destructive** — it is primarily a read surface.
That is the finding, and it is why the log is a safe place to put verbs that are
destructive elsewhere.

| Verb | Destructive | Confirmation | Evidence |
| --- | --- | --- | --- |
| Copy Revision Number / Subject / Branch Name | no | none | log-tab.md:267-268 |
| `Cherry-pick` | **yes** on the target branch | no dialog; "IntelliJ IDEA will apply and commit changes to the target branch" (apply-changes:244) | apply-changes:244 |
| `Cherry-pick` when already contained | — | **the button is disabled**: "This button is disabled if the selected commit is already contained in the current branch" (log-tab.md:221) | log-tab.md:221 |
| `Reset Current Branch to Here…` | **yes** | **Yes** — the Git Reset dialog asks for the mode: "select the mode in which the working tree will be updated" (log-tab.md:286) | log-tab.md:286 |
| `Revert Commit` | no — additive (creates a revert commit) | none documented (log-tab.md:288) | log-tab.md:288 |
| `Undo Commit` | **yes** — removes a commit | **disabled unless your own**: "This command is only available for the commits made by you" (log-tab.md:292) | log-tab.md:292 |
| `Edit Commit Message` | **yes** — a rewrite | **disabled once pushed** (log-tab.md:295) | log-tab.md:295 |
| `Drop Commits` | **yes** | see `06-history-rewrite` | GitBundle:261-263 |
| `Cherry-Pick Selected Changes` | no — lands in a changelist | a dialog to choose/create the changelist (apply-changes:265) | apply-changes:265 |
| `Extract Selected Changes to Separate Commit` | **yes** — a rewrite | "there will be two commits with new hashes" (edit:78); no confirm documented | edit:78 |
| `Revert Selected Changes` | **yes** — discards a file's changes | none documented; "Changes that revert the selected file will be placed in the active changelist" (log-tab.md:405) | log-tab.md:405 |
| `Get from Revision` / `Get from Branch` | **yes** — overwrites a file | none documented (log-tab.md:420; apply-changes:285) | — |
| `Checkout Revision` / `Show Repository at Revision` | no | none | log-tab.md:277-281 |

**The gating convention is the useful part.** Rebased encodes the safety rule in
*enablement*, not in a confirmation:

- Cherry-pick is disabled when the commit is already contained (log-tab.md:221)
- Undo Commit is disabled unless the commit is yours (log-tab.md:292)
- Edit Commit Message is disabled once pushed (log-tab.md:295)

**Three different preconditions, three different disable rules, and a confirm
dialog is used only where the user must choose a mode (Reset).** That is a cleaner
division of labour than "confirm everything".

`Revert Selected Changes` is the one destructive verb whose *destination* is worth
noting: it does not discard silently, it "will be placed in the active changelist"
(log-tab.md:405) — so the change becomes reviewable rather than gone.

### What can be undone, from where, and for how long?

- **The log is where undo *starts*, not where it happens.** The undo lives in the
  success notification: `Dropped {N} {commits}` with an undo action, `Commit message
  changed` with one, `Commits Squashed` (GitBundle:267, 280, 289). See
  `06-history-rewrite` for the full lifecycle.
- **Undo Commit** is available for the user's own unpushed commits (log-tab.md:292;
  `Git.Uncommit`).
- **Revert of a pushed commit is a new commit**, and the docs say so plainly: "To
  revert a pushed commit, right-click it on the Log tab … and select `Revert
  commit`" (tutorial:129). **The log never pretends a push can be undone.**
- **Indexing can be paused and resumed**: "Indexing was paused as it took longer
  than expected. Resume." (`Vcs.Log.ResumeIndexing`). Not history, but it is a
  long-running operation the user may need to stop.
- **Not undoable from this surface**: `Get from Revision`, `Revert Selected
  Changes` (it stages a revert you must then commit), and anything on a protected
  branch.

### Which options are on by default, and what is remembered between uses?

| Option | Default | Evidence |
| --- | --- | --- |
| Commit sort | **By commit date** — "By default, the commits are sorted By commit date. You can also sort them Topologically" (log-tab.md:196) | log-tab.md:196 |
| Long Edges | **off** — "If this option is disabled (**by default**), long branches are replaced with a down arrow" (log-tab.md:236) | log-tab.md:236 |
| Compact References View | present, default **UNVERIFIED** (log-tab.md:228 describes the enabled state) — re-tested against log-tab/advanced-settings 2026-10-06: no page states a default (advanced-settings has no `Vcs.Log` entries) → runtime capture (R3) | log-tab.md:228 |
| Show Details | present; `Show Details` / `Show Diff Preview` are layout toggles (log-tab.md:370-372) | log-tab.md:370-372 |
| Diff Preview Location | `Right` vs `Bottom` (log-tab.md:368) | log-tab.md:368 |
| Current-branch commits highlighted | **on** — light blue background (log-tab.md:140) | log-tab.md:140 |
| My commits bold | **on** (log-tab.md:141) | log-tab.md:141 |
| Merge commits greyed out | present (log-tab.md:248) | log-tab.md:248 |
| Not Cherry-Picked Commits greyed | present, **Git-only** (log-tab.md:253) | log-tab.md:253 |
| Enable Git Log Indexing | present; improves filtering, file-history refs, and Search Everywhere (log-tab.md:204-211) | log-tab.md:204-211 |
| Regex / Match Case | **off** — a text-filter toggle (log-tab.md:170-173) | log-tab.md:170-173 |
| Branches pane visible | user-choosable via `Hide Git Branches` | log-tab.md:32 |
| Filter defaults | `All` for every dimension — "If you want to see commits from all local and remote branches, select `All`" (log-tab.md:178); bundle `vcs.log.filter.all=All` (VcsLog:198) | log-tab.md:178; VcsLog:198 |
| Search finalisation | "press Enter or move the focus away from the search field" (log-tab.md:163) | log-tab.md:163 |

Remembered between uses:

- **Filters are per-tab, and that is the point**: opening a new tab "matching your
  filters" (investigate:29) is how a filter set is *kept*. So filters persist in
  the tab, not in a global setting.
- **Presentation settings are global** (`action.Vcs.Log.*`), so the graph
  rendering is remembered across tabs.
- **Branch-filter single-click behaviour is remembered** as a setting
  (log-tab.md:68-75).
- **`vcs.log.filter.recent=Recent`** (VcsLog:200) — a third filter preset beside
  `All` and `Clear`, whose semantics are undocumented.

### What feedback appears, and where does focus land?

Feedback is the log's strongest suit — see the state matrix above. The design in
one line: **every state has a named string, and every non-terminal state has a
recovery action.**

Graph tooltips as micro-feedback:

- `Jump to {subject} by {author} on {date} at {time}` (VcsLog:129)
- `Jump to commit {hash}` (130) / `Jump to commit {hash} in {root}` (131) — the
  per-root variant
- `... {N} more in details pane` (132) — a truncated ref list that says how much is
  hidden
- `Click to collapse` / `Click to expand` on the Paths column (127-128)

Progress and cancellation:

- Indexing is a **status-bar** item, not a modal: `{0} Log Indexing…` with a
  `Cancelling` state and a tooltip explaining that indexing "could be restarted
  manually from {0} Log toolbar" (VcsLog:151-153). **A long-running background
  operation that is cancellable and explainable, in a status bar.**
- Process strings for commits, details, and filters (VcsLog:157-160).

**Optimistic paint: no** — and the log is the surface where that is most visible,
because a wrong optimistic row is a wrong *history*. The five error strings
(`Error loading commits` / `commit details` / `changes` / `filtering`) are the
evidence that each async read reports independently.

Focus after the action:

- `←`/`→` move parent/child and therefore move the selection (log-tab.md:144).
  "This is especially useful if you have commits to different repositories and
  multiple branches all mixed in the Log tab" (line 144) — so the arrow keys are a
  *navigation* primitive, not a selection convenience.
- Clicking a graph arrow jumps, and the whole commit's data loads
  (VcsLog:129-131).
- `Go to Hash/Branch/Tag` "will take you to the latest commit in that branch"
  (investigate:21) — so the jump target is a commit, selected in the graph.
- **A jump that lands outside the current filters does not clear the filters** — it
  offers `View and reset filters` or `View in new tab` (VcsLog:174-175). **The
  filter set is never silently discarded; the user chooses.**
- **UNVERIFIED**: whether the Commits pane or the Branches pane holds focus after
  the tool window opens. (re-tested against log-tab/investigate-changes 2026-10-06:
  silent on focus placement → runtime capture (R3))

## Flow — how tasks chain

### Entry points

1. `⌥9` / `View | Tool Windows | Git` (tutorial:30).
2. `Git.Log` = "Show Git Repository Log…"; `Vcs.Show.Log` = "Show VCS Log".
3. `Git.Show.Command` = "Show Git Log for Command" — the log *of a past command*.
4. From any file: `Git | Show History` → "The **History tab** of the Git tool
   window opens" (tutorial:191, investigate:56).
5. From a selection: `Git | Show History for Selection` → "The History for
   Selection dialog will open … If nothing is selected, the history will be
   displayed for the current line" (tutorial:194-195, investigate:122).
6. From a directory: "A new tab is added to the Git tool window that shows commits
   filtered by the selected folders" (investigate:130).
7. `Push All up to Here…` and `Cherry-pick` are consumed *by* this surface.
8. **Automatically**: the `Update Info` tab after a project update (sync:78).

### Internal sequence, with dialog chaining

**The reference review journey — filter, inspect, act:**

```
⌥9 → Log tab
  → Branches pane: select a branch   (single click = Update Branch Filter | Navigate Log to Branch Head)
  → Commits pane: ⌃F → hash/branch/tag → jumps to the latest commit in that branch
  → ⌃L to return to the search field
  → select a commit
       → Commit Details (message, hash, author, email, date, GPG, root, branches)
       → "If a commit message references another commit, you can click this
           commit's hash to jump to it in the log"                          (log-tab.md:433)
       → Changed Files pane → ⌘D → Diff Viewer
       → Changed Files pane → right-click → Revert / Cherry-Pick / Extract / Drop / Patch
  → commit row context menu → Cherry-pick | Revert | Undo | Reword | Drop |
       Squash | Interactively Rebase | Push All up to Here | New Branch / Tag | Reset
```

**Compare two commits:**

```
Log → select TWO commits → Compare Versions
  → "The Changes tool window with a list of files modified between the selected
     commits opens"                                                   (investigate:47-48)
  → ⌘D per file
```

**Review how a merge was resolved — the flow's tail, and its best idea:**

```
Log → select the MERGE commit
  → (no conflicts were resolved) Changed Files pane suggests reviewing changes
     from both parents                                                    (investigate:149)
  → select a file from one node → ⌘D
  → TWO-panel diff (current vs the selected parent)
  → (conflicts WERE resolved) Changed Files pane lists the conflicted files
  → select a file → ⌘D
  → THREE-panel diff: "compare the current version with each of its parents,
     and see how exactly conflicts were resolved"                        (investigate:153)
```

(investigate:145-153; the verb is `Vcs.Log.ShowChangesFromParents` = "Show Changes
to Parents", log-tab.md:426.) **This is the audit trail for conflict resolution,
and it closes the loop opened by the Conflicts dialog** (which documents the same
review path at resolve-conflicts.md:76).

**File history:**

```
any file → Git | Show History
  → History tab (name: "History: {file}")
  → select a revision → diff preview on the right
  → ⌘D → full Diff Viewer
  → toolbar: Branch filter, Refresh, Show All Affected Files (⌥⇧A),
             View Options, Open on GitHub/GitLab, Enable Git Log Indexing,
             the Renaming column
  → right-click a line → Annotate
```

(investigate:56-115.)

**Selection history — a separate window, not a tab:**

```
editor: select a fragment (or just place the caret)
  → Git | Current File | Show History for Selection
  → "The history for the selected fragment will open in a separate window"
```

(investigate:121-122.)

**A jump that leaves the filter set — the escape that does not destroy state:**

```
⌃F → type a hash
  → {hash} doesn't match the filters
  → [View and reset filters]   (keep filtering, widen it)
  → [View in new tab]          (keep the filter set intact, look elsewhere)
```

(VcsLog:174-175.)

### What happens next on success

- Selecting a commit fills Changed Files and Commit Details; `Show Only Affected
  Changes` can restrict the file list to the Paths filter (log-tab.md:253).
- `Push All up to Here…` opens the Push Commits dialog with the commit list capped
  at the selection (commit-and-push-changes.md:206-208) — see `08-remotes-sync`.
- A successful cherry-pick commits to the target branch immediately
  (apply-changes:244).
- **The `Update Info` tab arriving after an update is the log's most common
  involuntary entry** (sync:78).

### What happens next on failure, and how the user recovers

| Failure | Evidence | Recovery |
| --- | --- | --- |
| Commits failed to load | `Error loading commits` (VcsLog:146) | `Refresh` (154) |
| Commit details failed | `Error loading commit details` (147) | `Refresh` |
| Changed files failed | `Error loading changes` (148) | `Refresh` |
| Filtering failed | `Error filtering commits` (151) | `Reset filters` (153) |
| Filters excluded everything | `No commits matching filters` (152) | `Reset filters` |
| Nothing committed yet | `No changes committed.` (149) | **`Commit local changes`** (150) — jumps to the Commit tool window |
| Bad hash typed | `'{0}' does not look like a commit hash` (167) | correct the input |
| Hash not found | `{0} not found` (168) | — |
| **Found but filtered out** | `{0} doesn't match the filters` (169) | `View and reset filters` **or** `View in new tab` (174-175) |
| Not in the selected branch's history | `{0} does not exist in history for {1} in selected branches` (VcsLog:186) | `View and show all branches` / `View in Log` |
| File-history load failed | `Could not load selected commits: {0}` (VcsLog:177) | **UNVERIFIED** — re-tested against log-tab/investigate-changes 2026-10-06: neither page names a recovery for this error → runtime capture (R3) |
| Indexing too slow | `{0} Log Indexing…` → `Cancelling` → `Resume Indexing` (151-153) | cancel, then resume from the toolbar |
| Tab already open | `VCS Log tab cannot be opened twice` (156) | **UNVERIFIED** — presumably the existing tab is focused, but nothing says so; re-tested against log-tab 2026-10-06: the page covers only *creating* tabs, not duplicate-tab handling → runtime capture (R3) |
| Cherry-pick of an already-contained commit | button disabled (log-tab.md:221) | the commit is already there |

### Escape / dismiss semantics

- **The filter set is never silently reset.** The two "your target is outside the
  filter" errors both offer a *keep the filters* path (VcsLog:174-175). This is the
  most important escape semantic on the surface: a log that clears your filters to
  show you something destroys the context you were working in.
- **Hiding the Branches pane is reversible** (`Git.Log.Hide.Branches`), so the
  layout is a user preference rather than a mode.
- **Indexing is cancellable and resumable**, with the restart path named in the
  tooltip (VcsLog:152).
- The `Paths Affected by Commit {0}` dialog (VcsLog:124) is a read-only
  drill-down, so it has no destructive dismiss semantics.

## Divergences (intentional)

| Dimension | Rebased behaviour | TurboGit behaviour | Why |
| --- | --- | --- | --- |
| UI | The Branches pane can be **hidden** by a first-class action (`Git.Log.Hide.Branches`) | Fixed four-pane layout (branches / graph / changed-files / commit-details) | egui panels are cheap, but a *user-toggleable pane* is a layout state TurboGit's `log_window.rs` does not model. **Partial gap**, small. |
| UI | Filters live in **per-query tabs**, created by "open a new tab matching your filters" (investigate:29) | One Log tab with a scope model (`log_ref_scope`, `RootCaches::ensure_ref_log`) | TurboGit's scope-chip model is arguably better than tab proliferation, and it already supports what the tab was for. **Intent matched by a different mechanism** — recorded so the difference is understood, not accidental. The one thing worth stealing is `vcs.log.duplicated.tab.id.error` (VcsLog:156): TurboGit's scope model must also refuse to open a duplicate scope. |
| UX | Ten states with named strings, and **every non-terminal state carries its own recovery action** — including `Commit local changes` for the empty-history state (VcsLog:149-155) | Log states exist but are not enumerated per-operation | **The highest-value gap on this surface.** `Error loading commits` vs `Error loading commit details` vs `Error loading changes` vs `Error filtering commits` are four different failures with four different recoveries, and `No changes committed.` offering `Commit local changes` is a lovely empty state. TurboGit already has `Reading`/`Empty`/`Populated` in `branches.rs`; the log needs the same discipline with more states. See ticket 15. |
| UX | A jump outside the filter set offers `View and reset filters` **or** `View in new tab` (VcsLog:174-175) | **UNVERIFIED** | **Open gap, and it is a correctness-of-interaction issue, not a feature.** Silently widening or clearing a filter set to reveal one commit destroys the user's context. Two named escapes, both non-destructive, is the right shape. re-tested 2026-10-06: no docs page can answer a TurboGit-side question; code read of `log_window.rs` shows jumps (`SelectParent`) are selection + scroll moves with no filter-reset/new-tab escape pair, and a target outside the loaded list is not modelled → close by code citation (R1 fold-in), not R3. |
| UX | The empty-history state offers a **call to action** (`Commit local changes`) rather than a bare message (VcsLog:150) | Empty states are descriptive | **Open gap, one line of copy.** Copy the intent, not the string. |
| UX | Commit row encoding is five independent signals — ref colour (yellow/green/violet), background (current vs other), bold (mine), asterisk (author≠committer), root stripe (multi-root) (log-tab.md:135-145) | Commit rows exist; the five-signal grammar is not established | **Open gap, and the asterisk is the highest-value single item** — it is the only signal that answers "is this my change?" at a glance, and the log already carries the data. |
| UX | Refs are capped at six with `... {N} more in details pane` (log-tab.md:432; VcsLog:132) | **UNVERIFIED** | **Open gap, small and well-specified** — including the *count* in the overflow affordance, which is what makes truncation honest. re-tested 2026-10-06: TurboGit-side, docs N/A; `log_window.rs` collapses a commit's refs into one pill whose names live in a hover tooltip — no cap, no hidden-count → close by code citation (R1 fold-in). |
| UI | `Long Edges` off by default, so collapsed branches show a down arrow (log-tab.md:236) | **UNVERIFIED** | **Open gap, small.** The default matters more than the setting: a log that draws every long branch as a full rail is noisy. re-tested 2026-10-06: TurboGit-side, docs N/A; `log_window.rs` paints no commit-graph lanes at all (a list with a selection rail), so there is no long-edge concept to default → close by code citation (R1 fold-in). |
| UX | Commit sort defaults to **By commit date**, with `Topologically` available (log-tab.md:196) | **UNVERIFIED** | **Open gap.** Topological sort is what makes a merge readable — "displaying incoming commits first, directly below the merge commit" (log-tab.md:196). re-tested 2026-10-06: TurboGit-side, docs N/A; `log_display.rs:175` sorts newest-first by commit time with an id tie-break, and no topological mode exists → close by code citation (R1 fold-in). |
| UX | `Not Cherry-Picked Commits` greys out commits not yet applied to the current branch (log-tab.md:253; `Git.Log.HighlightCherryPickedCommits`) | **UNVERIFIED** | **Open gap with real value** — it answers "what have I not taken yet?" during a backport or a stack surgery, which is exactly the rebase-heavy workflow TurboGit supports. re-tested 2026-10-06: TurboGit-side, docs N/A; `log_window.rs`'s only cherry verbs are CherryPickTo/CherryPickAcross — no non-picked highlighting pass exists → close by code citation (R1 fold-in). |
| UI | Issue-tracker links render in commit messages when configured (log-tab.md:147-152) | Not implemented | **Intentionally absent** for a parity pass — issue-tracker integration is a separate feature with its own configuration surface, and no `research/` evidence describes TurboGit's intent. Recorded as a decision. |
| UX | Copy verbs are three distinct first-class actions (`Vcs.CopyRevisionNumberAction`, `Vcs.CopyCommitSubjectAction`, `Vcs.CopyCurrentBranchName`) | **UNVERIFIED** | **Open gap, trivial and worth it** — a log where you cannot copy a hash is annoying, and three members cost three lines. re-tested 2026-10-06: TurboGit-side, docs N/A; `commit_menu.rs` has CopyHash and CopyMessage, and no copy-branch-name verb exists in `branch_menu.rs` — two of three members present → close by code citation (R1 fold-in). |
| UI | Ten tabs' worth of presentation toggles (`action.Vcs.Log.*`: Compact References, Tag Names, Long Edges, Commit Timestamp, References on the Left, Root Names, Columns, Regex, Match Case, Details/Diff-Preview location) | Not established | **Out of scope for a first parity pass, deliberately.** These are refinement toggles with no workflow consequence; the *workflow* toggles (Regex, Match Case, Details, Diff Preview) matter more than the cosmetic ones. Recorded so the omission is a priority judgement, not an oversight. |

## Needs runtime capture

1. **Default states of `Compact References View`, `Show Tag Names`,
   `Show Details`, `Show Diff Preview`, and `Diff Preview Location`** — all exist
   as actions; none has a documented default.
2. **What `vcs.log.filter.recent=Recent` (VcsLog:200) filters to** — a third preset
   beside `All` and `Clear` with no description anywhere.
3. **The `Paths Affected by Commit {0}` dialog's contents** (VcsLog:124) — the title
   is known, the body is not.
4. **What happens when `VCS Log tab cannot be opened twice` (VcsLog:156)** — is the
   existing tab focused, or is this purely an error toast?
5. **Whether the Commit Details pane's six-branch cap applies to tags as well as
   branches** (log-tab.md:432 says "more than six branches").
6. **Initial focus on tool-window open** — Commits pane, Branches pane, or search
   field.
7. **The Graph Options submenu's visual form** — checkboxes, radio pairs, or a
   dialog; and whether `Collapse Linear Branches` is also clickable directly on the
   graph ("It is also possible to collapse an individual expanded branch by
   clicking it", log-tab.md:200).

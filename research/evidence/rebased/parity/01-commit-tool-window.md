# Commit tool window

> Surface `commit-tool-window`. Filled by the agent pass from installed-bundle
> evidence plus verified documentation. Bundle wins on names/ids/keystrokes; docs
> win on UX and flow. See `research/evidence/rebased/DISTILL.md`.

## Sources

- https://www.jetbrains.com/help/idea/commit-and-push-changes.html — verified 200, 21006 chars → `docs-text/commit-and-push-changes.md`
- https://www.jetbrains.com/help/idea/working-with-git-tutorial.html — verified 200, 15473 chars → `docs-text/working-with-git-tutorial.md`
- `actions-labeled.tsv`, `keymap-bindings.txt`
- `strings/GitBundle.properties`, `strings/VcsBundle.properties` — the `changes.*`, `new.changelist.*`, `commit.*` families

Docs resolve 2/2. This is the largest action set (56) and the widest surface: it
owns staging vs changelists, partial commit, amend, commit-and-push, the commit
options panel, and the push dialog entry.

## UI — what exists

### Action inventory (bundle)

56 actions harvested from the installed bundle. The decisive subset — the ones
that define the surface's *modes* and *verbs*:

**Mode switch (the surface's defining pair):**

| Action id | Label | Key |
| --- | --- | --- |
| `Git.Stage.Enable` | Staging Area | — |
| `Git.Stage.Disable` | Changelists | — |

Descriptions from the bundle state the whole design in two lines:
`Git.Stage.Enable.description` = "Use the native Git concept of staging for making
commits"; `Git.Stage.Disable.description` = "Disable staging area and switch to
changelists". This is a **first-class user-facing mode toggle**, not a hidden
preference.

**Staging verbs:**

| Action id | Label | Key |
| --- | --- | --- |
| `Git.Stage.Add` | — | — |
| `Git.Stage.Add.All` | Stage All | — |
| `Git.Stage.Add.Tracked` | Stage All Tracked | — |
| `Git.Stage.Add.No.Content` | — | — |
| `Git.Stage.Reset` | — | — |
| `Git.Stage.Revert` | — | — |
| `Git.Stage.ToggleIgnored` | — | — |
| `Git.Stage.ToggleCommitAll` | Commit All when Nothing Is Staged | — |
| `Git.Stage.Merge` | — | — |
| `Git.Stage.Stash.Files` | Stash Files | — |

Descriptions: `Git.Stage.Add.All.description` = "Add all unstaged changes and
untracked files to index"; `Git.Stage.Add.Tracked.description` = "Add all unstaged
changes in tracked files to index"; `Git.Stage.ToggleCommitAll.description` =
"Commit all files when there are no staged files". **The last one is a forgiveness
affordance**, not a staging verb. Note `Git.Stage.Add.All` and
`Git.Stage.Add.Tracked` differ *only* in whether untracked files are included —
a distinction that is easy to lose and is a real user decision.

**Compare-from-staging (the three-way door):**

| Action id | Label |
| --- | --- |
| `Git.Stage.Compare.Local.Staged` | Compare with Staged Version |
| `Git.Stage.Compare.Staged.Head` | Compare with HEAD Version |
| `Git.Stage.Compare.Staged.Local` | Compare with Local Version |
| `Git.Stage.Compare.Three.Versions` | Compare HEAD, Staged and Local Versions |
| `Git.Stage.ThreeSideDiff` | Compare HEAD, Staged and Local Versions |
| `Git.Stage.Show.Local` | Show Local Version |
| `Git.Stage.Show.Staged` | Show Staged Version |

`Git.Stage.ThreeSideDiff.description` = "Show diff window with HEAD, staged and
local versions that **allows to interactively add changes into staging area**" —
so the three-way view is the *staging* surface, not just a viewer.

**Commit verbs:**

| Action id | Label | Key |
| --- | --- | --- |
| `Git.Commit.Stage` | Commit… | — |
| `Git.Commit.And.Push.Executor` | — | — |
| `Vcs.Commit.CloseDialog` | Close Commit Dialog | — |
| `Vcs.ToggleAmendCommitMode` | — | `⌥M` |
| `Vcs.RunCommitChecks` | — | — |
| `Vcs.ReformatCommitMessage` | — | — |
| `ChangesView.ShowCommitOptions` | — | `⌃O` |
| `ChangesView.ToggleCommitUi` | — | — |
| `ChangesView.SingleClickPreview` | — | — |
| `ChangesView.SetDefault` | — | `⌃␣` |
| `ChangesView.Edit` / `ChangesView.EditSource` | — | — |
| `Git.ChangesView.Merge` | — | — |
| `ChangesView.AddUnversioned` | — | — |

Note `Vcs.ToggleAmendCommitMode` at `⌥M` (`keymap-bindings.txt`) — a **toggle**,
not a one-shot, so amend can be left on.

**List management:**

| Action id | Label | Key |
| --- | --- | --- |
| `ChangesView.NewChangeList` | New changelist | — |
| `ChangesView.RemoveChangeList` | — | — |
| `ChangesView.Rename` | — | F2, ⇧F6 |
| `ChangesView.Move` | — | `⌥⇧M` |
| `ChangesView.GroupBy.Directory` | — | `⌃⌥P` |
| `ChangesView.GroupBy.Repository` | — | — |
| `ChangesView.Refresh` | — | — |
| `ChangesView.ShowIgnored` | Ignored Files / Show ignored files | — |
| `ChangesView.Revert` | — | `⌃⌥Z` |
| `ChangesView.RevertFiles` | — | — |
| `ChangesView.RemoveDeleted` | — | — |
| `ChangesView.Browse` | — | — |
| `ChangesView.SelectFile` | Select Opened File in Changes View | — |

`ChangesView.GroupBy.Module` is also in the keymap at `⌃⌥M`
(`keymap-bindings.txt`) — a *different* verb from `⌃⌥P`, so grouping has two
independent accelerators, and `⌥⇧M` (`ChangesView.Move`) is a third distinct
`M`-chorded gesture. Cheap to get wrong; recorded.

**Partial-commit machinery (the reason this surface is hard):**

| Action id | Label | Key |
| --- | --- | --- |
| `ChangesView.CreatePatch` | — | — |
| `ChangesView.CreatePatchFromChanges` | — | — |
| `ChangesView.CreatePatchToClipboard` | — | — |
| `ChangesView.ApplyPatch` | — | — |
| `ChangesView.ApplyPatchFromClipboard` | — | — |
| `Vcs.ApplySelectedChanges` | — | — |
| `Vcs.Diff.IncludeChangedLinesIntoCommit` | — | — |
| `Vcs.Diff.IncludeWholeChangedLinesIntoCommit` | — | — |
| `Vcs.Diff.ExcludeChangedLinesFromCommit` | — | — |
| `Vcs.RollbackChangedLines` | — | `⌃⌥Z` |
| `Vcs.MoveChangedLinesToChangelist` | — | `⌥⇧M` |
| `Vcs.Diff.ShowDiffInEditorTab` | — | — |
| `Vcs.Diff.ShowDiffInNewWindow` | — | — |
| `Vcs.Diff.ToggleSearchInChanges` | — | — |
| `Vcs.Diff.ToggleDiffAligningMode` | — | — |
| `Vcs.Diff.ShowCombinedDiff` | — | — |
| `Vcs.Diff.EnableDisableCombinedDiff` | — | — |

`Vcs.Diff.IncludeChangedLinesIntoCommit` and its two siblings: line-level
inclusion is a **three-member verb family** (include line, include whole changed
line, exclude line from commit), not one toggle.

**Shelf/stash from this surface:** `ChangesView.Shelve`,
`ChangesView.ShelveSilently`, `ChangesView.UnshelveSilently`,
`ChangesView.SaveToShelve`, `Vcs.Shelf.*`, `Vcs.Show.Shelf`, `Vcs.ShowLocal.Changes`.

### Topology

Vertical tool window on the left, `⌥0` (`ActivateCommitToolWindow Opt+0`,
`keymap-bindings.txt`; docs write `Alt+0`). Regions, from
`commit-and-push-changes.md`:

1. **Sub-tab strip** — at minimum Commit / Shelf / Stash / Console. Bundle:
   `Vcs.Show.Shelf`, `Git.Show.Stash` ("Open the Git Stash tab"), and
   `shelf.tab=Shelf` (VcsBundle). Shelf and stash can be **merged into one** —
   see defaults.
2. **Changes tree** — grouped by changelist, or by directory/module/repository.
   Under staging mode, staged/unstaged buckets instead; plus
   `Unversioned Files` and `Merge Conflicts` nodes
   (`set-up-a-git-repository.md:73`, `resolve-conflicts.md:18`).
   Node names in the tutorial: "the new file is tracked by Git and is added to
   the **Changes changelist** in the Commit tool window" (line 51) — so
   **"Changes" is a changelist, not the view name.** The terminology matters and
   is a live trap for a client that calls its own commit view "Changes".
3. **File rows** — each with a checkbox, a status word, and a colour
   (`working-with-git-tutorial.md:107`: "blue to modified, green — to newly
   added"). Double-click opens the diff (line 112).
4. **Commit card** — message field, then the options toggle (`⌃O`,
   `ChangesView.ShowCommitOptions`).
5. **Commit controls** — `Commit` / `Commit and Push` (`⌘⌥K`,
   `commit-and-push-changes.md:84`), with a dropdown arrow for `Commit and
   Rebase…` (`GitBundle:46`).
6. **Diff preview pane** — optional; a double-click routing setting decides
   whether double-click opens the diff or the source
   (`commit-and-push-changes.md:157-162`).

**The commit options panel** (`⌃O`), field by field
(`commit-and-push-changes.md:36-79`):

- **Author** — "if you are committing changes made by another person, you can
  specify the author of these changes"
- **Sign-off commit** — "Adds the following line at the end of the commit
  message: `Signed off by: <username>`"; bundle `commit.options.sign.off.commit.checkbox=Si&gn-off commit`
  and `commit.options.sign.off.commit.message.line`. **The bundle's wording is
  "Signed off by"** — one word, no hyphen. A client that writes "Signed-off-by"
  does not match.
- **Commit Checks** group: Reformat code, Rearrange code, Optimize imports,
  Cleanup, Update copyright, Check malicious dependencies
- **Advanced Commit Checks** group: Run advanced checks after a commit is done,
  Analyze code, Check TODO, Run Configuration
- **After Commit** group: Upload files to, Always use selected server or group
  (FTP/SFTP/WebDAV plugin-gated)

**One asymmetry worth copying**: "if you want to Commit and Push (⌘⌥K) your changes
right away, **the checks will be done before the commit**" (line 62) — i.e.
Commit-and-Push moves checks earlier rather than skipping them. And "Also, this
option is not available for the modal commit interface" (line 63).

### State matrix

| State | Which region | What it shows | Source |
| --- | --- | --- | --- |
| default | file row | checkbox unselected; status word; colour by change kind | tutorial:107 |
| default | Commit button | the bundle ships **two label variants**: `commit.action.name=Comm&it` and `commit.action.name.mac=Commi&t` (GitBundle:47, 49) — the mnemonic moves on macOS | GitBundle:47,49 |
| hover | gutter marker | the change-marker toolbar appears, carrying Commit / Amend / Stage | commit-and-push-changes.md:120-124 |
| pressed | file checkbox | the row's diff preview updates | — |
| disabled | Commit | blocked entirely by conflicts: banner `Cannot proceed due to merge conflicts` with action `Resolve…` (VcsBundle:1010-1011) | VcsBundle:1010-1011 |
| focus | message field | `⌃␣` completes **project file names** inside the commit message (tutorial:120). Note `ChangesView.SetDefault` is also `⌃SPACE` but is a *different* verb (set-default-changelist) | tutorial:120 |
| focus | message field | recent-message history available via the field's history control (line 30) | commit-and-push-changes.md:30 |
| selected | file row | diff preview shows that file | commit-and-push-changes.md:157 |
| selected | changelist | `⌘K` commits "the entire active changelist" (line 25) | commit-and-push-changes.md:25 |
| progress | commit | `Setting Git user name…` (GitBundle:1590) | GitBundle:1590 |
| error | identity | `Git User Name Is Not Defined` (GitBundle:1583), `You need to specify username and email for Git` (GitBundle:1585) | GitBundle:1583-1585 |
| error | identity, multi-root | `Git user.name and user.email properties are not defined in {0,choice,1#root|2#roots}` — **count-pluralised per root** (GitBundle:1587) | GitBundle:1587 |

## UX — how it behaves

### How is it reached, and what hides behind a secondary affordance?

Reached by `⌥0`, by `View | Tool Windows | Commit` (tutorial:28), and — this is
the interesting part — **it can be dissolved entirely**:

- "The Commit tool window can become the **Local changes tab in the Log tab** of
  the Git tool window" by clearing `Enable Commit tool window` in advanced
  settings (commit-and-push-changes.md:151-152). Bundle:
  `advanced.setting.vcs.commit.tool.window=Enable Commit tool window`.
- "you can set up the Commit tool window to work as a window with a list of local
  changes and **switch on the commit controls** … when you are ready to commit" —
  the `Toggle commit controls` option, `advanced.setting.vcs.non.modal.commit.toggle.ui`,
  documented at lines 148-149.
- View modes: "click Options and select View Mode | Window … To always keep the
  window in front of the IDE, select View Mode | Float" (line 144). The surface is
  one of at least three shapes.
- "If you want the Commit tool window to close after you commit your changes,
  click Options and clear the **Close Tool Window After Committing** option"
  (line 145); bundle: `advanced.setting.vcs.non.modal.commit.close.in.windowed.mode`.

Behind secondary affordances:

- **All commit options are behind `⌃O`** — six checkboxes in three groups. The
  default commit is one keystroke and zero options.
- **Line-level inclusion** is behind a per-line context menu (`Split Chunks and
  Include Selected Lines into Commit`, line 111) *or* a gutter checkbox that
  appears on hover (line 112).
- **Author override** is inside the options panel, not the commit card.
- **Run Git hooks** is a checkbox in commit settings that is easy to miss
  entirely: "You can disable running Git hooks for a particular commit in the
  commit settings by clearing the **Run Git hooks** checkbox" (line 86).

### Which verbs are destructive, and what do they confirm or preflight?

| Verb | Destructive | Confirmation | Evidence |
| --- | --- | --- | --- |
| Commit | no — additive | none needed | — |
| Commit and Push | no | Push dialog appears: "You will be able to review the current commit as well as all other commits before they are pushed" (line 84) | line 84 |
| Revert (`⌃⌥Z`) | **yes** — discards local edits | a confirm; TurboGit models this as `PendingConfirm` | `ChangesView.Revert` key `⌃⌥Z` |
| Rollback (`ChangesView.RevertFiles`) | **yes** — "If a file is added to Git but not committed, you can right-click it in the Commit window and choose **Rollback**" (set-up-a-git-repository.md:83) | bundle label `stage.revert.action.text=Rollback` | set-up:83; VcsBundle |
| Remove changelist | **yes** — the changelist is deleted, not just deselected | a three-way policy: `Show options` / `Remove silently` / `Do nothing` (`remove.changelist.combobox.*`, VcsBundle) | VcsBundle |
| Exclude line from commit | no — leaves the change pending | none | `Vcs.Diff.ExcludeChangedLinesFromCommit` |
| File deletion (by an external/IDE action) | **yes** | another three-way policy: `When files are deleted:` → `Ask` / `Remove silently` / `Do not remove` (`settings.when.files.are.deleted`, `radio.after.deletion.*`) | VcsBundle |
| Committing unsaved files | **yes, implicitly** — a commit can trigger a save | `Save Files During Commit` with `Save Now` / **`Postpone Save`** (VcsBundle:312-318) | VcsBundle:312-318 |

**The preflight pattern to copy is the policy-verb, not the dialog.** Rebased does
not ask "are you sure?" once; it asks per *class* of loss, and the class is a
setting with three values (ask / silent / nothing). Three separate instances:
changelist removal, file deletion, patch-created-in-explorer
(`show.patch.in.explorer.after.creation.*`). That is a better model than a single
global "confirm before destructive".

**A third confirmation that is easy to miss**: committing unsaved editor files
pops `Save Files During Commit` with the safe option *named as the deferral*
(`Postpone Save`, VcsBundle:318). TurboGit has no editor, so this is intentionally
absent — but the naming pattern (safe option = the thing that does nothing)
recurs.

### What can be undone, from where, and for how long?

- **Commit message, immediately after committing**: "You can edit your commit
  message by clicking **Edit Commit Message in the notification popup**"
  (tutorial:67). Also from the Log for any unpushed commit
  (commit-and-push-changes.md:67). So there is a post-commit reword window, and
  the bundle has an **undo** for it: `Commit message changed` (GitBundle:280),
  `Undo commit message edit failed` (GitBundle:281), `Cannot undo commit message
  edit` (GitBundle:283). Three separate strings for one feature = the undo is
  real and both its failure modes are named.
- **Undo Commit** on an unpushed commit: `Git.Uncommit` = "Undo Commit…",
  described as "Undo last commit and put its changes into selected changelist"
  (`actions-labeled.tsv`). Docs: "To undo the latest not pushed commit,
  right-click it on the Log tab … and select Undo Commit" (tutorial:122).
- **Boundary**: once pushed, the message edit and the undo are gone — `Edit
  Commit Message` "is only available for commits that haven't been pushed yet"
  (log-tab.md:295), and the bundle has `Commits from current branch have already
  been pushed to protected branch {0}` (GitBundle:259). **Unpushed is the undo
  boundary.**
- **Not undoable from this surface**: excluded lines (never committed),
  rolled-back changes, removed changelists.
- **Amend** produces "a single commit instead of two different ones"
  (edit-project-history.md:26) — the old commit ceases to exist, so amend is a
  rewrite with no undo offered at the point of use. The undo path is Undo Commit
  beforehand.

### Which options are on by default, and what is remembered between uses?

This surface has the richest default set in the whole product.

| Option | Default | Evidence |
| --- | --- | --- |
| Staging area vs changelists | **changelists** — "if you are more used to the concept of staging … press ⌘⌥S … and select **Enable staging area**" implies it starts off (line 172) | line 172; `settings.enable.staging.area` |
| Commit all when nothing staged | present, enable state UNVERIFIED (re-tested 2026-10-06 against commit-and-push-changes + settings-version-control-git: neither page carries this setting or states a default → R3) | `settings.commit.group.title`; `Git.Stage.ToggleCommitAll` |
| Close tool window after committing | on (you *clear* it) — line 145 | line 145 |
| Toggle commit controls | off (you *select* it) — line 149 | line 149 |
| Show diff on double-click | user chooses `Diff` or `Source` — lines 158-162 | lines 158-162 |
| Open diff as editor tab | on (you *clear* it) — line 168 | line 168 |
| Commit Checks (reformat, optimize imports, …) | **all off** — opt-in per commit | lines 44-56 |
| Sign-off | off | line 40 |
| Author override | off | line 38 |
| **Combine stashes and shelves in one tab** | off — you *enable* it (shelving:22-25) | shelving-and-unshelving-changes.md:22-25; `settings.enable.stashes.and.shelves=Combine stashes and shelves in one tab` |
| Warn if committing in detached HEAD or during rebase | present as a setting | `settings.detached.head` |
| Warn when committing files with names that might cause issues on other systems | present | `settings.bad.file.names` |
| Warn when committing files larger than N MB | present | `settings.large.files` + `settings.large.files.suffix=MB` |
| Warn if CRLF line separators are about to be committed | present | `settings.crlf` |
| Show Push dialog for Commit and Push | present as a setting | `settings.push.dialog` / `settings.push.dialog.for.protected.branches` |
| Clear initial commit message | present | `checkbox.clear.initial.commit.message=Clear &initial commit message` |
| Create changelists automatically | present | `settings.changelists.create.automatically.checkbox` |
| Allow putting changes within one file into different changelists | present — and it is the mechanism behind per-line commit | `settings.partial.changelists.enable.checkbox` |
| Run Git hooks | on (you *clear* it) — line 86 | line 86 |

Remembered between uses:

- **Commit message template**: "You can also define a commit template that will be
  used as the default commit message … `git config --local commit.template <path>`"
  (line 33) — a *git config* mechanism, not an app setting. **Worth copying: the
  template is the user's own file and the app just reads it.**
- **Message history** — the field offers "the list of recent commit messages"
  (line 30), so it is remembered per project.
- **Stage/mode choice is persisted** and switching is lossless: "When you switch
  from using changelists to the Git staging area, all existing changelists are
  saved. You can switch between the two modes **without losing your changes**"
  (line 175). That guarantee is the whole reason the toggle is safe to expose.
- **Git identity** is written once: "Git will store this information in
  `.git/config` in order to assign you as an author to your commits"
  (tutorial:65), via `Setting Git user name…` (GitBundle:1590).

### What feedback appears, and where does focus land?

- **Success**: "IntelliJ IDEA notifies you after a successful commit" (tutorial:66)
  — and the notification carries the `Edit Commit Message` action (tutorial:67).
  The post-commit notification is *interactive*, not a toast.
- **Untracked-file add**: "In the dialog that opens, click **Add** so that Git
  could start tracking the file" (tutorial:42) — a modal offer, not a silent add.
  And: "If you accidentally close this dialog, you can add the file to Git in the
  Project tool window: right-click the file and select Git | Add" (tutorial:44) —
  **the accidental-dismissal recovery is documented, which means it is a designed
  path.**
- **Identity prompt** before first commit, with two labels (`label.user.name=&Name:`,
  `label.user.email=&E-mail:` GitBundle:1580-1581).
- **Force-adding an ignored file**: "If you attempt to add a file that's on the
  .gitignore list, IntelliJ IDEA will suggest force-adding it. **Clicking Cancel in
  the confirmation dialog only cancels force-adding ignored files – all other
  files will be added**" (set-up-a-git-repository.md:78). A
  **partially-cancelling dialog** — Cancel does not cancel the batch. Easy to get
  wrong, explicitly stated.
- **Excluded changes**: "Unselected changes will stay in the current changelist, so
  that you can commit them separately" (line 114).
- **Commit-with-save** progress: `Save Files During Commit` (VcsBundle:312-318).

**Optimistic paint**: yes for the file list (rows repaint as you stage), no for
the commit itself — the commit is notification-gated.

Focus after the action:

- After Commit, the changed rows leave the list. Whether focus moves to the
  message field, the next row, or the tool window is **UNVERIFIED**; the window
  *closing* is a setting (line 145), so focus behaviour is necessarily
  setting-dependent and no single answer exists. Re-tested 2026-10-06 against
  commit-and-push-changes/undo-changes/managing-changelists: all silent on focus
  → needs runtime capture (R3).
- After the untracked-file dialog is cancelled, focus is in the Project view
  (tutorial:44).

## Flow — how tasks chain

### Entry points

1. `⌥0` / `View | Tool Windows | Commit` (tutorial:28).
2. `⌘K` — commit, with the active changelist pre-selected
   (commit-and-push-changes.md:25).
3. Gutter change-marker → inline commit bar (lines 120-124) — **commit without
   leaving the editor**.
4. `⌘⌥K` — Commit and Push (line 84).
5. As the `Local changes` tab of the Log tool window (line 151).
6. The modal commit dialog, if the Modal Commit Interface plugin is enabled
   (line 88).

### Internal sequence, with dialog chaining

**Plain commit:**

```
Commit tool window
  → tick file rows (⌘K ticks the whole active changelist)
  → type message  (⌃␣ completes file names)
  → [⌃O] options panel: Author / Sign-off / Commit Checks / Advanced / After Commit
  → Commit  (or Commit ▾ → Commit and Push / Commit and Rebase…)
      → (no identity set?)  Name/E-mail dialog                  [dialog]
      → (unsaved files?)     Save Files During Commit            [dialog]  ⌽ Postpone Save
      → (hooks fail?)         hook output surfaces
  → notification with "Edit Commit Message"
```

**Partial commit (chunks then lines):**

```
select file → ⌘D  (Diff Viewer)
  → tick each CHUNK's checkbox
  → (need one line?) right-click line → Split Chunks and Include Selected Lines into Commit
        or hover gutter → tick the line's own checkbox
  → Commit
  → "Unselected changes will stay in the current changelist"
```

(commit-and-push-changes.md:101-114.) The chain is file → chunk → line, three
granularities, and the third is reachable two different ways.

**Staging-mode partial commit:**

```
select file containing the change
  → context menu → Compare HEAD, Staged and Local Versions   [3-way diff]
  → edit the CENTRE pane to add exactly what you want staged
  → commit as normal
```

(commit-and-push-changes.md:186-189.) The staging model inverts the direction:
instead of *selecting* changes to commit, you *edit* the staged version. Same
outcome, opposite gesture.

**Commit and push:**

```
Commit ▾ → Commit and Push…  (or ⌘⌥K)
  → Push Commits dialog                                          [dialog]
      per-repository list, only the current repository selected by default
      commit list w/ per-commit checkboxes
      changed-files preview (⌘D per file)
      target remote (Define remote if none) → Define Remote dialog  [chained dialog]
      target branch (click-to-edit field; Edit all targets)
      [ ] Push tags: All | Current Branch
      Push ▾ → Push | Force push
  → (rejected) Push Rejected dialog                               [chained dialog]
      Update all repositories / affected only
      [ ] Remember the update method choice and silently update in the future
      Rebase | Merge
  → (protected branch) Force push unavailable
```

(commit-and-push-changes.md:191-232 + 234-245.) Three dialogs deep in the worst
case, and **each link in the chain is a real escape route**, not a dead end.

### What happens next on success

- Rows leave the tree; the notification offers `Edit Commit Message`
  (tutorial:66-67).
- With `Commit All when Nothing Is Staged` on, an empty selection still commits
  everything.
- After Commit and Push, the branch's outgoing counter decrements
  (manage-branches.md:11).

### What happens next on failure, and how the user recovers

| Failure | Evidence | Recovery |
| --- | --- | --- |
| No git identity | `Git User Name Is Not Defined`; `You need to specify username and email for Git` (GitBundle:1583-1585) | the Name/E-mail dialog; `Setting Git user name…` (GitBundle:1590); per-root variant GitBundle:1587 |
| Cannot set identity | `Could not set user.name and user.email` (GitBundle:1588) | **UNVERIFIED** — re-tested 2026-10-06 against commit-and-push-changes ("Set your Git username" documents terminal config only) and set-up-a-git-repository: no recovery route documented → R3 |
| Unsaved files | `Save Files During Commit` → `Save Now` / `Postpone Save` (VcsBundle:312-318) | both outcomes are non-fatal |
| Commit blocked by conflicts | banner `Cannot proceed due to merge conflicts` + `Resolve…` (VcsBundle:1010-1011) | resolve, then commit |
| Push rejected | Push Rejected dialog (commit-and-push-changes.md:236) | Rebase or Merge, optionally remembered |
| Push rejected on protected branch | "These choice options are only available if the current branch is not listed in the **Protected branches** field, otherwise, you can only perform the push operation" (line 232) | unprotect, or change the push strategy |
| Changelist name collision | `A changelist with that name already exists` / `Cannot create new changelist with empty name` (VcsBundle:601-602) | rename; validation is live |
| Shelve/stash during a failed operation | `Could not shelve local uncommitted changes:` (GitBundle:346) / `…stash…` (GitBundle:347) | the templated pair means the failure names which mechanism was used |

**A pattern in the failure strings worth naming**: GitBundle:346/347 and
413/414/424/447 are four *templated pairs* — `shelf` vs `stash`, `failed` vs
saved-to-shelf. The app does not know which mechanism it will use until it runs, so
it templates the message afterwards. That is the same problem
`CleanTreeMethod` creates in TurboGit.

### Escape / dismiss semantics

- The options panel (`⌃O`) is a toggle, so it never blocks.
- The untracked-file Add dialog is **cancelling but not neutral** — Cancel
  suppresses only the force-add of ignored files (set-up-a-git-repository.md:78).
- `Close Commit Dialog` (`Vcs.Commit.CloseDialog`) is its own action, so the
  surface has an explicit dismiss rather than relying on focus loss.
- The Push dialog's Force option is a **dropdown item, not a separate button**
  (line 231) — so force-push requires one extra deliberate step, and is removed
  entirely on protected branches.

## Divergences (intentional)

| Dimension | Rebased behaviour | TurboGit behaviour | Why |
| --- | --- | --- | --- |
| UI | Two commit models — **changelists** (default) and **staging area** — with a user-facing mode switch that is lossless in both directions (commit-and-push-changes.md:172-175) | Staging buckets only: `UNSTAGED` / `STAGED` / `Merge conflicts` / `Unversioned Files` (`crates/turbogit-ui/src/ui/commit_window.rs`) | Staging is the model a developer arriving from the CLI already has, and it composes with per-line staging. Changelists are a *grouping* concept layered on top; adopting it as a second peer mode doubles the surface's state space. Intent matched (group local work, commit separately) by the existing multi-root subgroups plus per-row checkbox selection. |
| UX | Per-line commit inclusion is a **three-member verb family** (include line / include whole changed line / exclude line) plus a split-chunks command (lines 111-112) | File/Hunk/Line granularity toggle, default Line, with `Enter`/`Esc` char-range staging (`crates/turbogit-app/src/granular.rs`, `state.rs`) | Same three granularities, same reach. A toggle is more discoverable than three sibling verbs, and char-range selection is strictly finer than "split chunks". |
| UX | `Commit All when Nothing Is Staged` is a *setting* (`Git.Stage.ToggleCommitAll`) | Has the same opt-in setting: `settings.commit_all_when_nothing_staged` (default off, `crates/turbogit-domain/src/model.rs:707`), surfaced in the settings modal (`settings_modal.rs:255`), gating empty selections with a "Nothing is staged — Commit will include every tracked change." note (`crates/turbogit-ui/src/ui/commit_window.rs:2203-2217`) | Empty-selection behaviour is a forgiveness decision. Now verified as a match in kind: both are opt-in settings. The only open question left is Rebased's default (row above). |
| UX | Post-commit notification carries `Edit Commit Message`; reword has its own undo with two named failure modes (GitBundle:280-283) | `Dialog::Reword` exists with a preflight, but the post-commit undo window is not established | **Open gap** — the undo *lifetime* (unpushed) is specified and cheap. See ticket 07. |
| UX | Commit options panel carries six checks in three groups, including IDE-authoring checks (reformat, optimize imports, update copyright) and FTP/SFTP upload | Not applicable | TurboGit is not an editor: no code style, no inspections, no deployment. Intentionally absent. The *structure* — a single options affordance behind one keystroke — is what parity requires, and that is preserved. |
| UI | Commit button has a **mac-specific label variant** in the bundle: `commit.action.name=Comm&it` vs `commit.action.name.mac=Commi&t` (GitBundle:47, 49) | Single label + `⌘K` | A platform detail with no bearing on an egui client. Noted because it proves the bundle ships platform-specific labels — so any parity claim about "the Commit button" must name the platform. |
| UX | Unsaved-file save prompt on commit (`Save Files During Commit`) | No editor, so no unsaved files | Intentionally absent. |
| UX | Force-adding a `.gitignore`d file prompts, and **Cancel only cancels that one sub-action** (set-up-a-git-repository.md:78) | `Unversioned Files` bucket stages directly | TurboGit has no force-add affordance to gate yet. If one is added, the partially-cancelling dialog semantics must be specified explicitly — recorded so it is not rediscovered as a surprise. |
| Flow | Commit tool window can be dissolved into a Log tab, floated, windowed, or made modal (lines 88, 144, 151) | Fixed left-side Commit panel, sub-tabs `LocalChanges` / `Shelf` / `Stash` | egui panels are not free-floating windows. Shape parity is unavailable; topology parity (changes tree + commit card + optional diff preview) is preserved. |
| UI | Shelf/Stash/Console sub-tabs, and an option to **merge** Stash and Shelf into one tab | `CommitSubTab::{LocalChanges, Shelf, Stash}` — the merge option is absent | Trivial to add, and the sub-tab set already matches. Recorded as a small partial. |
| UI | `Changes` is the name of the *default changelist* (tutorial:51) | The Commit tab is **labelled** "Changes" (`shell.rs`) | Name collision. A TurboGit user reading "Changes" in a Rebased screenshot would conclude it is the default changelist. Recorded so the assembled spec can disambiguate: TurboGit's tab label should be read as the *view*, and any changelist-like grouping should not reuse the word. |

## Needs runtime capture

1. ~~**Default state of `Enable staging area`**~~ **CLOSED by `undo-changes` (fetched 2026-10-05), no capture needed**: "By default, IntelliJ IDEA uses the changelists concept where modified files are staged automatically" (`docs: undo-changes`, Unstage files section) — stated as a default, not inferred. Corroborated by `docs: managing-changelists` ("Initially, there is a single default changelist called Changes"). Note the recovered `settings-version-control-git` page adds a hazard neither source had: "If you enable the staging area, all existing changelists will be deleted" — which *contradicts* the lossless-switch claim at commit-and-push-changes.md:175 quoted in "Remembered between uses" above.
2. **Default states of the six commit-options checkboxes.** The docs list them as
   available; none is marked on.
3. **Default of `Git.Stage.ToggleCommitAll`** and the exact behaviour when the
   selection is empty.
4. **Focus target after Commit** — necessarily setting-dependent (the window may
   close), and no source says what focus does in each case.
5. **The untracked-file dialog's full field list** — tutorial:42 says "click Add";
   whether it has a "do not ask again", a recursive-add toggle, or a
   gitignore-pattern field is undocumented.
6. **Row-level affordance inventory in staging mode** — whether the
   unstaged/staged split rows carry inline stage/unstage buttons, right-click only,
   or gutter chevrons.
7. **Changelist drag-reorder** — referenced indirectly by the silent-shelve
   instruction ("drag a file or a changelist from the Commit tab to the Shelf tab
   … wait for a second until it's activated", shelving:171), which implies a live
   drag with a delay, but the full drag grammar is undocumented.

# Branches popup

> Surface `branches-popup`. Filled by the agent pass from installed-bundle
> evidence plus verified documentation. Bundle wins on names/ids/keystrokes; docs
> win on UX and flow. See `research/evidence/rebased/DISTILL.md`.

## Sources

- https://www.jetbrains.com/help/idea/manage-branches.html, verified 200, 13864 chars → `docs-text/manage-branches.md`
- `actions-labeled.tsv`, action ids, labels, default keys
- `keymap-bindings.txt`, the shipped default accelerators
- `strings/GitBundle.properties`, the `branches.*`, `checkout.*`, `delete.branch.*`, `smart.operation.*` families

Docs resolve 1/1, so the flow half is fully evidenced. This is the richest UX
surface in the set: it owns the dirty-checkout decision, the tracking-divergence
decision, the immediate-delete-with-restore decision, the Recent cap, and
directory grouping.

## UI, what exists

### Action inventory (bundle)

11 actions harvested from the installed bundle.

| Action id | Label | Key |
| --- | --- | --- |
| `Git.Cleanup.Branches` | Clean Up Branches… | — |
| `Git.CompareWithBranch` | Compare with Branch or Tag… | — |
| `Git.CreateNewBranch` | New Branch… | ⌘⌥N |
| `Git.CreateNewBranch.FromCommit` | New Branch… | — |
| `Git.FindMergedLocalBranches` | Find Merged Local Branches | — |
| `Git.Log.Branches.Change.Branch.Filter` | Update Branch Filter in Log | — |
| `Git.Log.Branches.Change.Branch.Filter.On.Selection` | Update Branch Filter | — |
| `Git.Log.Branches.Navigate.Log.To.Branch.On.Selection` | Navigate Log to Branch Head | — |
| `Git.Log.Branches.Navigate.Log.To.Selected.Branch` | Navigate Log to Selected Branch Head | — |
| `Git.Rename.Local.Branch` | — | F2, ⇧F6 |
| `Git.ShowBranches` | Show Branches | — |

Two gaps the prefix missed, both first-class row verbs in this surface
(`actions-labeled.tsv`, corroborated by the bundle string table):

| Action id | Label | Key |
| --- | --- | --- |
| `Git.Checkout.Update` | Checkout and Update | — |
| `Git.Tag` | New Tag… | — |
| `Git.AddCommitToRemoteBranch` | Add Commits to Remote Branch… | — |

And the row-menu verbs, whose labels are in
`strings/GitBundle.properties` (the action table carries no text for them),
this is the **actual per-row menu inventory**, and it is longer than the four
documented items:

| Bundle key | Label | Evidence |
| --- | --- | --- |
| `branches.checkout` | Checkout | GitBundle:1108 |
| `branches.checkout.and.rebase.onto.current` | Checkout and Rebase onto Current | GitBundle:1112 |
| `branches.checkout.and.rebase.onto.branch` | Checkout and Rebase onto {0} | GitBundle:1113 |
| `branches.compare.with.current` | Compare with Current | GitBundle:1114 (region) |
| `branches.show.diff.with.working.tree` | Show Diff with Working Tree | GitBundle:1115 (region) |
| `branches.merge.into.current` | Merge into Current | GitBundle:1116 (region) |
| `branches.rebase.current.onto.selected` | Rebase Current onto Selected | GitBundle:1117 (region) |
| `branches.update` | Update | GitBundle:1118 (region) |
| `branches.new.branch.from.branch` | New Branch from {0}… | GitBundle:1126 |
| `branches.new.branch.from.branch.current` | New Branch from Current… | GitBundle:1127 |
| `branches.action.delete` | &Delete | settings dump (`&Delete`) |
| `branches.action.pull.into.branch.using.merge.selected` | Pull Into Selected Using Merge | settings dump |
| `branches.action.pull.into.branch.using.rebase.selected` | Pull Into Selected Using Rebase | settings dump |
| `branches.checkout.tag.or.revision` | Checkout Tag or Revision… | settings dump |
| `branches.action.delete` (remote) | Delete Remote {Branch\|Branches} | GitBundle:1434 |
| `branch.ui.handler.delete.all` | Delete All | GitBundle:1435 |

The Branches **pane** context menu (log-tab.md:83-129) adds `Show Diff with
Working Tree`, `New Worktree from {selected_branch}`, and `Compare with
{current_branch}`, the pane and the popup are near-identical surfaces with a
few pane-only additions.

### Topology

The popup is a **tree of four fixed top-level nodes plus a settings gear**
(manage-branches.md:59-70):

1. `Recent`, "shows up to five recently checked-out branches"
2. `Local Branches`, "lists all local branches"
3. `Remote Branches`, "shows all remote branches available after the latest fetch"
4. `Tags`, "lists all available tags that you can check out, compare with the
   working tree, merge, push, or delete"

The bundle confirms the node titles: `group.Git.Recent.Branch.title=Recent`
(GitBundle:713) and a per-repo variant `Recent Branches in {0}` (GitBundle:714).

Also on the popup chrome:

- **Incoming/outgoing counters on the branch name itself**. "The VCS widget also
  indicates the number of incoming commits that have not yet been fetched (blue
  arrow icon next to the branch name) and the number of outgoing commits (green
  arrow icon next to the branch name)" (manage-branches.md:11). Corroborated by
  the setting `advanced.setting.git.update.incoming.outgoing.info` = "Check for
  incoming and outgoing commits".
- **Search field** with `⌃F` to return to it: "While searching for a particular
  branch and navigating the list of branches, press Ctrl+F to move the focus back
  to the search field" (manage-branches.md:55).
- **Settings gear, upper-right**, opening four toggles (manage-branches.md:70;
  all four confirmed in the bundle as `action.git.branches.*`):
  - `Group by Directory` (GitBundle:1783)
  - `Group by Repository` (GitBundle:1784)
  - `Show Recent Branches` (GitBundle:1785)
  - `Show Tags` (GitBundle:1786)
- **Repository scope filter**`Actions` / `Repositories`
  (GitBundle:1256-1258, `action.git.branches.popup.filter.by.*`). In multi-root
  projects "branches are automatically grouped by repositories. To check out the
  necessary branch, in the VCS widget, choose the repository first"
  (manage-branches.md:99). The Log pane has a third state:
  `Common Remote Branches` when synchronous control is on, vs
  `Repositories | Remote Branches` when it is off (manage-branches.md:79).
- **Directory grouping** is automatic: "IntelliJ IDEA automatically groups
  branches by directory and stores them in expandable lists. In order for
  branches to be grouped by directory, prefixes in branches' names should be
  separated with forward slashes", example given: `jd/2023.1`
  (manage-branches.md:68-69).
- **Favourites pinned to the top**. "The main branch is marked as a favorite by
  default. Favorite branches are always displayed at the top" (line 50).

### State matrix

| State | Which region | What it shows | Source |
| --- | --- | --- | --- |
| default | row | a favourite star outline is **hidden**; it appears on hover | manage-branches.md:52 |
| hover | branch row | "hover over the branch name, and click the star outline that appears on the left" | manage-branches.md:52 |
| hover | branch row | name is copyable. "To copy the name of a branch, hover over the branch and press Ctrl+C" | manage-branches.md:46 |
| pressed | star | branch becomes a favourite; "select the branch you want to mark as favourite and press Space" | manage-branches.md:53 |
| selected | branch row | drives the incoming/outgoing counters and the row menu | manage-branches.md:11 |
| selected | dirty tree | the popup is replaced by a **decision dialog**, not a menu, see UX | manage-branches.md:105 |
| disabled | `Interactively Rebase from Here` | three documented reasons, surfaced in the **status bar on hover** rather than a modal | apply-changes-from-one-branch-to-another.md:124-132 |
| focus | search field | `⌃F` returns focus here from anywhere in the list | manage-branches.md:55 |
| focus | popup | the gear's toggles are the only persisted popup state | manage-branches.md:70 |
| progress | fetch | no progress state documented for the popup's fetch; fetch is exposed on the gear row / Branches pane | SOURCES Tier 3; `Git.Branches.Popup.Fetch` in `actions-labeled.tsv` |
| warning | widget | "Branches have diverged warning in the VCS widget … the root projects are on different branches" | manage-branches.md:177 |
| error | blocked op | `Cannot {0} because of unmerged files` | GitBundle:1426 |

## UX, how it behaves

### How is it reached, and what hides behind a secondary affordance?

Entry is the header widget: "all operations with branches are performed in the
VCS widget. To invoke it, in the main window header, click the widget with the
name of the branch that is currently checked out" (manage-branches.md:9). A
second entry is `Git | Branches…` (`actions-labeled.tsv`, `Git.Branches`,
label `_Branches…`, key `⌘⌥N` per the bundle's `Git.CreateNewBranch`). The third
is the Branches pane of the Git tool window (manage-branches.md:10).

Hidden behind secondary affordances:

- **The four node toggles** are behind the gear (manage-branches.md:70).
- **Compare-with-current and show-diff-with-working-tree** are row-menu items,
  not toolbar buttons (manage-branches.md:118, 128).
- **The unmerged-commit list after a delete** is a *link inside the notification*
  (manage-branches.md:165), the commits are not otherwise reachable.
- **Remote-branch removal after deleting a tracking local branch** is a second
  link in the same notification (manage-branches.md:166).
- **Single-click behaviour** of the Branches pane is a setting, not a default:
  "Branches Pane Settings. Select what action to perform on a single click:
  Update Branch Filter … Navigate Log to Branch Head" (log-tab.md:68-75). The
  corresponding action ids are `Git.Log.Branches.Change.Branch.Filter.On.Selection`
  and `Git.Log.Branches.Navigate.Log.To.Branch.On.Selection`, and the whole
  `group.Git.Log.Branches.Settings` group is in the bundle.
- **The restore link after closing a delete notification** is in the
  Notifications tool window (manage-branches.md:167).

### Which verbs are destructive, and what do they confirm or preflight?

Four distinct destructive decisions live on this surface. Each has a different
policy, and the *differences* are the specification.

**(a) Checkout over a dirty tree → Force vs Smart.** This is the reference case.

The trigger: "If your local changes are going to be overwritten by checkout,
IntelliJ IDEA displays a list of files that prevent you from checking out the
selected branch, and suggests choosing between Force Checkout and Smart Checkout"
(manage-branches.md:105).

- `Force Checkout`, "your local uncommitted changes will be overwritten, and
  you will lose them" (line 106). Bundle key
  `checkout.operation.force.checkout=&Force Checkout` (GitBundle:1340).
- `Smart Checkout`, "IntelliJ IDEA will shelve uncommitted changes, check out the
  selected branch, and then unshelve the changes. **If a conflict occurs during
  the unshelve operation, you will be prompted to merge the changes**"
  (line 107). **Smart Checkout can itself conflict, this is stated, not
  inferred.**

The dialog's north panel is templated, and the templates are in the bundle,
which is how we know the mechanism is *shelve* or *stash* by configuration, not
two hardcoded buttons:

- `smart.operation.dialog.north.panel.label.shelf.text` = "Your local changes to
  the following files would be overwritten by {0}. **{1} can shelve the changes,
  {0} and unshelve them after that.**" (GitBundle:426)
- `smart.operation.dialog.north.panel.label.stash.text` = the same with
  stash/unstash (GitBundle:427)
- button labels: `smart.operation.dialog.ok.action.shelf.description` = "Shelve
  local changes, {0}, unshelve" (GitBundle:428);
  `…stash.description` = "Stash local changes, {0}, unstash" (GitBundle:429)
- the destructive sibling: `smart.operation.dialog.operation.name.and.overwrite.local.changes`
  = "{0} and overwrite local changes" (GitBundle:430)
- the cancel: `smart.operation.dialog.don.t.operation.name` = "Don't {0}"
  (GitBundle:433)
- and the title/heading pattern: `smart.operation.dialog.smart.operation.name`
  = "Smart {0}" (GitBundle:432), `…git.operation.name.problem` = "Git {0} Problem"
  (GitBundle:431)

So the *names* `Force Checkout` and `Smart Checkout` are the *resolved* values of
templates parameterised on the operation name, which is why grepping the bundles
for "Smart Checkout" returns nothing. Record this: **the literal string is not in
the bundle; the template that produces it is.** Anyone re-harvesting must follow
the `smart.operation.dialog.*` family, not literal-grep.

The policy switch: "If you want to clean your working copy using stash instead of
shelve, go to the Version Control | Git settings page and choose **Stash** under
the **Clean working tree using** setting" (manage-branches.md:108). Bundle:
`settings.clean.working.tree=Clean working tree using:` and the two values
`local.changes.save.policy.shelve=Shelve` / `…stash=Stash`, plus
`settings.stash=Stash`. **Shelve is the default** (it is listed first and is the
mechanism the smart-checkout description leads with).

**(b) Checkout where a local branch already tracks the remote.** Three-way
decision, and the automatic case is stated first:

- "If no commits will be lost, and the local branch already tracks the remote,
  IntelliJ IDEA **automatically resets** the local branch to the remote branch and
  then checks it out." (manage-branches.md:85), **no dialog at all.**
- "If the local branch contains commits that can be lost because of the reset,
  IntelliJ IDEA will offer you to:" (line 87)
  - `Drop Local Commits`, "will drop your local commits, reset the local branch,
    and change tracking" (line 89)
  - `Rebase onto Remote`, "will rebase your local branch onto the remote branch,
    keep your local commits, reset the local branch, and change tracking" (line 91)

Both prompts are bundled, and the bundled form is better than the doc's prose
because it names the branch pair explicitly:
`branches.checkout.with.reset.local.has.more.commits` = "Local branch '{0}' has
commits that do not exist in '{1}'. **Rebase '{0}' onto '{1}', or drop local
commits?**" (GitBundle:1133);
`branches.create.with.reset.local.has.more.commits` = "Local branch '{0}' has
commits that do not exist in '{1}'. Do you want to drop local commits?"
(GitBundle:1135); `branches.drop.local.commits=Drop Local Commits`
(GitBundle:1136); `checkout.and.rebase=Rebase onto Remote` (GitBundle:1130).

**(c) Delete branch → immediate, no confirmation.** The sharpest policy on the
surface.

- "**Check out a branch that you are going to use for further work.**", a
  *precondition the user must satisfy first*, stated as a step
  (manage-branches.md:161). You cannot delete the branch you are on.
- "right-click the branch you want to delete and choose Delete" (line 163).
- "**it will still be deleted immediately (equivalent to the `git branch --D` or
  `git branch --delete --force` command)**, but the notification will also contain
  a link allowing you to view the unmerged commits" (line 165). Bundled
  corroboration: `delete.branch.operation.name=branch deletion` (GitBundle:1373)
  and `delete.branch.operation.unmerged.commits.were.discarded=Unmerged commits
  were discarded` (GitBundle:1356), the outcome is stated as a *fact about
  something that already happened*.
- The unmerged-commits viewer is its own dialog:
  `branch.not.fully.merged.dialog.title=Branch Was Not Fully Merged`
  (GitBundle:1467), body "The branch {0} was not fully merged to {1}. Below is the
  list of unmerged commits." (GitBundle:1469), and its recovery button
  `branch.not.fully.merged.dialog.restore.button=&Restore` (GitBundle:1468).

So: **no preflight, forced delete, post-hoc disclosure, one-click restore.** The
disclosure is retrospective by design.

**(d) Delete remote branch → confirmed.** Different policy from local delete, in
the same surface: `Delete remote {0,choice,1#branch|2#branches} {1}?`, a real
question (GitBundle:1436). Plus two follow-up questions:
`Delete tracking local branch {0} as well` (GitBundle:1437) and
`Delete tracking local {branches}` (GitBundle:1438), plus `Delete All`
(GitBundle:1435) and `Delete Remote {Branch|Branches}` (GitBundle:1434). The
count-pluralised question is the tell that this one asks and local delete does
not.

**(e) The hard block.** With unresolved conflicts, branch operations are refused
outright rather than confirmed: `Cannot {0} because of unmerged files`
(GitBundle:1426), and the notification `You need to resolve all merge conflicts
before {0}. After resolving conflicts do not forget to commit your files to the
current branch before switching to another branch.` with a `Resolve conflicts…`
action (GitBundle:1427-1428). Also `branch.ui.handler.merge.notification.description`
= "The following files have unresolved conflicts. You need to resolve them before
{0}." (GitBundle:1429) with title `Unresolved files remain.` (GitBundle:1430).

### What can be undone, from where, and for how long?

**Branch delete, the reference case for undo reach and lifetime.**

- Immediately: "After you have deleted a branch, a notification will be displayed
  in the bottom-right corner **from which you can restore the deleted branch**"
  (manage-branches.md:164).
- After the notification is dismissed: "If you've closed the notification and
  later decide you want to restore a deleted branch, the link will be available
  in the **Notifications tool window until you restart IntelliJ IDEA**"
  (line 167).

**Lifetime = until application restart.** Not until the branch list refreshes,
not until a session marker, restart. That is a hard, citable bound, and it is
the strongest single argument for TurboGit to have an equivalent.

Three distinct links, so three distinct recoveries:

1. **Restore the branch** (always, while the notification lives).
2. **View the unmerged commits**, only when the branch had unmerged work
   (line 165). The commits survive; the reference to them lives in that link.
3. **Remove the remote branch**, only when the deleted branch was tracking one
   (line 166): "If the deleted branch was tracking a remote branch, there will
   also be a link in this notification to remove the remote branch."

Rollback vocabulary also exists for the multi-root case, with a *reason* attached:
`delete.branch.operation.you.may.rollback.not.to.let.branches.diverge` = "You may
rollback (recreate {0} in these roots) not to let branches diverge."
(GitBundle:1358), and the partial-success report
`delete.branch.operation.however.branch.deletion.has.succeeded.for.the.following`
(GitBundle:1357).

**Not undoable:** the commit drops inside `Drop Local Commits` and inside
`Force Checkout`. Both are disclosed before the fact and neither offers a
post-hoc link. The difference is that `Drop Local Commits` is only reachable
through a dialog that named the losing commits, while `Force Checkout` names the
losing *files*.

### Which options are on by default, and what is remembered between uses?

| Option | Default | Evidence |
| --- | --- | --- |
| Clean working tree using | **Shelve** | manage-branches.md:108; `local.changes.save.policy.shelve` listed as `Shelve`, stash as the alternative |
| Group by Directory | on (auto), toggleable | manage-branches.md:68, 70; `action.git.branches.group.by.directory.text` |
| Show Recent Branches | on, toggleable | manage-branches.md:70; `action.git.branches.show.recent.text=Show Recent Branches` |
| Show Tags | on, toggleable | manage-branches.md:70; `action.git.branches.show.tags.text=Show Tags` |
| Group by Repository | on in multi-root | manage-branches.md:99; `action.git.branches.group.by.repository.text` |
| Recent node size | **hard cap of five** | manage-branches.md:61. "Recent branches node shows up to five recently checked-out branches" |
| Main branch favourited | **yes** | manage-branches.md:50, "The main branch is marked as a favorite by default" |
| Execute branch operations on all roots | off (multi-root opt-in) | manage-branches.md:175, "note that this option is only available if your project has multiple roots" |
| Check for incoming and outgoing commits | on (the counters are described unconditionally) | manage-branches.md:11; `advanced.setting.git.update.incoming.outgoing.info` |

Remembered between uses:

- **New branch dialog** remembers nothing itself, but **suggests prefixes as you
  type**: "Once you start typing a name for your new branch, IntelliJ IDEA will
  suggest relevant prefixes based on the names of existing local branches"
  (manage-branches.md:21). This is derived from the current branch set, not
  persisted.
- **`Checkout branch` checkbox state** is per-dialog, not persisted. Bundle key
  `new.branch.dialog.checkout.branch.checkbox=&Checkout branch` (GitBundle:1176)
  and title `branches.create.new.branch.dialog.title=Create New Branch`
  (GitBundle:1088). The docs say "make sure the Checkout branch option is
  selected if you want to switch to that branch" (lines 20, 28, 34), which
  describes the intent, not a default. **The default state of that checkbox is
  UNVERIFIED**; see capture list. Re-tested 2026-10-06 against manage-branches
  (the only docs page describing the dialog; it says only "make sure the Checkout
  branch option is selected if you want to switch", lines 20/28/34) and
  settings-version-control-git (no branch-creation settings): no default stated → R3.
- **Multi-root cross-repo branch creation** is an explicit dialog option:
  `new.branch.dialog.operation.create.description=Create new branches in other
  repositories` (GitBundle:1168), so "create in all roots" is opt-in per
  invocation, not a default.
- **Popup filter mode** (`Actions` vs `Repositories`,
  GitBundle:1256-1258) is a toggle, so it persists.
- **Update method for this surface**: `dialog.message.update.branch` (GitBundle:1658)
  "You are about to update {0} from {1}. Choose 'Merge' if you want to create a
  merge commit, 'Rebase' if you want to rebase onto {1}." The *persisted* default
  lives in settings as `settings.update.method=Update method:` with values
  `settings.git.update.method.merge=Merge` / `…rebase=Rebase`.

### What feedback appears, and where does focus land?

Progress:

- `Collecting unmerged commits…` while the delete's disclosure list is built
  (GitBundle:1349), **the app admits this is slow, after the fact.**
- `Restoring Branch {0}…` during a restore (GitBundle:1350).
- `Deleting {0}` for a remote branch (GitBundle:1375 region).

Success:

- `Deleted branch {0}` (GitBundle:1354) and a bold variant
  `<b>Deleted Branch:</b> {0}` (GitBundle:1355), the notification leads with the
  label, not a generic "done".
- `Checked out {0}` (GitBundle:1337), `Checked out new branch {0} from {1}`
  (GitBundle:1338), and a repository-scoped variant
  `checkout.operation.in={0}{1,choice,0#|1# in {2}}` (GitBundle:1339), the
  checkout confirmation names *which repository* when multi-root.
- `Branch {0} was created` (GitBundle, `checkout.new.branch.operation.branch.was.created`).
- Checkout on a clean tree: "a notification will pop up in the bottom-right
  corner of the IntelliJ IDEA window" (manage-branches.md:103), so even the
  success case is a notification, not just a repaint.

Failure, the multi-root pattern, and it recurs:

- `Could not {0} {1}` (GitBundle:1360), operation name and reference.
- `Branch {0} wasn't deleted` (GitBundle:1353).
- `Could not Restore {0}` (GitBundle:1352).
- `Error during rollback of branch deletion` (GitBundle:1351).
- `Cannot {0} because of unmerged files` (GitBundle:1426).
- **Partial success across roots**, with a divergence-prevention offer:
  `However, checkout has succeeded for the following {0,choice,1#repository|2#repositories}`
  (GitBundle:1347) paired with `You may rollback … not to let branches diverge`
  (GitBundle:1345, 1358). Docs state the policy: "If an operation fails at least
  in one of the repositories, IntelliJ IDEA prevents branches from diverging by
  suggesting that you roll back this operation in the repositories where it was
  successful" (manage-branches.md:176).

That last one is a genuinely good idea worth copying by intent: on multi-root
partial failure, offer to *undo the successful roots* so the roots do not end up
on divergent branches.

**Optimistic paint: no** for anything destructive. Checkout success is a
notification. Delete success is a notification carrying a restore link.

Focus after the action:

- After delete, the popup stays open on the same node set minus the deleted row
  (implied by the notification carrying the recovery, not a modal).
- After `Restore`, `Restoring Branch {0}…` then the row returns (GitBundle:1350).
- **UNVERIFIED**: where focus lands after a completed checkout, the popup
  closing, or staying with the new branch selected. Neither source states it.
  Re-tested 2026-10-06 against manage-branches/undo-changes: manage-branches
  documents only the success *notification* ("a notification will pop up in the
  bottom-right corner", line 103), silent on focus → needs runtime capture (R3).

## Flow, how tasks chain

### Entry points

1. Header branch-name widget click (manage-branches.md:9).
2. `Git | Branches…` main menu, `Git.Branches`, label `_Branches…`
   (`actions-labeled.tsv`).
3. Branches pane of the Git tool window (manage-branches.md:10; log-tab.md:22).
4. Row menu on a commit in the Log, for `Branch {branch_name}` / `Branches`
   (log-tab.md:321-323), the popup's actions are re-hosted on the commit row.

### Internal sequence, with dialog chaining

**Create:**

```
popup → New Branch from '{current}'  /  New Branch from {selected}…
  → New Branch dialog (title "Create New Branch")           [dialog]
      name field  (prefix suggestions from existing local branches, live)
      [ ] Checkout branch
  → create
  → (multi-root opt-in) create new branches in other repositories
```

Three documented starting points, one dialog (manage-branches.md:16-34):
from the current branch, from a selected local *or remote* branch
(`New Branch from Selected`), and from a selected **commit** in the Log. The
commit route is a separate action id, `Git.CreateNewBranch.FromCommit`, whose
description is "Create a new branch starting from the selected commit"
(`actions-labeled.tsv`), so the *start point is a parameter*, not a second
dialog. `action.Git.New.Branch.description=Select only one branch to proceed
create a new branch` (GitBundle:672) is the multi-selection guard.

**Checkout of a remote branch as a new local:**

```
popup → Remote Branches → {branch} → Checkout
  → (clean) create + track + checkout  → "Checked out new branch {0} from {1}"
  → (local exists, tracking matches, nothing lost) auto-reset, NO dialog
  → (local exists, commits would be lost)                   [dialog]
        "Local branch '{0}' has commits that do not exist in '{1}'.
         Rebase '{0}' onto '{1}', or drop local commits?"
        ├── Drop Local Commits
        └── Rebase onto Remote
```

(manage-branches.md:79-91.) Note the auto-reset branch: **a silent reset is
acceptable when nothing can be lost**, and that is a precondition the app can
evaluate, not something the user must approve.

**Checkout of a local branch with a dirty tree:**

```
popup → Local Branches → {branch} → Checkout   (or "Checkout and Update")
  → (clean, or changes don't conflict) checkout → notification "Checked out {0}"
  → (changes would be overwritten)                                     [dialog]
        north panel lists the blocking files
        "Your local changes to the following files would be overwritten by {0}.
         {1} can shelve the changes, {0} and unshelve them after that."
        ├── Force Checkout            → "and overwrite local changes"  (loss)
        ├── Smart Checkout            → shelve → checkout → unshelve
        │     └── (unshelve conflicts) → Conflicts dialog            [chained dialog]
        └── Don't {0}                 → cancel
```

(manage-branches.md:101-107 + GitBundle:426-433.) **The chain matters: Smart
Checkout can land the user in conflict resolution, on the way out of a checkout
dialog.** That is the flow fact a naive port misses.

`Checkout and Update` is the same path plus an update step: "To automatically
sync a branch with its remote version while switching, select Checkout and
Update" (manage-branches.md:98), with the update method taken from
`dialog.message.update.branch` (GitBundle:1658), a merge/rebase choice.

**Delete:**

```
popup → {branch} → Delete
  → (prerequisite: NOT the current branch)
  → IMMEDIATE delete, no confirm
  → notification "Deleted branch {0}"
       ├── Restore                                  (always, until restart)
       ├── view unmerged commits  → "Branch Was Not Fully Merged"   [dialog]
       │       └── &Restore
       └── remove the remote branch  (if it was tracking one)
```

(manage-branches.md:157-167 + GitBundle:1354-1358, 1467-1469.) A multi-select
delete additionally asks about tracking branches and offers `Delete All`
(GitBundle:1434-1438).

**Compare:**

```
popup → {branch} → Compare with Current
  → new tab listing commits in {branch} not in current
  → "Swap Branches" link swaps the base
  → ⌘A on the commits list → Changed Files pane
```

(manage-branches.md:114-122.)

```
popup → {branch} → Show Diff with Working Tree
  → Changes tool window, per-file status colour:
      grey  = only in the selected branch
      green = only in the current branch
      blue  = differs
  → "Swap Branches" link
  → ⌘D per file → Diff Viewer
```

(manage-branches.md:124-138.) The three-colour status grammar is a *contract*,
not decoration: it distinguishes "add", "delete" and "modify" without reading
direction.

**Multi-root divergence:**

```
popup → {branch} in repo A only → checkout
  → "Branches have diverged" warning in the widget
  → ignore, or disable "Execute branch operations on all roots",
    or check out the same branch name in the rest manually
```

(manage-branches.md:177-178.)

### What happens next on success

- Checkout: a notification, and the widget's branch name and counters update
  (manage-branches.md:11, 103).
- Create: the new branch appears under Local Branches; if `Checkout branch` was
  ticked, the widget switches to it (manage-branches.md:20).
- Delete: the row is gone; the notification is the only handle on the recovery
  (manage-branches.md:164).
- Smart Checkout: unshelve may conflict → Conflicts dialog
  (manage-branches.md:107) → the whole checkout flow resumes there.

### What happens next on failure, and how the user recovers

| Failure | Evidence | Recovery |
| --- | --- | --- |
| Unmerged files block the operation | `Cannot {0} because of unmerged files` (GitBundle:1426); "You need to resolve all merge conflicts before {0}" (GitBundle:1427) | `Resolve conflicts…` link in the same notification (GitBundle:1428) |
| Checkout fails, multi-root | `However, checkout has succeeded for the following {repositories}` (GitBundle:1347) | `You may rollback … not to let branches diverge` (GitBundle:1345, 1358); docs at manage-branches.md:176 |
| Delete fails | `Branch {0} wasn't deleted` (GitBundle:1353) | **UNVERIFIED**, no bundled recovery action; re-tested 2026-10-06 against manage-branches/undo-changes: docs cover success-with-restore only, never a failed delete → R3 |
| Restore fails | `Could not Restore {0}` (GitBundle:1352); `Error during rollback of branch deletion` (GitBundle:1351) | **UNVERIFIED**, re-tested 2026-10-06 against manage-branches/undo-changes: silent on restore failure → R3 |
| New branch creation fails | `Could not create new branch {0}` (GitBundle:1363 region) | error-during-rollback strings exist (`checkout.new.branch.operation.error.during.rollback`) but no user recovery documented |
| Revision not found | `Revision not found{0,choice,0#|1# in {1}}` (GitBundle:1334); `Could not checkout {0}` (GitBundle:1333/1336) | **UNVERIFIED**, re-tested 2026-10-06 against manage-branches/undo-changes: docs document checkout of tags/branches but no failure recovery → R3 |
| Rebase from here disabled | three reasons, in the status bar on hover (apply-changes-from-one-branch-to-another.md:124-132) | read the status bar; requires the Status Text widget enabled |

### Escape / dismiss semantics

- The smart-operation dialog's cancel is `Don't {0}`, the operation name
  repeated as a negative (GitBundle:433). Copying this: the cancel verb names
  what will *not* happen.
- `Abort Merge` / `Abort Rebase` remain reachable from the widget for unfinished
  operations (apply-changes-from-one-branch-to-another.md:59, 99).
- Delete has **no escape**, it is immediate by design. The recovery is
  post-hoc, and its lifetime is bounded by process restart.
- Blocked-by-conflicts operations are not dismissible into a force; they are
  refused, with a resolve link.

## Divergences (intentional)

| Dimension | Rebased behaviour | TurboGit behaviour | Why |
| --- | --- | --- | --- |
| UI | Popup is a floating panel anchored to the header widget; `Git.Branches` is a menu item | Branches is a full tab in the shell plus a separate status-bar popup (`crates/turbogit-ui/src/ui/branch_widget.rs`) | egui owns no native `NSMenu`/`NSPopover` equivalent with a live-updating tree and a gear menu. Match intent: same four nodes, same toggles, same accelerators, our own chrome. |
| UI | Remote branches grouped as `Common Remote Branches` (sync on) vs `Repositories \| Remote Branches` (sync off), one setting changes the node's *name and shape* (manage-branches.md:79) | Per-repo reveal via a remote rollup row; no setting-driven relabelling | A node that changes identity under a setting is a support burden. TurboGit's rollup keeps one shape and defers the difference to expansion. |
| UX | Local delete is immediate, forced, with post-hoc restore + unmerged-commits link (manage-branches.md:165) | `Operation::DeleteBranch` runs `branch_delete(..., true)`, force, but **gated behind a confirmation** that already discloses what an unmerged delete loses (`crates/turbogit-app/src/operation.rs`, and `BranchDeletePending`/`BranchDeleteUndo` in `state.rs`) | TurboGit keeps the force semantics and the disclosure, but asks *before* rather than telling *after*. The information is identical; the ordering is safer, and TurboGit already models a delete-undo window. Deliberate, documented departure. |
| UX | `Force Checkout` and `Smart Checkout` are the only two checkout resolutions, and the smart one's mechanism (shelve vs stash) is a global setting (`settings.clean.working.tree`) | `Operation::Merge` carries a per-invocation `clean: CleanTreeMethod` (`operation.rs`) | A global setting makes the destructive path's mechanism unpredictable across sessions. TurboGit makes it a per-dialog choice, which matches the docs' own framing that merge offers Smart merge as a suggestion (`apply-changes-from-one-branch-to-another.md:58`). Same items, saner scope. |
| Flow | Smart Checkout can conflict during unshelve and chains into conflict resolution (manage-branches.md:107) | Same chain exists via `CleanTreeMethod` + the conflict resolver | No divergence, recorded to make explicit that the chain is required, not optional. |
| UX | Recent node hard-capped at five (manage-branches.md:61); main branch favourited by default (line 50) | Recent group exists in the status-bar popup only (`branch_widget.rs`); no favourite flag in the tree model | **Open gap, not a settled divergence.** Both are cheap and specified; see ticket 04. Logged here so the table does not imply agreement. |
| UI | Directory grouping is automatic and optional per user setting; grouping key is the `/` prefix (manage-branches.md:68-69) | Directory grouping is automatic and **not** user-toggleable (`branches_tree.rs` `DirNode`) | Matching intent: the grouping rule matches, and the toggle is a one-line settings addition. Recorded as partial parity. |
| UX | Multi-root failure offers to roll back the successful roots so branches do not diverge (manage-branches.md:176; GitBundle:1345/1358) | `PendingConfirm` and `Affected::All` exist, but no cross-root divergence rollback offer | **Open gap.** The idea is worth copying; see ticket 06. |
| Flow | Rename offers `Unset upstream branch` in the same dialog, and warns that only the local branch is renamed (manage-branches.md:41-45) | `Dialog::RenameBranch` is a single input (`crates/turbogit-ui/src/ui/dialogs.rs`) | **Open gap**, the upstream-unset checkbox and the remote-creation consequence are both specified. See ticket 05. |

## Needs runtime capture

1. **Menu ordering within the row context menu.** `SOURCES.md` Tier 2 records
   that the popup builds actions dynamically via `ActionManager`, so ordering is
   absent from both the bundle and public source. This is the single largest
   evidence hole on the surface.
2. **Default state of the `Checkout branch` checkbox** in the New Branch dialog.
   The docs only say "make sure the … option is selected if you want to switch";
   they never say what it starts as. GitBundle:1176 proves the checkbox exists.
3. **Focus target after a successful checkout**, does the popup close?
4. **User-facing recovery for a failed branch delete or failed restore**
   (GitBundle:1352-1353), no bundled action, no documented route.
5. **The exact Recent-node overflow affordance**, the cap of five is documented
   (manage-branches.md:61) but not whether a sixth entry is hidden entirely or
   reachable via a "show all" affordance.
6. **The gear menu's own layout**, whether the four toggles are checkboxes in a
   menu or a small popover with separators.
7. **Whether the blocking-file list in the smart-operation dialog is scrollable
   and whether it is per-file or a count** when many files block.

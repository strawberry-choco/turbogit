# Remotes, fetch and push

> Surface `remotes-sync`. Filled by the agent pass from installed-bundle evidence
> plus verified documentation. Bundle wins on names/ids/keystrokes; docs win on
> UX and flow. See `research/evidence/rebased/DISTILL.md`.

## Sources

- https://www.jetbrains.com/help/idea/sync-with-a-remote-repository.html — verified 200, 6821 chars → `docs-text/sync-with-a-remote-repository.md`
- `actions-labeled.tsv`, `keymap-bindings.txt`
- `strings/GitBundle.properties` — the `push.*`, `update.*`, `remotes.*`, `fetch*` families
- Cross-read: `docs-text/commit-and-push-changes.md:191-251` (the Push Commits dialog and Push Rejected flow live there, not in the sync page)
- https://www.jetbrains.com/help/idea/settings-version-control-git.html and …/advanced-settings.html — fetched 2026-10-06 → `docs-text/settings-version-control-git.md`, `docs-text/advanced-settings.md` (settings semantics; state no checkbox defaults)

Docs resolve 1/1, and the bundle is unusually complete here: `push.*` alone
contributes 20 strings, and the **entire push-rejected recovery family** is
present — which is rare, and is why this surface can be fully specified without
runtime capture for its main flow.

## UI — what exists

### Action inventory (bundle)

7 actions harvested from the installed bundle.

| Action id | Label | Key |
| --- | --- | --- |
| `Git.Configure.Remotes` | Manage Remotes… | — |
| `Git.Fetch` | Fetch | — |
| `Git.Pull` | Pull… | — |
| `Git.Unshallow` | Unshallow repository | — |
| `Vcs.Push` | — | — |
| `Vcs.Push.Force` | — | — |
| `Vcs.Push.Simple` | — | — |

The push action trio is the important structural fact: **`Vcs.Push` (the dialog),
`Vcs.Push.Simple` (push without a dialog), and `Vcs.Push.Force` are three distinct
actions**, not one action with a mode. The settings tree confirms the split:
`settings.push.dialog=Show Push dialog for Commit and Push` and
`settings.push.dialog.for.protected.branches=Show Push dialog only when committing
to protected branches` — i.e. **whether the dialog appears is itself a setting**.

Three more actions in `actions-labeled.tsv` that belong to this surface:

| Action id | Label | Description |
| --- | --- | --- |
| `Git.Branches.Popup.Fetch` | — | — |
| `Git.PushUpToCommit` | Push All up to Here… | "Push all previous commits including the selected one" |
| `Git.Tag` | New Tag… | — |

`Git.PushUpToCommit` is a **partial push from the Log**: "select the last commit
you want to push and choose the `Push All up to Here…` option"
(commit-and-push-changes.md:206).

### Topology

**A. The widget's counters** — the surface's always-visible part: "The VCS widget
also indicates the number of incoming commits that have not yet been fetched (blue
arrow icon next to the branch name) and the number of outgoing commits (green arrow
icon next to the branch name)" (sync:9; manage-branches.md:11). Setting:
`advanced.setting.git.update.incoming.outgoing.info` = "Check for incoming and
outgoing commits".

**B. The Push Commits dialog** (commit-and-push-changes.md:191-232). Regions, in
the order the doc describes them:

1. **Repository list** — "showing all Git repositories (for multi-repository
   projects)"; **only the current repository is selected by default** (line 209-210)
2. **Commit list** — "all commits made in the current branch in each repository
   since the last push", with per-commit checkboxes (line 207-208)
3. **Right-hand preview pane** — "The right-hand pane shows the changes included in
   the selected commit" (line 218)
4. **Target panel**: remote name (click-to-change) and branch name
   (click-to-edit field) (lines 210, 215)

Bundle keys for the target panel, which carry real information:

- `push.dialog.target.panel.define.remote=Define remote` (GitBundle:899)
- `push.dialog.target.panel.add.remote=Add Remote` (900)
- `push.dialog.target.panel.can.t.push=Cannot push` (903)
- `push.dialog.target.panel.empty.repository=Empty repository` (904)
- `push.dialog.target.panel.new=New` (905)
- `push.dialog.target.panel.upstream.checkbox=Set upstream` (906)
- `push.dialog.target.panel.upstream.label=Upstream` (907)
- `push.dialog.target.panel.new.and.upstream=New & Upstream` (908)
- `push.dialog.push.tags=Push &tags` (896), with combo values
  `Current Branch` (897) and `All` (898)
- `push.dialog.preview.commits.before.push=For Commit and Push to non-protected
  branches, preview commits before push` (909) — a **preview preference scoped by
  branch protection**

So the target panel has **three** distinct target states, not two: an existing
remote, `New` (create the branch), and `New & Upstream` (create it and track it).
Plus `Empty repository` as a separate case.

**C. The Push Rejected dialog** (commit-and-push-changes.md:234-245): which
repositories to update (`Update all repositories` vs affected only), a
`Remember the update method choice and silently update in the future` checkbox, and
`Rebase` / `Merge` buttons.

**D. The Pull Changes dialog** (sync:36-59): a repository dropdown for multi-root
(sync:38), a remote list (sync:40), a branch picker (sync:42), and
`Modify options` (sync:44).

**E. The Update Project dialog** (sync:68-79): "Select the update type (this
strategy will be applied to all roots that are under Git version control)" with
three radio options, each documented with its git equivalent (sync:72-76).

**F. The Git Remotes dialog** — title `Git Remotes` (GitBundle:1524), two columns
`Name` and `URL` (GitBundle:1538-1539), with Add / Edit / Remove, plus
`push.local.history.system.label.before=Before push` /
`push.local.history.system.label.after=After push` (GitBundle:911-912) — **separate
fetch and push URLs per remote**, which is the dialog's reason for existing beyond
simple add/remove.

### State matrix

| State | Which region | What it shows | Source |
| --- | --- | --- | --- |
| default | repository list | **only the current repository selected** (commit-and-push-changes.md:209) | line 209 |
| default | commit list | only commits since the last push, per repository (line 207) | line 207 |
| default | target branch | a click-to-edit text field; "you can type an existing branch name, **or create a new branch**" (line 215) | line 215 |
| default | preview preference | `For Commit and Push to non-protected branches, preview commits before push` (GitBundle:909) | GitBundle:909 |
| hover | commit row | "You can press `⌃Q` for the selected commit to display extra info, such as the commit author, time, hash and the commit message" (line 211) | line 211 |
| pressed | remote name | the label becomes a picker: "click the repository name. You can either select an existing repository from the list or click `Define remote`" (line 210) | line 210 |
| pressed | branch name | "The label turns into a text field where you can type an existing branch name, or create a new branch" (line 215) | line 215 |
| selected | whole repository | "If you select an entire repository, **all files from all commits will be listed** in the right pane" (line 221) | line 221 |
| selected | file in preview | "If the same file was modified within several commits, it will only be listed **once** … and if you invoke the Diff Viewer for this file, **all changes will be zipped together**" (line 222) | line 222 |
| selected | commit by another author | "this commit is marked with an **asterisk**" (line 220) — "The asterisk next to a commit marks commits where the author and the user who committed the changes are different" (log-tab.md:142) | line 220; log-tab.md:142 |
| disabled | `Force push` | "only available if the current branch is not listed in the **Protected branches** field, otherwise, you can only perform the push operation" (line 232) | line 232 |
| disabled | push target | `Cannot push` (GitBundle:903) | GitBundle:903 |
| disabled | Push | `Nothing to update` (GitBundle:1330) | GitBundle:1330 |
| progress | update | `Updating…` (GitBundle:1324) | GitBundle:1324 |
| progress | fetch | `Fetching…` (GitBundle:1100) | GitBundle:1100 |
| progress | remote add/remove | `Adding Remote…` (GitBundle:1525, 902), `Removing {1 Remote\|2 Remotes}…` (1530), `Changing Remote…` (1533) | GitBundle:1525-1533 |
| progress | push | `Before push` / `After push` labels (GitBundle:911-912) — **two phases, both labelled** | GitBundle:911-912 |
| error | unfinished operation | three distinct refusals: "You have unfinished merge. These conflicts must be resolved before update." / "Unmerged files detected…" / "You have unfinished rebase process…" (GitBundle:1326-1328) | GitBundle:1326-1328 |
| error | skip a root | **per-root skip reasons**: `no tracked branch` / `detached HEAD` (GitBundle:566-567) | GitBundle:566-567 |
| empty | push | `Everything is up to date` (GitBundle:930) | GitBundle:930 |
| warning | fetch | `Pruned obsolete {1 reference\|2 references}: {0}` (GitBundle:86) | GitBundle:86 |

**The per-root skip reasons (GitBundle:566-567) are the strongest multi-root signal
in the whole bundle.** A multi-root update does not fail when one root cannot be
updated; it *skips that root with a stated reason*. `Operation::Fetch` already
carries `roots: Vec<RootId>` and a per-root report (`operation.rs`), so TurboGit's
shape is right — the skip *reasons* are the missing half.

## UX — how it behaves

### How is it reached, and what hides behind a secondary affordance?

Three entry points, and **the choice between them is the surface's core
distinction**:

| Verb | What it does | Entry |
| --- | --- | --- |
| **Fetch** | "all new data … is downloaded into your local copy. This new data is **not integrated into your local files**, and changes are not applied to your code" (sync:13) | `Git | Fetch`; the widget's corner button; or automatic |
| **Pull** | "you not only download new data, but also **integrate it into your local working copy**" (sync:34) | `Git | Pull` |
| **Update** | "fetches changes from all project roots and branches, and merges the tracked remote branches into your working copy (equivalent to pull)" (sync:64) | `Git | Update Project` or `⌃T` |

`fetch` is described as "a safe way to get an update" because it cannot touch the
working tree (sync:14) — **the docs rank the three verbs by safety**, and that
ordering is the specification.

Automatic fetch: "Enable automatic fetch of all remotes in your project … This
background task runs **every 20 minutes by default** (configurable via the
`git.update.incoming.info.time` registry key)" (sync:21-22).

Hidden behind secondary affordances:

- **All pull/rebase/ff-only/no-ff/squash/no-commit/no-verify flags** hide behind
  `Modify options` (sync:44). The default pull shows three fields.
- **`Update` vs `Pull` is itself a choice behind a menu**, and the whole
  Update Project dialog can be suppressed: "If you choose not to show the Update
  Project dialog in the future … go to the Version Control | Confirmation settings
  page, select `Update` under **Show options before**" (sync:77). The bundle has
  `settings.general.show.options.before.command.label=Show options before:` — a
  **general pattern**: any command can be set to show its options dialog or not.
- **Force push is a dropdown item on the Push button**, not a separate button
  (commit-and-push-changes.md:231).
- **The unmerged-commit list after a delete** is a link in a notification — see
  `02-branches-popup`.
- **Per-remote push vs fetch URLs** hide in the Git Remotes dialog's two columns
  (GitBundle:1538-1539).
- **`Define remote` replaces the remote name inline** when no remote exists
  (commit-and-push-changes.md:213; GitBundle:899) — the dialog adapts rather than
  erroring.

### Which verbs are destructive, and what do they confirm or preflight?

| Verb | Destructive | Confirmation | Evidence |
| --- | --- | --- | --- |
| Fetch | no — cannot touch the working tree | none | sync:14 |
| Pull / Update | **yes** — modifies the working copy | the Update Project dialog is a *strategy* choice, not a confirm; `Show options before` can suppress it | sync:68-77 |
| **Force push** | **yes** — "erasing its history and causing data loss" | **a dropdown item, plus it is removed entirely on a protected branch** (line 232). Under the hood it is `--force-with-lease`, "a safer option that helps you ensure you do not overwrite someone else's commits" (line 250) | lines 231-232, 250 |
| Push rejected → update | **yes** | the Push Rejected dialog; and it can be *silenced* — `Remember the update method choice and silently update in the future` (line 240) | lines 234-245 |
| **Remove a remote** | **yes** | **Yes.** Title `Remove {1 Remote\|2 Remotes}`; body `Remove {1 remote\|2 remotes} {0}?` (GitBundle:1528-1529) | GitBundle:1528-1529 |
| Add/change a remote | no | on failure only | GitBundle:1527, 1535 |
| Unshallow | **yes** — rewrites history depth | **UNVERIFIED** — the action exists (`Git.Unshallow`, "Unshallow repository") and the doc only says "You can fetch the rest of the history later by selecting `Git | Unshallow Repository`" (set-up:29). Re-tested 2026-10-06: set-up:29 is still the only mention in the whole doc set (a web search finds the same sentence echoed in Rider's per-hosting pages); no page documents whether Unshallow confirms → needs runtime capture (R3) | set-up:29 |
| Checkout-and-update | **yes** | inherits pull's risk | manage-branches.md:98 |

**The force-push design is the model worth copying.** Three layers of safety, in
order: (1) it requires a second deliberate gesture (a dropdown selection);
(2) it is *absent* on a protected branch, not merely warned about; (3) underneath
it is `--force-with-lease` rather than `--force`, so a concurrent push is detected
rather than clobbered. **And when even that fails, there is a distinct string**:
`Force-with-lease push {0} to {1} was rejected` (GitBundle:917) — the safest
operation has its own failure message, which means the app distinguishes "the
remote refused" from "your lease was stale".

**The remove-remote confirmation is a good small pattern**: the title and the body
are *the same sentence* with different pluralisation, and the body takes the
count-pluralised object first. There is exactly one question and no ambiguity about
how many objects are about to go.

### What can be undone, from where, and for how long?

- **Push cannot be undone.** The recovery is a *revert commit*, and the docs route
  it through a different surface: "To revert a pushed commit, right-click it on the
  Log tab of the Git tool window and select `Revert commit`" (tutorial:129). **Push's
  undo lives on the history-rewrite surface, not here.**
- **Force push cannot be undone at all** — the remote's old history is gone. Line 250
  is explicit that it "erases its history and causing data loss".
- **Fetch is undoable in the only sense that matters**: nothing was changed. The
  fetched data lives in a remote branch, so "Fetched changes are stored as a remote
  branch, which gives you a chance to review them before you merge them"
  (sync:14).
- **Pull/Update is undoable by `Abort Merge` / `Abort Rebase`** if it stopped at a
  conflict (apply-changes:59, 99).
- **`Remember the update method choice and silently update in the future` is itself
  a persistent-state change**, and the doc explains how to undo it: "To change the
  update strategy, **deselect** this option to invoke the Push Rejected dialog the
  next time push of the current branch is rejected, apply a different update
  procedure, and select the Remember the update method choice option once again"
  (line 242). So the escape is documented, and it is a settings round-trip rather
  than a per-push choice.
- **Not undoable**: remote removal, unshallow.

### Which options are on by default, and what is remembered between uses?

| Option | Default | Evidence |
| --- | --- | --- |
| Update method | **Merge** — "Merge the incoming changes into the current branch … This is equivalent to running `git fetch` and then `git merge`" (sync:72), listed first with `settings.git.update.method.merge=Merge` before `…rebase=Rebase` | sync:72; settings tree |
| Show Update Project dialog | **on** — you can opt out (sync:77) | sync:77 |
| Automatically open the `Update Info` tab after updating | **on** — "By default, the tab opens automatically. To change this behavior … clear the `Automatically open the 'Update info' tab after updating the project` checkbox" (sync:78) | sync:78; `advanced.setting.git.update.incoming.auto.open.enabled` |
| Pull remote | **`origin`** — "If you have several remotes defined for your project, select a remote from the list (**by default, it is origin**)" (sync:40) | sync:40 |
| Pull branch | "the remote branch tracked by the current local branch is selected" (sync:42) | sync:42 |
| Remember non-default pull branch | **yes** — "If you specify a different branch, IntelliJ IDEA will **remember your choice and display this branch by default in the future**" (sync:42) | sync:42 |
| Auto-update on push rejected | **off** — "provided that the `Auto-update if push of the current branch was rejected` option … **is not selected**", i.e. the dialog appears (line 236) | line 236; `settings.auto.update.on.push.rejected` |
| Push tags | **off** — "By default, when you perform the push operation, tags are not sent to the remote repositories, unless you select this checkbox" (line 225) | line 225 |
| Push tag scope when enabled | `Current Branch` (i.e. `--follow-tags`), with `All` (= `--tags`) as the alternative (lines 227-229) | lines 227-229; GitBundle:897-898 |
| Preview commits before push (non-protected) | **on by documented behaviour** — after Commit and Push "You will be able to review the current commit as well as all other commits before they are pushed to the remote", stated with no condition (commit-and-push-changes.md:84); settings-version-control-git describes the opt-outs but states no checkbox default | commit-and-push-changes.md:84; docs: settings-version-control-git |
| Fetch remote changes automatically | **off** — you enable it (sync:21) | sync:21 |
| Automatic fetch interval | 20 minutes | sync:22 |
| Fetch tags | `Auto` — the enum is `Never` / `Auto` / `Always` / `Sync` (`settings.git.fetch.tag.mode.*`), with `Auto` as the default-named member | settings tree |
| Show Push dialog for Commit and Push | **on by documented behaviour** — the Commit and Push action is documented as leading to review-before-push unconditionally: "You will be able to review the current commit as well as all other commits before they are pushed to the remote" (commit-and-push-changes.md:84); the settings page's "Otherwise, your changes will be pushed automatically" defines the opt-out without stating the default (docs: settings-version-control-git) | commit-and-push-changes.md:84; docs: settings-version-control-git |
| Use Safe Force Push | **on by documented behaviour** — force push "is equivalent to git push --force-with-lease" (commit-and-push-changes.md:231) and "Under the hood … IntelliJ IDEA performs the git push --force-with-lease operation" (:250), both unconditional; advanced-settings describes the toggle ("uses --force-with-lease … instead of just --force", docs: advanced-settings) but states no default — the unconditional main-page statement is only true if the setting defaults to on | commit-and-push-changes.md:231, 250; docs: advanced-settings |
| Use credential helper | present | `settings.credential.helper` |
| Check remote for incoming changes | present | `settings.git.check.remote.text` |
| Password storage | `In native Keychain` — "**only available for macOS and Linux**" (set-up:149) | set-up:149 |
| Unshallow on clone | off — an opt-in checkbox with a depth field (set-up:28) | set-up:28 |

Remembered between uses:

- **The pull branch choice is explicitly remembered** (sync:42) — one of the few
  places the docs say a *user's choice* persists rather than a setting.
- **The update method is persisted globally** and *also* written into the push
  rejection flow: "the `Auto-update if push of the current branch was rejected`
  checkbox … will be selected, and the applied update method will become the
  **default** one" (line 241). So the flow writes back to settings. Copying this
  means a user's one-off choice silently becomes their policy — worth doing
  deliberately, not by accident.
- **`Execute branch operations on all roots`** (multi-root branch control) and
  **`Select all repositories with new commits for push`**
  (`advanced.setting.vcs.push.all.with.commits`) — the second is the push-scoped
  sibling of the push dialog's default selection. **If that setting is on, the
  "only the current repository" default is overridden**, and the doc's statement
  about the default is conditional on it being off.
- **Credentials**: `In native Keychain` on macOS, `KeePass`, or
  `Do not save, forget passwords after restart` (set-up:147-155).

### What feedback appears, and where does focus land?

**The push notification family is the best-specified feedback in the whole
product** — ten distinct outcomes, all in `push.notification.description.*`
(GitBundle:913-930):

| Outcome | String |
| --- | --- |
| plain success | `Pushed {0} {1 commit\|2 commits} to {1}` (922) |
| success + one tag | `Pushed {0} {commits} to {1}, and tag {2} to {3}` (921) |
| success + many tags | `Pushed {0} {commits} to {1}, and {2} tags to {3}` (920) |
| success, tags only | `Pushed tag {0} to {1}` (919) / `Pushed {0} tags to {1}` (918) |
| new branch | `Pushed {0} to new branch {1}` (916) |
| new branch + one tag | `Pushed {0} to new branch {1}, and tag {2} to {3}` (915) |
| new branch + many tags | `Pushed {0} to new branch {1}, and {2} tags to {3}` (914) |
| force | `Force pushed {0} to {1}` (913) |
| up to date | `Everything is up to date` (930) |
| rejected by remote | `Push {0} to {1} was rejected by the remote` (927) / `Push of {0} was rejected by the remote` (928) |
| rejected, no target named | `Push to {0} was rejected` (929) |
| **lease rejected** | `Force-with-lease push {0} to {1} was rejected` (917) |
| rejected, update conflicted | `Push was canceled, because there were conflicts during the update. Check that conflicts were resolved correctly, and invoke push again.` (924) |
| rejected, update incomplete | `Push was canceled, because not all conflicts were resolved during the update. Resolve the conflicts and try pushing again` (926) |
| rejected, update failed | `Push was rejected, and update failed with an error` (925) |
| rejected, update cancelled | `Push was rejected, and update was canceled` (923) |

**Six distinct rejection notifications, and four of them tell the user what to do
next.** That is the standard: a rejection is a *state machine with named exits*,
not one error string. Note also that the rejection messages distinguish
"conflicts during the update" from "not all conflicts were resolved" — a distinction
most clients collapse.

Update/fetch progress and errors:

- `Updating…` (1324), `Fetching…` (1100), `Nothing to update` (1330),
  `Cannot update` (1325/564)
- `Pruned obsolete {1 reference\|2 references}: {0}` (86) — a **non-error side
  effect of fetch, reported anyway**
- `Cannot set an upstream branch` (1331) with a `Choose upstream branch` action (565)
- Three refusals for an unfinished operation, GitBundle:1326-1328, with the rebase
  one carrying an **actionable tail**: "Then you may **continue rebase**. You also
  may **abort rebase** to restore the original branch and stop rebasing" (1329)
- `Update Checks not Supported. Git 2.9+ Required` (1098) — a version gate
- Remotes dialog: `Cannot add remote {0} '{1}'` (1527), `Cannot change remote {0}
  to {1} '{2}'` (1535), `Cannot remove {1 remote\|2 remotes} {0}` (1532), and a
  composed form `{0} in {1}:\n{2}` (1537) — **an operation/error/root triple**, so a
  multi-root failure is reportable per root
- `operation was not executed` (1536) — a distinct outcome from "failed"

**Optimistic paint: no.** Push reports after the remote responds.

Focus after the action:

- The **Update Info tab opens automatically** by default after an update
  (sync:78) — "It lists all commits that were made since the last sync with the
  remote, and lets you review the changes the same way as in the Log tab". **So a
  successful update navigates the user to a review surface by default.** That is
  the single most surprising and most valuable behaviour on this surface.
- `Cannot set an upstream branch` offers `Choose upstream branch` (GitBundle:565).
- **UNVERIFIED**: focus after a successful push (the dialog closes; whether the Log
  is focused is not stated). Re-tested 2026-10-06: silent across sync /
  commit-and-push / settings pages → runtime-only (R3).

## Flow — how tasks chain

### Entry points

1. `Git | Fetch`, `Git | Pull…`, `Git | Update Project` (`⌃T`) — sync:17, 36, 68.
2. The widget's corner fetch button — sync:19.
3. `⌃⇧K` or `Git | Push` — commit-and-push-changes.md:202.
4. Branch popup → `Push` for any local branch that has a remote — line 204.
5. Log → `Push All up to Here…` for a partial push — line 206.
6. Branches pane context menu → `Update` / `Push`, **multi-selectable for batch** —
   log-tab.md:117-122.
7. `Git | Manage Remotes…` (add `⌥Insert`) — set-up:127-131.
8. The Log tab's Branches pane → `Edit Remote` / `Remove Remote` — set-up:133-136.
9. `Git | Unshallow Repository` — set-up:29.

### Internal sequence, with dialog chaining

**Fetch — the safe path, no dialog:**

```
Git | Fetch   (or the widget's button, or the 20-minute background task)
  → Fetching…                                    [progress]
  → "Pruned obsolete {N references}: {list}"     [if anything was pruned]
  → widget's incoming counter updates
```

**Update Project — the default sync, and it navigates:**

```
⌃T  (or Git | Update Project)
  → Update Project dialog                                          [dialog]
      Merge | Rebase | Reset to the Remote Branch
  → Updating…
  → the "Update Info" tab opens AUTOMATICALLY in the Git tool window
      (a Log-shaped review of everything committed since the last sync)
  → (filterable: "Filter 'Update Project' information by paths:")
```

(sync:64-79.)

**Push — the reference flow, three dialogs deep:**

```
⌃⇧K  (or Commit ▾ → Commit and Push)
  → Push Commits dialog                                            [dialog]
      repository list — only the current repo selected by default
      commit list, per-commit checkboxes
      ⌃Q on a commit → author / time / hash / message
      right pane: the selected commit's changed files (⌘D per file)
         a file touched by several commits is listed once, diffs zipped
      target remote  (click to change)
         └── no remote at all → "Define remote" replaces the name
                → Add Remote dialog: name + URL [+ Fetch remote]      [chained]
                    failure → "Could not add remote: {0}"
      target branch  (click to edit; Edit all targets link)
      [ ] Set upstream / New / New & Upstream
      [ ] Push tags:  Current Branch | All
      Push ▾ →  Push  |  Force push     (Force absent on protected branches)
  → (rejected) Push Rejected dialog                                  [chained]
        [x] Update all repositories        (else: affected only)
        [ ] Remember the update method choice and silently update in the future
        Rebase  |  Merge
  → (update conflicts)  Conflicts dialog                              [chained]
  → (still unresolved) "Push was canceled, because not all conflicts were
     resolved during the update. Resolve the conflicts and try pushing again"
  → success: "Pushed {N commits} to {branch}"
```

(commit-and-push-changes.md:191-245; GitBundle:899-930.)

**Partial push from the Log:**

```
Log → select the LAST commit you want pushed → Push All up to Here…
  → Push Commits dialog
      "there will be a list of all commits up to the selected commit hash"
  → Push
```

(line 206-208; `Git.PushUpToCommit.description` = "Push all previous commits
including the selected one".)

**Define a remote — inline, in place:**

```
Git | Push  (on a repo with no remotes)
  → Push Commits dialog, with "Define remote" where the remote name would be
  → click it → dialog: remote name + URL
      [ ] Fetch remote — "to also fetch the branch list and commit history
           right away" (set-up:121)
  → saved; editable later via Git | Manage Remotes
```

(set-up:112-122.) **The remote is defined in the flow that needed it, not in a
separate settings trip.** That is the design point.

**Shallow clone → later unshallow:**

```
Clone dialog → [x] Shallow clone with a history truncated to  [N] commits
  → Git | Unshallow Repository
```

(set-up:28-29.) The two halves of one journey, on two different surfaces.

**Multi-root sync, with per-root skips:**

```
Git | Update Project
  → strategy applies to ALL roots (sync:70)
  → for each root:
        no tracked branch  → skipped, reason "no tracked branch"      (GitBundle:566)
        detached HEAD      → skipped, reason "detached HEAD"          (GitBundle:567)
        otherwise          → updated
  → failures compose as "{operation} in {root}:\n{error}"            (GitBundle:1537)
```

**Divergence is the multi-root failure policy for branches, and it is the mirror
image of this one**: sync *skips* what it cannot do and reports; branch operations
*offer to roll back* what they did do (manage-branches.md:176). Two different
theories of failure on the same product, both deliberate.

### What happens next on success

- **Update**: the `Update Info` tab opens automatically and lists everything
  committed since the last sync, reviewable like the Log (sync:78). Pressing `⌘A`
  with the caret in the commit list shows all files modified since the last update
  (sync:79).
- **Push**: the outgoing counter decrements (manage-branches.md:11); the dialog
  closes; the notification states the commit count and target.
- **Fetch**: only the incoming counter changes.
- **Define remote**: it is saved and editable later via `Git | Manage Remotes`
  (set-up:213).

### What happens next on failure, and how the user recovers

| Failure | Evidence | Recovery |
| --- | --- | --- |
| Push rejected, nothing attempted | `Push to {0} was rejected` (GitBundle:929) | the Push Rejected dialog, unless auto-update is on (line 236) |
| Rejected by the remote | `Push {0} to {1} was rejected by the remote` (927) | as above |
| **Force-with-lease rejected** | `Force-with-lease push {0} to {1} was rejected` (917) | fetch and retry — the lease, not the push, is stale. **The message distinguishes this from an ordinary rejection**, so the client must too |
| Update during rejection hit conflicts | `Push was canceled, because there were conflicts during the update. Check that conflicts were resolved correctly, and invoke push again.` (924) | resolve, then push again |
| Update left conflicts unresolved | `Push was canceled, because not all conflicts were resolved during the update. Resolve the conflicts and try pushing again` (926) | the Conflicts dialog |
| Update itself errored | `Push was rejected, and update failed with an error` (925) | **UNVERIFIED** — no bundled next step. Re-tested 2026-10-06: docs silent on this outcome (commit-and-push, sync pages) → runtime capture (R3) |
| Update cancelled by the user | `Push was rejected, and update was canceled` (923) | re-push |
| No upstream configured | `Cannot set an upstream branch` + action `Choose upstream branch` (GitBundle:1331, 565) | the action |
| Unfinished merge / unmerged files | three refusals, GitBundle:1326-1328 | resolve first |
| Unfinished rebase | GitBundle:1328, with the tail "Then you may **continue rebase**. You also may **abort rebase**…" (1329) | two named actions |
| A root has no tracked branch | `no tracked branch` (GitBundle:566) | **skipped, not failed** — the root is named in the report |
| A root is in detached HEAD | `detached HEAD` (GitBundle:567) | **skipped** |
| Add remote failed | `Could not add remote: {0}` (GitBundle:901) / `Cannot add remote {0} '{1}'` (1527) | fix the name/URL inline; the dialog stays |
| Remove remote failed | `Cannot remove {1 remote\|2 remotes} {0}` (GitBundle:1532) | **UNVERIFIED** — re-tested 2026-10-06: set-up's remote sections (Define a remote / Add a second remote / Remove Remote) are silent on failure recovery → runtime capture (R3) |
| Change remote failed | `Cannot change remote {0} to {1} '{2}'` (GitBundle:1535) | **UNVERIFIED** — re-tested 2026-10-06: same silence; the only documented edit routes are the Branches-pane `Edit Remote` and the Push dialog's remote-name click (set-up:134-136) → runtime capture (R3) |
| Git too old for update checks | `Update Checks not Supported. Git 2.9+ Required` (GitBundle:1098) | upgrade git |
| Nothing to do | `Nothing to update` (1330) / `Everything is up to date` (930) | — |

### Escape / dismiss semantics

- The Push dialog's Force option is a **dropdown item**, so plain `Push` is the
  default and force requires a second gesture (line 231).
- `Remember the update method choice and silently update in the future` is the
  escape from a *future* dialog, and it is reversible only through settings
  (line 242) — so the dismiss has a durable side effect and the doc says so.
- The Pull and Update dialogs are suppressible via `Show options before`
  (sync:77), so "cancel the dialog" and "stop asking" are separate choices.
- `Abort Merge` / `Abort Rebase` remain the exits for a sync that stopped at a
  conflict (apply-changes:59, 99; GitBundle:1329).

## Divergences (intentional)

| Dimension | Rebased behaviour | TurboGit behaviour | Why |
| --- | --- | --- | --- |
| UI | Push has **three distinct actions** — `Vcs.Push` (dialog), `Vcs.Push.Simple` (no dialog), `Vcs.Push.Force`; and whether the dialog appears is a *setting* (`settings.push.dialog`) | `Dialog::Push` is a dialog; `Vcs.Push.Simple` has no TurboGit equivalent as a separate affordance | **Partial gap.** The dialog/no-dialog distinction is worth having for the common "just push" case, and it is a small addition. `Vcs.Push.Force` maps onto TurboGit's existing `--force` option in `Dialog::Push`. See ticket 13. |
| UX | Push is reviewed in a dialog by default, and the **preview is scoped by branch protection** — "For Commit and Push to non-protected branches, preview commits before push" (GitBundle:909) | `Dialog::Push` always previews, with a "Preview dry-run" action running real `git push --dry-run` (`crates/turbogit-ui/src/ui/push_dialog.rs`) | TurboGit's dry-run is *stronger* evidence than a preview list — it reports what the remote would actually do. The protection-scoped skip is worth adding as an option, since a protected-branch push is the case where a preview matters least. |
| UX | Push dialog defaults to **only the current repository**, overridable by a setting (`advanced.setting.vcs.push.all.with.commits`) | `Dialog::Push` offers a PUSH SCOPE segmented control (ThisRepo / Selected / Subtree / All) with a default | Same capability, more explicit. TurboGit's segmented control names the scope in the UI rather than relying on a settings toggle — strictly more discoverable. |
| UX | A rejected push offers `Remember … and silently update in the future`, which **writes the choice back into settings** and becomes the default (line 241) | `Dialog::Push` handles rejection via a remediation banner | The recovery is specified and TurboGit's banner is the same idea. **The write-back-to-settings half is an open gap** and it is worth doing deliberately, because a user who picks Rebase once may not mean it forever. See ticket 14. |
| UX | Six distinct rejection notifications, four with an explicit next action (GitBundle:923-929) | Not established at this granularity | **Open gap, and the highest-value one on this surface.** Distinguishing "lease rejected" from "rejected by the remote" from "update conflicted" from "update incomplete" changes what the user does next. All six are specified. |
| UX | Multi-root sync **skips** unupdatable roots with a named reason (`no tracked branch`, `detached HEAD`; GitBundle:566-567) | `Operation::Fetch { roots: Vec<RootId> }` iterates roots; a per-root report exists | The *shape* matches. The missing half is the **skip-reason vocabulary** — a bare "failed" is strictly less actionable. **Open gap**, cheap. |
| UX | Multi-root **branch** failure offers to roll back the roots that succeeded, "not to let branches diverge" (manage-branches.md:176; GitBundle:1345, 1358) | `PendingConfirm` + `Affected::All` exist; no cross-root divergence rollback | **Open gap**, and it is the more important of the two multi-root policies: divergence is unrecoverable without manual repair. See ticket 06. |
| UX | Credentials via Keychain / KeePass / don't-save (set-up:147-155) | Not implemented (no credential store) | **Intentionally absent** — a credential store is a security surface with its own threat model, out of scope for a parity pass. Recorded so the gap is a decision, not an oversight. |
| UX | Automatic fetch every 20 minutes, configurable by a registry key (sync:21-22) | `incoming_poll.rs` exists in `crates/turbogit-app/tests/` | The mechanism appears to exist. The *interval* and the "all remotes" scope need confirming against TurboGit's own settings rather than copied. **Not a divergence; a check.** |
| UI | Fetch reports pruned obsolete references as a notice (GitBundle:86) | Not established | **Open gap, small.** A prune is invisible to the user otherwise, and `--prune` changes the local ref set. |
| UX | Unshallow is a menu item with no documented confirmation (set-up:29) | Not implemented | **Open gap, and the doc's silence is not permission to skip the confirmation** — unshallow rewrites a shallow clone into a full one, which is a large, slow, hard-to-reverse operation on a big repository. Any TurboGit implementation should confirm. |
| UI | "New", "New & Upstream", "Set upstream", "Upstream", "Empty repository" are five distinct target-panel states (GitBundle:903-908) | `Dialog::Push` has a Remote/Branch field pair plus `--set-upstream` as an option | The states exist in TurboGit's model but are not *presented* as named target modes. **Open gap, small** — worth naming the three target states explicitly so the user knows whether they are creating or overwriting. |

## Needs runtime capture

1. **The checkbox states behind `Show Push dialog only when committing to
   protected branches`, `Show Push dialog for Commit and Push`, `Use Safe Force
   Push`, and `preview commits before push`.** The docs state the *behaviour*
   unconditionally, so the effective default for the last three is on (see the
   defaults table), but settings-version-control-git and advanced-settings
   state no checkbox default — capture the states, not the behaviour.
2. **The `Fetch tags:` default** — the enum is `Never` / `Auto` / `Always` /
   `Sync` (`settings.git.fetch.tag.mode.*`) and the member named `…mode.default=Auto`
   suggests but does not prove `Auto`.
3. **The target-panel's `Empty repository` state** (GitBundle:904) — what it looks
   like and whether it is an error or a normal first-push state.
4. **Whether `Update all repositories` in the Push Rejected dialog is on or off by
   default** — the doc says "If this option is cleared, only the affected
   repositories will be updated" (line 238), which implies it starts checked.
5. **The exact set and order of rows in the Git Remotes dialog** — the two columns
   are known (`Name`, `URL`) but the per-row actions and their order are not.
6. **Whether `Unshallow repository` confirms, and what it shows about the size of
   the operation** before starting.
7. **Focus after a successful push** — whether the dialog closes and where focus
   lands. Contrast with the Update flow, which is specified to open a tab.

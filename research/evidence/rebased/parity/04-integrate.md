# Integrate: merge, rebase, cherry-pick

> Surface `integrate`. Filled by the agent pass from installed-bundle evidence plus
> verified documentation. Bundle wins on names/ids/keystrokes; docs win on UX and
> flow. See `research/evidence/rebased/DISTILL.md`.

## Sources

- https://www.jetbrains.com/help/idea/apply-changes-from-one-branch-to-another.html — verified 200, 22307 chars → `docs-text/apply-changes-from-one-branch-to-another.md`
- https://www.jetbrains.com/help/idea/investigate-changes.html — verified 200, 14274 chars → `docs-text/investigate-changes.md`
- `actions-labeled.tsv`, `keymap-bindings.txt`
- `strings/GitBundle.properties` — the `merge.option.*`, `pull.option.*`, `rebase.option.*`, `abort.operation.*`, `git.status.bar.widget.*` families
- Cross-read: `docs-text/resolve-conflicts.md` (the conflict hand-off), `docs-text/sync-with-a-remote-repository.md` (update vs pull)

Docs resolve 2/2, and this is the best-evidenced surface for **option
vocabulary**: every flag in all three dialogs has a bundle label stating its
effect in plain words. The option inventory below is therefore complete, not
sampled.

## UI — what exists

### Action inventory (bundle)

10 actions harvested from the installed bundle.

| Action id | Label | Key |
| --- | --- | --- |
| `Git.AddCommitToRemoteBranch` | Add Commits to Remote Branch… | — |
| `Git.CherryPick.Abort` | Abort Cherry-Pick | — |
| `Git.CherryPick.Continue` | Continue Cherry-Pick | — |
| `Git.Merge` | Merge… | — |
| `Git.Merge.Abort` | Abort Merge | — |
| `Git.Merge.Commit` | Commit merge | — |
| `Git.Rebase` | Rebase… | — |
| `Git.Rebase.Abort` | Abort Rebase | — |
| `Git.Rebase.Continue` | Continue Rebase | — |
| `Git.Rebase.Skip` | Skip Commit | — |

Plus `Vcs.CherryPick` in `actions-labeled.tsv`.

**Five of the ten are operation-lifecycle verbs, not integrations** — Abort /
Continue / Skip for three operations, plus `Commit merge`. That ratio is the
finding: **this surface's real job is managing operations that failed partway**,
not starting them. The starting verbs are one per operation.

Bundle labels for the lifecycle:

- `abort.operation.cherry.pick.name=cherry-pick` (GitBundle:11) — so `Abort {0}`
  is templated on the operation name
- `git.status.bar.widget.text.cherry-pick=Cherry-picking in {0}` (981) — **the
  status bar is the in-progress indicator**, per root
- `progress.text.cherry.picking.commits=Cherry-picking commits in memory…` (749)
- `notification.title.add.to.remote.branch.conflict=Cherry-pick conflict` (752),
  `notification.content…=Failed to cherry-pick commits onto {0}:\n{1}` (753)

### The five integration verbs (from the doc's own framing)

apply-changes:8-18 lists the whole family, and it is **five verbs, not three**:

1. Merge branches
2. Rebase branches
3. Cherry-pick separate commits
4. **Apply separate changes from a commit**
5. **Apply specific file to a branch**

Verbs 4 and 5 are the ones usually forgotten, and they are what makes this a
*surface* rather than three dialogs.

### Option vocabulary — complete, from the bundle

**Merge** (`Modify options`, popup title `Add Merge Options`, GitBundle:119-120):

| Flag | Bundle label |
| --- | --- |
| `--no-ff` | "Create a merge commit even if it can be fast-forwarded" (121) |
| `--ff-only` | "Merge only if it can be fast-forwarded" (122) |
| `--squash` | "Create a single commit for all merged changes" (123) |
| `-m` | "Specify a message for the merge commit" (124) |
| `--no-commit` | "Do not commit the merge result" (125) |
| `--no-verify` | "Bypass the pre-merge and commit message hooks" (126) |
| `--allow-unrelated-histories` | "Allow merging histories that do not share a common ancestor." (127) |

**Pull** (popup title `Add Pull Options`, GitBundle:149-154) — note it is a
**different flag set from merge**, not a subset:

| Flag | Bundle label |
| --- | --- |
| `--rebase` | "Rebase the current branch on top of incoming changes" (154) |
| `--ff-only` | "Merge only if it can be fast-forwarded" (150) |
| `--no-ff` | "Create a merge commit even if it can be fast-forwarded" (151) |
| `--squash` | "Create a single commit for all pulled changes" (152) |
| `--no-commit` | "Merge, but do not commit the result" (153) |

**No `--no-verify` and no `--allow-unrelated-histories` on pull.** Five flags, not
seven. And the wording differs where the semantics differ: merge says "for all
**merged** changes", pull says "for all **pulled** changes"; merge's `--no-commit`
is "Do not commit the merge result", pull's is "**Merge**, but do not commit the
result".

**Rebase** (dialog title `Add Rebase Options`, GitBundle:200-208):

| Flag | Bundle label |
| --- | --- |
| `--onto` | "Specify a new base for the rebased commits" (201) |
| `--interactive` | "Edit commits before rebasing" (202) |
| `--update-refs` | "Auto-update branches pointing to rebased commits" (203) |
| `--rebase-merges` | "Recreate commits topology" (204) — the vaguest label in the set |
| `Select another branch to rebase` | (205) |
| `--keep-empty` | "Do not remove empty commits during rebase" (206) |
| `--root` | "Rebase all commits in the branch" (207) |
| `--autosquash` | "Auto-squash commits marked for fixup or squash" (208) |

**Every flag label states its effect, not its name.** `rebase.option.rebase.merges`
= "Recreate commits topology" is the exception, and it is the flag with the
scariest documented consequence ("any resolved merge conflicts or manual
amendments in these merge commits will have to be resolved or reapplied
manually", apply-changes:87). **The vaguest label is on the most destructive
flag** — worth doing better.

**Cherry-pick settings** (a fourth vocabulary, GitBundle:839-842):
`settings.cherry.pick=Cherry-Pick`, `settings.cherry.pick.empty.commit.group=On empty cherry-pick commit:` with values `Skip commit` / `Create empty commit`. **A policy for a case most clients leave to git's default.**

Plus `settings.add.suffix=Add the 'cherry-picked from <hash>' suffix when picking
commits pushed to protected branches` (793) — again a per-operation suffix.

### Topology

**Three dialogs, each = one field set + `Modify options`.**

**Merge** (apply-changes:39-55): branch picker → `Modify options` → the seven flags
above → `Merge`. The dialog title is not in the bundle's harvested set, but the
doc calls it "the Merge dialog" (line 39).

**Rebase**: see `06-history-rewrite` for the full field list, the `--onto` pair, and
the per-field validation strings.

**Pull** (sync:36-59): repository dropdown (multi-root) → remote list (default
`origin`) → branch picker (default: the tracked remote branch) → `Modify options`
→ the five flags → `Pull`.

**Two dialogs that have no flags at all** and are pure confirmations:

- **`Get from Branch`** — no dialog; "IntelliJ IDEA will copy the entire contents of
  the file to the current branch" (apply-changes:287)
- **`Cherry-pick`** — no dialog on the happy path; "Click Cherry-pick on the
  toolbar. IntelliJ IDEA will apply and commit changes to the target branch"
  (apply-changes:244)

**The four verb-specific row-menu items** that skip the dialog entirely
(apply-changes:100-106) — these are the fast paths:

| Verb | Availability | Bundle |
| --- | --- | --- |
| `Checkout and Rebase onto <current_branch_name>` | remote **and** local | `branches.checkout.and.rebase.onto.branch=Checkout and Rebase onto {0}` (1113) |
| `Rebase <current_branch_name> onto <selected_branch_name>` | remote **and** local | `branches.rebase.current.onto.selected=Rebase Current onto Selected` (1117) |
| `Pull into <current_branch_name> Using Rebase` | **remote only** | `branches.action.pull.into.branch.using.rebase.selected` |
| `Pull into <current_branch_name> Using Merge` | **remote only** | `branches.action.pull.into.branch.using.merge.selected` |
| `Merge <selected_branch> into <current_branch>` | — | `branches.merge.into.current=Merge into Current` (1116) |
| `New Worktree from <selected_branch>` | — | log-tab.md:108 |

The asymmetry is the spec: **local branches can be rebased *onto* and *from*, but
"Pull into … Using X" is remote-only** — because pull requires a remote.

### State matrix

| State | Which region | What it shows | Source |
| --- | --- | --- | --- |
| default | merge dialog | `Modify options` collapsed; the seven flags hidden | apply-changes:40 |
| default | pull dialog | remote `origin`; the tracked remote branch | sync:40, 42 |
| default | options popup | every flag off | inferred from `merge.option.*` having no default marker — **UNVERIFIED**; re-tested against apply-changes + settings-version-control-git/advanced-settings 2026-10-06: no page states a default for any `Modify options` flag → runtime capture (R3) |
| hover | flag row | the label *is* the explanation — no tooltip needed | GitBundle:121-127 |
| disabled | `Cherry-pick` | "This button is disabled if the selected commit is already contained in the current branch" (log-tab.md:221) | log-tab.md:221 |
| disabled | `Checkout and Rebase onto …` | "If the remote branch doesn't exist locally, IntelliJ IDEA will **silently create a tracked local branch**, checkout into it and rebase" (apply-changes:102) — so the local case is *not* a precondition, it is created | apply-chances:102 |
| disabled | any rewrite verb | `Cannot {0} during cherry-pick / merge / rebase / revert` (GitBundle:254-257) | GitBundle:254-257 |
| disabled | any rewrite verb | `Selected {N commits} {is\|are} from different repositories` (251-252) | GitBundle:251-252 |
| focus | merge dialog | **the centre pane of the Merge Revisions dialog is a fully-functional editor** (tutorial:167) | tutorial:167 |
| selected | a file in Changed Files | enables `Cherry-Pick Selected Changes` and `Get from Branch` | apply-changes:263, 285 |
| progress | merge/rebase/cherry-pick | `Cherry-picking commits in memory…` (GitBundle:749); `Updating…` (1324) | GitBundle:749, 1324 |
| **progress: in-flight** | **status bar** | `Cherry-picking in {0}` (GitBundle:981) — **per root** | GitBundle:981 |
| **progress: aborted** | notification | "The operation {0} was aborted" family | GitBundle:352 region |
| error | unfinished operation | "You have unfinished merge. These conflicts must be resolved before update." / "Unmerged files detected…" / "You have unfinished rebase process…" (1326-1328), each with `Cannot update` (1325) | GitBundle:1325-1328 |
| error | cherry-pick onto a remote | `Cherry-pick conflict` + "Failed to cherry-pick commits onto {0}:\n{1}" (752-753) | GitBundle:752-753 |
| error | conflict left behind | `{0} was performed with conflicts` (GitBundle:32) | GitBundle:32 |
| error | branch blocked by conflicts | "If conflicts occur … the **Merge Conflicts node will appear in the corresponding changelist** in the Commit tool window with a link to resolve them" (apply-changes:57) | apply-changes:57 |
| error | not-cherry-picked comparison | `Could not compare with branch {0}` (1231); `Highlighting of non-picked commits in '{0}' has been cancelled` (1232) | GitBundle:1231-1232 |

**`git.status.bar.widget.text.cherry-pick=Cherry-picking in {0}` (GitBundle:981) is
the topology fact for the whole surface: an in-flight integration is announced in
the status bar, per root, and that is where its Abort/Continue/Skip verbs live.**
The doc agrees: "The status bar displays the current state of a cherry-pick
operation. You can manage the cherry-pick process using the following actions"
(apply-changes:227-231) — Abort, Continue — and "select `Abort Cherry-Pick` in the
VCS widget or click in the status bar" (line 229).

## UX — how it behaves

### How is it reached, and what hides behind a secondary affordance?

Every integration verb has **two routes**: a dialog, and a fast path that skips it.

| Verb | Dialog route | Fast path |
| --- | --- | --- |
| Merge | `Git | Merge…` (apply-changes:39) | row menu → `Merge 'x' into 'y'` (line 37) |
| Rebase | `Git | Rebase…` (line 72) | `Checkout and Rebase onto …` / `Rebase … onto …` / `Pull into … Using Rebase` (lines 100-106) |
| Cherry-pick | — | toolbar `Cherry-pick` (line 244) |
| Cherry-pick N commits onto a remote | `Add Commits to Remote Branch…` | — |
| Apply separate changes | — | Changed Files pane → `Cherry-Pick Selected Changes` (line 263) |
| Apply a file | — | Changed Files pane → `Get from Branch` (line 285) |
| Update vs pull | `⌃T` / `Git | Pull` | — |

**Behind secondary affordances:**

- **All flags** hide behind `Modify options` (apply-changes:40, 76; sync:44). The
  un-modified dialog is 1–3 fields.
- **The interactive rebase is a flag, not a mode**: `--interactive` = "Edit commits
  before rebasing" (GitBundle:202) turns the Rebase dialog into the plan editor
  (apply-chances:194). One dialog, two outcomes.
- **The empty-cherry-pick policy** is a setting, not a dialog
  (GitBundle:840-842).
- **The two-dot range filter** is hidden in the toolbar: "In the Commits pane, go to
  `Branch | Select`. Specify … `branch1..branch2` … Press `⌃↵`"
  (manage-branches.md:149-153). **A textual range expression, committed with
  `⌃↵`.**

### Which verbs are destructive, and what do they confirm or preflight?

Every verb here changes history or the working tree. The confirmations are
*situational*, not blanket:

| Situation | Behaviour | Evidence |
| --- | --- | --- |
| **Merge, clean tree, no conflicts** | **No confirmation.** "Git will merge the two branches, and the merge commit will appear in the Log tab" (apply-changes:56) | line 56 |
| **Merge, local changes would be overwritten** | **Offered Smart merge** — "If you have local changes that will be overwritten by merge, IntelliJ IDEA will suggest performing **Smart merge**. If you select this option, IntelliJ IDEA will stash uncommitted changes, perform merge, and then unstash the changes" (apply-chances:58) | line 58 |
| **Merge, conflicts** | Conflict resolution, not a confirm (line 57) | line 57 |
| **Checkout, dirty tree** | Force vs Smart Checkout (manage-branches.md:105) | see `02-branches-popup` |
| **Checkout, tracking divergence** | auto-reset, or Drop Local Commits vs Rebase onto Remote (manage-branches.md:85-91) | see `02-branches-popup` |
| **Rebase onto a protected branch** | **Blocked outright** — "You cannot modify the history for protected branches where `git push --force` is not allowed" (edit:11) | edit:11 |
| **Rewrite during another operation** | **Blocked** — `Cannot {0} during …` (GitBundle:254-257) | GitBundle:254-257 |
| **Cherry-pick an already-contained commit** | **Disabled** (log-tab.md:221) | log-tab.md:221 |
| **`--no-commit` merge** | the user commits later, deliberately (GitBundle:125) | GitBundle:125 |
| **`Get from Branch`** | **No confirmation documented** — "IntelliJ IDEA will copy the entire contents of the file to the current branch" (apply-changes:287) | line 287 |
| **`Revert Selected Changes`** | no confirm; stages a revert instead (log-tab.md:405) | log-tab.md:405 |
| **Abort** | no confirm; it is itself the recovery | apply-chances:59 |

**The design in one sentence: confirm when the user's local work is at risk, stay
silent when it is not.** A clean merge is silent; a dirty merge offers Smart merge;
a conflicting merge goes to resolution; a protected branch refuses. Four different
responses, each matched to the actual risk.

**And the escape hatch is always present**: "You can cancel an unfinished merge
operation by selecting the Abort action from the VCS widget" (apply-chances:59);
"selecting the Abort or Continue actions respectively from the top of the VCS
widget" for rebase (line 99); "select `Abort Cherry-Pick` in the VCS widget or
click in the status bar" (line 229).

### What can be undone, from where, and for how long?

| Operation | Undo | Lifetime | Evidence |
| --- | --- | --- | --- |
| Merge | `Abort Merge` **only while unfinished** | until the merge commit exists | apply-chances:59 |
| Merge (completed) | `Revert Commit` — additive, not undo | permanent revert commit | log-tab.md:288 |
| Rebase | `Abort Rebase` — "restore the original branch and stop rebasing" | until complete | GitBundle:1329 |
| Rebase (completed) | Undo Commit / Drop / reword, each with a notification undo | unpushed | see `06-history-rewrite` |
| Cherry-pick | `Abort Cherry-Pick` **only while unfinished** | until the commit exists | apply-chances:229 |
| **Cherry-pick that failed with conflicts** | **the changes land in the Commit tool window unreviewed-but-recoverable**: "If the cherry-pick failed with conflicts, the selected changes will appear in the **Changes area** that you can see in the Commit tool window. You can **review these changes and commit them later** if necessary" (apply-changes:246) | indefinite — the work is in the working tree | line 246 |
| Smart merge's stash | the stash is popped immediately; a conflict routes to resolution (apply-changes:58) | — | line 58 |
| Smart Checkout's shelve | see `02-branches-popup` / `07-shelve-stash` | — | — |

**The cherry-pick-failure case is the notable one.** A failed cherry-pick is *not*
rolled back automatically: the partially-applied changes stay in the working tree
as ordinary uncommitted changes, reviewable and committable. **An integration
failure degrades into a review task rather than an error state.** That is a good
default and worth copying explicitly.

Optional, and policy-driven: "If you want IntelliJ IDEA to **create changelists
automatically in case of cherry-pick fail**, switch on the corresponding setting in
`Settings | Version Control | Changelists`" (apply-changes:247) — bundle
`settings.changelists.create.automatically.checkbox=Create changelists
&automatically` (VcsBundle:149). So even the failure's organisation is a setting.

### Which options are on by default, and what is remembered between uses?

| Option | Default | Evidence |
| --- | --- | --- |
| All merge/pull/rebase flags | **off** — every one is opt-in behind `Modify options` | apply-chances:40, 76; sync:44 |
| Pull remote | **`origin`** | sync:40 |
| Pull branch | the tracked remote branch; **a different choice is remembered** — "IntelliJ IDEA will remember your choice and display this branch by default in the future" (sync:42) | sync:42 |
| Update method | **Merge** (see `08-remotes-sync`) | sync:72 |
| Auto-update on push rejected | **off** — the Push Rejected dialog appears | commit-and-push-changes.md:236 |
| `--no-verify` | off; and there are two levels — per-commit in commit settings, and globally via `advanced.setting.git.commit.do.not.run.commit.hooks` (commit-and-push:86-87) | lines 86-87 |
| On empty cherry-pick commit | present, default **UNVERIFIED** (`Skip commit` / `Create empty commit`) — re-tested 2026-10-06: settings-version-control-git documents no Cherry-Pick section, and a web search for "On empty cherry-pick commit" finds no page naming it → runtime capture (R3, screenshot Version \| Git) | GitBundle:840-842 |
| `cherry-picked from <hash>` suffix | **off** — "By default, no suffix is added when cherry-picking a change" (docs: settings-version-control-git) | GitBundle:793 |
| Unfinished-operation refusals | on — three separate `Cannot update` messages | GitBundle:1326-1328 |
| Automatically open `Update Info` after updating | **on** | sync:78 |
| Protected branches | configurable; GitHub protection auto-detected on checkout (edit:11) | edit:11 |

Remembered between uses:

- **The pull branch choice** — the one place a user's ad-hoc selection persists
  (sync:42).
- **The update method**, written back from the Push Rejected flow
  (commit-and-push-changes.md:241) — see `08-remotes-sync`.
- **`rebase.autoSquash` is git's config, not the app's** (apply-changes:151) —
  deliberate deference.
- **Nothing else persists.** Every flag resets per dialog.

### What feedback appears, and where does focus land?

Progress:

- `Cherry-picking commits in memory…` (GitBundle:749) — note **"in memory"**:
  this path does not touch the repository until it succeeds
- `Updating…` (1324), `Fetching…` (1100)
- `Cherry-picking in {0}` (981) — the **status-bar** in-flight state

Success:

- **Merge: no notification.** "the merge commit will appear in the Log tab of the
  Git tool window `⌥9`" (apply-chances:56). **The proof of a merge is the log, not
  a toast** — the operation is observable in the model the user already watches.
- **Cherry-pick: no notification** on the happy path either (apply-chances:244).
- Push, by contrast, has ten distinct notifications (GitBundle:913-930) — see
  `08-remotes-sync`. **The feedback budget is spent where the outcome is remote and
  ambiguous, not where it is local and inspectable.**

Error:

- `Cherry-pick conflict` + "Failed to cherry-pick commits onto {0}:\n{1}"
  (GitBundle:752-753)
- `{0} was performed with conflicts` (GitBundle:32) — the operation *succeeded with
  a caveat*, reported as its own state
- "If there are unresolved conflicts left after a merge, the **Merge Conflicts node
  will appear in the corresponding changelist** in the Commit tool window with a
  link to resolve them" (apply-chances:57)
- `Cannot {0} during {…}` (GitBundle:254-257)
- `Could not compare with branch {0}` / `Highlighting of non-picked commits in
  '{0}' has been cancelled` (GitBundle:1231-1232)
- Unfinished-operation refusals with an actionable tail for rebase
  (GitBundle:1326-1329)

**Optimistic paint: no**, and specifically: a failed cherry-pick's partial result
is shown as real uncommitted changes rather than optimistically as a commit
(apply-chances:246). The paint is truthful about what actually happened.

Focus after the action:

- After a successful merge, the new commit is visible in the Log tab (line 56) —
  so **focus follows the model, not a dialog**.
- After `Abort Merge` / `Abort Rebase`, the original branch is restored
  (GitBundle:1329) and the working tree returns to its prior state.
- **UNVERIFIED**: whether any dialog focuses its first field on open, and where
  focus lands after the Merge dialog closes. (re-tested against apply-changes/
  sync/undo-changes 2026-10-06: docs describe dialog contents, never focus →
  runtime capture (R3))

## Flow — how tasks chain

### Entry points

1. `Git | Merge…`, `Git | Rebase…` (apply-chances:39, 72).
2. Branch popup / Branches pane row menu — every fast path (lines 100-106;
   log-tab.md:83-129).
3. Log commit row — `Cherry-pick`, `Add Commits to Remote Branch…`,
   `Cherry-Pick Selected Changes`, `Reset Current Branch to Here…`
   (log-tab.md:264-338).
4. Changed Files pane — `Cherry-Pick Selected Changes`, `Get from Branch`,
   `Get from Revision` (log-tab.md:405-420).
5. **The status bar / VCS widget** — Abort, Continue, Skip for an in-flight
   operation (apply-chances:227-231).
6. `⌃T` → Update Project, for the merge-based sync variant (sync:68).

### Internal sequence, with dialog chaining

**Merge — the clean case, and the dirty case:**

```
select the target branch in the Branches popup
  → Checkout            (switch first — the merge direction depends on it)
  → Merge 'feature' into 'main'         (fast path, no dialog)
      → (clean, no conflicts) merge commit appears in the Log      ✓ done
      → (local changes would be overwritten)                          [dialog]
            Smart merge offered → stash → merge → unstash
              → (unstash conflicts) Conflicts dialog                 [chained]
      → (conflicts) Conflicts dialog                                 [chained]
            → Merge Conflicts node persists in the Commit tool window
      → Abort Merge available throughout, from the widget
```

(apply-changes:31-59; tutorial:136-180.)

**Rebase — one dialog, two outcomes:**

```
Git | Rebase…
  → Rebase dialog                                       [dialog]
      target branch picker
      [Modify options]
          --onto (new base, old base)
          --interactive     ← turns this dialog into the plan editor
          --update-refs / --rebase-merges / --keep-empty / --root / --autosquash
          Select another branch to rebase
  → Rebase
      → (--interactive) Rebasing Commits plan editor               [chained]
            Action | Hash | Subject; 7 verbs; graph; Reset; View Git Commands
          → Start Rebasing
      → (conflict) rebase pauses → Conflicts dialog                 [chained]
      → status bar: rebase in progress; Abort / Continue / Skip
  → (Complete or Abort)
```

(apply-changes:70-99, 165-221; GitBundle:200-208, 254-257.)

**Cherry-pick a single commit:**

```
checkout the TARGET branch first        ← a precondition, stated at length
  → Log → select the commit
  → (verify in Commit Details: "make sure these are the changes you want")
  → Cherry-pick on the toolbar
  → applied and committed to the target branch
  → (conflicts) the changes appear in the Commit tool window's Changes area;
    review and commit later
  → Push
```

(apply-changes:233-249.) Note the two-step shape: **checkout, then cherry-pick** —
the tool does not switch branches for you.

**Cherry-pick selected changes (not the whole commit):**

```
checkout the target branch
  → Log → the commit
  → Commit details pane → select the FILES → Cherry-Pick Selected Changes
  → [dialog] select an existing changelist or name a new one
  → Commit, then Push
```

(apply-changes:252-267.) **Verb 4 of the five.**

**Apply a whole file to the current branch — verb 5:**

```
select the branch that HAS the file → Show Diff with Working Tree
  → Changes tool window, with the three-colour status grammar:
       grey  = only in the selected branch
       green = only in the current branch
       blue  = differs
  → [Swap Branches link]
  → select the file → Get from Branch  (or the toolbar icon)
  → "IntelliJ IDEA will copy the entire contents of the file to the current branch"
  → Commit and push
```

(apply-changes:269-287; manage-branches.md:124-140.) **The three-colour status is
a contract** — it distinguishes add from delete from modify without reading
direction, and it appears identically in both places the doc describes it.

**Cherry-pick onto a remote branch:**

```
Log → select commits → Add Commits to Remote Branch…
  → [dialog] remote-branch search
      "Search remote branches" (GitBundle:758)
      "Nothing found for {0}" (759)  ← an explicit empty-search state
  → (conflict) Cherry-pick conflict: "Failed to cherry-pick commits onto {0}:\n{1}"
      (GitBundle:752-753)
```

(`actions-labeled.tsv`; GitBundle:744-745, 758-759, 752-753.) **A remote branch as
a cherry-pick target** — no local checkout needed, and TurboGit has
`Dialog::CherryPickAcross` for the multi-repo version.

**Two-dot range, for reviewing what is missing:**

```
⌥9 → Commits pane → Branch | Select
  → type  branch1..branch2
  → ⌃↵
  → the list contains the commits in branch2 not in branch1
```

(manage-branches.md:146-154.)

### What happens next on success

- **Merge**: the merge commit appears in the Log tab (apply-chances:56).
- **Rebase**: the branch's commits are replayed; a `Stop to Edit` row pauses the
  rebase (apply-chances:141).
- **Cherry-pick**: the commit is applied *and committed* to the target branch
  (apply-chances:244).
- **Get from Branch**: the file's contents are replaced, and the user then commits
  and pushes (apply-chances:287).
- The next step is always named: "Push the changes to the target branch"
  (apply-chances:249, 267), "Commit and push the changes" (line 287).

### What happens next on failure, and how the user recovers

| Failure | Evidence | Recovery |
| --- | --- | --- |
| **Merge conflicts** | `{0} was performed with conflicts` (GitBundle:32); a `Merge Conflicts` node appears in the corresponding changelist (apply-chances:57) | resolve in the Conflicts dialog; the node persists |
| **Rebase conflicts** | `Merge conflicts detected. Resolve them before continuing rebase.` (GitBundle:385) | resolve, then `Continue Rebase` — available from the status bar, the widget, **and** `Git | Continue rebase` (apply-chances:143) |
| **Cherry-pick conflicts** | the changes land in the Commit tool window (apply-chances:246) | review and commit later; optionally auto-changelist (line 247) |
| **Cherry-pick onto a remote failed** | `Cherry-pick conflict` + the target and error (GitBundle:752-753) | **UNVERIFIED** — no bundled recovery action; re-tested 2026-10-06: the live apply-changes page no longer documents the "Add Commits to Remote Branch…" flow at all, and a web search finds no other page that does → runtime capture (R3) |
| **Smart merge / Smart checkout stash conflict** | "If a conflict occurs during the unshelve operation, you will be prompted to merge the changes" (manage-branches.md:107) | the Conflicts dialog |
| **Protected branch** | "You cannot modify the history for protected branches where `git push --force` is not allowed" (edit:11) | unprotect, or work on another branch |
| **During another operation** | `Cannot {0} during {…}` (GitBundle:254-257) | finish or abort that operation |
| **Cross-repository selection** | `Selected {N commits} {is\|are} from different repositories` (GitBundle:252) | narrow the selection |
| **Cherry-pick already contained** | button disabled (log-tab.md:221) | nothing to do |
| **Unfinished merge blocks update** | "You have unfinished merge. These conflicts must be resolved before update." (GitBundle:1326) | resolve, or Abort Merge |
| **Unmerged files block update** | GitBundle:1327 | resolve |
| **Unfinished rebase blocks update** | GitBundle:1328, + "Then you may **continue rebase**. You also may **abort rebase**…" (1329) | two named actions |
| **`--rebase-merges` lost resolutions** | "any resolved merge conflicts or manual amendments in these merge commits will have to be resolved or reapplied manually" (apply-chances:87) | manual — **a documented cost of the flag, stated up front** |
| **`--ff-only` cannot fast-forward** | the merge refuses | the user picks a different strategy |
| **Non-cherry-picked comparison failed** | `Could not compare with branch {0}` (GitBundle:1231) | retry; the cancel message (1232) is a separate state |
| **Remote search found nothing** | `Nothing found for {0}` (GitBundle:759) | refine the search |

### Escape / dismiss semantics

- **Abort is the universal exit, and it is always reachable from two places**: the
  VCS widget and the status bar (apply-chances:59, 99, 229-231). The docs give the
  status-bar route for every operation, so the escape does not depend on finding a
  menu.
- **`Skip Commit`** (`Git.Rebase.Skip`) is a *third* option that is neither abort
  nor continue — it drops the current commit and proceeds. So a paused rebase
  offers three ways forward, not two.
- **A failed cherry-pick cannot be dismissed** — its changes are in the working
  tree and must be reviewed or reverted (apply-chances:246).
- **`--no-commit` is the deliberate way to get out of a merge's commit step**:
  "a merge will be performed, but a merge commit will not be created **so that you
  can inspect the result of the merge before committing**" (apply-changes:50). The
  escape is designed into the flag, not bolted on.

## Divergences (intentional)

| Dimension | Rebased behaviour | TurboGit behaviour | Why |
| --- | --- | --- | --- |
| UI | Merge and Pull have **different flag sets** (7 vs 5) with **differing wording** for the flags they share (GitBundle:121-127 vs 150-154) | `Dialog::Merge` and the merge path carry `MergeOpts`; pull goes through `Vcs.UpdateProject` / `Operation::Fetch` | **Intent matched by divergence.** Omitting `--no-verify` and `--allow-unrelated-histories` from pull is correct — they mean subtly different things pre- and post-fetch — and TurboGit's separate `Operation` types make the wrong combination unrepresentable. Recorded so the difference is understood as deliberate rather than an omission. |
| UX | Every flag's label states its **effect**, not its name (GitBundle:121-127) | `Dialog::Merge` has `OPTIONS` with `--no-ff`, verify-signatures, allow-unrelated, `--no-verify` (`dialogs.rs`) | **Partial gap, and the wording is the substance.** `--no-ff` tells the user nothing; "Create a merge commit even if it can be fast-forwarded" tells them everything. TurboGit's option labels read as git flags where Rebased's read as consequences. Copy the *style* — the full label vocabulary is specified above. |
| UX | The clean merge is **silent** — success is the commit appearing in the Log (apply-chances:56) | `Operation::Merge` reports through the activity feed and a toast | Comparable, and TurboGit's is arguably more visible. **No divergence worth claiming**; recorded because the principle (don't spend feedback budget on locally-inspectable outcomes) is worth preserving if the toast volume ever grows. |
| UX | A **failed cherry-pick stays applied** as uncommitted changes, reviewable and committable (apply-chances:246) | `Operation::Merge` routes to `integrate_service::smart_merge`; conflict hand-off is explicit in `OpKind` | Same philosophy, and TurboGit's `OpKind`-based settlement makes the partial case representable. **No divergence.** |
| UX | In-flight state lives in the **status bar, per root** — `Cherry-picking in {0}` (GitBundle:981) — with Abort/Continue/Skip there | `PendingConfirm` and `worktree_lifecycle.rs` model in-flight work; the status bar exists (24px) | **Partial gap.** Rebased makes the in-flight operation a *first-class, per-root status item with its own verbs*. TurboGit has the lifecycle machinery but not a status-bar presentation of it. Worth adding; see ticket 16. |
| UX | `Skip Commit` is a **third** resolution for a paused rebase, distinct from Abort and Continue (`Git.Rebase.Skip`) | **UNVERIFIED whether TurboGit exposes skip** | **Open gap, small and high-value.** Skip is what you want when the *next* commit is the one that conflicts; Abort throws away the whole rebase and Continue retries the same commit. Three options where two are modelled loses the common case. re-tested 2026-10-06: TurboGit-side, docs N/A; `interactive_rebase.rs` offers Drop (plan editor) and Abort & restore, but no mid-conflict Skip verb → close by code citation (R1 fold-in). |
| UX | Empty-cherry-pick is a **policy setting** — `Skip commit` / `Create empty commit` (GitBundle:840-842) | Not established | **Open gap, small and well-specified.** A cherry-pick that becomes empty is a real occurrence (the change is already upstream) and the right answer is genuinely a choice. |
| UX | `settings.add.suffix` — add a `cherry-picked from <hash>` suffix when picking commits pushed to protected branches (GitBundle:793) | Not established | **Open gap, small.** The provenance suffix is how a team knows a commit was backported rather than authored there. |
| UX | `rebase.option.rebase.merges` is labelled "Recreate commits topology" (GitBundle:204) — **vague, on the flag with the worst documented consequence** | `--rebase-merges` presented as a git flag | **Deliberate improvement over the reference.** The doc itself warns that resolved conflicts and manual amendments "will have to be resolved or reapplied manually" (apply-chances:87); a label that said *that* would be better than either. Copy the intent, not the vagueness. |
| UX | **Verb 5, "apply a specific file to a branch"** (`Get from Branch`), with a three-colour status grammar distinguishing only-here / only-there / differs (apply-changes:269-287) | Not established as a discrete verb | **Open gap, real value.** "Take that one file from the other branch" is a daily operation that neither `cherry-pick` nor `merge` covers, and the three-colour grammar is fully specified. See ticket 17. |
| UX | **Verb 4, "apply separate changes from a commit"**, via the Commit Details pane → `Cherry-Pick Selected Changes` (apply-changes:252-267) | `Dialog::CherryPickAcross` covers multi-commit multi-repo; single-commit file-level selection is **UNVERIFIED** | **Partial.** The multi-repo case is stronger in TurboGit; the simple two-step case is not evidenced as present. re-tested 2026-10-06: TurboGit-side, docs N/A; `log_window.rs`'s changed-file row menu offers only Show blame and Show history for file… — no per-file cherry-pick → close by code citation (R1 fold-in). |
| UI | Branch-row fast paths are **verb-per-row** with availability rules (remote-only for "Pull into … Using X") (apply-chances:100-106) | `branch_menu.rs` has 10 items including `CheckoutAndPull`, `Merge`, `Rebase`, `Pull`, `Push` | The item set matches closely. **The availability rules are the part to check** — specifically that "Pull into … Using Rebase/Merge" is remote-only, which prevents offering a pull from a local branch. **UNVERIFIED** whether TurboGit's gates encode that. re-tested 2026-10-06: TurboGit-side, docs N/A; `branch_menu.rs` has no "Pull into … Using X" rows at all — its Pull gate requires current-branch + upstream ("no upstream to pull from") and Merge/Rebase are disabled on remote rows — so the remote-only rule is encoded as absence, not as a gate → close by code citation (R1 fold-in). |
| UX | The two-dot range filter with `⌃↵` (manage-branches.md:149-153) | **UNVERIFIED** | **Open gap, small.** `branch1..branch2` typed into a filter is a power-user affordance with a keystroke, and TurboGit's log already has a scope model to hang it on. re-tested 2026-10-06: TurboGit-side, docs N/A; no `branch1..branch2` range parsing exists in `log_display.rs`/`log_window.rs` → confirmed absent, close by code citation (R1 fold-in). |
| UX | `Add Commits to Remote Branch…` — cherry-pick onto a **remote** branch with a searchable picker and an explicit empty state (`Nothing found for {0}`) | `Dialog::CherryPickAcross` is multi-*repository*; remote-as-target is **UNVERIFIED** | **Partial gap.** TurboGit's multi-repo picker has per-target predictions that Rebased's lacks; Rebased's remote-target path has no TurboGit equivalent. Both are worth having. re-tested 2026-10-06: TurboGit-side, docs N/A; `dialogs.rs`'s `cherry_pick_target` offers only `BranchKind::Local` rows and `CherryPickAcross` targets roots, not remote branches → confirmed absent, close by code citation (R1 fold-in). |

## Needs runtime capture

1. **Default state of every flag** in all three dialogs. The bundle gives labels
   but no defaults, and "every flag off" is inferred from the `Modify options`
   structure rather than stated.
2. **Default of `On empty cherry-pick commit:`** — `Skip commit` or
   `Create empty commit` (GitBundle:840-842).
3. **Whether the status bar's in-flight item is per-root or global when several
   roots are mid-operation** — the string is per-root (`Cherry-picking in {0}`) but
   the presentation with two concurrent operations is unknown.
4. **The Merge dialog's field order and whether the preview is present.** The doc
   names only the branch picker and `Modify options`; Rebased's merge dialog is
   *not* documented as having a commits-incoming preview the way TurboGit's does.
5. **The `Add Commits to Remote Branch…` dialog's full field list** — only the
   search box and its empty state are known (GitBundle:758-759).
6. **Whether `Get from Branch` really has no confirmation.** The doc describes only
   the success path (apply-changes:287), and it overwrites a file's entire contents.

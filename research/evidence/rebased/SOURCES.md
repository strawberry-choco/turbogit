# Source map. UX and user-flow evidence for Rebased / IntelliJ Git

> The bundle extraction (`actions.txt`, `strings/`, `keymap-bindings.txt`) covers
> **what exists**. It cannot cover **how a task flows**. This file records where
> the flow evidence comes from, and which candidate sources turned out to be dead
> ends.
>
> Status of each link verified 2026-10-05 by HTTP status against the live site.
> `https://www.jetbrains.com/help/idea/<slug>.html`

## Tier 1, official docs (verified, current, authoritative for flow)

These pages are *already written as step-by-step user flows*: click target, dialog
contents, defaults, decision points, confirmations, notifications, shortcuts. They
carry exactly the UX and flow half that nothing in the bundle can supply. Last
modified dates observed: 17–19 August 2026, so they describe the current UI the
installed build came from.

| # | Slug (verified 200) | Covers which of our twelve tasks |
| --- | --- | --- |
| 1 | `set-up-a-git-repository` | Task 1 onboarding: init, clone, attach existing |
| 2 | `working-with-git-tutorial` | End-to-end narrative: create → add → commit → push → merge → conflict → history. The single best overview of intended flow |
| 3 | `commit-and-push-changes` | Task 2 commit, task 3 hunk/line granularity, amend, commit-and-push split |
| 4 | `manage-branches` | Task 6: create (3 ways), checkout, compare, rename, delete, favorites, grouping, **Recent node ≤5 branches**, per-prefix directory grouping, `Checkout and Update` vs plain checkout, Force Checkout vs Smart Checkout for dirty trees |
| 5 | `resolve-conflicts` | Task 7: Conflicts dialog, Accept Yours/Theirs/Resolve Manually, 3-pane tool (left local read-only, right repository read-only, centre editable starting from base), Apply Non-Conflicting Changes (all/left/right), `Resolve using Left`/`Resolve using Right`, simple-conflict magic resolve, `Apply Changes` → next file vs `Save and Close` → list, `Revert conflict resolution`, `Accept and Finish` when none remain |
| 6 | `apply-changes-from-one-branch-to-another` | Task 8: merge, rebase, cherry-pick, the integrate family |
| 7 | `shelving-and-unshelving-changes` | Task 11: shelve vs stash, silent shelve, unstash variants |
| 8 | `sync-with-a-remote-repository` | Task 9/12: fetch, pull/update policy, push rejected recovery, multi-repo synchronous control |
| 9 | `edit-project-history` | Task 5: amend, reword older commit, rebase from here, drop, interactive rebase |
| 10 | `investigate-changes` | Log navigation, filters, commit details, review how a merge was resolved |
| 11 | `log-tab` | Log tool window structure and its own affordances |
| 12 | `using-patches` | Create/apply patch, apply from clipboard |
| 13 | `settings-version-control-git` | The Version Control \| Git settings reference page (found 2026-10-06 via the "Version Control Settings: Git" cross-reference in `commit-and-push-changes`). States almost no checkbox defaults, useful negative evidence for the R3 capture list, except: cherry-pick suffix off by default, background fetch every 20 minutes, stash-diff-against-local on by default. Carries one hazard: "If you enable the staging area, all existing changelists will be deleted", which contradicts the lossless-switch claim in `commit-and-push-changes` |
| 14 | `advanced-settings` | The advanced-settings reference page (found 2026-10-06: `settings-advanced-settings` → 404, resolved by topic search, the **third** live instance of the 404-means-renamed lesson). Describes Use Safe Force Push (`--force-with-lease`), recursive submodule clone, incoming/outgoing commit checks; states no checkbox defaults |

Slugs that 404, **all four resolved in the table below; two were renames, not
dead ends. Do not trust the old slug; do not conclude absence from a 404.** A
fifth instance surfaced 2026-10-06: `settings-advanced-settings` →
`advanced-settings` (row 14 above), resolved the same way.

**Resolution of every "dead" slug, verified 2026-10-05. Two of four were alive.**

| Old slug | Reality | Verified |
| --- | --- | --- |
| `switch-between-branches` | genuinely folded into `manage-branches` | already held |
| `compare-file-with-diff` | → `comparing-files-and-folders` | 200, 03 Sep 2026 |
| `undo-the-last-commit` | → **`undo-changes`** | 200, 05 May 2026 |
| `group-changelists` | → **`managing-changelists`** (page title "Group changes into changelists") | 200, 17 Aug 2026 |

**The generalisable lesson, now that two of four "dead ends" turned out to be
renames: a 404 is *possibly renamed* until checked.** The method that worked,
twice: search the **topic**, not the slug, then read the recovered page's sibling
links. A 404 plus an outbound link from a live sibling is a rename until
disproved.

`harvest.py` should therefore treat a 404 as a **rename candidate**, not a
terminal result: on a 404, search the topic and follow the surviving pages' "How
it works" footers before recording a dead slug. Checking only the surviving
pages' footers would have caught `compare-file-with-diff` (whose content moved
into a page `differences-viewer` links to) but not `undo-the-last-commit` or
`group-changelists`, those needed a topic search, because no page we already held
linked to them by name.

**What the two recoveries were worth:** `undo-changes` carried a
`Select Target Changelist` dialog (Name **defaulted to the commit message**,
Comment, Set active, Track context) and a **four-mode** Git Reset (Soft / Mixed /
Hard / **Keep**, where `Keep` discards committed work but keeps local work). Neither
was in the spec before this. `managing-changelists` confirmed the destination
dialog's three fields reappear in `Move to Another Changelist` (`⌥⇧M`), one
component, three call sites. Detail in `docs/spec/rebased-parity.md` §7.1a.

### Why this tier matters more than anything else here

It supplies specifics that no artifact and no screenshot would give cleanly:
the **decision points and defaults** of each journey. Examples already harvested
from two pages, ready to become specs:

- **Dirty checkout** offers Force Checkout vs Smart Checkout; Smart Checkout
  shelves → checks out → unshelves, and can itself conflict.
- **Checkout of an existing local branch whose tracking differs**: auto-reset if
  nothing can be lost, otherwise offer Drop Local Commits vs Rebase onto Remote.
- **New branch dialog** has a `Checkout branch` checkbox, suggests prefixes from
  existing local branch names as you type.
- **Delete branch** is immediate even when unmerged, then offers restoration and
  a link to view unmerged commits; restore stays available in Notifications until
  restart.
- **Recent branches node** is capped at five; branch grouping is by `/` prefix and
  can be turned off from the widget's Settings gear.
- **Push dialog** (multi-root) selects only the current repository by default;
  `Define remote` appears inline when there is none; target branch becomes an
  editable field; commits authored by someone else are marked with an asterisk.
- **Conflicts** can be left and resumed, a Merge Conflicts node with a Resolve
  link appears in the Commit tool window.

Those are the things that decide whether the result *feels* like Rebased.

## Tier 2, git4idea source: **not available where expected**

Checked 2026-10-05:

- Repo exists; https://raw.githubusercontent.com/JetBrains/intellij-community/master/README.md → 200.
- `plugins/git4idea/src` contains only a `.keep`. Root now has
  `frontend/`, `backend/`, `shared/`, `rt/`, `localHistory/`, `terminal/`,
  `performanceTesting/` alongside `BUILD.bazel`, the frontend/backend split
  restructure.
- `plugins/git4idea/frontend/src` contains only `com/…`; there is no
  `git4idea/ui/branch` path.
- Recursive tree API (17.6 MB, `truncated: true`, 48 315 entries) has **zero**
  hits for `BranchPopup`, `git4idea/ui`, or `dvcs/ui/`.

So menu **ordering**, the one thing neither docs nor the bundle guarantees,
cannot currently be read from public source. Note the local bundle ships
`intellij.vcs.git.frontend.jar`, i.e. exactly the frontend module whose sources
are absent from the community repo.

## Tier 3, local bytecode (partial tie-breaker, weak)

No disassembler available: the bundled JBR ships `javac` but **not** `javap`, and
`/usr/bin/javap` refuses without a system JRE. A hand-rolled constant-pool parser
works and preserves source order, but the popup builds actions dynamically via
`ActionManager`, so few action-id literals appear. One usable result:

```
BranchesDashboardTreeComponent  (pool order)
  Git.Fetch
  Git.New.Branch.In.Log
  Git.Log.Branches.Settings
  Git.Log.Branches.Navigate.Log.To.Selected.Branch
  Git.Log.Branches.Toolbar.Actions
```

Treat as corroboration only, never as the source of truth.

## Resolution

1. **Docs first.** Write every UX and flow spec from Tier 1. It covers all twelve
   tasks, needs no permissions, and is quotable directly into tickets. Nothing else
   has to be unblocked first.
2. **Bundle for names, ids and keystrokes.** Tier 1 states journeys; the harvested
   artifacts state exact wording and accelerators. Use both together.
3. **Runtime capture demoted to arbiter.** Use it only where the docs are silent or
   ambiguous: exact spacing and density, exact menu ordering, latency and feedback
   behaviour. It no longer has to carry the whole UX/flow load, so the Screen
   Recording and Accessibility grants are no longer on the critical path.

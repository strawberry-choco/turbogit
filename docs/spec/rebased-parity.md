# Rebased parity specification

How **Rebased** — a stripped-down IntelliJ IDEA Git client, version 1.1.19, at
`/Applications/Rebased.app` — presents and behaves across **UI**, **UX** and
**user flow**, written so TurboGit can be brought to parity.

This document is assembled from eleven per-surface evidence files under
[`research/evidence/rebased/parity/`](../research/evidence/rebased/parity/). Each
claims are traceable to a file under `research/`. Nothing here was inferred from
general IntelliJ knowledge; where the evidence is silent, the text says
`UNVERIFIED` and the closing **[unresolved](#7-unresolved)** table says what would
close it.

**TurboGit parity is parity of structure and flow, not of finish.** Panel
topology, item inventory, menu order, default keystrokes, dialog field order and
defaults, terminology, and empty/error/progress states are in scope. Darcula
colours, insets, their icon set, their accessibility gaps, and IDEA inheritance
(editor, terminal, debugger, build-tool prompts, trust prompts) are not.

- **Method and evidence** — §2
- **Copy / do-not-copy** — §3
- **Surfaces** — §4 (conflicts, branches, commit, history, shelf/stash, remotes, log, integrate, patches, onboarding, diff)
- **Gap list** — §5
- **Reverse gap list** — §6
- **Unresolved** — §7
- **Divergences** — §8
- **Tickets** — §9

---

## 1. Background

Rebased is not a general IDE; it is a Git client that happens to be built on
IntelliJ's VCS stack. That distinction shapes everything below, because most of
what a naive harvest of "IntelliJ Git" would collect is **not Git-client
behaviour**.

Two consequences shaped this pass:

1. **The Git surfaces are a minority of the product.** The eleven surfaces
   harvested here account for 213 action ids. `docs-text/investigate-changes.md`
   devotes large sections to blame annotations, inlay hints, and editor gutters;
   `docs-text/working-with-git-tutorial.md` spends a step on a GitHub share
   wizard. All of it is either IDE inheritance (`DISTILL.md` rule 5) or
   GitHub/GitLab integration, which has no TurboGit equivalent and no
   specification. It is excluded, and saying so is part of the deliverable —
   a parity claim that included those sections would be claiming work nobody
   wants.

2. **The Git client's real substance is decision-making, not widgets.** The
   highest-value findings in this document are almost all *decisions*: what
   happens on a dirty checkout, whether delete is asked or told, how long an
   undo lives, what a rejected push says next, what an empty state offers. Those
   live in the documentation, not the bundle, and they are the reason the docs
   were read in full rather than mined for action ids.

The evidence base is unusually good in one respect: **the shipped string bundles
contain the product's own state machines.** `VcsLogBundle.properties` alone
enumerates ten log states with their recovery actions; the `push.*` family gives
ten distinct push outcomes including six distinct rejection messages; the
`rebase.log.*` family gives a full undo lifecycle with impossible-and-failed
states for three verbs. That is why so much of §4 can be specified without
guessing — and why the flagged gaps cluster in exactly the places where the
product is genuinely simpler than its documentation (onboarding) or genuinely
unfinished (the diff viewer's own failure states).

---

## 2. Method

### 2.1 What was harvested

```
research/evidence/rebased/
  MANIFEST.txt            the exact build
  actions.txt             1821 action ids declared by the bundled plugins
  actions-labeled.tsv     id | label | description | key  (Git/Vcs/Diff subset)
  keymap-bindings.txt     402 default keystrokes
  keymap-default.xml      the keymap source
  strings/                GitBundle (1933), VcsBundle (1338), VcsLogBundle (362)
  settings-labels/        20 searchable-option sets
  docs-text/              20 verified documentation pages (13 at harvest + 7 recovered 2026-10-05/06)
  parity/NN-<surface>.md  the eleven evidence files this document summarises
  HARVEST-REPORT.md       what resolved, what did not
  SOURCES.md              which online sources work, and which are dead ends
```

Verified documentation: **17 of 17 pages resolved.** The harvest's one failure,
`compare-file-with-diff` → HTTP 404, has since been **closed** — see §2.1a.

### 2.1a Closing the one 404 — it was a renamed page, not a dead end

`SOURCES.md` listed `compare-file-with-diff` among the dead slugs, and
`HARVEST-REPORT.md` recorded it as the harvest's only unresolved page. **It is not
dead; it was renamed.** Verified 200 on 2026-10-05:

| Old slug (404) | Current slug (200) | Last modified |
|---|---|---|
| `compare-file-with-diff` | **`comparing-files-and-folders`** — "Compare files, folders, and text sources" | 03 September 2026 |

`differences-viewer.html` is live and unchanged (17 August 2026, the version the
harvest already read). The content the 404 stood in for now lives in
`comparing-files-and-folders`, which cross-links **both** ways with
`differences-viewer` and with a third page, `comparing-file-versions`
("Compare file and folder versions using Diff viewer").

**The lesson for `harvest.py`:** treat a 404 as *possibly renamed* until checked,
not as a dead end. The tell was that the surviving `differences-viewer` page's
"How it works" footer links outward to comparison pages that were not in its own
topic — so the content had moved, not vanished. A 404 plus an outbound link from a
live sibling is a rename until disproved.

**Three `UNVERIFIED` items on `11-diff-viewer` are now answered:**

1. **Three-file comparison exists.** The page heading is "Compare two **or
   three** files", and the route is the context-menu action `Switch to
   Three-Side Viewer` — "available only when you are comparing two files against
   each other. It allows you to **load a third file and add it to the
   comparison**." This is the human-facing half of the `Diff.ComparePartial.*`
   triple in the bundle, so those three action ids are **user-reachable, not
   internal-only** — answering a §7.5 capture item directly.
2. **Swap sides is a named toolbar control** (`Swap Sides` button), documented for
   both the file case and the text-source case; the three-file case reuses it.
3. **`Open Blank Diff Window`** via `⌃⇧A` Find Action, for pasting or dragging
   arbitrary text into both panes.

Plus one item the harvest could not have caught: **`External Diff Tools`
configuration** — `Tools | Diff & Merge | External Diff Tools`, an
`Enable external tools` checkbox, a tool table (group / program path / argument
pattern with `%1` local, `%2` remote, `%3` current), and `Test Diff` /
`Test Three-Side Diff` buttons. Launchable from the viewer via one icon. This is
the concrete content behind `Diff.ShowInExternalTool`, which the spec had listed as
a bare action id with no documented behaviour.

**Deliberately not chased:** `command-line-differences-viewer` and
`command-line-merge-tool` (terminal integration — §8.3 excludes),
`comparing-deployed-files-and-folders-with-their-local-versions` (deployment model
— §8.3 excludes), and `differences-viewer-for-folders` (a folder-compare surface
TurboGit has no counterpart for; noted, not specified).

### 2.2 Source precedence

1. **The bundle wins on names, ids and keystrokes.** It comes from the exact
   installed build; the docs describe a moving version.
2. **The documentation wins on UX and flow.** Those exist nowhere else.
3. **Docs mangle Mac modifiers.** `Ctrl0K` is `⌘K`, `Alt09` is `⌥9`, `Ctrl+A`
   is `⌘A`. Converted throughout.
4. **Neither source ⇒ `UNVERIFIED`.** 82 such flags remain across the eleven
   files, down from the original 106 after the 2026-10-06 re-test (§7.1b) closed
   the rest. Never a plausible-sounding guess.

### 2.3 A harvest caveat worth recording

Literal grepping the bundles for a documented label can produce a **false
negative**, and during this pass it did — repeatedly and informatively.

`Force Checkout` appears in `GitBundle.properties:1340`. `Smart Checkout` appears
**nowhere**. `Apply Changes`, `Save and Close`, `Apply Non-Conflicting Changes`
and `Revert conflict resolution` all appear nowhere. Two of those are real
(their documentation wording is a paraphrase); two are the product's actual
buttons under different names; and two — `Smart Checkout` and the non-conflicting
apply actions — are **resolved at runtime from templates**:

```
smart.operation.dialog.north.panel.label.shelf.text
  = "Your local changes to the following files would be overwritten by {0}.
     {1} can shelve the changes, {0} and unshelve them after that."
smart.operation.dialog.ok.action.shelf.description
  = "Shelve local changes, {0}, unshelve"
smart.operation.dialog.operation.name.and.overwrite.local.changes
  = "{0} and overwrite local changes"
smart.operation.dialog.don.t.operation.name
  = "Don't {0}"
```

with `checkout.operation.name=checkout` (GitBundle:1341) as the `{0}`. So the
button reads `Smart checkout` because the template says `Smart {0}`. **Anyone
re-harvesting this bundle must follow the `smart.operation.dialog.*` family, not
literal-grep**, or the single most-discussed behaviour in the product disappears.

The same pattern explains `Conflicts`: the docs name the commit buttons `Apply
Changes` and `Save and Close`; the bundle has `Review Changes`
(VcsBundle:575) and `Accept and Finish` (VcsBundle:594), with a *close*
confirmation (`Discard Changes?` / `Continue Merge`, VcsBundle:583-586) rather
than a close button. The bundle wins; `review-project-history` tickets adopt its
names.

### 2.4 What could not be read at all

`SOURCES.md` Tier 2 records the decisive negative: `plugins/git4idea/src`
contains only a `.keep`, the repository has been restructured into
frontend/backend/shared, and a recursive tree query over 48 315 entries has
**zero** hits for `BranchPopup` or `git4idea/ui/`. The local bundle ships
`intellij.vcs.git.frontend.jar` — exactly the module whose sources are absent.

**So menu ordering — the one thing neither the bundle nor the docs guarantees —
cannot be established from any available source.** Every surface's divergence
table records this. Runtime capture is the only route, which is why each evidence
file's capture list leads with its ordering question.

### 2.5 TurboGit baseline

| Layer | Count | Where |
|---|---:|---|
| `Operation::*` variants | **10** | `crates/turbogit-app/src/operation.rs:78-149` |
| `OpKind::*` variants | **10** | `crates/turbogit-app/src/operation.rs:27-42` |
| `Dialog::*` variants | **16** | `crates/turbogit-app/src/state.rs:468-501` |
| `RebaseKind::*` | 4 | `crates/turbogit-app/src/operation.rs:51-66` |
| `Tab::*` | 5 — Commit, Log, Branches, Worktrees, Submodules | `crates/turbogit-app/src/state.rs:374-382` |
| `CommitSubTab::*` | 3 — LocalChanges, Shelf, Stash | `crates/turbogit-app/src/state.rs:385-392` |
| `PendingConfirm::*` | 10 | `crates/turbogit-app/src/state.rs:623-666` |
| `HistoryVerb::*` | 2 — Drop, Reword | `crates/turbogit-app/src/state.rs:522-528` |

`Operation::*`, in source order: `Fetch`, `Merge`, `Rebase`, `DeleteBranch`,
`Shelve`, `WorktreeAdd`, `WorktreeRemove`, `DropCommit`, `RewordCommit`, `Custom`.
`OpKind::*` is the same ten with `Custom` renamed `Other`.

> **A correction to the brief.** It states "14 `Operation::*` variants" and "20
> `Dialog::*` variants". The code has **10 `Operation::*`** and **16
> `Dialog::*`**. The likely source of both miscounts: `DialogState` is a payload
> **struct** sharing the `Dialog` enum's name, and `Operation` has associated
> constructor functions (`shelve`, `drop_commit`, `reword_commit`, `rebase_*`) that
> read as variants when scanning.
>
> **A correction to an earlier draft of this document**, which claimed 12
> `Operation::*` by counting the constructors. Re-verified by enumerating
> `operation.rs:78-149` directly: it is 10. Recorded rather than quietly fixed,
> because a parity document that misstates its own baseline is not auditable.
>
> **The two enums are not parallel lists**, and §5 must not imply otherwise.
> `Operation` has no variants for `Tag`, `Stash`, `NewBranch`, `CherryPick*`,
> `RenameBranch`, `CompareBranches`, `ManageRemotes` or `Reword` — those go
> through `Operation::custom` (`operation.rs:153-163`) or a settlement-free
> file-write path. So "every `Dialog` has an `Operation`" is false in both
> directions.

---

## 3. Copy / do-not-copy

| Copy from Rebased | Do **not** copy | Why |
|---|---|---|
| Panel topology; every item a surface offers | Darcula colours, insets, radii, row heights | Finish is not parity; `theme.rs` owns TurboGit's tokens |
| Menu and context-menu ordering and grouping | Their icon set | Their glyphs are not TurboGit's system; `icons.rs` uses Lucide |
| Default keystrokes — `⌘⌥N`, `⌥M`, `⌃⌥Z`, `⌃⇧D`, `⌘⌥R`, `⌘⌥A`, `⌃⇧⇥`, `F2`/`⇧F6`, `⌥⇧H`, `⌃⌥U`, `⌥⇧M` | macOS-native chrome egui does not own | No `NSMenu`, no `NSPopover`, no reordering drag |
| Dialog field order, defaults, validation timing | IDEA inheritance: editor, terminal, debugger, build-tool prompts, trust prompt, WSL detection | Not a Git client |
| Terminology: Changelist, Shelve, Amend, Smart Checkout | Their literal string files and images | Rule 6 of `DISTILL.md` |
| Empty, progress and error states — **all of them, and their recovery actions** | Their accessibility gaps | TurboGit's own R4 exists because a11y is weak everywhere |
| Their *asymmetry* where it is deliberate (six rejection messages, not one) | Their *inconsistency* where it is a bug (shelf `Delete` vs stash `Drop`) | Copy decisions, not accidents |

### 3.1 Where egui cannot match the mechanism

`DISTILL.md` requires matching intent and recording the divergence rather than
silently dropping it. Six places, consolidated:

| Rebased mechanism | Why egui cannot | Intent matched by | Recorded in |
|---|---|---|---|
| Popup is a floating `NSPopover` anchored to the header widget | No native popover with a live tree + gear menu | Branches tab + status-bar popup, same four nodes, same toggles, same `⌘⌥N` | `02` UI |
| Branches node changes **name and shape** with the sync setting (`Common Remote Branches` vs `Repositories \| Remote Branches`) | — | One shape (remote rollup), difference deferred to expansion | `02` UI |
| Changelists are **drag-reorderable**; a `⌃`-drag between tabs copies instead of moves, with a 1s activation delay | No drag-and-drop of tree rows between tabs; the delay is an artifact of hover-activated tabs | Same items reachable by menu. **The `⌃`-copy semantics and the delay are deliberately not reproduced** | `07` UX |
| Commit tool window can be **floated, windowed, made modal, or dissolved** into a Log tab | egui panels are not free-floating | Fixed left panel; topology preserved | `01` Flow |
| Diff viewer opens in an **editor tab or a separate window** | No second OS window; `Diff.OpenDiffInEditor` exists but cannot float | Two actions instead of two settings — *better* than the reference, which has both | `11` UI |
| `⌘K` selects the active **changelist**, and there are many changelists | No changelist model | Staging buckets + per-root subgroups; the *intent* (group local work, commit separately) holds | `01` UI |

---

## 4. The surfaces

Ordered by evidence richness, as the brief requires. Each subsection states the
three dimensions and links its evidence file.

### 4.1 `05-conflicts` — [full evidence](../research/evidence/rebased/parity/05-conflicts.md)

**UI.** *Two* surfaces, not one. A **Conflicts dialog** — a file list with a
per-row trio (`Accept Yours` / `Accept Theirs` / manual) and a toolbar carrying
`Diff.ApplyNonConflicts`, `.Left`, `.Right` plus `Resolve All Simple Conflicts`; and
a **three-pane merge tool** whose centre pane starts from the **base revision**,
not from either side (`resolve-conflicts.md:26`). Seven conflict *kinds* are named
in the bundle (`both deleted`, `both added`, `both modified`, `deleted by you`,
`deleted by them`, `added by you`, `added by them`). Keystrokes `⌘⌥R` / `⌘⌥A` for
apply-side, `⌃⇧⇥` to switch panes.

**UX.** Conflicts arrive from **six** operations — `pull, merge, rebase,
cherry-pick, unstash changes or apply a patch` (`resolve-conflicts.md:9`) — so a
conflict can come from a *recovery* action, not only an integration one. A closed
dialog leaves a `Merge Conflicts` node in the Commit tool window with a resolve
link. Two bulk buttons differ **deliberately** in scope: `Apply Non-Conflicting
Changes` *skips* simple conflicts "since you must make sure that they are resolved
properly", while `Resolve All Simple Conflicts` handles them. Three confirmations
exist, and the safe option is always the one that does nothing
(`Continue Merge`, `Keep Editing`). Close-with-partial-work is refused by default.

**Flow.** Six conflict sources → dialog → `Resolve Manually` → 3-pane tool →
`Review Changes` → next file (focus advances *into* the next tool) → `Accept and
Finish`. **Smart Checkout can chain into this surface from a checkout dialog** and
conflict during the unshelve.

**The behaviours worth stealing:** the centre pane seeded from base; the two
bulk actions with honestly different scopes; progress strings with real
pluralisation (`Resolving {0,choice,1#Conflict|2#Conflicts}`); and a *negative*
result reported rather than left silent (`No conflicts were resolved
automatically`).

### 4.2 `02-branches-popup` — [full evidence](../research/evidence/rebased/parity/02-branches-popup.md)

**UI.** Four fixed nodes — `Recent`, `Local Branches`, `Remote Branches`, `Tags`
(`manage-branches.md:59-67`) — plus incoming/outgoing counters on the branch name,
a search field (`⌃F` returns focus to it), and a **settings gear with four
toggles**: `Group by Directory`, `Group by Repository`, `Show Recent Branches`,
`Show Tags`. Directory grouping keys on the `/` prefix. Favourites pin to the top
and **the main branch is favourited by default**. Per-row menu is larger than the
docs' four items — the bundle supplies 15+ (`Checkout`, `Checkout and Rebase onto
Current`, `Compare with Current`, `Show Diff with Working Tree`, `Merge into
Current`, `Rebase Current onto Selected`, `Update`, `New Branch from {0}…`, `New
Worktree from {branch}`, `Pull Into Selected Using Merge`/`Rebase`, `Delete`).

**UX.** Four distinct destructive decisions, each with its own policy:

- **Dirty checkout** → `Force Checkout` (loss) vs `Smart Checkout` (shelve →
  checkout → unshelve, **which can itself conflict**). The mechanism — shelve or
  stash — is a setting (`Clean working tree using`; the default is *not*
  established — the Shelve-first inference was withdrawn, §7.1b), and the
  whole dialog is templated (§2.3).
- **Tracking divergence** → silent auto-reset if *nothing can be lost*; otherwise
  `Drop Local Commits` vs `Rebase onto Remote`.
- **Delete branch** → **immediate, forced, no confirmation**, equivalent to
  `git branch -D`, with post-hoc disclosure: a restore link, a link to view the
  unmerged commits (`Branch Was Not Fully Merged`), and a link to remove the remote
  branch. **Restore survives in the Notifications tool window until the app
  restarts** — a hard, citable bound.
- **Delete remote branch** → *does* confirm, with a count-pluralised question.

**Flow.** Create-from-current / from-selected / from-commit → one dialog with a
live prefix suggester and a `Checkout branch` checkbox. Checkout of a remote →
auto-reset *or* a two-way choice. Dirty checkout → the templated decision, whose
Smart branch chains into conflicts. Delete → immediate, then a notification with
three independent links. Multi-root divergence → a `Branches have diverged`
warning in the widget.

**The behaviours worth stealing:** the Recent node capped at **five** — **landed
2026-10-07** by ticket 03 (`d99df82`), which took the cap and the reflog that
feeds it (see §5.2) — and the `Show Recent Branches` toggle that gates it here,
**landed 2026-10-07** by ticket 04 (`612975b`) with the other three behind the
surface's own gear (see §5.2); the rename dialog's second field, `Unset upstream branch` —
**landed 2026-10-07** by ticket 05 (`51a777c`), with the consequence the user
cannot see from a rename spelled beside it; delete that asks *after* and gives
three recoveries with a restart-bounded lifetime — **the two disclosure links
landed 2026-10-07** by ticket 07 (`91cf199`), on TurboGit's deliberate *asks
before* shape rather than the reference's ask-after one, so the unmerged-commits
link sits in the pre-flight confirmation *and* in the notification afterwards,
both opening one viewer from one captured snapshot. What did **not** move is the
window's **lifetime**: TurboGit keeps its own 10-second bound (§7.6 q6, reverse
gap §6 #20) rather than the reference's "until the app restarts", and ticket 07
deliberately left it alone. Gap 4's multi-root failure policy that offers to
**roll back the roots that succeeded**, specifically "not to let branches
diverge" — **landed 2026-10-09** by ticket 06 (`1c4339b`, split into a record and
an offer by `53c8b94`, merged as `4402cbe`; see §5.2's row). What landed is
**narrower than the reference's one sentence suggests, and deliberately so**: the
divergence is *recorded* for every bulk run that moved some roots and failed
others, but the *offer* to roll back appears only where the app can honour it
(`BulkOp::CreateBranch`), so a `Merge` cascade records its divergence with **no
repair path** — the accepted limitation, and the reason this row is not simply
"done".

### 4.3 `01-commit-tool-window` — [full evidence](../research/evidence/rebased/parity/01-commit-tool-window.md)

**UI.** 56 actions — the largest set. The defining pair is a **user-facing mode
switch**: `Git.Stage.Enable` ("Use the native Git concept of staging for making
commits") vs `Git.Stage.Disable` ("Disable staging area and switch to
changelists"). Regions: sub-tab strip (Commit / Shelf / Stash / Console,
mergeable), changes tree, file rows, commit card, commit controls, diff preview.
Six commit options behind **one keystroke** (`⌃O`) in three groups. *(Runtime
correction, R3 session 3: build 1.1.19's popup carries a `Git` group — Author
field, Sign-off commit — and a `Commit Checks` group; the docs' six IDE checks
are absent from the product, consistent with §8.3 #1.)* The bundle
ships **two label variants** for the commit button — `commit.action.name=Comm&it`
and `commit.action.name.mac=Commi&t` — so any parity claim about it must name the
platform. Note `Changes` is the name of the *default changelist*, not the view.

**UX.** `⌘K` commits the active changelist; `⌃␣` completes file names in the
message; `⌥M` *toggles* amend mode. The preflight pattern is the **policy-verb,
not the dialog**: three separate classes of loss each carry a three-way
`Ask / Remove silently / Do nothing` setting. Undo boundary is **unpushed**, and
the post-commit notification carries `Edit Commit Message` with its own undo whose
two failure modes are separately named. Commit-and-Push moves checks *earlier*
rather than skipping them.

**Flow.** Plain commit may chain through a `Name/E-mail` dialog and a
`Save Files During Commit` dialog whose safe option is `Postpone Save`. Partial
commit is **three granularities** — file → chunk → line — the third reachable two
ways. Staging mode inverts the gesture: *edit the staged version* rather than
select changes. Commit-and-push chains three dialogs deep, and every link is a
real escape route.

**The behaviours worth stealing:** a clean filename-completing commit field; the
**partially-cancelling** dialog (`Cancel` suppresses force-adding ignored files but
"all other files will be added"); and `Commit All when Nothing Is Staged` as a
forgiveness setting.

### 4.4 `06-history-rewrite` — [full evidence](../research/evidence/rebased/parity/06-history-rewrite.md)

**UI.** 10 actions, two of which hide three more (`Git.Fixup.To.Commit` vs
`Git.Squash.Into.Commit` fold *staged work into an earlier commit*;
`Git.Squash.Commits` combines *selected commits* — conflating them loses half the
surface). The plan editor is `Rebasing Commits`, with columns `Action | Hash |
Subject` and **seven** verbs from `rebase.entry.action.name.*`: Pick, Edit, Reword,
Squash, Fixup, Drop, **Update Ref** — plus an `Unknown` fallback. Regions include a
**graph preview of the resulting history**, a `View Git Commands` raw-todo view, a
whole-plan `Reset`, and `Start Rebasing`. The rebase options dialog has
**per-field validation** and `--onto` as **two ordered fields** (new base, old
base).

**UX.** Every verb is destructive, with per-verb copy: the drop confirmation names
the **branch in bold**, and detached HEAD gets its **own sentence** rather than a
parenthetical. Gating is a **state machine** — `Cannot {0} during
cherry-pick / merge / rebase / revert` — plus three repository-state reasons and
two multi-root reasons. `Pick` on a modified row **resets that row**.

**The undo lifecycle is the finding.** Three verbs have full, explicit
lifecycles, each with success, in-progress, impossible and failed states:

| Verb | success | undo in progress | undo impossible | undo failed |
|---|---|---|---|---|
| Drop | `Dropped {N} {commits}` | `Undoing drop…` | `Cannot undo drop` | `Undo drop failed` |
| Reword | `Commit message changed` | — | `Cannot undo commit message edit` + **"Repository has already been changed"** | `Undo commit message edit failed` |
| Squash | `Commits Squashed` | — | — | — |

**The reword guard is the important one:** the undo's validity is checked *at undo
time*, not assumed at reword time — strictly stronger than a pre-flight model. And
coverage is **asymmetric**: squash is not undoable by name. A client must not
assume "rewrite ⇒ undo".

**Flow.** Reword is a mini-editor whose OK reads `Continue Rebasing`; interactive
rebase can pause at a `Stop to Edit` row; **fixup/squash-into require staging in
the Commit window and then choosing the verb in the Log** — the only two-surface
sequence here. `Squash` and `Fixup` share one split control. The push of a
rewritten history is *always* the named next step.

### 4.5 `07-shelve-stash` — [full evidence](../research/evidence/rebased/parity/07-shelve-stash.md)

**UI.** 18 actions across **two separate namespaces** — `Vcs.Shelf.*` and
`Git.Stash.*` — with separate tab roots (`Shelves` / `Stashes`), separate failures,
and separate semantics. The distinction the docs state: a shelf is IDE-generated
and applies **some** selected changes; a stash is git-generated, applies **all**
uncommitted changes, and works outside the IDE. Tabs can be **merged into one**.
Shelve dialog: file list (all selected) + a live "N of M will be shelved" summary,
toolbar, changelist selector, message field (= the shelf name), diff pane.
Unshelve dialog: Name (existing **or** new — one field, two roles), Comment, Set
active, Track context, Remove-applied-from-shelf. Stash dialog: **Git root
selector**, branch readout, Message, Keep index.

**UX.** `Shelve Silently` derives the shelf name from the changelist, and the docs
name the consequence: numerous shelves called `Default`. A third verb, `Save to
Shelf`, copies without resetting. Destructive surface: shelf `Delete` and stash
`Drop`/`Clear` carry **no confirmation at all** — the mitigations are structural
(`Unshelve` keeps by default; the destructive sibling is named `Unshelve and
Remove`; already-unshelved files live in `Recently Deleted` until explicitly
deleted). Shelving has a **third role** the docs nearly hide: `Shelve base
revisions` exists so a later 3-way merge has a base — the only thing that survives
a rebase that removed it.

**Flow.** Dirty tree → Smart Checkout → *this surface* → conflicts. Patch →
`Import Patches…` → a shelf → `Unshelve Changes` — three homes for one patch.

**The behaviour worth stealing, and one to refuse.** Steal: `Import to shelf` as a
first-class deferral, and the shelf name derived only on the silent path. **Refuse
to copy:** the conflicted-unstash instruction "Resolve conflicts and **drop the
stash**" — the stash is the only copy of the pre-update work, and telling the user
to delete it is a hazard, not a design.

**Landed on this surface: the panes, the verbs, and the Unshelve dialog (2026-10-10).**
Ticket 12's two halves are both in: the Shelf/Stash panes and their row verbs
(`1215d09`), then `Dialog::Unshelve` (`c6b00c9` + `9103ba9`, merged as `d1a39ba`). The
dialog keeps the reference's **`Remove successfully applied files from the shelf`** —
off on every open, with the consequence stated beside the tick and one affirmative that
renames and recolours itself — and re-points the reference's overloaded **`Name`** field
to the only question this app can actually ask: *which of this root's shelves to apply*.
The other four field roles (`Name` as a new changelist, `Comment`, `Set active`,
`Track context`) are **divergences by decision rather than gaps** — the reference's own
text binds each one to a **changelist**, and this programme keeps the changelist model
out of scope, so there is no destination for the dialog to name and no consumer for a
comment to reach. The mitigations the reference relies on for this destructive surface
are intact: the keep default is carried in the label and the fill rather than a tooltip,
and the shelf record drops only once git's apply has landed. See §5.5's Unshelve-dialog
row for the shape and the two recorded costs (no golden capture, and an absence check
too case-sensitive to catch an uppercase micro-header), and §8.4 gap 12 for the rest.

### 4.6 `08-remotes-sync` — [full evidence](../research/evidence/rebased/parity/08-remotes-sync.md)

**UI.** Three actions for push — `Vcs.Push` (dialog), `Vcs.Push.Simple`,
`Vcs.Push.Force` — and whether the dialog appears is a *setting*. The Push Commits
dialog: repository list (**only the current repository selected by default**),
commit list with per-commit checkboxes, right-hand preview, target panel with
**five named states** (`New`, `New & Upstream`, `Set upstream`, `Upstream`,
`Empty repository`) plus inline `Define remote`. Counters on the widget.

**UX.** The three verbs are ranked by safety in the docs themselves: fetch "does
not affect your local development environment, this is a safe way to get an
update"; pull "integrate[s] it into your local working copy"; update does both
across all roots. Push rejections carry **six distinct notifications, four with an
explicit next action** — including a separate `Force-with-lease push {0} to {1}
was rejected`, so the product distinguishes "the remote refused" from "your lease
is stale".

**The three-layer force-push safety model is the best safety design in the
product:** (1) it is a dropdown item, so plain Push is the default; (2) it is
*absent* on a protected branch, not warned about; (3) underneath it is
`--force-with-lease`.

**Flow.** A successful update **navigates by default** — the `Update Info` tab
opens automatically listing everything committed since the last sync, reviewable
like the Log. Multi-root sync **skips** unupdatable roots with a named reason
(`no tracked branch`, `detached HEAD`) rather than failing.

**That skip policy and the branch-operation rollback policy are opposite
theories of failure on the same product, and both are deliberate.**

TurboGit now runs **both**, and ticket 06 (`4402cbe`) is the change that kept them
apart rather than merging them: the preflight's `Skipped` rows (this surface's
sync family) arm nothing at all, while a divergence is recorded only from roots
that actually *disagree* — one changed, one failed. `branch_divergence_for`'s
comment says so at the point of decision, precisely so a later reader does not
"unify" the two and lose either the skip reason or the divergence. See §5.2's row
and §8.4 gap 4; there is no divergence row on this surface's own §5.6 table.

### 4.7 `03-log-tool-window` — [full evidence](../research/evidence/rebased/parity/03-log-tool-window.md)

**UI.** Four panes, and **the right side is a vertical split**, not a tab pair:
Branches (left), Commits (centre), Changed Files (right upper), Commit Details
(right, *under* it). Tabs are **per-query**, created by "open a new tab matching
your filters" — with a guard against duplicates. Per-commit encoding is **five
independent signals**: ref colour (yellow/green/violet), background (current vs
other), bold (mine), asterisk (author ≠ committer), root stripe (multi-root).

**UX.** Almost nothing here is destructive, which is why this is a safe home for
verbs that are destructive elsewhere. The gating convention is the point:
**safety lives in enablement, not confirmation** — cherry-pick disabled when
already contained, Undo Commit disabled unless yours, Edit Commit Message disabled
once pushed, and a confirm dialog only where a *mode* must be chosen (Reset).

**The state machine is the best-evidenced in the harvest** — ten named states, and
**every non-terminal state carries its own recovery action**:

| State | String | Recovery offered |
|---|---|---|
| empty, no history | `No changes committed.` | **`Commit local changes`** |
| empty, filters excluded all | `No commits matching filters` | `Reset filters` |
| error, commits | `Error loading commits` | `Refresh` |
| error, details | `Error loading commit details` | `Refresh` |
| error, changes | `Error loading changes` | `Refresh` |
| error, filtering | `Error filtering commits` | `Reset filters` |
| found but filtered out | `{0} doesn't match the filters` | **`View and reset filters`** *or* **`View in new tab`** |

**Landed 2026-10-07** by ticket 15 (`3ec61e2`) as `LogState` in
`turbogit-app/src/log_states.rs`: **thirteen** named states — four loading, five
failures each naming the read that died, three empties, plus `Changes log` as the
named default so `named_states()` can census every one — with the recovery each
state offers painted by `log_window.rs::log_state_slot` and dispatched by
`apply_log_recovery` (see §5.7, §8.4 gap 14). TurboGit's Log is **one** tab with
one filter set, so the reference's `View in new tab` reads here as **`View in
Diff`**; that escape pair ships but is **untested at every layer**, so it is
recorded as shipped-unproven, not as landed.

**Flow.** The audit tail closes the conflicts loop: a merge commit with resolved
conflicts gives a **three-panel** diff of the result against *each* parent; without
resolved conflicts, two panels.

### 4.8 `04-integrate` — [full evidence](../research/evidence/rebased/parity/04-integrate.md)

**UI.** 10 actions, **five of which are operation-lifecycle** (Abort / Continue /
Skip across three operations, plus `Commit merge`). That ratio is the finding: the
surface's real job is managing operations that failed partway. The doc frames
**five** integration verbs, not three — adding *apply separate changes from a
commit* and *apply a specific file to a branch*. **The option vocabulary is
complete**, every flag labelled with its effect: merge has 7, pull has **5** and a
**different set** (`--rebase` yes, `--no-verify` and `--allow-unrelated-histories`
no), rebase has 8. Four row-menu fast paths skip the dialogs entirely.

**UX.** Confirmation is **situational, not blanket**: a clean merge is *silent*; a
dirty merge offers Smart merge; a conflicting merge goes to resolution; a
protected branch refuses; a selection spanning repositories is refused with the
reason. `Skip Commit` is a **third** resolution for a paused rebase, distinct from
Abort and Continue — and it is the one you want when the *next* commit conflicts.

**Flow.** `checkout the target branch first, then cherry-pick` — two steps, stated
at length. A **failed cherry-pick stays applied** as uncommitted changes,
"review these changes and commit them later" — **an integration failure degrades
into a review task rather than an error state.**

### 4.9 `10-patches` — [full evidence](../research/evidence/rebased/parity/10-patches.md)

**UI.** 6 actions, four routes, two dialogs. Patch File Settings: location (or
`To clipboard`), base path, **Reverse patch**, encoding. Apply Patch: file path,
`Show Diff` as a *preflight check*, `Change Directory Paths` (a counting repair
pair: `Remove Leading Directory` ×N with **the count in square brackets**, plus
per-step `Restore`), changelist destination, Set active, Track context,
`Analyze … on the fly`, **`Import to shelf`**. A patch has **four homes**: file,
clipboard, shelf, lazy clipboard detection.

**UX.** **Preflight instead of confirmation**, and consistently so: `Show Diff`
with conflicting lines in red *is* the confirmation. `Import to shelf` is the
non-destructive exit.

**The undo model is the strongest in the harvest, and it is entirely in the
bundle:** apply has an **atomic, total rollback** — "All files changed during
'Apply Patch' action were rolled back" — with a separately named terminal state
`Rollback failed`. Contrast with the stash, whose conflicted-unstash recovery
*requires* dropping the only copy: **the two mechanisms differ precisely in
whether the original is recoverable.** Naming the rollback-failure case is what
makes the guarantee honest. **Half of this landed 2026-10-09** by ticket 18
(`fa65cbc`, record lifetime fixed by `18113f1`): the total rollback, both titled
states and the named recovery are TurboGit's now, on the `From file` and
`From clipboard` routes, with `Show Diff` as the preflight the reference uses
instead of a confirmation (see §5.9). **`Import to shelf` — the non-destructive
exit this surface prefers — landed 2026-10-10 with ticket 18c** (`77a6c92`..`8b83775`,
merged as `f2a770d`): the imported patch is parked as **its own bytes** at
`<project>/.turbogit/shelf/<slug>-<nanos>.patch`, addressed by
`Shelf.source: ShelfSource` (`Stash` | `Patch { file }`). **The design is the
rationale, not an oversight**: a stash can only exist for work already in a tree, so
stashing an unapplied patch would mean *applying* it — the very act the deferral
defers — hence a file, not a stash entry, and no new `Operation`/`OpKind`: the patch
arm is a field of `Operation::Unshelve`, and `AppState::unshelve` branches on
`source` (`Stash` → the existing stash-by-name path, unchanged; `Patch` →
`patch_apply::apply_patch` on **that row's own root**). **The create direction and Patch File
Settings landed 2026-10-10** with ticket 18b (`2bed4ce`, merged as `76270a9`): the
five create routes, the four settings fields in the reference's order, the counting
repair pair with its visible count, `Reverse patch` as git's `-R`, and the
dialog-free clipboard verb — the patch arriving as **git's own bytes** across a
`Vec<u8>` port rather than as a `String`. Ticket 18c also landed the
**`When patch is created:` policy** (`Ask / Show in {0} / Do nothing`, all three arms
enforced in one place — `report_created_patch` — with the captured default `Ask`
carried by `serde(default)` and persisted, see §5.9). What remains in the bundle is
the **changelist destination**, the **4-pane** conflict view, `filetype.patch`, and
the reference's **apply-side `Change Directory Paths`** — 18b's counted repair pair
lives on the create window, and `apply_patch.rs` carries no repair affordance at all.

**The behaviours worth stealing:** path repair by counting segments with a visible
count; a git-format patch's author and message **auto-filling the destination
changelist**; and `When patch is created:` as a three-way `Ask / Show in {0} / Do
nothing` policy.

### 4.10 `09-onboarding` — [full evidence](../research/evidence/rebased/parity/09-onboarding.md)

**UI.** **2 actions** — the smallest set, and that is the finding. Sorting the
doc's four routes by whether they are Git or IDE inheritance leaves: clone (two
columns — services left, URL right — plus a shallow checkbox whose unit is in the
*label*), init (one directory field), and the welcome screen. The trust prompt,
build-tool prompt and share-settings prompt are **excluded** as IDE inheritance.
**The Git surfaces do not exist before init** — "Now that you have created a new
project with a Git repository, dedicated tool windows for working with Git become
available."

**UX.** Almost nothing is destructive; the verbs *create*. The one Git-specific
preflight on the surface is `init.warning.already.under.git` — a titled warning
when the target directory is already inside a repository, which is the one mistake
that produces a nested, confusing repo. **`Enable Version Control Integration`
disappears** when Git is already enabled: absence is the "already done" signal.
A missing git binary is an **offer to download**, not an error.

**The behaviours worth stealing:** `Checking Git version…` **inside the clone
dialog, before any work**; `Nothing to commit, working tree is clean` as a
*status-bar statement* rather than an empty panel; and the trust prompt's safe
option whose own label states its cost — "Safe mode, **Version Control is not
available**".

### 4.11 `11-diff-viewer` — [full evidence](../research/evidence/rebased/parity/11-diff-viewer.md)

**UI.** **42 actions — the largest set**, and the number is the finding: the Diff
Viewer is a *workbench* doing three jobs (review, stage, resolve), not a viewer.
Two mode switches (side-by-side ⇄ unified; editor tab ⇄ window) and **two complete
five-value policies** in one `⌃⇧D` menu: whitespace (`None` default, `Trim`,
`Ignore`, `Ignore … and empty lines`, `Ignore imports and formatting`) and
highlighting (`Words` / `Lines` / `Split changes` / `Characters` / `None`). Two
editing rules: only the **right part / lower line** is editable, and only **local**
files. The same three-colour grammar is reused for branch-comparison file status.

**UX.** Dangerous operations look harmless and are handled with **previews, not
confirmations**. The best single interaction: the accept chevron is **one control
whose meaning changes under a modifier** — "Click and to apply changes. This
behavior is the default one. Press `Ctrl` to change … and append changes."
Undo is `⌃Z` / `⌃⇧Z` with the load-bearing rider **"Conflicts will be kept in sync
with the text"** — a real, testable invariant. `Disable editing` defaults to **on**,
so the viewer opens read-only and editing is opt-in.

**Flow.** `⌘D` is the same keystroke on **six** surfaces. Staging is available two
ways (apply from the viewer, or tick in the Commit window) plus the three-way
"edit the staged version" route. The viewer is the preflight for a destructive
operation on *another* surface (`Show Diff` in the Apply Patch dialog).

**The honest gap: this surface has no documented error or loading state at all.**
Every failure string in the bundles belongs to a different surface. Either the
viewer is genuinely simple or its failures were never designed — and the
distinction matters, which is why it is the first capture item.

---

## 5. Gap list

Rebased surface → TurboGit equivalent → verdict.
**has** = parity · **partial** = the mechanism exists, some inventory or
behaviour missing · **missing** = no equivalent found · **absent** =
intentionally not pursued.

### 5.1 Conflicts → TurboGit

| Rebased | TurboGit | Verdict |
|---|---|---|
| Conflicts dialog (file list, per-row trio) | `conflict_resolver.rs` dedicated tool window + `conflicts.rs` inline editor | **has** — same topology, our chrome |
| Three panes, centre editable | `Local \| Result \| Incoming`, three equal panes | **has** |
| `Accept Yours` / `Accept Theirs` | `Take ours` / `Take theirs` / `Take both` ×2 orderings | **has** — superset (4 vs 2) |
| Centre seeded from **base** | Result is a user composition | **partial** — deliberate divergence |
| `Apply Non-Conflicting Changes` (all/left/right) | — | **missing** — no equivalent anywhere in `crates/` |
| `Resolve All Simple Conflicts` | folded into one `Apply to all N remaining` | **partial** — deliberate divergence |
| Conflict kinds (7 named) | `ChangeStatus::Conflicted` letter | **partial** — one kind, not seven |
| `Merge Conflicts` node after dialog close | `Merge conflicts` group in the Commit window | **has** |
| `Revert conflict resolution` per file | per-conflict Undo in the resolver | **has** |
| `Review Changes` / `Accept and Finish` | resolver auto-advance + header Continue/Abort | **has** |
| `Discard Changes?` on close-with-work | confirm path | **partial** — not verified |
| `Cannot Resolve Conflict` / `Cannot find file for {0}` | — | **partial** — error surface not specified |
| Conflict *pause* notification with Show/Cancel | `conflict_propagation.rs` | **partial** |
| **No `apply non-conflicting` equivalent** | — | **missing** — named in the reverse list as a candidate |

### 5.2 Branches popup → TurboGit

| Rebased | TurboGit | Verdict |
|---|---|---|
| Four nodes: Recent / Local / Remote / Tags | `branches_tree.rs::build_recent_group` (Recent, first view-level group, above the repo sections) + repo sections + Local/Remote/Tags | ~~**partial** — **no `Recent` node in the Branches tab**; Recent exists in the status-bar popup only~~ **has 2026-10-07**: ticket 03 (`d99df82`) — all four nodes now exist. The Recent group reads git's HEAD reflog through the new `recent_checkouts` port at the engine seam rather than app-side bookkeeping, so it survives a restart and reflects a checkout made in a terminal; three rules shape it in order — join against each repository's *live local* branches (a deleted name or a raw sha yields no clickable row), merge across repositories on the checkout's own timestamp, then cap at `RECENT_CAP = 5`, the cap shared with the status-bar popup so the two cannot drift. ~~**Still open**: the group is unconditional — the `Show Recent Branches` toggle is gap 3~~ **gated 2026-10-07**: ticket 04 (`612975b`) spent that toggle at the renderer (`branch_tree_view.rs:385`), so the group is no longer unconditional. **Still open**: the popup keeps its separate in-process `recent_branches` list |
| Directory grouping on `/` | `DirNode` from slash-separated names, gated by `BranchesViewOptions::group_by_directory` inside `branches_tree.rs::build_tree(…, group_by_directory)` | **has** — and **user-toggleable 2026-10-07**: ticket 04 (`612975b`). Off **flattens rather than truncates**: the same rows come back with full `folder/leaf` names and no directory header, so un-grouping never loses a branch (asserted by `branches_grouping.rs::ungrouped_directory_paints_full_names_and_no_directory_header`) |
| `Group by Repository` | repo-level `RepoSection`, gated at **two** layers: `branches_tree.rs:493` (builder name qualifier) and `branch_tree_view.rs:504` `paint_ungrouped_sections` (renderer) | **has** — and **user-toggleable 2026-10-07**: ticket 04 (`612975b`). The qualifier fires on **ambiguity, not on the toggle**: a name several repositories share gets its repository prepended only when the band that used to say it is gone, so a name unique to one repo stays bare |
| Gear with 4 toggles | `widgets::icon_button(Icon::SETTINGS)` in the Branches toolbar after New Branch / the scope chip, hosting four checkboxes through `menu_host` (`branches.rs:240-256`, `toolbar` at `branches.rs:382`); the previously-dead `BranchesViewOptions` (`persistence.rs:92`) wired into `UiPersist::branches_view` with `#[serde(default)]` (all four on), read in `launch_in`, written the frame a toggle changes via `save_durable_ui`, exposed live as `UiState::branches_view` | ~~**partial** — no gear, no `Show Recent Branches` toggle~~ **has 2026-10-07**: ticket 04 (`612975b`) — all four exist, persist **per project**, and each is spent at the layer that owns its effect: directory in the tree builder, repository at builder + renderer, Recent and Tags as renderer-level group visibility (`branch_tree_view.rs:385`, `:232`). A toggle changes what is *painted*, not what the surface *knows* — Recent and Tags keep their data in the model either way. `build_branch_view` keeps its all-on signature as the wrapper the pure-builder ratchet, the ~28 unit call sites and the Git Log pane's tree all still call unchanged; only the Branches surface calls `build_branch_view_with`. No new theme token (`PAD_STRIP`, `theme::ITEM_SPACING`). **Still open — recorded as divergence, not landed**: the gear's own **composition** is an unverified capture (§8.4; the runtime-capture list at §7.5 names "the gear's layout"). Checkbox vs popover, separators, apply-on-click vs apply-on-dismiss, and whether the two grouping levels compose or exclude each other are each `UNVERIFIED`, so the shipped shape is the simplest defensible reading — four checkboxes in the bundle's own order, independent, applying the frame they are clicked. Labels are TurboGit's own reading of the four `action.git.branches.*` names; no bundle string copied |
| Favourites, main favourited by default | `favorite` pin-to-top on `BranchLeaf` | **partial** — no default-on for the main branch |
| Incoming/outgoing counters on the widget | ahead/behind in the repo header | **has** |
| Search field + `⌃F` to refocus | `widgets::search_input` + fuzzy match | **has** |
| Row menu: 15+ items | `branch_menu.rs` ~~10 items~~ **11 items** — `Show Diff with Working Tree` joined the menu on 2026-10-08, ticket 17 (`0d0186a`) | **partial** — ~~missing `Show Diff with Working Tree`,~~ missing `New Worktree from {branch}`, `Update`, `Pull Into Selected Using Merge/Rebase` |
| `New Branch` with `Checkout branch` checkbox + live prefix suggester | `Dialog::NewBranch` — name, start-from, "Switch to the new branch now" (default yes) | **partial** — no prefix suggester |
| Rename with `Unset upstream branch` | `Dialog::RenameBranch` — the new name plus the `Unset upstream branch` checkbox (rendered only on a branch that tracks something) and a note naming both outcomes: the next push creates a new remote branch, the old one is left behind | **has 2026-10-07**: ticket 05 (`51a777c`). Row added at landing — this gap was carried as ticket 05 (§9), not as a row here. The decision reaches git through the new `GitExecutor::unset_branch_upstream` (cli / git2 / fake), and `branch_service::rename` runs it **after** the rename naming the **new** branch: `--unset-upstream` with no argument targets the *current* branch, and the old name stops resolving the moment `-m` moves the ref. The inline row editor keeps its one-field behaviour (`rename_branch(…, false)`), since a row editor has no room to state a consequence |
| **Dirty checkout: Force vs Smart** | `CleanTreeMethod` per-invocation on `Operation::Merge` | **partial** — the mechanism exists; the *dialog* is not evidenced |
| Silent auto-reset when nothing can be lost | — | **missing** |
| **Drop Local Commits vs Rebase onto Remote** | `RebaseKind::Branch` | **partial** — one side of the choice |
| **Delete: immediate, forced, no confirm** | `Operation::DeleteBranch` force + `PendingConfirm` | **partial** — **deliberate divergence**: TurboGit asks *before* |
| **Restore, lifetime = until restart** | `BranchDeletePending` / `BranchDeleteUndo` | **partial** — lifetime not established |
| Link to view unmerged commits (`Branch Was Not Fully Merged`) | `UnmergedCommits` (`state.rs`) → `Dialog::BranchNotFullyMerged`, opened by `AppState::open_unmerged_commits` from the pre-flight confirmation **and** the post-delete banner | ~~**missing**~~ **has 2026-10-07**: ticket 07 (`91cf199`) — one snapshot record, captured by `AppState::unmerged_commits` **while the ref still exists**, is the payload of both links and of the viewer. That is what makes the two disclosures unable to state the fact differently, and what lets the viewer list commits by a branch name that no longer resolves. `Restore` inside it is gated on the branch actually being gone (`unmerged_restore_available`), so the copy opened from the confirmation lists without offering to restore. TurboGit's ask-first shape means the link appears **before** the loss, not only after |
| Link to remove the tracking remote | `PendingConfirm::DeleteRemoteBranch`, reached from the post-delete banner's quiet link | ~~**partial**~~ **has 2026-10-07**: ticket 07 (`91cf199`) — the remote twin the local delete leaves behind is named by the `upstream` read into `BranchDeletePending` before the ref went away, so the offer is a fact about the delete rather than a lookup after it. Removing it is destructive, so the link routes through the same remote-delete confirmation the list's own `Delete` uses instead of pressing it |
| Multi-root divergence warning + rollback offer | Two decisions, taken at two places, on purpose. `branch_divergence_for` (`state.rs:5990`) reads the completed run's **per-root rows** on the `AppEvent::BulkCompleted` arm (`state.rs:4742`) and, whenever some roots landed and others failed, pushes a durable `Warning` entry into the activity feed through one builder for both sentences (`divergence_report`, `state.rs:6057`) — for **any** bulk op, reversible or not. The **offer** is the separate arm at `state.rs:4764`: `self.ui.branch_divergence = if record.reversible() { diverged } else { None }`, gated by `bulk_history.rs:102`, which is still exactly `op == BulkOp::CreateBranch`. The window (`ui/branch_divergence.rs`, rendered `ui/mod.rs:150` after the monitor it refers to) repairs through the **one existing** `AppState::bulk_rollback(offer.record_id)` (`state.rs:5867` → `:5767`) rather than a second undo mechanism, so the preflight matrix and the cascade run monitor stay the authority on what a rollback may reach; `Keep divergent`, the decline and the frame's own close button each record an answer and clear the offer, and a later run that lands everywhere replaces a stale one. `Skipped` rows arm neither half — the preflight refused them before anything ran, which is §4.6's *opposite* theory of failure and is deliberately not unified here | ~~**missing** — the rollback offer~~ **has 2026-10-09, ticket 06 (`1c4339b`, split by `20e3760` + `53c8b94`, merged as `4402cbe`)** — pinned by `branch_divergence_rollback.rs` (10 tests) and `branch_divergence_offer.rs` (6). **Why the split was the ticket's real content**: the record is a fact about roots that disagree and cannot repair themselves; the offer is a promise the app must be able to honour. Fusing them either promises a rollback that fails on the spot (`Custom` moves arbitrary git) or leaves a divergence unrecorded — `53c8b94`'s `a_custom_run_that_moved_one_root_and_failed_in_another_records_without_offering` pins the now-correct half. **Widening `reversible()` to cover `Custom` is a regression, not a fix.** **Still open, recorded as an accepted limitation, not landed**: a `BulkOp::Merge` cascade diverges exactly the same way and `reversible()` refuses it, so a merge-cascade divergence is **recorded with no repair path** — the feed names the split and nothing closes it. **No golden coverage**: `branch_divergence_offer.rs` asserts structure, prose and button fills through `egui_kittest`, and `tests/snapshots/` has **no** divergence PNG (same shape as §8.4 #15/#16's unphotographed surfaces) |
| `Configure synchronous branch control` | `SettingsCategory::MultiRoot` | **partial** |
| Menu **ordering** | — | **unresolved** — no source; see §7 |

### 5.3 Commit tool window → TurboGit

| Rebased | TurboGit | Verdict |
|---|---|---|
| Vertical tool window, changes tree + commit card + diff preview | `commit_window.rs` — 340px panel + flexible diff | **has** |
| Staging area (staged/unstaged buckets) | `UNSTAGED` / `STAGED` / `Merge conflicts` / `Unversioned` | **has** |
| Changelists + `Staging Area`/`Changelists` mode switch | — | **absent** — deliberate; see §8 |
| Per-file checkbox + status + colour | `paint_checkbox`, status letter, partial dot | **has** |
| Chunk-level and line-level commit inclusion | hunk gutter stage buttons + per-line toggles + char-range drag; whole-file via the Commit window's file checkbox | **has** — different mechanism, same reach |
| `Compare HEAD, Staged and Local Versions` (edit the staged version) | — | **divergent by decision** (2026-10-20, tickets 02 and 05; `docs/adr/0029-working-tree-preview-reads-head-and-a-click-applies-the-index-facing-patch.md`). The chip group was removed because three sites wrote `diff_comparison` and two of them were the app overriding the user's choice on every row click. The preview now pins one comparison, `HEAD ↔ worktree`, so a file's staged and unstaged lines are on screen together and the view is stable under the row toggle it offers, because staging a line cannot change `git diff HEAD`. Staged-ness is a **per-line derived fact** painted on the row, not a view the user switches into, so the capability the reference's three-way switch reached is answered without a switch. Declined, never a gap |
| Amend (`⌥M` toggle) + choose the commit | `ui.amend` checkbox, `AMEND_LABEL` | **partial** — the commit chooser not evidenced |
| `Commit ▾` split → Commit and Push / Commit and Rebase | `commit_split_menu` → "Commit and Push…" | **has** |
| Commit options behind `⌃O`, three groups | — | **missing** — the IDE-only checks are absent, but the *structure* is not present either |
| `Commit All when Nothing Is Staged` | `settings.commit_all_when_nothing_staged` + empty-selection note (`commit_window.rs:2203-2217`) | **has** — ticket 21b, implemented; Rebased's default is R3 |
| Identity prompt before first commit | `git config` read; no prompt evidenced | **partial** — Rebased probes identity at *project open*, not first commit (runtime 2026-10-06, `runtime/R3-SESSION-1.md`) |
| Post-commit notification with `Edit Commit Message` | `Dialog::Reword` exists; no post-commit window | **partial** — **open gap** |
| Undo Commit (own unpushed commits) | `Git.Uncommit` exists; `Undo Commit…` menu item not evidenced | **partial** — and it opens a **dialog**, not a bare verb: `Select Target Changelist` with Name (defaulted to the commit message), Comment, Set active, Track context (§7.1a) |
| Commit message template via `commit.template` | — | **removed** (2026-10-24) — the `Template` button, the `commit_template` setting and its Advanced-row went with the single-description commit card |
| Recent-message history in the field | `recent_messages_row` — up to 6 recent messages as chips (`commit_window.rs:1330-1347`) | **has** — a superset: the reference offers a history list, TurboGit offers a chip row |
| `Unversioned Files` node + `Add` offer | `Unversioned Files` bucket | **partial** — no modal offer |
| Force-add `.gitignore`d file, partially-cancelling dialog | stages directly | **absent** — no force-add affordance to gate |
| Shelf/Stash/Console sub-tabs + merge-into-one | `CommitSubTab::{LocalChanges,Shelf,Stash}` | **partial** — ~~panes are placeholders~~ **the Shelf and Stash panes landed 2026-10-08, ticket 12 (`1215d09`)** (see §5.5); merge option absent, and there is no Console sub-tab |
| Window/Float/modal/dissolve-into-Log | fixed left panel | **absent** — egui constraint |
| Ignore-files node, `Show Ignored Files` | `Git.Stage.ToggleIgnored` present in Rebased only | **missing** |

### 5.4 History rewrite → TurboGit

| Rebased | TurboGit | Verdict |
|---|---|---|
| `Rebasing Commits` plan editor, `Action\|Hash\|Subject` | `interactive_rebase.rs` with Plan/Preview/Log tabs | **has** |
| Pick / Reword / Squash / Fixup / Drop | 5 chips | **has** |
| **Edit** (`Stop to Edit`) | in the domain enum, parsed from `REBASE-TODO`, **no chip** | **partial** — **open gap** |
| **Update Ref** per-commit verb | `--update-refs` dialog flag only | **missing** |
| `Pick` on a modified row resets that row | — | **missing** |
| Reorder ↑↓ | `⇧↑`/`⇧↓` + pointer drag | **has** |
| Whole-plan `Reset` | — (preflight is modal) | **partial** |
| Graph preview of resulting history | `Preview` tab | **has** |
| `View Git Commands` raw-todo view | `Log` tab (editable `REBASE-TODO`) | **has** — better |
| `Start Rebasing` | `Start` via `execute_with_backup` | **has** |
| Safety backup ref | `refs/turbogit/preflight-backup` | **has** — TurboGit is arguably stronger |
| Drop confirmation naming the branch + detached-HEAD variant | `RewritePreflight` | **partial** — uniform, not per-variant |
| **Drop/Reword undo in the success notification** | `Operation::DropCommit`/`RewordCommit` | **partial** — **open gap** |
| Reword undo invalidated if the repo changed | — | **missing** |
| Gating: `Cannot {0} during cherry-pick/merge/rebase/revert` | `ui/rewrite_gating.rs::blocked` → `Block::Owned(OwnedOperation)`, the owner read from the **row's own root** by `integrate_service::owning_operation` | ~~**missing**~~ **has 2026-10-07**: ticket 11 (`09693e8`), on the `OwnedOperation` half of ticket 10 (`dcd4017`). Gating is a state machine over *which operation owns the repository*, not a boolean: the four states each name themselves, and the marker names were measured against git 2.50 in `repo_operation_state.rs` (8 tests) rather than assumed, read through a linked worktree's `gitdir:` pointer file with a load-bearing question order (a rebase outranks the holder files it may be stopped on; `MERGE_HEAD` last). At most **one** reason per row, ordered owner → commit shape → position → scope → branch policy, so two surfaces cannot pick differently. The reason reaches **both** the row's own hover text and the shell's status bar: `widgets::menu_item` raises it into a one-slot, one-frame channel (`widgets::take_status_hint`) that `shell::render` **takes** before the bottom panel is laid out — take, not peek, so moving off the row goes silent a frame later. Three bounds the reference does not enumerate here are kept (root commit, off the current branch, protected branch); dropping them would have been a regression, and protected-branch stays an up-front refusal. **Not landed**: the generic owner-less `Cannot {0}` fallback (nothing produces it — TurboGit always names the owner), and detached HEAD is *gated*, never confirmed — the reference's drop confirmation for a detached repository was not adopted, per the enablement-over-confirmation posture |
| Gating: cross-repository / external selection | `rewrite_gating::Scope::{Own, Several, External}` from `UiState::repo_selection`, computed per root | **partial 2026-10-07**: ticket 11 (`09693e8`) enumerated, computed and unit-proved both reasons (`a fleet scope of several repositories states the scope reason`, `a commit outside the fleet scope states the external reason`), discriminated by construction rather than by two booleans — but **no surface paints them today**: a non-empty `repo_selection` replaces the tool window with the multi-repo summary (`shell.rs::show_tool_window`), so there is no commit row to right-click while two repositories are in scope. A fleet rewrite entry is the surface that will paint them. TurboGit's menu acts on one commit, so the reference's "Selected N commits are from different repositories" maps to the workspace's checked-repository set — the same set the status bar's `N repos in scope` counter reports — not to a multi-commit log selection |
| `Interactive rebase is already in progress for this Git root` | `Dialog::Rebase`'s start gate, naming the owner per root | ~~**missing**~~ **has 2026-10-07**: ticket 10 (`dcd4017`) — the disabled start button states which operation holds *that* root ("A rebase is already in progress in alpha"), the same per-root discrimination ticket 11's commit menu inherits. A rebase running in root A does not refuse root B. Wording is TurboGit's per the copy rule; no bundle string copied |
| `--onto` as two ordered fields | `RebaseKind::Onto { new_base, old_base }` (`operation.rs`) + `Dialog::Rebase`'s `--onto` option over `NEW BASE` then `OLD BASE` | ~~**missing**~~ **has 2026-10-07**: ticket 10 (`dcd4017`) — **order is the requirement and it is asserted**, not just present: `rebase_onto_fields.rs` (8 tests) pins the first label's galley origin strictly above the second's. Each field validates in its own right through `AppState::revision_exists` (a cached `git rev-parse` per `(value, verdict)`, not one call per frame), its verdict paints beside *that* field rather than into a shared error region, and the confirm control is withheld while either is empty or unresolvable — enablement, not a confirmation. The dispatched kind passes both flags to `git rebase --onto` and still settles as one rebase; the commits-to-rebase preview keys on the **cutoff** (`old base`) so the list describes the rebase about to run; while the option is on, the plain target and `Interactive` mode are marked superseded and say so instead of quietly losing what was typed. **Still `UNVERIFIED`** (decided by inference, not capture): `--onto` shipped as a revealing option rather than two always-present fields, and each field accepts a branch name or a hash only — a date or another rev expression is git-legal but no captured material says the reference takes it |
| `Squash Commits` (combine selected) | — | **missing** |
| `Fixup…` / `Squash Into…` (fold staged work into an earlier commit) | — | **missing** |
| `Extract Selected Changes to Separate Commit` | — | **missing** |
| `Revert Commit` | revert via confirm in `log_commit_actions.rs` | **has** |
| `Reset Current Branch to Here…` with a mode choice | no reset-to-commit anywhere in `crates/turbogit-{ui,app}/src` | **missing** — four modes, not three: Soft / Mixed / Hard / **Keep** (`--keep` discards *committed* work and keeps *local* work; §7.1a) |

### 5.5 Shelf and stash → TurboGit

| Rebased | TurboGit | Verdict |
|---|---|---|
| Shelf tab + Stash tab, mergeable | `CommitSubTab::{Shelf,Stash}` — **two real panes**, separate as the reference ships them by default. Shelf: `AppState::shelves_for(root)` for the focused root, newest first, read off `ui.shelves` — the app's own store, loaded at `with_flat_state`, which is the **first read-back of a store that had only ever been written** (a store that will not parse sets `last_error` rather than painting an empty pane over shelved work; `shelve_stash::load_shelves`). Stash: `RootCaches::stashes(root)`, filled by `fetch_stashes → shelve_stash::list → git stash list` on the `pump_read` cache seam and invalidated with that root's other caches | **partial** — **panes landed 2026-10-08, ticket 12 (`1215d09`); the merge-into-one setting (`settings.enable.stashes.and.shelves`) is still absent.** ~~panes are labelled placeholders (ADR-0008)~~ **ADR-0008's own text and the strip comment at `commit_window.rs:223` still say "placeholder" and are now false** — the decision record is amended at close-out, not here |
| Shelving selects *some* changes | `Operation::Shelve { changes, message }` | **has** — different mechanism (stash-all-then-discard) |
| Stashing takes *all* uncommitted | `Operation::Shelve` with an empty set | **has** |
| Shelve dialog: file list + "N of M" summary + name + diff | `Dialog::Shelve` — **name only** | **partial** |
| `Shelve Silently` (name from the changelist) | — | **missing** |
| `Save to Shelf` (copy, no reset) | — | **missing** |
| Unshelve dialog: Name (existing **or** new), Comment, Set active, Track context, Remove-applied | ~~—~~ `Dialog::Unshelve` — **three of the reference's five field roles, one of its two `Name` roles.** One opener `AppState::open_unshelve_dialog` (`state.rs:3159`), reached from the Shelf pane's header verb and from `Action::Unshelve`, which is **palette-only** because the VCS popup's action set is a frozen pin (ADR-0011). **`Name` role 1 is adapted, not copied**: the reference's field names a *destination changelist*, and this app has none, so the field's question — *which shelf of this root gets applied* — is answered by a `selectable_label` list over `shelves_for(root)`, newest preselected, with the consequence stated in one sentence ("…nothing here sorts it into a named list, because this application has none"). **`Remove successfully applied files from the shelf` landed** as a checkbox **off on every open**, gated by exactly **one** affirmative that renames and recolours itself with the tick — `Unshelve` (`KitButton::Primary`) becomes `Unshelve and Remove` (`KitButton::Danger`) — and withheld rather than greyed when there is nothing to apply, because the accent fill survives a disabled state. Both defaults are re-armed at open, and **the root is stamped at open and never re-read**, because a same-named shelf exists in exactly one repository and a picker that followed the focus could apply it to the wrong tree. Removal still settles only on the `OpKind::Unshelve` **success** arm through `remove_shelf`'s **write-then-commit** (`save_shelves` first, `ui.shelves` only after it answers) | **has** — **landed 2026-10-10, ticket 12's second half** (`c6b00c9` feat, `9103ba9` test, merged as `d1a39ba`), pinned by `unshelve_dialog`'s 9 tests driven through the production opener. ~~missing — deliberately not built with ticket 12~~ **The four remaining reference fields are divergences by decision, not gaps**: `Name` role 2 (a new changelist to create), `Comment`, `Set active` and `Track context` all belong to the **changelist model this programme keeps out of scope** — and they are genuinely absent rather than present-but-inert: the dialog has **no `text_edit` of any kind** and `DialogState` gained only **three** fields (`unshelve_shelf`, `unshelve_root`, `unshelve_remove`). **Two recorded costs, neither fixed here**: (a) **no golden capture** — see §8.4 gap 12; (b) **a test-fidelity hole** — the absence checks use case-sensitive `painted_exact` while `group_title` → `micro_header` **uppercases**, so a future `COMMENT` or `SET ACTIVE` micro-header would evade them, and `Name` role 2 and `Comment` are asserted **nowhere at all** (the loop covers only `Set active`/`Track context`). Fix belongs to the test-only pass |
| `Unshelve` / `Unshelve and Remove` | ~~—~~ Two named controls on every shelf row, **opposite defaults carried in the label and the fill** rather than a tooltip: `Unshelve` (`KitButton::Secondary`, keeps the shelved copy) and `Unshelve and Remove` (`KitButton::Danger`, takes it). One dispatch for the pair — `Operation::Unshelve { root, name, remove }` → `OpKind::Unshelve`, so the activity feed records which half ran. The parked entry is re-found **by name** (`git stash push -m <shelf name>` stores it as `On <branch>: <name>`, and the CLI and libgit2 adapters hand the message back at different depths, so `shelf_name_of` recognises both spellings and `find_shelf_stash` takes the newest match). The record's removal rides `shelf_removal_pending`, armed **before** dispatch and spent only in the **success** arm — a pop that failed leaves the row painted and the pending is cleared on the failure arm | **has** — landed 2026-10-08, ticket 12 (`1215d09`), on the pane's rows. Reached by row verb, not by the reference's menu path. **The dialog that names the fields (row above) landed 2026-10-10** and carries the same pair as its single affirmative, so the row verbs and the dialog now dispatch through one `Operation::Unshelve { root, name, remove }` |
| `Already Unshelved` filter + `Restore` | — | **missing** |
| `Recently Deleted` node | — | **missing** |
| Stash dialog: root selector, branch readout, Message, Keep index | `Dialog::Stash` — message + `--keep-index`; buttons Stash/Pop latest/Cancel | **partial** — no root selector |
| Stash `Apply` / `Pop` / `Drop` / `Clear` | `Git.Stash.*` in Rebased only → **three of the four**, one per stash row: `Apply Stash (keeps)` (`KitButton::Secondary`) / `Pop Stash (removes)` (`KitButton::Danger`) / `Delete Stash` (`KitButton::Quiet`), each dispatched with `Affected::Root(stash.root)` over `v.stash_apply` / `v.stash_pop` / `v.stash_drop` at the entry's own `stash@{n}` index. The row's identity cell prints that ref, because it is what the verbs hand the engine | **partial** — **three landed 2026-10-08, ticket 12 (`1215d09`)**; ~~missing~~. **`Clear` (drop the whole stack) is not among them.** Keep/remove is again in the label, not a tooltip. **Recorded divergence**: `Delete Stash` asks first (`PendingConfirm::DeleteStash`) where the reference's docs state `Drop`/`Clear` carry no confirmation — see §7.5's shelve row |
| `Unstash…` as a new branch, `Reinstate Index` | `Git.Stash.UnstashAs` in Rebased only | **missing** |
| `Import Patches…` → a shelf | `Dialog::ImportPatches` (`dialogs.rs:1335`), opened from the Shelf pane header | **has — landed 2026-10-10, ticket 18c (`77a6c92`..`8b83775`, merged as `f2a770d`)** — the Shelf view takes patch **files** as a source: the chosen file's bytes are parked as their own payload at `<project>/.turbogit/shelf/<slug>-<nanos>.patch` under a row whose `source` is `ShelfSource::Patch { file }`, and `Unshelve` applies **that row's own bytes into its own root** rather than re-finding a stash entry. One `Shelf` type and one `Operation::Unshelve` serve both homes — the branch is on the field, not on a new verb. See §5.9's `Import to shelf` row for the write discipline, the eleven tests and the `source`-unasserted preservation residue |
| Shelf **base revisions** for 3-way merge | — | **missing** — a real risk after a rebase |
| Relocatable shelf location | — | **missing** |
| Conflicted-unstash notification (2 actions) | — | **partial** — and TurboGit should *not* copy the "drop the stash" instruction |
| Empty states: 3 for stash, 4 for a merged tab | ~~—~~ One absence word and one pending word per pane, each naming its own mechanism — Shelf: `No shelf store for this project yet.` (the store always answers, so this is an absence, never a promise of an arrival) + `Nothing is shelved in this repository. Use “Shelve…”…`; Stash: `Loading stashes…` (a cold cache, before git has answered) + `Nothing is stashed in this repository. Use “Stash…”…` (`the_two_empty_states_name_their_own_mechanism`) | **partial** — landed 2026-10-08, ticket 12 (`1215d09`). **The reference's counts are not matched**: four words where the surface has three states for a stash and four for a merged tab, and the merged tab does not exist here (§5.5 row 1) |
| Drag between tabs, `⌃` to copy | — | **absent** — egui constraint, recorded |

### 5.6 Remotes, fetch and push → TurboGit

| Rebased | TurboGit | Verdict |
|---|---|---|
| Fetch / Pull / Update as three ranked verbs | `Operation::Fetch`; update path; pull via `Vcs.UpdateProject` | **has** |
| Widget incoming/outgoing counters | ahead/behind in the repo header | **has** |
| Push dialog: repo list, commit list, preview, target panel | `Dialog::Push` — scope, banner, outgoing tree, changed-files preview, subset warning, Remote/Branch, 4 options | **has** — richer |
| **Only the current repository selected by default** | PUSH SCOPE segmented control defaulting to ThisRepo | **has** |
| Inline `Define remote` | `Define remote` link + `Add Remote` in the `Dialog::Push` target half (`ui/push_dialog.rs`); `Dialog::ManageRemotes` for the full list | **has 2026-10-07**: ticket 14 (`6a6aa3b`) — a repository with no remote gets the link in place of the Remote field; it opens Name/URL (`origin` prefilled) with `Add Remote`/`Cancel`, keeps git's failure inline as `Could not add remote: …`, and on success the new name takes the Remote field so the push continues without a trip to `ManageRemotes` |
| `Edit all targets` link; per-commit `Push All up to Here…` | per-commit checkboxes in the outgoing tree | **has** |
| `Push tags: All \| Current Branch` | `--tags` / `--follow-tags` | **partial** — the two-way scope is one checkbox |
| Push as a **dropdown**: Push / Force push | `--force` option | **partial** |
| **Force absent on a protected branch** | protected-branch blocking in `Dialog::Push` | **has** |
| `Push Rejected` dialog: repos, remember, Rebase/Merge | remediation banner | **partial** |
| **Six distinct rejection notifications, 4 with next actions** | `sync_service::classify_push_failure` → six `PushOutcome` classes, painted by `ui/push_rejected.rs` | ~~**—** **missing** — highest-value gap here~~ **has 2026-10-07**: ticket 13 (`798db84`) — one title and body per class, with a next step on exactly the four classes that can honour one (`Update and retry` / `Fetch and retry` / `Resolve conflicts`); `UpdateUnresolved` and `UpdateErrored` stay message-only |
| `Force-with-lease` rejected as its own message | `PushOutcome::LeaseRejected` | ~~**—** **missing**~~ **has 2026-10-07**: ticket 13 (`798db84`) — classified on git's `(stale info)` detail and worded as a stale lease, not a failed push |
| `Auto-update if push … was rejected` writing back to settings | `VcsSettings::auto_update_on_push_rejected` | ~~**—** **missing**~~ **has 2026-10-07**: ticket 13 (`798db84`) — set by the Push dialog's remember checkbox, reversible in Settings → Update, off by default |
| **Update opens the `Update Info` tab automatically** | `log_window` after a fetch | **partial** — not established as automatic |
| Update strategy: Merge / Rebase / Reset to Remote | rebase mode mapping | **partial** — Reset-to-remote not evidenced |
| Per-root **skip reasons** (`no tracked branch`, `detached HEAD`) | `Operation::Fetch { roots }` per-root report | **partial** — the reasons are missing |
| `Git Remotes` dialog, two URL columns | `Dialog::ManageRemotes` — list, add, edit URLs, rename, remove, set upstream, multi-root apply | **has** — richer |
| Remove-remote confirmation | `PendingConfirm` | **has** |
| `Vcs.Push.Simple` (push with no dialog) | `ui/push_control.rs::push_without_dialog`, the one-click verb in the VCS popup's push row | **has 2026-10-07**: ticket 14 (`6a6aa3b`) — `sync_service::direct_push_target` resolves it behind four gates (multi-root, a checked root subset, a protected branch, no remote at all); outside those gates the verb is **absent** rather than disabled, the palette route opens the dialog instead of a silent no-op, and the request is the panel's own `PushRequest` shape, so a rejection classifies like one from the dialog |
| Automatic fetch every 20 min | `incoming_poll.rs` | **partial** — interval unverified |
| `Pruned obsolete references` reported | — | **missing** |
| `Unshallow repository` | in the bundle only | **missing** |
| Credential storage (Keychain/KeePass) | — | **absent** — security surface, out of scope |

### 5.7 Log tool window → TurboGit

| Rebased | TurboGit | Verdict |
|---|---|---|
| Four panes, right side a **vertical split** | branches / graph / changed-files / commit-details | **has** |
| Per-query tabs matching filters | `log_ref_scope` + `RootCaches::ensure_ref_log` | **has** — different mechanism, same intent |
| Duplicate-tab guard | — | **missing** |
| Branches pane **hideable** | fixed | **missing** |
| Five-signal commit encoding (ref colour, background, bold, asterisk, root stripe) | **2 of 5**: per-root stripe only | **partial** — asterisk, bold-for-mine, background-for-mine all absent; ref colour is tooltip-only (`log_window.rs:1763-1770`). The **asterisk is the highest-value single item** — it is the only signal that answers "is this my change?" at a glance |
| Refs capped at 6 with `... {N} more` | — | **missing** |
| Sort By commit date (default) / Topologically | sorts by time desc (`log_display.rs:175`) | **partial** — date sort yes, topological no |
| `Collapse`/`Expand Linear Branches`; `Long Edges` off by default | — | **missing** |
| `Show First Parent` / `No Merges` | — | **missing** |
| `Not Cherry-Picked Commits` greying | `Git.Log.HighlightCherryPickedCommits` in Rebased only | **missing** |
| Search: text, regex (`.*`), match case (`Cc`), `⌃L` to focus | **plain substring, always case-insensitive** (`log_display.rs:176-182`); no regex engine in the workspace | **partial** — substring only. **But TurboGit's pickaxe (`git log -S`, `root_caches.rs:72`) exceeds the reference**: it matches commits whose *content* changed, which no reference feature does (§6) |
| Filters: Branch / User / Date / Paths | scope model | **partial** |
| `Go to Hash/Branch/Tag` (`⌃F`) | — | **partial** |
| **Ten named states, recovery action on each** | ~~`Reading`/`Empty`/`Populated` in `branches.rs`; not this granular~~ `log_states.rs::LogState` — **thirteen** named states, plain app-layer data with no egui reference: four loading (`Loading Git Log…`, `Loading commits…`, `Loading selected details`, `Applying Filters…`), five failures each naming its own read (`Error loading commits` / `…commit details` / `…changes`, `Error filtering commits`, `Error calculating file history`), three empties carrying **two different** actions, and `Changes log` named as the default so the census covers it. A failure is a fact about **one read of one key**, recorded in `RootCaches` and forgotten by the same read answering (`forget_log_read`) or by invalidation — never a surface-wide flag, which is how one pane's crash blackouts another's. `apply_log_recovery` **takes the `Refresh` action as a closure from the painting pane** — the listing, the details and the changed files are three different reads of three different keys, so each supplies its own (`FileAction::Refresh` → `state.refresh_commit_changes()`, `DetailAction::Refresh` → `state.refresh_commit_details()`) and nothing about the machine is decided per call site | ~~**partial** — **highest-value gap here**~~ **has 2026-10-07**: ticket 15 (`3ec61e2`) — the machine and its per-state recovery exist, proven by 22 tests in `tests/log_states.rs` on top of the `named_states()` census that pins "no two failures share a label". **Still open — coverage, not behaviour**: the proof is at the *derivation* layer. The only `painted_text` assertion of a machine string anywhere in the workspace is `log_paging_rows.rs::an_empty_window_says_so_outside_the_paged_rows`, asserting `No commits matching filters` — the string rename, not the exclusions. Nothing paints `No changes committed.` + `Commit local changes` on a fresh repo, nothing asserts the two empties never appear together, and details-vs-commits error distinctness has no paint assertion |
| **Found-but-filtered ⇒ `View and reset filters` / `View in new tab`** | `state.rs:1205 ui.log_hidden_jump` set at `log_window.rs:1343`, painted by `log_window.rs::hidden_jump_band`: the sentence over **two** `kit_button`s — `View and reset filters` (Primary) and, because this Log is one tab with one filter set, **`View in Diff`** (Secondary) in the second escape's place. Neither is automatic: the widening escape drops the filters, re-asks the jump and scrolls to the row; the look-elsewhere escape leaves the filter set, the chips and the band exactly as they were | **partial, and NOT claimed as landed** — the mechanism exists but is **untested at every layer**: no test anywhere references `log_hidden_jump`, `View and reset filters` or `View in Diff`, so neither of the ticket's two `filled_rects` escape assertions nor the load-bearing half (chips still painted after either escape) has any coverage. A correctness-of-interaction item shipped on faith; verification item 3 unmet |
| `Commit local changes` on an empty history | ~~—~~ `LogRecovery::CommitLocalChanges` labels `Commit local changes` and dispatches `state.ui.tab = Tab::Commit` — an empty history is a trip to the Commit surface, not a dead sentence. Reachable because `cli.rs:353` and `git2_exec.rs:290` now read an unborn HEAD as **no history** rather than as a **failed read**, which is the distinction that puts `No changes committed.` on a fresh repo instead of `Error loading commits` | ~~**missing** — one line of copy~~ **has 2026-10-07**: ticket 15 (`3ec61e2`) — proven at the derivation layer by `the_two_empty_states_are_distinct_states_with_distinct_actions` and `an_unborn_repository_reads_as_nothing_committed_rather_than_as_an_error`; **no paint-layer assertion** (see the row above) |
| `Show Changes to Parents` (3-panel merge review) | `log_display` / changed-files pane | **partial** |
| `Compare Versions` (two commits) | `Dialog::CompareBranches` | **has** |
| `History` tab per file + `History for Selection` | `blame_view.rs`; file history not evidenced | **partial** |
| Copy revision / subject / branch name | ~~—~~ **Two of the three already existed before this ticket**, on the log's own commit context menu, from `f4a46fa`: `Copy hash` (`commit_menu.rs:271` → `ctx.copy_text(cid)` at `log_window.rs:1538`, toast naming the short ref) and `Copy commit message` (`commit_menu.rs:278` → `log_window.rs:1549`), both driven by the 54 tests in `commit_context_menu.rs`. ~~**`Copy current branch name` has no analogue anywhere in the workspace**, and ticket 15 added no copy verb at all~~ **The third verb landed** as `Copy branch name`, the first item of the **branch** context menu (`branch_menu.rs`, copied by `branches.rs::apply_branch_action` through `ctx.copy_text`), and it diverges on purpose: it copies the row the menu was opened on, not the checked-out branch, because this menu is invoked on a row | **landed** — three of three, the third on the branch menu rather than the log and under a narrower name. **Not claimed by ticket 15**, whose scope is the log's states; it was deferred to its own dispatch as a *branch-menu* question, and that is where it shipped. The `—` in this row's TurboGit column was the §9.1 failure mode exactly as described: unverified, and wrong in the optimistic direction |
| Issue-tracker links in messages | — | **absent** — separate feature, no spec |
| Ten presentation toggles | — | **absent** — deliberate: refinement, not workflow |

### 5.8 Integrate → TurboGit

| Rebased | TurboGit | Verdict |
|---|---|---|
| Merge dialog + row-menu fast path | `Dialog::Merge` + `branch_menu` `Merge` | **has** |
| Rebase dialog + 4 fast paths | `Dialog::Rebase` + `RebaseKind::Branch` | **has** |
| 7 merge flags, 5 pull flags (different sets), 8 rebase flags | `MergeOpts` / `RebaseOpts` | **partial** — sets differ; labels are git-flag names |
| Flag labels state **effects** | `OPTIONS` shows `--no-ff` etc. | **partial** — **the wording is the substance** |
| `--onto` (2 fields) / `--interactive` / `--update-refs` / `--rebase-merges` / `--keep-empty` / `--root` / `--autosquash` / switch-branch | `RebaseKind::{Standard,Autosquash,Interactive,Branch}`; `--update-refs`, keep-empty, autosquash | **partial** — `--onto`, `--root`, `--rebase-merges` missing |
| Cherry-pick: checkout target, then pick | `Dialog::CherryPickTarget` then apply | **has** |
| `Add Commits to Remote Branch…` with a searchable picker | `Dialog::CherryPickAcross` (multi-**repo**) | **partial** — remote-as-target missing. Doc drift (§7.1b): the live page no longer documents this flow; the journey is bundle-only |
| `Cherry-Pick Selected Changes` (verb 4) | — | **missing** — still open after ticket 17: no `CherryPickSelectedChanges` operation, dialog or menu item exists anywhere in the workspace (verified by search, not by inference). Verb 4 is per-file cherry-pick from the **Commit Details** pane (`apply-changes:252-267`), a different surface from verb 5's branch-row entry point |
| `Get from Branch` (verb 5) + 3-colour status grammar | `Dialog::GetFromBranch` + `PendingConfirm::GetFileFromBranch` + `Operation::GetFileFromBranch` (`OpKind::GetFileFromBranch`, with the exhaustive `report_for` grown by a new arm rather than a wildcard), over `integrate_service::branch_file_changes` — **one git read, three rungs off git's own section headers**. Window in `ui/get_from_branch.rs`; snapshot payload on `DialogState`'s `get_from_branch_*` group; a **durable** `ActivityEntry` (`state.rs:3788`) names the verb, the path and the root on settlement. `status_ink` paints each rung from a shared `Palette` token — green `Added`, blue `Modified`, grey `Deleted` — and `STATUS_DELETED` is a **new** constant registered in the closed `TOKEN_INVENTORY` by this ticket | ~~**missing**~~ **has** — landed 2026-10-08, ticket 17 (`0d0186a`). The take is **withheld, not disabled**, everywhere but the blue rung, and this is enforced in product rather than assumed: the rung gate at `ui/get_from_branch.rs:136` admits only `Modified`, whose control is the confirmation's **sole** producer (`:88`), so a green row cannot offer a take that would delete a file. `Swap Branches` is a **positional exchange** of the two one-sided rows, not a re-tint of the same rows — the inversion the reference's grammar forbids. **One deliberate departure**: the reference documents no confirm step at all (`apply-changes:285-287`); this adds one, because the verb destroys a whole file's contents and `PendingConfirm` is where this app puts that. Proof: `get_from_branch.rs` in both `turbogit-services` (182 lines) and `turbogit-ui` (578 lines, incl. the end-to-end take over a real repository). **No golden covers the window** — it has never been captured, and `05-branches-popup.png` is stale because of its 11th menu item (see §5.8's row above) |
| `Show Diff with Working Tree` | `branch_menu.rs:311` — the **11th** item of the branch row's context menu (`BranchMenuAction::DiffWithWorkingTree`), sharing the compare glyph with the commit-level item above it because the two are one question read at two levels; it opens verb 5's window through `state.open_get_from_branch`, which reads the window's snapshot once at open and never from the pane | ~~**missing**~~ **has** — landed 2026-10-08, ticket 17 (`0d0186a`); the item census is pinned by `branch_context_menu.rs::the_menu_renders_the_eleven_items_in_the_designed_order`, and the gate (offered on every branch row **except** the branch you are on, and nothing at all on a detached HEAD) by `get_from_branch.rs::the_branch_row_offers_the_comparison_except_on_itself` |
| Smart merge (stash → merge → unstash) | `CleanTreeMethod` on `Operation::Merge` | **has** |
| Abort / Continue / **Skip** | ~~`PendingConfirm`; abort paths~~ All three as **one named dispatch unit**: `Operation::Resolve { root, owner, verb }` (`operation.rs:342`) over the closed `LifecycleVerb` set (`:73`), with `offered_for` exhaustive over `OwnedOperation` — a rebase paints three controls, a merge / cherry-pick / revert paint two. `Skip` needed a **port method** because git has no equivalent flag for a merge: `GitExecutor::rebase_skip` (`engine-api/src/lib.rs:248` → `cli.rs:1428`, `git2_exec.rs:1044`, `fake.rs:443`) | **has** — landed 2026-10-08, ticket 16 (`c8ef0a0`). All seven of the surface's lifecycle verbs map 1:1 onto it (`Merge.Abort/Continue`, `Rebase.Abort/Continue/Skip`, `CherryPick.Abort/Continue`); `report_for`'s exhaustive `OpKind` match grew the arm, so a verb that forgot its settlement is a compile error, not a silent `Other`. The conflict resolver's own header verbs (`conflict_resolver.rs:742`, `:757`) were folded into the same operation instead of a private `Operation::custom` closure, and now resolve which operation git actually holds (`held_operation:732`) rather than assuming `merge` — `git merge --abort` in a repository that is rebasing was a second failure on top of the one being recovered |
| In-flight state in the **status bar, per root** | 24px status bar; ~~`worktree_lifecycle.rs`~~ `ui/status_inflight.rs` — `items()` maps **every registered root** through `integrate_service::owning_operation` (an `exists()` read of git's own `rebase-merge` / `rebase-apply` / `CHERRY_PICK_HEAD` / `REVERT_HEAD` / `MERGE_HEAD` markers; no git process runs), and paints one announcement per holding root with that state's verbs attached. The same three verbs are reachable from the palette (`popups.rs:62`) as the menu route the reference documents | **has** — landed 2026-10-08, ticket 16 (`c8ef0a0`, the two new tokens registered in the closed inventory by `b89839b`). **Read from git, not from an app-side flag**, deliberately: a paused operation survives a restart and can be started outside this app, and a dispatch-time bool announces nothing in either case. A **separate channel** from ticket 11's one-frame `take_status_hint` slot — that keeps exactly one producer and one consumer. **Recorded divergence**: two roots mid-operation at once paint **side-by-side in registration order** inside the frozen 24px band rather than one row per root (`two_roots_mid_operation_announce_side_by_side_in_registration_order`); cost is up to five `exists()` syscalls per root per frame |
| Silent clean merge | activity feed + toast | **has** — arguably more visible |
| Failed cherry-pick **stays applied**, reviewable | conflict hand-off via `OpKind` | **has** |
| `On empty cherry-pick commit:` policy | — | **missing** |
| `cherry-picked from <hash>` suffix | — | **missing** — Rebased's default is **off** (`docs: settings-version-control-git`) |
| Two-dot range filter (`branch1..branch2`, `⌃↵`) | — | **missing** |
| `Pull into … Using Merge/Rebase` remote-only | `branch_menu` `Pull`, `CheckoutAndPull` | **missing** — and **inverted**: both gate on `has_upstream` with no `is_local` check (`branch_menu.rs:83-89`, `:112-118`), so Pull is offered on any tracking *local* branch. The reference's rule exists because pull requires a remote |

### 5.9 Patches → TurboGit

| Rebased | TurboGit | Verdict |
|---|---|---|
| Create from uncommitted / commit / file revision | `ChangesView.CreatePatch*` exist in the *Rebased* bundle only | **has — landed 2026-10-10, ticket 18b (`2bed4ce`, merged as `76270a9`)** — all **five** create routes reach the one settings window: the two local-changes palette verbs (the checked files when any are checked, the whole set when none — the checked set read at open, not at the write, so the patch and the Commit box describe the same files), the changed-file row and the file-history row (both `PatchOrigin::FileRevision`, one path in one revision), and the Log commit menu, which now routes through `open_create_patch_on` **instead of the write-only native-save path it used to call**. **The row correction ticket 18 owed and explicitly did not make is made here**: this cell's old verdict "**no patch surface found**" was by then plainly false and contradicted §7.6 #9, which had already recorded `CommitMenuAction::CreatePatch` — the honest reading was *one route exists, on the wrong surface, untested*, and that route now shares the open path with the other four. **The fidelity decision is bytes, not text**: the port is `GitExecutor::patch_bytes(root, origin, reverse) -> TgResult<Vec<u8>>` (`engine-api/src/lib.rs:422`), answered by `git diff --no-color` through `run_bytes` rather than `run` (`cli.rs:1966-2007`) precisely so a lossy decode cannot replace bytes the encoding field is asked to write; pinned at the engine layer by `patch_create_bytes.rs::a_patch_body_that_is_not_text_survives_the_read_byte_for_byte`, which writes a `0xff 0xfe` body and asserts `String::from_utf8(bytes).is_err()` on git's answer. `^!` scopes a commit to its first parent alone, and local changes read `HEAD` so staged and unstaged content travel together. `patch_create_paths.rs` (13) pins the base-path and segment rewriting over `[u8]`, and `create_patch.rs` (15) the routes. **Still open inside this landed row**: commit naming is proven on a **warm** log cache only (the suite seeds `cached(…)`), so the **cold-cache** path — `patch_origin_subject` against a root whose log has not loaded, which answers `""` — is untested; and `test_support`'s `patch_reads()` recorder is wired (`test-support/src/lib.rs:174,204,778`) but **consumed by no test** |
| `Copy as Patch to Clipboard` (no dialog) | — | **has — landed 2026-10-10, ticket 18b (`2bed4ce`, merged as `76270a9`)**, and asserted the way the verb is defined: **by the dialog it does not open**. `the_clipboard_route_opens_no_dialog_at_all` pins `state.ui.dialog == None` *and* asserts **both** titles absent from the painted frame (`SETTINGS_TITLE` and `COMMENT_PROMPT` — neither the settings window nor the comment step appears), then that the clipboard holds `diff --git` with the change's body, and the success toast one frame later. **The bug this exposed was real and is what `4e14474` fixed**: the clipboard slot was drained inside the create dialog's own paint frame, so the verb defined by opening no dialog filled a slot no frame would ever read. The drain is now `create_patch::drain_clipboard`, **unconditional** and called from `crate::ui::render` every frame — both routes fill the same slot, so neither can hand over a different artifact. **A refusal, not a lossy conversion, at the text boundary**: `a_patch_that_is_not_text_refuses_the_clipboard_rather_than_losing_bytes` pins that a non-UTF-8 payload says so instead of quietly stringifying, which is the bytes-over-port decision held at the one seam that cannot carry bytes. **Still open**: only the *verb* route is proven — the settings dialog's own **`To clipboard` arm** (`create_patch_to_clipboard`, `create_patch.rs:199`) has **no test that reaches it through the checkbox**; the field-order suite merely asserts the checkbox *paints* in position, never that checking it redirects the write, and the arm's effect is pinned only by setting the field directly |
| Patch File Settings: location, base path, reverse, encoding | — | **has — landed 2026-10-10, ticket 18b (`2bed4ce`, merged as `76270a9`)** — one window (`ui/create_patch.rs`, title `Patch File Settings`) reached from every create route, carrying **all four fields painted in the reference's order**, asserted by `the_four_settings_fields_are_painted_in_the_references_order` because the order is the discovery: destination (location, or `To clipboard`), then the two things that decide what the paths inside say (base path, `Reverse patch`), then the encoding that decides what the bytes are. `the_base_path_moves_the_paths_the_dialog_paints` pins the base path against the paths themselves; `the_reverse_field_changes_the_bytes_the_settings_hand_over` pins the reverse field against the payload; `the_encoding_field_is_read_back_off_the_written_file` pins encoding **off disk**, not off the widget. The comment step is **not a confirmation**: its text *derives* the file name (`the_comment_becomes_the_name_of_the_file_that_gets_written`), and `renaming_the_file_does_not_rename_the_comment` holds the two fields apart. **`Changelist destination`, `Set active` and `Track context` are NOT among the four fields and did not land** — the window has no changelist concept, consistent with the row below staying open. **Known cost**: the dialog is the **fourth** window in this surface with **zero golden coverage** (after apply-patch, `Get from Branch` and the divergence offer) — `acceptance_matrix.rs` has no case for it, so everything above is paint-text and on-disk assertion, never a rendered pixel |
| Apply from file / clipboard / on-the-fly / drag | `ChangesView.ApplyPatch*` in Rebased only | **partial — two of the four routes landed 2026-10-09, ticket 18 (`fa65cbc`, merged as `f2fb686`)**: `Apply patch…` and `Apply patch from Clipboard` are **palette-only** entries (`popups.rs`, held off the VCS popup because that action set is a frozen pin) opening one window (`ui/apply_patch.rs`) whose two internal routes are `From file` / `From clipboard`; the file route reads through the `AppState::patch_reader` seam (`Choose file…` or a typed path — a missing native picker is a toast saying to type it, not an error). The engine verb is the new `GitExecutor::apply_patch_to_worktree` (`git apply`, no `--cached`, index left exactly as it stands). **`on the fly` stays absent by §8.3 #8's deliberate decision; drag-and-drop is not implemented.** **Recorded divergence**: the clipboard route is a *paste into the window*, not a read of the system clipboard — `the_clipboard_route_skips_the_file_path_the_file_route_asks_for` pins that, and the module header says why (no CI runner owns a clipboard) |
| `Show Diff` as a preflight | — | **has — landed 2026-10-09, ticket 18 (`fa65cbc`)** — `Preflight instead of confirmation`, and genuinely non-mutating: `Show Diff` (and `⌃D`, read only while the window paints) calls `AppState::preflight_apply_patch` → `GitExecutor::check_patch` and paints git's verdict — `Applies cleanly — N file section(s) would be written`, or the refusal with `The patch does not apply cleanly. No file has been changed.` beside it. Pinned by `show_diff_returns_git_verdict_without_writing_and_without_dispatching` (no byte written, nothing dispatched) and `switching_the_target_root_drops_a_verdict_measured_against_another_tree` (a verdict is a fact about one tree). The blast radius — `FILES THE PATCH NAMES · N` — is stated from the *parsed* patch, so it answers before git does |
| `Change Directory Paths` counting repair pair | — | **has — landed 2026-10-10, ticket 18b (`2bed4ce`, merged as `76270a9`)** — the count is the point and it is asserted in its **strong form `[0]→1→2→3→2→1→0→[3]`**, never as a final value. `three_presses_paint_three_and_two_restores_paint_one` opens on `[0]` with git's own deep path, then walks three `Remove Leading Directory` presses; **each press asserts three things in the frame right after it** — that its own count painted, that the path that count produced is in `FILES THE PATCH NAMES`, and that the **next** count is *absent* ("one press is one segment — the count never runs ahead of the press") — then two `Restore Leading Directory` steps one at a time (3→2→1, each re-asserting the path it gave back), `Restore All` landing the bound at `[0]`, and six further presses that **cannot reach `[4]`**: `[3]` painted, `[4]` asserted not painted, and the shallowest spelling the file's own name `App.java` and never nothing. The paths are read off the **same** `create_patch_payload()` the write uses (`state.rs:3737`, consumed at `:3763`/`:3773`), so a counter that moved no path would fail. A counter that reset between frames or only appeared on close would pass a single end-state assert and still lose the mechanism — which is why every press is its own frame. `the_two_all_buttons_are_the_same_counters_ends` pins `Remove All`/`Restore All` as the two **ends of one counter** rather than a second mechanism, each landing on the bound the patch itself sets. `switching_the_create_root_drops_a_count_measured_against_another_tree` keeps a count a fact about one tree, the same rule §5.9's apply-preflight row holds for a verdict |
| Changelist destination | — | **missing** |
| `Import to shelf` deferral + `Import Patches…` | — | **has — landed 2026-10-10, ticket 18c (`77a6c92`..`8b83775`, merged as `f2a770d`)** — the reference's non-destructive exit exists, at **three** entry points: the apply window's `Import to shelf` parks **the patch as the window holds it** (`state.rs:3377`) without touching the working tree; the Shelf view's `Import Patches…` (`dialogs.rs:1335`) takes a patch **file**; and the `When patch is created: Ask` follow-up can park the file it just named. The parked row is **not a git stash entry, and that is the design**: a stash can only exist for work already in a tree, so stashing an unapplied patch would mean applying it — the act the deferral defers. The bytes live per-shelf at `<project>/.turbogit/shelf/<slug>-<nanos>.patch`, addressed by `Shelf.source: ShelfSource` (`Stash` \| `Patch { file }`), the file living beside the index rather than inside `shelf.ron` because the index is rewritten whole on every verb. `AppState::unshelve` branches on the field: `Stash` → the existing stash-by-name path **unchanged**; `Patch` → `patch_apply::apply_patch` on **that row's own root**. **No new `Operation`/`OpKind`** — the patch arm is a field of `Operation::Unshelve`, so settlement, keep/remove and `report_for` apply unchanged. `import_to_shelf.rs` (11) pins the discipline: park-without-worktree-touch, per-root listing only, a refused index write adds no row **and leaves no payload behind**, a non-patch file imports nothing, unshelve applies into its own root and keeps the row, `Unshelve and Remove` takes the payload with it, a failed apply keeps **both** row and bytes, delete reclaims the payload, and two imports never share one payload file. **Open residue named for the pending test-only pass — the top pin**: both `shelf_store_record_preservation` suites are behaviourally correct about `source` but **never assert it** — their wire probes read only `root` — so a `Patch`→`Stash` degradation on relaunch would pass every committed test |
| **Atomic total rollback + named `Rollback failed`** | — | **has — landed 2026-10-09, ticket 18 (`fa65cbc`; record lifetime fixed by `18113f1`)** — **the highest-value item in this surface, now held.** What makes it total is a *record*, not a git verb: `patch_apply::Recorded` captures the bytes of every path the patch names **before** `git apply` runs, with `None` content recorded as **absence** — so rolling back a create is a removal rather than a zero-length file the developer never had. Three outcomes in one value, so the two titled states are mutually exclusive **by construction** (`Applied` / `Apply patch aborted` quoting the reference's own guarantee `All files changed during 'Apply Patch' action were rolled back`, which only the aborted case is allowed to say / `Rollback failed`, naming each unrestored path, the reason for that one refused write, **and** the files that did come back so the tree's trustworthy half is stated). Restoring is a plain write of recorded bytes — `Rollback`, deliberately **not** a port method — because reverse-applying onto a half-written tree asks `git` to agree with a state it never saw. `Restore recorded content` is the state's one action (`retry_apply_patch_restore`), and the record it needs now outlives the frame that showed it: `AppState::rollback_recovery: HashMap<RootId, ApplyOutcome>` holds **only** the unresolved `RollbackFailed` variant, re-seeded after every transition that drops what the window paints, so editing the patch text, switching route, switching root and reopening can no longer strand an unrestored tree with no recovery (the three `*_leaves_the_failed_rollback_resolvable` tests, plus `a_second_refused_retry_keeps_the_state_named_and_records_it_again`). Both titled states reach the durable feed as their own entry (`record_apply_outcome`, `Warning` for aborted / `Error` for rollback-failed) because git's error does not know what the rollback then did. **Known accepted costs**: an older held failure can repaint after a newer clean `Aborted`, and a *successful* retry writes nothing to the feed, so the feed's last word stays `Rollback failed` |
| 4-pane patch-conflict view | 3-pane resolver | **partial** |
| Git-format patch auto-fills author + message | — | **has — in the window only, landed 2026-10-09, ticket 18 (`fa65cbc`)**: `patch.rs::read_provenance` parses git's commit header, and the apply window's `PATCH PROVENANCE` pair fills with what it states (`a_git_format_patch_fills_the_authors_and_message_it_already_states`), keeps a hand-edit instead of overwriting it (`state.rs:3585-3589`), and — the part silence would get wrong — says `This patch carries no commit header, so there is nothing to fill.` for a plain `git diff` (`a_patch_with_no_header_says_it_has_none`). The reader exists because discarding provenance is what turns an applied patch into an anonymous local change (`patch.rs:400`). **This is not the reference's auto-fill**: TurboGit has no changelist, so the two fields have no consumer past the window — the `Changelist destination` row above stays **missing**, and what landed is the *preservation* of a patch's own authorship (pinned at the engine layer by `patch_provenance.rs`) rather than a destination for it |
| `When patch is created:` three-way policy | `PendingConfirm` (no third state) | **has — landed 2026-10-10, ticket 18c (`77a6c92`..`8b83775`, merged as `f2a770d`)** — `VcsSettings.when_patch_is_created: PatchCreatedPolicy` with all three arms (`Ask` the captured default under `#[default]` + `serde(default)`, `ShowInFiles`, `DoNothing`), **enforced in `report_created_patch` rather than scattered at call sites**, and every arm still reaches the durable activity feed (`Patch created · <name>` — pinned by `every_arm_records_the_created_patch_in_the_durable_feed`, which walks all three). `Ask` arms the follow-up panel (park to shelf / take to the reading surface / dismiss — and the dismiss keeps the feed entry the write already made, "which is what makes silence auditable", `state.rs:4237`); the clipboard create route creates no file and so asks nothing (`the_clipboard_route_creates_no_file_and_so_asks_nothing`). Persistence is pinned three ways: across a restart, a pre-policy `state.ron` **still loads and lands on `Ask`** rather than erroring, and a chosen non-default arm round-trips through the current file shape (`patch_created_policy.rs`, 8 tests). **One recorded divergence, not dropped into silence**: the reference's `Show in {0}` reveals the file in the OS file manager; this build has no reveal seam, so the arm hands the patch to the apply window instead — named in `show_created_patch`'s own header. **Still open inside this landed row**: the **rootless fallback** — a patch created outside every root gets the `Wrote <name>` toast instead of an ask that could not honour its own first verb — is **untested**; and the reference's OS-reveal behaviour for `Show in {0}` is the divergence above, never a match |
| Reverse patch checkbox | — | **has — landed 2026-10-10, ticket 18b (`2bed4ce`, merged as `76270a9`), on the CREATE side** — `Reverse patch` is **git's own `-R` and nothing else**: `GitExecutor::patch_bytes` takes `reverse: bool` and pushes `-R` onto git's argv (`cli.rs:1971`); there is no app-side inversion of hunks. Proven by **behaviour, not by label**: `the_reverse_origin_is_a_patch_git_applies_backwards` hands the answer back to `apply_patch_to_worktree` and then reads the file off disk, asserting it holds **HEAD's content** (`one\nTWO\nthree\n`) — a checkbox that merely wrote different bytes would still pass a text assert, so the round trip is the proof. The settings field is pinned separately by `the_reverse_field_changes_the_bytes_the_settings_hand_over`, and the raw-patch memo is keyed on `(origin, reverse)` so a stale answer cannot survive a toggle. **The apply side is unchanged and stays forward-only**: ticket 18's `apply_patch_to_worktree` still carries no direction, `ApplyDirection::Reverse` still lives only on the pre-existing *index* path (`apply_patch_to_index`, used by `partial.rs` for granular stage/unstage), and no field on the apply window sets either — the old note that "a patch meant to be reversed arrives as the reversed patch" is now **true by construction**, because the create side is what produces that patch |
| `filetype.patch` registered | `panes.rs` binary routing only | **partial** |

### 5.10 Onboarding → TurboGit

| Rebased | TurboGit | Verdict |
|---|---|---|
| `Clone…` with a **services column** + URL + shallow | `welcome.rs:422-478` — URL input + `Shallow clone (--depth 1)` | **partial** — the dialog exists; the two-column layout does not |
| `Checking Git version…` before work | — | **missing** — one call, high value |
| `Create Git Repository…` with an already-under-git guard | — | **missing** — the one Git-specific preflight |
| Welcome-screen `Clone Repository` | `welcome.rs:422-478`, plus `PendingConfirm::CloneRepo` (`state.rs:2171`) | **has** — two entry points |
| Recents with branch names (3-value mode) | **has** — branch chip computed live at render, never persisted (`recents.rs:7-9`, `welcome.rs:592-599`) | **has** — and TurboGit's live-compute is *better* than the reference's stored name, which goes stale the moment a branch is switched outside the app. The reference's `Projects with identical names` middle value has no equivalent, and is the one part worth adding |
| Four clone routes by setup state | `welcome.rs` + workspace picker | **partial** |
| `Enable Version Control Integration` absent when done | — | **missing** |
| Git-not-found ⇒ offer to download | — | **missing** |
| `Nothing to commit, working tree is clean` as a status | — | **missing** |
| `Add` newly-created-file offer, incl. files made outside the IDE | `Unversioned Files` bucket | **partial** |
| `Add to .gitignore` / `.git/info/exclude` (named separately) | — | **partial** — not evidenced |
| `Rollback` for added-but-uncommitted | `Git.Stage.Revert` in Rebased only | **partial** |
| Shallow clone + `Unshallow` later | `Shallow clone (--depth 1)` checkbox (`welcome.rs:433-438`) | **partial** — clone side has, `Unshallow` does not |
| Trust/build-tool/share-settings prompts | — | **absent** — IDE inheritance |
| WSL2 detection | — | **absent** — platform |

### 5.11 Diff viewer → TurboGit

| Rebased | TurboGit | Verdict |
|---|---|---|
| 2-way side-by-side ⇄ unified | `diff/view.rs` — same | **has** |
| Editor tab ⇄ separate window | `Diff.OpenDiffInEditor`, `ShowStandaloneDiff` | **has** — as actions, not two settings |
| `⌘D` from six surfaces | `⌘D`-equivalent in commit/log/diff | **has** |
| `F7`/`⇧F7` difference nav | `F7`/`⇧F7` hunk nav | **has** |
| **`F7` past the last hunk offers the next file** | `hunk_nav.rs:26-51` + `diff_data.rs:115-117` | **has** — corrected 2026-10-06 (§7.1b); the gate differs (settings checkbox vs double-press edge) |
| `⌃⇧⇥` pane focus, "the active pane has the caret" | ~~—~~ `Diff.FocusOppositePane`: `ui.diff_pane = ui.diff_pane.opposite()` + `request_repaint` (`shell.rs:314`), a repaint and not a re-read because nothing about the comparison changed; the pane that holds it paints a caret bar at the current line (`diff/view.rs:645`, `theme::CARET_WIDTH`, registered in `TOKEN_INVENTORY` by `22f5149`) | **has** — landed 2026-10-07, ticket 20 (`6429b89`). **Coverage is behavioural, not keystroke-level**: the route is read at `shell.rs:303` and no test drives it (zero `send_key` uses repo-wide; all 30 `key_pressed` sites are production readers in `crates/*/src`, none in a test), and the popup's own `open(true)`/clear arm is unexercised. **Still open**: the bundle's *second* action, `Diff.FocusOppositePaneAndScroll` (move the caret **and** re-sync the scroll), is deferred by design and appears nowhere in `crates/`; its default binding is unresolved (§7.3) |
| `F4` to source, caret position preserved | ~~—~~ `Jump to Source` (`shell.rs:427-440`): the line is **derived** — first changed row of the current hunk on the pane that holds the caret (`diff::preview_caret_line`) — not a second canonical selection; blame opens for that file at `HEAD` and the line travels in `state.ui.blame_caret_line`, which `blame_view.rs:192-213` scrolls to once and clears, so no stale line survives an unrelated route in | **has** — landed 2026-10-07 (`6429b89`, with the read side wired by `cb911b0`: it was written but never read, so the jump landed at the top of the file). Two divergences recorded honestly: TurboGit has no editor, so its source surface is the blame *line listing*, and blame counts `HEAD`'s lines, so a working-tree change above the caret makes the numbers diverge. A pane with no numbered row at the caret (an insertion read as `Before`) has no line to jump to and the key does nothing. Gate: working-tree preview only, shared with `⌃⇧D`/`⌃⇧⇥`. Same keystroke-coverage caveat as the row above |
| Accept / Append via **one chevron + `Ctrl` modifier** | gutter stage/unstage buttons | **partial** — **copy the modifier pattern** |
| `Ignore Left/Right Side` | — | **missing** |
| `Apply All Non-Conflicting Changes` (+ left/right) | — | **missing** |
| `Resolve Simple Conflicts` | per-conflict resolve | **partial** |
| `Compare Contents` → Base \| Middle | 3-pane resolver | **has** |
| `CompareWithBase.{L,R,Result}` + `ComparePartial.{3}` | — | **partial** — TurboGit has these as internal-only. The reference reaches them: `Compare Contents` covers the base triple, and `Switch to Three-Side Viewer` ("load a third file and add it to the comparison") is the user-facing half of `ComparePartial` (§2.1a) |
| `⌃Z`/`⌃⇧Z` undoes a merge, **conflicts stay in sync** | per-conflict Undo in the resolver (`conflict_resolver.rs:165-206`) | **partial** — no text-level undo (the diff is read-only), but the resolver's Undo always returns the conflict to unresolved, matching the invariant's intent |
| 5-value whitespace policy | ~~one boolean `Ignore whitespace` (`diff/view.rs:874`)~~ **that boolean is retired** — it asked one of the ten questions this menu answers, and asked it of `git` rather than of the view. `DiffWhitespace` (`turbogit-domain/src/model.rs:919`) carries all five in the reference's spelling and order, `None` default, decided by `services/diff_policy.rs::considered_equal` at the display-model fold (`app/diff_model.rs::flush_changed_run`) | **has** — landed 2026-10-07, ticket 20 (`6429b89`). **The fold placement is load-bearing, not incidental**: two of the five values (line split/join, import-block changes) have no `git` flag, and the CLI producer and the in-process `similar` producer do not agree on git's whitespace flags value for value — so a flag-per-value design would answer a different question depending on which producer ran. One implementation, applied after the patch value settles, is the only shape where all five mean the same thing on every path; it therefore cannot live in `diff_engine.rs`. A run the policy calls equal stops being a difference and becomes one unchanged row (`RowKind::Context`, `line_ord: 0`). Pinned by `diff_policies::the_whitespace_ladder_hides_one_more_kind_of_change_at_each_rung` + `diff_policy`'s 5 unit tests (the ladder-distinguishability proof) and `an_equalized_difference_paints_one_unchanged_line`. **Known cost**: the `+N −M` counts and `(no differences)` now follow the policy, since they read the model — with whitespace ignored the counts drop and the file does not empty; arguably correct, and unverified against the reference. `engine/cli.rs`'s `-w` mapping and `diff_engine.rs`'s `WhitespaceMode` are now unreachable from production (no caller sets the field); removing them is boundary trimming, not a policy job, so they stayed |
| 5-value highlighting incl. `Split changes` | ~~—~~ `DiffHighlighting` (`model.rs:967`), all five, via `diff_policy::{highlight_runs, split_changes}` → `diff_model::{pair_run, pair_highlight}` into `Row::highlight`, painted by `ui/diff/view.rs::paint_highlight` in three shapes (`Row` band / `Runs` / `Off`). `Split changes` partitions a run and carries a line appearing on both sides through as Context belonging to no change, so `A␤B` vs `A X␤B X` reads as two changes; `None` bands nothing | **has** — landed 2026-10-07, ticket 20 (`6429b89`). `Lines` keeps the behaviour the surface had before the menu existed, because `Default` is unresolved in the evidence (§7.3) and inventing one is worse than keeping the incumbent — recorded in `domain::model::DiffHighlighting`'s doc. Pinned by `diff_policies::each_highlighting_value_cuts_the_change_at_its_own_granularity`. The two policies' **order** inside the fold is itself load-bearing and compiles either way: whitespace decides whether the run is a difference *at all*, and only a surviving run is cut up by highlighting |
| `Align Changes in Side-by-Side` | `diff/model.rs` metrics | **missing** |
| `Synchronize Scrolling` (hold to release) | panes share one `ScrollArea` (`diff/view.rs:224`) | **partial** — synced by construction (§7.1b); only the hold-to-release escape is missing |
| `Disable editing` **default on** | editable | **partial** — deliberate divergence |
| Staging at 3 granularities | File/Hunk/Line + char-range | **has** |
| "Edit the staged version" (3-way) | — | **missing** |
| `Compare with Clipboard` | — | **missing** — low value |
| 3-colour grammar shared with branch comparison | `theme.rs` roles | **partial** — corrected 2026-10-06 (§7.1b): only added-green is a shared token (`STATUS_ADDED` = `DIFF_ADD_ACCENT`); copying the add/modify/delete grammar is a vocabulary change |
| Binary-file reduced option set | `panes.rs` binary routing | **has** — TurboGit is ahead |
| **Loading / error states** | `diff_load.rs` decode limits | **has** — TurboGit is ahead; the reference is silent |

---

## 6. Reverse gap list

**What TurboGit has that Rebased does not. Parity must not delete these.** Each is
a differentiator; the risk is that a parity pass "simplifies" a surface down to
the reference's shape.

| # | TurboGit capability | Where | Why it is a differentiator | Rebased's closest thing |
|---|---|---|---|---|
| 1 | **Multi-root / workspace** — recursive project tree, workspace picker, multi-root apply on every dialog, `Affected::All` | `project_tree.rs`, `sidebar.rs`, `multi_selection.rs`, `tree_selection.rs`, `popups.rs`, `pinned_views.rs` | Rebased only offers *synchronous* multi-root as a setting, and its failure mode is divergence. TurboGit treats roots as first-class everywhere. | `Execute branch operations on all roots`; `multi-rooted projects` in the docs |
| 2 | **Bulk / fleet operations** with a preflight matrix and a cascade run monitor | `bulk_preflight.rs`, `bulk_monitor.rs`, `bulk_ops.rs` | Rebased has **no** bulk surface. A cross-root dry-run matrix before acting is a category, not a feature. | none |
| 3 | **Smart grouping** — two systems: five built-in computed groups (`Diverged`, `Conflicted`, `Unpushed`, `Unpulled`, `Dirty`) *plus* user-defined rules | `smart_groups.rs`, `smart_rule_modal.rs`, `turbogit-app/src/smart_rules.rs` | Rebased groups only by directory, repository, and (in the log) by branch/user/date/path. Predicate-based grouping over *vcs state* is a different thing. | `Group by Directory` / `Group by Repository` |
| 4 | **Worktrees** as a first-class tab with lifecycle management, freshness policy and epoch-guarded admission | `worktrees.rs`, `worktree_lifecycle.rs`, ADR-0019 | Rebased has `New Worktree from {branch}` and a `Worktrees…` action — **as a menu item, not a surface**. TurboGit models the worktree set as something with a lifecycle. | `Git.CreateNewWorkingTree`, `Git.Show.WorkingTrees` |
| 5 | **Submodules** tab with init/update/deinit and cross-repo conflict propagation | `submodules.rs`, `conflict_propagation.rs` | Rebased mentions submodules only as a clone flag and an update behaviour. TurboGit gives them a surface and a propagation rule. | `clone --recurse-submodules`; "they will also be updated automatically" |
| 6 | **Activity log** — a session-durable operation feed, filterable by repo and time, with per-entry severity | `activity.rs`, `activity_panel.rs` | Rebased reports outcomes as transient notifications. A durable record of *what this session did* is a different affordance, and it is what makes multi-root auditable. | notifications only |
| 7 | **Activity-driven operation settlement** — `OpKind` identity, exhaustive match, unrepresentable misses | `operation.rs`, `state.rs` | Rebased discovers lifecycle bugs at runtime. TurboGit makes "a new operation forgot its settlement arm" a **compile error**. | none |
| 8 | **Rewrite safety backup ref** written before every rewrite | `cli.rs:59` `REWRITE_BACKUP_REF`, `history_editor.rs:89` | Rebased has `Abort Rebase` and post-hoc undo, but **no pre-rewrite backup ref**. TurboGit's is a stronger guarantee than anything the reference documents. | `Abort Rebase` |
| 9 | **Char-range staging** (`Enter`/`Esc` on a selection) | `partial.rs:29-39`, `shell.rs:291-301` | Finer than "split chunks" and finer than the reference's line checkbox. | `Split Chunks and Include Selected Lines into Commit` |
| 10 | **Keyed read / cached reads** — `read` answers and admits, `peek` answers only | `keyed_read.rs`, `diff_load.rs` | A concurrency discipline, not a feature: it is why the diff and log stay correct under multi-root. | none |
| 11 | **Tri-state checkbox selection summary** | `multi_selection.rs` | Rebased's checkboxes are per-file with no aggregate semantics. | none |
| 12 | **Pinned views** — saved named selections persisted to `.turbogit/ui.ron` | `pinned_views.rs` | Rebased's recents are per-project and per-branch; a saved *query* is a different object. | recents with branch names |
| 13 | **Command palette** (`⌃⇧A`, fuzzy) | `popups.rs` | Rebased reaches most verbs through a context menu. | context menus |
| 14 | **Blame view** replacing the graph in place | `blame_view.rs` | Rebased's annotate is a gutter overlay in an editor TurboGit does not have. | `Annotate with Git Blame` |
| 15 | **Protected-branch guard as an up-front refusal** | `Operation::Rebase`, `Operation::DropCommit` | Rebased discovers the problem at push time and explains it then. TurboGit refuses before acting, so its own "your rewrite will be rejected" warning is unnecessary. | `settings.protected.branched` |
| 16 | **Per-root in-flight admission control** (one fetch per root) | `worktree_lifecycle.rs` | Rebased skips roots with a stated reason but does not appear to *admit* work per root. | `update.skip.root.reason.*` |
| 17 | **Bulk preflight before destructive work** | `bulk_preflight.rs`, `rewrite_preflight.rs` | Rebased's preflight is either per-verb confirmation or *post-hoc* disclosure. TurboGit has a reusable matrix. | confirmations; post-hoc notifications |
| 18 | **Design-token discipline** — every shared token in `theme.rs`, pinned by tests | `theme.rs`, `tests/design_tokens.rs`, `widget_library.rs` | Rebased's own a11y and theming are acknowledged weak; TurboGit's R4 work is an asset, not a finishing cost. | — |
| 19 | **Pickaxe log search** — matches commits whose *content* changed | `root_caches.rs:72` → `cli.rs:302-304` (`git log -S`) | The reference's search is text/regex/match-case over message, hash and author. None of those reach commit *content*. Found while resolving §7.6 q5 — a parity pass that "adds regex" to match the reference would be a **regression** against this. | `.*` / `Cc` over the commit list |
| 20 | **A 10-second branch-delete undo, enforced in code** | `branches.rs:695-698`, `state.rs:679-684` | The reference's bound is "until the app restarts" — *looser*. TurboGit's is a deliberate, short, self-clearing window. Whether 10 s is the right number is a product question the evidence cannot settle, but having a bound at all is the stronger design. | restore link in a notification, lifetime = until restart |

---

## 7. Unresolved

Every `UNVERIFIED` (82 across the eleven files, after the 2026-10-06 re-test
closed 24; see §7.1b) and every documentation failure, with what would close it.
Grouped by cause, because most share one cause.

### 7.1 Documentation failures — **all four now closed**

Every slug `SOURCES.md` recorded as dead has been re-checked. **All four were
renames.** This is the single highest-yield hour in the whole project: three more
pages resolved, and one of them (`undo-changes`) turned out to carry a dialog and
a four-mode reset the spec had recorded as `UNVERIFIED` or missing.

| Old slug (404) | Current slug | Verified | Was it load-bearing? |
|---|---|---|---|
| `compare-file-with-diff` | `comparing-files-and-folders` | 200, 03 Sep 2026 | Yes — §2.1a, three `UNVERIFIED` answered |
| `undo-the-last-commit` | **`undo-changes`** | 200, 05 May 2026 | **Yes — see §7.1a. Four `UNVERIFIED` answered, one gap closed** |
| `group-changelists` | **`managing-changelists`** | 200, 17 Aug 2026 | Partly — the page is titled "Group changes into changelists"; §8.3 #1 stands, but two facts it settles are now cited |
| `switch-between-branches` | folded into `manage-branches` | already held | No — confirmed folded, not renamed |

`manage-branches` was already harvested, so that one is a confirmation rather than
a recovery.

**The method that worked, twice now:** a 404 plus an outbound link from a live
sibling page means *renamed until disproved*. Both recoveries were found by
searching the **topic**, not the slug. Neither was guessable from the 404 alone.
`harvest.py` should search the topic on a 404 and follow sibling footers before
recording a dead end — two of four "dead" slugs in this project were alive.

### 7.1a What `undo-changes` added — and one correction to §5.4

The spec had `Undo Commit` as a single log-menu verb with a one-line description
(`§5.4`) and `Reset Current Branch to Here…` as `missing` with the note "no
reset-to-commit anywhere in `crates/turbogit-{ui,app}/src`". Both were partly
wrong, and the page adds a dialog nobody had specified.

**The `Select Target Changelist` dialog** (`undo-changes`, `Undo the last commit`
step 3-5). `Undo Commit` is not a bare verb — it opens a dialog with:

| Field | Default | Evidence |
|---|---|---|
| **Name** | **the commit message** — "You can either select an existing changelist from the Name list, or specify the name of a new changelist (**the commit message is used by default**)" | undo-changes |
| **Comment** | empty — "When the new changelist is submitted to the repository, this comment will appear in the Comment text area of the Commit Changes dialog" | undo-changes |
| `[ ] Set active` | off — "When this checkbox is cleared, the current active changelist remains active" | undo-changes |
| `[ ] Track context` | off | undo-changes |

**Why the default matters more than the fields:** undo is a *destructive* verb, and
the destination is pre-filled with something the user can recognise — the commit
they are undoing. The dialog does not ask "where should this go?" so much as "is
this where you want it?". A client that opened an empty destination field would be
making the user do lookup work at the moment they are least equipped for it.

**And the same three fields, in the same order, reappear** in `Move to Another
Changelist` (`managing-changelists`, `⌥⇧M`) and in the Unshelve dialog
(`shelving-and-unshelving-changes.md:182-187`). **One destination dialog, three
call sites** — that is a reusable component, not three dialogs. Worth building once.

**Four reset modes, not three.** The spec's §5.4 row said the Reset dialog "asks
for the mode" with no enumeration. All four, verbatim:

| Mode | Effect (verbatim) |
|---|---|
| **Soft** | "all changes from commits that were made after the selected commit will be staged (that means they will be moved to the Commit window so that you can review them and commit later if necessary)" |
| **Mixed** | "changes made after the selected commit will be preserved but will not be staged for commit" |
| **Hard** | "all changes made after the selected commit will be discarded (both staged and committed)" |
| **Keep** | "committed changes made after the selected commit will be discarded, but **local changes will be kept intact**" |

`Keep` is the one worth noting: `--keep` is not a soft-reset variant, it discards
*committed* work while preserving *uncommitted* work. A client offering only
soft/mixed/hard would let a user destroy local work they meant to keep.

**Two further facts, both safety-relevant:**

- **Multi-commit revert creates N commits**: "If you apply this action to multiple
  commits selected in the Log view, **a separate commit will be created to revert
  each of them**." So a 5-commit selection produces 5 revert commits, not one.
  That is stated as a note, which means it has surprised people.
- **Dropping is documented as a `warning` block**, and Drop Selected Changes
  carries a `tip` that names the recovery: if already pushed, "you will have to
  either use force push … or update your project first to sync the local branch
  with the remote tracked branch and then push the merge commit." That tip is the
  only place the reference tells a user how to recover a drop of already-pushed
  work.

**One correction to my own §5.4 row.** It read `Reset Current Branch to Here…` →
**missing**, on the grounds that no reset-to-commit exists in TurboGit. That is
still true of TurboGit, and the row is right — but I had also implied the
reference's dialog was unenumerated when I had simply never fetched the page. The
gap is TurboGit's, and it is now fully specified rather than open-ended.

### 7.1b The 2026-10-06 re-test: every `UNVERIFIED` re-tested by topic (D5 complete)

The method of §0.1/§7.1 — search the *topic*, not the slug — was applied to all
106 flags. Result: **24 closed** (9 from documentation, 15 from TurboGit's own
source), **82 re-deferred to R3** with dated re-test notes in the evidence files
naming what was checked and why it does not answer the flag. Two more pages were
recovered the same way as §7.1 and are now Tier 1 sources in `SOURCES.md`:

| New page | Found via | What it settled |
|---|---|---|
| `settings-version-control-git` | cross-reference from `commit-and-push-changes` | cherry-pick suffix **default off**; fetch-every-20-min stated; staging-area switch hazard (below) |
| `advanced-settings` | recovered from a 404 on `settings-advanced-settings` — the **third** live instance of the 404-means-renamed lesson | Use Safe Force Push, recursive submodule clone, incoming/outgoing checks — described, **no defaults stated** |

**Closures that changed the spec** (rows corrected in place, flagged here):

1. **§5.3 / §7.6 q1 / §8.4 #20 — `Commit All when Nothing Is Staged` is now
   implemented.** `settings.commit_all_when_nothing_staged` exists as an opt-in
   setting (default off, `crates/turbogit-domain/src/model.rs:707`), is exposed in
   the settings modal (`settings_modal.rs:255`), and gates empty selections with a
   "Nothing is staged — Commit will include every tracked change" note
   (`commit_window.rs:2203-2217`). This is ticket 21b, implemented in the working
   tree 2026-10-05. §7.6 q1's "the UI makes it unreachable" verdict is therefore
   superseded; what remains open is only **Rebased's** default (R3).
2. **§5.11 — `F7` past the last hunk offering the next file was never a gap.**
   TurboGit already crosses files: edge-nudge then same-direction press inside
   `EDGE_WINDOW` (`crates/turbogit-ui/src/ui/hunk_nav.rs:26-31, 43-51`;
   `crates/turbogit-app/src/diff_data.rs:115-117`). The divergence is only the
   *gate*: IDEA offers it via a settings checkbox, TurboGit via a double-press on
   the edge. Ticket 20 shrinks accordingly.
3. **§5.11 — `Synchronize Scrolling` is also not missing.** Both side-by-side
   panes share one `ScrollArea::vertical()` (`diff/view.rs:224`), so they are
   synced by construction; what is missing is only the reference's hold-to-release
   escape toggle.
4. **§5.11 — the three-colour grammar row was overstated.** Only the added-green
   token is shared (`STATUS_ADDED` *is* `DIFF_ADD_ACCENT`, `theme.rs:349-354`);
   TurboGit's deleted colour is red and modified has no diff highlight. Copying
   IDEA's add/modify/delete grammar is a vocabulary change, not token plumbing.
5. **§4.2 — the `Clean working tree using` "Shelve first" inference is
   withdrawn.** `02-branches-popup` had inferred Shelve-as-default from item
   order in `manage-branches`; the settings page lists **Stash** first. Neither
   page states the default — the ordering heuristic is unstable across pages, so
   this is an R3 capture after all. The §4.2 sentence reads as an example of the
   setting, not a verified default.
6. **One contradiction recorded, not resolved:** `commit-and-push-changes.md:175`
   says switching to the staging area *saves all existing changelists*; the new
   `settings-version-control-git` page says enabling the staging area means **"all
   existing changelists will be deleted."** Both are current docs. R3 resolves
   which is true of build 1.1.19. *(R3 session 2 captured the actual settings
   page: its note reads only "This will disable changelists support" — it does
   not settle save-vs-delete either. Still open, now with three sources.)*

**Behaviour-level closures worth noting** (checkbox states still R3, but the
behaviour is unconditional in docs): the Push dialog always previews commits
before pushing (`commit-and-push-changes`), force push is
`--force-with-lease`-backed by default, and clone pulls in submodules
unconditionally (`set-up-a-git-repository`). `Enable staging area` defaults to
**off** — changelists are the default mode (`docs: undo-changes`).

**Doc drift observed:** the live `apply-changes-from-one-branch-to-another`
page (15 Apr 2026) **no longer documents the "Add Commits to Remote Branch…"
flow** that §4.8/§5.8 are built around — that journey is now bundle-only. The
rows stand, but their docs citation is historical.



### 7.2 The structural hole: menu ordering

**Update 2026-10-06 (R3 session 2): the main menu bar is captured.** The full
ordered tree — including the Git menu's eight groups and its
`Uncommitted Changes` submenu — is in
[`runtime/R3-SESSION-2.md`](../research/evidence/rebased/runtime/R3-SESSION-2.md)
with the raw dump at `runtime/screenshots/menubar.txt`. What remains open is
exactly the runtime-only tail:

`SOURCES.md` Tier 2 records the decisive negative: `plugins/git4idea/src`
contains only a `.keep`, the repository has been restructured into
frontend/backend/shared, and a recursive tree query over 48 315 entries has
**zero** hits for `BranchPopup`. The local bundle ships
`intellij.vcs.git.frontend.jar` — exactly the module whose sources are absent.

| Item | What would close it |
|---|---|
| ~~Row context-menu order (branches, commits, changed files)~~ — **partly closed 2026-10-06 (R3 session 3)**: the Log commit-row and changed-file-row menus are captured in order (`runtime/R3-SESSION-3.md`), with live enablement states — all rewrite verbs greyed on a pushed commit. Branch-row menu remains (the Log's Branches strip ignores scripted clicks) | branch-row menu: a manual click |
| Toolbar order in the log, Commit window, diff viewer | runtime capture |
| Whether `⇧F6` and `F2` are both bound, and which wins | `keymap-bindings.txt` lists both; the resolution order is platform behaviour |
| Dialog button order and alignment (footer left/centre/right) — **partly closed**: Create New Branch is `Cancel | Create`, affirmative primary right-aligned (session 3) | runtime capture per dialog |
| Order of the six commit options within their three groups — **moot**: the ⌃O popup in build 1.1.19 carries a `Git` group (Author, Sign-off commit) and a `Commit Checks` group; the docs' six IDE checks are absent from the product (session 3) | — |
| Gear-menu composition (checkboxes vs popover with separators) | runtime capture |

**Every surface's capture list leads with its ordering question, for this reason.**

### 7.3 Defaults that neither source states

The bundles give labels and settings *names*; the docs give behaviour. Neither
marks a default for a control whose whole purpose is to be a default.

| Surface | Unknown default | Closes with |
|---|---|---|
| commit | ~~`Enable staging area`~~ — **closed 2026-10-06**: changelists are the default (`docs: undo-changes`, corroborated by `managing-changelists`, and the settings page: `Enable staging area` off) | ~~settings screenshot~~ |
| commit | all six commit-option checkboxes — **still open**: not on the Commit settings page (dialog-level; R3 capture list) | runtime capture |
| commit | focus target after Commit (necessarily setting-dependent) | runtime capture |
| branches | ~~the `Checkout branch` checkbox's initial state~~ — **closed 2026-10-06** (R3 session 3): **on** by default; Branch Name pre-fills with the current branch name; an undocumented `Overwrite existing branch` checkbox (off) also exists | ~~runtime capture~~ |
| branches | overflow affordance when a 6th recent branch exists | runtime capture |
| branches | focus after a successful checkout | runtime capture |
| branches | ~~the `Clean working tree using` default~~ — **closed 2026-10-06** (R3 session 2): **Shelve** is the default (⦿ radio). The settings page lists Stash first; the original Shelve-first inference was right and the §7.1b withdrawal is corrected | ~~settings screenshot~~ |
| integrate | **every flag in all three dialogs** (dialog-level) | runtime capture |
| integrate | `On empty cherry-pick commit:` | settings screenshot (not on the VC\|Git page) |
| integrate | status-bar presentation with two concurrent root operations | runtime capture |
| remotes | ~~`Show Push dialog…`; `…protected branches`; `Use Safe Force Push`; `preview commits`~~ — **behaviour closed 2026-10-06** (`docs: commit-and-push-changes`); **checkbox states closed by R3 sessions 2–3**: Show Push dialog **on**, Show-only-protected off, `Use Safe Force Push` **on** | ~~settings screenshot~~ |
| remotes | ~~`Fetch tags:`~~ — **closed 2026-10-06** (R3 session 2): **Auto (Follow git config)** | ~~settings screenshot~~ |
| remotes | `Update all repositories` in Push Rejected (implied checked) | runtime capture |
| shelve | ~~the `Clean working tree using` default~~ — **closed** (see branches row) | ~~settings screenshot~~ |
| patches | patch file encoding; ~~`When patch is created`~~ — **closed 2026-10-06**: **Ask** | settings screenshot (encoding only) |
| diff | highlighting granularity; side-by-side vs unified; `Align Changes` — `settings-tools-diff-and-merge` (re-read 2026-10-06) is silent; the Tools \| Diff & Merge settings page itself not yet captured | settings screenshot |
| log | `Compact References View` (a Log-gear toggle, not on the settings page); ~~`Show Tag Names`~~ **off**; `Show Details`; ~~`Show Diff Preview`~~ **off**, Location **Bottom** | runtime capture (gear menu) |
| onboarding | ~~`Recursively clone submodules`~~ — **closed 2026-10-06**: behaviour unconditional (`docs: set-up-a-git-repository`), checkbox **on** (R3 session 3, Advanced Settings) | ~~settings screenshot~~ |

**R3 session 2 (2026-10-06) captured ~30 further defaults** beyond the rows
above — `Protected branches = master;main`, `Ask for confirmation to drop
commits = on`, `When files are created/deleted = Ask`, `Restore workspace when
switching branches = on`, `Remove successfully applied files from the shelf =
off`, shelf base revisions on (500K cap), `.idea/shelf` default location,
`Update method = Merge`, `Enable staging area = off`, commit-message
inspections off, all Log-page toggles, GitHub/GitLab page defaults, and the
full `File Status Colors` table — all cited in
[`runtime/R3-SESSION-2.md`](../research/evidence/rebased/runtime/R3-SESSION-2.md).

### 7.4 Failure states with no documented recovery

| Item | Evidence | Closes with |
|---|---|---|
| `Rollback failed` after a patch apply | GitBundle:374 — **the most dangerous single state in the product, with no documented route** | runtime capture |
| `Could not Restore {0}`; `Branch {0} wasn't deleted` | GitBundle:1352-1353 | runtime capture |
| Six per-verb rewrite failures | GitBundle:275-278, 285 — no bundled recovery action | runtime capture |
| `Could not set user.name and user.email` | GitBundle:1588 | runtime capture |
| `Could not load changes of {N commits}` | GitBundle:279 | runtime capture |
| `Could not find HEAD branch in repository which contains selected {N}` | GitBundle:271 | runtime capture |
| `Cannot add/change/remove remote` | GitBundle:1527, 1532, 1535 | runtime capture |
| `Failed to stash {0}`; `Cannot Stash Changes Now` (**no reason given**) | GitBundle:451, 438 | runtime capture |
| `Cherry-pick conflict` onto a remote | GitBundle:752-753 | runtime capture |
| `Cannot find file for {0}` (conflict resolution) | GitBundle:1517 | runtime capture |
| Unmerged-file scan failure | GitBundle:61 | runtime capture |
| `Revision not found` / `Could not checkout {0}` | GitBundle:1333-1334 | runtime capture |
| `VCS Log tab cannot be opened twice` — does it focus the existing tab? | VcsLog:156 | runtime capture |
| **The diff viewer's own loading and error states — none exist in any source** | see §4.11 | runtime capture; highest value on that surface |
| `Ignore Left/Right Side` and bulk-apply undo | `Diff.Ignore*` | runtime capture |

### 7.5 Interaction details

| Item | Closes with |
|---|---|
| Conflicts: button order; focus after `Accept and Finish`; whether post-finish undo survives in the tray; the file list's row affordance; greyed-vs-hidden for `Accept and Finish` | runtime capture |
| Conflicts: the 4-pane patch-conflict resolution model and what the 4th `Result` pane means | runtime capture |
| Branches: menu ordering; the Recent overflow; the gear's layout; whether the blocking-file list is scrollable | runtime capture |
| Commit: the untracked-file dialog's full field list; staging-mode row affordances; changelist drag grammar | runtime capture |
| Rewrite: the plan editor's layout; `Update Ref`'s visibility rule; the `Stop to Edit` pause UX; whether the reword OK always reads `Continue Rebasing` | runtime capture |
| Remotes: the `Empty repository` target-panel state; the Git Remotes dialog's row actions; focus after push; whether `Unshallow` confirms | runtime capture |
| Log: what `vcs.log.filter.recent=Recent` filters to; the `Paths Affected by Commit` dialog's body; the 6-branch cap and tags; initial focus; the Graph Options submenu's form | runtime capture |
| Integrate: the Merge dialog's field order and whether it has an incoming-commits preview; the remote-branch picker's full field list; whether `Get from Branch` confirms | runtime capture |
| Patches: the first dialog's field list; what an unprompted clipboard detector does with non-patch content; whether `Show Diff` is available before a file is chosen | runtime capture |
| Onboarding: whether the already-under-git guard blocks or warns; whether a clone dialog exists; the empty state of a service's repo list; clone into a non-empty directory; whether the external-path constraint is enforced | runtime capture |
| Diff: ~~whether `Open Repository Version` checks out or opens read-only~~ — **closed 2026-10-06**: it opens the repository version of the file *for editing* (`docs: log-tab`); whether `FocusOppositePaneAndScroll` is bound | runtime capture |
| Shelve: **whether shelf `Delete` and stash `Drop`/`Clear` really have no confirmation** — the docs only describe the menu path, and this decides whether TurboGit's confirming stance is a divergence or a match. **Still uncaptured at runtime**, but no longer un decided: ticket 12 (`1215d09`) shipped both of the deletes the panes expose **behind a confirmation** (`PendingConfirm::DeleteShelf` / `DeleteStash`), landing on TurboGit's existing confirming stance as the documented fallback — recorded as a **divergence**, not a verified match (see §8.4 gap 12). The `Clear` path is not built, so its policy is still open | runtime capture |
| Shelve: the `Already Unshelved` node's shape; the merged tab's row grammar; focus after Delete/Drop/Clear | runtime capture |

### 7.6 TurboGit-side unknowns — **resolved 2026-10-05**

These were questions about *TurboGit*, not about Rebased, so no amount of Rebased
capture answers them. All nine have now been read off the code, and **four gap
verdicts in §5 changed as a result.** The table is kept so the reasoning is
auditable, not because the questions are open.

| # | Question | Verdict | Evidence |
|---|---|---|---|
| 1 | `Commit All when Nothing Is Staged`? | ~~**partial** — the fallback exists in the service layer but the UI makes it unreachable~~ **superseded 2026-10-06: has** — ticket 21b implemented (see §7.1b) | `settings.commit_all_when_nothing_staged` (`crates/turbogit-domain/src/model.rs:707`), settings modal (`settings_modal.rs:255`), empty-selection note and gate (`commit_window.rs:2203-2217`). Original verdict: `changes.rs:46-47` fallback gated out by `has_selected_changes` |
| 2 | Remote-only gate on `Pull into … Using X`? | ~~**not implemented** — and the polarity is *inverted*~~ **has 2026-10-07**: ticket 21a gated both items on `has_upstream && !is_local` | `branch_menu.rs` (`Pull` and `CheckoutAndPull`). Originally: both gated on `has_upstream` only with **no `is_local` check**, so Pull was offered on any tracking local branch, while `Merge`/`Rebase` *did* check `!is_local` — source-branch verbs, for which the opposite polarity is correct. The P4 read found no wrong git command behind the old gate: `Pull` dispatches the port's branch-less `pull` (a plain `git pull` on the checked-out branch's upstream), so the defect was a mislabelled offer, not a wrong ref |
| 3 | `History for Selection`? | **partial** — file history yes, selection/line history no | File history is a path-scoped mode of the Log window (`log_window.rs:2-4`, `:1975-1979`, `root_caches.rs:51`); `LogScope` is `Path`/`Ref`/`Search` only (`root_caches.rs:50-57`) and `BlameTarget` carries no line range (`state.rs:699-703`) |
| 4 | Asterisk for author ≠ committer? | **not implemented** — and of the reference's five row signals TurboGit paints **two** | Per-root stripe: **yes** (`log_window.rs:1644-1645`). Ref colour: **only in the hover tooltip** (`:1763-1770`), the row itself uses one neutral pill (`:1743-1744`). Background-for-mine, bold-for-mine, asterisk: **none** — `body_font()` is unconditional (`:1408-1410`), and `commit_row` never reads `c.committer` (`committer` appears only in the details pane, `:2286`) |
| 5 | Log search regex / match-case? | **not implemented** — plain substring, always case-insensitive | `log_display.rs:176-182` is `to_lowercase().contains()` on both sides. No regex engine in the workspace; `widgets::search_input` has no toggle buttons (`inputs.rs:21-23`). The code-change search is **pickaxe** (`git log -S`, `root_caches.rs:72` → `cli.rs:302-304`), which counts a *literal* string |
| 6 | Branch-delete undo lifetime? | **has — and 10 s, not "until restart"** | `branches.rs:695-698` expires at `Duration::from_secs(10)`, lazily, at next paint of that pane. Survives `AppState::refresh` (which never touches `branches_undo`, `state.rs:1844-1872`); does not survive restart — absent from `UiPersist` (`persistence.rs:79-108`) |
| 7 | Per-root in-flight in the status bar? | ~~**not implemented**~~ **has 2026-10-08**: ticket 16 (`c8ef0a0`) put a per-root in-flight item with its own Abort/Continue/Skip in the bar, read from git's markers rather than from any app-side flag — see §5.8's two marked rows and §8.4 gap 15. The evidence below is the original read and stands as history: `ui.busy` is still the single global `bool` it describes, and the new item deliberately does **not** route through it | The 24px bar (`shell.rs:596-674`) paints a version line, counters, chips, a root total, and `if state.ui.busy { ui.spinner() }` (`:662-664`). `ui.busy` is a **single global `bool`** (`state.rs:1107`) that cannot name a root. `activity.rs` records only *completed* outcomes |
| 8 | Is there a clone dialog? | **has** — on the Welcome screen, not a `Dialog::*` | `welcome.rs:422-478` (card "Clone a repository"), URL input at `:465-469`, `Shallow clone (--depth 1)` at `:433-438`, `clone_into` (`state.rs:2887-2899`) → `GitExecutor::clone`. Second entry: `PendingConfirm::CloneRepo` (`state.rs:2171`) |
| 9 | Any patch surface? | ~~**partial** — create yes, apply no~~ ~~**partial, and the other way round since 2026-10-09: apply yes (two of four routes), create still only on the log's menu**~~ **has, both directions since 2026-10-10: apply (two of four routes, ticket 18) and create (all five routes + the settings dialog, ticket 18b — `2bed4ce`, merged as `76270a9`)** | ~~`CommitMenuAction::CreatePatch` writes a patch via a native save dialog (`src/app.rs:32-36`) — the create route is **unchanged by ticket 18**~~ **that route no longer writes through a save dialog**: ticket 18b re-pointed the log's commit menu at `open_create_patch_on` (`log_window.rs:1605-1608`), so it opens the same `Patch File Settings` window the other four routes do, and the `state.rs:3342-3374` write-only path this cell cited no longer exists. The `patch_writer` seam (`src/app.rs:31-36`) survived and was **repurposed**, not orphaned — `AppState::choose_create_patch_location` (`state.rs:3862`) is now its only consumer, filling the settings dialog's location and name fields. See §5.9's five marked create-side rows for what landed and for the four things still open inside it. ~~**No patch-file apply exists**~~ **a patch-file apply now does**: ticket 18 (`fa65cbc`, merged as `f2fb686`) landed the apply window with `GitExecutor::apply_patch_to_worktree` (`git apply`, no `--cached`) behind a preflight and a total rollback — see §5.9 — and `src/app.rs:41` is its own native picker. `GitExecutor::apply_patch_to_index` keeps its old role and nothing new: `git apply --cached` over an in-memory value for granular staging (`partial.rs:192`, `:213`, `:237`), and the one whole worktree call is now proven all-or-nothing where those per-section calls are not (`patch_apply_verbs.rs::one_whole_call_is_all_or_nothing_where_the_per_section_calls_are_not`) |

#### Two corrections this produced

- **The `Operation` count in this document was wrong** (said 12; it is 10). See
  §2.5.
- **Question 6 inverted a risk I had recorded as open.** Ticket 07 asked for
  TurboGit's branch-delete undo lifetime to be *decided*, on the assumption there
  was no bound. There is one — 10 seconds, enforced in `branches.rs`. That is a
  **stricter** bound than the reference's "until you restart", so there is nothing
  to decide; ticket 07 is now scoped to the two disclosure links only. Whether 10 s
  is the *right* bound is a product question the evidence cannot settle, and is
  flagged rather than answered.
- **Question 5 refines §5.7's log-search row.** The spec's `partial` verdict was
  right but for the wrong reason: TurboGit does not use fuzzy matching in the log
  (`fuzzy_word` is the *branch list* filter, `branches.rs:127`), and its pickaxe
  path is a genuine asset — `git log -S` finds commits whose *content* changed,
  which no reference feature matches. Recorded in §6 as a reverse gap.

---

## 8. Divergences

Every intentional departure, with its dimension. The per-surface tables carry the
detail; this is the consolidated view, split by whether it is a **deliberate
improvement** or an **accepted cost**.

### 8.1 Deliberate improvements over the reference

| # | Dim | Rebased | TurboGit | Why |
|---|---|---|---|---|
| 1 | UX | 2 resolution choices (yours/theirs) | 4 — ours, theirs, both ×2 orderings | "Take both" is the common case for non-overlapping edits git still flagged, and ordering matters for import lists |
| 2 | UX | Two bulk buttons differing only in whether they touch one conflict class | One `Apply to all N remaining` | Two adjacent bulk buttons is a mis-click generator |
| 3 | UX | Force push is a setting-driven global | `CleanTreeMethod` is **per invocation** | A global makes the destructive path's mechanism unpredictable across sessions |
| 4 | UX | Ask **before** nothing on branch delete; disclose **after** | `PendingConfirm` asks **before** | Same information, safer ordering; the undo window already exists |
| 5 | UX | Undo validity assumed at reword time | Checked **at undo time** (adopting Rebased's own guard) | Adopted from the reference — TurboGit's model was the weaker one |
| 6 | UX | Uniform undo coverage implied | **Asymmetric** undo: drop and reword undoable, squash not | Matches the reference's actual coverage rather than over-promising |
| 7 | UX | Changelists as a peer second mode | Staging buckets only | Staging is the model a CLI user arrives with; changelists double the state space |
| 8 | UX | Per-line commit inclusion as a 3-member verb family | A File/Hunk/Line toggle + char ranges | A toggle is more discoverable; char ranges are strictly finer than "split chunks" |
| 9 | UX | Editable diff; `Disable editing` off by default | **`Disable editing` semantics** (read-only until opted in) | Most diff openings are reviews; editing should be a deliberate mode |
| 10 | UX | Accept/Append as two controls | **One chevron + modifier** (adopting the reference) | Adopted — one control, accumulating outcome behind a modifier |
| 11 | UX | `rebase.option.rebase.merges` = "Recreate commits topology" (vague, on the most destructive flag) | A label stating the *consequence* | The reference's vaguest label is on its worst flag; the doc itself warns resolved conflicts must be reapplied manually |
| 12 | Flow | Conflicts understood **after** a failed push | Protected-branch guard refuses **before** | TurboGit never has to explain a later rejection |
| 13 | UX | Conflicted unstash: "Resolve conflicts and **drop the stash**" | **Must name the stash as still needed** | Refused deliberately: the stash is the only copy; copying the instruction teaches users to delete their work |
| 14 | UX | Shelf `Delete` vs stash `Drop` — two words for one verb | One verb per mechanism with an explicit object | An inconsistency, not a design |
| 15 | UI | Diff hosting is **two settings** (`Show Diff in Editor Tab` + `Open Diff as Editor Tab`) | **Two actions** | A reference inconsistency; actions are discoverable, settings are not |
| 16 | UX | Entry points fixed regardless of setup state | A tab strip that always exists | The states are mutually exclusive, so the tabs are never *wrong* — only absent in the reference. Progressive reveal would add a shell state machine for no benefit |
| 17 | UI | "Changes" names the default changelist | "Changes" names the *view* | Recorded because a user reading a Rebased screenshot would conclude otherwise. Any TurboGit changelist-like grouping must not reuse the word |
| 18 | UI | Diff viewer has no documented error or decode states | `diff_load.rs` limits, `panes.rs` binary routing | **TurboGit is ahead; keep it that way.** The reference's silence is not a specification |

### 8.2 Accepted costs of the egui/ Rust medium

| # | Dim | Rebased | TurboGit | Cost accepted |
|---|---|---|---|---|
| 1 | UI | Floating `NSPopover` anchored to the header | A tab plus a status-bar popup | No hover-in-place tree with a live gear menu |
| 2 | UI | A node whose *name and shape* change with a setting | One shape; the difference is deferred to expansion | Loses a clever idea that is a support burden |
| 3 | UX | Drag-reorderable changelists; `⌃`-drag copies; a 1s activation delay | Menu-reachable equivalents | **The `⌃`-copy semantics and the 1s delay are deliberately not reproduced** |
| 4 | Flow | Commit window floated / windowed / modal / dissolved | A fixed left panel | Shape parity unavailable; topology preserved |
| 5 | UI | A separate OS window for the diff | `Diff.OpenDiffInEditor` cannot float | Minor |
| 6 | UI | Real `NSMenu` with platform mnemonics | egui menus | No system-level menu integration |

### 8.3 Intentionally absent

| # | Dim | Rebased | Verdict | Why |
|---|---|---|---|---|
| 1 | UX | Commit checks: reformat, rearrange, optimize imports, cleanup, copyright, malicious deps | **absent** | Not an editor: no code style, no inspections. The *structure* — one options affordance behind one keystroke — is what parity requires, and the absence of the checks does not excuse the structure being missing (see §5.3) |
| 2 | UX | `After Commit` FTP/SFTP/WebDAV upload | **absent** | No deployment model |
| 3 | UX | Unsaved-file save prompt on commit | **absent** | No editor, so no unsaved files |
| 4 | UX | Credential storage (Keychain / KeePass) | **absent** | A security surface with its own threat model; out of scope for a parity pass |
| 5 | UI | WSL2 detection and `\\wsl$` paths | **absent** | Platform, and the target is macOS/Linux |
| 6 | UX | Trust prompt, build-tool prompt, share-settings prompt | **absent** | IDE inheritance per `DISTILL.md` rule 5. **But the copy pattern transfers:** the safe option's label states its own cost — "Safe mode, **Version Control is not available**" |
| 7 | UX | Issue-tracker links in commit messages | **absent** | A separate feature with its own configuration surface; no TurboGit intent is documented in `research/` |
| 8 | UX | `Analyze and apply patch from clipboard on the fly` | **absent, deliberately** | An unprompted clipboard watcher is a privacy-adjacent surprise, and no invalid-clipboard state is specified. The doc's word is "suggest" and that word is load-bearing. If implemented, suggestion-only with a clear dismissal |
| 9 | UX | Drag an **email attachment** into the editor | **absent** | IDE integration with no TurboGit equivalent and no clear home |
| 10 | UI | `Vcs.Diff.ResolveConflictsInImports` | **absent** | IDE inheritance |
| 11 | UX | Ten log presentation toggles | **absent** | Refinement, not workflow. The *workflow* toggles (regex, match case, details, diff-preview location) are in §5.7 as gaps; the cosmetic ones are a priority judgement |
| 12 | UX | A force-add affordance for `.gitignore`d files | **absent** | Nothing to gate. **Trap recorded:** if added, the partially-cancelling semantics (Cancel suppresses only the force-add) must be specified deliberately |
| 13 | UX | Line-ending (`core.autocrlf`) pre-commit warning + `Fix and Commit` | **absent** | It mutates global git config as a side effect of a commit. TurboGit shows the discrepancy in the diff and lets the user decide |
| 14 | UX | `Change Directory Paths` path repair | ~~**absent**~~ **no longer absent — landed 2026-10-10, ticket 18b (`2bed4ce`)**. It was listed here only as *deferred with the rest of the patch surface, not rejected*, and the deferral has now been paid: the counted repair pair is implemented and asserted press by press (see §5.9's row). **Nothing about it is intentional absence any more**; the row stays numbered so §5.9's cross-reference does not dangle |

### 8.4 Open gaps recorded as divergences

These appear in a per-surface divergence table **as gaps, not decisions**. They are
collected here so they are not mistaken for settled positions.

| # | Gap | Surface |
|---|---|---|
| 1 | ~~Recent node (cap 5) missing from the Branches tab~~ — **resolved 2026-10-07**: ticket 03 (`d99df82`) landed the `Recent` group as the branches tree's first view-level group, derived from HEAD's reflog at the engine seam, joined to live local branches and capped at five (see §5.2). ~~**Still open** — the group is unconditional; gating it is gap 3's `Show Recent Branches` toggle~~ **gated 2026-10-07** by ticket 04 (`612975b`), whose gear carries that toggle and hides the group at the renderer (see §5.2, gap 3) | `02` |
| 2 | Favourite-the-main-branch-by-default | `02` |
| 3 | ~~Gear with four grouping toggles~~ — **resolved 2026-10-07**: ticket 04 (`612975b`) landed the Branches toolbar's settings gear (`widgets::icon_button(Icon::SETTINGS)`, hosted through `menu_host` like every other menu on the surface) over the four toggles — `Group by Directory`, `Group by Repository`, `Show Recent Branches`, `Show Tags` — as `BranchesViewOptions` in `UiPersist` with `#[serde(default)]` (all four on), read at `launch_in`, written the frame a toggle changes, and exposed live as `UiState::branches_view`. Each toggle is spent at the layer that owns its effect rather than one flag fanned out, and `build_branch_view` keeps its all-on signature as the wrapper for the pure-builder ratchet and the Git Log pane's tree (see §5.2). **Still open, deliberately not claimed as landed**: the gear's own **composition** — checkbox vs popover, separators, apply-on-click vs apply-on-dismiss, whether the two grouping levels compose or exclude — was never captured (see §7.5's "the gear's layout"), so what shipped is the simplest defensible reading of the four documented names, not an evidenced menu | `02` |
| 4 | **Partially resolved 2026-10-09** — this is ticket 06's row (the ticket table cites "§4.2, §4.6"; §4.6 holds no divergence *row*, only the "opposite theories of failure" note at its tail, which this ticket honours rather than unifies — the per-item row is §5.2's). ~~Multi-root divergence rollback offer~~ **landed as two separate decisions**: `branch_divergence_for` records the divergence from the run's **per-root rows** for **any** bulk op that changed some roots and failed others, and pushes it to the **durable activity feed** as a `Warning`; the rollback **offer** arms only when `record.reversible()` — still exactly `op == BulkOp::CreateBranch` — and repairs through the existing `bulk_rollback(record_id)`, so the preflight matrix and the cascade run monitor remain the authority and no second undo mechanism was added. `Skipped` rows arm neither half, and no global success/failure flag exists on the path. `53c8b94` is what split the two after `1c4339b` had fused them: fusing either promises a rollback the app cannot honour or leaves a divergence unrecorded. **Still open — two things, both recorded not fixed**: (a) **the merge-cascade case** — a `BulkOp::Merge` cascade can land in some roots and fail in others exactly the same way, and `reversible()` refuses it, so its divergence is **recorded with no repair path**. Accepted limitation: the feed names a split nothing closes; giving it one is a separate decision about merge undo, not a gap this branch had the authority to fill. (b) **`BulkOp::Custom` runs arbitrary git** — recording its divergence is right, and the safety question of whether a destructive typed command should ever be offered a rollback is the programme's tracked decision, **not** closed here; widening `reversible()` to cover `Custom` would be a regression. Also open, unchanged by the ticket: the feed is a **1000-entry in-RAM `Vec` not covered by `UiPersist`** (`activity.rs:74`, so "durable" means per-session, and the divergence sentence does not survive a restart), and `bulk_rollback` issues **N synchronous git calls on the UI thread** — both on the programme's review list. **No golden coverage** — `tests/snapshots/` has no divergence PNG, as §8.4 #15 and #16 record for their surfaces | `02` |
| 5 | `Checkout branch` default + prefix suggester | `02` |
| 6 | Silent auto-reset when no commits can be lost | `02` |
| 7 | ~~Post-hoc delete disclosure: unmerged-commits link~~ — **resolved 2026-10-07**: ticket 07 (`91cf199`) landed **both** disclosure links (see §5.2). One `UnmergedCommits` snapshot, captured while the ref still exists, drives the link in the pre-flight confirmation and the link in the post-delete notification; both open `Dialog::BranchNotFullyMerged`, and the remote-twin link routes through `PendingConfirm::DeleteRemoteBranch`. **Deliberately not landed with it**: TurboGit still asks *before* deleting (its own divergence, §5.2 row "Delete: immediate, forced, no confirm"), and the undo window's **lifetime is untouched** — the 10-second bound stays as reverse gap §6 #20. Gaps 5 (`Checkout branch` + prefix suggester) and 6 (silent auto-reset) are **not** these tickets and stay open | `02` |
| 8 | Post-commit `Edit Commit Message` window | `01` |
| 9 | `Edit` and `Update Ref` plan verbs | `06` |
| 10 | `Pick`-resets-row; per-commit undo notifications; repo-changed guard | `06` |
| 11 | ~~`--onto`; in-flight-rebase guard; four-reason gating~~ — **resolved 2026-10-07**: ticket 10 (`dcd4017`) landed `--onto` as `RebaseKind::Onto { new_base, old_base }` behind a `--onto` option whose two base fields are ordered, individually validated and individually errored (asserted by galley origin, not by presence), together with the in-flight-rebase guard that names the holding operation **per root** on the rebase dialog's start button; ticket 11 (`09693e8`) then landed the four-reason gating as the closed `rewrite_gating::Block` set — cherry-pick / rebase / merge / revert each naming itself, at most one reason per row, ordered by what the developer can act on next, reaching both the row's hover text and the shell's status bar through the one-slot `take_status_hint` channel (see §5.4). **Deliberately not landed with it**: the generic owner-less `Cannot {0}` fallback (no producer), a detached-HEAD drop *confirmation* (TurboGit gates that case instead), and any gate on the interactive-rebase plan editor's own verbs — that editor *is* the rebase. **Still open, named here so it is not mistaken for settled**: the cross-repository/external pair is computed and unit-proven but paints on no surface while a non-empty scope replaces the tool window, so the *multi-root* half of this gap is enablement without a reachable row | `06` |
| 12 | **Partially resolved 2026-10-08; ticket 12's own scope closed 2026-10-10** — this is ticket 12's row (the ticket header cites "§4.5, §5.5, §8.4 gap 12"; verified on this tip: §4.5 is the `07-shelve-stash` surface, §5.5 is the Shelf-and-stash gap table, so the citation maps here). Of this bundle's five items, **two** landed, and they are **both halves ticket 12 was split into** — "panes first, unshelve dialog second": **the panes-and-verbs half** landed: ~~Shelf/Stash panes~~ — ticket 12 (`1215d09`), which made both sub-tabs real (ADR-0008's placeholders retired), the Shelf pane listing `shelves_for(root)` for the focused root from a store that is now read back at launch, the Stash pane listing `RootCaches::stashes(root)` through the `pump_read` seam, and **eight verbs reachable** — two header verbs (`Shelve…`, `Stash…`) plus three per shelf row (`Unshelve`, `Unshelve and Remove`, `Delete Shelf`) and three per stash row (`Apply Stash (keeps)`, `Pop Stash (removes)`, `Delete Stash`) — pinned by `shelf_stash_panes`'s 8 tests. See §5.5's four marked rows for the shape, the name-based re-find of the parked entry and the success-arm-only record removal. Then **the second half — the `Unshelve` dialog — landed 2026-10-10** (`c6b00c9` feat + `9103ba9` test, merged into `rebased-parity` as `d1a39ba`): `Dialog::Unshelve` behind one opener, entered from the Shelf pane header and by palette-only `Action::Unshelve`, carrying the reference's `Remove successfully applied files from the shelf` (off on every open, one affirmative that renames and recolours) and **`Name` role 1 re-pointed** to *which of this root's shelves to apply*. **The four changelist-dependent fields (`Name` role 2, `Comment`, `Set active`, `Track context`) are divergences by decision, not gaps in this ticket** — they belong to a changelist model the programme keeps out of scope, and they are genuinely absent (no `text_edit` anywhere in the dialog, only three new `DialogState` fields), not built-and-inert. See §5.5's Unshelve-dialog row for the shape and the two recorded costs. **Still open — three of the five, and none of them ticket 12's**: **`Recently Deleted`** (and with it the `Already Unshelved` filter + `Restore`); **base revisions** for 3-way merge; the **relocatable shelf location**. **No golden coverage** — the Unshelve dialog is the matrix's **fifth uncaptured window**, after Patch File Settings, the apply window, Get-from-Branch and the divergence offer (`acceptance_matrix.rs` never opens `Dialog::Unshelve`), and the goldens themselves are evidence-free: the suite self-sets `UPDATE_SNAPSHOTS=force`, so all **11** numbered captures are repainted on every plain run and can never fail. This merge reverted that batch and committed no PNG. **Recorded as a divergence, not a verified match**: shelf `Delete` and stash `Delete` **ask for confirmation** — whether the reference confirms either was never observed at runtime (§7.5), so this landed on TurboGit's existing confirming stance as the documented fallback, and a sitting that shows the reference destroying silently turns it into a match with only the note changing. **Also open outside the bundle**: the merged-into-one-tab setting, stash `Clear`, and the fact that **ADR-0008's text and `commit_window.rs:223`'s strip comment still call these tabs placeholders** — re-checked on this merge, `commit_window.rs:223` is still the exact line, and both were **deliberately left for close-out** (the ADR is a decision record and its amendment belongs there, not to a feature merge; the code comment goes with it) | `07` |
| 13 | ~~Six rejection notifications; `Vcs.Push.Simple`; settings write-back~~ — **resolved 2026-10-07**: ticket 13 (`798db84`) classified the six push-rejection outcomes and painted one message each, with a next step on the four classes that can honour one, force-with-lease as its own class, and the `auto_update_on_push_rejected` write-back; ticket 14 (`6a6aa3b`) then landed `Vcs.Push.Simple` as the no-dialog direct push verb, together with the push panel's inline `Define remote` (see §5.6). **Still open** — of the five bundled here, two remain: per-root skip reasons (`no tracked branch`, `detached HEAD`), and `Pruned obsolete references` reporting | `08` |
| 14 | **Partially resolved 2026-10-07** — of this bundle's seven items, **one** landed: ~~ten log states with per-state recovery~~, i.e. `LogState` in `turbogit-app/src/log_states.rs` (`3ec61e2`), thirteen named states over the four things the log loads independently, every failure naming the read that died and every error or empty state carrying its own recovery, dispatched per pane at `log_window.rs::log_state_slot` / `apply_log_recovery`, with the failure recorded per read of per key in `RootCaches` rather than as a surface-wide flag (see §4.7, §5.7). Its `Commit local changes` copy rule landed with it, together with the unborn-HEAD read that makes the two empty states distinguishable at all. **Still open — six of the seven**: the **two non-destructive filter escapes**, which *ship* as `hidden_jump_band` (`View and reset filters` + `View in Diff`, the second standing in for `View in new tab` because this Log has one tab) but are **untested at every layer** and are therefore **not claimed as landed**; the **asterisk**; the **six-ref cap**; **topological sort**; **`Not Cherry-Picked`** greying; and **copy current branch name** — the copy-verbs item is one verb short, not three, since `Copy hash` and `Copy commit message` have existed on the commit context menu since `f4a46fa` and are a branch-menu question rather than a log-state one. **Verdict: open, not closed — and most of what is open was never scheduled.** Only the ten-state machine was ever ticketed (§9 ticket 15, landed as `3ec61e2`); **copy current branch name** is that ticket's own deferred remainder and the two filter escapes shipped untested, while the **asterisk**, the **six-ref cap**, **topological sort**, **`Not Cherry-Picked`** greying and the **regex / match-case** search (§7.6 #5, still `to_lowercase().contains()` at `log_display.rs:176-182`) sit in no ticket, so no merge could have closed them | `03` |
| 15 | **Partially resolved 2026-10-08 (twice)** — this is the `04-integrate` bundle row and it spans **two tickets**: 16 (the status-bar in-flight item with Abort/Continue/Skip) and 17 (`Get from Branch` + the three-colour status grammar). Of its five items **one** landed outright: ~~`Skip Commit`~~ — ticket 16 (`c8ef0a0`, token inventory `b89839b`), which landed all seven lifecycle verbs as the named `Operation::Resolve` behind a per-root status-bar announcement read from git's own markers, `Skip` needing a new port method (`GitExecutor::rebase_skip`). See §5.8's two marked rows for the shape, the side-by-side two-root divergence and the deliberate no-app-side-flag decision. **A second item is now half-landed**: ~~**verbs 4 and 5** as a pair, of which neither has landed~~ — **verb 5 landed 2026-10-08 by ticket 17 (`0d0186a`)**: `Get from Branch` with its **three-colour status grammar**, entered from `Show Diff with Working Tree` as the branch row's 11th item, all recorded in §5.8's three rows above. **Still open — three items whole and one half**: `Cherry-Pick Selected Changes` (**verb 4**), which ticket 17 did **not** touch and which has no operation, dialog or menu item anywhere in the workspace; the **empty-cherry-pick policy**; the `cherry-picked from <hash>` **suffix** (Rebased's default is **off**); and the **two-dot range filter**. Do not read this row as the integrate surface being settled: the bar carries the *lifecycle* half of the bundle, the file-level take is new and unphotographed (no golden of the window), and verb 4 is untouched | `04` |
| 16 | **Partially resolved 2026-10-09; the create half closed 2026-10-10 by ticket 18b, and the deferral-and-policy half 2026-10-10 by ticket 18c (`77a6c92`..`8b83775`, merged as `f2a770d`) — substantially landed, with named residues, not struck complete** — this is ticket 18's row (the ticket header cites "gap 16", and this bundle row *is* the patch surface, so the citation maps here; the per-item rows are §5.9's). Of the surface's fourteen §5.9 items, **four** landed with ticket 18: ~~Apply from file / clipboard~~ — **two of its four routes** (`From file`, `From clipboard`; `on the fly` stays §8.3 #8's deliberate absence, drag is unimplemented), ~~`Show Diff` as a preflight~~ — non-mutating by assertion, over `check_patch`, with the parsed blast radius stated before git answers, ~~**atomic total rollback + named `Rollback failed`**~~ — the surface's highest-value item, held by bytes recorded before `git apply` rather than by asking git to undo it, with `Restore recorded content` as the state's one action and `record_apply_outcome` putting both titled states in the feed, and ~~Git-format provenance auto-filling author + message~~ **in the window only** (no changelist, so nothing consumes the fields past it). See §5.9's four apply-side rows and §4.9's landed-half note (ticket 18b's five create-side rows are marked in the same table, and the tail of this row counts both). **The record's lifetime was a second, separate defect and `18113f1` is what closed it**: the pre-apply bytes lived in the window-scoped `apply_patch_outcome`, which patch-text edit, route switch, root switch and reopen all cleared — stranding an unrestored tree with no recovery. They now live in `AppState::rollback_recovery: HashMap<RootId, ApplyOutcome>`, per root because the apply is already `Affected::Root`, holding **only** the unresolved `RollbackFailed` variant and re-seeding it after each of those transitions; the route switch moved into `AppState::set_apply_patch_source` so the clear rule has one owner. **Known accepted costs, recorded rather than fixed**: an older failure can repaint after a newer clean `Aborted`, and a *successful* retry writes nothing to the durable feed, so the feed's last word on that root stays `Rollback failed` — the second is on the programme's review list. **Eleven of the fourteen now landed, three still open — the create direction closed 2026-10-10 by ticket 18b (`2bed4ce`, merged as `76270a9`)**, which took the five items this row listed as its own remainder: ~~**create-patch in all three directions**~~ (**all five create routes**, and the §5.9/§7.6 #9 disagreement this row said neither merge would settle **is settled here** — §5.9's row 1 carries the correction it owed, and §7.6 #9's create claim is rewritten against it), ~~`Copy as Patch to Clipboard`~~ (asserted by the dialog it does **not** open, and by the bytes that reached the clipboard), ~~**Patch File Settings**~~ (location, base path, reverse, encoding — four fields, painted in the reference's order), ~~`Change Directory Paths` and its counted repair pair~~ (the count asserted in the strong form `[0]→1→2→3→2→1→0→[3]`, press by press, against the paths each press produced), and ~~the **`Reverse patch`** field~~ (git's own `-R`, proven by applying it and reading HEAD's content back). 18b also fixed a real defect rather than only adding surface: the clipboard slot was drained inside the dialog's paint frame, so the verb defined by opening no dialog could never copy — `4e14474` moved the drain to the shell frame, unconditional. **Ticket 18c then took the last two of the five this row listed**: ~~**`Import to shelf`**~~ — the reference's non-destructive exit landed as parking at **three** entry points (apply window, Shelf view's `Import Patches…`, the created-patch ask), the patch kept as **its own bytes** at `<project>/.turbogit/shelf/<slug>-<nanos>.patch` addressed by `Shelf.source: ShelfSource` (`Stash` | `Patch { file }`) and **deliberately not a git stash entry** — a stash can only exist for work already in a tree, so stashing an unapplied patch would mean applying it, the act the deferral defers — with `unshelve` branching on the field and **no new `Operation`/`OpKind`**, and ~~the three-way `When patch is created:` policy~~ — **captured default `Ask`, persisted under `serde(default)` across restart and legacy-load, enforced at the single site `report_created_patch` with every arm reaching the durable feed**; its one divergence is named, not hidden: the reference's `Show in {0}` reveals in the OS file manager, and with no reveal seam this build hands the patch to the apply window instead. **Still open — three of the fourteen, none of them in this ticket set**: **changelist destination**, the **4-pane** patch-conflict view, and `filetype.patch` registration; and §4.9's enumeration of the reference's apply dialog also carries an **apply-side `Change Directory Paths`** that never landed anywhere in the set — 18b's counted repair pair lives on the create window and `apply_patch.rs` has no repair affordance. **Also open, and worth naming before anyone calls this surface done**: **five surfaces now have no golden or painted-confirmation coverage** — Patch File Settings, the apply-patch window, the `Get from Branch` window (§8.4 #15's standing record), the divergence offer, and the created-patch ask / `Import Patches…` side (the ask's buttons are paint-asserted only inside the create-window tests; the import dialog is asserted at state level, never at pixel level) — and **the matrix rewrites all 11 golden PNGs on every plain run even without `UPDATE_SNAPSHOTS`**, so the goldens are evidence-free; this merge reverted that batch and committed no PNG. **Top pin for the pending test-only pass**: the two `shelf_store_record_preservation` suites **never assert `Shelf.source` at all** — their wire probes read only `root` — so although the behaviour is verified correct (oldest no-root/no-source stores load; stash rows rewrite keyless per row; `Patch { file }` survives a whole-file write), a `Patch`→`Stash` degradation on relaunch would pass every committed test. Five further costs recorded in §5.9 rather than hidden: the dialog's **`To clipboard` checkbox is proven to paint, never to redirect** — the arm's effect is pinned only by setting `create_patch_to_clipboard` directly (`patch_created_policy.rs::the_clipboard_route_creates_no_file_and_so_asks_nothing`), and no test presses the checkbox itself; commit naming is proven **warm-cache only**, `test_support`'s `patch_reads()` is **recorded but consumed by no test**, the policy's **rootless fallback** (an `Ask` with no root answers the `Wrote` toast, because an ask that could not honour its own first verb is a decoy) is **untested**, and unversioned files are outside a local-changes patch because git has no diff for a path it never held | `10` |
| 17 | In-dialog git check; already-under-git guard; git-not-found offer; `Nothing to commit` status; add-offer for external files | `09` |
| 18 | **Partially resolved 2026-10-07** — this is ticket 20's row (the ticket header cites "§8.4 gap 18"; do not conflate it with reverse gap §6 #18, *Design-token discipline*, which is a different 18). Of this bundle's five items, **two** landed: ~~whitespace/highlighting policies~~ and ~~`F4` caret preservation~~, i.e. both five-value policies as one `⌃⇧D`-reachable menu (`6429b89` — `DiffWhitespace` + `DiffHighlighting` in `turbogit-domain/src/model.rs`, decided in `services/diff_policy.rs` and folded once at `app/diff_model.rs::flush_changed_run`, reachable by `⌃⇧D` and by mouse from the file-row trigger that retired the boolean checkbox, persisted in `UiPersist` with `serde(default)` and round-trip-proven by `9551d71`), and `F4` `Jump to Source` with the caret line derived and carried into blame (`cb911b0` reads what `6429b89` wrote — before it the jump landed at the top of the file). See §5.11's four marked rows for the detail and the honest caveats. `F7`-crosses-files is **not** new work and was never this ticket's: the 2026-10-06 re-test (§7.1b) found `hunk_nav.rs` already does it, and §9.1 records ticket 20 shrinking for exactly that reason. **Still open — three of the five, plus five named escapes**: **non-conflicting apply** (surface `05`, ticket 01; §5.1 still reads "no equivalent anywhere in `crates/`"), **`⌃Z` merge undo with the conflicts-stay-in-sync rider** (§5.11 row 903, stays **partial** — TurboGit's diff is read-only, so there is no text-level undo), and the `F7` **gate** decision — whether to surface IDEA's named `Go to the next file after reaching last change` as a setting instead of the double-press idiom, which §9.1 says is all that was left of that item and is undecided. Deliberately deferred by design, not landed: `Diff.FocusOppositePaneAndScroll` (only the plain `FocusOppositePane` landed), `Align Changes in Side-by-Side`, `Synchronize Scrolling` as press-and-hold, and the `F4` working-tree/`HEAD` line-number divergence | `11`, `05` |
| 19 | ~~**`Pull` / `CheckoutAndPull` offered on tracking local branches** — the reference's remote-only rule is inverted here. Found by reading the code, not the evidence: **a defect, not a parity gap.**~~ **resolved 2026-10-07**: ticket 21a gated both items on `has_upstream && !is_local`. Collapsed form kept as a divergence: the reference splits the family into `Using Merge` / `Using Rebase`; TurboGit's one `Pull` stays collapsed, and because it also needs the row checked out (a remote-tracking row never is) and both engines list remote rows with `tracking: None`, the family is currently withheld on every real row — the two-item split is a separate decision | `04` |
| 20 | **`Commit All when Nothing Is Staged`** — ~~the service fallback exists (`changes.rs:46-47`) but the UI gate makes it unreachable~~ **resolved 2026-10-05**: ticket 21b implemented in the working tree (§7.1b). | `01` |

---

## 9. Tickets

One ticket per implementable unit, at `.scratch/rebased-parity/issues/`. Each
names the surface, the spec section, three structural kittest assertions in the
existing style (`painted_text`, `galley_origin`, `filled_rects` — see
`crates/test-support/src/harness.rs`), and what remains unresolved.

| # | Ticket | Surface | Spec § |
|---|---|---|---|
| 01 | Conflict resolution: bulk non-conflicting apply + Overwrite Changes pre-check | `05-conflicts` | §4.1 |
| 02 | Conflict resolution: named conflict kinds and per-kind reporting | `05-conflicts` | §4.1 |
| 03 | Branches: Recent node capped at five | `02-branches-popup` | §4.2 |
| 04 | Branches: the four grouping toggles behind a gear | `02-branches-popup` | §4.2 |
| 05 | Branches: rename with `Unset upstream branch` | `02-branches-popup` | §4.2 |
| 06 | Multi-root: divergence rollback offer — **landed 2026-10-09 (`1c4339b`, split by `20e3760` + `53c8b94`, merged as `4402cbe`): the divergence *record* for any bulk op that changed some roots and failed others, reaching the durable feed, plus the rollback *offer* only where `reversible()` holds (`CreateBranch`), routed through the existing `bulk_rollback`. **This row stays open on two named points**: a `Merge` cascade records its divergence with **no repair path** (accepted limitation), and whether a `Custom` run's arbitrary git should ever be offered a rollback is a separate tracked decision — not closed here. No golden coverage. See §5.2 and §8.4 gap 4** | `02`, `08` | §4.2, §4.6 |
| 07 | Branches: post-delete disclosure links | `02-branches-popup` | §4.2 |
| 08 | Rebase plan: `Edit` and `Update Ref` verbs | `06-history-rewrite` | §4.4 |
| 09 | Rewrite: post-hoc undo with a repo-changed guard | `06-history-rewrite` | §4.4 |
| 10 | Rebase: `--onto` as two ordered fields | `06-history-rewrite` | §4.4 |
| 11 | Rewrite gating: in-flight state machine + cross-root reasons | `06-history-rewrite` | §4.4 |
| 12 | Shelf and Stash: real panes, and the unshelve dialog — **both halves landed, and this ticket is complete**: panes half 2026-10-08 (`1215d09`, merged as `8d5d068`); unshelve dialog half 2026-10-10 (`c6b00c9` + `9103ba9`, merged as `d1a39ba`) — `Dialog::Unshelve` behind one opener, per-root shelf picker with the newest preselected, the Remove checkbox off on every open and one affirmative that renames and recolours. **The four changelist-dependent fields are divergences by decision, not unbuilt work.** What still sits in §8.4 gap 12 is **not** this ticket's remainder: `Recently Deleted` with the `Already Unshelved` filter and `Restore`, base revisions for 3-way merge, and the relocatable shelf location | `07-shelve-stash` | §4.5 |
| 13 | Push: the six rejection notifications | `08-remotes-sync` | §4.6 |
| 14 | Push: `Define remote` inline, and `Push.Simple` | `08-remotes-sync` | §4.6 |
| 15 | Log: the ten-state machine with per-state recovery | `03-log-tool-window` | §4.7 |
| 16 | Integrate: status-bar in-flight item with Abort/Continue/Skip | `04-integrate` | §4.8 |
| 17 | Integrate: verb 5, `Get from Branch` + the three-colour grammar | `04-integrate` | §4.8 |
| 18 | Patches: create/apply with atomic rollback — **apply + preflight + atomic rollback with the named `Rollback failed` and its per-root recovery landed 2026-10-09 (`fa65cbc` + `18113f1`, merged as `f2fb686`); the create-patch half landed 2026-10-10 (`2bed4ce`, merged as `76270a9`) — all five create routes, the four `Patch File Settings` fields, the counted repair pair, `Reverse patch` as git's `-R`, and the dialog-free clipboard verb, with the patch crossing the port as `Vec<u8>`** — **this row stays open**: `Import to shelf`, the changelist destination, the 4-pane conflict view, the `When patch is created:` policy and `filetype.patch` are not built, and neither patch window has a golden | `10-patches` | §4.9 |
| 19 | Onboarding: the in-dialog git check, plus init's already-under-git guard | `09-onboarding` | §4.10 |
| 20 | Diff: whitespace and highlighting policies; `F4`, `⌃⇧⇥` — **shrunk 2026-10-06**: `F7`-crosses-files is already implemented (§7.1b) | `11-diff-viewer` | §4.11 |
| 21 | **Two defects found by reading the code** — `Pull` offered on tracking local branches; `Commit All when Nothing Is Staged` unreachable | `04`, `01` | §7.6, §8.4 |

**Ticket 21 is not a parity item.** It answers the opposite question — two places
where reading TurboGit's own source found a bug the competitor's documentation
could never reveal. Both would still be bugs if Rebased were deleted.

**Sequencing.** 19 (the git check), 20 (diff policies) and 15 (log states) are
self-contained and unblock nothing else. 01 and 18 carry the most safety.
03/04/12 are the largest surfaces. 06 and 13 are the highest-value *behaviour* per
line. **None of the twenty requires a decision this spec does not already make** —
every open question in §7 is a *default value* or an *ordering*, both of which are
cheap to settle at implementation time by a human looking at Rebased.

### 9.1 Three tickets that shrank when the code was read

§7.6 was resolved on 2026-10-05 by reading TurboGit's own source rather than
capturing Rebased, and **two ticket premises turned out to be wrong**:

- **Ticket 19** was written as "build a clone dialog" because §5.10 read `missing`.
  TurboGit has one — a Welcome-screen card (`welcome.rs:422-478`) with URL input,
  shallow checkbox and a `Clone` button. The ticket is now the environment check
  plus init's guard; the service-column layout is **blocked** on the
  hosting-integration decision (§8.3 #7), not merely unknown.
- **Ticket 07** asked for the branch-delete undo lifetime to be decided, assuming
  no bound existed. There is one: **10 seconds**, enforced at `branches.rs:695-698`.
  It is stricter than the reference's "until restart", so there is nothing to
  decide.
- **Ticket 20** listed `F7`-crosses-files as work; the 2026-10-06 re-test (§7.1b)
  found TurboGit already implements it (`hunk_nav.rs`). The ticket keeps the
  whitespace/highlighting policies, `F4` and `⌃⇧⇥`.

**The lesson generalises: a gap list built from the competitor's documentation is
a hypothesis about your own product, not a measurement of it.** Any row whose
TurboGit column said `—` was unverified, and several of them were wrong in the
optimistic direction. The remaining `—` rows in §5 should be treated the same way.

---

## Provenance

| Artifact | Path |
|---|---|
| This document | `docs/spec/rebased-parity.md` |
| Per-surface evidence (11 files, ~5 850 lines) | `research/evidence/rebased/parity/*.md` |
| Harvest report | `research/evidence/rebased/HARVEST-REPORT.md` |
| Source map and dead ends | `research/evidence/rebased/SOURCES.md` |
| Action inventory | `research/evidence/rebased/actions-labeled.tsv` |
| Keystrokes | `research/evidence/rebased/keymap-bindings.txt` |
| String bundles | `research/evidence/rebased/strings/*.properties` |
| Documentation | `research/evidence/rebased/docs-text/*.md` |
| Runtime capture (R3 session 1: startup git invocations, `idea.log`) | `research/evidence/rebased/runtime/` |
| Tickets | `.scratch/rebased-parity/issues/NN-*.md` |

Rebased **1.1.19** (`MANIFEST.txt`). Documentation pages last modified
17–19 August 2026, so they describe the current UI the installed build came from.

**No sentence in this document or in the eleven evidence files was copied from the
harvested string bundles.** Terminology matches — Changelist, Shelve, Amend, Smart
Checkout, `Already Unshelved`, `Revert conflict resolution` — because terminology
is parity. Wording is ours. Where a bundle string is quoted it is quoted as
*evidence of a label*, in a table cell, for the purpose of establishing a name.

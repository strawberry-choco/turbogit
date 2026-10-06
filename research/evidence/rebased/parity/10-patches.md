# Patches

> Surface `patches`. Filled by the agent pass from installed-bundle evidence plus
> verified documentation. Bundle wins on names/ids/keystrokes; docs win on UX and
> flow. See `research/evidence/rebased/DISTILL.md`.

## Sources

- https://www.jetbrains.com/help/idea/using-patches.html — verified 200, 5909 chars → `docs-text/using-patches.md`
- `actions-labeled.tsv`
- `strings/GitBundle.properties` — the `create.patch.*`, `patch.apply.*`, `show.patch.*` families (GitBundle:132-135, 174-175, 182, 352-374)

Docs resolve 1/1. This is the thinnest documentation page in the set (5909 chars)
and the smallest action set (6), but the bundle compensates: the
`patch.apply.*` family specifies the **apply-conflict and rollback model** in
detail, which the docs barely touch.

## UI — what exists

### Action inventory (bundle)

6 actions harvested from the installed bundle.

| Action id | Label | Key |
| --- | --- | --- |
| `ChangesView.ApplyPatch` | — | — |
| `ChangesView.ApplyPatchFromClipboard` | — | — |
| `ChangesView.CreatePatch` | — | — |
| `ChangesView.CreatePatchFromChanges` | — | — |
| `ChangesView.CreatePatchToClipboard` | — | — |
| `Vcs.ApplySelectedChanges` | — | — |

Labels recovered from the bundle, which turn six anonymous ids into a coherent
vocabulary:

| Action | Bundle label / key |
| --- | --- |
| `ChangesView.CreatePatch` / `.FromChanges` | `Create Patch…` (`action.name.create.patch`, GitBundle:174) |
| — | "Creates Patch for selected revision(s)" (`action.description.create.patch.for.selected.revisions`, GitBundle:175) |
| patch button in the commit flow | `Create Patch` (`create.patch.commit.action.title`, GitBundle:353) |
| apply | `Apply Patch` (`button.apply.patch`, GitBundle:357) |
| patch apply dialog | `Apply Patch` (`patch.apply.dialog.title`, GitBundle:358) |
| patch file chooser | `Select Patch File` (`patch.apply.select.title`, GitBundle:369) |
| file field | `Patch file name:` (`patch.apply.file.name.field`, GitBundle:352) |
| settings dialog | `Patch File Settings` (`create.patch.settings.dialog.title`, GitBundle:356) |

**Create has two destinations and Apply has two sources — four routes, two
dialogs.** `…ToClipboard` and `…FromClipboard` are the clipboard pair, and
`Vcs.ApplySelectedChanges` is the *internal* apply (used by the diff viewer's
`Apply` chevrons, see `11-diff-viewer`), not a user-facing patch verb.

### Topology

**A. The Patch File Settings dialog** (using-patches:16-24) — the same four fields
regardless of whether the patch comes from uncommitted changes, a commit, or a
file revision:

1. **Patch file location** — "choose the default location **or select the `To
   clipboard` option** if you do not want to save the patch to a file" (line 18)
2. **Base path** — "specify the path relative to which paths inside the patch file
   will be written. Normally, this is your project directory, but you may want to
   use a relative path, for example, if the modified files are stored inside a Git
   repository" (line 20)
3. **Reverse patch** — "select this option if you want to create a patch that
   **reverts the changes you have made**" (line 22)
4. **Encoding** — "select the encoding for the patch file from the drop-down list"
   (line 24)

**B. The Apply Patch dialog** (using-patches:58-83) — field order as documented:

1. **Path to the .patch file** (line 58)
2. **`Show Diff` (`⌘D`)** — a *check*, not a view: "To check if your patch can be
   applied without conflicts, click Show Diff `⌘D`. **If there are conflicts, the
   corresponding lines will be highlighted in red**" (line 61)
3. **`Change Directory Paths`** — a sub-dialog with **five** buttons (lines 63-73):
   - `Map Base Directory` — "specify a directory relative to which file names in
     the patch file will be interpreted. You can map the base directory to a
     single file, directory, or selection" (line 65)
   - `Remove Leading Directory` — "removes one slash in the path to the target
     file. Click the button as many times as many leading directories you need to
     remove. **The number of removed slashes is indicated in square brackets**"
     (line 67)
   - `Remove All Leading Directories` (line 69)
   - `Restore Leading Directory` — "revert the last Remove Leading Directory
     action" (line 71)
   - `Restore All Leading Directories` (line 73)
4. **Changelist destination** — "Select the changelist to which you want to apply
   the patch **or specify the name of a new changelist** in the Name field, and
   enter a comment to this changelist (optionally)" (line 75)
5. **Set active** checkbox (line 77)
6. **Track context** checkbox (line 79)
7. **Analyze and apply patch from clipboard on the fly** — clipboard-only:
   "you can select the `Analyze and apply patch from clipboard on the fly` option for
   IntelliJ IDEA to **automatically detect patches in clipboard contents and
   suggest applying them**" (line 81)
8. **`Import to shelf`** — "If you want to move the patch to a temporary storage
   (shelf) before applying it, click Import to shelf" (line 83)
9. Footer: OK

**The `Change Directory Paths` group is the most interesting control on the
surface.** It is a *counting* pair: `Remove Leading Directory` can be pressed N
times, the count is displayed in square brackets, and `Restore` reverses it one step
at a time. **A patch that nearly applies is repaired by counting path segments, not
by editing a path.** The bracket count exists so the user can see the state of a
repeated action.

### State matrix

| State | Which region | What it shows | Source |
| --- | --- | --- | --- |
| default | Patch File Settings | the default save location; base path = the project directory | using-patches:18, 20 |
| default | Apply Patch `Show Diff` | conflict lines **highlighted in red** — a preflight, not a navigation | using-patches:61 |
| default | `Change Directory Paths` | removed-slash count in **square brackets** | using-patches:67 |
| hover | patch row in Shelf | `{0} created on {1} at {2}` (VcsBundle:1292) | VcsBundle:1292 |
| disabled | `Analyze and apply patch from clipboard…` | clipboard-only; absent for a file-based apply | using-patches:81 |
| disabled | `Import to shelf` | for the shelf route only | using-patches:83 |
| focus | patch file field | `Patch file name:` (GitBundle:352) | GitBundle:352 |
| selected | a patch in the Shelf | drives the diff preview; `Select stash or shelf to view files` when none (VcsBundle:1289) | VcsBundle:1289 |
| progress | create | `Creating Patch…` (GitBundle:354); `Loading Patch Base Revision` (182) | GitBundle:182, 354 |
| progress | apply | `Before patch` / `After patch` (GitBundle:370-371) — **two labelled phases** | GitBundle:370-371 |
| error | create | `Error creating patch: {0}` (GitBundle:355) | GitBundle:355 |
| **error: apply conflict** | conflict view, titled per file | `Patch Conflict for {0}` (GitBundle:362) / `Patch Conflict` (363), with **four named panes**: `Local version` (364), `Merge result` (365), `Patched version` (366), `Result` (367), plus `Patch` (368) | GitBundle:362-368 |
| **rollback: clean** | notification | `Apply patch aborted` + "**All files changed during 'Apply Patch' action were rolled back**" (GitBundle:372-373) | GitBundle:372-373 |
| **rollback: failed** | notification | `Rollback failed` (GitBundle:374) | GitBundle:374 |
| success | notification | `Apply patch` (GitBundle:359) | GitBundle:359 |
| success | file | a `Patch` filetype is registered: `filetype.patch.description=Patch`, `filetype.patch.display.name=Patch` (GitBundle:360-361) | GitBundle:360-361 |

**The four conflict pane names are the specification of this surface's hardest
case**, and they are not obvious: `Local version` (what you have), `Merge result`
(what you'd get), `Patched version` (what the patch wants), and `Result` — a fourth
pane that appears only in the `patch.apply.conflict.patched.somehow.version` case
(GitBundle:367), i.e. when the patch was applied in a way that needed adjusting.
**Four panes for a patch conflict, where the merge conflict has three.**

## UX — how it behaves

### How is it reached, and what hides behind a secondary affordance?

**Create — three sources, one dialog:**

| Source | Route | Doc |
| --- | --- | --- |
| Uncommitted changes | Commit tool window → select file or changelist → `Create Patch from Local Changes` | using-patches:12 |
| A whole commit | Log tab → the commit → `Create Patch` | using-patches:29 |
| A file revision | file → `Show History` → right-click a revision → `Create Patch` | using-patches:41-47 |

Plus the clipboard shortcut: "right-click the necessary file(s) in the Commit tool
window and choose `Copy as Patch to Clipboard`" (using-patches:25) — **which skips
the Patch File Settings dialog entirely** (line 25 describes it as the alternative
to the dialog's `To clipboard` option).

**Apply — two sources:**

| Source | Route | Doc |
| --- | --- | --- |
| A saved file | `Git | Patch | Apply patch` | using-patches:53 |
| The clipboard | `Git | Patch | Apply patch from Clipboard` — "convenient when you receive a patch by email and **do not want to save it**" | using-patches:55 |
| Automatically | `Analyze and apply patch from clipboard on the fly` | using-patches:81 |
| Drag-and-drop | "You can **drag a file or an email attachment to any place in the editor**" | using-patches:59 |

Plus the **shelf route**: "In the Shelf view, choose `Import Patches from the
context menu` … The selected patch appears in the Shelf tab as a shelf" (lines
214-216) — bundle `action.ImportIntoShelfAction.text=Import Patches…`,
description "Copies a patch file to the shelf".

And `Apply Patch` from within the Apply dialog: `Import to shelf` "If you want to
move the patch to a temporary storage (shelf) before applying it" (line 83).

**So a patch has four possible homes**: a file, the clipboard, a shelf, and (via
`To clipboard`) the clipboard. And the shelf is a *first-class* destination, not an
afterthought.

Hidden behind secondary affordances:

- **`Change Directory Paths` is a sub-dialog** behind one button (lines 63-73).
- **`Analyze and apply … on the fly`** is a checkbox, and it is the only route with
  no user gesture at all (line 81).
- **The `Before patch` content is fetched lazily** — `Loading Patch Base Revision`
  (GitBundle:182) — so the apply dialog has an async load with no visible
  placeholder specified.
- **`Show Diff` is a preflight check**, hidden in a button rather than run
  automatically (line 61).

### Which verbs are destructive, and what do they confirm or preflight?

| Verb | Destructive | Confirmation | Evidence |
| --- | --- | --- | --- |
| Create Patch | no | none | — |
| Create Patch (clipboard) | no | **none, and no dialog** — it is the deliberate fast path | using-patches:25 |
| **Apply Patch** | **yes** — writes files | **Preflight, not confirmation**: `Show Diff` with conflicts highlighted in red (line 61). The user inspects, then commits | using-patches:61 |
| Apply Patch (on the fly) | **yes** | **A suggestion, not a confirmation** — "automatically detect patches in clipboard contents and **suggest** applying them" (line 81) | line 81 |
| **`Import to shelf`** | no — it *avoids* touching files | none; it is the cautious route | line 83 |
| Path rewriting (`Remove/Restore Leading Directory`) | no | none; a repair control | lines 67-73 |
| Reverse patch creation | no — but it inverts the patch's meaning | the checkbox is the disclosure (line 22) | line 22 |

**The design pattern is a preflight instead of a confirmation, and it is
consistent.** `Show Diff` before applying is the same idea as the Push dialog's
`Preview dry-run` or the Rewrite Preflight: *show what will happen, then let the
user commit*. The patch case is the purest instance — the preflight is literally a
diff, and the conflict signal is colour ("the corresponding lines will be
highlighted in red", line 61).

**`Import to shelf` is the conservative escape**, and it is worth copying as a
first-class option rather than a checkbox: applying a patch you are not sure about
is a *deferral*, not a yes/no.

**Not confirmed at all**: applying a patch with conflicts, once the user has
proceeded. The failure path is handled by rollback, not by a gate.

### What can be undone, from where, and for how long?

**This is where the bundle is decisive, and it is the strongest finding on the
surface.**

| Situation | Undo | Evidence |
| --- | --- | --- |
| **Apply patch fails or is aborted** | **Full automatic rollback**: "All files changed during 'Apply Patch' action were rolled back" | `patch.apply.aborted.message` (GitBundle:373) |
| Rollback itself fails | **`Rollback failed`** — an explicit, named terminal state | GitBundle:374 |
| Applied patch (success) | **Not undoable as a patch.** The changes land as *local uncommitted changes* — "Select the changelist to which you want to apply the patch" (line 75) — so the patch's recovery is the **Commit tool window's** Revert, not a patch-level undo | using-patches:75 |
| Applied to a shelf first | the shelf copy remains; the patch can be re-applied "as many times as you need" | shelving:33 |
| Create Patch | nothing to undo — it only reads | — |

**The rollback is atomic and total, and that is the point.** GitBundle:373 says
*all* files changed during the action were rolled back — so a failed apply leaves
the working tree exactly as it was. The existence of a separate `Rollback failed`
string proves the authors considered the case where the guarantee does not hold,
and named it rather than leaving it undefined. **A client must have both strings.**

And note the pairing with the *stash* warning: `stash.unstash.unresolved.conflict.warning.notification.message`
says "Resolve conflicts and **drop the stash**" (GitBundle:447) — because stashing
into a conflicted index loses the safe copy. **Patch apply has no such warning,
because it has a real rollback.** The two mechanisms differ precisely in whether
the original is recoverable.

### Which options are on by default, and what is remembered between uses?

| Option | Default | Evidence |
| --- | --- | --- |
| Patch file location | the default location (not the clipboard) | using-patches:18 |
| Base path | "Normally, this is your project directory" | using-patches:20 |
| Reverse patch | **off** | using-patches:22 |
| Encoding | a drop-down; default **UNVERIFIED**. Re-tested against using-patches/settings-tools-diff-and-merge/advanced-settings 2026-10-06: all three name or omit the drop-down without stating a default → needs runtime capture (R3, D7) | using-patches:24 |
| Changelist destination | the current/active one; a new name may be typed instead | using-patches:75 |
| Set active | off (using-patches:77) | using-patches:77 |
| Track context | off (using-patches:79) | using-patches:79 |
| Analyze clipboard on the fly | **off** — it is a checkbox you select (line 81) | using-patches:81 |
| `Change Directory Paths` | untouched; no slashes removed | using-patches:67 |
| `When patch is created` | `Ask` / `Show in {0}` / `Do nothing` — a three-way policy, with **`Ask` as the first-named value** (GitBundle:132-135) | GitBundle:132-135 |

**`show.patch.in.explorer.after.creation` is a three-way policy with the
safe/asking option first** — the same "ask / do it silently / do nothing" shape as
changelist removal and file deletion (see `01-commit-tool-window`). **This is a
product-wide convention, not a one-off.** Worth adopting as a standard.

Remembered between uses:

- **Nothing about a patch is remembered.** Base path, encoding, and reverse are all
  per-dialog. The patch is a file; the dialog is a one-shot.
- The one persisted thing adjacent to patches is the **shelf location** (see
  `07-shelve-stash`), because `Import to shelf` and `Import Patches` both land
  there.
- `Import Patches…` reuses the shelf's existing persistence, so a patch-as-shelf
  inherits the shelf's lifetime rather than having its own.

### What feedback appears, and where does focus land?

Progress:

- `Creating Patch…` (GitBundle:354)
- `Loading Patch Base Revision` (GitBundle:182) — the lazy "before" fetch
- `Before patch` / `After patch` (GitBundle:370-371) — **two labelled phases**,
  matching the two-sided nature of the operation

Success / error:

- `Apply patch` as a notification title (GitBundle:359)
- `Error creating patch: {0}` (GitBundle:355)
- `Patch Conflict for {0}` / `Patch Conflict` with the four-pane view
  (GitBundle:362-368)
- `Apply patch aborted` + the rollback statement (GitBundle:372-373)
- `Rollback failed` (GitBundle:374)
- `Could not add remote`-style composition is absent here; instead the create
  error is `Error creating patch: {0}` — a single-cause template, because patch
  creation is single-root by nature

**Optimistic paint: no**, and specifically the opposite: on failure the app *rolls
back*, so the paint returns to the pre-action state rather than showing a
half-applied result. **Rollback is the feedback.**

Focus after the action:

- `Copy as Patch to Clipboard` gives no confirmation dialog (line 25), so focus
  stays wherever it was. **UNVERIFIED** whether a toast confirms the copy.
  Re-tested against using-patches 2026-10-06: the page names no feedback for the
  copy → needs runtime capture (R3).
- After a successful file apply, the changes are in the Commit tool window, so
  focus would move there — **UNVERIFIED**, but it follows from the destination being
  a changelist (line 75). Re-tested against using-patches/managing-changelists
  2026-10-06: both name the destination changelist, neither names a focus target →
  needs runtime capture (R3).
- After `Import Patches…`, the new shelf appears in the Shelf tab (line 216) and
  the user must then `Unshelve Changes` from it (line 218) — **two steps, and the
  second is not automatic.**

## Flow — how tasks chain

### Entry points

**Create:**
1. Commit tool window → file or changelist → `Create Patch from Local Changes`
   (using-patches:12) → `ChangesView.CreatePatchFromChanges`.
2. Commit tool window → files → `Copy as Patch to Clipboard` (line 25) →
   `ChangesView.CreatePatchToClipboard`. **No dialog.**
3. Log tab → a commit → `Create Patch` (line 29) → `ChangesView.CreatePatch`.
4. Any file → `Show History` → a revision → `Create Patch` (lines 41-47) →
   `ChangesView.CreatePatch`.

**Apply:**
1. `Git | Patch | Apply patch…` (line 53) → `ChangesView.ApplyPatch`.
2. `Git | Patch | Apply patch from Clipboard` (line 55) →
   `ChangesView.ApplyPatchFromClipboard`.
3. `Analyze and apply patch from clipboard on the fly` (line 81) — no menu.
4. Drag a file or **an email attachment** into the editor (line 59).
5. Shelf view → `Import Patches…` (line 214) → then `Unshelve Changes` (line 218).

`Vcs.ApplySelectedChanges` is the internal apply used by the diff viewer's chevron
(see `11-diff-viewer`), not a patch-file verb.

### Internal sequence, with dialog chaining

**Create a patch from uncommitted changes:**

```
Commit tool window → select a file or changelist → Create Patch from Local Changes
  → [dialog] review the file selection; "enter a commit comment
             (it will be used as the patch file name)"                    (using-patches:14)
  → Create Patch
  → Patch File Settings dialog                                          [chained dialog]
        location  (or To clipboard)
        base path
        [ ] Reverse patch
        encoding
  → Creating Patch…  →  "When patch is created: Ask | Show in {0} | Do nothing"
```

**The commit comment becomes the filename** — that is the one piece of field-order
information the docs give for the first dialog, and it is load-bearing: the user
names the patch by typing a comment.

**Apply a patch file, the full chain:**

```
Git | Patch | Apply patch…
  → Apply Patch dialog                                                  [dialog]
      Patch file name:  {path}
      [Show Diff ⌘D]  → preflight: conflicting lines highlighted in red
      [Change Directory Paths]                                          [chained dialog]
            Map Base Directory
            Remove Leading Directory      ×N, count in [brackets]
            Remove All Leading Directories
            Restore Leading Directory     ×N (reverses one at a time)
            Restore All Leading Directories
      Name: {existing changelist | new name}
      Comment: (optional)
      [ ] Set active
      [ ] Track context
      [ ] Import to shelf      ← defers instead of applying
  → OK
  → Before patch  →  After patch
      → (conflict) Patch Conflict for {file}
            Local version | Merge result | Patched version [| Result] | Patch
      → (ok) the changes land in the named changelist, ready to commit
```

**Apply from the clipboard — the email path:**

```
receive a patch by email
  → copy it (never saved to a file)
  → Git | Patch | Apply patch from Clipboard
  → the Apply Patch dialog opens with the content already in place
  → "For Git format patches, IntelliJ IDEA extracts the commit message and the
     author and automatically fills the corresponding fields in the Commit tool
     window"                                                           (using-patches:56)
  → OK → the changelist is pre-filled with the patch's own message and author
```

**Two facts here that only the doc states:** clipboard patches skip the file path
entirely, and **a git-format patch's metadata pre-fills the destination
changelist**. The patch carries an author and a message, and they are honoured
rather than discarded.

**Clipboard detection — the zero-gesture route:**

```
copy a patch to the clipboard
  → (with the setting on) IntelliJ IDEA detects it and *suggests* applying it
  → accept → the Apply Patch dialog, pre-filled
```

(using-patches:81.)

**Patch → shelf → work (the deferred route):**

```
Shelf view → Import Patches…
  → select the patch file
  → "The selected patch appears in the Shelf tab as a shelf"
  → select the new shelf → Unshelve Changes
  → the Unshelve dialog opens (see 07-shelve-stash)
  → OK
```

(using-patches:214-218.) **Three surfaces for one patch**: the file system, the
shelf, and the changelist.

### What happens next on success

- **Create**: the patch file is written (or the clipboard filled), and the
  `When patch is created` policy fires — `Ask` / `Show in {0}` / `Do nothing`
  (GitBundle:132-135).
- **Apply**: the changes are **uncommitted local changes in a changelist**
  (using-patches:75). Nothing is committed. The next step is the user's.
- **Apply from clipboard**: the changelist additionally carries the patch's author
  and message (line 56).
- **Import to shelf**: nothing is applied; the shelf row appears and the user
  unshelves deliberately (line 218).

### What happens next on failure, and how the user recovers

| Failure | Evidence | Recovery |
| --- | --- | --- |
| **Apply failed / aborted** | `Apply patch aborted` + "**All files changed during 'Apply Patch' action were rolled back**" (GitBundle:372-373) | the working tree is already restored; retry or abandon |
| **Rollback failed** | `Rollback failed` (GitBundle:374) | **UNVERIFIED** — no bundled next step. This is the one genuinely dangerous state on the surface and it has no documented recovery. Re-tested against using-patches 2026-10-06: the page does not describe the rollback at all (it is bundle-only) → needs runtime capture (R3) |
| **Patch conflict** | `Patch Conflict for {0}` with the four panes (GitBundle:362-368) | resolve in the conflict view. **Closed 2026-10-06** (docs: resolve-conflicts): patch conflicts are funnelled through the standard Conflicts dialog — "conflicts may arise when you attempt to perform one of the following operations: pull, merge, rebase, cherry-pick, unstash changes or apply a patch. If there are conflicts, these operations will fail, and you will be prompted to accept the upstream version, prefer your version, or merge the changes" — i.e. per-file first, then per-conflict-chunk in the manual tool (accept/ignore per side, `Apply Changes` / `Save and Close` per file). The four-pane `Patch Conflict` pane naming remains bundle-only and is not named in the page |
| **Create failed** | `Error creating patch: {0}` (GitBundle:355) | **UNVERIFIED** — no bundled recovery. Re-tested against using-patches 2026-10-06: the page documents no create-failure state → needs runtime capture (R3) |
| **Paths do not match** | `Show Diff` highlights conflicting lines in red (using-patches:61) | `Change Directory Paths` → `Remove Leading Directory` ×N (the bracket count shows the state) |
| Patch file found outside the content root | `File to patch found outside content root: {0}` (GitBundle:378) | **UNVERIFIED** — re-tested against using-patches 2026-10-06: the page covers path repair only via `Change Directory Paths`, not this failure → needs runtime capture (R3) |
| Patch clipboard is not a patch | — | **UNVERIFIED** — no empty/invalid-clipboard state is specified for the on-the-fly detector, which is a real gap given it runs unprompted. Re-tested against using-patches 2026-10-06: the page states only the detect-and-suggest behaviour (line 81) → needs runtime capture (R3) |
| Detected a Git-format patch | author + message auto-filled (using-patches:56) | nothing to recover |

### Escape / dismiss semantics

- **`Import to shelf` is the non-destructive exit** from the Apply dialog (line 83)
  — the operation becomes a shelf you can apply later or not at all.
- **`Restore Leading Directory` reverses one step at a time** (line 71), and
  `Restore All Leading Directories` reverses them all (line 73) — so path repair is
  fully reversible *within the dialog*, which is what makes experimenting safe.
- **Abort rolls back silently and completely** (GitBundle:373) — there is no
  "partially applied" state to clean up by hand, which is the strongest escape
  semantics in the harvest.
- **`Show Diff` is optional** (line 61), so the user *can* apply blind. That is a
  deliberate omission of a gate, and the rollback is what makes it safe.
- `Copy as Patch to Clipboard` has no dialog and therefore no escape at all
  (line 25) — harmless, since it only reads.

## Divergences (intentional)

| Dimension | Rebased behaviour | TurboGit behaviour | Why |
| --- | --- | --- | --- |
| UI | **Four patch homes** — file, clipboard, shelf, and a lazy clipboard detector (using-patches:18, 25, 55, 81, 214) | `ChangesView.CreatePatch*` / `ApplyPatch*` action ids exist in `actions-labeled.tsv` but **no TurboGit patch surface was found** in `crates/turbogit-ui/src/ui/` | **The largest single gap in the whole parity set.** Not one patch verb is evidenced as implemented. The full spec is above and the scope is modest. See ticket 18. |
| UX | Apply has an **atomic total rollback** — "All files changed during 'Apply Patch' action were rolled back" (GitBundle:373) — plus a named `Rollback failed` terminal state (374) | Not established | **Open gap, and it is the highest-value item in this surface.** A patch apply touches many files; a partial failure that leaves the tree half-patched is the worst outcome in the product. `git apply` is atomic for a whole patch, so the mechanism is cheap — but the *named* rollback-failure state is what makes the guarantee honest. |
| UX | **Preflight instead of confirmation** — `Show Diff` with conflicting lines in red, then commit (using-patches:61) | Not established (no surface) | Copy the pattern: a diff-based preflight, not a yes/no dialog. It matches TurboGit's existing Push dry-run and Rewrite Preflight idiom, so this is a consistency win as well as a safety one. |
| UX | `Import to shelf` — **deferral as a first-class option** in the apply dialog (using-patches:83), plus `Import Patches…` in the Shelf view (line 214) | Not established | **Open gap.** "I'm not sure yet" needs a destination, and the shelf already exists in TurboGit's model (`CommitSubTab::Shelf`). Cheap to add once patches exist. |
| UX | Path repair is a **counting pair** — `Remove Leading Directory` ×N with the count in square brackets, plus per-step `Restore` (using-patches:67-73) | Not established | **Open gap, small but distinctive.** A patch that nearly applies is the common case, and counting segments is a better interaction than typing a corrected prefix. The bracket count is what makes a repeated action legible. |
| UX | A git-format patch's **author and message auto-fill the destination changelist** (using-patches:56) | Not established | **Open gap, small and correct.** The patch already carries the metadata; discarding it loses provenance. Pairs with the `cherry-picked from <hash>` suffix setting (GitBundle:793) — same intent. |
| UX | `When patch is created:` is a three-way policy — `Ask` / `Show in {0}` / `Do nothing` (GitBundle:132-135) | Not established | **Open gap, and it reveals a product convention**: the ask/silent/nothing triple appears for changelist removal, file deletion, and patch creation. TurboGit's `PendingConfirm` enum is the right shape for it; adopting the triple consistently is a small, high-value change. |
| UX | `Analyze and apply patch from clipboard on the fly` — an **unprompted detector** that suggests rather than acts (using-patches:81) | Not established | **Deliberately cautious, and worth flagging as risky to copy.** A clipboard watcher that acts without a gesture is a privacy-adjacent surprise, and no invalid-clipboard state is specified. If TurboGit implements it, it must be suggestion-only with a clear dismissal — the doc's word is "suggest", and that word is load-bearing. |
| UI | Patch is a **registered filetype** — `filetype.patch.description=Patch` (GitBundle:360-361) | Not applicable in the same way | egui has no filetype registry, but it does mean patch files can be *viewed and diffed* as a first-class type. Recorded so the capability is not lost. |
| UX | Reverse patch inverts a patch's meaning via one checkbox (using-patches:22) | Not established | **Open gap, trivial.** One checkbox, and it is the cheapest way to produce a revert-as-patch. |
| UX | Drag-and-drop of a file **or an email attachment** into the editor (using-patches:59) | No drag-and-drop target | **Intentionally absent.** egui file drops are possible, but "drop an email attachment" is an IDE-integration behaviour with no TurboGit equivalent and no clear home. Recorded as a deliberate omission, not an oversight. |

## Needs runtime capture

1. **The first dialog's field list** — the doc only says "make sure that all the
   changes you want to include in the patch are selected, enter a commit comment"
   (line 14). Whether it has more fields is unknown.
2. **Default encoding** in the Patch File Settings dialog (using-patches:24).
3. **The `Patch Conflict` view's interaction model** — resolution granularity is
   now answered by the docs (per-file three-option prompt, then
   per-conflict-chunk manual resolution, docs: resolve-conflicts), but the bundle
   names four panes (GitBundle:364-367) where the standard tool has three, and
   what the fourth `Result` pane means is still unclear.
4. **Recovery for `Rollback failed` (GitBundle:374)** — the most dangerous state on
   the surface has no documented route. **Highest-value capture in this surface.**
5. **What the on-the-fly clipboard detector does with non-patch clipboard content**
   — it runs unprompted, and no empty/invalid state is specified.
6. **Whether `Show Diff` is available before the patch file is chosen** or only
   after — the field order suggests after, but a pre-check would be more useful.
7. **The `When patch is created` default** — `Ask` is listed first among the three
   values, which is suggestive but not proof.

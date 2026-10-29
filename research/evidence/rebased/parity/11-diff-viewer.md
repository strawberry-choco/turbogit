# Diff viewer

> Surface `diff-viewer`. Filled by the agent pass from installed-bundle evidence plus
> verified documentation. Bundle wins on names/ids/keystrokes; docs win on UX and
> flow. See `research/evidence/rebased/DISTILL.md`.

## Sources

- https://www.jetbrains.com/help/idea/differences-viewer.html, verified 200, 9722 chars → `docs-text/differences-viewer.md`
- ~~`compare-file-with-diff` → HTTP 404~~, **RESOLVED 2026-10-05: renamed, not dead.** The content is now https://www.jetbrains.com/help/idea/comparing-files-and-folders.html. "Compare files, folders, and text sources", verified 200, last modified 03 September 2026. Now harvested: `docs-text/comparing-files-and-folders.md` (fetched 2026-10-05). It cross-links `differences-viewer`, `comparing-file-versions`, and `differences-viewer-for-folders`; its own "How it works" footer carries `Read-only files` and `File type associations` (both checked 2026-10-06, see the state-matrix note below).
- `actions-labeled.tsv`, `keymap-bindings.txt`
- Cross-read: `docs-text/commit-and-push-changes.md:155-189` (the Diff Viewer's
  staging role), `docs-text/resolve-conflicts.md` (the three-pane merge tool this
  surface extends), `docs-text/shelving-and-unshelving-changes.md:78-168` (the
  full option list, which is the *same* list)

**The documentation gap that this file originally carried is closed.** The harvest
reported "1 / 2" pages for this surface because it looked for
`compare-file-with-diff`; that slug was renamed, so the surface is now fully
evidenced at 2/2 by content rather than by slug. What the recovered page adds:

1. **Three-file comparison, and it is user-reachable.** The heading is "Compare two
   **or three** files", and the route is the context-menu action `Switch to
   Three-Side Viewer`, "available only when you are comparing two files against
   each other. It allows you to load a third file and add it to the comparison."
   **This answers the open question below about whether the `Diff.ComparePartial.*`
   triple is internal-only: it is not**, that triple is the machinery behind this
   menu item.
2. **The entry-point table lives on this page, not the other one.** `differences-viewer`
   carries it: *Project tool window | context menu of a file | `Compare File with
   Editor`*; *Project | `Compare Files`*; *Commit tool window | `⌘D`*; *Git tool
   window | context menu of a folder or file | `Show Diff`*. Four routes, two of
   which (`Compare File with Editor`, `Compare Files`) are **project-tree** verbs
   this client has no counterpart for.
3. **Swap sides is a named toolbar control** (`Swap Sides` button), documented for
   both the file case and the text-source case; the three-file case reuses it.
4. **`Open Blank Diff Window`** via `⌃⇧A` Find Action, paste or drag arbitrary
   text into both panes.
5. **External diff tools are configurable**: `Tools | Diff & Merge | External Diff
   Tools`, an `Enable external tools` checkbox, a tool table (tool group / program
   path / argument pattern where `%1` = local changes, `%2` = content from the
   remote server, `%3` = current version without local changes), and `Test Diff` /
   `Test Three-Side Diff` verification buttons. Launchable from the viewer by one
   icon, this is the concrete content behind `Diff.ShowInExternalTool`, which the
   harvested docs left as a bare action id.

**Not pursued, deliberately:** `command-line-differences-viewer` and
`command-line-merge-tool` (terminal integration), and
`comparing-deployed-files-and-folders-with-their-local-versions` (deployment
model) are IDEA inheritance, excluded by `DISTILL.md` rule 5.
`differences-viewer-for-folders` is a folder-compare surface with no TurboGit
counterpart; noted, not specified.

## UI, what exists

### Action inventory (bundle)

**42 actions, the largest action set of any surface.** That number is itself the
finding: the Diff Viewer is not a viewer, it is a *workbench* with three
distinct jobs.

| Action id | Key | Job |
| --- | --- | --- |
| `Diff.ShowDiff` | `⌘D` | open a diff |
| `Diff.ShowStandaloneDiff` | — | open standalone |
| `Diff.OpenInEditor` | — | open the file for editing |
| `Diff.OpenDiffInEditor` | — | open the diff in the editor |
| `Vcs.ShowDiffWithLocal` | — | compare a revision with the working tree |
| `Vcs.ShowDiffWithLocal.Before` | — | compare with the *previous* local revision |
| `Git.Ref.Diff.With.Local` | — | same, from the ref menu |
| `Diff.NextChange` | `⌥⇧→` | next difference |
| `Diff.PrevChange` | `⌥⇧←` | previous difference |
| `Diff.FocusOppositePane` | `⌃⇧⇥` | switch panes |
| `Diff.FocusOppositePaneAndScroll` | — | switch panes *and* scroll |
| `Diff.ApplyLeftSide` | `⌘⌥R` | apply the left side |
| `Diff.ApplyRightSide` | `⌘⌥A` | apply the right side |
| `Diff.AppendLeftSide` | — | append the left side |
| `Diff.AppendRightSide` | — | append the right side |
| `Diff.IgnoreLeftSide` | — | ignore the left side |
| `Diff.IgnoreRightSide` | — | ignore the right side |
| `Diff.ApplyNonConflicts` | — | bulk apply |
| `Diff.ApplyNonConflicts.Left` | — | bulk apply, one side |
| `Diff.ApplyNonConflicts.Right` | — | bulk apply, other side |
| `Diff.MagicResolveConflicts` | — | one-click simple-conflict resolve |
| `Diff.ResolveConflict` | — | resolve one conflict |
| `Diff.NextConflict` / `Diff.PreviousConflict` | — | walk conflicts |
| `Diff.CompareWithBase.Left` / `.Right` / `.Result` | — | the three "compare with base" variants |
| `Diff.ComparePartial.Base.Left` / `.Base.Right` / `.Left.Right` | — | the three "compare partial" variants |
| `Diff.ShowSettingsPopup` | `⌃⇧D` | the settings menu |
| `Diff.ShowInExternalTool` | — | open externally |
| `Vcs.Diff.ToggleDiffAligningMode` | — | align changes side-by-side |
| `Vcs.Diff.ToggleSearchInChanges` | — | search within the diff |
| `Vcs.Diff.ShowDiffInEditorTab` / `ShowDiffInNewWindow` | — | two hosting modes |
| `Vcs.Diff.ShowCombinedDiff` / `EnableDisableCombinedDiff` | — | combined-diff mode |
| `Vcs.Diff.IncludeChangedLinesIntoCommit` | — | stage one line |
| `Vcs.Diff.IncludeWholeChangedLinesIntoCommit` | — | stage one changed line |
| `Vcs.Diff.ExcludeChangedLinesFromCommit` | — | exclude one line |
| `Vcs.Diff.ResolveConflictsInImports` | — | IDE-internal; **excluded** per `DISTILL.md` rule 5 |

**The `CompareWithBase.{Left|Right|Result}` and `ComparePartial.{Base.Left|Base.Right|Left.Right}`
triples are the structural signature.** Six actions forming two symmetric sets of
three, and the third member of each is the *middle*: the Result (for base) and
Left↔Right (for partial). The docs explain the semantics: "**Note that `Base`
refers to the file version that the local and the repository versions originated
from (initially displayed in the middle pane), while `Middle` refers to the
resulting version**" (differences-viewer.md:123; resolve-conflicts.md:50). So there
are three panes and three pairwise comparisons among them, and the naming scheme is
positional.

### Topology

**Two viewer modes**, documented as a single control (shelving:101-102,
differences-viewer.md:52-53): "The side-by-side mode has two panels, and the
unified mode has one panel."

**Two hosting modes**: editor tab or separate window, both
`Vcs.Diff.ShowDiffInEditorTab` / `ShowDiffInNewWindow`, and switchable from
`Settings` on the toolbar (differences-viewer.md:63-64). The Commit window's
double-click routing is a third, separate setting
(commit-and-push-changes.md:157-162).

**The toolbar** (differences-viewer.md:30-141, and identically shelving:80-168).
Note that `shelving` reproduces the *same item table*, which is how the option list
below is known to be complete rather than sampled:

| Control | Key | Notes |
| --- | --- | --- |
| Previous / Next Difference | `⇧F7` / `F7` | "When the last or the first difference is reached, IntelliJ IDEA **suggests** clicking the arrow buttons or pressing F7 / Shift+F7 once more and comparing other files modified locally" (differences-viewer.md:34) |
| Compare Previous/Next File | `⌥←` / `⌥→` | "only available if more than one file has been modified locally" (line 41) |
| Jump to Source | `F4` | "The caret is placed in the same position as in the Diff Viewer" (line 46) |
| Collapse Unchanged Fragments | — | "The number of non-collapsible unchanged lines is configurable on the Diff & Merge settings page" (line 50) |
| Side-by-side / Unified | — | the mode switch |
| Settings | `⌃⇧D` | the menu below |
| Help | `F1` | — |
| — | `⌃⇧⇥` | "Switch between the panes of the Diff Viewer. **The active pane has the caret**" (line 107) |
| Accept | — | "Apply differences between panes (in case of the side-by-side viewer) or between lines (in case of the unified viewer)" (line 111) |
| Compare Contents | — | "invoke the list of options allowing you to compare different versions of a file to resolve a conflict" (line 122) |
| Apply All Non-Conflicting Changes | — | line 127 |
| Apply Non-Conflicting Changes from the Left/Right Side | — | line 131 |
| Resolve Simple Conflicts | — | line 135 |
| Annotate with GitBlame | — | **context menu only** (line 139) |

**The Settings menu** (`⌃⇧D`), the complete option list, and the best-evidenced
part of this surface (differences-viewer.md:59-99):

- `Align Changes in Side-by-Side Diff`, "vertically aligns the corresponding lines
  by padding differences with empty space" (line 61)
- `Synchronize Scrolling`, "If this button is released, each pane can be scrolled
  independently" (line 62), **a toggle, and releasing it is meaningful**
- `Show Diff in Editor Tab` / `Show Diff in Separate Window` (63-64)
- `Ignore Differences`, a **five-value** whitespace policy (65-86):
  - `None`, "white spaces are important, and all the differences are
    highlighted. **This option is selected by default**" (67)
  - `Trim whitespaces`, trims leading/trailing `"\t", " "`; lines differing only in
    trailing whitespace "are considered equal" (69-72)
  - `Ignore whitespaces`, "not important, regardless of their location" (75)
  - `Ignore whitespaces and empty lines`, **plus three sub-rules**: all
    whitespaces; all added/removed lines consisting of whitespace only; "all
    changes that consist of splitting or joining lines without changes to
    non-whitespace parts" (79-84)
  - `Ignore imports and formatting`, "whitespaces within String literals are
    respected though" (86)
- `Highlighting Differences`, a **five-value** granularity (87-99):
  `Words` / `Lines` / `Split changes` / `Characters` / `None`, with a stated
  rationale for `None`: "**Use the None option when you work with the files that
  were significantly modified. In such cases, highlighting might introduce
    additional difficulties during a review**" (98-99)
- `Split changes` has a worked example: "A \n B and A X \n B X are treated as two
  changes instead of one" (92-93)

**Two editing constraints, stated as rules** (differences-viewer.md:54-56):

- "You can edit code and perform the `Accept`, `Append`, `Revert` actions in **both
  viewers**", the same three verbs in side-by-side and unified
- "You can change text only in the **right part** of the side-by-side viewer or in
  the **lower line** of the unified viewer"
- "You can edit only **local** versions of your files. You cannot edit files that
  have read-only status"

**The three-colour change grammar** (differences-viewer.md:12-22): Green = Added,
Blue = Modified, Gray = Deleted. Three categories, and, this matters, the same
three are reused as the *file status* grammar in the Changes window when comparing
branches: "Files that exist in the selected branch and are missing in the current
branch are marked with **gray**. Files that exist in the current branch but are
missing in the selected branch are marked with **green**. Files that contain
differences … are marked with **blue**" (manage-branches.md:131-135;
apply-changes:278-282). **One grammar, two scopes.**

### State matrix

| State | Which region | What it shows | Source |
| --- | --- | --- | --- |
| default | viewer | side-by-side or unified per the mode control; `Ignore Differences` = `None`; highlighting per the granularity control | differences-viewer.md:52, 67 |
| default | edits | only the right part / lower line is editable; local files only | lines 54-56 |
| **pressed** | the accept chevron | **the chevron's meaning changes under a modifier**: "Click and to apply changes. **This behavior is the default one.** Press `Ctrl` to change or to or and append changes" (lines 114-116) | lines 114-116 |
| hover | a change | the accept/append pair appears for that hunk | implied by lines 110-117 |
| disabled | Compare Previous/Next File | "only available if more than one file has been modified locally" (line 41) | line 41 |
| disabled | `Resolve Simple Conflicts` | "**There are no simple conflicts to resolve**", the tooltip *is* the reason (VcsBundle:588) | VcsBundle:588 |
| disabled | `Annotate with GitBlame` | "only available from the context menu of the gutter" (line 139) | line 139 |
| focus | a pane | `⌃⇧⇥` moves the caret between panes; "The active pane has the caret" (line 107) | line 107 |
| focus | a pane | `F4` places the caret in the editor "in the same position as in the Diff Viewer" (line 46) | line 46 |
| selected | a change | `Accept` applies "differences between panes … or between lines" depending on mode (line 111) | line 111 |
| progress | — | **no progress state is documented for the Diff Viewer itself.** Reads are async elsewhere in the product | — |
| error | binary file | "most options listed above are available for **text files only**. Binary files have a limited number of diff options" (shelving:167) | shelving:167 |
| error | unreadable | **UNVERIFIED** — re-tested 2026-10-06 against `differences-viewer`, `comparing-files-and-folders` and that page's footer sibling `read-only-files` (all on-topic, all silent on an unreadable file *in the viewer*; `read-only-files` covers only the status-bar closed-lock marker) → needs runtime capture (R3) | — |

**The `Ctrl`-to-append chevron is the single most valuable state on this surface**
(lines 114-116). A single control does two different things depending on a
modifier, and the docs spell out both. It is the keyboard equivalent of "the
obvious thing" versus "the careful thing", available without a menu.

## UX, how it behaves

### How is it reached, and what hides behind a secondary affordance?

**`⌘D` is the universal entry**, `Diff.ShowDiff` at `⌘D` in
`keymap-bindings.txt`, and it is the same keystroke on every surface: the Commit
window row (commit-and-push-changes.md:105), the Log's Changed Files pane
(log-tab.md:354), a stash row (shelving:270), a shelved change (shelving:48), the
Apply Patch dialog (using-patches:61), the History tab (investigate:60). **One
keystroke, six surfaces.** That is the strongest single argument that the Diff
Viewer is one surface rather than six.

Other entries: `Vcs.ShowDiffWithLocal` (revision vs working tree),
`Vcs.ShowDiffWithLocal.Before` (vs the *previous* local revision, a distinct
comparison, not a repeat), `Diff.OpenInEditor`, `Diff.ShowInExternalTool`, and
`Vcs.ShowDiffInEditorTab` / `ShowDiffInNewWindow`.

Hidden behind secondary affordances:

- **Everything.** Nine toolbar controls plus a Settings menu with two five-value
  policies. `⌃⇧D` is the only way to reach the option set, and `⌘⌥R` / `⌘⌥A` the
  only way to reach apply-left/right without the mouse.
- **`Accept` vs `Append` vs `Revert`** are the chevron plus a modifier
  (lines 114-116), one control, three outcomes.
- **The "which three versions?" question** hides behind `Compare Contents`
  (line 122) and behind the `CompareWithBase.*` / `ComparePartial.*` triples.
- **`Synchronize Scrolling` is a hold-to-release control**: "If this button is
  released, each pane can be scrolled independently" (line 62), a press-and-hold,
  not a checkbox.
- **Blame is context-menu only** (line 139), deliberately not a toolbar button.

### Which verbs are destructive, and what do they confirm or preflight?

**The Diff Viewer is where destructive operations look harmless, and it handles
that with previews rather than confirmations.**

| Verb | Destructive | Confirmation | Evidence |
| --- | --- | --- | --- |
| `Accept` / apply a side | **yes**, writes the file | **none**; but the *effect is visible before you commit to it*, because the pane updates as you accept | lines 110-117 |
| `Append` (the modified chevron) | **yes**, and differently, adds rather than replaces | the modifier **is** the disclosure | lines 114-116 |
| `Revert` (in the viewer) | **yes**, discards | listed as a first-class viewer action (line 54) | line 54 |
| `Ignore Left/Right Side` | **yes**, discards that side's change | **none documented** | `Diff.IgnoreLeftSide` / `.Right` |
| `Apply All Non-Conflicting Changes` | **yes, in bulk** | **none**, one click | line 127 |
| `Resolve Simple Conflicts` | **yes** | **none** | line 135 |
| `Include Changed Lines Into Commit` | no, stages, does not commit | none | `Vcs.Diff.IncludeChangedLinesIntoCommit` |
| `Exclude Changed Lines From Commit` | no, leaves pending | none | `Vcs.Diff.ExcludeChangedLinesFromCommit` |
| `Compare with Clipboard` | no | none | differences-viewer.md:196 |
| `Open Repository Version` "for editing" | **yes, potentially**. "Open editor with selected revision of the file" (`Vcs.Log.OpenRepositoryVersion.description`) | **UNVERIFIED**, re-tested 2026-10-06 against `log-tab`, which documents the same verb ("Click to open the repository version of the selected file for editing") with no confirmation mentioned → needs runtime capture (R3) | settings tree; log-tab |
| `Show in External Tool` | no | none | — |

**The preflight principle, stated by the product itself:** "To check if your patch
can be applied without conflicts, click Show Diff `⌘D`. If there are conflicts, the
corresponding lines will be highlighted in red" (using-patches:61), i.e. *the diff
is the confirmation*. The same logic governs the whole viewer: you see the change
and the alternative side by side, and applying is the act of choosing.

**What is genuinely missing: no bulk action here confirms, and `Ignore Left/Right
Side` has no undo documented here.** `Revert` exists as a viewer verb, which is the
in-viewer answer, but see the undo section.

**`Accept` and `Append` sharing a control with a modifier is the safety
mechanism**: the destructive-ish one (replace) is the default, and the
accumulating one (append) requires holding a key. Worth copying exactly.

### What can be undone, from where, and for how long?

| Action | Undo | Evidence |
| --- | --- | --- |
| **Any merge operation in the viewer** | `⌃Z` / `⌃⇧Z`. "Use this keyboard shortcut to **undo/redo a merge operation**. **Conflicts will be kept in sync with the text**" (differences-viewer.md:155-156) | differences-viewer.md:155-156 |
| Accept / apply a side | the viewer's `Revert` verb (line 54), and `⌃Z` for a merge | line 54 |
| Append | `⌃Z`; also recoverable because append *adds* rather than replaces | lines 114-116 |
| `Ignore Left/Right Side` | **closed 2026-10-06**, recovery is documented for the merge result the verb feeds (not the verb by name): "If you need to revert the merge result, right-click the file in the Conflicts dialog and select `Revert conflict resolution`. The file will return to its conflicted state"; plus `⌃Z` "undo/redo a merge operation" | resolve-conflicts; differences-viewer |
| Apply All Non-Conflicting Changes | **closed 2026-10-06**, "it's easier to apply all of them simultaneously using the Apply all non-conflicting changes action and then undo the unwanted ones using the Revert action from this toolbar" (the central-pane toolbar) | resolve-conflicts |
| Resolve Simple Conflicts | **closed 2026-10-06**, same file-level recovery: `Revert conflict resolution` returns the file "to its conflicted state"; `⌃Z` covers a merge operation in the editor | resolve-conflicts; differences-viewer |
| Staged lines (Include/Exclude) | the staging area is the record; unstage by excluding again | — |
| **Everything, once committed** | the changes are in history; the viewer's reach ends | — |

**The `⌃Z` guarantee is the strongest statement on this surface, and it has an
important rider: "Conflicts will be kept in sync with the text."** That is a real
obligation, undoing must not desynchronise the conflict markers from the buffer.
Any TurboGit implementation of merge-undo has to preserve that invariant, and it is
testable: undo, then assert every conflict marker still corresponds to a conflict in
the text.

**Note what is *not* undoable**: `Ignore Left/Right Side` and the bulk-apply
actions have no documented recovery, and both discard work. This is the surface's
one real risk, and it is a **capture item** rather than a conclusion.

### Which options are on by default, and what is remembered between uses?

| Option | Default | Evidence |
| --- | --- | --- |
| `Ignore Differences` (whitespace) | **`None`**, "white spaces are important, and all the differences are highlighted. **This option is selected by default**" (differences-viewer.md:67; shelving:110) | line 67 |
| Highlighting granularity | **UNVERIFIED** — the five options are listed with no default marked (lines 89-98); re-tested 2026-10-06 against `settings-tools-diff-and-merge` (the page that titles itself "the default behavior of the Diff Viewer") — silent on this control → needs runtime capture (R3) | — |
| Side-by-side vs Unified | **UNVERIFIED** — re-tested 2026-10-06 against `settings-tools-diff-and-merge` and `differences-viewer` — neither marks a default viewer mode → needs runtime capture (R3) | — |
| `Synchronize Scrolling` | **UNVERIFIED**, re-tested 2026-10-06 against `settings-tools-diff-and-merge`, silent; the wording ("If this button is released, each pane can be scrolled independently", line 62) still implies synchronized is the held/default state → needs runtime capture (R3) | line 62 |
| `Align Changes in Side-by-Side Diff` | **UNVERIFIED** — re-tested 2026-10-06 against `settings-tools-diff-and-merge` — silent → needs runtime capture (R3) (line 61) | — |
| Editor tab vs separate window | **on** (editor tab), you *clear* `Open Diff as Editor Tab` for a window (commit-and-push-changes.md:168) | line 168 |
| **Double-click routing in the Commit window** | user-chosen: `Diff` or `Source` (commit-and-push-changes.md:157-162) | lines 157-162 |
| `Automatically apply non-conflicting changes` (Diff & Merge page) | **off**, you are told to it from the dialog (resolve-conflicts.md:60) | line 60 |
| Non-collapsible unchanged lines | configurable on the Diff & Merge page (differences-viewer.md:50) | line 50 |
| Annotations preload on file open | a setting (VcsBundle `advanced.setting.vcs.annotations.preload`) | VcsBundle |
| Line numbers / indent guides / soft wraps / whitespaces | each a toggle in `Editor settings` (shelving:160-161) | shelving:160-161 |
| Disable editing | **on by default**. "Enable editing of the local copy of the selected file, **which is disabled by default**" (shelving:157) | shelving:157 |

**The last row is the one to copy.** "Disable editing" defaults to *on*, i.e. the
Diff Viewer opens read-only and you must opt in to edit. The reason is implicit in
the surrounding text: editing from a diff is a mode commitment, and most diff
openings are read-only reviews. **Default to read-only; make editing a deliberate
act.**

Remembered between uses:

- **The Commit window's double-click routing is a persisted setting**
  (commit-and-push-changes.md:157-162), so the review-vs-edit choice is
  remembered, and the Diff Viewer honours it.
- `Show Diff in Separate Window` / `Open Diff as Editor Tab` are two settings for
  what is conceptually one choice (commit-and-push-changes.md:166-168), a small
  inconsistency in the reference, worth not copying.
- **The diff's own presentation settings are global**, not per-comparison, so the
  whitespace policy applies everywhere it applies.
- **Binary files get a reduced option set** (shelving:167), the settings persist,
  but most of them do not apply.

### What feedback appears, and where does focus land?

**Feedback in the Diff Viewer is the diff itself.** There is no toast for
"applied"; the pane repaints and the change is now on the result side. That is the
right call for a surface whose entire purpose is showing you the change.

What does appear:

- **Progress: nothing documented.** No spinner string, no status-bar entry for a
  diff load. **UNVERIFIED**, and it is a real question, because a diff over a
  large file in a large repo is slow. Re-tested 2026-10-06 against
  `differences-viewer` and `settings-tools-diff-and-merge`, both silent → R3.
- **The `Ctrl`-tooltip** on the chevron: "Press `Ctrl` to change or to or and
  append changes" (lines 115-116), the feedback for a *mode* is a hint on the
  control, not a status bar.
- **Binary files**: a reduced option set rather than an error (shelving:167).
- **The "next file" suggestion**: "When the last or the first difference is
  reached, IntelliJ IDEA **suggests** clicking the arrow buttons or pressing
  F7 / Shift+F7 once more and comparing other files modified locally. **This
  behavior depends on the `Go to the next file after reaching last change` option
  in the Diff Viewer settings**" (differences-viewer.md:34). **A navigation action
  that crosses from one file's diff into the next, gated by a setting.** That is a
  lovely detail: `F7` past the last hunk offers the next file.

**Optimistic paint: yes, and correctly so**, the applied side is reflected
immediately because the alternative was visible on screen at the moment of the
choice. This is the one place optimistic paint is right: the user has just seen the
old and new values side by side.

Focus after the action:

- `⌃⇧⇥` moves the caret between panes; "The active pane has the caret" (line 107).
- `F4` opens the source "and the caret is placed in the same position as in the
  Diff Viewer" (line 46), **position is preserved across the surface boundary.**
- Accepting does not change which pane has the caret (UNVERIFIED, re-tested
  2026-10-06: no docs page describes focus after accept → R3).
- **`Diff.FocusOppositePaneAndScroll`** exists as a *separate action* from
  `Diff.FocusOppositePane`, so "move the caret" and "move the caret and
  re-sync the scroll" are distinct, and the second needs its own binding. **UNVERIFIED
  whether it has one**, re-tested 2026-10-06: `differences-viewer`'s shortcut table
  lists only `⌃⇧D` / `⌃⇧⇥` / `⌃Z`; the general `mastering-keyboard-shortcuts` page
  carries no diff rows; the old keymap-table slug `reference-keymap-mac-defaults`
  404s → needs runtime capture (R3).

## Flow, how tasks chain

### Entry points

1. `⌘D` from: the Commit window (a file row), the Log's Changed Files pane, a
   stash row, a shelved change, the Apply Patch dialog, the History tab.
2. `Vcs.ShowDiffWithLocal`, a revision against the working tree.
3. `Vcs.ShowDiffWithLocal.Before`, against the *previous* local revision.
4. `Diff.OpenInEditor` / `Diff.OpenDiffInEditor` / `Diff.ShowStandaloneDiff` /
   `Diff.ShowInExternalTool`.
5. **From a merge conflict**, "When you double-click a file with conflicting
   changes, IntelliJ IDEA opens a tool for resolving conflicts locally" (see
   `05-conflicts`).
6. **From the Shelve dialog's Diff pane**, base vs the version about to be shelved
   (shelving:78).
7. **From the Push dialog's commit preview**, "right-click a file and select
   `Show Diff` or press `⌘D`" (commit-and-push-changes.md:125).
8. **From branch comparison**, `Show Diff with Working Tree` opens a *Changes*
   window, and `⌘D` on a row opens this viewer (manage-branches.md:138).
9. **From a merge commit**, `Show Changes to Parents` gives a two- or three-panel
   diff (log-tab.md:426; investigate:150-153).

### Internal sequence, with dialog chaining

**Review a change (the 95% case):**

```
any surface → ⌘D
  → Diff Viewer  (editor tab | separate window)
      side-by-side | unified
  → navigate:  F7 / ⇧F7  (past the last hunk → suggests the next file)
      ⌃⇧⇥  between panes      F4  to the source, caret preserved
  → ⌃⇧D → Settings → Ignore Differences / Highlighting / Align / Sync scrolling
  → read-only by default; edit only if "Disable editing" is turned off
```

**Stage from the diff (the staging role):**

```
Commit window → the file → ⌘D
  → Diff Viewer
  → Accept / Append a change → the change is STAGED (not committed)
  → (or, in the Commit window's own chunk list)
        tick the chunk's checkbox
        → (one line?) Split Chunks and Include Selected Lines into Commit
             or hover the gutter → tick the line
  → Commit
```

(commit-and-push-changes.md:101-114, 181-189.) **Note the two routes to the same
outcome**: applying from the viewer, or ticking in the Commit window. The viewer is
a *staging surface*, not just an inspector.

**Stage one line, precisely, the three-way route:**

```
Commit window → the file → right-click → Compare HEAD, Staged and Local Versions
  → three-way Diff Viewer:
       left   = repository version
       right  = local version
       centre = a fully-functional editor where you make the changes you want staged
  → commit
```

(commit-and-push-changes.md:186-189.) **Editing the staged version directly**, rather
than selecting hunks. See `01-commit-tool-window` for why the two models differ.

**Resolve conflicts (the merge extension):**

```
double-click a conflicted file
  → three-pane merge tool (left | centre Result | right)
  → per conflict: accept / ignore per side, or Resolve using Left / Right
  → Resolve simple conflicts (one click, for simple conflicts only)
  → Apply All Non-Conflicting Changes  (deliberately skips simple conflicts)
  → [⌃⇧D] Compare Contents → Base | Middle
  → Review Changes → next file        OR        Accept and Finish
  → (closing with partial work) Discard Changes? → Discard | Continue Merge
  → ⌃Z / ⌃⇧Z undoes a merge operation; "Conflicts will be kept in sync with the text"
```

(resolve-conflicts.md:20-56; differences-viewer.md:104-136, 155-156.) **This is the
one place where the Diff Viewer stops being read-only and becomes the workspace**,
and it is why the surface carries 42 actions.

**Review how a merge was resolved, afterwards:**

```
Log → the merge commit
  → (conflicts were resolved) Show Changes to Parents
  → three-panel diff: the result against EACH parent
  → ⌘D per file
```

(investigate:152-153.) **The audit trail for conflict resolution**, closing the loop
with the Conflicts dialog.

**Apply a patch, the viewer as preflight:**

```
Apply Patch dialog → Show Diff ⌘D
  → "If there are conflicts, the corresponding lines will be highlighted in red"
  → back to the Apply Patch dialog → OK
```

(using-patches:61.) **The diff is the confirmation for a destructive operation on a
different surface**, see `10-patches`.

### What happens next on success

- An applied change is staged; the Commit window's chunk list updates.
- `F4` opens the source with the caret in the corresponding position (line 46).
- A resolved conflict is marked resolved; `Accept and Finish` becomes enabled when
  none remain (resolve-conflicts.md:56).
- Past the last difference, `F7` **offers the next file** (differences-viewer.md:34).

### What happens next on failure, and how the user recovers

| Failure | Evidence | Recovery |
| --- | --- | --- |
| A merge operation was applied | not a failure, but `⌃Z` / `⌃⇧Z` undo it, and "Conflicts will be kept in sync with the text" (differences-viewer.md:155-156) | undo/redo |
| Ignored a side by mistake | **closed 2026-10-06**, the recovery is documented for the merge result the ignore feeds: "revert the merge result … `Revert conflict resolution`. The file will return to its conflicted state" (docs: resolve-conflicts); `⌃Z` for a merge operation (docs: differences-viewer) | resolve-conflicts; differences-viewer |
| Bulk-applied by mistake | **closed 2026-10-06**, "undo the unwanted ones using the Revert action from this toolbar" (the central-pane toolbar) (docs: resolve-conflicts) | resolve-conflicts |
| Resolved a simple conflict wrongly | **closed 2026-10-06**, "revert the merge result" via `Revert conflict resolution` in the Conflicts dialog; the file returns "to its conflicted state" (docs: resolve-conflicts) | resolve-conflicts |
| Binary file | "Binary files have a limited number of diff options" (shelving:167) | reduced functionality, not an error |
| Read-only file | "You cannot edit files that have read-only status" (line 56) | unlock first |
| `Open Repository Version` for editing | **closed 2026-10-06, neither.** "Click to open the repository version of the selected file for editing" (docs: log-tab), an editable editor buffer of the revision, not a working-tree change; rolling the file back is a *different* action in the same menu, `Get from Revision` ("Roll back the file to the state in the selected revision") | log-tab |
| Diff fails to load | **UNVERIFIED** — re-tested 2026-10-06 against `differences-viewer`, `comparing-files-and-folders`, `read-only-files` — no error state is documented for the viewer anywhere → needs runtime capture (R3) | — |
| `Compare Contents` used outside a conflict | **UNVERIFIED** — re-tested 2026-10-06: `differences-viewer` documents it only under its "Merge actions" toolbar group and `resolve-conflicts` only inside the merge tool; both are silent on availability outside merge mode → needs runtime capture (R3) | — |

**The honest summary: this surface has no documented error states.** Every bundled
string about failure belongs to a *different* surface (stash, patch, merge). The
Diff Viewer is treated as infallible, which is either because it is genuinely
simple or because its failures were never designed. **That is the largest
structural uncertainty on this surface and the top capture item.**

### Escape / dismiss semantics

- **`⌃Z` / `⌃⇧⌧` is the universal escape** for anything the viewer applied
  (differences-viewer.md:155-156), with the conflict-sync obligation attached.
- **`Disable editing` defaults to on**, so the viewer opens read-only and editing
  is opt-in (shelving:157). The escape from an accidental edit is therefore
  structural: you cannot edit by accident.
- **Closing the merge tool with partial work warns**: `Discard Changes?` →
  `Discard Changes` / `Continue Merge` (VcsBundle:583-586).
- **`Synchronize Scrolling` releases back to independent scrolling** (line 62),
  a press-and-hold with a defined released state.
- **Nothing else confirms.** No dialog in the viewer's own vocabulary asks "are you
  sure"; the design substitutes visibility for confirmation.

## Divergences (intentional)

| Dimension | Rebased behaviour | TurboGit behaviour | Why |
| --- | --- | --- | --- |
| UI | **42 actions**, three jobs in one surface: review, stage, resolve | `crates/turbogit-ui/src/ui/diff/`, 2-way (Side-by-Side ⇄ Unified), stage at File/Hunk/Line, `F7`/`⇧F7` nav, `Enter`/`Esc` char-range staging | The *action count* is not the target; the three jobs are. TurboGit has review and stage. **The third job, conflict resolution, lives in a separate surface** (`conflict_resolver.rs` + `conflicts.rs`), which is defensible. |
| UX | Staging is available **two ways**: apply from the viewer, or tick chunks/lines in the Commit window; plus the three-way "edit the staged version" route (commit-and-push-changes.md:101-114, 186-189) | File/Hunk/Line granularity toggle (default Line) + char-range staging | Same three granularities. TurboGit's toggle is more discoverable; the **"edit the staged version" route is the one worth adding**, because it is the only way to stage an arbitrary subset that is not a contiguous line range. **Open gap.** |
| UX | The accept chevron is **one control with a `Ctrl` modifier** switching replace ↔ append (differences-viewer.md:114-116) | Gutter stage/unstage buttons on hunk-header bands (`diff/actions.rs:367-412`) | **Copy the chevron-plus-modifier pattern.** One control, two outcomes, the accumulating one behind a modifier, and no menu. TurboGit's two buttons are more discoverable but take more space and cannot express "append". Deliberate improvement to adopt. |
| UX | **Editing is off by default**, `Disable editing` is on (shelving:157) | Editable; the diff is a review surface | **Deliberate departure, and the reference is right.** Most diff openings are reviews; making editing opt-in is a mode commitment the user should make once. Copy the default, keep the affordance. |
| UX | `Ignore Differences` is a **five-value whitespace policy** and highlighting is a **five-value granularity** (differences-viewer.md:65-99) | **verified 2026-10-06**, one boolean `Ignore whitespace` checkbox (`state.ui.diff_ignore_whitespace`, diff/view.rs:874; consumed at diff/actions.rs:96); no five-value policy, no highlighting-granularity control | **Open gap, and the values are fully specified including the rationale for each.** `Split changes` with its worked example ("A \n B and A X \n B X are treated as two changes") and the `None` rationale ("highlighting might introduce additional difficulties during a review") are the kind of detail that makes an option usable. Whitespace policy in particular is a real interoperability feature. |
| UX | Past the last difference, `F7` **offers the next file**, gated by `Go to the next file after reaching last change` (differences-viewer.md:34) | **verified 2026-10-06, the crossing exists**: edge-nudge, then a same-direction press inside `EDGE_WINDOW`, crosses to the adjacent changed file (hunk_nav.rs:26-31, 43-51; diff_data.rs:115-117; state.rs:1075, spec R7) | **Correction, not a gap.** TurboGit already crosses files; the mechanics differ, not the capability. IDEA gates it on a settings checkbox, TurboGit on a second press within the edge window. Compare which gate feels better at R3; do not "add" what exists. |
| UX | `F4` places the caret in the editor "in the same position as in the Diff Viewer" (differences-viewer.md:46) | **verified absent 2026-10-06**, no jump-to-source action or caret hand-off anywhere in `diff/` (grep for `F4`/jump/caret: no hits) | **Open gap, trivial and worth it.** Position preservation across a surface jump is the difference between reading and navigating. |
| UX | `⌃Z` / `⌃⇧Z` undoes a merge operation **and keeps conflicts in sync with the text** (differences-viewer.md:155-156) | **verified 2026-10-06**, no text-level `⌃Z` (the diff is read-only); the conflict resolver instead carries per-conflict Undo: "Undo always takes the conflict back to unresolved", backed by an undo stack that refreshes the composed buffer (conflict_resolver.rs:165-180, 202-206) | **Present in per-conflict form.** IDEA's invariant (undo must not desynchronise conflict markers from the buffer) is approximated by the resolver's undo stack restoring the composed buffer; a structural test can still assert the same property there. |
| UX | `Synchronize Scrolling` is a press-and-hold that releases to independent scrolling (differences-viewer.md:62) | **verified 2026-10-06**, side-by-side panes share ONE `ScrollArea::vertical().id_salt("diff_viewer")` (diff/view.rs:224): scrolling is synchronized by construction, and no independent-scroll escape exists | **Open gap, small.** Two panes that scroll together by default and independently on demand is better than a checkbox, because the default is the common case. TurboGit has only the default half. |
| UI | Two hosting modes (editor tab / separate window) as **two separate settings**, `Show Diff in Editor Tab` in the viewer and `Open Diff as Editor Tab` in advanced settings (differences-viewer.md:63; commit-and-push-changes.md:168) | `Diff.OpenDiffInEditor` and `Diff.ShowStandaloneDiff` as two actions | Same capability via actions rather than settings, which is more discoverable. **Deliberate departure, do not copy the two-settings split**, which is a reference inconsistency. |
| UX | `Open Repository Version` opens a revision "for editing" (`Vcs.Log.OpenRepositoryVersion.description`) | **verified absent 2026-10-06**, no counterpart anywhere in `crates/` (grep for "repository version": no hits) | **Open question, deliberately not resolved.** Editing a committed revision is a destructive-adjacent operation with no documented confirmation. If TurboGit implements it, the pre-existing-commit-restore path must be a preflight, not a dialog. |
| UX | **No documented error states for the viewer itself** | `diff_load.rs` + `diff_data.rs` have decode limits and an error path | **TurboGit is ahead here, and it should stay ahead.** The reference's silence is not a specification. TurboGit's binary/decode handling (`panes.rs` image + binary routing) exceeds what the docs describe. Recorded so the gap list does not read as a deficiency. |
| UI | `Align Changes in Side-by-Side Diff`, "vertically aligns the corresponding lines by padding differences with empty space" (differences-viewer.md:61) | **verified 2026-10-06**, correspondence is structural, not padded: a deletion and its addition share ONE row (`DisplayRow::Pair`, diff/view.rs:264-267), so the panes are row-aligned without an empty-space toggle | **Open gap, narrowed.** Pairing aligns the changed lines, but there is no padding of *unchanged* regions. IDEA's toggle exists for the drift a shared-row model cannot express. Padding is still the cheap add. |
| UX | The **three-colour grammar is shared** between change kinds (Green/Blue/Gray) and branch-comparison file status (gray/green/blue) (differences-viewer.md:12-22; manage-branches.md:131-135) | **partially verified 2026-10-06**, `theme.rs` owns the sharing for the added green: `STATUS_ADDED` *is* `DIFF_ADD_ACCENT` (`#6FAE75`, theme.rs:349-354, 373-377), one token two scopes. But the grammar is not three-colour: deleted lines are **red** (`DIFF_DEL_*`, theme.rs:364-366), and modified has no diff highlight (only a status-letter blue, `STATUS_MODIFIED`) | **Open gap, refined.** The design-system contract (one token, owned by the theme, consumed in two scopes) holds for the added green and is exactly the `AGENTS.md` rule; the *vocabulary* differs. TurboGit is add/del two-colour vs IDEA's add/modify/delete three-colour. Copying the grammar means adding a modified treatment and reconsidering red-vs-gray for deleted. |
| UI | `Vcs.Diff.ResolveConflictsInImports` — IDE-internal import resolution | — | **Excluded per `DISTILL.md` rule 5** (IDEA inheritance). Listed in the action inventory for completeness only. |
| UX | `Compare with Clipboard`, `Switch to Three-Side Viewer`, `Show Whitespaces`, `Show Line Numbers`, `Show Indent Guides`, `Soft-Wrap`, five context-menu toggles (differences-viewer.md:160-199) | **verified absent 2026-10-06**, none of the six exists in `diff/` (grep for clipboard/soft-wrap/indent-guides/three-side: no hits) | **Open gap, low value.** The first is a nice trick; the rest are editor conveniences that TurboGit's shell does not need. Recorded as a deliberate priority judgement, not an oversight. |

## Needs runtime capture

**Four items closed by documentation** (items 4 and 7 below on 2026-10-05 via the
renamed page; item 2 on 2026-10-06 via `resolve-conflicts`; item 6's "checks out
or opens read-only" half on 2026-10-06 via `log-tab`). The remaining gaps cluster
in one place: **the viewer has no documented failure or loading behaviour at all.** That is a documentation gap, not
necessarily a product gap, and TurboGit already exceeds it, so the residue here
is the reference's silence, not a missing feature.

1. **Loading and error states for the viewer itself.** No progress string, no error
   string, no empty state. A diff over a large file or a large repository is slow,
   and nothing in either source says what the user sees while it happens. **The
   highest-value capture on this surface**, and note the asymmetry: TurboGit has
   `diff_load.rs` decode limits and an error path, so this is the reference
   lacking a specification rather than TurboGit lacking a feature.
2. ~~**What `Ignore Left/Right Side` and `Apply All Non-Conflicting Changes` do about
   undo.**~~ **RESOLVED 2026-10-06 via `resolve-conflicts`.** The bulk apply recovers
   per-change ("undo the unwanted ones using the Revert action from this toolbar");
   a wrong merge result of any kind recovers at file level via `Revert conflict
   resolution` (the file returns to its conflicted state). See the undo and
   failure tables above.
3. **The default for `Highlighting Differences`** and for side-by-side vs unified,
   both are five-way and two-way controls with no marked default
   (differences-viewer.md:52, 89-98); re-tested against
   `settings-tools-diff-and-merge` 2026-10-06, silent.
4. ~~**The missing `compare-file-with-diff` page's content.**~~ **RESOLVED.** The
   slug was renamed, not dead: the content is `comparing-files-and-folders`
   (verified 200, 03 September 2026). A standalone file-comparison UI **does**
   exist, its chrome is documented, and it adds `Switch to Three-Side Viewer`,
   `Swap Sides`, `Open Blank Diff Window`, and configurable **external diff tools**.
   See Sources at the top of this file.
5. **Whether `Diff.FocusOppositePaneAndScroll` has a default binding.** It is a
   separate action with no key in `actions-labeled.tsv` or `keymap-bindings.txt`.
6. **Whether `Open Repository Version` confirms.** The "checks out or opens
   read-only?" half **RESOLVED 2026-10-06 via `log-tab`** ("open the repository
   version of the selected file for editing", an editable editor buffer; working-tree
   rollback is the separate `Get from Revision`). Whether it preflights or confirms
   before opening remains undocumented.
7. ~~**Whether the "compare with base" / "compare partial" triples are user-reachable
   from a menu or are internal-only.**~~ **RESOLVED, user-reachable.**
   `Switch to Three-Side Viewer` in the editor context menu is the human-facing half
   of the `Diff.ComparePartial.*` triple ("It allows you to load a third file and
   add it to the comparison"), and `Compare Contents` in the merge toolbar is the
   `CompareWithBase.*` half. Neither is named by an action id in the docs, which is
   why the bundle ids alone could not settle it.

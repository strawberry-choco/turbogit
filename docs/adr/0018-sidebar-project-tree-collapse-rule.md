# The sidebar PROJECTS tree nests folders and collapses single-repo chains into path labels

The sidebar's PROJECTS section previously rendered a *flat* two-level tree:
repos grouped by their first path component under the project directory
(`SidebarGroup` → `SidebarRepo`). A project whose repos sat three levels deep
showed every repo lumped under the topmost folder, and a single-repo project
showed a redundant group row. Issue #05's original screen was flat by design;
the redesign replaces it with a recursive project tree. The decisions below
are the ones a future reader would otherwise re-litigate.

## D1 — the tree is rooted at the project directory, with folder and repo nodes

The PROJECTS tree is built from the project directory downward, not from
first path components upward. Each directory in the tree is one of two node
kinds: a **folder node** (a directory that is not itself a repository root,
grouping the repos beneath it) or a **repo node** (a repository root,
carrying the root's git state: status dot, branch, ahead/behind). The old
"project group" concept — a display-only bucket named after the first path
segment — disappears; folder nodes *are* directories in the tree. CONTEXT.md
gains "Project tree", "Folder node", "Repo node", and "Path label" to name
these.

## D2 — a folder displays iff its subtree holds at least two repo nodes; otherwise it collapses and its repo is promoted

The collapse rule is recursive and applies at any depth:

- a **repo node** always displays (a directory with its own `.git` is a git
  item — example 1 `a/.git` → `a`);
- a **folder node** displays only when its subtree holds **at least two** repo
  nodes (example 2 `a/b`, `a/c` → folder `a` with repos `b`, `c`);
- a folder whose subtree holds exactly **one** repo collapses, and that repo
  is **promoted** to the folder's parent (example 4 `a/.git` + `a/b/.git` →
  repo `a` with repo `b` beneath it).

The threshold counts repos anywhere in the subtree, not just direct children:
a folder with one direct repo plus a second repo nested under a deeper folder
still displays (example 3 — `a` holds repo `b` and the `c` folder which holds
`d` and `e`, so `a` and `c` both render as folders). "Subtree holds ≥ 2 repos"
and "≥ 2 children that will themselves display" are equivalent formulations,
because every repo in a non-repo folder's subtree surfaces as exactly one
displayed child of that folder.

## D3 — a promoted repo carries a path label: first character per folder, repo name in full

When a folder collapses, the single repo beneath it is promoted, but it is
labeled with the collapsed chain, not its bare basename: each collapsed folder
contributes its **first character** and the repo's own name is kept in full,
joined by `/`. `foo/bar/.git` under a project root renders as `f/bar`; a deep
chain `a/b/c/.git` where `a` is the project directory renders as `a/b/c`.
The chain starts at the topmost collapsing folder (the project directory
itself when it is the one collapsing) and runs down to the repo's parent — the
displayed parent, when there is one, is never repeated in the label. The first
character means the first Unicode character, so non-ASCII folder names
abbreviate the same way (`项目/代码` → `项/代码`).

Rationale: promoting without a path label would silently drop folder identity
(the user could not tell `app` in `web/app` from `app` in `services/app`);
keeping the full chain would re-introduce exactly the folder nesting the
collapse rule removes. One character per folder is the minimal disambiguator.

## D4 — repo nodes with nested repos are expandable; collapse state keys by path

A repo node may contain child nodes (a repository inside a repository —
example 4). Such a node renders as a repo row with an expander, **expanded by
default**, so nested repos are visible immediately. The persisted collapse
state (`UiState::sidebar_collapsed`) switches from group *names* to relative
paths: in a nested tree the same name can legitimately appear at different
depths, so a name is no longer a stable key. Stale name-keyed entries from
older sessions match nothing and are harmless.

## D5 — the display rule is uniform under filtering

The live filter field and the smart-group filters re-collapse the tree on the
surviving repos with the same rule and the same path labels. A folder with one
surviving repo collapses again, and the surviving repo is promoted with its
abbreviated chain label. Filtering therefore never produces a shape that
contradicts the unfiltered display rule; the tree's structure stays honest
under every view. (The alternative — keeping the full folder shape and merely
dropping rows — was rejected: it would show a folder with a single child,
the exact shape D2 exists to eliminate.)

## D6 — discovery stays bounded; the display rule applies at any depth found

Root discovery keeps `SCAN_MAX_DEPTH = 3` in `multi_root`; "applies at any
depth" describes the *display* rule, not discovery. Repos beyond the scan
cap stay undiscovered exactly as before — this change is about how the tree is
shaped from the roots the scanner already finds, not about finding more of
them. Deep scans (`scan_deep`) remain the Welcome/Attach flow's concern.

## Model impact

The flat `SidebarGroup` / `SidebarTree` model in `sidebar.rs` is consumed by
three modules — `sidebar` (build/render/filter), `multi_selection`
(group checkboxes, the selection summary), and `smart_groups` (member counts,
group filtering). All three iterate `groups → repos`; replacing the model with
a recursive node tree means each of those passes becomes a recursive walk.
The domain model (`Root`, `RootId`) and the multi-root manager are untouched —
the nested tree is pure presentation logic over the flat registered-root list.

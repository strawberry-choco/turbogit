# TurboGit

A desktop Git client that presents a multi-root project — several Git
repositories operated on as one coherent unit — with IntelliJ-style workflows.
The UI vocabulary applies IntelliJ-style IDE conventions to Git concepts. This
is the glossary for the language used across code, UI, and docs.

## Language

**Project**:
The directory tree TurboGit was opened on; contains one or more roots. Shown
on the welcome screen and in recent projects.
_Avoid_: workspace, solution

**Project tree**:
The nested tree of the sidebar's PROJECTS section: folder nodes and repo
nodes arranged by the project's directory structure, rooted at the project
directory.
_Avoid_: repo list, flat group list

**Folder node**:
A project-tree node for a directory that is not itself a repository root.
Displayed only when its subtree holds at least two repo nodes; a folder whose
subtree holds exactly one repo collapses and that repo is promoted to the
folder's parent.
_Avoid_: folder item, project group

**Repo node**:
A project-tree node for a repository root, carrying the root's git state
(status dot, current branch, ahead/behind). A repo node may itself contain
child nodes when repositories nest.
_Avoid_: git item, repo row

**Path label**:
The label of a repo node promoted out of a collapsed folder chain: each
folder in the chain contributes its first character, joined with the repo's
own name by `/` (e.g. `f/bar` from `foo/bar`). Applies wherever the collapse
rule fires, including filtered trees.
_Avoid_: abbreviated name, short label

**Repository root (root)**:
A directory with its own `.git`, identified by its path; one Git repository
within the scanned project tree. Every piece of mutable Git state (branches,
status, stashes, conflicts) belongs to exactly one root. Multi-root means
several roots managed in one window with synchronous operations.
_Avoid_: repo (when the multi-root context matters), project, folder,
working copy

**Multi-root project**:
One project containing more than one registered repository root; the tool
operates on all of them as one coherent unit.
_Avoid_: workspace repo, meta-repo, solution

**Root scanner**:
The part of the multi-root module that discovers candidate roots under the
project directory and builds `Root` snapshots through the engine interface.
_Avoid_: discovery service, scan helper, VCS manager

**Root caches**:
The in-memory cache layer keyed by repository root: commit logs, ref
decorations, changed-file lists, path-scoped logs, and ahead/behind counts.
Invalidated as one unit through one interface — per root or all roots — never
poked field-by-field by callers.
_Avoid_: log cache / ref cache (as if separate concepts), cache clearing

**Keyed read**:
The one interface by which a surface reaches a cached Git value: a target goes in,
a verdict and the value come back, and asking the question is also what starts
getting the answer when it isn't there. Which comparison a diff is, which line a
blame covers, which side of a pane is wanted — those are keys, and keys belong to
the read, never to a caller. A Granular op reads through it without asking it to
fetch anything.
_Avoid_: ensure / ensure_diff / ensure_blame (the three calls it replaced), cache
lookup, read slot, fetcher

**Worktree lifecycle**:
The app-side module that owns freshness policy for a root's worktree data:
list request admission (one fetch per root in flight), mutation epochs,
completion-time invalidation for worktree-mutating operations, and per-row
dirty probe dispatch and settlement. Cached list values and the cache
invalidation interface stay in RootCaches; when the Worktrees tool window is
open stays with the shell.
_Avoid_: worktree manager, cache refresher, worktree cache

**Mutation epoch**:
A per-root counter bumped whenever a root's cached worktree list is
invalidated. A list fetch stamps the epoch at dispatch; if the epoch has moved
by settlement time, the result is dropped rather than resurrecting a
pre-mutation list over the fresh refetch.
_Avoid_: generation counter, cache version

**Git engine**:
The module that talks to git. Its interface is `GitExecutor`, and it answers with
domain values — a push plan, a file's conflict versions, a patch — rather than
git's own text for callers to substring. Production runs the CLI adapter, or the
composed adapter that answers some reads in-process and delegates the rest to it;
those two differ only in how much is in-process, and neither is a libgit2-only
path. Tests substitute an adapter seeded with values per repository shape, so a
suite needs a `git` binary only when its acceptance is that git really moved. An
adapter's conventions are not guaranteed across adapters: git's own DWIM, for
example, runs only under the CLI.
_Avoid_: VCS manager, executor wrapper, git backend

**Operation**:
The unit the Shell dispatches: one value carrying what to run, its display
label, the roots its results affect, whether it changes a repository root's
worktree set, and — for the operations that have one — its identity, which is
what settlement matches on. Named variants carry plain data; `Custom` carries a
one-shot closure for work whose label nothing inspects. Retrying is a property of
that variant set rather than a method on the type: a caller that wants a second
attempt constructs the operation again, which is what the Shelve confirmation
already does. An Operation is *not* the
Git engine method it eventually calls: it also owns the label, the scope and
the settlement, none of which the engine knows about.
_Avoid_: task, job, request, command (the palette already owns "command"), git call

**Dispatch seam**:
Where an Operation decides whether its work runs on a worker thread or on the
calling thread: `Spawned` under the Shell, `Inline` under the Headless harness.
Both modes post their results into the same channel, so settlement is the same
event pump either way. Internal to the app layer — a caller selects a mode by
choosing a constructor, never by naming the seam, and presentation code does
not learn it exists.
_Avoid_: async mode, sync flag, executor strategy, thread pool

**Headless harness**:
The deterministic way headless tests construct the app: an `AppState` over
explicitly given repository roots, registered synchronously through the
production registration path, with no background threads — completed
operations refresh root status synchronously. Built via `AppState::for_roots`.
_Avoid_: test fixture, mock app, fake state

**Shell**:
The always-present frame of the main window — workspace sidebar, tab strip,
status bar. Everything else renders inside it, the activity log panel at the
bottom of the central body; nothing sits above the content, so the central body
starts at the top window edge.
_Avoid_: chrome (too vague), app frame

**Sidebar**:
The 280px workspace column on the left of the shell: the workspace header (the
workspace-switch trigger), the repo/branch filter, the SMART GROUPS rules, pinned
views, and the PROJECTS tree. It chooses which root a surface is about; the tab
strip, not the sidebar, chooses the tool window.
_Avoid_: sidebar rail (there is no icon strip; the only rail in the app is the
2px accent at a selected row's leading edge), activity bar

**Tab**:
A clickable control in the shell's tab strip that activates a tool window. Not
the content itself. The Commit tool window also has internal sub-tabs (Local
Changes / Shelf / Stash); those are "sub-tabs".
_Avoid_: page, view

**Tool window**:
A full page of content inside the shell, selected via the tab strip. Exactly one
is active at a time (Changes, Log, Branches, Worktrees, Submodules).
_Avoid_: tab (reserved), panel

**Tool window header**:
The 28px uppercase title bar at the top of a tool window body ("CHANGES"), with
action icons on the right. Distinct from dialog titles.
_Avoid_: section header (used for smaller group titles)

**Group title**:
An 11px uppercase muted label introducing a group inside a pane ("RECENT",
"COMMIT MESSAGE").
_Avoid_: heading

**Sub-tab**:
A tab inside a tool window's body (Commit tool window: Local Changes /
Shelf / Stash), as opposed to shell tabs in the tab strip.
_Avoid_: inner tab, nested tab

**Popup**:
A non-modal floating layer anchored near its trigger (branches popup, VCS ops,
command palette). Closes on Esc/outside click; never dims the background.
_Avoid_: dropdown, flyout

**Dialog**:
A modal floating layer with a dimmed backdrop that blocks interaction until
dismissed (Push, Settings, New Branch…).
_Avoid_: window, popup (that is non-modal)

**Inert control**:
A visible, enabled-looking control with deliberately no behavior in v1 (unwired
branch actions, unbound settings rows). Rendered per the mockup; scope gaps are
recorded, never hidden.
_Avoid_: disabled (reserved for genuinely disabled state), stub

**Placeholder pane**:
Content shown when a rendered tab or sub-tab has no backing feature yet
(e.g. Shelf, Stash): a labeled empty pane stating the feature arrives later.
_Avoid_: empty state (reserved for data-driven emptiness)

**Recent project**:
A project previously opened in TurboGit, listed on the welcome screen before
anything is open.
_Avoid_: recent repos

**Changelist**:
A named bucket of local uncommitted changes; exactly one is active at a time,
and new edits land in the active changelist. The changelist model is what the
Commit tool window's one tree **replaced**: it ships the staging sections (see
Staging area mode) plus an "Unversioned Files" group at the bottom of the tree
and a "Merge conflicts" group of its own. User-created changelists are backlog.
_Avoid_: change set, change list, pending changes, group

**Staging area mode**:
The organization that ships: the Commit tool window's one tree mirrors Git's
index, with unstaged content under UNSTAGED and fully staged files under
STAGED, plus the "Unversioned Files" and "Merge conflicts" groups. It replaced
the **Changelist** model, and granularly-completed paths stay visible under
STAGED rather than leaving the list.
_Avoid_: index view, git staging

**Outgoing commits**:
The commits a root is ahead of its upstream — what the Push dialog lists per
root before pushing.
_Avoid_: unpushed commits, pending commits

**Shelf**:
An IDE-managed patch store: shelved work is stored as patch text and can be
re-applied repeatedly.
_Avoid_: stash (that is Git's own), clipboard

**Stash**:
Git's native whole-tree parking spot (`git stash`); applies back onto a clean
tree.
_Avoid_: shelf

**Synchronous branch control**:
The multi-root mode where one branch operation runs on every root as if they
were one repository, with rollback when some roots fail.
_Avoid_: batch mode, global branches

**Protected branch**:
A branch pattern for which force-push is forbidden.

**Conflict**:
A file whose three index versions (base, ours, theirs) disagree and await
resolution.

**Hunk**:
A contiguous block of changed lines in a diff, as emitted by git's unified
diff format. The unit of granular review; selectable as a whole or narrowed
to individual lines within it.
_Avoid_: chunk

**Partial staging**:
Staging or unstaging a subset of a file's changes — by hunk or by line —
instead of whole files. Covers both directions (stage and unstage);
discard remains a whole-file operation. Not available on conflicted files,
which resolve through the conflict modal.
_Avoid_: interactive add, chunk staging

**Granular op**:
One Operation of partial staging — a single stage or unstage of one hunk or
line selection on one file. A kind of Operation rather than a parallel concept:
it is dispatched through the same seam and settles through the same pump, and
it is `Custom` rather than a named variant precisely because nothing selects
behaviour on its label. The unit the granular module owns end-to-end: input
resolution, dispatch ordering, and completion settlement (exclusions and
preview focus).
_Avoid_: hunk action, partial op

**Display row**:
One visual line of the diff view. In side-by-side mode a paired deletion and
addition occupy a single display row with two cells; every other diff line is
its own display row. The unit the diff viewer scrolls and positions by.
Unified mode pages by underlying diff lines, so it shows more rows than
side-by-side for the same diff whenever pairs exist.
_Avoid_: row model, diff line (a display row may pair two)

**Loaded window**:
The prefix of a listing the app currently holds for one root — its newest
commits, oldest last — whether that listing is the root's whole log, one
file's history, one ref's history, or a pickaxe search. It is the unit a Batch
is added to, what "N shown" counts, and what a scope change replaces wholesale.
A fresh operation or a filter that moves the listing resets it to one Batch;
it is never evicted as it grows, so scrolling back to already-loaded history
reads from cache rather than re-fetching.
_Avoid_: page (that is a Batch), history, log

**Batch**:
One fetched slice of a listing — the unit a Loaded window grows by, and the
only thing the app ever asks git for. A cold read asks for one batch; a later
one asks for the batch after it, one row longer, so the row the window already
holds comes back as the boundary checksum. A batch that comes back short says
the listing is over, and its length against what was asked for is what sets
the window's has-more flag. The glossary reserves "page" for a tab's rejected
synonyms, so a fetched slice is a batch.
_Avoid_: page (reserved), chunk, slice

**Lane**:
The graph column a commit's node is drawn in — the numbered column of the
commit graph, assigned by a walk over the whole Loaded window rather than over
the rows on screen, so a commit's lane does not change as it scrolls past. Its
colour is a property of the window, not of the current filter.
_Avoid_: graph column (that is the gutter), colour, thread

**Current hunk**:
The single hunk of the open diff that all hunk navigation and granular verbs
act on: buttons, hover, and keyboard navigation set it; stage/unstage consume
it. There is exactly one at a time. Moving past the last (or before the
first) hunk does nothing on the first key press; pressing again within a
short window crosses to the first hunk of the next (or previous) file.
_Avoid_: hovered hunk, selected hunk (there is only one)

**File filter**:
A `/`-triggered text filter over the changed-file list in the Commit tool
window; shared by both sub-tabs. Distinct from log search, which filters
commits.
_Avoid_: search (reserved for log search), quick filter

**Image diff**:
A diff of a file whose versions are both decodable images (PNG, JPEG, GIF,
WebP). Shown as the two picture versions side by side with their dimensions
and byte sizes, instead of line content. An oversized or undecodable image
falls back to a binary change.
_Avoid_: picture compare, photo diff

**Binary change**:
A diff of a file whose content is not text on at least one side. Shown as a
one-line description — kind of change plus byte sizes before/after — never as
line content, and never a target of partial staging.
_Avoid_: binary file diff, non-text diff

**Rename header**:
The diff-view line naming a detected rename ("Renamed from X · N% similar")
shown above the renamed file's content diff, paired with an arrow annotation
on its changed-file list entry. Metadata only — never staged text. Detection
follows git's own defaults, unpinned by TurboGit.
_Avoid_: move marker, path change

**Drop commit**:
Removing one commit from the current branch's history by replaying its
descendants without it — the same meaning a dropped line carries in a rebase
plan. Only commits reachable from the current branch can be dropped; a merge
commit and a root commit never can. It does not remove the descendants, and it
is not a ref deletion.
_Avoid_: delete commit, remove commit, discard commit

**Reword commit**:
Replacing one commit's message by rewriting that commit. Bounded exactly as a
drop is: current-branch commits only, never a merge commit, never a root commit.
Distinct from amending, which rewrites the tip commit from the Commit tool
window.
_Avoid_: rename commit, edit commit message, amend

**Detached HEAD**:
A repository root checked out at a commit rather than a branch. It has no
current branch, so branch-scoped actions state why they are blocked and history
edits are unavailable. Reached by checking out a commit or a tag.
_Avoid_: detached (as an action name — checking out a commit is the verb), no
branch
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

**Git engine**:
The module that talks to git. Its interface is `GitExecutor`; production uses
the CLI adapter, tests use an in-memory adapter.
_Avoid_: VCS manager, executor wrapper, git backend

**Shell**:
The always-present frame of the main window — topbar, toolbar, sidebar rail,
tab strip, status bar. Everything else renders inside it.
_Avoid_: chrome (too vague), app frame

**Sidebar rail**:
The 48px vertical icon strip on the left edge. Its buttons switch tool windows
or activate inert features.
_Avoid_: sidebar, activity bar

**Tab**:
A clickable control in the shell's tab strip that activates a tool window. Not
the content itself. The Commit tool window also has internal sub-tabs (Local
Changes / Unversioned Files); those are "sub-tabs".
_Avoid_: page, view

**Tool window**:
A full page of content inside the shell, selected via the tab strip or rail.
Exactly one is active at a time (Commit, Git Log).
_Avoid_: tab (reserved), panel

**Tool window header**:
The 28px uppercase title bar at the top of a tool window body ("COMMIT"), with
action icons on the right. Distinct from dialog titles.
_Avoid_: section header (used for smaller group titles)

**Group title**:
An 11px uppercase muted label introducing a group inside a pane ("RECENT",
"COMMIT MESSAGE").
_Avoid_: heading

**Sub-tab**:
A tab inside a tool window's body (Commit tool window: Local Changes /
Unversioned Files / Shelf / Stash), as opposed to shell tabs in the tab strip.
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
A visible, enabled-looking control with deliberately no behavior in v1 (topbar
menu items, unwired branch actions, unbound settings rows). Rendered per the
mockup; scope gaps are recorded, never hidden.
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
and new edits land in the active changelist. v1 ships only the canonical ones
("Default Changelist", "Unversioned Files", merge conflicts); user-created
changelists are backlog.
_Avoid_: change set, change list, pending changes, group

**Staging area mode**:
The alternative organization mirroring Git's index (Unstaged/Staged) instead
of changelists.
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
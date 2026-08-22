# TurboGit

A desktop Git client that presents a multi-root project — one workspace over several Git repositories — with IntelliJ-style workflows. This is the glossary for the domain language used across code, UI, and docs.

## Language

**Multi-root project**:
One workspace containing more than one registered repository root; the tool operates on all of them as one coherent unit.
_Avoid_: workspace repo, meta-repo, solution

**Repository root (root)**:
A directory with its own `.git`, identified by its path. Every piece of mutable Git state (branches, status, stashes, conflicts) belongs to exactly one root.
_Avoid_: project, folder, working copy

**Root scanner**:
The part of the multi-root module that discovers candidate roots under the project directory and builds `Root` snapshots through the engine interface.
_Avoid_: discovery service, scan helper, VCS manager

**Git engine**:
The module that talks to git. Its interface is `GitExecutor`; production uses the CLI adapter, tests use an in-memory adapter.
_Avoid_: VCS manager, executor wrapper, git backend

**Changelist**:
A named bucket of local changes with exactly one active at a time; new edits land in the active changelist.
_Avoid_: change set, pending changes

**Staging area mode**:
The alternative organization mirroring Git's index (Unstaged/Staged) instead of changelists.
_Avoid_: index view, git staging

**Shelf**:
An IDE-managed patch store: shelved work is stored as patch text and can be re-applied repeatedly.
_Avoid_: stash (that is Git's own), clipboard

**Stash**:
Git's native whole-tree parking spot (`git stash`); applies back onto a clean tree.
_Avoid_: shelf

**Synchronous branch control**:
The multi-root mode where one branch operation runs on every root as if they were one repository, with rollback when some roots fail.
_Avoid_: batch mode, global branches

**Protected branch**:
A branch pattern for which force-push is forbidden.

**Conflict**:
A file whose three index versions (base, ours, theirs) disagree and await resolution.

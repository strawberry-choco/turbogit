# TurboGit — Feature Implementation Checklist

> Working backlog derived from [`product-spec.md`](./product-spec.md). Check items off as they are implemented. Each ID maps to a section in the spec. Phases (0–4) follow the roadmap in §9.
>
> **Stack (resolved):** Rust + egui (`eframe`) for UI, `egui_dock` for tool windows, `git2` (reads) + `git` CLI subprocess (porcelain). See spec §10.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done

---

## Phase 0 — Engine & Core

- [ ] **A9** Git executable config + WSL detection (`Settings → VCS → Git`)
- [ ] **Engine** Git Executor abstraction (CLI / libgit2 hybrid)
- [ ] **A3/A4** Enable VCS integration + directory mappings (persist to vcs.xml equiv.)
- [ ] **A1** Create local repo from project (`git init` + mapping)
- [ ] **A2** Clone from URL (incl. shallow clone A8)
- [ ] **A5** Auto-detect unregistered Git roots + notify
- [ ] **A6** Manage remotes (add/edit/remove)
- [ ] **Core** `Root` model: remotes, branches, currentBranch, head, status
- [ ] **Core** `VcsSettings` + protected-branch patterns

## Phase 1 — Single-root daily flow

- [ ] **C1** Local Changes view (changelists, active concept)
- [ ] **C2** Unversioned files bucket
- [ ] **C3/P** Ignored files (.gitignore honor)
- [ ] **C4** Multiple changelists + move changes between them (C5)
- [ ] **C6** Staging-area mode (Unstaged/Staged, preserve changelists on switch)
- [ ] **C7** Editor gutter change markers
- [ ] **C8** Inline diff + inline commit from gutter
- [ ] **C9** Commit message field (history, Amend, rules, Reformat, template)
- [ ] **C10** Advanced commit options (author, sign-off, reformat, imports, cleanup, copyright, TODO, run-config, analyze)
- [ ] **C11** Pre-commit hooks (per-commit + IDE-wide disable)
- [ ] **C12** Partial commits (by chunk / by line / by selection)
- [ ] **C13** Amend commit
- [ ] **C14** Commit & Push flow
- [ ] **C15** After-commit upload (optional)
- [ ] **E1** VCS branch widget (incoming/outgoing indicators)
- [ ] **E2** Branches popup (Recent/Local/Remote/Tags, dir grouping)
- [ ] **E3** New branch (from current / selected / commit)
- [ ] **E4** Checkout (local, remote→local, collision handling)
- [ ] **E5** Smart checkout (shelve/unshelve on conflict; Force vs Smart)
- [ ] **E6** Checkout and update
- [ ] **E7** Rename branch (+ unset upstream)
- [ ] **E8** Delete branch (+ restore, delete remote)
- [ ] **E9** Compare branches (with current / working tree)
- [ ] **E10** Favorite branches
- [ ] **E13** Workspace context restoration per branch
- [ ] **D1** Fetch (one branch / all remotes)
- [ ] **D2/D3** Pull / Update Project (merge vs rebase strategy)
- [ ] **D4** Push dialog (preview, edit target, no-remote define)
- [ ] **D5** Force push (force-with-lease) + protected-branch block
- [ ] **D6** Push tags (All / Current Branch)
- [ ] **D7** Push-rejected handling (auto-update, multi-repo)
- [ ] **D8** Incoming/outgoing indicators
- [ ] **D9** Explicit incoming check on remotes (Auto/Always/Never)
- [ ] **D10** Clean working tree on update (Stash/Shelve)
- [ ] **K1** Compare file versions
- [ ] **K2** Compare folders
- [ ] **K4** Compare with branch / working tree
- [ ] **K5** Diff options (whitespace, word/line, magic wand)
- [ ] **H1** Git Log tab (graph, branches/commits/files/details panes)
- [ ] **H2** Log filtering & search (branch/user/date/path/search, Go-to-hash, new filtered tab)
- [ ] **H3** Show repository at revision
- [ ] **H4** Compare two commits
- [ ] **H5** File history (Show History)
- [ ] **H6** History for selection (line-level)
- [ ] **H7** Directory history
- [ ] **H8** Annotate / Git Blame
- [ ] **M1** VCS Operations Popup (`Alt+``)

## Phase 2 — Integrate & resolve

- [ ] **F1** Merge (options: --no-ff/--ff-only/--squash/-m/--no-commit/--no-verify/--allow-unrelated-histories)
- [ ] **F2** Smart merge (stash → merge → unstash)
- [ ] **F3** Rebase (options: --onto/--rebase-merges/--keep-empty/--root/--update-refs/--autosquash)
- [ ] **F4** Pull into using rebase/merge
- [ ] **F8** Abort / Continue (merge/rebase/cherry-pick)
- [ ] **G1** Auto-merge non-conflicting changes
- [ ] **G2** Conflicts dialog (Accept Yours/Theirs/Merge; Resolve All Simple)
- [ ] **G3** 3-way merge editor (Local | Result | Server)
- [ ] **G4** Apply non-conflicting changes (All/Left/Right)
- [ ] **G5** Resolve simple conflicts (one-click)
- [ ] **G6** Per-line accept/ignore + Resolve using Left/Right
- [ ] **G7** Revert conflict resolution
- [ ] **G8** LF/CRLF conflict handling + commit warning
- [ ] **G9** Merge Conflicts node in Commit window
- [ ] **J5** Stash changes (per root, Keep index)
- [ ] **J6** Apply / Pop stash
- [ ] **J7** Stash as new branch
- [ ] **J8** Drop / clear stashes
- [ ] **I1** Revert commit
- [ ] **I2** Undo last commit

## Phase 3 — Multi-root ⭐

- [ ] **B1** Register multiple Git roots (mappings + auto-detect)
- [ ] **B2** Per-root status tracking (changes grouped per repository)
- [ ] **B3** Unified Log across roots (colored stripes, Show Root Names)
- [ ] **B4** Batch operations (Commit / Update Project / Push across roots)
- [ ] **B5** Synchronous branch control (execute on all roots)
- [ ] **B6** Per-repo branch control (sub-popups, current-root resolution)
- [ ] **B7** Branch divergence warning + rollback on partial failure
- [ ] **B8** Push dialog listing all repositories
- [ ] **B9** Submodule support (nested roots)
- [ ] **B10** Git worktree support (each worktree = root; New Worktree from)
- [ ] **B11** Root filtering in Log (Roots section in Path filter)
- [ ] **J1–J4, J9–J11** Shelve + unshelve + patches + combined tabs

## Phase 4 — History editing & polish

- [ ] **F5** Interactive rebase (pick/stop/reword/squash/fixup/drop/reorder)
- [ ] **F6** Cherry-pick commit
- [ ] **F7** Cherry-pick selected changes (partial)
- [ ] **F9** Get file from branch
- [ ] **I3** Edit commit message (reword, F2)
- [ ] **I4** Amend any earlier commit (Fixup / Squash Into + Commit and Rebase)
- [ ] **I5** Squash commits
- [ ] **I6** Drop commit
- [ ] **I7** Extract selected changes to separate commit
- [ ] **I8** Split commit (via Stop-to-Edit)
- [ ] **H9** Log indexing (fast filter, Search Everywhere history)
- [ ] **H10** Open commit/file on GitHub/GitLab
- [ ] **N1/N2** Local History (auto snapshots, restore/compare)
- [ ] **O1–O4** Tags (create, checkout, push, show in branches pane)
- [ ] **P1–P3** Ignore files (add to .gitignore, ignore/unignore, settings patterns)
- [ ] **Q** Full settings UI (all rows in spec §Q table)
- [ ] **L1–L6** Hosting plugins: GitHub/GitLab accounts, clone, share, PR/MR review+create, open-on-web, gists

# Git2Executor Migration Plan

> **Status:** open — written before the subcrate split, so paths below reflect
> the pre-split single-crate layout; the code now lives in
> `crates/turbogit-engine`. As of 2026-08-29 the migration is partial:
> `git2_exec.rs` still delegates sync/mutating ops to `cli.rs`.

## Goal

Replace every remaining `self.cli.<method>(…)` delegation in `Git2Executor` with a real libgit2 implementation. Keep `cli.rs` as a fallback only; do not change the `GitExecutor` trait contract.

---

## Phase A — Trivial (no parsing, direct libgit2 calls)

Approximately 45 LOC total. All use the existing `self.open(root)` / `err()` helpers; no new modules.

| Order | Method | libgit2 API | Notes |
|---|---|---|---|
| A1 | `current_branch` | `repo.head()` → `shorthand()` | Detached → `None`. Replace the whole `Err` branch with `repo.head()` success/failure path. |
| A2 | `show_file_bytes` | `revparse_single(rev).peel_to_blob().content()` | Return raw `Vec<u8>`. |
| A3 | `show_file` | Same as A2, but `.to_str_lossy().into_owned()` | Call `show_file_bytes` internally to avoid duplication. |
| A4 | `submodule_paths` | `repo.submodules()` → `.path()` | Filter out `None`. |
| A5 | `remotes` | `repo.remotes()` → each `.name()` / `.url()` | Return `Remote { name, url }`. |
| A6 | `config_get` | `repo.config().get_string(key)` | Missing key → `Ok(None)`. |

**Entry condition:** Phase L2 existing methods compile cleanly (`cargo check`).
**Exit condition:** All six methods produce identical results to the CLI on a real repo (verified by spot-checking or a focused integration test).

---

## Phase B — Small (short iterator loop or existing-pattern reuse)

Approximately 90 LOC total.

| Order | Method | libgit2 API | Notes |
|---|---|---|---|
| B1 | `tag_checkout` | `revparse_single(name).peel_to_commit()` + `checkout_tree` + `set_head` | Structurally identical to existing `branch_checkout`. Duplicate-and-adapt. |
| B2 | `outgoing_commits` | `revwalk` with `hide(upstream)` / `push(branch)` | Collect full SHAs, newest-first (revwalk default). |
| B3 | `ahead_behind` | `repo.graph_ahead_behind(base, head)` | Returns `(ahead, behind)` directly — matches CLI's already-flipped return type. |
| B4 | `stash_list` | `repo.stash_foreach(...)` | Build `Stash { index, message, root }`. `root` is the same `RootId(root.into())` the CLI uses. |
| B5 | `ref_decorations` | Iterate `repo.refs()` → classify each refname into `CommitRef::Branch` / `::Remote` / `::Tag` | Port the same classification logic as `parse_ref_name` in `cli.rs`. |

**Entry condition:** Phase A merged and green.
**Exit condition:** Every B-method output is byte-for-byte equivalent to the CLI on a repo with branches, stashes, and tags (use a scripted `setup` in `tests/`).

---

## Phase C — Medium (porcelain-parity or multi-step logic)

Approximately 200–300 LOC total. Each is independently landed; no cross-method dependencies.

| Order | Method | Scope | Key risks |
|---|---|---|---|
| C1 | `commit_files` | `commit.diff_to_tree()` → iterate `diff.delta()` → build `Change { path, status, orig_path }` | Rename/copy detection must match `diff-tree --name-status`. `Change::chunks` stays empty (matches CLI behavior for this path). |
| C2 | `log` | `revwalk` → iterate commits → extract author/committer/email/raw-message | `Commit` body must preserve exact whitespace (`%B`). Filter by `path` and `max_count`. |
| C3 | `branches` | `repo.branches()` → for each, resolve upstream from config to fill `Branch::tracking` | `-vv` bracket info is not directly exposed; each local branch needs `config.get_string(...)` to find its upstream. |
| C4 | `worktree_list` | `repo.worktrees()` (if available in git2 0.21) or fall back to `run("git worktree list --porcelain")` | Version gate. If libgit2 exposes worktrees, replicate the CLI's "path != root → emit" filtering. |

**Entry condition:** Phase B merged and green.
**Exit condition:** Each method passes against a scripted test repo with the relevant content (renamed files, tagged commits, submodules, multiple worktrees).

---

## Phase D — Large / deferred

These require significant new plumbing (credential callbacks, certificate handling, porcelain-shape replication) and are **not** started in this plan:

- Network: `clone`, `fetch`, `pull`, `push`, `push_dry_run`, `tag_push`, `branch_delete_remote`
- Status/porcelain: `status`
- Diff formatting: `diff`, `blame`
- History manipulation: `merge`, `rebase`, `rebase_interactive`, `cherry_pick`, `revert`, `abort`, `continue_op`

Revisit when libgit2 coverage catches up or when a concrete use case forces it.

---

## Verification per phase

After each phase lands:

1. `cargo fmt -- --check`
2. `cargo check --all-targets`
3. `cargo clippy --all-targets -- -D warnings`
4. `cargo test --all-targets` (focused on the area the phase touched if the full suite is slow)

If any phase introduces a new public helper in `model.rs` or `engine/mod.rs`, note the change in the phase entry.

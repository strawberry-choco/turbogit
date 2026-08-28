//! libgit2-backed [`GitExecutor`] (library-migration plan Phase L2).
//!
//! Composition over the CLI engine: an inner [`CliExecutor`] answers every
//! operation that is not migrated yet, so this backend is a strict subset of
//! the CLI backend's behavior — identical results, identical error shapes for
//! everything below except the overridden methods.
//!
//! Migrated here (real `git2` implementations):
//! - branches: create (+ checkout / start point), checkout, delete
//!   (`-d`/`-D` semantics), rename
//! - tags: create (lightweight + annotated), list
//! - stash: push, pop, drop, apply
//! - staging: add, add-all, unstage (`reset_default` against HEAD)
//! - partial staging: `apply_patch_to_index` in the forward direction via
//!   [`git2::Repository::apply`] + [`git2::ApplyLocation::Index`]
//!
//! Deliberately still on the CLI (see the method comments): reverse patch
//! application and intent-to-add — libgit2 1.9 (as bound by git2 0.21) has no
//! equivalent for either.

use crate::cli::CliExecutor;
use git2::{ApplyLocation, BranchType, ErrorCode, IndexAddOption, StashFlags};
use std::path::{Path, PathBuf};
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;
use turbogit_engine_api::{ApplyDirection, GitExecutor};

/// Executor that performs supported operations in-process through libgit2
/// and falls back to the system `git` CLI for everything else.
pub struct Git2Executor {
    /// Fallback engine: every non-migrated operation delegates here.
    pub cli: CliExecutor,
}

impl Git2Executor {
    /// Wrap a constructed [`CliExecutor`] as the delegation fallback.
    pub fn new(cli: CliExecutor) -> Self {
        Self { cli }
    }

    /// Open the repository at `root`.
    fn open(&self, root: &Path) -> TgResult<git2::Repository> {
        git2::Repository::open(root).map_err(|_| TgError::NotARepo(root.display().to_string()))
    }
}

/// Map a `git2` error into [`TgError`]. The error type has no libgit2 variant
/// yet (outside this phase's scope), so failures surface as
/// [`TgError::Other`] carrying the full libgit2 message.
fn err(e: git2::Error) -> TgError {
    TgError::Other(format!("libgit2: {e}"))
}

/// Repo-relative, forward-slash path form libgit2 expects for index paths.
fn rel(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Is `tip` fully merged into `base`? Mirrors the reachability check behind
/// `git branch -d`. [`git2::Repository::graph_descendant_of`] answers
/// strictly (equal SHAs are not descendants), so equality counts as merged.
fn merged_into(
    repo: &git2::Repository,
    base: git2::Oid,
    tip: git2::Oid,
) -> Result<bool, git2::Error> {
    Ok(base == tip || repo.graph_descendant_of(base, tip)?)
}

/// Map a libgit2 `Commit` to the project's `Commit` model. Used by `log`
/// to honor the CLI's parser quirks: trailing whitespace stripped from
/// the raw message, and the same author-time value used for both the
/// `author.time` and `committer.time` fields (the CLI's `%at` only
/// yields author time, but the model stores it twice).
fn commit_to_commit(commit: &git2::Commit<'_>, root: &RootId) -> Commit {
    let author = commit.author();
    let committer = commit.committer();
    let time = author.when().seconds();
    Commit {
        id: commit.id().to_string(),
        parents: commit.parent_ids().map(|o| o.to_string()).collect(),
        author: Signature {
            name: author.name().unwrap_or("").to_string(),
            email: author.email().unwrap_or("").to_string(),
            time,
        },
        committer: Signature {
            name: committer.name().unwrap_or("").to_string(),
            email: committer.email().unwrap_or("").to_string(),
            time,
        },
        message: commit.message().unwrap_or("").trim_end().to_string(),
        time,
        root: root.clone(),
    }
}

/// Returns `true` if `commit` changed `path` relative to its first
/// parent. The CLI's `git log -- <path>` filter is implemented by
/// libgit2's revwalk having no path filter of its own, so we walk each
/// commit and diff against parent 0. Root commits (no parent) are
/// diffed against an empty tree, so all files in that commit count as
/// "added" and match any path filter.
fn commit_touches_path(repo: &git2::Repository, commit: &git2::Commit<'_>, path: &Path) -> bool {
    let new_tree = match commit.tree() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let old_tree = match commit.parent(0) {
        Ok(p) => match p.tree() {
            Ok(t) => t,
            Err(_) => return false,
        },
        Err(_) => match empty_tree(repo) {
            Ok(t) => t,
            Err(_) => return false,
        },
    };
    let diff = match repo.diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None) {
        Ok(d) => d,
        Err(_) => return false,
    };
    diff.deltas()
        .any(|d| d.new_file().path() == Some(path) || d.old_file().path() == Some(path))
}

/// Resolve an empty tree OID (the `--root` baseline) into a `Tree`.
/// Mirrors the same trick used by `commit_files`.
fn empty_tree(repo: &git2::Repository) -> Result<git2::Tree<'_>, git2::Error> {
    let oid = repo.treebuilder(None)?.write()?;
    repo.find_tree(oid)
}

impl GitExecutor for Git2Executor {
    // ---------------------------------------------------------------- read ----
    // Reads stay on the CLI until the read-only migration phases land.

    fn status(&self, root: &Path) -> TgResult<RootStatus> {
        self.cli.status(root)
    }

    fn log(&self, root: &Path, opts: &LogOpts) -> TgResult<Vec<Commit>> {
        // libgit2 parity for `git log --pretty=format:%H\x00%P\x00%an\x00
        // %ae\x00%cn\x00%ce\x00%at\x00%B`. Differences worth knowing:
        // - `%B` ends in a trailing newline; the CLI parser trims trailing
        //   whitespace, so we apply the same `trim_end` to libgit2's
        //   `commit.message()` (which is raw bytes including the newline).
        // - `%at` is the author time. The CLI parser assigns the SAME value
        //   to `author.time` and `committer.time`; we mirror that.
        // - Path filter has no libgit2 0.21 shortcut: we post-filter each
        //   commit by diffing it against its first parent (or the empty
        //   tree for the root commit) and matching any delta path.
        let repo = self.open(root)?;
        let mut walk = repo.revwalk().map_err(err)?;
        // TIME alone gives newest-first by committer time. For merges
        // the topological order matters; combined with TIME this matches
        // `git log`'s default (commit-date, parents-after-children).
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
            .map_err(err)?;
        if let Some(branch) = &opts.branch {
            let oid = repo.revparse_single(branch).map_err(err)?.id();
            walk.push(oid).map_err(err)?;
        } else {
            walk.push_head().map_err(err)?;
        }

        let root_id = RootId(root.into());
        let mut commits = Vec::new();
        for entry in walk {
            let oid = entry.map_err(err)?;
            let commit = repo.find_commit(oid).map_err(err)?;
            if let Some(path) = &opts.path
                && !commit_touches_path(&repo, &commit, path)
            {
                continue;
            }
            commits.push(commit_to_commit(&commit, &root_id));
            if let Some(max) = opts.max_count
                && commits.len() >= max
            {
                break;
            }
        }
        Ok(commits)
    }

    fn current_branch(&self, root: &Path) -> TgResult<Option<String>> {
        let repo = self.open(root)?;
        let head = match repo.head() {
            Ok(head) => head,
            Err(e) if e.code() == ErrorCode::UnbornBranch => {
                // CLI parity: `symbolic-ref --short HEAD` succeeds on an
                // unborn HEAD — the branch name exists before any commit
                // does. `Repository::head` resolves the ref and fails
                // instead, so read the symbolic target directly. This keeps
                // `is_repo` true for freshly initialized repositories.
                let head = repo.find_reference("HEAD").map_err(err)?;
                return Ok(head
                    .symbolic_target()
                    .map_err(err)?
                    .and_then(|t| t.strip_prefix("refs/heads/"))
                    .map(|s| s.to_string()));
            }
            Err(e) => return Err(err(e)),
        };
        // CLI parity: `symbolic-ref --short HEAD` returns None when HEAD is
        // detached. libgit2 reports detached HEAD as `is_branch() == false`;
        // a named branch returns its shorthand.
        if head.is_branch() {
            Ok(head.shorthand().ok().map(|s| s.to_string()))
        } else {
            Ok(None)
        }
    }

    fn show_file_bytes(&self, root: &Path, rev: &str, path: &Path) -> TgResult<Vec<u8>> {
        let repo = self.open(root)?;
        let spec = format!("{}:{}", rev, path.to_string_lossy());
        let obj = repo.revparse_single(&spec).map_err(err)?;
        let blob = obj.peel_to_blob().map_err(err)?;
        Ok(blob.content().to_vec())
    }

    fn show_file(&self, root: &Path, rev: &str, path: &Path) -> TgResult<String> {
        let bytes = self.show_file_bytes(root, rev, path)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn ref_decorations(&self, root: &Path) -> TgResult<Vec<(CommitId, Vec<CommitRef>)>> {
        let repo = self.open(root)?;
        let mut by_sha: std::collections::HashMap<CommitId, Vec<CommitRef>> =
            std::collections::HashMap::new();
        let mut order: Vec<CommitId> = Vec::new();

        // Same classification logic as `parse_ref_name` in `cli.rs`.
        let classify = |refname: &str| -> Option<CommitRef> {
            if let Some(name) = refname.strip_prefix("refs/heads/") {
                return Some(CommitRef {
                    kind: GitRefKind::Branch,
                    name: name.to_string(),
                });
            }
            if let Some(rest) = refname.strip_prefix("refs/remotes/") {
                if rest.is_empty() {
                    return None;
                }
                return Some(CommitRef {
                    kind: GitRefKind::Remote,
                    name: rest.to_string(),
                });
            }
            if let Some(name) = refname.strip_prefix("refs/tags/") {
                return Some(CommitRef {
                    kind: GitRefKind::Tag,
                    name: name.to_string(),
                });
            }
            None
        };

        // Iterate every ref, resolve it to the commit it ultimately points at
        // (peel annotated tags to their commit), group by SHA, keep insertion
        // order for stable output.
        if let Ok(refs) = repo.references() {
            for ref_ in refs {
                let ref_ = match ref_ {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let refname = match ref_.name() {
                    Ok(n) => n,
                    Err(_) => continue,
                };
                let cr = match classify(refname) {
                    Some(c) => c,
                    None => continue,
                };
                // `%(objectname)` in `git for-each-ref` returns the OID of
                // whatever the ref points at (tag object for annotated tags,
                // commit for branches and lightweight tags), so we key by the
                // raw target, not the peeled commit.
                let target = match ref_.target() {
                    Some(o) => o,
                    None => continue,
                };
                let sha = target.to_string();
                if !by_sha.contains_key(&sha) {
                    order.push(sha.clone());
                }
                by_sha.entry(sha).or_default().push(cr);
            }
        }
        Ok(order
            .into_iter()
            .map(|id| (id.clone(), by_sha.remove(&id).unwrap_or_default()))
            .collect())
    }

    fn commit_files(&self, root: &Path, commit: &str) -> TgResult<Vec<Change>> {
        // libgit2 parity for `git diff-tree --no-commit-id --name-status -r
        // --root -M <commit>`. The CLI treats the parent as an empty tree
        // when the commit has none (--root), and post-processes for renames
        // at the default similarity. We mirror both: `diff_tree_to_tree`
        // against an empty builder for the root case, then `find_similar`
        // with `renames(true)` to surface R<score> entries.
        let repo = self.open(root)?;
        let commit = repo
            .revparse_single(commit)
            .map_err(err)?
            .peel_to_commit()
            .map_err(err)?;
        let new_tree = commit.tree().map_err(err)?;
        let old_tree = match commit.parent(0) {
            Ok(parent) => parent.tree().map_err(err)?,
            // `--root` parity: an empty tree stands in for the missing
            // parent. `TreeBuilder::write` materializes the (currently
            // empty) builder into an OID, which we resolve back into a
            // `Tree` for `diff_tree_to_tree`.
            Err(_) => {
                let oid = repo.treebuilder(None).map_err(err)?.write().map_err(err)?;
                repo.find_tree(oid).map_err(err)?
            }
        };
        let mut diff = repo
            .diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)
            .map_err(err)?;
        let mut find_opts = git2::DiffFindOptions::new();
        find_opts.renames(true);
        diff.find_similar(Some(&mut find_opts)).map_err(err)?;

        let mut changes = Vec::new();
        for delta in diff.deltas() {
            // `old_file().path()` and `new_file().path()` are `Option<&Path>`
            // because the entry's path may be malformed UTF-8 or absent on
            // the respective side of the diff; fall back to the side that
            // exists. For `Copied` and `Renamed` both sides are present.
            let new_path = delta
                .new_file()
                .path()
                .map(|p| p.to_path_buf())
                .or_else(|| delta.old_file().path().map(|p| p.to_path_buf()));
            let old_path = delta.old_file().path().map(|p| p.to_path_buf());
            let path = match new_path.or(old_path) {
                Some(p) => p,
                None => continue,
            };
            let (status, orig_path) = match delta.status() {
                git2::Delta::Added => (ChangeStatus::Added, None),
                git2::Delta::Deleted => (ChangeStatus::Deleted, None),
                git2::Delta::Modified => (ChangeStatus::Modified, None),
                git2::Delta::Renamed => (
                    ChangeStatus::Renamed,
                    delta.old_file().path().map(|p| p.to_path_buf()),
                ),
                git2::Delta::Copied => (
                    ChangeStatus::Copied,
                    delta.old_file().path().map(|p| p.to_path_buf()),
                ),
                // Typechange and anything libgit2 reports that has no
                // CLI name-status letter for it surfaces as Modified to
                // match `parse_name_status_line`'s fallthrough in `cli.rs`.
                _ => (ChangeStatus::Modified, None),
            };
            changes.push(Change {
                path,
                status,
                chunks: vec![],
                staged: false,
                unstaged: false,
                orig_path,
            });
        }
        Ok(changes)
    }

    fn branches(&self, root: &Path) -> TgResult<Vec<Branch>> {
        // libgit2 parity for `git branch -a -vv`:
        // - Local branches: `repo.branches(Some(Local))`, then `.upstream()`
        //   gives the short remote-tracking ref (e.g. `origin/main`) matching
        //   the CLI's `[origin/main: ...]` bracket. When no upstream is
        //   configured the call returns an error; that maps to `None`.
        // - Remote-tracking branches: `repo.branches(Some(Remote))` returns
        //   names like `origin/main`. The CLI's parser strips the `remote/`
        //   prefix and drops the `tracking` field for remotes. We do the
        //   same here so downstream consumers (state::ahead_behind, the
        //   branch popup) see identical shapes on both sides.
        // - Detached HEAD: `BranchType::Local` iteration never yields a
        //   phantom HEAD entry, so nothing extra to filter.
        let repo = self.open(root)?;
        let mut result = Vec::new();

        let locals = match repo.branches(Some(git2::BranchType::Local)) {
            Ok(it) => it,
            Err(e) => return Err(err(e)),
        };
        for entry in locals {
            let (branch, _type) = match entry {
                Ok(e) => e,
                Err(e) => return Err(err(e)),
            };
            let name = match branch.name() {
                Ok(Some(n)) => n.to_string(),
                // libgit2 can return Ok(None) when the ref name is not
                // valid UTF-8; skip such refs to match the CLI's
                // `split_whitespace().next()` which would yield the raw
                // bytes but the model can't hold non-UTF-8 names.
                Ok(None) | Err(_) => continue,
            };
            // _type is always Local here, but the Branches iterator
            // yields (Branch, BranchType) tuples; silence the unused
            // warning by consuming it.
            let _ = _type;

            let tracking = match branch.upstream() {
                Ok(up) => match up.name() {
                    Ok(Some(un)) => Some(un.to_string()),
                    Ok(None) | Err(_) => None,
                },
                Err(_) => None,
            };

            result.push(Branch {
                name,
                kind: BranchKind::Local,
                tracking,
                favorite: false,
                protected: false,
                exists: true,
            });
        }

        let remotes = match repo.branches(Some(git2::BranchType::Remote)) {
            Ok(it) => it,
            Err(e) => return Err(err(e)),
        };
        for entry in remotes {
            let (branch, _) = match entry {
                Ok(e) => e,
                Err(e) => return Err(err(e)),
            };
            let name = match branch.name() {
                Ok(Some(n)) => n.to_string(),
                Ok(None) | Err(_) => continue,
            };
            // libgit2 returns full short names like `origin/main`.
            // Strip the `remote/` prefix to match the CLI's parser,
            // which yields `main` from `remotes/origin/main`.
            let short = name
                .split_once('/')
                .map(|(_, rest)| rest.to_string())
                .unwrap_or(name);

            result.push(Branch {
                name: short,
                kind: BranchKind::Remote,
                tracking: None,
                favorite: false,
                protected: false,
                exists: true,
            });
        }

        Ok(result)
    }

    fn ahead_behind(&self, root: &Path, branch: &str, upstream: &str) -> TgResult<(usize, usize)> {
        let repo = self.open(root)?;
        let branch_oid = repo
            .revparse_single(branch)
            .map_err(err)?
            .peel_to_commit()
            .map_err(err)?
            .id();
        let upstream_oid = repo
            .revparse_single(upstream)
            .map_err(err)?
            .peel_to_commit()
            .map_err(err)?
            .id();
        // libgit2's `graph_ahead_behind` returns `(ahead, behind)` directly,
        // matching the trait contract (`ahead_behind` returns `(usize, usize)`
        // with that order per the CLI impl).
        let (ahead, behind) = repo
            .graph_ahead_behind(branch_oid, upstream_oid)
            .map_err(err)?;
        Ok((ahead, behind))
    }

    fn outgoing_commits(
        &self,
        root: &Path,
        branch: &str,
        upstream: &str,
    ) -> TgResult<Vec<CommitId>> {
        let repo = self.open(root)?;
        let branch_oid = repo
            .revparse_single(branch)
            .map_err(err)?
            .peel_to_commit()
            .map_err(err)?
            .id();
        let upstream_oid = repo
            .revparse_single(upstream)
            .map_err(err)?
            .peel_to_commit()
            .map_err(err)?
            .id();
        let mut walk = repo.revwalk().map_err(err)?;
        walk.hide(upstream_oid).map_err(err)?;
        walk.push(branch_oid).map_err(err)?;
        walk.map(|oid| oid.map(|o| o.to_string()).map_err(err))
            .collect::<Result<_, _>>()
    }

    fn remotes(&self, root: &Path) -> TgResult<Vec<Remote>> {
        let repo = self.open(root)?;
        let names = repo.remotes().map_err(err)?;
        let mut result = Vec::new();
        for name in names.iter_bytes() {
            let name = String::from_utf8_lossy(name).into_owned();
            let url = repo
                .find_remote(&name)
                .map_err(err)?
                .url()
                .unwrap_or_default()
                .to_string();
            result.push(Remote { name, url });
        }
        Ok(result)
    }

    fn stash_list(&self, root: &Path) -> TgResult<Vec<Stash>> {
        let mut repo = self.open(root)?;
        let mut result = Vec::new();
        repo.stash_foreach(|index, message, _oid| {
            result.push(Stash {
                message: message.to_string(),
                root: RootId(root.to_path_buf().into()),
                index,
            });
            true // continue
        })
        .map_err(err)?;
        Ok(result)
    }

    fn worktree_list(&self, root: &Path) -> TgResult<Vec<Worktree>> {
        let repo = self.open(root)?;

        // `Repository::worktrees()` returns a `StringArray` of names for
        // every linked worktree (empty when there are none). The main
        // worktree is reported first; we filter it out to match
        // `git worktree list`, which only emits linked worktrees.
        let names: Vec<String> = repo
            .worktrees()
            .map_err(err)?
            .iter()
            .collect::<Result<Vec<Option<&str>>, _>>()
            .map_err(err)?
            .into_iter()
            .flatten()
            .map(String::from)
            .collect();

        let mut result = Vec::new();
        for name in names {
            let wt = match repo.find_worktree(&name) {
                Ok(wt) => wt,
                Err(_) => {
                    // Metadata exists but worktree directory missing/corrupt —
                    // surface it so the UI can show prunable worktrees
                    // (matches CLI behavior).
                    result.push(Worktree {
                        path: PathBuf::from(name),
                        branch: String::new(),
                        root: RootId(root.into()),
                    });
                    continue;
                }
            };

            let path = wt.path().to_path_buf();

            // Skip the main worktree — its path equals the repo root.
            if path == root {
                continue;
            }

            let branch = (|| -> Option<String> {
                let wt_repo = git2::Repository::open_from_worktree(&wt).ok()?;
                if wt_repo.head_detached().ok()? {
                    return None;
                }
                wt_repo.head().ok()?.shorthand().ok().map(|s| s.to_string())
            })();

            result.push(Worktree {
                path,
                branch: branch.unwrap_or_default(),
                root: RootId(root.into()),
            });
        }
        Ok(result)
    }

    fn submodule_paths(&self, root: &Path) -> TgResult<Vec<PathBuf>> {
        let repo = self.open(root)?;
        let subs = repo.submodules().map_err(err)?;
        let mut result = Vec::new();
        for sub in subs {
            let mut rel = PathBuf::new();
            rel.push(sub.path());
            result.push(rel);
        }
        Ok(result)
    }

    fn config_get(&self, root: &Path, key: &str) -> TgResult<Option<String>> {
        let repo = self.open(root)?;
        let cfg = repo.config().map_err(err)?;
        match cfg.get_string(key) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(err(e)),
        }
    }
    // ---------------------------------------------------------- mutating ----

    fn init(&self, root: &Path) -> TgResult<()> {
        self.cli.init(root)
    }

    fn clone(&self, url: &str, dest: &Path, depth: Option<usize>) -> TgResult<()> {
        self.cli.clone(url, dest, depth)
    }

    fn add_remote(&self, root: &Path, name: &str, url: &str) -> TgResult<()> {
        self.cli.add_remote(root, name, url)
    }

    fn fetch(&self, root: &Path, remote: Option<&str>) -> TgResult<()> {
        self.cli.fetch(root, remote)
    }

    fn pull(&self, root: &Path, rebase: bool) -> TgResult<()> {
        self.cli.pull(root, rebase)
    }

    fn push(&self, root: &Path, remote: &str, branch: &str, force: bool) -> TgResult<()> {
        self.cli.push(root, remote, branch, force)
    }

    fn push_dry_run(
        &self,
        root: &Path,
        remote: &str,
        branch: &str,
        force: bool,
    ) -> TgResult<String> {
        self.cli.push_dry_run(root, remote, branch, force)
    }

    fn commit(&self, root: &Path, message: &str, amend: bool) -> TgResult<CommitId> {
        self.cli.commit(root, message, amend)
    }

    fn commit_index(&self, root: &Path, message: &str, amend: bool) -> TgResult<CommitId> {
        self.cli.commit_index(root, message, amend)
    }

    fn merge(&self, root: &Path, target: &str, opts: &MergeOpts) -> TgResult<()> {
        self.cli.merge(root, target, opts)
    }

    fn rebase(&self, root: &Path, onto: &str, opts: &RebaseOpts) -> TgResult<()> {
        self.cli.rebase(root, onto, opts)
    }

    fn cherry_pick(&self, root: &Path, commit: &str) -> TgResult<()> {
        self.cli.cherry_pick(root, commit)
    }

    fn abort(&self, root: &Path, op: &str) -> TgResult<()> {
        self.cli.abort(root, op)
    }

    fn continue_op(&self, root: &Path, op: &str) -> TgResult<()> {
        self.cli.continue_op(root, op)
    }

    fn rebase_interactive(&self, root: &Path, plan: &[RebasePlanEntry]) -> TgResult<()> {
        self.cli.rebase_interactive(root, plan)
    }

    fn worktree_add(&self, root: &Path, path: &Path, branch: &str) -> TgResult<()> {
        self.cli.worktree_add(root, path, branch)
    }

    // --------------------------------------------------- staging / worktree ----

    fn add(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let repo = self.open(root)?;
        let mut index = repo.index().map_err(err)?;
        for p in paths {
            let spec = rel(p);
            if root.join(p).exists() {
                index.add_path(Path::new(&spec)).map_err(err)?;
            } else {
                // `git add <deleted-path>` records the deletion; add_bypath
                // cannot (the file is gone), so drop the entry instead.
                index.remove_path(Path::new(&spec)).map_err(err)?;
            }
        }
        index.write().map_err(err)?;
        Ok(())
    }

    fn add_all(&self, root: &Path) -> TgResult<()> {
        let repo = self.open(root)?;
        let mut index = repo.index().map_err(err)?;
        // DEFAULT skips ignored files like `git add -A`; deletions are picked
        // up too (the index is updated to match the working tree).
        index
            .add_all(["*"], IndexAddOption::DEFAULT, None)
            .map_err(err)?;
        index.write().map_err(err)?;
        Ok(())
    }

    fn unstage(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        let repo = self.open(root)?;
        // `restore --staged` resets against HEAD; on an unborn branch both
        // git and libgit2 fail to resolve HEAD, which propagates here.
        let head_commit = repo.head().map_err(err)?.peel_to_commit().map_err(err)?;
        let head = head_commit.as_object();
        let specs: Vec<String> = if paths.is_empty() {
            // CLI maps empty to `restore --staged .` (everything). libgit2's
            // reset_default rejects an empty pathspec (count > 0 assert), so
            // enumerate the staged entries explicitly instead. Index entry
            // paths are already forward-slash separated.
            let index = repo.index().map_err(err)?;
            let specs: Vec<String> = index
                .iter()
                .map(|e| String::from_utf8_lossy(&e.path).into_owned())
                .collect();
            if specs.is_empty() {
                return Ok(());
            }
            specs
        } else {
            paths.iter().map(|p| rel(p)).collect()
        };
        repo.reset_default(Some(head), specs.iter().map(String::as_str))
            .map_err(err)?;
        Ok(())
    }

    fn restore(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        self.cli.restore(root, paths)
    }

    fn apply_patch_to_index(
        &self,
        root: &Path,
        patch: &str,
        direction: ApplyDirection,
    ) -> TgResult<()> {
        // Forward maps 1:1 onto libgit2's `git apply --cached` emulation:
        // [`ApplyLocation::Index`] applies to the index only and commits the
        // index write itself (git_apply uses an index writer internally).
        //
        // Reverse cannot migrate yet: libgit2 1.9 (bound by git2 0.21) has no
        // reverse flag on `git_apply` — its only option flag is
        // GIT_APPLY_CHECK — so the exact
        // `git apply --cached --reverse --recount` behavior stays on the CLI.
        if direction == ApplyDirection::Reverse {
            return self.cli.apply_patch_to_index(root, patch, direction);
        }
        if patch.trim().is_empty() {
            // `git apply` succeeds silently on empty input.
            return Ok(());
        }
        let repo = self.open(root)?;
        let diff = git2::Diff::from_buffer(patch.as_bytes()).map_err(err)?;
        repo.apply(&diff, ApplyLocation::Index, None).map_err(err)?;
        Ok(())
    }

    fn add_intent_to_add(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        // Delegated to the CLI: libgit2 1.9 cannot *create* intent-to-add
        // entries. `IndexEntryExtendedFlag::INTENT_TO_ADD` exists only for
        // reading such entries; no `git_index_add*` API produces them, and a
        // hand-built zero-OID entry would corrupt later index operations.
        // Keep the exact `git add -N` behavior on the CLI.
        self.cli.add_intent_to_add(root, paths)
    }

    // ----------------------------------------------------------- branches ----

    fn branch_create(
        &self,
        root: &Path,
        name: &str,
        checkout: bool,
        start_point: Option<&str>,
    ) -> TgResult<()> {
        let repo = self.open(root)?;
        let obj = repo
            .revparse_single(start_point.unwrap_or("HEAD"))
            .map_err(err)?;
        let commit = obj.peel_to_commit().map_err(err)?;
        // force=false fails on an existing name, matching plain `git branch`.
        let branch = repo.branch(name, &commit, false).map_err(err)?;
        if checkout {
            // Tree first, HEAD second — the standard libgit2 switch order.
            // SAFE strategy carries uncommitted changes over or refuses,
            // like `git checkout -b`.
            repo.checkout_tree(commit.as_object(), None).map_err(err)?;
            let refname = branch.get().name().map_err(err)?.to_string();
            repo.set_head(&refname).map_err(err)?;
        }
        Ok(())
    }

    fn branch_checkout(&self, root: &Path, name: &str) -> TgResult<()> {
        let repo = self.open(root)?;
        match repo.find_branch(name, BranchType::Local) {
            Ok(branch) => {
                let commit = branch.get().peel_to_commit().map_err(err)?;
                repo.checkout_tree(commit.as_object(), None).map_err(err)?;
                let refname = branch.get().name().map_err(err)?.to_string();
                repo.set_head(&refname).map_err(err)?;
                Ok(())
            }
            // `git switch` also accepts remote-tracking refs (DWIM) and other
            // commit-ish spellings; keep those rare inputs on the CLI
            // verbatim instead of approximating them here.
            Err(_) => self.cli.branch_checkout(root, name),
        }
    }

    fn branch_delete(&self, root: &Path, name: &str, force: bool) -> TgResult<()> {
        let repo = self.open(root)?;
        let mut branch = repo.find_branch(name, BranchType::Local).map_err(err)?;
        if !force {
            // Mirror `git branch -d`: refuse unless the tip is reachable from
            // the upstream when one is configured, else from HEAD. libgit2's
            // own delete has no unmerged check, so it is done here.
            let tip = branch.get().peel_to_commit().map_err(err)?.id();
            let head_tip = repo
                .head()
                .ok()
                .and_then(|h| h.peel_to_commit().ok())
                .map(|c| c.id());
            let merged = match branch.upstream().ok() {
                Some(upstream) => {
                    let base = upstream.get().peel_to_commit().map_err(err)?.id();
                    merged_into(&repo, base, tip)
                }
                None => match head_tip {
                    Some(head) => merged_into(&repo, head, tip),
                    None => Ok(false),
                },
            }
            .map_err(err)?;
            if !merged {
                return Err(TgError::Other(format!(
                    "the branch '{name}' is not fully merged;\nuse force delete to discard its commits"
                )));
            }
        }
        // Refuses the checked-out branch, matching the CLI failure mode.
        branch.delete().map_err(err)?;
        Ok(())
    }

    fn branch_delete_remote(&self, root: &Path, remote: &str, name: &str) -> TgResult<()> {
        self.cli.branch_delete_remote(root, remote, name)
    }

    fn branch_rename(&self, root: &Path, old: &str, new: &str) -> TgResult<()> {
        let repo = self.open(root)?;
        let mut branch = repo.find_branch(old, BranchType::Local).map_err(err)?;
        // force=false matches `git branch -m` (non-forced rename).
        branch.rename(new, false).map_err(err)?;
        Ok(())
    }

    // --------------------------------------------------------------- tags ----

    fn tag_create(&self, root: &Path, name: &str, message: Option<&str>) -> TgResult<()> {
        let repo = self.open(root)?;
        let obj = repo.revparse_single("HEAD").map_err(err)?;
        match message {
            Some(m) => {
                // Same identity source as `git tag -a`: user.name/email from
                // config; unset identity fails exactly like the CLI does.
                let sig = repo.signature().map_err(err)?;
                repo.tag_annotation_create(name, &obj, &sig, m)
                    .map_err(err)?;
            }
            None => {
                repo.tag_lightweight(name, &obj, false).map_err(err)?;
            }
        }
        Ok(())
    }

    fn tag_list(&self, root: &Path) -> TgResult<Vec<String>> {
        let repo = self.open(root)?;
        let names = repo.tag_names(None).map_err(err)?;
        Ok(names
            .iter_bytes()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .filter(|n| !n.is_empty())
            .collect())
    }

    fn tag_checkout(&self, root: &Path, name: &str) -> TgResult<()> {
        let repo = self.open(root)?;
        // Resolve the tag (annotated or lightweight), then peel to the
        // commit. `git switch <tag>` checks out a detached HEAD pointing at
        // the tag's commit, so we mirror that by setting HEAD detached to
        // the peeled commit.
        let obj = match repo.refname_to_id(&format!("refs/tags/{name}")) {
            Ok(id) => repo.find_object(id, None).map_err(err)?,
            Err(_) => repo.revparse_single(name).map_err(err)?,
        };
        let commit = obj.peel_to_commit().map_err(err)?;
        repo.checkout_tree(commit.as_object(), None).map_err(err)?;
        repo.set_head_detached(commit.id()).map_err(err)?;
        Ok(())
    }

    fn tag_push(&self, root: &Path, remote: &str, name: Option<&str>, all: bool) -> TgResult<()> {
        self.cli.tag_push(root, remote, name, all)
    }

    // ------------------------------------------------------- diff / blame ----

    fn diff(&self, root: &Path, opts: &DiffOpts) -> TgResult<String> {
        self.cli.diff(root, opts)
    }

    fn blame(&self, root: &Path, path: &Path, rev: Option<&str>) -> TgResult<Vec<BlameLine>> {
        self.cli.blame(root, path, rev)
    }

    // ------------------------------------------------------ revert / undo ----

    fn revert(&self, root: &Path, commit: &str) -> TgResult<()> {
        self.cli.revert(root, commit)
    }

    fn undo_last_commit(&self, root: &Path) -> TgResult<()> {
        self.cli.undo_last_commit(root)
    }

    fn stash_push(&self, root: &Path, message: &str, keep_index: bool) -> TgResult<()> {
        let mut repo = self.open(root)?;
        // Same identity source as `git stash push`: config user.name/email.
        let sig = repo.signature().map_err(err)?;
        let mut flags = StashFlags::empty();
        if keep_index {
            flags.insert(StashFlags::KEEP_INDEX);
        }
        match repo.stash_save2(&sig, Some(message), Some(flags)) {
            Ok(_) => Ok(()),
            // `git stash push` exits 0 with "No local changes to save";
            // libgit2 reports nothing-to-stash as NotFound. Map it to
            // success so callers see identical behavior.
            Err(e) if e.code() == ErrorCode::NotFound => Ok(()),
            Err(e) => Err(err(e)),
        }
    }

    fn stash_pop(&self, root: &Path, index: usize) -> TgResult<()> {
        let mut repo = self.open(root)?;
        repo.stash_pop(index, None).map_err(err)?;
        Ok(())
    }

    fn stash_drop(&self, root: &Path, index: usize) -> TgResult<()> {
        let mut repo = self.open(root)?;
        repo.stash_drop(index).map_err(err)?;
        Ok(())
    }

    fn stash_apply(&self, root: &Path, index: usize) -> TgResult<()> {
        let mut repo = self.open(root)?;
        // Default options do not reinstate the index, matching
        // `git stash apply` without `--index`.
        repo.stash_apply(index, None).map_err(err)?;
        Ok(())
    }
}

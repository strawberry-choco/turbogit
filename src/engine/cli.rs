//! CLI-backed [`GitExecutor`] that shells out to the system `git` binary.
//!
//! Every operation runs `git` with `current_dir(<root>)` so that paths and
//! repository resolution are always correct. Output is parsed from porcelain /
//! stable formats. See `engine/mod.rs` for the trait contract.

#![allow(dead_code)]

use crate::engine::GitExecutor;
use crate::error::{TgError, TgResult};
use crate::model::*;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Executor that drives git through the command line.
pub struct CliExecutor {
    pub settings: VcsSettings,
}

impl CliExecutor {
    /// Spawn `git` in `root` (used as the working directory), capturing stdout,
    /// stderr and the exit code. On a non-zero exit, return
    /// [`TgError::Cli`]; on success return `(stdout, stderr, 0)`.
    fn run(&self, root: &Path, args: &[&str]) -> TgResult<(String, String, i32)> {
        let bin = crate::model::git_binary(&self.settings);
        let output = Command::new(&bin)
            .args(args)
            .current_dir(root)
            .env("GIT_EDITOR", "true")
            .output()?;
        let code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            return Err(TgError::Cli { code, stderr });
        }
        Ok((stdout, stderr, 0))
    }
}

impl GitExecutor for CliExecutor {
    // ---------------------------------------------------------------- read ----

    fn status(&self, root: &Path) -> TgResult<RootStatus> {
        let (out, _, _) = self.run(root, &["status", "--porcelain=v2", "-b"])?;
        let mut changes: Vec<Change> = Vec::new();
        let mut conflicted: Vec<PathBuf> = Vec::new();

        for line in out.lines() {
            if line.starts_with('#') {
                // Header lines (branch.head / branch.ab / branch.oid).
                continue;
            } else if let Some(rest) = line.strip_prefix("1 ") {
                // 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>[\t<orig>]
                let xy = match rest.split_whitespace().next() {
                    Some(x) => x,
                    None => continue,
                };
                let path = nth_field(rest, 7)
                    .split('\t')
                    .next()
                    .unwrap_or("")
                    .to_string();
                if path.is_empty() {
                    continue;
                }
                let status = map_xy(xy);
                // Porcelain v2 `1 <XY>`: first char is the index (staged)
                // status; a space means the index is unchanged → unstaged.
                let staged = !xy.starts_with(' ');
                changes.push(Change {
                    path: PathBuf::from(path),
                    status,
                    chunks: vec![],
                    staged,
                });
            } else if let Some(rest) = line.strip_prefix("? ") {
                changes.push(Change {
                    path: PathBuf::from(rest.trim()),
                    status: ChangeStatus::Unversioned,
                    chunks: vec![],
                    staged: false,
                });
            } else if let Some(rest) = line.strip_prefix("! ") {
                changes.push(Change {
                    path: PathBuf::from(rest.trim()),
                    status: ChangeStatus::Ignored,
                    chunks: vec![],
                    staged: false,
                });
            } else if let Some(rest) = line.strip_prefix("u ") {
                // u <XY> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <hU> <hB> <path>
                // The conflict path is the final whitespace token (index varies
                // across git versions, so take the last field rather than a
                // fixed column).
                let path = rest.split_whitespace().last().unwrap_or("").to_string();
                if path.is_empty() {
                    continue;
                }
                let p = PathBuf::from(path);
                changes.push(Change {
                    path: p.clone(),
                    status: ChangeStatus::Conflicted,
                    chunks: vec![],
                    staged: false,
                });
                conflicted.push(p);
            }
            // Other porcelain lines ("2" rename summary, etc.) are ignored.
        }

        Ok(RootStatus {
            changes,
            conflicted,
        })
    }

    fn log(&self, root: &Path, opts: &LogOpts) -> TgResult<Vec<Commit>> {
        let mut a: Vec<String> = vec![
            "log".to_string(),
            "--pretty=format:%H%x00%P%x00%an%x00%ae%x00%cn%x00%ce%x00%at%x00%s%x1e".to_string(),
        ];
        if let Some(n) = opts.max_count {
            a.push(format!("-n{}", n));
        }
        if let Some(b) = &opts.branch {
            a.push(b.clone());
        }
        if let Some(p) = &opts.path {
            a.push("--".to_string());
            a.push(p.to_string_lossy().to_string());
        }
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        let (out, _, _) = self.run(root, &args)?;

        let root_id = RootId(root.to_path_buf());
        let mut commits = Vec::new();
        for raw in out.split('\u{1e}') {
            // git emits a record separator (`\x1e`) followed by a newline
            // between commits, so every record after the first carries a
            // leading `\n`. Trim it so parsed fields (especially the commit
            // id) are not polluted.
            let rec = raw.trim();
            if rec.is_empty() {
                continue;
            }
            let f: Vec<&str> = rec.split('\0').collect();
            if f.len() < 8 {
                continue;
            }
            let id = f[0].to_string();
            let parents: Vec<CommitId> = f[1]
                .split_whitespace()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            let an = f[2].to_string();
            let ae = f[3].to_string();
            let cn = f[4].to_string();
            let ce = f[5].to_string();
            let time: i64 = f[6].trim().parse().unwrap_or(0);
            let message = f[7].to_string();
            commits.push(Commit {
                id,
                parents,
                author: Signature {
                    name: an,
                    email: ae,
                    time,
                },
                committer: Signature {
                    name: cn,
                    email: ce,
                    time,
                },
                message,
                time,
                root: root_id.clone(),
            });
        }
        Ok(commits)
    }

    fn branches(&self, root: &Path) -> TgResult<Vec<Branch>> {
        let (out, _, _) = self.run(root, &["branch", "-a", "-vv"])?;
        let mut result = Vec::new();
        for line in out.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Strip the leading "* " / "  " marker.
            let content = trimmed.strip_prefix('*').unwrap_or(trimmed).trim_start();
            // First whitespace-delimited token is the ref name.
            let name = match content.split_whitespace().next() {
                Some(n) => n,
                None => continue,
            };
            // Skip detached-HEAD / symbolic ref annotation lines.
            if name.starts_with('(') || name.contains("->") {
                continue;
            }

            let tracking = if let (Some(s), Some(e)) = (content.find('['), content.find(']')) {
                if s < e {
                    let inner = &content[s + 1..e];
                    let t = inner.split(':').next().unwrap_or("").trim();
                    if t.is_empty() {
                        None
                    } else {
                        Some(t.to_string())
                    }
                } else {
                    None
                }
            } else {
                None
            };

            let (kind, disp_name) = if let Some(without) = name.strip_prefix("remotes/") {
                let local = match without.find('/') {
                    Some(i) => &without[i + 1..],
                    None => without,
                };
                (BranchKind::Remote, local.to_string())
            } else {
                (BranchKind::Local, name.to_string())
            };

            result.push(Branch {
                name: disp_name,
                kind,
                tracking: if kind == BranchKind::Remote {
                    None
                } else {
                    tracking
                },
                favorite: false,
                protected: false,
                exists: true,
            });
        }
        Ok(result)
    }

    fn current_branch(&self, root: &Path) -> TgResult<Option<String>> {
        match self.run(root, &["symbolic-ref", "--short", "HEAD"]) {
            Ok((out, _, _)) => Ok(Some(out.trim().to_string())),
            Err(TgError::Cli { code, stderr }) => {
                // Detached HEAD: "fatal: ref HEAD is not a symbolic ref".
                if stderr.contains("not a symbolic") || code == 128 {
                    Ok(None)
                } else {
                    Err(TgError::Cli { code, stderr })
                }
            }
            Err(e) => Err(e),
        }
    }

    fn ahead_behind(&self, root: &Path, branch: &str, upstream: &str) -> TgResult<(usize, usize)> {
        // `git rev-list --left-right --count upstream...branch` prints
        // "<behind>\t<ahead>\n" (commits only on the left = behind, right = ahead).
        let (out, _, _) = self.run(
            root,
            &[
                "rev-list",
                "--left-right",
                "--count",
                &format!("{upstream}...{branch}"),
            ],
        )?;
        let mut parts = out.trim().split('\t');
        let behind = parts
            .next()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let ahead = parts
            .next()
            .and_then(|s| s.trim().parse::<usize>().ok())
            .unwrap_or(0);
        Ok((ahead, behind))
    }

    fn outgoing_commits(
        &self,
        root: &Path,
        branch: &str,
        upstream: &str,
    ) -> TgResult<Vec<CommitId>> {
        // `git rev-list upstream..branch` prints one full SHA per line,
        // newest-first — the same commits, in the same order, as
        // `git log @{u}..HEAD`.
        let (out, _, _) = self.run(root, &["rev-list", &format!("{upstream}..{branch}")])?;
        Ok(out
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect())
    }

    fn remotes(&self, root: &Path) -> TgResult<Vec<Remote>> {
        let (out, _, _) = self.run(root, &["remote", "-v"])?;
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for line in out.lines() {
            let mut parts = line.splitn(2, '\t');
            let name = match parts.next() {
                Some(n) if !n.is_empty() => n,
                _ => continue,
            };
            let rest = match parts.next() {
                Some(r) => r,
                None => continue,
            };
            let url = rest.split_whitespace().next().unwrap_or("").trim();
            if url.is_empty() {
                continue;
            }
            if seen.insert(name.to_string()) {
                result.push(Remote {
                    name: name.to_string(),
                    url: url.to_string(),
                });
            }
        }
        Ok(result)
    }

    fn stash_list(&self, root: &Path) -> TgResult<Vec<Stash>> {
        let (out, _, _) = self.run(root, &["stash", "list"])?;
        let mut result = Vec::new();
        for line in out.lines() {
            // stash@{n}: WIP on branch: subject
            if let Some(colon) = line.find(": ") {
                let prefix = &line[..colon];
                let message = line[colon + 2..].to_string();
                if let (Some(s), Some(e)) = (prefix.find('{'), prefix.find('}')) {
                    if let Ok(index) = prefix[s + 1..e].parse::<usize>() {
                        result.push(Stash {
                            message,
                            root: RootId(root.to_path_buf()),
                            index,
                        });
                    }
                }
            }
        }
        Ok(result)
    }

    fn worktree_list(&self, root: &Path) -> TgResult<Vec<Worktree>> {
        let (out, _, _) = self.run(root, &["worktree", "list", "--porcelain"])?;
        let mut result = Vec::new();
        let mut cur_path: Option<PathBuf> = None;
        let mut cur_branch = String::new();

        let flush =
            |path: Option<PathBuf>, branch: String, root: &Path, out: &mut Vec<Worktree>| {
                if let Some(p) = path {
                    if p != root {
                        let b = if let Some(stripped) = branch.strip_prefix("refs/heads/") {
                            stripped.to_string()
                        } else {
                            branch
                        };
                        out.push(Worktree {
                            path: p,
                            branch: b,
                            root: RootId(root.to_path_buf()),
                        });
                    }
                }
            };

        for line in out.lines() {
            if let Some(rest) = line.strip_prefix("worktree ") {
                flush(
                    cur_path.take(),
                    std::mem::take(&mut cur_branch),
                    root,
                    &mut result,
                );
                cur_path = Some(PathBuf::from(rest.trim()));
            } else if let Some(rest) = line.strip_prefix("branch ") {
                cur_branch = rest.trim().to_string();
            }
        }
        flush(cur_path.take(), cur_branch, root, &mut result);
        Ok(result)
    }

    fn submodule_paths(&self, root: &Path) -> TgResult<Vec<PathBuf>> {
        let (out, _, _) = self.run(root, &["submodule", "status"])?;
        let mut result = Vec::new();
        for line in out.lines() {
            let line = line.trim();
            if line.is_empty() || line.len() < 2 {
                continue;
            }
            // Leading status char + space, then path (optionally " (commit)").
            let rest = &line[1..];
            let path = rest.split_whitespace().next().unwrap_or("");
            if !path.is_empty() {
                result.push(PathBuf::from(path));
            }
        }
        Ok(result)
    }

    fn config_get(&self, root: &Path, key: &str) -> TgResult<Option<String>> {
        let args = ["config", "--get", key];
        match self.run(root, &args) {
            Ok((out, _, _)) => {
                let v = out.trim();
                if v.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(v.to_string()))
                }
            }
            Err(TgError::Cli { code, stderr }) => {
                // git config --get exits 1 when the key is unset.
                if code == 1 {
                    Ok(None)
                } else {
                    Err(TgError::Cli { code, stderr })
                }
            }
            Err(e) => Err(e),
        }
    }

    // ---------------------------------------------------------- mutating ----

    fn init(&self, root: &Path) -> TgResult<()> {
        if let Some(parent) = root.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let root_str = root.to_string_lossy();
        let args = ["init", root_str.as_ref()];
        let cwd = root.parent().unwrap_or(root);
        self.run(cwd, &args)?;
        Ok(())
    }

    fn clone(&self, url: &str, dest: &Path, depth: Option<usize>) -> TgResult<()> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut a: Vec<String> = vec!["clone".to_string()];
        if let Some(d) = depth {
            a.push(format!("--depth={}", d));
        }
        a.push(url.to_string());
        a.push(dest.to_string_lossy().to_string());
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        let cwd = dest.parent().unwrap_or(dest);
        self.run(cwd, &args)?;
        Ok(())
    }

    fn add_remote(&self, root: &Path, name: &str, url: &str) -> TgResult<()> {
        let args = ["remote", "add", name, url];
        self.run(root, &args)?;
        Ok(())
    }

    fn fetch(&self, root: &Path, remote: Option<&str>) -> TgResult<()> {
        let mut a: Vec<String> = vec!["fetch".to_string()];
        match remote {
            Some(r) => a.push(r.to_string()),
            None => a.push("--all".to_string()),
        }
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    fn pull(&self, root: &Path, rebase: bool) -> TgResult<()> {
        let mut a: Vec<String> = vec!["pull".to_string()];
        if rebase {
            a.push("--rebase".to_string());
        }
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    fn push(&self, root: &Path, remote: &str, branch: &str, force: bool) -> TgResult<()> {
        let mut a: Vec<String> = vec!["push".to_string()];
        if force {
            a.push("--force-with-lease".to_string());
        }
        a.push(remote.to_string());
        a.push(branch.to_string());
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    fn push_dry_run(
        &self,
        root: &Path,
        remote: &str,
        branch: &str,
        force: bool,
    ) -> TgResult<String> {
        let mut a: Vec<String> = vec!["push".to_string(), "--dry-run".to_string()];
        if force {
            a.push("--force-with-lease".to_string());
        }
        a.push(remote.to_string());
        a.push(branch.to_string());
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        // The human-readable dry-run report goes to stderr; stdout stays empty.
        let (_, stderr, _) = self.run(root, &args)?;
        Ok(stderr)
    }

    fn commit(&self, root: &Path, message: &str, amend: bool) -> TgResult<CommitId> {
        let mut a: Vec<String> = vec!["commit".to_string(), "-a".to_string()];
        if amend {
            a.push("--amend".to_string());
        }
        a.push("-m".to_string());
        a.push(message.to_string());
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        let (head, _, _) = self.run(root, &["rev-parse", "HEAD"])?;
        Ok(head.trim().to_string())
    }

    fn merge(&self, root: &Path, target: &str, opts: &MergeOpts) -> TgResult<()> {
        let mut a: Vec<String> = vec!["merge".to_string()];
        if opts.no_ff {
            a.push("--no-ff".to_string());
        }
        if opts.ff_only {
            a.push("--ff-only".to_string());
        }
        if opts.squash {
            a.push("--squash".to_string());
        }
        if opts.no_commit {
            a.push("--no-commit".to_string());
        }
        if opts.no_verify {
            a.push("--no-verify".to_string());
        }
        if opts.allow_unrelated {
            a.push("--allow-unrelated-histories".to_string());
        }
        if let Some(m) = &opts.message {
            a.push("-m".to_string());
            a.push(m.clone());
        }
        a.push(target.to_string());
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    fn rebase(&self, root: &Path, onto: &str, opts: &RebaseOpts) -> TgResult<()> {
        let mut a: Vec<String> = vec!["rebase".to_string()];
        if let Some(o) = &opts.onto {
            a.push("--onto".to_string());
            a.push(o.clone());
        }
        if opts.rebase_merges {
            a.push("--rebase-merges".to_string());
        }
        if opts.keep_empty {
            a.push("--keep-empty".to_string());
        }
        if opts.root {
            a.push("--root".to_string());
        }
        if opts.update_refs {
            a.push("--update-refs".to_string());
        }
        if opts.autosquash {
            a.push("--autosquash".to_string());
        }
        a.push(onto.to_string());
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    fn cherry_pick(&self, root: &Path, commit: &str) -> TgResult<()> {
        let args = ["cherry-pick", commit];
        self.run(root, &args)?;
        Ok(())
    }

    fn commit_index(&self, root: &Path, message: &str, amend: bool) -> TgResult<CommitId> {
        let mut a: Vec<String> = vec!["commit".to_string()];
        if amend {
            a.push("--amend".to_string());
        }
        a.push("-m".to_string());
        a.push(message.to_string());
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        let (head, _, _) = self.run(root, &["rev-parse", "HEAD"])?;
        Ok(head.trim().to_string())
    }

    fn abort(&self, root: &Path, op: &str) -> TgResult<()> {
        let args = [op, "--abort"];
        self.run(root, &args)?;
        Ok(())
    }

    fn continue_op(&self, root: &Path, op: &str) -> TgResult<()> {
        let args = [op, "--continue"];
        self.run(root, &args)?;
        Ok(())
    }

    fn rebase_interactive(&self, root: &Path, plan: &[RebasePlanEntry]) -> TgResult<()> {
        if plan.is_empty() {
            return Ok(());
        }
        let todo: String = plan
            .iter()
            .map(|e| {
                let verb = match e.action {
                    RebaseAction::Pick => "pick",
                    RebaseAction::Reword => "reword",
                    RebaseAction::Edit => "edit",
                    RebaseAction::Squash => "squash",
                    RebaseAction::Fixup => "fixup",
                    RebaseAction::Drop => "drop",
                };
                format!("{} {}\n", verb, e.commit)
            })
            .collect();
        let base_rev = format!("{}~1", plan[0].commit);
        let tmp = std::env::temp_dir().join(format!("turbogit-rebase-{}.txt", plan[0].commit));
        std::fs::write(&tmp, todo)?;
        let bin = crate::model::git_binary(&self.settings);
        let todo_str = tmp.to_string_lossy().replace('\\', "/");
        let status = Command::new(&bin)
            .args(["rebase", "-i", &base_rev])
            .current_dir(root)
            .env("GIT_SEQUENCE_EDITOR", format!("cp {}", todo_str))
            .env("GIT_EDITOR", "true")
            .status()?;
        let _ = std::fs::remove_file(&tmp);
        if !status.success() {
            return Err(TgError::Cli {
                code: status.code().unwrap_or(-1),
                stderr: "interactive rebase did not complete (conflict or halted)".to_string(),
            });
        }
        Ok(())
    }

    fn stash_push(&self, root: &Path, message: &str, keep_index: bool) -> TgResult<()> {
        let mut a: Vec<String> = vec!["stash".to_string(), "push".to_string()];
        if keep_index {
            a.push("--keep-index".to_string());
        }
        a.push("-m".to_string());
        a.push(message.to_string());
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    fn stash_pop(&self, root: &Path, index: usize) -> TgResult<()> {
        let refspec = format!("stash@{{{}}}", index);
        let args = ["stash", "pop", &refspec];
        self.run(root, &args)?;
        Ok(())
    }

    fn stash_drop(&self, root: &Path, index: usize) -> TgResult<()> {
        let refspec = format!("stash@{{{}}}", index);
        let args = ["stash", "drop", &refspec];
        self.run(root, &args)?;
        Ok(())
    }

    fn worktree_add(&self, root: &Path, path: &Path, branch: &str) -> TgResult<()> {
        let mut a: Vec<String> = vec!["worktree".to_string(), "add".to_string()];
        a.push(path.to_string_lossy().to_string());
        a.push(branch.to_string());
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    // --------------------------------------------------- staging / worktree ----

    fn add(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut a: Vec<String> = vec!["add".to_string()];
        for p in paths {
            a.push(p.to_string_lossy().to_string());
        }
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    fn add_all(&self, root: &Path) -> TgResult<()> {
        let args = ["add", "-A"];
        self.run(root, &args)?;
        Ok(())
    }

    fn unstage(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        if paths.is_empty() {
            let args = ["restore", "--staged", "."];
            self.run(root, &args)?;
        } else {
            let mut a: Vec<String> = vec!["restore".to_string(), "--staged".to_string()];
            for p in paths {
                a.push(p.to_string_lossy().to_string());
            }
            let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
            self.run(root, &args)?;
        }
        Ok(())
    }

    fn restore(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        if paths.is_empty() {
            let args = ["checkout", "--", "."];
            self.run(root, &args)?;
        } else {
            let mut a: Vec<String> = vec!["checkout".to_string(), "--".to_string()];
            for p in paths {
                a.push(p.to_string_lossy().to_string());
            }
            let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
            self.run(root, &args)?;
        }
        Ok(())
    }

    fn apply_patch_to_index(&self, root: &Path, patch: &str) -> TgResult<()> {
        let bin = crate::model::git_binary(&self.settings);
        let mut child = Command::new(&bin)
            .args(["apply", "--cached", "--recount"])
            .current_dir(root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        use std::io::Write;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(patch.as_bytes())?;
        }
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(TgError::Cli {
                code: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }
        Ok(())
    }

    // ----------------------------------------------------------- branches ----

    fn branch_create(
        &self,
        root: &Path,
        name: &str,
        checkout: bool,
        start_point: Option<&str>,
    ) -> TgResult<()> {
        let mut a: Vec<String> = if checkout {
            vec!["checkout".to_string(), "-b".to_string()]
        } else {
            vec!["branch".to_string()]
        };
        a.push(name.to_string());
        if let Some(sp) = start_point {
            a.push(sp.to_string());
        }
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    fn branch_checkout(&self, root: &Path, name: &str) -> TgResult<()> {
        let args = ["switch", name];
        self.run(root, &args)?;
        Ok(())
    }

    fn branch_delete(&self, root: &Path, name: &str, force: bool) -> TgResult<()> {
        let mut a: Vec<String> = vec!["branch".to_string()];
        a.push(if force {
            "-D".to_string()
        } else {
            "-d".to_string()
        });
        a.push(name.to_string());
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    fn branch_delete_remote(&self, root: &Path, remote: &str, name: &str) -> TgResult<()> {
        let args = ["push", remote, "--delete", name];
        self.run(root, &args)?;
        Ok(())
    }

    fn branch_rename(&self, root: &Path, old: &str, new: &str) -> TgResult<()> {
        let args = ["branch", "-m", old, new];
        self.run(root, &args)?;
        Ok(())
    }

    // --------------------------------------------------------------- tags ----

    fn tag_create(&self, root: &Path, name: &str, message: Option<&str>) -> TgResult<()> {
        let mut a: Vec<String> = vec!["tag".to_string()];
        if let Some(m) = message {
            a.push("-a".to_string());
            a.push(name.to_string());
            a.push("-m".to_string());
            a.push(m.to_string());
        } else {
            a.push(name.to_string());
        }
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    fn tag_list(&self, root: &Path) -> TgResult<Vec<String>> {
        let (out, _, _) = self.run(root, &["tag", "-l"])?;
        Ok(out
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect())
    }

    fn tag_checkout(&self, root: &Path, name: &str) -> TgResult<()> {
        let args = ["switch", name];
        self.run(root, &args)?;
        Ok(())
    }

    fn tag_push(&self, root: &Path, remote: &str, name: Option<&str>, all: bool) -> TgResult<()> {
        let mut a: Vec<String> = vec!["push".to_string(), remote.to_string()];
        if all {
            a.push("--tags".to_string());
        } else if let Some(n) = name {
            a.push(n.to_string());
        } else {
            a.push("--follow-tags".to_string());
        }
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    // ------------------------------------------------------- diff / blame ----

    fn diff(&self, root: &Path, opts: &DiffOpts) -> TgResult<String> {
        let mut a: Vec<String> = vec!["diff".to_string()];
        if opts.staged {
            a.push("--cached".to_string());
        }
        if opts.ignore_whitespace {
            a.push("--ignore-all-space".to_string());
        }
        if opts.stat {
            a.push("--stat".to_string());
        }
        if let (Some(l), Some(r)) = (&opts.left, &opts.right) {
            a.push(format!("{}..{}", l, r));
        } else if let Some(c) = &opts.commit {
            a.push(format!("{}^!", c));
        } else if let Some(l) = &opts.left {
            a.push(l.clone());
        }
        if let Some(p) = &opts.path {
            a.push("--".to_string());
            a.push(p.to_string_lossy().to_string());
        }
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        let (out, _, _) = self.run(root, &args)?;
        Ok(out)
    }

    fn blame(&self, root: &Path, path: &Path, rev: Option<&str>) -> TgResult<Vec<BlameLine>> {
        let mut a: Vec<String> = vec!["blame".to_string(), "--line-porcelain".to_string()];
        if let Some(r) = rev {
            a.push(r.to_string());
        }
        a.push("--".to_string());
        a.push(path.to_string_lossy().to_string());
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        let (out, _, _) = self.run(root, &args)?;
        Ok(parse_blame(&out))
    }

    fn show_file(&self, root: &Path, rev: &str, path: &Path) -> TgResult<String> {
        let spec = format!("{}:{}", rev, path.to_string_lossy());
        let args = ["show", &spec];
        let (out, _, _) = self.run(root, &args)?;
        Ok(out)
    }

    // ------------------------------------------------------ revert / undo ----

    fn revert(&self, root: &Path, commit: &str) -> TgResult<()> {
        let args = ["revert", "--no-edit", commit];
        self.run(root, &args)?;
        Ok(())
    }

    fn undo_last_commit(&self, root: &Path) -> TgResult<()> {
        let args = ["reset", "--soft", "HEAD~1"];
        self.run(root, &args)?;
        Ok(())
    }

    fn stash_apply(&self, root: &Path, index: usize) -> TgResult<()> {
        let refspec = format!("stash@{{{}}}", index);
        let args = ["stash", "apply", &refspec];
        self.run(root, &args)?;
        Ok(())
    }
}

// ----------------------------------------------------------------- helpers -

/// The Nth whitespace-separated field of `s` (0-based).
fn nth_field(s: &str, n: usize) -> &str {
    s.split_whitespace().nth(n).unwrap_or("")
}

/// Map a porcelain v2 XY status pair to a [`ChangeStatus`].
///
/// Prefers the index (X) letter; falls back to the worktree (Y) letter when the
/// index is unchanged. Typechange (`T`) is reported as Modified.
fn map_xy(xy: &str) -> ChangeStatus {
    let x = xy.chars().next().unwrap_or('.');
    let y = xy.chars().nth(1).unwrap_or('.');
    let c = if x != '.' { x } else { y };
    match c {
        'A' => ChangeStatus::Added,
        'M' => ChangeStatus::Modified,
        'D' => ChangeStatus::Deleted,
        'R' => ChangeStatus::Renamed,
        'C' => ChangeStatus::Copied,
        'T' => ChangeStatus::Modified,
        'U' => ChangeStatus::Conflicted,
        _ => ChangeStatus::Modified,
    }
}

/// Parse `git blame --line-porcelain` output into per-line records.
fn parse_blame(s: &str) -> Vec<BlameLine> {
    let mut out: Vec<BlameLine> = Vec::new();
    let mut commit = String::new();
    let mut author = String::new();
    let mut time: i64 = 0;
    let mut line_no: usize = 0;

    for line in s.lines() {
        if let Some(rest) = line.strip_prefix('\t') {
            out.push(BlameLine {
                commit: commit.clone(),
                author: author.clone(),
                time,
                line_no,
                content: rest.to_string(),
            });
            line_no += 1;
        } else if line.len() >= 40
            && line.is_char_boundary(40)
            && line[..40].chars().all(|c| c.is_ascii_hexdigit())
        {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                commit = parts[0].to_string();
                line_no = parts[2].parse().unwrap_or(line_no + 1);
            }
            author.clear();
            time = 0;
        } else if let Some(rest) = line.strip_prefix("author-time ") {
            time = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("author ") {
            // `author-mail` / `author-tz` are handled by the prefix check order.
            author = rest.trim().to_string();
        }
    }
    out
}

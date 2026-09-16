//! CLI-backed [`GitExecutor`] that shells out to the system `git` binary.
//!
//! Every operation runs `git` with `current_dir(<root>)` so that paths and
//! repository resolution are always correct. Output is parsed from porcelain /
//! stable formats. See `engine/mod.rs` for the trait contract.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::*;
use turbogit_engine_api::{ApplyDirection, GitExecutor};

/// Git-for-Windows rejects Windows verbatim (`\\?\`) prefixed paths handed
/// to `git worktree …`. Canonicalized roots carry that prefix (Rust
/// `canonicalize` yields `\\?\`-prefixed absolute paths on Windows), so
/// strip it before passing a worktree destination through to the CLI.
fn portable_path(p: &Path) -> PathBuf {
    #[cfg(windows)]
    if let Some(rest) = p.to_string_lossy().strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    p.to_path_buf()
}

/// Executor that drives git through the command line.
pub struct CliExecutor {
    pub settings: VcsSettings,
}

impl CliExecutor {
    /// Spawn `git` in `root` (used as the working directory), capturing stdout,
    /// stderr and the exit code. On a non-zero exit, return
    /// [`TgError::Cli`]; on success return `(stdout, stderr, 0)`.
    fn run(&self, root: &Path, args: &[&str]) -> TgResult<(String, String, i32)> {
        let bin = turbogit_domain::model::git_binary(&self.settings);
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

    /// [`Self::run`] variant capturing **raw stdout bytes** — binary-safe, no
    /// UTF-8 conversion is applied to the payload (R8 image/binary diffs).
    /// stderr is still decoded lossily for error reporting; error mapping is
    /// identical to [`Self::run`].
    fn run_bytes(&self, root: &Path, args: &[&str]) -> TgResult<Vec<u8>> {
        let bin = turbogit_domain::model::git_binary(&self.settings);
        let output = Command::new(&bin)
            .args(args)
            .current_dir(root)
            .env("GIT_EDITOR", "true")
            .output()?;
        if !output.status.success() {
            return Err(TgError::Cli {
                code: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }
        Ok(output.stdout)
    }

    /// [`Self::run`] variant with extra environment overrides (issue 31: the
    /// tag dialog's tagger identity rides on `GIT_COMMITTER_*`, which is how
    /// `git tag -a` picks the tagger). Error mapping is identical.
    fn run_env(
        &self,
        root: &Path,
        args: &[&str],
        envs: &[(&str, String)],
    ) -> TgResult<(String, String, i32)> {
        let bin = turbogit_domain::model::git_binary(&self.settings);
        let mut cmd = Command::new(&bin);
        cmd.args(args).current_dir(root).env("GIT_EDITOR", "true");
        for (k, val) in envs {
            cmd.env(k, val);
        }
        let output = cmd.output()?;
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

    /// Run an arbitrary git command in `root` — the raw-args escape hatch
    /// behind the "Custom command…" bulk operation (issue 13). Stdout is
    /// returned; error mapping matches [`CliExecutor::run`].
    fn run_raw(&self, root: &Path, args: &[String]) -> TgResult<String> {
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let (out, _, _) = self.run(root, &arg_refs)?;
        Ok(out)
    }

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
                // Porcelain v2 `1 <XY>`: the first char is the index (staged)
                // status; '.' means the index matches HEAD → unstaged. The
                // second char is the worktree status; '.' means the worktree
                // matches the index → nothing unstaged left.
                let staged = !xy.starts_with('.');
                let unstaged = xy.chars().nth(1).is_some_and(|y| y != '.');
                changes.push(Change {
                    path: PathBuf::from(path),
                    status,
                    chunks: vec![],
                    staged,
                    unstaged,
                    orig_path: None,
                });
            } else if let Some(rest) = line.strip_prefix("2 ") {
                // Rename/copy entry. Paths never contain tabs, so the LAST
                // tab always separates `<path>` from `<origPath>`; the score
                // column before `<path>` is space-separated on current git
                // (older drafts documented a tab there — both accepted).
                // 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>\t<origPath>
                let tabs: Vec<&str> = rest.splitn(3, '\t').collect();
                let (head, path, orig) = match tabs.as_slice() {
                    [head, path, orig] => (*head, path.trim(), orig.trim()),
                    [head, orig] => (
                        *head,
                        tail_from_field(head, 8).unwrap_or("").trim(),
                        orig.trim(),
                    ),
                    _ => continue,
                };
                if path.is_empty() {
                    continue;
                }
                let xy = head.split_whitespace().next().unwrap_or("");
                let status = map_xy(xy);
                let staged = !xy.starts_with('.');
                let unstaged = xy.chars().nth(1).is_some_and(|y| y != '.');
                changes.push(Change {
                    path: PathBuf::from(path),
                    status,
                    chunks: vec![],
                    staged,
                    unstaged,
                    orig_path: if orig.is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(orig))
                    },
                });
            } else if let Some(rest) = line.strip_prefix("? ") {
                changes.push(Change {
                    path: PathBuf::from(rest.trim()),
                    status: ChangeStatus::Unversioned,
                    chunks: vec![],
                    staged: false,
                    unstaged: false,
                    orig_path: None,
                });
            } else if let Some(rest) = line.strip_prefix("! ") {
                changes.push(Change {
                    path: PathBuf::from(rest.trim()),
                    status: ChangeStatus::Ignored,
                    chunks: vec![],
                    staged: false,
                    unstaged: false,
                    orig_path: None,
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
                    unstaged: false,
                    orig_path: None,
                });
                conflicted.push(p);
            }
            // Other porcelain lines (extension headers, etc.) are ignored.
        }

        Ok(RootStatus {
            changes,
            conflicted,
        })
    }

    fn log(&self, root: &Path, opts: &LogOpts) -> TgResult<Vec<Commit>> {
        let mut a: Vec<String> = vec![
            "log".to_string(),
            // %B carries the FULL raw message (subject + body); rows show its
            // first line while the details pane shows all of it (issue #12).
            // %G? carries the committer signature state (issue 17).
            "--pretty=format:%H%x00%P%x00%an%x00%ae%x00%cn%x00%ce%x00%at%x00%G?%x00%B%x1e"
                .to_string(),
        ];
        if let Some(n) = opts.max_count {
            a.push(format!("-n{}", n));
        }
        if let Some(b) = &opts.branch {
            a.push(b.clone());
        }
        // Pickaxe (issue 17): `git log -S<str>` keeps only commits where the
        // occurrence count of the string changed — the code-change search.
        if let Some(s) = &opts.pickaxe {
            a.push(format!("-S{}", s));
        }
        if let Some(p) = &opts.path {
            a.push("--".to_string());
            a.push(p.to_string_lossy().to_string());
        }
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        let (out, _, _) = self.run(root, &args)?;

        let root_id = RootId(root.into());
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
            if f.len() < 9 {
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
            let signature = parse_signature_state(f[7]);
            // %B ends with git's trailing newline; rows/labels expect the
            // message without it.
            let message = f[8].trim_end().to_string();
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
                signature,
            });
        }
        Ok(commits)
    }

    fn ref_decorations(&self, root: &Path) -> TgResult<Vec<(CommitId, Vec<CommitRef>)>> {
        let (out, _, _) = self.run(
            root,
            &[
                "for-each-ref",
                "--format=%(objectname)%09%(refname)",
                "--sort=-committerdate",
            ],
        )?;
        let mut order: Vec<CommitId> = Vec::new();
        let mut by_sha: std::collections::HashMap<CommitId, Vec<CommitRef>> =
            std::collections::HashMap::new();
        let mut has_tags = false;
        let mut has_remote_refs = false;
        for line in out.lines() {
            let Some((sha, refname)) = line.split_once('\t') else {
                continue;
            };
            if sha.len() < 40 {
                continue;
            }
            let Some(r) = parse_ref_name(refname) else {
                continue;
            };
            match r.kind {
                GitRefKind::Tag => has_tags = true,
                GitRefKind::Remote => has_remote_refs = true,
                GitRefKind::Branch => {}
            }
            let sha = sha.to_string();
            if !by_sha.contains_key(&sha) {
                order.push(sha.clone());
            }
            by_sha.entry(sha).or_default().push(r);
        }

        // Sync states (issue 17): remote-tracking refs report Gone when
        // their branch is missing from the remote, tags report pushed vs
        // local-only. Everything comes from one `ls-remote` per remote;
        // states are only assigned when every remote could be consulted —
        // a failed ls-remote (offline) leaves refs unmarked rather than
        // misreporting pushed tags as local-only or live refs as gone.
        if has_tags || has_remote_refs {
            let remotes: Vec<String> = self.run(root, &["remote"]).map(|(out, _, _)| {
                out.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_string)
                    .collect()
            })?;
            if remotes.is_empty() {
                for refs in by_sha.values_mut() {
                    for r in refs.iter_mut().filter(|r| r.kind == GitRefKind::Tag) {
                        r.state = RefState::LocalOnly;
                    }
                }
            } else {
                // Per remote: the branch names and tag names it actually has.
                let mut remote_refs: HashMap<String, (HashSet<String>, HashSet<String>)> =
                    HashMap::new();
                let mut all_ok = true;
                for remote in &remotes {
                    match self.run(root, &["ls-remote", remote]) {
                        Ok((out, _, _)) => {
                            let entry = remote_refs.entry(remote.clone()).or_default();
                            for line in out.lines() {
                                let Some((_, refname)) = line.split_once('\t') else {
                                    continue;
                                };
                                let refname = refname.trim();
                                if let Some(branch) = refname.strip_prefix("refs/heads/") {
                                    entry.0.insert(branch.to_string());
                                } else if let Some(tag) = refname.strip_prefix("refs/tags/") {
                                    // Skip peeled `^{}` duplicates — the
                                    // plain ref's presence is the answer.
                                    if !tag.ends_with("^{}") {
                                        entry.1.insert(tag.to_string());
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            all_ok = false;
                            break;
                        }
                    }
                }
                if all_ok {
                    for refs in by_sha.values_mut() {
                        for r in refs.iter_mut() {
                            match r.kind {
                                GitRefKind::Remote => {
                                    // `<remote>/<branch…>` — gone when the
                                    // remote no longer has that branch.
                                    if let Some((remote, branch)) = r.name.split_once('/')
                                        && let Some((heads, _)) = remote_refs.get(remote)
                                        && !heads.contains(branch)
                                    {
                                        r.state = RefState::Gone;
                                    }
                                }
                                GitRefKind::Tag => {
                                    let pushed = remote_refs
                                        .values()
                                        .any(|(_, tags)| tags.contains(&r.name));
                                    r.state = if pushed {
                                        RefState::Pushed
                                    } else {
                                        RefState::LocalOnly
                                    };
                                }
                                GitRefKind::Branch => {}
                            }
                        }
                    }
                }
            }
        }

        Ok(order
            .into_iter()
            .map(|id| {
                let refs = by_sha.remove(&id).unwrap_or_default();
                (id, refs)
            })
            .collect())
    }

    fn commit_files(&self, root: &Path, commit: &str) -> TgResult<Vec<Change>> {
        // --root covers parentless commits; -r recurses into trees; -M turns
        // on rename detection at git's own default similarity so renames
        // surface as `R<score>\told\tnew` (plumbing defaults it off).
        let (out, _, _) = self.run(
            root,
            &[
                "diff-tree",
                "--no-commit-id",
                "--name-status",
                "-r",
                "--root",
                "-M",
                commit,
            ],
        )?;
        Ok(out.lines().filter_map(parse_name_status_line).collect())
    }

    fn branches(&self, root: &Path) -> TgResult<Vec<Branch>> {
        let (out, _, _) = self.run(root, &["branch", "-a", "-vv"])?;
        // Branch-tip data, one `git for-each-ref` call: committer dates for
        // the popup's stale badge (issue 32) plus the short hash / subject /
        // author that power search-by-message and the detail panel's
        // latest-commit block (issue 02). Keyed by (kind, short name) so
        // every row gets data without an N-call fan-out.
        let mut last_touched: HashMap<(BranchKind, String), chrono::DateTime<chrono::Utc>> =
            HashMap::new();
        let mut tips: HashMap<(BranchKind, String), BranchTip> = HashMap::new();
        if let Ok((refs, _, _)) = self.run(
            root,
            &[
                "for-each-ref",
                "--format=%(refname:short)%00%(objectname:short)%00%(subject)%00%(authorname)%00%(committerdate:iso-strict)",
                "refs/heads",
                "refs/remotes",
            ],
        ) {
            for line in refs.lines() {
                let mut it = line.split('\0');
                let (Some(name), Some(oid), Some(subject), Some(author), Some(date)) =
                    (it.next(), it.next(), it.next(), it.next(), it.next())
                else {
                    continue;
                };
                // `refs/remotes/<remote>/HEAD` is a symbolic ref the `-vv`
                // listing skips; never invent a branch row for it.
                if name.ends_with("/HEAD") {
                    continue;
                }
                let Ok(dt) = chrono::DateTime::parse_from_rfc3339(date) else {
                    continue;
                };
                // `%(refname:short)` under refs/remotes is `origin/main`;
                // strip the first component to match the -vv parser's short
                // remote names (multi-remote safe).
                let (kind, short) = match name.split_once('/') {
                    Some((_, rest)) => (BranchKind::Remote, rest.to_string()),
                    None => (BranchKind::Local, name.to_string()),
                };
                let dt = dt.with_timezone(&chrono::Utc);
                last_touched.insert((kind, short.clone()), dt);
                tips.insert(
                    (kind, short),
                    BranchTip {
                        short_hash: oid.to_string(),
                        message: subject.to_string(),
                        author: author.to_string(),
                        time: dt,
                    },
                );
            }
        }

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
            // Skip detached-HEAD / symbolic ref annotation lines. The arrow lives
            // outside the first token (`remotes/origin/HEAD -> origin/main`),
            // so test the whole line, not just the name.
            if name.starts_with('(') || trimmed.contains("->") {
                continue;
            }

            // Bracket annotation: `[upstream]`, `[upstream: ahead 2, behind 1]`,
            // or `[upstream: gone]`. The part after the first `:` carries the
            // sync markers; `upstream` is the tracking ref.
            let (mut tracking, mut ahead, mut behind, mut gone) = (None, 0, 0, false);
            if let (Some(s), Some(e)) = (content.find('['), content.find(']'))
                && s < e
            {
                let inner = &content[s + 1..e];
                let (up, rest) = match inner.find(':') {
                    Some(i) => (inner[..i].trim(), &inner[i + 1..]),
                    None => (inner, ""),
                };
                if !up.is_empty() {
                    tracking = Some(up.to_string());
                }
                for part in rest.split(',') {
                    let p = part.trim();
                    if p.contains("gone") {
                        gone = true;
                    } else if let Some(v) = p.strip_prefix("ahead ") {
                        ahead = v.trim().parse().unwrap_or(0);
                    } else if let Some(v) = p.strip_prefix("behind ") {
                        behind = v.trim().parse().unwrap_or(0);
                    }
                }
            }

            let (kind, disp_name, remote) = if let Some(without) = name.strip_prefix("remotes/") {
                let (remote_name, local) = match without.find('/') {
                    Some(i) => (&without[..i], &without[i + 1..]),
                    None => (without, without),
                };
                (
                    BranchKind::Remote,
                    local.to_string(),
                    Some(remote_name.to_string()),
                )
            } else {
                (BranchKind::Local, name.to_string(), None)
            };

            result.push(Branch {
                name: disp_name.clone(),
                kind,
                tracking: if kind == BranchKind::Remote {
                    None
                } else {
                    tracking
                },
                favorite: false,
                protected: false,
                exists: true,
                ahead: if kind == BranchKind::Local { ahead } else { 0 },
                behind: if kind == BranchKind::Local { behind } else { 0 },
                gone: kind == BranchKind::Local && gone,
                last_touched: last_touched.get(&(kind, disp_name.clone())).copied(),
                tip: tips.get(&(kind, disp_name)).cloned(),
                remote,
            });
        }
        Ok(result)
    }

    fn current_branch(&self, root: &Path) -> TgResult<Option<String>> {
        match self.run(root, &["symbolic-ref", "--short", "HEAD"]) {
            Ok((out, _, _)) => Ok(Some(out.trim().to_string())),
            Err(TgError::Cli { stderr, .. }) => {
                // Detached HEAD: "fatal: ref HEAD is not a symbolic ref".
                // Only that case maps to `None`; every other failure (e.g.
                // "not a git repository") must propagate so repo discovery
                // does not mistake arbitrary directories for repositories.
                if stderr.contains("not a symbolic") {
                    Ok(None)
                } else {
                    Err(TgError::Cli { code: 128, stderr })
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

    fn is_ancestor(&self, root: &Path, upstream: &str, branch: &str) -> TgResult<bool> {
        // `git merge-base --is-ancestor` exits 0 when upstream IS an
        // `git merge-base --is-ancestor` exits 0 when upstream IS an
        // ancestor of branch (i.e. branch is fully merged into upstream
        // — safe to delete), and 1 otherwise. Anything else is an
        // unexpected error.
        match self.run(root, &["merge-base", "--is-ancestor", upstream, branch]) {
            Ok(_) => Ok(true),
            Err(TgError::Cli { code: 1, .. }) => Ok(false),
            Err(e) => Err(e),
        }
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
        // `git remote -v` prints `name<TAB>url (fetch)` / `url (push)` lines,
        // so a remote with divergent fetch/push URLs spans two lines. Collect
        // per-name, preserving remote order (order of first sighting).
        let mut order: Vec<String> = Vec::new();
        let mut fetch: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let mut push: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in out.lines() {
            let mut parts = line.splitn(2, '\t');
            let name = match parts.next() {
                Some(n) if !n.is_empty() => n.trim().to_string(),
                _ => continue,
            };
            let rest = match parts.next() {
                Some(r) => r,
                None => continue,
            };
            let mut words = rest.split_whitespace();
            let Some(url) = words.next() else { continue };
            let url = url.trim();
            if url.is_empty() {
                continue;
            }
            let is_push = rest.contains("(push)");
            if is_push {
                push.insert(name.clone(), url.to_string());
            } else {
                if !fetch.contains_key(&name) {
                    order.push(name.clone());
                }
                fetch.insert(name.clone(), url.to_string());
            }
        }
        Ok(order
            .into_iter()
            .map(|name| Remote {
                name: name.clone(),
                fetch_url: fetch.get(&name).cloned(),
                push_url: push.get(&name).cloned(),
            })
            .collect())
    }

    fn stash_list(&self, root: &Path) -> TgResult<Vec<Stash>> {
        let (out, _, _) = self.run(root, &["stash", "list"])?;
        let mut result = Vec::new();
        for line in out.lines() {
            // stash@{n}: WIP on branch: subject
            if let Some(colon) = line.find(": ") {
                let prefix = &line[..colon];
                let message = line[colon + 2..].to_string();
                if let (Some(s), Some(e)) = (prefix.find('{'), prefix.find('}'))
                    && let Ok(index) = prefix[s + 1..e].parse::<usize>()
                {
                    result.push(Stash {
                        message,
                        root: RootId(root.into()),
                        index,
                    });
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
                if let Some(p) = path
                    && p != root
                {
                    let b = if let Some(stripped) = branch.strip_prefix("refs/heads/") {
                        stripped.to_string()
                    } else {
                        branch
                    };
                    // The list is decoupled from the dirty probe (ticket 01):
                    // dirtiness is computed per worktree on demand by
                    // `worktree_dirty`, never here. A prunable (missing) worktree
                    // still lists, with no branch resolution needed.
                    out.push(Worktree {
                        path: p,
                        branch: b,
                        dirty: None,
                        root: RootId(root.into()),
                    });
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
                // Normalize git's emitted path (on Windows `git` prints a
                // forward-slash absolute path) to the canonical form rooted at
                // `\\?\` so linked worktrees agree with the git2 backend and
                // the main-worktree filtering below compares like-for-like.
                let p = PathBuf::from(rest.trim());
                cur_path = Some(p.canonicalize().unwrap_or(p));
            } else if let Some(rest) = line.strip_prefix("branch ") {
                cur_branch = rest.trim().to_string();
            }
        }
        flush(cur_path.take(), cur_branch, root, &mut result);
        Ok(result)
    }

    fn worktree_dirty(&self, path: &Path) -> TgResult<bool> {
        // Strictly cheaper than a full porcelain status, same answer: any
        // unmerged index entry, a tracked diff that stops at the first change
        // (`diff --quiet` exits 1 on the first difference and never touches
        // untracked files), or a single non-ignored untracked enumeration
        // (`--directory` collapses a whole untracked build dir into one
        // entry). A prunable (missing) worktree — whose command cannot run —
        // answers clean, matching the list's error-tolerant reading.
        let probe = (|| -> TgResult<bool> {
            let (conflicted, _, _) = self.run(path, &["ls-files", "-u"])?;
            if !conflicted.trim().is_empty() {
                return Ok(true);
            }
            match self.run(path, &["diff", "--quiet", "HEAD"]) {
                Ok(_) => {}
                // Exit code 1 = differences were found.
                Err(TgError::Cli { code: 1, .. }) => return Ok(true),
                Err(e) => return Err(e),
            }
            let (untracked, _, _) = self.run(
                path,
                &["ls-files", "--others", "--exclude-standard", "--directory"],
            )?;
            Ok(!untracked.trim().is_empty())
        })();
        Ok(probe.unwrap_or(false))
    }

    fn submodule_paths(&self, root: &Path) -> TgResult<Vec<PathBuf>> {
        Ok(self
            .submodule_status(root)?
            .into_iter()
            .map(|s| s.path)
            .collect())
    }

    fn submodule_status(&self, root: &Path) -> TgResult<Vec<Submodule>> {
        // `git submodule status` lines: "<status><sha> <path> [(<describe>)]"
        // with status ' ' in sync, '+' head off the record, '-' uninitialized,
        // 'U' conflicted; the sha is the submodule's HEAD (the recorded one
        // when uninitialized). The recorded gitlink per path comes from the
        // index (`git ls-files -s`, mode 160000).
        let (out, _, _) = self.run(root, &["submodule", "status"])?;
        let (recorded, _) = {
            let (idx, _, _) = self.run(root, &["ls-files", "-s"])?;
            let mut map = HashMap::new();
            for line in idx.lines() {
                // "160000 <sha> <stage>\t<path>"
                let Some((meta, path)) = line.split_once('\t') else {
                    continue;
                };
                let mut tokens = meta.split_whitespace();
                let mode = tokens.next().unwrap_or("");
                if mode != "160000" {
                    continue;
                }
                if let Some(sha) = tokens.next() {
                    map.insert(path.trim().to_string(), sha.to_string());
                }
            }
            (map, ())
        };

        let mut result = Vec::new();
        for line in out.lines() {
            if line.trim().is_empty() {
                continue;
            }
            // The status char is always the first byte of the raw line (' '
            // included) — never trim before slicing it off.
            let status_char = line.chars().next().unwrap_or(' ');
            let mut tokens = line[1..].split_whitespace();
            let Some(sha) = tokens.next() else {
                continue;
            };
            // The path is the first token that is not a parenthetical
            // describe (`(heads/main)` / `(<sha>)`).
            let Some(path) = tokens.find(|t| !t.starts_with('(')) else {
                continue;
            };
            let state = match status_char {
                '+' => SubmoduleState::NeedsUpdate,
                '-' => SubmoduleState::Uninitialized,
                'U' => SubmoduleState::Conflicted,
                _ => SubmoduleState::UpToDate,
            };
            let head = if state == SubmoduleState::Uninitialized {
                None
            } else {
                Some(sha.to_string())
            };
            result.push(Submodule {
                path: PathBuf::from(path),
                head,
                recorded: recorded.get(path).cloned().or_else(|| {
                    // Uninitialized lines carry the recorded sha in place of
                    // a HEAD; `ls-files` agrees but belt-and-braces both.
                    (state == SubmoduleState::Uninitialized).then(|| sha.to_string())
                }),
                state,
                root: RootId(root.into()),
            });
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

    fn set_remote_url(
        &self,
        root: &Path,
        name: &str,
        fetch_url: Option<&str>,
        push_url: Option<&str>,
    ) -> TgResult<()> {
        let mut ran = false;
        if let Some(url) = fetch_url {
            let args = ["remote", "set-url", name, url];
            self.run(root, &args)?;
            ran = true;
        }
        if let Some(url) = push_url {
            let args = ["remote", "set-url", "--push", name, url];
            self.run(root, &args)?;
            ran = true;
        }
        if !ran {
            return Err(TgError::Other(
                "set_remote_url needs at least one of fetch_url or push_url".into(),
            ));
        }
        Ok(())
    }

    fn rename_remote(&self, root: &Path, old: &str, new: &str) -> TgResult<()> {
        let args = ["remote", "rename", old, new];
        self.run(root, &args)?;
        Ok(())
    }

    fn remove_remote(&self, root: &Path, name: &str) -> TgResult<()> {
        let args = ["remote", "remove", name];
        self.run(root, &args)?;
        Ok(())
    }

    fn set_branch_upstream(&self, root: &Path, branch: &str, upstream: &str) -> TgResult<()> {
        let args = ["branch", &format!("--set-upstream-to={upstream}"), branch];
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

    fn push(
        &self,
        root: &Path,
        remote: &str,
        branch: &str,
        force: bool,
        tags: bool,
        no_verify: bool,
        set_upstream: bool,
        selected_oldest: Option<&str>,
    ) -> TgResult<()> {
        let mut a: Vec<String> = vec!["push".to_string()];
        if force {
            a.push("--force-with-lease".to_string());
        }
        if tags {
            a.push("--tags".to_string());
        }
        if no_verify {
            a.push("--no-verify".to_string());
        }
        if set_upstream {
            a.push("--set-upstream".to_string());
        }
        a.push(remote.to_string());
        if let Some(sha) = selected_oldest {
            // Subset push (issue #24): the user selected a suffix of the
            // outgoing list, so the oldest selected commit (inclusive)
            // becomes the refspec target.
            a.push(format!("{sha}:refs/heads/{branch}"));
        } else {
            a.push(branch.to_string());
        }
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
        if opts.verify_signatures {
            a.push("--verify-signatures".to_string());
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

    fn merge_auto_merged_files(
        &self,
        root: &Path,
        conflicted: &[PathBuf],
    ) -> TgResult<Vec<PathBuf>> {
        // No merge in progress → nothing to list.
        if !root.join(".git").join("MERGE_HEAD").exists() {
            return Ok(Vec::new());
        }
        // `git diff-tree --name-only --diff-filter=M HEAD MERGE_HEAD` lists
        // every path that changed between the two tips. We then subtract the
        // conflicted paths (those still unmerged in the index) to get the
        // auto-merged set — git has already resolved them onto HEAD.
        // `git diff-tree --name-only --diff-filter=M -r MERGE_HEAD^1
        // MERGE_HEAD` lists every path the merge attempted to resolve
        // (auto-merged + conflicted). Subtracting the conflicted paths
        // leaves the auto-merged set — files git resolved by itself.
        // We use the first parent of MERGE_HEAD (the side branch tip)
        // instead of HEAD so identical changes still show up.
        let (stdout, _, _) = self.run(
            root,
            &[
                "diff-tree",
                "--name-only",
                "--diff-filter=M",
                "-r",
                "MERGE_HEAD^1",
                "MERGE_HEAD",
            ],
        )?;
        let conflicted: std::collections::HashSet<PathBuf> = conflicted.iter().cloned().collect();
        Ok(stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .filter(|p| !conflicted.contains(p))
            .collect())
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
        let bin = turbogit_domain::model::git_binary(&self.settings);
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

    fn worktree_add(&self, root: &Path, path: &Path, branch: &str, create: bool) -> TgResult<()> {
        let mut a: Vec<String> = vec!["worktree".to_string(), "add".to_string()];
        if create {
            // `-b` consumes the branch name: `git worktree add -b <branch> <path>`.
            a.push("-b".to_string());
            a.push(branch.to_string());
        }
        a.push(portable_path(path).to_string_lossy().to_string());
        if !create {
            a.push(branch.to_string());
        }
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    fn worktree_remove(&self, root: &Path, path: &Path, force: bool) -> TgResult<()> {
        let mut a: Vec<String> = vec!["worktree".to_string(), "remove".to_string()];
        if force {
            a.push("--force".to_string());
        }
        a.push(portable_path(path).to_string_lossy().to_string());
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    fn submodule_update(&self, root: &Path, path: &Path, init: bool) -> TgResult<()> {
        let mut a: Vec<String> = vec!["submodule".to_string(), "update".to_string()];
        if init {
            a.push("--init".to_string());
        }
        a.push("--".to_string());
        a.push(path.to_string_lossy().to_string());
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
        Ok(())
    }

    fn submodule_deinit(&self, root: &Path, path: &Path, force: bool) -> TgResult<()> {
        let mut a: Vec<String> = vec!["submodule".to_string(), "deinit".to_string()];
        if force {
            a.push("-f".to_string());
        }
        a.push("--".to_string());
        a.push(path.to_string_lossy().to_string());
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

    fn apply_patch_to_index(
        &self,
        root: &Path,
        patch: &str,
        direction: ApplyDirection,
    ) -> TgResult<()> {
        let bin = turbogit_domain::model::git_binary(&self.settings);
        let mut args: Vec<&str> = vec!["apply", "--cached", "--recount"];
        if direction == ApplyDirection::Reverse {
            args.push("--reverse");
        }
        let mut child = Command::new(&bin)
            .args(&args)
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

    fn check_patch(&self, root: &Path, patch: &str) -> TgResult<()> {
        let bin = turbogit_domain::model::git_binary(&self.settings);
        let mut child = Command::new(&bin)
            .args(["apply", "--check", "--recount"])
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

    fn add_intent_to_add(&self, root: &Path, paths: &[PathBuf]) -> TgResult<()> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut a: Vec<String> = vec!["add".to_string(), "-N".to_string(), "--".to_string()];
        for p in paths {
            a.push(p.to_string_lossy().to_string());
        }
        let args: Vec<&str> = a.iter().map(|s| s.as_str()).collect();
        self.run(root, &args)?;
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

    fn tag_create(&self, root: &Path, spec: &TagSpec) -> TgResult<()> {
        let mut a: Vec<String> = vec!["tag".to_string()];
        if spec.message.is_some() {
            a.push("-a".to_string());
        }
        if spec.sign {
            a.push("-s".to_string());
        }
        a.push(spec.name.clone());
        if let Some(m) = &spec.message {
            a.push("-m".to_string());
            a.push(m.clone());
        }
        if let Some(t) = &spec.target {
            a.push(t.clone());
        }
        // The tag dialog's tagger identity override (issue 31): `git tag -a`
        // takes the tagger from the committer identity, so override it with
        // GIT_COMMITTER_* for this one invocation only.
        let mut envs: Vec<(&str, String)> = Vec::new();
        if let Some(id) = &spec.tagger {
            let (name, email) = parse_identity(id);
            if let Some(n) = name {
                envs.push(("GIT_COMMITTER_NAME", n));
            }
            if let Some(e) = email {
                envs.push(("GIT_COMMITTER_EMAIL", e));
            }
        }
        let args: Vec<&str> = a.iter().map(String::as_str).collect();
        if envs.is_empty() {
            self.run(root, &args)?;
        } else {
            self.run_env(root, &args, &envs)?;
        }
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
        // `git switch <tag>` on modern git requires `--detach` (tags are not
        // branch names). The libgit2 impl always does a detached checkout of
        // the peeled commit, so pass `--detach` here for parity.
        let args = ["switch", "--detach", name];
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
        let bytes = self.run_bytes(root, &args)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn show_file_bytes(&self, root: &Path, rev: &str, path: &Path) -> TgResult<Vec<u8>> {
        // Same spec convention as `show_file` (`<rev>:<path>`); `run_bytes`
        // keeps the blob bytes intact for binary content.
        let spec = format!("{}:{}", rev, path.to_string_lossy());
        let args = ["show", &spec];
        self.run_bytes(root, &args)
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

/// Substring of `s` starting at the Nth whitespace-separated token (0-based).
///
/// Unlike [`nth_field`] this keeps the remainder of the line, so a final
/// column that may contain spaces (rename/copy paths) survives intact.
/// Returns `None` when `s` has fewer than `n + 1` tokens.
fn tail_from_field(s: &str, n: usize) -> Option<&str> {
    let mut count = 0;
    let mut in_token = false;
    for (i, c) in s.char_indices() {
        if c.is_whitespace() {
            in_token = false;
        } else if !in_token {
            in_token = true;
            if count == n {
                return Some(&s[i..]);
            }
            count += 1;
        }
    }
    None
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

/// Map a `for-each-ref` refname to a [`CommitRef`] decoration (issue #12).
///
/// Local branches keep their short name; remote-tracking branches collapse
/// `refs/remotes/<remote>/<name…>` to `<remote>/<name…>`; tags drop the
/// `refs/tags/` prefix. Other namespaces (notes, stash, …) are ignored.
fn parse_ref_name(refname: &str) -> Option<CommitRef> {
    let (kind, name) = if let Some(name) = refname.strip_prefix("refs/heads/") {
        (GitRefKind::Branch, name)
    } else if let Some(rest) = refname.strip_prefix("refs/remotes/") {
        let rest = if rest.is_empty() { return None } else { rest };
        (GitRefKind::Remote, rest)
    } else {
        (GitRefKind::Tag, refname.strip_prefix("refs/tags/")?)
    };
    Some(CommitRef::new(kind, name))
}

/// `%G?` → [`SignatureState`] (issue 17): G verified good, B verified bad,
/// U/E/X present but unverifiable, anything else (N) unsigned.
fn parse_signature_state(code: &str) -> SignatureState {
    match code {
        "G" => SignatureState::Good,
        "B" => SignatureState::Bad,
        "U" | "E" | "X" => SignatureState::Unverified,
        _ => SignatureState::Unsigned,
    }
}

/// Parse one `git diff-tree --name-status` line into a [`Change`].
///
/// Shapes: `X\tpath` and rename/copy `X<score>\told\tnew` (the new path wins;
/// the old path is carried as [`Change::orig_path`]).
fn parse_name_status_line(line: &str) -> Option<Change> {
    let mut parts = line.splitn(3, '\t');
    let code = parts.next()?.trim();
    let status = match code.chars().next()? {
        'A' => ChangeStatus::Added,
        'D' => ChangeStatus::Deleted,
        'R' => ChangeStatus::Renamed,
        'C' => ChangeStatus::Copied,
        // Typechange and anything unexpected read as Modified.
        _ => ChangeStatus::Modified,
    };
    let (path, orig_path) = match status {
        ChangeStatus::Renamed | ChangeStatus::Copied => {
            let old = parts.next()?;
            let new = parts.next()?;
            (new.to_string(), Some(PathBuf::from(old)))
        }
        _ => (parts.next()?.to_string(), None),
    };
    if path.is_empty() {
        return None;
    }
    Some(Change {
        path: PathBuf::from(path),
        status,
        chunks: vec![],
        staged: false,
        unstaged: false,
        orig_path,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use turbogit_domain::model::SignatureState;

    #[test]
    fn signature_state_maps_the_g_question_mark_codes() {
        assert_eq!(parse_signature_state("N"), SignatureState::Unsigned);
        assert_eq!(parse_signature_state("G"), SignatureState::Good);
        assert_eq!(parse_signature_state("B"), SignatureState::Bad);
        assert_eq!(parse_signature_state("U"), SignatureState::Unverified);
        assert_eq!(parse_signature_state("E"), SignatureState::Unverified);
        assert_eq!(parse_signature_state("X"), SignatureState::Unverified);
        assert_eq!(parse_signature_state(""), SignatureState::Unsigned);
    }
}

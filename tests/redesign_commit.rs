//! Issue #11 — Commit tool window redesign tests.
//!
//! Drives the real `turbogit::ui::render()` through `egui_kittest` against
//! temporary git repositories seeded with modified / added / unversioned /
//! conflicted files, asserting painted labels (canonical groups, count
//! badges, M/A/C file-row badges) and public `AppState` transitions only.
//!
//! Harness helpers are local to this file (per issue spec: do not edit
//! `tests/redesign_harness.rs`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use turbogit::engine::{AppEvent, GitExecutor};
use turbogit::model::{Root, RootId, VcsSettings};
use turbogit::state::{AppState, Dialog};

// ---------------------------------------------------------------- helpers --

/// Run `git` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Run `git` without asserting success (for commands that may legitimately
/// fail, e.g. a merge that conflicts).
fn git_unchecked(repo: &Path, args: &[&str]) {
    let _ = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output();
}

struct Repo {
    path: PathBuf,
}

/// Create an initialized temp repository with one base commit on the default
/// branch and repo-local user config so commits work headlessly. The caller
/// keeps `parent` (a `TempDir`) alive for the duration of the test.
fn temp_repo(parent: &Path, name: &str) -> Repo {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    Repo { path }
}

impl Repo {
    fn branch(&self) -> String {
        git(&self.path, &["rev-parse", "--abbrev-ref", "HEAD"])
            .trim()
            .to_string()
    }
    fn subjects(&self) -> Vec<String> {
        git(&self.path, &["log", "--format=%s"])
            .lines()
            .map(str::to_string)
            .collect()
    }
    fn commit_count(&self) -> usize {
        git(&self.path, &["rev-list", "--count", "HEAD"])
            .trim()
            .parse()
            .unwrap()
    }
}

/// Seed one change of each tracked kind: modified (`M`), staged added (`A`)
/// and unversioned (`?`).
fn seed_changes(repo: &Path) {
    std::fs::write(repo.join("base.txt"), "modified content\n").unwrap();
    std::fs::write(repo.join("added.txt"), "new tracked file\n").unwrap();
    git(repo, &["add", "added.txt"]);
    std::fs::write(repo.join("untracked.txt"), "untracked\n").unwrap();
}

/// Create a real merge conflict in `conf.txt` on `repo`'s default branch.
fn seed_conflict(repo: &Path, branch: &str) {
    git(repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("conf.txt"), "side\n").unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "side commit"]);
    git(repo, &["checkout", "-q", branch]);
    std::fs::write(repo.join("conf.txt"), "main line\n").unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "main commit"]);
    git_unchecked(repo, &["merge", "--no-edit", "side"]); // expected to conflict
}

/// Build an `AppState` over the given roots with synchronous status scans
/// (no background threads), so tests are deterministic.
fn app_state(roots: &[PathBuf]) -> AppState {
    let (tx, rx) = crossbeam_channel::unbounded();
    let settings = VcsSettings::default();
    let executor: Arc<dyn GitExecutor> = Arc::new(turbogit::engine::cli::CliExecutor {
        settings: settings.clone(),
    });
    let mut st = AppState {
        project_dir: roots
            .first()
            .and_then(|r| r.parent())
            .map(Path::to_path_buf)
            .unwrap_or_default(),
        executor,
        settings,
        multi: Default::default(),
        tx,
        rx,
        selected_root: None,
        clone_url: String::new(),
        last_error: None,
        ui: Default::default(),
        log_cache: Default::default(),
        ahead_behind: Default::default(),
        recents_config_dir: None,
        dir_picker: None,
        ref_cache: Default::default(),
        files_cache: Default::default(),
    };
    for r in roots {
        let id = RootId(r.clone());
        let status = st.executor.status(r).expect("status scan");
        let current_branch = st.executor.current_branch(r).ok().flatten();
        st.multi.register_root(Root {
            id: id.clone(),
            path: r.clone(),
            remotes: vec![],
            branches: vec![],
            current_branch,
            head: None,
            status,
        });
        if st.selected_root.is_none() {
            st.selected_root = Some(id);
        }
    }
    st
}

/// Drain worker-thread events exactly like `app.rs`, but re-status
/// synchronously after completed ops so tests stay deterministic.
fn drain_events(state: &mut AppState) {
    while let Ok(ev) = state.rx.try_recv() {
        match ev {
            AppEvent::StatusScanned {
                root,
                status: Ok(s),
            } => {
                if let Some(r) = state.multi.roots.iter_mut().find(|r| r.id == root) {
                    r.status = s;
                }
            }
            AppEvent::StatusScanned { .. } => {}
            AppEvent::OpCompleted { label, result } => {
                state.ui.busy = false;
                match result {
                    Ok(()) => {
                        state.ui.toast = Some(format!("✓ {label}"));
                        for root in &mut state.multi.roots {
                            if let Ok(s) = state.executor.status(&root.path) {
                                root.status = s;
                            }
                        }
                    }
                    Err(e) => {
                        state.ui.toast = Some(format!("✗ {label}: {e}"));
                        state.last_error = Some(e.to_string());
                    }
                }
            }
            AppEvent::DiffReady { key, result } => {
                state.ui.diff_loading = false;
                match result {
                    Ok(text) => {
                        state.ui.diff_error = None;
                        state.ui.diff_cache = Some((key, text));
                    }
                    Err(e) => state.ui.diff_error = Some(e.to_string()),
                }
            }
            AppEvent::AheadBehind {
                root,
                ahead,
                behind,
            } => {
                state.ahead_behind.insert(root, (ahead, behind));
            }
            _ => {}
        }
    }
}

/// Headless harness driving the full app UI with event draining per frame.
/// `max_steps` is raised well above the default because the async diff
/// preview legitimately spins (repainting) while `git diff` runs on a
/// worker thread.
fn harness(state: AppState) -> Harness<'static, AppState> {
    Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            drain_events(state);
            turbogit::ui::render(ui, state);
        },
        state,
    )
}

/// Poll `f` until it returns true or the deadline elapses (worker threads run
/// asynchronously, so completion is observed by polling).
fn wait_until<F: Fn() -> bool>(ms: u64, f: F) -> bool {
    let start = Instant::now();
    loop {
        if f() {
            return true;
        }
        if start.elapsed() >= Duration::from_millis(ms) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// The primary Commit action button lives on the same row as
/// "Commit and Push..." — the shell rail/toolbar also contain items labeled
/// "Commit", so disambiguate geometrically (same y-center).
fn commit_action_button<'h>(h: &'h Harness<'_, AppState>) -> egui_kittest::Node<'h> {
    let row_y = h.get_by_label("Commit and Push...").rect().center().y;
    let mut on_row: Vec<_> = h
        .get_all_by_label("Commit")
        .filter(|n| (n.rect().center().y - row_y).abs() < 4.0)
        .collect();
    assert_eq!(on_row.len(), 1, "expected exactly one Commit action button");
    on_row.remove(0)
}

fn commit_button_is_disabled(h: &Harness<'_, AppState>) -> bool {
    commit_action_button(h).accesskit_node().is_disabled()
}

// ------------------------------------------------------------------ tests --

#[test]
fn canonical_groups_count_badges_and_status_rows_paint() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "paint-repo");
    let branch = repo.branch();
    seed_changes(&repo.path);
    seed_conflict(&repo.path, &branch);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    // Canonical collapsible groups with count badges:
    // Default Changelist = base.txt (M) + added.txt (A); conflicts excluded.
    h.get_by_label("Default Changelist (2)");
    h.get_by_label("Unversioned Files (1)");
    h.get_by_label("Merge conflicts (1)");

    // File rows paint with M/A/C badges matching actual file states.
    h.get_by_label("M base.txt");
    h.get_by_label("A added.txt");
    h.get_by_label("? untracked.txt");
    h.get_by_label("C conf.txt");

    // Single-root project: no root sub-groups / select-all checkboxes.
    assert!(
        h.query_by_label("Select all paint-repo").is_none(),
        "select-all must only appear for multi-root projects"
    );
}

#[test]
fn file_row_checkboxes_toggle_commit_inclusion() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "toggle-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    let p = repo.path.join("base.txt");

    assert!(!h.state().ui.selected.contains(&p));

    h.get_by_label("M base.txt").click();
    h.run();
    assert!(
        h.state().ui.selected.contains(&p),
        "checking a row includes the change"
    );

    h.get_by_label("M base.txt").click();
    h.run();
    assert!(
        !h.state().ui.selected.contains(&p),
        "unchecking excludes the change again"
    );
}

#[test]
fn commit_stays_disabled_without_message_or_selection_and_enables_with_both() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "gate-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // Empty message AND empty selection → disabled.
    assert!(commit_button_is_disabled(&h));

    // Message set but selection still empty → still disabled (asserted).
    h.state_mut().ui.commit_message = "has message".into();
    h.run();
    assert!(
        commit_button_is_disabled(&h),
        "Commit must stay disabled while no change is included"
    );

    // Selection made but message cleared → still disabled (asserted).
    h.state_mut().ui.commit_message.clear();
    h.get_by_label("M base.txt").click();
    h.run();
    assert!(
        commit_button_is_disabled(&h),
        "Commit must stay disabled while the message is empty"
    );

    // Both present → enabled.
    h.state_mut().ui.commit_message = "ready".into();
    h.run();
    assert!(
        !commit_button_is_disabled(&h),
        "Commit must enable with a non-empty message and at least one included change"
    );
}

#[test]
fn commit_executes_for_valid_input() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "commit-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("M base.txt").click();
    h.state_mut().ui.commit_message = "issue11: real commit".into();
    h.run();
    commit_action_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"issue11: real commit".to_string())),
        "commit should land in the temp repository, got {:?}",
        repo.subjects()
    );
}

#[test]
fn amend_commits_with_amend_flag() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "amend-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    let before = repo.commit_count();
    assert_eq!(before, 1);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("M base.txt").click();
    h.get_by_label("Amend").click();
    h.state_mut().ui.commit_message = "amended subject".into();
    h.run();
    commit_action_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"amended subject".to_string())
            && repo.commit_count() == before),
        "amend must rewrite HEAD without adding a commit (count={}, subjects={:?})",
        repo.commit_count(),
        repo.subjects()
    );
}

#[test]
fn commit_and_push_chains_commit_then_opens_push_dialog() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "push-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("M base.txt").click();
    h.state_mut().ui.commit_message = "push chain subject".into();
    h.run();
    h.get_by_label("Commit and Push...").click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"push chain subject".to_string())),
        "commit-and-push should first perform the commit"
    );
    assert_eq!(
        h.state().ui.dialog,
        Some(Dialog::Push),
        "commit-and-push must open the push dialog afterwards"
    );
}

#[test]
fn multi_root_shows_root_subgroups_with_select_all() {
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "repo-a");
    let b = temp_repo(parent.path(), "repo-b");
    std::fs::write(a.path.join("base.txt"), "modified a\n").unwrap();
    std::fs::write(b.path.join("other.txt"), "modified b\n").unwrap();

    let mut h = harness(app_state(&[a.path.clone(), b.path.clone()]));

    // Root sub-groups paint with count badges.
    h.get_by_label("repo-a (1)");
    h.get_by_label("repo-b (1)");

    // Select-all for repo-a includes exactly its own files.
    h.get_by_label("Select all repo-a").click();
    h.run();
    let selected = h.state().ui.selected.clone();
    assert!(
        selected.contains(&a.path.join("base.txt")),
        "repo-a select-all should include its file, selected={selected:?}"
    );
    assert!(
        !selected.contains(&b.path.join("other.txt")),
        "repo-a select-all must not include repo-b files, selected={selected:?}"
    );

    // Select-all for repo-b adds its own file too.
    h.get_by_label("Select all repo-b").click();
    h.run();
    let selected = h.state().ui.selected.clone();
    assert!(selected.contains(&a.path.join("base.txt")));
    assert!(selected.contains(&b.path.join("other.txt")));
}

#[test]
fn diff_preview_reflects_selected_file() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "preview-repo");
    seed_changes(&repo.path);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    // Clicking a file row selects it for the preview pane.
    h.get_by_label("base.txt").click();
    h.run();
    assert_eq!(h.state().ui.preview_change, Some(PathBuf::from("base.txt")));
    h.get_by_label("Preview: base.txt");

    // Selecting another row swaps the preview to that file.
    h.get_by_label("added.txt").click();
    h.run();
    assert_eq!(
        h.state().ui.preview_change,
        Some(PathBuf::from("added.txt"))
    );
    h.get_by_label("Preview: added.txt");
}

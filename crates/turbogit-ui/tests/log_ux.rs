//! Issue 17 — Log UX upgrades, UI seam: Load-more pagination, removable
//! path-filter chips, code-change search, upgraded commit details, and ref
//! state decorations. Headless egui_kittest harness over
//! [`turbogit_ui::ui::render`], mirroring `git_log.rs`'s painted-output
//! assertions plus public [`AppState`] transitions.
//!
//! Unlike `git_log.rs` this harness pumps worker events every frame (as
//! `app.rs` does in production) so page fetches dispatched by the UI land
//! during `settle`.

use std::path::{Path, PathBuf};

use egui::Shape;
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable},
};
use tempfile::TempDir;
use turbogit_app::state::{AppState, LOG_PAGE_SIZE};
use turbogit_ui::theme::{configure_style, install_fonts};

// --- Painted-output helpers (mirrors git_log.rs) ------------------------------

fn painted_text(harness: &Harness<'_, AppState>) -> Vec<String> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

#[track_caller]
fn assert_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

#[track_caller]
fn assert_not_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        !texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was unexpectedly painted; painted text:\n{texts:#?}"
    );
}

/// Step frames until the painted output stabilizes.
pub fn settle(harness: &mut Harness<'_, AppState>) {
    let mut prev = String::new();
    for _ in 0..30 {
        harness.step();
        let fingerprint = format!("{:?}", painted_text(harness));
        if fingerprint == prev {
            return;
        }
        prev = fingerprint;
    }
    panic!("log layout did not settle within 30 frames");
}

/// Step frames until `needle` is painted. Unlike [`settle`] this survives
/// stability races against in-flight worker fetches: a stable frame does
/// not mean the pending fetch has landed yet.
#[track_caller]
pub fn settle_until(harness: &mut Harness<'_, AppState>, needle: &str) {
    for _ in 0..100 {
        harness.step();
        if painted_text(harness).iter().any(|t| t.contains(needle)) {
            settle(harness);
            return;
        }
    }
    panic!("`{needle}` never appeared within 100 frames");
}

// --- Fixture -------------------------------------------------------------------

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    std::fs::write(dir.join(name), body).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
    repo: PathBuf,
    /// Newest commit of the first page (index `LOG_PAGE_SIZE`).
    newest: String,
    /// The oldest commit — hidden until Load more (index 0).
    oldest: String,
}
/// A single-root project with `LOG_PAGE_SIZE + 1` commits so exactly one
/// page hides the oldest commit behind Load more.
fn seeded_project() -> Seed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    // Seed the page-sized history in ONE `git fast-import` batch instead of
    // `LOG_PAGE_SIZE + 1` per-commit subprocesses (each ~150ms under
    // Git-for-Windows; a 201-commit loop costs ~30s and starves the async
    // fetch budget in the harness). The commits are still real objects on
    // `main`, so the production `fetch_log` / `load_more_log` paths drive
    // them exactly as with a hand-seeded history.
    let mut stream = String::new();
    // Helper to emit a `data <len>\n<bytes>` block with the exact byte
    // length (multi-digit messages/files break hardcoded lengths).
    let data = |stream: &mut String, s: &str| {
        stream.push_str(&format!("data {}\n{s}", s.len()));
    };
    stream.push_str("commit refs/heads/main\n");
    stream.push_str("mark :1\n");
    stream.push_str("author t <t@t> 1000000000 +0000\n");
    stream.push_str("committer t <t@t> 1000000000 +0000\n");
    data(&mut stream, "c0\n");
    stream.push_str("M 644 inline f.txt\n");
    data(&mut stream, "body 0\n");
    let mut prev: u64 = 1;
    for i in 1..=LOG_PAGE_SIZE {
        stream.push_str(&format!("commit refs/heads/main\nmark :{}\n", i + 1));
        stream.push_str(&format!("author t <t@t> {} +0000\n", 1_000_000_000 + i));
        stream.push_str(&format!("committer t <t@t> {} +0000\n", 1_000_000_000 + i));
        data(&mut stream, &format!("c{i}\n"));
        stream.push_str(&format!("from :{prev}\n"));
        stream.push_str("M 644 inline f.txt\n");
        data(&mut stream, &format!("body {i}\n"));
        prev = i as u64 + 1;
    }
    let mut child = std::process::Command::new("git")
        .arg("fast-import")
        .current_dir(&repo)
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn git fast-import");
    use std::io::Write as _;
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stream.as_bytes())
        .expect("write fast-import stream");
    let out = child.wait_with_output().expect("wait fast-import");
    assert!(
        out.status.success(),
        "git fast-import failed with {:?}; stderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    // Point HEAD at the new main and re-read the boundary commits.
    git(&repo, &["reset", "-q", "--hard", "main"]);
    let mut oldest = String::new();
    let mut newest = String::new();
    for i in 0..=LOG_PAGE_SIZE {
        let id = git(
            &repo,
            &["rev-parse", &format!("HEAD~{}", LOG_PAGE_SIZE - i)],
        )
        .trim()
        .to_string();
        if i == 0 {
            oldest = id.clone();
        }
        if i == LOG_PAGE_SIZE {
            newest = id.clone();
        }
    }
    Seed {
        _tmp: tmp,
        project,
        repo,
        newest,
        oldest,
    }
}

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

/// Harness with the Log tool window active over `state`'s project. The log
/// is primed through the production page-sized fetch path
/// (`AppState::fetch_log`); worker events are pumped every frame.
fn harness_over(state: AppState) -> Harness<'static, AppState> {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(
        1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH,
        800.0,
    ));
    settle(&mut harness);
    harness
}

/// Harness with the Log tool window active over a single-root project. The
/// log is primed through the production page-sized fetch path
/// (`AppState::fetch_log`); worker events are pumped every frame.
fn log_harness(seed: &Seed) -> Harness<'static, AppState> {
    let mut state = AppState::for_roots(&seed.project, std::slice::from_ref(&seed.repo));
    assert_eq!(state.multi.roots.len(), 1, "one root registered");
    let root_id = state.multi.roots[0].id.clone();
    state.fetch_log(root_id);
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);
    // The page fetch lands on a worker thread; await the newest commit so
    // every test starts from a settled, populated log.
    settle_until(&mut harness, &short(&seed.newest));
    harness
}

// --- Issue 17: Load more pagination ---------------------------------------------

#[test]
fn commit_list_paginates_via_load_more() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    // First page: the newest commit is visible, the oldest is not, and the
    // status line + Load more affordance are painted.
    assert_painted(&harness, &short(&seed.newest));
    assert_not_painted(&harness, &short(&seed.oldest));
    assert_painted(&harness, &format!("{} shown", LOG_PAGE_SIZE));
    assert_painted(&harness, "Load more");

    harness.get_by_label("Load more").click();
    settle_until(&mut harness, &short(&seed.oldest));

    // The widened fetch reveals the whole history and Load more goes away.
    assert_painted(&harness, &short(&seed.oldest));
    assert_painted(&harness, &format!("{} shown", LOG_PAGE_SIZE + 1));
    assert_not_painted(&harness, "Load more");
    assert_eq!(
        harness.state().ui.log_page,
        1,
        "Load more must advance the page index"
    );
}

// --- Issue 17: removable path-filter chip ---------------------------------------

/// A quick two-commit single-root project: `c1` touches `f.txt`, `c2`
/// touches `g.txt` — scoping to `f.txt` must hide `c2`.
struct SmallSeed {
    _tmp: TempDir,
    project: PathBuf,
    repo: PathBuf,
    /// The commit touching `f.txt` — the only one visible once scoped.
    c1: String,
    /// The commit touching `g.txt`.
    #[allow(dead_code)]
    c2: String,
}

fn small_project() -> SmallSeed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    let c1 = commit_file(&repo, "f.txt", "one\n", "alpha: first");
    let c2 = commit_file(&repo, "g.txt", "two\n", "alpha: second");
    SmallSeed {
        _tmp: tmp,
        project,
        repo,
        c1,
        c2,
    }
}

/// Drive the full user path: select `row_label`, right-click its changed-file
/// entry `file`, and activate "Show history for file..." in the context menu.
fn scope_log_to_file(harness: &mut Harness<'_, AppState>, row_label: &str, file: &str) {
    harness.get_by_label(row_label).click();
    settle(harness);
    harness.get_by_label(file).click_secondary();
    settle(harness);
    harness.get_by_label("Show history for file...").click();
    settle(harness);
}

#[test]
fn path_scope_renders_as_a_removable_chip() {
    let seed = small_project();
    let mut state = AppState::for_roots(&seed.project, std::slice::from_ref(&seed.repo));
    state.fetch_log(state.multi.roots[0].id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);

    // No chip while unscoped.
    assert_not_painted(&harness, "Path filter:");

    // Scope via the changed-files context menu…
    scope_log_to_file(
        &mut harness,
        &format!("{} alpha: first", short(&seed.c1)),
        "f.txt",
    );
    assert!(harness.state().ui.log_path_scope.is_some());

    // …the scope renders as a chip in the log header.
    assert_painted(&harness, "Path filter:");
    // Only the scoped commit's rows remain.
    assert_painted(&harness, "alpha: first");
    assert_not_painted(&harness, "alpha: second");

    // The chip's × removes the scope; the full log returns.
    harness.get_by_label("Remove path filter").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.log_path_scope, None);
    assert_not_painted(&harness, "Path filter:");
}

// --- Issue 17: search covers code change -----------------------------------------

#[test]
fn commit_search_finds_code_changes_not_just_metadata() {
    let seed = small_project();
    let mut state = AppState::for_roots(&seed.project, std::slice::from_ref(&seed.repo));
    state.fetch_log(state.multi.roots[0].id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);

    // Searching a string that only appears in a commit's *content* ("one"
    // is f.txt's body — no message, hash, or author contains it) must
    // surface that commit via the pickaxe, and hide the one that never
    // touched the string.
    harness.get_by_label("Search commits").click();
    harness.get_by_label("Search commits").type_text("one");
    settle(&mut harness);

    assert_painted(&harness, "alpha: first");
    assert_not_painted(&harness, "alpha: second");
}

// --- Issue 17: commit details — committer, signature, parents, copy hash ----

#[test]
fn details_show_committer_copy_hash_and_clickable_parents() {
    let seed = small_project();
    let mut state = AppState::for_roots(&seed.project, std::slice::from_ref(&seed.repo));
    state.fetch_log(state.multi.roots[0].id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);

    harness
        .get_by_label(&format!("{} alpha: second", short(&seed.c2)))
        .click();
    settle(&mut harness);

    // Committer line (screen 09) — and an unsigned commit must not claim a
    // signature.
    assert_painted(&harness, "Committer:");
    assert_not_painted(&harness, "signed ✓");

    // Copy hash affordance with painted feedback (the toast).
    assert_painted(&harness, "Copy hash");
    harness.get_by_label("Copy hash").click();
    settle(&mut harness);
    assert_painted(&harness, "Copied");

    // Parent hashes are links: clicking one selects that commit.
    harness.get_by_label(&short(&seed.c1)).click();
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(seed.c1.as_str()),
        "clicking a parent hash must select the parent commit"
    );
}

// --- Issue 17: remote gone + tag pushed states in the branches pane -------------

/// A repo with a bare `origin`: `main` was pushed (and then deleted on the
/// remote without a local prune), tag `v1.0` was pushed, `v2.0` never left
/// the machine.
fn ref_project() -> (TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    let repo = project.join("alpha");
    let origin = tmp.path().join("origin.git");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("f.txt"), "one\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "c1"]);
    git(&repo, &["tag", "v1.0"]);
    git(
        &repo,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    git(
        &repo,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    git(&repo, &["push", "-q", "origin", "v1.0"]);
    git(&repo, &["tag", "v2.0"]);
    // Delete the branch inside the bare remote: a `git push :main` would
    // prune the local remote-tracking ref, leaving nothing to mark gone.
    git(&origin, &["update-ref", "-d", "refs/heads/main"]);
    (tmp, project, repo)
}

#[test]
fn branches_pane_marks_remote_gone_and_tag_push_states() {
    let (_tmp, project, repo) = ref_project();
    let mut state = AppState::for_roots(&project, &[repo]);
    state.fetch_log(state.multi.roots[0].id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);

    // The pane is now the shared repo-grouped tree (branch-tree extraction,
    // plan step 4): remote branches group under their `origin` header with
    // the prefix stripped, still carrying the gone marker; the TAGS rows
    // carry their push state (screen 09) via the view model's TagLeaf.
    // Tags start collapsed like in the Branches window — open the group.
    // Remote groups start collapsed too (the Log pane's default) — open `origin`.
    harness.get_by_label("Tags").click();
    click_button(&mut harness, "origin");
    settle(&mut harness);
    assert_painted(&harness, "origin");
    assert_painted(&harness, "gone");
    assert_painted(&harness, "v1.0");
    assert_painted(&harness, "pushed");
    assert_painted(&harness, "v2.0");
    assert_painted(&harness, "local only");
}

// --- Branch-tree extraction (plan step 4): ref-scoped graph filtering --------

/// Click the `Role::Button` labelled exactly `label`. Branch rows carry their
/// name twice in the a11y tree (the row's label and the inner text's value),
/// so a bare `get_by_label` would be ambiguous.
#[track_caller]
fn click_button(harness: &mut Harness<'_, AppState>, label: &str) {
    let node = harness
        .query_all_by_label_contains(label)
        .find(|n| {
            n.accesskit_node().role() == egui::accesskit::Role::Button
                && n.accesskit_node().label().is_some_and(|l| l == label)
        })
        .unwrap_or_else(|| panic!("no button labelled {label:?}"));
    node.click();
}

/// Extend [`ref_project`] with a second branch: `topic` carries one commit
/// `main` does not, so scoping the graph to `main` must hide it.
fn ref_project_with_topic() -> (TempDir, PathBuf, PathBuf, String) {
    let (tmp, project, repo) = ref_project();
    git(&repo, &["checkout", "-q", "-b", "topic"]);
    let c2 = commit_file(&repo, "g.txt", "two\n", "topic: side commit");
    git(&repo, &["checkout", "-q", "main"]);
    (tmp, project, repo, c2)
}

#[test]
fn activating_a_ref_scopes_the_graph_to_its_history() {
    let (_tmp, project, repo, topic_commit) = ref_project_with_topic();
    let mut state = AppState::for_roots(&project, std::slice::from_ref(&repo));
    let root_id = state.multi.roots[0].id.clone();
    state.fetch_log(root_id.clone());
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);

    // Unscoped: main's fetched page — the side commit lives only on topic.
    settle_until(&mut harness, "c1");
    assert_not_painted(&harness, "topic: side commit");

    // Activating the `topic` row in the branches pane scopes the graph.
    click_button(&mut harness, "topic");
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.log_ref_scope,
        Some((root_id.clone(), "topic".to_string())),
        "row activation sets the ref scope"
    );
    // The scope renders as a removable chip, same gesture as the path scope.
    assert_painted(&harness, "Ref filter:");
    // The graph narrows to the ref's history: the scoped listing fetches
    // through the engine seam and surfaces the side commit.
    settle_until(&mut harness, "topic: side commit");
    let _ = topic_commit;

    // The chip's × clears the scope; the main page returns.
    harness.get_by_label("Remove ref filter").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.log_ref_scope, None);
    assert_not_painted(&harness, "Ref filter:");
    assert_painted(&harness, "c1");
}

#[test]
fn switching_repository_drops_the_ref_scope() {
    // Two repos: activating beta's `topic` scopes the graph, then switching
    // the selected repository to alpha must clear it (plan D9).
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    let beta = project.join("beta");
    std::fs::create_dir_all(&alpha).unwrap();
    std::fs::create_dir_all(&beta).unwrap();
    for dir in [&alpha, &beta] {
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["config", "user.email", "t@t"]);
        git(dir, &["config", "user.name", "t"]);
    }
    commit_file(&alpha, "a.txt", "a\n", "alpha commit");
    commit_file(&beta, "b0.txt", "x\n", "beta root commit");
    git(&beta, &["checkout", "-q", "-b", "topic"]);
    commit_file(&beta, "b.txt", "b\n", "topic commit");
    let _beta_id = git(&beta, &["rev-parse", "HEAD"]).trim().to_string();

    let mut state = AppState::for_roots(&project, &[alpha, beta]);
    let alpha_root = state.multi.roots[0].id.clone();
    let beta_root = state.multi.roots[1].id.clone();
    for r in &state.multi.roots.clone() {
        state.fetch_log(r.id.clone());
    }
    state.ui.tab = turbogit_app::state::Tab::Log;
    let mut harness = harness_over(state);
    settle_until(&mut harness, "beta root commit");

    // Activate beta's `topic` row: the scope sets and follows the repo.
    click_button(&mut harness, "topic");
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.log_ref_scope,
        Some((beta_root.clone(), "topic".to_string()))
    );
    assert_eq!(
        harness.state().selected_root.as_ref(),
        Some(&beta_root),
        "the selection follows the scoped ref's repository"
    );

    // Switching the selected repository drops the scope.
    harness.state_mut().selected_root = Some(alpha_root);
    settle(&mut harness);
    assert_eq!(
        harness.state().ui.log_ref_scope,
        None,
        "the scope must never outlive its repository"
    );
}

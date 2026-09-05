//! Issue 31 — tag dialog upgrade (screen 16).
//!
//! Drives the real `turbogit_ui::ui::render()` through `egui_kittest` against
//! temporary git repositories, asserting painted labels, public `AppState`
//! transitions, and the exact `TagSpec` / push sequence handed to the
//! executor boundary (via [`RecordingExecutor`]).
//!
//! Covered behaviors:
//! - the screen-16 groups paint: TAG NAME, TARGET (defaulting to HEAD),
//!   TYPE (Lightweight / Annotated), MESSAGE, TAGGER, GPG signing, OPTIONS
//! - the tag name validates live ("✓ valid" / a clear reason: duplicates,
//!   illegal characters) and an invalid name disables Create
//! - the Lightweight type skips the annotated-only fields
//! - the TARGET picker lists recent commits and tags any of them, not just
//!   the branch head
//! - Create dispatches the full `TagSpec` through the executor port and
//!   closes the dialog
//! - "Push tag to origin immediately" pushes the new tag to origin after
//!   creating it and reports the outcome

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::kittest::{NodeT, Queryable as _};
use egui_kittest::{Harness, Node};
use test_support::RecordingExecutor;
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{RootId, TagSpec, VcsSettings};
use turbogit_engine::cli::CliExecutor;

// ---------------------------------------------------------------- helpers --

/// Run `git <args>` in `dir`, asserting success; returns stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Append a line to `file.txt`, stage, commit; returns the new HEAD SHA.
fn commit(dir: &Path, msg: &str) -> String {
    let file = dir.join("file.txt");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .expect("opening work file");
    use std::io::Write;
    writeln!(f, "{msg}").expect("appending work file");
    drop(f);
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// A repo with two commits on `main`; returns `(guard, repo, [c1, c2])`.
fn repo_two_commits() -> (tempfile::TempDir, PathBuf, Vec<String>) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    let c1 = commit(&repo, "one");
    let c2 = commit(&repo, "two");
    (tmp, repo, vec![c1, c2])
}

/// AppState with a recording executor wrapped around the real CLI engine.
fn app_state_recording(project: &Path, roots: &[PathBuf]) -> (AppState, Arc<RecordingExecutor>) {
    let exec: Arc<RecordingExecutor> = Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
        settings: VcsSettings::default(),
    })));
    let state = AppState::for_roots(project, roots)
        .with_executor(exec.clone())
        .with_settings(VcsSettings::default());
    (state, exec)
}

/// Headless harness driving the full app UI with event draining per frame.
fn harness(state: AppState) -> Harness<'static, AppState> {
    Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    )
}

/// Open the Tag dialog on the given root and let the egui window geometry
/// settle (the window re-frames for several frames after opening, so clicks
/// must wait for stable coordinates).
fn open_tag(h: &mut Harness<'_, AppState>, root: &Path) {
    h.state_mut().selected_root = Some(RootId(root.to_path_buf().into()));
    h.state_mut().ui.dialog = Some(Dialog::Tag);
    settle(h);
}

/// Step frames until painted button geometry is stable for 3 consecutive
/// frames.
fn settle(h: &mut Harness<'_, AppState>) {
    let mut stable = 0;
    let mut prev = String::new();
    for _ in 0..300 {
        h.step();
        std::thread::sleep(Duration::from_millis(5));
        let fp = format!(
            "{:?}",
            h.query_all_by_role(egui::accesskit::Role::Button)
                .map(|n| (
                    n.accesskit_node().label().as_deref().map(str::to_owned),
                    n.rect(),
                ))
                .collect::<Vec<_>>()
        );
        if fp == prev {
            stable += 1;
            if stable >= 3 {
                return;
            }
        } else {
            stable = 0;
            prev = fp;
        }
    }
    panic!("tag dialog layout did not settle within 300 frames");
}

/// Type `text` into the dialog field with the given accessible label.
fn type_into_field(h: &mut Harness<'_, AppState>, label: &str, text: &str) {
    let field = h.get_by_label(label);
    field.focus();
    field.type_text(text);
    settle(h);
}

/// Click a dialog button and step frames until the click lands.
fn click_dialog_button(h: &mut Harness<'_, AppState>, label: &str) {
    h.query_all_by_label(label)
        .next()
        .unwrap_or_else(|| panic!("no button labeled {label}"))
        .click();
    settle(h);
}

/// Step frames until `pred` holds on public state (async op completion,
/// toasts).
fn pump_until(h: &mut Harness<'_, AppState>, what: &str, mut pred: impl FnMut(&AppState) -> bool) {
    for _ in 0..600 {
        if pred(h.state()) {
            return;
        }
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for: {what}");
}

/// Poll until `f` is true or the deadline elapses.
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

// ------------------------------------------------------------------ tests --

#[test]
fn dialog_paints_the_screen_16_groups() {
    let (tmp, repo, _shas) = repo_two_commits();
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_tag(&mut h, &repo);

    assert!(h.query_all_by_label("TAG NAME").next().is_some());
    assert!(h.query_all_by_label("TARGET").next().is_some());
    // The picker row defaults to HEAD on the current branch.
    assert!(h.query_all_by_label("HEAD on main").next().is_some());
    assert!(h.query_all_by_label("TYPE").next().is_some());
    assert!(h.query_all_by_label("Lightweight").next().is_some());
    assert!(h.query_all_by_label("Annotated").next().is_some());
    // Annotated is the default type, so the annotated-only fields paint.
    assert!(h.query_all_by_label("MESSAGE").next().is_some());
    assert!(h.query_all_by_label("TAGGER").next().is_some());
    assert!(h.query_all_by_label("Sign with GPG key").next().is_some());
    assert!(h.query_all_by_label("OPTIONS").next().is_some());
    assert!(
        h.query_all_by_label("Push tag to origin immediately")
            .next()
            .is_some()
    );
    assert!(h.query_all_by_label("Create tag").next().is_some());
}

#[test]
fn name_validation_is_live() {
    let (tmp, repo, _shas) = repo_two_commits();
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_tag(&mut h, &repo);

    // An empty name is not yet valid.
    assert!(
        h.query_all_by_label("A tag name is required")
            .next()
            .is_some(),
        "the empty name should show its reason"
    );

    type_into_field(&mut h, "Tag name", "v0.9.0");
    assert!(h.query_all_by_label("✓ valid").next().is_some());

    // Illegal characters name the violated rule.
    h.state_mut().ui.dlg.tag_name = "has space".into();
    h.run();
    assert!(
        h.query_all_by_label("cannot contain a space")
            .next()
            .is_some()
    );
}

#[test]
fn a_duplicate_name_shows_the_reason_and_disables_create() {
    let (tmp, repo, _shas) = repo_two_commits();
    git(&repo, &["tag", "v0.9.0"]);
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_tag(&mut h, &repo);

    type_into_field(&mut h, "Tag name", "v0.9.0");
    assert!(
        h.query_all_by_label("A tag named 'v0.9.0' already exists")
            .next()
            .is_some(),
        "the duplicate reason should paint"
    );
    assert!(
        dialog_button(&h, "Create tag")
            .accesskit_node()
            .is_disabled(),
        "an invalid name must disable Create"
    );
}

fn dialog_button<'h>(h: &'h Harness<'_, AppState>, label: &'h str) -> Node<'h> {
    h.query_all_by_label(label)
        .next()
        .unwrap_or_else(|| panic!("no button labeled {label}"))
}

#[test]
fn lightweight_type_hides_the_annotated_fields() {
    let (tmp, repo, _shas) = repo_two_commits();
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_tag(&mut h, &repo);

    assert!(h.query_all_by_label("MESSAGE").next().is_some());
    dialog_button(&h, "Lightweight").click();
    h.run();
    assert!(h.query_all_by_label("MESSAGE").next().is_none());
    assert!(h.query_all_by_label("TAGGER").next().is_none());
    assert!(
        h.query_all_by_label("Sign with GPG key").next().is_none(),
        "a lightweight tag has nothing to sign"
    );
}

#[test]
fn the_target_picker_lists_recent_commits_and_tags_one() {
    let (tmp, repo, shas) = repo_two_commits();
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_tag(&mut h, &repo);

    // Change… expands the picker; the two commits are listed by sha+subject.
    let row1 = format!("{}  one", &shas[0][..7]);
    let row2 = format!("{}  two", &shas[1][..7]);
    click_dialog_button(&mut h, "Change…");
    assert!(h.query_all_by_label(row1.as_str()).next().is_some());
    assert!(h.query_all_by_label(row2.as_str()).next().is_some());

    // Pick the first commit as the tag point.
    click_dialog_button(&mut h, row1.as_str());
    assert!(
        h.query_all_by_label(row1.as_str()).next().is_some(),
        "the picked commit should show in the TARGET row"
    );
}

#[test]
fn create_dispatches_the_spec_and_closes_the_dialog() {
    let (tmp, repo, _shas) = repo_two_commits();
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_tag(&mut h, &repo);

    type_into_field(&mut h, "Tag name", "v0.9.0");
    type_into_field(&mut h, "Tag message", "Release 0.9.0");
    click_dialog_button(&mut h, "Create tag");

    let want = TagSpec {
        name: "v0.9.0".into(),
        target: None,
        message: Some("Release 0.9.0".into()),
        tagger: None,
        sign: false,
    };
    let dispatched = wait_until(5_000, || {
        exec.recorded().iter().any(
            |c| matches!(c, test_support::RecordedCall::TagCreate { spec, .. } if *spec == want),
        )
    });
    assert!(
        dispatched,
        "expected TagCreate {want:?}, got {:?}; dialog={:?} toast={:?}",
        exec.recorded(),
        h.state().ui.dialog,
        h.state().ui.toast
    );
    // The tag landed and the dialog closed.
    assert_eq!(git(&repo, &["tag", "-l"]).trim(), "v0.9.0");
    assert!(wait_until(2_000, || h.state().ui.dialog.is_none()));
}

#[test]
fn tagger_and_gpg_signing_reach_the_dispatched_spec() {
    let (tmp, repo, _shas) = repo_two_commits();
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_tag(&mut h, &repo);

    type_into_field(&mut h, "Tag name", "signed-release");
    type_into_field(&mut h, "Tag message", "signed");
    type_into_field(&mut h, "Tagger", "Stink Ma <stink@turbogit.dev>");
    click_dialog_button(&mut h, "Sign with GPG key");
    click_dialog_button(&mut h, "Create tag");

    let want = TagSpec {
        name: "signed-release".into(),
        target: None,
        message: Some("signed".into()),
        tagger: Some("Stink Ma <stink@turbogit.dev>".into()),
        sign: true,
    };
    assert!(
        wait_until(5_000, || exec.recorded().iter().any(
            |c| matches!(c, test_support::RecordedCall::TagCreate { spec, .. } if *spec == want)
        )),
        "expected TagCreate {want:?}, got {:?}",
        exec.recorded()
    );
}

#[test]
fn a_lightweight_tag_skips_message_and_signing_in_the_spec() {
    let (tmp, repo, _shas) = repo_two_commits();
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_tag(&mut h, &repo);

    // A stale message buffer from a previous visit must not leak into a
    // lightweight tag.
    h.state_mut().ui.dlg.tag_msg = "leftover".into();
    click_dialog_button(&mut h, "Lightweight");
    type_into_field(&mut h, "Tag name", "plain");
    click_dialog_button(&mut h, "Create tag");

    let want = TagSpec {
        name: "plain".into(),
        target: None,
        message: None,
        tagger: None,
        sign: false,
    };
    assert!(
        wait_until(5_000, || exec.recorded().iter().any(
            |c| matches!(c, test_support::RecordedCall::TagCreate { spec, .. } if *spec == want)
        )),
        "expected TagCreate {want:?}, got {:?}",
        exec.recorded()
    );
}

#[test]
fn push_immediately_pushes_to_origin_and_reports_it() {
    let (tmp, repo, _shas) = repo_two_commits();
    // A bare remote named origin already holding main.
    let bare = tmp.path().join("origin.git");
    git(
        tmp.path(),
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            &bare.to_string_lossy(),
        ],
    );
    git(&repo, &["remote", "add", "origin", &bare.to_string_lossy()]);
    git(&repo, &["push", "-q", "origin", "main"]);

    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_tag(&mut h, &repo);

    type_into_field(&mut h, "Tag name", "v0.9.0");
    click_dialog_button(&mut h, "Push tag to origin immediately");
    click_dialog_button(&mut h, "Create tag");

    pump_until(&mut h, "the TagPush dispatch and success toast", |s| {
        exec.recorded().iter().any(|c| {
            matches!(
                c,
                test_support::RecordedCall::TagPush { remote, name, .. }
                    if remote == "origin" && name.as_deref() == Some("v0.9.0")
            )
        }) && matches!(&s.ui.toast, Some(t) if t.kind == turbogit_app::state::ToastKind::Success)
    });
    // The remote itself holds the tag.
    let remote_tags = git(&bare, &["tag", "-l"]);
    assert_eq!(remote_tags.trim(), "v0.9.0");
}

#[test]
fn a_push_failure_reports_the_outcome_with_the_created_context() {
    let (tmp, repo, _shas) = repo_two_commits();
    // A remote that is not a repository — the tag exists locally, the push
    // fails, and the outcome is reported with the created-tag context.
    let not_a_repo = tmp.path().join("nowhere");
    std::fs::create_dir_all(&not_a_repo).unwrap();
    git(
        &repo,
        &["remote", "add", "origin", &not_a_repo.to_string_lossy()],
    );

    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_tag(&mut h, &repo);

    type_into_field(&mut h, "Tag name", "v0.9.0");
    click_dialog_button(&mut h, "Push tag to origin immediately");
    click_dialog_button(&mut h, "Create tag");

    pump_until(
        &mut h,
        "the failure toast naming the created tag and the push failure",
        |s| {
            matches!(&s.ui.toast, Some(t)
                if t.kind == turbogit_app::state::ToastKind::Error
                    && t.message.contains("v0.9.0 was created")
                    && t.message.contains("pushing to origin failed"))
        },
    );
}

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use egui_kittest::kittest::{NodeT, Queryable as _};
use egui_kittest::{Harness, Node};
use test_support::RecordingExecutor;
use test_support::git_seed::{commit as seed_commit, git};
use test_support::harness::{KITTEST_DEFAULT_BOX, filled_rects, shell_harness_over_unstyled};
use test_support::wait_until;
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{RootId, TagSpec, VcsSettings};
use turbogit_engine::cli::CliExecutor;

fn append_commit(dir: &Path, msg: &str) -> String {
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
    seed_commit(dir, "file.txt", &format!("{msg}\n"), msg);
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn repo_two_commits() -> (tempfile::TempDir, PathBuf, Vec<String>) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    git(
        tmp.path(),
        &["init", "-q", "-b", "main", repo.to_str().unwrap()],
    );
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    let c1 = append_commit(&repo, "one");
    let c2 = append_commit(&repo, "two");
    (tmp, repo, vec![c1, c2])
}

fn app_state_recording(project: &Path, roots: &[PathBuf]) -> (AppState, Arc<RecordingExecutor>) {
    let exec: Arc<RecordingExecutor> = Arc::new(RecordingExecutor::new(Arc::new(CliExecutor {
        settings: VcsSettings::default(),
    })));
    let state = AppState::for_roots(project, roots)
        .with_executor(exec.clone())
        .with_settings(VcsSettings::default());
    (state, exec)
}

fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, KITTEST_DEFAULT_BOX, 1024)
}

/// Open the Tag dialog on the given root and let the egui window geometry
/// settle (the window re-frames for several frames after opening, so clicks
/// must wait for stable coordinates).
fn open_tag(h: &mut Harness<'_, AppState>, root: &Path) {
    h.state_mut().selected_root = Some(RootId(root.to_path_buf().into()));
    h.state_mut().ui.dialog = Some(Dialog::Tag);
    settle(h);
}

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

fn type_into_field(h: &mut Harness<'_, AppState>, label: &str, text: &str) {
    let field = h.get_by_label(label);
    field.focus();
    field.type_text(text);
    settle(h);
}

fn click_dialog_button(h: &mut Harness<'_, AppState>, label: &str) {
    h.query_all_by_label(label)
        .next()
        .unwrap_or_else(|| panic!("no button labeled {label}"))
        .click();
    settle(h);
}

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

#[test]
fn dialog_paints_the_screen_16_groups() {
    let (tmp, repo, _shas) = repo_two_commits();
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_tag(&mut h, &repo);

    assert!(h.query_all_by_label("TAG NAME").next().is_some());
    assert!(h.query_all_by_label("TARGET").next().is_some());
    assert!(h.query_all_by_label("HEAD on main").next().is_some());
    assert!(h.query_all_by_label("TYPE").next().is_some());
    assert!(h.query_all_by_label("Lightweight").next().is_some());
    assert!(h.query_all_by_label("Annotated").next().is_some());
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
fn tag_footer_preserves_cancel_create_order_minimum_targets_and_divider() {
    let (tmp, repo, _shas) = repo_two_commits();
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_tag(&mut h, &repo);

    let cancel = h.get_by_label("Cancel");
    let create = h
        .query_all_by_label("Create tag")
        .last()
        .expect("Create tag button");
    assert!(cancel.rect().left() < create.rect().left());
    assert!(cancel.rect().height() >= 28.0);
    assert!(create.rect().height() >= 28.0);
    assert!(
        filled_rects(&h)
            .into_iter()
            .any(|(rect, fill)| fill == turbogit_ui::theme::Palette::LINE
                && (rect.height() - 1.0).abs() < 0.01
                && rect.top() < create.rect().top()),
        "the shared footer divider must be present above the actions"
    );
}

#[test]
fn name_validation_is_live() {
    let (tmp, repo, _shas) = repo_two_commits();
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_tag(&mut h, &repo);

    assert!(
        h.query_all_by_label("A tag name is required")
            .next()
            .is_some(),
        "the empty name should show its reason"
    );

    type_into_field(&mut h, "Tag name", "v0.9.0");
    assert!(h.query_all_by_label("✓ valid").next().is_some());

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

    let row1 = format!("{}  one", &shas[0][..7]);
    let row2 = format!("{}  two", &shas[1][..7]);
    click_dialog_button(&mut h, "Change…");
    assert!(h.query_all_by_label(row1.as_str()).next().is_some());
    assert!(h.query_all_by_label(row2.as_str()).next().is_some());

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

    assert!(
        wait_until(5_000, || exec
            .recorded()
            .iter()
            .any(|c| matches!(c, test_support::RecordedCall::TagCreate { .. }))),
        "no TagCreate was dispatched; calls: {:?}",
        exec.recorded()
    );
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
fn cancelling_creates_no_tag_and_answers_nothing() {
    let (tmp, repo, _shas) = repo_two_commits();
    let head_before = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    open_tag(&mut h, &repo);

    type_into_field(&mut h, "Tag name", "v9.9.9");
    click_dialog_button(&mut h, "Cancel");

    assert!(h.state().ui.dialog.is_none(), "the dialog went");
    assert_eq!(git(&repo, &["tag", "-l"]).trim(), "", "no tag was created");
    assert_eq!(
        git(&repo, &["rev-parse", "HEAD"]).trim(),
        head_before,
        "and the repository did not move",
    );
    assert!(
        !exec
            .recorded()
            .iter()
            .any(|c| matches!(c, test_support::RecordedCall::TagCreate { .. })),
        "nothing was dispatched at all: {:?}",
        exec.recorded()
    );
}

#[test]
fn push_immediately_pushes_to_origin_and_reports_it() {
    let (tmp, repo, _shas) = repo_two_commits();
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
    let remote_tags = git(&bare, &["tag", "-l"]);
    assert_eq!(remote_tags.trim(), "v0.9.0");
}

#[test]
fn a_push_failure_reports_the_outcome_with_the_created_context() {
    let (tmp, repo, _shas) = repo_two_commits();
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

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable as _};
use test_support::git_seed::git;
use test_support::harness::{KITTEST_DEFAULT_BOX, filled_rects, shell_harness_over_unstyled};
use test_support::{RecordedCall, RecordingExecutor, wait_until};
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;

fn commit(dir: &Path, msg: &str) {
    let file = dir.join("file.txt");
    let mut body = std::fs::read_to_string(&file).unwrap_or_default();
    body.push_str(msg);
    body.push('\n');
    test_support::git_seed::commit(dir, "file.txt", &body, msg);
}

fn fresh_repo(tmp: &Path, name: &str) -> PathBuf {
    let repo = tmp.join(name);
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    commit(&repo, "seed");
    repo
}

fn add_divergent_origin(repo: &Path) {
    git(
        repo,
        &["remote", "add", "origin", "https://fetch.git/repo.git"],
    );
    git(
        repo,
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            "https://push.git/repo.git",
        ],
    );
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
    panic!("layout did not settle within 300 frames");
}

fn open_popup(h: &mut Harness<'_, AppState>) {
    h.state_mut().ui.branches_popup = true;
    settle(h);
}

fn open_manager(h: &mut Harness<'_, AppState>) {
    open_popup(h);
    h.query_all_by_label("Manage remotes…")
        .next()
        .expect("footer button painted")
        .click();
    settle(h);
    assert_eq!(
        h.state().ui.dialog,
        Some(Dialog::ManageRemotes),
        "footer click opens the manager"
    );
}

fn type_into_field(h: &mut Harness<'_, AppState>, label: &str, text: &str) {
    let field = h.get_by_label(label);
    field.focus();
    field.type_text(text);
    settle(h);
}

fn click_button(h: &mut Harness<'_, AppState>, label: &str) {
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
fn manager_footer_uses_shared_divider_and_minimum_action_target() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.clone().into()));
    open_manager(&mut h);

    let close = h.get_by_label("Close");
    assert!(
        close.rect().height() >= 28.0,
        "Close minimum target: {:?}",
        close.rect()
    );
    assert!(
        filled_rects(&h)
            .into_iter()
            .any(|(rect, fill)| fill == turbogit_ui::theme::Palette::LINE
                && (rect.height() - 1.0).abs() < 0.01),
        "the shared footer divider must be present"
    );
}

#[test]
fn footer_opens_the_manager_listing_fetch_and_push_urls() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    add_divergent_origin(&repo);
    let (state, _exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.clone().into()));

    open_manager(&mut h);

    assert!(h.query_all_by_label("origin").next().is_some());
    assert!(
        h.query_all_by_label("fetch: https://fetch.git/repo.git")
            .next()
            .is_some()
    );
    assert!(
        h.query_all_by_label("push: https://push.git/repo.git")
            .next()
            .is_some()
    );
}

#[test]
fn add_remote_dispatches_and_refreshes_the_list() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.clone().into()));
    open_manager(&mut h);

    type_into_field(&mut h, "Remote name", "origin");
    type_into_field(&mut h, "Remote fetch URL", "https://fetch.git/repo.git");
    click_button(&mut h, "Add remote");

    let dispatched = wait_until(5_000, || {
        exec.recorded().iter().any(|c| {
            matches!(
                c,
                RecordedCall::AddRemote { name, url, .. }
                    if name == "origin" && url == "https://fetch.git/repo.git"
            )
        })
    });
    assert!(dispatched, "expected AddRemote, got {:?}", exec.recorded());

    pump_until(&mut h, "add refresh", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .map(|r| r.remotes.len())
            .unwrap_or(0)
            == 1
    });
    assert!(h.query_all_by_label("origin").next().is_some());
    assert!(
        h.query_all_by_label("fetch: https://fetch.git/repo.git")
            .next()
            .is_some()
    );
}

#[test]
fn rename_remote_dispatches_and_refreshes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    add_divergent_origin(&repo);
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.clone().into()));
    open_manager(&mut h);

    click_button(&mut h, "Rename");
    h.state_mut().ui.dlg.remotes_rename_new.clear();
    settle(&mut h);
    type_into_field(&mut h, "New remote name", "upstream");
    click_button(&mut h, "Rename");

    let dispatched = wait_until(5_000, || {
        exec.recorded().iter().any(|c| {
            matches!(
                c,
                RecordedCall::RenameRemote { old, new, .. }
                    if old == "origin" && new == "upstream"
            )
        })
    });
    assert!(
        dispatched,
        "expected RenameRemote, got {:?}",
        exec.recorded()
    );

    pump_until(&mut h, "rename refresh", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .map(|r| r.remotes.iter().any(|r| r.name == "upstream"))
            .unwrap_or(false)
    });
    assert!(h.query_all_by_label("upstream").next().is_some());
}

#[test]
fn edit_url_dispatches_and_refreshes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    add_divergent_origin(&repo);
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.clone().into()));
    open_manager(&mut h);

    click_button(&mut h, "Edit URL");
    h.state_mut().ui.dlg.remotes_edit_fetch.clear();
    settle(&mut h);
    type_into_field(&mut h, "Edit fetch URL", "https://new.git/repo.git");
    click_button(&mut h, "Save");

    let dispatched = wait_until(5_000, || {
        exec.recorded().iter().any(|c| {
            matches!(
                c,
                RecordedCall::SetRemoteUrl { name, fetch_url, .. }
                    if name == "origin" && fetch_url.as_deref() == Some("https://new.git/repo.git")
            )
        })
    });
    assert!(
        dispatched,
        "expected SetRemoteUrl, got {:?}",
        exec.recorded()
    );

    pump_until(&mut h, "edit-url refresh", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .and_then(|r| r.remotes.first())
            .map(|r| r.fetch_url.as_deref() == Some("https://new.git/repo.git"))
            .unwrap_or(false)
    });
    assert!(
        h.query_all_by_label("fetch: https://new.git/repo.git")
            .next()
            .is_some()
    );
}

#[test]
fn remove_remote_dispatches_and_refreshes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    add_divergent_origin(&repo);
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.clone().into()));
    open_manager(&mut h);

    click_button(&mut h, "Remove");

    let dispatched = wait_until(5_000, || {
        exec.recorded().iter().any(|c| {
            matches!(
                c,
                RecordedCall::RemoveRemote { name, .. } if name == "origin"
            )
        })
    });
    assert!(
        dispatched,
        "expected RemoveRemote, got {:?}",
        exec.recorded()
    );

    pump_until(&mut h, "remove refresh", |s| {
        s.selected_root
            .as_ref()
            .and_then(|id| s.multi.by_id(id))
            .map(|r| r.remotes.is_empty())
            .unwrap_or(false)
    });
    assert!(
        h.query_all_by_label("No remotes configured.")
            .next()
            .is_some()
    );
}

#[test]
fn set_upstream_per_branch_dispatches_through_the_engine_seam() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = fresh_repo(tmp.path(), "alpha");
    add_divergent_origin(&repo);
    // Provide the tracking ref `--set-upstream-to` requires, without a
    // network round trip.
    git(&repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let (state, exec) = app_state_recording(tmp.path(), std::slice::from_ref(&repo));
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(repo.clone().into()));
    open_manager(&mut h);

    click_button(&mut h, "Set upstream…");
    click_button(&mut h, "main → origin/main");

    let dispatched = wait_until(5_000, || {
        exec.recorded().iter().any(|c| {
            matches!(
                c,
                RecordedCall::SetBranchUpstream { branch, upstream, .. }
                    if branch == "main" && upstream == "origin/main"
            )
        })
    });
    assert!(
        dispatched,
        "expected SetBranchUpstream, got {:?}",
        exec.recorded()
    );
    assert_eq!(
        git(&repo, &["config", "--get", "branch.main.remote"]).trim(),
        "origin"
    );
}

#[test]
fn apply_add_across_selection_reports_per_repo_outcomes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let alpha = fresh_repo(tmp.path(), "alpha");
    let beta = fresh_repo(tmp.path(), "beta");
    let (state, exec) = app_state_recording(tmp.path(), &[alpha.clone(), beta.clone()]);
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(alpha.clone().into()));
    open_manager(&mut h);

    type_into_field(&mut h, "Remote name", "origin");
    type_into_field(&mut h, "Remote fetch URL", "https://fetch.git/repo.git");
    click_button(&mut h, "beta");
    click_button(&mut h, "Apply add");

    let dispatched = wait_until(5_000, || {
        exec.recorded().iter().any(|c| {
            matches!(
                c,
                RecordedCall::AddRemote { name, .. } if name == "origin"
            )
        })
    });
    assert!(dispatched, "expected AddRemote, got {:?}", exec.recorded());

    pump_until(&mut h, "apply outcomes", |s| {
        s.multi
            .roots
            .iter()
            .all(|r| r.remotes.iter().any(|r| r.name == "origin"))
    });
    assert!(h.query_all_by_label("✓ alpha").next().is_some());
    assert!(h.query_all_by_label("✓ beta").next().is_some());
    assert!(
        h.state()
            .ui
            .toast
            .as_ref()
            .map(|t| t.message.contains("2 of 2 ok"))
            .unwrap_or(false),
        "toast aggregates the per-repo outcomes"
    );
}

#[test]
fn apply_add_reports_a_failing_repo_without_blocking_the_others() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let alpha = fresh_repo(tmp.path(), "alpha");
    let beta = fresh_repo(tmp.path(), "beta");
    git(
        &alpha,
        &["remote", "add", "origin", "https://old.git/repo.git"],
    );
    let (state, _exec) = app_state_recording(tmp.path(), &[alpha.clone(), beta.clone()]);
    let mut h = harness(state);
    h.state_mut().selected_root = Some(RootId(alpha.clone().into()));
    open_manager(&mut h);

    type_into_field(&mut h, "Remote name", "origin");
    type_into_field(&mut h, "Remote fetch URL", "https://new.git/repo.git");
    click_button(&mut h, "beta");
    click_button(&mut h, "Apply add");

    pump_until(&mut h, "mixed apply outcomes", |s| {
        s.ui.toast
            .as_ref()
            .map(|t| t.message.contains("failed: alpha"))
            .unwrap_or(false)
    });
    assert!(
        h.state()
            .multi
            .by_id(&RootId(beta.clone().into()))
            .map(|r| r.remotes.iter().any(|r| r.name == "origin"))
            .unwrap_or(false)
    );
    assert!(h.query_all_by_label("✓ beta").next().is_some());
    assert!(
        h.query_all_by_label("✗ alpha").next().is_some(),
        "the failing repo's outcome row renders"
    );
}

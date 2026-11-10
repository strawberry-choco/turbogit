#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::kittest::Queryable;
use egui_kittest::{Harness, Node};
use test_support::RecordingExecutor;
use test_support::git_seed::git;
use test_support::harness::{
    KITTEST_DEFAULT_BOX, assert_not_painted, assert_painted, filled_rects, painted_galleys,
    painted_text, shell_harness_over_unstyled,
};
use test_support::wait_until;
use turbogit_app::push_session::Focus;
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::error::TgError;
use turbogit_domain::model::{RootId, VcsSettings};
use turbogit_engine_api::{GitExecutor, PushTags};
use turbogit_ui::theme::Palette;

fn head_sha(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

fn bare_ref(bare: &Path, refname: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(bare)
        .args(["rev-parse", "--verify", refname])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

struct Repo {
    path: PathBuf,
    remote: PathBuf,
    shas: [String; 3],
}

fn repo_ahead_of_origin(parent: &Path, name: &str) -> Repo {
    let path = parent.join(name);
    let remote = parent.join(format!("{name}-remote.git"));
    std::fs::create_dir_all(&path).unwrap();

    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);

    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    let base_msg = format!("{name} base");
    git(&path, &["commit", "-q", "-m", &base_msg]);
    let c1 = head_sha(&path);

    git(
        &path,
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(
        &path,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&path, &["push", "-q", "-u", "origin", "main"]);

    let c2_file = format!("{name}-c2.txt");
    std::fs::write(path.join(&c2_file), "second\n").unwrap();
    git(&path, &["add", "."]);
    let c2_msg = format!("{name} second");
    git(&path, &["commit", "-q", "-m", &c2_msg]);
    let c2 = head_sha(&path);

    let c3_file = format!("{name}-c3.txt");
    std::fs::write(path.join(&c3_file), "third\n").unwrap();
    git(&path, &["add", "."]);
    let c3_msg = format!("{name} third");
    git(&path, &["commit", "-q", "-m", &c3_msg]);
    let c3 = head_sha(&path);

    Repo {
        path,
        remote,
        shas: [c1, c2, c3],
    }
}

/// `repo_ahead_of_origin` plus true divergence: a second clone fast-forwards
/// the bare remote to its own commit d1, so the original repo (c2, c3) and
/// the remote (d1) have divergent `main` tips. The original repo fetches
/// afterwards so git reports `(non-fast-forward)` rather than `(fetch first)`.
///
/// Returns the repo plus the remote-side d1 SHA.
fn repo_diverged_from_origin(parent: &Path, name: &str) -> (Repo, String) {
    let repo = repo_ahead_of_origin(parent, name);

    let other = parent.join(format!("{name}-other"));
    git(
        parent,
        &[
            "clone",
            repo.remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git(&other, &["checkout", "main"]);
    git(&other, &["config", "user.email", "test@example.com"]);
    git(&other, &["config", "user.name", "Test"]);
    std::fs::write(other.join("divergent.txt"), "d1\n").unwrap();
    git(&other, &["add", "."]);
    let d1_msg = format!("{name} divergent");
    git(&other, &["commit", "-q", "-m", &d1_msg]);
    let d1 = head_sha(&other);
    git(&other, &["push", "-q", "origin", "main"]);

    git(&repo.path, &["fetch", "origin"]);

    (repo, d1)
}

fn app_state(project_dir: &Path, roots: &[PathBuf]) -> AppState {
    AppState::for_roots(project_dir, roots)
}

fn app_state_with(
    project_dir: &Path,
    roots: &[PathBuf],
    executor: Arc<dyn GitExecutor>,
    settings: VcsSettings,
) -> AppState {
    AppState::for_roots(project_dir, roots)
        .with_executor(executor)
        .with_settings(settings)
}

/// Headless harness driving the full app UI with event draining per frame.
///
/// Deliberately unstyled: the production tokens and embedded fonts would change the
/// frame the painted-geometry assertions below measure. `max_steps` is 1024 rather
/// than kittest's default of 4, and this suite drives its clicks through
/// `Harness::run()`, which panics once a run exceeds the budget.
fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, KITTEST_DEFAULT_BOX, 1024)
}

/// The rows are built by the app now, not by the paint, so a test that sets the
/// dialog field directly gets an empty table. The dialog is opened for the first
/// root, the caller-supplied-root case: that row is consented and no other is.
fn open_push_dialog(h: &mut Harness<'_, AppState>) {
    match h.state().multi.roots.first().map(|root| root.id.clone()) {
        Some(root) => h.state_mut().open_push_for_roots(&[root]),
        None => h.state_mut().open_push(),
    }
    h.run();
}

fn dialog_push_button<'h>(h: &'h Harness<'_, AppState>) -> Node<'h> {
    // The window is titled `Push Commits`, so the plain `Push` label belongs to
    // one node: the primary button.
    let mut nodes: Vec<Node<'h>> = h.query_all_by_label("Push").collect();
    assert!(
        !nodes.is_empty(),
        "expected the dialog's Push button, found none among {:?}",
        painted_text(h)
    );
    nodes.pop().expect("at least one Push button")
}

fn toast_galley(h: &Harness<'_, AppState>) -> Option<String> {
    painted_text(h).into_iter().find(|t| t.starts_with("Push:"))
}

#[test]
fn clicking_a_row_focuses_the_preview_without_touching_consent() {
    let parent = tempfile::tempdir().unwrap();
    let alpha = repo_ahead_of_origin(parent.path(), "alpha");
    let beta = repo_ahead_of_origin(parent.path(), "beta");

    let mut h = harness(app_state(
        parent.path(),
        &[alpha.path.clone(), beta.path.clone()],
    ));
    open_push_dialog(&mut h);

    let before: Vec<bool> = h
        .state()
        .ui
        .dlg
        .push
        .rows
        .iter()
        .map(|row| row.consented)
        .collect();
    assert_eq!(
        before,
        vec![true, false],
        "only the repository the app is pointed at is consented on open"
    );

    let targets: Vec<Node<'_>> = h.query_all_by_label("main → origin : main").collect();
    assert_eq!(targets.len(), 2, "every row prints its own target line");
    targets[1].clone().click();
    h.run();

    assert_eq!(
        h.state().ui.dlg.push.focus,
        Some(Focus::Root(RootId(beta.path.clone().into()))),
        "the row click must set the preview focus"
    );
    let after: Vec<bool> = h
        .state()
        .ui
        .dlg
        .push
        .rows
        .iter()
        .map(|row| row.consented)
        .collect();
    assert_eq!(after, before, "a preview click must not change consent");
    assert_painted(&h, "beta-c2.txt");
    assert_not_painted(&h, "alpha-c3.txt");
}

#[test]
fn consenting_both_rows_pushes_both_remotes() {
    let parent = tempfile::tempdir().unwrap();
    let alpha = repo_ahead_of_origin(parent.path(), "alpha");
    let beta = repo_ahead_of_origin(parent.path(), "beta");

    let mut h = harness(app_state(
        parent.path(),
        &[alpha.path.clone(), beta.path.clone()],
    ));
    open_push_dialog(&mut h);

    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Include beta")
        .click();
    h.run();
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "beta")
        .click();
    h.run();
    assert!(
        h.state().ui.dlg.push.rows[1].consented,
        "the row's checkbox is the only push filter"
    );

    dialog_push_button(&h).click();
    h.run();

    let alpha_tip = head_sha(&alpha.path);
    let beta_tip = head_sha(&beta.path);
    assert!(
        wait_until(15_000, || {
            bare_ref(&alpha.remote, "refs/heads/main").as_deref() == Some(alpha_tip.as_str())
                && bare_ref(&beta.remote, "refs/heads/main").as_deref() == Some(beta_tip.as_str())
        }),
        "both consented remotes must move; got alpha {:?} beta {:?}",
        bare_ref(&alpha.remote, "refs/heads/main"),
        bare_ref(&beta.remote, "refs/heads/main")
    );
}

#[test]
fn per_root_failure_is_surfaced_while_other_roots_succeed() {
    let parent = tempfile::tempdir().unwrap();
    let alpha = repo_ahead_of_origin(parent.path(), "alpha");
    let beta = repo_ahead_of_origin(parent.path(), "beta");
    std::fs::remove_dir_all(&beta.remote).unwrap();

    let mut h = harness(app_state(
        parent.path(),
        &[alpha.path.clone(), beta.path.clone()],
    ));
    open_push_dialog(&mut h);

    // beta's remote is gone, and an unconsented row is never pushed, so the
    // failure this test is about only exists once beta is in scope and consented.
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Include beta")
        .click();
    h.run();
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "beta")
        .click();
    h.run();

    dialog_push_button(&h).click();
    h.run();

    let alpha_tip = head_sha(&alpha.path);
    assert!(
        wait_until(15_000, || {
            bare_ref(&alpha.remote, "refs/heads/main").as_deref() == Some(alpha_tip.as_str())
        }),
        "the healthy root must still be pushed by the batch"
    );

    let mut surfaced = false;
    for _ in 0..40 {
        if let Some(toast) = toast_galley(&h) {
            assert!(
                toast.contains("Push"),
                "expected a failure toast, got: {toast}"
            );
            assert!(
                toast.contains("beta"),
                "failure toast must name the failing root, got: {toast}"
            );
            surfaced = true;
            break;
        }
        h.run();
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(surfaced, "per-root failure must surface in the op toast");
}

fn recording_engine(settings: VcsSettings) -> (Arc<RecordingExecutor>, VcsSettings) {
    let cli: Arc<dyn GitExecutor> = Arc::new(turbogit_engine::cli::CliExecutor {
        settings: settings.clone(),
    });
    let rec = Arc::new(RecordingExecutor::new(cli));
    (rec, settings)
}

#[test]
fn preview_runs_real_dry_run_and_paints_report_verbatim() {
    let parent = tempfile::tempdir().unwrap();
    let solo = repo_ahead_of_origin(parent.path(), "solo");

    let (rec, settings) = recording_engine(VcsSettings::default());
    // Expected VERBATIM report, captured straight from the engine seam before
    // the UI runs (dry-run is non-mutating, so the second invocation inside
    // the dialog must produce byte-identical output).
    let expected = rec
        .inner
        .push_dry_run(&solo.path, "origin", "main", false)
        .expect("dry-run succeeds when ahead of origin");
    assert!(expected.contains("main -> main"));

    let dyn_exec: Arc<dyn GitExecutor> = rec.clone();
    let mut h = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&solo.path),
        dyn_exec,
        settings,
    ));
    open_push_dialog(&mut h);

    h.get_by_label("Preview dry-run").click();
    h.run();

    let texts = painted_text(&h);
    assert!(
        texts.iter().any(|t| t.contains(expected.trim_end())),
        "verbatim dry-run report not painted;\nexpected:\n{expected}\n\npainted:\n{texts:#?}"
    );
    assert_painted(&h, "Dry-run report (verbatim):");

    let c1 = solo.shas[0].clone();
    assert_eq!(
        bare_ref(&solo.remote, "refs/heads/main").as_deref(),
        Some(c1.as_str()),
        "preview must not mutate the remote"
    );

    assert!(
        rec.contains_dry_run("origin", "main", false),
        "expected PushDryRun {{ origin, main, force: false }}, got: {:?}",
        rec.recorded()
    );
}

#[test]
fn preview_shows_rejection_verbatim_when_diverged() {
    let parent = tempfile::tempdir().unwrap();
    let (solo, d1) = repo_diverged_from_origin(parent.path(), "solo");

    let (rec, settings) = recording_engine(VcsSettings::default());
    let err = rec
        .inner
        .push_dry_run(&solo.path, "origin", "main", false)
        .expect_err("diverged push must be rejected");
    let expected = match &err {
        TgError::Cli { stderr, .. } => stderr.clone(),
        other => panic!("expected TgError::Cli, got: {other:?}"),
    };
    assert!(expected.contains("[rejected]"));
    assert!(expected.contains("(non-fast-forward)"));

    let dyn_exec: Arc<dyn GitExecutor> = rec.clone();
    let mut h = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&solo.path),
        dyn_exec,
        settings,
    ));
    open_push_dialog(&mut h);

    h.get_by_label("Preview dry-run").click();
    h.run();

    let texts = painted_text(&h);
    assert!(
        texts.iter().any(|t| t.contains(expected.trim_end())),
        "verbatim rejection output not painted;\nexpected:\n{expected}\n\npainted:\n{texts:#?}"
    );
    assert_painted(&h, "Push rejected by git:");
    assert_not_painted(&h, "Dry-run report (verbatim):");

    assert_eq!(
        bare_ref(&solo.remote, "refs/heads/main").as_deref(),
        Some(d1.as_str()),
        "rejected preview must not mutate the remote"
    );
}

#[test]
fn force_acknowledgment_switches_push_to_force_with_lease_at_boundary() {
    let parent = tempfile::tempdir().unwrap();
    let mut settings = VcsSettings::default();
    settings.protected_branch_patterns.clear();

    let ack = repo_ahead_of_origin(parent.path(), "ack");
    let (rec, settings) = recording_engine(settings);
    let dyn_exec: Arc<dyn GitExecutor> = rec.clone();
    let mut h = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&ack.path),
        dyn_exec,
        settings.clone(),
    ));
    open_push_dialog(&mut h);

    h.get_by_label("Force push (--force-with-lease)").click();
    h.run();
    dialog_push_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || rec.contains_push("origin", "main", true)),
        "acknowledged force push must reach the boundary as force=true, got: {:?}",
        rec.recorded()
    );
    let ack_tip = head_sha(&ack.path);
    assert!(
        wait_until(15_000, || {
            bare_ref(&ack.remote, "refs/heads/main").as_deref() == Some(ack_tip.as_str())
        }),
        "--force-with-lease push must update the remote"
    );

    let plain = repo_ahead_of_origin(parent.path(), "plain");
    let (rec2, settings2) = recording_engine(settings);
    let dyn_exec2: Arc<dyn GitExecutor> = rec2.clone();
    let mut h2 = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&plain.path),
        dyn_exec2,
        settings2,
    ));
    open_push_dialog(&mut h2);

    dialog_push_button(&h2).click();
    h2.run();

    assert!(
        wait_until(15_000, || rec2.contains_push("origin", "main", false)),
        "plain push must reach the boundary as force=false, got: {:?}",
        rec2.recorded()
    );
    assert!(
        !rec2.contains_push("origin", "main", true),
        "unchecked acknowledgment must never send force=true"
    );
}

#[test]
fn protected_branch_refuses_force_per_repository_and_keeps_plain_push_available() {
    let parent = tempfile::tempdir().unwrap();
    let guarded = repo_ahead_of_origin(parent.path(), "guarded");
    let (rec, settings) = recording_engine(VcsSettings::default());
    let dyn_exec: Arc<dyn GitExecutor> = rec.clone();
    let mut h = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&guarded.path),
        dyn_exec,
        settings,
    ));
    open_push_dialog(&mut h);

    h.get_by_label("Force push (--force-with-lease)").click();
    h.run();

    assert_painted(&h, "protected — the push refuses force");
    // The reference disables the force affordance, never the push, so the
    // primary button must still be live on a protected branch.
    assert_not_painted(&h, "Nothing to update");

    dialog_push_button(&h).click();
    h.run();
    std::thread::sleep(Duration::from_millis(400));
    h.run();
    assert!(
        rec.recorded().is_empty(),
        "a refused force push must never reach the engine, got: {:?}",
        rec.recorded()
    );

    let plain = repo_ahead_of_origin(parent.path(), "plain");
    let (rec2, settings2) = recording_engine(VcsSettings::default());
    let dyn_exec2: Arc<dyn GitExecutor> = rec2.clone();
    let mut h2 = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&plain.path),
        dyn_exec2,
        settings2,
    ));
    open_push_dialog(&mut h2);
    dialog_push_button(&h2).click();
    h2.run();
    assert!(
        wait_until(15_000, || rec2.contains_push("origin", "main", false)),
        "plain push must still reach the boundary on a protected branch, got: {:?}",
        rec2.recorded()
    );
}

#[test]
fn push_footer_keeps_action_order_minimum_targets_and_shared_divider() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_ahead_of_origin(parent.path(), "footer");
    let mut h = harness(app_state(parent.path(), std::slice::from_ref(&repo.path)));
    open_push_dialog(&mut h);

    let cancel = h
        .query_all_by_label("Cancel")
        .last()
        .expect("Cancel button");
    let preview = h
        .query_all_by_label("Preview dry-run")
        .next()
        .expect("Preview button");
    let push = dialog_push_button(&h);
    assert!(cancel.rect().left() < preview.rect().left());
    assert!(preview.rect().left() < push.rect().left());
    for button in [&cancel, &preview, &push] {
        assert!(
            button.rect().height() >= 28.0,
            "minimum action target: {:?}",
            button.rect()
        );
    }

    let footer_top = preview.rect().top();
    assert!(
        filled_rects(&h)
            .into_iter()
            .any(|(rect, fill)| fill == Palette::LINE
                && (rect.height() - 1.0).abs() < 0.01
                && rect.top() <= footer_top),
        "the shared footer divider must sit above the actions"
    );
}

fn painted_ink_for(h: &Harness<'_, AppState>, needle: &str) -> Option<egui::Color32> {
    painted_galleys(h)
        .into_iter()
        .find(|g| g.text.contains(needle))
        .map(|g| g.color)
}

#[test]
fn push_severity_messages_use_the_shared_semantic_inks() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_ahead_of_origin(parent.path(), "sev");
    let settings = VcsSettings {
        protected_branch_patterns: vec!["main".to_string()],
        ..VcsSettings::default()
    };
    let (rec, settings) = recording_engine(settings);
    let dyn_exec: Arc<dyn GitExecutor> = rec.clone();
    let mut h = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&repo.path),
        dyn_exec,
        settings,
    ));
    open_push_dialog(&mut h);

    h.get_by_label("Force push (--force-with-lease)").click();
    h.run();

    assert_eq!(
        painted_ink_for(&h, "the push refuses force"),
        Some(Palette::STATE_ERROR),
        "the protected-branch refusal must render in the shared error ink"
    );

    // The same control on an unprotected branch warns instead of refusing.
    let loose = repo_ahead_of_origin(parent.path(), "loose");
    let settings2 = VcsSettings {
        protected_branch_patterns: vec!["never".to_string()],
        ..VcsSettings::default()
    };
    let (rec2, settings2) = recording_engine(settings2);
    let dyn_exec2: Arc<dyn GitExecutor> = rec2.clone();
    let mut h2 = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&loose.path),
        dyn_exec2,
        settings2,
    ));
    open_push_dialog(&mut h2);
    h2.get_by_label("Force push (--force-with-lease)").click();
    h2.run();
    assert_eq!(
        painted_ink_for(&h2, "Force push rewrites"),
        Some(Palette::STATE_WARNING),
        "an allowed force push must warn in the shared warning ink"
    );
}

#[test]
fn a_dialog_whose_only_row_cannot_be_pushed_offers_no_push() {
    let parent = tempfile::tempdir().unwrap();
    let loose = repo_without_remote(parent.path(), "loose");

    let mut h = harness(app_state(parent.path(), std::slice::from_ref(&loose)));
    open_push_dialog(&mut h);

    assert_painted(&h, "no remote to push to");
    // Consent is not enough: the row's obstacle keeps it out of the payload, so
    // the button must not dispatch an empty batch and call it a success.
    assert!(
        !h.query_all_by_label("Nothing to update").next().is_none(),
        "an unpushable dialog must relabel its primary action, got: {:?}",
        painted_text(&h)
    );
}

#[test]
fn push_tags_checkbox_propagates_to_engine_boundary() {
    let parent = tempfile::tempdir().unwrap();
    let solo = repo_ahead_of_origin(parent.path(), "solo");

    let (rec, settings) = recording_engine(VcsSettings::default());
    let dyn_exec: Arc<dyn GitExecutor> = rec.clone();
    let mut h = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&solo.path),
        dyn_exec,
        settings,
    ));
    open_push_dialog(&mut h);

    assert_painted(&h, "Push tags:");

    h.get_by_label("All").click();
    h.run();
    dialog_push_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || {
            rec.recorded().iter().any(|c| {
                matches!(
                    c,
                    test_support::RecordedCall::Push {
                        remote,
                        branch,
                        tags: PushTags::All,
                        ..
                    } if remote == "origin" && branch == "main"
                )
            })
        }),
        "Push tags must reach the boundary as tags=All; got: {:?}",
        rec.recorded()
    );
}

#[test]
fn skip_pre_push_hooks_propagates_to_engine_boundary() {
    let parent = tempfile::tempdir().unwrap();
    let solo = repo_ahead_of_origin(parent.path(), "solo");

    let (rec, settings) = recording_engine(VcsSettings::default());
    let dyn_exec: Arc<dyn GitExecutor> = rec.clone();
    let mut h = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&solo.path),
        dyn_exec,
        settings,
    ));
    open_push_dialog(&mut h);

    h.get_by_label("Run Git hooks").click();
    h.run();
    dialog_push_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || {
            rec.recorded().iter().any(|c| {
                matches!(
                    c,
                    test_support::RecordedCall::Push {
                        remote,
                        branch,
                        no_verify: true,
                        ..
                    } if remote == "origin" && branch == "main"
                )
            })
        }),
        "Skip pre-push hooks must reach the boundary as no_verify=true; got: {:?}",
        rec.recorded()
    );
}

#[test]
fn protected_force_refusal_survives_the_other_push_options() {
    let parent = tempfile::tempdir().unwrap();
    let guarded = repo_ahead_of_origin(parent.path(), "guarded");
    let (rec, settings) = recording_engine(VcsSettings::default());
    let dyn_exec: Arc<dyn GitExecutor> = rec.clone();
    let mut h = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&guarded.path),
        dyn_exec,
        settings,
    ));
    open_push_dialog(&mut h);

    h.get_by_label("Force push (--force-with-lease)").click();
    h.get_by_label("All").click();
    h.get_by_label("Run Git hooks").click();
    h.run();

    assert_painted(&h, "protected — the push refuses force");

    dialog_push_button(&h).click();
    h.run();
    std::thread::sleep(Duration::from_millis(400));
    h.run();

    assert!(
        rec.recorded().is_empty(),
        "a refused force push with the other options set must never reach the engine; got: {:?}",
        rec.recorded()
    );
}

use turbogit_services::sync_service::PushScope;

/// `repo_ahead_of_origin` plus divergence, WITHOUT the follow-up fetch: the
/// remote moved while the repo's remote-tracking ref still names the old tip.
/// That is exactly a stale force-with-lease, the push (not the lease) would
/// otherwise overwrite, so git refuses with `(stale info)`.
fn repo_with_stale_lease(parent: &Path, name: &str) -> Repo {
    let repo = repo_ahead_of_origin(parent, name);
    let other = parent.join(format!("{name}-other"));
    git(
        parent,
        &[
            "clone",
            repo.remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    git(&other, &["checkout", "main"]);
    git(&other, &["config", "user.email", "test@example.com"]);
    git(&other, &["config", "user.name", "Test"]);
    std::fs::write(other.join("divergent.txt"), "d1\n").unwrap();
    git(&other, &["add", "."]);
    git(&other, &["commit", "-q", "-m", "divergent"]);
    git(&other, &["push", "-q", "origin", "main"]);
    repo
}

fn wait_for_painted(h: &mut Harness<'_, AppState>, needle: &str, ms: u64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(ms);
    loop {
        h.run();
        if painted_text(h).iter().any(|t| t.contains(needle)) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn lease_rejection_paints_lease_wording_not_remote_wording() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_stale_lease(parent.path(), "alpha");
    let settings = VcsSettings {
        protected_branch_patterns: Vec::new(),
        ..VcsSettings::default()
    };
    let (rec, settings) = recording_engine(settings);
    let dyn_exec: Arc<dyn GitExecutor> = rec.clone();
    let mut h = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&repo.path),
        dyn_exec,
        settings,
    ));
    open_push_dialog(&mut h);

    h.get_by_label("Force push (--force-with-lease)").click();
    h.run();
    dialog_push_button(&h).click();

    assert!(
        wait_for_painted(&mut h, "Force-with-lease push", 20_000),
        "the lease rejection must paint the lease-specific wording; painted: {:?}",
        painted_text(&h)
    );
    assert_painted(&h, "the lease, not the push, is stale");
    assert_not_painted(&h, "rejected by the remote");
    assert!(
        rec.contains_push("origin", "main", true),
        "the force flag must reach the engine, got: {:?}",
        rec.recorded()
    );
}

fn repo_without_remote(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    for (file, message) in [("a.txt", "{name} first"), ("b.txt", "{name} second")] {
        std::fs::write(path.join(file), "x\n").unwrap();
        git(&path, &["add", "."]);
        git(
            &path,
            &["commit", "-q", "-m", &message.replace("{name}", name)],
        );
    }
    path
}

fn repo_with_no_commits(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    path
}

fn row_y(h: &Harness<'_, AppState>, text: &str) -> Option<f32> {
    painted_galleys(h)
        .into_iter()
        .find(|g| g.text == text)
        .map(|g| g.rect.center().y)
}

fn painted_target_state(h: &Harness<'_, AppState>) -> Option<String> {
    let top = row_y(h, "Branch:")?;
    let bottom = row_y(h, "Force push (--force-with-lease)")?;
    const NAMES: [&str; 5] = [
        "New",
        "New & Upstream",
        "Set upstream",
        "Upstream",
        "Empty repository",
    ];
    painted_galleys(h)
        .into_iter()
        .find(|g| {
            NAMES.contains(&g.text.as_str())
                && g.rect.center().y > top
                && g.rect.center().y < bottom
        })
        .map(|g| g.text)
}

fn open_vcs_popup(h: &mut Harness<'_, AppState>) {
    h.state_mut().ui.vcs_popup = true;
    h.run();
}

fn push_row_rects(h: &Harness<'_, AppState>) -> Vec<(egui::Rect, egui::Color32)> {
    let y = row_y(h, "Push…").expect("the popup offers the dialog-opening verb");
    filled_rects(h)
        .into_iter()
        .filter(|(rect, _)| {
            (rect.center().y - y).abs() < 6.0 && rect.height() > 10.0 && rect.height() < 40.0
        })
        .collect()
}

fn push_verb<'h>(h: &'h Harness<'_, AppState>, label: &'h str) -> Option<Node<'h>> {
    let y = row_y(h, "Push…")?;
    h.query_all_by_label(label)
        .into_iter()
        .find(|n| (n.rect().center().y - y).abs() < 6.0)
}

fn use_this_repo_scope(h: &mut Harness<'_, AppState>) {
    h.state_mut().ui.dlg.push_scope = PushScope::ThisRepo;
    h.run();
}

fn wait_for_state(h: &mut Harness<'_, AppState>, ms: u64, f: impl Fn(&AppState) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_millis(ms);
    loop {
        h.run();
        if f(h.state()) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn field_on_row<'h>(h: &'h Harness<'_, AppState>, y: f32) -> Node<'h> {
    let mut found = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .filter(|n| n.rect().height() < 30.0 && (n.rect().center().y - y).abs() < 6.0);
    found
        .next()
        .unwrap_or_else(|| panic!("no single-line text field painted on the row at y {y}"))
}

fn row_right_of(h: &Harness<'_, AppState>, text: &str) -> f32 {
    painted_galleys(h)
        .into_iter()
        .find(|g| g.text == text)
        .map(|g| g.rect.max.x)
        .unwrap_or(0.0)
}

#[test]
fn the_direct_and_dialog_verbs_are_two_fills_and_the_direct_one_pushes_alone() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_ahead_of_origin(parent.path(), "fast");
    let settings = VcsSettings {
        protected_branch_patterns: Vec::new(),
        ..VcsSettings::default()
    };
    let (rec, settings) = recording_engine(settings);
    let dyn_exec: Arc<dyn GitExecutor> = rec.clone();
    let mut h = harness(app_state_with(
        parent.path(),
        std::slice::from_ref(&repo.path),
        dyn_exec,
        settings,
    ));
    open_vcs_popup(&mut h);

    let rects = push_row_rects(&h);
    assert_eq!(rects.len(), 2, "the push row paints two button rects");
    let fills: Vec<egui::Color32> = rects.iter().map(|(_, fill)| *fill).collect();
    assert!(
        fills.contains(&Palette::ACCENT),
        "the direct verb wears the primary fill, so the two verbs are told apart          before either label is read: {fills:?}"
    );
    assert_ne!(
        fills[0], fills[1],
        "…and the dialog-opening verb wears another"
    );
    assert_ne!(rects[0].0, rects[1].0, "…on two distinct rects");

    push_verb(&h, "Push")
        .expect("the direct verb is offered")
        .click();
    h.run();
    assert_eq!(
        h.state().ui.dialog,
        None,
        "the direct verb must not open the dialog"
    );
    assert!(
        wait_until(15_000, || rec.recorded().iter().any(|c| {
            matches!(c, test_support::RecordedCall::Push { remote, branch, .. }
                if remote == "origin" && branch == "main")
        })),
        "the direct verb pushes the tracked branch at the resolved remote; got {:?}",
        rec.recorded()
    );

    open_vcs_popup(&mut h);
    push_verb(&h, "Push…")
        .expect("the dialog verb stays reachable")
        .click();
    h.run();
    assert_eq!(h.state().ui.dialog, Some(Dialog::Push));
    assert_painted(&h, "Preview dry-run");
}

fn no_direct_rect(h: &Harness<'_, AppState>) -> bool {
    !push_row_rects(h)
        .iter()
        .any(|(_, fill)| *fill == Palette::ACCENT)
}

#[test]
fn the_direct_verb_is_withheld_when_it_would_have_to_choose() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_ahead_of_origin(parent.path(), "guarded");
    let open = VcsSettings {
        protected_branch_patterns: Vec::new(),
        ..VcsSettings::default()
    };

    let mut h = harness(app_state(parent.path(), std::slice::from_ref(&repo.path)));
    open_vcs_popup(&mut h);
    assert!(
        no_direct_rect(&h),
        "a push on `main`, which the settings protect, gets no one-click verb"
    );
    assert_eq!(
        push_row_rects(&h).len(),
        1,
        "…and the dialog verb is still there"
    );

    let beta = repo_ahead_of_origin(parent.path(), "beta");
    let mut h = harness(
        app_state(parent.path(), &[repo.path.clone(), beta.path.clone()])
            .with_settings(open.clone()),
    );
    open_vcs_popup(&mut h);
    assert!(
        no_direct_rect(&h),
        "a multi-root workspace gets no one-click push"
    );

    let orphan = repo_without_remote(parent.path(), "orphan");
    let mut h =
        harness(app_state(parent.path(), std::slice::from_ref(&orphan)).with_settings(open));
    open_vcs_popup(&mut h);
    assert!(
        no_direct_rect(&h),
        "a remote-less repository gets no one-click push"
    );

    h.state_mut().ui.vcs_popup = false;
    h.run();
    assert!(
        !turbogit_ui::ui::push_control::push_without_dialog(h.state_mut()),
        "an unnameable push dispatches nothing"
    );
    assert_eq!(
        h.state().ui.dialog,
        Some(Dialog::Push),
        "…and hands the user the surface that can decide"
    );
}

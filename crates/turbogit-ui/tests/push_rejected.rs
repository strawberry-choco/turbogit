//! Ticket 13 — the six classified push-rejection notifications.
//!
//! Six situations collapsed into one string is the failure this surface exists
//! to prevent, so the suite asserts the *asymmetry*: each class says one thing
//! no other class says, the four classes with a next action paint exactly one
//! accent-filled control, and the two without one paint none.
//!
//! Most classes are driven by injecting one [`PushOutcome`] into
//! `AppState::ui.push_rejection`, because the six classes are a presentation
//! contract. The classification that turns git's stderr into an outcome is
//! exercised against real git here too — in the silent-update round trip and in
//! `push_dialog.rs`'s stale-lease case — while the two classes that need a
//! half-finished recovery (an errored update, an unresolved one) have no
//! deterministic fixture and are reached by injection only.
//!
//! Every wording assert is scoped by position (see [`notification_rect`]): push
//! strings also paint in the toolbar, the VCS popup and the failure toast, so an
//! unscoped assert passes for the wrong reason.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::Rect;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use test_support::git_seed::git;
use test_support::harness::{
    KITTEST_DEFAULT_BOX, filled_rects, painted_galleys, shell_harness_over_unstyled,
};
use turbogit_app::operation::PushRequest;
use turbogit_app::state::{AppState, PushRejection};
use turbogit_domain::model::VcsSettings;
use turbogit_services::sync_service::PushOutcome;
use turbogit_ui::theme::Palette;

// ---------------------------------------------------------------- helpers --

/// One rejection class: the outcome, the title its window paints, the one
/// phrase only this class may say, and the primary verb it must offer (`None`
/// for the two classes that have no next step).
struct Class {
    outcome: PushOutcome,
    title: &'static str,
    needle: &'static str,
    verb: Option<&'static str>,
}

fn classes() -> Vec<Class> {
    let remote = "origin".to_string();
    let branch = "main".to_string();
    vec![
        Class {
            outcome: PushOutcome::Rejected {
                remote: remote.clone(),
                branch: branch.clone(),
            },
            title: "Push rejected",
            needle: "to incorporate the remote's changes",
            verb: Some("Update and retry"),
        },
        Class {
            outcome: PushOutcome::RejectedByRemote {
                remote: remote.clone(),
                branch: branch.clone(),
            },
            title: "Push rejected by the remote",
            needle: "was rejected by the remote. Fetch to refresh",
            verb: Some("Fetch and retry"),
        },
        Class {
            outcome: PushOutcome::LeaseRejected { remote, branch },
            title: "Force-with-lease push rejected",
            needle: "the lease, not the push, is stale",
            verb: Some("Fetch and retry"),
        },
        Class {
            outcome: PushOutcome::UpdateConflicted,
            title: "Push canceled — update conflicted",
            needle: "there were conflicts during the update",
            verb: Some("Resolve conflicts"),
        },
        Class {
            outcome: PushOutcome::UpdateUnresolved,
            title: "Push canceled — conflicts unresolved",
            needle: "not all conflicts were resolved",
            verb: None,
        },
        Class {
            outcome: PushOutcome::UpdateErrored {
                detail: "boom".to_string(),
            },
            title: "Push rejected — update failed",
            needle: "update failed with an error: boom",
            verb: None,
        },
    ]
}

/// The verbs the actionable classes offer — the strings a silent class must
/// never paint, and the count that differs between the two groups.
const VERBS: [&str; 3] = ["Update and retry", "Fetch and retry", "Resolve conflicts"];

/// A `git init` repository with one commit on `main` and no remote.
fn repo_dir(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "base"]);
    path
}

/// A repo on `main` pushed to its own bare origin, then advanced locally by one
/// commit — so `origin` is reachable and a push of `main` can genuinely land.
fn repo_ahead_of_origin(parent: &Path, name: &str) -> (PathBuf, PathBuf) {
    let path = repo_dir(parent, name);
    let remote = parent.join(format!("{name}-remote.git"));
    git(
        &path,
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(
        &path,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&path, &["push", "-q", "-u", "origin", "main"]);
    std::fs::write(path.join("local.txt"), "local\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "local work"]);
    (path, remote)
}

/// The same repo, then truly diverged: a second clone advances the bare remote
/// with its own commit, so a push from the original is refused by real git with
/// `[rejected] (non-fast-forward)`.
fn repo_diverged_from_origin(parent: &Path, name: &str) -> (PathBuf, PathBuf) {
    let (path, remote) = repo_ahead_of_origin(parent, name);
    let other = parent.join(format!("{name}-other"));
    git(
        parent,
        &["clone", remote.to_str().unwrap(), other.to_str().unwrap()],
    );
    // The bare remote's HEAD may be unborn; land on main before committing.
    git(&other, &["checkout", "main"]);
    git(&other, &["config", "user.email", "test@example.com"]);
    git(&other, &["config", "user.name", "Test"]);
    std::fs::write(other.join("remote-side.txt"), "remote\n").unwrap();
    git(&other, &["add", "."]);
    git(&other, &["commit", "-q", "-m", "remote work"]);
    git(&other, &["push", "-q", "origin", "main"]);
    (path, remote)
}

/// `origin/main` as the bare repository holds it; `None` while the ref is
/// unborn. Its own `Command` because a non-zero `rev-parse` IS the answer and
/// `git_seed::git` asserts success.
fn bare_tip(bare: &Path, refname: &str) -> Option<String> {
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

fn head_sha(repo: &Path) -> String {
    git(repo, &["rev-parse", "HEAD"]).trim().to_string()
}

/// A harness over one registered root, already painted.
///
/// The root must be real and registered: the surface drops a rejection whose
/// repositories are gone, so an unregistered fixture would paint nothing and the
/// suite would assert against an empty frame.
fn harness_with_root(parent: &Path) -> Harness<'static, AppState> {
    let root = repo_dir(parent, "alpha");
    let mut h = shell_harness_over_unstyled(
        AppState::for_roots(parent, &[root]),
        KITTEST_DEFAULT_BOX,
        1024,
    );
    h.run();
    h
}

/// Store one outcome class and paint it.
fn show(h: &mut Harness<'_, AppState>, outcome: PushOutcome) {
    let root_id = h.state().multi.roots[0].id.clone();
    let settings = h.state().settings.clone();
    let request = PushRequest::single(
        root_id.clone(),
        "origin".to_string(),
        "main".to_string(),
        &settings,
    );
    h.state_mut().ui.push_rejection = Some(PushRejection {
        outcome,
        request,
        affected_roots: vec![root_id],
        update_all: false,
        remember: false,
    });
    // A toast would give the frame a SECOND `Dismiss` control (and a second
    // "rejected" sentence): the notification is the surface under test, so the
    // transient one is cleared before every class is painted.
    h.state_mut().ui.toast = None;
    h.run();
    assert!(
        h.state().ui.push_rejection.is_some(),
        "the rejection must survive the frame that paints it (a stale-root \
         drop would leave nothing to assert); painted: {:?}",
        painted_lines(h)
    );
}

/// Close the surface, so the next frame is the baseline the accent-fill diff
/// measures against.
fn close(h: &mut Harness<'_, AppState>) {
    h.state_mut().ui.push_rejection = None;
    h.run();
}

/// The frame's painted text with each galley's wrap points folded away.
///
/// A wrapped label's galley text carries `\n` at every break, so a raw
/// `contains` over a sentence can miss wording that is plainly on screen — and
/// an absence assert built on that would miss it for the wrong reason.
fn painted_lines(h: &Harness<'_, AppState>) -> Vec<String> {
    painted_galleys(h)
        .into_iter()
        .map(|g| g.text.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

/// The notification's own window rect, taken from the area the surface labels
/// with its title.
///
/// The largest node carrying that label is the window area itself (the title
/// bar's own label node is smaller), and an area rect is what a containment
/// claim needs: the window is a floating surface over the shell, so its rect is
/// the position every wording assert below scopes by.
fn window_frame(h: &Harness<'_, AppState>, title: &str) -> Rect {
    let nodes: Vec<egui_kittest::Node<'_>> = h.query_all_by_label(title).collect();
    assert!(
        !nodes.is_empty(),
        "the `{title}` window must be queryable; painted: {:?}",
        painted_lines(h)
    );
    let frame = nodes
        .iter()
        .map(|n| n.rect())
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("at least one node rect");
    assert!(
        frame.height() > 40.0,
        "the `{title}` frame must be a window, not a title strip: {frame:?}"
    );
    frame
}

/// Assert the notification paints `needle` **inside its own window frame**.
///
/// Position is the whole point of the assert: the same words also paint in the
/// failure toast (verbatim git stderr), in the toolbar and in the VCS popup, and
/// an unscoped `contains` would pass on any of those. The frame is derived from
/// this class's title bar — a different galley from the message — so relating
/// the two by geometry proves which surface drew the sentence.
fn assert_says(h: &Harness<'_, AppState>, needle: &str, title: &str) {
    let frame = window_frame(h, title);
    let inside = painted_galleys(h).iter().any(|g| {
        frame.contains_rect(g.rect) && {
            let folded = g.text.split_whitespace().collect::<Vec<_>>().join(" ");
            folded.contains(needle)
        }
    });
    assert!(
        inside,
        "`{needle}` must paint inside the `{title}` window frame {frame:?}; painted: {:?}",
        painted_lines(h)
    );
}

/// Assert no galley in the frame says `needle`.
fn assert_never_says(h: &Harness<'_, AppState>, needle: &str) {
    let lines = painted_lines(h);
    assert!(
        !lines.iter().any(|t| t.contains(needle)),
        "`{needle}` must not paint at all; painted: {lines:#?}"
    );
}

/// Every `ACCENT`-filled rect in the frame.
///
/// `Palette::ACCENT` is the brand fill the primary button family paints idle
/// with, which is the ticket's "accent-filled" claim in TurboGit's own tokens.
fn accent_rects(h: &Harness<'_, AppState>) -> Vec<Rect> {
    filled_rects(h)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::ACCENT)
        .map(|(rect, _)| rect)
        .collect()
}

/// The accent fills the notification *adds*, measured against the closed frame.
///
/// The shell behind the floating window paints accent fills of its own, so a
/// whole-frame count would credit those to the notification — and "a silent
/// class paints none" is precisely the assertion that pollution would fake.
fn added_accent_rects(h: &mut Harness<'_, AppState>, outcome: PushOutcome) -> Vec<Rect> {
    close(h);
    let before = accent_rects(h);
    show(h, outcome);
    let after = accent_rects(h);
    after
        .into_iter()
        .filter(|rect| !before.contains(rect))
        .collect()
}

/// Poll frames until `done` holds (an inline pump settles on the frame it is
/// drained on; the git side effect lands a moment later).
fn pump_until<F: Fn(&mut Harness<'_, AppState>) -> bool>(
    h: &mut Harness<'_, AppState>,
    ms: u64,
    done: F,
) -> bool {
    let deadline = Instant::now() + Duration::from_millis(ms);
    loop {
        h.run();
        if done(h) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

// ------------------------------------------------------------------ tests --

/// The assertion that forces the classification to exist: six classes, six
/// sentences, and each one excludes the other five. One shared string — or two
/// classes sharing a phrase — fails here.
#[test]
fn each_rejection_class_paints_its_own_sentence_and_none_other() {
    let parent = tempfile::tempdir().unwrap();
    let mut h = harness_with_root(parent.path());
    let all = classes();
    for class in &all {
        show(&mut h, class.outcome.clone());
        assert_says(&h, class.needle, class.title);
        for other in all.iter().filter(|o| o.title != class.title) {
            assert_never_says(&h, other.needle);
        }
    }
    close(&mut h);
    // Closed, the surface says nothing at all: every sentence above belongs to
    // a live rejection rather than to the shell.
    for class in &all {
        assert_never_says(&h, class.needle);
    }
}

/// The lease class is the one the reference separates for a reason: the remote
/// did not refuse, the user's expectation of the remote tip did. Its sentence
/// must not be reachable from the remote-refusal wording, in either direction.
#[test]
fn lease_rejection_and_remote_refusal_stay_separate_sentences() {
    let parent = tempfile::tempdir().unwrap();
    let mut h = harness_with_root(parent.path());
    let all = classes();
    let lease = all
        .iter()
        .find(|c| c.title.starts_with("Force-with-lease"))
        .unwrap();
    let remote = all
        .iter()
        .find(|c| c.title.ends_with("by the remote"))
        .unwrap();

    show(&mut h, lease.outcome.clone());
    assert_says(&h, lease.needle, lease.title);
    assert_never_says(&h, remote.needle);

    show(&mut h, remote.outcome.clone());
    assert_says(&h, remote.needle, remote.title);
    assert_never_says(&h, lease.needle);
    close(&mut h);
}

/// An update that STOPPED on conflicts is not an update that LEFT them
/// unresolved; the two sentences differ and the affordances differ with them.
#[test]
fn conflicts_during_the_update_are_not_unresolved_conflicts() {
    let parent = tempfile::tempdir().unwrap();
    let mut h = harness_with_root(parent.path());
    let all = classes();
    let during = all
        .iter()
        .find(|c| c.title.ends_with("update conflicted"))
        .unwrap();
    let left = all
        .iter()
        .find(|c| c.title.ends_with("conflicts unresolved"))
        .unwrap();

    show(&mut h, during.outcome.clone());
    assert_says(&h, during.needle, during.title);
    assert_never_says(&h, left.needle);

    show(&mut h, left.outcome.clone());
    assert_says(&h, left.needle, left.title);
    assert_never_says(&h, during.needle);
    close(&mut h);
}

/// The four actionable classes each paint exactly one accent-filled control,
/// and that control is the named next step — a rect of its own, never the
/// dismissal wearing a fill.
#[test]
fn actionable_classes_paint_one_accent_action_beside_the_dismissal() {
    let parent = tempfile::tempdir().unwrap();
    let mut h = harness_with_root(parent.path());
    for class in classes().into_iter().filter(|c| c.verb.is_some()) {
        let verb = class.verb.expect("filtered above");
        let added = added_accent_rects(&mut h, class.outcome.clone());
        assert_eq!(
            added.len(),
            1,
            "`{}` must paint exactly one accent-filled action; got {added:?}",
            class.title
        );
        let actions: Vec<egui_kittest::Node<'_>> = h.query_all_by_label(verb).collect();
        assert_eq!(
            actions.len(),
            1,
            "the `{verb}` control must be the notification's own action"
        );
        let action_rect = actions[0].rect();
        drop(actions);
        assert!(
            action_rect.contains(added[0].center()),
            "the accent fill must belong to `{verb}`: action {action_rect:?}, fill {:?}",
            added[0]
        );
        let dismiss_rect = h.get_by_label("Dismiss").rect();
        assert!(
            !dismiss_rect.contains(added[0].center()),
            "the primary action and the dismissal must be different rects: \
             action {:?}, dismissal {dismiss_rect:?}",
            added[0]
        );
        assert_says(&h, verb, class.title);
    }
    close(&mut h);
}

/// The asymmetry is the point: an empty-handed notification offers no button it
/// cannot honour, so it paints no accent fill and no next-step verb at all.
#[test]
fn silent_classes_paint_no_accent_action_at_all() {
    let parent = tempfile::tempdir().unwrap();
    let mut h = harness_with_root(parent.path());
    for class in classes().into_iter().filter(|c| c.verb.is_none()) {
        let added = added_accent_rects(&mut h, class.outcome.clone());
        assert!(
            added.is_empty(),
            "`{}` has no next step and must paint no accent-filled rect; got {added:?}",
            class.title
        );
        for verb in VERBS {
            assert_never_says(&h, verb);
        }
        // The dismissal is still there — an empty-handed surface can still be
        // closed, and it is the only control it may offer.
        assert_says(&h, "Dismiss", class.title);
    }
    close(&mut h);
}

/// `Update all repositories` is off by default: the documented fallback for a
/// reference default the docs never state. A rejection in one repository must
/// not integrate another repository's branch behind the user, and widening the
/// scope stays an explicit click.
#[test]
fn the_update_scope_defaults_to_the_affected_repositories_only() {
    let parent = tempfile::tempdir().unwrap();
    let mut h = harness_with_root(parent.path());
    let rejected = classes()
        .into_iter()
        .find(|c| c.title == "Push rejected")
        .unwrap();
    show(&mut h, rejected.outcome.clone());

    let title = rejected.title;
    assert_says(&h, "Update affected repositories only", title);
    assert_says(&h, "Update all repositories", title);
    assert!(
        !h.state().ui.push_rejection.as_ref().unwrap().update_all,
        "affected-only is the shipped default"
    );

    h.get_by_label("Update all repositories").click();
    h.run();
    assert!(
        h.state().ui.push_rejection.as_ref().unwrap().update_all,
        "the scope choice is the user's to widen"
    );

    h.get_by_label("Update affected repositories only").click();
    h.run();
    assert!(
        !h.state().ui.push_rejection.as_ref().unwrap().update_all,
        "and narrow again"
    );
    close(&mut h);
}

/// The remembered-opt-in checkbox is a settings round-trip, not a per-push flag:
/// it writes `auto_update_on_push_rejected` and reverses it, and the sentence
/// above it changes to match what the settings now authorise.
#[test]
fn the_remember_checkbox_writes_the_opt_in_and_reverses_it() {
    let parent = tempfile::tempdir().unwrap();
    let mut h = harness_with_root(parent.path());
    let rejected = classes()
        .into_iter()
        .find(|c| c.title == "Push rejected")
        .unwrap();
    let title = rejected.title;
    let label = "Remember the update method choice and silently update in the future";
    show(&mut h, rejected.outcome.clone());
    assert!(!h.state().settings.auto_update_on_push_rejected);

    h.get_by_label(label).click();
    h.run();
    assert!(
        h.state().settings.auto_update_on_push_rejected,
        "ticking must write the setting, not merely the notification"
    );
    assert_says(&h, "will update silently", title);

    h.get_by_label(label).click();
    h.run();
    assert!(
        !h.state().settings.auto_update_on_push_rejected,
        "un-ticking clears it again — the opt-in reverses in place"
    );
    assert_never_says(&h, "will update silently");
    close(&mut h);
}

/// The next-step control must do the work it names, not decorate the message.
/// `Resolve conflicts` opens the resolver and runs no git work; `Update and
/// retry` re-dispatches the stored request and the commits actually land.
#[test]
fn each_next_step_control_does_the_work_it_names() {
    let parent = tempfile::tempdir().unwrap();
    let (repo, remote) = repo_ahead_of_origin(parent.path(), "alpha");
    let mut h = shell_harness_over_unstyled(
        AppState::for_roots(parent.path(), &[repo.clone()]),
        KITTEST_DEFAULT_BOX,
        1024,
    );
    h.run();

    let conflicted = classes()
        .into_iter()
        .find(|c| c.title.ends_with("update conflicted"))
        .unwrap();
    show(&mut h, conflicted.outcome.clone());
    let untouched = bare_tip(&remote, "refs/heads/main");
    h.get_by_label("Resolve conflicts").click();
    h.run();
    assert!(h.state().ui.conflict_resolver_open);
    assert!(h.state().ui.push_rejection.is_none());
    assert!(
        h.state().ui.push_in_flight.is_none(),
        "handing off to the resolver runs no git work"
    );
    assert_eq!(
        bare_tip(&remote, "refs/heads/main"),
        untouched,
        "the hand-off alone must never advance the remote"
    );

    h.state_mut().ui.conflict_resolver_open = false;
    let rejected = classes()
        .into_iter()
        .find(|c| c.title == "Push rejected")
        .unwrap();
    show(&mut h, rejected.outcome.clone());
    h.get_by_label("Update and retry").click();
    let landed = pump_until(&mut h, 20_000, |h| {
        h.state().ui.push_rejection.is_none() && h.state().ui.push_in_flight.is_none()
    });
    assert!(
        landed,
        "the retry must settle; painted: {:?}",
        painted_lines(&h)
    );
    assert_eq!(
        bare_tip(&remote, "refs/heads/main").as_deref(),
        Some(head_sha(&repo).as_str()),
        "`Update and retry` must push the commits it was rejected on"
    );
}

/// The settings write-back has to be honoured by something: with the opt-in on,
/// a rejected ordinary push updates and re-pushes without asking, and the
/// notification never opens. With it off — the default — the same rejection
/// asks, and nothing moves on the remote.
#[test]
fn the_silent_update_opt_in_repushes_without_asking() {
    let parent = tempfile::tempdir().unwrap();
    let (repo, remote) = repo_diverged_from_origin(parent.path(), "alpha");
    let mut settings = VcsSettings {
        protected_branch_patterns: Vec::new(),
        ..VcsSettings::default()
    };
    settings.auto_update_on_push_rejected = false;
    let mut h = shell_harness_over_unstyled(
        AppState::for_roots(parent.path(), &[repo.clone()]).with_settings(settings.clone()),
        KITTEST_DEFAULT_BOX,
        1024,
    );
    h.run();
    let root_id = h.state().multi.roots[0].id.clone();
    let request = PushRequest::single(root_id, "origin".to_string(), "main".to_string(), &settings);
    let diverged_before = bare_tip(&remote, "refs/heads/main");

    // Off (the shipped default): the rejection asks, and the remote stays put.
    h.state_mut().dispatch_push(request.clone(), None);
    let asked = pump_until(&mut h, 20_000, |h| h.state().ui.push_rejection.is_some());
    assert!(
        asked,
        "a rejected push must open the notification; painted: {:?}",
        painted_lines(&h)
    );
    assert_says(&h, "to incorporate the remote's changes", "Push rejected");
    assert_eq!(
        bare_tip(&remote, "refs/heads/main"),
        diverged_before,
        "asking must not integrate anything on its own"
    );

    // On: the next rejection is not a question.
    close(&mut h);
    h.state_mut().settings.auto_update_on_push_rejected = true;
    h.state_mut().dispatch_push(request, None);
    let merged_and_pushed = pump_until(&mut h, 25_000, |h| {
        h.state().ui.push_in_flight.is_none()
            && head_sha(&repo) == bare_tip(&remote, "refs/heads/main").unwrap_or_default()
    });
    assert!(
        merged_and_pushed,
        "the silent update must fetch, integrate and re-push; remote tip {:?}, local head {}",
        bare_tip(&remote, "refs/heads/main"),
        head_sha(&repo)
    );
    assert!(
        h.state().ui.push_rejection.is_none(),
        "with the opt-in on, an ordinary rejection must not ask again; painted: {:?}",
        painted_lines(&h)
    );
    assert_never_says(&h, "Update and retry");
}

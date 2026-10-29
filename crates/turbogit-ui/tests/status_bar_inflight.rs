use std::path::{Path, PathBuf};

use egui::{Color32, Rect, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use tempfile::TempDir;
use test_support::git_seed::{GitEnv, git_with_env, pin_eol_lf};
use test_support::harness::{filled_rects, painted_galleys, settle, shell_harness_over_animated};
use turbogit_app::operation::{LifecycleVerb, Operation};
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{RootId, VcsSettings};
use turbogit_services::integrate_service::{OwnedOperation, owning_operation};
use turbogit_ui::theme::{self, Palette};
use turbogit_ui::ui::popups::Action;
use turbogit_ui::ui::shell::STATUS_BAR_HEIGHT;
use turbogit_ui::ui::sidebar::SIDEBAR_WIDTH;
use turbogit_ui::ui::status_inflight::{InFlight, items_via, target_for};

#[track_caller]
fn fill_of(harness: &Harness<'_, AppState>, rect: Rect) -> Color32 {
    filled_rects(harness)
        .into_iter()
        .find(|(painted, _)| {
            (painted.center() - rect.center()).length() < 1.0
                && (painted.size().x - rect.size().x).abs() < 2.0
                && (painted.size().y - rect.size().y).abs() < 2.0
        })
        .unwrap_or_else(|| panic!("no filled rect painted at {rect:?}"))
        .1
}

#[track_caller]
fn band_button(harness: &Harness<'_, AppState>, label: &str) -> Rect {
    let band = status_band();
    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| {
            n.accesskit_node().label().as_deref() == Some(label) && band.contains(n.rect().center())
        })
        .unwrap_or_else(|| panic!("no `{label}` button painted in the status bar band"))
        .rect()
}

struct Project {
    _tmp: TempDir,
    project: PathBuf,
    alpha: PathBuf,
    beta: PathBuf,
}

/// `git` that may fail: a conflicting merge is the point, and git exits non-zero
/// exactly because it stopped mid-operation. It keeps its own runner because the
/// owner's asserts success, but takes its identity from the owner's builder.
fn git_allow_fail(dir: &Path, args: &[&str]) {
    let _ = GitEnv::fixture()
        .command(dir, args)
        .env("GIT_EDITOR", "true")
        .output();
}

fn write_commit(dir: &Path, file: &str, body: &str, msg: &str) {
    std::fs::write(dir.join(file), body).unwrap();
    git_with_env(dir, &["add", "."], &GitEnv::fixture());
    git_with_env(dir, &["commit", "-q", "-m", msg], &GitEnv::fixture());
}

fn repo(parent: &Path, name: &str) -> PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    git_with_env(&dir, &["init", "-q", "-b", "main"], &GitEnv::fixture());
    git_with_env(&dir, &["config", "user.email", "t@t"], &GitEnv::fixture());
    git_with_env(&dir, &["config", "user.name", "t"], &GitEnv::fixture());
    pin_eol_lf(&dir);
    write_commit(&dir, "base.txt", "base\n", "the base commit");
    write_commit(&dir, "a.txt", "one\n", &format!("{name} 1"));
    dir
}

fn project() -> Project {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("ws");
    std::fs::create_dir_all(&root).unwrap();
    let alpha = repo(&root, "alpha");
    let beta = repo(&root, "beta");
    Project {
        _tmp: tmp,
        project: root,
        alpha,
        beta,
    }
}

/// git's own held states, written the way the sequencer leaves them. These are
/// the markers `owning_operation` reads, so the announcement is driven by the
/// repository's state rather than by a flag the app could have set itself.
fn rebase_in_flight(repo: &Path) {
    std::fs::create_dir_all(repo.join(".git/rebase-merge")).unwrap();
}

fn cherry_pick_in_flight(repo: &Path) {
    std::fs::write(repo.join(".git/CHERRY_PICK_HEAD"), "deadbeef\n").unwrap();
}

/// A real merge stopped on a real conflict: `MERGE_HEAD` plus unmerged paths, and
/// `git merge --abort` can actually finish it.
fn merge_conflict_in_flight(repo: &Path) {
    write_commit(repo, "c.txt", "MAIN-side\n", "main side");
    git_with_env(
        repo,
        &["checkout", "-q", "-b", "side", "HEAD~1"],
        &GitEnv::fixture(),
    );
    write_commit(repo, "c.txt", "OTHER-side\n", "other side");
    git_with_env(repo, &["checkout", "-q", "main"], &GitEnv::fixture());
    git_allow_fail(repo, &["merge", "--no-edit", "side"]);
    assert!(
        repo.join(".git/MERGE_HEAD").exists(),
        "the seeded merge must be in flight"
    );
    assert_eq!(
        owning_operation(repo),
        Some(OwnedOperation::Merge),
        "the fixture is the state the bar announces"
    );
}

fn window_size() -> Vec2 {
    egui::vec2(1280.0 + SIDEBAR_WIDTH, 800.0)
}

fn status_band() -> Rect {
    let height = window_size().y;
    Rect::from_min_max(
        egui::pos2(0.0, height - STATUS_BAR_HEIGHT - 8.0),
        egui::pos2(f32::INFINITY, height),
    )
}

fn shell(p: &Project) -> Harness<'static, AppState> {
    let state = AppState::for_roots(&p.project, &[p.alpha.clone(), p.beta.clone()])
        .with_settings(VcsSettings::default());
    let mut harness = shell_harness_over_animated(state, window_size());
    harness.state_mut().ui.show_status_bar = true;
    harness.state_mut().ui.tab = Tab::Commit;
    settle(&mut harness);
    harness
}

fn band_text(harness: &Harness<'_, AppState>) -> Vec<String> {
    let band = status_band();
    painted_galleys(harness)
        .into_iter()
        .filter(|g| band.contains(g.pos))
        .map(|g| g.text)
        .collect()
}

#[track_caller]
fn assert_band_shows(harness: &Harness<'_, AppState>, needle: &str) {
    let in_band = band_text(harness);
    assert!(
        in_band.iter().any(|t| t == needle),
        "`{needle}` must paint inside the status bar band, which painted {in_band:?}"
    );
}

#[track_caller]
fn assert_band_hides(harness: &Harness<'_, AppState>, needle: &str) {
    let in_band = band_text(harness);
    assert!(
        !in_band.iter().any(|t| t == needle),
        "the status bar band must not paint `{needle}`; it painted {in_band:?}"
    );
}

#[track_caller]
fn click_in_band(harness: &mut Harness<'_, AppState>, label: &str) {
    let band = status_band();
    let node = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| {
            n.accesskit_node().label().as_deref() == Some(label) && band.contains(n.rect().center())
        })
        .unwrap_or_else(|| panic!("no `{label}` button painted in the status bar band"));
    node.click();
    harness.step();
}

#[test]
fn skip_is_offered_for_a_rebase_and_for_no_other_operation() {
    assert_eq!(
        LifecycleVerb::offered_for(OwnedOperation::Rebase).to_vec(),
        vec![
            LifecycleVerb::Abort,
            LifecycleVerb::Continue,
            LifecycleVerb::Skip
        ],
        "a paused rebase has three resolutions, and the third is the one you \
         want when the NEXT commit is the one that conflicts"
    );
    for owner in [
        OwnedOperation::Merge,
        OwnedOperation::CherryPick,
        OwnedOperation::Revert,
    ] {
        assert_eq!(
            LifecycleVerb::offered_for(owner).to_vec(),
            vec![LifecycleVerb::Abort, LifecycleVerb::Continue],
            "{owner:?} resolves two ways; a bar that offered Skip here would be \
             decorating the state rather than modelling it"
        );
    }
}

#[test]
fn a_rebase_in_flight_announces_its_root_and_offers_all_three_verbs() {
    let p = project();
    rebase_in_flight(&p.alpha);
    let h = shell(&p);

    assert_band_shows(&h, "Rebasing in alpha");
    assert_band_shows(&h, "Abort");
    assert_band_shows(&h, "Continue");
    assert_band_shows(&h, "Skip");

    assert_band_hides(&h, "Rebasing in beta");
    for text in band_text(&h) {
        assert!(
            !(text.contains("beta") && text.contains("in ")),
            "an idle root must not be announced: {text}"
        );
    }
}

#[test]
fn a_cherry_pick_announces_abort_and_continue_but_never_skip() {
    let p = project();
    cherry_pick_in_flight(&p.alpha);
    let h = shell(&p);

    assert_band_shows(&h, "Cherry-picking in alpha");
    assert_band_shows(&h, "Abort");
    assert_band_shows(&h, "Continue");
    assert_band_hides(&h, "Skip");
    assert_band_hides(&h, "Cherry-picking in beta");
}

#[test]
fn a_merge_in_flight_is_announced_with_two_verbs() {
    let p = project();
    merge_conflict_in_flight(&p.alpha);
    let h = shell(&p);

    assert_band_shows(&h, "Merging in alpha");
    assert_band_shows(&h, "Abort");
    assert_band_shows(&h, "Continue");
    assert_band_hides(&h, "Skip");
}

#[test]
fn the_three_verbs_are_distinct_controls_and_abort_is_separated_from_both() {
    let p = project();
    rebase_in_flight(&p.alpha);
    let h = shell(&p);

    let abort = band_button(&h, "Abort");
    let continue_rect = band_button(&h, "Continue");
    let skip = band_button(&h, "Skip");

    let abort_fill = fill_of(&h, abort);
    let continue_fill = fill_of(&h, continue_rect);
    let skip_fill = fill_of(&h, skip);
    assert_ne!(
        abort_fill, continue_fill,
        "the destructive verb cannot wear the same fill as its partner"
    );
    assert_ne!(
        continue_fill, skip_fill,
        "Continue and Skip are different resolutions and must read differently"
    );
    assert_ne!(
        abort_fill, skip_fill,
        "three same-coloured controls in a status bar make the destructive one \
         indistinguishable — the hazard the spec records for the reference"
    );
    assert_eq!(abort_fill, Palette::SURFACE_INFLIGHT_ABORT);
    assert_eq!(continue_fill, Palette::ACCENT);

    let to_abort = continue_rect.min.x - abort.max.x;
    let to_partner = skip.min.x - continue_rect.max.x;
    assert!(
        to_abort >= theme::STATUS_INFLIGHT_ABORT_GAP - 0.5,
        "the destructive verb needs its own air: {to_abort} pt before Continue"
    );
    assert!(
        to_partner < to_abort - 0.5,
        "Abort reads as separated from the pair, not as one of three evenly \
         spaced controls: gap to Continue {to_abort}, gap inside the pair {to_partner}"
    );
}

#[test]
fn all_three_verbs_are_reachable_from_the_palette_and_absent_from_the_frozen_popup() {
    let palette: Vec<&str> = Action::palette_actions()
        .iter()
        .map(|a| a.label())
        .collect();
    for verb in ["Abort Operation", "Continue Operation", "Skip Commit"] {
        assert!(
            palette.contains(&verb),
            "`{verb}` must be reachable without the status bar: {palette:?}"
        );
    }
    let popup: Vec<&str> = Action::all().iter().map(|a| a.label()).collect();
    for verb in ["Abort Operation", "Continue Operation", "Skip Commit"] {
        assert!(
            !popup.contains(&verb),
            "the VCS popup's documented action set stays frozen"
        );
    }
}

#[test]
fn the_menu_route_resolves_the_root_that_offers_the_verb() {
    let p = project();
    rebase_in_flight(&p.alpha);
    cherry_pick_in_flight(&p.beta);
    let mut state = AppState::for_roots(&p.project, &[p.alpha.clone(), p.beta.clone()])
        .with_settings(VcsSettings::default());

    state.selected_root = Some(RootId(p.alpha.clone().into()));
    assert_eq!(
        target_for(&state, LifecycleVerb::Skip),
        Some((RootId(p.alpha.clone().into()), OwnedOperation::Rebase)),
        "only the rebase holds a commit to skip"
    );
    assert_eq!(
        target_for(&state, LifecycleVerb::Abort),
        Some((RootId(p.alpha.clone().into()), OwnedOperation::Rebase)),
        "the focused root answers first"
    );

    state.selected_root = Some(RootId(p.beta.clone().into()));
    assert_eq!(
        target_for(&state, LifecycleVerb::Skip),
        Some((RootId(p.alpha.clone().into()), OwnedOperation::Rebase)),
        "Skip belongs to the root that is rebasing, not to the one in focus"
    );
    assert_eq!(
        target_for(&state, LifecycleVerb::Continue),
        Some((RootId(p.beta.clone().into()), OwnedOperation::CherryPick)),
        "the focused cherry-pick answers Continue"
    );
}

#[test]
fn no_verb_is_resolved_when_no_root_holds_an_operation() {
    let p = project();
    let state = AppState::for_roots(&p.project, &[p.alpha.clone(), p.beta.clone()])
        .with_settings(VcsSettings::default());
    assert_eq!(target_for(&state, LifecycleVerb::Abort), None);
    assert!(
        items_via(&state.multi.roots, &|root| {
            state.holding_operation(root.id.as_path())
        })
        .is_empty()
    );
}

#[test]
fn two_roots_mid_operation_announce_side_by_side_in_registration_order() {
    let p = project();
    rebase_in_flight(&p.alpha);
    cherry_pick_in_flight(&p.beta);
    let state = AppState::for_roots(&p.project, &[p.alpha.clone(), p.beta.clone()])
        .with_settings(VcsSettings::default());
    let found: Vec<InFlight> = items_via(&state.multi.roots, &|root| {
        state.holding_operation(root.id.as_path())
    });
    assert_eq!(
        found
            .iter()
            .map(|item| (item.root.name(), item.owner))
            .collect::<Vec<_>>(),
        vec![
            ("alpha".to_owned(), OwnedOperation::Rebase),
            ("beta".to_owned(), OwnedOperation::CherryPick),
        ],
        "both roots are announced, each with its own operation"
    );
}

#[test]
fn abort_in_the_bar_finishes_the_operation_and_is_recorded_durably() {
    let p = project();
    merge_conflict_in_flight(&p.alpha);
    let mut h = shell(&p);
    assert_band_shows(&h, "Merging in alpha");

    click_in_band(&mut h, "Abort");
    settle(&mut h);

    assert_eq!(
        owning_operation(&p.alpha),
        None,
        "the merge is no longer in flight in the root the bar named"
    );
    assert_eq!(
        owning_operation(&p.beta),
        None,
        "aborting one root touches no other root"
    );
    assert_band_hides(&h, "Merging in alpha");
    assert_band_hides(&h, "Abort");

    let recorded: Vec<&str> = h
        .state()
        .ui
        .activity
        .entries
        .iter()
        .map(|e| e.message.as_str())
        .collect();
    assert!(
        recorded.contains(&"Abort merge"),
        "the operation settles into the session-durable feed under its own name, \
         got {recorded:?}"
    );
}

#[test]
fn an_inflight_verb_dispatches_as_a_named_operation_on_its_own_root() {
    let op = Operation::Resolve {
        root: RootId(PathBuf::from("/ws/alpha").into()),
        owner: OwnedOperation::Rebase,
        verb: LifecycleVerb::Skip,
    };
    assert_eq!(op.label(), "Skip commit");
    assert_eq!(
        op.affected(),
        turbogit_app::root_caches::Affected::Root(RootId(PathBuf::from("/ws/alpha").into())),
        "the announcement is per root, so so is the invalidation"
    );
    assert_eq!(op.kind(), turbogit_app::operation::OpKind::Resolve);
    assert_eq!(
        LifecycleVerb::Abort.label_for(OwnedOperation::Merge),
        "Abort merge"
    );
    assert_eq!(
        LifecycleVerb::Continue.label_for(OwnedOperation::CherryPick),
        "Continue cherry-pick"
    );
}

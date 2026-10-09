//! The status bar's **per-root in-flight
//! operation item** and its Abort / Continue / Skip verbs.
//!
//! The reference announces an in-flight operation in the status bar, one
//! announcement per root, and puts the recovery verbs there — the doc is
//! explicit that you may "click in the status bar", and that the same verbs
//! stay reachable from a menu when the notification has been dismissed.
//!
//! Seams under test:
//!
//! 1. `LifecycleVerb::offered_for` — the state machine, pure. `Skip` belongs to
//!    a rebase and to nothing else; a table that handed every verb to every
//!    operation would be decoration, not a model.
//! 2. The **status bar** through the real shell over two real repositories —
//!    `painted_text` scoped to the bar's own band, because operation words paint
//!    in the toolbar, the popup and the conflict resolver too.
//! 3. `filled_rects` scoped to the same band — the destructive verb must be a
//!    distinguishable control, not one of three identical ones.
//! 4. `status_inflight::target_for` — the menu route's root resolution, which is
//!    where a multi-root client either names a root or picks a arbitrary one.
//! 5. One end-to-end click: a real conflicted merge, `Abort` in the bar, and the
//!    durable activity record that follows. The transient toast is not the
//!    record, and the record only exists because `OpKind` grew
//!    a settlement arm.
//!
//! Per-root-ness is the requirement, not a nicety: a single global "operation in
//! flight" flag would make this whole suite pass while shipping the wrong thing,
//! so the idle sibling root is asserted silent in every painted case.

use std::path::{Path, PathBuf};
use std::process::Command;

use egui::{Color32, Rect, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use tempfile::TempDir;
use test_support::harness::{filled_rects, painted_galleys, settle, shell_harness_over_animated};
use turbogit_app::operation::{LifecycleVerb, Operation};
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{RootId, VcsSettings};
use turbogit_services::integrate_service::{OwnedOperation, owning_operation};
use turbogit_ui::theme::{self, Palette};
use turbogit_ui::ui::popups::Action;
use turbogit_ui::ui::shell::STATUS_BAR_HEIGHT;
use turbogit_ui::ui::sidebar::SIDEBAR_WIDTH;
use turbogit_ui::ui::status_inflight::{InFlight, items, target_for};

/// The fill painted for one control: the filled rect the frame drew AT the
/// control's own allocated rect, matched by centre and size rather than by
/// position in the shape list.
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

/// The band's own node for one verb label: the button wearing that label whose
/// centre lands inside the status bar, so a toolbar or popup control of the same
/// name can never be what the measurement aimed at.
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

// ------------------------------------------------------------------- fixtures --

struct Project {
    _tmp: TempDir,
    project: PathBuf,
    alpha: PathBuf,
    beta: PathBuf,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .env("GIT_EDITOR", "true")
        .output()
        .expect("git must be on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `git` that may fail: a conflicting merge is the point, and git exits non-zero
/// exactly because it stopped mid-operation.
fn git_allow_fail(dir: &Path, args: &[&str]) {
    let _ = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .env("GIT_EDITOR", "true")
        .output();
}

fn write_commit(dir: &Path, file: &str, body: &str, msg: &str) {
    std::fs::write(dir.join(file), body).unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", msg]);
}

fn repo(parent: &Path, name: &str) -> PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    git(&dir, &["config", "core.autocrlf", "false"]);
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
    git(repo, &["checkout", "-q", "-b", "side", "HEAD~1"]);
    write_commit(repo, "c.txt", "OTHER-side\n", "other side");
    git(repo, &["checkout", "-q", "main"]);
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

// -------------------------------------------------------------------- harness --

fn window_size() -> Vec2 {
    egui::vec2(1280.0 + SIDEBAR_WIDTH, 800.0)
}

/// The bar's own band, with the same 8 px of slack a vertically-centred label's
/// origin needs above a 24 px panel.
fn status_band() -> Rect {
    let height = window_size().y;
    Rect::from_min_max(
        egui::pos2(0.0, height - STATUS_BAR_HEIGHT - 8.0),
        egui::pos2(f32::INFINITY, height),
    )
}

/// The shell over both roots with the status bar region on. `for_roots` builds
/// the headless shell with it hidden, and an announcement nobody can see is not
/// an announcement.
fn shell(p: &Project) -> Harness<'static, AppState> {
    let state = AppState::for_roots(&p.project, &[p.alpha.clone(), p.beta.clone()])
        .with_settings(VcsSettings::default());
    let mut harness = shell_harness_over_animated(state, window_size());
    harness.state_mut().ui.show_status_bar = true;
    harness.state_mut().ui.tab = Tab::Commit;
    settle(&mut harness);
    harness
}

/// Every string painted inside the status bar's band.
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

/// Click the button labelled `label` whose centre is inside the status bar's own
/// band — the only way to aim at the bar's control rather than a toolbar or
/// popup control wearing the same word.
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

// ------------------------------------------------------- the verb state machine --

/// `Skip` is a rebase resolution and nothing else. The reference names the action
/// `Git.Rebase.Skip` with the label `Skip Commit`; merges and cherry-picks get
/// Abort and Continue only.
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

// ------------------------------------------------------------- the announcement --

/// Verification 1: a rebase paused in root A paints the in-flight item naming
/// root A with all three verbs, and root B's status region paints nothing.
#[test]
fn a_rebase_in_flight_announces_its_root_and_offers_all_three_verbs() {
    let p = project();
    rebase_in_flight(&p.alpha);
    let h = shell(&p);

    assert_band_shows(&h, "Rebasing in alpha");
    assert_band_shows(&h, "Abort");
    assert_band_shows(&h, "Continue");
    assert_band_shows(&h, "Skip");

    // The sibling is silent: per-root scoping is the requirement.
    assert_band_hides(&h, "Rebasing in beta");
    for text in band_text(&h) {
        assert!(
            !(text.contains("beta") && text.contains("in ")),
            "an idle root must not be announced: {text}"
        );
    }
}

/// Verification 2: a cherry-pick offers Abort and Continue and does NOT offer
/// Skip — asserted as an absence inside the bar's band.
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

/// A merge announces the same two verbs, and no third.
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

/// Verification 3: three distinct rects with distinct fills, the destructive one
/// separated from its two partners. Three same-coloured controls would make the
/// one that throws the whole rebase away indistinguishable.
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
    // And which way the distinctness goes: the destructive verb wears the danger
    // tone, the forward-going one wears the accent, the third a plain raised
    // control. The token layer owns those values; the bar only picks roles.
    assert_eq!(abort_fill, Palette::SURFACE_INFLIGHT_ABORT);
    assert_eq!(continue_fill, Palette::ACCENT);

    // Abort is separated from both partners by the named air, and the two
    // partners sit closer to each other than either sits to Abort.
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

// ------------------------------------------------------------------ the routes --

/// The menu route exists because the reference's own doc says a dismissed
/// notification must still leave a way back. The command palette is TurboGit's
/// menu for verbs; the VCS popup's set is a frozen pin and must not gain one.
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

/// Two roots can hold operations at once. The menu route names the one it acts
/// on: the focused root when it holds one that offers the verb, otherwise the
/// first registered root that does.
#[test]
fn the_menu_route_resolves_the_root_that_offers_the_verb() {
    let p = project();
    rebase_in_flight(&p.alpha);
    cherry_pick_in_flight(&p.beta);
    let mut state = AppState::for_roots(&p.project, &[p.alpha.clone(), p.beta.clone()])
        .with_settings(VcsSettings::default());

    // Focused root wins when it can honour the verb.
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

    // A focused root that cannot honour the verb does not veto it: the search
    // moves on to the root that can.
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

/// Nothing in flight: the route resolves to no root rather than acting on an
/// arbitrary one.
#[test]
fn no_verb_is_resolved_when_no_root_holds_an_operation() {
    let p = project();
    let state = AppState::for_roots(&p.project, &[p.alpha.clone(), p.beta.clone()])
        .with_settings(VcsSettings::default());
    assert_eq!(target_for(&state, LifecycleVerb::Abort), None);
    assert!(items(&state.multi.roots).is_empty());
}

/// The per-root facts are one item per holding root, in registration order — the
/// documented fallback for two simultaneous operations, since the bar is one
/// 24 px band and cannot stack a row per root.
#[test]
fn two_roots_mid_operation_announce_side_by_side_in_registration_order() {
    let p = project();
    rebase_in_flight(&p.alpha);
    cherry_pick_in_flight(&p.beta);
    let found: Vec<InFlight> = items(&p_roots(&p));
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

fn p_roots(p: &Project) -> Vec<turbogit_domain::model::Root> {
    let state = AppState::for_roots(&p.project, &[p.alpha.clone(), p.beta.clone()])
        .with_settings(VcsSettings::default());
    state.multi.roots.clone()
}

// ------------------------------------------------------- the verb actually runs --

/// The whole channel end to end: a real conflicted merge is announced, `Abort`
/// in the bar runs `git merge --abort` on THAT root, the announcement goes away
/// with the marker, and the durable activity record names the verb.
///
/// The activity assertion is the one that keeps a status-bar item from becoming
/// the record: the bar is a live view of git's state, and the feed is what
/// survives it.
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

/// The dispatch unit carries its identity: an in-flight verb is a named
/// [`Operation::Resolve`], so settlement matches on `OpKind` and a new verb that
/// forgot its arm is a compile error rather than a silent `Other`.
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

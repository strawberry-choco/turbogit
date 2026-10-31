//! The New Branch dialog's name field: what it pre-fills, what it offers while the
//! reader types, what its keys mean, and what an invocation remembers.
//!
//! Every assertion here is what a person at the keyboard can see: painted text, a
//! painted fill, which widget holds focus, and which branch git ends up holding.

use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::Key;
use egui_kittest::{
    Harness,
    kittest::{NodeT as _, Queryable as _},
};
use tempfile::TempDir;
use test_support::git_seed::{GitEnv, git_with_env};
use test_support::harness::{
    PaintedGalley, assert_not_painted, assert_painted, filled_rects, galley_origin,
    painted_galleys, painted_text, settle_quiet,
};
use test_support::srcscan;
use turbogit_app::state::{AppState, Dialog, NewBranchBase, Tab};
use turbogit_domain::model::{BranchKind, BranchSpec, RootId};
use turbogit_ui::theme::{GROUP_ROW_HEIGHT, Palette};

const FAMILIES: &[&str] = &[
    "feat/blame-window",
    "feat/build-cache",
    "feature/login",
    "users/kye/scratch",
];

fn commit_file(repo: &Path, path: &str, body: &str, message: &str) {
    std::fs::write(repo.join(path), body).unwrap();
    git_with_env(repo, &["add", "--", path], &GitEnv::fixture());
    git_with_env(repo, &["commit", "-q", "-m", message], &GitEnv::fixture());
}

/// `main`, four families in use, one flat name, and `origin/release/2.4` sitting on
/// a different commit than the local branch that shares its bare name.
fn seeded_project() -> (TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let repo = project.join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    git_with_env(
        &repo,
        &["-c", "init.defaultBranch=main", "init", "-q", "."],
        &GitEnv::fixture(),
    );
    std::fs::write(repo.join("README.md"), "x\n").unwrap();
    git_with_env(&repo, &["add", "."], &GitEnv::fixture());
    git_with_env(&repo, &["commit", "-q", "-m", "init"], &GitEnv::fixture());

    git_with_env(
        &project,
        &["clone", "--bare", "alpha", "origin.git"],
        &GitEnv::fixture(),
    );
    git_with_env(
        &repo,
        &["remote", "add", "origin", "../origin.git"],
        &GitEnv::fixture(),
    );
    for family in FAMILIES {
        git_with_env(&repo, &["branch", family], &GitEnv::fixture());
    }
    git_with_env(&repo, &["branch", "zeta"], &GitEnv::fixture());
    git_with_env(&repo, &["branch", "release/2.4"], &GitEnv::fixture());

    // The remote's `release/2.4` must not be the local one: the bare-name spelling
    // resolves to the local branch, so an equal tip would let that bug pass. Created
    // after the bare clone, so the push opens a new remote ref rather than racing one.
    git_with_env(
        &repo,
        &["checkout", "-q", "-b", "shipped"],
        &GitEnv::fixture(),
    );
    commit_file(&repo, "ship.txt", "shipped\n", "the remote's release");
    git_with_env(
        &repo,
        &["push", "-q", "origin", "shipped:release/2.4"],
        &GitEnv::fixture(),
    );
    git_with_env(&repo, &["checkout", "-q", "main"], &GitEnv::fixture());
    git_with_env(&repo, &["fetch", "-q", "origin"], &GitEnv::fixture());
    (tmp, project)
}

fn repo_of(project: &Path) -> PathBuf {
    project.join("alpha")
}

fn harness_for(project_dir: PathBuf) -> Harness<'static, AppState> {
    let state = AppState::new(project_dir);
    let mut fonts_installed = false;
    let mut harness = Harness::builder().with_step_dt(1.0 / 60.0).build_ui_state(
        move |ui, state| {
            state.drain_events();
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
                fonts_installed = true;
            }
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(1008.0, 768.0));
    harness
}

/// The locals `open_branches` waits for: the seeded set, sorted as the assert reads it.
fn expected_locals() -> Vec<String> {
    let mut names: Vec<String> = [FAMILIES, &["main", "release/2.4", "shipped", "zeta"]]
        .concat()
        .into_iter()
        .map(str::to_owned)
        .collect();
    names.sort();
    names
}

fn open_branches(harness: &mut Harness<'_, AppState>) {
    harness.state_mut().ui.tab = Tab::Branches;
    settle_quiet(harness);
    let mut arrived = local_branches(harness);
    arrived.sort();
    assert_eq!(arrived, expected_locals(), "the read landed on the surface");
    // Eight locals under four auto-expanded directories put the last row below
    // this viewport's fold, so the sentinel is the top row, not `zeta`'s ink.
    assert_painted(harness, "main");
}

/// Arrive on the Branches tab and wait for the read, but without the exact local set:
/// a case that branches after seeding cannot match the seeded `expected_locals`. The
/// surface-loaded proof then comes from the candidate rows the test asserts next.
fn settle_branches_surface(harness: &mut Harness<'_, AppState>) {
    harness.state_mut().ui.tab = Tab::Branches;
    settle_quiet(harness);
    assert_painted(harness, "main");
}

fn opened_dialog(harness: &mut Harness<'_, AppState>) {
    harness.get_by_label("New Branch").click();
    settle_quiet(harness);
    assert_eq!(harness.state().ui.dialog, Some(Dialog::NewBranch));
}

fn name_field<'h>(harness: &'h Harness<'_, AppState>) -> egui_kittest::Node<'h> {
    harness.get_by_role_and_label(egui::accesskit::Role::TextInput, "Branch name")
}

/// The reader's own gesture: focus the field, then type. kittest queues both into
/// one frame, which is the case the pre-selection has to survive.
fn type_into_name(harness: &mut Harness<'_, AppState>, text: &str) {
    name_field(harness).focus();
    name_field(harness).type_text(text);
    settle_quiet(harness);
}

/// The candidate rows this frame painted: a row's AccessKit name is its text, and a
/// family's text always ends with the slash that makes it completable.
fn offered_rows(harness: &Harness<'_, AppState>) -> Vec<String> {
    let mut rows: Vec<String> = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter_map(|n| n.accesskit_node().label().as_deref().map(str::to_owned))
        .filter(|label| label.ends_with('/'))
        .collect();
    rows.sort();
    rows.dedup();
    rows
}

fn row_galley(harness: &Harness<'_, AppState>, text: &str) -> Option<PaintedGalley> {
    painted_galleys(harness)
        .into_iter()
        .find(|galley| galley.text == text)
}

/// The quiet count painted on the same line as a row, right of its text.
fn row_note(harness: &Harness<'_, AppState>, row: &PaintedGalley) -> Option<PaintedGalley> {
    painted_galleys(harness).into_iter().find(|galley| {
        (galley.pos.y - row.pos.y).abs() < 2.0
            && galley.pos.x > row.rect.right()
            && galley.color == Palette::T_MUTED
    })
}

#[track_caller]
fn pump_until(harness: &mut Harness<'_, AppState>, what: &str, pred: impl Fn(&AppState) -> bool) {
    for _ in 0..600 {
        if pred(harness.state()) {
            return;
        }
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!(
        "timed out waiting for: {what}\n  last_error={:?}\n  painted={:?}",
        harness.state().last_error,
        painted_text(harness)
    );
}

fn local_branches(harness: &Harness<'_, AppState>) -> Vec<String> {
    let root = harness.state().selected_root.clone().expect("a root");
    harness
        .state()
        .multi
        .by_id(&root)
        .expect("the repository")
        .branches
        .iter()
        // A remote row carries the bare name, so `origin/main` and `main` are two
        // entries with one string; without this filter `branch_created` can be
        // satisfied by a remote row the dialog never touched.
        .filter(|b| b.kind == BranchKind::Local)
        .map(|b| b.name.clone())
        .collect()
}

fn branch_created(harness: &Harness<'_, AppState>, name: &str) -> bool {
    local_branches(harness).iter().any(|n| n == name)
}

fn armed_fill(harness: &Harness<'_, AppState>) -> Option<egui::Rect> {
    let rows = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter_map(|node| {
            node.accesskit_node()
                .label()
                .filter(|label| label.ends_with('/'))
                .map(|label| (label.to_owned(), node.rect()))
        })
        .collect::<Vec<_>>();
    filled_rects(harness)
        .into_iter()
        .find(|(rect, fill)| {
            *fill == Palette::SURFACE_3
                && rows
                    .iter()
                    .any(|(_, row_rect)| (rect.min - row_rect.min).length_sq() < 1.0)
        })
        .map(|(rect, _)| rect)
}

#[test]
fn a_pre_filled_field_paints_no_candidate_row_until_something_is_typed() {
    let (_project, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    opened_dialog(&mut harness);

    assert_eq!(
        harness.state().ui.dlg.new_branch.name,
        "main",
        "the field opens pre-filled with the current branch"
    );
    assert!(
        offered_rows(&harness).is_empty(),
        "a pre-fill nobody has typed into is not a prefix: {:?}",
        offered_rows(&harness)
    );
    assert!(
        name_field(&harness).is_focused(),
        "and the field is the focus owner"
    );
}

#[test]
fn the_first_keystroke_replaces_the_pre_fill() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    opened_dialog(&mut harness);

    type_into_name(&mut harness, "x");

    assert_eq!(
        harness.state().ui.dlg.new_branch.name,
        "x",
        "the selection is live, not decorative: one keystroke leaves one character"
    );
}

#[test]
fn typing_a_prefix_offers_the_families_that_extend_it_with_their_counts() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    opened_dialog(&mut harness);

    type_into_name(&mut harness, "f");

    assert_eq!(
        offered_rows(&harness),
        vec!["feat/".to_owned(), "feature/".to_owned()],
        "both families that extend `f`, and nothing else"
    );
    for family in ["feat/", "feature/"] {
        assert_painted(&harness, family);
    }
    let feat = row_galley(&harness, "feat/").expect("feat/ row painted");
    let feature = row_galley(&harness, "feature/").expect("feature/ row painted");
    assert_eq!(
        row_note(&harness, &feat)
            .expect("feat/ carries a count")
            .text,
        "2",
        "two branches live under feat/"
    );
    assert_eq!(
        row_note(&harness, &feature)
            .expect("feature/ carries a count")
            .text,
        "1",
        "one under feature/"
    );
    assert_eq!(
        feat.family,
        egui::FontFamily::Monospace,
        "a branch name never reads in the chrome face"
    );
}

#[test]
fn clicking_a_row_leaves_the_field_holding_exactly_that_prefix_and_no_list() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    opened_dialog(&mut harness);

    type_into_name(&mut harness, "fe");
    assert!(offered_rows(&harness).contains(&"feat/".to_owned()));
    harness
        .get_by_role_and_label(egui::accesskit::Role::Button, "feat/")
        .click();
    settle_quiet(&mut harness);

    assert_eq!(harness.state().ui.dlg.new_branch.name, "feat/");
    assert!(
        offered_rows(&harness).is_empty(),
        "the accepted row no longer strictly extends the buffer: {:?}",
        offered_rows(&harness)
    );
}

#[test]
fn a_prefix_no_family_extends_offers_nothing_and_enter_still_creates() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    opened_dialog(&mut harness);

    type_into_name(&mut harness, "z");
    assert!(
        offered_rows(&harness).is_empty(),
        "`zeta` is a branch, not a family: {:?}",
        offered_rows(&harness)
    );

    harness.key_press(Key::Enter);
    pump_until(&mut harness, "the typed name is created", |s| {
        s.multi
            .roots
            .first()
            .is_some_and(|r| r.branches.iter().any(|b| b.name == "z"))
    });
}

#[test]
fn the_remote_name_deny_set_holds_for_a_prefix_even_when_a_local_branch_lives_under_it() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir.clone());
    open_branches(&mut harness);
    opened_dialog(&mut harness);

    type_into_name(&mut harness, "origin/");
    assert!(
        !offered_rows(&harness).contains(&"origin/".to_owned()),
        "the remote row's slashes belong to a remote name, not to a naming \
         convention: {:?}",
        offered_rows(&harness)
    );

    // The same rule read from the other side. A local branch `origin/experiment`
    // does NOT reopen the offer: the deny set is keyed on the first segment being a
    // remote name, because `git branch origin/x` is the ambiguous refname git itself
    // warns about, ambiguity that a local branch of that prefix does not resolve.
    git_with_env(
        &repo_of(&dir),
        &["branch", "origin/experiment"],
        &GitEnv::fixture(),
    );
    let mut harness = harness_for(dir);
    settle_branches_surface(&mut harness);
    opened_dialog(&mut harness);
    type_into_name(&mut harness, "origin");
    assert!(
        !offered_rows(&harness).contains(&"origin/".to_owned()),
        "the deny set wins over a local branch of the same prefix: {:?}",
        offered_rows(&harness)
    );
}

#[test]
fn shallower_families_are_offered_first() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    opened_dialog(&mut harness);

    type_into_name(&mut harness, "users");

    assert_eq!(
        harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .filter_map(|n| n.accesskit_node().label().as_deref().map(str::to_owned))
            .filter(|label| label.starts_with("users") && label.ends_with('/'))
            .collect::<Vec<_>>(),
        vec!["users/".to_owned(), "users/kye/".to_owned()],
        "the query's own order is on screen, shallower first"
    );
}

#[test]
fn no_row_is_a_complete_branch_name() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    opened_dialog(&mut harness);

    type_into_name(&mut harness, "fea");
    let branches = local_branches(&harness);
    for row in offered_rows(&harness) {
        assert!(
            !branches.contains(&row),
            "`{row}` is a branch's whole name, so the list would be a second \
             branch picker. Rows: {:?}",
            offered_rows(&harness)
        );
    }
    assert!(offered_rows(&harness).contains(&"feature/".to_owned()));
}

#[test]
fn the_list_floats_over_the_dialog_without_moving_it() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    opened_dialog(&mut harness);

    let before = galley_origin(&harness, "Start from:").expect("the stating row is painted");
    let field_before = name_field(&harness).rect();
    assert!(
        galley_origin(&harness, "Change…").is_none(),
        "the dialog states one start point and offers no second list"
    );

    type_into_name(&mut harness, "f");

    assert_eq!(
        galley_origin(&harness, "Start from:"),
        Some(before),
        "the stated start point does not move when the list appears"
    );
    assert_eq!(name_field(&harness).rect(), field_before);
    let row = row_galley(&harness, "feat/").expect("the list is open");
    assert!(
        row.pos.y > field_before.bottom(),
        "the list hangs below the field it completes: {:?} vs {field_before:?}",
        row.pos
    );
}

#[test]
fn the_pre_fill_is_the_branch_the_action_was_pointed_at() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);

    harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .into_iter()
        .find(|n| n.accesskit_node().label() == Some("release/2.4".to_owned()))
        .expect("the local release/2.4 row")
        .click_secondary();
    settle_quiet(&mut harness);
    harness.get_by_label("New branch from").click();
    settle_quiet(&mut harness);

    assert_eq!(harness.state().ui.dlg.new_branch.name, "release/2.4");
    assert_painted(&harness, "release/2.4");
    // The start point `release/2.4` is not the branch the reader is on, so it wears
    // no `current` qualifier even though its name is now in the field.
    assert_not_painted(&harness, "(current)");
}

#[test]
fn a_remote_row_states_the_remote_spelling_and_the_branch_starts_there() {
    let (_tmp, dir) = seeded_project();
    let repo = repo_of(&dir);
    let mut harness = harness_for(dir.clone());
    open_branches(&mut harness);

    // Reveal the remotes and narrow the tree so the identically-named pair is on
    // screen together: the guard that the local branch and the remote-tracking ref
    // share a bare name, which is what makes the tip-equality claim below real.
    harness.state_mut().ui.branches_filter = "release".into();
    harness.state_mut().ui.branches_tree.show_remotes = true;
    settle_quiet(&mut harness);
    let remote_rows: Vec<_> = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter(|n| n.accesskit_node().label() == Some("release/2.4".to_owned()))
        .collect();
    assert_eq!(
        remote_rows.len(),
        2,
        "the local branch and the remote-tracking ref share a bare name"
    );

    // The remote row's own `Branch`, read from the surface that painted it, fed to
    // `open_new_branch` through the same `BranchSpec::from(&Branch)` spelling the
    // row menu's `New branch from` uses. `From<&Branch>` is what carries `remote`
    // alongside `name`, so dropping it would resolve to the local of the same name.
    let root = harness.state().multi.roots[0].id.clone();
    let remote_branch = harness
        .state()
        .multi
        .by_id(&root)
        .and_then(|r| {
            r.branches
                .iter()
                .find(|b| b.kind == BranchKind::Remote && b.name == "release/2.4")
        })
        .expect("the remote-tracking release/2.4 row")
        .clone();
    harness.state_mut().open_new_branch(
        NewBranchBase::Branch(BranchSpec::from(&remote_branch)),
        true,
        None,
    );
    settle_quiet(&mut harness);

    assert_eq!(
        harness.state().ui.dlg.new_branch.start,
        NewBranchBase::Branch(BranchSpec::remote("origin", "release/2.4")),
        "the remote row reaches the reducer as a ref, not a name"
    );
    assert_painted(&harness, "origin/release/2.4");
    assert_painted(&harness, "(remote)");

    type_into_name(&mut harness, "cut-from-remote");
    harness.get_by_label("Create").click();
    pump_until(&mut harness, "the branch exists", |s| {
        s.multi
            .roots
            .first()
            .is_some_and(|r| r.branches.iter().any(|b| b.name == "cut-from-remote"))
    });

    let root = RootId(repo.clone().into());
    let tip = harness
        .state()
        .multi
        .by_id(&root)
        .and_then(|r| r.branches.iter().find(|b| b.name == "cut-from-remote"))
        .and_then(|b| b.tip.as_ref())
        .map(|t| t.short_hash.clone());
    let remote_tip = git_with_env(
        &repo,
        &["rev-parse", "origin/release/2.4"],
        &GitEnv::fixture(),
    );
    let local_tip = git_with_env(&repo, &["rev-parse", "release/2.4"], &GitEnv::fixture());
    assert_ne!(
        remote_tip.trim(),
        local_tip.trim(),
        "the fixture must keep the two refs apart or this proves nothing"
    );
    let created = git_with_env(&repo, &["rev-parse", "cut-from-remote"], &GitEnv::fixture());
    assert_eq!(
        created.trim(),
        remote_tip.trim(),
        "a branch cut from a remote row starts at the remote-tracking ref, not at \
         the local branch that happens to share its name"
    );
    assert!(
        tip.is_some(),
        "the refresh reports the new branch's tip to the surface"
    );
}

#[test]
fn an_invocation_remembers_nothing_it_was_not_given() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);

    opened_dialog(&mut harness);
    type_into_name(&mut harness, "half-typed-name");
    harness.state_mut().ui.branches_tree.selected = Some("zeta".into());
    harness.get_by_label("Cancel").click();
    settle_quiet(&mut harness);
    assert_eq!(harness.state().ui.dialog, None);

    opened_dialog(&mut harness);
    assert_eq!(
        harness.state().ui.dlg.new_branch.name,
        "main",
        "the typed residue is gone"
    );
    assert_eq!(
        harness.state().ui.dlg.new_branch.suggest,
        turbogit_app::new_branch::SuggestState::default(),
        "and so is the fold and the highlight"
    );

    harness.get_by_label("Cancel").click();
    settle_quiet(&mut harness);
    let root = harness.state().multi.roots[0].id.clone();
    harness.state_mut().open_new_branch(
        NewBranchBase::Branch(BranchSpec::remote("origin", "release/2.4")),
        false,
        None,
    );
    settle_quiet(&mut harness);
    assert_eq!(harness.state().ui.dlg.new_branch.root, Some(root));
    harness.get_by_label("Cancel").click();
    settle_quiet(&mut harness);

    opened_dialog(&mut harness);
    assert_painted(&harness, "(current)");
    assert_not_painted(&harness, "origin/release/2.4");
    assert!(
        harness.state().ui.dlg.new_branch.checkout,
        "the last invocation opened with `checkout` off; this one is the palette's, which \
         asked to switch, so the earlier decision did not carry over"
    );
}

#[test]
fn opening_again_replaces_the_session_rather_than_patching_it() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);

    opened_dialog(&mut harness);
    type_into_name(&mut harness, "fe");
    harness.key_press(Key::ArrowDown);
    let first = harness.state().ui.dlg.new_branch.clone();

    let root = first.root.clone();
    harness.state_mut().open_new_branch(
        NewBranchBase::Branch(BranchSpec::local("zeta")),
        true,
        None,
    );
    settle_quiet(&mut harness);
    let again = harness.state().ui.dlg.new_branch.clone();

    assert_eq!(again.root, root);
    assert_eq!(again.name, "zeta", "the typed residue is gone");
    assert_eq!(
        again.suggest,
        turbogit_app::new_branch::SuggestState::default()
    );
    assert_ne!(
        again.generation, first.generation,
        "and the salt moved, so the previous field's egui state is not this one's"
    );
}

#[test]
fn enter_creates_when_nothing_is_armed_and_accepts_when_something_is() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    opened_dialog(&mut harness);

    type_into_name(&mut harness, "fe");
    harness.key_press(Key::ArrowDown);
    settle_quiet(&mut harness);
    assert!(
        armed_fill(&harness).is_some(),
        "Down arms a row and paints its fill"
    );

    harness.key_press(Key::Enter);
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.dlg.new_branch.name,
        "feat/",
        "Enter does what the highlighted row says"
    );
    assert!(!branch_created(&harness, "feat/"), "and it created nothing");

    type_into_name(&mut harness, "build-cache");
    assert_eq!(
        harness.state().ui.dlg.new_branch.name,
        "feat/build-cache",
        "the caret came back to the end of the accepted row"
    );
    harness.key_press(Key::Enter);
    pump_until(&mut harness, "the accepted-and-finished name", |s| {
        s.multi
            .roots
            .first()
            .is_some_and(|r| r.branches.iter().any(|b| b.name == "feat/build-cache"))
    });
}

#[test]
fn tab_arms_the_first_row_and_stays_in_the_field_while_the_list_shows() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    opened_dialog(&mut harness);

    type_into_name(&mut harness, "fe");
    harness.key_press(Key::Tab);
    settle_quiet(&mut harness);

    assert!(
        armed_fill(&harness).is_some(),
        "Tab arms the first row instead of walking focus"
    );
    assert!(
        name_field(&harness).is_focused(),
        "and focus stays in the field"
    );

    harness.key_press(Key::Tab);
    settle_quiet(&mut harness);
    assert_eq!(harness.state().ui.dlg.new_branch.name, "feat/");
    assert!(offered_rows(&harness).is_empty());
    assert!(name_field(&harness).is_focused());

    // With no list on screen Tab is egui's again, so the checkbox stays reachable.
    harness.key_press(Key::Tab);
    settle_quiet(&mut harness);
    assert!(
        !name_field(&harness).is_focused(),
        "a closed list must not trap the keyboard in the field"
    );
}

#[test]
fn the_highlight_cycles_and_a_stale_arm_is_pruned_by_typing() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    opened_dialog(&mut harness);

    type_into_name(&mut harness, "f");
    harness.key_press(Key::ArrowDown);
    settle_quiet(&mut harness);
    let first = armed_fill(&harness).expect("Down arms");
    harness.key_press(Key::ArrowDown);
    settle_quiet(&mut harness);
    let second = armed_fill(&harness).expect("a second Down arms another row");
    assert_ne!(first, second, "the highlight moved");
    harness.key_press(Key::ArrowUp);
    settle_quiet(&mut harness);
    assert_eq!(armed_fill(&harness), Some(first), "and Up walks it back");

    // The stale-arm case: the armed text stopped being an offer, so Enter creates
    // what the reader typed rather than accepting a row that is gone.
    type_into_name(&mut harness, "eiz");
    settle_quiet(&mut harness);
    assert!(offered_rows(&harness).is_empty());
    harness.key_press(Key::Enter);
    pump_until(&mut harness, "the typed name is created", |s| {
        s.multi.roots.first().is_some_and(|r| {
            r.branches
                .iter()
                .any(|b| b.name == "feiz" || b.name == "feat/eiz" || b.name == "feature/eiz")
        })
    });
    assert!(
        !branch_created(&harness, "feat/"),
        "no armed row was accepted behind the reader's back"
    );
}

#[test]
fn escape_folds_the_list_then_closes_the_dialog_and_touches_nothing_behind_it() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    let root = harness.state().multi.roots[0].id.clone();
    harness.state_mut().ui.branches_filter = "feat".into();
    harness.state_mut().ui.branches_tree.selected = Some("zeta".into());
    harness.state_mut().ui.branches_tree.selected_root = Some(root);
    settle_quiet(&mut harness);

    opened_dialog(&mut harness);
    type_into_name(&mut harness, "f");
    assert!(!offered_rows(&harness).is_empty());

    harness.key_press(Key::Escape);
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::NewBranch),
        "the first Escape folds the list"
    );
    assert!(offered_rows(&harness).is_empty());
    assert_eq!(
        harness.state().ui.branches_filter,
        "feat",
        "and it cleared nothing on the surface behind the window"
    );
    assert_eq!(
        harness.state().ui.branches_tree.selected.as_deref(),
        Some("zeta"),
        "not even the selection"
    );

    type_into_name(&mut harness, "e");
    assert!(
        !offered_rows(&harness).is_empty(),
        "typing again answers the fold"
    );

    // The reopened list folds under its own Escape before the dialog answers one;
    // Escape closes only from the folded state, never from an open list.
    harness.key_press(Key::Escape);
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::NewBranch),
        "the Escape over the reopened list folds it"
    );
    assert!(offered_rows(&harness).is_empty());

    harness.key_press(Key::Escape);
    settle_quiet(&mut harness);
    assert_eq!(harness.state().ui.dialog, None);
    assert_eq!(
        harness.state().ui.branches_filter,
        "feat",
        "the Escape that closed the dialog was the dialog's, not the surface's"
    );
    assert_eq!(
        harness.state().ui.branches_tree.selected.as_deref(),
        Some("zeta")
    );
}

#[test]
fn arrows_never_move_focus_and_a_branch_selected_behind_the_dialog_stays_selected() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    let root = harness.state().multi.roots[0].id.clone();
    harness.state_mut().ui.branches_tree.selected = Some("zeta".into());
    harness.state_mut().ui.branches_tree.selected_root = Some(root);
    settle_quiet(&mut harness);

    opened_dialog(&mut harness);
    for key in [Key::ArrowDown, Key::ArrowUp, Key::ArrowDown] {
        harness.key_press(key);
        settle_quiet(&mut harness);
        assert!(
            name_field(&harness).is_focused(),
            "an arrow key does not walk out of the name field"
        );
    }
    assert_eq!(
        harness.state().ui.branches_tree.selected.as_deref(),
        Some("zeta"),
        "the tree behind the dialog took none of them"
    );
    assert_eq!(
        harness.state().multi.roots[0].current_branch.as_deref(),
        Some("main"),
        "and Enter is still the dialog's"
    );
}

#[test]
fn an_empty_name_creates_nothing_and_says_so() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    opened_dialog(&mut harness);

    type_into_name(&mut harness, "zeta");
    // Select-all, then Delete: the buffer is empty and pre-filled no more.
    harness.state_mut().ui.dlg.new_branch.name.clear();
    settle_quiet(&mut harness);
    assert!(
        harness
            .get_by_label("Create")
            .accesskit_node()
            .is_disabled(),
        "the affirmative is gated on a name"
    );

    harness.key_press(Key::Enter);
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().ui.dialog,
        Some(Dialog::NewBranch),
        "Enter cannot bypass the gate the button wears"
    );
    assert_eq!(harness.state().ui.dlg.new_branch.name, "");
}

#[test]
fn only_open_new_branch_may_open_the_new_branch_dialog() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let writers: Vec<String> = srcscan::crate_sources(&crates)
        .into_iter()
        .filter(|(_, path)| {
            srcscan::code_only(&srcscan::read_source(path)).contains("Some(Dialog::NewBranch)")
        })
        .map(|(krate, path)| format!("{krate}:{}", path.file_name().unwrap().to_string_lossy()))
        .collect();
    assert_eq!(
        writers,
        vec!["turbogit-app:state.rs".to_owned()],
        "`Dialog::NewBranch` is opened by one function, because it is the only \
         place a session is replaced whole. A surface that assigns the dialog \
         directly re-shows the last invocation's typed name and start point."
    );

    let state = srcscan::read_source(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("crates/turbogit-app/src/state.rs"),
    );
    let code = srcscan::code_with_literals(&state);
    assert!(
        srcscan::fn_code(&code, "open_new_branch").contains("Some(Dialog::NewBranch)"),
        "and the one write is inside `open_new_branch` itself"
    );
}

#[test]
fn a_completion_row_wears_the_menu_row_height() {
    let (_tmp, dir) = seeded_project();
    let mut harness = harness_for(dir);
    open_branches(&mut harness);
    opened_dialog(&mut harness);
    type_into_name(&mut harness, "f");

    for label in ["feat/", "feature/"] {
        let row = harness
            .get_by_role_and_label(egui::accesskit::Role::Button, label)
            .rect();
        assert_eq!(
            row.height(),
            GROUP_ROW_HEIGHT,
            "the `{label}` row is a menu row, so it is GROUP_ROW_HEIGHT tall"
        );
    }
}

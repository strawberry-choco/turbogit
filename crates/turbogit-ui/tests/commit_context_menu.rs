use std::path::Path;
use std::sync::Arc;

use turbogit_domain::model::{Commit, CommitId, RootId, Signature, SignatureState};

use turbogit_ui::ui::commit_menu::{CommitFacts, CommitMenuAction, commit_menu_items};
use turbogit_ui::ui::rewrite_gating::Scope;
use turbogit_ui::ui::widgets::MenuItemState;

fn commit(subject: &str, parents: usize) -> Commit {
    let who = Signature {
        name: "Author".into(),
        email: "author@example.com".into(),
        time: 1_700_000_000,
    };
    Commit {
        id: "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into(),
        parents: (0..parents)
            .map(|n| format!("cafe{n:0>46}"))
            .collect::<Vec<CommitId>>(),
        author: who.clone(),
        committer: who,
        message: format!("{subject}\n\nThe body the pane still shows."),
        time: 1_700_000_000,
        root: RootId(Arc::from(Path::new("/repo/alpha"))),
        signature: SignatureState::Unsigned,
    }
}

const CLEAN: CommitFacts = CommitFacts {
    dirty: false,
    protected_branch: false,
    on_current_branch: true,
    multi_root: true,
    owner: None,
    scope: Scope::Own,
    repo_name: "alpha",
};

#[track_caller]
fn state(facts: CommitFacts, target: &Commit, item: CommitMenuAction) -> MenuItemState {
    commit_menu_items(target, &facts)[item.index()]
}

#[test]
fn the_designed_order_is_the_array_slots() {
    assert_eq!(CommitMenuAction::ORDER.len(), 11);
    for (slot, action) in CommitMenuAction::ORDER.iter().enumerate() {
        assert_eq!(action.index(), slot, "{action:?} is painted in slot {slot}");
    }
}

#[test]
fn an_ordinary_commit_on_the_current_branch_offers_everything() {
    let target = commit("feature: one", 1);
    let items = commit_menu_items(&target, &CLEAN);
    for (action, item) in CommitMenuAction::ORDER.iter().zip(items) {
        assert_eq!(item, MenuItemState::enabled(), "{action:?}");
    }
}

#[test]
fn a_dirty_worktree_states_itself_on_every_verb_that_needs_a_clean_tree() {
    let target = commit("feature: one", 1);
    let dirty = CommitFacts {
        dirty: true,
        ..CLEAN
    };
    let s = |item: CommitMenuAction| state(dirty, &target, item);

    for action in [
        CommitMenuAction::CherryPickTo,
        CommitMenuAction::CherryPickAcross,
        CommitMenuAction::Checkout,
        CommitMenuAction::RevertCommit,
    ] {
        assert_eq!(
            s(action),
            MenuItemState::disabled("Resolve the uncommitted changes first"),
            "{action:?}"
        );
    }
    for action in [
        CommitMenuAction::CopyHash,
        CommitMenuAction::CopyMessage,
        CommitMenuAction::CreatePatch,
        CommitMenuAction::NewBranch,
        CommitMenuAction::NewTag,
    ] {
        assert_eq!(s(action), MenuItemState::enabled(), "{action:?}");
    }
}

#[test]
fn a_merge_commit_states_why_it_can_be_neither_dropped_nor_reworded() {
    let merge = commit("Merge branch 'side'", 2);
    assert_eq!(
        state(CLEAN, &merge, CommitMenuAction::DropCommit),
        MenuItemState::disabled("a merge commit cannot be dropped")
    );
    assert_eq!(
        state(CLEAN, &merge, CommitMenuAction::RewordCommit),
        MenuItemState::disabled("a merge commit cannot be reworded")
    );
    assert_eq!(
        state(CLEAN, &merge, CommitMenuAction::CopyHash),
        MenuItemState::enabled()
    );
    assert_eq!(
        state(CLEAN, &merge, CommitMenuAction::RevertCommit),
        MenuItemState::enabled()
    );
}

#[test]
fn a_root_commit_states_why_it_can_be_neither_dropped_nor_reworded() {
    let root = commit("the beginning of this history", 0);
    assert_eq!(
        state(CLEAN, &root, CommitMenuAction::DropCommit),
        MenuItemState::disabled("a root commit cannot be dropped")
    );
    assert_eq!(
        state(CLEAN, &root, CommitMenuAction::RewordCommit),
        MenuItemState::disabled("a root commit cannot be reworded")
    );
    for action in [
        CommitMenuAction::CopyHash,
        CommitMenuAction::CopyMessage,
        CommitMenuAction::NewBranch,
        CommitMenuAction::RevertCommit,
    ] {
        assert_eq!(
            state(CLEAN, &root, action),
            MenuItemState::enabled(),
            "{action:?}"
        );
    }
}

#[test]
fn a_commit_off_the_current_branch_is_read_only_and_says_so() {
    let elsewhere = commit("someone else's work", 1);
    let off_branch = CommitFacts {
        on_current_branch: false,
        ..CLEAN
    };
    assert_eq!(
        state(off_branch, &elsewhere, CommitMenuAction::DropCommit),
        MenuItemState::disabled("not on the current branch")
    );
    assert_eq!(
        state(off_branch, &elsewhere, CommitMenuAction::RewordCommit),
        MenuItemState::disabled("not on the current branch")
    );
    assert_eq!(
        state(off_branch, &elsewhere, CommitMenuAction::CherryPickTo),
        MenuItemState::enabled()
    );
}

#[test]
fn a_protected_current_branch_refuses_the_history_verbs() {
    let target = commit("feature: one", 1);
    let protected = CommitFacts {
        protected_branch: true,
        ..CLEAN
    };
    for action in [
        CommitMenuAction::RevertCommit,
        CommitMenuAction::DropCommit,
        CommitMenuAction::RewordCommit,
    ] {
        assert_eq!(
            state(protected, &target, action),
            MenuItemState::disabled("the current branch is protected"),
            "{action:?}"
        );
    }
    assert_eq!(
        state(protected, &target, CommitMenuAction::NewBranch),
        MenuItemState::enabled()
    );
}

#[test]
fn one_repository_states_why_the_cross_repository_pick_is_unavailable() {
    let target = commit("feature: one", 1);
    let single = CommitFacts {
        multi_root: false,
        ..CLEAN
    };
    assert_eq!(
        state(single, &target, CommitMenuAction::CherryPickAcross),
        MenuItemState::disabled("only one repository is open")
    );
    assert_eq!(
        state(single, &target, CommitMenuAction::CherryPickTo),
        MenuItemState::enabled()
    );
}

#[test]
fn the_worktree_guard_speaks_before_the_structural_ones() {
    let merge = commit("Merge branch 'side'", 2);
    let blocked = CommitFacts {
        dirty: true,
        protected_branch: true,
        on_current_branch: false,
        multi_root: false,
        owner: None,
        scope: Scope::Own,
        repo_name: "alpha",
    };
    assert_eq!(
        state(blocked, &merge, CommitMenuAction::CherryPickAcross),
        MenuItemState::disabled("Resolve the uncommitted changes first")
    );
    assert_eq!(
        state(blocked, &merge, CommitMenuAction::RevertCommit),
        MenuItemState::disabled("Resolve the uncommitted changes first")
    );
    assert_eq!(
        state(blocked, &merge, CommitMenuAction::DropCommit),
        MenuItemState::disabled("a merge commit cannot be dropped")
    );
}

use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use test_support::harness::{filled_rects, painted_galleys, widget_harness};
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::commit_menu::commit_menu;

fn menu_harness(
    target: Commit,
    facts: CommitFacts<'static>,
) -> (Harness<'static, ()>, Rc<RefCell<Option<CommitMenuAction>>>) {
    let returned = Rc::new(RefCell::new(None));
    let returned_ui = returned.clone();
    let mut harness = widget_harness(egui::vec2(420.0, 460.0), move |ui| {
        let action = commit_menu(ui, &target, &facts);
        if action.is_some() {
            *returned_ui.borrow_mut() = action;
        }
    });
    harness.step();
    (harness, returned)
}

#[test]
fn the_menu_renders_the_eleven_items_in_the_designed_order() {
    let (harness, _returned) = menu_harness(commit("feature: one", 1), CLEAN);

    let mut galleys = painted_galleys(&harness);
    let row_pitch = 26.0;
    let first = galleys
        .iter()
        .map(|g| g.pos.y)
        .fold(f32::INFINITY, f32::min);
    let row = |y: f32| ((y - first) / row_pitch).round() as i32;
    galleys.sort_by(|a, b| {
        row(a.pos.y)
            .cmp(&row(b.pos.y))
            .then(a.pos.x.partial_cmp(&b.pos.x).unwrap())
    });
    let texts: Vec<&str> = galleys.iter().map(|g| g.text.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "Copy hash",
            "deadbee",
            "Copy commit message",
            "Create patch",
            "Cherry-pick to…",
            "alpha",
            "Cherry-pick across…",
            "Checkout",
            "Revert commit",
            "Drop commit",
            "Reword commit",
            "New branch",
            "New tag",
        ],
        "item order and the two data segments"
    );

    let data = galleys
        .iter()
        .find(|g| g.text == "deadbee")
        .expect("the short reference paints");
    assert_eq!(data.family, egui::FontFamily::Monospace);

    let rules = filled_rects(&harness)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::RULE_STRUCTURAL)
        .count();
    assert_eq!(rules, 3, "exactly three menu rules");
}

#[track_caller]
fn row_segments<S>(harness: &Harness<'_, S>, label: &str) -> Vec<String> {
    let galleys = painted_galleys(harness);
    let y = galleys
        .iter()
        .find(|g| g.text == label)
        .unwrap_or_else(|| panic!("the {label:?} row paints"))
        .pos
        .y;
    let menu_left = harness
        .get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some("Copy hash".to_string()))
        .map(|n| n.rect().min.x)
        .unwrap_or_else(|| {
            panic!("the commit menu is open (Copy hash item), so {label:?} is in it")
        });
    let mut segments: Vec<(f32, String)> = galleys
        .iter()
        .filter(|g| (g.pos.y - y).abs() < 2.0 && g.pos.x >= menu_left - 2.0)
        .map(|g| (g.pos.x, g.text.clone()))
        .collect();
    segments.sort_by(|a, b| a.0.total_cmp(&b.0));
    segments.into_iter().map(|(_, text)| text).collect()
}

#[test]
fn cherry_pick_to_names_the_repository_only_when_there_is_more_than_one() {
    let (harness, _returned) = menu_harness(commit("feature: one", 1), CLEAN);
    assert_eq!(
        row_segments(&harness, "Cherry-pick to…"),
        vec!["Cherry-pick to…", "alpha"],
        "in a multi-repository project the item says which one it would land in"
    );
    let name = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "alpha")
        .expect("the repository name paints");
    assert_eq!(
        name.family,
        egui::FontFamily::Monospace,
        "a repository name is a value: it reads in the data face, never the label's"
    );

    let single = CommitFacts {
        multi_root: false,
        ..CLEAN
    };
    let (harness, _returned) = menu_harness(commit("feature: one", 1), single);
    assert_eq!(
        row_segments(&harness, "Cherry-pick to…"),
        vec!["Cherry-pick to…"],
        "one repository needs no naming, and the single-repository menu is unchanged"
    );
}

#[test]
fn no_item_is_primary_and_only_drop_commit_is_danger() {
    let (harness, _returned) = menu_harness(commit("feature: one", 1), CLEAN);

    assert!(
        !filled_rects(&harness)
            .iter()
            .any(|(_, fill)| *fill == Palette::BRAND),
        "brand ink on a copy action would be absurd: a commit menu has no primary"
    );

    let danger: Vec<String> = painted_galleys(&harness)
        .into_iter()
        .filter(|g| g.color == Palette::DANGER)
        .map(|g| g.text)
        .collect();
    assert_eq!(danger, vec!["Drop commit".to_string()]);
}

#[test]
fn reword_commit_is_offered_and_returns_its_action() {
    let (mut harness, returned) = menu_harness(commit("feature: one", 1), CLEAN);
    assert!(
        !harness
            .get_by_label("Reword commit")
            .accesskit_node()
            .is_disabled(),
        "an ordinary commit on an ordinary branch can be reworded"
    );
    harness.get_by_label("Reword commit").click();
    harness.step();
    assert_eq!(
        *returned.borrow(),
        Some(CommitMenuAction::RewordCommit),
        "and the pick reaches the surface, which opens the message editor"
    );
}

#[test]
fn reword_commit_is_not_painted_as_danger() {
    let (harness, _returned) = menu_harness(commit("feature: one", 1), CLEAN);
    let danger: Vec<String> = painted_galleys(&harness)
        .into_iter()
        .filter(|g| g.color == Palette::DANGER)
        .map(|g| g.text)
        .collect();
    assert!(
        !danger.iter().any(|row| row == "Reword commit"),
        "a reword can lose nothing, so it carries no danger ink: {danger:?}"
    );
}

#[test]
fn drop_commit_is_offered_and_returns_its_action() {
    let (mut harness, returned) = menu_harness(commit("feature: one", 1), CLEAN);
    assert!(
        !harness
            .get_by_label("Drop commit")
            .accesskit_node()
            .is_disabled(),
        "an ordinary commit on an ordinary branch can be dropped"
    );
    harness.get_by_label("Drop commit").click();
    harness.step();
    assert_eq!(
        *returned.borrow(),
        Some(CommitMenuAction::DropCommit),
        "and the pick reaches the surface, which opens the preflight"
    );
}

#[test]
fn clicking_an_item_returns_its_action_and_a_blocked_one_returns_nothing() {
    let (mut harness, returned) = menu_harness(commit("feature: one", 1), CLEAN);
    harness.get_by_label("New tag").click();
    harness.step();
    assert_eq!(
        *returned.borrow(),
        Some(CommitMenuAction::NewTag),
        "the component reports the pick and nothing else"
    );

    let single = CommitFacts {
        multi_root: false,
        ..CLEAN
    };
    let (mut harness, returned) = menu_harness(commit("feature: one", 1), single);
    assert!(
        harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .any(|n| n.accesskit_node().label() == Some("Cherry-pick across…".to_string())),
        "a gated item stays visible"
    );
    let item = harness.get_by_label("Cherry-pick across…");
    assert!(
        item.accesskit_node().is_disabled(),
        "the accessibility node carries the gate"
    );
    item.click();
    harness.step();
    assert_eq!(*returned.borrow(), None, "and swallows its own click");
}

#[test]
fn a_blocked_item_states_its_reason_on_hover() {
    let dirty = CommitFacts {
        dirty: true,
        ..CLEAN
    };
    let (mut harness, _returned) = menu_harness(commit("feature: one", 1), dirty);
    assert_menu_item_disabled(
        &mut harness,
        "Revert commit",
        "Resolve the uncommitted changes first",
    );
}

#[track_caller]
fn assert_menu_item_disabled(harness: &mut Harness<'_, ()>, label: &str, reason: &str) {
    harness.remove_cursor();
    harness.step();
    let item = harness.get_by_label(label);
    assert!(
        item.accesskit_node().is_disabled(),
        "{label} must be gated, not hidden"
    );
    let rect = item.rect();
    harness.hover_at(rect.center() - egui::vec2(0.0, 3.0));
    harness.step();
    harness.hover_at(rect.center());
    let mut seen = false;
    for _ in 0..80 {
        harness.step();
        if painted_galleys(harness)
            .iter()
            .any(|g| g.text.contains(reason))
        {
            seen = true;
            break;
        }
    }
    assert!(
        seen,
        "the blocked action must explain itself: {reason} never painted; painted {:?}",
        painted_galleys(harness)
            .iter()
            .map(|g| &g.text)
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_whole_menu_fits_the_window_s_minimum_height() {
    let (harness, _returned) = menu_harness(commit("feature: one", 1), CLEAN);
    const WINDOW_MIN_HEIGHT: f32 = 680.0;
    let rows = painted_galleys(&harness);
    let top = rows.iter().map(|g| g.pos.y).fold(f32::MAX, f32::min);
    let bottom = rows.iter().map(|g| g.rect.max.y).fold(f32::MIN, f32::max);
    let menu_height = (bottom - top) + 2.0 * 8.0;
    assert!(
        menu_height < WINDOW_MIN_HEIGHT,
        "eleven items and three rules measure {menu_height}px against a \
         {WINDOW_MIN_HEIGHT}px window"
    );
}

mod surface {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::Duration;

    use egui::Modifiers;
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use tempfile::TempDir;
    use test_support::git_seed::{GitEnv, git_with_env, pin_eol_lf};
    use test_support::harness::{
        assert_menu_item_gated, assert_not_painted, assert_painted, filled_rects, painted_galleys,
    };
    use turbogit_app::events::{AppEvent, LogBatchMode};
    use turbogit_app::state::{AppState, Tab};
    use turbogit_domain::model::{LogOpts, RootId, VcsSettings};
    use turbogit_engine::cli::CliExecutor;
    use turbogit_engine_api::GitExecutor;
    use turbogit_ui::theme::Palette;

    pub(crate) struct Seed {
        _tmp: TempDir,
        pub project: PathBuf,
        pub alpha: PathBuf,
        pub c1: String,
        pub c2: String,
        pub base: String,
    }

    fn commit_file(dir: &Path, name: &str, body: &str, msg: &str, epoch: i64) -> String {
        let stamp = format!("@{epoch} +0000");
        let mut env = GitEnv::fixture();
        env.author_date = Some(stamp.clone());
        env.committer_date = Some(stamp);
        std::fs::write(dir.join(name), body).unwrap();
        git_with_env(dir, &["add", "."], &env);
        git_with_env(dir, &["commit", "-q", "-m", msg], &env);
        git_with_env(dir, &["rev-parse", "HEAD"], &env)
            .trim()
            .to_string()
    }

    fn commit_as_configured(dir: &Path, name: &str, body: &str, msg: &str, epoch: i64) -> String {
        let stamp = format!("@{epoch} +0000");
        let run = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_COMMITTER_DATE", &stamp)
                .env("GIT_AUTHOR_DATE", &stamp)
                .output()
                .expect("git must be on PATH");
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        std::fs::write(dir.join(name), body).unwrap();
        run(&["add", "."]);
        run(&["commit", "-q", "-m", msg]);
        git_with_env(dir, &["rev-parse", "HEAD"], &GitEnv::fixture())
            .trim()
            .to_string()
    }

    pub(crate) fn seeded_repo(tag: &str) -> Seed {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join(tag);
        let alpha = project.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        git_with_env(&alpha, &["init", "-q", "-b", "main"], &GitEnv::fixture());
        git_with_env(&alpha, &["config", "user.email", "t@t"], &GitEnv::fixture());
        git_with_env(&alpha, &["config", "user.name", "t"], &GitEnv::fixture());
        pin_eol_lf(&alpha);
        let base = commit_file(&alpha, "base.txt", "base\n", "the base commit", 100);
        let c1 = commit_file(&alpha, "a.txt", "one\n", "alpha: first commit", 200);
        let c2 = commit_file(
            &alpha,
            "b.txt",
            "two\n",
            "alpha: second commit\n\nThe body the pane still shows.",
            300,
        );
        git_with_env(&alpha, &["branch", "feature", &c1], &GitEnv::fixture());
        Seed {
            _tmp: tmp,
            project,
            alpha,
            base,
            c1,
            c2,
        }
    }

    pub(crate) fn log_harness(seed: &Seed) -> Harness<'static, AppState> {
        let mut state = AppState::new(seed.project.clone());
        let engine = CliExecutor {
            settings: VcsSettings::default(),
        };
        for root in state.multi.roots.clone() {
            let commits = engine.log(&root.path, &LogOpts::default()).expect("log");
            state
                .tx
                .send(AppEvent::LogLoaded {
                    root: root.id.clone(),
                    commits: Ok(commits),
                    mode: LogBatchMode::Replace,
                })
                .expect("send LogLoaded");
        }
        state.drain_events();
        state.ui.tab = Tab::Log;
        harness_over_state(state)
    }

    pub(crate) fn warm_logs(harness: &mut Harness<'_, AppState>) {
        let engine = CliExecutor {
            settings: VcsSettings::default(),
        };
        let roots = harness.state().multi.roots.clone();
        for root in roots {
            let commits = engine.log(&root.path, &LogOpts::default()).expect("log");
            harness
                .state_mut()
                .tx
                .send(AppEvent::LogLoaded {
                    root: root.id.clone(),
                    commits: Ok(commits),
                    mode: LogBatchMode::Replace,
                })
                .unwrap();
        }
        harness.step();
    }

    fn harness_over_state(state: AppState) -> Harness<'static, AppState> {
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
        harness.set_size(egui::vec2(
            1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH,
            800.0,
        ));
        settle(&mut harness);
        harness
    }

    pub(crate) use test_support::harness::settle_quiet as settle;

    pub(crate) fn short(id: &str) -> String {
        id[..7.min(id.len())].to_string()
    }

    pub(crate) fn row_node<'t>(
        harness: &'t Harness<'_, AppState>,
        subject: &str,
    ) -> egui_kittest::Node<'t> {
        harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .find(|n| {
                n.accesskit_node()
                    .label()
                    .is_some_and(|l| l.contains(subject))
            })
            .unwrap_or_else(|| panic!("commit row for {subject}"))
    }

    pub(crate) use test_support::harness::right_click_row;

    pub(crate) fn menu_target(harness: &Harness<'_, AppState>) -> Option<String> {
        harness
            .state()
            .ui
            .log_commit_menu
            .as_ref()
            .map(|(_, id)| id.clone())
    }

    pub(crate) fn click_menu_item(harness: &mut Harness<'_, AppState>, label: &str) {
        test_support::harness::click_menu_item(harness, "Copy hash", label);
    }

    pub(crate) fn press(harness: &mut Harness<'_, AppState>, label: &str) {
        let node = harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .chain(
                harness
                    .get_all_by_role(egui::accesskit::Role::CheckBox)
                    .collect::<Vec<_>>(),
            )
            .find(|n| n.accesskit_node().label().as_deref() == Some(label))
            .unwrap_or_else(|| panic!("no control labelled `{label}` is painted"));
        node.click();
        harness.step();
    }

    pub(crate) fn accept_patch_settings(harness: &mut Harness<'_, AppState>) {
        assert_eq!(
            harness.state().ui.dialog,
            Some(turbogit_app::state::Dialog::PatchFileSettings),
            "a commit already names itself, so the settings open without the comment step"
        );
        press(harness, "Save to…");
        settle(harness);
        press(harness, "OK");
        settle(harness);
    }

    /// What the last frame asked the platform to put on the clipboard. egui
    /// carries a copy as an output command rather than a field, so this reads
    /// the same gesture the window's clipboard handler would act on.
    pub(crate) fn copied_text(harness: &Harness<'_, AppState>) -> String {
        harness
            .output()
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    pub(crate) fn toast(harness: &Harness<'_, AppState>) -> Option<String> {
        harness.state().ui.toast.as_ref().map(|t| t.message.clone())
    }

    pub(crate) fn wait_for(harness: &mut Harness<'_, AppState>, pred: impl Fn(&AppState) -> bool) {
        for _ in 0..600 {
            harness.step();
            std::thread::sleep(Duration::from_millis(10));
            if pred(harness.state()) {
                return;
            }
        }
        panic!(
            "condition not met; toast={:?} last_error={:?} feed={:?}",
            harness.state().ui.toast,
            harness.state().last_error,
            harness
                .state()
                .ui
                .activity
                .entries
                .iter()
                .map(|e| e.message.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn right_clicking_a_commit_row_opens_the_menu_and_selects_the_row() {
        let seed = seeded_repo("open");
        let mut harness = log_harness(&seed);

        right_click_row(&mut harness, "alpha: second commit");

        assert_eq!(
            menu_target(&harness).as_deref(),
            Some(seed.c2.as_str()),
            "the surface owns the open state, keyed by the right-clicked commit"
        );
        assert_eq!(
            harness.state().ui.selected_commit.as_deref(),
            Some(seed.c2.as_str()),
            "and the press selected the row it opened"
        );
        assert_painted(&harness, "Copy commit message");
        assert_painted(&harness, "Cherry-pick across…");
        assert_painted(&harness, "New tag");
    }

    #[test]
    fn the_menu_closes_on_escape_an_outside_click_and_another_row() {
        let seed = seeded_repo("dismiss");
        let mut harness = log_harness(&seed);

        right_click_row(&mut harness, "alpha: second commit");
        assert!(menu_target(&harness).is_some());
        harness.key_press(egui::Key::Escape);
        harness.step();
        assert_eq!(menu_target(&harness), None, "Escape closes it");

        right_click_row(&mut harness, "alpha: second commit");
        harness.step();
        harness.hover_at(egui::pos2(1400.0, 8.0));
        harness.step();
        harness.drag_at(egui::pos2(1400.0, 8.0));
        harness.step();
        harness.drop_at(egui::pos2(1400.0, 8.0));
        harness.step();
        assert_eq!(menu_target(&harness), None, "a click outside closes it");

        right_click_row(&mut harness, "the base commit");
        assert_eq!(menu_target(&harness).as_deref(), Some(seed.base.as_str()));
        row_node(&harness, "alpha: first commit").click();
        settle(&mut harness);
        assert_eq!(
            menu_target(&harness),
            None,
            "a click on another row closes it"
        );
        assert_eq!(
            harness.state().ui.selected_commit.as_deref(),
            Some(seed.c1.as_str()),
            "and that click still selected the row it landed on"
        );

        right_click_row(&mut harness, "alpha: first commit");
        assert_eq!(
            menu_target(&harness).as_deref(),
            Some(seed.c1.as_str()),
            "a second right-click retargets rather than vanishing"
        );
        assert_painted(&harness, "Copy commit message");
    }

    #[test]
    fn the_two_copy_verbs_copy_different_things_and_say_which() {
        let seed = seeded_repo("copy");
        let mut harness = log_harness(&seed);
        right_click_row(&mut harness, "alpha: second commit");

        click_menu_item(&mut harness, "Copy hash");
        assert_eq!(
            copied_text(&harness),
            seed.c2.clone(),
            "the FULL hash goes to the clipboard"
        );
        assert_eq!(
            toast(&harness).as_deref(),
            Some(format!("Copied {}", short(&seed.c2))).as_deref(),
            "the confirmation names the short reference, not the wall of hex"
        );

        right_click_row(&mut harness, "alpha: second commit");
        click_menu_item(&mut harness, "Copy commit message");
        assert_eq!(
            copied_text(&harness),
            "alpha: second commit\n\nThe body the pane still shows.",
            "subject and body both"
        );
        let message = toast(&harness).expect("a confirmation for the message copy");
        assert!(
            message.to_lowercase().contains("message"),
            "the two confirmations are distinguishable: {message}"
        );
    }

    #[test]
    fn revert_asks_before_creating_the_inverse_commit() {
        let seed = seeded_repo("revert");
        let mut harness = log_harness(&seed);
        harness
            .state_mut()
            .settings
            .protected_branch_patterns
            .clear();

        right_click_row(&mut harness, "alpha: second commit");
        click_menu_item(&mut harness, "Revert commit");
        wait_for(&mut harness, |s| s.ui.confirm.is_some());
        assert!(
            matches!(
                &harness.state().ui.confirm,
                Some(turbogit_app::state::PendingConfirm::RevertCommit { commit }) if commit == &seed.c2
            ),
            "the ask names the right-clicked commit"
        );
        assert_painted(&harness, &format!("Revert commit {}?", short(&seed.c2)));

        harness.get_by_label("OK").click();
        wait_for(&mut harness, |s| {
            s.caches
                .log(&s.selected_root.clone().unwrap())
                .is_some_and(|cs| {
                    cs.iter()
                        .any(|c| c.message.to_lowercase().starts_with("revert"))
                })
        });
        let subject = git_with_env(
            &seed.alpha,
            &["log", "-1", "--format=%s"],
            &GitEnv::fixture(),
        );
        assert!(
            subject.to_lowercase().starts_with("revert"),
            "HEAD is the inverse commit; got {subject:?}"
        );
    }

    #[test]
    fn new_branch_and_new_tag_seed_their_dialogs_with_the_right_clicked_commit() {
        let seed = seeded_repo("seeded");
        let mut harness = log_harness(&seed);

        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "New branch");
        let st = harness.state();
        assert_eq!(st.ui.dialog, Some(turbogit_app::state::Dialog::NewBranch));
        assert_eq!(
            st.ui.dlg.new_branch_base,
            turbogit_app::state::NewBranchBase::Commit(seed.c1.clone()),
            "the base is the commit that was clicked, not the tip"
        );
        assert!(
            !st.ui.dlg.new_branch_checkout,
            "creating a branch does not move me"
        );
        assert_eq!(menu_target(&harness), None, "and the menu closed");

        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "New tag");
        let st = harness.state();
        assert_eq!(
            st.ui.dialog,
            Some(turbogit_app::state::Dialog::Tag),
            "the tag dialog opens"
        );
        assert_eq!(
            st.ui.dlg.tag_target, seed.c1,
            "seeded with the right-clicked commit rather than the tip"
        );
    }

    #[test]
    fn the_cherry_pick_picker_refuses_the_current_branch() {
        let seed = seeded_repo("pick-current");
        let mut harness = log_harness(&seed);
        harness
            .state_mut()
            .settings
            .protected_branch_patterns
            .clear();
        settle(&mut harness);

        right_click_row(&mut harness, "alpha: second commit");
        click_menu_item(&mut harness, "Cherry-pick to…");
        settle(&mut harness);
        assert_eq!(
            harness.state().ui.dialog,
            Some(turbogit_app::state::Dialog::CherryPickTarget),
            "the picker opens for the right-clicked commit"
        );

        let main = harness.get_by_label("main (current)");
        assert!(
            main.accesskit_node().is_disabled(),
            "the current branch is not a destination: it is rendered, not hidden"
        );
        let main_rect = main.rect();
        assert_stated_on_hover(&mut harness, "main (current)", "this is the current branch");

        harness.hover_at(main_rect.center());
        harness.step();
        harness.get_by_label("main (current)").click();
        settle(&mut harness);
        assert_eq!(
            harness.state().ui.dialog,
            Some(turbogit_app::state::Dialog::CherryPickTarget),
            "a click on the refused row changes nothing"
        );
        assert_eq!(
            git_with_env(
                &seed.alpha,
                &["rev-list", "--count", "main"],
                &GitEnv::fixture()
            )
            .trim(),
            "3",
            "and no commit lands on the branch the picker refused"
        );
    }

    #[test]
    fn a_protected_branch_that_is_not_checked_out_is_refused_as_protected() {
        let seed = seeded_repo("pick-protected");
        git_with_env(
            &seed.alpha,
            &["checkout", "-q", "feature"],
            &GitEnv::fixture(),
        );
        let mut harness = log_harness(&seed);
        settle(&mut harness);

        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "Cherry-pick to…");
        settle(&mut harness);

        assert!(
            harness
                .get_by_label("main (protected)")
                .accesskit_node()
                .is_disabled(),
            "a protected branch is refused whether or not it is checked out"
        );
        assert_stated_on_hover(
            &mut harness,
            "main (protected)",
            "'main' is a protected branch",
        );
        assert!(
            harness
                .get_by_label("feature (current)")
                .accesskit_node()
                .is_disabled(),
            "and the current branch is still refused on its own account"
        );
    }

    #[test]
    fn the_tag_dialog_re_opens_from_the_menu_with_the_last_visits_state_cleared() {
        let seed = seeded_repo("tag-reopen");
        let mut harness = log_harness(&seed);

        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "New tag");
        settle(&mut harness);
        harness.get_by_label("Sign with GPG key").click();
        settle(&mut harness);
        assert!(
            harness.state().ui.dlg.tag_signing_key_fetched,
            "ticking the box reads the key, and records that it did"
        );
        harness.get_by_label("Sign with GPG key").click();
        settle(&mut harness);
        harness.get_by_label("Change…").click();
        settle(&mut harness);
        harness.get_by_label("HEAD").click();
        settle(&mut harness);
        harness.get_by_label("Lightweight").click();
        settle(&mut harness);
        let first = harness.state();
        assert_eq!(
            first.ui.dlg.tag_type,
            turbogit_app::state::TagType::Lightweight,
            "the first visit really did leave a Lightweight selection"
        );
        assert!(
            first.ui.dlg.tag_candidates.is_some(),
            "and a cached target list"
        );
        assert!(
            first.ui.dlg.tag_existing.is_some(),
            "and a cached tag list to validate against"
        );
        harness.get_by_label("Cancel").click();
        settle(&mut harness);

        git_with_env(&seed.alpha, &["tag", "v-leaked"], &GitEnv::fixture());

        right_click_row(&mut harness, "alpha: second commit");
        click_menu_item(&mut harness, "New tag");
        settle(&mut harness);

        let st = harness.state();
        assert_eq!(st.ui.dialog, Some(turbogit_app::state::Dialog::Tag));
        assert_eq!(
            st.ui.dlg.tag_target, seed.c2,
            "seeded with the commit that was right-clicked this time"
        );
        assert_eq!(
            st.ui.dlg.tag_type,
            turbogit_app::state::TagType::Annotated,
            "no Lightweight selection survives the close"
        );
        assert!(
            !st.ui.dlg.tag_target_picker_open,
            "and the target picker does not re-open expanded"
        );
        assert!(
            st.ui.dlg.tag_candidates.is_none(),
            "the commit list is read again for this visit"
        );
        assert!(
            !st.ui.dlg.tag_signing_key_fetched && st.ui.dlg.tag_signing_key.is_none(),
            "the signing key is fetched for this visit, not remembered from the last"
        );
        assert_painted(&harness, "MESSAGE");
        assert_painted(&harness, "TAGGER");

        harness.state_mut().ui.dlg.tag_name = "v-leaked".into();
        settle(&mut harness);
        assert_painted(&harness, "A tag named 'v-leaked' already exists");
    }

    #[test]
    fn cancelling_the_tag_dialog_from_the_menu_creates_nothing() {
        let seed = seeded_repo("tag-cancel");
        let mut harness = log_harness(&seed);

        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "New tag");
        settle(&mut harness);
        harness.state_mut().ui.dlg.tag_name = "v9.9.9".into();
        settle(&mut harness);
        assert_painted(&harness, "✓ valid");

        harness.get_by_label("Cancel").click();
        settle(&mut harness);

        assert!(harness.state().ui.dialog.is_none(), "the dialog went");
        assert_eq!(
            git_with_env(&seed.alpha, &["tag", "-l"], &GitEnv::fixture()).trim(),
            "",
            "and no tag was created"
        );
        assert_eq!(
            git_with_env(&seed.alpha, &["rev-parse", "HEAD"], &GitEnv::fixture()).trim(),
            seed.c2,
            "and the repository is exactly where it was"
        );
        assert_eq!(
            git_with_env(
                &seed.alpha,
                &["symbolic-ref", "--short", "HEAD"],
                &GitEnv::fixture()
            )
            .trim(),
            "main",
            "still on the branch it was on"
        );
    }

    pub(crate) struct CrossRoot {
        pub _tmp: TempDir,
        pub harness: Harness<'static, AppState>,
        pub selected: RootId,
        pub selected_path: PathBuf,
        pub row_root: RootId,
        pub row_root_path: PathBuf,
        pub landing: String,
        pub other_landing: String,
        pub commit: String,
        pub parent: String,
    }

    pub(crate) fn cross_root_row(tag: &str) -> CrossRoot {
        let (tmp, project, clean, dirty) = two_root_project(tag);
        let clean_base = git_with_env(&clean, &["rev-parse", "HEAD~1"], &GitEnv::fixture())
            .trim()
            .to_string();
        git_with_env(
            &clean,
            &["branch", "landing", &clean_base],
            &GitEnv::fixture(),
        );
        git_with_env(&dirty, &["branch", "landing", "HEAD"], &GitEnv::fixture());
        let landing = git_with_env(&clean, &["rev-parse", "landing"], &GitEnv::fixture())
            .trim()
            .to_string();
        let other_landing = git_with_env(&dirty, &["rev-parse", "landing"], &GitEnv::fixture())
            .trim()
            .to_string();
        let mut state = AppState::new(project);
        state.drain_events();
        state.ui.tab = Tab::Log;
        let mut harness = harness_over_state(state);
        warm_logs(&mut harness);
        settle(&mut harness);

        let roots = harness.state().multi.roots.clone();
        let id_of = |path: &Path| -> RootId {
            roots
                .iter()
                .find(|r| r.path == path)
                .unwrap_or_else(|| panic!("the {path:?} root"))
                .id
                .clone()
        };
        let (selected, row_root) = (id_of(&dirty), id_of(&clean));
        harness.state_mut().selected_root = Some(selected.clone());
        harness.state_mut().ui.log_filter = "clean work".to_string();
        settle(&mut harness);

        let commit = git_with_env(&clean, &["rev-parse", "HEAD"], &GitEnv::fixture())
            .trim()
            .to_string();
        let parent = clean_base;
        CrossRoot {
            _tmp: tmp,
            harness,
            selected,
            selected_path: dirty,
            row_root,
            row_root_path: clean,
            landing,
            other_landing,
            commit,
            parent,
        }
    }

    #[test]
    fn a_press_on_a_commit_row_selects_its_repository_as_well_as_its_commit() {
        let CrossRoot {
            _tmp,
            mut harness,
            selected,
            row_root,
            commit,
            parent,
            ..
        } = cross_root_row("press-root");
        assert_eq!(
            harness.state().ui.selected_commit,
            None,
            "nothing is selected yet, so the press below is what moves the root"
        );
        assert_eq!(
            harness.state().selected_root,
            Some(selected),
            "the app starts out looking at the other repository"
        );

        row_node(&harness, "clean work commit").click();
        settle(&mut harness);

        let st = harness.state();
        assert_eq!(
            st.selected_root,
            Some(row_root),
            "the press made the row's repository the selected one"
        );
        assert_eq!(
            st.ui.selected_commit.as_deref(),
            Some(commit.as_str()),
            "and the pressed commit the selected commit"
        );
        assert_eq!(
            st.ui.log_selected_file, None,
            "with the previous commit's changed file cleared"
        );
        assert_painted(&harness, "Committer");
        assert_painted(&harness, &short(&parent));
    }

    #[test]
    fn a_menu_item_opens_its_dialog_on_the_pressed_rows_repository() {
        let CrossRoot {
            _tmp,
            mut harness,
            row_root,
            row_root_path,
            selected_path,
            landing,
            other_landing,
            ..
        } = cross_root_row("dialog-root");
        let other_head = git_with_env(&selected_path, &["rev-parse", "HEAD"], &GitEnv::fixture())
            .trim()
            .to_string();

        right_click_row(&mut harness, "clean work commit");
        assert_eq!(
            harness.state().selected_root,
            Some(row_root),
            "the press made the row's repository the selected one"
        );
        click_menu_item(&mut harness, "Cherry-pick to…");
        settle(&mut harness);
        assert_eq!(
            harness.state().ui.dialog,
            Some(turbogit_app::state::Dialog::CherryPickTarget),
            "the picker opened"
        );

        picker_row(&harness, "landing").click();
        wait_for(&mut harness, |s| s.ui.toast.is_some());

        assert_ne!(
            git_with_env(
                &row_root_path,
                &["rev-parse", "landing"],
                &GitEnv::fixture()
            )
            .trim(),
            landing,
            "the cherry-picked commit landed on the branch in the ROW's repository, \
             not nowhere"
        );
        assert_eq!(
            git_with_env(
                &row_root_path,
                &["log", "-1", "--format=%s", "landing"],
                &GitEnv::fixture()
            )
            .trim()
            .to_string(),
            "clean work commit",
            "and it is the row's commit that landed there"
        );
        assert_eq!(
            git_with_env(
                &selected_path,
                &["rev-parse", "landing"],
                &GitEnv::fixture()
            )
            .trim(),
            other_landing,
            "the OTHER repository's branch did not move"
        );
        assert_eq!(
            git_with_env(&selected_path, &["rev-parse", "HEAD"], &GitEnv::fixture()).trim(),
            other_head,
            "nor did its checkout"
        );
    }

    fn picker_row<'h>(
        harness: &'h Harness<'_, AppState>,
        branch: &'h str,
    ) -> egui_kittest::Node<'h> {
        harness
            .query_all_by_label(branch)
            .filter(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
            .min_by_key(|n| n.rect().left() as i32)
            .unwrap_or_else(|| panic!("the picker's {branch:?} row"))
    }

    /// Two roots with DIFFERENT subjects, so a row names its own root: two repos
    /// committed with identical content, author and date produce identical hashes,
    /// and a menu aimed at one could not be told apart from the other.
    pub(crate) fn two_root_project(tag: &str) -> (TempDir, PathBuf, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join(tag);
        let clean = project.join("clean");
        let dirty = project.join("dirty");
        std::fs::create_dir_all(&clean).unwrap();
        std::fs::create_dir_all(&dirty).unwrap();
        for (repo, name) in [(&clean, "clean"), (&dirty, "dirty")] {
            git_with_env(repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
            git_with_env(repo, &["config", "user.email", "t@t"], &GitEnv::fixture());
            git_with_env(repo, &["config", "user.name", "t"], &GitEnv::fixture());
            commit_file(
                repo,
                "base.txt",
                "base\n",
                &format!("{name} base commit"),
                100,
            );
            commit_file(
                repo,
                "work.txt",
                "work\n",
                &format!("{name} work commit"),
                200,
            );
        }
        std::fs::write(dirty.join("uncommitted.txt"), "mine\n").unwrap();
        (tmp, project, clean, dirty)
    }

    #[test]
    fn each_root_is_gated_on_its_own_worktree() {
        let (_tmp, project, clean, dirty) = two_root_project("per-root");
        let mut state = AppState::new(project.clone());
        state.drain_events();
        state.ui.tab = Tab::Log;
        let mut harness = harness_over_state(state);
        warm_logs(&mut harness);
        harness
            .state_mut()
            .refresh(turbogit_app::root_caches::Affected::All);
        settle(&mut harness);
        warm_logs(&mut harness);
        settle(&mut harness);
        let roots = harness.state().multi.roots.clone();
        let clean_id = roots
            .iter()
            .find(|r| r.path == clean)
            .expect("clean")
            .id
            .clone();
        let dirty_id = roots
            .iter()
            .find(|r| r.path == dirty)
            .expect("dirty")
            .id
            .clone();
        assert!(
            harness
                .state()
                .multi
                .by_id(&dirty_id)
                .is_some_and(|r| !r.status.changes.is_empty()),
            "the dirty root's worktree really is dirty once read"
        );

        harness.state_mut().ui.log_root_filter = Some(clean_id);
        settle(&mut harness);
        right_click_row(&mut harness, "clean work commit");
        assert!(
            menu_target(&harness).is_some(),
            "the clean root's menu opened"
        );
        assert_eq!(
            super::row_segments(&harness, "Cherry-pick to…"),
            vec!["Cherry-pick to…", "clean"],
            "with two repositories in scope the item names the one it would land in"
        );
        assert!(
            !is_menu_item_gated(&mut harness, "Cherry-pick to…"),
            "a dirty SIBLING root must not gate the clean root's cherry-pick"
        );

        harness.key_press(egui::Key::Escape);
        settle(&mut harness);
        harness.state_mut().ui.log_root_filter = Some(dirty_id);
        settle(&mut harness);
        right_click_row(&mut harness, "dirty work commit");
        assert_menu_item_gated(&mut harness, "Copy hash", "Cherry-pick to…");
        assert_stated_on_hover(
            &mut harness,
            "Cherry-pick to…",
            "Resolve the uncommitted changes first",
        );
    }

    pub(crate) fn is_menu_item_gated(harness: &mut Harness<'_, AppState>, label: &str) -> bool {
        harness.remove_cursor();
        harness.step();
        harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .find(|n| n.accesskit_node().label() == Some(label.to_string()))
            .unwrap_or_else(|| panic!("menu item {label}"))
            .accesskit_node()
            .is_disabled()
    }

    #[track_caller]
    pub(crate) fn assert_stated_on_hover(
        harness: &mut Harness<'_, AppState>,
        label: &str,
        reason: &str,
    ) {
        harness.remove_cursor();
        harness.step();
        let rect = harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .find(|n| n.accesskit_node().label() == Some(label.to_string()))
            .unwrap_or_else(|| panic!("menu item {label}"))
            .rect();
        harness.hover_at(rect.center() - egui::vec2(0.0, 3.0));
        harness.step();
        harness.hover_at(rect.center());
        for _ in 0..80 {
            harness.step();
            if painted_galleys(harness)
                .iter()
                .any(|g| g.text.contains(reason))
            {
                return;
            }
        }
        panic!("the blocked {label} item never stated {reason:?}");
    }

    #[test]
    fn an_item_click_closes_the_menu_before_its_action_runs() {
        let seed = seeded_repo("item-close");
        let mut harness = log_harness(&seed);
        right_click_row(&mut harness, "alpha: first commit");
        assert!(menu_target(&harness).is_some());

        click_menu_item(&mut harness, "New branch");
        let st = harness.state();
        assert_eq!(st.ui.log_commit_menu, None, "closed");
        assert_eq!(
            st.ui.dialog,
            Some(turbogit_app::state::Dialog::NewBranch),
            "and the action ran"
        );
    }

    #[test]
    fn the_open_menu_keeps_its_own_commit_when_the_selection_moves() {
        let seed = seeded_repo("sticky");
        let mut harness = log_harness(&seed);
        right_click_row(&mut harness, "alpha: second commit");
        let target = menu_target(&harness);
        assert_eq!(target.as_deref(), Some(seed.c2.as_str()));

        harness.state_mut().ui.selected_commit = Some(seed.c1.clone());
        harness.step();
        assert_eq!(
            menu_target(&harness),
            target,
            "the menu still describes the commit it was opened on"
        );
    }

    #[test]
    fn the_unselected_pane_still_reads_as_an_empty_state() {
        let seed = seeded_repo("empty-pane");
        let harness = log_harness(&seed);
        assert_painted(&harness, "Select a commit…");
        assert_not_painted(&harness, "Commit details\'s actions");
    }

    #[test]
    fn checkout_at_a_commit_detaches_head_at_that_exact_commit() {
        let seed = seeded_repo("checkout");
        let mut harness = log_harness(&seed);

        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "Checkout");
        wait_for(&mut harness, |s| {
            s.multi
                .roots
                .first()
                .is_some_and(|r| r.current_branch.is_none())
        });

        assert_eq!(
            git_with_env(&seed.alpha, &["rev-parse", "HEAD"], &GitEnv::fixture()).trim(),
            seed.c1,
            "HEAD stands on the commit that was right-clicked, not on its tip"
        );
        // `git symbolic-ref` exits non-zero once HEAD is detached, that IS the
        // state, so it is read without asserting success.
        let symbolic = Command::new("git")
            .args(["symbolic-ref", "HEAD"])
            .current_dir(&seed.alpha)
            .output()
            .expect("git must be on PATH");
        assert!(
            !symbolic.status.success(),
            "HEAD is no longer a symbolic ref to a branch"
        );
        assert_painted(&harness, "<detached>");
    }

    #[test]
    fn the_drop_preflight_says_what_will_happen_before_anything_runs() {
        let seed = seeded_repo("preflight");
        let mut harness = log_harness(&seed);
        let head = git_with_env(&seed.alpha, &["rev-parse", "HEAD"], &GitEnv::fixture())
            .trim()
            .to_string();
        harness
            .state_mut()
            .settings
            .protected_branch_patterns
            .clear();
        settle(&mut harness);

        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "Drop commit");
        settle(&mut harness);

        assert_eq!(
            harness.state().ui.dialog,
            Some(turbogit_app::state::Dialog::RewritePreflight),
            "the item opens a preflight, not a rewrite"
        );
        assert_painted(&harness, "DROPPED COMMIT");
        assert_painted(&harness, "alpha: first commit");
        assert_painted(&harness, "REPLAYED COMMITS");
        assert_painted(&harness, "alpha: second commit");
        assert_painted(&harness, "2 → 1 COMMITS");
        assert_not_painted(&harness, "REWRITTEN");
        assert_painted(&harness, "~3s estimated");
        assert_painted(&harness, "RECOVERY");
        assert_painted(&harness, "refs/turbogit/preflight-backup");
        assert_eq!(
            git_with_env(&seed.alpha, &["rev-parse", "HEAD"], &GitEnv::fixture()).trim(),
            head
        );
    }

    #[test]
    fn the_reword_editor_opens_seeded_with_the_commit_s_own_message() {
        let seed = seeded_repo("reword-editor");
        let mut harness = log_harness(&seed);
        let head = git_with_env(&seed.alpha, &["rev-parse", "HEAD"], &GitEnv::fixture())
            .trim()
            .to_string();
        harness
            .state_mut()
            .settings
            .protected_branch_patterns
            .clear();
        settle(&mut harness);

        right_click_row(&mut harness, "alpha: second commit");
        click_menu_item(&mut harness, "Reword commit");
        settle(&mut harness);

        assert_eq!(
            harness.state().ui.dialog,
            Some(turbogit_app::state::Dialog::Reword),
            "the item opens an editor, not a rewrite"
        );
        assert_eq!(
            harness.state().ui.dlg.reword_message,
            "alpha: second commit\n\nThe body the pane still shows.",
            "the editor is seeded with the commit's actual message, body and all"
        );
        assert_painted(&harness, "alpha: second commit");
        assert_painted(&harness, "The body the pane still shows.");
        assert_eq!(
            git_with_env(&seed.alpha, &["rev-parse", "HEAD"], &GitEnv::fixture()).trim(),
            head,
            "and nothing has moved: this is a draft"
        );
    }

    #[test]
    fn the_editor_takes_a_typed_subject_and_a_typed_body() {
        let seed = seeded_repo("reword-typing");
        let mut harness = log_harness(&seed);
        harness
            .state_mut()
            .settings
            .protected_branch_patterns
            .clear();
        settle(&mut harness);

        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "Reword commit");
        settle(&mut harness);
        harness
            .get_by_role_and_label(Role::TextInput, "REWORD-MESSAGE")
            .focus();
        harness.run();
        harness.key_press_modifiers(Modifiers::CTRL, egui::Key::U);
        harness.run();
        assert_eq!(
            harness.state().ui.dlg.reword_message,
            "",
            "the developer can empty the message entirely, and the field says so"
        );
        assert_painted(&harness, "A commit cannot have an empty message");

        harness
            .get_by_role_and_label(Role::TextInput, "REWORD-MESSAGE")
            .type_text("alpha: the first thing");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        harness
            .get_by_role_and_label(Role::TextInput, "REWORD-MESSAGE")
            .type_text("A body typed just now.");
        harness.run();

        assert_eq!(
            harness.state().ui.dlg.reword_message,
            "alpha: the first thing\n\nA body typed just now.",
            "one field wrote the subject and the body, in git's own arrangement"
        );
        assert_painted(&harness, "The first line is the subject");
    }

    #[test]
    fn cancelling_the_reword_editor_changes_nothing() {
        let seed = seeded_repo("reword-cancel");
        let mut harness = log_harness(&seed);
        let head = git_with_env(&seed.alpha, &["rev-parse", "HEAD"], &GitEnv::fixture())
            .trim()
            .to_string();
        let count = git_with_env(
            &seed.alpha,
            &["rev-list", "--count", "main"],
            &GitEnv::fixture(),
        )
        .trim()
        .to_string();
        harness
            .state_mut()
            .settings
            .protected_branch_patterns
            .clear();
        settle(&mut harness);

        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "Reword commit");
        settle(&mut harness);
        harness.state_mut().ui.dlg.reword_message = "a message that will be discarded".to_owned();
        settle(&mut harness);
        harness.get_by_label("Cancel").click();
        settle(&mut harness);

        assert_eq!(harness.state().ui.dialog, None, "the editor closes");
        assert_eq!(
            harness.state().ui.dlg.reword_message,
            "",
            "and the draft is discarded, not held for the next open"
        );
        assert_eq!(
            git_with_env(&seed.alpha, &["rev-parse", "HEAD"], &GitEnv::fixture()).trim(),
            head
        );
        assert_eq!(
            git_with_env(
                &seed.alpha,
                &["rev-list", "--count", "main"],
                &GitEnv::fixture()
            )
            .trim(),
            count,
            "the history is where it was"
        );
        assert!(
            !git_with_env(&seed.alpha, &["show-ref"], &GitEnv::fixture())
                .contains("preflight-backup"),
            "and no backup ref was written: nothing ran"
        );
    }

    #[test]
    fn a_reword_with_no_message_is_refused_and_states_why() {
        let seed = seeded_repo("reword-empty");
        let mut harness = log_harness(&seed);
        let head = git_with_env(&seed.alpha, &["rev-parse", "HEAD"], &GitEnv::fixture())
            .trim()
            .to_string();
        harness
            .state_mut()
            .settings
            .protected_branch_patterns
            .clear();
        settle(&mut harness);

        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "Reword commit");
        settle(&mut harness);
        harness.state_mut().ui.dlg.reword_message = "   \n  ".to_owned();
        settle(&mut harness);

        assert_painted(&harness, "A commit cannot have an empty message");
        assert!(
            harness
                .get_by_label("Reword this commit")
                .accesskit_node()
                .is_disabled(),
            "so the confirm cannot be pressed at all"
        );
        let root = harness
            .state()
            .selected_root
            .clone()
            .expect("a selected root");
        let target = git_with_env(&seed.alpha, &["rev-parse", "HEAD~1"], &GitEnv::fixture())
            .trim()
            .to_string();
        harness.state_mut().open_rewrite_preflight(
            &root,
            &target,
            turbogit_app::state::HistoryVerb::Reword {
                message: "  \n ".to_owned(),
            },
        );
        settle(&mut harness);
        assert_ne!(
            harness.state().ui.dialog,
            Some(turbogit_app::state::Dialog::RewritePreflight),
            "no preflight opens for a message that is not a message, and the \
             editor the developer is standing in stays where it was"
        );
        let toast = harness
            .state()
            .ui
            .toast
            .as_ref()
            .map(|t| t.message.clone())
            .expect("the refusal is stated");
        assert_eq!(toast, "A commit cannot have an empty message");
        assert!(
            !toast.contains("git would keep"),
            "the git mechanism is not a second statement of the bound: {toast:?}"
        );
        assert_eq!(
            git_with_env(&seed.alpha, &["rev-parse", "HEAD"], &GitEnv::fixture()).trim(),
            head
        );
    }

    #[test]
    fn the_reword_preflight_says_what_it_will_change_and_what_it_costs() {
        let seed = seeded_repo("reword-preflight");
        let mut harness = log_harness(&seed);
        let head = git_with_env(&seed.alpha, &["rev-parse", "HEAD"], &GitEnv::fixture())
            .trim()
            .to_string();
        harness
            .state_mut()
            .settings
            .protected_branch_patterns
            .clear();
        settle(&mut harness);

        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "Reword commit");
        settle(&mut harness);
        assert_eq!(
            harness.state().ui.dialog,
            Some(turbogit_app::state::Dialog::Reword)
        );

        harness.state_mut().ui.dlg.reword_message = "alpha: the first commit".to_owned();
        settle(&mut harness);
        harness.get_by_label("Reword this commit").click();
        settle(&mut harness);

        assert_eq!(
            harness.state().ui.dialog,
            Some(turbogit_app::state::Dialog::RewritePreflight),
            "the editor's confirm opens the preflight, not the rewrite"
        );
        assert_painted(&harness, "REWORDED COMMIT");
        assert_painted(&harness, "alpha: first commit");
        assert_painted(&harness, "→");
        assert_painted(&harness, "alpha: the first commit");
        assert_painted(&harness, "REPLAYED COMMITS · 2");
        assert_painted(&harness, "2 COMMITS REWRITTEN");
        assert_not_painted(&harness, "→ 2 COMMITS");
        assert_painted(&harness, "~6s estimated");
        assert_painted(&harness, "RECOVERY");
        assert_painted(&harness, "refs/turbogit/preflight-backup");
        assert_eq!(
            git_with_env(&seed.alpha, &["rev-parse", "HEAD"], &GitEnv::fixture()).trim(),
            head,
            "and nothing has moved: this is still a briefing"
        );
    }

    #[test]
    fn the_reword_badge_is_singular_at_one_commit() {
        let seed = seeded_repo("reword-badge-one");
        let mut harness = log_harness(&seed);
        harness
            .state_mut()
            .settings
            .protected_branch_patterns
            .clear();
        settle(&mut harness);

        right_click_row(&mut harness, "alpha: second commit");
        click_menu_item(&mut harness, "Reword commit");
        settle(&mut harness);
        harness.state_mut().ui.dlg.reword_message = "alpha: the second commit".to_owned();
        settle(&mut harness);
        harness.get_by_label("Reword this commit").click();
        settle(&mut harness);

        assert_painted(&harness, "REPLAYED COMMITS · 1");
        assert_painted(&harness, "1 COMMIT REWRITTEN");
        assert_not_painted(&harness, "1 COMMITS REWRITTEN");
    }

    fn repo_with_merge_and_branch(tag: &str) -> (TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join(tag);
        let repo = project.join("alpha");
        std::fs::create_dir_all(&repo).unwrap();
        git_with_env(&repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
        git_with_env(&repo, &["config", "user.email", "t@t"], &GitEnv::fixture());
        git_with_env(&repo, &["config", "user.name", "t"], &GitEnv::fixture());
        commit_file(&repo, "a.txt", "one\n", "alpha: first commit", 100);
        git_with_env(&repo, &["checkout", "-q", "-b", "side"], &GitEnv::fixture());
        commit_file(&repo, "s.txt", "side\n", "alpha: side work", 120);
        git_with_env(&repo, &["checkout", "-q", "main"], &GitEnv::fixture());
        git_with_env(
            &repo,
            &["checkout", "-q", "-b", "topic"],
            &GitEnv::fixture(),
        );
        commit_file(&repo, "t.txt", "topic\n", "alpha: topic work", 150);
        git_with_env(&repo, &["checkout", "-q", "main"], &GitEnv::fixture());
        commit_file(&repo, "b.txt", "two\n", "alpha: second commit", 200);
        git_with_env(
            &repo,
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                "Merge topic into main",
                "topic",
            ],
            &GitEnv::fixture(),
        );
        (tmp, project, repo)
    }

    #[test]
    fn the_drop_preflight_shows_the_same_cautions_the_plan_editor_does() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("alpha");
        std::fs::create_dir_all(&repo).unwrap();
        git_with_env(&repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
        git_with_env(&repo, &["config", "user.email", "t@t"], &GitEnv::fixture());
        git_with_env(&repo, &["config", "user.name", "t"], &GitEnv::fixture());
        commit_as_configured(&repo, "base.txt", "base\n", "alpha: base commit", 100);
        commit_as_configured(&repo, "shared.txt", "one\n", "alpha: first commit", 200);
        git_with_env(&repo, &["config", "user.name", "Other"], &GitEnv::fixture());
        commit_as_configured(&repo, "shared.txt", "two\n", "alpha: second commit", 300);
        git_with_env(&repo, &["config", "user.name", "t"], &GitEnv::fixture());
        commit_as_configured(&repo, "shared.txt", "three\n", "alpha: third commit", 400);

        let mut harness = harness_over_state(state_with_log_at(repo));
        warm_logs(&mut harness);
        settle(&mut harness);
        harness
            .state_mut()
            .settings
            .protected_branch_patterns
            .clear();
        settle(&mut harness);

        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "Drop commit");
        settle(&mut harness);

        assert_painted(&harness, "CAUTIONS · 2");
        assert_painted(&harness, "Conflicts likely on 1 file");
        assert_painted(
            &harness,
            "Mixed committer identities (2 authors) — verify signatures",
        );
    }

    #[test]
    fn a_long_history_elides_the_middle_and_says_how_much_is_hidden() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("alpha");
        std::fs::create_dir_all(&repo).unwrap();
        git_with_env(&repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
        git_with_env(&repo, &["config", "user.email", "t@t"], &GitEnv::fixture());
        git_with_env(&repo, &["config", "user.name", "t"], &GitEnv::fixture());
        for n in 1..=30 {
            commit_file(
                &repo,
                &format!("c{n}.txt"),
                &format!("{n}\n"),
                &format!("alpha: commit {n:02}"),
                100 + n,
            );
        }

        let mut harness = harness_over_state(state_with_log_at(repo));
        warm_logs(&mut harness);
        settle(&mut harness);
        harness
            .state_mut()
            .settings
            .protected_branch_patterns
            .clear();
        settle(&mut harness);

        right_click_row(&mut harness, "alpha: commit 15");
        click_menu_item(&mut harness, "Drop commit");
        settle(&mut harness);

        assert_painted(&harness, "REPLAYED COMMITS · 15");
        assert_painted(&harness, "16 → 15 COMMITS");
        assert_not_painted(&harness, "· 1 DROPPED");
        assert_painted(&harness, "… 3 commits in between …");
        let times_painted = |subject: &str| {
            painted_galleys(&harness)
                .iter()
                .filter(|g| g.text == subject)
                .count()
        };
        assert_eq!(
            times_painted("alpha: commit 16"),
            2,
            "the oldest commit that moves is in the list as well as in the graph"
        );
        assert_eq!(
            times_painted("alpha: commit 30"),
            2,
            "and so is the tip the replay moves to"
        );
        assert_eq!(
            times_painted("alpha: commit 22"),
            1,
            "while a commit from the elided middle is only in the graph"
        );
    }

    fn menu_over(tag: &str) -> (TempDir, Harness<'static, AppState>, RootId) {
        let (tmp, _project, repo) = repo_with_merge_and_branch(tag);
        let mut harness = harness_over_state(state_with_log_at(repo));
        warm_logs(&mut harness);
        settle(&mut harness);
        let root = harness.state().multi.roots[0].id.clone();
        (tmp, harness, root)
    }

    struct GateCase {
        tag: &'static str,
        verb: &'static str,
        subject: &'static str,
        scope: Option<&'static str>,
        stated: &'static str,
    }

    const HISTORY_GATES: &[GateCase] = &[
        GateCase {
            tag: "gates-merge",
            verb: "Drop commit",
            subject: "Merge topic into main",
            scope: None,
            stated: "a merge commit cannot be dropped",
        },
        GateCase {
            tag: "gates-root",
            verb: "Drop commit",
            subject: "alpha: first commit",
            scope: None,
            stated: "a root commit cannot be dropped",
        },
        GateCase {
            tag: "gates-protected",
            verb: "Drop commit",
            subject: "alpha: second commit",
            scope: None,
            stated: "the current branch is protected",
        },
        GateCase {
            tag: "gates-off-branch",
            verb: "Drop commit",
            subject: "alpha: side work",
            scope: Some("side"),
            stated: "not on the current branch",
        },
        GateCase {
            tag: "reword-gates-merge",
            verb: "Reword commit",
            subject: "Merge topic into main",
            scope: None,
            stated: "a merge commit cannot be reworded",
        },
        GateCase {
            tag: "reword-gates-protected",
            verb: "Reword commit",
            subject: "alpha: second commit",
            scope: None,
            stated: "the current branch is protected",
        },
        GateCase {
            tag: "reword-gates-off-branch",
            verb: "Reword commit",
            subject: "alpha: side work",
            scope: Some("side"),
            stated: "not on the current branch",
        },
        GateCase {
            tag: "reword-gates-root",
            verb: "Reword commit",
            subject: "alpha: first commit",
            scope: None,
            stated: "a root commit cannot be reworded",
        },
    ];

    #[test]
    fn every_refused_history_verb_is_gated_and_states_its_reason() {
        for case in HISTORY_GATES {
            println!(
                "{tag} — {verb}: {stated}",
                tag = case.tag,
                verb = case.verb,
                stated = case.stated
            );

            let (_tmp, mut harness, root) = menu_over(case.tag);
            if let Some(branch) = case.scope {
                harness.state_mut().ui.log_ref_scope = Some((root.clone(), branch.into()));
                wait_for(&mut harness, |s| s.caches.ref_log(&root, branch).is_some());
                settle(&mut harness);
            }
            right_click_row(&mut harness, case.subject);
            assert_menu_item_gated(&mut harness, "Copy hash", case.verb);
            assert_stated_on_hover(&mut harness, case.verb, case.stated);
        }
    }

    #[test]
    fn the_detached_head_warning_still_applies_once_the_state_exists() {
        let seed = seeded_repo("checkout-warn");
        let mut harness = log_harness(&seed);
        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "Checkout");
        wait_for(&mut harness, |s| {
            s.multi
                .roots
                .first()
                .is_some_and(|r| r.current_branch.is_none())
        });

        harness.state_mut().ui.tab = Tab::Commit;
        settle(&mut harness);
        assert_painted(&harness, "<detached>");
    }

    #[test]
    fn a_dirty_worktree_routes_checkout_through_the_branch_confirmation() {
        let seed = seeded_repo("checkout-dirty");
        std::fs::write(seed.alpha.join("a.txt"), "uncommitted\n").unwrap();
        let mut harness = log_harness(&seed);
        harness
            .state_mut()
            .refresh(turbogit_app::root_caches::Affected::All);
        settle(&mut harness);
        warm_logs(&mut harness);
        settle(&mut harness);

        right_click_row(&mut harness, "alpha: first commit");
        assert_menu_item_gated(&mut harness, "Copy hash", "Checkout");
        assert_stated_on_hover(
            &mut harness,
            "Checkout",
            "Resolve the uncommitted changes first",
        );
    }

    #[test]
    fn the_dirty_commit_checkout_shares_the_branch_confirmation_and_cancel_is_safe() {
        let seed = seeded_repo("checkout-confirm");
        let mut harness = log_harness(&seed);
        let before = harness
            .state()
            .multi
            .roots
            .first()
            .and_then(|r| r.current_branch.clone())
            .expect("main is checked out");

        harness.state_mut().ui.confirm = Some(turbogit_app::state::PendingConfirm::CheckoutDirty {
            root: harness.state().multi.roots[0].id.clone(),
            target: seed.c1.clone(),
            kind: turbogit_domain::model::BranchKind::Local,
            detach: true,
        });
        settle(&mut harness);
        assert_painted(&harness, "Bring along");
        assert_painted(&harness, "Set aside");
        assert_painted(&harness, "uncommitted changes");

        harness.get_by_label("Cancel").click();
        settle(&mut harness);
        assert!(harness.state().ui.confirm.is_none(), "the dialog went");
        assert_eq!(
            harness
                .state()
                .multi
                .roots
                .first()
                .and_then(|r| r.current_branch.clone())
                .expect("still on a branch"),
            before,
            "and cancelling changed nothing"
        );
        assert_eq!(
            git_with_env(&seed.alpha, &["rev-parse", "HEAD"], &GitEnv::fixture()).trim(),
            seed.c2
        );

        std::fs::write(seed.alpha.join("a.txt"), "uncommitted\n").unwrap();
        harness.state_mut().ui.confirm = Some(turbogit_app::state::PendingConfirm::CheckoutDirty {
            root: harness.state().multi.roots[0].id.clone(),
            target: seed.c1.clone(),
            kind: turbogit_domain::model::BranchKind::Local,
            detach: true,
        });
        settle(&mut harness);
        harness.get_by_label("Bring along").click();
        wait_for(&mut harness, |st| {
            st.multi
                .roots
                .first()
                .is_some_and(|r| r.current_branch.is_none())
        });
        assert_eq!(
            git_with_env(&seed.alpha, &["rev-parse", "HEAD"], &GitEnv::fixture()).trim(),
            seed.c1,
            "the confirmation landed on the commit, detached"
        );
    }

    pub(crate) fn state_with_log_at(repo: PathBuf) -> AppState {
        let project = repo.parent().expect("a project dir").to_path_buf();
        let mut state = AppState::new(project);
        state.drain_events();
        state.ui.tab = Tab::Log;
        state
    }

    pub(crate) fn choose_patch_path(harness: &mut Harness<'_, AppState>, choice: Option<PathBuf>) {
        harness.state_mut().patch_writer = Some(Box::new(move || choice.clone()));
    }

    #[test]
    fn create_patch_writes_that_commit_s_own_change_and_nothing_above_it() {
        let seed = seeded_repo("patch");
        let mut harness = log_harness(&seed);
        let out = seed.project.join("mine.patch");
        right_click_row(&mut harness, "alpha: first commit");
        choose_patch_path(&mut harness, Some(out.clone()));
        click_menu_item(&mut harness, "Create patch");
        settle(&mut harness);
        accept_patch_settings(&mut harness);
        let file = out;

        let text = std::fs::read_to_string(&file).expect("the patch was written");
        assert!(
            text.contains("a.txt"),
            "the commit's own path is in the patch: {text}"
        );
        assert!(
            !text.contains("base.txt"),
            "an ancestor's change must not ride along: {text}"
        );
        assert!(
            text.contains("alpha: first commit") || text.contains("one"),
            "the change itself is in the patch: {text}"
        );

        git_with_env(
            &seed.alpha,
            &["checkout", "-q", &seed.base],
            &GitEnv::fixture(),
        );
        let check = Command::new("git")
            .args(["apply", "--check", file.to_str().unwrap()])
            .current_dir(&seed.alpha)
            .output()
            .expect("git must be on PATH");
        assert!(
            check.status.success(),
            "git apply accepts the patch: {}",
            String::from_utf8_lossy(&check.stderr)
        );
        assert_painted(&harness, "mine.patch");
        assert_painted(&harness, "Import to shelf");
        assert_eq!(
            harness
                .state()
                .ui
                .patch_created
                .as_ref()
                .map(|a| a.path.file_name().map(|n| n.to_string_lossy().into_owned())),
            Some(Some("mine.patch".to_string())),
            "the default arm leaves the created patch waiting for an answer"
        );
    }

    #[test]
    fn the_hash_chip_answers_no_click() {
        let seed = seeded_repo("chip");
        let mut harness = log_harness(&seed);
        row_node(&harness, "alpha: second commit").click();
        settle(&mut harness);

        let galleys = painted_galleys(&harness);
        let chip = filled_rects(&harness)
            .into_iter()
            .find(|(rect, fill)| {
                *fill == Palette::SURFACE_3
                    && galleys
                        .iter()
                        .any(|g| g.text == short(&seed.c2) && rect.contains_rect(g.rect))
            })
            .map(|(rect, _)| rect)
            .expect("the hash chip paints");
        let center = chip.center();
        let clickable: Vec<_> = harness
            .query_all(
                egui_kittest::kittest::by()
                    .predicate(|node| node.data().supports_action(egui::accesskit::Action::Click)),
            )
            .filter(|node| node.rect().contains(center))
            .map(|node| node.accesskit_node().role())
            .collect();
        assert!(
            clickable.is_empty(),
            "a chip that copies nothing must not answer a click, yet {:?} does at \
             {center:?}",
            clickable
        );
    }

    #[test]
    fn a_rename_and_a_binary_survive_into_the_patch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git_with_env(&repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
        git_with_env(&repo, &["config", "user.email", "t@t"], &GitEnv::fixture());
        git_with_env(&repo, &["config", "user.name", "t"], &GitEnv::fixture());
        commit_file(
            &repo,
            "before.txt",
            "one\ntwo\nthree\n",
            "a base commit",
            100,
        );
        git_with_env(
            &repo,
            &["mv", "before.txt", "after.txt"],
            &GitEnv::fixture(),
        );
        std::fs::write(repo.join("pic.png"), bytes_of_a_png()).unwrap();
        git_with_env(&repo, &["add", "."], &GitEnv::fixture());
        git_with_env(
            &repo,
            &["commit", "-q", "-m", "rename and add a binary"],
            &GitEnv::fixture(),
        );
        let renamed = git_with_env(&repo, &["rev-parse", "HEAD"], &GitEnv::fixture())
            .trim()
            .to_string();

        let mut harness = harness_over_state(state_with_log_at(repo.clone()));
        warm_logs(&mut harness);
        settle(&mut harness);
        right_click_row(&mut harness, "rename and add a binary");
        let file = tmp.path().join("mixed.patch");
        choose_patch_path(&mut harness, Some(file.clone()));
        click_menu_item(&mut harness, "Create patch");
        settle(&mut harness);
        accept_patch_settings(&mut harness);

        let text = std::fs::read_to_string(&file).expect("the patch was written");
        assert!(
            text.contains("rename from before.txt") && text.contains("rename to after.txt"),
            "the rename reads as a rename, the way git's own diff says it: {text}"
        );
        assert!(
            text.contains("GIT binary patch") || text.contains("Binary files"),
            "a binary appears the way git represents binary content: {text}"
        );
        let _ = renamed;
    }

    /// PNG-magic bytes with a NUL in them: a NUL is exactly what makes git
    /// classify a blob as binary, which is what the patch's representation of it
    /// depends on.
    fn bytes_of_a_png() -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00];
        bytes.extend((0..64u8).map(|n| n.wrapping_mul(31).saturating_add(7)));
        bytes
    }

    #[test]
    fn an_empty_commit_refuses_the_write_and_says_so() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git_with_env(&repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
        git_with_env(&repo, &["config", "user.email", "t@t"], &GitEnv::fixture());
        git_with_env(&repo, &["config", "user.name", "t"], &GitEnv::fixture());
        commit_file(&repo, "base.txt", "base\n", "a base commit", 100);
        git_with_env(
            &repo,
            &["commit", "-q", "--allow-empty", "-m", "an empty commit"],
            &GitEnv::fixture(),
        );

        let mut harness = harness_over_state(state_with_log_at(repo.clone()));
        warm_logs(&mut harness);
        settle(&mut harness);
        right_click_row(&mut harness, "an empty commit");
        let file = tmp.path().join("empty.patch");
        choose_patch_path(&mut harness, Some(file.clone()));
        click_menu_item(&mut harness, "Create patch");
        settle(&mut harness);
        assert_eq!(
            harness.state().ui.dialog,
            Some(turbogit_app::state::Dialog::PatchFileSettings),
            "the empty commit still opens its settings"
        );
        press(&mut harness, "OK");
        settle(&mut harness);

        assert_painted(&harness, "nothing to write");
        assert_eq!(
            harness.state().ui.dialog,
            Some(turbogit_app::state::Dialog::PatchFileSettings),
            "a dialog that failed stays open with its reason"
        );
        assert!(
            !file.exists(),
            "and the refusal writes no empty file to masquerade as a patch"
        );
    }

    #[test]
    fn cancelling_the_patch_dialog_writes_nothing_and_says_nothing() {
        let seed = seeded_repo("patch-cancel");
        let mut harness = log_harness(&seed);
        let path = seed.project.join("never-written.patch");
        let _ = std::fs::remove_file(&path);

        right_click_row(&mut harness, "alpha: first commit");
        choose_patch_path(&mut harness, None);
        click_menu_item(&mut harness, "Create patch");
        settle(&mut harness);
        let typed = harness.state().ui.dlg.create_patch_location.clone();
        press(&mut harness, "Save to…");
        settle(&mut harness);
        assert_eq!(
            harness.state().ui.dlg.create_patch_location,
            typed,
            "a declined chooser changes nothing"
        );
        press(&mut harness, "Cancel");
        settle(&mut harness);

        assert_eq!(harness.state().ui.dialog, None, "Cancel closes the flow");
        assert!(!path.exists(), "a cancelled dialog leaves no file behind");
        assert!(
            harness.state().ui.toast.is_none(),
            "and no confirmation either"
        );
    }

    #[test]
    fn writing_a_patch_is_not_an_operation() {
        let seed = seeded_repo("patch-not-an-op");
        let mut harness = log_harness(&seed);
        let before = harness
            .state()
            .ui
            .activity
            .entries
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>();

        right_click_row(&mut harness, "alpha: first commit");
        choose_patch_path(&mut harness, Some(seed.project.join("op-check.patch")));
        click_menu_item(&mut harness, "Create patch");
        settle(&mut harness);

        assert_eq!(
            harness
                .state()
                .ui
                .activity
                .entries
                .iter()
                .map(|e| e.message.clone())
                .collect::<Vec<_>>(),
            before,
            "the activity feed gained no entry for a file write"
        );
    }

    #[test]
    fn the_details_pane_keeps_everything_it_says_and_loses_everything_it_did() {
        let seed = seeded_repo("pane");
        let mut harness = log_harness(&seed);
        row_node(&harness, "alpha: second commit").click();
        settle(&mut harness);

        assert_painted(&harness, "alpha: second commit");
        assert_painted(&harness, "Committer");
        assert_painted(&harness, "Parents");
        assert_painted(&harness, "changed");
        assert_painted(&harness, &short(&seed.c2));
        for gone in [
            "ACTIONS",
            "Actions",
            "Create branch here",
            "click to copy full hash",
        ] {
            assert_not_painted(&harness, gone);
        }
        let copies = painted_galleys(&harness)
            .into_iter()
            .filter(|g| g.text.contains("Copy hash"))
            .count();
        assert_eq!(copies, 0, "no copy affordance left in the pane");
    }
}

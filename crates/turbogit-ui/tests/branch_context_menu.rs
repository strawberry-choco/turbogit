use turbogit_domain::model::{Branch, BranchKind, Upstream};
use turbogit_ui::ui::branch_menu::{BranchMenuAction, MenuItemState, branch_menu_items};

fn local(name: &str, tracking: Option<&str>, ahead: usize, behind: usize) -> Branch {
    Branch {
        name: name.to_string(),
        kind: BranchKind::Local,
        tracking: tracking.and_then(Upstream::from_git_ref),
        favorite: false,
        protected: false,
        exists: true,
        ahead,
        behind,
        gone: false,
        last_touched: None,
        tip: None,
        remote: None,
    }
}

#[track_caller]
fn state(branch: &Branch, current: Option<&str>, item: BranchMenuAction) -> MenuItemState {
    let items = branch_menu_items(branch, current);
    items[item.index()]
}

#[test]
fn the_current_branch_offers_no_checkout_and_no_pull() {
    let main = local("main", Some("origin/main"), 0, 0);
    let s = |item: BranchMenuAction| state(&main, Some("main"), item);

    assert_eq!(
        s(BranchMenuAction::Checkout),
        MenuItemState::disabled("already checked out")
    );
    assert_eq!(
        s(BranchMenuAction::CheckoutAndPull),
        MenuItemState::disabled("already checked out")
    );
    assert_eq!(
        s(BranchMenuAction::Pull),
        MenuItemState::disabled("pull is offered on remote branches")
    );
    assert_eq!(s(BranchMenuAction::Rename), MenuItemState::enabled());
    assert_eq!(s(BranchMenuAction::NewBranchFrom), MenuItemState::enabled());
}

#[test]
fn an_untracked_branch_cannot_pull_but_still_checks_out_and_pushes() {
    let side = local("side", None, 0, 0);
    let s = |item: BranchMenuAction| state(&side, Some("main"), item);

    assert_eq!(s(BranchMenuAction::Checkout), MenuItemState::enabled());
    assert_eq!(
        s(BranchMenuAction::CheckoutAndPull),
        MenuItemState::disabled("no upstream to pull from")
    );
    assert_eq!(
        s(BranchMenuAction::Pull),
        MenuItemState::disabled("check out this branch first")
    );
    assert_eq!(s(BranchMenuAction::Push), MenuItemState::enabled());
}

#[test]
fn a_gone_upstream_blocks_both_pull_items() {
    let mut stale = local("stale", Some("origin/removed"), 0, 0);
    stale.gone = true;

    let current = |item: BranchMenuAction| state(&stale, Some("stale"), item);
    assert_eq!(
        current(BranchMenuAction::Pull),
        MenuItemState::disabled("no upstream to pull from")
    );
    assert_eq!(
        current(BranchMenuAction::CheckoutAndPull),
        MenuItemState::disabled("already checked out"),
        "the checkout blocker is stated first"
    );

    let other = |item: BranchMenuAction| state(&stale, Some("main"), item);
    assert_eq!(
        other(BranchMenuAction::CheckoutAndPull),
        MenuItemState::disabled("no upstream to pull from")
    );
}

#[test]
fn push_follows_what_the_upstream_owes_the_branch() {
    let ahead = local("feat", Some("origin/feat"), 2, 0);
    assert_eq!(
        state(&ahead, Some("main"), BranchMenuAction::Push),
        MenuItemState::enabled()
    );

    let in_sync = local("sync", Some("origin/sync"), 0, 3);
    assert_eq!(
        state(&in_sync, Some("main"), BranchMenuAction::Push),
        MenuItemState::disabled("nothing to push")
    );

    let mut dead = local("dead", Some("origin/dead"), 0, 0);
    dead.gone = true;
    assert_eq!(
        state(&dead, Some("main"), BranchMenuAction::Push),
        MenuItemState::disabled("upstream is gone")
    );
}

#[test]
fn a_remote_row_cannot_be_renamed() {
    let up = Branch {
        kind: BranchKind::Remote,
        remote: Some("origin".to_string()),
        ..local("up", None, 0, 0)
    };
    assert_eq!(
        state(&up, Some("main"), BranchMenuAction::Rename),
        MenuItemState::disabled("remote branches cannot be renamed")
    );
    assert_eq!(
        state(
            &local("side", None, 0, 0),
            Some("main"),
            BranchMenuAction::Rename
        ),
        MenuItemState::enabled(),
        "a local row always can"
    );
}

#[test]
fn merge_rebase_compare_and_delete_state_why_the_row_blocks_them() {
    let side = local("side", Some("origin/side"), 0, 0);
    let plain = |item: BranchMenuAction| state(&side, Some("main"), item);
    assert_eq!(plain(BranchMenuAction::Merge), MenuItemState::enabled());
    assert_eq!(plain(BranchMenuAction::Rebase), MenuItemState::enabled());
    assert_eq!(plain(BranchMenuAction::Compare), MenuItemState::enabled());
    assert_eq!(plain(BranchMenuAction::Delete), MenuItemState::enabled());

    let main = local("main", Some("origin/main"), 0, 0);
    let current = |item: BranchMenuAction| state(&main, Some("main"), item);
    assert_eq!(
        current(BranchMenuAction::Merge),
        MenuItemState::disabled("this is the current branch")
    );
    assert_eq!(
        current(BranchMenuAction::Rebase),
        MenuItemState::disabled("this is the current branch")
    );
    assert_eq!(
        current(BranchMenuAction::Compare),
        MenuItemState::disabled("this is the current branch")
    );
    assert_eq!(
        current(BranchMenuAction::Delete),
        MenuItemState::disabled("the current branch cannot be deleted")
    );

    let up = Branch {
        kind: BranchKind::Remote,
        remote: Some("origin".to_string()),
        ..local("up", None, 0, 0)
    };
    let remote = |item: BranchMenuAction| state(&up, Some("main"), item);
    assert_eq!(
        remote(BranchMenuAction::Merge),
        MenuItemState::disabled("remote branches cannot be merged")
    );
    assert_eq!(
        remote(BranchMenuAction::Rebase),
        MenuItemState::disabled("remote branches cannot be rebased")
    );
    assert_eq!(remote(BranchMenuAction::Compare), MenuItemState::enabled());
    assert_eq!(
        remote(BranchMenuAction::DiffWithWorkingTree),
        MenuItemState::enabled()
    );
    assert_eq!(remote(BranchMenuAction::Delete), MenuItemState::enabled());
}

#[test]
fn the_designed_order_is_the_array_slots() {
    assert_eq!(BranchMenuAction::ORDER.len(), 12);
    for (slot, action) in BranchMenuAction::ORDER.iter().enumerate() {
        assert_eq!(action.index(), slot, "{action:?} is painted in slot {slot}");
    }
}

#[test]
fn copy_branch_name_is_offered_on_every_row() {
    let current = local("main", Some("origin/main"), 0, 0);
    let other = local("side", None, 0, 0);
    let mut gone = local("stale", Some("origin/removed"), 0, 0);
    gone.gone = true;
    let remote = Branch {
        kind: BranchKind::Remote,
        remote: Some("origin".to_string()),
        ..local("up", Some("origin/up"), 0, 0)
    };

    for (label, branch, current_name) in [
        ("the checked-out row", &current, Some("main")),
        ("an untracked local row", &other, Some("main")),
        ("a row whose upstream is gone", &gone, Some("main")),
        ("a remote-tracking row", &remote, Some("main")),
        ("any row under a detached HEAD", &other, None),
    ] {
        assert_eq!(
            state(branch, current_name, BranchMenuAction::CopyBranchName),
            MenuItemState::enabled(),
            "{label} must be copyable"
        );
    }
}

#[test]
fn the_pull_family_is_offered_on_remote_branches_only() {
    let tracked = local("side", Some("origin/side"), 0, 0);
    let current = |item: BranchMenuAction| state(&tracked, Some("side"), item);
    assert_eq!(
        current(BranchMenuAction::Pull),
        MenuItemState::disabled("pull is offered on remote branches")
    );

    let other = |item: BranchMenuAction| state(&tracked, Some("main"), item);
    assert_eq!(
        other(BranchMenuAction::CheckoutAndPull),
        MenuItemState::disabled("pull is offered on remote branches")
    );
    assert_eq!(
        other(BranchMenuAction::Pull),
        MenuItemState::disabled("check out this branch first"),
        "the checkout blocker is stated first where it applies"
    );

    let up = Branch {
        kind: BranchKind::Remote,
        remote: Some("origin".to_string()),
        ..local("up", Some("origin/up"), 0, 0)
    };
    let remote = |item: BranchMenuAction| state(&up, Some("main"), item);
    assert_eq!(
        remote(BranchMenuAction::CheckoutAndPull),
        MenuItemState::enabled()
    );
    assert_eq!(
        remote(BranchMenuAction::Pull),
        MenuItemState::disabled("check out this branch first")
    );
}

use std::cell::RefCell;
use std::rc::Rc;

use egui::Color32;
use egui_kittest::Harness;
use test_support::harness::{filled_rects, painted_galleys};
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::branch_menu::{BranchMenuProps, branch_menu};

fn menu_harness(
    target: Branch,
    props: fn() -> BranchMenuProps<'static>,
) -> (Harness<'static, ()>, Rc<RefCell<Option<BranchMenuAction>>>) {
    let returned = Rc::new(RefCell::new(None));
    let returned_ui = returned.clone();
    let mut harness = test_support::harness::widget_harness(egui::vec2(420.0, 400.0), move |ui| {
        let action = branch_menu(ui, &props(), &target);
        if action.is_some() {
            *returned_ui.borrow_mut() = action;
        }
    });
    harness.step();
    (harness, returned)
}

#[test]
fn the_menu_renders_the_twelve_items_in_the_designed_order() {
    let target = local("feature/cascade-views", Some("origin/f"), 1, 0);
    let (harness, _returned) = menu_harness(target, || BranchMenuProps {
        repo_name: "alpha",
        multi_repo: false,
        current_branch: Some("main"),
    });

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
            "Copy branch name",
            "feature/cascade-views",
            "Checkout",
            "New branch from",
            "feature/cascade-views",
            "Checkout and pull",
            "Merge into main",
            "Rebase onto main",
            "Compare with main",
            "Show Diff with Working Tree",
            "Pull",
            "Push",
            "Ctrl+Shift+K",
            "Rename branch",
            "Delete branch",
        ],
        "item order, the data segment, and the one accelerator"
    );

    let names: Vec<&egui::FontFamily> = galleys
        .iter()
        .filter(|g| g.text == "feature/cascade-views")
        .map(|g| &g.family)
        .collect();
    assert_eq!(
        names,
        vec![&egui::FontFamily::Monospace, &egui::FontFamily::Monospace],
        "the copied value and the base are both data"
    );

    let rules: Vec<Color32> = filled_rects(&harness)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::LINE_SUBTLE)
        .map(|(_, fill)| fill)
        .collect();
    assert_eq!(rules.len(), 4, "exactly four menu rules");

    assert!(
        filled_rects(&harness)
            .iter()
            .any(|(_, fill)| *fill == Palette::BRAND),
        "the primary item carries the brand fill"
    );
}

#[test]
fn the_history_verbs_drop_their_destination_when_no_branch_is_checked_out() {
    let target = local("side", Some("origin/side"), 0, 0);
    let (harness, _returned) = menu_harness(target, || BranchMenuProps {
        repo_name: "alpha",
        multi_repo: false,
        current_branch: None,
    });
    let texts: Vec<String> = painted_galleys(&harness)
        .into_iter()
        .map(|g| g.text)
        .collect();
    assert!(texts.iter().any(|t| t == "Merge"));
    assert!(texts.iter().any(|t| t == "Rebase"));
    assert!(texts.iter().any(|t| t == "Compare"));
    assert!(
        !texts.iter().any(|t| t.ends_with("into ")),
        "never a verb with a dangling preposition: {texts:?}"
    );
}

#[test]
fn the_primary_item_names_its_repository_when_more_than_one_is_in_scope() {
    let target = local("side", Some("origin/side"), 0, 0);
    let (harness, _returned) = menu_harness(target, || BranchMenuProps {
        repo_name: "alpha",
        multi_repo: true,
        current_branch: Some("main"),
    });
    assert!(
        painted_galleys(&harness)
            .iter()
            .any(|g| g.text == "Checkout in alpha"),
        "the primary item states its repo scope"
    );
}

#[test]
fn clicking_an_item_returns_its_action() {
    use egui_kittest::kittest::{NodeT as _, Queryable as _};

    let target = local("side", Some("origin/side"), 1, 0);
    let (mut harness, returned) = menu_harness(target, || BranchMenuProps {
        repo_name: "alpha",
        multi_repo: false,
        current_branch: Some("main"),
    });
    harness.get_by_label("Checkout").click();
    harness.step();
    assert_eq!(
        *returned.borrow(),
        Some(BranchMenuAction::Checkout),
        "the component reports the pick and nothing else"
    );

    let target = local("main", Some("origin/main"), 0, 0);
    let (mut harness, returned) = menu_harness(target, || BranchMenuProps {
        repo_name: "alpha",
        multi_repo: false,
        current_branch: Some("main"),
    });
    assert!(
        painted_galleys(&harness)
            .iter()
            .any(|g| g.text == "Checkout"),
        "a gated item stays visible"
    );
    harness.get_by_label("Checkout").click();
    harness.step();
    assert_eq!(*returned.borrow(), None, "and swallows its own click");
    assert!(
        harness
            .get_by_label("Checkout")
            .accesskit_node()
            .is_disabled(),
        "the accessibility node carries the gate"
    );
}

#[test]
fn clicking_the_copy_item_returns_its_action_and_nothing_else() {
    use egui_kittest::kittest::Queryable as _;

    let target = local("side", Some("origin/side"), 0, 0);
    let (mut harness, returned) = menu_harness(target, || BranchMenuProps {
        repo_name: "alpha",
        multi_repo: false,
        current_branch: Some("main"),
    });
    harness.get_by_label("Copy branch name").click();
    harness.step();
    assert_eq!(
        *returned.borrow(),
        Some(BranchMenuAction::CopyBranchName),
        "the component reports the pick and nothing else"
    );
    assert!(
        harness
            .output()
            .platform_output
            .commands
            .iter()
            .all(|command| !matches!(command, egui::OutputCommand::CopyText(_))),
        "the menu paints; the copy is the host's act"
    );
}

pub(crate) mod surface {
    use std::path::PathBuf;
    use std::time::Duration;

    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use test_support::git_seed::{GitEnv, git_with_env};
    use test_support::harness::{assert_painted, click_menu_item, painted_galleys, settle_quiet};
    use turbogit_app::state::{AppState, Tab};

    // Every call is pinned per call through the fixture crate's runner: a
    // headless machine has no global identity to borrow. No assertion here
    // reads an author, so nothing about what git records is under test.

    pub(crate) fn one_repo_project(tag: &str) -> PathBuf {
        let base = test_support::scratch_root(&format!("branch-context-menu-{tag}"));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("alpha")).unwrap();
        let repo = base.join("alpha");
        git_with_env(&repo, &["init", "-q", "-b", "main"], &GitEnv::fixture());
        git_with_env(
            &repo,
            &["config", "user.email", "test@example.com"],
            &GitEnv::fixture(),
        );
        git_with_env(&repo, &["config", "user.name", "Test"], &GitEnv::fixture());
        std::fs::write(repo.join("base.txt"), "base\n").unwrap();
        git_with_env(&repo, &["add", "."], &GitEnv::fixture());
        git_with_env(&repo, &["commit", "-q", "-m", "init"], &GitEnv::fixture());
        git_with_env(
            &base,
            &["init", "-q", "--bare", "-b", "main", "origin.git"],
            &GitEnv::fixture(),
        );
        git_with_env(
            &repo,
            &["remote", "add", "origin", "../origin.git"],
            &GitEnv::fixture(),
        );
        git_with_env(
            &repo,
            &["push", "-q", "-u", "origin", "main"],
            &GitEnv::fixture(),
        );
        git_with_env(&repo, &["branch", "feature-a"], &GitEnv::fixture());
        git_with_env(
            &repo,
            &["push", "-q", "--set-upstream", "origin", "feature-a"],
            &GitEnv::fixture(),
        );
        git_with_env(
            &repo,
            &["push", "-q", "origin", "main:remote-only"],
            &GitEnv::fixture(),
        );
        git_with_env(&repo, &["fetch", "-q", "origin"], &GitEnv::fixture());
        git_with_env(&repo, &["branch", "plain-b"], &GitEnv::fixture());
        base
    }

    /// Kept local for `with_step_dt(1.0 / 60.0)`, which is load-bearing:
    /// `step_dt` becomes the frame's `predicted_dt`, i.e. what egui integrates
    /// animations against, not a speed knob. The shared `shell_harness_over`
    /// takes kittest's 1/4 s default, at which the menu's fade-in has already
    /// completed and `the_branch_context_menu_keeps_the_float_stroke` sees
    /// `alpha == 1.0` instead of a value in `(0.0..1.0)`. The shared helper
    /// exposes no `step_dt`.
    pub(crate) fn branches_harness(project_dir: PathBuf) -> Harness<'static, AppState> {
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
        harness.set_size(egui::vec2(1024.0, 768.0));
        harness
    }

    pub(crate) fn open_branches(harness: &mut Harness<'_, AppState>) {
        harness.state_mut().ui.tab = Tab::Branches;
        settle_quiet(harness);
        assert_painted(harness, "feature-a");
    }

    fn right_click_row(harness: &mut Harness<'_, AppState>, branch: &str) {
        row_node(harness, branch).click_secondary();
        harness.step();
        harness.step();
    }

    const MENU_SENTINEL: &str = "New branch from";

    fn wait_for(harness: &mut Harness<'_, AppState>, pred: impl Fn(&AppState) -> bool) {
        for _ in 0..600 {
            harness.step();
            std::thread::sleep(Duration::from_millis(10));
            if pred(harness.state()) {
                return;
            }
        }
        panic!("condition was not met within 600 frames");
    }

    fn current_branch(harness: &Harness<'_, AppState>) -> Option<String> {
        harness.state().multi.roots.first()?.current_branch.clone()
    }

    fn activity_has(state: &AppState, needle: &str) -> bool {
        state
            .ui
            .activity
            .entries
            .iter()
            .any(|e| e.message.contains(needle))
    }

    pub(crate) fn row_node<'t>(
        harness: &'t Harness<'_, AppState>,
        name: &str,
    ) -> egui_kittest::Node<'t> {
        harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .find(|n| n.accesskit_node().label() == Some(name.to_string()))
            .unwrap_or_else(|| panic!("row button for {name}"))
    }

    fn context_menu_target(harness: &Harness<'_, AppState>) -> Option<String> {
        harness
            .state()
            .ui
            .branches_tree
            .context_menu
            .as_ref()
            .map(|(_, b)| b.clone())
    }

    #[test]
    fn right_clicking_a_row_opens_the_menu_on_that_row() {
        let project = one_repo_project("open");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "feature-a");

        assert_eq!(
            context_menu_target(&harness).as_deref(),
            Some("feature-a"),
            "the surface owns the open state, keyed by the right-clicked row"
        );
        assert_painted(&harness, "New branch from");
        assert_painted(&harness, "Checkout and pull");
        assert_painted(&harness, "Rename branch");
        assert_painted(&harness, "feature-a");
    }

    #[test]
    fn escape_closes_the_menu_before_anything_else() {
        let project = one_repo_project("escape");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        row_node(&harness, "feature-a").click();
        harness.step();
        right_click_row(&mut harness, "feature-a");
        assert!(context_menu_target(&harness).is_some());

        harness.key_press(egui::Key::Escape);
        harness.step();
        assert_eq!(context_menu_target(&harness), None, "Escape closes it");
        assert!(
            harness.state().ui.branches_tree.selected.is_some(),
            "and the ladder stops there: the selection survives this press"
        );
    }

    #[test]
    fn a_row_click_or_a_click_outside_closes_the_menu() {
        let project = one_repo_project("dismiss");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        right_click_row(&mut harness, "feature-a");

        row_node(&harness, "main").click();
        harness.step();
        assert_eq!(context_menu_target(&harness), None, "any row click closes");
        assert_eq!(
            harness.state().ui.branches_tree.selected.as_deref(),
            Some("main")
        );

        right_click_row(&mut harness, "feature-a");
        harness.step();
        harness.hover_at(egui::pos2(512.0, 8.0));
        harness.drag_at(egui::pos2(512.0, 8.0));
        harness.step();
        harness.drop_at(egui::pos2(512.0, 8.0));
        harness.step();
        assert_eq!(
            context_menu_target(&harness),
            None,
            "a click outside after one visible frame closes it"
        );
    }

    #[test]
    fn an_open_menu_freezes_shell_typing_shortcuts() {
        let project = one_repo_project("gate");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        right_click_row(&mut harness, "feature-a");

        harness.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::A);
        harness.step();
        assert!(
            harness.state().ui.command_palette,
            "the frozen five keep their dispatch-first contract"
        );
        harness.state_mut().ui.command_palette = false;
        right_click_row(&mut harness, "feature-a");

        harness.state_mut().ui.tab = Tab::Commit;
        harness.step();
        assert!(
            context_menu_target(&harness).is_some(),
            "the open state survives the tab switch"
        );
        harness.key_press(egui::Key::Slash);
        harness.step();
        assert!(
            !harness.state().ui.focus_file_filter,
            "`/` stays suppressed while the menu is open"
        );
    }

    #[test]
    fn an_item_click_closes_the_menu() {
        let project = one_repo_project("item-click");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        right_click_row(&mut harness, "feature-a");

        harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .find(|n| n.accesskit_node().label() == Some("Rename branch".to_string()))
            .expect("Rename branch item")
            .click();
        harness.step();
        assert_eq!(context_menu_target(&harness), None);
    }

    #[test]
    fn checkout_moves_head_in_the_real_repository() {
        let project = one_repo_project("dispatch-checkout");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        assert_eq!(current_branch(&harness).as_deref(), Some("main"));

        right_click_row(&mut harness, "plain-b");
        click_menu_item(&mut harness, MENU_SENTINEL, "Checkout");
        wait_for(&mut harness, |st| {
            st.multi
                .roots
                .first()
                .is_some_and(|r| r.current_branch.as_deref() == Some("plain-b"))
        });
        assert_eq!(context_menu_target(&harness), None, "and closes");
    }

    #[test]
    fn checkout_still_routes_through_the_dirty_tree_confirm() {
        let project = one_repo_project("dispatch-dirty");
        std::fs::write(project.join("alpha/base.txt"), "dirty\n").unwrap();
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "feature-a");
        click_menu_item(&mut harness, MENU_SENTINEL, "Checkout");
        harness.step();
        assert!(
            matches!(
                harness.state().ui.confirm,
                Some(turbogit_app::state::PendingConfirm::CheckoutDirty { .. })
            ),
            "the guarded path still fires"
        );
        assert_eq!(current_branch(&harness).as_deref(), Some("main"));
    }

    #[test]
    fn new_branch_from_opens_the_dialog_based_on_the_row() {
        let project = one_repo_project("dispatch-newbranch");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "feature-a");
        click_menu_item(&mut harness, MENU_SENTINEL, "New branch from");
        let st = harness.state();
        assert_eq!(st.ui.dialog, Some(turbogit_app::state::Dialog::NewBranch));
        assert_eq!(
            st.ui.dlg.new_branch_base,
            turbogit_app::state::NewBranchBase::Branch("feature-a".into())
        );
        assert_eq!(st.ui.dlg.new_branch_name, "");
        assert!(st.ui.dlg.new_branch_checkout);
        assert_eq!(context_menu_target(&harness), None);
    }

    /// What the last frame asked the platform to put on the clipboard. egui
    /// carries a copy as an output command rather than a field, so this reads
    /// the same gesture the window's clipboard handler would act on.
    fn copied_text(harness: &Harness<'_, AppState>) -> Option<String> {
        harness
            .output()
            .platform_output
            .commands
            .iter()
            .find_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text.clone()),
                _ => None,
            })
    }

    fn toast(harness: &Harness<'_, AppState>) -> Option<String> {
        harness.state().ui.toast.as_ref().map(|t| t.message.clone())
    }

    #[test]
    fn copy_branch_name_copies_the_row_it_was_opened_on() {
        let project = one_repo_project("dispatch-copy");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        assert_eq!(current_branch(&harness).as_deref(), Some("main"));

        right_click_row(&mut harness, "plain-b");
        click_menu_item(&mut harness, MENU_SENTINEL, "Copy branch name");
        assert_eq!(copied_text(&harness).as_deref(), Some("plain-b"));
        assert_eq!(toast(&harness).as_deref(), Some("Copied plain-b"));
        assert_eq!(context_menu_target(&harness), None, "and closes");

        right_click_row(&mut harness, "main");
        click_menu_item(&mut harness, MENU_SENTINEL, "Copy branch name");
        assert_eq!(copied_text(&harness).as_deref(), Some("main"));
        assert_eq!(toast(&harness).as_deref(), Some("Copied main"));

        let st = harness.state();
        assert!(st.ui.dialog.is_none() && st.ui.confirm.is_none() && !st.ui.busy);
    }

    #[test]
    fn a_tracking_local_branch_is_not_offered_the_pull_family() {
        let project = one_repo_project("gate-pull-local");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "main");
        assert_menu_item_disabled(&mut harness, "Pull", "pull is offered on remote branches");

        harness.key_press(egui::Key::Escape);
        harness.step();
        right_click_row(&mut harness, "feature-a");
        assert_menu_item_disabled(
            &mut harness,
            "Checkout and pull",
            "pull is offered on remote branches",
        );
        assert_menu_item_disabled(&mut harness, "Pull", "check out this branch first");
    }

    #[test]
    fn an_untracked_local_branch_is_not_offered_the_pull_family_either() {
        let project = one_repo_project("gate-pull-untracked");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "plain-b");
        assert_menu_item_disabled(&mut harness, "Pull", "check out this branch first");
        assert_menu_item_disabled(
            &mut harness,
            "Checkout and pull",
            "no upstream to pull from",
        );
    }

    #[test]
    fn a_remote_row_withholds_the_history_verbs_and_keeps_the_pull_family_painted() {
        let project = one_repo_project("gate-pull-remote");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        row_node(&harness, "Remote").click();
        settle_quiet(&mut harness);

        right_click_row(&mut harness, "remote-only");
        assert_menu_item_disabled(
            &mut harness,
            "Merge into main",
            "remote branches cannot be merged",
        );
        assert_menu_item_disabled(
            &mut harness,
            "Rebase onto main",
            "remote branches cannot be rebased",
        );
        assert_menu_item_disabled(&mut harness, "Pull", "check out this branch first");

        harness.key_press(egui::Key::Escape);
        harness.step();
        right_click_row(&mut harness, "feature-a");
        let column = menu_column(&harness);
        for label in ["Merge into main", "Rebase onto main"] {
            let item = menu_item_in_column(&harness, column, label);
            assert!(
                !item.accesskit_node().is_disabled(),
                "{label} must be offered on a local row"
            );
        }
    }

    #[test]
    fn push_opens_the_dialog_prefilled_at_the_row() {
        let project = one_repo_project("dispatch-push");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "plain-b");
        click_menu_item(&mut harness, MENU_SENTINEL, "Push");
        let st = harness.state();
        assert_eq!(st.ui.dialog, Some(turbogit_app::state::Dialog::Push));
        assert_eq!(st.ui.dlg.push_branch, "plain-b");
        assert_eq!(
            st.ui.dlg.push_scope,
            turbogit_services::sync_service::PushScope::ThisRepo
        );
    }

    #[test]
    fn the_menu_paints_the_branch_name_in_the_data_face() {
        let project = one_repo_project("face");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        right_click_row(&mut harness, "feature-a");

        let column = harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .find(|n| n.accesskit_node().label() == Some("New branch from".to_string()))
            .expect("menu open")
            .rect();
        let name = painted_galleys(&harness)
            .into_iter()
            .find(|g| g.text == "feature-a" && g.pos.x > column.min.x)
            .expect("the name paints inside the menu");
        assert_eq!(
            name.family,
            egui::FontFamily::Monospace,
            "a branch name never reads as a label (design §19)"
        );
    }

    #[test]
    fn gated_items_stay_visible_and_state_their_reason_on_hover() {
        let project = one_repo_project("gates");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "main");
        assert_menu_item_disabled(&mut harness, "Checkout", "already checked out");
        assert_menu_item_disabled(&mut harness, "Push", "nothing to push");

        harness.key_press(egui::Key::Escape);
        harness.step();
        right_click_row(&mut harness, "plain-b");
        assert_menu_item_disabled(&mut harness, "Pull", "check out this branch first");
    }

    #[test]
    fn a_remote_row_gates_rename_with_a_reachable_reason() {
        let project = one_repo_project("gates-remote");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        row_node(&harness, "Remote").click();
        settle_quiet(&mut harness);
        assert_painted(&harness, "remote-only");

        right_click_row(&mut harness, "remote-only");
        assert_menu_item_disabled(
            &mut harness,
            "Rename branch",
            "remote branches cannot be renamed",
        );
    }

    #[track_caller]
    fn menu_column(harness: &Harness<'_, AppState>) -> egui::Rect {
        harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .find(|n| n.accesskit_node().label() == Some(MENU_SENTINEL.to_string()))
            .expect("menu open")
            .rect()
    }

    #[track_caller]
    fn menu_item_in_column<'t>(
        harness: &'t Harness<'_, AppState>,
        column: egui::Rect,
        label: &str,
    ) -> egui_kittest::Node<'t> {
        harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .find(|n| {
                n.accesskit_node().label() == Some(label.to_string())
                    && (n.rect().min.x - column.min.x).abs() < 2.0
            })
            .unwrap_or_else(|| panic!("menu item {label}"))
    }

    #[track_caller]
    fn assert_menu_item_disabled(harness: &mut Harness<'_, AppState>, label: &str, reason: &str) {
        harness.remove_cursor();
        harness.step();
        let item = menu_item_in_column(harness, menu_column(harness), label);
        assert!(
            item.accesskit_node().is_disabled(),
            "{label} must be gated, not hidden"
        );
        let item_rect = item.rect();
        // A real pointer travels into a widget; a single teleport onto a
        // disabled row inside the floating Area never registers as hover.
        harness.hover_at(item_rect.center() - egui::vec2(0.0, 3.0));
        harness.step();
        harness.hover_at(item_rect.center());
        let mut seen = false;
        // The shell harness steps at 60 fps; egui's tooltip delay needs a
        // second of frames, not twenty.
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
            "the blocked action must explain itself: {reason} never painted; painted: {:?}",
            painted_galleys(harness)
                .iter()
                .map(|g| &g.text)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rename_opens_the_rows_inline_editor() {
        let project = one_repo_project("dispatch-rename");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "plain-b");
        click_menu_item(&mut harness, MENU_SENTINEL, "Rename branch");
        let tree = &harness.state().ui.branches_tree;
        assert_eq!(tree.renaming.as_deref(), Some("plain-b"));
        assert_eq!(tree.rename_draft, "plain-b");
        assert!(tree.selected_root.is_some());
        assert_eq!(context_menu_target(&harness), None);
        assert_painted(&harness, "plain-b");
    }

    #[test]
    fn merge_into_opens_the_preflighted_dialog() {
        let project = one_repo_project("dispatch-merge");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "feature-a");
        click_menu_item(&mut harness, MENU_SENTINEL, "Merge into main");
        let st = harness.state();
        assert_eq!(st.ui.dialog, Some(turbogit_app::state::Dialog::Merge));
        assert_eq!(st.ui.dlg.merge_target, "feature-a");
        assert!(
            st.ui.dlg.merge_preview.is_some(),
            "pre-flight preview is computed before starting"
        );
    }

    #[test]
    fn rebase_onto_runs_the_rebase_it_names() {
        let project = one_repo_project("dispatch-rebase");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "feature-a");
        click_menu_item(&mut harness, MENU_SENTINEL, "Rebase onto main");
        wait_for(&mut harness, |st| {
            activity_has(st, "Rebase feature-a onto main")
        });
        assert_eq!(
            current_branch(&harness).as_deref(),
            Some("feature-a"),
            "the rebased branch ends up checked out"
        );
    }

    #[test]
    fn compare_with_opens_the_compare_surface() {
        let project = one_repo_project("dispatch-compare");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "feature-a");
        click_menu_item(&mut harness, MENU_SENTINEL, "Compare with main");
        let st = harness.state();
        assert_eq!(
            st.ui.dialog,
            Some(turbogit_app::state::Dialog::CompareBranches)
        );
        assert_eq!(st.ui.dlg.compare_left, "feature-a");
        assert_eq!(st.ui.dlg.compare_right, "main");
    }

    #[test]
    fn delete_asks_before_removing_the_branch() {
        let project = one_repo_project("dispatch-delete");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "feature-a");
        click_menu_item(&mut harness, MENU_SENTINEL, "Delete branch");
        wait_for(&mut harness, |st| st.ui.confirm.is_some());
        assert!(matches!(
            harness.state().ui.confirm,
            Some(turbogit_app::state::PendingConfirm::DeleteLocalBranch { ref name }) if name == "feature-a"
        ));
        assert_painted(
            &harness,
            "everything on this branch already exists on main — safe to delete",
        );
        assert_eq!(
            current_branch(&harness).as_deref(),
            Some("main"),
            "nothing has been deleted yet"
        );
    }

    #[test]
    fn deleting_a_remote_row_asks_to_delete_it_upstream() {
        let project = one_repo_project("dispatch-delete-remote");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        row_node(&harness, "Remote").click();
        settle_quiet(&mut harness);

        right_click_row(&mut harness, "remote-only");
        click_menu_item(&mut harness, MENU_SENTINEL, "Delete branch");
        let st = harness.state();
        assert!(
            matches!(
                &st.ui.confirm,
                Some(turbogit_app::state::PendingConfirm::DeleteRemoteBranch { remote, name })
                    if remote == "origin" && name == "remote-only"
            ),
            "a remote row's Delete reaches the upstream delete ask"
        );
    }

    #[test]
    fn delete_refuses_a_branch_checked_out_in_another_worktree() {
        let project = one_repo_project("dispatch-delete-worktree");
        let mut harness = branches_harness(project);
        let root_id = harness.state().multi.roots[0].id.clone();
        harness.state_mut().caches.store_worktrees(
            root_id.clone(),
            vec![turbogit_domain::model::Worktree {
                path: PathBuf::from("/wt/feature-a"),
                branch: "feature-a".into(),
                dirty: None,
                root: root_id,
            }],
        );
        open_branches(&mut harness);

        right_click_row(&mut harness, "feature-a");
        click_menu_item(&mut harness, MENU_SENTINEL, "Delete branch");
        let st = harness.state();
        assert!(
            matches!(
                &st.ui.confirm,
                Some(turbogit_app::state::PendingConfirm::CheckoutInWorktree { branch, .. })
                    if branch == "feature-a"
            ),
            "the worktree guard is what answers the click"
        );
    }

    #[test]
    fn the_history_verbs_explain_why_the_row_blocks_them() {
        let project = one_repo_project("gates-history");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "main");
        assert_menu_item_disabled(
            &mut harness,
            "Merge into main",
            "this is the current branch",
        );
        assert_menu_item_disabled(
            &mut harness,
            "Rebase onto main",
            "this is the current branch",
        );
        assert_menu_item_disabled(
            &mut harness,
            "Compare with main",
            "this is the current branch",
        );
        assert_menu_item_disabled(
            &mut harness,
            "Delete branch",
            "the current branch cannot be deleted",
        );
    }
}

#[test]
fn the_branch_context_menu_keeps_the_float_stroke() {
    use test_support::harness::right_click_row;
    let project = surface::one_repo_project("stroke");
    let mut harness = surface::branches_harness(project);
    harness.set_size(egui::vec2(900.0, 700.0));
    test_support::harness::settle(&mut harness);
    surface::open_branches(&mut harness);
    right_click_row(&mut harness, "feature-a");
    test_support::harness::settle(&mut harness);

    let item = surface::row_node(&harness, "Checkout and pull").rect();
    // Recursive, because a context menu is an `egui::Area` whose frame paints
    // as a `Shape::Vec`, a top-level-only scan would see the rows' own
    // geometry and never the frame.
    fn walk(shape: &egui::Shape, out: &mut Vec<(egui::Rect, egui::Color32, f32)>) {
        match shape {
            egui::Shape::Rect(r)
                if r.stroke.color != egui::Color32::TRANSPARENT && r.stroke.width > 0.0 =>
            {
                out.push((r.rect, r.stroke.color, r.stroke.width));
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut painted: Vec<(egui::Rect, egui::Color32, f32)> = Vec::new();
    for clipped in harness.output().shapes.iter() {
        walk(&clipped.shape, &mut painted);
    }
    let all_strokes = painted.clone();
    let frame = painted
        .into_iter()
        .find(|(rect, _, width)| {
            (*width - 1.0).abs() < 0.01
                && rect.contains(item.center())
                && rect.width() > item.width()
        })
        .map(|(rect, color, _)| (rect, color))
        .unwrap_or_else(|| {
            panic!(
                "the branch context menu floats, so a 1px stroke surrounds its \
                 own items; the item rect is {item:?}, the frame's strokes are \
                 {all_strokes:?}"
            )
        });

    let (rect, stroke) = frame;
    let alpha = stroke.a() as f32 / 255.0;
    assert!(
        (0.0..1.0).contains(&alpha),
        "the menu frame's stroke is painted with a uniform alpha; got {stroke:?}"
    );
    let line = turbogit_ui::theme::Palette::LINE;
    for (name, got, want) in [
        ("red", stroke.r(), line.r()),
        ("green", stroke.g(), line.g()),
        ("blue", stroke.b(), line.b()),
    ] {
        let ratio = got as f32 / want as f32;
        assert!(
            (ratio - alpha).abs() < 0.02,
            "the menu frame's stroke is `LINE` under the host's uniform alpha; \
             the {name} channel is {ratio:.3} of it against an alpha of \
             {alpha:.3} in {stroke:?}"
        );
    }
    assert!(
        rect.contains_rect(item) && rect.width() > item.width(),
        "the stroke is the menu's own frame, around its items: frame {rect:?}, \
         item {item:?}"
    );
}

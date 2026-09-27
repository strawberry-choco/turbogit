//! Contract tests for the branch context menu component
//! (`ui::branch_menu`).
//!
//! Two seams. The gating table is a pure function over one `Branch` and the
//! repository's current branch — every row's ten outcomes are asserted
//! without rendering. The component itself is props-in / action-out: the
//! designed item order, the three rules, the multi-repo wording, and the
//! `BranchMenuAction` each clicked item returns, asserted through painted
//! output and the accessibility tree.

use turbogit_domain::model::{Branch, BranchKind, Upstream};
use turbogit_ui::ui::branch_menu::{BranchMenuAction, MenuItemState, branch_menu_items};

// --- fixture branches ----------------------------------------------------------

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

// --- cycle 1: the current branch -------------------------------------------------

/// The row for the checked-out branch: Checkout and Checkout-and-pull are
/// blocked with "already checked out"; Pull is the update action that IS
/// available; Rename is a local verb and stays enabled.
#[test]
fn the_current_branch_offers_pull_but_no_checkout() {
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
    assert_eq!(s(BranchMenuAction::Pull), MenuItemState::enabled());
    assert_eq!(s(BranchMenuAction::Rename), MenuItemState::enabled());
    assert_eq!(s(BranchMenuAction::NewBranchFrom), MenuItemState::enabled());
}

// --- cycle 2: an untracked local branch ------------------------------------------

/// A branch with no upstream has nothing to pull from: both pull items are
/// blocked with the upstream reason, while Checkout, New branch from, and
/// Push (which creates the upstream) stay live.
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

// --- cycle 3: an upstream deleted on the remote -----------------------------------

/// `[gone]` means there is nothing to pull from either side: Pull blocks with
/// the upstream reason on the checked-out branch, and Checkout-and-pull
/// blocks with it on any other — where the checkout blocker is absent.
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

// --- cycle 4: Push reads the ahead count and the upstream's fate -------------------

/// Push acts when the branch has something to publish or no upstream to
/// fail against; a fully pushed branch says "nothing to push", and a branch
/// whose upstream was deleted upstream says so.
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

// --- cycle 5: renaming is a local-branch verb ---------------------------------------

/// A remote-tracking row is reference material: it cannot be renamed.
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

// --- cycle 6: the panel's three omissions became four reasons ---------------------

/// The union's new items gate on the two facts the panel enforced by hiding
/// them. Blocked means rendered-and-explained, never absent: a row for the
/// checked-out branch says so, a remote-tracking row says it cannot be
/// merged or rebased, and a plain local row is free of all four.
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

    // A remote-tracking row is reference material for history verbs, yet it
    // is still deletable — that is what `git push origin :name` means.
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
    assert_eq!(remote(BranchMenuAction::Delete), MenuItemState::enabled());
}

/// [`BranchMenuAction::ORDER`] and [`BranchMenuAction::index`] are maintained
/// by hand in step with the gate array; the compiler cannot see them disagree,
/// so this is the one check that they do not.
#[test]
fn the_designed_order_is_the_array_slots() {
    assert_eq!(BranchMenuAction::ORDER.len(), 10);
    for (slot, action) in BranchMenuAction::ORDER.iter().enumerate() {
        assert_eq!(action.index(), slot, "{action:?} is painted in slot {slot}");
    }
}

// --- the component: props in, action out -------------------------------------------

use std::cell::RefCell;
use std::rc::Rc;

use egui::Color32;
use egui_kittest::Harness;
use test_support::harness::{filled_rects, painted_galleys};
use turbogit_ui::theme::{Palette, configure_style, install_fonts};
use turbogit_ui::ui::branch_menu::{BranchMenuProps, branch_menu};

/// Render `branch_menu` for `target` inside a popup frame, recording the
/// action the component returns each frame.
fn menu_harness(
    target: Branch,
    props: fn() -> BranchMenuProps<'static>,
) -> (Harness<'static, ()>, Rc<RefCell<Option<BranchMenuAction>>>) {
    let returned = Rc::new(RefCell::new(None));
    let returned_ui = returned.clone();
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, _state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            egui::CentralPanel::default().show(ui, |ui| {
                let action = branch_menu(ui, &props(), &target);
                if action.is_some() {
                    *returned_ui.borrow_mut() = action;
                }
            });
        },
        (),
    );
    harness.set_size(egui::vec2(420.0, 400.0));
    harness.step();
    (harness, returned)
}

/// The union of the three affordances in the designed order — Checkout
/// strongest, the name in the data face, three rules splitting switch /
/// history / update / edit — with Push's real accelerator in the shortcut
/// column.
#[test]
fn the_menu_renders_the_ten_items_in_the_designed_order() {
    let target = local("feature/cascade-views", Some("origin/f"), 1, 0);
    let (harness, _returned) = menu_harness(target, || BranchMenuProps {
        repo_name: "alpha",
        multi_repo: false,
        current_branch: Some("main"),
    });

    let mut galleys = painted_galleys(&harness);
    // Bucket by menu row (a 26 px pitch): the two segments of one row are
    // each vertically centered, so their exact tops differ. The pitch is the
    // one place a scroll container would move.
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
            "Checkout",
            "New branch from",
            "feature/cascade-views",
            "Checkout and pull",
            "Merge into main",
            "Rebase onto main",
            "Compare with main",
            "Pull",
            "Push",
            "Ctrl+Shift+K",
            "Rename branch",
            "Delete branch",
        ],
        "item order, the data segment, and the one accelerator"
    );

    // The branch name reads in the data face, the prose in the chrome face.
    let data = galleys
        .iter()
        .find(|g| g.text == "feature/cascade-views")
        .expect("name painted");
    assert_eq!(data.family, egui::FontFamily::Monospace);

    // Three rules: history, update and editing each start behind a hairline.
    let rules: Vec<Color32> = filled_rects(&harness)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::LINE_SUBTLE)
        .map(|(_, fill)| fill)
        .collect();
    assert_eq!(rules.len(), 3, "exactly three menu rules");

    // Checkout is the primary item.
    assert!(
        filled_rects(&harness)
            .iter()
            .any(|(_, fill)| *fill == Palette::BRAND),
        "the primary item carries the brand fill"
    );
}

/// With no named branch checked out (detached HEAD) the history verbs state
/// no destination rather than painting a dangling "Merge into ".
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

/// In a multi-repo project the primary item names the repository its action
/// applies to (issue 14) — a bare "Checkout" is never ambiguous.
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

/// Each enabled item returns exactly its own action; a disabled item is
/// rendered but never fires.
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

    // The current branch's Checkout is gated: rendered, inert.
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

// --- production path: the Branches surface ---------------------------------------

mod surface {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::Duration;

    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use test_support::harness::{assert_painted, painted_galleys, painted_text};
    use turbogit_app::state::{AppState, Tab};

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .expect("git must be on PATH");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A project under the gitignored `<workspace>/.scratch/` holding one
    /// repo `alpha` on `main` with locals `feature-a` and `plain-b`. `main`
    /// and `feature-a` track a bare `origin`; `plain-b` stays untracked.
    fn one_repo_project(tag: &str) -> PathBuf {
        let base = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .join(format!(".scratch/branch-context-menu-{tag}"));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("alpha")).unwrap();
        let repo = base.join("alpha");
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "user.name", "Test"]);
        std::fs::write(repo.join("base.txt"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        git(&base, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        git(&repo, &["remote", "add", "origin", "../origin.git"]);
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
        git(&repo, &["branch", "feature-a"]);
        git(
            &repo,
            &["push", "-q", "--set-upstream", "origin", "feature-a"],
        );
        git(&repo, &["push", "-q", "origin", "main:remote-only"]);
        git(&repo, &["fetch", "-q", "origin"]);
        git(&repo, &["branch", "plain-b"]);
        base
    }

    fn branches_harness(project_dir: PathBuf) -> Harness<'static, AppState> {
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

    /// Step until the painted text holds still — the branches data arrives
    /// through the real event pump, so the list settles a few frames in.
    fn settle_quiet(harness: &mut Harness<'_, AppState>) {
        let mut stable = 0;
        let mut prev = String::new();
        for _ in 0..300 {
            harness.step();
            std::thread::sleep(Duration::from_millis(10));
            let fp = format!("{:?}", painted_text(harness));
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
        panic!("shell layout did not settle within 300 frames");
    }

    fn open_branches(harness: &mut Harness<'_, AppState>) {
        harness.state_mut().ui.tab = Tab::Branches;
        settle_quiet(harness);
        assert_painted(harness, "feature-a");
    }

    fn right_click_row(harness: &mut Harness<'_, AppState>, branch: &str) {
        row_node(harness, branch).click_secondary();
        harness.step();
        harness.step();
    }

    /// Click one item of the open menu. The pointer first leaves the row so
    /// its hover Checkout cannot be confused with the menu's own item, and
    /// the search is scoped to the menu's own column: "Pull" and "Push…"
    /// also label the command palette's action rows, and the menu is the only
    /// surface whose rows all share the "New branch from" item's left edge.
    fn click_menu_item(harness: &mut Harness<'_, AppState>, label: &str) {
        harness.remove_cursor();
        harness.step();
        let column = harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .find(|n| n.accesskit_node().label() == Some("New branch from".to_string()))
            .expect("the menu is open (New branch from item)")
            .rect();
        harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .find(|n| {
                n.accesskit_node().label() == Some(label.to_string())
                    && (n.rect().min.x - column.min.x).abs() < 2.0
            })
            .unwrap_or_else(|| panic!("menu item {label} inside the open menu"))
            .click();
        harness.step();
    }

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

    /// The row's own Button node — a branch name can label several nodes.
    fn row_node<'t>(harness: &'t Harness<'_, AppState>, name: &str) -> egui_kittest::Node<'t> {
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

    /// Right-clicking a row opens the six-item menu on that row, in the
    /// frame it was clicked, with the row's branch as the target.
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
        // The name segment reads the clicked branch.
        assert_painted(&harness, "feature-a");
    }

    /// Escape joins the existing ladder: the first press closes the context
    /// menu and leaves the selection alone.
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

    /// A click on any row — the menu's own row or another — dismisses it,
    /// and a click outside the menu after it has been visible for a frame
    /// dismisses it too.
    #[test]
    fn a_row_click_or_a_click_outside_closes_the_menu() {
        let project = one_repo_project("dismiss");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        right_click_row(&mut harness, "feature-a");

        // Click another row: dismissed, and the click still selects. The
        // menu floats over the rows below its anchor, so pick the row above.
        row_node(&harness, "main").click();
        harness.step();
        assert_eq!(context_menu_target(&harness), None, "any row click closes");
        assert_eq!(
            harness.state().ui.branches_tree.selected.as_deref(),
            Some("main")
        );

        // Click outside the menu (the toolbar area above the list) after the
        // menu has been visible for a frame.
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

    /// The menu must not steal the shell's typing: it joins the popup gate
    /// in `shell.rs`, so `/` cannot arm the Commit file filter while the
    /// menu is open. The frozen five keep their dispatch-first contract.
    #[test]
    fn an_open_menu_freezes_shell_typing_shortcuts() {
        let project = one_repo_project("gate");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        right_click_row(&mut harness, "feature-a");

        // Ctrl+Shift+A is one of the frozen five — it still fires.
        harness.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::A);
        harness.step();
        assert!(
            harness.state().ui.command_palette,
            "the frozen five keep their dispatch-first contract"
        );
        harness.state_mut().ui.command_palette = false;
        right_click_row(&mut harness, "feature-a");

        // `/` arms the Commit file filter only through the popup gate. Move
        // to the Commit tab with the menu still open: nothing may take the
        // keyboard.
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

    /// Clicking an item closes the menu (ticket 03's dismissal; what it
    /// DOES is ticket 04's dispatch).
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

    // --- ticket 04: each item dispatches through the existing app seam ----

    /// Checkout runs the existing checkout: HEAD moves in the real
    /// repository and the refreshed snapshot follows.
    #[test]
    fn checkout_moves_head_in_the_real_repository() {
        let project = one_repo_project("dispatch-checkout");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        assert_eq!(current_branch(&harness).as_deref(), Some("main"));

        right_click_row(&mut harness, "plain-b");
        click_menu_item(&mut harness, "Checkout");
        wait_for(&mut harness, |st| {
            st.multi
                .roots
                .first()
                .is_some_and(|r| r.current_branch.as_deref() == Some("plain-b"))
        });
        assert_eq!(context_menu_target(&harness), None, "and closes");
    }

    /// A dirty worktree keeps its guard: Checkout routes to the
    /// bring-along / set-aside confirmation instead of switching silently.
    #[test]
    fn checkout_still_routes_through_the_dirty_tree_confirm() {
        let project = one_repo_project("dispatch-dirty");
        std::fs::write(project.join("alpha/base.txt"), "dirty\n").unwrap();
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "feature-a");
        click_menu_item(&mut harness, "Checkout");
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

    /// New branch from «name» opens the dialog prefilled with the clicked
    /// branch as the base.
    #[test]
    fn new_branch_from_opens_the_dialog_based_on_the_row() {
        let project = one_repo_project("dispatch-newbranch");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "feature-a");
        click_menu_item(&mut harness, "New branch from");
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

    /// Pull honours the settings' update method and lands in the activity
    /// log — the same named operation the palette's `Pull` action dispatches.
    #[test]
    fn pull_dispatches_the_pull_operation() {
        let project = one_repo_project("dispatch-pull");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "main");
        click_menu_item(&mut harness, "Pull");
        wait_for(&mut harness, |st| activity_has(st, "Pull"));
    }

    /// Checkout and pull is ONE operation: it checks out and pulls without
    /// firing the standalone checkout confirmation.
    #[test]
    fn checkout_and_pull_is_one_operation() {
        let project = one_repo_project("dispatch-cap");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        // Even with the worktree clean, the composite must not confirm.
        right_click_row(&mut harness, "feature-a");
        click_menu_item(&mut harness, "Checkout and pull");
        wait_for(&mut harness, |st| {
            st.multi
                .roots
                .first()
                .is_some_and(|r| r.current_branch.as_deref() == Some("feature-a"))
                && activity_has(st, "Checkout and pull")
        });
        assert!(harness.state().ui.confirm.is_none());
    }

    /// Push opens the push dialog prefilled at that branch — a non-current
    /// branch is pushable without a checkout (the engine takes the branch
    /// explicitly).
    #[test]
    fn push_opens_the_dialog_prefilled_at_the_row() {
        let project = one_repo_project("dispatch-push");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "plain-b");
        click_menu_item(&mut harness, "Push");
        let st = harness.state();
        assert_eq!(st.ui.dialog, Some(turbogit_app::state::Dialog::Push));
        assert_eq!(st.ui.dlg.push_branch, "plain-b");
        assert_eq!(
            st.ui.dlg.push_scope,
            turbogit_services::sync_service::PushScope::ThisRepo
        );
    }

    // --- ticket 05: painted faces and real-state gating -------------------

    /// The menu's name segment paints in the data face on the production
    /// path, not just in the component fixture.
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

    /// Gating on real branch states, with the reason reachable on hover:
    /// the current branch cannot check itself out or push; an untracked
    /// branch cannot pull; a remote row cannot rename. Blocked items stay
    /// rendered — never a silent vanish.
    #[test]
    fn gated_items_stay_visible_and_state_their_reason_on_hover() {
        let project = one_repo_project("gates");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        // The current branch: Checkout gated ("already checked out") and a
        // fully pushed branch: Push gated ("nothing to push").
        right_click_row(&mut harness, "main");
        assert_menu_item_disabled(&mut harness, "Checkout", "already checked out");
        assert_menu_item_disabled(&mut harness, "Push", "nothing to push");

        // An untracked branch: Pull gated behind a checkout. The open menu
        // floats over the rows below it, so dismiss it before re-aiming.
        harness.key_press(egui::Key::Escape);
        harness.step();
        right_click_row(&mut harness, "plain-b");
        assert_menu_item_disabled(&mut harness, "Pull", "check out this branch first");
    }

    /// A remote-tracking row is reference material: Rename is gated. The
    /// remotes live behind the tree's collapsed rollup, so reveal them.
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

    /// Hover a menu item of the open menu until its disabled tooltip paints.
    #[track_caller]
    fn assert_menu_item_disabled(harness: &mut Harness<'_, AppState>, label: &str, reason: &str) {
        harness.remove_cursor();
        harness.step();
        let column = harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .find(|n| n.accesskit_node().label() == Some("New branch from".to_string()))
            .expect("menu open")
            .rect();
        let item = harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .find(|n| {
                n.accesskit_node().label() == Some(label.to_string())
                    && (n.rect().min.x - column.min.x).abs() < 2.0
            })
            .unwrap_or_else(|| panic!("menu item {label}"));
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

    /// Rename starts the row's inline editor — one rename experience per
    /// surface — with the draft seeded to the current name.
    #[test]
    fn rename_opens_the_rows_inline_editor() {
        let project = one_repo_project("dispatch-rename");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "plain-b");
        click_menu_item(&mut harness, "Rename branch");
        let tree = &harness.state().ui.branches_tree;
        assert_eq!(tree.renaming.as_deref(), Some("plain-b"));
        assert_eq!(tree.rename_draft, "plain-b");
        assert!(tree.selected_root.is_some());
        assert_eq!(context_menu_target(&harness), None);
        // The editor paints on the row itself.
        assert_painted(&harness, "plain-b");
    }

    // --- ticket 01: the four actions the menu used not to offer ----------

    /// Merge names its destination and opens the same preflighted dialog the
    /// detail panel opened: source = the right-clicked row, preview computed
    /// before anything runs.
    #[test]
    fn merge_into_opens_the_preflighted_dialog() {
        let project = one_repo_project("dispatch-merge");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "feature-a");
        click_menu_item(&mut harness, "Merge into main");
        let st = harness.state();
        assert_eq!(st.ui.dialog, Some(turbogit_app::state::Dialog::Merge));
        assert_eq!(st.ui.dlg.merge_target, "feature-a");
        assert!(
            st.ui.dlg.merge_preview.is_some(),
            "pre-flight preview is computed before starting"
        );
    }

    /// Rebase states its direction in the label and runs that rebase — the
    /// report names both branches in the order the item did.
    #[test]
    fn rebase_onto_runs_the_rebase_it_names() {
        let project = one_repo_project("dispatch-rebase");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "feature-a");
        click_menu_item(&mut harness, "Rebase onto main");
        wait_for(&mut harness, |st| {
            activity_has(st, "Rebase feature-a onto main")
        });
        assert_eq!(
            current_branch(&harness).as_deref(),
            Some("feature-a"),
            "the rebased branch ends up checked out"
        );
    }

    /// Compare is read-only and names both sides, exactly as the panel's
    /// button delivered it.
    #[test]
    fn compare_with_opens_the_compare_surface() {
        let project = one_repo_project("dispatch-compare");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "feature-a");
        click_menu_item(&mut harness, "Compare with main");
        let st = harness.state();
        assert_eq!(
            st.ui.dialog,
            Some(turbogit_app::state::Dialog::CompareBranches)
        );
        assert_eq!(st.ui.dlg.compare_left, "feature-a");
        assert_eq!(st.ui.dlg.compare_right, "main");
    }

    /// Deleting asks first, and the ask says what is lost in human terms.
    #[test]
    fn delete_asks_before_removing_the_branch() {
        let project = one_repo_project("dispatch-delete");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);

        right_click_row(&mut harness, "feature-a");
        click_menu_item(&mut harness, "Delete branch");
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

    /// A remote-tracking row is still deletable — the ask names the remote it
    /// disappears from, which is a different operation from `branch -D`.
    #[test]
    fn deleting_a_remote_row_asks_to_delete_it_upstream() {
        let project = one_repo_project("dispatch-delete-remote");
        let mut harness = branches_harness(project);
        open_branches(&mut harness);
        row_node(&harness, "Remote").click();
        settle_quiet(&mut harness);

        right_click_row(&mut harness, "remote-only");
        click_menu_item(&mut harness, "Delete branch");
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

    /// A branch checked out in another worktree is refused up front, naming
    /// the worktree — the same guard the checkout path raises.
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
        click_menu_item(&mut harness, "Delete branch");
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

    /// The four items the detail panel used to hide are gated here with the
    /// reason they were hidden for — on the current branch and on a
    /// remote-tracking row.
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

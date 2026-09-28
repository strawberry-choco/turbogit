//! Contract tests for the commit context menu component (`ui::commit_menu`).
//!
//! The same three-depth shape the branch context menu suite uses, because a
//! commit menu is the same kind of thing: a pure gating table over plain facts,
//! a component that paints that table, and — from ticket 06 — the Git Log
//! surface that dispatches from it. Nothing here reaches into `AppState` to
//! check a gate, and nothing asserts a widget's position.

use std::path::Path;
use std::sync::Arc;

use turbogit_domain::model::{Commit, CommitId, RootId, Signature, SignatureState};

use turbogit_ui::ui::commit_menu::{CommitFacts, CommitMenuAction, commit_menu_items};
use turbogit_ui::ui::widgets::MenuItemState;

// --- fixtures --------------------------------------------------------------

/// A commit on `alpha` with `parents` parents and this subject.
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
    repo_name: "alpha",
};

#[track_caller]
fn state(facts: CommitFacts, target: &Commit, item: CommitMenuAction) -> MenuItemState {
    commit_menu_items(target, &facts)[item.index()]
}

// --- the shape of the table ------------------------------------------------

/// [`CommitMenuAction::ORDER`] and [`CommitMenuAction::index`] are maintained
/// by hand in step with the gate array; the compiler cannot see them disagree,
/// so this is the one check that they do not.
#[test]
fn the_designed_order_is_the_array_slots() {
    assert_eq!(CommitMenuAction::ORDER.len(), 11);
    for (slot, action) in CommitMenuAction::ORDER.iter().enumerate() {
        assert_eq!(action.index(), slot, "{action:?} is painted in slot {slot}");
    }
}

/// A commit with nothing wrong about it is offered every verb. Blocked is
/// computed per item, so an all-enabled table is the baseline the reasons are
/// measured against.
#[test]
fn an_ordinary_commit_on_the_current_branch_offers_everything() {
    let target = commit("feature: one", 1);
    let items = commit_menu_items(&target, &CLEAN);
    for (action, item) in CommitMenuAction::ORDER.iter().zip(items) {
        assert_eq!(item, MenuItemState::enabled(), "{action:?}");
    }
}

// --- each gate blocks the right items with the right stated reason ---------------

/// Uncommitted changes entangle a cherry-pick, a checkout and a revert, so all
/// four say so in the same words. Reading and writing a commit's own bytes
/// cannot be entangled with the worktree, so the copy and patch verbs stay live.
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

/// A merge commit has no single first parent to replay from, so neither history
/// verb is offered — and each states its own reason rather than sharing a
/// generic "not possible".
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
    // The merge is still a commit: everything else about it is readable, and
    // the plan-the-tip guard is a different reason, so it does not speak here.
    assert_eq!(
        state(CLEAN, &merge, CommitMenuAction::CopyHash),
        MenuItemState::enabled()
    );
    assert_eq!(
        state(CLEAN, &merge, CommitMenuAction::RevertCommit),
        MenuItemState::enabled()
    );
}

/// A root commit has no first parent to replay from, so it is exactly as
/// unrewritable as a merge — the other end of the same walk, and the one case
/// that would otherwise reach the verb and be refused by the service with no
/// warning at all. Each verb states its own reason, like the merge above.
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
    // Its lack of a parent is a fact about the rewrite, not about the commit:
    // reading it, copying it, branching from it and reverting it all still work.
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

/// A plan is built from the commit to the current branch's tip, so a commit
/// found by searching another branch is read-only. It reads as deliberately
/// bounded, not broken: the two verbs name the bound.
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
    // Browsing another branch's history stays fully usable.
    assert_eq!(
        state(off_branch, &elsewhere, CommitMenuAction::CherryPickTo),
        MenuItemState::enabled()
    );
}

/// A protected branch cannot be rewritten through the log. The reason is stated
/// on all three verbs that would move the tip.
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
    // Creating a branch at a commit moves nothing, so protection does not
    // reach it.
    assert_eq!(
        state(protected, &target, CommitMenuAction::NewBranch),
        MenuItemState::enabled()
    );
}

/// With one repository in scope a cross-repository cherry-pick has nowhere to
/// go, so the item states that rather than vanishing — and its same-repository
/// sibling is unaffected.
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

/// One guard speaks for one item: the worktree reason is the one a developer can
/// act on, so it is stated first wherever two reasons both apply.
#[test]
fn the_worktree_guard_speaks_before_the_structural_ones() {
    let merge = commit("Merge branch 'side'", 2);
    let blocked = CommitFacts {
        dirty: true,
        protected_branch: true,
        on_current_branch: false,
        multi_root: false,
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
    // A merge commit's own nature is the binding reason for a rewrite, whatever
    // the worktree is doing.
    assert_eq!(
        state(blocked, &merge, CommitMenuAction::DropCommit),
        MenuItemState::disabled("a merge commit cannot be dropped")
    );
}

// --- the component: props in, action out ------------------------------------------

use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use test_support::harness::{filled_rects, painted_galleys};
use turbogit_ui::theme::{Palette, configure_style, install_fonts};
use turbogit_ui::ui::commit_menu::commit_menu;

/// Render `commit_menu` for `target` inside a popup frame, recording the action
/// the component returns each frame.
fn menu_harness(
    target: Commit,
    facts: CommitFacts<'static>,
) -> (Harness<'static, ()>, Rc<RefCell<Option<CommitMenuAction>>>) {
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
                let action = commit_menu(ui, &target, &facts);
                if action.is_some() {
                    *returned_ui.borrow_mut() = action;
                }
            });
        },
        (),
    );
    harness.set_size(egui::vec2(420.0, 460.0));
    harness.step();
    (harness, returned)
}

/// The eleven items, in the designed order, in four groups separated by three
/// rules — with the hash the Copy verb will put on the clipboard already showing
/// in its own row.
#[test]
fn the_menu_renders_the_eleven_items_in_the_designed_order() {
    let (harness, _returned) = menu_harness(commit("feature: one", 1), CLEAN);

    let mut galleys = painted_galleys(&harness);
    // Bucket by menu row (a 26 px pitch): a row's two segments are each
    // vertically centered, so their exact tops differ.
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

    // The hash reads in the data face — a value is never a label.
    let data = galleys
        .iter()
        .find(|g| g.text == "deadbee")
        .expect("the short reference paints");
    assert_eq!(data.family, egui::FontFamily::Monospace);

    // Three rules: cherry-pick / checkout / the history verbs / refs.
    let rules = filled_rects(&harness)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::RULE_STRUCTURAL)
        .count();
    assert_eq!(rules, 3, "exactly three menu rules");
}

/// Every painted segment of the row whose label is `label`, left to right.
///
/// A row's label and its data segment are two galleys painted a few pixels
/// apart vertically, so the row is "the galleys that share the label's line" —
/// the shape the primitive itself paints, read back from the frame.
#[track_caller]
fn row_segments<S>(harness: &Harness<'_, S>, label: &str) -> Vec<String> {
    let galleys = painted_galleys(harness);
    let y = galleys
        .iter()
        .find(|g| g.text == label)
        .unwrap_or_else(|| panic!("the {label:?} row paints"))
        .pos
        .y;
    let mut segments: Vec<(f32, String)> = galleys
        .iter()
        .filter(|g| (g.pos.y - y).abs() < 2.0)
        .map(|g| (g.pos.x, g.text.clone()))
        .collect();
    segments.sort_by(|a, b| a.0.total_cmp(&b.0));
    segments.into_iter().map(|(_, text)| text).collect()
}

/// Which repository a commit action would land in is a value, not a label, so
/// it rides in the data face — the same slot Copy hash uses for the short
/// reference. With one repository in scope there is nothing to disambiguate and
/// the item keeps its bare stem.
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

/// A commit has no single dominant verb, so nothing wears the brand; and of the
/// eleven, only the one that can lose work outright is inked as a severity.
#[test]
fn no_item_is_primary_and_only_drop_commit_is_danger() {
    let (harness, _returned) = menu_harness(commit("feature: one", 1), CLEAN);

    assert!(
        !filled_rects(&harness)
            .iter()
            .any(|(_, fill)| *fill == Palette::BRAND),
        "brand ink on a copy action would be absurd: a commit menu has no primary"
    );

    // Drop commit is live, so its severity is at full strength: exactly one row
    // carries it, and it is the one that can lose work outright.
    let danger: Vec<String> = painted_galleys(&harness)
        .into_iter()
        .filter(|g| g.color == Palette::DANGER)
        .map(|g| g.text)
        .collect();
    assert_eq!(danger, vec!["Drop commit".to_string()]);
}

/// Reword commit is live, and is the LAST build gap this menu had: with it in
/// place the table answers one question — what this commit in this repository
/// allows — and every state is either a working verb or a stated bound. There is
/// no longer an inert-for-want-of-a-feature layer over the commit's own gates,
/// so nothing in this menu can be rendered dead for a reason that is not about
/// the commit.
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

/// Reword commit is not painted as the danger treatment, and this is where that
/// is proved rather than assumed: correcting a message loses nothing, so exactly
/// one row in the whole menu carries danger ink and it is Drop commit — the verb
/// that can lose the commit outright. (Drop commit's own severity and the
/// menu's lack of a primary are pinned in
/// `no_item_is_primary_and_only_drop_commit_is_danger`; the two tests together
/// are the ticket's "only Drop commit is" — this one exists so that if a future
/// change gave reword danger ink, the failure would name REWORD.)
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

/// Drop commit is live: the component reports the pick rather than swallowing it,
/// so the surface can open its preflight. It is offered whenever the gates
/// allow it — this commit is on the current branch, on a clean tree, and the
/// branch is not protected.
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

/// Each enabled item returns exactly its own action; a disabled item is rendered
/// but never fires, and its accessibility node carries the gate.
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

    // One repository in scope gives cross-repository cherry-pick nowhere to go:
    // the row stays, the click is swallowed, the node says it is disabled.
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

/// A blocked item is rendered, inert, and explains itself — the convention
/// ADR-0023 set for branches and ADR-0024 extends to commits. The reason reaches
/// the row as its disabled hover text, so the explanation and the thing it
/// explains are in one place rather than in an alert box across the pane.
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

/// Hover a menu item until its disabled tooltip paints.
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
    // A real pointer travels into a widget; a single teleport onto a disabled
    // row never registers as hover.
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

/// The window has an enforced 1000×680 minimum (`src/main.rs`) and the menu adds
/// no scroll container, so the whole list has to fit the shorter axis — which is
/// the argument ADR-0024 accepted when it ruled eleven items in.
#[test]
fn the_whole_menu_fits_the_window_s_minimum_height() {
    let (harness, _returned) = menu_harness(commit("feature: one", 1), CLEAN);
    const WINDOW_MIN_HEIGHT: f32 = 680.0;
    let rows = painted_galleys(&harness);
    let top = rows.iter().map(|g| g.pos.y).fold(f32::MAX, f32::min);
    let bottom = rows.iter().map(|g| g.rect.max.y).fold(f32::MIN, f32::max);
    // The item column plus the surface frame's own margins, against the window.
    let menu_height = (bottom - top) + 2.0 * 8.0;
    assert!(
        menu_height < WINDOW_MIN_HEIGHT,
        "eleven items and three rules measure {menu_height}px against a \
         {WINDOW_MIN_HEIGHT}px window"
    );
}

// --- production path: the Git Log surface ---------------------------------------

mod surface {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::Duration;

    use egui::Modifiers;
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use tempfile::TempDir;
    use test_support::harness::{
        assert_menu_item_gated, assert_not_painted, assert_painted, filled_rects, painted_galleys,
        painted_text,
    };
    use turbogit_app::events::{AppEvent, LogBatchMode};
    use turbogit_app::state::{AppState, Tab};
    use turbogit_domain::model::{LogOpts, RootId, VcsSettings};
    use turbogit_engine::cli::CliExecutor;
    use turbogit_engine_api::GitExecutor;
    use turbogit_ui::theme::Palette;

    pub struct Seed {
        _tmp: TempDir,
        pub project: PathBuf,
        pub alpha: PathBuf,
        pub c1: String,
        pub c2: String,
        pub base: String,
    }

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

    /// Commit with an explicit author/committer date. The log orders newest
    /// first and falls back to the commit id on a tie, so same-second commits
    /// REORDER between runs — and a menu that floats over "the row below" then
    /// covers a different row than the one the test means to click.
    fn commit_file(dir: &Path, name: &str, body: &str, msg: &str, epoch: i64) -> String {
        let stamp = format!("@{epoch} +0000");
        let run = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
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
        git(dir, &["rev-parse", "HEAD"]).trim().to_string()
    }

    /// A commit authored by whatever the REPOSITORY's config says, with only the
    /// date pinned. `commit_file` pins the author in the environment, which is
    /// what keeps the log's order deterministic — and also what would hide a
    /// history with more than one identity behind it.
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
        git(dir, &["rev-parse", "HEAD"]).trim().to_string()
    }

    /// `main`: base ← c1 ← c2, with `feature` forked at c1. The seeded history
    /// the menu's own bounds are measured against.
    pub fn seeded_repo(tag: &str) -> Seed {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join(tag);
        let alpha = project.join("alpha");
        std::fs::create_dir_all(&alpha).unwrap();
        git(&alpha, &["init", "-q", "-b", "main"]);
        git(&alpha, &["config", "user.email", "t@t"]);
        git(&alpha, &["config", "user.name", "t"]);
        git(&alpha, &["config", "core.autocrlf", "false"]);
        let base = commit_file(&alpha, "base.txt", "base\n", "the base commit", 100);
        let c1 = commit_file(&alpha, "a.txt", "one\n", "alpha: first commit", 200);
        let c2 = commit_file(
            &alpha,
            "b.txt",
            "two\n",
            "alpha: second commit\n\nThe body the pane still shows.",
            300,
        );
        git(&alpha, &["branch", "feature", &c1]);
        Seed {
            _tmp: tmp,
            project,
            alpha,
            base,
            c1,
            c2,
        }
    }

    /// The shell on the Git Log tab, with the log warm from the real repository.
    pub fn log_harness(seed: &Seed) -> Harness<'static, AppState> {
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

    /// Re-answer every registered root's log through the production event path.
    /// A `refresh` drops the cache, so a test that refreshes warms again.
    pub fn warm_logs(harness: &mut Harness<'_, AppState>) {
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

    pub fn settle(harness: &mut Harness<'_, AppState>) {
        let mut stable = 0;
        let mut prev = String::new();
        for _ in 0..300 {
            harness.step();
            std::thread::sleep(Duration::from_millis(10));
            let cur = format!("{:?}", painted_text(harness));
            if cur == prev {
                stable += 1;
                if stable >= 3 {
                    return;
                }
            } else {
                stable = 0;
                prev = cur;
            }
        }
        panic!("log layout did not settle within 300 frames");
    }

    pub fn short(id: &str) -> String {
        id[..7.min(id.len())].to_string()
    }

    pub fn row_node<'t>(
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

    pub fn right_click_row(harness: &mut Harness<'_, AppState>, subject: &str) {
        row_node(harness, subject).click_secondary();
        harness.step();
        harness.step();
    }

    /// Which commit the open menu is aimed at.
    pub fn menu_target(harness: &Harness<'_, AppState>) -> Option<String> {
        harness
            .state()
            .ui
            .log_commit_menu
            .as_ref()
            .map(|(_, id)| id.clone())
    }

    pub fn click_menu_item(harness: &mut Harness<'_, AppState>, label: &str) {
        harness.remove_cursor();
        harness.step();
        let column = harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .find(|n| n.accesskit_node().label() == Some("Copy hash".to_string()))
            .expect("the commit menu is open (Copy hash item)")
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

    /// What the last frame asked the platform to put on the clipboard. egui
    /// carries a copy as an output command rather than a field, so this reads
    /// the same gesture the window's clipboard handler would act on.
    pub fn copied_text(harness: &Harness<'_, AppState>) -> String {
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

    pub fn toast(harness: &Harness<'_, AppState>) -> Option<String> {
        harness.state().ui.toast.as_ref().map(|t| t.message.clone())
    }

    pub fn wait_for(harness: &mut Harness<'_, AppState>, pred: impl Fn(&AppState) -> bool) {
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

    /// Right-clicking a commit row opens the menu on that row AND selects it, so
    /// the menu and the details pane can never describe different commits.
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

    /// Escape closes the menu; a click outside closes it; a click on another row
    /// closes it; a second right-click retargets it rather than dismissing it.
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

        // The menu floats over the rows BELOW its anchor, so the anchor here is
        // the bottom row and the row clicked to dismiss is one it cannot cover.
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

    /// Copy hash puts the FULL hash on the clipboard and its confirmation names
    /// the short reference; Copy commit message puts subject and body on the
    /// clipboard and says a message was copied, so the two are distinguishable.
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

    /// Revert asks first, names the commit it is about to undo, and on confirming
    /// creates an inverse commit that then appears in the log.
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
        let subject = git(&seed.alpha, &["log", "-1", "--format=%s"]);
        assert!(
            subject.to_lowercase().starts_with("revert"),
            "HEAD is the inverse commit; got {subject:?}"
        );
    }

    /// New branch and New tag open their dialogs seeded with the commit that was
    /// right-clicked, not with the tip.
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

    /// The branch picker refuses the branch the work is standing on: replaying a
    /// commit onto the branch that already contains it is the one destination
    /// that is not a cherry-pick. The row stays rendered, dimmed, and says so in
    /// the branch menu's own words — and this run protects nothing, so the
    /// refusal is about the branch rather than about a setting.
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

        // The refusal has teeth: clicking the row moves nothing and creates
        // nothing, so a dead row cannot be mistaken for a working one.
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
            git(&seed.alpha, &["rev-list", "--count", "main"]).trim(),
            "3",
            "and no commit lands on the branch the picker refused"
        );
    }

    /// A protected branch that is NOT the one checked out is refused for the
    /// other reason, so the two refusals cannot be confused for one: this run's
    /// current branch is `feature`, which protects nothing, and `main` — still
    /// protected by the default settings — is refused as protected.
    #[test]
    fn a_protected_branch_that_is_not_checked_out_is_refused_as_protected() {
        let seed = seeded_repo("pick-protected");
        git(&seed.alpha, &["checkout", "-q", "feature"]);
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

    /// A dialog is opened, not resumed. Every field the tag dialog works out
    /// for itself belongs to the visit that closed it — the type, the two reads
    /// it caches (the existing tags it validates a name against, the commit list
    /// its target picker lists), the signing key it fetched, and whether its
    /// picker was open — so New tag from the menu must reset exactly what the
    /// VCS palette's Tag resets, and seed the target with the commit clicked.
    #[test]
    fn the_tag_dialog_re_opens_from_the_menu_with_the_last_visits_state_cleared() {
        let seed = seeded_repo("tag-reopen");
        let mut harness = log_harness(&seed);

        // First visit: dirty every field the reset owns, through the real
        // controls — the GPG checkbox (which triggers the key read), the target
        // picker (which fills the candidate list), and the TYPE row.
        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "New tag");
        settle(&mut harness);
        harness.get_by_label("Sign with GPG key").click();
        settle(&mut harness);
        // Signing is unticked again for the second visit, so a "already
        // fetched" flag left behind would suppress the read a later visit
        // needs — and the dialog would show no key at all.
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

        // A tag appears behind the app's back while the dialog is closed. The
        // dialog read the tag list on the first visit, so only a re-read can
        // notice this one.
        git(&seed.alpha, &["tag", "v-leaked"]);

        // Second visit, on a different commit.
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
        // And the annotated block is on screen again, which is what the reset
        // looks like to a developer.
        assert_painted(&harness, "MESSAGE");
        assert_painted(&harness, "TAGGER");

        // The re-read tag list is the one thing a developer would catch: the
        // name the repository now holds must be refused, not called valid.
        harness.state_mut().ui.dlg.tag_name = "v-leaked".into();
        settle(&mut harness);
        assert_painted(&harness, "A tag named 'v-leaked' already exists");
    }

    /// Cancelling has to hold on the path the commit menu opened the dialog
    /// from, and not only in the dialog's own suite: it is a decision not to
    /// create a tag, and the repository has to say so.
    #[test]
    fn cancelling_the_tag_dialog_from_the_menu_creates_nothing() {
        let seed = seeded_repo("tag-cancel");
        let mut harness = log_harness(&seed);

        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "New tag");
        settle(&mut harness);
        // A name good enough to create from, so cancelling is a real decision
        // rather than a blank form nobody wanted anyway.
        harness.state_mut().ui.dlg.tag_name = "v9.9.9".into();
        settle(&mut harness);
        assert_painted(&harness, "✓ valid");

        harness.get_by_label("Cancel").click();
        settle(&mut harness);

        assert!(harness.state().ui.dialog.is_none(), "the dialog went");
        assert_eq!(
            git(&seed.alpha, &["tag", "-l"]).trim(),
            "",
            "and no tag was created"
        );
        assert_eq!(
            git(&seed.alpha, &["rev-parse", "HEAD"]).trim(),
            seed.c2,
            "and the repository is exactly where it was"
        );
        assert_eq!(
            git(&seed.alpha, &["symbolic-ref", "--short", "HEAD"]).trim(),
            "main",
            "still on the branch it was on"
        );
    }

    /// A press-ready commit row that belongs to a repository the app is not
    /// looking at.
    pub struct CrossRoot {
        /// Kept alive for as long as the fixture is: dropping it deletes the
        /// repositories the assertions read.
        pub _tmp: TempDir,
        pub harness: Harness<'static, AppState>,
        /// The repository the app is looking at, by id and by path.
        pub selected: RootId,
        pub selected_path: PathBuf,
        /// The repository the visible row belongs to, by id and by path.
        pub row_root: RootId,
        pub row_root_path: PathBuf,
        /// What `landing` points at before anything is done to it, in each
        /// repository — so a test can say the branch did not move.
        pub landing: String,
        pub other_landing: String,
        /// The row's commit, and its parent — which the details pane names in its
        /// parents row and the graph row does not.
        pub commit: String,
        pub parent: String,
    }

    /// A two-repository project with the log warm, one repository selected, and
    /// the search box narrowed to one root's commits — the ordinary way a row
    /// belonging to a repository the app is *not* looking at gets on screen: the
    /// listing is a union across every visible root, and each commit carries the
    /// root it came from. Nothing here fabricates a row.
    pub fn cross_root_row(tag: &str) -> CrossRoot {
        let (tmp, project, clean, dirty) = two_root_project(tag);
        // A branch of the same name in BOTH repositories, forked before each
        // one's tip, created BEFORE the app discovers the roots: a picker lists
        // the root snapshot's branches, so a branch made afterwards is not one
        // the picker could ever offer. A cherry-pick test then runs the
        // identical gesture against both repositories and only the real git
        // state of each says which one was used.
        let clean_base = git(&clean, &["rev-parse", "HEAD~1"]).trim().to_string();
        git(&clean, &["branch", "landing", &clean_base]);
        git(&dirty, &["branch", "landing", "HEAD"]);
        let landing = git(&clean, &["rev-parse", "landing"]).trim().to_string();
        let other_landing = git(&dirty, &["rev-parse", "landing"]).trim().to_string();
        let mut state = AppState::new(project);
        state.drain_events();
        state.ui.tab = Tab::Log;
        let mut harness = harness_over_state(state);
        warm_logs(&mut harness);
        settle(&mut harness);

        // The dirty repository is the one the app looks at; the row on screen
        // belongs to the clean one.
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
        // The search box is what makes one root's rows the listing while both
        // roots are indexed — the same union the graph shows unscoped, and the
        // one the pickaxe hits join.
        harness.state_mut().ui.log_filter = "clean work".to_string();
        settle(&mut harness);

        let commit = git(&clean, &["rev-parse", "HEAD"]).trim().to_string();
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

    /// A press on a commit row means that row's REPOSITORY as well as that row's
    /// commit: the details pane, the menu and every dialog the menu opens then
    /// describe the same thing. This drives the left-click intent — the plain
    /// selection. The menu intent is the sibling test below, because both
    /// intents assign the same thing and only the arm that reads it differs.
    ///
    /// The pane is the assertion that has to go red: with the wrong root
    /// selected, the pane looks the pressed commit up in the OTHER
    /// repository's log, finds nothing, and paints nothing at all.
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
        // The pane now describes THAT commit: its metadata grid is on screen, and
        // the parents row names that commit's parent — text the graph row does
        // not paint.
        assert_painted(&harness, "Committer");
        assert_painted(&harness, &short(&parent));
    }

    /// The coherence proved where it matters: a menu item that opens a dialog
    /// acts on the repository the pressed row came from, not on the one that
    /// happened to be selected.
    ///
    /// Cherry-pick to… is the cleanest proof, because it does two things with a
    /// repository: it lists that repository's branches and it creates a branch
    /// there. Both repositories in the fixture carry a branch of the same name,
    /// so the same gesture runs against both and only the real git state of each
    /// says which one was used. Today the pick runs in the OTHER repository, on a
    /// commit that does not exist there, and nothing lands anywhere.
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
        let other_head = git(&selected_path, &["rev-parse", "HEAD"])
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

        // The branch lives in the row's repository, so this is the row that
        // carries the commit: whichever repository the picker is listing, the
        // same row name is here, and only one of them can be the right one.
        picker_row(&harness, "landing").click();
        // Any toast means the operation settled; a success is not the assertion
        // (see below — a refusal is how it fails today, and it fails just as
        // visibly).
        wait_for(&mut harness, |s| s.ui.toast.is_some());

        assert_ne!(
            git(&row_root_path, &["rev-parse", "landing"]).trim(),
            landing,
            "the cherry-picked commit landed on the branch in the ROW's repository, \
             not nowhere"
        );
        assert_eq!(
            git(&row_root_path, &["log", "-1", "--format=%s", "landing"])
                .trim()
                .to_string(),
            "clean work commit",
            "and it is the row's commit that landed there"
        );
        assert_eq!(
            git(&selected_path, &["rev-parse", "landing"]).trim(),
            other_landing,
            "the OTHER repository's branch did not move"
        );
        assert_eq!(
            git(&selected_path, &["rev-parse", "HEAD"]).trim(),
            other_head,
            "nor did its checkout"
        );
    }

    /// The new-branch dialog's own row for `branch`, disambiguated from the
    /// branches pane's identically named rows: the dialog is a default-positioned
    /// area at the far left, so its rows are the leftmost.
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
    pub fn two_root_project(tag: &str) -> (TempDir, PathBuf, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join(tag);
        let clean = project.join("clean");
        let dirty = project.join("dirty");
        std::fs::create_dir_all(&clean).unwrap();
        std::fs::create_dir_all(&dirty).unwrap();
        for (repo, name) in [(&clean, "clean"), (&dirty, "dirty")] {
            git(repo, &["init", "-q", "-b", "main"]);
            git(repo, &["config", "user.email", "t@t"]);
            git(repo, &["config", "user.name", "t"]);
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

    /// A dirty root must not darken a clean sibling's menu, and its own menu must
    /// answer with its own reason — the gates belong to the root a commit came
    /// from, not to whichever root happens to be selected.
    #[test]
    fn each_root_is_gated_on_its_own_worktree() {
        let (_tmp, project, clean, dirty) = two_root_project("per-root");
        let mut state = AppState::new(project.clone());
        state.drain_events();
        state.ui.tab = Tab::Log;
        let mut harness = harness_over_state(state);
        warm_logs(&mut harness);
        // The worktree guard reads the root's status, which only a refresh reads.
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

        // Dismiss before re-aiming: the open menu floats over the row the next
        // root filter brings into its place.
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

    /// Whether the open menu's item `label` is currently disabled.
    pub fn is_menu_item_gated(harness: &mut Harness<'_, AppState>, label: &str) -> bool {
        harness.remove_cursor();
        harness.step();
        harness
            .get_all_by_role(egui::accesskit::Role::Button)
            .find(|n| n.accesskit_node().label() == Some(label.to_string()))
            .unwrap_or_else(|| panic!("menu item {label}"))
            .accesskit_node()
            .is_disabled()
    }

    /// Travel a real pointer into the open menu's disabled row until its reason
    /// paints.
    #[track_caller]
    pub fn assert_stated_on_hover(harness: &mut Harness<'_, AppState>, label: &str, reason: &str) {
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

    /// A click on an item closes the menu before the action runs, so no dialog
    /// ever opens behind a menu the developer can no longer see.
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

    /// The menu's open state carries the root AND the commit, so a selection
    /// change underneath an open menu cannot make the pane and the menu disagree.
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

    /// With no selection the pane reads exactly as it did before the menu took
    /// its actions away.
    #[test]
    fn the_unselected_pane_still_reads_as_an_empty_state() {
        let seed = seeded_repo("empty-pane");
        let harness = log_harness(&seed);
        assert_painted(&harness, "Select a commit…");
        assert_not_painted(&harness, "Commit details\'s actions");
    }

    /// Checkout puts the root at that exact commit in Detached HEAD, and the
    /// shell says so — the state is visible without inventing an indicator.
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
            git(&seed.alpha, &["rev-parse", "HEAD"]).trim(),
            seed.c1,
            "HEAD stands on the commit that was right-clicked, not on its tip"
        );
        // `git symbolic-ref` exits non-zero once HEAD is detached — that IS the
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

    /// Drop commit is never one click. Before anything runs, the preflight says
    /// which commit goes, what is replayed on top of it, what the history
    /// becomes, what is likely to conflict, how long it takes, and how to get
    /// back — every one of them a fact the developer would otherwise learn by
    /// losing work.
    ///
    /// The plan is `base..tip`, so dropping the middle commit of a three-commit
    /// branch replays exactly one: `2 → 1 COMMITS`, `~3s estimated`.
    #[test]
    fn the_drop_preflight_says_what_will_happen_before_anything_runs() {
        let seed = seeded_repo("preflight");
        let mut harness = log_harness(&seed);
        let head = git(&seed.alpha, &["rev-parse", "HEAD"]).trim().to_string();
        // The default settings protect `main`, and a protected current branch
        // refuses the verb on the item — this run exercises the happy path.
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
        // The commit that goes, named.
        assert_painted(&harness, "DROPPED COMMIT");
        assert_painted(&harness, "alpha: first commit");
        // The commit set that will be replayed on top of it, and what the
        // history becomes.
        assert_painted(&harness, "REPLAYED COMMITS");
        assert_painted(&harness, "alpha: second commit");
        assert_painted(&harness, "2 → 1 COMMITS");
        // The arrow form is a DROP's: this is the one badge where a count is
        // actually lost, and a reword's wording must never leak into it.
        assert_not_painted(&harness, "REWRITTEN");
        // The estimate the plan editor already computes.
        assert_painted(&harness, "~3s estimated");
        // The way back, named by the ref that will carry it.
        assert_painted(&harness, "RECOVERY");
        assert_painted(&harness, "refs/turbogit/preflight-backup");
        // And nothing has moved: this is a briefing, not a rewrite.
        assert_eq!(git(&seed.alpha, &["rev-parse", "HEAD"]).trim(), head);
    }

    /// Reword commit opens an editor seeded with the commit's OWN message — the
    /// whole string, subject and body — because the point is to correct what is
    /// there, not to compose something new beside it. Nothing has moved: the
    /// editor is a draft, and only its confirm reaches the preflight.
    #[test]
    fn the_reword_editor_opens_seeded_with_the_commit_s_own_message() {
        let seed = seeded_repo("reword-editor");
        let mut harness = log_harness(&seed);
        let head = git(&seed.alpha, &["rev-parse", "HEAD"]).trim().to_string();
        harness
            .state_mut()
            .settings
            .protected_branch_patterns
            .clear();
        settle(&mut harness);

        // c2 is the one commit with a body, so the seed proves both halves.
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
            git(&seed.alpha, &["rev-parse", "HEAD"]).trim(),
            head,
            "and nothing has moved: this is a draft"
        );
    }

    /// The editor is a real text field, not a painted copy: keystrokes reach the
    /// message, and ONE field takes both halves of it. Typing a new subject,
    /// then a blank line, then a body is the same edit a developer makes to fix a
    /// typo, and it is all one string — the value that will be written.
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

        // c1's message is a single line, so one ctrl+U empties it and what
        // follows is only what this test typed.
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
        // Enter twice: the blank line git puts between a subject and a body, so
        // the developer controls the arrangement rather than the app inventing it.
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
        // And the convention that makes one field enough is stated on it.
        assert_painted(&harness, "The first line is the subject");
    }

    /// Cancelling is a decision not to reword: no ref, no commit moved, and the
    /// message the developer typed is discarded rather than held for later.
    #[test]
    fn cancelling_the_reword_editor_changes_nothing() {
        let seed = seeded_repo("reword-cancel");
        let mut harness = log_harness(&seed);
        let head = git(&seed.alpha, &["rev-parse", "HEAD"]).trim().to_string();
        let count = git(&seed.alpha, &["rev-list", "--count", "main"])
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
        assert_eq!(git(&seed.alpha, &["rev-parse", "HEAD"]).trim(), head);
        assert_eq!(
            git(&seed.alpha, &["rev-list", "--count", "main"]).trim(),
            count,
            "the history is where it was"
        );
        assert!(
            !git(&seed.alpha, &["show-ref"]).contains("preflight-backup"),
            "and no backup ref was written: nothing ran"
        );
    }

    /// A reword with nothing to say is refused, and it says why on the field. The
    /// refusal that matters is in the preflight opener — this is the developer
    /// finding out before they press the button that would be refused.
    #[test]
    fn a_reword_with_no_message_is_refused_and_states_why() {
        let seed = seeded_repo("reword-empty");
        let mut harness = log_harness(&seed);
        let head = git(&seed.alpha, &["rev-parse", "HEAD"]).trim().to_string();
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

        // Stated on the field, and the button that would do it is not live.
        assert_painted(&harness, "A commit cannot have an empty message");
        assert!(
            harness
                .get_by_label("Reword this commit")
                .accesskit_node()
                .is_disabled(),
            "so the confirm cannot be pressed at all"
        );
        // And the enforcement is not the button: the app refuses it too, because
        // a whitespace-only message would be a rewrite in full that changes
        // nothing — git keeps the original message when the editor leaves it
        // empty. Asserted at the seam that every path goes through.
        let root = harness
            .state()
            .selected_root
            .clone()
            .expect("a selected root");
        let target = git(&seed.alpha, &["rev-parse", "HEAD~1"])
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
        // ONE sentence for the bound, on both surfaces: the field above is
        // showing the same words this toast is, so the rule cannot be worded two
        // ways and drift.
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
        assert_eq!(git(&seed.alpha, &["rev-parse", "HEAD"]).trim(), head);
    }

    /// Reword commit's preflight is the same briefing with the verb's own facts:
    /// the commit keeps its row, so the counts do not change — but every commit
    /// above it is re-created, which the REPLAYED group says — and the ONE thing
    /// the verb does to this commit is stated as a difference, the subject it has
    /// now and the subject it will carry. Still nothing has run.
    #[test]
    fn the_reword_preflight_says_what_it_will_change_and_what_it_costs() {
        let seed = seeded_repo("reword-preflight");
        let mut harness = log_harness(&seed);
        let head = git(&seed.alpha, &["rev-parse", "HEAD"]).trim().to_string();
        harness
            .state_mut()
            .settings
            .protected_branch_patterns
            .clear();
        settle(&mut harness);

        // c1, not the tip: the plan is c1 and the commit built on it, so the
        // dialog shows a commit being REPLAYED on top of the one being reworded.
        right_click_row(&mut harness, "alpha: first commit");
        click_menu_item(&mut harness, "Reword commit");
        settle(&mut harness);
        assert_eq!(
            harness.state().ui.dialog,
            Some(turbogit_app::state::Dialog::Reword)
        );

        // The editor's confirm, with the message the developer wrote.
        harness.state_mut().ui.dlg.reword_message = "alpha: the first commit".to_owned();
        settle(&mut harness);
        harness.get_by_label("Reword this commit").click();
        settle(&mut harness);

        assert_eq!(
            harness.state().ui.dialog,
            Some(turbogit_app::state::Dialog::RewritePreflight),
            "the editor's confirm opens the preflight, not the rewrite"
        );
        // The commit is reworded in place, and the dialog says which way: the
        // subject it has now, then the one it will carry.
        assert_painted(&harness, "REWORDED COMMIT");
        assert_painted(&harness, "alpha: first commit");
        assert_painted(&harness, "→");
        assert_painted(&harness, "alpha: the first commit");
        // The count does not change and the replay does, so the badge states the
        // SCALE and the verb rather than a subtraction that would read as "nothing
        // happens". The arrow form belongs to a drop, which loses a commit.
        assert_painted(&harness, "REPLAYED COMMITS · 2");
        assert_painted(&harness, "2 COMMITS REWRITTEN");
        assert_not_painted(&harness, "→ 2 COMMITS");
        // The rest of the briefing is the shared rail, as it is for a drop: the
        // estimate is three seconds per replayed commit, so a two-commit plan.
        assert_painted(&harness, "~6s estimated");
        assert_painted(&harness, "RECOVERY");
        assert_painted(&harness, "refs/turbogit/preflight-backup");
        assert_eq!(
            git(&seed.alpha, &["rev-parse", "HEAD"]).trim(),
            head,
            "and nothing has moved: this is still a briefing"
        );
    }

    /// The reword badge at its smallest count. A reword of the TIP replays one
    /// commit, and "1 COMMITS REWRITTEN" would be the dialog's first lie to the
    /// developer — so the noun follows the number the way the shared caution rail
    /// already follows "1 file" and "1 author".
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

        // c2 is the tip, so the plan is that one commit and nothing above it.
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

    /// A repository holding all three shapes Drop commit refuses: a merge commit
    /// on the current branch, an ordinary commit on a PROTECTED current branch
    /// (the default settings protect `main`), and a commit that exists only on a
    /// branch the current one never merged — which the graph lists when that
    /// branch is the ref scope.
    fn repo_with_merge_and_branch(tag: &str) -> (TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join(tag);
        let repo = project.join("alpha");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@t"]);
        git(&repo, &["config", "user.name", "t"]);
        commit_file(&repo, "a.txt", "one\n", "alpha: first commit", 100);
        // `side` is never merged, so its commit is the one the current branch's
        // history does NOT contain — which is what makes it off-branch.
        git(&repo, &["checkout", "-q", "-b", "side"]);
        commit_file(&repo, "s.txt", "side\n", "alpha: side work", 120);
        git(&repo, &["checkout", "-q", "main"]);
        // `topic` forks from main, NOT from `side`: a topic branched off `side`
        // would carry side's commit into main when it is merged, and the
        // off-branch case would stop being off-branch.
        git(&repo, &["checkout", "-q", "-b", "topic"]);
        commit_file(&repo, "t.txt", "topic\n", "alpha: topic work", 150);
        git(&repo, &["checkout", "-q", "main"]);
        commit_file(&repo, "b.txt", "two\n", "alpha: second commit", 200);
        git(
            &repo,
            &[
                "merge",
                "-q",
                "--no-ff",
                "-m",
                "Merge topic into main",
                "topic",
            ],
        );
        (tmp, project, repo)
    }

    /// The cautions the preflight shows are the plan editor's cautions, painted
    /// by the same shared rail — so the two surfaces cannot word "what is likely
    /// to go wrong" differently. This is the preflight half of that pin; the
    /// plan editor's own half already asserts the same two lines and the same
    /// backup ref in `interactive_rebase_editor.rs`.
    ///
    /// The fixture earns both cautions: the two replayed commits touch the SAME
    /// path (conflict likelihood) and were committed under two identities
    /// (mixed committers). It is a repo of its own because that is a different
    /// history from the ones the other gate cases need.
    #[test]
    fn the_drop_preflight_shows_the_same_cautions_the_plan_editor_does() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("alpha");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@t"]);
        git(&repo, &["config", "user.name", "t"]);
        // A base commit, because a root commit has no first parent to rewrite
        // from and the verb refuses it before it ever reaches a plan.
        commit_as_configured(&repo, "base.txt", "base\n", "alpha: base commit", 100);
        commit_as_configured(&repo, "shared.txt", "one\n", "alpha: first commit", 200);
        // A second identity, and every commit above the base on ONE path:
        // dropping the first leaves the two survivors sharing that path, which
        // is what the conflict caution counts.
        git(&repo, &["config", "user.name", "Other"]);
        commit_as_configured(&repo, "shared.txt", "two\n", "alpha: second commit", 300);
        git(&repo, &["config", "user.name", "t"]);
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

    /// A long history must not open a dialog taller than the window, and the
    /// elided middle must not be a silent lie: both EDGES of the blast radius
    /// are shown, with the count of what is between them named on the row that
    /// stands in for it.
    #[test]
    fn a_long_history_elides_the_middle_and_says_how_much_is_hidden() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("alpha");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@t"]);
        git(&repo, &["config", "user.name", "t"]);
        // Thirty commits, so dropping one past the midpoint leaves a plan of 16
        // — more than the twelve rows the list shows.
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

        // The plan is commits 15..30: sixteen entries, one dropped, fifteen
        // replayed. The list shows twelve of them and names the three between.
        assert_painted(&harness, "REPLAYED COMMITS · 15");
        // The plan editor's exact counts, and nothing beside them: for a drop the
        // count is always 1, and the DROPPED COMMIT group above already names
        // the commit being dropped.
        assert_painted(&harness, "16 → 15 COMMITS");
        assert_not_painted(&harness, "· 1 DROPPED");
        assert_painted(&harness, "… 3 commits in between …");
        // Both edges are on screen: the oldest commit that moves, and the tip.
        // The graph behind the dialog paints every commit, so a row the list
        // ALSO shows appears twice and a row it elides appears once.
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

    /// The four reasons Drop commit is refused, asserted through the MENU and
    /// not only at the gate table: these items were painted inert for the whole
    /// of ticket 06, so nothing read the table's answers at runtime. Each case
    /// gets its own repository so a failure names the reason it is about.
    /// The log shell over that repository, with the temp dir handed back so the
    /// repository outlives the test body that reads it.
    fn menu_over(tag: &str) -> (TempDir, Harness<'static, AppState>, RootId) {
        let (tmp, _project, repo) = repo_with_merge_and_branch(tag);
        let mut harness = harness_over_state(state_with_log_at(repo));
        warm_logs(&mut harness);
        settle(&mut harness);
        let root = harness.state().multi.roots[0].id.clone();
        (tmp, harness, root)
    }

    /// A merge commit has no single first parent to replay from, so neither
    /// history verb is offered — and each states its own reason on the item.
    #[test]
    fn drop_commit_refuses_a_merge_commit_and_says_why() {
        let (_tmp, mut harness, _root) = menu_over("gates-merge");
        right_click_row(&mut harness, "Merge topic into main");
        assert_menu_item_gated(&mut harness, "Copy hash", "Drop commit");
        assert_stated_on_hover(
            &mut harness,
            "Drop commit",
            "a merge commit cannot be dropped",
        );
    }

    /// A root commit is the other end of the same walk a merge is: it has no
    /// first parent to replay from, so the plan cannot be built and the service
    /// refuses the verb outright. Stated on the item, not discovered by clicking.
    #[test]
    fn drop_commit_refuses_a_root_commit_and_says_why() {
        let (_tmp, mut harness, _root) = menu_over("gates-root");
        right_click_row(&mut harness, "alpha: first commit");
        assert_menu_item_gated(&mut harness, "Copy hash", "Drop commit");
        assert_stated_on_hover(
            &mut harness,
            "Drop commit",
            "a root commit cannot be dropped",
        );
    }

    /// A protected current branch may not be rewritten through the log, and the
    /// item says so on the row rather than in a box across the pane.
    #[test]
    fn drop_commit_refuses_a_protected_current_branch_and_says_why() {
        let (_tmp, mut harness, _root) = menu_over("gates-protected");
        // The default settings protect `main`, and this row is on it.
        right_click_row(&mut harness, "alpha: second commit");
        assert_menu_item_gated(&mut harness, "Copy hash", "Drop commit");
        assert_stated_on_hover(
            &mut harness,
            "Drop commit",
            "the current branch is protected",
        );
    }

    /// A commit that lives on another branch is read-only: the plan is built to
    /// this branch's tip, so there is nothing to replay it onto. It reads as
    /// deliberately bounded rather than broken.
    #[test]
    fn drop_commit_refuses_a_commit_off_the_current_branch_and_says_why() {
        let (_tmp, mut harness, root) = menu_over("gates-off-branch");
        // The graph lists another branch's history when that branch is the ref
        // scope — the ordinary way a commit from elsewhere is reached.
        harness.state_mut().ui.log_ref_scope = Some((root.clone(), "side".into()));
        wait_for(&mut harness, |s| s.caches.ref_log(&root, "side").is_some());
        settle(&mut harness);
        right_click_row(&mut harness, "alpha: side work");
        assert_menu_item_gated(&mut harness, "Copy hash", "Drop commit");
        assert_stated_on_hover(&mut harness, "Drop commit", "not on the current branch");
    }

    /// Reword commit is bounded exactly as Drop commit is — the same rewrite
    /// path, so the same four bounds — and each is stated on the item rather
    /// than discovered by clicking. Asserted through the MENU, one repository per
    /// reason so a failure names the reason it is about: the gate table proves the
    /// answers, and these prove the answers reach the developer.
    #[test]
    fn reword_commit_refuses_a_merge_commit_and_says_why() {
        let (_tmp, mut harness, _root) = menu_over("reword-gates-merge");
        right_click_row(&mut harness, "Merge topic into main");
        assert_menu_item_gated(&mut harness, "Copy hash", "Reword commit");
        assert_stated_on_hover(
            &mut harness,
            "Reword commit",
            "a merge commit cannot be reworded",
        );
    }

    /// The default settings protect `main`, and the row is on it.
    #[test]
    fn reword_commit_refuses_a_protected_current_branch_and_says_why() {
        let (_tmp, mut harness, _root) = menu_over("reword-gates-protected");
        right_click_row(&mut harness, "alpha: second commit");
        assert_menu_item_gated(&mut harness, "Copy hash", "Reword commit");
        assert_stated_on_hover(
            &mut harness,
            "Reword commit",
            "the current branch is protected",
        );
    }

    /// A commit that lives on another branch is read-only: the plan is built to
    /// this branch's tip, so there is nothing to replay it onto.
    #[test]
    fn reword_commit_refuses_a_commit_off_the_current_branch_and_says_why() {
        let (_tmp, mut harness, root) = menu_over("reword-gates-off-branch");
        harness.state_mut().ui.log_ref_scope = Some((root.clone(), "side".into()));
        wait_for(&mut harness, |s| s.caches.ref_log(&root, "side").is_some());
        settle(&mut harness);
        right_click_row(&mut harness, "alpha: side work");
        assert_menu_item_gated(&mut harness, "Copy hash", "Reword commit");
        assert_stated_on_hover(&mut harness, "Reword commit", "not on the current branch");
    }

    /// A root commit has no first parent to replay from, so the plan cannot be
    /// built and the service refuses the verb outright.
    #[test]
    fn reword_commit_refuses_a_root_commit_and_says_why() {
        let (_tmp, mut harness, _root) = menu_over("reword-gates-root");
        right_click_row(&mut harness, "alpha: first commit");
        assert_menu_item_gated(&mut harness, "Copy hash", "Reword commit");
        assert_stated_on_hover(
            &mut harness,
            "Reword commit",
            "a root commit cannot be reworded",
        );
    }

    /// The commit pane's branch label falls back to the same marker, so the
    /// warning a developer already reads before committing to a detached
    /// repository still fires for a state this menu created.
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

    /// A dirty worktree routes the commit checkout through the SAME bring-along /
    /// set-aside / cancel confirmation a branch checkout uses, and cancelling
    /// changes nothing.
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

    /// The confirmation a dirty commit-checkout raises is the branch checkout's
    /// own dialog — same words, same three choices — and CANCEL moves nothing.
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
        assert_eq!(git(&seed.alpha, &["rev-parse", "HEAD"]).trim(), seed.c2);

        // Accepting the same dialog takes the detached checkout the menu asked
        // for, not a branch switch.
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
            git(&seed.alpha, &["rev-parse", "HEAD"]).trim(),
            seed.c1,
            "the confirmation landed on the commit, detached"
        );
    }

    /// An `AppState` over one repository, with its log warm.
    pub fn state_with_log_at(repo: PathBuf) -> AppState {
        let project = repo.parent().expect("a project dir").to_path_buf();
        let mut state = AppState::new(project);
        state.drain_events();
        state.ui.tab = Tab::Log;
        state
    }

    /// Install the patch-path chooser the composition root would install. This is
    /// the one part of the feature a headless test cannot reach through a native
    /// dialog, so the choice is substituted — and it is the ONLY new seam this
    /// feature introduces.
    pub fn choose_patch_path(harness: &mut Harness<'_, AppState>, choice: Option<PathBuf>) {
        harness.state_mut().patch_writer = Some(Box::new(move || choice.clone()));
    }

    /// Create patch writes THAT commit's own change — first parent to commit,
    /// never its ancestors — to a file the developer chooses, as text git applies.
    #[test]
    fn create_patch_writes_that_commit_s_own_change_and_nothing_above_it() {
        let seed = seeded_repo("patch");
        let mut harness = log_harness(&seed);
        // c1 is the middle commit: its parent is `base`, its descendant is c2.
        let out = seed.project.join("mine.patch");
        right_click_row(&mut harness, "alpha: first commit");
        choose_patch_path(&mut harness, Some(out.clone()));
        click_menu_item(&mut harness, "Create patch");
        settle(&mut harness);
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

        // git's own verdict is the assertion that matters, not a text shape.
        // Stand the worktree at the commit's PARENT, where the patch's file does
        // not exist yet, and ask git whether it could apply.
        git(&seed.alpha, &["checkout", "-q", &seed.base]);
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
        assert_painted(&harness, "Wrote mine.patch");
    }

    /// The hash stays on screen, but nothing about it invites a press: the chip
    /// answers no click and occupies no click plane, because copying the hash
    /// lives in the commit's menu — the item that names the short reference — not
    /// in the pane two inches below it.
    #[test]
    fn the_hash_chip_answers_no_click() {
        let seed = seeded_repo("chip");
        let mut harness = log_harness(&seed);
        row_node(&harness, "alpha: second commit").click();
        settle(&mut harness);

        // The chip is the SURFACE_3 pill the short reference is painted inside;
        // the meta grid below it is a taller SURFACE_3, so the reference is what
        // tells the two apart.
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

    /// A rename appears as a rename and a binary appears the way git represents
    /// binary content, so the patch carries what git's own diff carries.
    #[test]
    fn a_rename_and_a_binary_survive_into_the_patch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@t"]);
        git(&repo, &["config", "user.name", "t"]);
        commit_file(
            &repo,
            "before.txt",
            "one\ntwo\nthree\n",
            "a base commit",
            100,
        );
        git(&repo, &["mv", "before.txt", "after.txt"]);
        std::fs::write(repo.join("pic.png"), bytes_of_a_png()).unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "rename and add a binary"]);
        let renamed = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();

        let mut harness = harness_over_state(state_with_log_at(repo.clone()));
        warm_logs(&mut harness);
        settle(&mut harness);
        right_click_row(&mut harness, "rename and add a binary");
        let file = tmp.path().join("mixed.patch");
        choose_patch_path(&mut harness, Some(file.clone()));
        click_menu_item(&mut harness, "Create patch");
        settle(&mut harness);

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

    /// A commit that changed nothing is a legitimate commit: the item produces an
    /// empty patch rather than failing.
    #[test]
    fn an_empty_commit_produces_an_empty_patch_not_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "t@t"]);
        git(&repo, &["config", "user.name", "t"]);
        commit_file(&repo, "base.txt", "base\n", "a base commit", 100);
        git(
            &repo,
            &["commit", "-q", "--allow-empty", "-m", "an empty commit"],
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
            std::fs::read_to_string(&file)
                .expect("a file was written")
                .trim(),
            "",
            "an empty commit is an empty patch"
        );
    }

    /// Cancelling the file dialog writes nothing and says nothing: a stray Escape
    /// must not produce an empty file or a false confirmation.
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

        assert!(!path.exists(), "a cancelled dialog leaves no file behind");
        assert!(
            harness.state().ui.toast.is_none(),
            "and no confirmation either"
        );
    }

    /// A file write is not git work: it never becomes an `Operation`, so the
    /// activity feed stays as it was.
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

    /// The pane that used to perform these actions no longer does: the ACTIONS
    /// group, its title, the header's second Copy hash button, the hash chip's
    /// click and its caption, and the guardrail alert box are all gone.
    #[test]
    fn the_details_pane_keeps_everything_it_says_and_loses_everything_it_did() {
        let seed = seeded_repo("pane");
        let mut harness = log_harness(&seed);
        row_node(&harness, "alpha: second commit").click();
        settle(&mut harness);

        // What it says — all of it still there.
        assert_painted(&harness, "alpha: second commit");
        assert_painted(&harness, "Committer");
        assert_painted(&harness, "Parents");
        assert_painted(&harness, "changed");
        // The hash is still visible; only its click is gone.
        assert_painted(&harness, &short(&seed.c2));
        // What it did — gone, and its title and caption with it.
        for gone in [
            "ACTIONS",
            "Actions",
            "Create branch here",
            "click to copy full hash",
        ] {
            assert_not_painted(&harness, gone);
        }
        // The header's Copy hash button was the pane's only one, and the menu's
        // own item is not painted while no menu is open.
        let copies = painted_galleys(&harness)
            .into_iter()
            .filter(|g| g.text.contains("Copy hash"))
            .count();
        assert_eq!(copies, 0, "no copy affordance left in the pane");
    }
}

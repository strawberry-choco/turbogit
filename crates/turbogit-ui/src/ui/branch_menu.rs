//! The branch context menu: one right-click action list for a branch row.
//!
//! Ten items for one `(RootId, Branch)` target — the union of every branch
//! action the surface offers — rendered through the shared
//! [`crate::ui::widgets::menu_item`] primitive. Props in, action out — the
//! same contract [`crate::ui::branch_tree_view`] holds: the component decides
//! no policy, dispatches nothing, and never touches `AppState`. The ten
//! gating rules live here once, in [`branch_menu_items`], so the wording and
//! enablement of a blocked action can never drift between surfaces.

use turbogit_domain::model::{Branch, BranchKind};

use crate::ui::icons::Icon;
use crate::ui::widgets::{MenuItemKind, MenuItemProps, menu_item, menu_rule};

/// What the user picked. Plain data — the surface owns what it means.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BranchMenuAction {
    Checkout,
    NewBranchFrom,
    CheckoutAndPull,
    Merge,
    Rebase,
    Compare,
    Pull,
    Push,
    Rename,
    Delete,
}

impl BranchMenuAction {
    /// The designed item order, top of the menu to bottom.
    pub const ORDER: [Self; 10] = [
        Self::Checkout,
        Self::NewBranchFrom,
        Self::CheckoutAndPull,
        Self::Merge,
        Self::Rebase,
        Self::Compare,
        Self::Pull,
        Self::Push,
        Self::Rename,
        Self::Delete,
    ];

    /// The item's slot in the array [`branch_menu_items`] returns.
    pub const fn index(self) -> usize {
        match self {
            Self::Checkout => 0,
            Self::NewBranchFrom => 1,
            Self::CheckoutAndPull => 2,
            Self::Merge => 3,
            Self::Rebase => 4,
            Self::Compare => 5,
            Self::Pull => 6,
            Self::Push => 7,
            Self::Rename => 8,
            Self::Delete => 9,
        }
    }
}

/// One item's gate: whether it acts, and what it says when it cannot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuItemState {
    pub enabled: bool,
    pub reason: Option<&'static str>,
}

impl MenuItemState {
    pub const fn enabled() -> Self {
        Self {
            enabled: true,
            reason: None,
        }
    }

    pub const fn disabled(reason: &'static str) -> Self {
        Self {
            enabled: false,
            reason: Some(reason),
        }
    }
}

/// The ten gates, in [`BranchMenuAction::ORDER`] — one function, one set of
/// strings. Every rule is a field read off the branch the listing already
/// carries; none costs a git call.
pub fn branch_menu_items(branch: &Branch, current: Option<&str>) -> [MenuItemState; 10] {
    let is_current = current == Some(branch.name.as_str());
    let is_local = branch.kind == BranchKind::Local;
    let has_upstream = branch.tracking.is_some() && !branch.gone;
    let mut items = [MenuItemState::enabled(); 10];
    items[BranchMenuAction::Checkout.index()] = if is_current {
        MenuItemState::disabled("already checked out")
    } else {
        MenuItemState::enabled()
    };
    items[BranchMenuAction::CheckoutAndPull.index()] = if is_current {
        MenuItemState::disabled("already checked out")
    } else if !has_upstream {
        MenuItemState::disabled("no upstream to pull from")
    } else {
        MenuItemState::enabled()
    };
    // The history verbs move this branch's commits into the checked-out one,
    // so they need a branch that is neither the target nor remote-only.
    items[BranchMenuAction::Merge.index()] = if is_current {
        MenuItemState::disabled("this is the current branch")
    } else if !is_local {
        MenuItemState::disabled("remote branches cannot be merged")
    } else {
        MenuItemState::enabled()
    };
    items[BranchMenuAction::Rebase.index()] = if is_current {
        MenuItemState::disabled("this is the current branch")
    } else if !is_local {
        MenuItemState::disabled("remote branches cannot be rebased")
    } else {
        MenuItemState::enabled()
    };
    // Comparing is read-only, so a remote-tracking row can be one side of it.
    items[BranchMenuAction::Compare.index()] = if is_current {
        MenuItemState::disabled("this is the current branch")
    } else {
        MenuItemState::enabled()
    };
    items[BranchMenuAction::Pull.index()] = if !is_current {
        MenuItemState::disabled("check out this branch first")
    } else if !has_upstream {
        MenuItemState::disabled("no upstream to pull from")
    } else {
        MenuItemState::enabled()
    };
    items[BranchMenuAction::Push.index()] = if branch.ahead > 0 || branch.tracking.is_none() {
        MenuItemState::enabled()
    } else if branch.gone {
        MenuItemState::disabled("upstream is gone")
    } else {
        MenuItemState::disabled("nothing to push")
    };
    items[BranchMenuAction::Rename.index()] = if is_local {
        MenuItemState::enabled()
    } else {
        MenuItemState::disabled("remote branches cannot be renamed")
    };
    // A remote-tracking row is still deletable — that is what a delete means
    // upstream. Only the branch the work is standing on is not.
    items[BranchMenuAction::Delete.index()] = if is_current {
        MenuItemState::disabled("the current branch cannot be deleted")
    } else {
        MenuItemState::enabled()
    };
    items
}

/// Everything the menu is told, each frame. Plain data: the owning
/// repository's name for the multi-repo wording, whether more than one
/// repository is in scope, and which branch is checked out. The last is both
/// the gate's input — a row whose name matches it *is* the current branch —
/// and the destination the history verbs name, so the two can never disagree.
pub struct BranchMenuProps<'a> {
    pub repo_name: &'a str,
    pub multi_repo: bool,
    pub current_branch: Option<&'a str>,
}

/// Paint the ten items for one branch and report which was clicked.
///
/// The menu is 296 px wide and grows to 320 px before the branch name
/// middle-truncates — no wrapping inside a menu row. "Checkout and pull"
/// carries `DOWNLOAD`: the embedded set has no checkout-and-pull composite
/// yet, and the design's stated fallback beats an invented inline path.
pub fn branch_menu(
    ui: &mut egui::Ui,
    props: &BranchMenuProps<'_>,
    target: &Branch,
) -> Option<BranchMenuAction> {
    let states = branch_menu_items(target, props.current_branch);
    // Menu rows sit flush; the rules state their own air.
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.set_min_width(296.0);
    ui.set_max_width(320.0);

    let checkout_label = if props.multi_repo {
        format!("Checkout in {}", props.repo_name)
    } else {
        "Checkout".to_owned()
    };
    // With no named branch checked out (detached HEAD) the verb falls back to
    // its bare stem rather than painting a dangling preposition.
    let directed = |verb: &str, stem: &str| match props.current_branch {
        Some(current) => format!("{verb} {current}"),
        None => stem.to_owned(),
    };
    let merge_label = directed("Merge into", "Merge");
    let rebase_label = directed("Rebase onto", "Rebase");
    let compare_label = directed("Compare with", "Compare");

    let mut clicked = None;
    let mut row = |ui: &mut egui::Ui,
                   action: BranchMenuAction,
                   state: &MenuItemState,
                   icon: Icon,
                   label: &str,
                   data: Option<&str>,
                   shortcut: Option<&str>,
                   kind: MenuItemKind| {
        if menu_item(
            ui,
            MenuItemProps {
                icon,
                label,
                data,
                shortcut,
                enabled: state.enabled,
                disabled_reason: state.reason,
                kind,
            },
        )
        .clicked()
        {
            clicked = Some(action);
        }
    };

    row(
        ui,
        BranchMenuAction::Checkout,
        &states[BranchMenuAction::Checkout.index()],
        Icon::CHECK,
        &checkout_label,
        None,
        None,
        MenuItemKind::Primary,
    );
    row(
        ui,
        BranchMenuAction::NewBranchFrom,
        &states[BranchMenuAction::NewBranchFrom.index()],
        Icon::GIT_BRANCH,
        "New branch from",
        Some(&target.name),
        None,
        MenuItemKind::Default,
    );
    row(
        ui,
        BranchMenuAction::CheckoutAndPull,
        &states[BranchMenuAction::CheckoutAndPull.index()],
        Icon::DOWNLOAD,
        "Checkout and pull",
        None,
        None,
        MenuItemKind::Default,
    );
    menu_rule(ui);
    row(
        ui,
        BranchMenuAction::Merge,
        &states[BranchMenuAction::Merge.index()],
        Icon::GIT_MERGE,
        &merge_label,
        None,
        None,
        MenuItemKind::Default,
    );
    row(
        ui,
        BranchMenuAction::Rebase,
        &states[BranchMenuAction::Rebase.index()],
        // The embedded Lucide set has no rebase glyph; `LAYERS` reads as the
        // commit stack being re-laid, and an approximate glyph from the set
        // beats inventing path data outside it.
        Icon::LAYERS,
        &rebase_label,
        None,
        None,
        MenuItemKind::Default,
    );
    row(
        ui,
        BranchMenuAction::Compare,
        &states[BranchMenuAction::Compare.index()],
        Icon::GIT_COMPARE,
        &compare_label,
        None,
        None,
        MenuItemKind::Default,
    );
    menu_rule(ui);
    row(
        ui,
        BranchMenuAction::Pull,
        &states[BranchMenuAction::Pull.index()],
        Icon::DOWNLOAD,
        "Pull",
        None,
        None,
        MenuItemKind::Default,
    );
    row(
        ui,
        BranchMenuAction::Push,
        &states[BranchMenuAction::Push.index()],
        Icon::UPLOAD,
        "Push",
        None,
        Some("Ctrl+Shift+K"),
        MenuItemKind::Default,
    );
    menu_rule(ui);
    row(
        ui,
        BranchMenuAction::Rename,
        &states[BranchMenuAction::Rename.index()],
        Icon::PENCIL,
        "Rename branch",
        None,
        None,
        MenuItemKind::Default,
    );
    row(
        ui,
        BranchMenuAction::Delete,
        &states[BranchMenuAction::Delete.index()],
        Icon::TRASH_2,
        "Delete branch",
        None,
        None,
        MenuItemKind::Danger,
    );
    clicked
}

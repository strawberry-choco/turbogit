use turbogit_domain::model::{Branch, BranchKind};

use crate::ui::icons::Icon;
use crate::ui::widgets::{MenuItemKind, MenuItemProps, menu_item, menu_rule};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BranchMenuAction {
    CopyBranchName,
    Checkout,
    NewBranchFrom,
    CheckoutAndPull,
    Merge,
    Rebase,
    Compare,
    DiffWithWorkingTree,
    Pull,
    Push,
    Rename,
    Delete,
}

impl BranchMenuAction {
    /// The designed item order, top of the menu to bottom.
    ///
    /// The copy verb leads, in a group of its own, exactly as it does in
    /// [`crate::ui::commit_menu`]: two menus opened in one window read as one
    /// kit, and the inert verb goes where the inert verb goes. The two
    /// read-only comparisons sit together in the middle group: neither
    /// moves a ref, and the item that opens the file-level list belongs beside the
    /// one that opens the commit-level list rather than down with the pull family.
    pub const ORDER: [Self; 12] = [
        Self::CopyBranchName,
        Self::Checkout,
        Self::NewBranchFrom,
        Self::CheckoutAndPull,
        Self::Merge,
        Self::Rebase,
        Self::Compare,
        Self::DiffWithWorkingTree,
        Self::Pull,
        Self::Push,
        Self::Rename,
        Self::Delete,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::CopyBranchName => 0,
            Self::Checkout => 1,
            Self::NewBranchFrom => 2,
            Self::CheckoutAndPull => 3,
            Self::Merge => 4,
            Self::Rebase => 5,
            Self::Compare => 6,
            Self::DiffWithWorkingTree => 7,
            Self::Pull => 8,
            Self::Push => 9,
            Self::Rename => 10,
            Self::Delete => 11,
        }
    }
}

pub use crate::ui::widgets::MenuItemState;

/// The twelve items' gates, in [`BranchMenuAction::ORDER`], one function, one
/// set of strings. Every rule is a field read off the branch the listing
/// already carries; none costs a git call. [`BranchMenuAction::CopyBranchName`]
/// takes the array's default because the name it copies is the one it was
/// handed: a row that can be pointed at can be copied.
pub fn branch_menu_items(branch: &Branch, current: Option<&str>) -> [MenuItemState; 12] {
    let is_current = current == Some(branch.name.as_str());
    let is_local = branch.kind == BranchKind::Local;
    let has_upstream = branch.tracking.is_some() && !branch.gone;
    let mut items = [MenuItemState::enabled(); 12];
    items[BranchMenuAction::Checkout.index()] = if is_current {
        MenuItemState::disabled("already checked out")
    } else {
        MenuItemState::enabled()
    };
    items[BranchMenuAction::CheckoutAndPull.index()] = if is_current {
        MenuItemState::disabled("already checked out")
    } else if !has_upstream {
        MenuItemState::disabled("no upstream to pull from")
    } else if is_local {
        MenuItemState::disabled("pull is offered on remote branches")
    } else {
        MenuItemState::enabled()
    };
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
    items[BranchMenuAction::Compare.index()] = if is_current {
        MenuItemState::disabled("this is the current branch")
    } else {
        MenuItemState::enabled()
    };
    items[BranchMenuAction::DiffWithWorkingTree.index()] = if is_current {
        MenuItemState::disabled("this is the current branch")
    } else {
        MenuItemState::enabled()
    };
    items[BranchMenuAction::Pull.index()] = if !is_current {
        MenuItemState::disabled("check out this branch first")
    } else if !has_upstream {
        MenuItemState::disabled("no upstream to pull from")
    } else if is_local {
        MenuItemState::disabled("pull is offered on remote branches")
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
    items[BranchMenuAction::Delete.index()] = if is_current {
        MenuItemState::disabled("the current branch cannot be deleted")
    } else {
        MenuItemState::enabled()
    };
    items
}

pub struct BranchMenuProps<'a> {
    pub repo_name: &'a str,
    pub multi_repo: bool,
    pub current_branch: Option<&'a str>,
}

pub fn branch_menu(
    ui: &mut egui::Ui,
    props: &BranchMenuProps<'_>,
    target: &Branch,
) -> Option<BranchMenuAction> {
    let states = branch_menu_items(target, props.current_branch);
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.set_min_width(296.0);
    ui.set_max_width(320.0);

    let checkout_label = if props.multi_repo {
        format!("Checkout in {}", props.repo_name)
    } else {
        "Checkout".to_owned()
    };
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
                state: *state,
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
        BranchMenuAction::CopyBranchName,
        &states[BranchMenuAction::CopyBranchName.index()],
        Icon::FILES,
        "Copy branch name",
        Some(&target.name),
        None,
        MenuItemKind::Default,
    );
    menu_rule(ui);
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
    row(
        ui,
        BranchMenuAction::DiffWithWorkingTree,
        &states[BranchMenuAction::DiffWithWorkingTree.index()],
        Icon::GIT_COMPARE,
        "Show Diff with Working Tree",
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

//! The branch context menu: one right-click action list for a branch row.
//!
//! Twelve items for one `(RootId, Branch)` target — the union of every branch
//! action the surface offers — rendered through the shared
//! [`crate::ui::widgets::menu_item`] primitive. Props in, action out — the
//! same contract [`crate::ui::branch_tree_view`] holds: the component decides
//! no policy, dispatches nothing, and never touches `AppState`. The eleven
//! gating rules live here once, in [`branch_menu_items`] — and they are the
//! whole set, because the copy item reads the name it was handed and so has
//! nothing to be blocked by — so the wording and enablement of a blocked action
//! can never drift between surfaces.

use turbogit_domain::model::{Branch, BranchKind};

use crate::ui::icons::Icon;
use crate::ui::widgets::{MenuItemKind, MenuItemProps, menu_item, menu_rule};

/// What the user picked. Plain data — the surface owns what it means.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BranchMenuAction {
    /// Copy THIS row's name — not the checked-out branch's, which may be another
    /// row. Inert: no ref moves and no repository state is consulted.
    CopyBranchName,
    Checkout,
    NewBranchFrom,
    CheckoutAndPull,
    Merge,
    Rebase,
    Compare,
    /// The reference's entry into its fifth integration verb: compare this
    /// branch's files with the working tree, which is where `Get from Branch`
    /// lives. Read-only, so — like [`Self::Compare`] — a remote-tracking row can be
    /// one side of it.
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

    /// The item's slot in the array [`branch_menu_items`] returns.
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

/// One item's gate: whether it acts, and what it says when it cannot.
///
/// The type moved to the widget layer with the second menu that needed it
/// ([`crate::ui::widgets::MenuItemState`]); this is its established import path
/// and stays.
pub use crate::ui::widgets::MenuItemState;

/// The twelve items' gates, in [`BranchMenuAction::ORDER`] — one function, one
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
    // The pull family means "fetch and integrate this branch's upstream", so
    // unlike the source-branch verbs below it is offered on remote rows only:
    // on a local row `Merge`/`Rebase` are how commits move, and
    // a "Pull" label there names an operation that is not what would run.
    items[BranchMenuAction::CheckoutAndPull.index()] = if is_current {
        MenuItemState::disabled("already checked out")
    } else if !has_upstream {
        MenuItemState::disabled("no upstream to pull from")
    } else if is_local {
        MenuItemState::disabled("pull is offered on remote branches")
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
    // The same rule, for the same reason: comparing a branch with the working tree
    // of that very branch is a list that can only come back empty, and a row that
    // is HEAD has nothing to take from itself. Remote-tracking rows are legal,
    // because `Get from Branch` reads a revision rather than moving a ref.
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

/// Paint the twelve items for one branch and report which was clicked.
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
        // `FILES` is this app's copy glyph — the one [`crate::ui::commit_menu`]
        // gives both of its copy verbs, because the embedded set has no
        // clipboard. `GIT_BRANCH` would be the literal match and would collide
        // with `New branch from` two rows below it.
        Icon::FILES,
        "Copy branch name",
        // The name is the value being copied, so it rides the data face — the
        // same slot and face `Copy hash` uses for the short reference.
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
    row(
        ui,
        BranchMenuAction::DiffWithWorkingTree,
        &states[BranchMenuAction::DiffWithWorkingTree.index()],
        // The comparison glyph again, deliberately: this item and the one above it
        // are the two readings of the same question — commit-level and file-level —
        // and a distinct icon would imply a distinct family.
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

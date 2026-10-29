use turbogit_app::integrate_service::OwnedOperation;
use turbogit_domain::model::Commit;

use crate::ui::icons::Icon;
use crate::ui::rewrite_gating::{self, RewriteVerb, Scope};
use crate::ui::widgets::{MenuItemKind, MenuItemProps, MenuItemState, menu_item, menu_rule};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CommitMenuAction {
    CopyHash,
    CopyMessage,
    CreatePatch,
    CherryPickTo,
    CherryPickAcross,
    Checkout,
    RevertCommit,
    DropCommit,
    RewordCommit,
    NewBranch,
    NewTag,
}

impl CommitMenuAction {
    pub const ORDER: [Self; 11] = [
        Self::CopyHash,
        Self::CopyMessage,
        Self::CreatePatch,
        Self::CherryPickTo,
        Self::CherryPickAcross,
        Self::Checkout,
        Self::RevertCommit,
        Self::DropCommit,
        Self::RewordCommit,
        Self::NewBranch,
        Self::NewTag,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::CopyHash => 0,
            Self::CopyMessage => 1,
            Self::CreatePatch => 2,
            Self::CherryPickTo => 3,
            Self::CherryPickAcross => 4,
            Self::Checkout => 5,
            Self::RevertCommit => 6,
            Self::DropCommit => 7,
            Self::RewordCommit => 8,
            Self::NewBranch => 9,
            Self::NewTag => 10,
        }
    }

    const fn ruled_before_next(self) -> bool {
        matches!(
            self,
            Self::CherryPickAcross | Self::Checkout | Self::RewordCommit
        )
    }
}

pub(crate) mod reason {
    pub(crate) const DIRTY: &str = "Resolve the uncommitted changes first";
    pub(crate) const SINGLE_ROOT: &str = "only one repository is open";
    pub(crate) const PROTECTED: &str = "the current branch is protected";
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitFacts<'a> {
    pub dirty: bool,
    pub protected_branch: bool,
    pub on_current_branch: bool,
    pub multi_root: bool,
    /// The operation that currently holds this repository, if any.
    ///
    /// Asked of the engine by [`turbogit_app::state::AppState::holding_operation`]
    /// rather than read from a cached snapshot, and deliberately so: an operation
    /// started by
    /// another process, or by this app's own worker between two refreshes, has
    /// to gate the verb the moment the menu opens. It is the one fact here that
    /// touches the filesystem, it is per root, which is the whole of the
    /// multi-root discrimination, a rebase in another repository is not this
    /// root's business, and it is asked once per open menu per frame, not once
    /// per row.
    pub owner: Option<OwnedOperation>,
    pub scope: Scope,
    pub repo_name: &'a str,
}

pub fn commit_menu_items(commit: &Commit, facts: &CommitFacts<'_>) -> [MenuItemState; 11] {
    let mut items = [MenuItemState::enabled(); 11];

    let needs_clean_tree = [
        CommitMenuAction::CherryPickTo,
        CommitMenuAction::CherryPickAcross,
        CommitMenuAction::Checkout,
        CommitMenuAction::RevertCommit,
    ];
    for action in needs_clean_tree {
        if facts.dirty {
            items[action.index()] = MenuItemState::disabled(reason::DIRTY);
        }
    }
    if !facts.multi_root && items[CommitMenuAction::CherryPickAcross.index()].enabled {
        items[CommitMenuAction::CherryPickAcross.index()] =
            MenuItemState::disabled(reason::SINGLE_ROOT);
    }
    if facts.protected_branch && !facts.dirty {
        items[CommitMenuAction::RevertCommit.index()] = MenuItemState::disabled(reason::PROTECTED);
    }

    let rewriteable = [
        (CommitMenuAction::DropCommit, RewriteVerb::Drop),
        (CommitMenuAction::RewordCommit, RewriteVerb::Reword),
    ];
    for (action, verb) in rewriteable {
        items[action.index()] = match rewrite_gating::blocked(commit, facts) {
            Some(block) => MenuItemState::disabled(block.stated(verb)),
            None => MenuItemState::enabled(),
        };
    }
    items
}

pub fn commit_menu(
    ui: &mut egui::Ui,
    commit: &Commit,
    facts: &CommitFacts<'_>,
) -> Option<CommitMenuAction> {
    let states = commit_menu_items(commit, facts);
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.set_min_width(296.0);
    ui.set_max_width(320.0);

    let short_ref = crate::ui::widgets::short_commit_ref(&commit.id);
    let repo_name = facts.multi_root.then_some(facts.repo_name);
    let mut clicked = None;
    macro_rules! row {
        ($action:expr, $icon:expr, $label:expr, $data:expr, $kind:expr) => {{
            let state = &states[$action.index()];
            if menu_item(
                ui,
                MenuItemProps {
                    icon: $icon,
                    label: $label,
                    data: $data,
                    shortcut: None,
                    state: *state,
                    kind: $kind,
                },
            )
            .clicked()
            {
                clicked = Some($action);
            }
            if $action.ruled_before_next() {
                menu_rule(ui);
            }
        }};
    }

    row!(
        CommitMenuAction::CopyHash,
        Icon::FILES,
        "Copy hash",
        Some(&short_ref),
        MenuItemKind::Default
    );
    row!(
        CommitMenuAction::CopyMessage,
        Icon::FILE_CODE,
        "Copy commit message",
        None,
        MenuItemKind::Default
    );
    row!(
        CommitMenuAction::CreatePatch,
        Icon::ARCHIVE,
        "Create patch",
        None,
        MenuItemKind::Default
    );
    row!(
        CommitMenuAction::CherryPickTo,
        Icon::ARROW_DOWN_CIRCLE,
        "Cherry-pick to…",
        repo_name,
        MenuItemKind::Default
    );
    row!(
        CommitMenuAction::CherryPickAcross,
        Icon::ARROW_RIGHT_LEFT,
        "Cherry-pick across…",
        None,
        MenuItemKind::Default
    );
    row!(
        CommitMenuAction::Checkout,
        Icon::CHECK,
        "Checkout",
        None,
        MenuItemKind::Default
    );
    row!(
        CommitMenuAction::RevertCommit,
        Icon::UNDO,
        "Revert commit",
        None,
        MenuItemKind::Default
    );
    row!(
        CommitMenuAction::DropCommit,
        Icon::TRASH_2,
        "Drop commit",
        None,
        MenuItemKind::Danger
    );
    row!(
        CommitMenuAction::RewordCommit,
        Icon::PENCIL,
        "Reword commit",
        None,
        MenuItemKind::Default
    );
    row!(
        CommitMenuAction::NewBranch,
        Icon::GIT_BRANCH,
        "New branch",
        None,
        MenuItemKind::Default
    );
    row!(
        CommitMenuAction::NewTag,
        Icon::TAG,
        "New tag",
        None,
        MenuItemKind::Default
    );
    clicked
}

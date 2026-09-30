//! The commit context menu: one right-click action list for a commit row.
//!
//! Eleven items for one commit — the union of every action the details pane
//! used to spread over an ACTIONS grid, a header button and a clickable hash
//! chip — painted through the same [`menu_item`] / [`menu_rule`] /
//! [`menu_surface`] primitives as every other menu in the window, and hosted by
//! [`crate::ui::widgets::menu_host`] like every other right-click.
//!
//! Props in, action out, exactly as [`crate::ui::branch_menu`] holds that
//! contract: this module decides no policy beyond the eleven gates, dispatches
//! nothing, and never touches `AppState`. What it is told is plain facts about
//! the commit and its repository ([`CommitFacts`]), so a gate that costs a git
//! call has nowhere to hide.
//!
//! No item is [`MenuItemKind::Primary`]: a commit has no single dominant verb,
//! the first slot belongs to Copy hash, and brand ink on a copy action would be
//! absurd. Only Drop commit is [`MenuItemKind::Danger`] — it is the one item
//! here that can lose work outright, and severity in this kit is per-item
//! rather than positional.

use turbogit_domain::model::Commit;

use crate::ui::icons::Icon;
use crate::ui::widgets::{MenuItemKind, MenuItemProps, MenuItemState, menu_item, menu_rule};

/// What the user picked. Plain data — the surface owns what it means.
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
    /// The designed item order, top of the menu to bottom. The four groups are
    /// separated by [`menu_rule`] in [`commit_menu`], never by a gap in here.
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

    /// The item's slot in the array [`commit_menu_items`] returns.
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

    /// Whether this item's row is followed by a group rule. Three rules, four
    /// groups: copy and cherry-pick / checkout / the history verbs / refs.
    const fn ruled_before_next(self) -> bool {
        matches!(
            self,
            Self::CherryPickAcross | Self::Checkout | Self::RewordCommit
        )
    }
}

/// The reasons a commit action is blocked, in one place so two menus that gate
/// the same way cannot word it differently.
///
/// The worktree guard is stated FIRST for every verb it blocks, because it is
/// the one the developer can resolve immediately; the structural guards — what
/// kind of commit this is, which history it sits in, what branch is checked out
/// — are stated after it.
mod reason {
    pub(crate) const DIRTY: &str = "Resolve the uncommitted changes first";
    pub(crate) const SINGLE_ROOT: &str = "only one repository is open";
    pub(crate) const PROTECTED: &str = "the current branch is protected";
    pub(crate) const OFF_BRANCH: &str = "not on the current branch";
    pub(crate) const MERGE_DROP: &str = "a merge commit cannot be dropped";
    pub(crate) const MERGE_REWORD: &str = "a merge commit cannot be reworded";
    // Stated in the service's own noun, so the item and the refusal the service
    // would give name the commit the same way. The fallback toast in
    // `AppState::open_rewrite_preflight` quotes that message verbatim.
    pub(crate) const ROOT_DROP: &str = "a root commit cannot be dropped";
    pub(crate) const ROOT_REWORD: &str = "a root commit cannot be reworded";
}

/// Plain facts about one commit's repository, each one read off state the log
/// already caches. Merge-ness is not here because it is not a fact to be told:
/// it is `commit.parents.len() > 1`, and a gate that could contradict the commit
/// it is gating would be a second source of truth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitFacts<'a> {
    /// The root's worktree has uncommitted changes.
    pub dirty: bool,
    /// The root's current branch is protected in settings.
    pub protected_branch: bool,
    /// The commit is an ancestor of the current branch's tip, which is what a
    /// plan built to the tip can replay. A commit found by searching another
    /// branch reads as deliberately read-only rather than broken.
    pub on_current_branch: bool,
    /// More than one repository root is in scope, which is the only condition
    /// under which a cross-repository cherry-pick has a destination.
    pub multi_root: bool,
    /// The repository's own name. Borrowed rather than owned so a name the
    /// frame is already painting costs no allocation, and — the reason it is
    /// here at all — so an action aimed at one repository can name it, the way
    /// [`crate::ui::branch_menu::BranchMenuProps`] names one. Only read when
    /// `multi_root`, where there is something to tell apart.
    pub repo_name: &'a str,
}

/// The eleven gates, in [`CommitMenuAction::ORDER`] — one function, one set of
/// strings, from plain data only.
pub fn commit_menu_items(commit: &Commit, facts: &CommitFacts<'_>) -> [MenuItemState; 11] {
    let is_merge = commit.parents.len() > 1;
    let mut items = [MenuItemState::enabled(); 11];

    // Copying and patching read the commit alone: no worktree, no branch, no
    // second repository, so nothing can block them.
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
    // A cross-repository pick with one repository in scope has nowhere to go;
    // it stays rendered and says so rather than vanishing. Only where the
    // worktree has not already spoken — one reason per item, the actionable one
    // first.
    if !facts.multi_root && items[CommitMenuAction::CherryPickAcross.index()].enabled {
        items[CommitMenuAction::CherryPickAcross.index()] =
            MenuItemState::disabled(reason::SINGLE_ROOT);
    }
    // Revert is also a commit on the current branch, so a protected branch
    // refuses it — but only after the worktree guard has had its say.
    if facts.protected_branch && !facts.dirty {
        items[CommitMenuAction::RevertCommit.index()] = MenuItemState::disabled(reason::PROTECTED);
    }

    // The two history verbs are bounded identically: one rewrite path, so the
    // same four reasons. The first two are what kind of commit this is — a merge
    // is blocked because nothing routes `--rebase-merges` through the
    // interactive path, a root commit because the plan is built over the first
    // parent and it has none — so they are asked together, before anything about
    // the repository or the branch. The remaining two are about where the commit
    // sits: the plan is built to this branch's tip, so a commit off that branch
    // has no row in it, and a protected branch refuses the rewrite outright.
    //
    // Root-ness is the fourth reason because an enabled item that can never work
    // is a defect: `targeted_plan` refuses a root commit outright, and before
    // this the developer only found out by clicking. It is a `parents` count
    // like merge-ness, so it costs no git call and cannot contradict the commit.
    let rewriteable = [
        (
            CommitMenuAction::DropCommit,
            reason::MERGE_DROP,
            reason::ROOT_DROP,
        ),
        (
            CommitMenuAction::RewordCommit,
            reason::MERGE_REWORD,
            reason::ROOT_REWORD,
        ),
    ];
    for (action, merge_reason, root_reason) in rewriteable {
        items[action.index()] = if is_merge {
            MenuItemState::disabled(merge_reason)
        } else if commit.parents.is_empty() {
            MenuItemState::disabled(root_reason)
        } else if !facts.on_current_branch {
            MenuItemState::disabled(reason::OFF_BRANCH)
        } else if facts.protected_branch {
            MenuItemState::disabled(reason::PROTECTED)
        } else {
            MenuItemState::enabled()
        };
    }
    items
}

/// Paint the eleven items for one commit and report which was clicked.
///
/// The menu is 296 px wide and grows to 320 px before a long value
/// middle-truncates — the branch menu's width, so two menus in one window read
/// as one kit. Eleven rows plus three rules is ~290 px of height, inside the
/// 1000×680 minimum `src/main.rs` enforces (ADR-0024), so there is no scroll
/// container.
///
/// Two glyphs are approximations from the embedded set rather than literal
/// matches: `FILES` carries both copy verbs because the set has no clipboard,
/// and `ARROW_RIGHT_LEFT` carries the cross-repository pick because it is
/// already the app's "across" mark. Both beat inventing path data outside it.
pub fn commit_menu(
    ui: &mut egui::Ui,
    commit: &Commit,
    facts: &CommitFacts<'_>,
) -> Option<CommitMenuAction> {
    let states = commit_menu_items(commit, facts);
    // There is no longer a "not in this build" layer over the commit's own gates.
    // Both history verbs shipped (ticket 10 drop, ticket 11 reword), so every
    // state in this table now answers ONE question — what THIS commit in THIS
    // repository allows — and an item is either live or states the bound that
    // stopped it.
    // Menu rows sit flush; the rules state their own air.
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.set_min_width(296.0);
    ui.set_max_width(320.0);

    let short_ref = crate::ui::widgets::short_commit_ref(&commit.id);
    // Which repository a cherry-pick would land in is a value, so it rides in
    // the data face — the same slot and face Copy hash uses for the short
    // reference, so a name never reads as a label. One repository needs no
    // naming, and the `multi_repo` gate is the only condition that changes what
    // the row says, exactly as the branch menu's `Checkout in {repo}` does.
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
                    enabled: state.enabled,
                    disabled_reason: state.reason,
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

use turbogit_domain::model::{Change, ChangeStatus};

use crate::ui::icons::Icon;
use crate::ui::widgets::{MenuItemKind, MenuItemProps, MenuItemState, menu_item, menu_rule};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangesMenuAction {
    StageToggle,
    Discard,
    Shelve,
    CreatePatch,
    CopyPatch,
    ShowBlame,
    Resolve,
    TakeOurs,
    TakeTheirs,
}

/// Whether the menu paints a rule under an item.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rule {
    After,
    None,
}

impl ChangesMenuAction {
    pub const fn index(self) -> usize {
        match self {
            Self::StageToggle => 0,
            Self::Discard => 1,
            Self::Shelve => 2,
            Self::CreatePatch => 3,
            Self::CopyPatch => 4,
            Self::ShowBlame => 5,
            Self::Resolve => 6,
            Self::TakeOurs => 7,
            Self::TakeTheirs => 8,
        }
    }

    fn presentation(self, stage_label: &str) -> (Icon, &str, Option<&'static str>) {
        match self {
            Self::StageToggle => (Icon::CHECK_SQUARE, stage_label, Some("Space")),
            Self::Discard => (Icon::UNDO, "Discard changes…", None),
            Self::Shelve => (Icon::FOLDER, "Shelve…", None),
            Self::CreatePatch => (
                Icon::ARCHIVE,
                crate::ui::create_patch::LOCAL_CHANGES_VERB,
                None,
            ),
            Self::CopyPatch => (Icon::FILES, crate::ui::create_patch::CLIPBOARD_VERB, None),
            Self::ShowBlame => (Icon::FILE_CODE, "Show blame", None),
            Self::Resolve => (Icon::GIT_MERGE, "Resolve…", None),
            Self::TakeOurs => (Icon::ARROW_LEFT, "Take ours", None),
            Self::TakeTheirs => (Icon::ARROW_RIGHT, "Take theirs", None),
        }
    }
}

/// `(item, rule)` in the designed order, top of the menu to bottom. The two
/// orders are two because a Merge-conflicts row has no index state to toggle and
/// never joins the selection, so the verbs reading the set have nothing there.
pub const FILE_ORDER: [(ChangesMenuAction, Rule); 6] = [
    (ChangesMenuAction::StageToggle, Rule::After),
    (ChangesMenuAction::Discard, Rule::None),
    (ChangesMenuAction::Shelve, Rule::After),
    (ChangesMenuAction::CreatePatch, Rule::None),
    (ChangesMenuAction::CopyPatch, Rule::After),
    (ChangesMenuAction::ShowBlame, Rule::None),
];

pub const CONFLICT_ORDER: [(ChangesMenuAction, Rule); 5] = [
    (ChangesMenuAction::Resolve, Rule::None),
    (ChangesMenuAction::TakeOurs, Rule::None),
    (ChangesMenuAction::TakeTheirs, Rule::After),
    (ChangesMenuAction::Discard, Rule::After),
    (ChangesMenuAction::ShowBlame, Rule::None),
];

pub struct ChangesMenuProps<'a> {
    pub change: &'a Change,
    /// The staging verb for the state the row's own checkbox is painted in.
    pub stage_label: &'a str,
    /// Whether the **Changes selection** holds any path git can diff or stash.
    /// `git stash push` never takes an untracked file and a patch needs a diff,
    /// so a selection of nothing but untracked rows answers these three verbs
    /// with nothing done and success reported.
    pub parkable: bool,
}

mod reason {
    pub(super) const UNTRACKED_RESTORE: &str = "an untracked file has no committed version";
    pub(super) const UNTRACKED_BLAME: &str = "untracked files have no committed version to blame";
    pub(super) const UNTRACKED_PARK: &str = "untracked files have nothing for git to park";
    pub(super) const UNMERGED: &str = "resolve the conflict first";
}

/// The nine items' gates, in [`ChangesMenuAction::index`] order. Every rule reads
/// a field the status snapshot already carries, so opening the menu costs no call.
pub fn changes_menu_items(props: &ChangesMenuProps<'_>) -> [MenuItemState; 9] {
    let untracked = props.change.status == ChangeStatus::Unversioned;
    let conflicted = props.change.status == ChangeStatus::Conflicted;
    let mut items = [MenuItemState::enabled(); 9];
    items[ChangesMenuAction::Discard.index()] = if conflicted {
        MenuItemState::disabled(reason::UNMERGED)
    } else if untracked {
        MenuItemState::disabled(reason::UNTRACKED_RESTORE)
    } else {
        MenuItemState::enabled()
    };
    items[ChangesMenuAction::ShowBlame.index()] = if untracked {
        MenuItemState::disabled(reason::UNTRACKED_BLAME)
    } else {
        MenuItemState::enabled()
    };
    if !props.parkable {
        for slot in [
            ChangesMenuAction::Shelve,
            ChangesMenuAction::CreatePatch,
            ChangesMenuAction::CopyPatch,
        ] {
            items[slot.index()] = MenuItemState::disabled(reason::UNTRACKED_PARK);
        }
    }
    items
}

pub fn changes_menu(
    ui: &mut egui::Ui,
    props: &ChangesMenuProps<'_>,
    order: &[(ChangesMenuAction, Rule)],
) -> Option<ChangesMenuAction> {
    let states = changes_menu_items(props);
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.set_min_width(280.0);
    ui.set_max_width(320.0);

    let mut clicked = None;
    for (action, rule) in order {
        let (icon, label, shortcut) = action.presentation(props.stage_label);
        let kind = if *action == ChangesMenuAction::Discard {
            MenuItemKind::Danger
        } else {
            MenuItemKind::Default
        };
        if menu_item(
            ui,
            MenuItemProps {
                icon,
                label,
                data: None,
                shortcut,
                state: states[action.index()],
                kind,
            },
        )
        .clicked()
        {
            clicked = Some(*action);
        }
        if *rule == Rule::After {
            menu_rule(ui);
        }
    }
    clicked
}

use turbogit_domain::model::DisplayName;
use turbogit_services::changes;
use turbogit_services::shelve_stash::stash_ref;

use crate::operation::Operation;
use crate::root_caches::Affected;
use crate::state::{AppState, BranchDeletePending, PendingConfirm, short_sha};

const DISCARD_PATHS_NAMED: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmAction {
    Confirm,
    ShelveFirst,
    BringAlong,
    SetAside,
    Close,
    OpenUnmerged,
}

pub struct Prompt {
    pub rows: Vec<Row>,
    pub disclosure: Option<Disclosure>,
    pub buttons: Vec<Button>,
}

pub struct Row {
    pub lines: Vec<Line>,
}

pub struct Line {
    pub text: String,
    pub warning: bool,
}

pub struct Button {
    pub label: String,
    pub action: ConfirmAction,
}

pub struct Disclosure {
    pub label: String,
    pub action: ConfirmAction,
}

pub fn prompt(state: &AppState, c: &PendingConfirm) -> Prompt {
    match c {
        PendingConfirm::Discard { changes } => {
            let n = changes.len();
            let mut named: Vec<String> = changes
                .iter()
                .take(DISCARD_PATHS_NAMED)
                .map(|change| change.path.display().to_string())
                .collect();
            if n > DISCARD_PATHS_NAMED {
                named.push(format!("(+{} more)", n - DISCARD_PATHS_NAMED));
            }
            Prompt {
                rows: vec![
                    Row::strip(vec![format!(
                        "Discard changes to {n} file(s)? This cannot be undone."
                    )]),
                    Row::strip(named),
                ],
                disclosure: None,
                buttons: vec![
                    Button::new("Shelve first", ConfirmAction::ShelveFirst),
                    Button::new(
                        format!("Discard {n}-file changes"),
                        ConfirmAction::Confirm,
                    ),
                    Button::new("Cancel", ConfirmAction::Close),
                ],
            }
        }
        PendingConfirm::DeleteLocalBranch { name } => {
            let mut rows = vec![Row::strip(vec![format!(
                "Delete local branch '{name}'? This cannot be undone."
            )])];
            if let Some(warning) = state.branch_ahead_warning(name) {
                rows.push(Row::warning(warning));
            }
            if let Some(consequence) = &state.ui.branches_delete_consequence {
                rows.push(Row::strip(vec![consequence.clone()]));
            }
            Prompt {
                rows,
                disclosure: state.ui.branches_delete_unmerged.as_ref().map(|unmerged| {
                    Disclosure {
                        label: unmerged.link_label(),
                        action: ConfirmAction::OpenUnmerged,
                    }
                }),
                buttons: vec![Button::ok(), Button::cancel()],
            }
        }
        PendingConfirm::DeleteRemoteBranch { remote, name } => Prompt::question(
            format!("Delete remote branch '{remote}/{name}'? This cannot be undone."),
            vec![Button::ok(), Button::cancel()],
        ),
        PendingConfirm::CheckoutDirty { target, .. } => Prompt {
            rows: vec![
                Row::strip(vec![format!(
                    "You have uncommitted changes. Switch to '{target}'?"
                )]),
                Row::strip(vec![
                    "Bring along — changes come with you; if any conflict with the other branch, checkout is refused and nothing changes."
                        .to_string(),
                ]),
                Row::strip(vec![
                    "Set aside — changes are stashed now and can be brought back afterwards."
                        .to_string(),
                ]),
            ],
            disclosure: None,
            buttons: vec![
                Button::new("Bring along", ConfirmAction::BringAlong),
                Button::new("Set aside", ConfirmAction::SetAside),
                Button::new("Cancel", ConfirmAction::Close),
            ],
        },
        PendingConfirm::CheckoutInWorktree { branch, worktree } => Prompt {
            rows: vec![
                Row::strip(vec![format!(
                    "'{branch}' is already checked out in another worktree:"
                )]),
                Row::strip(vec![worktree.display().to_string()]),
                Row::strip(vec!["Switch there, or close that worktree first.".to_string()]),
            ],
            disclosure: None,
            buttons: vec![Button::ok()],
        },
        PendingConfirm::RemoveWorktree { path } => Prompt::question(
            format!(
                "Remove worktree '{}'? This cannot be undone.",
                path.display_name_or("<worktree>")
            ),
            vec![Button::ok(), Button::cancel()],
        ),
        PendingConfirm::DeinitSubmodule { path } => Prompt::question(
            format!(
                "De-init submodule '{}'? Its working copy is removed.",
                path.display_name_or("<submodule>")
            ),
            vec![Button::ok(), Button::cancel()],
        ),
        PendingConfirm::InitHere => Prompt::question(
            "Initialize a git repository in this directory?".to_string(),
            vec![Button::ok(), Button::cancel()],
        ),
        PendingConfirm::CloneRepo => Prompt::question(
            "Clone the repository at the given URL?".to_string(),
            vec![Button::ok(), Button::cancel()],
        ),
        PendingConfirm::RevertCommit { commit } => Prompt::question(
            format!(
                "Revert commit {}? A new inverse commit will be created on the current branch.",
                crate::state::short_sha(commit)
            ),
            vec![Button::ok(), Button::cancel()],
        ),
        PendingConfirm::GetFileFromBranch { root, source, path } => Prompt::question(
            format!(
                "Replace {} in {} with the entire contents it has on '{source}'? The file's current contents are lost, and nothing is committed.",
                path.display(),
                root.name()
            ),
            vec![
                Button::new("Replace file", ConfirmAction::Confirm),
                Button::cancel(),
            ],
        ),
        PendingConfirm::DeleteShelf { name, .. } => Prompt::question(
            format!(
                "Delete shelf '{name}'? Its row leaves the shelf, and the changes it parked \
                 are dropped from this repository's stash list."
            ),
            vec![Button::ok(), Button::cancel()],
        ),
        PendingConfirm::DeleteStash { index, .. } => Prompt::question(
            format!(
                "Delete stash {}? The entry is removed, and with it the only copy of what it \
                 holds.",
                stash_ref(*index)
            ),
            vec![Button::ok(), Button::cancel()],
        ),
    }
}

/// Run the control `action` names, and leave the confirmation slot exactly as
/// that control leaves it.
///
/// An action a verb's row does not declare does nothing: the row is what pairs
/// a verb with its controls, so there is no such pair to arrive.
pub fn apply(state: &mut AppState, c: &PendingConfirm, action: ConfirmAction) {
    match c {
        PendingConfirm::Discard { changes } => match action {
            ConfirmAction::ShelveFirst => {
                let changes_owned = changes.clone();
                let n = changes.len();
                state.ui.confirm = None;
                if let Some(root) = state.selected_root.clone() {
                    state.dispatch(Operation::shelve(
                        root,
                        changes_owned,
                        format!("Shelved {n} file(s) before discard"),
                    ));
                }
            }
            ConfirmAction::Confirm => {
                if let Some(live) = state.ui.confirm.clone() {
                    run(state, live);
                    state.ui.confirm = None;
                }
            }
            ConfirmAction::Close => state.ui.confirm = None,
            _ => {}
        },
        PendingConfirm::DeleteLocalBranch { name } => match action {
            ConfirmAction::OpenUnmerged => {
                if let Some(unmerged) = state.ui.branches_delete_unmerged.clone() {
                    state.open_unmerged_commits(&unmerged);
                }
            }
            ConfirmAction::Confirm => {
                if let Some(root) = state.selected_root.clone()
                    && let Some(tip_sha) = state.branch_tip(&root, name)
                {
                    let upstream = state.branch_upstream(&root, name);
                    let unmerged = state.ui.branches_delete_unmerged.clone();
                    state.ui.branches_delete_pending = Some(BranchDeletePending {
                        root,
                        name: name.clone(),
                        tip_sha,
                        upstream,
                        unmerged,
                    });
                }
                run(state, c.clone());
                clear_branch_delete(state);
            }
            ConfirmAction::Close => clear_branch_delete(state),
            _ => {}
        },
        PendingConfirm::CheckoutDirty {
            root,
            target,
            kind,
            detach,
        } => match action {
            ConfirmAction::BringAlong => {
                state.ui.confirm = None;
                if *detach {
                    state.checkout_detached_op(root, target);
                } else {
                    state.checkout_branch_op(root, *kind, target);
                }
            }
            ConfirmAction::SetAside => {
                state.ui.confirm = None;
                if *detach {
                    state.checkout_detached_set_aside(root, target);
                } else {
                    state.checkout_branch_set_aside(root, *kind, target);
                }
            }
            ConfirmAction::Close => state.ui.confirm = None,
            _ => {}
        },
        PendingConfirm::DeleteRemoteBranch { .. }
        | PendingConfirm::CheckoutInWorktree { .. }
        | PendingConfirm::RemoveWorktree { .. }
        | PendingConfirm::DeinitSubmodule { .. }
        | PendingConfirm::InitHere
        | PendingConfirm::CloneRepo
        | PendingConfirm::RevertCommit { .. }
        | PendingConfirm::GetFileFromBranch { .. }
        | PendingConfirm::DeleteShelf { .. }
        | PendingConfirm::DeleteStash { .. } => match action {
            ConfirmAction::Confirm => {
                run(state, c.clone());
                state.ui.confirm = None;
            }
            ConfirmAction::Close => state.ui.confirm = None,
            _ => {}
        },
    }
}

pub fn run(state: &mut AppState, c: PendingConfirm) {
    match c {
        PendingConfirm::Discard { changes } => {
            let Some(root) = state.selected_root.clone() else {
                return;
            };
            state.dispatch(Operation::custom(
                "Discard changes",
                Affected::Root(root.clone()),
                move |v| changes::discard_changes(v, root.as_path(), &changes),
            ));
        }
        PendingConfirm::DeleteLocalBranch { name } => {
            let Some(root) = state.selected_root.clone() else {
                return;
            };
            state.dispatch(Operation::DeleteBranch { root, name });
        }
        PendingConfirm::DeleteRemoteBranch { remote, name } => {
            let Some(root) = state.selected_root.clone() else {
                return;
            };
            state.dispatch(Operation::custom(
                "Delete remote branch",
                Affected::Root(root.clone()),
                move |v| v.branch_delete_remote(root.as_path(), &remote, &name),
            ));
        }
        PendingConfirm::CheckoutDirty {
            root,
            target,
            kind,
            detach,
        } => {
            if detach {
                state.checkout_detached_op(&root, &target);
            } else {
                state.checkout_branch_op(&root, kind, &target);
            }
        }
        PendingConfirm::CheckoutInWorktree { .. } => {}
        PendingConfirm::InitHere => state.init_repo(),
        PendingConfirm::CloneRepo => state.clone_repo(),
        PendingConfirm::RemoveWorktree { path } => {
            if let Some(root) = state.selected_root.clone() {
                state.dispatch(Operation::WorktreeRemove { root, path });
            }
        }
        PendingConfirm::DeinitSubmodule { path } => {
            let Some(root) = state.selected_root.clone() else {
                return;
            };
            state.dispatch(Operation::custom(
                "Deinit submodule",
                Affected::Root(root.clone()),
                move |v| v.submodule_deinit(root.as_path(), &path, false),
            ));
        }
        PendingConfirm::RevertCommit { commit } => {
            let Some(root) = state.selected_root.clone() else {
                return;
            };
            let settings = state.settings.clone();
            state.dispatch(Operation::custom(
                format!("Revert {}", short_sha(&commit)),
                Affected::Root(root.clone()),
                move |v| {
                    turbogit_services::integrate_service::revert_commit(
                        v,
                        root.as_path(),
                        &commit,
                        &settings,
                    )
                },
            ));
        }
        PendingConfirm::GetFileFromBranch { root, source, path } => {
            state.dispatch(Operation::GetFileFromBranch { root, source, path });
        }
        PendingConfirm::DeleteShelf { root, name } => {
            let git_root = root.clone();
            let git_name = name.clone();
            state.dispatch(Operation::custom(
                format!("Delete shelf '{name}'"),
                Affected::Root(root.clone()),
                move |v| {
                    turbogit_services::shelve_stash::discard_shelf_content(
                        v,
                        git_root.as_path(),
                        &git_name,
                    )
                },
            ));
            state.remove_shelf(&root, &name);
        }
        PendingConfirm::DeleteStash { root, index } => {
            state.dispatch(Operation::custom(
                format!("Delete stash {}", stash_ref(index)),
                Affected::Root(root.clone()),
                move |v| v.stash_drop(root.as_path(), index),
            ));
        }
    }
}

fn clear_branch_delete(state: &mut AppState) {
    state.ui.confirm = None;
    state.ui.branches_delete_consequence = None;
    state.ui.branches_delete_unmerged = None;
}

impl Prompt {
    fn question(text: String, buttons: Vec<Button>) -> Self {
        Self {
            rows: vec![Row::strip(vec![text])],
            disclosure: None,
            buttons,
        }
    }
}

impl Row {
    fn strip(lines: Vec<String>) -> Self {
        Self {
            lines: lines.into_iter().map(Line::plain).collect(),
        }
    }

    fn warning(text: String) -> Self {
        Self {
            lines: vec![Line::warning(text)],
        }
    }
}

impl Line {
    fn plain(text: String) -> Self {
        Self {
            text,
            warning: false,
        }
    }

    fn warning(text: String) -> Self {
        Self {
            text,
            warning: true,
        }
    }
}

impl Button {
    fn new(label: impl Into<String>, action: ConfirmAction) -> Self {
        Self {
            label: label.into(),
            action,
        }
    }

    fn ok() -> Self {
        Self::new("OK", ConfirmAction::Confirm)
    }

    fn cancel() -> Self {
        Self::new("Cancel", ConfirmAction::Close)
    }
}

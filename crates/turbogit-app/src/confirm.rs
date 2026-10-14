//! Confirmation: everything about a pending confirmation except painting it.
//!
//! [`PendingConfirm`](crate::state::PendingConfirm) is a flat list of verbs the
//! Shell arms and a window asks about, and this module owns both halves of the
//! answer — the question's text ([`prompt`]) and what each of its controls does
//! ([`apply`]) — so the two can no longer disagree. A verb's extra disclosure,
//! its button row and its dispatched operation sit in one match; the renderer
//! paints what it is handed, and a verb cannot be half-painted because there is
//! no second match to leave out of. It is the mirror image of [`crate::settle`]
//! (one completion, one owner) for the question asked *before* the work runs,
//! and [`crate::state::AppState::run_confirmed`] is the thin delegation into
//! it that the headless suites call.
//!
//! It does **not** cover the window: the title, the anchor, the order and the
//! rule that closing it clears the slot stay in the renderer, because that is
//! chrome and painting, and the renderer is the only place that can paint.
//!
//! One sentence per load-bearing difference between the thirteen questions:
//!
//! - **The row is painted from a snapshot; three disclosures are read live.**
//!   The window clones [`PendingConfirm`](crate::state::PendingConfirm) once
//!   per frame, so the question cannot change under the reader's cursor. The
//!   ahead-of-upstream warning, what is lost and the unmerged count are read
//!   from `AppState` at paint time, because each one is an answer about the
//!   repository as it is now — and they are read here rather than carried in
//!   the verb, which is why [`prompt`] takes `&AppState` and not the clone.
//! - **The discard's confirm button re-reads the live slot.** "Shelve first" is
//!   declared first in the row and clears `ui.confirm`; the destructive button
//!   after it asks the slot whether it is still there, so a frame that produced
//!   both clicks parks the changes instead of destroying what was just parked.
//!   The code compiles and behaves the same either way, which is why this is
//!   written down at the call site.
//! - **A branch delete's tip is read before the delete runs.** The delete
//!   removes the local ref, and with it the upstream entry and the commits the
//!   branch alone reached, so the record settlement arms its undo from has to be
//!   taken in the same breath as the delete that destroys it.
//! - **`run` keeps the dirty-checkout arm.** The dialog reaches its three
//!   choices through [`apply`], and the arm stays because
//!   [`AppState::run_confirmed`](crate::state::AppState::run_confirmed) is a
//!   public entry a caller may still reach directly; it keeps the effect it has
//!   today rather than becoming a second answer to the same question.

use turbogit_domain::model::DisplayName;
use turbogit_services::changes;
use turbogit_services::shelve_stash::stash_ref;

use crate::operation::Operation;
use crate::root_caches::Affected;
use crate::state::{AppState, BranchDeletePending, PendingConfirm, short_sha};

/// How many paths the discard question names before it counts the rest.
const DISCARD_PATHS_NAMED: usize = 8;

/// What the window's controls ask the app to do.
///
/// Thirteen verbs, six buttons: ten of the questions are the same `OK` /
/// `Cancel` pair, and the three that are not are what the remaining variants
/// here exist for — the behaviour that used to live in the renderer. The
/// renderer's interface therefore stops growing when a verb is added: it paints
/// a row of these and hands the clicked one back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmAction {
    /// The verb's own effect, which is everything a plain `OK` did.
    Confirm,
    /// The discard's non-destructive alternative: park the changes first.
    ShelveFirst,
    /// The dirty checkout's bring-along: carry the changes to the other branch.
    BringAlong,
    /// The dirty checkout's set-aside: stash the changes, then switch.
    SetAside,
    /// Dismiss the question. Nothing runs.
    Close,
    /// The unmerged link: look at what a branch delete would lose.
    OpenUnmerged,
}

/// One pending question, as plain data: what the window says, and what each of
/// its controls asks for.
///
/// It carries no egui types, which is what lets the whole question be read
/// without an egui `Context` — the app-side suites assert these strings
/// directly, and the renderer only paints them.
pub struct Prompt {
    /// The question's text, as the strips of labels the window paints it in,
    /// top to bottom.
    pub rows: Vec<Row>,
    /// The quiet link between the text and the buttons, when this question has
    /// one.
    pub disclosure: Option<Disclosure>,
    /// The buttons, left to right, in the order a click is applied.
    pub buttons: Vec<Button>,
}

/// One horizontal strip of labels.
///
/// Ten of the questions are a single line and the discard's file list is a
/// strip of many, which is why the window's text is rows rather than one flat
/// run of lines: painted as lines, the discard question would stack its eight
/// paths into eight rows and read as a different question.
pub struct Row {
    pub lines: Vec<Line>,
}

/// One label inside a strip.
pub struct Line {
    pub text: String,
    /// Painted in the warning ink rather than the window's own — the
    /// ahead-of-upstream line, and the only line today that is.
    pub warning: bool,
}

/// One control in the button row: what it says, and what it asks for.
pub struct Button {
    pub label: String,
    pub action: ConfirmAction,
}

/// The one link a question carries above its buttons.
///
/// Painted **quiet**, because the confirm has to keep one filled control and
/// `OK` is it: a second accent-filled control beside the answer would make the
/// link and the verb look like the same weight of decision.
pub struct Disclosure {
    /// The link's own text. [`UnmergedCommits::link_label`](crate::state::UnmergedCommits::link_label)
    /// spells it, so the pre-flight question and the notification after the ref
    /// is gone cannot count the same commits differently.
    pub label: String,
    /// What the link asks for.
    pub action: ConfirmAction,
}

/// Describe `c` — the question as it is asked, with the disclosures it carries
/// read from `state` as they are now.
pub fn prompt(state: &AppState, c: &PendingConfirm) -> Prompt {
    match c {
        // Issue #02: rich confirmation — the affected files are listed so the
        // reader can see the blast radius, and "Shelve first" offers a
        // non-destructive alternative beside Discard / Cancel.
        PendingConfirm::Discard { changes } => {
            let n = changes.len();
            // The count is in the header, so the paths past the eighth are
            // counted rather than listed.
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
            // Issue #02: ahead-of-upstream warning. If the branch tracks an
            // upstream and that upstream is not an ancestor of it, the branch
            // carries commits nothing else has. Read live, like the two below.
            if let Some(warning) = state.branch_ahead_warning(name) {
                rows.push(Row::warning(warning));
            }
            // Issue 12: what is lost, in human terms.
            if let Some(consequence) = &state.ui.branches_delete_consequence {
                rows.push(Row::strip(vec![consequence.clone()]));
            }
            Prompt {
                rows,
                // The count is a link, and it is here rather than only in the
                // notification after the ref is gone: TurboGit asks first, so
                // the evidence belongs at the moment of decision.
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
                // Issue 07: plain-language care, never a bare "cannot
                // checkout". The conflict implication is stated before acting.
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
            // Issue 07: refused up front, naming the worktree — no confusing
            // checkout failure.
            rows: vec![
                Row::strip(vec![format!(
                    "'{branch}' is already checked out in another worktree:"
                )]),
                Row::strip(vec![worktree.display().to_string()]),
                Row::strip(vec!["Switch there, or close that worktree first.".to_string()]),
            ],
            disclosure: None,
            // Acknowledged and nothing more: closing this question is the whole
            // answer, so the row has no destructive button to refuse.
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
            // `short_sha` is `short_commit_ref`, so this is the same seven
            // characters the log's rows show and the bytes the user reads today.
            format!(
                "Revert commit {}? A new inverse commit will be created on the current branch.",
                crate::state::short_sha(commit)
            ),
            vec![Button::ok(), Button::cancel()],
        ),
        PendingConfirm::GetFileFromBranch { root, source, path } => Prompt::question(
            // The whole blast radius, stated because the reference documents no
            // confirm step at all for this verb, an absence this app departs
            // from on purpose: one file, in one named root, losing its entire
            // current contents — and nothing committed, since the take is a
            // working-tree write the developer then commits by hand.
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
        // Surface 07's two destructive verbs. **Whether the reference confirms
        // either of them was never observed at runtime**, so these carry
        // TurboGit's own confirming stance — the documented fallback, recorded
        // as a divergence rather than a silent invention.
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
                // Stash the changes and answer the question: the worktree goes
                // clean and the stash entry keeps the work for later recovery,
                // so this is the row's answer to a question the destructive
                // button beside it would end.
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
                // Re-read the live slot, not the snapshot `c`: "Shelve first"
                // is declared first in this row and clears `ui.confirm`, so a
                // frame that produced both clicks asks the slot whether it is
                // still there, and the discard is skipped.
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
                // Capture the tip for the undo window **before** the branch is
                // gone (issue 12). The delete removes the local ref, and with it
                // the upstream entry and the commits the branch alone reached,
                // so these reads cannot be repeated afterwards.
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
            // Each answer closes the question **before** it dispatches: the
            // checkout resolves asynchronously and the dialog must not be there
            // to answer a second time when it lands.
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
        // The ten questions whose row is `OK` / `Cancel` and whose `OK` is the
        // verb's own app-side effect — including `CheckoutInWorktree`, whose
        // arm there is empty because acknowledging it is the whole answer.
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

/// The verb's own effect, with the dialog's chrome out of the way: what
/// [`AppState::run_confirmed`](crate::state::AppState::run_confirmed) delegates
/// to, and what [`apply`] runs when a control confirms.
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
            // Force delete: the confirmation already discloses what is
            // lost in human terms (issue 12) before this runs, so an
            // unmerged branch deletes with the user's informed consent
            // instead of a bare refusal.
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
        // Issue 07: the dirty-checkout dialog dispatches its own choices;
        // this arm covers the generic confirm path (bring the changes
        // along) for completeness.
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
        // `Get from Branch`: the root, the branch and the path all
        // rode in the confirmation, so what runs is exactly what was
        // disclosed — never a re-read of whichever root is focused now, which
        // is how a per-root verb turns into a write in the wrong repository.
        PendingConfirm::GetFileFromBranch { root, source, path } => {
            state.dispatch(Operation::GetFileFromBranch { root, source, path });
        }
        // Surface 07: a shelf's record is app state, so it goes the frame the
        // verb is confirmed; its *content* is a stash entry, so that half is
        // git work and is dispatched. A git-side failure leaves the content
        // listed under Stash, where it is still recoverable — which is the
        // outcome the immediate record removal depends on, not an oversight.
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

/// The three fields a branch delete's dialog owns, cleared together — the
/// confirm slot and the two disclosures only this question shows, so the next
/// delete cannot inherit the last one's warnings.
fn clear_branch_delete(state: &mut AppState) {
    state.ui.confirm = None;
    state.ui.branches_delete_consequence = None;
    state.ui.branches_delete_unmerged = None;
}

impl Prompt {
    /// The shape ten of the questions have: one line of text, then a row.
    fn question(text: String, buttons: Vec<Button>) -> Self {
        Self {
            rows: vec![Row::strip(vec![text])],
            disclosure: None,
            buttons,
        }
    }
}

impl Row {
    /// One strip of ordinary labels.
    fn strip(lines: Vec<String>) -> Self {
        Self {
            lines: lines.into_iter().map(Line::plain).collect(),
        }
    }

    /// One label in the warning ink.
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

    /// `OK` — confirm and run.
    fn ok() -> Self {
        Self::new("OK", ConfirmAction::Confirm)
    }

    /// `Cancel` — dismiss and run nothing.
    fn cancel() -> Self {
        Self::new("Cancel", ConfirmAction::Close)
    }
}

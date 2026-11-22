//! Rewrite history: the preflight a drop or a reword runs, the reword
//! editor, the undo offer, and the gate that names why a rewrite cannot
//! start in a given repository.

use std::collections::HashSet;

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::history_editor::{self, PlanPreview};
use turbogit_app::operation::Operation;
use turbogit_app::reselection::{RewrittenAnchor, subject_of_message};
use turbogit_app::shell_writes;
use turbogit_app::state::{
    AppState, EMPTY_COMMIT_MESSAGE, HistoryVerb, RewriteUndo, RewriteUndoPending, RewriteUndoVerb,
};
use turbogit_app::{OwnedOperation, integrate_service};
use turbogit_domain::model::{Commit, RootId};

use crate::bridge::AppHandle;
use crate::screens::short;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::input::{InputProps, text_area};

const REPLAYED_ROWS: usize = 12;
const UNDO_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);

/// Which rewrite a gate or a preflight is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RewriteVerb {
    Drop,
    Reword,
    Interactive,
}

/// How many repositories a rewrite's selection spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Own,
    Several,
    External,
}

pub fn scope_of(selection: &HashSet<RootId>, root: &RootId) -> Scope {
    if selection.is_empty() {
        return Scope::Own;
    }
    if selection.len() > 1 {
        return Scope::Several;
    }
    if selection.contains(root) {
        Scope::Own
    } else {
        Scope::External
    }
}

/// The facts the gate decides on, gathered by the caller.
#[derive(Clone, Copy, Debug)]
pub struct Facts {
    pub owner: Option<OwnedOperation>,
    pub on_current_branch: bool,
    pub protected_branch: bool,
    pub scope: Scope,
}
/// Why a rewrite cannot start, in the one sentence that wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    Owned(OwnedOperation),
    MergeCommit,
    RootCommit,
    OffCurrentBranch,
    SeveralInScope,
    OutsideScope,
    Protected,
}

mod text {
    pub(super) const CHERRY_PICK: &str = "Cannot rewrite history during a cherry-pick";
    pub(super) const REBASE: &str = "Cannot rewrite history during a rebase";
    pub(super) const MERGE: &str = "Cannot rewrite history during a merge";
    pub(super) const REVERT: &str = "Cannot rewrite history during a revert";
    pub(super) const MERGE_DROP: &str = "a merge commit cannot be dropped";
    pub(super) const MERGE_REWORD: &str = "a merge commit cannot be reworded";
    pub(super) const MERGE_REBASE: &str = "a merge commit cannot be rebased";
    pub(super) const ROOT_DROP: &str = "a root commit cannot be dropped";
    pub(super) const ROOT_REWORD: &str = "a root commit cannot be reworded";
    pub(super) const ROOT_REBASE: &str = "a root commit cannot be rebased";
    pub(super) const OFF_BRANCH: &str = "not on the current branch";
    pub(super) const SEVERAL: &str = "the rewrite runs in one repository; several are in scope";
    pub(super) const EXTERNAL: &str = "the commit is in a repository outside the ones in scope";
    pub(super) const PROTECTED: &str = "the current branch is protected";
}

impl Block {
    pub fn stated(self, verb: RewriteVerb) -> &'static str {
        match self {
            Self::Owned(OwnedOperation::CherryPick) => text::CHERRY_PICK,
            Self::Owned(OwnedOperation::Rebase) => text::REBASE,
            Self::Owned(OwnedOperation::Merge) => text::MERGE,
            Self::Owned(OwnedOperation::Revert) => text::REVERT,
            Self::MergeCommit => match verb {
                RewriteVerb::Drop => text::MERGE_DROP,
                RewriteVerb::Reword => text::MERGE_REWORD,
                RewriteVerb::Interactive => text::MERGE_REBASE,
            },
            Self::RootCommit => match verb {
                RewriteVerb::Drop => text::ROOT_DROP,
                RewriteVerb::Reword => text::ROOT_REWORD,
                RewriteVerb::Interactive => text::ROOT_REBASE,
            },
            Self::OffCurrentBranch => text::OFF_BRANCH,
            Self::SeveralInScope => text::SEVERAL,
            Self::OutsideScope => text::EXTERNAL,
            Self::Protected => text::PROTECTED,
        }
    }
}

/// The gate's one reason, in priority order: the owner first, then the
/// commit's shape, then where it sits, then the repository's policy.
pub fn blocked(commit: &Commit, facts: &Facts) -> Option<Block> {
    if let Some(owner) = facts.owner {
        return Some(Block::Owned(owner));
    }
    if commit.parents.len() > 1 {
        return Some(Block::MergeCommit);
    }
    if commit.parents.is_empty() {
        return Some(Block::RootCommit);
    }
    if !facts.on_current_branch {
        return Some(Block::OffCurrentBranch);
    }
    match facts.scope {
        Scope::Several => return Some(Block::SeveralInScope),
        Scope::External => return Some(Block::OutsideScope),
        Scope::Own => {}
    }
    if facts.protected_branch {
        return Some(Block::Protected);
    }
    None
}

/// The one sentence a computed caution paints.
pub fn caution_line(caution: &history_editor::RebaseCaution) -> String {
    match caution {
        history_editor::RebaseCaution::ConflictRisk { files } => {
            if *files == 1 {
                "Conflicts likely on 1 file".to_string()
            } else {
                format!("Conflicts likely on {files} files")
            }
        }
        history_editor::RebaseCaution::MixedIdentities { authors } => {
            format!("Mixed committer identities ({authors} authors)")
        }
    }
}

/// The layer: the reword editor, the preflight, and the undo offer.
pub fn layer(handle: AppHandle) -> impl IntoElement {
    let show = handle.clone();
    let body = handle.clone();
    when(
        move || {
            show.read(|s| {
                matches!(
                    s.ui.dialog,
                    Some(
                        turbogit_app::state::Dialog::RewritePreflight
                            | turbogit_app::state::Dialog::Reword
                    )
                )
            })
        },
        move || {
            let body_handle = body.clone();
            dynamic(move || {
                body_handle.read(|_| ());
                let which = body_handle.peek(|s| s.ui.dialog);
                match which {
                    Some(turbogit_app::state::Dialog::RewritePreflight) => {
                        preflight_dialog(body_handle.clone()).into_element()
                    }
                    Some(turbogit_app::state::Dialog::Reword) => {
                        reword_dialog(body_handle.clone()).into_element()
                    }
                    _ => view().into_element(),
                }
            })
        },
    )
}

fn window_title(verb: &HistoryVerb) -> &'static str {
    match verb {
        HistoryVerb::Drop => "Drop commit",
        HistoryVerb::Reword { .. } => "Reword commit",
    }
}

fn group_title(verb: &HistoryVerb) -> &'static str {
    match verb {
        HistoryVerb::Drop => "DROPPED COMMIT",
        HistoryVerb::Reword { .. } => "REWORDED COMMIT",
    }
}

fn counts_badge(verb: &HistoryVerb, preview: &PlanPreview) -> String {
    match verb {
        HistoryVerb::Drop => format!("{} → {} COMMITS", preview.before, preview.after),
        HistoryVerb::Reword { .. } => {
            let kept = preview.kept.len();
            if kept == 1 {
                "1 COMMIT REWRITTEN".to_string()
            } else {
                format!("{kept} COMMITS REWRITTEN")
            }
        }
    }
}

fn confirm_label(verb: &HistoryVerb) -> &'static str {
    match verb {
        HistoryVerb::Drop => "Drop this commit",
        HistoryVerb::Reword { .. } => "Reword this commit",
    }
}

fn close(handle: &AppHandle) {
    handle.update(|s| {
        s.ui.dialog = None;
        s.ui.dlg.rewrite_preflight = None;
        s.ui.dlg.reword = None;
        s.ui.dlg.reword_message.clear();
    });
}

pub fn preflight_dialog(handle: AppHandle) -> impl IntoElement {
    let Some(pf) = handle.peek(|s| s.ui.dlg.rewrite_preflight.clone()) else {
        return view().into_element();
    };
    let t = theme();
    let preview = history_editor::plan_preview(&pf.plan);
    let subject = pf
        .plan
        .iter()
        .find(|e| e.commit == pf.commit)
        .map(|e| e.subject.clone())
        .unwrap_or_default();
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::rewrite("preflight"))
        .child(
            text(group_title(&pf.verb))
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text(short(&pf.commit))
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.foreground),
                )
                .child(
                    text(subject)
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.foreground)
                        .flex_1(),
                ),
        );
    if let Some(new_subject) = pf.verb.reworded_subject() {
        body = body.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text("→")
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground),
                )
                .child(
                    text(new_subject)
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.foreground),
                ),
        );
    }
    body = body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::CELL_GAP))
            .child(
                text(format!("REPLAYED COMMITS · {}", preview.kept.len()))
                    .text_size(px(data::TYPE_SECTION))
                    .text_color(t.muted_foreground)
                    .flex_1(),
            )
            .child(
                text(counts_badge(&pf.verb, &preview))
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.primary),
            ),
    );
    let kept = &preview.kept;
    let elided = kept.len().saturating_sub(REPLAYED_ROWS);
    let head = REPLAYED_ROWS / 2;
    let shown: Vec<&turbogit_domain::model::RebasePlanEntry> = if elided == 0 {
        kept.iter().collect()
    } else {
        kept.iter()
            .take(head)
            .chain(kept.iter().skip(kept.len() - (REPLAYED_ROWS - head)))
            .collect()
    };
    for (index, entry) in shown.iter().enumerate() {
        if elided > 0 && index == head {
            body = body.child(
                text(format!("… {elided} commits in between …"))
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
            );
        }
        body = body.child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::CELL_GAP))
                .child(
                    text(short(&entry.commit))
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.muted_foreground),
                )
                .child(
                    text(entry.subject.clone())
                        .text_size(px(data::TYPE_CONTROL))
                        .text_color(t.foreground),
                ),
        );
    }
    for caution in &pf.cautions {
        body = body.child(
            text(caution_line(caution))
                .text_size(px(data::TYPE_CHIP))
                .text_color(data::STATE_WARNING),
        );
    }
    body = body.child(
        text(format!(
            "Recovery: {}",
            handle.peek(|s| s.rewrite_backup_ref())
        ))
        .text_size(px(data::TYPE_CHIP))
        .text_color(t.muted_foreground),
    );
    let estimate = history_editor::estimate(&pf.plan);
    body = body.child(
        text(estimate)
            .text_size(px(data::TYPE_CHIP))
            .text_color(t.muted_foreground),
    );

    let cancel_handle = handle.clone();
    let confirm_handle = handle.clone();
    dialog(
        DialogProps::new("rewrite-preflight", window_title(&pf.verb))
            .on_cancel(move || close(&cancel_handle))
            .child(body)
            .action(confirm_label(&pf.verb))
            .on_action(move || {
                dispatch_preflight(&confirm_handle);
                close(&confirm_handle);
            }),
    )
    .into_element()
}

fn dispatch_preflight(handle: &AppHandle) {
    let Some(pf) = handle.peek(|s| s.ui.dlg.rewrite_preflight.clone()) else {
        return;
    };
    let branch = handle.peek(|s| {
        s.multi
            .by_id(&pf.root)
            .and_then(|r| r.current_branch.clone())
            .unwrap_or_default()
    });
    let settings = handle.peek(|s| s.settings.clone());
    let anchor = match &pf.verb {
        HistoryVerb::Drop => RewrittenAnchor::Dropped,
        HistoryVerb::Reword { message } => RewrittenAnchor::Reworded {
            subject: subject_of_message(message).to_owned(),
        },
    };
    let verb = match &pf.verb {
        HistoryVerb::Drop => RewriteUndoVerb::Drop,
        HistoryVerb::Reword { .. } => RewriteUndoVerb::Reword,
    };
    let operation = match &pf.verb {
        HistoryVerb::Drop => Operation::drop_commit(&pf.root, &branch, &pf.commit, &settings),
        HistoryVerb::Reword { message } => {
            Operation::reword_commit(&pf.root, &branch, &pf.commit, message, &settings)
        }
    };
    handle.update(|s| {
        s.arm_reselect_after_rewrite(pf.root.clone(), pf.plan.clone(), pf.commit.clone(), anchor);
        s.ui.rewrite_undo_pending = Some(RewriteUndoPending {
            root: pf.root.clone(),
            verb,
            commit: pf.commit.clone(),
        });
        s.dispatch(operation);
    });
}

pub fn reword_dialog(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let Some(draft) = handle.peek(|s| s.ui.dlg.reword.clone()) else {
        return view().into_element();
    };
    let message = handle.peek(|s| s.ui.dlg.reword_message.clone());
    let change_handle = handle.clone();
    let field = text_area(
        InputProps::new()
            .multiline(true)
            .default_value(message.clone())
            .placeholder("New commit message")
            .accessibility_label("REWORD-MESSAGE")
            .test_id(&test_ids::rewrite("reword-message"))
            .on_change(move |text: &str| {
                let text = text.to_string();
                change_handle.update(move |s| s.ui.dlg.reword_message = text);
            }),
    );
    let blank = message.trim().is_empty();
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::rewrite("reword"))
        .child(
            text(short(&draft.commit))
                .text_size(px(data::TYPE_BODY))
                .text_color(t.muted_foreground),
        )
        .child(field)
        .child(
            text("The first line is the subject; the rest is the body.")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        );
    if blank {
        body = body.child(
            text(EMPTY_COMMIT_MESSAGE)
                .text_size(px(data::TYPE_CHIP))
                .text_color(data::STATE_WARNING),
        );
    }
    let cancel_handle = handle.clone();
    let confirm_handle = handle.clone();
    dialog(
        DialogProps::new("reword", "Reword commit")
            .on_cancel(move || close(&cancel_handle))
            .child(body)
            .action("Reword this commit")
            .on_action(move || {
                let (blank, message) = confirm_handle.peek(|s| {
                    (
                        s.ui.dlg.reword_message.trim().is_empty(),
                        s.ui.dlg.reword_message.trim_end().to_owned(),
                    )
                });
                if blank {
                    return;
                }
                let root = draft.root.clone();
                let commit = draft.commit.clone();
                confirm_handle.update(move |s| {
                    s.ui.dialog = None;
                    s.ui.dlg.reword = None;
                    s.open_rewrite_preflight(&root, &commit, HistoryVerb::Reword { message });
                });
            }),
    )
    .into_element()
}

/// The undo offer the shell paints under the activity feed.
pub fn undo_banner(handle: AppHandle) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || show.read(|s| s.ui.rewrite_undo.is_some()),
        move || {
            let paint = handle.clone();
            dynamic(move || {
                let Some(offer) = paint.peek(|s| s.ui.rewrite_undo.clone()) else {
                    return view().into_element();
                };
                if offer.created_at.elapsed() > UNDO_WINDOW {
                    return view().into_element();
                }
                undo_offer(paint.clone(), offer).into_element()
            })
        },
    )
}

fn success_title(verb: RewriteUndoVerb, commit: &str) -> String {
    match verb {
        RewriteUndoVerb::Drop => format!("Dropped commit {}", short(commit)),
        RewriteUndoVerb::Reword => "Commit message changed".to_string(),
    }
}

fn cannot_undo(verb: RewriteUndoVerb) -> &'static str {
    match verb {
        RewriteUndoVerb::Drop => "Cannot undo drop — the repository has changed since",
        RewriteUndoVerb::Reword => {
            "Cannot undo commit message edit — the repository has changed since"
        }
    }
}

fn undo_offer(handle: AppHandle, offer: RewriteUndo) -> impl IntoElement {
    let t = theme();
    let mut strip = view()
        .flex_row()
        .items_center()
        .gap(px(data::CELL_GAP))
        .p(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
        .rounded(px(data::CONTROL_RADIUS))
        .bg(t.card)
        .border(px(1.0))
        .border_color(t.border)
        .test_id(&test_ids::rewrite("undo"));
    if offer.withheld {
        strip = strip.child(
            text(cannot_undo(offer.verb))
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        );
    } else {
        let press = handle.clone();
        let undo_offer = offer.clone();
        strip = strip
            .child(
                text(success_title(offer.verb, &offer.commit))
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.foreground)
                    .flex_1(),
            )
            .child(button(
                ButtonProps::new("Undo")
                    .variant(ButtonVariant::Primary)
                    .test_id(&test_ids::rewrite("undo-action"))
                    .on_press(move || {
                        let offer = undo_offer.clone();
                        press.update(move |s| {
                            s.ui.rewrite_undo = None;
                            s.dispatch(shell_writes::rewrite::undo(
                                &offer.root,
                                offer.verb,
                                &offer.tip_sha,
                            ));
                        });
                    }),
            ));
    }
    strip
}

/// The rewrite route: the gate's vocabulary over the workspace, and the
/// two verbs that open the reword editor and the preflight.
pub fn rewrite_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let paint = handle.clone();
    dynamic(move || {
        paint.read(|_| ());
        let paint = paint.clone();
        let root = paint.peek(|s| s.multi.roots.first().map(|r| r.id.clone()));
        let head = root
            .as_ref()
            .and_then(|root| paint.peek(|s| s.recent_commits(root, 1)))
            .and_then(|commits| commits.first().cloned());
        let held = root
            .as_ref()
            .and_then(|root| paint.peek(|s| s.holding_operation(root.as_path())));
        let protected = root
            .as_ref()
            .is_some_and(|root| paint.peek(|s| s.rebase_branch_is_protected(root)));
        let reason = head.as_ref().and_then(|commit| {
            blocked(
                commit,
                &Facts {
                    owner: held,
                    on_current_branch: true,
                    protected_branch: protected,
                    scope: Scope::Own,
                },
            )
        });
        let mut body = view()
            .flex_1()
            .flex_col()
            .gap(px(data::PANEL_PADDING))
            .bg(t.background)
            .p(px(data::PANEL_PADDING))
            .test_id(&test_ids::route("rewrite"))
            .child(
                text("Rewrite history")
                    .text_size(px(data::TYPE_PANE_TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.foreground),
            )
            .child(
                text(match &head {
                    Some(commit) => format!("{}  {}", short(&commit.id), commit.subject()),
                    None => "No commit is loaded for the first repository.".to_string(),
                })
                .text_size(px(data::TYPE_BODY))
                .text_color(t.foreground),
            );
        if let Some(block) = reason {
            body = body.child(
                text(block.stated(RewriteVerb::Drop))
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(data::STATE_WARNING)
                    .test_id(&test_ids::rewrite("gate")),
            );
        } else if let (Some(root), Some(commit)) = (root.clone(), head.clone()) {
            let reword_handle = paint.clone();
            let drop_handle = paint.clone();
            let reword_root = root.clone();
            let reword_commit = commit.id.clone();
            let drop_root = root.clone();
            let drop_commit = commit.id.clone();
            body = body
                .child(button(
                    ButtonProps::new("Reword commit…")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::rewrite("reword-open"))
                        .on_press(move || {
                            reword_handle.update(|s| {
                                if s.caches.log(&reword_root).is_none()
                                    && let Some(commits) = s.recent_commits(&reword_root, 50)
                                {
                                    s.caches.store_log(reword_root.clone(), commits);
                                }
                                s.open_reword_editor(&reword_root, &reword_commit);
                            });
                        }),
                ))
                .child(button(
                    ButtonProps::new("Drop commit…")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::rewrite("drop-open"))
                        .on_press(move || {
                            drop_handle.update(|s| {
                                s.open_rewrite_preflight(
                                    &drop_root,
                                    &drop_commit,
                                    HistoryVerb::Drop,
                                )
                            });
                        }),
                ));
        }
        if let Some(offer) = paint.peek(|s| {
            s.ui.rewrite_undo
                .clone()
                .filter(|o| o.created_at.elapsed() <= UNDO_WINDOW)
        }) {
            body = body.child(undo_offer(paint.clone(), offer));
        }
        body
    })
}

/// The repository a rewrite dialog was opened for, or the first root.
pub fn rewrite_root(state: &AppState) -> Option<RootId> {
    state.ui.dlg.repo_scope.first().cloned()
}

/// Whether the repository is running one of the operations that owns the
/// working tree.
pub fn operation_in_progress(held: Option<OwnedOperation>) -> bool {
    integrate_service::operation_in_progress(held)
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbogit_domain::model::{Signature, SignatureState};

    fn commit(parents: usize) -> Commit {
        let who = Signature {
            name: "Author".into(),
            email: "author@example.com".into(),
            time: 1,
        };
        Commit {
            id: "0123456789abcdef0123456789abcdef01234567".into(),
            parents: (0..parents).map(|n| format!("{n}123")).collect(),
            author: who.clone(),
            committer: who,
            message: "subject".into(),
            time: 1,
            root: RootId(std::path::Path::new("/ws/alpha").into()),
            signature: SignatureState::Unsigned,
        }
    }

    fn facts() -> Facts {
        Facts {
            owner: None,
            on_current_branch: true,
            protected_branch: false,
            scope: Scope::Own,
        }
    }

    #[test]
    fn an_owned_operation_wins_over_every_other_reason() {
        let mut blocked_facts = facts();
        blocked_facts.owner = Some(OwnedOperation::Revert);
        blocked_facts.protected_branch = true;
        assert_eq!(
            blocked(&commit(2), &blocked_facts).map(|b| b.stated(RewriteVerb::Drop)),
            Some("Cannot rewrite history during a revert")
        );
    }

    #[test]
    fn a_merge_commit_states_its_verb_specific_reason() {
        assert_eq!(
            blocked(&commit(2), &facts()).map(|b| b.stated(RewriteVerb::Drop)),
            Some("a merge commit cannot be dropped")
        );
        assert_eq!(
            blocked(&commit(2), &facts()).map(|b| b.stated(RewriteVerb::Reword)),
            Some("a merge commit cannot be reworded")
        );
        assert_eq!(
            blocked(&commit(2), &facts()).map(|b| b.stated(RewriteVerb::Interactive)),
            Some("a merge commit cannot be rebased")
        );
    }

    #[test]
    fn the_scope_and_the_branch_policy_are_the_last_two_reasons() {
        let mut several = facts();
        several.scope = Scope::Several;
        assert_eq!(
            blocked(&commit(1), &several).map(|b| b.stated(RewriteVerb::Drop)),
            Some("the rewrite runs in one repository; several are in scope")
        );
        let mut external = facts();
        external.scope = Scope::External;
        assert_eq!(
            blocked(&commit(1), &external).map(|b| b.stated(RewriteVerb::Drop)),
            Some("the commit is in a repository outside the ones in scope")
        );
        let mut protected = facts();
        protected.protected_branch = true;
        assert_eq!(
            blocked(&commit(1), &protected).map(|b| b.stated(RewriteVerb::Drop)),
            Some("the current branch is protected")
        );
    }

    #[test]
    fn an_ordinary_commit_is_not_blocked() {
        assert_eq!(blocked(&commit(1), &facts()), None);
    }
}

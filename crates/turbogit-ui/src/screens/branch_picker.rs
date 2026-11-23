//! The top-bar branch picker: a popover with the current branch pinned, a
//! searchable grouped list of locals, remotes, and tags, and the footer
//! verbs. A choice checks out and closes; Escape closes.

use nsh_base::{CommandItem, theme};
use nsh_core::prelude::*;
use turbogit_app::state::{AppState, NewBranchBase};
use turbogit_domain::model::{BranchKind, Root, RootId};

use crate::bridge::AppHandle;
use crate::screens::branches::checkout_branch;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::command::{CommandProps, command};
use crate::ui::popover::{PopoverProps, popover};

/// One picker row: the command's item and what choosing it does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Local { name: String },
    Remote { remote: String, name: String },
    Tag { name: String },
}

impl Entry {
    fn id(&self) -> String {
        match self {
            Entry::Local { name } => format!("local:{name}"),
            Entry::Remote { remote, name } => format!("remote:{remote}:{name}"),
            Entry::Tag { name } => format!("tag:{name}"),
        }
    }

    fn label(&self) -> String {
        match self {
            Entry::Local { name } | Entry::Tag { name } => name.clone(),
            Entry::Remote { remote, name } => format!("{remote}/{name}"),
        }
    }

    fn group(&self) -> &'static str {
        match self {
            Entry::Local { .. } => "LOCAL",
            Entry::Remote { .. } => "REMOTE",
            Entry::Tag { .. } => "TAGS",
        }
    }
}

/// The root the picker acts on: the selected change row's root, else the
/// first registered root, the workspace's own default.
pub fn picker_root(state: &AppState) -> Option<RootId> {
    state
        .action_root()
        .or_else(|| state.multi.roots.first().map(|root| root.id.clone()))
}

/// Flattened rows: locals without the current (pinned separately), favorites
/// first then alphabetical; remotes with the tracking rows a local carries;
/// tags sorted.
pub fn entries(state: &AppState, root: &Root) -> Vec<Entry> {
    let current = root.current_branch.as_deref();
    let mut locals: Vec<&turbogit_domain::model::Branch> = root
        .branches
        .iter()
        .filter(|b| b.kind == BranchKind::Local && Some(b.name.as_str()) != current)
        .collect();
    locals.sort_by(|a, b| {
        b.favorite
            .cmp(&a.favorite)
            .then_with(|| a.name.cmp(&b.name))
    });
    let mut out: Vec<Entry> = locals
        .iter()
        .map(|b| Entry::Local {
            name: b.name.clone(),
        })
        .collect();

    let mut remote_rows: Vec<(String, String)> = root
        .branches
        .iter()
        .filter(|b| b.kind == BranchKind::Remote)
        .map(|b| {
            (
                b.remote.clone().unwrap_or_else(|| "origin".to_string()),
                b.name.clone(),
            )
        })
        .collect();
    for local in &root.branches {
        let Some(track) = &local.tracking else {
            continue;
        };
        if track.remote == "." {
            continue;
        }
        if !remote_rows.iter().any(|(_, name)| name == &track.branch) {
            remote_rows.push((track.remote.clone(), track.branch.clone()));
        }
    }
    remote_rows.sort_by(|a, b| a.1.cmp(&b.1));
    out.extend(
        remote_rows
            .into_iter()
            .map(|(remote, name)| Entry::Remote { remote, name }),
    );

    let mut tags: Vec<String> = state
        .caches
        .tags(&root.id)
        .map(|tags| tags.iter().map(|(name, _)| name.clone()).collect())
        .unwrap_or_default();
    if tags.is_empty() {
        tags = state.tag_names(&root.id).unwrap_or_default();
    }
    tags.sort();
    out.extend(tags.into_iter().map(|name| Entry::Tag { name }));
    out
}

fn current_label(handle: &AppHandle) -> String {
    handle.read(|s| {
        picker_root(s)
            .as_ref()
            .and_then(|root| s.multi.by_id(root))
            .and_then(|root| root.current_branch.clone())
            .unwrap_or_else(|| "Branches".to_string())
    })
}

/// The picker's trigger and popover, the shell's top-bar control.
pub fn picker_trigger(handle: AppHandle) -> impl IntoElement {
    let label_handle = handle.clone();
    let text_handle = handle.clone();
    let open = signal(false);
    let label = move || current_label(&label_handle);
    let label_text = move || current_label(&text_handle);
    let t = theme();
    let trigger = view()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
        .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
        .rounded(px(data::CONTROL_RADIUS))
        .bg(t.secondary)
        .role(AccessibilityRole::Button)
        .label(move || SharedString::from(format!("Branch: {}", label())))
        .test_id(&test_ids::picker("trigger"))
        .child(
            text("⑂")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        )
        .child(text(move || SharedString::from(label_text())).text_size(px(data::TYPE_CONTROL)));
    let body_handle = handle.clone();
    let open_write = open;
    let popover_handle = handle.clone();
    popover(
        PopoverProps::new(trigger)
            .width(420.0)
            .test_id(&test_ids::picker("popover"))
            .open(Prop::bind(move || Some(open.get())))
            .on_open_change(move |next| {
                open_write.set(next);
                popover_handle.update(move |s| s.ui.branches_popup = next);
            })
            .child(picker_body(body_handle, open)),
    )
}

fn picker_body(handle: AppHandle, open: Signal<bool>) -> impl IntoElement {
    let t = theme();
    let root = handle.peek(picker_root);
    let mut body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::picker("body"));
    let Some(root_id) = root.clone() else {
        body = body.child(
            text("Select a change to choose its repository.")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        );
        return body;
    };
    let Some(repo) = handle.peek(|s| s.multi.by_id(&root_id).cloned()) else {
        body = body.child(
            text("Select a change to choose its repository.")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        );
        return body;
    };
    if let Some(current) = &repo.current_branch {
        body = body.child(
            text(format!("✓ {current}"))
                .text_size(px(data::TYPE_CONTROL))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(data::ACCENT_TEXT)
                .test_id(&test_ids::picker("current")),
        );
    }
    let entries = handle.peek(|s| entries(s, &repo));
    let items: Vec<CommandItem> = entries
        .iter()
        .map(|entry| CommandItem::new(entry.id(), entry.label()).group(entry.group()))
        .collect();
    let select_handle = handle.clone();
    let close = open;
    let list = command(
        CommandProps::new(items)
            .placeholder("Search branches")
            .accessibility_label("Branch picker")
            .search_accessibility_label("Search branches")
            .test_id(&test_ids::picker("list"))
            .on_query_change({
                let handle = handle.clone();
                move |text: &str| {
                    let text = text.to_string();
                    handle.update(move |s| s.ui.branch_filter = text);
                }
            })
            .on_cancel(move || {
                close.set(false);
                select_handle.update(|s| s.ui.branches_popup = false);
            })
            .on_select({
                let handle = handle.clone();
                move |id: &str| {
                    let id = id.to_string();
                    handle.update(move |s| choose(s, &id));
                    close.set(false);
                }
            }),
    );
    body = body.child(list);
    if let Some(notice) = handle.peek(sync_notice) {
        body = body.child(
            text(notice)
                .text_size(px(data::TYPE_CHIP))
                .text_color(data::STATE_INFO),
        );
    }
    let new_handle = handle.clone();
    let remotes_handle = handle.clone();
    body.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::SPACING_X))
            .child(button(
                ButtonProps::new("+ New branch")
                    .variant(ButtonVariant::Secondary)
                    .test_id(&test_ids::picker("new"))
                    .on_press(move || {
                        new_handle.update(|s| {
                            s.open_new_branch(None, NewBranchBase::Head, true, None);
                            s.ui.branches_popup = false;
                        });
                    }),
            ))
            .child(button(
                ButtonProps::new("Manage remotes…")
                    .variant(ButtonVariant::Ghost)
                    .test_id(&test_ids::picker("remotes"))
                    .on_press(move || {
                        remotes_handle.update(|s| {
                            s.ui.dlg.remotes_row_action = None;
                            s.ui.dlg.remotes_apply_results = None;
                            s.ui.dlg.remotes_apply_scope.clear();
                            s.open_dialog(turbogit_app::state::Dialog::ManageRemotes, None);
                            s.ui.branches_popup = false;
                        });
                    }),
            )),
    )
}

/// Checks the row out: the local branch with its preflights, the remote as a
/// new local, the tag detached.
pub fn choose(state: &mut AppState, id: &str) {
    let Some(root_id) = picker_root(state) else {
        return;
    };
    let Some(repo) = state.multi.by_id(&root_id).cloned() else {
        return;
    };
    state.ui.branches_popup = false;
    if let Some(name) = id.strip_prefix("tag:") {
        state.checkout_detached_op(&root_id, name);
        return;
    }
    if let Some(rest) = id.strip_prefix("local:") {
        if let Some(branch) = repo
            .branches
            .iter()
            .find(|b| b.name == rest && b.kind == BranchKind::Local)
            .cloned()
        {
            checkout_branch(state, &root_id, &branch);
        }
        return;
    }
    if let Some(rest) = id.strip_prefix("remote:") {
        let name = rest.split_once(':').map_or(rest, |(_, name)| name);
        state.checkout_branch_op(&root_id, BranchKind::Remote, name);
    }
}

/// The picker route: the list inline, without the popover.
pub fn picker_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let open = signal(false);
    let list_handle = handle.clone();
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::PANEL_PADDING))
        .bg(t.background)
        .p(px(data::PANEL_PADDING))
        .test_id(&test_ids::route("branch_picker"))
        .child(
            text("Branch picker")
                .text_size(px(data::TYPE_PANE_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(
            text("The top bar's picker lists locals, remotes, and tags with the current branch pinned; the same list is below.")
                .text_size(px(data::TYPE_CHIP))
                .text_color(t.muted_foreground),
        )
        .child(
            view()
                .flex_col()
                .gap(px(data::SPACING_Y))
                .rounded(px(data::CARD_RADIUS))
                .border(px(1.0))
                .border_color(t.border)
                .bg(t.card)
                .p(px(data::PANEL_PADDING))
                .child(picker_body(list_handle, open)),
        )
}

/// A local branch row's display name, remote-qualified where a remote names
/// it: one spelling shared with the picker's labels.
pub fn qualified_label(branch: &turbogit_domain::model::Branch) -> String {
    match (&branch.remote, branch.kind) {
        (Some(_), BranchKind::Remote) => branch.name.clone(),
        _ => branch.name.clone(),
    }
}

/// The sync notice a multi-root workspace shows under the list.
pub fn sync_notice(state: &AppState) -> Option<String> {
    (state.multi.roots.len() > 1 && state.settings.synchronous_branches).then(|| {
        format!(
            "Synchronous branch operations across {} repositories",
            state.multi.roots.len()
        )
    })
}

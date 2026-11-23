//! Settings: a draft of the workspace settings over seven categories, with
//! Apply, Cancel, and a per-category Restore defaults. Apply is the one write
//! path: it persists through `turbogit_app::persistence`, rebuilds the
//! executor, and keeps the dialog open.
//!
//! The draft is a signal the page reads; a text field edits it without
//! echoing state on every keystroke, so the uncontrolled input keeps the
//! text the typist is entering. The page's structure rebuilds only when the
//! category or a restore changes.

use nsh_base::{SelectItem, theme};
use nsh_core::prelude::*;
use turbogit_app::state::{AppState, SettingsCategory, Toast};
use turbogit_domain::model::{
    CleanTreeMethod, DateFormat, IncomingCheckInterval, PatchCreatedPolicy, UpdateMethod,
    VcsSettings,
};

use crate::bridge::AppHandle;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::input::{InputProps, input};
use crate::ui::select::{SelectProps, select};
use crate::ui::switch::{SwitchProps, switch};
use crate::ui::tab::{TabItem, TabsProps, tabs};

const CATEGORIES: [SettingsCategory; 7] = [
    SettingsCategory::General,
    SettingsCategory::Git,
    SettingsCategory::UpdateMethod,
    SettingsCategory::MultiRoot,
    SettingsCategory::ProtectedBranches,
    SettingsCategory::Appearance,
    SettingsCategory::Advanced,
];

const MODAL_WIDTH: f32 = 768.0;
const PANEL_HEIGHT: f32 = 420.0;

/// Opens the dialog over the current settings, the draft starting from the
/// saved values and the category on General.
pub fn open(handle: &AppHandle) {
    handle.update(|s| {
        if s.ui.settings_draft.is_none() {
            s.ui.settings_draft = Some(s.settings.clone());
            s.ui.settings_category = SettingsCategory::General;
        }
        s.ui.settings_open = true;
    });
}

fn seed(state: &mut AppState) {
    if state.ui.settings_open && state.ui.settings_draft.is_none() {
        state.ui.settings_draft = Some(state.settings.clone());
        state.ui.settings_category = SettingsCategory::General;
    }
}

fn close(handle: &AppHandle) {
    handle.update(|s| {
        s.ui.settings_open = false;
        s.ui.settings_draft = None;
    });
}

/// The dialog layer: the settings modal while `ui.settings_open`.
pub fn layer(handle: AppHandle) -> impl IntoElement {
    let show = handle.clone();
    when(
        move || show.read(|s| s.ui.settings_open),
        move || {
            let seeded = handle.clone();
            seeded.update(seed);
            let (body, draft, dirty) = settings_body(handle.clone());
            let apply = handle.clone();
            let cancel = handle.clone();
            let gate = dirty;
            dialog(
                DialogProps::new("settings", "Settings")
                    .width(MODAL_WIDTH)
                    .action("Apply")
                    .action_disabled_when(move || !gate.get())
                    .on_action(move || apply_settings(&apply, draft, dirty))
                    .on_cancel(move || close(&cancel))
                    .child(body),
            )
        },
    )
}

/// The route's screen: the same categories and controls inline, with the
/// dialog's actions in a footer row.
pub fn settings_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let (body, draft, dirty) = settings_body(handle.clone());
    let apply = handle.clone();
    let cancel = handle.clone();
    view()
        .flex_1()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .p(px(data::PANEL_PADDING))
        .bg(t.background)
        .test_id(&test_ids::route("settings"))
        .child(body)
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::SPACING_X))
                .child(button(
                    ButtonProps::new("Apply")
                        .variant(ButtonVariant::Primary)
                        .disabled_if(move || !dirty.get())
                        .test_id(&test_ids::settings("apply"))
                        .on_press(move || apply_settings(&apply, draft, dirty)),
                ))
                .child(button(
                    ButtonProps::new("Cancel")
                        .variant(ButtonVariant::Ghost)
                        .test_id(&test_ids::settings("cancel"))
                        .on_press(move || close(&cancel)),
                )),
        )
}

fn is_dirty(state: &AppState) -> bool {
    state
        .ui
        .settings_draft
        .as_ref()
        .is_some_and(|draft| *draft != state.settings)
}

fn category_slug(category: SettingsCategory) -> &'static str {
    match category {
        SettingsCategory::General => "general",
        SettingsCategory::Git => "git",
        SettingsCategory::UpdateMethod => "update-method",
        SettingsCategory::MultiRoot => "multi-root",
        SettingsCategory::ProtectedBranches => "protected-branches",
        SettingsCategory::Appearance => "appearance",
        SettingsCategory::Advanced => "advanced",
    }
}

/// The draft as it stands, read without subscribing the caller.
fn snapshot(draft: Signal<VcsSettings>) -> VcsSettings {
    nsh_core::element::reactive::untracked(|| draft.get())
}

/// One edit: the signal drives the controls, the state holds the draft Apply
/// reads, and the dirty signal drives the Apply gate.
fn commit(handle: &AppHandle, draft: Signal<VcsSettings>, dirty: Signal<bool>, next: VcsSettings) {
    draft.set(next.clone());
    let saved = handle.peek(|s| s.settings.clone());
    dirty.set(snapshot(draft) != saved);
    handle.update(move |s| s.ui.settings_draft = Some(next));
}

fn settings_body(handle: AppHandle) -> (AnyElement, Signal<VcsSettings>, Signal<bool>) {
    let t = theme();
    let category = signal(handle.peek(|s| s.ui.settings_category));
    let generation = signal(0u64);
    let draft = signal(
        handle
            .peek(|s| s.ui.settings_draft.clone())
            .unwrap_or_default(),
    );
    let dirty = signal(handle.peek(is_dirty));

    let items: Vec<TabItem> = CATEGORIES
        .iter()
        .map(|category| {
            TabItem::new(category.label()).test_id(&test_ids::settings(format!(
                "tab:{}",
                category_slug(*category)
            )))
        })
        .collect();
    let select_handle = handle.clone();
    let select_category = category;
    let strip = tabs(
        TabsProps::new(items, move || {
            CATEGORIES
                .iter()
                .position(|value| *value == select_category.get())
                .unwrap_or(0)
        })
        .on_change(move |at| {
            let Some(next) = CATEGORIES.get(at).copied() else {
                return;
            };
            select_category.set(next);
            let select_handle = select_handle.clone();
            select_handle.update(move |s| s.ui.settings_category = next);
        }),
    );

    let panel_handle = handle.clone();
    let panel_category = category;
    let panel_generation = generation;
    let panel_draft = draft;
    let panel_dirty = dirty;
    let panel = dynamic(move || {
        let category = panel_category.get();
        let _ = panel_generation.get();
        let restore_draft = panel_draft;
        let restore_handle = panel_handle.clone();
        let restore_dirty = panel_dirty;
        let restore_generation = panel_generation;
        let page = match category {
            SettingsCategory::General => general_page(&panel_handle, panel_draft, panel_dirty),
            SettingsCategory::Git => {
                git_page(&panel_handle, panel_draft, panel_dirty, panel_generation)
            }
            SettingsCategory::UpdateMethod => {
                update_method_page(&panel_handle, panel_draft, panel_dirty)
            }
            SettingsCategory::MultiRoot => multi_root_page(&panel_handle, panel_draft, panel_dirty),
            SettingsCategory::ProtectedBranches => {
                protected_branches_page(&panel_handle, panel_draft, panel_dirty, panel_generation)
            }
            SettingsCategory::Appearance => {
                appearance_page(&panel_handle, panel_draft, panel_dirty)
            }
            SettingsCategory::Advanced => advanced_page(&panel_handle, panel_draft, panel_dirty),
        };
        view()
            .flex_col()
            .gap(px(data::SPACING_Y))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .child(
                        text(category.label())
                            .text_size(px(data::TYPE_PANE_TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.card_foreground)
                            .flex_1(),
                    )
                    .child(button(
                        ButtonProps::new("Restore defaults")
                            .variant(ButtonVariant::Ghost)
                            .size(crate::ui::ControlSize::Compact)
                            .test_id(&test_ids::settings("restore-defaults"))
                            .on_press(move || {
                                let next = restore_defaults(restore_draft.get(), category);
                                commit(&restore_handle, restore_draft, restore_dirty, next);
                                restore_generation.update(|n| *n += 1);
                            }),
                    )),
            )
            .child(scroll().max_h(px(PANEL_HEIGHT)).child(page))
            .into_element()
    });

    let body = view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::settings("body"))
        .child(strip)
        .child(panel);
    (body.into_element(), draft, dirty)
}

fn restore_defaults(mut draft: VcsSettings, category: SettingsCategory) -> VcsSettings {
    let d = VcsSettings::default();
    match category {
        SettingsCategory::General => draft.restore_workspace = d.restore_workspace,
        SettingsCategory::Git => draft.git_executable = d.git_executable,
        SettingsCategory::UpdateMethod => {
            draft.update_method = d.update_method;
            draft.clean_tree_method = d.clean_tree_method;
            draft.incoming_poll = d.incoming_poll;
            draft.incoming_interval = d.incoming_interval;
        }
        SettingsCategory::MultiRoot => draft.synchronous_branches = d.synchronous_branches,
        SettingsCategory::ProtectedBranches => {
            draft.protected_branch_patterns = d.protected_branch_patterns
        }
        SettingsCategory::Appearance => {
            draft.date_format = d.date_format;
            draft.gutter_markers = d.gutter_markers;
        }
        SettingsCategory::Advanced => {
            draft.warn_crlf = d.warn_crlf;
            draft.warn_detached = d.warn_detached;
        }
    }
    draft
}

fn apply_settings(handle: &AppHandle, draft: Signal<VcsSettings>, dirty: Signal<bool>) {
    let next = snapshot(draft);
    handle.update(move |s| {
        s.settings = next.clone();
        s.ui.settings_draft = Some(next.clone());
        let _ = turbogit_app::persistence::save_settings(&s.project_dir, &s.settings);
        s.rebuild_executor();
        s.save_durable_ui();
        s.ui.toast = Some(Toast::success("Settings saved"));
    });
    dirty.set(false);
}

/// Asks the engine for `executable`'s version and stores the answer with the
/// path it answers for, the guard the badge reads.
fn refresh_version_for(handle: &AppHandle, executable: &str) {
    let executable = executable.to_string();
    handle.update(move |s| {
        if s.ui.git_version_checked_path.as_deref() == Some(executable.as_str()) {
            return;
        }
        let mut draft = s.settings.clone();
        draft.git_executable = executable.clone();
        s.ui.git_version_badge =
            Some(turbogit_app::resolve_git_version(&draft).map_err(|e| e.to_string()));
        s.ui.git_version_checked_path = Some(executable);
    });
}

fn general_page(handle: &AppHandle, draft: Signal<VcsSettings>, dirty: Signal<bool>) -> AnyElement {
    row(
        "Restore workspace context on branch switch",
        "Reopen the tabs and selections the last session left.",
        switch_control(
            handle,
            draft,
            dirty,
            "restore-workspace",
            "Restore workspace context on branch switch",
            |s, on| s.restore_workspace = on,
            |d| d.restore_workspace,
        ),
    )
}

fn git_page(
    handle: &AppHandle,
    draft: Signal<VcsSettings>,
    dirty: Signal<bool>,
    generation: Signal<u64>,
) -> AnyElement {
    let t = theme();
    let change = handle.clone();
    let change_draft = draft;
    refresh_version_for(handle, &snapshot(draft).git_executable);
    let version = dynamic({
        let handle = handle.clone();
        move || {
            handle.read(|_| ());
            let version = handle.peek(|s| s.ui.git_version_badge.clone());
            match version {
                Some(Ok(version)) => view()
                    .flex_row()
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        text(format!("{version} ✓"))
                            .text_size(px(data::TYPE_CHIP))
                            .text_color(data::STATE_SUCCESS),
                    )
                    .into_element(),
                Some(Err(reason)) => view()
                    .flex_row()
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        text("✗")
                            .text_size(px(data::TYPE_CHIP))
                            .text_color(data::STATE_ERROR),
                    )
                    .child(
                        text(reason)
                            .text_size(px(data::TYPE_CHIP))
                            .text_color(data::STATE_ERROR),
                    )
                    .into_element(),
                None => view().into_element(),
            }
        }
    });
    let mut page = view().flex_col().gap(px(data::SPACING_Y)).child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::SPACING_X))
            .child(
                view()
                    .flex_col()
                    .gap(px(2.0))
                    .flex_1()
                    .child(
                        text("Git executable")
                            .text_size(px(data::TYPE_BODY))
                            .text_color(t.foreground),
                    )
                    .child(
                        text("Leave empty to use the git found on PATH.")
                            .text_size(px(data::TYPE_CHIP))
                            .text_color(t.muted_foreground),
                    ),
            )
            .child(version)
            .child(button(
                ButtonProps::new("Browse")
                    .variant(ButtonVariant::Ghost)
                    .size(crate::ui::ControlSize::Compact)
                    .disabled(true)
                    .test_id(&test_ids::settings("git-browse")),
            )),
    );
    page = page.child(input(
        InputProps::new()
            .default_value(snapshot(draft).git_executable)
            .remount_on(generation)
            .accessibility_label("Git executable")
            .test_id(&test_ids::settings("git-executable"))
            .on_change(move |text: &str| {
                let mut next = snapshot(change_draft);
                next.git_executable = text.to_string();
                commit(&change, change_draft, dirty, next);
                refresh_version_for(&change, text);
            }),
    ));
    page = page.child(
        view()
            .flex_row()
            .items_center()
            .gap(px(data::SPACING_X))
            .child(
                view()
                    .role(AccessibilityRole::Button)
                    .label("Manage Remotes")
                    .test_id(&test_ids::settings("manage-remotes"))
                    .disabled(true)
                    .opacity(0.45)
                    .px(px(data::DENSITY_COMPACT_BUTTON_PAD_X))
                    .py(px(data::DENSITY_COMPACT_BUTTON_PAD_Y))
                    .rounded(px(data::CONTROL_RADIUS))
                    .bg(t.secondary)
                    .child(
                        text("Manage Remotes")
                            .text_size(px(data::TYPE_CONTROL))
                            .text_color(t.foreground),
                    ),
            )
            .child(
                text("The remote manager opens from the Branches tab.")
                    .text_size(px(data::TYPE_CHIP))
                    .text_color(t.muted_foreground),
            ),
    );
    page.into_element()
}

fn update_method_page(
    handle: &AppHandle,
    draft: Signal<VcsSettings>,
    dirty: Signal<bool>,
) -> AnyElement {
    let method_handle = handle.clone();
    let clean = handle.clone();
    let auto = handle.clone();
    let poll = handle.clone();
    let interval_handle = handle.clone();
    let interval_draft = draft;
    let value = snapshot(draft);
    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(setting_row(
            "Update method",
            "How incoming commits are integrated when you run Update.",
            select(
                SelectProps::new(vec![
                    SelectItem::new("merge", "Merge"),
                    SelectItem::new("rebase", "Rebase"),
                ])
                .default_value(match value.update_method {
                    UpdateMethod::Merge => "merge",
                    UpdateMethod::Rebase => "rebase",
                })
                .label(move || {
                    SharedString::from(match draft.get().update_method {
                        UpdateMethod::Merge => "Merge",
                        UpdateMethod::Rebase => "Rebase",
                    })
                })
                .accessibility_label("Update method")
                .test_id(&test_ids::settings("update-method"))
                .on_change(move |value: &SharedString| {
                    let mut next = snapshot(draft);
                    next.update_method = match value.as_str() {
                        "rebase" => UpdateMethod::Rebase,
                        _ => UpdateMethod::Merge,
                    };
                    commit(&method_handle, draft, dirty, next);
                }),
            )
            .into_element(),
        ))
        .child(row(
            "Auto-update on push rejection",
            "After a rejected push, update silently with the method above instead of asking.",
            switch_control(
                &auto,
                draft,
                dirty,
                "auto-update",
                "Update without asking",
                |s, on| s.auto_update_on_push_rejected = on,
                |d| d.auto_update_on_push_rejected,
            ),
        ))
        .child(setting_row(
            "Clean working tree",
            "Sweep uncommitted changes before an update runs.",
            select(
                SelectProps::new(vec![
                    SelectItem::new("stash", "Stash"),
                    SelectItem::new("shelve", "Shelve"),
                ])
                .default_value(match value.clean_tree_method {
                    CleanTreeMethod::Stash => "stash",
                    CleanTreeMethod::Shelve => "shelve",
                })
                .label(move || {
                    SharedString::from(match draft.get().clean_tree_method {
                        CleanTreeMethod::Stash => "Stash",
                        CleanTreeMethod::Shelve => "Shelve",
                    })
                })
                .accessibility_label("Clean working tree")
                .test_id(&test_ids::settings("clean-tree"))
                .on_change(move |value: &SharedString| {
                    let mut next = snapshot(draft);
                    next.clean_tree_method = match value.as_str() {
                        "shelve" => CleanTreeMethod::Shelve,
                        _ => CleanTreeMethod::Stash,
                    };
                    commit(&clean, draft, dirty, next);
                }),
            )
            .into_element(),
        ))
        .child(row(
            "Incoming check",
            "Poll remotes in the background and badge incoming commits in the workspace tree.",
            switch_control(
                &poll,
                draft,
                dirty,
                "incoming-poll",
                "Check for incoming commits",
                |s, on| s.incoming_poll = on,
                |d| d.incoming_poll,
            ),
        ))
        .child(dynamic(move || {
            let current = interval_draft.get();
            setting_row(
                "Check interval",
                "How often the background check runs.",
                select(
                    SelectProps::new(vec![
                        SelectItem::new("15", "Every 15 minutes"),
                        SelectItem::new("30", "Every 30 minutes"),
                        SelectItem::new("60", "Every hour"),
                    ])
                    .default_value(match current.incoming_interval {
                        IncomingCheckInterval::Min15 => "15",
                        IncomingCheckInterval::Min30 => "30",
                        IncomingCheckInterval::Min60 => "60",
                    })
                    .disabled(!current.incoming_poll)
                    .label({
                        let draft = interval_draft;
                        move || {
                            SharedString::from(match draft.get().incoming_interval {
                                IncomingCheckInterval::Min15 => "Every 15 minutes",
                                IncomingCheckInterval::Min30 => "Every 30 minutes",
                                IncomingCheckInterval::Min60 => "Every hour",
                            })
                        }
                    })
                    .accessibility_label("Check interval")
                    .test_id(&test_ids::settings("incoming-interval"))
                    .on_change({
                        let handle = interval_handle.clone();
                        move |value: &SharedString| {
                            let mut next = snapshot(interval_draft);
                            next.incoming_interval = match value.as_str() {
                                "15" => IncomingCheckInterval::Min15,
                                "30" => IncomingCheckInterval::Min30,
                                _ => IncomingCheckInterval::Min60,
                            };
                            commit(&handle, interval_draft, dirty, next);
                        }
                    }),
                )
                .into_element(),
            )
        }))
        .into_element()
}

fn multi_root_page(
    handle: &AppHandle,
    draft: Signal<VcsSettings>,
    dirty: Signal<bool>,
) -> AnyElement {
    row(
        "Sync branch operations across roots",
        "Check out, rename, and delete the matching branch in every repository.",
        switch_control(
            handle,
            draft,
            dirty,
            "synchronous-branches",
            "Sync branch operations across roots",
            |s, on| s.synchronous_branches = on,
            |d| d.synchronous_branches,
        ),
    )
}

fn protected_branches_page(
    handle: &AppHandle,
    draft: Signal<VcsSettings>,
    dirty: Signal<bool>,
    generation: Signal<u64>,
) -> AnyElement {
    let t = theme();
    let add = handle.clone();
    let add_draft = draft;
    let add_dirty = dirty;
    let add_generation = generation;
    let chips_handle = handle.clone();
    let chip_theme = t.clone();
    let chips = dynamic(move || {
        let t = chip_theme.clone();
        let patterns = add_draft.get().protected_branch_patterns;
        let mut chips = view().flex_row().items_center().gap(px(data::SPACING_X));
        for pattern in patterns {
            let remove = chips_handle.clone();
            let remove_draft = add_draft;
            let remove_dirty = add_dirty;
            let name = pattern.clone();
            let label = pattern.clone();
            chips = chips.child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(4.0))
                    .rounded(px(data::CONTROL_RADIUS))
                    .bg(t.secondary)
                    .px(px(6.0))
                    .py(px(2.0))
                    .child(
                        text(label)
                            .text_size(px(data::TYPE_BODY))
                            .text_color(t.foreground),
                    )
                    .child(
                        view()
                            .role(AccessibilityRole::Button)
                            .label(format!("Remove {name}"))
                            .test_id(&test_ids::settings(format!("pattern-remove:{name}")))
                            .px(px(2.0))
                            .on_press(move || {
                                let mut next = snapshot(remove_draft);
                                next.protected_branch_patterns.retain(|p| p != &name);
                                commit(&remove, remove_draft, remove_dirty, next);
                            })
                            .child(
                                text("×")
                                    .text_size(px(data::TYPE_CONTROL))
                                    .text_color(t.muted_foreground),
                            ),
                    ),
            );
        }
        chips.into_element()
    });
    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(
            text("Protected branch patterns")
                .text_size(px(data::TYPE_DETAIL_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.foreground),
        )
        .child(
            text("Cascading push, delete, and force operations refuse to touch matching branches.")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        )
        .child(chips)
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(data::SPACING_X))
                .child(input(
                    InputProps::new()
                        .placeholder("release/*")
                        .accessibility_label("New pattern")
                        .remount_on(add_generation)
                        .test_id(&test_ids::settings("new-pattern"))
                        .on_change({
                            let add = add.clone();
                            move |text: &str| {
                                let text = text.to_string();
                                add.update(move |s| s.ui.settings_new_pattern = text);
                            }
                        }),
                ))
                .child(button(
                    ButtonProps::new("+ Add pattern")
                        .variant(ButtonVariant::Secondary)
                        .size(crate::ui::ControlSize::Compact)
                        .test_id(&test_ids::settings("add-pattern"))
                        .on_press(move || {
                            let mut next = snapshot(add_draft);
                            let pattern =
                                add.peek(|s| s.ui.settings_new_pattern.trim().to_string());
                            if !pattern.is_empty()
                                && !next.protected_branch_patterns.contains(&pattern)
                            {
                                next.protected_branch_patterns.push(pattern);
                            }
                            commit(&add, add_draft, add_dirty, next);
                            add.update(|s| s.ui.settings_new_pattern.clear());
                            add_generation.update(|n| *n += 1);
                        }),
                )),
        )
        .into_element()
}

fn appearance_page(
    handle: &AppHandle,
    draft: Signal<VcsSettings>,
    dirty: Signal<bool>,
) -> AnyElement {
    let format = handle.clone();
    let gutter = handle.clone();
    let value = snapshot(draft);
    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(setting_row(
            "Date format",
            "Applies to the log, commit details, and every activity timestamp.",
            select(
                SelectProps::new(vec![
                    SelectItem::new("relative", "Relative"),
                    SelectItem::new("absolute", "Absolute"),
                    SelectItem::new("iso", "ISO"),
                ])
                .default_value(match value.date_format {
                    DateFormat::Relative => "relative",
                    DateFormat::Absolute => "absolute",
                    DateFormat::Iso => "iso",
                })
                .label(move || {
                    SharedString::from(match draft.get().date_format {
                        DateFormat::Relative => "Relative",
                        DateFormat::Absolute => "Absolute",
                        DateFormat::Iso => "ISO",
                    })
                })
                .accessibility_label("Date format")
                .test_id(&test_ids::settings("date-format"))
                .on_change(move |value: &SharedString| {
                    let mut next = snapshot(draft);
                    next.date_format = match value.as_str() {
                        "absolute" => DateFormat::Absolute,
                        "iso" => DateFormat::Iso,
                        _ => DateFormat::Relative,
                    };
                    commit(&format, draft, dirty, next);
                }),
            )
            .into_element(),
        ))
        .child(row(
            "Highlight modified lines in the gutter",
            "Mark changed lines in the diff's line-number column.",
            switch_control(
                &gutter,
                draft,
                dirty,
                "gutter-markers",
                "Highlight modified lines in the gutter",
                |s, on| s.gutter_markers = on,
                |d| d.gutter_markers,
            ),
        ))
        .into_element()
}

fn advanced_page(
    handle: &AppHandle,
    draft: Signal<VcsSettings>,
    dirty: Signal<bool>,
) -> AnyElement {
    let t = theme();
    let crlf = handle.clone();
    let detached = handle.clone();
    let policy = handle.clone();
    let value = snapshot(draft);
    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(row(
            "Warn before committing CRLF",
            "Ask before a commit writes CRLF line endings.",
            switch_control(
                &crlf,
                draft,
                dirty,
                "warn-crlf",
                "Warn before committing CRLF",
                |s, on| s.warn_crlf = on,
                |d| d.warn_crlf,
            ),
        ))
        .child(row(
            "Warn when committing in detached HEAD or mid-rebase",
            "Ask before a commit that no branch will point at.",
            switch_control(
                &detached,
                draft,
                dirty,
                "warn-detached",
                "Warn when committing in detached HEAD or mid-rebase",
                |s, on| s.warn_detached = on,
                |d| d.warn_detached,
            ),
        ))
        .child(setting_row(
            "When patch is created",
            "What to do with a patch file after a create route has written it.",
            select(
                SelectProps::new(
                    PatchCreatedPolicy::ALL
                        .iter()
                        .map(|policy| SelectItem::new(policy.label(), policy.label()))
                        .collect(),
                )
                .default_value(value.when_patch_is_created.label())
                .label(move || SharedString::from(draft.get().when_patch_is_created.label()))
                .accessibility_label("When patch is created")
                .test_id(&test_ids::settings("patch-policy"))
                .on_change(move |value: &SharedString| {
                    let value = value.clone();
                    let mut next = snapshot(draft);
                    next.when_patch_is_created = PatchCreatedPolicy::ALL
                        .into_iter()
                        .find(|p| value == p.label())
                        .unwrap_or_default();
                    commit(&policy, draft, dirty, next);
                }),
            )
            .into_element(),
        ))
        .child(
            text("CRLF conversion")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        )
        .child(unbacked_row("Convert to LF on commit", true))
        .child(unbacked_row("Convert to CRLF on checkout", false))
        .child(unbacked_row("No conversion", false))
        .child(
            text("Commit checks")
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.muted_foreground),
        )
        .child(unbacked_row("Sign-off commits", false))
        .into_element()
}

fn row(label: &str, description: &str, control: AnyElement) -> AnyElement {
    let t = theme();
    view()
        .flex_row()
        .items_center()
        .gap(px(data::SPACING_X))
        .child(
            view()
                .flex_col()
                .gap(px(2.0))
                .flex_1()
                .child(
                    text(label.to_string())
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.foreground),
                )
                .child(
                    text(description.to_string())
                        .text_size(px(data::TYPE_CHIP))
                        .text_color(t.muted_foreground),
                ),
        )
        .child(control)
        .into_element()
}

fn setting_row(label: &str, description: &str, control: AnyElement) -> AnyElement {
    row(label, description, control)
}

fn switch_control(
    handle: &AppHandle,
    draft: Signal<VcsSettings>,
    dirty: Signal<bool>,
    id: &'static str,
    label: &'static str,
    apply: fn(&mut VcsSettings, bool),
    read: fn(&VcsSettings) -> bool,
) -> AnyElement {
    let write_handle = handle.clone();
    switch(
        SwitchProps::new(move || read(&draft.get()))
            .accessibility_label(label)
            .test_id(&test_ids::settings(id))
            .on_change(move |on| {
                let mut next = snapshot(draft);
                apply(&mut next, on);
                commit(&write_handle, draft, dirty, next);
            }),
    )
    .into_element()
}

fn unbacked_row(label: &'static str, checked: bool) -> AnyElement {
    let t = theme();
    view()
        .flex_row()
        .items_center()
        .gap(px(data::SPACING_X))
        .opacity(0.45)
        .disabled(true)
        .role(AccessibilityRole::Checkbox)
        .label(label)
        .accessibility_state(move || AccessibilityState {
            checked: Some(if checked {
                nsh_core::Checked::True
            } else {
                nsh_core::Checked::False
            }),
            ..AccessibilityState::default()
        })
        .child(
            text(if checked { "✓" } else { " " })
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        )
        .child(
            text(label.to_string())
                .text_size(px(data::TYPE_CONTROL))
                .text_color(t.foreground),
        )
        .into_element()
}

//! The welcome screen: the hero, the clone box, the three launch actions,
//! the recents list, and the getting-started card. The launch actions run
//! through the file-dialog seam (`picks::pick_folder`); a recent row opens
//! its own path; the recents' branch chip re-reads on the refresh tick.

use std::path::PathBuf;

use nsh_base::theme;
use nsh_core::prelude::*;
use turbogit_app::state::AppState;

use crate::bridge::AppHandle;
use crate::screens::picks::pick_folder;
use crate::test_ids;
use crate::theme as data;
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::card::{CardProps, card};
use crate::ui::checkbox::{CheckboxProps, CheckboxState, checkbox};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::input::{InputProps, input};
use crate::ui::row::{RowProps, RowState, row};

const GUTTER: f32 = 20.0;
const RECENTS_WINDOW: usize = 3;
/// How long a recents row's branch chip holds its answer before the tick
/// re-reads it, the egui cache's TTL.
const REFRESH_SECS: u64 = 5;

const HINTS: [&str; 5] = [
    "Stage files in the Commit tool window.",
    "Write a message and commit your changelist.",
    "Push branches to share your work.",
    "Pull to bring in teammates' changes.",
    "Browse history in the Git Log tool window.",
];

const CHANGELOG: [(&str, &str); 4] = [
    (
        "v0.9.0",
        "• Attach a workspace root and index every repository in a folder tree.",
    ),
    (
        "v0.9.0",
        "• Recents now show workspace repo counts and restore a workspace on click.",
    ),
    (
        "v0.9.0",
        "• The header reports the app version, git version, and indexed repo count.",
    ),
    ("v0.9.0", "• A “What's new” link opens this changelog."),
];

/// The welcome screen over the live state. The clone URL and the recents
/// disclosure live in signals; everything else reads the state through
/// bindings.
pub fn welcome_screen(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let changelog_open = signal(false);

    // The environment check fills its own cache once per mount; the clone
    // box's bindings read the cached answer.
    handle.update(|s| {
        let _ = s.git_env_check();
    });

    // The recents' branch chip holds its answer until this tick: a branch
    // that moved outside the app catches up on it.
    let refresh = handle.clone();
    set_interval(std::time::Duration::from_secs(REFRESH_SECS), move || {
        refresh.update(|s| s.invalidate_welcome_branches());
    });

    view()
        .flex_1()
        .bg(t.background)
        .child(
            scroll().flex_1().child(
                view()
                    .flex_col()
                    .gap(px(GUTTER))
                    .p(px(data::PANEL_PADDING * 2.0))
                    .child(hero(changelog_open))
                    .child(clone_box(handle.clone()))
                    .child(quick_actions(handle.clone()))
                    .child(
                        view()
                            .flex_row()
                            .gap(px(GUTTER))
                            .child(view().flex_1().child(recents_card(handle.clone())))
                            .child(view().flex_1().child(getting_started())),
                    ),
            ),
        )
        .child(changelog_layer(changelog_open))
}

/// The brand band: the wordmark and tagline on the left, What's new on the
/// right, on the primary-tinted band the hero paints.
fn hero(changelog_open: Signal<bool>) -> impl IntoElement {
    let t = theme();
    let open = changelog_open;
    view()
        .flex_row()
        .items_center()
        .gap(px(data::SPACING_X))
        .rounded(px(data::CARD_RADIUS))
        .bg(t.primary.opacity(0.22))
        .p(px(data::PANEL_PADDING))
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_center()
                .w(px(64.0))
                .h(px(64.0))
                .rounded(px(data::CARD_RADIUS))
                .bg(t.primary)
                .child(
                    text("⑂")
                        .text_size(px(30.0))
                        .text_color(t.primary_foreground),
                ),
        )
        .child(
            view().flex_col().gap(px(data::SPACING_Y)).child(
                text("TurboGit")
                    .text_size(px(data::TYPE_WORDMARK))
                    .font_weight(FontWeight::BOLD)
                    .text_color(t.foreground),
            ),
        )
        .child(
            view().flex_col().gap(px(2.0)).child(
                text("A fast, keyboard-friendly Git client for your desktop.")
                    .text_size(px(data::TYPE_TAGLINE))
                    .text_color(t.muted_foreground),
            ),
        )
        .child(
            view().flex_1().flex_row().justify_end().child(button(
                ButtonProps::new("What's new")
                    .variant(ButtonVariant::Ghost)
                    .test_id(&test_ids::welcome("whats-new"))
                    .on_press(move || open.set(true)),
            )),
        )
}

/// The clone box: the URL input, the shallow toggle, and the Clone action
/// gated on the environment check and a non-empty URL.
fn clone_box(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let url = signal(String::new());
    let shallow = signal(false);
    let clone_generation = signal(0_u64);

    let generation_for_input = clone_generation;
    let url_field = input(
        InputProps::new()
            .placeholder("Repository URL")
            .accessibility_label("Repository URL")
            .remount(move || generation_for_input.get())
            .on_change(move |text: &str| url.set(text.to_string()))
            .test_id(&test_ids::welcome("clone-url")),
    );

    let shallow_for_box = shallow;
    let shallow_box = checkbox(
        CheckboxProps::new(move || {
            if shallow_for_box.get() {
                CheckboxState::Checked
            } else {
                CheckboxState::Unchecked
            }
        })
        .accessibility_label("Shallow clone (--depth 1)")
        .on_change(move |next| shallow.set(next == CheckboxState::Checked))
        .test_id(&test_ids::welcome("clone-shallow")),
    );

    let url_for_action = url;
    let shallow_for_action = shallow;
    let handle_for_action = handle.clone();
    let generation_for_action = clone_generation;
    let action_row = dynamic(move || {
        let git = handle_for_action
            .peek(|s| s.ui.git_env_check.clone())
            .map(|(_, result)| result);
        let url_text = url_for_action.get().trim().to_string();
        let can_clone = git.as_ref().is_some_and(|result| result.is_ok()) && !url_text.is_empty();
        let clone = button(
            ButtonProps::new("Clone")
                .test_id(&test_ids::welcome("clone"))
                .disabled(!can_clone)
                .on_press({
                    let handle_for_action = handle_for_action.clone();
                    move || {
                        let url_text = url_for_action
                            .get()
                            .trim()
                            .trim_end_matches('/')
                            .to_string();
                        if url_text.is_empty() {
                            return;
                        }
                        let depth = shallow_for_action.get().then_some(1usize);
                        let generation = generation_for_action;
                        let url_source = url_for_action;
                        pick_folder(handle_for_action.clone(), "Clone", move |s, parent| {
                            let name = url_text
                                .trim_end_matches(['/', '\\'])
                                .rsplit(['/', '\\'])
                                .next()
                                .unwrap_or("repo")
                                .trim_end_matches(".git")
                                .to_string();
                            if name.is_empty() {
                                s.ui.toast = Some(turbogit_app::state::Toast::error(
                                    "Clone: could not derive a folder name from the URL",
                                ));
                                return;
                            }
                            let dest = parent.join(&name);
                            s.clone_into(&url_text, &dest, depth);
                            // The clone consumed the URL: the remount takes
                            // the signal's cleared reading.
                            url_source.set(String::new());
                            generation.update(|n| *n += 1);
                        });
                    }
                }),
        );
        view().flex_col().gap(px(data::SPACING_Y)).children({
            let warning = (!git.as_ref().is_some_and(|result| result.is_ok())).then(|| {
                text(format!(
                    "Git was not found ({reason}). Install Git, or set its location in \
                         Settings › Git.",
                    reason = git
                        .clone()
                        .and_then(|result| result.err())
                        .unwrap_or_else(|| "git could not be resolved".to_string())
                ))
                .text_size(px(data::TYPE_BODY))
                .text_color(data::STATE_WARNING)
                .into_element()
            });
            let action = view()
                .flex_row()
                .child(view().flex_1().child(clone))
                .into_element();
            warning
                .into_iter()
                .chain(std::iter::once(action))
                .collect::<Vec<_>>()
        })
    });

    card(
        CardProps::new()
            .header("Clone a repository")
            .test_id(&test_ids::welcome("clone-card"))
            .child(
                view()
                    .flex_row()
                    .items_center()
                    .gap(px(data::SPACING_X))
                    .child(view().flex_1().child(url_field))
                    .child(shallow_box),
            )
            .child(action_row)
            .child(
                text("The clone lands in the folder you pick, named after the URL.")
                    .text_size(px(data::TYPE_CONTROL))
                    .text_color(t.muted_foreground),
            ),
    )
}

/// The three launch cards in one row: Open Project, Initialize Repository,
/// Attach Workspace Root.
fn quick_actions(handle: AppHandle) -> impl IntoElement {
    view().flex_row().gap(px(GUTTER)).children([
        action_card(
            handle.clone(),
            "Open Project",
            "Open a folder as a TurboGit project.",
            test_ids::welcome("open-project"),
            |s, dir| s.open_project(&dir),
        ),
        action_card(
            handle.clone(),
            "Initialize Repository",
            "Create a fresh Git repository in a chosen folder.",
            test_ids::welcome("initialize"),
            |s, dir| s.initialize_and_enter(&dir),
        ),
        action_card(
            handle,
            "Attach Workspace Root",
            "Index every repo in a folder tree.",
            test_ids::welcome("attach-workspace"),
            |s, dir| s.attach_workspace(&dir),
        ),
    ])
}

/// One launch card: the press target is the whole card, named by its title.
fn action_card(
    handle: AppHandle,
    title: &'static str,
    body: &'static str,
    id: String,
    enter: fn(&mut AppState, PathBuf),
) -> impl IntoElement {
    let t = theme();
    let press_handle = handle;
    view()
        .flex_col()
        .flex_1()
        .gap(px(data::SPACING_Y))
        .rounded(px(data::CARD_RADIUS))
        .bg(t.card)
        .p(px(data::PANEL_PADDING))
        .role(AccessibilityRole::Button)
        .label(SharedString::from(title))
        .hover(|s| s.bg(t.accent))
        .focus_visible(|s| s.ring(px(data::RAIL_WIDTH)).ring_color(t.ring))
        .on_press(move || pick_folder(press_handle.clone(), title, enter))
        .test_id(&id)
        .child(
            text(title)
                .text_size(px(data::TYPE_DETAIL_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.card_foreground),
        )
        .child(
            text(body)
                .text_size(px(data::TYPE_BODY))
                .text_color(t.muted_foreground),
        )
}

/// "N repos" or "1 repo", the one counted spelling.
fn counted(n: usize) -> String {
    if n == 1 {
        format!("{n} repo")
    } else {
        format!("{n} repos")
    }
}

/// The recents card: one row per stored project, the disclosure past three,
/// and the remove control on each row.
fn recents_card(handle: AppHandle) -> impl IntoElement {
    let t = theme();
    let expanded = signal(false);
    let rows_handle = handle.clone();
    let empty_handle = handle.clone();
    let disclosure_handle = handle.clone();

    let mut props = CardProps::new()
        .header("Recent projects")
        .test_id(&test_ids::welcome("recents"));
    props = props
        .child(when(
            move || empty_handle.read(|s| s.ui.recent_projects.is_empty()),
            move || {
                text("No recent projects yet.")
                    .text_size(px(data::TYPE_BODY))
                    .text_color(t.muted_foreground)
            },
        ))
        .child(each(
            {
                let rows_handle = rows_handle.clone();
                let window = expanded;
                move || {
                    rows_handle.read(|s| {
                        let count = s.ui.recent_projects.len();
                        let shown = if window.get() {
                            count
                        } else {
                            count.min(RECENTS_WINDOW)
                        };
                        s.ui.recent_projects
                            .iter()
                            .take(shown)
                            .cloned()
                            .collect::<Vec<_>>()
                    })
                }
            },
            |project: &turbogit_app::recents::RecentProject| project.path.display().to_string(),
            move |project, _| {
                let t = theme();
                let row_handle = rows_handle.clone();
                let branch_handle = row_handle.clone();
                let open_handle = rows_handle.clone();
                let name = project.get().name.clone();
                view()
                    .flex_col()
                    .gap(px(data::DENSITY_LIST_GAP))
                    .child(row(RowProps::new()
                        .state(RowState::Default)
                        .test_id(&test_ids::welcome(format!("recent:{name}")))
                        .accessibility_label(name)
                        .on_press(move || {
                            let recent = project.get();
                            open_handle.update(|s| s.open_recent(&recent));
                        })
                        .child(
                            view()
                                .flex_col()
                                .gap(px(2.0))
                                .child(
                                    text(move || project.get().name.clone())
                                        .text_size(px(data::TYPE_DETAIL_TITLE))
                                        .text_color(t.card_foreground),
                                )
                                .child(
                                    text(move || project.get().path.display().to_string())
                                        .text_size(px(data::TYPE_CONTROL))
                                        .text_color(t.muted_foreground),
                                )
                                .child(
                                    text(move || {
                                        turbogit_app::recents::format_last_opened(
                                            project.get().last_opened,
                                        )
                                    })
                                    .text_size(px(data::TYPE_CONTROL))
                                    .text_color(t.muted_foreground),
                                ),
                        )
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(data::SPACING_X))
                                .child(
                                    text(move || {
                                        branch_handle.read(|s| {
                                            s.current_branch_of(&project.get().path)
                                                .unwrap_or_default()
                                        })
                                    })
                                    .text_size(px(data::TYPE_CONTROL))
                                    .text_color(data::ACCENT_TEXT),
                                )
                                .child(
                                    text(move || {
                                        let recent = project.get();
                                        if recent.kind
                                            == turbogit_app::recents::RecentKind::Workspace
                                        {
                                            recent.repo_count.map(counted).unwrap_or_default()
                                        } else {
                                            Default::default()
                                        }
                                    })
                                    .text_size(px(data::TYPE_CONTROL))
                                    .text_color(data::ACCENT_TEXT),
                                ),
                        )))
                    .child(remove_button(row_handle, project))
            },
        ));
    props = props.child(when(
        move || disclosure_handle.read(|s| s.ui.recent_projects.len() > RECENTS_WINDOW),
        move || {
            let toggle = expanded;
            button(
                ButtonProps::new("")
                    .label_of(move || {
                        if toggle.get() {
                            "Show fewer projects".to_string()
                        } else {
                            "Show all projects".to_string()
                        }
                    })
                    .variant(ButtonVariant::Ghost)
                    .test_id(&test_ids::welcome("disclosure"))
                    .on_press(move || toggle.set(!toggle.get())),
            )
        },
    ));
    card(props)
}

/// The row's remove control: it drops only that row and names what it
/// dropped.
fn remove_button(
    handle: AppHandle,
    project: nsh_core::element::reactive::ReadSignal<turbogit_app::recents::RecentProject>,
) -> impl IntoElement {
    let remove_handle = handle;
    button(
        ButtonProps::new(format!("Remove recent project {}", project.get().name))
            .label_of(move || format!("Remove recent project {}", project.get().name))
            .variant(ButtonVariant::Ghost)
            .size(crate::ui::ControlSize::Compact)
            .test_id(&test_ids::welcome(format!(
                "remove-recent:{}",
                project.get().name
            )))
            .on_press(move || {
                let path = project.get().path;
                remove_handle.update(move |s| {
                    s.remove_recent(&path);
                });
            }),
    )
}

/// The getting-started card: the five first steps.
fn getting_started() -> impl IntoElement {
    let t = theme();
    card(
        CardProps::new().header("Getting started").child(
            view().flex_col().gap(px(data::SPACING_Y)).children(
                HINTS
                    .iter()
                    .enumerate()
                    .map(|(at, hint)| {
                        view()
                            .flex_row()
                            .items_center()
                            .gap(px(data::SPACING_X))
                            .child(
                                text((at + 1).to_string())
                                    .text_size(px(data::TYPE_CONTROL))
                                    .text_color(data::ACCENT_TEXT),
                            )
                            .child(
                                text(*hint)
                                    .text_size(px(data::TYPE_BODY))
                                    .text_color(t.card_foreground),
                            )
                            .into_element()
                    })
                    .collect::<Vec<_>>(),
            ),
        ),
    )
}

/// The changelog modal the hero's What's new opens.
fn changelog_layer(open: Signal<bool>) -> impl IntoElement {
    let t = theme();
    when(
        move || open.get(),
        move || {
            let close_for_action = open;
            dialog(
                DialogProps::new("whats-new", "What's New")
                    .action("Close")
                    .on_action(move || close_for_action.set(false))
                    .child(
                        view().flex_col().gap(px(data::SPACING_Y)).children(
                            CHANGELOG
                                .iter()
                                .map(|(version, note)| {
                                    view()
                                        .flex_row()
                                        .gap(px(data::SPACING_X))
                                        .child(
                                            text(*version)
                                                .text_size(px(data::TYPE_BODY))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(t.primary),
                                        )
                                        .child(
                                            text(*note)
                                                .text_size(px(data::TYPE_BODY))
                                                .text_color(t.card_foreground),
                                        )
                                        .into_element()
                                })
                                .collect::<Vec<_>>(),
                        ),
                    ),
            )
        },
    )
}

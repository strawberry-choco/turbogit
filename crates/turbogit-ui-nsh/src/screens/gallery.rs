//! The gallery: every control in every state, the KitchenSink the live
//! lanes and the review gate walk. A section per control family; a bridge
//! section when a handle carries one, so the lanes can drive the pump.

use nsh_base::{SelectItem, theme};
use nsh_core::prelude::*;

use crate::bridge::AppHandle;
use crate::test_ids;
use crate::theme as data;
use crate::ui::ControlSize;
use crate::ui::alert::{AlertAnswer, alert_dialog};
use crate::ui::button::{ButtonProps, ButtonVariant, button};
use crate::ui::card::{CardProps, card};
use crate::ui::checkbox::{CheckboxProps, CheckboxState, checkbox};
use crate::ui::chip::{ChipProps, ChipVariant, chip};
use crate::ui::combobox::{ComboboxProps, combobox};
use crate::ui::data_table::{DataTableColumnProps, DataTableProps, data_table};
use crate::ui::dialog::{DialogProps, dialog};
use crate::ui::icon_button::{IconButtonProps, icon_button};
use crate::ui::input::{InputProps, input, text_area};
use crate::ui::menu::{MenuEntry, MenuProps, context_menu_host, menu};
use crate::ui::pane_header::{PaneHeaderProps, pane_header};
use crate::ui::popover::{PopoverProps, popover};
use crate::ui::progress::{ProgressProps, progress};
use crate::ui::row::{RowProps, RowState, row};
use crate::ui::scrollbar::{ScrollbarProps, scrollbar};
use crate::ui::select::{SelectProps, select};
use crate::ui::tab::{TabItem, TabsProps, tabs};
use crate::ui::toast::ToastShared;
use crate::ui::toolbar::{ToolbarProps, toolbar};
use crate::ui::tooltip::{TooltipProps, tooltip};
use crate::ui::tree::{TreeEntryProps, TreeProps, tree};
use nsh_base::DataTableRows;

/// The gallery's root: every section in one scroll, the scrollbar on the
/// right, the overlay layers the root mounts.
pub fn kitchen_sink(handle: AppHandle, toasts: ToastShared) -> impl IntoElement {
    let list = element_ref();
    let host = view()
        .element_ref(&list)
        .flex_1()
        .child(
            scroll().flex_1().child(
                view()
                    .flex_col()
                    .gap(px(data::SPACING_Y))
                    .p(px(data::PANEL_PADDING))
                    .child(section("buttons", "Buttons", buttons_section()))
                    .child(section(
                        "icon-buttons",
                        "Icon buttons",
                        icon_buttons_section(),
                    ))
                    .child(section("inputs", "Inputs", inputs_section()))
                    .child(section("checkboxes", "Checkboxes", checkbox_section()))
                    .child(section("selects", "Selects", select_section()))
                    .child(section("chips", "Chips", chips_section()))
                    .child(section("tabs", "Tabs", tabs_section()))
                    .child(section("toolbar", "Toolbar", toolbar_section()))
                    .child(section("pane-header", "Pane header", pane_header_section()))
                    .child(section("rows", "Rows", rows_section()))
                    .child(section("card", "Card", card_section()))
                    .child(section("data-table", "Data table", data_table_section()))
                    .child(section("tree", "Tree", tree_section()))
                    .child(section("menus", "Menus", menu_section()))
                    .child(section("popover", "Popover", popover_section()))
                    .child(section(
                        "overlays",
                        "Overlays",
                        overlays_section(handle.clone(), toasts),
                    ))
                    .child(section("progress", "Progress", progress_section()))
                    .child(section("bridge", "Bridge", bridge_section(handle))),
            ),
        )
        .child(
            view()
                .absolute()
                .right_0()
                .top_0()
                .bottom_0()
                .child(scrollbar(ScrollbarProps::new(
                    nsh_base::ScrollbarTarget::Ref(list),
                ))),
        );
    view()
        .flex_1()
        .flex_row()
        .relative()
        .bg(theme().background)
        .child(host)
}

/// A titled section: the section header's type step, muted ink, then the
/// section's content in a column.
fn section(name: &'static str, title: &'static str, content: impl IntoElement) -> impl IntoElement {
    let t = theme();
    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .test_id(&test_ids::gallery_section(name))
        .child(
            text(title)
                .text_size(px(data::TYPE_SECTION))
                .text_color(t.muted_foreground),
        )
        .child(content)
}

fn row_of(children: impl IntoIterator<Item = AnyElement>) -> impl IntoElement {
    let mut row = view()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(data::CELL_GAP));
    for child in children {
        row = row.child(child);
    }
    row
}

fn buttons_section() -> impl IntoElement {
    let t = theme();
    row_of([
        button(ButtonProps::new("Primary").test_id(&test_ids::demo("button-primary")))
            .into_element(),
        button(
            ButtonProps::new("Secondary")
                .variant(ButtonVariant::Secondary)
                .test_id(&test_ids::demo("button-secondary")),
        )
        .into_element(),
        button(
            ButtonProps::new("Ghost")
                .variant(ButtonVariant::Ghost)
                .test_id(&test_ids::demo("button-ghost")),
        )
        .into_element(),
        button(
            ButtonProps::new("Danger")
                .variant(ButtonVariant::Danger)
                .test_id(&test_ids::demo("button-danger")),
        )
        .into_element(),
        button(
            ButtonProps::new("Disabled")
                .disabled(true)
                .test_id(&test_ids::demo("button-disabled")),
        )
        .into_element(),
        button(
            ButtonProps::new("Selected")
                .selected(true)
                .variant(ButtonVariant::Secondary)
                .test_id(&test_ids::demo("button-selected")),
        )
        .into_element(),
        button(
            ButtonProps::new("Large")
                .size(ControlSize::Large)
                .test_id(&test_ids::demo("button-large")),
        )
        .into_element(),
        button(
            ButtonProps::new("Press")
                .on_press(|| {})
                .test_id(&test_ids::demo("button-press")),
        )
        .into_element(),
        // The pressed fill, shown statically for the review: a press of
        // `button-press` answers live.
        view()
            .px(px(data::BUTTON_PAD_X))
            .py(px(data::BUTTON_PAD_Y))
            .rounded(px(data::CONTROL_RADIUS))
            .bg(t.primary.map(|c| c.darken(0.24)))
            .child(
                text("Pressed")
                    .text_size(px(data::TYPE_BODY))
                    .text_color(t.primary_foreground),
            )
            .into_element(),
    ])
}

fn icon_buttons_section() -> impl IntoElement {
    row_of([
        icon_button(IconButtonProps::new("+", "Add").test_id(&test_ids::demo("icon-add")))
            .into_element(),
        icon_button(IconButtonProps::new("-", "Remove").test_id(&test_ids::demo("icon-remove")))
            .into_element(),
        icon_button(
            IconButtonProps::new("+", "Add disabled")
                .disabled(true)
                .test_id(&test_ids::demo("icon-disabled")),
        )
        .into_element(),
        icon_button(
            IconButtonProps::new("+", "Add selected")
                .selected(true)
                .test_id(&test_ids::demo("icon-selected")),
        )
        .into_element(),
    ])
}

fn inputs_section() -> impl IntoElement {
    let count = signal(String::new());
    view()
        .flex_col()
        .max_w(px(640.0))
        .gap(px(data::SPACING_Y))
        .children([
            input(
                InputProps::new()
                    .placeholder("Single line")
                    .test_id(&test_ids::demo("input-single")),
            )
            .into_element(),
            input(
                InputProps::new()
                    .default_value("read only")
                    .readonly(true)
                    .test_id(&test_ids::demo("input-readonly")),
            )
            .into_element(),
            input(
                InputProps::new()
                    .placeholder("Masked")
                    .masked(true)
                    .test_id(&test_ids::demo("input-masked")),
            )
            .into_element(),
            input(
                InputProps::new()
                    .placeholder("Disabled")
                    .disabled(true)
                    .test_id(&test_ids::demo("input-disabled")),
            )
            .into_element(),
            text_area(
                InputProps::new()
                    .placeholder("Multiline: the commit message editor's role")
                    .test_id(&test_ids::demo("input-multiline")),
            )
            .into_element(),
            text_area(
                InputProps::new()
                    .placeholder("Multiline, disabled")
                    .disabled(true)
                    .test_id(&test_ids::demo("input-multiline-disabled")),
            )
            .into_element(),
            // Uncontrolled: the echo label reads what the typist typed, the
            // input keeps its own value (the bridge rule).
            input(
                InputProps::new()
                    .placeholder("Echoes below")
                    .on_change(move |text: &str| count.set(text.to_string()))
                    .test_id(&test_ids::demo("input-echo")),
            )
            .into_element(),
            text(move || format!("echo: {}", count.get()))
                .test_id(&test_ids::demo("input-echo-label"))
                .text_size(px(data::TYPE_BODY))
                .into_element(),
        ])
}

fn checkbox_section() -> impl IntoElement {
    let checked = signal(CheckboxState::Unchecked);
    row_of([
        checkbox(
            CheckboxProps::new(|| CheckboxState::Unchecked)
                .test_id(&test_ids::demo("checkbox-unchecked")),
        )
        .into_element(),
        checkbox(
            CheckboxProps::new(|| CheckboxState::Checked)
                .test_id(&test_ids::demo("checkbox-checked")),
        )
        .into_element(),
        checkbox(
            CheckboxProps::new(|| CheckboxState::Indeterminate)
                .test_id(&test_ids::demo("checkbox-indeterminate")),
        )
        .into_element(),
        checkbox(
            CheckboxProps::new(|| CheckboxState::Unchecked)
                .disabled(true)
                .test_id(&test_ids::demo("checkbox-disabled")),
        )
        .into_element(),
        checkbox(
            CheckboxProps::new(move || checked.get())
                .accessibility_label("Toggle")
                .on_change(move |next| checked.set(next))
                .test_id(&test_ids::demo("checkbox-live")),
        )
        .into_element(),
        text(move || format!("state: {:?}", checked.get()))
            .text_size(px(data::TYPE_BODY))
            .test_id(&test_ids::demo("checkbox-live-label"))
            .into_element(),
    ])
}

fn select_items() -> Vec<SelectItem> {
    vec![
        SelectItem::new("main", "main"),
        SelectItem::new("feature/log-lanes", "feature/log-lanes"),
        SelectItem::new("origin/main", "origin/main"),
    ]
}

fn select_section() -> impl IntoElement {
    let chosen = signal(String::new());
    let combo_chosen = signal(Vec::<SharedString>::new());
    let combo_multi = signal(Vec::<SharedString>::new());
    view()
        .flex_col()
        .max_w(px(640.0))
        .gap(px(data::SPACING_Y))
        .children([
            row_of([
                select(
                    SelectProps::new(select_items())
                        .placeholder("Choose a branch")
                        .label(move || SharedString::from(chosen.get().clone()))
                        .on_change(move |value: &SharedString| chosen.set(value.to_string()))
                        .test_id(&test_ids::demo("select")),
                )
                .into_element(),
                select(
                    SelectProps::new(select_items())
                        .disabled(true)
                        .placeholder("Disabled")
                        .test_id(&test_ids::demo("select-disabled")),
                )
                .into_element(),
            ])
            .into_element(),
            text(move || format!("branch: {}", chosen.get()))
                .test_id(&test_ids::demo("select-label"))
                .text_size(px(data::TYPE_BODY))
                .into_element(),
            combobox(
                ComboboxProps::new(select_items())
                    .placeholder("Filter branches")
                    .on_change(move |values: &Vec<SharedString>| combo_chosen.set(values.clone()))
                    .test_id(&test_ids::demo("combobox")),
            )
            .into_element(),
            text(move || format!("picked: {}", combo_chosen.get().join(", ")))
                .test_id(&test_ids::demo("combobox-label"))
                .text_size(px(data::TYPE_BODY))
                .into_element(),
            combobox(
                ComboboxProps::new(select_items())
                    .multiple(true)
                    .placeholder("Pick several")
                    .on_change(move |values: &Vec<SharedString>| combo_multi.set(values.clone()))
                    .test_id(&test_ids::demo("combobox-multiple")),
            )
            .into_element(),
            text(move || format!("multi: {}", combo_multi.get().join(", ")))
                .test_id(&test_ids::demo("combobox-multiple-label"))
                .text_size(px(data::TYPE_BODY))
                .into_element(),
            combobox(
                ComboboxProps::new(select_items())
                    .disabled(true)
                    .placeholder("Disabled")
                    .test_id(&test_ids::demo("combobox-disabled")),
            )
            .into_element(),
        ])
}

fn chips_section() -> impl IntoElement {
    let t = theme();
    row_of([
        chip(ChipProps::new("3 files").test_id(&test_ids::demo("chip-neutral"))).into_element(),
        chip(
            ChipProps::new("12")
                .variant(ChipVariant::Raised)
                .test_id(&test_ids::demo("chip-raised")),
        )
        .into_element(),
        chip(
            ChipProps::new("scope")
                .variant(ChipVariant::Accent)
                .test_id(&test_ids::demo("chip-accent")),
        )
        .into_element(),
        // State is colored text, never a chip: the repository-state map's
        // paint (R6).
        text("dirty")
            .text_size(px(data::TYPE_CONTROL))
            .text_color(data::RepoState::Dirty.color())
            .into_element(),
        text("diverged")
            .text_size(px(data::TYPE_CONTROL))
            .text_color(data::RepoState::Diverged.color())
            .into_element(),
        text("unpushed commits")
            .text_size(px(data::TYPE_CONTROL))
            .text_color(data::RepoState::Unpushed.color())
            .into_element(),
        // The diff pairs, staged the foreground and unstaged the muted one.
        view()
            .px(px(data::CELL_GAP))
            .rounded(px(data::MARK_RADIUS))
            .bg(data::DIFF_ADD_BG)
            .child(
                text("+ added, staged")
                    .text_size(px(data::TYPE_BODY))
                    .text_color(data::DIFF_ADD_TEXT),
            )
            .into_element(),
        view()
            .px(px(data::CELL_GAP))
            .rounded(px(data::MARK_RADIUS))
            .bg(data::DIFF_ADD_UNSTAGED_BG)
            .child(
                text("+ added, unstaged")
                    .text_size(px(data::TYPE_BODY))
                    .text_color(data::DIFF_ADD_UNSTAGED_TEXT),
            )
            .into_element(),
        view()
            .px(px(data::CELL_GAP))
            .rounded(px(data::MARK_RADIUS))
            .bg(data::DIFF_DEL_BG)
            .child(
                text("- deleted, staged")
                    .text_size(px(data::TYPE_BODY))
                    .text_color(data::DIFF_DEL_TEXT),
            )
            .into_element(),
        // A ref accent and the lane rails, the data tokens the graph reads.
        text("main")
            .text_size(px(data::TYPE_CONTROL))
            .text_color(t.primary)
            .into_element(),
        row_of(data::LANES.iter().map(|lane| {
            view()
                .w(px(data::RAIL_WIDTH))
                .h(px(data::FILE_ROW_HEIGHT))
                .bg(*lane)
                .into_element()
        }))
        .into_element(),
    ])
}

fn tabs_section() -> impl IntoElement {
    let selected = signal(1usize);
    let items = vec![
        TabItem::new("Commit").test_id(&test_ids::tab("Commit")),
        TabItem::new("Log").test_id(&test_ids::tab("Log")),
        TabItem::new("Branches").test_id(&test_ids::tab("Branches")),
        TabItem::new("Worktrees").test_id(&test_ids::tab("Worktrees")),
        TabItem::new("Submodules").test_id(&test_ids::tab("Submodules")),
    ];
    view().flex_col().gap(px(data::SPACING_Y)).children([
        tabs(TabsProps::new(items, move || selected.get()).on_change(move |at| selected.set(at)))
            .into_element(),
        text(move || format!("tab: {}", selected.get()))
            .test_id(&test_ids::demo("tabs-label"))
            .text_size(px(data::TYPE_BODY))
            .into_element(),
    ])
}

fn toolbar_section() -> impl IntoElement {
    view().flex_col().gap(px(data::SPACING_Y)).children([
        toolbar(
            ToolbarProps::new()
                .child(button(
                    ButtonProps::new("Fetch")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::demo("button-fetch")),
                ))
                .child(button(
                    ButtonProps::new("Pull")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::demo("button-pull")),
                ))
                .child(button(
                    ButtonProps::new("Push").test_id(&test_ids::demo("button-push")),
                ))
                .test_id(&test_ids::toolbar("sync")),
        )
        .into_element(),
        toolbar(
            ToolbarProps::new()
                .disabled(true)
                .child(button(
                    ButtonProps::new("Fetch")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::demo("button-fetch-disabled")),
                ))
                .test_id(&test_ids::toolbar("sync-disabled")),
        )
        .into_element(),
    ])
}

fn pane_header_section() -> impl IntoElement {
    pane_header(
        PaneHeaderProps::new("Local changes")
            .count("4")
            .action(button(
                ButtonProps::new("Stage all")
                    .variant(ButtonVariant::Secondary)
                    .test_id(&test_ids::demo("button-stage-all")),
            )),
    )
    .into_element()
}

fn rows_section() -> impl IntoElement {
    view().flex_col().gap(px(data::DENSITY_LIST_GAP)).children([
        row(RowProps::new()
            .child(text("src/main.rs").text_size(px(data::TYPE_BODY)))
            .test_id(&test_ids::demo("row-default")))
        .into_element(),
        row(RowProps::new()
            .state(RowState::Hover)
            .child(text("src/lib.rs (hover)").text_size(px(data::TYPE_BODY)))
            .test_id(&test_ids::demo("row-hover")))
        .into_element(),
        row(RowProps::new()
            .state(RowState::Selected)
            .child(text("src/theme.rs (selected)").text_size(px(data::TYPE_BODY)))
            .test_id(&test_ids::demo("row-selected")))
        .into_element(),
    ])
}

fn card_section() -> impl IntoElement {
    card(
        CardProps::new()
            .header("Working copy")
            .child(text("2 modified, 1 unversioned").text_size(px(data::TYPE_BODY)))
            .test_id(&test_ids::demo("card")),
    )
    .into_element()
}

fn data_table_section() -> impl IntoElement {
    const WORKTREES: [(&str, &str); 4] = [
        ("main", "/repos/turbogit"),
        ("log-lanes", "/repos/turbogit-wt-log"),
        ("blame", "/repos/turbogit-wt-blame"),
        ("submodules", "/repos/turbogit-wt-sub"),
    ];
    let rows = DataTableRows::new(WORKTREES.len())
        .texts("name", WORKTREES.iter().map(|(name, _)| *name))
        .texts("path", WORKTREES.iter().map(|(_, path)| *path));
    view()
        .h(px(data::GROUP_ROW_HEIGHT * 6.0))
        .child(data_table(
            DataTableProps::new(
                vec![
                    DataTableColumnProps::new("name", "Name").width(140.0),
                    DataTableColumnProps::new("path", "Path").width(360.0),
                ],
                rows,
            )
            .accessibility_label("Worktrees")
            .test_id(&test_ids::demo("data-table")),
        ))
        .into_element()
}

fn tree_section() -> impl IntoElement {
    view()
        .h(px(data::GROUP_ROW_HEIGHT * 5.0))
        .child(tree(
            TreeProps::new(vec![
                TreeEntryProps::new("src", "src")
                    .child(TreeEntryProps::new("src/theme.rs", "theme.rs"))
                    .child(TreeEntryProps::new("src/bridge.rs", "bridge.rs")),
                TreeEntryProps::new("tests", "tests")
                    .child(TreeEntryProps::new("tests/tokens.rs", "tokens.rs")),
            ])
            .default_expanded("src")
            .accessibility_label("Files")
            .test_id(&test_ids::demo("tree")),
        ))
        .into_element()
}

fn menu_section() -> impl IntoElement {
    let t = theme();
    let menu_trigger = view()
        .px_3()
        .py_1()
        .rounded(px(data::CONTROL_RADIUS))
        .bg(t.secondary)
        .test_id(&test_ids::demo("menu-trigger"))
        .child(text("Open menu").text_size(px(data::TYPE_BODY)));
    let menu = menu(
        MenuProps::new(
            vec![
                MenuEntry::item("New branch"),
                MenuEntry::item("Stage all"),
                MenuEntry::Separator,
                MenuEntry::item("Checkout").disabled(),
            ],
            menu_trigger,
        )
        .on_select(|_| {}),
    );
    let context_host = view()
        .px_3()
        .py_1()
        .rounded(px(data::CONTROL_RADIUS))
        .bg(t.secondary)
        .test_id(&test_ids::demo("context-host"))
        .child(text("Press for context menu").text_size(px(data::TYPE_BODY)));
    let context = context_menu_host(
        context_host,
        vec!["Copy hash".into(), "Revert commit".into()],
        |_| {},
    );
    row_of([menu.into_element(), context.into_element()])
}

fn popover_section() -> impl IntoElement {
    let t = theme();
    // The trigger view carries the test id: the base popover attaches the
    // toggle handler to the trigger, so a selector presses the handler's
    // owner, not the wrapper.
    let trigger = view()
        .px_3()
        .py_1()
        .rounded(px(data::CONTROL_RADIUS))
        .bg(t.secondary)
        .test_id(&test_ids::demo("popover-trigger"))
        .child(text("Open popover").text_size(px(data::TYPE_BODY)));
    row_of([popover(
        PopoverProps::new(trigger)
            .width(220.0)
            .child(text("Checkout branch").text_size(px(data::TYPE_BODY)))
            .child(
                button(
                    ButtonProps::new("Checkout")
                        .variant(ButtonVariant::Secondary)
                        .test_id(&test_ids::demo("popover-checkout")),
                )
                .into_element(),
            ),
    )
    .into_element()])
}

fn overlays_section(handle: AppHandle, toasts: ToastShared) -> impl IntoElement {
    let t = theme();
    let dialog_open = signal(false);
    let toast_note = signal(String::from("toast: none shown"));
    let dialog_handle = handle.clone();

    let open_dialog = view()
        .on_press(move || dialog_open.set(true))
        .px_3()
        .py_1()
        .rounded(px(data::CONTROL_RADIUS))
        .bg(t.secondary)
        .test_id(&test_ids::demo("open-dialog"))
        .child(text("Open dialog").text_size(px(data::TYPE_BODY)));
    let open_toast = view()
        .on_press(move || {
            toasts.push("Shelved".into());
            toast_note.set("toast: pushed".to_string());
        })
        .px_3()
        .py_1()
        .rounded(px(data::CONTROL_RADIUS))
        .bg(t.secondary)
        .test_id(&test_ids::demo("push-toast"))
        .child(text("Push toast").text_size(px(data::TYPE_BODY)));
    let open_alert = view()
        .on_press(move || {
            spawn(async move {
                let answer = alert_dialog(
                    "Discard the staged lines?",
                    "This cannot be undone.",
                    ("cancel", "Cancel"),
                    ("discard", "Discard"),
                )
                .await;
                if let AlertAnswer::Action(action) = answer {
                    toasts.push(format!("alert: {action}").into());
                }
            });
        })
        .px_3()
        .py_1()
        .rounded(px(data::CONTROL_RADIUS))
        .bg(t.secondary)
        .test_id(&test_ids::demo("open-alert"))
        .child(text("Open alert").text_size(px(data::TYPE_BODY)));

    let dialog_layer = when(
        move || dialog_open.get(),
        move || {
            let close = dialog_open;
            let confirm = dialog_open;
            let confirm_handle = dialog_handle.clone();
            dialog(
                DialogProps::new("rename-branch", "Rename branch")
                    .action("Rename")
                    .on_cancel(move || close.set(false))
                    .on_action(move || {
                        confirm_handle.update(|s| s.last_error = None);
                        confirm.set(false);
                    })
                    .child(input(InputProps::new().placeholder("feature/new-name")).into_element()),
            )
        },
    );

    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(row_of([
            open_dialog.into_element(),
            open_toast.into_element(),
            open_alert.into_element(),
            tooltip(TooltipProps::new(
                view()
                    .px_3()
                    .py_1()
                    .rounded(px(data::CONTROL_RADIUS))
                    .bg(t.secondary)
                    .test_id(&test_ids::demo("tooltip-trigger"))
                    .child(text("Hover for tooltip").text_size(px(data::TYPE_BODY))),
                "The workspace picker opens here in N2",
            ))
            .into_element(),
        ]))
        .child(
            text(move || toast_note.get().clone())
                .test_id(&test_ids::demo("toast-note"))
                .text_size(px(data::TYPE_BODY)),
        )
        .child(dialog_layer)
}

fn progress_section() -> impl IntoElement {
    view()
        .flex_col()
        .max_w(px(640.0))
        .gap(px(data::SPACING_Y))
        .children([
            progress(
                ProgressProps::new(30.0)
                    .accessibility_label("Fetch")
                    .test_id(&test_ids::demo("progress-30")),
            )
            .into_element(),
            progress(
                ProgressProps::new(80.0)
                    .accessibility_label("Rebase")
                    .test_id(&test_ids::demo("progress-80")),
            )
            .into_element(),
            progress(
                ProgressProps::new(0.0)
                    .indeterminate()
                    .accessibility_label("Preparing")
                    .test_id(&test_ids::demo("progress-indeterminate")),
            )
            .into_element(),
        ])
}

/// The bridge section: queue an event into the state, read the drained
/// count through a binding. The pump the root drives turns the queue into
/// a revision, the revision turns it into paint.
fn bridge_section(handle: AppHandle) -> impl IntoElement {
    let push = handle.clone();
    let error = handle.clone();
    view()
        .flex_col()
        .gap(px(data::SPACING_Y))
        .child(
            button(
                ButtonProps::new("Queue event")
                    .variant(ButtonVariant::Secondary)
                    .on_press(move || {
                        push.update(|s| {
                            let _ = s
                                .tx
                                .send(turbogit_app::events::AppEvent::Error("bridge event".into()));
                        });
                    })
                    .test_id(&test_ids::demo("bridge-push")),
            )
            .into_element(),
        )
        .child(
            text(move || {
                error.read(|s| {
                    s.last_error
                        .clone()
                        .unwrap_or_else(|| "last error: none".into())
                })
            })
            .test_id(&test_ids::demo("bridge-error"))
            .text_size(px(data::TYPE_BODY))
            .into_element(),
        )
}

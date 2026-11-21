//! The screens and the route table. One route per surface the migration
//! plan names: the built ones paint their screens, the ones later phases
//! own paint a named empty state. [`placeholder`] is the painter every
//! unbuilt route shares, and [`route_for_tab`] is the shell's map from a
//! tab to its route.

pub mod feedback;
pub mod gallery;
pub mod log;
pub mod log_graph;
pub mod picks;
pub mod shell;
pub mod welcome;
pub mod workspace_picker;

use nsh_base::theme;
use nsh_core::prelude::*;

use crate::test_ids;
use crate::theme as data;

/// One route of the table: the surface's id (the plan's `New file` name),
/// what a reader calls it, and the phase that builds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Route {
    pub id: &'static str,
    pub label: &'static str,
    pub phase: &'static str,
}

const N3: &str = "Stage changes and commit (N3)";
const N4: &str = "Branch, sync, and inspect refs (N4)";
const N5: &str = "Rewrite history and resolve conflicts (N5)";
const N6: &str = "Secondary surfaces and audit (N6)";

/// Every surface the plan assigns to N3 to N6, registered before any of
/// them is built. The route table later phases fill; a route with no
/// screen paints [`placeholder`].
pub const ROUTES: &[Route] = &[
    Route {
        id: "changes",
        label: "Changes",
        phase: N3,
    },
    Route {
        id: "diff_view",
        label: "Diff viewer",
        phase: N3,
    },
    Route {
        id: "image_diff",
        label: "Image diff",
        phase: N3,
    },
    Route {
        id: "shelf",
        label: "Shelf",
        phase: N3,
    },
    Route {
        id: "commit",
        label: "Commit",
        phase: N3,
    },
    Route {
        id: "branch_picker",
        label: "Branch picker",
        phase: N4,
    },
    Route {
        id: "branches",
        label: "Branches",
        phase: N4,
    },
    Route {
        id: "branch_dialogs",
        label: "Branch dialogs",
        phase: N4,
    },
    Route {
        id: "divergence",
        label: "Divergence",
        phase: N4,
    },
    Route {
        id: "worktrees",
        label: "Worktrees",
        phase: N4,
    },
    Route {
        id: "submodules",
        label: "Submodules",
        phase: N4,
    },
    Route {
        id: "remotes",
        label: "Remotes",
        phase: N4,
    },
    Route {
        id: "sync",
        label: "Sync",
        phase: N4,
    },
    Route {
        id: "merge",
        label: "Merge",
        phase: N4,
    },
    Route {
        id: "rebase",
        label: "Rebase",
        phase: N5,
    },
    Route {
        id: "rebase_editor",
        label: "Rebase editor",
        phase: N5,
    },
    Route {
        id: "cherry_pick",
        label: "Cherry-pick",
        phase: N5,
    },
    Route {
        id: "get_from_branch",
        label: "Get from branch",
        phase: N5,
    },
    Route {
        id: "patches",
        label: "Patches",
        phase: N5,
    },
    Route {
        id: "unshelve",
        label: "Unshelve",
        phase: N5,
    },
    Route {
        id: "conflicts",
        label: "Conflicts",
        phase: N5,
    },
    Route {
        id: "rewrite",
        label: "Rewrite",
        phase: N5,
    },
    Route {
        id: "bulk",
        label: "Bulk operations",
        phase: N5,
    },
    Route {
        id: "blame",
        label: "Blame",
        phase: N6,
    },
    Route {
        id: "settings",
        label: "Settings",
        phase: N6,
    },
    Route {
        id: "shortcuts",
        label: "Shortcuts",
        phase: N6,
    },
    Route {
        id: "palette",
        label: "Command palette",
        phase: N6,
    },
    Route {
        id: "custom_command",
        label: "Custom command",
        phase: N6,
    },
];

/// The route a shell tab shows. The Log tab is the built screen; the other
/// four name their later-phase route.
pub fn route_for_tab(tab: turbogit_app::state::Tab) -> &'static str {
    use turbogit_app::state::Tab as T;
    match tab {
        T::Commit => "changes",
        T::Log => "log",
        T::Branches => "branches",
        T::Worktrees => "worktrees",
        T::Submodules => "submodules",
    }
}

/// The named empty state an unbuilt route paints: the surface's name, the
/// phase that builds it, and the route's test id.
pub fn placeholder(id: &str) -> impl IntoElement {
    let t = theme();
    let route = ROUTES.iter().find(|r| r.id == id);
    let (label, phase) = route
        .map(|r| (r.label, r.phase))
        .unwrap_or(("Unregistered route", "a later phase"));
    view()
        .flex_1()
        .items_center()
        .justify_center()
        .test_id(&test_ids::route(id))
        .child(
            view()
                .flex_col()
                .items_center()
                .gap(px(data::SPACING_Y))
                .child(
                    text(label)
                        .text_size(px(data::TYPE_PANE_TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.foreground),
                )
                .child(
                    text(format!("This surface arrives with {phase}."))
                        .text_size(px(data::TYPE_BODY))
                        .text_color(t.muted_foreground),
                ),
        )
}

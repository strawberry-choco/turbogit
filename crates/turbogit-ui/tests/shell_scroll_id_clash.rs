//! No page paints an egui id-clash error.
//!
//! egui derives a ScrollArea's persistent id from its nesting of anonymous
//! child uis plus the constant default salt, so two un-salted ScrollAreas at
//! the same depth share one id — egui then paints the red "🔥 First/Second use
//! of ScrollArea ID" error over both, and the pair silently scrolls together.
//!
//! Every ScrollArea in the app carries an explicit `id_salt` naming what it
//! shows; this suite walks every tool-window page and sub-tab of the real
//! shell plus the Welcome surfaces — the frames where the always-on shell
//! chrome (sidebar, activity rail) coexists with page scrolls, which is where
//! this class bites — and asserts the debug painter stays quiet (🔥 is egui's
//! own error marker, never app copy), so a future unsalted scroll that lands
//! on an occupied id fails here rather than painting over the user's pane.

use egui_kittest::{Harness, kittest::Queryable};
use test_support::harness::{painted_text, repo_shell_harness, settle, shell_harness};

/// egui's id-clash diagnostics all read "… use of <kind> ID <hash>" and are
/// painted with the 🔥 prefix in the debug layer.
fn clash_text(harness: &Harness<'_, turbogit_app::state::AppState>) -> Vec<String> {
    painted_text(harness)
        .into_iter()
        .filter(|t| {
            t.contains("🔥")
                || t.contains("First use of")
                || t.contains("Second use of")
                || t.contains("Double use of")
        })
        .collect()
}

fn assert_no_clash(harness: &Harness<'_, turbogit_app::state::AppState>, page: &str) {
    let clash = clash_text(harness);
    assert!(
        clash.is_empty(),
        "egui painted an id-clash error on {page}: {clash:?}"
    );
}

#[test]
fn no_tool_page_paints_an_id_clash_error() {
    let (mut h, _project) = repo_shell_harness();
    settle(&mut h);

    for tab in ["Changes", "Log", "Branches", "Worktrees", "Submodules"] {
        h.get_by_label(tab).click();
        h.run();
        assert_no_clash(&h, tab);

        // The Commit tool window's own sub-tab strip swaps its panes in place;
        // each pane brings its own list scroll, so each is checked on its own.
        for sub in ["Local Changes", "Shelf", "Stash"] {
            if h.query_by_label(sub).is_none() {
                continue;
            }
            h.get_by_label(sub).click();
            h.run();
            assert_no_clash(&h, &format!("{tab} > {sub}"));
        }
    }
}

#[test]
fn no_welcome_surface_paints_an_id_clash_error() {
    let (mut h, _project) = shell_harness();
    settle(&mut h);
    assert_no_clash(&h, "Welcome");

    h.state_mut().ui.show_changelog = true;
    h.run();
    assert_no_clash(&h, "Welcome changelog");
}

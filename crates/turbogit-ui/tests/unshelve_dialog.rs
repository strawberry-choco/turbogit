//! Ticket 12, second half — the **Unshelve dialog**.
//!
//! The panes carry the reference's two verbs with their two defaults baked into
//! their labels; the dialog is the surface that carries the reference's *fields*
//! (`shelving:182-190`): `Name`, `Comment`, `Set active`, `Track context`,
//! `Remove successfully applied files from the shelf`. TurboGit has no changelist
//! model (the programme keeps changelists out of scope), so the fields that map
//! onto its model — a destination picked from a list, and the remove-after-apply
//! choice — are implemented, and the ones that do not are recorded as absences
//! rather than built as inert controls. This suite pins both halves.
//!
//! Everything is driven through the production opener (`AppState::open_unshelve_dialog`)
//! and the real shell, and the consequences are read back from **git** — a stash
//! list and a working tree, not a mock.

use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use test_support::git_seed::git;
use test_support::harness::{
    filled_rects, galley_origin, painted_galleys, shell_harness_over_unstyled,
};
use turbogit_app::state::{AppState, Dialog};
use turbogit_domain::model::{RootId, Shelf};
use turbogit_ui::ui::popups::{Action, run_action};

// --------------------------------------------------------------- fixtures --

/// An initialised repository with one commit and repo-local identity, so commits
/// and `git stash push` both work headlessly.
fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    path
}

fn shelf_record(name: &str, root: &Path) -> Shelf {
    Shelf {
        name: name.to_string(),
        changes: Vec::new(),
        created_at: chrono::Utc::now(),
        root: RootId(root.into()),
        // Every row this dialog lists was shelved through git's stash list.
        source: turbogit_domain::model::ShelfSource::Stash,
    }
}

fn app_state(roots: &[PathBuf]) -> AppState {
    let dir = roots
        .first()
        .and_then(|r| r.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    AppState::for_roots(&dir, roots)
}

fn harness(state: AppState) -> Harness<'static, AppState> {
    let mut harness = shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024);
    test_support::harness::settle(&mut harness);
    harness
}

/// Shelve one real change through the production dialog path, so the shelf record
/// and the stash entry holding its content both come from a real click.
fn shelve_one_change(repo: &Path, name: &str) {
    std::fs::write(repo.join("base.txt"), "shelved work\n").unwrap();
    let mut state = app_state(&[repo.to_path_buf()]);
    state.ui.selected.insert(PathBuf::from("base.txt"));
    state.ui.dlg.shelve_name = name.to_string();
    state.ui.dialog = Some(Dialog::Shelve);
    let mut h = harness(state);
    h.get_by_label("Shelve selected").click();
    h.run();
    h.run();
    assert_eq!(stash_lines(repo).len(), 1, "the shelve parked the change");
}

fn stash_lines(repo: &Path) -> Vec<String> {
    git(repo, &["stash", "list"])
        .lines()
        .map(str::to_owned)
        .collect()
}

// ------------------------------------------------------- assertion helpers --

/// The reference's checkbox, spelled exactly as the doc spells it.
const REMOVE_FIELD: &str = "Remove successfully applied files from the shelf";

/// `needle` painted as its own galley inside the dialog's list region: below the
/// `NAME` heading the list sits under, and within the dialog's own width of it.
/// The sub-tab strip paints `Shelf` and the pane (when its tab is open) paints
/// row verbs, so an unscoped match would pass on the shell rather than the dialog.
const DIALOG_FIELD_WINDOW: f32 = 420.0;

fn painted_in_dialog(h: &Harness<'_, AppState>, needle: &str) -> bool {
    let top = galley_origin(h, "NAME").expect("the dialog paints its NAME field heading");
    painted_galleys(h).iter().any(|g| {
        g.text.contains(needle) && g.pos.y > top.y && g.pos.x <= top.x + DIALOG_FIELD_WINDOW
    })
}

/// The exact-text galley exists (a label, not a substring of a longer sentence).
fn painted_exact(h: &Harness<'_, AppState>, text: &str) -> bool {
    painted_galleys(h).iter().any(|g| g.text == text)
}

/// The fill painted behind a control's label: the smallest filled rect the label's
/// own origin sits inside — one `kit_button` paints one fill and one galley.
fn fill_behind(h: &Harness<'_, AppState>, label: &str) -> egui::Color32 {
    let origin = galley_origin(h, label).unwrap_or_else(|| panic!("{label} paints as a label"));
    filled_rects(h)
        .into_iter()
        .filter(|(rect, _)| rect.contains(origin))
        .min_by_key(|(rect, _)| (rect.width() * rect.height()) as i64)
        .map(|(_, fill)| fill)
        .unwrap_or_else(|| panic!("no filled rect behind {label}"))
}

/// Open the dialog the way the app's own routes do, on the focused root.
fn open_dialog(h: &mut Harness<'static, AppState>) {
    h.state_mut().open_unshelve_dialog();
    h.run();
}

// ------------------------------------------------------------------ tests --

#[test]
fn the_name_field_lists_this_roots_shelves_and_not_another_roots() {
    let parent = tempfile::tempdir().unwrap();
    let alpha = temp_repo(parent.path(), "alpha");
    let beta = temp_repo(parent.path(), "beta");
    let mut state = app_state(&[alpha.clone(), beta.clone()]);
    state.selected_root = state.multi.roots.first().map(|r| r.id.clone());
    state.ui.shelves.push(shelf_record("weather", &alpha));
    state.ui.shelves.push(shelf_record("tides", &beta));
    let mut h = harness(state);

    open_dialog(&mut h);

    assert!(
        painted_in_dialog(&h, "weather"),
        "the focused root's shelf is a selectable row in the dialog"
    );
    assert!(
        !painted_in_dialog(&h, "tides"),
        "a shelf is per root: another root's row cannot be an unshelve destination here"
    );
}

#[test]
fn the_remove_option_is_off_every_time_the_dialog_opens() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "default");
    let mut state = app_state(std::slice::from_ref(&repo));
    state.ui.shelves.push(shelf_record("weather", &repo));
    // A previous open left the box ticked; the captured default is re-armed, not inherited.
    state.ui.dlg.unshelve_remove = true;
    let mut h = harness(state);

    open_dialog(&mut h);

    assert!(
        painted_exact(&h, REMOVE_FIELD),
        "the reference's own field is here, spelled as the doc spells it"
    );
    assert!(
        !h.state().ui.dlg.unshelve_remove,
        "`Remove successfully applied files from the shelf` is OFF by default on every open"
    );
    assert!(
        painted_exact(&h, "Unshelve"),
        "with the option off the affirmative keeps the keeping verb's name"
    );
    assert!(
        !painted_exact(&h, "Unshelve and Remove"),
        "and the destructive name is not painted while the option is off"
    );
}

#[test]
fn ticking_the_remove_option_renames_and_recolors_the_affirmative() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "rename");
    let mut state = app_state(std::slice::from_ref(&repo));
    state.ui.shelves.push(shelf_record("weather", &repo));
    let mut h = harness(state);
    open_dialog(&mut h);

    let keep_fill = fill_behind(&h, "Unshelve");
    h.get_by_label(REMOVE_FIELD).click();
    h.run();

    assert!(
        h.state().ui.dlg.unshelve_remove,
        "the click landed on the option"
    );
    assert!(
        painted_exact(&h, "Unshelve and Remove"),
        "the affirmative now says what it will destroy"
    );
    assert!(
        !painted_exact(&h, "Unshelve"),
        "one affirmative, not a keeping button beside a destroying one"
    );
    assert_ne!(
        fill_behind(&h, "Unshelve and Remove"),
        keep_fill,
        "the destructive choice must not wear the keeping choice's fill"
    );
}

#[test]
fn unshelving_through_the_dialog_keeps_the_shelf_and_gits_copy() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "keep");
    shelve_one_change(&repo, "weather");
    let mut h = harness(app_state(std::slice::from_ref(&repo)));
    open_dialog(&mut h);

    h.get_by_label("Unshelve").click();
    h.run();
    h.run();

    assert_eq!(
        h.state().ui.shelves.len(),
        1,
        "the unticked option is the captured default: the shelf record survives"
    );
    assert!(
        !stash_lines(&repo).is_empty(),
        "and so does the stash entry the content lives in"
    );
    assert!(
        git(&repo, &["status", "--porcelain"]).contains("base.txt"),
        "while the work itself lands back in the tree"
    );
    assert!(
        h.state().ui.dialog.is_none(),
        "the dialog is gone once its verb has been dispatched"
    );
}

#[test]
fn ticking_the_option_and_confirming_destroys_the_shelf_and_gits_copy() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "remove");
    shelve_one_change(&repo, "weather");
    let mut h = harness(app_state(std::slice::from_ref(&repo)));
    open_dialog(&mut h);
    h.get_by_label(REMOVE_FIELD).click();
    h.run();

    h.get_by_label("Unshelve and Remove").click();
    h.run();
    h.run();

    assert!(
        h.state().ui.confirm.is_none(),
        "the window itself is the gate on the destructive path: an explicit tick plus a \
         danger-labelled affirmative, and no second confirmation on top of it"
    );

    assert!(
        h.state().ui.shelves.is_empty(),
        "the ticked option takes the shelf record with it"
    );
    assert!(
        stash_lines(&repo).is_empty(),
        "and the stash entry, because the apply landed"
    );
    assert!(
        git(&repo, &["status", "--porcelain"]).contains("base.txt"),
        "while the work itself lands back in the tree"
    );
    let stored = turbogit_services::shelve_stash::load_shelves(parent.path())
        .expect("the store this build wrote parses");
    assert!(
        stored.is_empty(),
        "the store is rewritten without the shelf that was destroyed: {stored:?}"
    );
}

#[test]
fn the_dialog_applies_to_the_root_it_opened_on_not_the_one_focused_now() {
    let parent = tempfile::tempdir().unwrap();
    let alpha = temp_repo(parent.path(), "alpha");
    let beta = temp_repo(parent.path(), "beta");
    shelve_one_change(&alpha, "weather");
    std::fs::write(beta.join("base.txt"), "beta work\n").unwrap();
    git(&beta, &["stash", "push", "-q", "-m", "beta-only"]);
    let mut state = app_state(&[alpha.clone(), beta.clone()]);
    state.selected_root = Some(RootId(alpha.clone().into()));
    let beta_shelf = shelf_record("tides", &beta);
    state.ui.shelves.push(beta_shelf);
    let mut h = harness(state);
    open_dialog(&mut h);

    // Focus moves while the dialog is open — the pane's rows follow the focus, the
    // dialog must not.
    h.state_mut().selected_root = Some(RootId(beta.clone().into()));
    h.run();

    assert!(
        painted_in_dialog(&h, "weather"),
        "the dialog keeps listing the shelves of the root it opened on"
    );
    h.get_by_label("Unshelve").click();
    h.run();
    h.run();

    assert!(
        git(&alpha, &["status", "--porcelain"]).contains("base.txt"),
        "and the work landed in that repository"
    );
    assert!(
        git(&beta, &["status", "--porcelain"]).is_empty(),
        "the newly focused tree is untouched"
    );
    assert_eq!(
        stash_lines(&beta).len(),
        1,
        "and the other root's stash entry is still the one git holds"
    );
}

#[test]
fn the_changelist_fields_are_absent_rather_than_inert() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "buckets");
    let mut state = app_state(std::slice::from_ref(&repo));
    state.ui.shelves.push(shelf_record("weather", &repo));
    let mut h = harness(state);
    open_dialog(&mut h);

    // `Set active` and `Track context` are changelist verbs, and `Comment`
    // describes the changelist the reference creates as the destination. TurboGit
    // has no changelist model, so it ships no control for them: a ticked box that
    // changes nothing is worse than an absent one.
    for absent in ["Set active", "Track context"] {
        assert!(
            !painted_exact(&h, absent),
            "`{absent}` would be an inert control: {:#?}",
            painted_galleys(&h)
        );
    }
    let texts: Vec<String> = painted_galleys(&h)
        .into_iter()
        .map(|g| g.text)
        .filter(|t| t.to_lowercase().contains("changelist"))
        .collect();
    assert!(
        texts.is_empty(),
        "the dialog never promises a changelist it cannot write: {texts:?}"
    );
}

#[test]
fn a_root_with_nothing_shelved_says_so_and_offers_no_verb() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "empty");
    let mut h = harness(app_state(std::slice::from_ref(&repo)));

    open_dialog(&mut h);

    assert!(
        painted_in_dialog(&h, "Nothing is shelved in this repository"),
        "the empty state names the mechanism and the way out of it"
    );
    assert!(
        !painted_exact(&h, "Unshelve"),
        "with nothing to apply the affirmative is withheld, not greyed: an accent fill \
         survives a disabled button and would paint a promise the click cannot keep"
    );
    assert!(painted_exact(&h, "Cancel"), "the way out is always there");
}

#[test]
fn both_the_pane_header_and_the_palette_open_the_same_dialog() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "routes");
    let mut state = app_state(std::slice::from_ref(&repo));
    state.ui.shelves.push(shelf_record("weather", &repo));
    let mut h = harness(state);

    // The Shelf pane's header action.
    h.get_by_label("Shelf").click();
    h.run();
    h.get_by_label("Unshelve\u{2026}").click();
    h.run();
    assert_eq!(
        h.state().ui.dialog,
        Some(Dialog::Unshelve),
        "the pane's own route opens the dialog"
    );

    // And the palette route runs the *same* opener, so neither can arm defaults the
    // other does not.
    h.state_mut().ui.dialog = None;
    run_action(h.state_mut(), Action::Unshelve);
    assert_eq!(
        h.state().ui.dialog,
        Some(Dialog::Unshelve),
        "the menu route reaches the same window"
    );
    assert!(
        !h.state().ui.dlg.unshelve_remove,
        "and arms the same captured default: off"
    );
    assert_eq!(
        h.state().ui.dlg.unshelve_root,
        h.state().selected_root,
        "on the root that was focused when it opened"
    );

    // The command palette is this app's menu route, and the VCS popup's action set
    // is a frozen pin that must not gain a verb here.
    let palette: Vec<&str> = Action::palette_actions()
        .iter()
        .map(|a| a.label())
        .collect();
    assert!(
        palette.contains(&"Unshelve\u{2026}"),
        "the menu route exists because the reference reaches the dialog from a menu: {palette:?}"
    );
    let popup: Vec<&str> = Action::all().iter().map(|a| a.label()).collect();
    assert!(
        !popup.contains(&"Unshelve\u{2026}"),
        "the VCS popup's documented action set stays frozen"
    );
}

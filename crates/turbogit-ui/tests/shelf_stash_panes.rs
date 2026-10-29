use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use test_support::git_seed::{git, pin_eol_lf};
use test_support::harness::{
    assert_not_painted, filled_rects, galley_origin, painted_galleys, shell_harness_over_unstyled,
};
use turbogit_app::state::{AppState, Dialog, PendingConfirm};
use turbogit_domain::model::{RootId, Shelf};

fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    pin_eol_lf(&path);
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

fn open_sub_tab(h: &mut Harness<'static, AppState>, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

const ROW_FIELD_WINDOW: f32 = 300.0;

fn painted_in_pane(h: &Harness<'_, AppState>, needle: &str, anchor: &str) -> bool {
    let top = galley_origin(h, anchor).unwrap_or_else(|| panic!("the {anchor} pane header paints"));
    painted_galleys(h).iter().any(|g| {
        g.text.contains(needle)
            && g.pos.y > top.y
            && g.pos.x >= top.x - 1.0
            && g.pos.x <= top.x + ROW_FIELD_WINDOW
    })
}

fn fill_behind(h: &Harness<'_, AppState>, label: &str) -> egui::Color32 {
    let origin = galley_origin(h, label).unwrap_or_else(|| panic!("{label} paints as a label"));
    filled_rects(h)
        .into_iter()
        .filter(|(rect, _)| rect.contains(origin))
        .min_by_key(|(rect, _)| (rect.width() * rect.height()) as i64)
        .map(|(_, fill)| fill)
        .unwrap_or_else(|| panic!("no filled rect behind {label}"))
}

fn stash_lines(repo: &Path) -> Vec<String> {
    git(repo, &["stash", "list"])
        .lines()
        .map(str::to_owned)
        .collect()
}

fn accept_confirm(h: &mut Harness<'static, AppState>) {
    let confirm: PendingConfirm = h
        .state()
        .ui
        .confirm
        .clone()
        .expect("a verb armed a confirmation");
    h.state_mut().run_confirmed(confirm);
    h.state_mut().ui.confirm = None;
    h.run();
}

#[test]
fn shelf_pane_lists_this_roots_shelves_and_not_another_roots() {
    let parent = tempfile::tempdir().unwrap();
    let alpha = temp_repo(parent.path(), "alpha");
    let beta = temp_repo(parent.path(), "beta");
    let mut state = app_state(&[alpha.clone(), beta.clone()]);
    state.selected_root = state.multi.roots.first().map(|r| r.id.clone());
    state.ui.shelves.push(shelf_record("weather", &alpha));
    state.ui.shelves.push(shelf_record("tides", &beta));
    let mut h = harness(state);

    open_sub_tab(&mut h, "Shelf");

    assert_not_painted(&h, "arrives in a later phase");
    assert!(
        painted_in_pane(&h, "weather", "SHELVES"),
        "the focused root's shelf paints in its own pane"
    );
    assert!(
        !painted_in_pane(&h, "tides", "SHELVES"),
        "a shelf is not global: another root's row must not list here"
    );
}

#[test]
fn shelf_pane_gives_two_unshelve_verbs_with_different_fills() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "verbs");
    let mut state = app_state(std::slice::from_ref(&repo));
    state.ui.shelves.push(shelf_record("weather", &repo));
    let mut h = harness(state);

    open_sub_tab(&mut h, "Shelf");

    let galleys = painted_galleys(&h);
    let unshelve = galleys.iter().any(|g| g.text == "Unshelve");
    let remove = galleys.iter().any(|g| g.text == "Unshelve and Remove");
    assert!(
        unshelve && remove,
        "the keep/remove pair paints as two distinct verbs; painted:\n{galleys:#?}"
    );
    assert_ne!(
        fill_behind(&h, "Unshelve"),
        fill_behind(&h, "Unshelve and Remove"),
        "two identical fills would make the destructive choice indistinguishable"
    );
}

#[test]
fn unshelve_puts_the_work_back_and_keeps_the_shelf_row() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "keep");
    shelve_one_change(&repo, "weather");
    let mut h = harness(app_state(std::slice::from_ref(&repo)));

    open_sub_tab(&mut h, "Shelf");
    assert!(
        painted_in_pane(&h, "weather", "SHELVES"),
        "the shelve the dialog made paints as a shelf row"
    );

    h.get_by_label("Unshelve").click();
    h.run();
    h.run();

    assert!(
        painted_in_pane(&h, "weather", "SHELVES"),
        "Unshelve keeps the shelf"
    );
    assert_eq!(
        h.state().ui.shelves.len(),
        1,
        "the record survives the keeping verb"
    );
    assert!(
        !stash_lines(&repo).is_empty(),
        "git's copy of the shelved content survives too"
    );
    assert!(
        git(&repo, &["status", "--porcelain"]).contains("base.txt"),
        "the shelved change is back in the working tree"
    );
}

#[test]
fn unshelve_and_remove_takes_the_shelf_row_and_gits_copy() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "remove");
    shelve_one_change(&repo, "weather");
    let mut h = harness(app_state(std::slice::from_ref(&repo)));

    open_sub_tab(&mut h, "Shelf");
    h.get_by_label("Unshelve and Remove").click();
    h.run();
    h.run();

    assert!(
        !painted_in_pane(&h, "weather", "SHELVES"),
        "the destructive sibling destroys the shelf"
    );
    assert!(
        h.state().ui.shelves.is_empty(),
        "and the record goes with it"
    );
    assert!(
        stash_lines(&repo).is_empty(),
        "and the stash entry the content lived in"
    );
    assert!(
        git(&repo, &["status", "--porcelain"]).contains("base.txt"),
        "while the work itself lands back in the tree"
    );
}

#[test]
fn stash_pane_lists_the_roots_stash_entries() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "stash-list");
    std::fs::write(repo.join("base.txt"), "half-done work\n").unwrap();
    git(&repo, &["stash", "push", "-q", "-m", "half-done"]);
    let mut h = harness(app_state(std::slice::from_ref(&repo)));

    open_sub_tab(&mut h, "Stash");
    h.run();
    h.run();

    assert!(
        painted_in_pane(&h, "stash@{0}", "STASHES"),
        "the entry's own git ref paints as a row"
    );
    assert!(
        painted_in_pane(&h, "half-done", "STASHES"),
        "and so does the message it was stashed under"
    );
}

#[test]
fn stash_pane_apply_and_pop_are_two_rects_with_two_fills() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "stash-verbs");
    std::fs::write(repo.join("base.txt"), "half-done work\n").unwrap();
    git(&repo, &["stash", "push", "-q", "-m", "half-done"]);
    let mut h = harness(app_state(std::slice::from_ref(&repo)));

    open_sub_tab(&mut h, "Stash");
    h.run();
    h.run();

    assert_ne!(
        fill_behind(&h, "Apply Stash (keeps)"),
        fill_behind(&h, "Pop Stash (removes)"),
        "the keep/remove pair must not be two identical rects"
    );
    let galleys = painted_galleys(&h);
    for label in ["Apply Stash (keeps)", "Pop Stash (removes)", "Delete Stash"] {
        assert!(
            galleys.iter().any(|g| g.text == label),
            "{label} paints in the stash pane; painted:\n{galleys:#?}"
        );
    }
}

#[test]
fn shelf_and_stash_deletes_ask_before_they_destroy() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "confirm");
    std::fs::write(repo.join("base.txt"), "half-done work\n").unwrap();
    git(&repo, &["stash", "push", "-q", "-m", "half-done"]);
    let mut state = app_state(std::slice::from_ref(&repo));
    state.ui.shelves.push(shelf_record("weather", &repo));
    let mut h = harness(state);

    open_sub_tab(&mut h, "Shelf");
    h.get_by_label("Delete Shelf").click();
    h.run();
    assert!(
        matches!(
            h.state().ui.confirm,
            Some(PendingConfirm::DeleteShelf { .. })
        ),
        "the shelf delete raises a confirmation instead of acting"
    );
    assert_eq!(
        h.state().ui.shelves.len(),
        1,
        "and nothing is destroyed while the question is open"
    );
    accept_confirm(&mut h);
    assert!(
        h.state().ui.shelves.is_empty(),
        "the confirmed answer is what removes the record"
    );

    open_sub_tab(&mut h, "Stash");
    h.run();
    h.get_by_label("Delete Stash").click();
    h.run();
    assert!(
        matches!(
            h.state().ui.confirm,
            Some(PendingConfirm::DeleteStash { .. })
        ),
        "the stash delete raises a confirmation too"
    );
    assert_eq!(
        stash_lines(&repo).len(),
        1,
        "the entry survives the question"
    );
    accept_confirm(&mut h);
    assert!(
        stash_lines(&repo).is_empty(),
        "and goes when the confirmation is accepted"
    );
}

#[test]
fn the_two_empty_states_name_their_own_mechanism() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "empty");
    let mut h = harness(app_state(std::slice::from_ref(&repo)));

    open_sub_tab(&mut h, "Shelf");
    assert!(
        painted_in_pane(&h, "Nothing is shelved in this repository", "SHELVES"),
        "the shelf's empty state says what a shelf holds and where to make one"
    );

    open_sub_tab(&mut h, "Stash");
    h.run();
    h.run();
    assert!(
        painted_in_pane(&h, "Nothing is stashed in this repository", "STASHES"),
        "the stash's empty state is its own sentence about its own mechanism"
    );
}

fn select_in_commit_list(state: &mut AppState, name: &str) {
    let key = state
        .multi
        .roots
        .iter()
        .find_map(|r| {
            r.status
                .changes
                .iter()
                .find(|c| c.path.ends_with(name))
                .map(|c| r.canonical_key(c))
        })
        .unwrap_or_else(|| panic!("{name} is a local change of a registered root"));
    state.ui.selected.insert(key);
}

fn shelve_one_change(repo: &Path, name: &str) {
    std::fs::write(repo.join("base.txt"), "shelved work\n").unwrap();
    let mut state = app_state(&[repo.to_path_buf()]);
    select_in_commit_list(&mut state, "base.txt");
    state.ui.dlg.shelve_name = name.to_string();
    state.ui.dialog = Some(Dialog::Shelve);
    let mut h = harness(state);
    h.get_by_label("Shelve selected").click();
    h.run();
    h.run();
    assert_eq!(
        stash_lines(repo).len(),
        1,
        "the shelve parked the change in git's stash stack"
    );
}

#[test]
fn apply_stash_puts_the_work_back_and_keeps_the_entry() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "stash-click");
    std::fs::write(repo.join("base.txt"), "half-done work\n").unwrap();
    git(&repo, &["stash", "push", "-q", "-m", "half-done"]);
    let mut h = harness(app_state(std::slice::from_ref(&repo)));

    open_sub_tab(&mut h, "Stash");
    h.run();
    h.run();
    h.get_by_label("Apply Stash (keeps)").click();
    h.run();
    h.run();

    assert_eq!(
        std::fs::read_to_string(repo.join("base.txt")).unwrap(),
        "half-done work\n",
        "the click put the entry's content back in the tree"
    );
    assert_eq!(
        stash_lines(&repo).len(),
        1,
        "and the keeping half of the pair really keeps: the entry is still git's"
    );
    assert!(
        git(&repo, &["stash", "list"]).contains("half-done"),
        "under the message it was stashed with"
    );
}

#[test]
fn pop_stash_removes_the_entry_its_own_row_names() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "stash-pop");
    std::fs::write(repo.join("base.txt"), "older work\n").unwrap();
    git(&repo, &["stash", "push", "-q", "-m", "older"]);
    std::fs::write(repo.join("base.txt"), "newer work\n").unwrap();
    git(&repo, &["stash", "push", "-q", "-m", "newer"]);
    let mut h = harness(app_state(std::slice::from_ref(&repo)));

    open_sub_tab(&mut h, "Stash");
    h.run();
    h.run();

    let older = galley_origin(&h, "stash@{1}").expect("the second entry paints its ref");
    let pops: Vec<_> = h.get_all_by_label("Pop Stash (removes)").collect();
    assert_eq!(pops.len(), 2, "one destructive verb per row");
    let target = pops
        .iter()
        .min_by_key(|n| (n.rect().center().y - older.y).abs() as i32)
        .expect("a pop on the older entry's line");
    target.click();
    h.run();
    h.run();

    let lines = stash_lines(&repo);
    assert_eq!(lines.len(), 1, "the pop took exactly one entry: {lines:?}");
    assert!(
        lines[0].contains("newer"),
        "and the survivor is the entry that was NOT clicked: {lines:?}"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("base.txt")).unwrap(),
        "older work\n",
        "the work that came back is the clicked row's own content"
    );
}

#[test]
fn the_shelf_pane_header_reaches_the_shelve_dialog() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "shelve-header");
    let mut state = app_state(std::slice::from_ref(&repo));
    state.ui.shelves.push(shelf_record("weather", &repo));
    let mut h = harness(state);

    open_sub_tab(&mut h, "Shelf");
    assert_eq!(
        h.state().ui.dialog,
        None,
        "listing the pane opens no window of its own"
    );

    h.get_by_label("Shelve\u{2026}").click();
    h.run();

    assert_eq!(
        h.state().ui.dialog,
        Some(Dialog::Shelve),
        "the pane's own header button is a route to the shelve dialog, not a painted \
         decoration"
    );
}

#[test]
fn the_stash_pane_header_reaches_the_stash_dialog() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "stash-header");
    std::fs::write(repo.join("base.txt"), "half-done work\n").unwrap();
    git(&repo, &["stash", "push", "-q", "-m", "half-done"]);
    let mut h = harness(app_state(std::slice::from_ref(&repo)));

    open_sub_tab(&mut h, "Stash");
    h.run();
    h.run();
    assert_eq!(h.state().ui.dialog, None, "and the same for the stash pane");

    h.get_by_label("Stash\u{2026}").click();
    h.run();

    assert_eq!(
        h.state().ui.dialog,
        Some(Dialog::Stash),
        "the stash pane's header opens the park-every-changes window"
    );
}

#[test]
fn a_failed_unshelve_and_remove_leaves_the_row_painted() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "pop-failed");
    let mut state = app_state(std::slice::from_ref(&repo));
    state.ui.shelves.push(shelf_record("weather", &repo));
    let mut h = harness(state);

    open_sub_tab(&mut h, "Shelf");
    h.get_by_label("Unshelve and Remove").click();
    h.run();
    h.run();

    assert!(
        painted_in_pane(&h, "weather", "SHELVES"),
        "a pop that failed leaves the row painted — the work is still wherever it is"
    );
    assert_eq!(
        h.state().ui.shelves.len(),
        1,
        "the record goes with the failure, not with the intent"
    );
    assert!(
        h.state().ui.shelf_removal_pending.is_none(),
        "and the armed removal is disarmed, so no later success can spend it"
    );
}

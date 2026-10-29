use std::path::{Path, PathBuf};
use std::sync::Arc;

use test_support::git_seed::{commit, git, pin_eol_lf, repo_with_conflict, repo_with_one_commit};
use turbogit_app::diff_model::{DiffModel, DiffValue, DisplayRow, Row, RowKind};
use turbogit_app::granular::{self, Direction, HunkTarget};
use turbogit_app::keyed_read::{DiffTarget, Read};
use turbogit_app::root_caches::RootHunkStats;
use turbogit_app::staged_lines;
use turbogit_app::state::AppState;
use turbogit_domain::model::{ChangeRef, DiffPolicies, RootId};

fn root_id(repo: &Path) -> RootId {
    RootId(repo.to_path_buf().into())
}

fn change(repo: &Path, rel: &str) -> ChangeRef {
    ChangeRef::new(root_id(repo), rel)
}

fn app_state(project: &Path, repo: &Path) -> AppState {
    AppState::for_roots(project, std::slice::from_ref(&repo.to_path_buf()))
}

fn stats<'a>(state: &'a mut AppState, repo: &Path) -> &'a RootHunkStats {
    let root = root_id(repo);
    state.ensure_hunk_stats(&root);
    state
        .caches
        .hunk_stats(&root)
        .expect("the root's entry fills on ask")
}

fn combined_value(state: &mut AppState, repo: &Path, rel: &str) -> Arc<DiffValue> {
    state.ensure_hunk_stats(&root_id(repo));
    let target = DiffTarget::working_tree(
        repo.to_path_buf(),
        DiffPolicies::default(),
        Some(PathBuf::from(rel)),
    );
    for _ in 0..4 {
        if let Read::Fresh(value) = state.read(target.clone()) {
            return value;
        }
        state.drain_events();
    }
    panic!("the pinned preview read never settles for {rel}");
}

fn rows(model: &DiffModel) -> Vec<&Row> {
    model
        .display
        .iter()
        .flat_map(|row| match row {
            DisplayRow::Full(r) => vec![r],
            DisplayRow::Pair(old, new) => old.iter().chain(new.iter()).collect(),
        })
        .collect()
}

fn geometry(model: &DiffModel) -> Vec<(RowKind, String, usize, usize)> {
    rows(model)
        .into_iter()
        .filter(|r| matches!(r.kind, RowKind::Del | RowKind::Add))
        .map(|r| (r.kind, r.text.clone(), r.old_no, r.new_no))
        .collect()
}

fn held(row: &Row) -> Row {
    Row {
        kind: row.kind,
        text: row.text.clone(),
        hunk: row.hunk,
        line_ord: row.line_ord,
        old_no: row.old_no,
        new_no: row.new_no,
        span: row.span,
        highlight: row.highlight.clone(),
    }
}

fn only_row<'a>(model: &'a DiffModel, kind: RowKind, text: &str) -> &'a Row {
    let found: Vec<&Row> = rows(model)
        .into_iter()
        .filter(|r| r.kind == kind && r.text == text)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "exactly one {kind:?} row named {text:?}, got {:?}",
        geometry(model)
    );
    found[0]
}

fn first_row<'a>(model: &'a DiffModel, kind: RowKind, text: &str) -> &'a Row {
    rows(model)
        .into_iter()
        .find(|r| r.kind == kind && r.text == text)
        .unwrap_or_else(|| panic!("no {kind:?} row named {text:?} in {:?}", geometry(model)))
}

fn body_line(body: &str, sign: char, text: &str) -> usize {
    body.lines()
        .filter(|l| l.starts_with(sign) && l.get(1..).is_some_and(|rest| rest == text))
        .count()
}

fn staged(repo: &Path, rel: &str) -> String {
    git(repo, &["diff", "--cached", "--", rel])
}

fn unstaged(repo: &Path, rel: &str) -> String {
    git(repo, &["diff", "--", rel])
}

fn porcelain(repo: &Path, rel: &str) -> String {
    let all = git(repo, &["status", "--porcelain"]);
    all.lines()
        .find(|l| l[3..].trim() == rel)
        .map(|l| l[..2].to_owned())
        .unwrap_or_else(|| panic!("{rel} absent from status:\n{all}"))
}

fn index_bytes(repo: &Path, rel: &str) -> String {
    git(repo, &["show", &format!(":{rel}")])
}

const BASE: &str = concat!(
    "alpha\n",
    "bravo\n",
    "charlie\n",
    "delta\n",
    "echo\n",
    "foxtrot\n",
    "golf\n",
    "hotel\n",
    "india\n",
    "juliet\n",
    "kilo\n",
    "lima\n",
    "mike\n",
    "november\n",
    "oscar\n",
    "papa\n",
    "quebec\n",
    "romeo\n",
    "sierra\n",
    "tango\n",
);

#[test]
fn unstaging_one_staged_line_from_the_combined_view_reaches_git() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "regression");
    commit(&repo, "f.txt", BASE, "f base");
    let all_three = BASE
        .replace("bravo\n", "BRAVO\n")
        .replace("juliet\n", "juliet\nuniform\n")
        .replace("tango\n", "");
    std::fs::write(repo.join("f.txt"), &all_three).unwrap();
    git(&repo, &["add", "--", "f.txt"]);

    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let row = held(only_row(&value.model, RowKind::Add, "BRAVO"));
    assert_eq!(
        stats(&mut state, &repo).row_is_staged(Path::new("f.txt"), &row),
        Some(true),
        "a fully staged file answers every row staged, geometry={:?}",
        geometry(&value.model)
    );

    granular::toggle_row(&mut state, &Some(change(&repo, "f.txt")), &row);
    state.drain_events();

    let after = staged(&repo, "f.txt");
    assert_eq!(
        (
            body_line(&after, '+', "BRAVO"),
            body_line(&after, '-', "bravo"),
        ),
        (0, 0),
        "the clicked pair must leave the index together:\n{after}"
    );
    assert_eq!(
        (
            body_line(&after, '+', "uniform"),
            body_line(&after, '-', "tango")
        ),
        (1, 1),
        "and each other staged hunk stays staged:\n{after}"
    );
    assert_eq!(
        body_line(&unstaged(&repo, "f.txt"), '+', "BRAVO"),
        1,
        "the unstaged half is now the worktree's alone"
    );
    assert_eq!(porcelain(&repo, "f.txt"), "MM", "partly staged, partly not");
}

#[test]
fn unstaging_one_staged_row_of_a_hunk_leaves_the_other_staged_change_staged() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "mixed-row");
    commit(&repo, "f.txt", BASE, "f base");
    let both = BASE
        .replace("charlie\n", "CHARLIE\n")
        .replace("delta\n", "delta\ndupadd\n");
    std::fs::write(repo.join("f.txt"), &both).unwrap();
    git(&repo, &["add", "--", "f.txt"]);

    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let row = held(only_row(&value.model, RowKind::Add, "CHARLIE"));
    assert_eq!(
        stats(&mut state, &repo).row_is_staged(Path::new("f.txt"), &row),
        Some(true)
    );

    granular::toggle_row(&mut state, &Some(change(&repo, "f.txt")), &row);
    state.drain_events();

    let after = staged(&repo, "f.txt");
    assert_eq!(
        (
            body_line(&after, '+', "CHARLIE"),
            body_line(&after, '-', "charlie"),
        ),
        (0, 0),
        "the clicked pair reversed out of the index:\n{after}"
    );
    assert_eq!(
        body_line(&after, '+', "dupadd"),
        1,
        "and the sibling staged addition in the same hunk stayed staged:\n{after}"
    );
    assert_eq!(
        index_bytes(&repo, "f.txt").matches("dupadd").count(),
        1,
        "the index holds charlie's restored line plus the still-staged dupadd"
    );
}

#[test]
fn two_staged_modifications_unstage_the_clicked_one_only() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "two-mods");
    commit(&repo, "f.txt", BASE, "f base");
    let both = BASE
        .replace("echo\n", "ECHO\n")
        .replace("romeo\n", "ROMEO\n");
    std::fs::write(repo.join("f.txt"), &both).unwrap();
    git(&repo, &["add", "--", "f.txt"]);

    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let row = held(only_row(&value.model, RowKind::Add, "ECHO"));

    granular::toggle_row(&mut state, &Some(change(&repo, "f.txt")), &row);
    state.drain_events();

    let after = staged(&repo, "f.txt");
    assert_eq!(
        (
            body_line(&after, '+', "ECHO"),
            body_line(&after, '-', "echo"),
            body_line(&after, '+', "ROMEO"),
            body_line(&after, '-', "romeo"),
        ),
        (0, 0, 1, 1),
        "the clicked modification reversed and the other stayed staged:\n{after}"
    );
    assert_eq!(
        porcelain(&repo, "f.txt"),
        "MM",
        "one reverted, one still staged"
    );
}

#[test]
fn the_displayed_bytes_do_not_move_while_the_answer_for_that_row_flips() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "byte-stable");
    commit(&repo, "f.txt", BASE, "f base");
    std::fs::write(repo.join("f.txt"), format!("{BASE}uniform\n")).unwrap();

    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let row = held(only_row(&value.model, RowKind::Add, "uniform"));
    let before = (
        row.hunk,
        row.line_ord,
        row.old_no,
        row.new_no,
        git(&repo, &["diff", "HEAD", "--", "f.txt"]),
    );
    assert_eq!(
        stats(&mut state, &repo).row_is_staged(Path::new("f.txt"), &row),
        Some(false)
    );

    granular::toggle_row(&mut state, &Some(change(&repo, "f.txt")), &row);
    state.drain_events();

    let again = combined_value(&mut state, &repo, "f.txt");
    let after = only_row(&again.model, RowKind::Add, "uniform");
    assert_eq!(
        (after.hunk, after.line_ord, after.old_no, after.new_no),
        (before.0, before.1, before.2, before.3),
        "the row the user clicked keeps its hunk, its ordinal and both gutters"
    );
    assert_eq!(
        geometry(&again.model),
        vec![(RowKind::Add, "uniform".to_owned(), 0, 21)],
        "no row appeared, moved or vanished"
    );
    assert_eq!(
        git(&repo, &["diff", "HEAD", "--", "f.txt"]),
        before.4,
        "and the displayed patch is byte-identical"
    );
    assert_eq!(
        stats(&mut state, &repo).row_is_staged(Path::new("f.txt"), after),
        Some(true),
        "while the answer for that row flips"
    );
}

#[test]
fn a_row_toggled_both_ways_returns_the_index_to_its_original_bytes() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "round-trip");
    commit(&repo, "f.txt", BASE, "f base");
    std::fs::write(repo.join("f.txt"), format!("{BASE}uniform\n")).unwrap();
    let change = Some(change(&repo, "f.txt"));
    let before = index_bytes(&repo, "f.txt");

    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let row = held(only_row(&value.model, RowKind::Add, "uniform"));
    granular::toggle_row(&mut state, &change, &row);
    state.drain_events();
    assert_eq!(
        body_line(&staged(&repo, "f.txt"), '+', "uniform"),
        1,
        "the first click stages it"
    );

    let value = combined_value(&mut state, &repo, "f.txt");
    let row = held(only_row(&value.model, RowKind::Add, "uniform"));
    granular::toggle_row(&mut state, &change, &row);
    state.drain_events();

    assert_eq!(
        index_bytes(&repo, "f.txt"),
        before,
        "the second click puts the index back exactly where it was"
    );
    assert!(staged(&repo, "f.txt").is_empty(), "nothing staged left");
    assert_eq!(porcelain(&repo, "f.txt"), " M", "only the worktree differs");
}

#[test]
fn a_mixed_hunk_through_the_gutter_plus_then_minus_moves_whole_and_back() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "mixed-hunk");
    commit(&repo, "f.txt", BASE, "f base");
    let half = BASE.replace("charlie\n", "CHARLIE\n");
    std::fs::write(repo.join("f.txt"), &half).unwrap();
    git(&repo, &["add", "--", "f.txt"]);
    std::fs::write(
        repo.join("f.txt"),
        half.replace("delta\n", "delta\nnewly\n"),
    )
    .unwrap();

    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    assert_eq!(
        geometry(&value.model).len(),
        3,
        "one pair and one lone addition, in one hunk: {:?}",
        geometry(&value.model)
    );

    granular::dispatch(
        &mut state,
        change(&repo, "f.txt"),
        HunkTarget::Whole(0, Direction::Stage),
    );
    state.drain_events();
    let after_stage = staged(&repo, "f.txt");
    assert_eq!(
        (
            body_line(&after_stage, '+', "CHARLIE"),
            body_line(&after_stage, '+', "newly"),
        ),
        (1, 1),
        "the hunk's unstaged row arrived and its staged row stayed:\n{after_stage}"
    );
    assert_eq!(porcelain(&repo, "f.txt"), "M ", "the hunk is wholly staged");
    assert_eq!(
        staged_lines::find(&stats(&mut state, &repo).line_sets, Path::new("f.txt"))
            .expect("listed")
            .local
            .added
            .len(),
        0,
        "and the local section has nothing left to stage"
    );

    granular::dispatch(
        &mut state,
        change(&repo, "f.txt"),
        HunkTarget::Whole(0, Direction::Unstage),
    );
    state.drain_events();
    assert!(
        staged(&repo, "f.txt").is_empty(),
        "the gutter − takes the hunk back out:\n{}",
        staged(&repo, "f.txt")
    );
    assert_eq!(
        porcelain(&repo, "f.txt"),
        " M",
        "back to a worktree-only change"
    );
}

#[test]
fn a_row_click_below_a_staged_insertion_stages_the_line_the_reader_clicked() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "shift-down");
    commit(&repo, "f.txt", BASE, "f base");
    let inserted = BASE.replace("juliet\n", "juliet\nuniform\n");
    std::fs::write(repo.join("f.txt"), &inserted).unwrap();
    git(&repo, &["add", "--", "f.txt"]);
    std::fs::write(repo.join("f.txt"), inserted.replace("sierra\n", "SIERRA\n")).unwrap();

    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let row = held(only_row(&value.model, RowKind::Del, "sierra"));
    assert_eq!(row.old_no, 19, "the pane's gutter is a HEAD number");
    assert_eq!(
        stats(&mut state, &repo).row_is_staged(Path::new("f.txt"), &row),
        Some(false)
    );

    granular::toggle_row(&mut state, &Some(change(&repo, "f.txt")), &row);
    state.drain_events();

    let after = staged(&repo, "f.txt");
    assert_eq!(
        (
            body_line(&after, '-', "sierra"),
            body_line(&after, '+', "SIERRA"),
            body_line(&after, '+', "uniform"),
        ),
        (1, 0, 1),
        "the translated click reaches exactly the clicked removal, and the \
         staged insertion above it survives:\n{after}"
    );
}

#[test]
fn a_row_click_below_a_staged_deletion_stages_the_line_the_reader_clicked() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "shift-up");
    commit(&repo, "f.txt", BASE, "f base");
    let without = BASE.replace("bravo\n", "");
    std::fs::write(repo.join("f.txt"), &without).unwrap();
    git(&repo, &["add", "--", "f.txt"]);
    std::fs::write(repo.join("f.txt"), without.replace("oscar\n", "OSCAR\n")).unwrap();

    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let row = held(only_row(&value.model, RowKind::Del, "oscar"));
    assert_eq!(row.old_no, 15, "HEAD 15");

    granular::toggle_row(&mut state, &Some(change(&repo, "f.txt")), &row);
    state.drain_events();

    let after = staged(&repo, "f.txt");
    assert_eq!(
        (
            body_line(&after, '-', "oscar"),
            body_line(&after, '+', "OSCAR"),
            body_line(&after, '-', "bravo"),
        ),
        (1, 0, 1),
        "the untranslated ask would have addressed index 15, which is `papa`, \
         and the index would have lost the wrong line:\n{after}"
    );
}

#[test]
fn two_identical_adjacent_additions_stage_and_unstage_apart() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "duplicates");
    commit(&repo, "f.txt", BASE, "f base");
    std::fs::write(
        repo.join("f.txt"),
        BASE.replace("echo\n", "echo\ndup\ndup\n"),
    )
    .unwrap();

    let change = Some(change(&repo, "f.txt"));
    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let copies: Vec<Row> = rows(&value.model)
        .into_iter()
        .filter(|r| r.kind == RowKind::Add)
        .map(held)
        .collect();
    assert_eq!(copies.len(), 2);
    assert_eq!(copies[0].text, copies[1].text, "identical text");
    assert_eq!(copies[0].new_no + 1, copies[1].new_no);

    for (which, want) in [(0usize, 1usize), (1usize, 2usize)] {
        assert_eq!(
            stats(&mut state, &repo).row_is_staged(Path::new("f.txt"), &copies[which]),
            Some(false),
            "copy {which} is still unstaged"
        );
        granular::toggle_row(&mut state, &change, &copies[which]);
        state.drain_events();
        assert_eq!(
            body_line(&staged(&repo, "f.txt"), '+', "dup"),
            want,
            "copy {which} staged on its own"
        );
    }
    assert_eq!(porcelain(&repo, "f.txt"), "M ", "both copies staged");

    let value = combined_value(&mut state, &repo, "f.txt");
    assert_eq!(
        rows(&value.model)
            .into_iter()
            .filter(|r| r.kind == RowKind::Add)
            .count(),
        2,
        "neither copy left the view"
    );
    let row = held(first_row(&value.model, RowKind::Add, "dup"));
    granular::toggle_row(&mut state, &change, &row);
    state.drain_events();
    assert_eq!(
        body_line(&staged(&repo, "f.txt"), '+', "dup"),
        1,
        "one click on one copy took just that copy back out:\n{}",
        staged(&repo, "f.txt")
    );
    assert_eq!(porcelain(&repo, "f.txt"), "MM", "one copy staged, one not");

    let value = combined_value(&mut state, &repo, "f.txt");
    let row = held(first_row(&value.model, RowKind::Add, "dup"));
    granular::toggle_row(&mut state, &change, &row);
    state.drain_events();
    assert!(
        staged(&repo, "f.txt").is_empty(),
        "the second click took the last copy out too:\n{}",
        staged(&repo, "f.txt")
    );
    assert_eq!(
        body_line(&unstaged(&repo, "f.txt"), '+', "dup"),
        2,
        "both copies are unstaged again"
    );
}

#[test]
fn a_repository_with_no_commits_offers_no_rows_to_click() {
    let project = tempfile::tempdir().unwrap();
    let work = project.path().join("unborn");
    git(
        project.path(),
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
    );
    git(&work, &["config", "user.email", "test@example.com"]);
    git(&work, &["config", "user.name", "Test"]);
    pin_eol_lf(&work);
    std::fs::write(work.join("f.txt"), "alpha\nbravo\n").unwrap();
    git(&work, &["add", "--", "f.txt"]);

    let mut state = app_state(project.path(), &work);
    let target = DiffTarget::working_tree(
        work.clone(),
        DiffPolicies::default(),
        Some(PathBuf::from("f.txt")),
    );
    let mut verdict = None;
    for _ in 0..4 {
        match state.read(target.clone()) {
            Read::Fresh(_) | Read::Empty => {
                panic!("an unborn preview must not answer rows to click");
            }
            Read::Failed(message) => {
                verdict = Some(message);
                break;
            }
            Read::Waiting => {
                state.drain_events();
            }
        }
    }
    assert_eq!(
        verdict.as_deref(),
        Some(
            "this repository has no commits yet: there is no first commit to compare \
             your changes against"
        ),
        "the pane's failed verdict is the sentence the engine produces"
    );

    assert_eq!(
        git(&work, &["ls-files"]).trim(),
        "f.txt",
        "the staged file stays staged"
    );
    assert_eq!(
        git(&work, &["status", "--porcelain"]).trim(),
        "A  f.txt",
        "and git says so"
    );
}

#[test]
fn unstaging_one_row_of_a_staged_creation_takes_the_whole_file_out() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "staged-creation");
    std::fs::write(repo.join("new.txt"), "alpha\nbravo\n").unwrap();
    git(&repo, &["add", "--", "new.txt"]);

    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "new.txt");
    let row = held(only_row(&value.model, RowKind::Add, "bravo"));
    assert_eq!(
        stats(&mut state, &repo).row_is_staged(Path::new("new.txt"), &row),
        Some(true),
        "geometry={:?}",
        geometry(&value.model)
    );

    granular::toggle_row(&mut state, &Some(change(&repo, "new.txt")), &row);
    state.drain_events();

    assert_eq!(
        git(&repo, &["ls-files", "--", "new.txt"]).trim(),
        "",
        "a creation reverse-applies whole, so the file leaves the index entirely"
    );
    assert_eq!(
        git(&repo, &["status", "--porcelain", "--", "new.txt"]).trim(),
        "?? new.txt",
        "and git says so"
    );
}

#[test]
fn a_row_staged_then_edited_again_flips_to_unstaged_and_stays_in_the_view() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "re-edited");
    commit(&repo, "f.txt", BASE, "f base");
    let with_echo = BASE.replace("echo\n", "ECHO\n");
    std::fs::write(repo.join("f.txt"), &with_echo).unwrap();
    git(&repo, &["add", "--", "f.txt"]);
    std::fs::write(repo.join("f.txt"), with_echo.replace("ECHO\n", "ECHO2\n")).unwrap();

    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let row = held(only_row(&value.model, RowKind::Add, "ECHO2"));
    assert_eq!(
        stats(&mut state, &repo).row_is_staged(Path::new("f.txt"), &row),
        Some(false),
        "the re-edited row shows text the index does not hold, geometry={:?}",
        geometry(&value.model)
    );

    granular::toggle_row(&mut state, &Some(change(&repo, "f.txt")), &row);
    state.drain_events();

    let after = staged(&repo, "f.txt");
    assert_eq!(
        (
            body_line(&after, '+', "ECHO2"),
            body_line(&after, '+', "ECHO"),
        ),
        (1, 1),
        "staging the worktree text of a re-edited line leaves the staged text \
         beside it, which is exactly what git then reports:\n{after}"
    );
    assert_eq!(porcelain(&repo, "f.txt"), "MM", "two states, not a third");
    let value = combined_value(&mut state, &repo, "f.txt");
    assert_eq!(
        body_line(&git(&repo, &["diff", "HEAD", "--", "f.txt"]), '+', "ECHO2"),
        1,
        "and the row is still in the view: {:?}",
        geometry(&value.model)
    );
}

#[test]
fn staging_one_half_of_a_replacement_pair_leaves_the_file_mm() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "mm-decision");
    commit(&repo, "f.txt", BASE, "f base");
    std::fs::write(repo.join("f.txt"), BASE.replace("echo\n", "ECHO\n")).unwrap();

    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let row = held(only_row(&value.model, RowKind::Add, "ECHO"));
    granular::toggle_row(&mut state, &Some(change(&repo, "f.txt")), &row);
    state.drain_events();

    let after = staged(&repo, "f.txt");
    assert_eq!(
        (
            body_line(&after, '+', "ECHO"),
            body_line(&after, '-', "echo"),
        ),
        (1, 0),
        "the clicked addition alone enters the index; its pair stays in the \
         worktree:\n{after}"
    );
    assert_eq!(
        body_line(&unstaged(&repo, "f.txt"), '-', "echo"),
        1,
        "and the deletion is still an unstaged change"
    );
    assert_eq!(porcelain(&repo, "f.txt"), "MM", "the meaning that ships");
}

#[test]
fn a_row_click_on_a_conflicted_path_does_nothing_at_all() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_conflict(project.path(), "conflicted-click");

    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "conf.txt");
    let before = staged(&repo, "conf.txt");
    let rows: Vec<Row> = rows(&value.model).into_iter().map(held).collect();
    assert!(
        rows.iter().any(|r| r.kind == RowKind::Add),
        "the displayed read shows the conflict markers as rows, geometry={:?}",
        geometry(&value.model)
    );
    assert_eq!(
        stats(&mut state, &repo).row_is_staged(Path::new("conf.txt"), &rows[0]),
        None,
        "no answer for any row of it"
    );

    for row in rows
        .iter()
        .filter(|r| matches!(r.kind, RowKind::Add | RowKind::Del))
    {
        granular::toggle_row(&mut state, &Some(change(&repo, "conf.txt")), row);
        state.drain_events();
    }

    assert_eq!(staged(&repo, "conf.txt"), before, "no index movement");
    assert!(state.ui.toast.is_none(), "and no error toast");
    assert!(state.caches.hunk_stats(&root_id(&repo)).is_some());
}

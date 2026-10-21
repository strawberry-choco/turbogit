//! The combined preview's row and hunk clicks, graded against real git.
//!
//! Every fixture is a temporary repository driven by the CLI executor through
//! the real [`AppState`], per `AGENTS.md`: rows taken from the display model the
//! pinned keyed read answers with, the click routed through
//! [`granular::dispatch`]/[`granular::toggle_row`] and settled by the production
//! `OpCompleted` → refresh → drain path, and every verdict read back out of
//! `git diff --cached`, `git diff`, `git status --porcelain` and `git show :path`
//! rather than out of an internal return.
//!
//! Two shapes of answer show up throughout, and they are the ticket:
//!
//! - A **stage** composes the single located ordinal of the `local` section.
//!   Measured over nine hunk shapes, every single forward-apply ordinal git
//!   accepts, because `filter_lines` never removes a line from the pre-image a
//!   forward apply matches. Staging is therefore row-granular.
//! - An **unstage** composes the clicked line's change unit of the `staged`
//!   section, and `filter_lines`' reverse branch keeps every other staged change
//!   of the hunk as post-image context so it stays staged. git will not take half
//!   of a replacement pair (a lone `+` reverse-deletes instead of restoring, a
//!   lone `-` leaves both texts), so the unit is a `-`/`+` pair, or a same-kind
//!   run, and never reaches a neighbouring change. Unstaging is therefore
//!   row-granular too, at the unit granularity git actually requires.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use test_support::git_seed::{commit, git, pin_eol_lf, repo_with_conflict, repo_with_one_commit};
use turbogit_app::diff_model::{DiffModel, DiffValue, DisplayRow, Row, RowKind};
use turbogit_app::granular::{self, Direction, HunkTarget};
use turbogit_app::keyed_read::{DiffTarget, Read};
use turbogit_app::root_caches::RootHunkStats;
use turbogit_app::staged_lines;
use turbogit_app::state::AppState;
use turbogit_domain::model::{DiffPolicies, RootId};

// ---------------------------------------------------------------- helpers --

fn root_id(repo: &Path) -> RootId {
    RootId(repo.to_path_buf().into())
}

fn app_state(project: &Path, repo: &Path) -> AppState {
    AppState::for_roots(project, std::slice::from_ref(&repo.to_path_buf()))
}

/// Fill `repo`'s hunk statistics the way the app fills them on ask. This is the
/// only warm-up a granular op needs: composition reads the root caches and never
/// the displayed value.
fn stats<'a>(state: &'a mut AppState, repo: &Path) -> &'a RootHunkStats {
    let root = root_id(repo);
    state.ensure_hunk_stats(&root);
    state
        .caches
        .hunk_stats(&root)
        .expect("the root's entry fills on ask")
}

/// The pinned working-tree read's answer for one file: the rows the pane paints,
/// with the gutters it paints them with.
///
/// It also fills the root's hunk statistics first, which is what the commit
/// window does on the frame that renders a preview
/// (`commit_window.rs`, `state.ensure_hunk_stats`), because a granular op
/// composes from those retained sections. The cold-cache case is pinned
/// separately, in `granular_ops.rs`.
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

/// A row outlives the model it came from only by value, and `Row` is not
/// `Clone`: this is the copy a test makes so it can hold one across a refresh.
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

/// The one changed row of `kind` whose text is `text`.
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

/// The first changed row of `kind` whose text is `text`, for the fixtures where
/// two rows legitimately carry identical text.
fn first_row<'a>(model: &'a DiffModel, kind: RowKind, text: &str) -> &'a Row {
    rows(model)
        .into_iter()
        .find(|r| r.kind == kind && r.text == text)
        .unwrap_or_else(|| panic!("no {kind:?} row named {text:?} in {:?}", geometry(model)))
}

/// How many body lines of `body` are the signed `text`, exactly.
///
/// Exact rather than a prefix match, because a fixture holding both `+ECHO` and
/// `+ECHO2` must not read as two `+ECHO`.
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

/// The index's own bytes for `rel`, which is what a "returns to what it was"
/// assertion is really about.
fn index_bytes(repo: &Path, rel: &str) -> String {
    git(repo, &["show", &format!(":{rel}")])
}

/// 20 lines, `alpha` to `tango`.
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

// ------------------------------------------------------------------ tests --

/// Three changes, all staged, one line unstaged from the combined view, graded
/// against `git diff --cached`. The click reverse-applies the retained `staged`
/// section, whose post-image is the index. Composing the same click from the
/// displayed patch instead hands git a new side the index never held, because
/// that post-image is the worktree, and a reverse apply matches new sides
/// against the index: `error: patch failed`, and the index unmoved. The other
/// two staged changes stay in their own hunks.
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

    granular::toggle_row(&mut state, &Some(PathBuf::from("f.txt")), &row);
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

/// Row-level unstage inside one hunk: a staged modification and a staged addition
/// share a hunk, and clicking the modification's row leaves the addition staged.
/// The unselected addition survives the reverse apply as a post-image context
/// line, so it stays in the index while only the clicked pair reverses. A
/// composer without the reverse branch cannot express this shape at all.
#[test]
fn unstaging_one_staged_row_of_a_hunk_leaves_the_other_staged_change_staged() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "mixed-row");
    commit(&repo, "f.txt", BASE, "f base");
    // A modification (`charlie` to `CHARLIE`) and an addition (`dupadd`) close
    // enough that git prints them in one hunk, both staged.
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

    granular::toggle_row(&mut state, &Some(PathBuf::from("f.txt")), &row);
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

/// Two staged modifications in different hunks: clicking one reverses it and the
/// other stays staged, so an unstage no longer drags the whole file's changes with
/// it. Each is its own replacement pair, addressed independently.
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

    granular::toggle_row(&mut state, &Some(PathBuf::from("f.txt")), &row);
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

/// The property that makes immediate clicking survivable, pinned rather than
/// asserted in prose: the displayed patch bytes do not move while the oracle's
/// answer for that row flips. Rows cannot shift, disappear or renumber under the
/// only action the pane offers.
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

    granular::toggle_row(&mut state, &Some(PathBuf::from("f.txt")), &row);
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

/// Direction both ways on the same row: one click stages, the next unstages, and
/// the index is left holding exactly the bytes it started with.
#[test]
fn a_row_toggled_both_ways_returns_the_index_to_its_original_bytes() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "round-trip");
    commit(&repo, "f.txt", BASE, "f base");
    std::fs::write(repo.join("f.txt"), format!("{BASE}uniform\n")).unwrap();
    let path = Some(PathBuf::from("f.txt"));
    let before = index_bytes(&repo, "f.txt");

    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let row = held(only_row(&value.model, RowKind::Add, "uniform"));
    granular::toggle_row(&mut state, &path, &row);
    state.drain_events();
    assert_eq!(
        body_line(&staged(&repo, "f.txt"), '+', "uniform"),
        1,
        "the first click stages it"
    );

    let value = combined_value(&mut state, &repo, "f.txt");
    let row = held(only_row(&value.model, RowKind::Add, "uniform"));
    granular::toggle_row(&mut state, &path, &row);
    state.drain_events();

    assert_eq!(
        index_bytes(&repo, "f.txt"),
        before,
        "the second click puts the index back exactly where it was"
    );
    assert!(staged(&repo, "f.txt").is_empty(), "nothing staged left");
    assert_eq!(porcelain(&repo, "f.txt"), " M", "only the worktree differs");
}

/// A mixed hunk through the gutter pair. `+` stages the hunk's unstaged rows out
/// of the `local` section and `−` takes the whole hunk back out of the `staged`
/// one. The gutter verb is a whole-hunk button, so it composes
/// `HunkSelection::Whole` for the covered hunk, which reverse-applies HEAD's
/// content for that hunk and takes every staged row of it with it.
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
        PathBuf::from("f.txt"),
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
        PathBuf::from("f.txt"),
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

/// Translation under a preceding **staged** insertion: the row's HEAD gutter is
/// not its index gutter, and the click has to hop through the staged sets before
/// it addresses the `local` section. An unstaged insertion above a line would not
/// shift this relation at all.
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
    // HEAD 19 `sierra` is index 20: the staged insertion sits above it.
    let row = held(only_row(&value.model, RowKind::Del, "sierra"));
    assert_eq!(row.old_no, 19, "the pane's gutter is a HEAD number");
    assert_eq!(
        stats(&mut state, &repo).row_is_staged(Path::new("f.txt"), &row),
        Some(false)
    );

    granular::toggle_row(&mut state, &Some(PathBuf::from("f.txt")), &row);
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

/// The same question with the shift the other way: a staged deletion above the
/// row makes its HEAD gutter larger than its index gutter.
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

    granular::toggle_row(&mut state, &Some(PathBuf::from("f.txt")), &row);
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

/// Two identical adjacent lines, clicked one at a time: each copy stages alone,
/// because a forward apply accepts every single ordinal and pairing is positional
/// rather than by content. Unstaging is per-row for the same reason: `filter_lines`'
/// reverse branch keeps the other staged copies as post-image context, and a
/// same-kind run is not a replacement pair, so git accepts reversing one `+dup`
/// and leaves the other staged. Widening this to the located hunk would cost
/// exactly that per-row control.
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

    let path = Some(PathBuf::from("f.txt"));
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
        granular::toggle_row(&mut state, &path, &copies[which]);
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
    // Unstage one copy: it alone reverses out of the index, the other stays staged.
    let row = held(first_row(&value.model, RowKind::Add, "dup"));
    granular::toggle_row(&mut state, &path, &row);
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
    granular::toggle_row(&mut state, &path, &row);
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

/// A repository nobody has committed in offers no rows to click, and that is the
/// decision rather than an omission. The preview is the `HEAD` comparison, the
/// engine refuses that read where no first commit exists, so the pane's answer is
/// the refusal sentence. A click needs a located ordinal out of a displayed row,
/// so with no rows there is no operation and the index does not move. The suite
/// keeps this fixture because the alternative, answering with a synthesized empty
/// baseline, would give this test two rows to click and a stage the user never
/// asked for.
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

    // Nothing reached the index, because nothing was clickable.
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

/// A staged creation is the one shape that still reverse-applies whole, and this
/// is where that click is proved. The fixture that carried it used to be a
/// repository with no commits, which no longer has a read to click, and the shape
/// is the same either way: a file git has never had a version of has an empty old
/// side, so `git apply --cached --reverse` refuses half of it with
/// `error: new file <path> depends on old contents` and one row click takes the
/// whole file out of the index.
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

    granular::toggle_row(&mut state, &Some(PathBuf::from("new.txt")), &row);
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

/// A line staged and then edited again: the row shows the worktree's text, which
/// the index does not hold, so ticket 03's rule answers it unstaged and the click
/// stages it. It does not vanish and it does not answer a third state.
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

    granular::toggle_row(&mut state, &Some(PathBuf::from("f.txt")), &row);
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

/// The `MM` meaning, pinned as a decision with the reason. Staging one half of a
/// replacement pair stages that half alone: the index ends up holding HEAD's line
/// and the replacement together, and git reports `MM`. This is the meaning that
/// ships because the pane promises one row, one line, and `MM` is git's honest
/// report of an index that differs from HEAD while the worktree differs from the
/// index. Quietly taking the pair's other half along would make a row click do
/// something the reader did not ask for.
#[test]
fn staging_one_half_of_a_replacement_pair_leaves_the_file_mm() {
    let project = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(project.path(), "mm-decision");
    commit(&repo, "f.txt", BASE, "f base");
    std::fs::write(repo.join("f.txt"), BASE.replace("echo\n", "ECHO\n")).unwrap();

    let mut state = app_state(project.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let row = held(only_row(&value.model, RowKind::Add, "ECHO"));
    granular::toggle_row(&mut state, &Some(PathBuf::from("f.txt")), &row);
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

/// Conflicted files keep their controls visible and inert, because the oracle has
/// no answer for any row of an unmerged path. That is derived from git's own
/// `diff --cc` flag rather than special-cased at the click.
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
        granular::toggle_row(&mut state, &Some(PathBuf::from("conf.txt")), row);
        state.drain_events();
    }

    assert_eq!(staged(&repo, "conf.txt"), before, "no index movement");
    assert!(state.ui.toast.is_none(), "and no error toast");
    assert!(state.caches.hunk_stats(&root_id(&repo)).is_some());
}

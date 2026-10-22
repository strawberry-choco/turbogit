//! The per-line staged oracle, graded against real git.
//!
//! Every fixture is a temporary repository driven by the CLI executor, per
//! `AGENTS.md`: [`AppState::for_roots`] over a `tempfile` directory, rows taken
//! from the display model the keyed read actually answers with, and answers
//! checked against `git status --porcelain`, `git diff` and
//! `git diff --cached` read back after each fixture. The recording fake is not
//! extended: nothing here needs a git state a real repository cannot produce.
//!
//! The oracle's contract is a row of a `HEAD`-based comparison, which is the
//! patch the preview pins. [`combined_value`] reads exactly that
//! through the production read, so the rows these tests ask about are the rows
//! the pane paints, gutter numbers and all.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use test_support::git_seed::{commit, git, pin_eol_lf, repo_with_conflict, repo_with_one_commit};
use turbogit_app::diff_model::{DiffModel, DiffValue, DisplayRow, Row, RowKind};
use turbogit_app::granular::{self, HunkTarget};
use turbogit_app::keyed_read::{DiffTarget, Read};
use turbogit_app::root_caches::{Affected, RootCaches, RootHunkStats, StatsView};
use turbogit_app::staged_lines;
use turbogit_app::staged_lines::{
    Displayed, Mapping, NoSource, Ordinal, RowSource, Side, Source, Space, Toggle, row_source,
};
use turbogit_app::state::AppState;
use turbogit_domain::model::{
    ChangeRef, ChangeStatus, DiffOpts, DiffPolicies, PatchLineKind, RootId, VcsSettings,
};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_services::{hunk_stats, partial};

// ---------------------------------------------------------------- helpers --

fn executor() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

fn root_id(repo: &Path) -> RootId {
    RootId(repo.to_path_buf().into())
}

fn app_state(project: &Path, repo: &Path) -> AppState {
    AppState::for_roots(project, std::slice::from_ref(&repo.to_path_buf()))
}

/// Fill `repo`'s hunk statistics the way the app fills them, on ask.
fn stats<'a>(state: &'a mut AppState, repo: &Path) -> &'a RootHunkStats {
    let root = root_id(repo);
    state.ensure_hunk_stats(&root);
    state
        .caches
        .hunk_stats(&root)
        .expect("the root's entry fills on ask")
}

/// What the caches hold for `repo` right now, without filling anything.
fn held_stats<'a>(caches: &'a RootCaches, repo: &Path) -> Option<&'a RootHunkStats> {
    caches.hunk_stats(&root_id(repo))
}

/// Every row of one display model, in paint order, pairs opened up.
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

/// The changed rows of one display model, in paint order.
fn changed_rows(model: &DiffModel) -> Vec<&Row> {
    rows(model)
        .into_iter()
        .filter(|r| matches!(r.kind, RowKind::Del | RowKind::Add))
        .collect()
}

/// The `(kind, text, old_no, new_no)` of every changed row, so a failed
/// assertion prints the fixture's own geometry.
fn geometry(model: &DiffModel) -> Vec<(RowKind, String, usize, usize)> {
    changed_rows(model)
        .into_iter()
        .map(|r| (r.kind, r.text.clone(), r.old_no, r.new_no))
        .collect()
}

/// The display model of one file's `HEAD ↔ worktree` diff, read through the
/// production keyed read: the pane's own rows with the pane's own numbers.
fn combined_value(state: &mut AppState, repo: &Path, rel: &str) -> Arc<DiffValue> {
    read_value(state, repo, rel, Some("HEAD"))
}

/// The display model of an unpinned working-tree read, `index ↔ worktree`: the
/// comparison this app no longer shows and [`Displayed::IndexVsWorktree`] still
/// names, so the oracle's declined-answer cases have a real display model.
fn working_tree_value(state: &mut AppState, repo: &Path, rel: &str) -> Arc<DiffValue> {
    read_value(state, repo, rel, None)
}

fn read_value(state: &mut AppState, repo: &Path, rel: &str, left: Option<&str>) -> Arc<DiffValue> {
    let target = DiffTarget::new(
        repo.to_path_buf(),
        left.map(str::to_owned),
        None,
        DiffPolicies::default(),
        Some(PathBuf::from(rel)),
    );
    // Ask, settle on the pump, ask again: the read's own two-frame life.
    for _ in 0..4 {
        if let Read::Fresh(value) = state.read(target.clone()) {
            return value;
        }
        state.drain_events();
    }
    panic!("the diff read never settles for {rel}");
}

/// The three states a reviewer grades an answer against: git's own index
/// verdict for the path, and the two patch bodies the oracle derives from.
#[derive(Debug)]
struct GitStates {
    /// `git status --porcelain`'s whole line for the path.
    porcelain: String,
    /// `git diff`: the unstaged body.
    unstaged: String,
    /// `git diff --cached`: the staged body.
    staged: String,
}

fn git_states(repo: &Path, rel: &str) -> GitStates {
    let porcelain = git(repo, &["status", "--porcelain"])
        .lines()
        .find(|l| {
            let path = l[3..].trim();
            path == rel || path.ends_with(&format!("-> {rel}"))
        })
        .unwrap_or_else(|| {
            panic!(
                "{rel} is not in git's status:\n{}",
                git(repo, &["status", "--porcelain"])
            )
        })
        .trim_end()
        .to_owned();
    GitStates {
        porcelain,
        unstaged: git(repo, &["diff", "--", rel]),
        staged: git(repo, &["diff", "--cached", "--", rel]),
    }
}

/// How many times `<prefix><text>` appears as its own line of one patch body,
/// with git's `+++`/`---` file headers excluded.
fn body_line(body: &str, prefix: char, text: &str) -> usize {
    let want = format!("{prefix}{text}");
    body.lines()
        .filter(|l| !l.starts_with("+++") && !l.starts_with("---"))
        .filter(|l| *l == want)
        .count()
}

/// The one changed row whose text is `text`, asserting it is unique. The point
/// of the oracle is that a row is its position, so a fixture that could match
/// twice must say so loudly rather than answer for the wrong line.
fn only_row<'a>(model: &'a DiffModel, kind: RowKind, text: &str) -> &'a Row {
    let found: Vec<&Row> = changed_rows(model)
        .into_iter()
        .filter(|r| r.kind == kind && r.text == text)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "exactly one {kind:?} row of {text:?}, got {} in {:?}",
        found.len(),
        geometry(model)
    );
    found[0]
}

/// The one changed row of `kind` whose own gutter is `old_no`.
///
/// A translation question is asked of a number rather than a text, so the
/// fixtures that hold two deletions of different text in one hunk can name the
/// row a click would be about without spelling its content twice.
fn only_row_at(model: &DiffModel, kind: RowKind, old_no: usize) -> &Row {
    let found: Vec<&Row> = changed_rows(model)
        .into_iter()
        .filter(|r| r.kind == kind && r.old_no == old_no)
        .collect();
    assert_eq!(
        found.len(),
        1,
        "exactly one {kind:?} row at old_no {old_no}, got {} in {:?}",
        found.len(),
        geometry(model)
    );
    found[0]
}

/// A row that does not come from a display model, for asking the oracle about a
/// path the pane shows no rows for.
fn row_at(kind: RowKind, old_no: usize, new_no: usize) -> Row {
    Row {
        kind,
        text: String::new(),
        hunk: 0,
        line_ord: 0,
        old_no,
        new_no,
        span: None,
        highlight: Default::default(),
    }
}

/// A file whose line 2 is modified, a line added after line 10, and line 20
/// deleted: far enough apart that git reports three hunks in each patch.
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

/// [`BASE`] with `bravo` modified, `uniform` added at line 11, `tango` deleted.
const ALL_THREE: &str = concat!(
    "alpha\n",
    "BRAVO\n",
    "charlie\n",
    "delta\n",
    "echo\n",
    "foxtrot\n",
    "golf\n",
    "hotel\n",
    "india\n",
    "juliet\n",
    "uniform\n",
    "kilo\n",
    "lima\n",
    "mike\n",
    "november\n",
    "oscar\n",
    "papa\n",
    "quebec\n",
    "romeo\n",
    "sierra\n",
);

/// Seed `repo` with [`BASE`] as `f.txt`, committed on top of the floor commit.
fn seed_base(repo: &Path) {
    commit(repo, "f.txt", BASE, "f base");
}

// --- the stale-answer guard, written first ----------------------------------

/// The guard that keeps the answer out of the diff value.
///
/// A line goes into the index through the app's own staging path
/// ([`granular::dispatch`], settled by the production `OpCompleted` → refresh →
/// drain route). Afterwards `git diff HEAD` is byte-for-byte what it was, so the
/// displayed diff and every one of its rows is the very value the click saw, and
/// the oracle still flips, because its life is one `RootHunkStats` entry and the
/// refresh dropped that entry as a unit. An answer folded into the display model
/// cannot do this: it would be re-derived from identical bytes.
#[test]
fn a_staged_line_flips_the_answer_while_the_displayed_bytes_do_not_move() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "stale-guard");
    seed_base(&repo);
    // One added line at the end: hunk 0's only changed line, so its ordinal is
    // 0 and a forward stage of it is the whole of the file's change.
    std::fs::write(repo.join("f.txt"), format!("{BASE}uniform\n")).unwrap();

    let mut state = app_state(parent.path(), &repo);
    // The rows come from an unpinned `index ↔ worktree` read, and for an addition
    // row that is the same answer a HEAD-based row gets, because both post-images
    // are the worktree. The handle is held across the operation on purpose: the
    // same rows, unmoved, are what the answer flips on.
    let value = working_tree_value(&mut state, &repo, "f.txt");
    let row = only_row(&value.model, RowKind::Add, "uniform");
    let displayed_before = git(&repo, &["diff", "HEAD", "--", "f.txt"]);

    assert_eq!(
        stats(&mut state, &repo).row_is_staged(Path::new("f.txt"), row),
        Some(false),
        "an unstaged addition reads unstaged, geometry={:?}",
        geometry(&value.model)
    );

    // One row click's real route, through the dispatch seam and settled by the
    // production `OpCompleted` → refresh → drain path. The row is addressed by
    // its worktree gutter; its direction is the oracle's own answer, which is
    // `Stage` here because the line is not in the index yet.
    granular::dispatch(
        &mut state,
        ChangeRef::new(root_id(&repo), "f.txt"),
        HunkTarget::Row(RowKind::Add, 0, 21),
    );
    state.drain_events();

    assert!(
        held_stats(&state.caches, &repo).is_none(),
        "the refresh dropped the root's whole entry, line sets included"
    );

    // Same row object, same bytes, one re-fill: the answer follows the index.
    assert_eq!(
        stats(&mut state, &repo).row_is_staged(Path::new("f.txt"), row),
        Some(true),
        "the staged line reads staged"
    );
    assert_eq!(
        git(&repo, &["diff", "HEAD", "--", "f.txt"]),
        displayed_before,
        "staging a line must not change one byte of the displayed diff"
    );
    assert_eq!(row.new_no, 21, "and the row itself did not move");

    let graded = git_states(&repo, "f.txt");
    assert_eq!(graded.porcelain, "M  f.txt", "staged, nothing unstaged");
    assert_eq!(body_line(&graded.staged, '+', "uniform"), 1);
    assert_eq!(body_line(&graded.unstaged, '+', "uniform"), 0);
}

// --- all three staged, and none of them -------------------------------------

#[test]
fn all_three_staged_makes_every_row_of_the_combined_diff_answer_staged() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "all-staged");
    seed_base(&repo);
    std::fs::write(repo.join("f.txt"), ALL_THREE).unwrap();
    git(&repo, &["add", "--", "f.txt"]);

    let mut state = app_state(parent.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let stats = stats(&mut state, &repo);

    assert_eq!(
        changed_rows(&value.model).len(),
        4,
        "one modified line is two rows, plus one addition and one deletion, geometry={:?}",
        geometry(&value.model)
    );
    for (kind, text) in [
        (RowKind::Del, "bravo"),
        (RowKind::Del, "tango"),
        (RowKind::Add, "BRAVO"),
        (RowKind::Add, "uniform"),
    ] {
        let row = only_row(&value.model, kind, text);
        assert_eq!(
            stats.row_is_staged(Path::new("f.txt"), row),
            Some(true),
            "a {kind:?} row of a fully staged file is staged: {text}"
        );
    }

    let graded = git_states(&repo, "f.txt");
    assert_eq!(
        graded.porcelain, "M  f.txt",
        "git agrees: staged, nothing unstaged"
    );
    assert_eq!(graded.unstaged, "", "and `git diff` has no body at all");
    for text in ["bravo", "tango"] {
        assert_eq!(
            body_line(&graded.staged, '-', text),
            1,
            "{text} removed in the index"
        );
    }
    for text in ["BRAVO", "uniform"] {
        assert_eq!(
            body_line(&graded.staged, '+', text),
            1,
            "{text} added in the index"
        );
    }
}

#[test]
fn nothing_staged_makes_every_row_answer_unstaged() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "none-staged");
    seed_base(&repo);
    std::fs::write(repo.join("f.txt"), ALL_THREE).unwrap();

    let mut state = app_state(parent.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let stats = stats(&mut state, &repo);

    // The pairing the display model is claimed to keep: a modified line is two
    // rows, and each numbers only its own side, so the two can be answered
    // apart. On this content both happen to be unstaged.
    let removed = only_row(&value.model, RowKind::Del, "bravo");
    let added = only_row(&value.model, RowKind::Add, "BRAVO");
    assert_eq!(
        (removed.old_no, removed.new_no),
        (2, 0),
        "a Del row carries its HEAD number and no worktree number"
    );
    assert_eq!(
        (added.old_no, added.new_no),
        (0, 2),
        "an Add row carries its worktree number and no HEAD number"
    );

    for row in changed_rows(&value.model) {
        assert_eq!(
            stats.row_is_staged(Path::new("f.txt"), row),
            Some(false),
            "nothing is in the index, so nothing is staged: {:?}",
            (row.kind, &row.text)
        );
    }

    let graded = git_states(&repo, "f.txt");
    assert_eq!(graded.porcelain, " M f.txt", "git agrees: unstaged only");
    assert_eq!(
        graded.staged, "",
        "and `git diff --cached` has no body at all"
    );
    assert_eq!(body_line(&graded.unstaged, '+', "BRAVO"), 1);
    assert_eq!(body_line(&graded.unstaged, '+', "uniform"), 1);
    assert_eq!(body_line(&graded.unstaged, '-', "tango"), 1);
}

// --- the mixed case, which is the whole point -------------------------------

#[test]
fn reverse_applying_one_line_out_of_the_index_flips_that_row_alone() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "mixed");
    seed_base(&repo);
    std::fs::write(repo.join("f.txt"), ALL_THREE).unwrap();
    git(&repo, &["add", "--", "f.txt"]);

    let mut state = app_state(parent.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let displayed_before = git(&repo, &["diff", "HEAD", "--", "f.txt"]);
    for row in changed_rows(&value.model) {
        assert_eq!(
            stats(&mut state, &repo).row_is_staged(Path::new("f.txt"), row),
            Some(true),
            "the fixture starts fully staged: {:?}",
            (row.kind, &row.text)
        );
    }

    // One row's reverse-apply, taken from the patch that can carry it: a staged
    // line's appliable source is `HEAD ↔ index`, never the combined display
    // (`The constraint, measured`). Choosing the source per line is the next
    // ticket's job, so the fixture names it here.
    let exec = executor();
    let staged_patch = exec
        .diff_patch(
            &repo,
            &DiffOpts {
                staged: true,
                path: Some(PathBuf::from("f.txt")),
                ..DiffOpts::default()
            },
        )
        .unwrap();
    let hunk = staged_patch.files[0]
        .hunks
        .iter()
        .position(|h| {
            h.lines
                .iter()
                .any(|l| l.text == "uniform" && l.kind == PatchLineKind::Added)
        })
        .expect("`uniform` is a hunk of its own in the staged patch");
    let selection = partial::Selection {
        hunks: [(
            hunk,
            partial::HunkSelection::Lines([0].into_iter().collect()),
        )]
        .into_iter()
        .collect(),
    };
    partial::unstage_selection(
        &exec,
        &repo,
        &staged_patch,
        &selection,
        ChangeStatus::Modified,
    )
    .expect("reverse-applying one staged line");
    // The op seam's own completion step, which is what retires the answer: the
    // statistics are compute-on-miss, so nothing else drops the entry, and a
    // test that moves git behind the app's back has to say so the way the app
    // does.
    state.refresh(Affected::Root(root_id(&repo)));

    let stats = stats(&mut state, &repo);
    assert_eq!(
        stats.row_is_staged(
            Path::new("f.txt"),
            only_row(&value.model, RowKind::Add, "uniform")
        ),
        Some(false),
        "the reverse-applied row is the one that flipped"
    );
    for (kind, text) in [
        (RowKind::Del, "bravo"),
        (RowKind::Del, "tango"),
        (RowKind::Add, "BRAVO"),
    ] {
        assert_eq!(
            stats.row_is_staged(Path::new("f.txt"), only_row(&value.model, kind, text)),
            Some(true),
            "every other row keeps its answer: {kind:?} {text}"
        );
    }
    assert_eq!(
        git(&repo, &["diff", "HEAD", "--", "f.txt"]),
        displayed_before,
        "and the displayed combined diff did not move by one byte"
    );

    let graded = git_states(&repo, "f.txt");
    assert_eq!(
        graded.porcelain, "MM f.txt",
        "git agrees: both halves present"
    );
    assert_eq!(
        body_line(&graded.staged, '+', "uniform"),
        0,
        "out of the index"
    );
    assert_eq!(
        body_line(&graded.staged, '+', "BRAVO"),
        1,
        "still in the index"
    );
    assert_eq!(
        body_line(&graded.unstaged, '+', "uniform"),
        1,
        "and now unstaged"
    );
}

// --- pairing is positional --------------------------------------------------

#[test]
fn two_identical_adjacent_additions_answer_apart_by_position() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "duplicate-adds");
    commit(&repo, "dup.txt", "alpha\nbravo\n", "base");
    // The index holds one `dup`...
    std::fs::write(repo.join("dup.txt"), "alpha\nbravo\ndup\n").unwrap();
    git(&repo, &["add", "--", "dup.txt"]);
    // ...and the worktree holds a second copy, identical and adjacent.
    std::fs::write(repo.join("dup.txt"), "alpha\nbravo\ndup\ndup\n").unwrap();

    let mut state = app_state(parent.path(), &repo);
    let value = combined_value(&mut state, &repo, "dup.txt");
    let stats = stats(&mut state, &repo);

    let adds: Vec<&Row> = rows(&value.model)
        .into_iter()
        .filter(|r| r.kind == RowKind::Add)
        .collect();
    assert_eq!(
        adds.iter()
            .map(|r| (r.text.as_str(), r.new_no))
            .collect::<Vec<_>>(),
        [("dup", 3), ("dup", 4)],
        "two rows of one identical text, adjacent in worktree space"
    );
    assert_eq!(
        stats.row_is_staged(Path::new("dup.txt"), adds[0]),
        Some(true),
        "the copy the index holds"
    );
    assert_eq!(
        stats.row_is_staged(Path::new("dup.txt"), adds[1]),
        Some(false),
        "the further copy it does not"
    );

    let graded = git_states(&repo, "dup.txt");
    assert_eq!(graded.porcelain, "MM dup.txt");
    assert_eq!(body_line(&graded.staged, '+', "dup"), 1, "one copy staged");
    assert_eq!(
        body_line(&graded.unstaged, '+', "dup"),
        1,
        "one copy unstaged"
    );
}

// --- staged, then edited again ----------------------------------------------

#[test]
fn a_line_staged_and_then_the_file_edited_again_still_reports_staged() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "re-edit");
    commit(&repo, "f.txt", "alpha\n", "base");
    std::fs::write(repo.join("f.txt"), "alpha\nstaged\n").unwrap();
    git(&repo, &["add", "--", "f.txt"]);
    // A further, unstaged edit, elsewhere in the file.
    std::fs::write(repo.join("f.txt"), "alpha\nstaged\nmore\n").unwrap();

    let mut state = app_state(parent.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let stats = stats(&mut state, &repo);

    assert_eq!(
        stats.row_is_staged(
            Path::new("f.txt"),
            only_row(&value.model, RowKind::Add, "staged")
        ),
        Some(true),
        "its displayed content is what the index holds, so the answer is staged"
    );
    assert_eq!(
        stats.row_is_staged(
            Path::new("f.txt"),
            only_row(&value.model, RowKind::Add, "more")
        ),
        Some(false),
        "the worktree edit stays unstaged, and is all the view shows of it"
    );

    let graded = git_states(&repo, "f.txt");
    assert_eq!(graded.porcelain, "MM f.txt");
    assert_eq!(body_line(&graded.staged, '+', "staged"), 1);
    assert_eq!(body_line(&graded.staged, '+', "more"), 0);
    assert_eq!(body_line(&graded.unstaged, '+', "more"), 1);
    assert_eq!(body_line(&graded.unstaged, '+', "staged"), 0);
}

#[test]
fn a_staged_addition_whose_text_the_worktree_rewrote_reports_unstaged() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "re-edit-rewritten");
    commit(&repo, "f.txt", "alpha\n", "base");
    std::fs::write(repo.join("f.txt"), "alpha\nstaged\n").unwrap();
    git(&repo, &["add", "--", "f.txt"]);
    std::fs::write(repo.join("f.txt"), "alpha\nREWRITTEN\n").unwrap();

    let mut state = app_state(parent.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let stats = stats(&mut state, &repo);

    // The other half of the two-state rule, and the reason there is no third
    // state to invent: HEAD never held `staged`, so the combined view shows one
    // `+REWRITTEN` row, and the index does not hold that text.
    assert_eq!(
        changed_rows(&value.model)
            .iter()
            .map(|r| (r.kind, r.text.clone()))
            .collect::<Vec<_>>(),
        [(RowKind::Add, "REWRITTEN".to_owned())]
    );
    assert_eq!(
        stats.row_is_staged(
            Path::new("f.txt"),
            only_row(&value.model, RowKind::Add, "REWRITTEN")
        ),
        Some(false),
        "the row shows the rewrite, which the index does not hold"
    );
    let one = staged_lines::find(&stats.line_sets, Path::new("f.txt")).expect("listed");
    assert_eq!(
        one.local.removed.runs(),
        &[(2, 2)],
        "and the index's own line is the unstaged removal it will reverse-apply"
    );

    let graded = git_states(&repo, "f.txt");
    assert_eq!(graded.porcelain, "MM f.txt");
    assert_eq!(body_line(&graded.staged, '+', "staged"), 1);
    assert_eq!(body_line(&graded.unstaged, '+', "REWRITTEN"), 1);
    assert_eq!(body_line(&graded.unstaged, '-', "staged"), 1);
}

// --- states that are not per-line questions ---------------------------------

#[test]
fn an_unmerged_path_answers_nothing_although_its_markers_are_ordinary_rows() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_conflict(parent.path(), "conflicted-oracle");

    let mut state = app_state(parent.path(), &repo);
    let value = combined_value(&mut state, &repo, "conf.txt");
    let stats = stats(&mut state, &repo);

    // The displayed patch is git's ordinary two-sided form: conflict markers as
    // `+` rows, and no sign in the value the rows come from that they are not a
    // real change. This is why the answer, not the patch, has to gate the
    // controls.
    assert!(
        !changed_rows(&value.model).is_empty(),
        "the pane really does show rows, geometry={:?}",
        geometry(&value.model)
    );
    for row in changed_rows(&value.model) {
        assert_eq!(
            stats.row_is_staged(Path::new("conf.txt"), row),
            None,
            "no line of an unmerged path is classifiable: {:?}",
            (row.kind, &row.text)
        );
    }
    let one = staged_lines::find(&stats.line_sets, Path::new("conf.txt"))
        .expect("`git diff` lists the unmerged path as `diff --cc`");
    assert!(one.conflicted, "git's own flag, never path text");

    let graded = git_states(&repo, "conf.txt");
    assert_eq!(
        graded.porcelain, "UU conf.txt",
        "git's verdict for the same path"
    );
    assert!(
        graded.staged.contains("Unmerged") && !graded.staged.contains("diff --git"),
        "`git diff --cached` says `* Unmerged path` and carries no section: {}",
        graded.staged
    );
}

#[test]
fn a_binary_change_answers_no_line_in_either_space() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "no-lines");
    commit(&repo, "data.bin", "one\n", "bin");
    std::fs::write(repo.join("data.bin"), [0u8, 1, 2, 0, 3]).unwrap();
    git(&repo, &["add", "--", "data.bin"]);

    let mut state = app_state(parent.path(), &repo);
    let binary = combined_value(&mut state, &repo, "data.bin");
    assert!(
        changed_rows(&binary.model).is_empty(),
        "git's binary section shows the pane no rows to ask about, geometry={:?}",
        geometry(&binary.model)
    );

    assert_numbers_no_line(&mut state, &repo, "data.bin");

    let graded = git_states(&repo, "data.bin");
    assert_eq!(graded.porcelain, "M  data.bin");
    assert!(
        graded.staged.contains("Binary files"),
        "git's own marker, got: {}",
        graded.staged
    );
}

/// A file mode is a POSIX concept, so this fixture sets the bit where git can
/// record it. What the oracle and the pane actually consume, a mode-only
/// section with no hunk, is pinned on every platform by the synthetic
/// `old mode`/`new mode` patch in `diff_display_shapes.rs`.
#[cfg(unix)]
#[test]
fn a_mode_only_change_answers_no_line_in_either_space() {
    use std::os::unix::fs::PermissionsExt;

    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "mode-only");
    commit(&repo, "script.sh", "echo hi\n", "script");
    std::fs::set_permissions(
        repo.join("script.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    git(&repo, &["add", "--", "script.sh"]);

    let mut state = app_state(parent.path(), &repo);
    assert_numbers_no_line(&mut state, &repo, "script.sh");

    let graded = git_states(&repo, "script.sh");
    assert_eq!(graded.porcelain, "M  script.sh");
    assert!(
        graded.staged.contains("new mode 100755"),
        "git's own mode headers, got: {}",
        graded.staged
    );
    assert!(
        !graded.staged.contains("@@"),
        "a mode-only section carries no hunk, so no line of it has a number in \
         any space: {}",
        graded.staged
    );
}

/// Every per-line answer for `rel` says no line in either space. The section is
/// listed, its sets are empty, and an empty set is never a claim that some line
/// is staged.
fn assert_numbers_no_line(state: &mut AppState, repo: &Path, rel: &str) {
    let stats = stats(state, repo);
    let entry = staged_lines::find(&stats.line_sets, Path::new(rel))
        .unwrap_or_else(|| panic!("{rel} is listed in the staged patch"));
    assert!(
        entry.staged.added.is_empty()
            && entry.staged.removed.is_empty()
            && entry.local.added.is_empty()
            && entry.local.removed.is_empty(),
        "{rel} numbers no line in either space"
    );
    assert_eq!(
        stats.row_is_staged(Path::new(rel), &row_at(RowKind::Add, 0, 1)),
        None,
        "an empty added set is not a claim that line 1 is staged: {rel}"
    );
    assert_eq!(
        stats.row_is_staged(Path::new(rel), &row_at(RowKind::Del, 1, 0)),
        None,
        "{rel}"
    );
}

#[test]
fn a_rename_with_content_is_keyed_by_its_new_name_and_answers_per_line() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "renamed");
    commit(&repo, "old.rs", "alpha\nbravo\n", "base");
    git(&repo, &["config", "diff.renames", "true"]);
    std::fs::rename(repo.join("old.rs"), repo.join("new.rs")).unwrap();
    std::fs::write(repo.join("new.rs"), "alpha\nBRAVO\n").unwrap();
    git(&repo, &["add", "--all"]);

    let mut state = app_state(parent.path(), &repo);
    let value = combined_value(&mut state, &repo, "new.rs");
    let stats = stats(&mut state, &repo);

    // git's path-scoped view of a rename cannot see the other half of it, so the
    // pane shows the new file's lines as plain additions. The whole-root patches
    // the oracle reads do see it, and key the section by its `b/` side.
    assert_eq!(
        changed_rows(&value.model)
            .iter()
            .map(|r| (r.kind, r.text.as_str(), r.new_no))
            .collect::<Vec<_>>(),
        [(RowKind::Add, "alpha", 1), (RowKind::Add, "BRAVO", 2)],
        "the renamed path's rows, geometry={:?}",
        geometry(&value.model)
    );
    for text in ["alpha", "BRAVO"] {
        assert_eq!(
            stats.row_is_staged(
                Path::new("new.rs"),
                only_row(&value.model, RowKind::Add, text)
            ),
            Some(true),
            "a renamed and staged line is an ordinary staged line: {text}"
        );
    }
    assert_eq!(
        staged_lines::find(&stats.line_sets, Path::new("old.rs")),
        None,
        "the old name owns no lines any more"
    );

    let graded = git_states(&repo, "new.rs");
    assert!(
        graded.porcelain.starts_with("R "),
        "git's rename verdict: {}",
        graded.porcelain
    );
    assert_eq!(
        graded.unstaged, "",
        "rename and content edit are both staged"
    );
    assert!(
        git(&repo, &["diff", "--cached"]).contains("rename to new.rs"),
        "the whole-root staged patch is the one that calls it a rename"
    );
}

// --- the one invalidation, on the production path ---------------------------

#[test]
fn the_answer_shares_the_hunk_spans_one_unit_lifetime_across_every_root() {
    let parent = tempfile::tempdir().unwrap();
    let first = repo_with_one_commit(parent.path(), "first-root");
    let second = repo_with_one_commit(parent.path(), "second-root");
    seed_base(&first);
    seed_base(&second);
    std::fs::write(first.join("f.txt"), BASE.replace("bravo", "BRAVO")).unwrap();
    std::fs::write(second.join("f.txt"), BASE.replace("charlie", "CHARLIE")).unwrap();

    let mut caches = RootCaches::default();
    let exec = executor();
    let first_root = root_id(&first);
    let second_root = root_id(&second);
    caches.ensure_hunk_stats(&exec, &first_root);
    caches.ensure_hunk_stats(&exec, &second_root);
    for (repo, text, worktree_line) in [(&first, "BRAVO", 2), (&second, "CHARLIE", 3)] {
        let held = held_stats(&caches, repo).expect("filled");
        assert_eq!(
            held.line_sets
                .iter()
                .map(|f| f.local.added.len() + f.staged.added.len())
                .sum::<usize>(),
            1,
            "{text}: the derivation runs at the same one site as the reduction"
        );
        assert_eq!(
            held.row_is_staged(Path::new("f.txt"), &row_at(RowKind::Add, 0, worktree_line)),
            Some(false),
            "{text} is not in the index on either root"
        );
    }

    // One root's refresh drops that root's entry and nothing else's. The answer
    // cannot outlive the entry it is a field of, because it is a field of it.
    caches.invalidate(&Affected::Root(first_root.clone()));
    assert!(caches.hunk_stats(&first_root).is_none());
    assert!(caches.hunk_stats(&second_root).is_some());
    // And the whole-cache path empties every map through the same interface.
    caches.invalidate(&Affected::All);
    assert!(
        caches.is_empty(),
        "no new map, so no new clear arm and no new clause"
    );

    // Re-filling reads git again, so the answer is whatever the index is now.
    git(&first, &["add", "--", "f.txt"]);
    caches.ensure_hunk_stats(&exec, &first_root);
    let filled = caches.hunk_stats(&first_root).expect("refilled");
    assert_eq!(
        staged_lines::row_is_staged(
            &filled.line_sets,
            Path::new("f.txt"),
            &row_at(RowKind::Add, 0, 2)
        ),
        Some(true),
        "a line staged between two fills answers staged on the next one"
    );
    assert!(
        hunk_stats::file_hunks(&exec.diff_patch(&first, &DiffOpts::default()).unwrap())
            .iter()
            .all(|f| f.path != "f.txt"),
        "and the unstaged patch it was derived from really does not list the file"
    );
}

// --- the unborn HEAD, where the read has no baseline -------------------------

/// A repository nobody has committed in yet: initialised, configured, no HEAD.
/// A `HEAD` read there is refused by the engine (`cli.rs` `guard_unborn_head`)
/// with `TgError::NoCommitsYet`, so the oracle is never asked about a
/// substituted patch and the pane never builds rows.
fn unborn_repo(project: &Path, name: &str) -> PathBuf {
    let work = project.join(name);
    git(
        project,
        &["init", "-q", "-b", "main", work.to_str().unwrap()],
    );
    git(&work, &["config", "user.email", "test@example.com"]);
    git(&work, &["config", "user.name", "Test"]);
    pin_eol_lf(&work);
    work
}

/// The sentence the pane prints for a refused `HEAD` read. It reaches the
/// surface as text, because `Read::Failed` holds a rendered message, and the
/// variant behind it is what keeps the producer's refusal matchable at the
/// engine boundary rather than something only a string can recognise.
const NO_COMMITS_MESSAGE: &str =
    "this repository has no commits yet: there is no first commit to compare your changes against";

/// Ask the production keyed read until it gives a terminal verdict, and hand
/// back the failure text. A `Fresh` answer here would mean the read was
/// answered somehow, which is the thing this program decided not to do.
fn read_failure(state: &mut AppState, target: DiffTarget) -> String {
    for _ in 0..4 {
        match state.read(target.clone()) {
            Read::Fresh(_) | Read::Empty => {
                panic!(
                    "an unborn HEAD must not answer for {:?}",
                    target.path.as_deref().unwrap_or(Path::new("<root>"))
                );
            }
            Read::Failed(message) => return message,
            Read::Waiting => {}
        }
        state.drain_events();
    }
    panic!(
        "the diff read never reaches a verdict for {:?}",
        target.path.as_deref().unwrap_or(Path::new("<root>"))
    );
}

/// The pane's answer on a fresh repository that has staged content: a refusal
/// naming the missing baseline, and no rows. Git's own evidence is in the same
/// test, because the content the refused read would have reported really does
/// exist and really does have no HEAD side.
#[test]
fn an_unborn_head_refuses_the_combined_read_and_shows_the_sentence() {
    let parent = tempfile::tempdir().unwrap();
    let repo = unborn_repo(parent.path(), "unborn-oracle");
    std::fs::write(repo.join("f.txt"), "alpha\nbravo\n").unwrap();
    git(&repo, &["add", "--", "f.txt"]);

    let graded = git_states(&repo, "f.txt");
    assert_eq!(
        graded.porcelain, "A  f.txt",
        "git's own verdict is a creation, not a rename"
    );
    assert!(
        graded.staged.contains("@@ -0,0 +1,2 @@"),
        "the staged patch has no pre-image at all, so a synthesized HEAD \
         would have shown both lines as additions: {}",
        graded.staged
    );

    let mut state = app_state(parent.path(), &repo);
    let target = DiffTarget::working_tree(
        repo.clone(),
        DiffPolicies::default(),
        Some(PathBuf::from("f.txt")),
    );
    assert_eq!(read_failure(&mut state, target), NO_COMMITS_MESSAGE);

    // The whole-root ask is the same refusal, and it is the read
    // `ensure_hunk_stats` makes.
    let root_target = DiffTarget::working_tree(repo.clone(), DiffPolicies::default(), None);
    assert_eq!(read_failure(&mut state, root_target), NO_COMMITS_MESSAGE);

    // What the refusal does to the root's statistics, pinned rather than
    // assumed. The two index-facing patches still fill, so the oracle's own
    // data is intact; only the HEAD side is missing, and it is missing because
    // the failed read is recorded against that one view (`root_caches.rs`
    // `diff_failed`) and contributes an empty list to the entry. The failure's
    // own suite is `tests/root_read_failures.rs`.
    let stats = stats(&mut state, &repo);
    assert!(
        stats.file(StatsView::Repo, Path::new("f.txt")).is_none(),
        "the refused HEAD read leaves the repo view of the path empty"
    );
    let sets = staged_lines::find(&stats.line_sets, Path::new("f.txt")).expect("the file");
    assert_eq!(
        (
            sets.staged.added.runs(),
            sets.staged.removed.runs(),
            sets.local.added.runs(),
            sets.local.removed.runs(),
        ),
        (&[(1usize, 2usize)][..], &[][..], &[][..], &[][..]),
        "the index-facing halves answer normally: {sets:?}"
    );
    assert!(!sets.conflicted, "a fresh repository is not a merge");

    // And the refusal is not a sticky verdict. `refresh` drops the read's rule
    // state with the values it governs, so once a first commit exists the same
    // target answers rows rather than remembering that it once failed.
    git(&repo, &["commit", "-q", "-m", "first"]);
    std::fs::write(repo.join("f.txt"), "alpha\nbravo\ncharlie\n").unwrap();
    state.refresh(Affected::All);
    let value = combined_value(&mut state, &repo, "f.txt");
    assert_eq!(
        geometry(&value.model),
        [(RowKind::Add, "charlie".to_owned(), 0, 3)],
        "the first real HEAD diff, answered through the same key"
    );
    assert_eq!(
        git(&repo, &["status", "--porcelain", "--", "f.txt"]).trim_end(),
        " M f.txt",
        "and git agrees that this is now an ordinary modification. Trimmed at the \
         tail only, because porcelain's first column is the index state and a \
         leading space is the answer."
    );
}

/// The other state of the same root, an empty index, refuses for the same
/// reason. The one surface that still answers is the unversioned file's own
/// pane, which never asks git for a HEAD side: it is built from the file's
/// bytes, so this program changes nothing about it.
#[test]
fn an_unborn_head_with_an_empty_index_refuses_and_the_untracked_pane_still_answers() {
    let parent = tempfile::tempdir().unwrap();
    let repo = unborn_repo(parent.path(), "unborn-oracle-empty");
    std::fs::write(repo.join("scratch.txt"), "untracked\n").unwrap();

    let mut state = app_state(parent.path(), &repo);
    let root_target = DiffTarget::working_tree(repo.clone(), DiffPolicies::default(), None);
    assert_eq!(read_failure(&mut state, root_target), NO_COMMITS_MESSAGE);

    let stats = stats(&mut state, &repo);
    assert!(
        stats.line_sets.is_empty(),
        "no tracked path, so the derivation has nothing to say: {:?}",
        stats.line_sets
    );
    assert!(
        stats
            .file(StatsView::Repo, Path::new("scratch.txt"))
            .is_none(),
        "and that path has no repo-view entry"
    );
    assert_eq!(
        stats.row_is_staged(Path::new("scratch.txt"), &row_at(RowKind::Add, 0, 1)),
        None,
        "a path in no patch gets no per-line answer"
    );

    let value = combined_value(&mut state, &repo, "scratch.txt");
    assert_eq!(
        geometry(&value.model),
        [(RowKind::Add, "untracked".to_owned(), 0, 1)],
        "the unversioned pane still shows the file's own content"
    );
}

// --- the coordinate translation, against real git ----------------------------

/// One row click's whole route: translate the gutter, find the position, compose
/// the selection from the RETAINED section, and hand it to git. Nothing here
/// re-reads a patch: the section the operation applies is the one the cold root
/// fill parsed and kept, which is the whole reason
/// [`staged_lines::section_as_patch`] exists.
fn apply_located(
    one: &staged_lines::FileLineSets,
    repo: &Path,
    source: staged_lines::Source,
    hunk: usize,
    ord: usize,
    stage: bool,
) {
    let exec = executor();
    let patch = staged_lines::section_as_patch(one, source).expect("the source patch is retained");
    let selection = partial::Selection {
        hunks: [(
            hunk,
            partial::HunkSelection::Lines([ord].into_iter().collect()),
        )]
        .into_iter()
        .collect(),
    };
    let outcome = if stage {
        partial::stage_selection(&exec, repo, &patch, &selection, ChangeStatus::Modified)
    } else {
        partial::unstage_selection(&exec, repo, &patch, &selection, ChangeStatus::Modified)
    };
    outcome.expect("the located line composes to a patch git accepts");
}

/// The case that breaks a naive implementation: a HEAD gutter number that is not
/// the same line in the index, because a staged insertion above it shifted every
/// number below. Two deletions in a row are the sharpest shape of it, because
/// asking the unstaged patch with the printed number finds a real changed line,
/// namely the one ABOVE the line that was clicked.
#[test]
fn a_head_gutter_hops_the_index_before_the_unstaged_patch_is_asked_for_a_position() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "hop-over-insertion");
    seed_base(&repo);
    let staged = BASE.replace("juliet\n", "juliet\nuniform\n");
    std::fs::write(repo.join("f.txt"), &staged).unwrap();
    git(&repo, &["add", "--", "f.txt"]);
    std::fs::write(
        repo.join("f.txt"),
        staged.replace("sierra\n", "").replace("tango\n", ""),
    )
    .unwrap();

    let mut state = app_state(parent.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let sets = stats(&mut state, &repo).line_sets.clone();
    let one = staged_lines::find(&sets, Path::new("f.txt")).expect("listed");
    let sierra = only_row(&value.model, RowKind::Del, "sierra").old_no;
    let tango_row = only_row(&value.model, RowKind::Del, "tango");
    let (tango, tango_hunk) = (tango_row.old_no, tango_row.hunk);

    assert_eq!(
        (sierra, tango),
        (19, 20),
        "the pane's gutters name HEAD, because the rows come from `HEAD <-> worktree`"
    );
    // The hop: both lines are one higher in the index than in HEAD.
    assert_eq!(
        one.map(Space::Head, Space::Index, 19),
        Mapping::Both(20),
        "and not one number lower, which is what an inverted shift would say"
    );
    assert_eq!(one.map(Space::Head, Space::Index, 20), Mapping::Both(21));
    // The trap, stated as the wrong answer: index 20 is `sierra` and is a real
    // changed line of the unstaged patch, so an untranslated ask does not fail,
    // it stages the neighbour.
    assert_eq!(
        one.locate(Source::Local, Side::Pre, 20),
        Ordinal::At { hunk: 0, ord: 0 },
        "the click on `tango` answered without the hop"
    );
    // The two answers a click actually composes from.
    let ask = |old_no, toggle| {
        row_source(
            &sets,
            Path::new("f.txt"),
            only_row_at(&value.model, RowKind::Del, old_no),
            Displayed::HeadVsWorktree,
            toggle,
        )
    };
    assert_eq!(
        ask(tango, Toggle::Stage),
        RowSource::Located {
            source: Source::Local,
            hunk: 0,
            ord: 1
        },
        "`tango` is index 21, the second deletion of the hunk"
    );
    assert_eq!(
        ask(sierra, Toggle::Stage),
        RowSource::Located {
            source: Source::Local,
            hunk: 0,
            ord: 0
        }
    );
    // And the numbering the plan insists stays separate: the displayed patch
    // counts the staged insertion as its own hunk, so the row's `hunk` is one
    // ahead of the index the retained unstaged section enumerates.
    assert_eq!(
        tango_hunk,
        1,
        "the displayed patch has two hunks, geometry={:?}",
        geometry(&value.model)
    );

    let displayed_before = git(&repo, &["diff", "HEAD", "--", "f.txt"]);
    let RowSource::Located { source, hunk, ord } = ask(tango, Toggle::Stage) else {
        panic!("the click has an answer");
    };
    apply_located(one, &repo, source, hunk, ord, true);
    state.refresh(Affected::Root(root_id(&repo)));

    let graded = git_states(&repo, "f.txt");
    assert_eq!(graded.porcelain, "MM f.txt", "both halves present");
    assert_eq!(
        body_line(&graded.staged, '-', "tango"),
        1,
        "the clicked deletion is in the index"
    );
    assert_eq!(
        body_line(&graded.staged, '-', "sierra"),
        0,
        "and its neighbour stayed out, which is what ordinal 1 rather than 0 buys"
    );
    assert_eq!(body_line(&graded.unstaged, '-', "sierra"), 1);
    assert_eq!(body_line(&graded.unstaged, '-', "tango"), 0);
    assert_eq!(
        git(&repo, &["diff", "HEAD", "--", "f.txt"]),
        displayed_before,
        "and the displayed combined read did not move by one byte"
    );

    let filled = stats(&mut state, &repo);
    assert_eq!(
        filled.row_is_staged(
            Path::new("f.txt"),
            only_row(&value.model, RowKind::Del, "tango")
        ),
        Some(true),
        "the clicked row flipped"
    );
    assert_eq!(
        filled.row_is_staged(
            Path::new("f.txt"),
            only_row(&value.model, RowKind::Del, "sierra")
        ),
        Some(false),
        "and the row the naive answer would have staged did not"
    );
}

/// The same translation with the shift going the other way: a staged deletion
/// above the line makes the index number smaller than the gutter the pane
/// prints, and the unstaged patch prints no such line at all.
#[test]
fn a_staged_deletion_above_shifts_the_gutter_the_other_way_and_the_hop_undoes_it() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "hop-under-deletion");
    seed_base(&repo);
    let without_bravo = BASE.replace("bravo\n", "");
    std::fs::write(repo.join("f.txt"), &without_bravo).unwrap();
    git(&repo, &["add", "--", "f.txt"]);
    std::fs::write(
        repo.join("f.txt"),
        without_bravo.replace("tango\n", "TANGO\n"),
    )
    .unwrap();

    let mut state = app_state(parent.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let sets = stats(&mut state, &repo).line_sets.clone();
    let one = staged_lines::find(&sets, Path::new("f.txt")).expect("listed");
    let tango = only_row(&value.model, RowKind::Del, "tango").old_no;
    assert_eq!(tango, 20, "HEAD 20, and index 19");
    assert_eq!(one.map(Space::Head, Space::Index, tango), Mapping::Both(19));
    assert_eq!(
        one.locate(Source::Local, Side::Pre, tango),
        Ordinal::Untouched,
        "the unstaged patch prints no index line 20, so an untranslated ask \
         declines this line rather than answering for another"
    );
    assert_eq!(
        row_source(
            &sets,
            Path::new("f.txt"),
            only_row_at(&value.model, RowKind::Del, tango),
            Displayed::HeadVsWorktree,
            Toggle::Stage
        ),
        RowSource::Located {
            source: Source::Local,
            hunk: 0,
            ord: 0
        },
        "the deletion half of the unstaged edit"
    );
    assert_eq!(
        row_source(
            &sets,
            Path::new("f.txt"),
            only_row(&value.model, RowKind::Add, "TANGO"),
            Displayed::HeadVsWorktree,
            Toggle::Stage
        ),
        RowSource::Located {
            source: Source::Local,
            hunk: 0,
            ord: 1
        },
        "and the addition half is the very next ordinal, one number lower in \
         HEAD's own space than a caller without a hop would guess"
    );

    apply_located(one, &repo, Source::Local, 0, 0, true);
    let graded = git_states(&repo, "f.txt");
    assert_eq!(graded.porcelain, "MM f.txt");
    assert_eq!(body_line(&graded.staged, '-', "bravo"), 1);
    assert_eq!(body_line(&graded.staged, '-', "tango"), 1);
    assert_eq!(body_line(&graded.unstaged, '+', "TANGO"), 1);
    assert_eq!(body_line(&graded.unstaged, '-', "tango"), 0);
}

/// One displayed hunk holding a staged change and an unstaged one: four rows,
/// two patches, and each patch's own ordinals, which are not the row's.
#[test]
fn one_displayed_hunk_anchors_rows_that_belong_to_two_different_patches() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "mixed-hunk");
    seed_base(&repo);
    let staged = BASE.replace("juliet\n", "juliet\nuniform\n");
    std::fs::write(repo.join("f.txt"), &staged).unwrap();
    git(&repo, &["add", "--", "f.txt"]);
    std::fs::write(
        repo.join("f.txt"),
        staged.replace("november\n", "NOVEMBER\n"),
    )
    .unwrap();

    let mut state = app_state(parent.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let sets = stats(&mut state, &repo).line_sets.clone();
    let one = staged_lines::find(&sets, Path::new("f.txt")).expect("listed");

    // Close enough together that `git diff HEAD` prints ONE hunk, so the four
    // rows share a displayed hunk index and a 0..3 run of displayed ordinals.
    let rows = [
        only_row(&value.model, RowKind::Add, "uniform"),
        only_row(&value.model, RowKind::Del, "november"),
        only_row(&value.model, RowKind::Add, "NOVEMBER"),
    ];
    assert_eq!(
        rows.iter().map(|r| r.hunk).collect::<Vec<_>>(),
        [0, 0, 0],
        "one hunk in the displayed patch, geometry={:?}",
        geometry(&value.model)
    );
    assert_eq!(
        rows.iter().map(|r| r.line_ord).collect::<Vec<_>>(),
        [0, 1, 2],
        "and the displayed patch's own changed-line run"
    );
    // The staged addition is the staged patch's ordinal 0, in the staged patch's
    // hunk 0; the unstaged edit is the unstaged patch's 0 and 1, in ITS hunk 0.
    // Neither answer is the row's `line_ord`.
    assert_eq!(
        row_source(
            &sets,
            Path::new("f.txt"),
            rows[0],
            Displayed::HeadVsWorktree,
            Toggle::Unstage
        ),
        RowSource::Located {
            source: Source::Staged,
            hunk: 0,
            ord: 0
        },
        "the staged addition, addressed where it is actually staged"
    );
    assert_eq!(
        row_source(
            &sets,
            Path::new("f.txt"),
            rows[1],
            Displayed::HeadVsWorktree,
            Toggle::Stage
        ),
        RowSource::Located {
            source: Source::Local,
            hunk: 0,
            ord: 0
        },
        "HEAD 14 `november` is index 15 under the staged insertion above it"
    );
    assert_eq!(one.map(Space::Head, Space::Index, 14), Mapping::Both(15));
    assert_eq!(
        row_source(
            &sets,
            Path::new("f.txt"),
            rows[2],
            Displayed::HeadVsWorktree,
            Toggle::Stage
        ),
        RowSource::Located {
            source: Source::Local,
            hunk: 0,
            ord: 1
        }
    );
    // The two wrong-way asks each name a different decline: a row that is already
    // staged has nothing for the unstaged patch to add, and a row the stage never
    // printed has nothing for the staged patch to remove.
    assert_eq!(
        row_source(
            &sets,
            Path::new("f.txt"),
            rows[0],
            Displayed::HeadVsWorktree,
            Toggle::Stage
        ),
        RowSource::Missing(NoSource::Untouched)
    );
    assert_eq!(
        row_source(
            &sets,
            Path::new("f.txt"),
            rows[1],
            Displayed::HeadVsWorktree,
            Toggle::Unstage
        ),
        RowSource::Missing(NoSource::Untouched)
    );
    assert!(
        one.staged.removed.is_empty(),
        "the staged patch only adds, which is why the deletion row has no \
         staged answer: {:?}",
        one.staged.removed.runs()
    );

    // The round trip the plan asks for, on the numbers real git printed rather
    // than on invented ones: every line of every row that exists in both images
    // of a hop maps forward and back to the same number, in each of the four
    // single hops and in both compositions.
    for row in changed_rows(&value.model) {
        let (space, number) = match row.kind {
            RowKind::Del => (Space::Head, row.old_no),
            RowKind::Add => (Space::Worktree, row.new_no),
            _ => continue,
        };
        for other in [Space::Head, Space::Index, Space::Worktree] {
            if let Mapping::Both(far) = one.map(space, other, number) {
                assert_eq!(
                    one.map(other, space, far),
                    Mapping::Both(number),
                    "{space:?} {number} -> {other:?} {far} -> back, row {:?}",
                    (row.kind, &row.text)
                );
            }
        }
    }
}

/// Ticket 03's duplicate fixture, walked all the way to an operation: two
/// adjacent `dup` lines the file holds, one of them staged. Content cannot tell
/// them apart, and the answers do not either, until the click has to say which
/// patch and which line of it.
#[test]
fn two_identical_adjacent_additions_reach_the_same_ordinal_in_two_different_patches() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "duplicate-hop");
    commit(&repo, "dup.txt", "alpha\nbravo\n", "base");
    std::fs::write(repo.join("dup.txt"), "alpha\nbravo\ndup\n").unwrap();
    git(&repo, &["add", "--", "dup.txt"]);
    std::fs::write(repo.join("dup.txt"), "alpha\nbravo\ndup\ndup\n").unwrap();

    let mut state = app_state(parent.path(), &repo);
    let value = combined_value(&mut state, &repo, "dup.txt");
    let sets = stats(&mut state, &repo).line_sets.clone();
    let one = staged_lines::find(&sets, Path::new("dup.txt")).expect("listed");
    let adds: Vec<&Row> = rows(&value.model)
        .into_iter()
        .filter(|r| r.kind == RowKind::Add)
        .collect();
    assert_eq!(
        adds.iter().map(|r| r.new_no).collect::<Vec<_>>(),
        [3, 4],
        "one identical text, two worktree numbers"
    );

    // The staged copy is the staged patch's only changed line; the further copy
    // is the unstaged patch's only changed line. Same hunk, same ordinal,
    // different patches, and only the position says which is which.
    assert_eq!(
        row_source(
            &sets,
            Path::new("dup.txt"),
            adds[0],
            Displayed::HeadVsWorktree,
            Toggle::Unstage
        ),
        RowSource::Located {
            source: Source::Staged,
            hunk: 0,
            ord: 0
        },
        "worktree 3 hops to index 3, the line the stage added"
    );
    assert_eq!(one.map(Space::Worktree, Space::Index, 3), Mapping::Both(3));
    assert_eq!(
        row_source(
            &sets,
            Path::new("dup.txt"),
            adds[1],
            Displayed::HeadVsWorktree,
            Toggle::Stage
        ),
        RowSource::Located {
            source: Source::Local,
            hunk: 0,
            ord: 0
        },
        "worktree 4 needs no hop, and is the unstaged patch's own ordinal 0"
    );
    // Each wrong-way ask declines, and declines differently: the staged copy is
    // context in the unstaged patch, while the worktree copy the index does not
    // hold has no index number to look up at all.
    assert_eq!(
        row_source(
            &sets,
            Path::new("dup.txt"),
            adds[0],
            Displayed::HeadVsWorktree,
            Toggle::Stage
        ),
        RowSource::Missing(NoSource::Context { hunk: 0 })
    );
    assert_eq!(
        row_source(
            &sets,
            Path::new("dup.txt"),
            adds[1],
            Displayed::HeadVsWorktree,
            Toggle::Unstage
        ),
        RowSource::Missing(NoSource::NoFarSide)
    );

    apply_located(one, &repo, Source::Staged, 0, 0, false);
    let graded = git_states(&repo, "dup.txt");
    assert_eq!(
        graded.porcelain, " M dup.txt",
        "both copies back out of the index"
    );
    assert_eq!(body_line(&graded.staged, '+', "dup"), 0);
    assert_eq!(
        body_line(&graded.unstaged, '+', "dup"),
        2,
        "one operation took the first copy and left the second, which no \
         content-based pairing could have chosen"
    );
    // The op seam's own completion step, because the fixture moved git behind the
    // app's back: the statistics are compute-on-miss, so nothing else drops the
    // entry, and a refetch that never happened would answer from the stale one.
    state.refresh(Affected::Root(root_id(&repo)));
    let filled = stats(&mut state, &repo);
    for row in adds {
        assert_eq!(
            filled.row_is_staged(Path::new("dup.txt"), row),
            Some(false),
            "and the oracle now calls both copies unstaged"
        );
    }
}

/// The hand-off in one round trip: the ordinal `row_source` reports for a real
/// staged line, composed out of the section the root fill retained, reverse-applies
/// exactly that line and nothing else.
#[test]
fn the_retained_section_and_the_located_ordinal_reverse_apply_one_staged_line() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(parent.path(), "retained-reverse");
    seed_base(&repo);
    std::fs::write(repo.join("f.txt"), ALL_THREE).unwrap();
    git(&repo, &["add", "--", "f.txt"]);

    let mut state = app_state(parent.path(), &repo);
    let value = combined_value(&mut state, &repo, "f.txt");
    let sets = stats(&mut state, &repo).line_sets.clone();
    let one = staged_lines::find(&sets, Path::new("f.txt")).expect("listed");
    let uniform = only_row(&value.model, RowKind::Add, "uniform");
    let displayed_before = git(&repo, &["diff", "HEAD", "--", "f.txt"]);

    assert_eq!(
        row_source(
            &sets,
            Path::new("f.txt"),
            uniform,
            Displayed::HeadVsWorktree,
            Toggle::Unstage
        ),
        RowSource::Located {
            source: Source::Staged,
            hunk: 1,
            ord: 0
        },
        "the second hunk of the staged section, its only changed line"
    );
    // The same hunk index git's own text says, reached without a second read:
    // the insertion is one hunk of its own because the three edits are far apart.
    let exec = executor();
    let read = exec
        .diff_patch(
            &repo,
            &DiffOpts {
                staged: true,
                ..DiffOpts::default()
            },
        )
        .unwrap();
    let position = read.files[0]
        .hunks
        .iter()
        .position(|h| {
            h.lines
                .iter()
                .any(|l| l.text == "uniform" && l.kind == PatchLineKind::Added)
        })
        .expect("`uniform` is a hunk of its own");
    assert_eq!(
        read.files[0].hunks[position]
            .lines
            .iter()
            .filter(|l| l.kind != PatchLineKind::Context)
            .count(),
        1,
        "one changed line in it, so ordinal 0 is the whole of the selection"
    );

    apply_located(one, &repo, Source::Staged, 1, 0, false);
    let graded = git_states(&repo, "f.txt");
    assert_eq!(
        graded.porcelain, "MM f.txt",
        "git agrees: both halves present"
    );
    assert_eq!(
        body_line(&graded.staged, '+', "uniform"),
        0,
        "out of the index"
    );
    assert_eq!(
        body_line(&graded.staged, '+', "BRAVO"),
        1,
        "still in the index"
    );
    assert_eq!(
        body_line(&graded.unstaged, '+', "uniform"),
        1,
        "and now unstaged"
    );
    assert_eq!(
        git(&repo, &["diff", "HEAD", "--", "f.txt"]),
        displayed_before,
        "the displayed read never moved"
    );
    assert_eq!(
        position, 1,
        "and the retained section agrees with git's own order"
    );
}

/// The states a click must not be able to compose from, on real repositories:
/// an unmerged path, and a path in no index-facing patch at all.
#[test]
fn an_unmerged_or_untracked_row_has_no_source_at_all() {
    let parent = tempfile::tempdir().unwrap();
    let repo = repo_with_conflict(parent.path(), "no-source");
    let mut state = app_state(parent.path(), &repo);
    let value = combined_value(&mut state, &repo, "conf.txt");
    let sets = stats(&mut state, &repo).line_sets.clone();
    for row in changed_rows(&value.model) {
        for (displayed, toggle) in [
            (Displayed::HeadVsWorktree, Toggle::Stage),
            (Displayed::HeadVsWorktree, Toggle::Unstage),
            (Displayed::IndexVsWorktree, Toggle::Stage),
        ] {
            assert_eq!(
                row_source(&sets, Path::new("conf.txt"), row, displayed, toggle),
                RowSource::Missing(NoSource::Conflicted),
                "no line of an unmerged path is appliable: {:?}",
                (row.kind, &row.text)
            );
        }
    }
    // A conflicted file retains no section either, so there is nothing to
    // compose even if a caller ignored the answer above.
    let one = staged_lines::find(&sets, Path::new("conf.txt")).expect("listed");
    assert!(
        one.local_section.is_none() && one.staged_section.is_none(),
        "a `diff --cc` body is not an appliable patch"
    );

    let clean = repo_with_one_commit(parent.path(), "untracked-no-source");
    std::fs::write(clean.join("scratch.txt"), "untracked\n").unwrap();
    let mut state = app_state(parent.path(), &clean);
    let value = combined_value(&mut state, &clean, "scratch.txt");
    let sets = stats(&mut state, &clean).line_sets.clone();
    assert_eq!(
        row_source(
            &sets,
            Path::new("scratch.txt"),
            only_row(&value.model, RowKind::Add, "untracked"),
            Displayed::HeadVsWorktree,
            Toggle::Stage
        ),
        RowSource::Missing(NoSource::PathAbsent),
        "an untracked path is in no index-facing patch, so this module has no \
         position to report and no reason to invent one"
    );
}

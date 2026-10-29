use std::path::{Path, PathBuf};
use std::sync::Arc;

use test_support::git_seed::repo_with_one_commit;
use test_support::srcscan;
use turbogit_app::diff_model::{DiffValue, DisplayRow, Highlight, RowKind, line_counts};
use turbogit_app::keyed_read::{DiffTarget, Keyed, Read};
use turbogit_app::state::AppState;
use turbogit_domain::model::{DiffHighlighting, DiffPolicies, DiffWhitespace};
use turbogit_engine::fake::FakeExecutor;

fn repo(dir: &Path) -> PathBuf {
    repo_with_one_commit(dir, "work")
}

fn patch(dels: &[&str], adds: &[&str]) -> String {
    let mut body = String::from(
        "diff --git a/ws.txt b/ws.txt\nindex 1111111..2222222 100644\n\
         --- a/ws.txt\n+++ b/ws.txt\n@@ -1,2 +1,2 @@\n one\n",
    );
    for d in dels {
        body.push_str(&format!("-{d}\n"));
    }
    for a in adds {
        body.push_str(&format!("+{a}\n"));
    }
    body.push_str(" three\n");
    body
}

fn rendered(work: &Path, text: &str, policies: DiffPolicies) -> Arc<DiffValue> {
    let mut fake = FakeExecutor::default();
    fake.diffs.insert(work.to_path_buf(), text.to_string());
    let mut state = AppState::for_roots(
        work.parent().unwrap(),
        std::slice::from_ref(&work.to_path_buf()),
    )
    .with_executor(Arc::new(fake));
    while state.drain_events() > 0 {}
    let target = DiffTarget::new(
        work.to_path_buf(),
        None,
        None,
        policies,
        Some(PathBuf::from("ws.txt")),
    );
    assert!(matches!(state.read(target.clone()), Read::Waiting));
    state.drain_events();
    let Read::Fresh(diff) = state.read(target) else {
        panic!("the canned patch settles as the diff's answer");
    };
    diff
}

#[test]
fn the_whitespace_ladder_hides_one_more_kind_of_change_at_each_rung() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path());
    let at = |ws: DiffWhitespace| DiffPolicies {
        whitespace: ws,
        ..DiffPolicies::default()
    };

    let trailing = patch(&["gamma"], &["gamma  "]);
    assert_eq!(
        line_counts(&rendered(&work, &trailing, at(DiffWhitespace::None)).model),
        (1, 1)
    );
    assert_eq!(
        line_counts(&rendered(&work, &trailing, at(DiffWhitespace::TrimWhitespace)).model),
        (0, 0),
        "a line that differs only at its end is not a difference"
    );

    let internal = patch(&["g amma"], &["gamma"]);
    assert_eq!(
        line_counts(&rendered(&work, &internal, at(DiffWhitespace::TrimWhitespace)).model),
        (1, 1)
    );
    assert_eq!(
        line_counts(&rendered(&work, &internal, at(DiffWhitespace::IgnoreWhitespace)).model),
        (0, 0)
    );

    let blank_line = patch(&[], &["  "]);
    assert_eq!(
        line_counts(&rendered(&work, &blank_line, at(DiffWhitespace::IgnoreWhitespace)).model),
        (1, 0)
    );
    assert_eq!(
        line_counts(
            &rendered(
                &work,
                &blank_line,
                at(DiffWhitespace::IgnoreWhitespaceAndEmptyLines)
            )
            .model
        ),
        (0, 0)
    );

    let joined = patch(&["a b c"], &["a", "b c"]);
    assert_eq!(
        line_counts(&rendered(&work, &joined, at(DiffWhitespace::IgnoreWhitespace)).model),
        (2, 1)
    );
    assert_eq!(
        line_counts(
            &rendered(
                &work,
                &joined,
                at(DiffWhitespace::IgnoreWhitespaceAndEmptyLines)
            )
            .model
        ),
        (0, 0)
    );

    let imports = patch(&["use std::fs;", "use std::io;"], &["use std::io;"]);
    assert_eq!(
        line_counts(
            &rendered(
                &work,
                &imports,
                at(DiffWhitespace::IgnoreWhitespaceAndEmptyLines)
            )
            .model
        ),
        (1, 2)
    );
    assert_eq!(
        line_counts(
            &rendered(
                &work,
                &imports,
                at(DiffWhitespace::IgnoreImportsAndFormatting)
            )
            .model
        ),
        (0, 0)
    );

    let literal = patch(&["let s = \"a b\";"], &["let s = \"a  b\";"]);
    assert_eq!(
        line_counts(
            &rendered(
                &work,
                &literal,
                at(DiffWhitespace::IgnoreWhitespaceAndEmptyLines)
            )
            .model
        ),
        (0, 0)
    );
    assert_eq!(
        line_counts(
            &rendered(
                &work,
                &literal,
                at(DiffWhitespace::IgnoreImportsAndFormatting)
            )
            .model
        ),
        (1, 1)
    );
}

#[test]
fn an_equalized_difference_paints_one_unchanged_line() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path());
    let policies = DiffPolicies {
        whitespace: DiffWhitespace::TrimWhitespace,
        ..DiffPolicies::default()
    };
    let diff = rendered(&work, &patch(&["gamma"], &["gamma  "]), policies);
    let rows: Vec<_> = diff
        .model
        .display
        .iter()
        .filter_map(|r| match r {
            DisplayRow::Full(row) if row.kind == RowKind::Context => Some(row),
            _ => None,
        })
        .collect();
    let equalized = rows
        .iter()
        .find(|r| r.text == "gamma  ")
        .expect("the new side's line stands where the pair was");
    assert_eq!(
        (equalized.old_no, equalized.new_no),
        (2, 2),
        "one line, one position on each side"
    );
    assert_eq!(
        equalized.highlight,
        Highlight::Off,
        "context never highlights"
    );
    assert_eq!(
        equalized.line_ord, 0,
        "an equalized line is not a selectable change, so it owns no changed-line ordinal"
    );
    assert!(
        diff.patch.to_string().contains("-gamma\n"),
        "the patch the read answers with is untouched: the policy is a view decision"
    );
}

#[test]
fn each_highlighting_value_cuts_the_change_at_its_own_granularity() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path());
    let at = |highlighting: DiffHighlighting| DiffPolicies {
        highlighting,
        ..DiffPolicies::default()
    };
    let two_words = patch(&["alpha beta gamma"], &["ALPHA beta GAMMA"]);
    let inner = patch(&["alpha BeTa gamMA"], &["Alpha beta GAMMA"]);

    let none = rendered(&work, &two_words, at(DiffHighlighting::None));
    let (adds, _) = line_counts(&none.model);
    assert_eq!(adds, 1);
    assert_eq!(
        highlight_of(&none.model),
        Highlight::Off,
        "the value that removes highlighting entirely"
    );

    assert_eq!(
        highlight_of(&rendered(&work, &two_words, at(DiffHighlighting::Lines)).model),
        Highlight::Row
    );

    let words = highlight_of(&rendered(&work, &two_words, at(DiffHighlighting::Words)).model);
    assert_eq!(run_count(&words), 2, "two changed words: {words:?}");

    let chars = highlight_of(&rendered(&work, &inner, at(DiffHighlighting::Characters)).model);
    let words_inner = highlight_of(&rendered(&work, &inner, at(DiffHighlighting::Words)).model);
    assert!(
        run_count(&chars) > run_count(&words_inner),
        "character granularity cuts inside a word: {chars:?} vs {words_inner:?}"
    );

    let carried = patch(&["alpha", "beta"], &["alpha", "BETA"]);
    let lines = rendered(&work, &carried, at(DiffHighlighting::Lines));
    let split = rendered(&work, &carried, at(DiffHighlighting::SplitChanges));
    assert_eq!(
        line_counts(&lines.model),
        (2, 2),
        "one change over both lines"
    );
    assert_eq!(
        line_counts(&split.model),
        (1, 1),
        "two changes, and the carried line is neither of them"
    );
}

#[test]
fn a_policy_change_reparses_once_and_never_serves_the_other_policys_model() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path());
    let text = patch(&["gamma"], &["gamma  "]);
    let mut fake = FakeExecutor::default();
    fake.diffs.insert(work.to_path_buf(), text.clone());
    let mut state = AppState::for_roots(
        work.parent().unwrap(),
        std::slice::from_ref(&work.to_path_buf()),
    )
    .with_executor(Arc::new(fake));
    while state.drain_events() > 0 {}

    let ask = |state: &mut AppState, ws: DiffWhitespace| {
        let target = DiffTarget::new(
            work.to_path_buf(),
            None,
            None,
            DiffPolicies {
                whitespace: ws,
                ..DiffPolicies::default()
            },
            Some(PathBuf::from("ws.txt")),
        );
        (target.key(), state.read(target))
    };
    let (key_none, first) = ask(&mut state, DiffWhitespace::None);
    assert!(matches!(first, Read::Waiting));
    state.drain_events();
    let (_, first) = ask(&mut state, DiffWhitespace::None);
    let Read::Fresh(first) = first else {
        panic!("the first policy's answer settles");
    };
    assert_eq!(line_counts(&first.model), (1, 1));

    let (key_trim, second) = ask(&mut state, DiffWhitespace::TrimWhitespace);
    assert_ne!(key_none, key_trim, "the policy is inside the key");
    assert!(
        matches!(second, Read::Waiting),
        "a policy change re-reads once"
    );
    state.drain_events();
    let (_, third) = ask(&mut state, DiffWhitespace::TrimWhitespace);
    let Read::Fresh(third) = third else {
        panic!("the second policy's answer settles");
    };
    assert_eq!(line_counts(&third.model), (0, 0));
    assert!(
        !state.read_pending(),
        "and the second ask spent no further git work"
    );
}

#[test]
fn both_policies_come_back_from_the_project_on_the_next_launch() {
    let project = tempfile::tempdir().unwrap();
    let recents = tempfile::tempdir().unwrap();
    let set = DiffPolicies {
        whitespace: DiffWhitespace::IgnoreWhitespaceAndEmptyLines,
        highlighting: DiffHighlighting::Characters,
    };

    let mut state = AppState::for_roots(project.path(), &[]);
    state.ui.diff_policies = set;
    state.save_durable_ui();

    let written =
        std::fs::read_to_string(project.path().join(".turbogit/ui.ron")).expect("ui.ron exists");
    assert!(
        written.contains("IgnoreWhitespaceAndEmptyLines") && written.contains("Characters"),
        "both policies reached the file: {written}"
    );
    assert_eq!(
        turbogit_app::persistence::load_ui_state(project.path()).diff_policies,
        set,
        "and both read back as the same pair"
    );

    let relaunched = AppState::launch_in(
        Some(project.path().to_path_buf()),
        Some(recents.path().to_path_buf()),
    );
    assert_eq!(
        relaunched.ui.diff_policies, set,
        "a launch restores the pair into the session the viewer reads"
    );
}

#[test]
fn a_ui_ron_written_before_the_policies_existed_loads_the_shipped_defaults() {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join(".turbogit")).unwrap();
    std::fs::write(
        project.path().join(".turbogit/ui.ron"),
        "(tab: \"Commit\", recent_repos: [], draft_message: \"hi\")",
    )
    .unwrap();

    let loaded = turbogit_app::persistence::load_ui_state(project.path());

    assert_eq!(loaded.diff_policies, DiffPolicies::default());
    assert_eq!(
        loaded.diff_policies.whitespace,
        DiffWhitespace::None,
        "the shipped posture is the reference's `None`: white space is important"
    );
    assert_eq!(
        loaded.diff_policies.highlighting,
        DiffHighlighting::Lines,
        "and highlighting keeps the rendering the surface had before the menu"
    );
}

fn highlight_of(model: &turbogit_app::diff_model::DiffModel) -> Highlight {
    for row in &model.display {
        if let DisplayRow::Pair(_, Some(add)) = row {
            return add.highlight.clone();
        }
        if let DisplayRow::Full(r) = row
            && r.kind == RowKind::Add
        {
            return r.highlight.clone();
        }
    }
    panic!("the model has no changed row to read a highlight from");
}

fn run_count(h: &Highlight) -> usize {
    match h {
        Highlight::Runs(r) => r.len(),
        Highlight::Row => 1,
        Highlight::Off => 0,
    }
}

const DIFF_POLICIES_SRC: &str = include_str!("diff_policies.rs");

#[test]
fn the_policy_suite_reads_the_owners_changed_row_count() {
    assert!(
        !DIFF_POLICIES_SRC
            .lines()
            .any(|line| srcscan::declares(line, "changed")),
        "diff_policies must not keep a local copy of the changed-row count: \
         turbogit_app::diff_model::line_counts owns it (lane 5, ticket 13), \
         and every count below reads it"
    );
}

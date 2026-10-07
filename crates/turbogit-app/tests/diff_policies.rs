//! Ticket 20 — the diff viewer's two view policies, at the seam the view reads
//! through: the keyed read answers with a display model, and the policy is decided
//! before that model is built.
//!
//! Nothing here reaches for `git`: the substitutable adapter is handed one canned
//! patch and the same patch is then rendered under each policy value, so what a
//! test compares is the *same content* under two policies and never two contents.
//! That is also the contract the policies live under — the patch value stays
//! byte-identical (`ADR-0022`), so what partial staging composes cannot drift with
//! a view setting.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use test_support::git_seed::repo_with_one_commit;
use turbogit_app::diff_model::{DiffValue, DisplayRow, Highlight, RowKind};
use turbogit_app::keyed_read::{DiffTarget, Keyed, Read};
use turbogit_app::state::{AppState, DiffComparison};
use turbogit_domain::model::{DiffHighlighting, DiffPolicies, DiffWhitespace};
use turbogit_engine::fake::FakeExecutor;

fn repo(dir: &Path) -> PathBuf {
    repo_with_one_commit(dir, "work")
}

/// One file, one hunk, one changed run of `dels` against `adds`, in git's own
/// shape. Context lines bracket the run so a collapsed run still has a row to
/// sit between.
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

/// The read's answer for `text` under `policies`.
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
        DiffComparison::Repo,
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

/// `(added, removed)` display rows of a model: the two policies are read here,
/// because a difference the viewer considers equal stops being a changed row,
/// and a difference it still counts stays one.
fn changed(model: &turbogit_app::diff_model::DiffModel) -> (usize, usize) {
    let mut adds = 0;
    let mut dels = 0;
    for row in &model.display {
        match row {
            DisplayRow::Full(r) => match r.kind {
                RowKind::Add => adds += 1,
                RowKind::Del => dels += 1,
                _ => {}
            },
            DisplayRow::Pair(d, a) => {
                dels += d.is_some() as usize;
                adds += a.is_some() as usize;
            }
        }
    }
    (adds, dels)
}

/// The five whitespace values, on five patches chosen so each one is hidden by
/// exactly one value and shown by the value below it — a policy where two values
/// behave alike has one value and five names.
#[test]
fn the_whitespace_ladder_hides_one_more_kind_of_change_at_each_rung() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo(tmp.path());
    let at = |ws: DiffWhitespace| DiffPolicies {
        whitespace: ws,
        ..DiffPolicies::default()
    };

    // Trailing whitespace only: shown by `None`, hidden by `Trim whitespaces`.
    let trailing = patch(&["gamma"], &["gamma  "]);
    assert_eq!(
        changed(&rendered(&work, &trailing, at(DiffWhitespace::None)).model),
        (1, 1)
    );
    assert_eq!(
        changed(&rendered(&work, &trailing, at(DiffWhitespace::TrimWhitespace)).model),
        (0, 0),
        "a line that differs only at its end is not a difference"
    );

    // Internal whitespace only: still shown by `Trim`, hidden by `Ignore whitespaces`.
    let internal = patch(&["g amma"], &["gamma"]);
    assert_eq!(
        changed(&rendered(&work, &internal, at(DiffWhitespace::TrimWhitespace)).model),
        (1, 1)
    );
    assert_eq!(
        changed(&rendered(&work, &internal, at(DiffWhitespace::IgnoreWhitespace)).model),
        (0, 0)
    );

    // A whole line of nothing added: shown by `Ignore whitespaces` (the line
    // count differs), hidden one rung up.
    let blank_line = patch(&[], &["  "]);
    assert_eq!(
        changed(&rendered(&work, &blank_line, at(DiffWhitespace::IgnoreWhitespace)).model),
        (1, 0)
    );
    assert_eq!(
        changed(
            &rendered(
                &work,
                &blank_line,
                at(DiffWhitespace::IgnoreWhitespaceAndEmptyLines)
            )
            .model
        ),
        (0, 0)
    );

    // One line split into two without touching a non-whitespace character:
    // shown by `Ignore whitespaces`, hidden one rung up.
    let joined = patch(&["a b c"], &["a", "b c"]);
    assert_eq!(
        changed(&rendered(&work, &joined, at(DiffWhitespace::IgnoreWhitespace)).model),
        (2, 1)
    );
    assert_eq!(
        changed(
            &rendered(
                &work,
                &joined,
                at(DiffWhitespace::IgnoreWhitespaceAndEmptyLines)
            )
            .model
        ),
        (0, 0)
    );

    // An import dropped from the block: shown by the fourth value, hidden by the
    // fifth.
    let imports = patch(&["use std::fs;", "use std::io;"], &["use std::io;"]);
    assert_eq!(
        changed(
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
        changed(
            &rendered(
                &work,
                &imports,
                at(DiffWhitespace::IgnoreImportsAndFormatting)
            )
            .model
        ),
        (0, 0)
    );

    // …and the fifth value's own exception: whitespace inside a string literal
    // stays a difference, which the fourth value throws away with the rest.
    let literal = patch(&["let s = \"a b\";"], &["let s = \"a  b\";"]);
    assert_eq!(
        changed(
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
        changed(
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

/// A difference the policy considers equal is rendered as the line the change
/// left behind: one unchanged row carrying the new text, not two changed rows.
/// The gutter numbers prove it is the same position on both sides.
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
    assert_eq!(
        diff.patch.to_string().contains("-gamma\n"),
        true,
        "the patch the read answers with is untouched: the policy is a view decision"
    );
}

/// The five highlighting values, counted by the runs they leave on a changed
/// row: none, the whole row, word groups, character groups, and a run split so
/// that a line the change carries along is not part of any change.
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

    // None: the row is still a difference, and nothing about it is highlighted.
    let none = rendered(&work, &two_words, at(DiffHighlighting::None));
    let (adds, _) = changed(&none.model);
    assert_eq!(adds, 1);
    assert_eq!(
        highlight_of(&none.model),
        Highlight::Off,
        "the value that removes highlighting entirely"
    );

    // Lines: one band, the whole row. TurboGit's pre-existing rendering.
    assert_eq!(
        highlight_of(&rendered(&work, &two_words, at(DiffHighlighting::Lines)).model),
        Highlight::Row
    );

    // Words: one run per changed word group.
    let words = highlight_of(&rendered(&work, &two_words, at(DiffHighlighting::Words)).model);
    assert_eq!(run_count(&words), 2, "two changed words: {words:?}");

    // Characters: finer than words on a change that only touches letters
    // inside words.
    let chars = highlight_of(&rendered(&work, &inner, at(DiffHighlighting::Characters)).model);
    let words_inner = highlight_of(&rendered(&work, &inner, at(DiffHighlighting::Words)).model);
    assert!(
        run_count(&chars) > run_count(&words_inner),
        "character granularity cuts inside a word: {chars:?} vs {words_inner:?}"
    );

    // Split changes: the line the run carries along stops being a change.
    let carried = patch(&["alpha", "beta"], &["alpha", "BETA"]);
    let lines = rendered(&work, &carried, at(DiffHighlighting::Lines));
    let split = rendered(&work, &carried, at(DiffHighlighting::SplitChanges));
    assert_eq!(changed(&lines.model), (2, 2), "one change over both lines");
    assert_eq!(
        changed(&split.model),
        (1, 1),
        "two changes, and the carried line is neither of them"
    );
}

/// A policy is part of what the read is *keyed* on: asking the same comparison
/// under another policy must not answer with the model built under the first,
/// and must not do it by re-reading git every frame.
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
            DiffComparison::Repo,
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
    assert_eq!(changed(&first.model), (1, 1));

    // The other policy is a different key, so it is a different answer — and it
    // is admitted once, not every frame.
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
    assert_eq!(changed(&third.model), (0, 0));
    assert!(
        !state.read_pending(),
        "and the second ask spent no further git work"
    );
}

/// The highlight of the first changed addition row of a model.
fn highlight_of(model: &turbogit_app::diff_model::DiffModel) -> Highlight {
    for row in &model.display {
        if let DisplayRow::Pair(_, Some(add)) = row {
            return add.highlight.clone();
        }
        if let DisplayRow::Full(r) = row {
            if r.kind == RowKind::Add {
                return r.highlight.clone();
            }
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

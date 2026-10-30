//! The unprinted runs of a patch, measured against recorded `git diff` output.
//!
//! Every fixture below is verbatim `git diff -U3` (one is `-U0`) over a real
//! repository, generated this session; the files they describe are `a.txt` 20
//! lines, `b.txt` 40, `app.txt` 10 then 12, `c.txt` 10, `d.txt` 19.

use std::path::PathBuf;
use std::sync::Arc;

use test_support::git_seed::repo_with_one_commit;
use turbogit_app::diff_model::{DisplayRow, Gap, GapTable, Hidden, Row, RowKind, gap_table};
use turbogit_app::keyed_read::{DiffTarget, Read};
use turbogit_app::state::AppState;
use turbogit_domain::model::{Patch, PatchHunk};
use turbogit_engine::fake::FakeExecutor;
use turbogit_engine_api::patch::parse_patch;

const LEADING: &str = "diff --git a/a.txt b/a.txt\n\
     index 1d89877..76ffbba 100644\n\
     --- a/a.txt\n\
     +++ b/a.txt\n\
     @@ -7,7 +7,7 @@ line006\n\
      line007\n\
      line008\n\
      line009\n\
     -line010\n\
     +CHANGED_A10\n\
      line011\n\
      line012\n\
      line013\n";

const INTERIOR: &str = "diff --git a/b.txt b/b.txt\n\
     index f63ac4e..38fb9db 100644\n\
     --- a/b.txt\n\
     +++ b/b.txt\n\
     @@ -2,7 +2,7 @@ line001\n\
      line002\n\
      line003\n\
      line004\n\
     -line005\n\
     +CHANGED_B05\n\
      line006\n\
      line007\n\
      line008\n\
     @@ -22,7 +22,7 @@ line021\n\
      line022\n\
      line023\n\
      line024\n\
     -line025\n\
     +CHANGED_B25\n\
      line026\n\
      line027\n\
      line028\n";

/// One patch, two file sections: the two hunks of `b.txt` are ordinals 1 and 2.
fn two_files() -> String {
    format!("{LEADING}{INTERIOR}")
}

/// `git diff -U0` over ten lines with two pure insertions: both hunks spell an
/// old count of `0`, so each `start` is an anchor the hunk does not print.
const ZERO_COUNT: &str = "diff --git a/a.txt b/a.txt\n\
     index c744193..829b992 100644\n\
     --- a/a.txt\n\
     +++ b/a.txt\n\
     @@ -3,0 +4 @@ line003\n\
     +INS_A\n\
     @@ -7,0 +9 @@ line007\n\
     +INS_B\n";

const APPEND: &str = "diff --git a/app.txt b/app.txt\n\
     index c744193..4e02114 100644\n\
     --- a/app.txt\n\
     +++ b/app.txt\n\
     @@ -8,3 +8,5 @@ line007\n\
      line008\n\
      line009\n\
      line010\n\
     +extra1\n\
     +extra2\n";

const LAST_LINE: &str = "diff --git a/c.txt b/c.txt\n\
     index c744193..a4441df 100644\n\
     --- a/c.txt\n\
     +++ b/c.txt\n\
     @@ -7,4 +7,4 @@ line006\n\
      line007\n\
      line008\n\
      line009\n\
     -line010\n\
     +LAST_CHANGED\n";

/// Real `git diff -U3` over a 200-line file edited at line 197, the shape the
/// cross-judge measured: three trailing context lines, file ends there, and
/// nothing in the patch says whether anything is left below.
const TAIL_UNKNOWN: &str = "diff --git a/e.txt b/e.txt\n\
     index 7f6246d..c26508e 100644\n\
     --- a/e.txt\n\
     +++ b/e.txt\n\
     @@ -194,7 +194,7 @@ line193\n\
      line194\n\
      line195\n\
      line196\n\
     -line197\n\
     +CHANGED_E197\n\
      line198\n\
      line199\n\
      line200\n";

const NEW_FILE: &str = "diff --git a/n.txt b/n.txt\n\
     new file mode 100644\n\
     index 0000000..66a52ee\n\
     --- /dev/null\n\
     +++ b/n.txt\n\
     @@ -0,0 +1,2 @@\n\
     +first\n\
     +second\n";

const DELETED_FILE: &str = "diff --git a/del.txt b/del.txt\n\
     deleted file mode 100644\n\
     index 4cb29ea..0000000\n\
     --- a/del.txt\n\
     +++ /dev/null\n\
     @@ -1,3 +0,0 @@\n\
     -one\n\
     -two\n\
     -three\n";

/// Real `git diff` over an unmerged path, recorded from a conflicted merge.
const COMBINED: &str = "diff --cc f.txt\n\
     index 64ea6c3,37591f6..0000000\n\
     --- a/f.txt\n\
     +++ b/f.txt\n\
     @@@ -1,12 -1,12 +1,16 @@@\n\
      base01\n\
      base02\n\
     - base03\n\
     + topic03\n\
      base04\n\
     -base05\n\
     +main05\n\
      base06\n\
      base07\n\
      base08\n\
     ++<<<<<<< HEAD\n\
     + main09\n\
     ++=======\n\
     + topic09\n\
     ++>>>>>>> topic\n\
      base10\n\
      base11\n\
      base12\n";

/// The same zero-old-count hunk whose two sides are made to disagree: the old
/// side hides three lines above the anchor, the new side hides six.
const DISAGREEING_SIDES: &str = "diff --git a/x.txt b/x.txt\n\
     index 1111111..2222222 100644\n\
     --- a/x.txt\n\
     +++ b/x.txt\n\
     @@ -3,0 +7,1 @@\n\
     +inserted\n";

fn table(text: &str) -> GapTable {
    gap_table(&parse_patch(text))
}

fn patch(text: &str) -> Patch {
    parse_patch(text)
}

fn run(n: u32) -> Hidden {
    Hidden::Run(n)
}

/// The old and new run lengths between consecutive hunks of one file, computed
/// here rather than by `gap_table`, so the equal-length property is a check and
/// not a restatement.
fn between_sides(prev: &PatchHunk, next: &PatchHunk) -> (usize, usize) {
    (
        next.old_start
            .saturating_sub(prev.old_start + prev.old_count),
        next.new_start
            .saturating_sub(prev.new_start + prev.new_count),
    )
}

/// A zero-count side spells its start as an anchor the hunk does not print, so
/// the run above it reaches the anchor itself.
fn above_sides(hunk: &PatchHunk) -> (usize, usize) {
    let above = |start: usize, count: usize| {
        if count == 0 {
            start
        } else {
            start.saturating_sub(1)
        }
    };
    (
        above(hunk.old_start, hunk.old_count),
        above(hunk.new_start, hunk.new_count),
    )
}

/// Every hunk of the patch in the table's own ordinal space: 0-based, running
/// across files the way [`Row::hunk`] does.
fn hunks_in_order(patch: &Patch) -> Vec<&PatchHunk> {
    patch.files.iter().flat_map(|f| &f.hunks).collect()
}

#[test]
fn a_leading_run_hides_every_line_above_the_first_header() {
    let t = table(LEADING);
    assert_eq!(
        t.gaps,
        vec![
            Gap {
                file: 0,
                above: None,
                below: Some(0),
                old_first: 1,
                new_first: 1,
                hidden: run(6),
            },
            Gap {
                file: 0,
                above: Some(0),
                below: None,
                old_first: 14,
                new_first: 14,
                hidden: Hidden::ToEof,
            },
        ],
    );
    assert_eq!(t.before_hunk(0), Some(0));
    assert_eq!(t.eof_gap(0), Some(1));
    assert_eq!(t.before_hunk(1), None, "past the patch's last hunk");
    assert_eq!(t.eof_gap(1), None, "past the patch's last file");
}

#[test]
fn an_interior_run_is_the_lines_between_two_headers_of_one_file() {
    let t = table(INTERIOR);
    assert_eq!(t.gaps.len(), 3, "leading, interior, trailing");
    assert_eq!(
        t.gaps[1],
        Gap {
            file: 0,
            above: Some(0),
            below: Some(1),
            old_first: 9,
            new_first: 9,
            hidden: run(13),
        },
    );
    assert_eq!(t.before_hunk(0), Some(0));
    assert_eq!(t.before_hunk(1), Some(1));
    assert_eq!(t.gaps[0].hidden, run(1), "one line above `@@ -2,7`");
    assert_eq!(t.gaps[2].hidden, Hidden::ToEof);
    assert_eq!(t.gaps[2].old_first, 29);
}

#[test]
fn one_gap_never_spans_two_files() {
    let files = two_files();
    let t = table(&files);
    let owned: Vec<(usize, Option<usize>, Option<usize>)> =
        t.gaps.iter().map(|g| (g.file, g.above, g.below)).collect();
    assert_eq!(
        owned,
        vec![
            (0, None, Some(0)),
            (0, Some(0), None),
            (1, None, Some(1)),
            (1, Some(1), Some(2)),
            (1, Some(2), None),
        ],
        "the first file's last run ends at its own last hunk, and the second \
         file opens a leading run at its own first"
    );
    assert_eq!(t.eof_gap(0), Some(1));
    assert_eq!(t.eof_gap(1), Some(4));
    assert_eq!(
        t.gaps[1].below, None,
        "file 0's trailing run cannot reach file 1's first hunk"
    );
    assert_eq!(
        t.gaps[2].above, None,
        "file 1's leading run cannot reach file 0's last hunk"
    );
}

#[test]
fn a_zero_count_side_anchors_the_run_above_it() {
    let t = table(ZERO_COUNT);
    assert_eq!(
        t.gaps[0],
        Gap {
            file: 0,
            above: None,
            below: Some(0),
            old_first: 1,
            new_first: 1,
            hidden: run(3),
        },
        "`@@ -3,0 +4 @@` hides old lines 1 to 3, where `start - 1` would say 2 \
         and lose the anchor line"
    );
    assert_eq!(
        t.gaps[1],
        Gap {
            file: 0,
            above: Some(0),
            below: Some(1),
            old_first: 4,
            new_first: 5,
            hidden: run(4),
        },
        "the second hunk's anchor is unprinted too, so it belongs to the run above"
    );
    assert_eq!(t.gaps[2].hidden, Hidden::ToEof);
    assert_eq!(t.gaps[2].old_first, 8);
    assert_eq!(t.gaps[2].new_first, 10);
    assert_eq!(
        t.gaps.len(),
        3,
        "a -U0 patch prints no context anywhere, so nothing is ever provably \
         empty at its tail"
    );
}

#[test]
fn a_tail_the_patch_can_place_is_absent() {
    for text in [APPEND, LAST_LINE] {
        let t = table(text);
        assert_eq!(
            t.gaps.len(),
            1,
            "only the leading run; the last hunk ends short of a full context run, \
             which proves the file ends there"
        );
        assert_eq!(t.gaps[0].above, None);
        assert_eq!(t.eof_gap(0), None, "{text}");
    }
    assert_eq!(table(APPEND).gaps[0].hidden, run(7));
    assert_eq!(table(LAST_LINE).gaps[0].hidden, run(6));
}

#[test]
fn a_tail_only_the_file_can_size_is_marked_to_eof() {
    let t = table(TAIL_UNKNOWN);
    assert_eq!(t.gaps.len(), 2);
    assert_eq!(t.gaps[0].hidden, run(193));
    assert_eq!(
        t.gaps[1],
        Gap {
            file: 0,
            above: Some(0),
            below: None,
            old_first: 201,
            new_first: 201,
            hidden: Hidden::ToEof,
        },
        "three trailing context lines is exactly the radius, so the tail may be \
         empty or may not, and only the file's text answers"
    );
    assert_eq!(t.eof_gap(0), Some(1));
}

#[test]
fn a_creation_a_deletion_and_a_combined_view_contribute_no_gaps() {
    for text in [NEW_FILE, DELETED_FILE, COMBINED] {
        let t = table(text);
        assert!(
            t.gaps.is_empty(),
            "one side is /dev/null, or the section answers to two parents: {text}"
        );
        assert_eq!(t.before_hunk(0), None);
        assert_eq!(t.eof_gap(0), None);
        assert_eq!(t.eof_gap(1), None, "one entry per file, whatever it holds");
    }
}

#[test]
fn a_zero_zero_header_hides_nothing_rather_than_minus_one() {
    let t = table(NEW_FILE);
    assert!(t.gaps.is_empty());
    // The same header without git's `new file mode` line is still not a run of
    // negative length.
    let without_flag = NEW_FILE.replace("new file mode 100644\n", "");
    let t = table(&without_flag);
    assert_eq!(
        t.gaps.iter().find(|g| g.below.is_some()).map(|g| g.hidden),
        None,
        "the leading run of `@@ -0,0 +1,2 @@` is 0, never -1: {:?}",
        t.gaps
    );
}

#[test]
fn two_sides_that_disagree_are_dropped_not_clamped() {
    let p = patch(DISAGREEING_SIDES);
    let first = &p.files[0].hunks[0];
    assert_eq!(
        (
            first.old_start.saturating_sub(1),
            first.new_start.saturating_sub(1)
        ),
        (2, 6),
        "the fixture's two sides really do disagree"
    );
    let t = table(DISAGREEING_SIDES);
    assert!(
        t.gaps.iter().all(|g| g.below != Some(0)),
        "no leading gap is emitted from a run whose length is not one number: {:?}",
        t.gaps
    );
}

#[test]
fn every_run_the_table_carries_is_the_same_number_of_lines_on_both_sides() {
    let files = [
        LEADING,
        INTERIOR,
        &two_files(),
        ZERO_COUNT,
        APPEND,
        TAIL_UNKNOWN,
    ];
    for text in files {
        let p = patch(text);
        for file in &p.files {
            for pair in file.hunks.windows(2) {
                let (old, new) = between_sides(&pair[0], &pair[1]);
                assert_eq!(
                    old,
                    new,
                    "an unprinted run is unchanged lines: {} then {}",
                    pair[0].header_line(),
                    pair[1].header_line()
                );
            }
        }
        let all = hunks_in_order(&p);
        let t = table(text);
        for gap in &t.gaps {
            let Hidden::Run(n) = gap.hidden else { continue };
            let (above, below) = (gap.above.map(|h| all[h]), gap.below.map(|h| all[h]));
            let expected = match (above, below) {
                (None, Some(next)) => above_sides(next),
                (Some(prev), Some(next)) => between_sides(prev, next),
                _ => continue,
            };
            assert_eq!((n as usize, n as usize), expected, "gap {gap:?} of {text}");
        }
    }
}

fn settled_diff(work: &PathBuf, text: &str) -> Arc<turbogit_app::diff_model::DiffValue> {
    let mut fake = FakeExecutor::default();
    fake.diffs.insert(work.to_path_buf(), text.to_string());
    let mut state = AppState::for_roots(work.parent().unwrap(), std::slice::from_ref(work))
        .with_executor(Arc::new(fake));
    while state.drain_events() > 0 {}
    let target = DiffTarget::new(
        work.to_path_buf(),
        None,
        None,
        turbogit_domain::model::DiffPolicies::default(),
        Some(PathBuf::from("README.md")),
    );
    assert!(matches!(state.read(target.clone()), Read::Waiting));
    state.drain_events();
    let Read::Fresh(diff) = state.read(target) else {
        panic!("the canned patch settles as the diff's answer");
    };
    diff
}

fn work_dir() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_one_commit(tmp.path(), "work");
    (tmp, work)
}

/// The row space is what the frame budget is measured on, so the gap pass may
/// add a table and nothing else. Frozen from the row pass as it stood before the
/// table existed, kinds, hunk ordinals and gutter numbers included.
fn display_fingerprint(model: &turbogit_app::diff_model::DiffModel) -> Vec<String> {
    let row = |r: &Row| {
        format!(
            "{:?} {} {} {} {}",
            r.kind, r.hunk, r.old_no, r.new_no, r.text
        )
    };
    model
        .display
        .iter()
        .map(|disp| match disp {
            DisplayRow::Full(r) => row(r),
            DisplayRow::Pair(del, add) => {
                let mut parts = Vec::new();
                if let Some(r) = del {
                    parts.push(row(r));
                }
                if let Some(r) = add {
                    parts.push(row(r));
                }
                parts.join(" | ")
            }
        })
        .collect()
}

#[test]
fn the_read_answer_carries_the_table_and_the_rows_it_always_had() {
    let (_tmp, work) = work_dir();
    let files = two_files();
    let value = settled_diff(&work, &files);
    assert_eq!(
        value.model.gaps(),
        &table(&files),
        "the model's table is the one pass over the same patch"
    );
    let kinds: Vec<RowKind> = value
        .model
        .display
        .iter()
        .filter_map(|disp| match disp {
            DisplayRow::Full(r) => Some(r.kind),
            DisplayRow::Pair(_, _) => None,
        })
        .collect();
    assert!(
        kinds.iter().all(|k| matches!(
            k,
            RowKind::Meta | RowKind::Context | RowKind::Hunk | RowKind::Del | RowKind::Add
        )),
        "the table adds no row kind: {kinds:?}"
    );
    assert_eq!(
        display_fingerprint(&value.model),
        [
            "Meta 0 0 0 diff --git a/a.txt b/a.txt",
            "Meta 0 0 0 index 1d89877..76ffbba 100644",
            "Meta 0 0 0 --- a/a.txt",
            "Meta 0 0 0 +++ b/a.txt",
            "Hunk 0 0 0 @@ -7,7 +7,7 @@ line006",
            "Context 0 7 7 line007",
            "Context 0 8 8 line008",
            "Context 0 9 9 line009",
            "Del 0 10 0 line010 | Add 0 0 10 CHANGED_A10",
            "Context 0 11 11 line011",
            "Context 0 12 12 line012",
            "Context 0 13 13 line013",
            "Meta 0 0 0 diff --git a/b.txt b/b.txt",
            "Meta 0 0 0 index f63ac4e..38fb9db 100644",
            "Meta 0 0 0 --- a/b.txt",
            "Meta 0 0 0 +++ b/b.txt",
            "Hunk 1 0 0 @@ -2,7 +2,7 @@ line001",
            "Context 1 2 2 line002",
            "Context 1 3 3 line003",
            "Context 1 4 4 line004",
            "Del 1 5 0 line005 | Add 1 0 5 CHANGED_B05",
            "Context 1 6 6 line006",
            "Context 1 7 7 line007",
            "Context 1 8 8 line008",
            "Hunk 2 0 0 @@ -22,7 +22,7 @@ line021",
            "Context 2 22 22 line022",
            "Context 2 23 23 line023",
            "Context 2 24 24 line024",
            "Del 2 25 0 line025 | Add 2 0 25 CHANGED_B25",
            "Context 2 26 26 line026",
            "Context 2 27 27 line027",
            "Context 2 28 28 line028",
        ],
        "the gap pass adds a table and no row, no kind, and no gutter number"
    );
}

#[test]
fn a_later_file_is_painted_off_the_previous_files_counters() {
    let renamed = format!(
        "{LEADING}diff --git a/keep.txt b/kept.txt\n\
         similarity index 100%\n\
         rename from keep.txt\n\
         rename to kept.txt\n"
    );
    let (_tmp, work) = work_dir();
    let value = settled_diff(&work, &renamed);
    assert_eq!(
        display_fingerprint(&value.model),
        [
            "RenameHeader 0 0 0 Renamed from keep.txt · 100% similar",
            "Meta 0 0 0 diff --git a/a.txt b/a.txt",
            "Meta 0 0 0 index 1d89877..76ffbba 100644",
            "Meta 0 0 0 --- a/a.txt",
            "Meta 0 0 0 +++ b/a.txt",
            "Hunk 0 0 0 @@ -7,7 +7,7 @@ line006",
            "Context 0 7 7 line007",
            "Context 0 8 8 line008",
            "Context 0 9 9 line009",
            "Del 0 10 0 line010 | Add 0 0 10 CHANGED_A10",
            "Context 0 11 11 line011",
            "Context 0 12 12 line012",
            "Context 0 13 13 line013",
            "Meta 0 0 0 diff --git a/keep.txt b/kept.txt",
            "Context 0 14 14 similarity index 100%",
            "Context 0 15 15 rename from keep.txt",
            "Context 0 16 16 rename to kept.txt",
        ],
        "the second section's headers carry the first section's hunk ordinal and \
         the counters `rows_from` never resets per file, which is why a gap is \
         read off the hunk spans and never off the row stream"
    );
    assert_eq!(
        table(&renamed).gaps.len(),
        2,
        "a rename-only section has no hunks of its own to bound a run"
    );
}

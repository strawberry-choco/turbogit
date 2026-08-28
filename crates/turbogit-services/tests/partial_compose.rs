//! Lane A — pure patch composition for partial staging (ADR-0013).
//!
//! Every expected value below is an independent hand-written literal; nothing
//! is computed by mirroring the code under test.

use turbogit_services::partial::{HunkSelection, Selection, compose_patch};

// --- fixtures (hand-written unified-diff text) -------------------------------

const FILE_META: &str = r#"diff --git a/src/app.rs b/src/app.rs
index 3e7a9c1..b2d41f7 100644
--- a/src/app.rs
+++ b/src/app.rs
"#;

const HUNK_0: &str = r#"@@ -1,6 +1,7 @@
 fn main() {
-    println!("alpha");
+    println!("ALPHA");
     let x = 1;
     let y = 2;
     let z = 3;
+    let w = 4;
 }
"#;

const HUNK_1: &str = r#"@@ -12,5 +13,5 @@ fn helper() {
     let a = calc_a();
-    let b = calc_b();
     let c = calc_c();
+    let d = calc_d();
     let e = calc_e();
 }
"#;

const HUNK_2: &str = r#"@@ -30,3 +31,3 @@ fn tail() {
     cleanup();
-    shutdown(false);
+    shutdown(true);
     log_exit();
 }
"#;

fn three_hunk_diff() -> String {
    format!("{FILE_META}{HUNK_0}{HUNK_1}{HUNK_2}")
}

fn whole_hunk(idx: usize) -> Selection {
    Selection {
        hunks: [(idx, HunkSelection::Whole)].into_iter().collect(),
    }
}

fn selected_lines(idx: usize, positions: &[usize]) -> Selection {
    Selection {
        hunks: [(
            idx,
            HunkSelection::Lines(positions.iter().copied().collect()),
        )]
        .into_iter()
        .collect(),
    }
}

// --- slice 1: whole-hunk selection -------------------------------------------

#[test]
fn whole_hunk_selection_keeps_meta_and_that_hunk_only() {
    let patch = compose_patch(&three_hunk_diff(), &whole_hunk(0));
    assert_eq!(patch, format!("{FILE_META}{HUNK_0}"));
}

// --- slice 2: mixed line selection within a hunk ------------------------------

const NO_NEWLINE_DIFF: &str = r#"diff --git a/notes.txt b/notes.txt
index 1111111..2222222 100644
--- a/notes.txt
+++ b/notes.txt
@@ -1,3 +1,3 @@
 first
-second
\ No newline at end of file
+second edited
\ No newline at end of file
 third
"#;

const NO_NEWLINE_KEEP_DEL: &str = r#"diff --git a/notes.txt b/notes.txt
index 1111111..2222222 100644
--- a/notes.txt
+++ b/notes.txt
@@ -1,3 +1,3 @@
 first
-second
\ No newline at end of file
 third
"#;

#[test]
fn line_selection_splices_unselected_lines_and_their_markers() {
    // Changed-line position 0 is `-second`; the added line is dropped and
    // each `\ No newline` marker must follow its own changed line.
    let patch = compose_patch(NO_NEWLINE_DIFF, &selected_lines(0, &[0]));
    assert_eq!(patch, NO_NEWLINE_KEEP_DEL);
}

const MIXED_DEL_DROP_DIFF: &str = r#"diff --git a/todo.txt b/todo.txt
index aaaaaaa..bbbbbbb 100644
--- a/todo.txt
+++ b/todo.txt
@@ -1,5 +1,4 @@
 alpha
-bravo
-charlie
+charlie2
 delta
 echo
"#;

#[test]
fn unselected_deletions_demote_to_context() {
    // Changed lines: 0=`-bravo`, 1=`-charlie`, 2=`+charlie2`. Keeping only
    // the charlie pair means bravo survives unchanged on both sides, so it
    // must be emitted as a context line — removing it outright would leave
    // the kept `-charlie` misaligned and the patch unappliable.
    let patch = compose_patch(MIXED_DEL_DROP_DIFF, &selected_lines(0, &[1, 2]));
    let expected = r#"diff --git a/todo.txt b/todo.txt
index aaaaaaa..bbbbbbb 100644
--- a/todo.txt
+++ b/todo.txt
@@ -1,5 +1,4 @@
 alpha
 bravo
-charlie
+charlie2
 delta
 echo
"#;
    assert_eq!(patch, expected);
}

// --- slice 3: all lines selected == original hunk text ------------------------

#[test]
fn selecting_every_changed_line_reproduces_original_hunk_text() {
    // Hunk 2 has exactly two changed lines (one `-`, one `+`).
    let patch = compose_patch(&three_hunk_diff(), &selected_lines(2, &[0, 1]));
    assert_eq!(patch, format!("{FILE_META}{HUNK_2}"));
}

// --- slice 4: empty selection -------------------------------------------------

#[test]
fn empty_selection_yields_empty_patch() {
    let patch = compose_patch(&three_hunk_diff(), &Selection::default());
    assert_eq!(patch, "");
}

// --- slice 5: multiple hunks with unselected ones (incl. trailing) ------------

fn whole_hunks(indices: &[usize]) -> Selection {
    Selection {
        hunks: indices.iter().map(|&i| (i, HunkSelection::Whole)).collect(),
    }
}

#[test]
fn unselected_hunks_are_dropped_including_trailing() {
    // Gap in the middle: hunks 0 and 2 selected, hunk 1 spliced out.
    let patch = compose_patch(&three_hunk_diff(), &whole_hunks(&[0, 2]));
    assert_eq!(patch, format!("{FILE_META}{HUNK_0}{HUNK_2}"));

    // Trailing hunk dropped: hunks 0 and 1 selected, hunk 2 gone.
    let patch = compose_patch(&three_hunk_diff(), &whole_hunks(&[0, 1]));
    assert_eq!(patch, format!("{FILE_META}{HUNK_0}{HUNK_1}"));
}

use turbogit_services::partial::{HunkSelection, Selection, compose};

fn compose_patch(text: &str, selection: &Selection) -> String {
    compose(
        &turbogit_engine_api::patch::parse_patch(text),
        selection,
        turbogit_engine_api::ApplyDirection::Forward,
    )
    .to_string()
}

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

const HUNK_2: &str = "@@ -30,4 +31,4 @@ fn tail() {\n     cleanup();\n-    shutdown(false);\n+    shutdown(true);\n     log_exit();\n }\n";

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

#[test]
fn whole_hunk_selection_keeps_meta_and_that_hunk_only() {
    let patch = compose_patch(&three_hunk_diff(), &whole_hunk(0));
    assert_eq!(patch, format!("{FILE_META}{HUNK_0}"));
}

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

const NO_NEWLINE_KEEP_DEL: &str = "diff --git a/notes.txt b/notes.txt\nindex 1111111..2222222 100644\n--- a/notes.txt\n+++ b/notes.txt\n@@ -1,3 +1,2 @@\n first\n-second\n\\ No newline at end of file\n third\n";

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
    let patch = compose_patch(MIXED_DEL_DROP_DIFF, &selected_lines(0, &[1, 2]));
    let expected = "diff --git a/todo.txt b/todo.txt\nindex aaaaaaa..bbbbbbb 100644\n--- a/todo.txt\n+++ b/todo.txt\n@@ -1,5 +1,5 @@\n alpha\n bravo\n-charlie\n+charlie2\n delta\n echo\n";
    assert_eq!(patch, expected);
}

#[test]
fn selecting_every_changed_line_reproduces_original_hunk_text() {
    let patch = compose_patch(&three_hunk_diff(), &selected_lines(2, &[0, 1]));
    assert_eq!(patch, format!("{FILE_META}{HUNK_2}"));
}

#[test]
fn empty_selection_yields_empty_patch() {
    let patch = compose_patch(&three_hunk_diff(), &Selection::default());
    assert_eq!(patch, "");
}

fn whole_hunks(indices: &[usize]) -> Selection {
    Selection {
        hunks: indices.iter().map(|&i| (i, HunkSelection::Whole)).collect(),
    }
}

#[test]
fn unselected_hunks_are_dropped_including_trailing() {
    let patch = compose_patch(&three_hunk_diff(), &whole_hunks(&[0, 2]));
    assert_eq!(patch, format!("{FILE_META}{HUNK_0}{HUNK_2}"));

    let patch = compose_patch(&three_hunk_diff(), &whole_hunks(&[0, 1]));
    assert_eq!(patch, format!("{FILE_META}{HUNK_0}{HUNK_1}"));
}

/// A multi-change hunk where one change is dropped on the new side: the
/// original header's new-side count no longer matches the body, so libgit2
/// (`git_apply` without `--recount`) refuses the patch. Compose must
/// recount the new-count so the body and header agree.
const RECOUNT_DIFF: &str = "diff --git a/code.rs b/code.rs\nindex 2d9efe4..0a173d3 100644\n--- a/code.rs\n+++ b/code.rs\n@@ -1,5 +1,5 @@\n fn main() {\n-    let alpha = 1;\n+    let ALPHA = 1;\n     let beta = 2;\n-    let gamma = 3;\n+    let GAMMA = 3;\n }\n";

const RECOUNT_KEEP_FIRST_DEL: &str = "diff --git a/code.rs b/code.rs\nindex 2d9efe4..0a173d3 100644\n--- a/code.rs\n+++ b/code.rs\n@@ -1,5 +1,4 @@\n fn main() {\n-    let alpha = 1;\n     let beta = 2;\n     let gamma = 3;\n }\n";

#[test]
fn dropped_addition_recounts_the_new_side_of_the_header() {
    let patch = compose_patch(RECOUNT_DIFF, &selected_lines(0, &[0]));
    assert_eq!(patch, RECOUNT_KEEP_FIRST_DEL);
}

//! The `Patch File Settings` transforms: what a created patch's paths say, how many
//! leading directories can be taken off them, what a comment turns into, and which
//! bytes reach the file.
//!
//! These patches are hand-written in git's own syntax rather than produced by
//! `git diff`, because the assertion is about the *transform* — a path moved, a
//! segment dropped, a content line left alone — and an expected value has to come
//! from somewhere other than the code that computes it. The suites in
//! `crates/turbogit-engine/tests/` pin the other half: that git really answers
//! `patch_bytes` with these shapes.

use std::path::{Path, PathBuf};
use turbogit_domain::model::PatchEncoding;
use turbogit_services::patch_create::{
    self, clipboard_text, encode, file_name, removable_leading_directories, rewrite_paths,
};

/// A patch with paths deep enough that three directories can come off every one of
/// them — the shape the counting repair pair is for.
const DEEP: &str = "diff --git a/src/main/java/App.java b/src/main/java/App.java
--- a/src/main/java/App.java
+++ b/src/main/java/App.java
@@ -1,2 +1,2 @@
-one
+two
 context
";

/// The same patch with one shallow path beside the deep ones: a removal that would
/// erase `README.md`'s name is not available to either.
const MIXED: &str = "diff --git a/README.md b/README.md
--- a/README.md
+++ b/README.md
@@ -1 +1 @@
-old
+new
diff --git a/src/main/java/App.java b/src/main/java/App.java
--- a/src/main/java/App.java
+++ b/src/main/java/App.java
@@ -1,2 +1,2 @@
-one
+two
";

fn root(parent: &Path) -> PathBuf {
    parent.join("project")
}

/// Three removals leave one directory, and two restores give both back: the count is
/// the state, and each step means exactly one segment on every path.
#[test]
fn removal_counts_segments_on_every_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(tmp.path());
    let bytes = DEEP.as_bytes();
    let removed = |n: usize| {
        String::from_utf8(rewrite_paths(bytes, &root, "", n)).expect("this fixture is text")
    };

    assert_eq!(removable_leading_directories(bytes), 3, "three directories");
    assert_eq!(
        removed(1),
        DEEP.replace("src/main/java/App.java", "main/java/App.java"),
        "one removal takes one segment off every path"
    );
    assert_eq!(
        removed(3),
        DEEP.replace("src/main/java/App.java", "App.java"),
        "the bound is reachable, and the file's own name survives it"
    );
    assert_eq!(removed(0), DEEP, "no removal is git's own answer");
    // The restore half of the pair: two back off three is one removed.
    assert_eq!(removed(3 - 2), removed(1), "restore is the inverse press");
}

/// The bound is the **shallowest** path, because one press rewrites every section: a
/// count that erased a file name would be a broken patch, not a shorter path.
#[test]
fn the_removal_bound_is_the_shallowest_path_in_the_patch() {
    assert_eq!(
        removable_leading_directories(MIXED.as_bytes()),
        0,
        "README.md can lose nothing, so the whole patch can lose nothing"
    );
}

/// Base path is the directory the patch's paths are read from. The default — the
/// project itself — leaves git's repo-relative answer alone; a base above the
/// project makes every path carry the project's name; a base inside it strips that
/// prefix. Those three are the field's whole grammar, and the reference's own
/// wording ("normally, this is your project directory") is why the first is default.
#[test]
fn the_base_path_moves_which_directory_the_paths_are_read_from() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(tmp.path());
    let rewrite = |base: &str| {
        String::from_utf8(rewrite_paths(DEEP.as_bytes(), &root, base, 0)).expect("text")
    };

    assert_eq!(rewrite(""), DEEP, "an empty base is the project directory");
    assert_eq!(
        rewrite("."),
        DEEP,
        "and so is the project spelled as a step"
    );
    assert_eq!(
        rewrite(".."),
        DEEP.replace("src/main/java/App.java", "project/src/main/java/App.java"),
        "a base above the project writes the project's own name into every path"
    );
    assert_eq!(
        rewrite("../elsewhere"),
        DEEP.replace(
            "src/main/java/App.java",
            "../project/src/main/java/App.java"
        ),
        "a base beside the project steps over it rather than pretending to contain it"
    );
    assert_eq!(
        rewrite("src/main"),
        DEEP.replace("src/main/java/App.java", "java/App.java"),
        "a base inside the project strips its own prefix"
    );
    assert_eq!(
        rewrite("unrelated"),
        DEEP.replace("src/main/java/App.java", "../src/main/java/App.java"),
        "a path the file is not under says so with a step rather than pretending"
    );
}

/// Base and removal compose: the count is of the directories the patch now writes,
/// which is what makes the two fields one dialog rather than two competing edits.
#[test]
fn a_base_path_and_a_removal_compose() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(tmp.path());
    let rewritten = String::from_utf8(rewrite_paths(DEEP.as_bytes(), &root, "..", 1))
        .expect("this fixture is text");
    assert_eq!(
        rewritten,
        DEEP.replace("src/main/java/App.java", "project/src/main/java/App.java")
            .replace("project/src/main/java/App.java", "src/main/java/App.java"),
        "the project's name is the first directory, so removing one is removing it"
    );
}

/// Rename and copy lines name the same paths as the section header, and a patch
/// whose four spellings disagree is a patch that renames to somewhere else.
#[test]
fn every_line_naming_a_path_is_rewritten() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(tmp.path());
    let patch = "diff --git a/old/dir/name.txt b/new/dir/name.txt
similarity index 92%
rename from old/dir/name.txt
rename to new/dir/name.txt
index 1234567..89abcde 100644
--- a/old/dir/name.txt
+++ b/new/dir/name.txt
@@ -1 +1 @@
-one
+two
";
    let rewritten = String::from_utf8(rewrite_paths(patch.as_bytes(), &root, "", 1))
        .expect("this fixture is text");
    assert_eq!(
        rewritten,
        "diff --git a/dir/name.txt b/dir/name.txt
similarity index 92%
rename from dir/name.txt
rename to dir/name.txt
index 1234567..89abcde 100644
--- a/dir/name.txt
+++ b/dir/name.txt
@@ -1 +1 @@
-one
+two
",
        "both sides, both rename lines, and the index line that is not a path"
    );
}

/// A create and a delete name one side as `/dev/null`. That is not a path to move: a
/// rewrite that turned it into a relative path would make the patch create a file
/// called `dev/null`.
#[test]
fn the_no_side_of_a_create_or_delete_is_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(tmp.path());
    for patch in [
        "--- /dev/null\n+++ b/deep/dir/new.txt\n",
        "--- a/deep/dir/gone.txt\n+++ /dev/null\n",
    ] {
        let rewritten =
            String::from_utf8(rewrite_paths(patch.as_bytes(), &root, "", 1)).expect("text");
        assert!(
            rewritten.contains("/dev/null"),
            "the null side survives: {rewritten}"
        );
    }
}

/// A path with a space is quoted on a `diff --git` line, because two tokens are
/// parsed there; the rewrite keeps the quoting rather than emitting a line that
/// splits into three paths.
#[test]
fn a_path_with_a_space_stays_parsable() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(tmp.path());
    let patch = "diff --git \"a/two words/it is.txt\" \"b/two words/it is.txt\"\n--- a/two words/it is.txt\n+++ b/two words/it is.txt\n";
    let rewritten = String::from_utf8(rewrite_paths(patch.as_bytes(), &root, "", 1)).expect("text");
    assert_eq!(
        rewritten, "diff --git \"a/it is.txt\" \"b/it is.txt\"\n--- a/it is.txt\n+++ b/it is.txt\n",
        "quoted where two tokens are read, plain where the line is one path"
    );
}

/// The body is not the header. A patch's content lines are its file's bytes, which
/// git does not promise are text: the rewrite must copy them through unchanged, or
/// the artifact differs from the repository it came from.
#[test]
fn a_content_line_is_copied_byte_for_byte() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(tmp.path());
    let mut bytes: Vec<u8> = b"diff --git a/deep/dir/x.txt b/deep/dir/x.txt\n".to_vec();
    bytes.extend_from_slice(b"--- a/deep/dir/x.txt\n+++ b/deep/dir/x.txt\n@@ -1 +1 @@\n-");
    bytes.extend_from_slice(&[0xff, 0xfe, 0x00, 0x80]);
    bytes.extend_from_slice(b"\n+plain\n");

    let out = rewrite_paths(&bytes, &root, "", 1);
    assert_eq!(
        &out[out.len() - "+plain\n".len()..],
        b"+plain\n",
        "the tail survives"
    );
    assert!(
        out.windows(4).any(|w| w == [0xff, 0xfe, 0x00, 0x80]),
        "the non-UTF-8 content bytes are still there, byte for byte"
    );
    let head = String::from_utf8_lossy(&out[..60]).into_owned();
    assert!(
        head.contains("a/dir/x.txt"),
        "while the header did get rewritten: {head}"
    );
}

/// The comment is the file name — and a comment is free text while a name is a path,
/// so every separator, parent step and empty comment has to be resolved here rather
/// than handed to the filesystem.
#[test]
fn a_comment_becomes_one_sane_file_name() {
    assert_eq!(
        file_name("Fix the login form"),
        "Fix_the_login_form.patch",
        "the comment's own words, and the extension the artifact has"
    );
    assert_eq!(
        file_name("patch already named"),
        "patch_already_named.patch"
    );
    for comment in ["../../etc/passwd", "a/b\\c:d", "   ", "", "\nsecond line\n"] {
        let name = file_name(comment);
        assert!(
            !name.contains('/') && !name.contains('\\') && !name.contains(".."),
            "a comment cannot select a directory or escape one: {name}"
        );
        assert!(
            name.ends_with(".patch") && !name.starts_with('.'),
            "every name is a patch file and none is hidden: {name}"
        );
        assert!(!name.is_empty(), "and a name is always something: {name:?}");
    }
    assert_eq!(
        file_name(""),
        "patch.patch",
        "no comment names the artifact"
    );
    assert_eq!(
        file_name("Fix the login form\nwith a long body that goes on"),
        "Fix_the_login_form.patch",
        "the first line is the comment's name and the body is not part of it"
    );
    assert_eq!(
        file_name(&"d".repeat(200)).len(),
        48 + ".patch".len(),
        "a paragraph does not become an endless file name"
    );
}

/// Encoding, stated as the bytes on disk. The default writes git's own output and
/// cannot corrupt it; the other two decode first and refuse rather than guess.
#[test]
fn the_encoding_field_is_a_statement_about_bytes() {
    let utf8 = "© one\n".as_bytes();

    assert_eq!(
        encode(&[0xff, 0xfe], PatchEncoding::Utf8).expect("verbatim"),
        vec![0xff, 0xfe],
        "UTF-8 is not a re-encode — which is the reason a patch travels as bytes"
    );
    assert_eq!(
        encode(utf8, PatchEncoding::Utf8WithBom).expect("bom"),
        [vec![0xEF, 0xBB, 0xBF], utf8.to_vec()].concat()
    );
    assert_eq!(
        encode("café\n".as_bytes(), PatchEncoding::Latin1).expect("latin1"),
        vec![b'c', b'a', b'f', 0xE9, b'\n'],
        "one byte per code point where ISO-8859-1 has one"
    );

    let outside_latin1 = "€ one\n";
    let err = encode(outside_latin1.as_bytes(), PatchEncoding::Latin1)
        .expect_err("€ has no Latin-1 byte");
    assert!(
        err.to_string().contains("ISO-8859-1"),
        "the refusal names the encoding that asked for it: {err}"
    );
    let err = encode(&[0xff, 0xfe], PatchEncoding::Utf8WithBom).expect_err("not UTF-8");
    assert!(
        err.to_string().contains("UTF-8"),
        "and says what cannot be honoured: {err}"
    );
}

/// The clipboard route is the one place a created patch is decoded, so it is the one
/// place a non-UTF-8 patch legitimately stops — with a reason, rather than with a
/// silent loss that would make the clipboard differ from the file.
#[test]
fn the_clipboard_route_decodes_or_refuses() {
    assert_eq!(
        clipboard_text("© one\n".as_bytes()).expect("valid utf-8"),
        "© one\n"
    );
    let err = clipboard_text(&[0xff, 0xfe]).expect_err("not valid utf-8");
    assert!(
        err.to_string().contains("clipboard"),
        "the refusal says which route it stops: {err}"
    );
}

/// A patch with no path header has nothing to remove — the state a surface reads as
/// "disable the button", not as an error.
#[test]
fn a_patch_naming_no_path_removes_nothing() {
    assert_eq!(removable_leading_directories(b""), 0);
    assert_eq!(removable_leading_directories(b"error: not a patch\n"), 0);
    assert_eq!(
        removable_leading_directories(b"diff --git a/x.txt b/x.txt\n"),
        0,
        "a bare name is already the floor"
    );
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(
        rewrite_paths(b"", &root(tmp.path()), "..", 5),
        Vec::new(),
        "and rewriting nothing rewrites nothing"
    );
}

/// The module's own surface is small enough that the dialog can call it by name;
/// pinning the re-export keeps the state layer from reaching into private helpers.
#[test]
fn the_transforms_are_the_public_surface() {
    assert_eq!(
        PatchEncoding::ALL.len(),
        3,
        "the field's list, its default and its transcode rule are one place"
    );
    assert_eq!(PatchEncoding::default(), PatchEncoding::Utf8);
    assert_eq!(PatchEncoding::Utf8WithBom.label(), "UTF-8 with BOM");
    let _ = patch_create::rewrite_paths as fn(&[u8], &Path, &str, usize) -> Vec<u8>;
}

/// A backslash is **not** one of this rewriter's quote triggers, so a path
/// carrying one and no space is written raw on a freshly written `diff --git`
/// line — and that raw backslash reaches `git apply` on a header line whose
/// reader treats `\` as an escape introducer.
///
/// This is a characterization pin, not an approval: quoting backslash paths is a
/// human ruling still owed, and this assertion is written to **fail** the moment
/// somebody quotes it, because that failure is where the ruling lands. In the same
/// edit, update the `HeaderPair` row of the leaf grammar's module doc — which
/// names the omission as a preserved defect — and note that the escape table's
/// backslash arm becomes reachable for the one case it exists for.
#[test]
fn a_backslash_path_is_written_unquoted_where_gits_own_spelling_would_escape_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = root(tmp.path());
    let patch = "diff --git a/deep/we\\ird.txt b/deep/we\\ird.txt\n--- a/deep/we\\ird.txt\n+++ b/deep/we\\ird.txt\n";

    let rewritten = String::from_utf8(rewrite_paths(patch.as_bytes(), &root, "", 1)).expect("text");

    assert_eq!(
        rewritten, "diff --git a/we\\ird.txt b/we\\ird.txt\n--- a/we\\ird.txt\n+++ b/we\\ird.txt\n",
        "every path was rewritten, and the backslash was written raw and unquoted"
    );
}

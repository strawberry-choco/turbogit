//! Reading a patch's **provenance** — the header a git-format patch carries ahead
//! of its first `diff --git` section.
//!
//! The engine owns git's diff syntax (`ADR-0022`), and this is the other half of
//! that ownership: a patch produced by `git format-patch` or `git show` states who
//! wrote it and what the commit said, and the apply surface fills its own fields
//! from that rather than asking the developer to retype it. The reading stops at
//! the section boundary, which is the assertion that matters most here — the patch
//! body is content, and content is free to contain a line that looks like a header.
//!
//! One case runs against **git's own output**, because a header format is a fact
//! about the producer: a suite that only reads text written in this file would be
//! asserting that this file and the service agree, not that a real patch parses.

use std::path::{Path, PathBuf};
use test_support::git_seed::{commit as git_commit, git};
use turbogit_engine_api::patch::read_provenance;

fn repo(parent: &Path) -> PathBuf {
    let dir = parent.join("app");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "ada@example.com"]);
    git(&dir, &["config", "user.name", "Ada Lovelace"]);
    git(&dir, &["config", "core.autocrlf", "false"]);
    git_commit(
        &dir,
        "note.txt",
        "one\ntwo\n",
        "raise the note's second line",
    );
    git_commit(
        &dir,
        "note.txt",
        "one\nTWO\n",
        "Notes: raise the second line",
    );
    dir
}

/// A real `git format-patch -1` file, read the way the apply window reads one.
#[test]
fn git_own_format_patch_output_names_its_author_and_subject() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    git(&dir, &["format-patch", "-1", "-o", "out"]);
    let written = std::fs::read_dir(dir.join("out"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "patch"))
        .expect("format-patch wrote a .patch file");
    let text = std::fs::read_to_string(&written).unwrap();

    let seen = read_provenance(&text).expect("a format-patch carries a header");
    assert_eq!(
        seen.author.as_deref(),
        Some("Ada Lovelace <ada@example.com>")
    );
    assert_eq!(
        seen.subject.as_deref(),
        Some("Notes: raise the second line")
    );
    assert!(seen.date.is_some(), "the header's date is the patch's own");
    let sha = seen.commit.expect("From <sha> names the commit");
    assert_eq!(sha.len(), 40, "the full reference, as git writes it: {sha}");
    assert_eq!(
        git(&dir, &["rev-parse", "HEAD"]).trim(),
        sha,
        "the recorded commit is the commit the patch was made from"
    );
}

/// The `[PATCH 2/3]` decoration is git's envelope, not the subject's own text.
#[test]
fn the_bracketed_patch_decoration_is_stripped_from_the_subject() {
    let seen = read_provenance(
        r#"From 1234567890abcdef1234567890abcdef12345678 Mon Sep 17 00:00:00 2001
From: Grace <grace@example.com>
Date: Tue, 1 Oct 2024 09:00:00 +0000
Subject: [PATCH 2/3] Fix the parsing of headers

A body line.
---
diff --git a/x.txt b/x.txt
--- a/x.txt
+++ b/x.txt
@@ -1,1 +1,1 @@
-a
+b
"#,
    )
    .expect("a format-patch header");
    assert_eq!(seen.subject.as_deref(), Some("Fix the parsing of headers"));
    assert_eq!(seen.author.as_deref(), Some("Grace <grace@example.com>"));
}

/// A subject that genuinely opens with a bracket keeps it: the decoration is
/// recognised by its shape, not by any bracket being there.
#[test]
fn a_subject_that_really_begins_with_a_bracket_keeps_it() {
    let seen = read_provenance(
        r#"From: Ada <ada@example.com>
Subject: [RFC] Try a new reader
---
diff --git a/x.txt b/x.txt
"#,
    )
    .expect("a header");
    assert_eq!(seen.subject.as_deref(), Some("[RFC] Try a new reader"));
}

/// A plain `git diff` file has no header at all, and absence is an answer.
#[test]
fn a_plain_diff_patch_carries_no_provenance() {
    assert!(
        read_provenance(
            r#"diff --git a/x.txt b/x.txt
--- a/x.txt
+++ b/x.txt
@@ -1,1 +1,1 @@
-a
+b
"#
        )
        .is_none()
    );
}

/// The one that makes the reader safe rather than merely convenient: everything
/// after the first `diff --git` is the patch's **content**, and content can name
/// anything. A header read that wandered into the body would let a patch file
/// invent an author for the developer's own commit.
#[test]
fn the_body_of_the_patch_is_never_read_as_its_header() {
    let seen = read_provenance(
        r#"diff --git a/notes.md b/notes.md
--- a/notes.md
+++ b/notes.md
@@ -1,1 +1,3 @@
+Subject: [PATCH] Whoever wrote this line did not write the patch
+From: Nobody <nobody@example.com>
+commit 0000000000000000000000000000000000000000
"#,
    );
    assert!(
        seen.is_none(),
        "a section's added lines are content, not the patch's own header: {seen:?}"
    );
}

/// The two git dump formats disagree on the author's field name — `format-patch`
/// writes `From:`, `git show` writes `Author:` — and a reader that knew only one
/// would silently discard provenance from the other.
#[test]
fn a_git_show_style_header_is_read_too() {
    let seen = read_provenance(
        r#"commit aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
Author: Barbara <barbara@example.com>
Date:   Mon Sep 30 00:00:00 2024 +0000

    The commit's own message body
---
diff --git a/x.txt b/x.txt
"#,
    )
    .expect("a git show header carries provenance");
    assert_eq!(
        seen.author.as_deref(),
        Some("Barbara <barbara@example.com>")
    );
    assert_eq!(
        seen.commit.as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
    assert!(seen.subject.is_none(), "no `Subject:` line exists here");
}

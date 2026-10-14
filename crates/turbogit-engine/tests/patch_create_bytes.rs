//! The bytes behind `Create patch`: which change each of the three sources answers
//! with, and what `Reverse patch` asks of git.
//!
//! Every assertion here is against **real git in a temporary repository**, because
//! the artifact is the thing under test: a patch that cannot be applied by git is a
//! broken artifact no matter what the application believes about it. The checks ask
//! git to apply what this read produced, or compare the bytes on disk, rather than
//! matching a text shape this file also generates.
//!
//! The byte-not-String decision is pinned by a suite of its own at the end: git's
//! output for a file whose content is not text must arrive holding those same bytes,
//! because everything upstream — the encoding field included — is a statement about
//! them.

use std::path::{Path, PathBuf};
use test_support::git_seed::{commit as git_commit, git, pin_eol_lf};
use turbogit_domain::model::{PatchOrigin, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;

fn engine() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

/// Two committed revisions with a shared name, so an origin's own change is
/// distinguishable from its ancestor's.
fn repo(parent: &Path) -> PathBuf {
    let dir = parent.join("app");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.email", "t@t"]);
    git(&dir, &["config", "user.name", "t"]);
    pin_eol_lf(&dir);
    git_commit(&dir, "base.txt", "shared\n", "base");
    git_commit(&dir, "a.txt", "one\ntwo\nthree\n", "first");
    git_commit(&dir, "a.txt", "one\nTWO\nthree\n", "second");
    dir
}

fn head(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

fn first(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD~1"]).trim().to_string()
}

/// A commit origin is THAT revision's change and none of its history: an ancestor's
/// file must not ride into the patch.
#[test]
fn a_commit_origin_carries_its_own_change_and_no_ancestor_s() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let bytes = engine()
        .patch_bytes(
            &dir,
            &PatchOrigin::Commit {
                id: head(&dir).clone(),
            },
            false,
        )
        .expect("git answers the commit's own diff");
    let text = String::from_utf8(bytes).expect("this fixture is text");
    assert!(text.contains("a.txt"), "the commit's path is named: {text}");
    assert!(text.contains("TWO"), "the change itself is named: {text}");
    assert!(
        !text.contains("base.txt"),
        "an ancestor's change must not ride along: {text}"
    );

    // git's verdict, not a text shape: at the parent, where TWO does not exist,
    // the patch must apply.
    git(&dir, &["checkout", "-q", &first(&dir)]);
    let ok = std::process::Command::new("git")
        .args(["apply", "--check", "-"])
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(text.as_bytes())
                .expect("write patch");
            child.wait_with_output()
        })
        .expect("git apply --check ran");
    assert!(ok.status.success(), "the patch applies at its parent");
}

/// A file revision is one path's change in one revision — what a row of a file
/// history is — and a file history's other paths stay out of it.
#[test]
fn a_file_revision_origin_names_one_path_only() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    git_commit(&dir, "b.txt", "beta\n", "third");
    let bytes = engine()
        .patch_bytes(
            &dir,
            &PatchOrigin::FileRevision {
                commit: first(&dir),
                path: PathBuf::from("a.txt"),
            },
            false,
        )
        .expect("git answers one path's revision");
    let text = String::from_utf8(bytes).expect("this fixture is text");
    assert!(
        text.contains("a.txt"),
        "the revision's path is named: {text}"
    );
    assert!(
        !text.contains("+beta"),
        "another path's change is not part of a file revision: {text}"
    );
    assert!(
        text.contains("TWO"),
        "the file's own change is named: {text}"
    );
}

/// Local changes are staged **and** unstaged content at once: `git diff` alone would
/// answer with one half of what the developer is looking at, and a patch missing the
/// staged half is a different artifact than the one the Commit window showed.
#[test]
fn local_changes_carry_staged_and_unstaged_content_together() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    // `a.txt` staged, `base.txt` unstaged, `new.txt` unversioned.
    std::fs::write(dir.join("a.txt"), "one\ntwo\nTHREE\n").unwrap();
    git(&dir, &["add", "a.txt"]);
    std::fs::write(dir.join("base.txt"), "shared\nadded\n").unwrap();
    std::fs::write(dir.join("new.txt"), "untracked\n").unwrap();

    let bytes = engine()
        .patch_bytes(
            &dir,
            &PatchOrigin::LocalChanges { paths: Vec::new() },
            false,
        )
        .expect("git answers the local changes");
    let text = String::from_utf8(bytes).expect("this fixture is text");
    assert!(text.contains("THREE"), "the staged half is in it: {text}");
    assert!(
        text.contains("+added"),
        "the unstaged half is in it: {text}"
    );
    assert!(
        !text.contains("untracked"),
        "a path git has never held has no diff to carry, and inventing one would \
         make a patch git cannot apply: {text}"
    );

    // Selection is the same origin with paths: a patch of one file names one file.
    let one = engine()
        .patch_bytes(
            &dir,
            &PatchOrigin::LocalChanges {
                paths: vec![PathBuf::from("base.txt")],
            },
            false,
        )
        .expect("git answers one selected path");
    let one = String::from_utf8(one).expect("this fixture is text");
    assert!(
        one.contains("base.txt"),
        "the selected path is named: {one}"
    );
    assert!(
        !one.contains("THREE"),
        "an unselected path does not join the patch: {one}"
    );
}

/// `Reverse patch` is git's `-R`, not a re-rendering: the patch it produces undoes
/// the change, which is checked by running it against the tree that has the change.
#[test]
fn the_reverse_origin_is_a_patch_git_applies_backwards() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    std::fs::write(dir.join("a.txt"), "one\nTWO\nthree\nextra\n").unwrap();

    let forward = engine()
        .patch_bytes(
            &dir,
            &PatchOrigin::LocalChanges { paths: Vec::new() },
            false,
        )
        .expect("the forward patch");
    let reverse = engine()
        .patch_bytes(&dir, &PatchOrigin::LocalChanges { paths: Vec::new() }, true)
        .expect("the reverse patch");
    assert_ne!(forward, reverse, "a reversed patch is not the same bytes");

    // The claim the checkbox makes: applying this patch reverts the change, so the
    // file is what HEAD holds afterwards. Run it through the engine's own worktree
    // apply, which is the verb `Apply patch` uses.
    let patch = turbogit_engine_api::patch::parse_patch(
        std::str::from_utf8(&reverse).expect("this fixture is text"),
    );
    engine()
        .apply_patch_to_worktree(&dir, &patch)
        .expect("git applies the reverse patch");
    assert_eq!(
        std::fs::read_to_string(dir.join("a.txt")).unwrap(),
        "one\nTWO\nthree\n",
        "the reverse patch put the file back to HEAD's content"
    );
}

/// The byte decision, stated as behaviour: a patch whose body is not text arrives
/// holding those bytes. A `String` at this seam would have replaced them already,
/// and the file the developer wrote would differ from the repository.
#[test]
fn a_patch_body_that_is_not_text_survives_the_read_byte_for_byte() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = repo(tmp.path());
    let raw: Vec<u8> = vec![0x66, 0x69, 0x6c, 0x65, 0x0a, 0xff, 0xfe, 0x0a];
    std::fs::write(dir.join("a.txt"), &raw).unwrap();

    let bytes = engine()
        .patch_bytes(
            &dir,
            &PatchOrigin::LocalChanges { paths: Vec::new() },
            false,
        )
        .expect("git answers the local change");
    assert!(
        bytes.windows(3).any(|w| w == [0xff, 0xfe, 0x0a]),
        "the non-UTF-8 bytes are in the patch as they are on disk, not replaced"
    );
    assert!(
        String::from_utf8(bytes.clone()).is_err(),
        "and the answer is still not text — which is why this read is not a String"
    );
}

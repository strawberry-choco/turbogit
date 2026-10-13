//! The conflict verbs are named entries, and each one still runs the git the
//! closure ran.
//!
//! Driven through the Headless harness against a repository carrying a real,
//! unresolved merge conflict, and asserted as **what the repository becomes**:
//! which side stage 2 carries, that the written resolution is staged, and that
//! every simple conflict is staged. The labels ride along because they are what
//! the user reads in the toast.
//!
//! The pair of "Apply merge resolution" sites is why the entry is one name: the
//! merge editor and the resolver window say the same verb with the same worker,
//! and two copies are what a change to one of them can make disagree.

use std::path::{Path, PathBuf};

use test_support::git_seed::{git, repo_with_conflict};
use turbogit_app::shell_writes::conflict as writes;
use turbogit_app::state::AppState;
use turbogit_domain::model::RootStatus;

/// A repository with a real conflict in `conf.txt`: `main` says `main`, `side`
/// says `side`, and `git merge side` was started and left in progress.
fn conflicted(project: &Path) -> PathBuf {
    repo_with_conflict(project, "work")
}

/// The same conflict plus `simple.txt` staged at stages 1, 2 and 3 with the
/// **same blob** in all three — the shape `Resolve simple conflicts` exists to
/// clear, and one git's own merge will never produce: both sides changing a file
/// to identical content auto-resolves, so the entry has to be placed with
/// `update-index` rather than waited for.
fn with_a_simple_conflict(work: &Path) {
    let body = "agreed\n";
    std::fs::write(work.join("simple.txt"), body).unwrap();
    let blob = blob_from_stdin(work, body);
    let script = format!(
        "100644 {blob} 1\tsimple.txt\n100644 {blob} 2\tsimple.txt\n100644 {blob} 3\tsimple.txt\n"
    );
    let mut child = std::process::Command::new("git")
        .arg("update-index")
        .arg("--index-info")
        .current_dir(work)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
    }
    assert!(child.wait().unwrap().success());
}

/// Write `body` into the repository's object store and return its blob hash.
fn blob_from_stdin(work: &Path, body: &str) -> String {
    use std::io::Write;
    let mut child = std::process::Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(work)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(body.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn state_for(work: &Path) -> AppState {
    let roots = [work.to_path_buf()];
    AppState::for_roots(work.parent().unwrap(), &roots)
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    git(dir, args).trim().to_string()
}

/// Stage 2 of `path` in the index — what an accepted side leaves behind.
/// Untrimmed: the assertion is the file's content, newline included.
fn stage2(work: &Path, path: &str) -> String {
    git(work, &["show", &format!(":{path}")])
}

fn unmerged(work: &Path) -> String {
    git_out(work, &["diff", "--name-only", "--diff-filter=U"])
}

fn status_of(state: &AppState) -> RootStatus {
    let id = state.selected_root.clone().unwrap();
    state.multi.by_id(&id).unwrap().status.clone()
}

#[test]
fn accept_ours_entry_stages_our_side() {
    let tmp = tempfile::tempdir().unwrap();
    let work = conflicted(tmp.path());
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = writes::accept_ours(&root, Path::new("conf.txt"));
    assert_eq!(op.label(), "Accept ours");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(stage2(&work, "conf.txt"), "main\n", "stage 2 is our side");
    assert!(unmerged(&work).is_empty(), "nothing is left unmerged");
}

#[test]
fn accept_theirs_entry_stages_their_side() {
    let tmp = tempfile::tempdir().unwrap();
    let work = conflicted(tmp.path());
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = writes::accept_theirs(&root, Path::new("conf.txt"));
    assert_eq!(op.label(), "Accept theirs");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(stage2(&work, "conf.txt"), "side\n", "stage 2 is their side");
}

#[test]
fn apply_merge_resolution_entry_writes_and_stages_the_composed_content() {
    let tmp = tempfile::tempdir().unwrap();
    let work = conflicted(tmp.path());
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();

    let op = writes::apply_merge_resolution(&root, Path::new("conf.txt"), "both\n");
    assert_eq!(op.label(), "Apply merge resolution");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        std::fs::read_to_string(work.join("conf.txt")).unwrap(),
        "both\n",
        "the composed content is what landed in the working tree"
    );
    assert_eq!(stage2(&work, "conf.txt"), "both\n", "and it is staged");
    assert!(unmerged(&work).is_empty(), "the path left the conflict set");
}

#[test]
fn resolve_simple_entry_stages_every_agreed_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let work = conflicted(tmp.path());
    with_a_simple_conflict(&work);
    let mut state = state_for(&work);
    let root = state.selected_root.clone().unwrap();
    let status = status_of(&state);
    assert!(
        status.conflicted.iter().any(|p| p.ends_with("simple.txt")),
        "the fixture carries the agreed conflict: {:?}",
        status.conflicted
    );

    let op = writes::resolve_simple(&root, &status);
    assert_eq!(op.label(), "Resolve simple conflicts");
    state.dispatch(op);
    state.drain_events();

    assert_eq!(
        stage2(&work, "simple.txt"),
        "agreed\n",
        "the agreed conflict is staged with the one content both sides carry"
    );
    assert_eq!(
        unmerged(&work),
        "conf.txt",
        "the genuinely differing conflict is left for the resolver"
    );
}

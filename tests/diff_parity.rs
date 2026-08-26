//! Phase L1 parity — in-process `similar` diffs vs the CLI `git diff` text.
//!
//! Drives real temporary repositories and asserts, per fixture, that the
//! row stream the UI's parser builds from [`turbogit::core::diff_engine`]
//! output equals the row stream built from `git diff` output. The producer
//! switch lives at the text level (the cached raw patch), so row-stream
//! equality is exactly the renderer-visible contract.
//!
//! Known cosmetic gap: the in-process patch carries no
//! `index <old>..<new> <mode>` line (blob hashes are not computed
//! in-process). Those metadata rows are filtered on both sides before
//! comparison; every other row — hunk headers included — must match
//! byte-for-byte. Tests additionally assert the CLI text has an `index`
//! line while the in-process text does not, proving each side really took
//! its own path.
//!
//! The 3-way merge is checked against the canonical raw-marker parser:
//! `similar`'s own marker rendering of a merge, fed through
//! [`turbogit::core::conflict::parse_conflict_markers`], must produce the
//! same segment tuples [`turbogit::core::diff_engine::merge_segments`]
//! builds structurally from merge regions.

use std::path::{Path, PathBuf};
use tempfile::TempDir;
use turbogit::core::conflict;
use turbogit::core::diff_engine;
use turbogit::engine::GitExecutor;
use turbogit::engine::cli::CliExecutor;
use turbogit::model::{DiffOpts, VcsSettings};
use turbogit::ui::diff::{RowSummary, parsed_rows};

// ---------------------------------------------------------------- helpers --

/// Run `git` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should be on PATH");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

struct Repo {
    path: PathBuf,
    /// Keeps the temp directory alive for the duration of the test.
    _dir: TempDir,
}

/// Create an initialized temp repository with one base commit on the default
/// branch and repo-local user config so commits work headlessly.
fn temp_repo(name: &str) -> Repo {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    Repo { path, _dir: dir }
}

fn executor() -> CliExecutor {
    CliExecutor {
        settings: VcsSettings::default(),
    }
}

/// Diff options for one path against HEAD (the viewer's Repo chip).
fn repo_opts(path: &str) -> DiffOpts {
    DiffOpts {
        left: Some("HEAD".to_owned()),
        path: Some(PathBuf::from(path)),
        ..DiffOpts::default()
    }
}

/// Row streams of both texts, equalized for the known cosmetic gap: the
/// in-process patch has no `index <hash>..<hash>` metadata line.
fn parity_rows(cli_text: &str, ip_text: &str) -> (Vec<RowSummary>, Vec<RowSummary>) {
    let strip = |text: &str| {
        parsed_rows(text)
            .into_iter()
            .filter(|r| !(r.kind == "meta" && r.text.starts_with("index ")))
            .collect::<Vec<_>>()
    };
    (strip(cli_text), strip(ip_text))
}

/// Assert full row-stream parity for one fixture, plus the provenance
/// markers: git emits an `index` metadata line, the in-process patch does
/// not.
fn assert_row_parity(cli_text: &str, ip_text: &str) {
    assert!(
        cli_text.contains("\nindex ") || cli_text.starts_with("index "),
        "fixture must have a CLI-produced index line:\n{cli_text}"
    );
    assert!(
        !ip_text.contains("\nindex ") && !ip_text.starts_with("index "),
        "in-process patch must not carry blob hashes:\n{ip_text}"
    );
    let (cli_rows, ip_rows) = parity_rows(cli_text, ip_text);
    assert_eq!(ip_rows, cli_rows, "row streams diverged");
}

/// 20-line base file. Edits to `bravo` (line 2) and `quebec` (line 17) are
/// separated by well over twice the default diff context, so both engines
/// report them as two independent hunks.
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

fn commit_words(repo: &Repo, content: &str) {
    std::fs::write(repo.path.join("words.txt"), content).unwrap();
    git(&repo.path, &["add", "words.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "words"]);
}

// ------------------------------------------------------------ diff parity --

#[test]
fn parity_simple_modify_rows_match_cli() {
    let repo = temp_repo("parity-simple");
    commit_words(&repo, "one\ntwo\nthree\n");
    std::fs::write(repo.path.join("words.txt"), "one\nTWO\nthree\n").unwrap();

    let ex = executor();
    let opts = repo_opts("words.txt");
    let cli = ex.diff(&repo.path, &opts).unwrap();
    let ip = diff_engine::diff_text(&ex, &repo.path, &opts).unwrap();

    assert!(cli.contains("@@ -1,3 +1,3 @@"));
    assert_row_parity(&cli, &ip);
}

#[test]
fn parity_multi_hunk_modify_rows_match_cli() {
    let repo = temp_repo("parity-multi-hunk");
    commit_words(&repo, BASE);
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.path.join("words.txt"), &worktree).unwrap();

    let ex = executor();
    let opts = repo_opts("words.txt");
    let cli = ex.diff(&repo.path, &opts).unwrap();
    let ip = diff_engine::diff_text(&ex, &repo.path, &opts).unwrap();

    for rows in [parsed_rows(&cli), parsed_rows(&ip)] {
        assert_eq!(
            rows.iter().filter(|r| r.kind == "hunk").count(),
            2,
            "expected two independent hunks"
        );
    }
    assert_row_parity(&cli, &ip);
}

#[test]
fn parity_newline_at_eof_changes_match_cli() {
    let repo = temp_repo("parity-eof-lost");
    commit_words(&repo, "end\n");
    // Losing the final newline: git hints after the + side only.
    std::fs::write(repo.path.join("words.txt"), "end").unwrap();

    let ex = executor();
    let opts = repo_opts("words.txt");
    let cli = ex.diff(&repo.path, &opts).unwrap();
    let ip = diff_engine::diff_text(&ex, &repo.path, &opts).unwrap();
    assert_row_parity(&cli, &ip);

    // Appending an unterminated line: hint after the appended + line.
    let repo = temp_repo("parity-eof-append");
    commit_words(&repo, BASE);
    let mut worktree = BASE.to_owned();
    worktree.push_str("unterminated");
    std::fs::write(repo.path.join("words.txt"), worktree).unwrap();

    let opts = repo_opts("words.txt");
    let cli = ex.diff(&repo.path, &opts).unwrap();
    let ip = diff_engine::diff_text(&ex, &repo.path, &opts).unwrap();
    assert_row_parity(&cli, &ip);
}

#[test]
fn parity_deleted_file_rows_match_cli() {
    let repo = temp_repo("parity-deleted");
    commit_words(&repo, "gone\nsoon\n");
    std::fs::remove_file(repo.path.join("words.txt")).unwrap();

    let ex = executor();
    let opts = repo_opts("words.txt");
    let cli = ex.diff(&repo.path, &opts).unwrap();
    let ip = diff_engine::diff_text(&ex, &repo.path, &opts).unwrap();

    assert!(cli.contains("deleted file mode"), "{cli}");
    assert!(ip.contains("deleted file mode"), "{ip}");
    assert!(ip.contains("--- a/words.txt\n+++ /dev/null\n"), "{ip}");
    assert_row_parity(&cli, &ip);
}

#[test]
fn parity_binary_change_rows_match_cli() {
    let repo = temp_repo("parity-binary");
    std::fs::write(repo.path.join("blob.dat"), b"\0old-bytes\0").unwrap();
    git(&repo.path, &["add", "blob.dat"]);
    git(&repo.path, &["commit", "-q", "-m", "blob"]);
    std::fs::write(repo.path.join("blob.dat"), b"\0new-bytes\0\0").unwrap();

    let ex = executor();
    let opts = repo_opts("blob.dat");
    let cli = ex.diff(&repo.path, &opts).unwrap();
    let ip = diff_engine::diff_text(&ex, &repo.path, &opts).unwrap();

    assert!(
        cli.contains("Binary files a/blob.dat and b/blob.dat differ"),
        "{cli}"
    );
    assert_row_parity(&cli, &ip);
}

#[test]
fn parity_no_changes_is_empty_like_git() {
    let repo = temp_repo("parity-clean");
    commit_words(&repo, "stable\n");

    let ex = executor();
    let opts = repo_opts("words.txt");
    let cli = ex.diff(&repo.path, &opts).unwrap();
    let ip = diff_engine::diff_text(&ex, &repo.path, &opts).unwrap();
    assert_eq!(cli, "");
    assert_eq!(ip, "");
    assert_eq!(parsed_rows(&ip), parsed_rows(&cli));
}

// ------------------------------------------------------- fallback behavior --

#[test]
fn fallback_whole_tree_target_delegates_to_cli() {
    let repo = temp_repo("fallback-whole-tree");
    commit_words(&repo, BASE);
    let worktree = BASE.replace("bravo", "BRAVO");
    std::fs::write(repo.path.join("words.txt"), &worktree).unwrap();

    let ex = executor();
    let opts = DiffOpts {
        left: Some("HEAD".to_owned()),
        ..DiffOpts::default()
    };
    let delegated = diff_engine::diff_text(&ex, &repo.path, &opts).unwrap();
    let cli = ex.diff(&repo.path, &opts).unwrap();
    assert_eq!(delegated, cli);
    assert!(delegated.contains("-bravo"), "{delegated}");
}

#[test]
fn fallback_unreadable_old_side_delegates_to_cli() {
    let repo = temp_repo("fallback-added");
    // Staged addition: HEAD lacks the path, so rename/new-file metadata is
    // git's call — the in-process path must defer to the CLI verbatim.
    std::fs::write(repo.path.join("added.txt"), "fresh\n").unwrap();
    git(&repo.path, &["add", "added.txt"]);

    let ex = executor();
    let opts = DiffOpts {
        staged: true,
        path: Some(PathBuf::from("added.txt")),
        ..DiffOpts::default()
    };
    let delegated = diff_engine::diff_text(&ex, &repo.path, &opts).unwrap();
    let cli = ex.diff(&repo.path, &opts).unwrap();
    assert_eq!(delegated, cli);
    assert!(delegated.contains("new file mode"), "{delegated}");
}

// ------------------------------------------------------------- 3-way merge --

/// The structured merge segments must equal what the canonical marker parser
/// extracts from `similar`'s own marker rendering of the same merge.
fn assert_merge_parity(base: &str, ours: &str, theirs: &str) {
    let rendered = similar::TextMerge::from_lines(base, ours, theirs).to_string();
    let (parser_segs, parser_conflicts) = conflict::parse_conflict_markers(&rendered);
    let merge_segs = diff_engine::merge_segments(base, ours, theirs);
    let merge_conflicts = merge_segs.iter().filter(|(_, _, c)| *c).count();

    assert_eq!(merge_segs, parser_segs, "segments diverge for {rendered:?}");
    assert_eq!(merge_conflicts, parser_conflicts);
}

#[test]
fn merge_segments_match_marker_parser_on_conflict() {
    assert_merge_parity("one\ntwo\n", "one\nours\n", "one\ntheirs\n");

    let segs = diff_engine::merge_segments("one\ntwo\n", "one\nours\n", "one\ntheirs\n");
    assert_eq!(
        segs,
        vec![
            ("one\n".to_owned(), String::new(), false),
            ("ours\n".to_owned(), "theirs\n".to_owned(), true),
        ]
    );
}

#[test]
fn merge_segments_match_marker_parser_on_insertion_conflict() {
    // Both sides insert different content at the same boundary.
    assert_merge_parity("one\n", "ours\none\n", "theirs\none\n");
}

#[test]
fn merge_autoresolves_non_overlapping_edits_like_markers() {
    let base = "a\nb\nc\nd\ne\nf\ng\n";
    let ours = base.replace('b', "B");
    let theirs = base.replace('f', "F");
    assert_merge_parity(base, &ours, &theirs);

    let segs = diff_engine::merge_segments(base, &ours, &theirs);
    assert!(
        segs.iter().all(|(_, _, c)| !c),
        "non-overlapping edits must not conflict: {segs:?}"
    );
    let composed: String = segs.iter().map(|(a, _, _)| a.as_str()).collect();
    assert_eq!(composed, "a\nB\nc\nd\ne\nF\ng\n");
}

#[test]
fn merge_identical_changes_resolve_without_conflict() {
    assert_merge_parity("x\n", "y\n", "y\n");
    let segs = diff_engine::merge_segments("x\n", "y\n", "y\n");
    assert_eq!(segs, vec![("y\n".to_owned(), String::new(), false)]);
}

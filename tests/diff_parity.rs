//! Phase L1 parity — in-process `similar` diffs vs the CLI `git diff`.
//!
//! Drives real temporary repositories and asserts, per fixture, that the
//! **Git engine**'s patch value from [`turbogit_services::diff_engine`] equals
//! the one from the executor. Both producers now answer with the same value
//! type, so the comparison is structural equality rather than a match of two
//! renderings of a text format: paths, **Hunk** spans, section headings, body
//! lines and **Binary change** metadata all have to agree.
//!
//! Known cosmetic gap: the in-process answer carries no blob-pair **Index**
//! header (hashes are not computed in-process). That one header is stripped on
//! both sides before comparison, and each side is separately asserted to have
//! taken its own path by whether it states it.
//!
//! The 3-way merge is checked against the canonical raw-marker parser:
//! `similar`'s own marker rendering of a merge, fed through
//! [`turbogit_services::conflict::parse_conflict_markers`], must produce the
//! same segment tuples [`turbogit_services::diff_engine::merge_segments`]
//! builds structurally from merge regions.

use std::path::PathBuf;
use tempfile::TempDir;
use test_support::git_seed::git;
use turbogit_domain::model::{DiffOpts, Patch, PatchHeaderLine, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_services::conflict;
use turbogit_services::diff_engine;

// ---------------------------------------------------------------- helpers --

struct Repo {
    path: PathBuf,
    /// Keeps the temp directory alive for the duration of the test.
    _dir: TempDir,
}

/// Create an initialized temp repository with one base commit on the default
/// branch and repo-local user config so commits work headlessly.
/// Local builder, not a `git_seed` recipe: `init -q` with **no `-b main`**, so
/// the default branch is whatever `init.defaultBranch` says (`master` on a
/// stock machine) where every recipe pins `main`, and the base commit is
/// `base.txt`, not `README.md`.
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

/// Both answers, equalized for the known cosmetic gap: the in-process patch
/// states no blob pair.
fn without_index_pairs(patch: &Patch) -> Patch {
    let mut out = patch.clone();
    for file in &mut out.files {
        file.headers
            .retain(|h| !matches!(h, PatchHeaderLine::Index { .. }));
    }
    out
}

fn states_a_blob_pair(patch: &Patch) -> bool {
    patch.files.iter().any(|f| {
        f.headers
            .iter()
            .any(|h| matches!(h, PatchHeaderLine::Index { .. }))
    })
}

/// Assert the two producers agree on one fixture, plus the provenance marker:
/// git's answer states a blob pair, the in-process one cannot.
#[track_caller]
fn assert_patch_parity(cli: &Patch, ip: &Patch) {
    assert!(
        states_a_blob_pair(cli),
        "fixture must have a CLI-produced blob pair:\n{cli}"
    );
    assert!(
        !states_a_blob_pair(ip),
        "in-process patch must not carry blob hashes:\n{ip}"
    );
    assert_eq!(
        without_index_pairs(ip),
        without_index_pairs(cli),
        "the two answers diverged\nfrom git:\n{cli}\nin-process:\n{ip}"
    );
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
fn parity_simple_modify_matches_cli() {
    let repo = temp_repo("parity-simple");
    commit_words(&repo, "one\ntwo\nthree\n");
    std::fs::write(repo.path.join("words.txt"), "one\nTWO\nthree\n").unwrap();

    let ex = executor();
    let opts = repo_opts("words.txt");
    let cli = ex.diff_patch(&repo.path, &opts).unwrap();
    let ip = diff_engine::patch(&ex, &repo.path, &opts).unwrap();

    assert_eq!(
        (
            cli.files[0].hunks[0].old_start,
            cli.files[0].hunks[0].old_count
        ),
        (1, 3),
        "the span git wrote: {cli}"
    );
    assert_patch_parity(&cli, &ip);
}

#[test]
fn parity_multi_hunk_modify_matches_cli() {
    let repo = temp_repo("parity-multi-hunk");
    commit_words(&repo, BASE);
    let worktree = BASE.replace("bravo", "BRAVO").replace("quebec", "QUEBEC");
    std::fs::write(repo.path.join("words.txt"), &worktree).unwrap();

    let ex = executor();
    let opts = repo_opts("words.txt");
    let cli = ex.diff_patch(&repo.path, &opts).unwrap();
    let ip = diff_engine::patch(&ex, &repo.path, &opts).unwrap();

    for answer in [&cli, &ip] {
        assert_eq!(
            answer.hunk_count(),
            2,
            "expected two independent hunks:\n{answer}"
        );
    }
    assert_patch_parity(&cli, &ip);
}

#[test]
fn parity_newline_at_eof_changes_match_cli() {
    let repo = temp_repo("parity-eof-lost");
    commit_words(&repo, "end\n");
    // Losing the final newline: git hints after the + side only.
    std::fs::write(repo.path.join("words.txt"), "end").unwrap();

    let ex = executor();
    let opts = repo_opts("words.txt");
    let cli = ex.diff_patch(&repo.path, &opts).unwrap();
    let ip = diff_engine::patch(&ex, &repo.path, &opts).unwrap();
    assert_eq!(
        ip.files[0].hunks[0]
            .lines
            .iter()
            .filter(|l| l.no_newline)
            .count(),
        1,
        "git marks only the side that lost its newline:\n{ip}"
    );
    assert_patch_parity(&cli, &ip);

    // Appending an unterminated line: hint after the appended + line.
    let repo = temp_repo("parity-eof-append");
    commit_words(&repo, BASE);
    let mut worktree = BASE.to_owned();
    worktree.push_str("unterminated");
    std::fs::write(repo.path.join("words.txt"), worktree).unwrap();

    let opts = repo_opts("words.txt");
    let cli = ex.diff_patch(&repo.path, &opts).unwrap();
    let ip = diff_engine::patch(&ex, &repo.path, &opts).unwrap();
    assert_patch_parity(&cli, &ip);
}

#[test]
fn parity_deleted_file_matches_cli() {
    let repo = temp_repo("parity-deleted");
    commit_words(&repo, "gone\nsoon\n");
    std::fs::remove_file(repo.path.join("words.txt")).unwrap();

    let ex = executor();
    let opts = repo_opts("words.txt");
    let cli = ex.diff_patch(&repo.path, &opts).unwrap();
    let ip = diff_engine::patch(&ex, &repo.path, &opts).unwrap();

    assert!(ip.files[0].deleted_file(), "{ip}");
    assert!(
        ip.files[0].headers.iter().any(|h| matches!(
            h,
            PatchHeaderLine::Sources { old, new }
                if old == "a/words.txt" && new == "/dev/null"
        )),
        "the missing side is named: {ip}"
    );
    // A failed new-side read cannot prove deletion; the CLI answer is kept whole.
    assert_eq!(ip, cli);
}

#[test]
fn parity_binary_change_matches_cli() {
    let repo = temp_repo("parity-binary");
    std::fs::write(repo.path.join("blob.dat"), b"\0old-bytes\0").unwrap();
    git(&repo.path, &["add", "blob.dat"]);
    git(&repo.path, &["commit", "-q", "-m", "blob"]);
    std::fs::write(repo.path.join("blob.dat"), b"\0new-bytes\0\0").unwrap();

    let ex = executor();
    let opts = repo_opts("blob.dat");
    let cli = ex.diff_patch(&repo.path, &opts).unwrap();
    let ip = diff_engine::patch(&ex, &repo.path, &opts).unwrap();

    assert!(cli.files[0].binary(), "a **Binary change**: {cli}");
    assert_patch_parity(&cli, &ip);
}

#[test]
fn parity_no_changes_is_empty_like_git() {
    let repo = temp_repo("parity-clean");
    commit_words(&repo, "stable\n");

    let ex = executor();
    let opts = repo_opts("words.txt");
    let cli = ex.diff_patch(&repo.path, &opts).unwrap();
    let ip = diff_engine::patch(&ex, &repo.path, &opts).unwrap();
    assert!(cli.is_empty(), "nothing changed: {cli}");
    assert!(ip.is_empty(), "nothing changed: {ip}");
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
    let delegated = diff_engine::patch(&ex, &repo.path, &opts).unwrap();
    let cli = ex.diff_patch(&repo.path, &opts).unwrap();
    assert_eq!(delegated, cli);
    assert!(
        delegated.files.iter().any(|f| f.hunks.iter().any(|h| {
            h.lines.iter().any(|l| {
                l.kind == turbogit_domain::model::PatchLineKind::Removed && l.text == "bravo"
            })
        })),
        "the removed line reaches the delegated answer:\n{delegated}"
    );
}

#[test]
fn fallback_unreadable_old_side_delegates_to_cli() {
    let repo = temp_repo("fallback-added");
    // Staged addition: HEAD lacks the path, so rename/new-file metadata is
    // git's call — the in-process path must defer to the CLI answer.
    std::fs::write(repo.path.join("added.txt"), "fresh\n").unwrap();
    git(&repo.path, &["add", "added.txt"]);

    let ex = executor();
    let opts = DiffOpts {
        staged: true,
        path: Some(PathBuf::from("added.txt")),
        ..DiffOpts::default()
    };
    let delegated = diff_engine::patch(&ex, &repo.path, &opts).unwrap();
    let cli = ex.diff_patch(&repo.path, &opts).unwrap();
    assert_eq!(delegated, cli);
    assert!(delegated.files[0].new_file(), "{delegated}");
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

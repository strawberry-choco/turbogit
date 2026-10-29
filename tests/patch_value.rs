use turbogit_domain::model::{Patch, PatchLineKind};
use turbogit_engine_api::patch::parse_patch;

/// The corpus is written with `concat!` rather than `\` continuations on
/// purpose: a continuation strips the whitespace that follows it, and a
/// context line's leading space **is** the line's meaning.
///
/// A simple edit: one hunk, one changed line.
const EDIT: &str = concat!(
    "diff --git a/notes.txt b/notes.txt\n",
    "index 1234567..89abcde 100644\n",
    "--- a/notes.txt\n",
    "+++ b/notes.txt\n",
    "@@ -1,3 +1,3 @@\n",
    " one\n",
    "-two\n",
    "+TWO\n",
    " three\n",
);

const HEADING: &str = concat!(
    "diff --git a/lib.rs b/lib.rs\n",
    "index aaa..bbb 100644\n",
    "--- a/lib.rs\n",
    "+++ b/lib.rs\n",
    "@@ -10,2 +10,3 @@ fn keep_me() {\n",
    " let a = 1;\n",
    "+let b = 2;\n",
    " a\n",
);

const RENAME_WITH_EDIT: &str = concat!(
    "diff --git a/src/old.rs b/src/new.rs\n",
    "index aaa..bbb 100644\n",
    "similarity index 92%\n",
    "rename from src/old.rs\n",
    "rename to src/new.rs\n",
    "--- a/src/old.rs\n",
    "+++ b/src/new.rs\n",
    "@@ -1 +1 @@\n",
    "-a\n",
    "+b\n",
);

const PURE_RENAME: &str = concat!(
    "diff --git a/keep.txt b/kept.txt\n",
    "index aaa..bbb 100644\n",
    "similarity index 100%\n",
    "rename from keep.txt\n",
    "rename to kept.txt\n",
);

const BINARY: &str = concat!(
    "diff --git a/logo.png b/logo.png\n",
    "index aaa..bbb 100644\n",
    "Binary files a/logo.png and b/logo.png differ\n",
);

const MODE_ONLY: &str = concat!(
    "diff --git a/script.sh b/script.sh\n",
    "old mode 100644\n",
    "new mode 100755\n",
);

const CREATED: &str = concat!(
    "diff --git a/new.txt b/new.txt\n",
    "new file mode 100644\n",
    "index 0000000..aaa1111\n",
    "--- /dev/null\n",
    "+++ b/new.txt\n",
    "@@ -0,0 +1,2 @@\n",
    "+first\n",
    "+second\n",
);

const DELETED: &str = concat!(
    "diff --git a/gone.txt b/gone.txt\n",
    "deleted file mode 100644\n",
    "index aaa1111..0000000\n",
    "--- a/gone.txt\n",
    "+++ /dev/null\n",
    "@@ -1 +0,0 @@\n",
    "-gone\n",
);

const NO_NEWLINE: &str = concat!(
    "diff --git a/tail.txt b/tail.txt\n",
    "index aaa..bbb 100644\n",
    "--- a/tail.txt\n",
    "+++ b/tail.txt\n",
    "@@ -1,2 +1,2 @@\n",
    " keep\n",
    "-last\n",
    "\\ No newline at end of file\n",
    "+LAST\n",
    "\\ No newline at end of file\n",
);

/// git quotes a path holding a control character, but **not** one holding a
/// plain space, which is why `SPACED_RENAME` below is the other case to handle.
const QUOTED_RENAME: &str = concat!(
    "diff --git \"a/we\\ttabbed.txt\" \"b/we\\ttabbed.txt\"\n",
    "index aaa..bbb 100644\n",
    "--- \"a/we\\ttabbed.txt\"\n",
    "+++ \"b/we\\ttabbed.txt\"\n",
    "@@ -1 +1 @@\n",
    "-x\n",
    "+y\n",
);

const SPACED_RENAME: &str = concat!(
    "diff --git a/keep me.txt b/kept me.txt\n",
    "index aaa..bbb 100644\n",
    "similarity index 100%\n",
    "rename from keep me.txt\n",
    "rename to kept me.txt\n",
);

const TWO_FILES: &str = concat!(
    "diff --git a/one.txt b/one.txt\n",
    "index aaa..bbb 100644\n",
    "--- a/one.txt\n",
    "+++ b/one.txt\n",
    "@@ -1 +1 @@\n",
    "-1\n",
    "+1!\n",
    "diff --git a/two.txt b/two.txt\n",
    "index ccc..ddd 100644\n",
    "--- a/two.txt\n",
    "+++ b/two.txt\n",
    "@@ -1 +1 @@\n",
    "-2\n",
    "+2!\n",
);

const CORPUS: &[(&str, &str)] = &[
    ("edit", EDIT),
    ("hunk heading", HEADING),
    ("rename with edit", RENAME_WITH_EDIT),
    ("pure rename", PURE_RENAME),
    ("binary change", BINARY),
    ("mode-only change", MODE_ONLY),
    ("created file", CREATED),
    ("deleted file", DELETED),
    ("missing newlines", NO_NEWLINE),
    ("quoted path", QUOTED_RENAME),
    ("path with a space", SPACED_RENAME),
    ("two files", TWO_FILES),
    ("empty diff", ""),
];

#[test]
fn every_shape_re_renders_as_the_git_text_it_was_read_from() {
    for (label, text) in CORPUS {
        let value = parse_patch(text);
        assert_eq!(
            &value.to_string(),
            text,
            "{label}: the value lost, or invented, something"
        );
    }
}

#[test]
fn parsing_a_value_twice_is_stable() {
    for (label, text) in CORPUS {
        let once = parse_patch(text);
        let twice = parse_patch(&once.to_string());
        assert_eq!(once, twice, "{label}: parse(render(parse(x))) drifted");
    }
}

#[test]
fn a_quoted_path_is_one_name_with_the_quoting_resolved() {
    let patch = parse_patch(QUOTED_RENAME);
    let file = &patch.files[0];
    assert_eq!(
        file.old_path, "we\ttabbed.txt",
        "git's C-style quoting is resolved here, once"
    );
    assert_eq!(file.new_path, "we\ttabbed.txt");
    assert_eq!(
        file.headers
            .iter()
            .find_map(|h| match h {
                turbogit_domain::model::PatchHeaderLine::Sources { old, .. } => Some(old.clone()),
                _ => None,
            })
            .expect("a source pair"),
        "a/we\ttabbed.txt"
    );
}

#[test]
fn a_missing_final_newline_belongs_to_the_line_it_qualifies() {
    let patch = parse_patch(NO_NEWLINE);
    let hunk = &patch.files[0].hunks[0];
    assert!(
        !hunk.lines.iter().any(|l| l.text.starts_with('\\')),
        "the marker must never survive as a line of its own"
    );
    let marked: Vec<usize> = hunk
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.no_newline)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        marked,
        vec![1, 2],
        "each marker belongs to the line it followed"
    );
}

#[test]
fn a_mode_only_change_is_its_own_case_rather_than_an_empty_hunk_list() {
    let patch = parse_patch(MODE_ONLY);
    let file = &patch.files[0];
    assert_eq!(file.old_mode(), Some("100644"));
    assert_eq!(file.new_mode(), Some("100755"));
    assert!(file.mode_only());

    let binary = parse_patch(BINARY);
    assert!(
        !binary.files[0].mode_only(),
        "a **Binary change** has no hunks either; the two must not be confused"
    );
}

#[test]
fn a_rename_without_a_content_change_names_both_paths() {
    let patch = parse_patch(PURE_RENAME);
    let file = &patch.files[0];
    assert!(file.renamed());
    assert_eq!(file.similarity(), Some(100));
    assert_eq!(file.old_path, "keep.txt");
    assert_eq!(file.new_path, "kept.txt");
    assert!(file.hunks.is_empty());
    assert!(
        !file.mode_only(),
        "no hunks is also what a rename looks like; the two must not be confused"
    );
}

#[test]
fn a_binary_change_is_a_file_with_metadata_and_no_hunks() {
    let patch = parse_patch(BINARY);
    assert_eq!(patch.files.len(), 1);
    let file = &patch.files[0];
    assert!(file.binary());
    assert!(file.hunks.is_empty());
    assert_eq!(file.new_path, "logo.png");
}

#[test]
fn a_combined_section_keeps_one_path_the_body_and_its_kind() {
    let patch = parse_patch(concat!(
        "diff --cc conf.txt\n",
        "index 83b9501,2299c37..0000000\n",
        "--- a/conf.txt\n",
        "+++ b/conf.txt\n",
        "@@@ -1,1 -1,1 +1,5 @@@\n",
        "++<<<<<<< HEAD\n",
        " +main line\n",
        "++=======\n",
        "+ side\n",
        "++>>>>>>> side\n",
    ));
    assert_eq!(patch.files.len(), 1, "one unmerged path, one section");
    let file = &patch.files[0];
    assert!(file.combined, "the section says which shape it was");
    assert_eq!(
        (file.old_path.as_str(), file.new_path.as_str()),
        ("conf.txt", "conf.txt")
    );
    let hunk = &file.hunks[0];
    assert_eq!(
        (
            hunk.old_start,
            hunk.old_count,
            hunk.new_start,
            hunk.new_count
        ),
        (1, 1, 1, 5),
        "the first parent's range and the new side's"
    );
    assert_eq!(hunk.lines[0].kind, PatchLineKind::Added);
    assert_eq!(hunk.lines[0].text, "+<<<<<<< HEAD");
    assert_eq!(hunk.lines[1].kind, PatchLineKind::Context);
    assert_eq!(hunk.lines[1].text, "+main line");
    assert_eq!(
        parse_patch(&patch.to_string()),
        patch,
        "the normalized header must read back as the same value:\n{patch}"
    );
}

#[test]
fn an_untracked_file_reads_as_a_whole_file_addition() {
    let patch = parse_patch(CREATED);
    let file = &patch.files[0];
    assert!(file.new_file());
    assert_eq!(file.hunks.len(), 1);
    assert_eq!(file.hunks[0].old_start, 0);
    assert_eq!(file.hunks[0].old_count, 0);
    assert_eq!(file.hunks[0].new_count, 2);
    assert!(
        file.hunks[0]
            .lines
            .iter()
            .all(|l| l.kind == PatchLineKind::Added),
        "every body line of a creation is an addition"
    );
}

#[test]
fn an_empty_diff_is_a_value_not_a_missing_one() {
    let patch = parse_patch("");
    assert!(patch.is_empty());
    assert_eq!(patch.hunk_count(), 0);
    assert_eq!(patch.to_string(), "");
}

#[test]
fn a_hunk_carries_its_span_on_each_side_and_the_heading_git_appended() {
    let patch = parse_patch(HEADING);
    let hunk = &patch.files[0].hunks[0];
    assert_eq!((hunk.old_start, hunk.old_count), (10, 2));
    assert_eq!((hunk.new_start, hunk.new_count), (10, 3));
    assert_eq!(hunk.heading.as_deref(), Some("fn keep_me() {"));

    let edit = parse_patch(EDIT);
    let hunk = &edit.files[0].hunks[0];
    assert_eq!(
        hunk.heading, None,
        "no heading is not the same answer as a blank one"
    );
    assert_eq!(hunk.lines.len(), 4);
    assert_eq!(hunk.lines[1].kind, PatchLineKind::Removed);
    assert_eq!(hunk.lines[1].text, "two");
    assert_eq!(hunk.lines[2].kind, PatchLineKind::Added);
}

#[test]
fn paths_are_two_plain_repository_relative_names_never_split_from_a_header() {
    let spaced = parse_patch(SPACED_RENAME);
    assert_eq!(spaced.files[0].old_path, "keep me.txt");
    assert_eq!(
        spaced.files[0].new_path, "kept me.txt",
        "a space is not a quote trigger, so the ` b/` boundary carries it"
    );

    let two = parse_patch(TWO_FILES);
    assert_eq!(two.files.len(), 2);
    assert_eq!(two.hunk_count(), 2);
    assert_eq!(
        two.files
            .iter()
            .map(|f| f.new_path.as_str())
            .collect::<Vec<_>>(),
        vec!["one.txt", "two.txt"]
    );
}

#[test]
fn the_value_stays_unified_diff_shaped_not_display_shaped() {
    let patch: Patch = parse_patch(EDIT);
    let hunk = &patch.files[0].hunks[0];
    assert_eq!(
        hunk.lines[1].kind,
        PatchLineKind::Removed,
        "the value keeps git's order; pairing is the display model's"
    );
    assert_eq!(hunk.lines[2].kind, PatchLineKind::Added);
}

#[test]
fn git_s_own_output_round_trips_through_the_value() {
    use std::path::Path;
    use test_support::git_seed::git;
    use turbogit_domain::model::{DiffOpts, VcsSettings};
    use turbogit_engine::cli::CliExecutor;

    let run = |dir: &Path, args: &[&str]| git(dir, args);

    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    run(&repo, &["init", "-q", "-b", "main"]);
    run(&repo, &["config", "user.name", "t"]);
    run(&repo, &["config", "user.email", "t@t"]);
    std::fs::write(repo.join("notes.txt"), "one\ntwo\n").unwrap();
    std::fs::write(repo.join("keep me.txt"), "stable\n").unwrap();
    std::fs::write(repo.join("logo.png"), [0x89u8, b'P', 0x00, 0xFF, 0xFE]).unwrap();
    std::fs::write(repo.join("script.sh"), "#!/bin/sh\n").unwrap();
    run(&repo, &["add", "--", "."]);
    run(&repo, &["commit", "-q", "-m", "base"]);

    std::fs::write(repo.join("notes.txt"), "one\nTWO\n").unwrap();
    run(&repo, &["mv", "keep me.txt", "kept me.txt"]);
    std::fs::write(repo.join("logo.png"), [0x89u8, b'Q', 0x01, 0xFE, 0xFD]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            repo.join("script.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    run(&repo, &["add", "-A"]);

    let text = run(&repo, &["diff", "--cached", "--no-color"]);
    assert!(
        text.contains("diff --git"),
        "the fixture must have produced a real diff, got {text:?}"
    );
    let value = parse_patch(&text);
    assert!(
        value.files.len() >= 3,
        "edit, rename, binary and mode all present, got {:?}",
        value
            .files
            .iter()
            .map(|f| (&f.new_path, f.binary(), f.renamed(), f.mode_only()))
            .collect::<Vec<_>>()
    );
    let rendered = value.to_string();
    assert_eq!(
        rendered.lines().collect::<Vec<_>>(),
        text.lines().collect::<Vec<_>>(),
        "git's own text round-tripped through the value lost a line"
    );

    let exec = CliExecutor {
        settings: VcsSettings::default(),
    };
    let via_service = turbogit_services::diff_engine::patch(
        &exec,
        &repo,
        &DiffOpts {
            staged: true,
            path: Some(std::path::PathBuf::from("notes.txt")),
            ..Default::default()
        },
    )
    .expect("the diff service answers a patch");
    assert_eq!(via_service.files.len(), 1, "one path, one file section");
    assert_eq!(via_service.hunk_count(), 1);
}

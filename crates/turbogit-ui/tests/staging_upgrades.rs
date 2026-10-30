use std::path::{Path, PathBuf};

use egui_kittest::{Harness, kittest::Queryable};
use test_support::git_seed::git;
use test_support::harness::{
    assert_not_painted, assert_painted, galley_origin, painted_galleys, painted_text,
    shell_harness_over_unstyled,
};
use test_support::wait_until;
use turbogit_app::state::AppState;

fn row(text: &str, staged: bool) -> String {
    format!("{text} ({})", if staged { "staged" } else { "unstaged" })
}

struct Repo {
    path: PathBuf,
}

fn temp_repo(parent: &Path, name: &str) -> Repo {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    Repo { path }
}

fn harness(roots: &[PathBuf]) -> Harness<'static, AppState> {
    let dir = roots
        .first()
        .and_then(|r| r.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let state = AppState::for_roots(&dir, roots);
    let mut h = shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024);
    test_support::harness::settle(&mut h);
    h
}

fn staged_contains(repo: &Repo, needle: &str) -> bool {
    git(&repo.path, &["diff", "--cached", "--name-only"]).contains(needle)
}

#[test]
fn file_list_paints_flat_rows_without_section_headers_or_hunk_counts() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "sections");

    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    std::fs::write(repo.path.join("other.txt"), "staged edit\n").unwrap();
    git(&repo.path, &["add", "other.txt"]);
    std::fs::write(repo.path.join("untracked.txt"), "new\n").unwrap();

    let h = harness(std::slice::from_ref(&repo.path));

    let text = painted_text(&h).join("\n");
    assert!(
        !text.contains("UNSTAGED") && !text.contains("STAGED"),
        "staging section headers must be removed, got:\n{text}"
    );
    for row in ["base.txt", "other.txt", "untracked.txt"] {
        h.get_by_label(row);
    }
    assert!(
        !text.contains("Unversioned Files"),
        "untracked files share the flat list; their own group is gone"
    );

    assert!(
        galley_origin(&h, "1 hunk").is_none(),
        "the unstaged row badge must be gone"
    );
    let text = painted_text(&h).join("\n");
    assert!(
        !text.contains("1/1"),
        "staged rows must not paint the staged/total ratio, got:\n{text}"
    );
}

fn seed_two_hunk_edit(repo: &Repo) {
    let gap = "\n".repeat(10);
    let v1 = format!("fn alpha() {{\n    let a = 1;\n}}\n{gap}fn omega() {{\n    let o = 2;\n}}\n");
    let v2 = format!(
        "fn alpha() {{\n    let alpha = 10;\n}}\n{gap}fn omega() {{\n    let omega = 20;\n}}\n"
    );
    std::fs::write(repo.path.join("code.rs"), &v1).unwrap();
    git(&repo.path, &["add", "code.rs"]);
    git(&repo.path, &["commit", "-q", "-m", "code"]);
    std::fs::write(repo.path.join("code.rs"), &v2).unwrap();
}

const ALPHA_ADD: &str = "    let alpha = 10;";
const OMEGA_ADD: &str = "    let omega = 20;";

fn open_preview(h: &mut Harness<'_, AppState>) {
    h.get_by_label("code.rs").click();
    h.run();
    assert!(
        wait_until(15_000, || h.query_by_label("Stage hunk 1").is_some()),
        "diff preview should load"
    );
    h.run();
}

#[test]
fn hunks_collapse_and_expand_on_demand() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "collapse");
    seed_two_hunk_edit(&repo);

    let mut h = harness(std::slice::from_ref(&repo.path));
    h.get_by_label("code.rs").click();
    open_preview(&mut h);

    assert_painted(&h, ALPHA_ADD);
    assert_painted(&h, OMEGA_ADD);

    h.get_by_label("Collapse hunk 1").click();
    h.run();
    let text = painted_text(&h).join("\n");
    assert!(
        text.contains("hidden"),
        "the collapsed hunk must report its hidden rows, got:\n{text}"
    );
    assert!(!text.contains(ALPHA_ADD), "collapsed body rows must vanish");
    assert!(
        text.contains(OMEGA_ADD),
        "the other hunk's body must stay visible, got:\n{text}"
    );

    h.get_by_label("Expand hunk 1").click();
    h.run();
    assert_painted(&h, ALPHA_ADD);
}

#[test]
fn hunk_headers_name_their_own_staged_state() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "hunk-state");
    seed_two_hunk_edit(&repo);

    let mut h = harness(std::slice::from_ref(&repo.path));
    h.get_by_label("code.rs").click();
    open_preview(&mut h);

    h.get_by_label("Stage hunk 1").click();
    h.run();
    assert!(
        wait_until(15_000, || staged_contains(&repo, "code.rs")),
        "the hunk stage must reach the index, toast={:?} busy={} staged={:?} text=\n{}",
        h.state().ui.toast,
        h.state().ui.busy,
        git(&repo.path, &["diff", "--cached", "--name-only"]),
        painted_text(&h).join("\n")
    );
    let text = painted_text(&h).join("\n");
    let rows: Vec<(f32, String)> = {
        let mut out: Vec<(f32, String)> = painted_galleys(&h)
            .into_iter()
            .filter(|g| g.text.starts_with("@@"))
            .map(|g| (g.pos.y, g.text))
            .collect();
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    };
    assert_eq!(
        rows.len(),
        2,
        "the fixture's premise: two hunk headers to name, got {rows:?}"
    );
    let named_at = |y: f32| {
        painted_galleys(&h)
            .into_iter()
            .find(|g| g.text.starts_with("· ") && (g.pos.y - y).abs() < 8.0)
            .map(|g| g.text)
    };
    assert_eq!(
        named_at(rows[0].0).as_deref(),
        Some("· staged"),
        "the hunk staged through its gutter control answers every one of its rows \
         staged, got {:?}",
        named_at(rows[0].0)
    );
    assert_eq!(
        named_at(rows[1].0).as_deref(),
        Some("· not staged"),
        "and the untouched hunk answers none of them staged, got {:?}",
        named_at(rows[1].0)
    );
    assert!(
        !text.contains("hunk staged") && !text.contains("hunks staged"),
        "the staged-hunk summary rail was removed deliberately, so no painted frame \
         may count staged hunks for the reader, got:\n{text}"
    );
    for stale in ["· local", "· index", "· staged / local"] {
        assert!(
            !text.contains(stale),
            "a hunk header must not classify itself against a comparison that no \
             longer exists, got:\n{text}"
        );
    }
}

#[test]
fn partially_staged_hunk_names_itself_part_in_its_header() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "part-header");
    std::fs::write(
        repo.path.join("code.rs"),
        "fn main() {\n    let alpha = 1;\n    let beta = 2;\n    let gamma = 3;\n}\n",
    )
    .unwrap();
    git(&repo.path, &["add", "code.rs"]);
    git(&repo.path, &["commit", "-q", "-m", "code"]);
    std::fs::write(
        repo.path.join("code.rs"),
        "fn main() {\n    let ALPHA = 1;\n    let beta = 2;\n    let GAMMA = 3;\n}\n",
    )
    .unwrap();

    let mut h = harness(std::slice::from_ref(&repo.path));
    h.get_by_label("code.rs").click();
    open_preview(&mut h);

    h.get_by_label(&row("    let alpha = 1;", false)).click();
    h.run();
    assert!(
        wait_until(15_000, || staged_contains(&repo, "code.rs")),
        "the line stage must reach the index, toast={:?} text=\n{}",
        h.state().ui.toast,
        painted_text(&h).join("\n")
    );
    assert!(
        wait_until(15_000, || {
            h.run();
            h.query_by_label(&row("    let alpha = 1;", true)).is_some()
                && h.query_by_label(&row("    let ALPHA = 1;", false))
                    .is_some()
        }),
        "both rows of the staged line are still in the preview, the clicked one \
         named staged and its partner not"
    );
    h.get_by_label(&row("    let ALPHA = 1;", false)).click();
    h.run();
    assert!(
        wait_until(15_000, || {
            git(&repo.path, &["diff", "--cached", "--", "code.rs"]).contains("+    let ALPHA = 1;")
        }),
        "both clicked lines must be staged, got:\n{}",
        git(&repo.path, &["diff", "--cached", "--", "code.rs"])
    );
    let mut suffixes: Vec<String> = painted_galleys(&h)
        .into_iter()
        .filter(|g| g.text.starts_with("· "))
        .map(|g| g.text)
        .collect();
    suffixes.sort();
    assert_eq!(
        suffixes,
        vec!["· part".to_owned()],
        "the one hunk on screen holds both states, so it names itself part and \
         nothing else does"
    );
}

/// `HunkTarget::Whole` reaches the index through git's own per-file hunk array,
/// addressed by position. Revealing context inserts rows between two hunks'
/// headers, so the one coupling it can break is which hunk a header's stage
/// button names: the same click must produce the same patch either way.
#[test]
fn staging_a_hunk_after_revealing_around_it_stages_what_staging_alone_stages() {
    let quiet_dir = tempfile::tempdir().unwrap();
    let loud_dir = tempfile::tempdir().unwrap();
    let quiet = temp_repo(quiet_dir.path(), "reveal-quiet");
    let loud = temp_repo(loud_dir.path(), "reveal-loud");
    seed_two_hunk_edit(&quiet);
    seed_two_hunk_edit(&loud);

    let mut q = harness(std::slice::from_ref(&quiet.path));
    open_preview(&mut q);
    let mut l = harness(std::slice::from_ref(&loud.path));
    open_preview(&mut l);

    assert_painted(&l, "6 unchanged lines hidden");
    l.get_by_label("Expand lines between hunks 1 and 2").click();
    l.run();
    assert!(
        wait_until(15_000, || {
            l.run();
            l.query_by_label("Expand lines between hunks 1 and 2")
                .is_none()
        }),
        "one tap opens the whole 6-line run, so its strip must go, got:\n{}",
        painted_text(&l).join("\n")
    );
    assert_not_painted(&l, "6 unchanged lines hidden");
    assert!(
        wait_until(15_000, || {
            l.run();
            !l.state().read_pending()
        }),
        "the reveal's read must settle before either pane stages"
    );

    q.get_by_label("Stage hunk 1").click();
    l.get_by_label("Stage hunk 1").click();
    assert!(
        wait_until(15_000, || {
            q.run();
            l.run();
            staged_contains(&quiet, "code.rs") && staged_contains(&loud, "code.rs")
        }),
        "both hunks must reach the index, quiet:\n{}\nloud:\n{}",
        git(&quiet.path, &["diff", "--cached"]),
        git(&loud.path, &["diff", "--cached"])
    );
    assert_eq!(
        git(&quiet.path, &["diff", "--cached"]),
        git(&loud.path, &["diff", "--cached"]),
        "the pane that opened 6 lines of context between its two hunks must stage          exactly what the pane that opened none stages"
    );
}

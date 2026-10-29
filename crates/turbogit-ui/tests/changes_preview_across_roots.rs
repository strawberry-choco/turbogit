use std::path::{Path, PathBuf};

use egui::Vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use test_support::git_seed::git;
use test_support::harness::{painted_text, settle, shell_harness_over};
use turbogit_app::state::AppState;

fn init_repo(name: &str, parent: &Path) -> PathBuf {
    let repo = parent.join(name);
    std::fs::create_dir_all(&repo).expect("repo dir");
    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    repo
}

fn commit_file(repo: &Path, name: &str, content: &str) {
    if let Some(parent) = repo.join(name).parent() {
        std::fs::create_dir_all(parent).expect("dir");
    }
    std::fs::write(repo.join(name), content).expect("write");
    git(repo, &["add", name]);
    git(repo, &["commit", "-m", "seed"]);
}

fn workspace(tmp: &Path) -> (PathBuf, PathBuf) {
    let alpha = init_repo("alpha", tmp);
    let beta = init_repo("beta", tmp);
    commit_file(&alpha, "shared.txt", "alpha version\n");
    commit_file(&beta, "shared.txt", "beta version\n");
    commit_file(&beta, "deep/nested-beta-file.txt", "one\n");
    std::fs::write(alpha.join("shared.txt"), "alpha version CHANGED\n").unwrap();
    std::fs::write(beta.join("shared.txt"), "beta version CHANGED\n").unwrap();
    std::fs::write(beta.join("deep/nested-beta-file.txt"), "one\ntwo\n").unwrap();
    (alpha, beta)
}

fn shell(project_dir: &Path, roots: &[PathBuf]) -> Harness<'static, AppState> {
    let state = AppState::for_roots(project_dir, roots);
    shell_harness_over(state, Vec2::new(1008.0, 900.0))
}

fn expand_group(h: &mut Harness<AppState>, root: &str) {
    h.get_by_label(&format!("Toggle {root}")).click();
    settle(h);
}

fn click_row(h: &mut Harness<AppState>, name: &str, n: usize) {
    let matching: Vec<usize> = h
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter_map(|node| node.accesskit_node().label().map(|l| l.to_string()))
        .enumerate()
        .filter(|(_, l)| l.as_str() == name)
        .map(|(i, _)| i)
        .collect();
    let at = *matching
        .get(n)
        .unwrap_or_else(|| panic!("no {n}th row body labelled {name}"));
    h.get_all_by_role(egui::accesskit::Role::Button)
        .nth(at)
        .expect("row body still present")
        .click();
    settle(h);
}

#[test]
fn a_change_owned_by_another_root_previews_that_change() {
    let project = tempfile::tempdir().expect("tempdir");
    let (alpha, beta) = workspace(project.path());
    let mut h = shell(project.path(), &[alpha.clone(), beta]);
    assert_eq!(h.state().multi.roots.len(), 2, "both roots register");
    assert_eq!(
        h.state().selected_path(),
        Some(alpha),
        "alpha is the focused root"
    );

    expand_group(&mut h, "beta");
    click_row(&mut h, "nested-beta-file.txt", 0);

    let texts = painted_text(&h);
    assert!(
        !texts.iter().any(|t| t.contains("(no differences)")),
        "beta's edit is real; the pane answered for the focused root instead:\n{texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("two")),
        "the pane shows the line beta's worktree added:\n{texts:?}"
    );
}

#[test]
fn two_roots_with_the_same_relative_path_preview_separately() {
    let project = tempfile::tempdir().expect("tempdir");
    let (alpha, beta) = workspace(project.path());
    let mut h = shell(project.path(), &[alpha, beta]);

    expand_group(&mut h, "beta");
    click_row(&mut h, "shared.txt", 1);

    let texts = painted_text(&h);
    assert!(
        texts.iter().any(|t| t.contains("beta version CHANGED")),
        "beta's row previews beta's file, not the focused root's same-named one:\n{texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("alpha version CHANGED")),
        "alpha's line belongs to alpha's row:\n{texts:?}"
    );
}

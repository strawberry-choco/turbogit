use std::path::{Path, PathBuf};
use std::time::Duration;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use tempfile::TempDir;
use test_support::git_seed::git;
use test_support::harness::{KITTEST_DEFAULT_BOX, painted_text, settle_quiet, shell_harness_over};
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{BranchKind, RootId};

fn repo(project: &Path, name: &str, branches: &[&str]) -> PathBuf {
    let dir = project.join(name);
    git(
        project,
        &[
            "-c",
            "init.defaultBranch=main",
            "init",
            "-q",
            dir.to_str().unwrap(),
        ],
    );
    git(dir.as_path(), &["config", "user.email", "t@t"]);
    git(dir.as_path(), &["config", "user.name", "t"]);
    std::fs::write(dir.join("README.md"), "x\n").unwrap();
    git(dir.as_path(), &["add", "."]);
    git(dir.as_path(), &["commit", "-q", "-m", "init"]);
    for b in branches {
        git(dir.as_path(), &["branch", b, "main"]);
    }
    dir
}

fn listed(dir: &Path, branch: &str) -> bool {
    git(dir, &["branch", "--list", branch]).contains(branch)
}

fn loaded() -> (TempDir, PathBuf, Harness<'static, AppState>) {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().to_path_buf();
    let alpha = repo(&project, "alpha", &["shared", "alpha-only"]);
    repo(&project, "beta", &["shared", "beta-only"]);

    let mut state = AppState::new(project.clone());
    state.selected_root = Some(RootId(alpha.as_path().into()));
    let mut harness = shell_harness_over(state, KITTEST_DEFAULT_BOX);
    harness.set_size(egui::vec2(1200.0, 900.0));
    harness.state_mut().ui.tab = Tab::Branches;
    for _ in 0..600 {
        let ready = harness.state().multi.roots.iter().any(|r| {
            r.branches.iter().any(|b| {
                b.name == "beta-only" && b.kind == BranchKind::Local && r.path.ends_with("beta")
            })
        });
        if ready {
            break;
        }
        harness.step();
        std::thread::sleep(Duration::from_millis(10));
    }
    settle_quiet(&mut harness);
    assert_eq!(
        harness.state().multi.roots.len(),
        2,
        "both repos must be registered for this to be a multi-root workspace"
    );
    (tmp, project, harness)
}

fn delete_row(h: &mut Harness<'static, AppState>, branch: &str) {
    let visible = h
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter(|n| n.accesskit_node().label() == Some(branch.to_string()))
        .count();
    assert_eq!(
        visible, 1,
        "`{branch}` must be on screen exactly once to be unambiguously aimed at"
    );
    h.get_all_by_role(egui::accesskit::Role::Button)
        .find(|n| n.accesskit_node().label() == Some(branch.to_string()))
        .unwrap()
        .click_secondary();
    h.step();
    h.step();
    h.remove_cursor();
    h.step();
    h.get_all_by_label("Delete branch")
        .next()
        .unwrap_or_else(|| panic!("no `Delete branch` item; painted={:?}", painted_text(h)))
        .click();
    settle_quiet(h);
    h.get_by_label("OK").click();
    for _ in 0..400 {
        h.step();
        std::thread::sleep(Duration::from_millis(5));
        if !h.state().ui.busy {
            break;
        }
    }
}

fn fold_repo(h: &mut Harness<'static, AppState>, path: &str) {
    let root = h
        .state()
        .multi
        .roots
        .iter()
        .find(|r| r.path.ends_with(path))
        .unwrap_or_else(|| panic!("no root under {path}"))
        .id
        .clone();
    h.state_mut().ui.branches_tree.collapsed_repos.insert(root);
    h.step();
}

#[test]
fn a_delete_empties_the_repo_of_its_own_row() {
    let (_tmp, project, mut harness) = loaded();
    let beta = project.join("beta");
    let alpha = project.join("alpha");

    delete_row(&mut harness, "beta-only");

    assert!(
        !listed(&beta, "beta-only"),
        "`beta-only` survived, so the delete went somewhere else. \
         selected_root={:?}, painted={:?}",
        harness.state().selected_root,
        painted_text(&harness)
    );
    assert!(
        listed(&beta, "shared") && listed(&alpha, "shared"),
        "only the branch the user aimed at may go"
    );
}

#[test]
fn a_delete_does_not_reach_into_the_other_repo_for_a_name_both_share() {
    let (_tmp, project, mut harness) = loaded();
    let beta = project.join("beta");
    let alpha = project.join("alpha");
    assert!(listed(&alpha, "shared") && listed(&beta, "shared"));

    fold_repo(&mut harness, "alpha");
    delete_row(&mut harness, "shared");

    assert!(
        listed(&alpha, "shared"),
        "alpha's `shared` was destroyed by a delete aimed at beta's row"
    );
    assert!(
        !listed(&beta, "shared"),
        "beta's `shared` survived; painted={:?}",
        painted_text(&harness)
    );
}

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use std::path::Path;
use test_support::harness::{KITTEST_DEFAULT_BOX, shell_harness_over_unstyled};
use turbogit_app::state::AppState;

fn git(repo: &Path, args: &[&str]) {
    let out = std::process::Command::new("git").args(args).current_dir(repo).output().unwrap();
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn probe_labels() {
    let parent = tempfile::tempdir().unwrap();
    let repo = parent.path().join("probe-repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "t@t"]);
    git(&repo, &["config", "user.name", "T"]);
    std::fs::write(repo.join("base.txt"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    std::fs::write(repo.join("conf.txt"), "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "base conf"]);
    git(&repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("conf.txt"), "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "side"]);
    git(&repo, &["checkout", "-q", "-"]);
    std::fs::write(repo.join("conf.txt"), "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "main"]);
    let _ = std::process::Command::new("git").args(["merge", "--no-edit", "side"]).current_dir(&repo).output();

    let state = AppState::for_roots(&parent.path().to_path_buf(), std::slice::from_ref(&repo));
    let mut h = shell_harness_over_unstyled(state, KITTEST_DEFAULT_BOX, 4096);
    h.get_by_label("Resolve…").click();
    h.run();
    let labels: Vec<String> = h
        .get_all_by_role(Role::Button)
        .filter_map(|n| n.accesskit_node().label().map(|s| s.to_string()))
        .collect();
    panic!("BUTTON LABELS: {labels:?}");
}

use std::path::Path;

use test_support::git_seed::{git, repo_with_one_commit};
use turbogit_app::shell_writes::branch;
use turbogit_app::state::AppState;

fn state_for(project: &Path, first: &Path, second: &Path) -> AppState {
    let roots = [first.to_path_buf(), second.to_path_buf()];
    AppState::for_roots(project, &roots)
}

fn star_of(state: &AppState, name: &str) -> Option<bool> {
    let id = state.selected_root.clone()?;
    state
        .multi
        .by_id(&id)?
        .branches
        .iter()
        .find(|b| b.name == name)
        .map(|b| b.favorite)
}

fn star_in(state: &AppState, index: usize, name: &str) -> Option<bool> {
    state.multi.roots[index]
        .branches
        .iter()
        .find(|b| b.name == name)
        .map(|b| b.favorite)
}

#[test]
fn toggling_a_favourite_flips_only_the_named_roots_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("ws");
    std::fs::create_dir_all(&project).unwrap();
    let alpha = repo_with_one_commit(&project, "alpha");
    let beta = repo_with_one_commit(&project, "beta");
    for work in [&alpha, &beta] {
        git(work, &["branch", "release", "main"]);
    }

    let mut state = state_for(&project, &alpha, &beta);
    let alpha_id = state.multi.roots[0].id.clone();
    assert_eq!(star_in(&state, 0, "release"), Some(false));
    assert_eq!(star_in(&state, 1, "release"), Some(false));

    branch::toggle_favorite(&mut state, &alpha_id, "release");

    assert_eq!(
        star_in(&state, 0, "release"),
        Some(true),
        "the named root's branch carries the star"
    );
    assert_eq!(
        star_in(&state, 1, "release"),
        Some(false),
        "a branch of the same name in another root is a different branch"
    );
    assert_eq!(
        star_of(&state, "release"),
        Some(true),
        "the focused root is the one that was toggled, so it is what the row paints"
    );

    branch::toggle_favorite(&mut state, &alpha_id, "release");
    assert_eq!(
        star_in(&state, 0, "release"),
        Some(false),
        "the entry toggles: the second call takes the star back off"
    );
}

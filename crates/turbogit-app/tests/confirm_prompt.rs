use std::path::PathBuf;
use std::sync::Arc;

use turbogit_app::confirm::{ConfirmAction, Prompt, prompt};
use turbogit_app::state::{AppState, PendingConfirm, UnmergedCommit, UnmergedCommits};
use turbogit_domain::model::{Change, ChangeStatus, RootId};

fn modified(n: usize) -> Vec<Change> {
    (0..n)
        .map(|i| Change {
            path: PathBuf::from(format!("file-{i:02}.txt")),
            status: ChangeStatus::Modified,
            chunks: vec![],
            staged: false,
            unstaged: true,
            orig_path: None,
        })
        .collect()
}

fn bare_state() -> (tempfile::TempDir, AppState) {
    let tmp = tempfile::tempdir().unwrap();
    let state = AppState::for_roots(tmp.path(), &[]);
    (tmp, state)
}

fn lines(prompt: &Prompt) -> Vec<String> {
    prompt
        .rows
        .iter()
        .flat_map(|row| row.lines.iter())
        .map(|line| line.text.clone())
        .collect()
}

#[test]
fn the_discard_question_names_the_count_the_first_eight_paths_and_the_remainder() {
    let (_tmp, state) = bare_state();

    let prompt = prompt(
        &state,
        &PendingConfirm::Discard {
            changes: modified(11),
        },
    );

    assert_eq!(
        lines(&prompt),
        vec![
            "Discard changes to 11 file(s)? This cannot be undone.",
            "file-00.txt",
            "file-01.txt",
            "file-02.txt",
            "file-03.txt",
            "file-04.txt",
            "file-05.txt",
            "file-06.txt",
            "file-07.txt",
            "(+3 more)",
        ],
        "the question paints its header, then the paths, then the remainder"
    );
}

#[test]
fn a_branch_delete_carries_the_unmerged_link_to_what_it_would_lose() {
    let (_tmp, mut state) = bare_state();
    state.ui.branches_delete_unmerged = Some(UnmergedCommits {
        root: RootId(Arc::from(PathBuf::from("/repo/alpha"))),
        branch: "feature".to_string(),
        merged_target: "main".to_string(),
        commits: vec![
            UnmergedCommit {
                id: "0".repeat(40),
                subject: Some("feature one".to_string()),
            },
            UnmergedCommit {
                id: "1".repeat(40),
                subject: Some("feature two".to_string()),
            },
        ],
        tip_sha: None,
    });

    let prompt = prompt(
        &state,
        &PendingConfirm::DeleteLocalBranch {
            root: RootId(Arc::from(PathBuf::from("/repo/alpha"))),
            name: "feature".to_string(),
        },
    );

    assert_eq!(
        lines(&prompt).first().map(String::as_str),
        Some("Delete local branch 'feature'? This cannot be undone.")
    );
    let disclosure = prompt
        .disclosure
        .as_ref()
        .expect("a delete that would lose commits carries the link to them");
    assert_eq!(disclosure.label, "View 2 unmerged commits");
    assert_eq!(
        disclosure.action,
        ConfirmAction::OpenUnmerged,
        "the link is a thing the app opens, not a thing the renderer dispatches"
    );
}

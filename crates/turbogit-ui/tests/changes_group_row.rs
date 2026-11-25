//! A repository group row shows its branch beside the repository name. A
//! branch name is arbitrary and often long, so the row has to cut it with an
//! ellipsis; an unbounded cell lays out at its natural width and paints past
//! the panel's edge, over whatever sits to the right.

mod support;

use support::{Project, commit_tab, seeded_repo};
use test_support::git_seed::git;

const LONG_BRANCH: &str = "feature/r202610w3-johann-async-docs-extremely-long-branch-name";

fn row_with_long_branch() -> support::Mounted {
    let project = Project::new();
    let repo = seeded_repo(project.path(), "alpha");
    git(&repo, &["checkout", "-q", "-b", LONG_BRANCH]);
    commit_tab(project.path(), std::slice::from_ref(&repo))
}

#[test]
fn a_long_branch_name_is_cut_inside_its_group_row() {
    let app = row_with_long_branch();
    let nodes = app.screen().layout_nodes();
    let row = nodes
        .iter()
        .find(|node| node.test_id.as_deref() == Some("changes:group:alpha"))
        .expect("the group row paints");
    let right = row.bounds.origin.x.0 + row.bounds.size.width.0;
    let parents: std::collections::HashMap<_, _> =
        nodes.iter().map(|node| (node.id, node.parent)).collect();

    let branch = nodes
        .iter()
        .filter(|node| {
            node.text
                .as_ref()
                .is_some_and(|t| t.text.text == LONG_BRANCH)
        })
        .find(|node| {
            let mut id = node.id;
            while let Some(parent) = parents.get(&id) {
                if *parent == row.id {
                    return true;
                }
                id = *parent;
            }
            false
        })
        .expect("the branch name paints in the group row");
    let reaches = branch.bounds.origin.x.0 + branch.bounds.size.width.0;
    assert!(
        reaches <= right + 1.0,
        "the branch reaches {reaches}, past the row's {right}"
    );

    let text = branch.text.as_ref().expect("the branch is a text view");
    assert_eq!(text.text.max_lines, 1, "the branch paints on one line");
    assert_eq!(
        text.text.ellipsize,
        nsh_core::EllipsizeMode::Tail,
        "the cut end carries the ellipsis"
    );
}

//! A changed-file row names the file before its directory, and the name keeps
//! its width whatever the directory's length. A name that cannot fit ends in
//! an ellipsis inside the row.

mod support;

use std::collections::HashMap;
use std::path::PathBuf;

use nsh_testing::LayoutNode;
use support::{Project, commit_tab, open_group};
use test_support::git_seed::git;

const DEEP: &str = "src/business-components/sidebar-tree/components/default-search-bar.tsx";
const NAME: &str = "default-search-bar.tsx";
const DIR: &str = "src/business-components/sidebar-tree/components/";
const LONG: &str = "a-file-name-that-no-narrow-panel-row-can-hold.tsx";

fn repo_with_deep_edit(project: &Project) -> PathBuf {
    let repo = support::seeded_repo(project.path(), "alpha");
    let file = repo.join(DEEP);
    std::fs::create_dir_all(file.parent().expect("the file has a directory")).expect("dirs");
    std::fs::write(&file, "one\n").expect("write");
    std::fs::write(repo.join(LONG), "one\n").expect("write the long name");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "add the files"]);
    std::fs::write(&file, "one\ntwo\n").expect("edit");
    std::fs::write(repo.join(LONG), "one\ntwo\n").expect("edit the long name");
    repo
}

fn open_rows(project: &Project) -> support::Mounted {
    let repo = repo_with_deep_edit(project);
    let mut app = commit_tab(project.path(), &[repo]);
    open_group(&mut app, "alpha");
    app
}

/// Every node's parent, so a text can be asked whether it paints in `row`.
fn ancestry(nodes: &[LayoutNode]) -> HashMap<nsh_core::NodeId, nsh_core::NodeId> {
    nodes.iter().map(|node| (node.id, node.parent)).collect()
}

fn inside(
    node: &LayoutNode,
    row: &LayoutNode,
    parents: &HashMap<nsh_core::NodeId, nsh_core::NodeId>,
) -> bool {
    let mut id = node.id;
    while let Some(parent) = parents.get(&id) {
        if *parent == row.id {
            return true;
        }
        id = *parent;
    }
    false
}

fn text_in<'a>(nodes: &'a [LayoutNode], row: &LayoutNode, label: &str) -> Option<&'a LayoutNode> {
    let parents = ancestry(nodes);
    nodes
        .iter()
        .filter(|node| inside(node, row, &parents))
        .find(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text.text == label)
        })
}

fn row_of<'a>(nodes: &'a [LayoutNode], path: &str) -> &'a LayoutNode {
    nodes
        .iter()
        .find(|node| node.test_id.as_deref() == Some(format!("changes:row:alpha:{path}").as_str()))
        .expect("the changed file paints a row")
}

#[test]
fn a_file_row_paints_the_name_before_its_directory() {
    let project = Project::new();
    let app = open_rows(&project);

    let nodes = app.screen().layout_nodes();
    let row = row_of(&nodes, DEEP);
    let name = text_in(&nodes, row, NAME).expect("the row paints the file name");
    let dir = text_in(&nodes, row, DIR).expect("the row paints the directory");

    assert!(
        name.bounds.size.width.0 > 0.0,
        "the file name is painted with a width: {:?}",
        name.bounds
    );
    assert!(
        name.bounds.origin.x.0 < dir.bounds.origin.x.0,
        "the name at {} comes before the directory at {}",
        name.bounds.origin.x.0,
        dir.bounds.origin.x.0
    );
    let row_right = row.bounds.origin.x.0 + row.bounds.size.width.0;
    let dir_right = dir.bounds.origin.x.0 + dir.bounds.size.width.0;
    assert!(
        dir_right <= row_right + 1.0,
        "the directory reaches {dir_right}, past the row's {row_right}"
    );
}

#[test]
fn a_name_wider_than_the_row_ends_in_an_ellipsis() {
    let project = Project::new();
    let app = open_rows(&project);

    let nodes = app.screen().layout_nodes();
    let row = row_of(&nodes, LONG);
    let name = text_in(&nodes, row, LONG).expect("the row paints the name");
    let row_right = row.bounds.origin.x.0 + row.bounds.size.width.0;
    let name_right = name.bounds.origin.x.0 + name.bounds.size.width.0;
    assert!(
        name_right <= row_right + 1.0,
        "the name reaches {name_right}, past the row's {row_right}"
    );

    let text = name.text.as_ref().expect("the name is a text view");
    assert_eq!(text.text.max_lines, 1, "the name paints on one line");
    assert_eq!(
        text.text.ellipsize,
        nsh_core::EllipsizeMode::Tail,
        "the cut end carries the ellipsis"
    );
}

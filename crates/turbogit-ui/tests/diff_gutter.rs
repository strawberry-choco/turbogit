//! Every staging diff row shares one gutter: a staged line's number sits in
//! the column a context line's does, and the code text starts where
//! `TEXT_X` says it does.

mod support;

use std::collections::HashMap;

use nsh_testing::LayoutNode;
use support::{Project, commit_tab, open_group};
use test_support::git_seed::git;
use turbogit_ui::screens::diff_view::TEXT_X;

const FILE: &str = "lines.txt";

fn edited_line(project: &Project) -> support::Mounted {
    let repo = support::seeded_repo(project.path(), "alpha");
    std::fs::write(repo.join(FILE), "one\n").expect("write");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "one line"]);
    std::fs::write(repo.join(FILE), "one\ntwo\n").expect("edit");
    let mut app = commit_tab(project.path(), &[repo]);
    open_group(&mut app, "alpha");
    app.press(&format!("id:changes:row:alpha:{FILE}"))
        .expect("the row previews");
    app.wait_for("id:diff:list")
        .expect("the staging fallback mounts");
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

/// The texts of `row` in paint order, with the leading gutter marks left out:
/// the number and the sign are the gutter, everything after them is code.
fn code_texts<'a>(
    nodes: &'a [LayoutNode],
    row: &LayoutNode,
    parents: &HashMap<nsh_core::NodeId, nsh_core::NodeId>,
) -> Vec<&'a LayoutNode> {
    nodes
        .iter()
        .filter(|node| inside(node, row, parents))
        .filter(|node| {
            node.text.as_ref().is_some_and(|text| {
                let text = text.text.text.trim();
                !matches!(text, "" | "+" | "−") && text.parse::<usize>().is_err()
            })
        })
        .collect()
}

#[test]
fn every_staging_row_shares_one_gutter_column() {
    let project = Project::new();
    let app = edited_line(&project);

    let nodes = app.screen().layout_nodes();
    let parents = ancestry(&nodes);
    let rows: Vec<&LayoutNode> = nodes
        .iter()
        .filter(|node| {
            node.test_id
                .as_deref()
                .is_some_and(|id| id.starts_with("diff:add:") || id.starts_with("diff:context:"))
        })
        .collect();
    assert!(
        rows.len() >= 2,
        "the comparison paints a context and an added line: {:?}",
        rows.iter().map(|row| &row.test_id).collect::<Vec<_>>()
    );

    let mut numbers = Vec::new();
    let mut code = Vec::new();
    for row in &rows {
        let gutter = nodes
            .iter()
            .filter(|node| inside(node, row, &parents))
            .filter_map(|node| {
                let text = node.text.as_ref()?.text.text.trim().to_owned();
                text.parse::<usize>().ok().map(|_| node.bounds.origin.x.0)
            })
            .next()
            .unwrap_or_else(|| panic!("{:?} paints a line number", row.test_id));
        numbers.push((row.test_id.clone(), gutter));
        let texts = code_texts(&nodes, row, &parents);
        let first = texts
            .first()
            .unwrap_or_else(|| panic!("{:?} paints its code text", row.test_id));
        code.push((row.test_id.clone(), first.bounds.origin.x.0));
    }

    let first = numbers[0].1;
    for (id, x) in &numbers {
        assert!(
            (x - first).abs() < 0.5,
            "{id:?} puts its number at {x}, the first row at {first}: {:?}",
            numbers
        );
    }
    // `TEXT_X` maps a press's x to a character, so it has to stay the text's
    // own origin: a gutter that grows without it moves the drag anchor off
    // the line it is dragging.
    let row_x = rows[0].bounds.origin.x.0;
    for (id, x) in &code {
        assert!(
            (x - row_x - TEXT_X).abs() < 0.5,
            "{id:?} starts its text at {x}, not at TEXT_X {TEXT_X} past the row's {row_x}"
        );
    }
}

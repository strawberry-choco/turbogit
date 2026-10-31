use std::collections::{HashMap, HashSet};

use turbogit_domain::model::{Branch, BranchKind};

/// One completable family of branch names, and how much of the repository lives
/// in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefix {
    /// Always ends with `/`. `feature/`, `feat/blame/`. Also the row's identity.
    pub text: String,
    /// Local branches under it, painted as the row's quiet right-hand count.
    pub branches: usize,
}

struct Row {
    branches: usize,
    depth: usize,
}

/// The rows a branch-name field offers for `typed`, ranked and capped at
/// `limit`.
///
/// The whole branch set goes in and the local-only rule is inside the function,
/// because a remote row's slashes belong to a remote name and not to a naming
/// convention: deriving from every row would offer `origin/` as a family to
/// write into. A call site cannot get that wrong, which is the point.
pub fn completing(branches: &[Branch], typed: &str, limit: usize) -> Vec<Prefix> {
    // A local branch really named `origin/experiment` would otherwise offer
    // `origin/`, and `git branch <new> origin/x` is the ambiguous refname git
    // itself warns about.
    let remote_names: HashSet<&str> = branches
        .iter()
        .filter_map(|b| b.remote.as_deref())
        .collect();

    let mut table: HashMap<String, Row> = HashMap::new();
    for branch in branches.iter().filter(|b| b.kind == BranchKind::Local) {
        let segments: Vec<&str> = branch.name.split('/').collect();
        if segments.len() < 2 {
            continue;
        }
        for i in 0..=segments.len() - 2 {
            let text = format!("{}/", segments[..=i].join("/"));
            let depth = i + 1;
            let row = table
                .entry(text)
                .or_insert_with(|| Row { branches: 0, depth });
            row.branches += 1;
        }
    }

    let lower = typed.to_lowercase();
    let typed_len = typed.chars().count();
    let mut matched: Vec<(usize, String, usize)> = table
        .iter()
        .filter(|(text, _)| text.chars().count() > typed_len)
        .filter(|(text, _)| text.to_lowercase().starts_with(&lower))
        .filter(|(text, _)| match text.split('/').next() {
            Some(first) => !remote_names.contains(first),
            None => true,
        })
        .map(|(text, row)| (row.depth, text.clone(), row.branches))
        .collect();

    matched.sort_by(|a, b| a.0.cmp(&b.0).then(b.2.cmp(&a.2)).then(a.1.cmp(&b.1)));
    matched.truncate(limit);
    matched
        .into_iter()
        .map(|(_, text, branches)| Prefix { text, branches })
        .collect()
}

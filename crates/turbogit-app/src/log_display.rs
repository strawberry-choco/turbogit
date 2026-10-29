use std::collections::HashMap;
use std::sync::Arc;

use turbogit_domain::model::{Commit, CommitId, RootId};

use crate::root_caches::RootCaches;
use crate::state::AppState;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogInputs {
    pub root_filter: Option<RootId>,
    pub path_scope: Option<std::path::PathBuf>,
    pub ref_scope: Option<(RootId, String)>,
    /// The live search text (`ui.log_filter`) verbatim. The pickaxe cache is
    /// keyed by its TRIMMED form, so a difference of leading or trailing
    /// whitespace must rebuild; the filter itself matches case-insensitively.
    pub filter: String,
    pub roots: Vec<RootId>,
    pub scope_root: Option<RootId>,
}

impl LogInputs {
    pub fn of(state: &AppState) -> Self {
        Self {
            root_filter: state.ui.log_root_filter.clone(),
            path_scope: state.ui.log_path_scope.clone(),
            ref_scope: state.ui.log_ref_scope.clone(),
            filter: state.ui.log_filter.clone(),
            roots: visible_roots(state),
            scope_root: state
                .ui
                .log_path_scope
                .as_ref()
                .and_then(|_| state.selected_root.clone()),
        }
    }
}

pub fn visible_roots(state: &AppState) -> Vec<RootId> {
    match &state.ui.log_root_filter {
        Some(id) => vec![id.clone()],
        None => state.multi.roots.iter().map(|r| r.id.clone()).collect(),
    }
}

pub type LogRow = Arc<Commit>;

pub struct LogDisplay {
    rows: Vec<LogRow>,
    lanes: HashMap<CommitId, usize>,
    revision: u64,
    inputs: LogInputs,
}

impl LogDisplay {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn row(&self, index: usize) -> Option<&Commit> {
        self.rows.get(index).map(|row| row.as_ref())
    }

    pub fn rows(&self) -> &[LogRow] {
        &self.rows
    }

    pub fn lane(&self, id: &str) -> Option<usize> {
        self.lanes.get(id).copied()
    }

    pub fn lanes(&self) -> &HashMap<CommitId, usize> {
        &self.lanes
    }
}

/// Whether the held window still answers the question in front of it. One
/// pure predicate, over the two things that can change the answer: the caches'
/// revision (a batch landed, a window settled, a scope refilled, anything was
/// invalidated) and the UI-side fingerprint.
/// `false` is the promise the log pane makes to its own paint path: the held
/// rows and their lane colours are still the answer, so the frame reads them
/// and computes nothing. It is what makes an idle frame cost nothing, and it
/// is testable with no renderer and no counter.
pub fn needs_rebuild(held: Option<&LogDisplay>, revision: u64, inputs: &LogInputs) -> bool {
    match held {
        None => true,
        Some(window) => window.revision != revision || window.inputs != *inputs,
    }
}

pub fn derive(
    revision: u64,
    inputs: &LogInputs,
    loaded: Vec<LogRow>,
    hits: Vec<LogRow>,
) -> LogDisplay {
    let mut rows = loaded;
    rows.sort_by(|a, b| b.time.cmp(&a.time).then(a.id.cmp(&b.id)));
    let lanes = assign_colors(&rows);
    if !inputs.filter.is_empty() {
        let filter = inputs.filter.to_lowercase();
        rows.retain(|c| {
            c.message.to_lowercase().contains(&filter)
                || c.id.to_lowercase().contains(&filter)
                || c.author.name.to_lowercase().contains(&filter)
        });
        if inputs.path_scope.is_none() {
            for c in hits {
                if !rows.iter().any(|v| v.id == c.id) {
                    rows.push(c);
                }
            }
            rows.sort_by(|a, b| b.time.cmp(&a.time).then(a.id.cmp(&b.id)));
        }
    }
    LogDisplay {
        rows,
        lanes,
        revision,
        inputs: inputs.clone(),
    }
}

fn assign_colors(commits: &[LogRow]) -> HashMap<CommitId, usize> {
    let mut color_of: HashMap<CommitId, usize> = HashMap::new();
    let mut lanes: Vec<Option<CommitId>> = Vec::new();
    let mut next_color = 0usize;
    for c in commits {
        let idx = lanes
            .iter()
            .position(|l| l.as_deref() == Some(c.id.as_str()))
            .unwrap_or_else(|| {
                if let Some(e) = lanes.iter().position(|l| l.is_none()) {
                    e
                } else {
                    lanes.push(None);
                    lanes.len() - 1
                }
            });
        color_of.entry(c.id.clone()).or_insert_with(|| {
            if idx < lanes.len() && lanes[idx].is_none() {
                let c = next_color;
                next_color += 1;
                c
            } else {
                idx
            }
        });
        lanes[idx] = c.parents.first().cloned();
        for p in c.parents.iter().skip(1) {
            if let Some(e) = lanes.iter_mut().find(|l| l.is_none()) {
                e.replace(p.clone());
                color_of.entry(p.clone()).or_insert_with(|| {
                    let c = next_color;
                    next_color += 1;
                    c
                });
            } else {
                lanes.push(Some(p.clone()));
                color_of.entry(p.clone()).or_insert_with(|| {
                    let c = next_color;
                    next_color += 1;
                    c
                });
            }
        }
    }
    color_of
}

pub fn sources(caches: &RootCaches, inputs: &LogInputs) -> (Vec<LogRow>, Vec<LogRow>) {
    let loaded: Vec<LogRow> = if let Some((root, ref_name)) = &inputs.ref_scope {
        owned(caches.ref_log(root, ref_name).unwrap_or_default())
    } else if inputs.path_scope.is_some() {
        match &inputs.scope_root {
            Some(root) => scoped_window(caches, inputs, root),
            None => Vec::new(),
        }
    } else {
        union(&inputs.roots, |id| caches.log(id))
    };
    let hits: Vec<LogRow> = match (inputs.path_scope.is_none(), inputs.filter.trim()) {
        (true, query) if !query.is_empty() => {
            union(&inputs.roots, |id| caches.search_log(id, query))
        }
        _ => Vec::new(),
    };
    (loaded, hits)
}

fn union<'a>(roots: &[RootId], read: impl Fn(&RootId) -> Option<&'a [Commit]>) -> Vec<LogRow> {
    roots
        .iter()
        .filter_map(&read)
        .flat_map(|window| window.iter().cloned().map(Arc::new))
        .collect()
}

fn scoped_window(caches: &RootCaches, inputs: &LogInputs, root: &RootId) -> Vec<LogRow> {
    if let Some(path) = &inputs.path_scope
        && let Some(commits) = caches.path_log(root, path)
    {
        return owned(commits);
    }
    owned(caches.log(root).unwrap_or_default())
}

fn owned(commits: &[Commit]) -> Vec<LogRow> {
    commits.iter().cloned().map(Arc::new).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use turbogit_domain::model::Signature;

    fn root(name: &str) -> RootId {
        RootId(Arc::from(Path::new(name)))
    }

    fn commit(root: &RootId, id: &str, parents: &[&str], time: i64) -> LogRow {
        authored(root, id, parents, time, "t")
    }

    fn authored(root: &RootId, id: &str, parents: &[&str], time: i64, author: &str) -> LogRow {
        Arc::new(Commit {
            id: id.to_owned(),
            parents: parents.iter().map(|p| (*p).to_owned()).collect(),
            author: Signature {
                name: author.to_owned(),
                email: "t@t".to_owned(),
                time,
            },
            committer: Signature {
                name: "t".to_owned(),
                email: "t@t".to_owned(),
                time,
            },
            message: format!("c{id}"),
            time,
            root: root.clone(),
            signature: Default::default(),
        })
    }

    fn linear() -> (RootId, Vec<LogRow>) {
        let r = root("alpha");
        let rows = vec![
            commit(&r, "a", &["b"], 300),
            commit(&r, "b", &["c"], 200),
            commit(&r, "c", &[], 100),
        ];
        (r, rows)
    }

    fn inputs_for(roots: &[RootId]) -> LogInputs {
        LogInputs {
            roots: roots.to_vec(),
            ..LogInputs::default()
        }
    }

    #[test]
    fn a_linear_history_walks_in_one_lane() {
        let (_r, rows) = linear();
        let lanes = assign_colors(&rows);
        assert_eq!(lanes.len(), 3, "a commit and both its parents are coloured");
        assert_eq!(lanes["a"], 0);
        assert_eq!(lanes["b"], 0, "the first parent inherits the lane");
        assert_eq!(lanes["c"], 0);
    }

    #[test]
    fn a_merge_gives_its_second_parent_a_fresh_lane() {
        let r = root("alpha");
        let rows = vec![
            commit(&r, "m", &["first", "side"], 400),
            commit(&r, "first", &["base"], 300),
            commit(&r, "side", &["base"], 200),
            commit(&r, "base", &[], 100),
        ];
        let lanes = assign_colors(&rows);
        assert_eq!(lanes["m"], 0);
        assert_eq!(lanes["first"], 0, "the first parent continues the lane");
        assert_eq!(lanes["side"], 1, "the merged-in branch opens the next lane");
        assert_eq!(lanes["base"], 0);
    }

    #[test]
    fn a_row_keeps_its_lane_when_a_batch_is_appended_above_it() {
        let r = root("alpha");
        let old = vec![commit(&r, "b", &["c"], 200), commit(&r, "c", &[], 100)];
        let grown = vec![commit(&r, "a", &["b"], 300)];
        let mut rows = grown;
        rows.extend(old.clone());
        let before = assign_colors(&old);
        let after = assign_colors(&rows);
        for (id, lane) in &before {
            assert_eq!(
                after.get(id),
                Some(lane),
                "row {id} was renumbered by the batch above it"
            );
        }
    }

    #[test]
    fn a_parent_below_the_loaded_window_takes_no_colour_of_its_own() {
        let r = root("alpha");
        let lanes = assign_colors(&[commit(&r, "a", &["b"], 300)]);
        assert_eq!(lanes["a"], 0);
        assert_eq!(lanes.get("b"), None, "an unwalked commit has no colour");
    }

    #[test]
    fn the_walk_is_pure_over_its_input() {
        let (_r, rows) = linear();
        assert_eq!(assign_colors(&rows), assign_colors(&rows));
    }

    #[test]
    fn the_display_window_is_a_stable_indexable_sequence() {
        let (r, rows) = linear();
        let window = derive(7, &inputs_for(&[r]), rows, Vec::new());
        assert_eq!(window.len(), 3);
        assert_eq!(window.rows().len(), 3);
        assert_eq!(window.row(0).map(|c| c.id.as_str()), Some("a"));
        assert_eq!(window.row(2).map(|c| c.id.as_str()), Some("c"));
        assert_eq!(window.row(3), None, "past the end is a miss, not a panic");
        assert!(!window.is_empty());
        assert_eq!(window.rows()[0].id, "a", "the rows own their commits");
    }

    #[test]
    fn an_empty_window_is_empty_rather_than_absent() {
        let window = derive(0, &LogInputs::default(), Vec::new(), Vec::new());
        assert_eq!(window.len(), 0);
        assert!(window.is_empty());
    }

    #[test]
    fn rows_are_ordered_newest_first_with_the_id_breaking_ties() {
        let r = root("alpha");
        let rows = vec![
            commit(&r, "z", &[], 100),
            commit(&r, "y", &[], 300),
            commit(&r, "x", &[], 300),
        ];
        let window = derive(0, &inputs_for(&[r]), rows, Vec::new());
        let ids: Vec<&str> = window.rows().iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["x", "y", "z"]);
    }

    #[test]
    fn the_lane_index_is_held_with_the_window() {
        let (r, rows) = linear();
        let window = derive(3, &inputs_for(&[r]), rows, Vec::new());
        for row in window.rows() {
            assert_eq!(
                window.lane(&row.id),
                window.lanes().get(&row.id).copied(),
                "row {} must read its lane off the held index",
                row.id
            );
        }
        assert_eq!(window.lane("nope"), None, "an unwalked commit has no lane");
    }

    #[test]
    fn lane_colours_come_from_the_whole_loaded_window() {
        let r = root("alpha");
        let rows = vec![
            commit(&r, "m", &["first", "side"], 400),
            commit(&r, "first", &["base"], 300),
            commit(&r, "side", &["base"], 200),
            commit(&r, "base", &[], 100),
        ];
        let unfiltered = derive(
            0,
            &inputs_for(std::slice::from_ref(&r)),
            rows.clone(),
            Vec::new(),
        );
        let mut inputs = inputs_for(&[r]);
        inputs.filter = "zzz-no-such-row".to_owned();
        let filtered = derive(0, &inputs, rows, Vec::new());
        assert_eq!(
            filtered.len(),
            0,
            "the filter hid every row — the walk's input is not the filter's"
        );
        assert_eq!(
            filtered.lanes(),
            unfiltered.lanes(),
            "hiding every row must not renumber a single lane"
        );
        assert_ne!(
            filtered.lane("side"),
            filtered.lane("first"),
            "and the merge's two branches still differ"
        );
    }

    #[test]
    fn the_live_filter_matches_message_hash_and_author_case_insensitively() {
        let r = root("alpha");
        let rows = vec![
            commit(&r, "keep", &[], 300),
            commit(&r, "HashMe", &[], 200),
            authored(&r, "a", &[], 100, "Ada Lovelace"),
        ];
        let mut inputs = inputs_for(&[r]);
        for (term, expected) in [
            ("keep", vec!["keep"]),
            ("hashme", vec!["HashMe"]),
            ("ADA", vec!["a"]),
            ("nomatch", vec![]),
        ] {
            inputs.filter = term.to_owned();
            let window = derive(0, &inputs, rows.clone(), Vec::new());
            let ids: Vec<&str> = window.rows().iter().map(|c| c.id.as_str()).collect();
            assert_eq!(ids, expected, "filter {term:?}");
        }
        let window = derive(0, &inputs_for(&[root("alpha")]), rows, Vec::new());
        assert_eq!(window.len(), 3);
    }

    #[test]
    fn pickaxe_hits_are_unioned_deduped_and_resorted() {
        let r = root("alpha");
        let rows = vec![
            commit(&r, "a", &[], 300),
            commit(&r, "b", &[], 200),
            commit(&r, "c", &[], 100),
        ];
        let hits = vec![commit(&r, "c", &[], 100), commit(&r, "old", &[], 50)];
        let mut inputs = inputs_for(&[r]);
        inputs.filter = "a".to_owned();
        let window = derive(0, &inputs, rows, hits);
        let ids: Vec<&str> = window.rows().iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            ["a", "c", "old"],
            "the matching row, then the hits oldest-first, one of each"
        );
    }

    #[test]
    fn pickaxe_hits_are_skipped_inside_a_path_scope() {
        let r = root("alpha");
        let rows = vec![commit(&r, "a", &[], 300)];
        let hits = vec![commit(&r, "hit", &[], 50)];
        let mut inputs = inputs_for(&[r]);
        inputs.filter = "a".to_owned();
        inputs.path_scope = Some(PathBuf::from("f.txt"));
        let window = derive(0, &inputs, rows, hits);
        let ids: Vec<&str> = window.rows().iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["a"], "a path scope never mixes in pickaxe hits");
    }

    #[test]
    fn a_held_window_with_the_same_revision_and_inputs_is_still_the_answer() {
        let (r, rows) = linear();
        let inputs = inputs_for(&[r]);
        let held = derive(4, &inputs, rows, Vec::new());
        assert!(
            !needs_rebuild(Some(&held), 4, &inputs),
            "a frame that changed nothing must not rebuild"
        );
        assert!(!needs_rebuild(Some(&held), 4, &inputs));
    }

    #[test]
    fn a_missing_window_is_always_built() {
        assert!(needs_rebuild(None, 0, &LogInputs::default()));
    }

    #[test]
    fn a_moved_cache_revision_rebuilds() {
        let (r, rows) = linear();
        let inputs = inputs_for(&[r]);
        let held = derive(4, &inputs, rows, Vec::new());
        assert!(needs_rebuild(Some(&held), 5, &inputs));
    }

    #[test]
    fn each_ui_input_rebuilds_on_its_own() {
        let (r, rows) = linear();
        let other = root("beta");
        let base = inputs_for(std::slice::from_ref(&r));
        let held = derive(4, &base, rows.clone(), Vec::new());
        let changes: Vec<(&str, LogInputs)> = vec![
            (
                "the roots filter narrows",
                LogInputs {
                    root_filter: Some(other.clone()),
                    ..base.clone()
                },
            ),
            (
                "a path scope opens",
                LogInputs {
                    path_scope: Some(PathBuf::from("f.txt")),
                    ..base.clone()
                },
            ),
            (
                "a ref scope opens",
                LogInputs {
                    ref_scope: Some((r.clone(), "main".to_owned())),
                    ..base.clone()
                },
            ),
            (
                "a search term is typed",
                LogInputs {
                    filter: "needle".to_owned(),
                    ..base.clone()
                },
            ),
            (
                "a second root comes into view",
                LogInputs {
                    roots: vec![r.clone(), other.clone()],
                    ..base.clone()
                },
            ),
            (
                "the scoped repository changes",
                LogInputs {
                    path_scope: Some(PathBuf::from("f.txt")),
                    scope_root: Some(other),
                    ..base.clone()
                },
            ),
        ];
        for (what, inputs) in changes {
            assert!(
                needs_rebuild(Some(&held), 4, &inputs),
                "{what} must rebuild the displayed list"
            );
        }
    }

    #[test]
    fn a_padded_search_term_rebuilds_against_the_trimmed_one() {
        let (r, rows) = linear();
        let mut base = inputs_for(&[r]);
        base.filter = "needle ".to_owned();
        let held = derive(4, &base, rows.clone(), Vec::new());
        let mut padded = inputs_for(&[root("alpha")]);
        padded.filter = "needle".to_owned();
        assert!(needs_rebuild(Some(&held), 4, &padded));
    }

    #[test]
    fn a_moved_scope_root_rebuilds_inside_a_path_scope() {
        let (r, rows) = linear();
        let mut scoped = inputs_for(std::slice::from_ref(&r));
        scoped.path_scope = Some(PathBuf::from("f.txt"));
        scoped.scope_root = Some(r);
        let held = derive(4, &scoped, rows, Vec::new());
        let moved = LogInputs {
            scope_root: Some(root("beta")),
            ..scoped
        };
        assert!(needs_rebuild(Some(&held), 4, &moved));
    }
}

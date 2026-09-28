//! The log pane's **derived display window** (log-view-scaling 04): the union
//! of the visible roots' loaded windows, sorted, live-filtered, and unioned
//! with the pickaxe hits — computed when the window it was derived from
//! changes, not once per frame.
//!
//! Plain data owned by the application layer, the same split as
//! [`crate::diff_model`] (ADR-0014): nothing here references egui, and the
//! value never borrows the caches. A borrow would have to be re-taken every
//! frame, which is the cost this value exists to remove — the held window owns
//! its rows so it can be indexed (and, from log-view-scaling 05, paged) from
//! outside the frame that built it.
//!
//! The lane walk lives here beside it for the same reason it stays pure: lane
//! colours are a property of the loaded window, so they are assigned when the
//! window changes and held with it. Its input is the WHOLE loaded window —
//! never the text-filtered subset, and never the drawn rows — because a
//! commit's colour must not change as it crosses the viewport or as a search
//! term narrows.

use std::collections::HashMap;
use std::sync::Arc;

use turbogit_domain::model::{Commit, CommitId, RootId};

use crate::state::AppState;

// --- the UI-side inputs a window is derived from --------------------------------

/// Everything outside the caches that the displayed rows depend on: which
/// roots are in view, which scope is active, and what the live search box
/// says. This is the *fingerprint* half of the invalidation rule — the caches'
/// own [`crate::root_caches::RootCaches::revision`] is the other half — and it
/// is why a window can be held across frames: two derivations with the same
/// revision and the same inputs have the same answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogInputs {
    /// The roots filter (`ui.log_root_filter`): one root, or all of them.
    pub root_filter: Option<RootId>,
    /// The active path scope (`ui.log_path_scope`).
    pub path_scope: Option<std::path::PathBuf>,
    /// The active ref scope (`ui.log_ref_scope`), which names its own root.
    pub ref_scope: Option<(RootId, String)>,
    /// The live search text (`ui.log_filter`) verbatim. The pickaxe cache is
    /// keyed by its TRIMMED form, so a difference of leading or trailing
    /// whitespace must rebuild; the filter itself matches case-insensitively.
    pub filter: String,
    /// The roots the graph unions, in union order — [`visible_roots`].
    pub roots: Vec<RootId>,
    /// The scope's repository (`selected_root`), carried ONLY under a path
    /// scope: a path scope's listing is read for the selected root, while a
    /// ref scope names its own root and an unscoped view reads every visible
    /// root. Carrying the selection unconditionally would rebuild the window
    /// on every row click, which changes nothing the window holds.
    pub scope_root: Option<RootId>,
}

impl LogInputs {
    /// The inputs as the log pane's state stands right now.
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

/// The roots whose history is displayed: the roots filter's one root, or every
/// registered root. The union, the pickaxe union, and the pane's own ref-fill
/// all read the same list from here, so "which roots are in view" has one
/// definition.
pub fn visible_roots(state: &AppState) -> Vec<RootId> {
    match &state.ui.log_root_filter {
        Some(id) => vec![id.clone()],
        None => state.multi.roots.iter().map(|r| r.id.clone()).collect(),
    }
}

// --- the held value -------------------------------------------------------------

/// One row of the display window: the commit, owned. `Arc` so a rebuild copies
/// each commit once and every access afterwards is a pointer, never a clone.
pub type LogRow = Arc<Commit>;

/// The display window the log pane paints from: an owned, indexable row
/// sequence plus the lane index those rows were walked for. Built once per
/// change to the loaded window or to [`LogInputs`], and held on
/// [`AppState`] (ADR-0014's "compute once per content, paint per frame" split).
pub struct LogDisplay {
    rows: Vec<LogRow>,
    lanes: HashMap<CommitId, usize>,
    revision: u64,
    inputs: LogInputs,
}

impl LogDisplay {
    /// How many rows the whole window holds — the number the "N shown" line
    /// reports, and the row count a paged list pages over
    /// (log-view-scaling 05). Not the number of rows currently drawn.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the window holds no rows at all.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The row at `index`, or `None` past the end. Indexable rather than
    /// panicking because the row count moves under any caller that has not
    /// just re-synced.
    pub fn row(&self, index: usize) -> Option<&Commit> {
        self.rows.get(index).map(|row| row.as_ref())
    }

    /// The whole row sequence, in display order.
    pub fn rows(&self) -> &[LogRow] {
        &self.rows
    }

    /// The lane a commit's graph node is drawn in, or `None` for a commit the
    /// walk never saw (one outside the loaded window).
    pub fn lane(&self, id: &str) -> Option<usize> {
        self.lanes.get(id).copied()
    }

    /// The whole lane index, as the row painter reads it.
    pub fn lanes(&self) -> &HashMap<CommitId, usize> {
        &self.lanes
    }
}

/// Whether the held window still answers the question in front of it. One
/// pure predicate, over the two things that can change the answer: the caches'
/// revision (a batch landed, a window settled, a scope refilled, anything was
/// invalidated) and the UI-side fingerprint.
///
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

/// The display window for one loaded union and one pickaxe union.
///
/// `loaded` is every visible root's window for the active scope, in cache
/// order and unfiltered; `hits` is the pickaxe rows the search cache holds.
/// The lane walk runs over `loaded` WHOLE — before the live filter and before
/// the hits are unioned in — so a commit's colour is a fact about the loaded
/// window rather than about what the developer happens to be looking at.
pub fn derive(
    revision: u64,
    inputs: &LogInputs,
    loaded: Vec<LogRow>,
    hits: Vec<LogRow>,
) -> LogDisplay {
    // Lane colours are assigned over the whole loaded window, not over the
    // rows that survive the filter: a commit keeps the colour the walk gave it
    // when its batch landed, whether or not a search term is narrowing the list
    // (spec: "Lane colours are a property of the loaded window").
    let lanes = assign_colors(&loaded);
    let mut rows = loaded;
    rows.sort_by(|a, b| b.time.cmp(&a.time).then(a.id.cmp(&b.id)));
    if !inputs.filter.is_empty() {
        let filter = inputs.filter.to_lowercase();
        rows.retain(|c| {
            c.message.to_lowercase().contains(&filter)
                || c.id.to_lowercase().contains(&filter)
                || c.author.name.to_lowercase().contains(&filter)
        });
        // Code-change hits (issue 17): a commit whose content changed the
        // query's count shows even when message/hash/author do not match. A hit
        // already in the window is not added twice; a hit the filter just hid
        // comes back, so the union is over ids and not over the filtered
        // subset. Skipped inside a path scope, which may only ever list commits
        // touching the scoped path.
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

/// Assign each commit a lane index using a lightweight DAG walk, so the list
/// reads like a commit graph (Epic D1). Newest-first input assumed.
///
/// Two properties this shape is load-bearing for:
/// - **It runs over the whole loaded window.** Its input is the owned row
///   sequence ([`LogRow`], not the old borrowed `&[&Commit]` union) and it is
///   called from [`derive`] BEFORE the live text filter and before the pickaxe
///   hits are unioned in. Scoping it to the viewport would make a commit change
///   colour as it scrolled through the list; scoping it to the filtered subset
///   would make a commit change colour as a search term was typed. Neither is a
///   colour the developer can learn.
/// - **It is a pure function over a commit sequence.** No state, no renderer,
///   no window: the rules below are testable on their own, which is where they
///   are pinned.
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
            // pick the lane's color, allocating a new one if needed
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

/// The loaded union and the pickaxe union, read out of the caches under the
/// active scope. Owned, and read only on a rebuild: a frame that reuses the
/// held window copies no commit at all.
///
/// The scope rules are the log pane's, unchanged: a ref scope shows only its own
/// ref's listing and hides every other root, a path scope shows the selected
/// root's scoped listing and nothing else, and an unscoped view unions every
/// visible root's window.
pub fn sources(state: &AppState, inputs: &LogInputs) -> (Vec<LogRow>, Vec<LogRow>) {
    let loaded: Vec<LogRow> = if let Some((root, ref_name)) = &state.ui.log_ref_scope {
        owned(state.caches.ref_log(root, ref_name).unwrap_or_default())
    } else if state.ui.log_path_scope.is_some() {
        match &state.selected_root {
            Some(root) => scoped_window(state, root),
            None => Vec::new(),
        }
    } else {
        // The unscoped union: every visible root's window, in the visible roots'
        // order, each root's rows in its own (newest-first) order. The sort that
        // makes the union newest-first across roots happens in [`derive`].
        union(&inputs.roots, |id| state.caches.log(id))
    };
    // The pickaxe union is skipped inside a path scope: the scoped view may only
    // ever list commits touching the scoped path, and the pickaxe cache is not
    // path-scoped. The pane's data-ensure step never fills it there either. The
    // cache is keyed by the trimmed RAW query (what the engine was given) — not
    // the lowercased text the filter matches with.
    let hits: Vec<LogRow> = match (
        state.ui.log_path_scope.is_none(),
        state.ui.log_filter.trim(),
    ) {
        (true, query) if !query.is_empty() => {
            union(&inputs.roots, |id| state.caches.search_log(id, query))
        }
        _ => Vec::new(),
    };
    (loaded, hits)
}

/// Every listed root's window for one reading, as owned rows. A root with
/// nothing cached contributes nothing, which is what makes the first frame of a
/// cold log an empty window rather than a miss.
fn union<'a>(roots: &[RootId], read: impl Fn(&RootId) -> Option<&'a [Commit]>) -> Vec<LogRow> {
    roots
        .iter()
        .filter_map(&read)
        .flat_map(|window| window.iter().cloned().map(Arc::new))
        .collect()
}

/// One scope's window for `root` as owned rows: the path scope's listing when one
/// is active and cached, else the root's unscoped window.
fn scoped_window(state: &AppState, root: &RootId) -> Vec<LogRow> {
    if let Some(path) = &state.ui.log_path_scope
        && let Some(commits) = state.caches.path_log(root, path)
    {
        return owned(commits);
    }
    owned(state.caches.log(root).unwrap_or_default())
}

/// A cached window as owned rows. One deep copy per commit, once per rebuild —
/// the price of holding the rows instead of borrowing them, and the reason the
/// read only happens when something has actually changed.
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

    /// A straight three-commit history on one root, newest first: `a` is HEAD,
    /// `b` its child, `c` the root commit.
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

    // --- the walk's own rules --------------------------------------------------

    /// A linear history reads as one lane: each commit hands its own lane to
    /// its single parent, so the whole walk allocates one colour.
    #[test]
    fn a_linear_history_walks_in_one_lane() {
        let (_r, rows) = linear();
        let lanes = assign_colors(&rows);
        assert_eq!(lanes.len(), 3, "a commit and both its parents are coloured");
        assert_eq!(lanes["a"], 0);
        assert_eq!(lanes["b"], 0, "the first parent inherits the lane");
        assert_eq!(lanes["c"], 0);
    }

    /// A merge's SECOND parent opens its own lane and keeps it: the lane a
    /// side branch runs in is the whole reason the graph is readable.
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
        // Two lanes carry `base` at once by the time the walk reaches it, and it
        // converges on the FIRST one — so the branches meet at a colour the
        // reader has already seen, rather than the root commit changing colour
        // between the two lines that arrive at it.
        assert_eq!(lanes["base"], 0);
    }

    /// Appending a batch must not renumber the rows already held: the walk runs
    /// over the whole window, so growing it can only add lanes, never move one.
    /// This is the property that makes recomputing on batch-append safe at all.
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

    /// A commit the window does not hold — a parent below the oldest loaded row
    /// — holds a lane in the walk but takes no colour of its own until a batch
    /// brings it in. It is a default-coloured node until then, which is why the
    /// walk has to run over the whole window rather than the drawn rows: the
    /// moment its batch lands, it is coloured like everything else.
    #[test]
    fn a_parent_below_the_loaded_window_takes_no_colour_of_its_own() {
        let r = root("alpha");
        let lanes = assign_colors(&[commit(&r, "a", &["b"], 300)]);
        assert_eq!(lanes["a"], 0);
        assert_eq!(lanes.get("b"), None, "an unwalked commit has no colour");
    }

    /// The walk is a function of the sequence alone: the same rows in, the
    /// same lanes out, twice.
    #[test]
    fn the_walk_is_pure_over_its_input() {
        let (_r, rows) = linear();
        assert_eq!(assign_colors(&rows), assign_colors(&rows));
    }

    // --- the held window -------------------------------------------------------

    /// The rows are an owned, indexable sequence: a row reached by index is
    /// the same commit the walk saw, and the count is the whole window's.
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

    /// An empty window is a settled answer, not a missing one: the pane paints
    /// "no commits match" off `is_empty`, so the two must not be confusable.
    #[test]
    fn an_empty_window_is_empty_rather_than_absent() {
        let window = derive(0, &LogInputs::default(), Vec::new(), Vec::new());
        assert_eq!(window.len(), 0);
        assert!(window.is_empty());
    }

    /// Newest first, with the id breaking ties in ascending order: two commits
    /// with the same timestamp keep a stable, reproducible order.
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

    /// The lane index is held WITH the window, and a row's own lane is
    /// readable from the window rather than recomputed by the painter.
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

    /// The whole point of the ticket: the walk runs over the WHOLE loaded
    /// window, so a commit the live filter hides keeps the lane the walk gave
    /// it. Scoping the walk to the filtered subset would renumber the lanes
    /// every time a search term narrowed the list.
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

    // --- the live filter -------------------------------------------------------

    /// The live filter matches message, hash or author, case-insensitively, and
    /// an empty filter is not a filter.
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
        // No filter at all is every row, in the union's own order.
        let window = derive(0, &inputs_for(&[root("alpha")]), rows, Vec::new());
        assert_eq!(window.len(), 3);
    }

    // --- the pickaxe union -----------------------------------------------------

    /// A pickaxe hit the metadata filter would miss is unioned in; a hit already
    /// in the window is not added twice, and a hit the filter hid comes back.
    /// The union is re-sorted afterwards so a hit from further back cannot land
    /// out of order.
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

    /// Inside a path scope the pickaxe cache is not consulted at all: the
    /// scoped view may only list commits touching the scoped path.
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

    // --- invalidation ----------------------------------------------------------

    /// Nothing changed, so the held window is still the answer to the question
    /// in front of it. This single `false` is what an idle frame rests on: the
    /// paint path reads the held rows and the held lanes and computes nothing.
    #[test]
    fn a_held_window_with_the_same_revision_and_inputs_is_still_the_answer() {
        let (r, rows) = linear();
        let inputs = inputs_for(&[r]);
        let held = derive(4, &inputs, rows, Vec::new());
        assert!(
            !needs_rebuild(Some(&held), 4, &inputs),
            "a frame that changed nothing must not rebuild"
        );
        // And it says so however the window was reached — a rebuilt handle is
        // the same answer, so a second frame is as quiet as the first.
        assert!(!needs_rebuild(Some(&held), 4, &inputs));
    }

    /// A window that has never been built is always built: the first frame of
    /// the log pane is a rebuild like any other.
    #[test]
    fn a_missing_window_is_always_built() {
        assert!(needs_rebuild(None, 0, &LogInputs::default()));
    }

    /// A moved cache revision rebuilds: a batch landed, a scope settled, a
    /// window said whether history continues, or something was invalidated.
    /// The revision is the caches' own word for "the loaded window moved".
    #[test]
    fn a_moved_cache_revision_rebuilds() {
        let (r, rows) = linear();
        let inputs = inputs_for(&[r]);
        let held = derive(4, &inputs, rows, Vec::new());
        assert!(needs_rebuild(Some(&held), 5, &inputs));
    }

    /// Each of the four UI inputs rebuilds on its own, and each is the whole
    /// story: a batch appended, a scope changed, the roots filter narrowed or
    /// widened, a search term typed.
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

    /// Whitespace is part of the live term: the pickaxe cache is keyed by the
    /// TRIMMED text, so a term that differs only in padding reads a different
    /// cache entry and has to rebuild.
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

    /// The scoped repository is a fingerprint input where it is read from: a
    /// path scope's listing is looked up for the selected root, so moving the
    /// selection inside a scope reads a different window. (Whether the
    /// selection is carried at all is `LogInputs::of`'s rule, checked where a
    /// real `AppState` is in hand.)
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

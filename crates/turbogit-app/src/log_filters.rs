//! The Commits pane's filter bar. IntelliJ's Git Log names are kept where they
//! name the same thing, so someone moving between the two tools does not have to
//! learn a second vocabulary.

use std::collections::{BTreeSet, HashMap};
use turbogit_domain::model::{Branch, BranchKind, RefState, Root, RootId};

pub const LABEL_ALL: &str = "All";
pub const LABEL_SELECT: &str = "Select\u{2026}";
pub const LABEL_BRANCH: &str = "Branch";
pub const LABEL_USER: &str = "User";
pub const LABEL_DATE: &str = "Date";
pub const LABEL_PATHS: &str = "Paths";
pub const LABEL_LAST_DAY: &str = "Last 24 hours";
pub const LABEL_LAST_WEEK: &str = "Last 7 days";
pub const LABEL_SELECT_PERIOD: &str = "Select Period";
pub const LABEL_ROOTS: &str = "Roots";
pub const LABEL_AFTER: &str = "After";
pub const LABEL_BEFORE: &str = "Before";

/// A picked period carries its own timestamps. Resolving "Last 24 hours" at
/// click time keeps the cache key stable across frames and keeps reads pure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DateFilter {
    #[default]
    All,
    Last24Hours {
        since: i64,
    },
    Last7Days {
        since: i64,
    },
    Period {
        after: i64,
        before: i64,
    },
}

impl DateFilter {
    pub fn is_all(self) -> bool {
        matches!(self, Self::All)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => LABEL_ALL,
            Self::Last24Hours { .. } => LABEL_LAST_DAY,
            Self::Last7Days { .. } => LABEL_LAST_WEEK,
            Self::Period { .. } => LABEL_SELECT_PERIOD,
        }
    }

    pub fn bounds(self) -> (Option<i64>, Option<i64>) {
        match self {
            Self::All => (None, None),
            Self::Last24Hours { since } | Self::Last7Days { since } => (Some(since), None),
            Self::Period { after, before } => (Some(after), Some(before)),
        }
    }
}

/// Parse one `YYYY-MM-DD` bound into unix seconds at the UTC start of that day.
/// A blank bound means no bound, which is how IntelliJ's Select Period dialog
/// lets you ask only for "after" or only for "until". Malformed text is an
/// error, not a blank, so the popup can say which field is wrong.
pub fn parse_day(text: &str) -> Result<Option<i64>, &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let date =
        chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| "expected YYYY-MM-DD")?;
    Ok(Some(
        date.and_hms_opt(0, 0, 0)
            .ok_or("expected YYYY-MM-DD")?
            .and_utc()
            .timestamp(),
    ))
}

/// One band of the `Branch` dropdown's ref list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RefGroup {
    Local,
    Remote,
    Tag,
}

/// A band's header copy and the polarity it starts in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupSpec {
    pub title: &'static str,
    pub starts_open: bool,
}

/// The single owner of band paint order, default polarity and header copy.
pub const GROUP_TABLE: [(RefGroup, GroupSpec); 3] = [
    (
        RefGroup::Local,
        GroupSpec {
            title: "Local branches",
            starts_open: true,
        },
    ),
    (
        RefGroup::Remote,
        GroupSpec {
            title: "Remote branches",
            starts_open: false,
        },
    ),
    (
        RefGroup::Tag,
        GroupSpec {
            title: "Tag refs",
            starts_open: false,
        },
    ),
];

/// The checked set of repositories the Log reads. `All` is the rest state and
/// adopts a later-attached repository; collapse-on-write keeps those the same
/// thing: ticking every box lands back on `All`, which is what makes a
/// later-attached repository join.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RootFilter {
    #[default]
    All,
    Only(Vec<RootId>),
}

impl RootFilter {
    pub fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    /// Whether the checkbox for `id` paints ticked.
    pub fn carries(&self, id: &RootId) -> bool {
        match self {
            Self::All => true,
            Self::Only(ids) => ids.contains(id),
        }
    }

    /// The selected repositories in registration order, pruned of any id that
    /// is no longer registered.
    pub fn selected<'a>(&self, registered: &'a [Root]) -> Vec<&'a Root> {
        match self {
            Self::All => registered.iter().collect(),
            Self::Only(ids) => registered
                .iter()
                .filter(|root| ids.contains(&root.id))
                .collect(),
        }
    }

    pub fn toggle(&mut self, id: &RootId, registered: &[Root]) {
        let mut ids: Vec<RootId> = self
            .selected(registered)
            .into_iter()
            .map(|root| root.id.clone())
            .collect();
        match ids.iter().position(|candidate| candidate == id) {
            Some(pos) => {
                ids.remove(pos);
            }
            None => ids.push(id.clone()),
        }
        // The pruned set has no stale members, so its length answers "is this
        // everything" and a full set is written as `All`, not as `Only`.
        *self = if ids.len() == registered.len() {
            Self::All
        } else {
            Self::Only(ids)
        };
    }

    pub fn select_all(&mut self) {
        *self = Self::All;
    }
}

/// One band's fold, remembered per surface for the session and never durable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fold {
    Open,
    Closed,
}

pub type RefFolds = HashMap<RefGroup, Fold>;

/// One kind band. `revs` is painted AND handed to git. No root axis, no
/// per-row label: a name picked from a band is read in every selected
/// repository that carries it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefBand {
    pub group: RefGroup,
    pub title: &'static str,
    pub open: bool,
    pub revs: Vec<String>,
}

/// The tag inventory the band model reads, in the spelling `RootCaches` stores.
pub type RefTags = HashMap<RootId, Vec<(String, RefState)>>;

/// The rev a listing row must hand a read.
///
/// A remote-tracking `Branch` arrives with a BARE `name` and the remote in its own field
/// (`crates/turbogit-engine/src/cli.rs:681-692`, and `git2_exec.rs:442-450` delegates the
/// listing to that parser), so the qualified spelling has to be rebuilt here. It is not a
/// nicety: `crates/turbogit-engine/tests/rev_spellings.rs` measures `origin/solo` resolving
/// on both adapters with no local twin while a bare `solo` errors on both, because a
/// revision argument resolves against ref names and git's branch-name shorthand is a
/// checkout rule that never reaches it. A `Remote` row with no remote keeps its bare name,
/// so the read fails loudly instead of inventing one.
pub fn rev_of(branch: &Branch) -> String {
    if branch.kind != BranchKind::Remote {
        return branch.name.clone();
    }
    match &branch.remote {
        // `branches_tree.rs:206-211` tolerates both spellings for the same row, so a
        // hand-seeded qualified name must not be doubled here.
        Some(remote) if branch.name.starts_with(&format!("{remote}/")) => branch.name.clone(),
        Some(remote) => format!("{remote}/{}", branch.name),
        None => branch.name.clone(),
    }
}

/// The one answer to "does this repository have this rev": a local branch
/// name, a `rev_of`-qualified remote name, or a tag name, by exact string
/// equality. No git call and no DWIM — a rev git would resolve through a
/// remote-tracking default is not carried here.
pub fn carries_ref(root: &Root, tags: &RefTags, rev: &str) -> bool {
    root.branches.iter().any(|b| rev_of(b) == rev)
        || tags
            .get(&root.id)
            .is_some_and(|list| list.iter().any(|(name, _)| name == rev))
}

/// The bands the dropdown paints: the kinds of the SELECTED repositories only,
/// in [`GROUP_TABLE`] order, unioned and deduped by rev, with empty kinds
/// contributing no band at all.
pub fn ref_bands(selected: &[&Root], tags: &RefTags, folds: &RefFolds) -> Vec<RefBand> {
    let mut bands = Vec::new();
    for (group, spec) in GROUP_TABLE {
        // A set is the dedupe and the sort in one; scanning a `Vec` for each rev made a
        // band with thousands of rows quadratic in its own size.
        let mut revs: BTreeSet<String> = BTreeSet::new();
        for root in selected {
            revs.extend(group_revs(root, group, tags));
        }
        if revs.is_empty() {
            continue;
        }
        bands.push(RefBand {
            group,
            title: spec.title,
            open: fold_of(folds, group, spec.starts_open),
            revs: revs.into_iter().collect(),
        });
    }
    bands
}

fn group_revs(root: &Root, group: RefGroup, tags: &RefTags) -> Vec<String> {
    match group {
        RefGroup::Local => root
            .branches
            .iter()
            .filter(|b| b.kind == BranchKind::Local)
            .map(|b| b.name.clone())
            .collect(),
        RefGroup::Remote => root
            .branches
            .iter()
            .filter(|b| b.kind == BranchKind::Remote)
            .map(rev_of)
            .collect(),
        RefGroup::Tag => tags
            .get(&root.id)
            .map(|list| list.iter().map(|(name, _)| name.clone()).collect())
            .unwrap_or_default(),
    }
}

/// The effective polarity: the recorded fold when there is one, the table's when there
/// is not.
fn fold_of(folds: &RefFolds, group: RefGroup, starts_open: bool) -> bool {
    match folds.get(&group) {
        Some(fold) => *fold == Fold::Open,
        None => starts_open,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_bound_is_open_and_a_bad_one_is_an_error() {
        assert_eq!(parse_day(""), Ok(None));
        assert_eq!(parse_day("   "), Ok(None));
        assert!(parse_day("06/11/2026").is_err());
    }

    #[test]
    fn a_day_is_read_at_utc_midnight() {
        assert_eq!(
            parse_day("2026-11-06"),
            Ok(Some(
                chrono::NaiveDate::from_ymd_opt(2026, 11, 6)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp()
            ))
        );
    }

    #[test]
    fn each_preset_carries_the_bound_git_will_read() {
        assert_eq!(DateFilter::All.bounds(), (None, None));
        assert_eq!(
            DateFilter::Last24Hours { since: 10 }.bounds(),
            (Some(10), None)
        );
        assert_eq!(
            DateFilter::Period {
                after: 10,
                before: 20
            }
            .bounds(),
            (Some(10), Some(20))
        );
    }
}

//! The keyed read — the diff surface's only way to reach cached Git data
//! (ADR-0021).
//!
//! One rule, held here, with no storage of its own: the values stay in the
//! [`crate::state::UiState`] fields they live in. A caller hands over a
//! *target* and gets back a *verdict*; the key is derived inside, compared
//! inside, and is not a thing a caller can name, format, or get subtly
//! differently wrong. Asking is also admitting — `read` dispatches at most one
//! fetch per frame and never a second while one is in flight — so no caller can
//! forget a step of the rule, and `peek` answers without admitting, which is
//! what a reader like the granular preview needs.
//!
//! Three kinds cross this interface: the diff's patch text, blame's lines, and
//! one pane's bytes — the first two answering from a single entry, the third
//! from a keyed map it keeps. Each states its target, its key derivation, the
//! field that holds its value, its store policy, and the fetch it runs on the
//! pump; the admission, lateness and failure rules are written once, here.

use std::path::PathBuf;
use std::sync::Arc;

use turbogit_domain::error::TgResult;
use turbogit_domain::model::{BlameLine, ChangeStatus, DiffOpts, DiffPolicies, Patch, RootId};
use turbogit_services::diff_engine;

use crate::diff_data::{PaneEntry, PaneSide};
use crate::diff_load::{PaneSideRequest, SideSpec};
use crate::diff_model::{DiffValue, FileMeta};
use crate::events::{AppEvent, FetchedBlob};
use crate::granular;
use crate::root_caches::StatsView;
use crate::state::{AppState, BlameTarget, UiState};

/// What one read answered.
///
/// `Fresh` carries an owned handle, not a borrow: one refcount bump per frame
/// and none per row, so a painter keeps `&mut AppState` free to write the hunk
/// cursor, the collapse set and its row state while holding the value
/// (ADR-0021, closing what ADR-0020 deferred).
pub enum Read<T> {
    /// The value for exactly this target.
    Fresh(T),
    /// Nothing to show yet, and a fetch is on the pump — or was, this frame.
    Waiting,
    /// The answer is settled and there is nothing in it.
    Empty,
    /// This target's fetch failed. The message names what went wrong, and the
    /// verdict only ever reaches the target that failed, never whatever else
    /// the surface happens to be showing.
    Failed(String),
}

/// Which read a kind is. Each one owns a slot: the key it has a fetch in
/// flight for, and the failure it last answered for a key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Diff,
    Blame,
    Pane,
}

/// One kind's rule state. Neither half is a value — those stay in `UiState`.
#[derive(Default)]
pub(crate) struct Slot {
    in_flight: Option<String>,
    failed: Option<(String, String)>,
}

impl Slot {
    /// Take the slot for `key`, answering whether it was free. One fetch per
    /// kind at a time is the whole of admission.
    fn take(&mut self, key: &str) -> bool {
        if self.in_flight.is_some() {
            return false;
        }
        self.in_flight = Some(key.to_owned());
        true
    }

    /// Free the slot — but only if this answer owns it. A late answer for a
    /// key someone else has since taken must not free their admission.
    fn release(&mut self, key: &str) {
        if self.in_flight.as_deref() == Some(key) {
            self.in_flight = None;
        }
    }

    fn fail(&mut self, key: &str, message: &str) {
        self.failed = Some((key.to_owned(), message.to_owned()));
    }

    fn succeeded(&mut self, key: &str) {
        if self.failed.as_ref().is_some_and(|(k, _)| k == key) {
            self.failed = None;
        }
    }

    /// The message this slot failed for `key`, if it did.
    fn failure(&self, key: &str) -> Option<&str> {
        self.failed
            .as_ref()
            .filter(|(k, _)| k == key)
            .map(|(_, m)| m.as_str())
    }

    fn in_flight(&self) -> Option<&str> {
        self.in_flight.as_deref()
    }
}

/// The read's own state, held beside the values it governs. `refresh` clears
/// it with them: an answer that no longer describes the worktree must not keep
/// reporting itself, and a failure must not outlive the state that caused it.
#[derive(Default)]
pub(crate) struct Reads {
    diff: Slot,
    blame: Slot,
    pane: Slot,
}

impl Reads {
    fn slot(&mut self, kind: Kind) -> &mut Slot {
        match kind {
            Kind::Diff => &mut self.diff,
            Kind::Blame => &mut self.blame,
            Kind::Pane => &mut self.pane,
        }
    }

    fn slot_of(&self, kind: Kind) -> &Slot {
        match kind {
            Kind::Diff => &self.diff,
            Kind::Blame => &self.blame,
            Kind::Pane => &self.pane,
        }
    }

    pub(crate) fn invalidate(&mut self) {
        self.diff = Slot::default();
        self.blame = Slot::default();
        self.pane = Slot::default();
    }

    /// Whether any read still expects an answer — the repaint gate's question
    /// (`ui/shell.rs`), which is allowed to say *a* surface is working but not
    /// which.
    pub(crate) fn any_pending(&self) -> bool {
        [
            self.diff.in_flight.as_ref(),
            self.blame.in_flight.as_ref(),
            self.pane.in_flight.as_ref(),
        ]
        .iter()
        .any(Option::is_some)
    }
}

impl AppState {
    /// Whether a keyed read is still waiting for its answer. The shell keeps
    /// frames coming while one is, so a completion lands without waiting for
    /// unrelated input.
    pub fn read_pending(&self) -> bool {
        self.reads.any_pending()
    }
}

mod private {
    /// Sealed: a read kind can only be written inside this module, which is
    /// what keeps the staleness rule from being re-written at a call site.
    pub trait Sealed {}
}

/// A cached Git value the surfaces read through [`AppState::read`].
pub trait Keyed: private::Sealed + Sized {
    /// The handle `Fresh` carries: owned, and cheap to clone.
    type Value;

    /// The slot this kind's admission and failure live in.
    const KIND: Kind;

    /// The cache key this target derives. Keys never reach a caller.
    fn key(&self) -> String;

    /// The stored value for this target, applying this kind's store policy.
    /// Takes the derived key so a frame asks for it once.
    fn answer(&self, state: &AppState, key: &str) -> Option<Self::Value>;

    /// Whether this kind's storage holds a settled answer for this target that
    /// is empty rather than absent — which is `Empty`, not something to fetch.
    /// Asked before [`Keyed::answer`] because a blank answer is also a stored
    /// one; only kinds whose values can be blank override it.
    fn settled_empty(&self, state: &AppState, key: &str) -> bool {
        let _ = (state, key);
        false
    }

    /// Run this kind's fetch on the pump, taking the slot.
    fn fetch(&self, state: &mut AppState);
}

/// The one settlement body the typed completion events call: a success stores
/// under its key and retires any failure for it; a failure records the error
/// against the key that failed and drops a stored value that describes
/// something else. `stored` names the field, which is the only per-kind
/// difference between the arms this replaces.
fn settle_single<V>(
    state: &mut AppState,
    kind: Kind,
    key: &str,
    result: TgResult<V>,
    stored: impl for<'a> Fn(&'a mut UiState) -> &'a mut Option<(String, V)>,
) {
    state.reads.slot(kind).release(key);
    match result {
        Ok(value) => {
            state.reads.slot(kind).succeeded(key);
            *stored(&mut state.ui) = Some((key.to_owned(), value));
        }
        Err(e) => {
            state.reads.slot(kind).fail(key, &e.to_string());
            let entry = stored(&mut state.ui);
            if entry.as_ref().is_some_and(|(k, _)| k != key) {
                *entry = None;
            }
        }
    }
}

/// Settle one [`AppEvent::DiffReady`].
///
/// The policies travel with the answer rather than being read off the live state:
/// the model is folded under the policy the *request* was made under, so a policy
/// change that landed while the load was in flight cannot store a model under the
/// key of the one it replaced.
pub(crate) fn settle_diff(
    state: &mut AppState,
    key: String,
    policies: DiffPolicies,
    result: TgResult<Patch>,
) {
    settle_single(
        state,
        Kind::Diff,
        &key,
        result.map(|patch| DiffValue::build(patch, policies)),
        |ui| &mut ui.diff_cache,
    );
}

/// Settle one [`AppEvent::BlameReady`].
pub(crate) fn settle_blame(state: &mut AppState, key: String, result: TgResult<Vec<BlameLine>>) {
    settle_single(state, Kind::Blame, &key, result.map(Arc::from), |ui| {
        &mut ui.blame_cache
    });
}

/// Settle one [`AppEvent::FileBytesReady`] — the **keep** policy, deliberately
/// unlike the other two kinds.
///
/// The slot frees only for the key that owns it, so a late answer cannot free
/// another pane's admission; the entry itself is stored whichever pane asked
/// for it, because the map is keyed and that entry will be right when the pane
/// comes back. Two store policies, one interface — do not unify them further.
pub(crate) fn settle_pane(
    state: &mut AppState,
    key: String,
    old: Option<FetchedBlob>,
    new: Option<FetchedBlob>,
) {
    state.reads.slot(Kind::Pane).release(&key);
    let entry = PaneEntry {
        old: old.map(PaneSide::from_blob),
        new: new.map(PaneSide::from_blob),
    };
    state.ui.pane_bytes.store(key, Arc::new(entry));
}

// --- the diff ----------------------------------------------------------------

/// A comparison's patch text for one file: everything the diff surface knows
/// about what it wants, and nothing about how it is keyed.
///
/// `Hash` is for painting: the row painters salt their widget state with the
/// target they are rendering, which needs a value that is stable per diff and
/// different between diffs — not the cache key, which is this module's.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct DiffTarget {
    pub root: PathBuf,
    /// Explicit revision pair, when the target is commit-to-commit rather than
    /// a working-tree comparison.
    pub left: Option<String>,
    pub right: Option<String>,
    /// The viewer's two view policies. Part of what the read is keyed on: they
    /// change the model, and a model built under another policy must never be
    /// served for this target.
    pub policies: DiffPolicies,
    /// The file, or `None` for the whole comparison.
    pub path: Option<PathBuf>,
}

impl DiffTarget {
    pub fn new(
        root: PathBuf,
        left: Option<String>,
        right: Option<String>,
        policies: DiffPolicies,
        path: Option<PathBuf>,
    ) -> Self {
        Self {
            root,
            left,
            right,
            policies,
            path,
        }
    }

    /// The commit preview's one fixed comparison: `HEAD ↔ worktree`.
    ///
    /// Which constructor a call site uses is how a reader tells the two kinds of
    /// target apart, and it is the whole of the pin. This one is the working-tree
    /// preview: it shows every changed line of the file, staged or not, and it is
    /// byte-stable under the only action the pane offers, because staging a line
    /// cannot change what the worktree differs from HEAD by. `new` is an explicit
    /// revision pair — a history-window diff, a commit-to-commit comparison — and
    /// those keep the revisions their caller named; a commit diff must not claim
    /// `HEAD` as its left side.
    ///
    /// In a repository nobody has committed in, this target has no answer and is
    /// not given one. The engine refuses a `HEAD` read on a root with no HEAD
    /// (`cli.rs`, `guard_unborn_head`, with the reason), and `read` hands the
    /// pane `Read::Failed` with that sentence. The alternative, diffing against
    /// the empty tree in HEAD's place, was built and then removed: it reports
    /// every staged line as an addition relative to a baseline the repository
    /// never had, which is a false answer about someone's first files.
    pub fn working_tree(root: PathBuf, policies: DiffPolicies, path: Option<PathBuf>) -> Self {
        Self::new(root, Some("HEAD".to_owned()), None, policies, path)
    }

    /// The sides this read actually diffs, exactly the pair the caller named.
    /// `pane_sides` and `key` both read the pair here, so the patch bytes and
    /// the pane's two sides can never be derived from different assumptions
    /// about what the working tree is.
    ///
    /// The pin is NOT here: it is [`DiffTarget::working_tree`], because a
    /// working-tree target and an explicit pair are different call sites, not
    /// different shapes of one value, and a mapping in this function would also
    /// rewrite commit-to-commit reads. A target with neither side named is still
    /// expressible through `new`, and it still means plain `git diff`,
    /// `index ↔ worktree`.
    fn sides(&self) -> (Option<String>, Option<String>) {
        (self.left.clone(), self.right.clone())
    }

    /// Which root-wide [`StatsView`] this target displays, when a root-wide
    /// read backs it at all.
    ///
    /// The three root views are exactly three pairs of sides, and a target is
    /// one pair: `HEAD ↔ worktree` is the pinned preview (the `Repo` view), a
    /// pair that names nothing is plain `git diff` (the `Local` view), and a
    /// commit-to-commit pair names its own revisions, for which no root read
    /// ever answered. That reports `None`, and a failed root read says
    /// nothing about it. The `Staged` view is not displayable through a target
    /// at all: `git diff --cached` is not a pair of named sides, and the chip
    /// rail that shows it reads the record per view itself.
    pub fn root_view(&self) -> Option<StatsView> {
        match (self.left.as_deref(), self.right.as_deref()) {
            (Some("HEAD"), None) => Some(StatsView::Repo),
            (None, None) => Some(StatsView::Local),
            _ => None,
        }
    }

    /// The two byte sources for one of this target's file sections, deriving
    /// which side is a rev, which is the worktree, and which is missing:
    ///
    /// | sides            | old side   | new side     |
    /// |------------------|------------|--------------|
    /// | no rev named     | index `:0` | worktree fs  |
    /// | explicit l..r    | `<left>`   | `<right>`    |
    /// | half-explicit l  | `<left>`   | worktree fs  |
    ///
    /// The pinned preview takes the third row, because
    /// [`DiffTarget::working_tree`] names `HEAD` and nothing else.
    ///
    /// Index revs use git's stage syntax (`:<n>:<path>`), the same style the
    /// conflict reader uses for `:1`/`:2`/`:3`. New files have no old side;
    /// deleted files no new side. Paths are repo-relative and slash-separated,
    /// because that is the form `git show` takes.
    fn pane_sides(&self, meta: &FileMeta) -> (PaneSideRequest, PaneSideRequest) {
        let (left, right) = self.sides();
        let rel = |p: &Option<String>| p.clone().unwrap_or_default();
        let old = if meta.new_file {
            SideSpec::Missing
        } else {
            match left {
                Some(l) => SideSpec::Rev(l),
                None => SideSpec::Index,
            }
        };
        let new = if meta.deleted_file {
            SideSpec::Missing
        } else {
            match right {
                Some(r) => SideSpec::Rev(r),
                None => SideSpec::Worktree,
            }
        };
        (
            PaneSideRequest {
                root: self.root.clone(),
                spec: old,
                path: rel(&meta.old_path),
            },
            PaneSideRequest {
                root: self.root.clone(),
                spec: new,
                path: rel(&meta.new_path),
            },
        )
    }
}

impl private::Sealed for DiffTarget {}

impl Keyed for DiffTarget {
    type Value = Arc<DiffValue>;

    const KIND: Kind = Kind::Diff;

    fn key(&self) -> String {
        let (left, right) = self.sides();
        format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            self.root, left, right, self.policies.whitespace, self.policies.highlighting, self.path
        )
    }

    fn answer(&self, state: &AppState, key: &str) -> Option<Self::Value> {
        let (cached, value) = state.ui.diff_cache.as_ref()?;
        (cached == key).then(|| Arc::clone(value))
    }

    fn settled_empty(&self, state: &AppState, key: &str) -> bool {
        state
            .ui
            .diff_cache
            .as_ref()
            .is_some_and(|(cached, value)| cached == key && value.is_blank())
    }

    /// Admit one load of this target's patch text, with the per-diff
    /// navigation state that described the outgoing content.
    ///
    /// An untracked preview never reaches `git diff`: its creation diff is
    /// synthesized from the worktree and stored under the same key, with no
    /// worker and no spinner — which is why the slot is taken only on the
    /// dispatch path. A synthesized answer holds it open forever, and the
    /// repaint gate would never stop asking for frames.
    fn fetch(&self, state: &mut AppState) {
        let key = self.key();
        if state.reads.slot_of(Self::KIND).in_flight().is_some() {
            return;
        }
        state.ui.diff_current_hunk = 0;
        // Collapse state (issue 20) describes the outgoing diff's hunks; it
        // dies with them, like the hunk navigation cursor above.
        state.ui.diff_collapsed.clear();

        if let Some(rel) = self.path.as_ref()
            && granular::status_of(state, &RootId(self.root.clone().into()), rel)
                == turbogit_domain::model::ChangeStatus::Unversioned
            && let Some(patch) = crate::diff_load::synthetic_untracked_patch(&self.root, rel)
        {
            state.ui.diff_cache = Some((key, DiffValue::build(patch, self.policies)));
            return;
        }

        state.reads.slot(Self::KIND).take(&key);
        let (left, right) = self.sides();
        // The engine is asked for the plain diff: the viewer's policies are view
        // decisions folded into the model, never flags that change which bytes git
        // answers with, because those bytes are what partial staging composes.
        let opts = DiffOpts {
            left,
            right,
            path: self.path.clone(),
            ..DiffOpts::default()
        };
        let root = self.root.clone();
        let policies = self.policies;
        // Phase L1: when the setting asks for in-process diffs, the load
        // computes the patch with `similar`; `diff_text` itself falls back to
        // the same CLI call whenever the in-process path cannot produce it
        // (multi-file targets, unreadable sides, non-UTF-8 content).
        let in_process = state.settings.in_process_diffs;
        state.pump_keyed_read(move |executor, tx| {
            let res = if in_process {
                diff_engine::patch(executor.as_ref(), &root, &opts)
            } else {
                executor.diff_patch(&root, &opts)
            };
            let _ = tx.send(AppEvent::DiffReady {
                key,
                policies,
                result: res,
            });
        });
    }
}

// --- blame -------------------------------------------------------------------

impl private::Sealed for BlameTarget {}

impl Keyed for BlameTarget {
    /// The blamed lines behind one handle, so the view keeps its rows while the
    /// painter writes state after the scroll pass ends.
    type Value = Arc<[BlameLine]>;

    const KIND: Kind = Kind::Blame;

    /// root | revision | path.
    fn key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.root.0.display(),
            self.rev,
            self.path.display()
        )
    }

    fn answer(&self, state: &AppState, key: &str) -> Option<Self::Value> {
        let (cached, lines) = state.ui.blame_cache.as_ref()?;
        (cached == key).then(|| Arc::clone(lines))
    }

    /// Fetch this target's per-line attribution off the frame path (issue 18);
    /// the answer lands as [`AppEvent::BlameReady`]. A file with nothing to
    /// blame answers `Fresh` with no lines, exactly as the view rendered it
    /// before — blame has no settled-empty state of its own to paint.
    fn fetch(&self, state: &mut AppState) {
        let key = self.key();
        if !state.reads.slot(Self::KIND).take(&key) {
            return;
        }
        let target = self.clone();
        state.pump_keyed_read(move |executor, tx| {
            let res = turbogit_services::history_service::blame(
                executor.as_ref(),
                &target.root.0,
                &target.path,
                Some(&target.rev),
            );
            let _ = tx.send(AppEvent::BlameReady { key, result: res });
        });
    }
}

// --- non-text pane bytes -----------------------------------------------------

/// Which non-text pane of a comparison is wanted. A discriminator, not the
/// model's [`crate::diff_model::PaneKind`]: a text section has no pane of its
/// own, and a caller
/// that wants bytes has already decided which kind it is painting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaneFlavour {
    Binary,
    Image,
}

/// One non-text pane: the comparison whose patch text named the section, the
/// section's own facts, and whether its sides are wanted as pixels or only as
/// lengths for the binary caption.
///
/// The caller hands over these facts; which side is a rev, which is the
/// worktree, and which is missing are derived here, as is the map key the
/// answer is stored under.
#[derive(Clone)]
pub struct PaneTarget {
    pub comparison: DiffTarget,
    pub flavour: PaneFlavour,
    pub file: FileMeta,
    pub decode: bool,
}

impl PaneTarget {
    pub fn new(comparison: DiffTarget, flavour: PaneFlavour, file: FileMeta, decode: bool) -> Self {
        Self {
            comparison,
            flavour,
            file,
            decode,
        }
    }

    /// The identity the UI layer keys its GPU textures by. ADR-0021 records
    /// that the texture cache stays in the UI crate on purpose, and it needs a
    /// string stable per pane; this is that string, derived here so no caller
    /// builds one from parts it should not be holding.
    pub fn texture_tag(&self) -> String {
        let suffix = match self.flavour {
            PaneFlavour::Binary => "#bin",
            PaneFlavour::Image => "#img",
        };
        format!("{}{suffix}", self.comparison.key())
    }
}

impl private::Sealed for PaneTarget {}

impl Keyed for PaneTarget {
    type Value = Arc<PaneEntry>;

    const KIND: Kind = Kind::Pane;

    fn key(&self) -> String {
        self.texture_tag()
    }

    /// The keyed map's own lookup: an entry is wanted for its key alone, which is
    /// what makes this kind's store policy the keep one — `settle_pane` stores
    /// whatever arrives, and ADR-0021 says not to unify the two policies.
    fn answer(&self, state: &AppState, key: &str) -> Option<Self::Value> {
        state.ui.pane_bytes.get(key)
    }

    /// Fetch both sides of the pane off the frame path (spec R8). One load is
    /// in flight across the whole diff surface, not one per pane, so a second
    /// pane asks and waits rather than spending a second worker.
    fn fetch(&self, state: &mut AppState) {
        let key = self.key();
        if !state.reads.slot(Self::KIND).take(&key) {
            return;
        }
        let (old, new) = self.comparison.pane_sides(&self.file);
        let decode = self.decode;
        state.pump_keyed_read(move |executor, tx| {
            let old = crate::diff_load::fetch_side(executor.as_ref(), &old, decode);
            let new = crate::diff_load::fetch_side(executor.as_ref(), &new, decode);
            let _ = tx.send(AppEvent::FileBytesReady { key, old, new });
        });
    }
}

impl AppState {
    /// Ask for a cached Git value, and start getting it if it is not there.
    ///
    /// Answers at most one verdict per call and dispatches at most one fetch
    /// per frame for the kind — a second ask while one is in flight waits
    /// rather than spending a second `git` run. A failure is what the surface
    /// sees until the target changes or a refresh retires it: asking again does
    /// not re-dispatch, which is what kept a failing comparison hammering the
    /// engine once a frame.
    pub fn read<K: Keyed>(&mut self, target: K) -> Read<K::Value> {
        let key = target.key();
        if target.settled_empty(self, &key) {
            return Read::Empty;
        }
        if let Some(value) = target.answer(self, &key) {
            return Read::Fresh(value);
        }
        if let Some(message) = self.reads.slot_of(K::KIND).failure(&key) {
            return Read::Failed(message.to_owned());
        }
        if self.reads.slot_of(K::KIND).in_flight().is_some() {
            return Read::Waiting;
        }
        target.fetch(self);
        Read::Waiting
    }

    /// Answer without admitting: the value for this target if the store already
    /// holds it, and nothing otherwise. Readers — the granular preview and the
    /// palette verbs — must not turn a deliberate silent no-op into spent git
    /// work, so they ask this question instead.
    pub fn peek<K: Keyed>(&self, target: K) -> Option<K::Value> {
        target.answer(self, &target.key())
    }

    /// The product rule for a failed root-wide diff read: when the view this
    /// target displays is recorded as failed for its root, the **whole file
    /// counts as failed to read**. The pane shows the recorded message and
    /// shows no content, no rows, and no control a click could not honour.
    /// This is what closes the window where the per-path read answers while the
    /// root read died: the composition path already declines by name when a
    /// retained section is absent, and a fully rendered diff whose every row
    /// click does nothing is the defect the record exists to prevent.
    ///
    /// Three clauses, each load-bearing. (1) Only a target that displays one
    /// of the three root views is a candidate ([`DiffTarget::root_view`]): a
    /// commit-to-commit diff names its own revisions. (2) An unversioned path
    /// is exempt: its pane never asked git the question that failed (its
    /// answer is synthesized from the file's bytes) and its staging route
    /// composes from that same value, so no control it paints is silently
    /// inert, and the refusal recorded against the root is a refusal of the
    /// `HEAD` comparison that file was never in. (3) The lookup is per view,
    /// never per pane or per root, because that is the key the fill failed
    /// under `RootCaches::diff_failed`.
    pub fn failed_view_message(&self, target: &DiffTarget) -> Option<String> {
        let view = target.root_view()?;
        let root = RootId(target.root.clone().into());
        if target
            .path
            .as_ref()
            .is_some_and(|path| granular::status_of(self, &root, path) == ChangeStatus::Unversioned)
        {
            return None;
        }
        self.caches
            .diff_view_failure(&root, view)
            .map(str::to_owned)
    }

    /// Whether a pane's bytes are still in the store, asked of the store's
    /// owner. The UI layer holds the tags it keys GPU textures by (see
    /// [`PaneTarget::texture_tag`]) and prunes the ones nobody caches any
    /// more — which is the last thing presentation code needs to know about
    /// the pane cache, and it never reads the cache itself (ADR-0021).
    pub fn pane_is_cached(&self, tag: &str) -> bool {
        self.ui.pane_bytes.contains(tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff_load::SideSpec;
    fn target(left: Option<&str>, right: Option<&str>) -> DiffTarget {
        DiffTarget::new(
            PathBuf::from("/repo"),
            left.map(str::to_owned),
            right.map(str::to_owned),
            turbogit_domain::model::DiffPolicies::default(),
            None,
        )
    }

    fn sides(left: Option<&str>, right: Option<&str>, meta: &FileMeta) -> (SideSpec, SideSpec) {
        let (old, new) = target(left, right).pane_sides(meta);
        (old.spec, new.spec)
    }

    /// The pane reads the same pair the patch text describes, which is what
    /// makes the side table the read's business rather than the painter's.
    #[test]
    fn the_working_tree_addresses_index_and_worktree() {
        let plain = FileMeta::default();
        // A target naming neither side is plain `git diff`, index↔worktree. It is
        // not the pinned preview, which names `HEAD`; this row is what `pane_sides`
        // still answers for it.
        assert_eq!(
            sides(None, None, &plain),
            (SideSpec::Index, SideSpec::Worktree)
        );
        // Explicit commit-to-commit targets pass their revs through.
        assert_eq!(
            sides(Some("abc123"), Some("def456"), &plain),
            (
                SideSpec::Rev("abc123".to_owned()),
                SideSpec::Rev("def456".to_owned())
            )
        );
        // One rev named against the worktree is `git diff <rev>`, so only the
        // side it names stops being the index.
        assert_eq!(
            sides(Some("HEAD"), None, &plain),
            (SideSpec::Rev("HEAD".to_owned()), SideSpec::Worktree)
        );
    }

    #[test]
    fn a_new_or_deleted_file_has_one_side_not_two() {
        let added = FileMeta {
            new_file: true,
            ..FileMeta::default()
        };
        let deleted = FileMeta {
            deleted_file: true,
            ..FileMeta::default()
        };
        assert_eq!(sides(None, None, &added).0, SideSpec::Missing);
        assert_eq!(sides(None, None, &deleted).1, SideSpec::Missing);
    }

    /// Paths arrive repo-relative and slash-separated because the patch value
    /// holds them that way — there is no prefix left here for a caller to
    /// strip, which is what `git show` wants.
    #[test]
    fn pane_paths_are_repo_relative_because_the_value_says_so() {
        let renamed = crate::diff_model::files_from(&turbogit_engine_api::patch::parse_patch(
            "diff --git a/old/dir/Art.png b/new/dir/Art.png\n\
             similarity index 100%\n\
             rename from old/dir/Art.png\n\
             rename to new/dir/Art.png\n",
        ))
        .into_iter()
        .next()
        .expect("one renamed section");
        let (old, new) = target(None, None).pane_sides(&renamed);
        assert_eq!(
            (old.path.as_str(), new.path.as_str()),
            ("old/dir/Art.png", "new/dir/Art.png")
        );
    }

    /// Each pane flavour addresses its own entry, and a different pair of sides
    /// is a different entry — the target is the only thing either side of the
    /// interface gets to address by.
    #[test]
    fn a_pane_is_addressed_per_flavour_and_per_pair_of_sides() {
        let file = FileMeta::default();
        let working_tree = target(None, None);
        let binary = PaneTarget::new(
            working_tree.clone(),
            PaneFlavour::Binary,
            file.clone(),
            false,
        );
        let image = PaneTarget::new(working_tree.clone(), PaneFlavour::Image, file.clone(), true);
        assert_eq!(
            binary.key(),
            binary.texture_tag(),
            "the tag the UI addresses its textures by is the key the read stores under"
        );
        assert_eq!(image.key(), image.texture_tag());
        assert_ne!(binary.key(), image.key());
        let commit_pair = target(Some("HEAD~1"), Some("HEAD"));
        assert_ne!(
            binary.key(),
            PaneTarget::new(commit_pair, PaneFlavour::Binary, file, false).key(),
            "another pair of sides under the same file is another entry"
        );
    }

    /// The axis is gone from the key, so one file under one policy has exactly
    /// one working-tree key: nothing a viewer can do to a file without changing
    /// it can produce a second entry for it.
    #[test]
    fn the_working_tree_key_names_no_axis() {
        let working_tree = target(None, None);
        assert!(
            !working_tree.key().contains("staged"),
            "the removed axis component must not survive in the key: {}",
            working_tree.key()
        );
        assert_eq!(working_tree.key(), target(None, None).key());
        assert_ne!(
            working_tree.key(),
            target(Some("HEAD~1"), Some("HEAD")).key()
        );
        let other_file = DiffTarget::new(
            PathBuf::from("/repo"),
            None,
            None,
            turbogit_domain::model::DiffPolicies::default(),
            Some(PathBuf::from("other.txt")),
        );
        assert_ne!(working_tree.key(), other_file.key());
    }

    // --- lateness, at the internal scripted seam (ADR-0021) -------------------
    //
    // Everything below runs against `FakeExecutor` and a temp directory, so no
    // git binary is involved: holding a dispatch is the only way to make an
    // answer arrive late or out of order, and lateness is the only window in
    // which the staleness and ownership rules below mean anything.

    use std::path::Path;

    use turbogit_domain::model::RootId;
    use turbogit_engine::fake::FakeExecutor;
    use turbogit_engine_api::GitExecutor;

    /// A 2×3 png called `name`, on disk for the worktree side and staged in the
    /// fake for the rev side, so every pane below has two resolvable sides
    /// without a git binary.
    fn seed_image(dir: &Path, name: &str, exec: &FakeExecutor) {
        let img = image::DynamicImage::new_rgb8(2, 3);
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let png = buf.into_inner();
        std::fs::write(dir.join(name), &png).unwrap();
        exec.files_bytes
            .lock()
            .unwrap()
            .insert(PathBuf::from(name), png);
    }

    /// A state with no roots — nothing to snapshot — holding every keyed-read
    /// dispatch instead of running it, against a fake that answers without git.
    fn held_state(dir: &Path, names: &[&str]) -> AppState {
        let exec = std::sync::Arc::new(FakeExecutor::new());
        for name in names {
            seed_image(dir, name, &exec);
        }
        AppState::for_roots(dir, &[]).with_executor(exec as std::sync::Arc<dyn GitExecutor>)
    }

    fn pane(root: &Path, name: &str) -> PaneTarget {
        let comparison = DiffTarget::new(
            root.to_path_buf(),
            None,
            None,
            turbogit_domain::model::DiffPolicies::default(),
            Some(PathBuf::from(name)),
        );
        let file = FileMeta {
            old_path: Some(format!("a/{name}")),
            new_path: Some(format!("b/{name}")),
            ..FileMeta::default()
        };
        PaneTarget::new(comparison, PaneFlavour::Image, file, true)
    }

    /// The pane map's **keep** policy, and the slot-ownership rule that goes
    /// with it: an answer that arrives after its pane stopped being the live
    /// one still stores its entry, and still does not free whoever holds the
    /// slot now.
    #[test]
    fn a_late_pane_answer_never_frees_another_panes_admission() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = held_state(dir.path(), &["art.png", "cover.png"]);
        state.hold_reads();

        let first = pane(dir.path(), "art.png");
        assert!(matches!(state.read(first.clone()), Read::Waiting));
        assert_eq!(state.held_reads(), 1, "one dispatch admitted");

        // A refresh retires the map and the read's slots together, so a second
        // pane can be admitted while the first answer is still outstanding —
        // which is exactly the window a real worker can land in.
        state.refresh(crate::root_caches::Affected::All);
        let second = pane(dir.path(), "cover.png");
        assert!(matches!(state.read(second.clone()), Read::Waiting));
        assert_eq!(state.held_reads(), 2, "the first answer is still owed");

        // Answer the *older* one first. Its release must not touch the slot the
        // newer answer owns.
        state.release_held_read(0);
        state.drain_events();
        assert!(
            matches!(state.read(second.clone()), Read::Waiting),
            "the live pane keeps its admission, and waits for its own answer"
        );
        assert!(
            state.peek(first.clone()).is_some(),
            "the late answer's entry is kept anyway — keyed, and right when \
             that pane comes back"
        );

        state.release_held_read(0);
        state.drain_events();
        assert!(matches!(state.read(second), Read::Fresh(_)));
        assert!(
            matches!(state.read(first), Read::Fresh(_)),
            "and the kept entry is what its own pane is answered with"
        );
    }

    /// One load is in flight across the whole diff surface, not one per pane:
    /// asking for a second pane while the first is outstanding spends no second
    /// worker.
    #[test]
    fn one_pane_load_is_in_flight_across_the_surface() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = held_state(dir.path(), &["art.png", "cover.png"]);
        state.hold_reads();

        let art = pane(dir.path(), "art.png");
        let cover = pane(dir.path(), "cover.png");
        assert!(matches!(state.read(art.clone()), Read::Waiting));
        assert!(matches!(state.read(cover.clone()), Read::Waiting));
        assert_eq!(
            state.held_reads(),
            1,
            "the second pane asked, waited, and dispatched nothing"
        );

        state.release_held_read(0);
        state.drain_events();
        assert!(matches!(state.read(art.clone()), Read::Fresh(_)));
        assert!(matches!(state.read(cover.clone()), Read::Waiting));
        assert_eq!(state.held_reads(), 1, "now it is the other pane's turn");
    }

    /// The single-entry reads do the opposite of the map: an answer for one key
    /// leaves nothing answerable for the key before it. Deliberate — two store
    /// policies, one interface; do not unify them further.
    #[test]
    fn a_single_entry_read_keeps_only_the_answer_it_was_given() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = held_state(dir.path(), &[]);
        state.hold_reads();

        let working_tree = DiffTarget::new(
            dir.path().to_path_buf(),
            None,
            None,
            turbogit_domain::model::DiffPolicies::default(),
            Some(PathBuf::from("art.png")),
        );
        let commit_pair = DiffTarget::new(
            dir.path().to_path_buf(),
            Some("HEAD~1".to_owned()),
            Some("HEAD".to_owned()),
            turbogit_domain::model::DiffPolicies::default(),
            Some(PathBuf::from("art.png")),
        );
        assert_ne!(
            working_tree.key(),
            commit_pair.key(),
            "two targets, one slot"
        );

        assert!(matches!(state.read(working_tree.clone()), Read::Waiting));
        state.release_held_read(0);
        state.drain_events();
        assert!(matches!(state.read(working_tree.clone()), Read::Empty));

        assert!(matches!(state.read(commit_pair.clone()), Read::Waiting));
        state.release_held_read(0);
        state.drain_events();
        assert!(matches!(state.read(commit_pair.clone()), Read::Empty));
        assert!(
            matches!(state.read(working_tree), Read::Waiting),
            "the map would still answer the first; a single entry does not"
        );
    }

    /// Blame's answer for one revision is gone the moment another one's
    /// arrives — the same single-entry rule, on the other kind.
    #[test]
    fn blame_answers_one_target_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = held_state(dir.path(), &[]);
        state.hold_reads();
        let root = RootId(dir.path().to_path_buf().into());
        let at = |rev: &str| BlameTarget {
            root: root.clone(),
            path: PathBuf::from("art.png"),
            rev: rev.to_owned(),
        };

        let first = at("aaa");
        assert!(matches!(state.read(first.clone()), Read::Waiting));
        state.release_held_read(0);
        state.drain_events();
        assert!(matches!(state.read(first.clone()), Read::Fresh(_)));

        let second = at("bbb");
        assert!(matches!(state.read(second.clone()), Read::Waiting));
        state.release_held_read(0);
        state.drain_events();
        assert!(matches!(state.read(second), Read::Fresh(_)));
        assert!(matches!(state.read(first), Read::Waiting));
    }
}

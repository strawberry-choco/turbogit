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
use crate::reveals::SideText;
use crate::root_caches::StatsView;
use crate::state::{AppState, BlameTarget, UiState};

pub enum Read<T> {
    Fresh(T),
    Waiting,
    Empty,
    /// This target's fetch failed. The message names what went wrong, and the
    /// verdict only ever reaches the target that failed, never whatever else
    /// the surface happens to be showing.
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Diff,
    Blame,
    Pane,
    Context,
}

impl Kind {
    /// Kept in step with [`Kind`] by hand; Rust has no variant count. A stale
    /// value fails as an out-of-bounds index at the first read of the missing
    /// kind, never as a wrong answer, which is the trade for the array below.
    pub const COUNT: usize = 4;

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Default)]
pub(crate) struct Slot {
    in_flight: Option<String>,
    failed: Option<(String, String)>,
}

impl Slot {
    fn take(&mut self, key: &str) -> bool {
        if self.in_flight.is_some() {
            return false;
        }
        self.in_flight = Some(key.to_owned());
        true
    }

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

#[derive(Default)]
pub(crate) struct Reads {
    slots: [Slot; Kind::COUNT],
}

impl Reads {
    fn slot(&mut self, kind: Kind) -> &mut Slot {
        &mut self.slots[kind.index()]
    }

    fn slot_of(&self, kind: Kind) -> &Slot {
        &self.slots[kind.index()]
    }

    pub(crate) fn invalidate(&mut self) {
        for slot in &mut self.slots {
            *slot = Slot::default();
        }
    }

    pub(crate) fn any_pending(&self) -> bool {
        self.slots.iter().any(|slot| slot.in_flight().is_some())
    }
}

impl AppState {
    pub fn read_pending(&self) -> bool {
        self.reads.any_pending()
    }
}

mod private {
    pub trait Sealed {}
}

pub trait Keyed: private::Sealed + Sized {
    type Value;

    const KIND: Kind;

    fn key(&self) -> String;

    fn answer(&self, state: &AppState, key: &str) -> Option<Self::Value>;

    /// Whether this kind's storage holds a settled answer for this target that
    /// is empty rather than absent, which is `Empty`, not something to fetch.
    /// Asked before [`Keyed::answer`] because a blank answer is also a stored
    /// one; only kinds whose values can be blank override it.
    fn settled_empty(&self, state: &AppState, key: &str) -> bool {
        let _ = (state, key);
        false
    }

    fn fetch(&self, state: &mut AppState);
}

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

pub(crate) fn settle_blame(state: &mut AppState, key: String, result: TgResult<Vec<BlameLine>>) {
    settle_single(state, Kind::Blame, &key, result.map(Arc::from), |ui| {
        &mut ui.blame_cache
    });
}

/// The landing pad is written first and folded into the reveal state second, so
/// a side that arrives for a comparison nobody is revealing is dropped rather
/// than adopted, and a failed read leaves no file stuck on `Ask::Waiting`.
pub(crate) fn settle_context(
    state: &mut AppState,
    key: String,
    comparison: String,
    file: usize,
    result: TgResult<Option<Arc<SideText>>>,
) {
    settle_single(state, Kind::Context, &key, result, |ui| {
        &mut ui.context_side
    });
    let landed = match &state.ui.context_side {
        Some((landed, side)) if landed == &key => side.clone(),
        _ => None,
    };
    state.ui.reveals.settle(&comparison, file, landed);
}

/// unlike the other two kinds.
///
/// The slot frees only for the key that owns it, so a late answer cannot free
/// another pane's admission; the entry itself is stored whichever pane asked
/// for it, because the map is keyed and that entry will be right when the pane
/// comes back. Two store policies, one interface, do not unify them further.
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

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct DiffTarget {
    pub root: PathBuf,
    pub left: Option<String>,
    pub right: Option<String>,
    pub policies: DiffPolicies,
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

    /// In a repository nobody has committed in, this target has no answer and is
    /// not given one. The engine refuses a `HEAD` read on a root with no HEAD
    /// (`cli.rs`, `guard_unborn_head`, with the reason), and `read` hands the
    /// pane `Read::Failed` with that sentence.
    pub fn working_tree(root: PathBuf, policies: DiffPolicies, path: Option<PathBuf>) -> Self {
        Self::new(root, Some("HEAD".to_owned()), None, policies, path)
    }

    fn sides(&self) -> (Option<String>, Option<String>) {
        (self.left.clone(), self.right.clone())
    }

    pub fn root_view(&self) -> Option<StatsView> {
        match (self.left.as_deref(), self.right.as_deref()) {
            (Some("HEAD"), None) => Some(StatsView::Repo),
            (None, None) => Some(StatsView::Local),
            _ => None,
        }
    }

    pub fn stages(&self) -> bool {
        self.root_view().is_some()
    }

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

    fn fetch(&self, state: &mut AppState) {
        let key = self.key();
        if state.reads.slot_of(Self::KIND).in_flight().is_some() {
            return;
        }
        state.ui.diff_current_hunk = 0;
        state.ui.diff_collapsed.clear();
        // Reveal counts are deliberately not cleared here. A stage is a refresh
        // and a refresh reaches this fetch, so wiping them would close every
        // region the user opened for the reason they staged a line; they are
        // intent keyed to this comparison and clamped by its own gap table, so
        // the answer settling here keeps them valid and `Reveals` resets only
        // when the key changes.

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
        // One context leg: `ContextTarget` reads one side's text and answers both
        // panes with it, and that is only true because nothing here sets
        // `ignore_whitespace`. A whitespace-ignoring diff makes lines the patch
        // calls unchanged differ between the sides, so the day an option reaches
        // this construction, the reveal must read two.
        let opts = DiffOpts {
            left,
            right,
            path: self.path.clone(),
            ..DiffOpts::default()
        };
        let root = self.root.clone();
        let policies = self.policies;
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

impl private::Sealed for BlameTarget {}

impl Keyed for BlameTarget {
    type Value = Arc<[BlameLine]>;

    const KIND: Kind = Kind::Blame;

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

    /// Fetch this target's per-line attribution off the frame path; the answer
    /// lands as [`AppEvent::BlameReady`]. A file with nothing to blame answers
    /// `Fresh` with no lines, exactly as the view rendered it before, blame has
    /// no settled-empty state of its own to paint.
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaneFlavour {
    Binary,
    Image,
}

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

    fn answer(&self, state: &AppState, key: &str) -> Option<Self::Value> {
        state.ui.pane_bytes.get(key)
    }

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

/// One file section's text, the source a revealed context line is read from.
/// A fourth kind rather than a field on the diff's value, so opening a diff
/// costs no git work and only a tap reads a file.
#[derive(Clone, Debug)]
pub struct ContextTarget {
    pub comparison: DiffTarget,
    /// Index into `DiffModel::files`, the same ordinal as `Gap::file`.
    pub file: usize,
}

impl ContextTarget {
    pub fn new(comparison: DiffTarget, file: usize) -> Self {
        Self { comparison, file }
    }
}

impl private::Sealed for ContextTarget {}

impl Keyed for ContextTarget {
    type Value = Option<Arc<SideText>>;

    const KIND: Kind = Kind::Context;

    fn key(&self) -> String {
        format!("{}#ctx{}", self.comparison.key(), self.file)
    }

    fn answer(&self, state: &AppState, key: &str) -> Option<Self::Value> {
        let (cached, side) = state.ui.context_side.as_ref()?;
        (cached == key).then(|| side.clone())
    }

    /// A side that refused to decode is a settled answer of "no text", not a
    /// failure and not something to ask again.
    fn settled_empty(&self, state: &AppState, key: &str) -> bool {
        state
            .ui
            .context_side
            .as_ref()
            .is_some_and(|(cached, side)| cached == key && side.is_none())
    }

    fn fetch(&self, state: &mut AppState) {
        let key = self.key();
        let Some(value) = state.peek(self.comparison.clone()) else {
            return;
        };
        let Some(meta) = value.model.files.get(self.file) else {
            return;
        };
        let (old, new) = self.comparison.pane_sides(meta);
        // One side, the new one when it exists: a gap is unchanged lines, so
        // either side's text answers both panes, and only the new side is at the
        // path a rename moved.
        let side = if matches!(new.spec, SideSpec::Missing) {
            old
        } else {
            new
        };
        if !state.reads.slot(Self::KIND).take(&key) {
            return;
        }
        let file = self.file;
        let comparison = self.comparison.key();
        state.pump_keyed_read(move |executor, tx| {
            let result = crate::diff_load::side_text(executor.as_ref(), &side).map(Arc::new);
            let _ = tx.send(AppEvent::ContextReady {
                key,
                comparison,
                file,
                result: Ok(result),
            });
        });
    }
}

impl AppState {
    /// Answers at most one verdict per call and dispatches at most one fetch
    /// per frame for the kind, a second ask while one is in flight waits
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

    /// holds it, and nothing otherwise. Readers, the granular preview and the
    /// palette verbs, must not turn a deliberate silent no-op into spent git
    /// work, so they ask this question instead.
    pub fn peek<K: Keyed>(&self, target: K) -> Option<K::Value> {
        target.answer(self, &target.key())
    }

    /// The product rule for a failed root-wide diff read: when the view this
    /// target displays is recorded as failed for its root, the **whole file
    /// counts as failed to read**. The pane shows the recorded message and
    /// shows no content, no rows, and no control a click could not honour.
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

    #[test]
    fn the_working_tree_addresses_index_and_worktree() {
        let plain = FileMeta::default();
        assert_eq!(
            sides(None, None, &plain),
            (SideSpec::Index, SideSpec::Worktree)
        );
        assert_eq!(
            sides(Some("abc123"), Some("def456"), &plain),
            (
                SideSpec::Rev("abc123".to_owned()),
                SideSpec::Rev("def456".to_owned())
            )
        );
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

    use std::path::Path;

    use crate::reveals::Edge;
    use turbogit_domain::model::RootId;
    use turbogit_engine::fake::FakeExecutor;
    use turbogit_engine_api::GitExecutor;

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

    #[test]
    fn a_late_pane_answer_never_frees_another_panes_admission() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = held_state(dir.path(), &["art.png", "cover.png"]);
        state.hold_reads();

        let first = pane(dir.path(), "art.png");
        assert!(matches!(state.read(first.clone()), Read::Waiting));
        assert_eq!(state.held_reads(), 1, "one dispatch admitted");

        state.refresh(crate::root_caches::Affected::All);
        let second = pane(dir.path(), "cover.png");
        assert!(matches!(state.read(second.clone()), Read::Waiting));
        assert_eq!(state.held_reads(), 2, "the first answer is still owed");

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

    const TWO_SECTIONS: &str = "diff --git a/one.txt b/one.txt\n\
         index 1111111..2222222 100644\n\
         --- a/one.txt\n\
         +++ b/one.txt\n\
         @@ -4,3 +4,3 @@\n\
          four\n\
         -FIVE\n\
         +five\n\
          six\n\
         diff --git a/two.txt b/two.txt\n\
         index 3333333..4444444 100644\n\
         --- a/two.txt\n\
         +++ b/two.txt\n\
         @@ -2,3 +2,3 @@\n\
          two\n\
         -THREE\n\
         +three\n\
          four\n";

    /// A state holding one settled two-file diff, with the read instrument still
    /// armed so a dispatch is a count rather than a thread.
    fn state_with_a_settled_diff(dir: &Path) -> AppState {
        std::fs::write(dir.join("one.txt"), "four\nfive\nsix\n").unwrap();
        std::fs::write(dir.join("two.txt"), "two\nthree\nfour\n").unwrap();
        let mut fake = FakeExecutor::new();
        fake.diffs
            .insert(dir.to_path_buf(), TWO_SECTIONS.to_owned());
        let mut state = AppState::for_roots(dir, &[])
            .with_executor(std::sync::Arc::new(fake) as std::sync::Arc<dyn GitExecutor>);
        state.hold_reads();
        let comparison = DiffTarget::new(
            dir.to_path_buf(),
            None,
            None,
            turbogit_domain::model::DiffPolicies::default(),
            Some(PathBuf::from("one.txt")),
        );
        assert!(matches!(state.read(comparison), Read::Waiting));
        state.release_held_read(0);
        state.drain_events();
        state.hold_reads();
        state
    }

    fn comparison_at(dir: &Path) -> DiffTarget {
        DiffTarget::new(
            dir.to_path_buf(),
            None,
            None,
            turbogit_domain::model::DiffPolicies::default(),
            Some(PathBuf::from("one.txt")),
        )
    }

    #[test]
    fn a_context_key_names_one_section_of_one_comparison() {
        let comparison = target(None, None);
        let zero = ContextTarget::new(comparison.clone(), 0);
        let one = ContextTarget::new(comparison.clone(), 1);
        assert_eq!(
            zero.key(),
            format!("{}#ctx0", comparison.key()),
            "the comparison's own key, suffixed by the section"
        );
        assert_ne!(zero.key(), one.key());
        assert_ne!(
            zero.key(),
            ContextTarget::new(target(Some("HEAD~1"), Some("HEAD")), 0).key(),
            "another pair of sides under the same section is another read"
        );
    }

    #[test]
    fn revealing_costs_no_git_work_until_a_side_is_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = state_with_a_settled_diff(dir.path());
        let key = comparison_at(dir.path()).key();

        assert!(state.ui.reveals.tap(&key, 0, Edge::Top, Some(9)));
        assert!(state.ui.reveals.admit(&key, 0));
        assert_eq!(state.held_reads(), 0, "intent is state, not a git run");
        assert!(!state.read_pending(), "and nothing is in flight");
        assert_eq!(state.ui.reveals.edges(&key, 0), [9, 0]);
    }

    #[test]
    fn one_context_fetch_is_admitted_and_the_other_section_waits() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = state_with_a_settled_diff(dir.path());
        let comparison = comparison_at(dir.path());
        let key = comparison.key();
        let first = ContextTarget::new(comparison.clone(), 0);
        let second = ContextTarget::new(comparison, 1);

        state.ui.reveals.admit(&key, 0);
        assert!(matches!(state.read(first.clone()), Read::Waiting));
        assert!(
            state.read_pending(),
            "the fourth kind is what a pending reveal reads as"
        );
        state.ui.reveals.admit(&key, 1);
        assert!(matches!(state.read(second.clone()), Read::Waiting));
        assert_eq!(
            state.held_reads(),
            1,
            "the second section asked, waited, and dispatched nothing"
        );

        state.release_held_read(0);
        state.drain_events();
        assert!(matches!(state.read(first), Read::Fresh(Some(_))));
        assert!(matches!(state.read(second.clone()), Read::Waiting));
        assert_eq!(state.held_reads(), 1, "now it is the other one's turn");
    }

    #[test]
    fn peeking_a_context_side_spends_no_git_work() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = state_with_a_settled_diff(dir.path());
        let side = ContextTarget::new(comparison_at(dir.path()), 0);

        assert!(
            state.peek(side.clone()).is_none(),
            "a cold pad says nothing"
        );
        assert_eq!(state.held_reads(), 0, "and costs nothing");

        state.ui.reveals.admit(&side.comparison.key(), 0);
        state.read(side.clone());
        state.release_held_read(0);
        state.drain_events();
        assert!(
            state.peek(side).is_some_and(|side| side.is_some()),
            "the settled text is what peek hands back"
        );
        assert_eq!(state.held_reads(), 0, "and answering it cost no second run");
    }
}

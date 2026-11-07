//! The `Branch` dropdown's band model and the repository selection it reads:
//! kind-grouped flat lists, the carrier test, and `RootFilter`'s set semantics.
//!
//! Every fixture here is a real repository, so remote rows arrive in the BARE spelling
//! the listing parser produces. A fake seeded with `origin/x`-shaped names would certify
//! the wrong payload as correct.

use std::path::{Path, PathBuf};

use test_support::git_seed::{git, repo_with_one_commit, repo_with_origin};
use turbogit_app::log_display::listing_roots;
use turbogit_app::log_filters::{
    Fold, GROUP_TABLE, RefBand, RefGroup, RootFilter, carries_ref, ref_bands, rev_of,
};
use turbogit_app::log_states::{self, LogState};
use turbogit_app::root_caches::Affected;
use turbogit_app::state::AppState;
use turbogit_domain::model::{Branch, BranchKind, Root, RootId, RootStatus};

/// A repository on `main` with `origin/main` and `origin/dev` published, `dev` deleted
/// locally, and one tag: one member in each of the three groups.
fn repo_with_every_group(project: &Path, name: &str) -> PathBuf {
    let work = repo_with_origin(project, name);
    git(&work, &["checkout", "-q", "-b", "dev"]);
    git(&work, &["commit", "-q", "--allow-empty", "-m", "dev work"]);
    git(&work, &["push", "-q", "-u", "origin", "dev"]);
    git(&work, &["checkout", "-q", "main"]);
    git(&work, &["branch", "-D", "dev"]);
    git(&work, &["tag", "v1"]);
    work
}

/// A repository whose only remote row has no local twin, the shape a bare name cannot
/// address.
fn repo_with_a_published_and_deleted_branch(project: &Path, name: &str) -> PathBuf {
    let work = repo_with_origin(project, name);
    git(&work, &["checkout", "-q", "-b", "solo"]);
    git(&work, &["commit", "-q", "--allow-empty", "-m", "solo work"]);
    git(&work, &["push", "-q", "origin", "solo"]);
    git(&work, &["checkout", "-q", "main"]);
    git(&work, &["branch", "-D", "solo"]);
    work
}

/// A repository on `main` with one extra local branch, `topic`.
fn repo_with_topic(project: &Path, name: &str) -> PathBuf {
    let work = repo_with_one_commit(project, name);
    git(&work, &["branch", "topic"]);
    work
}

fn pair(name: &str, seed: fn(&Path, &str) -> PathBuf) -> (tempfile::TempDir, AppState) {
    let tmp = tempfile::tempdir().expect("temp project dir");
    let project = tmp.path().to_path_buf();
    let work = seed(&project, name);
    (tmp, AppState::for_roots(&project, &[work]))
}

fn root_named(state: &AppState, name: &str) -> RootId {
    state
        .multi
        .roots
        .iter()
        .find(|root| root.id.name() == name)
        .unwrap_or_else(|| panic!("no repository named {name} is registered"))
        .id
        .clone()
}

fn remote_row(name: &str, remote: Option<&str>) -> Branch {
    Branch {
        name: name.to_owned(),
        kind: BranchKind::Remote,
        tracking: None,
        favorite: false,
        protected: false,
        exists: true,
        ahead: 0,
        behind: 0,
        gone: false,
        last_touched: None,
        tip: None,
        remote: remote.map(str::to_owned),
    }
}

fn local_branch(name: &str) -> Branch {
    Branch {
        name: name.to_owned(),
        kind: BranchKind::Local,
        tracking: None,
        favorite: false,
        protected: false,
        exists: true,
        ahead: 0,
        behind: 0,
        gone: false,
        last_touched: None,
        tip: None,
        remote: None,
    }
}

fn hand_root(dir: &Path, name: &str, branches: &[&str]) -> Root {
    let path = dir.join(name);
    Root {
        id: RootId(path.clone().into()),
        path,
        remotes: Vec::new(),
        branches: branches.iter().map(|b| local_branch(b)).collect(),
        current_branch: None,
        head: None,
        status: RootStatus::default(),
    }
}

/// The dropdown's own read: warm the tag evidence, then band the selected set.
fn bands(state: &mut AppState) -> Vec<RefBand> {
    state.warm_branch_tags();
    let selected = state.ui.log_roots.selected(&state.multi.roots);
    ref_bands(
        &selected,
        state.caches.tags_by_root(),
        &state.ui.log_ref_folds,
    )
}

fn groups_of(bands: &[RefBand]) -> Vec<RefGroup> {
    bands.iter().map(|band| band.group).collect()
}

fn band(bands: &[RefBand], group: RefGroup) -> Option<&RefBand> {
    bands.iter().find(|b| b.group == group)
}

#[test]
fn bands_are_the_kinds_of_the_selected_repositories_only() {
    let tmp = tempfile::tempdir().expect("temp project dir");
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_every_group(&project, "alpha");
    let beta = repo_with_every_group(&project, "beta");
    git(&beta, &["checkout", "-q", "-b", "betasonly"]);
    git(&beta, &["commit", "-q", "--allow-empty", "-m", "beta work"]);
    let mut state = AppState::for_roots(&project, &[alpha, beta]);

    state.ui.log_roots = RootFilter::Only(vec![root_named(&state, "alpha")]);
    let all = bands(&mut state);
    let mut revs: Vec<String> = all.iter().flat_map(|b| b.revs.clone()).collect();
    revs.sort();
    assert!(
        !revs.iter().any(|rev| rev == "betasonly"),
        "a branch of an UNSELECTED repository contributes no row, saw {revs:?}"
    );
    assert_eq!(
        groups_of(&all),
        vec![RefGroup::Local, RefGroup::Remote, RefGroup::Tag],
        "kinds in GROUP_TABLE order, no repository axis"
    );
    let titles: Vec<&str> = all.iter().map(|b| b.title).collect();
    assert_eq!(
        titles,
        GROUP_TABLE
            .iter()
            .map(|(_, spec)| spec.title)
            .collect::<Vec<&str>>(),
        "a band title is the kind's own copy, never `alpha · Local branches`"
    );
    assert_eq!(
        band(&all, RefGroup::Local).map(|b| &b.revs),
        Some(&vec!["main".to_owned()])
    );
    assert_eq!(
        band(&all, RefGroup::Remote).map(|b| &b.revs),
        Some(&vec!["origin/dev".to_owned(), "origin/main".to_owned()]),
        "a remote row's painted rev is the qualified spelling it hands git"
    );

    state.ui.log_roots = RootFilter::All;
    let unioned = bands(&mut state);
    assert_eq!(
        band(&unioned, RefGroup::Local).map(|b| &b.revs),
        Some(&vec!["betasonly".to_owned(), "main".to_owned()]),
        "with both repositories ticked the union reaches beta's own branch"
    );
}

#[test]
fn a_name_in_three_repositories_is_one_row() {
    let tmp = tempfile::tempdir().expect("temp project dir");
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_one_commit(&project, "alpha");
    let beta = repo_with_one_commit(&project, "beta");
    let gamma = repo_with_one_commit(&project, "gamma");
    let mut state = AppState::for_roots(&project, &[alpha, beta, gamma]);
    let all = bands(&mut state);

    assert_eq!(
        band(&all, RefGroup::Local).map(|b| &b.revs),
        Some(&vec!["main".to_owned()]),
        "one deduped row answers for all three repositories; a pick reads the name \
         in every carrier"
    );
}

#[test]
fn carries_ref_matches_the_qualified_remote_spelling_and_no_bare_twin() {
    let (_tmp, mut state) = pair("alpha", repo_with_a_published_and_deleted_branch);
    state.warm_branch_tags();
    let root = &state.multi.roots[0];
    let tags = state.caches.tags_by_root().clone();

    assert!(
        carries_ref(root, &tags, "origin/solo"),
        "the qualified spelling the listing row hands a read is carried"
    );
    assert!(
        !carries_ref(root, &tags, "solo"),
        "the bare twin a DWIM would resolve is NOT carried: exact equality, no git call"
    );
    assert!(
        carries_ref(root, &tags, "main"),
        "a local branch name is carried"
    );
}

#[test]
fn listing_roots_is_the_selected_set_intersected_with_the_carriers() {
    let tmp = tempfile::tempdir().expect("temp project dir");
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_topic(&project, "alpha");
    let beta = repo_with_one_commit(&project, "beta");
    let mut state = AppState::for_roots(&project, &[alpha, beta]);
    let alpha_id = root_named(&state, "alpha");
    let beta_id = root_named(&state, "beta");

    state.ui.log_ref_scope = Some("topic".to_owned());
    assert_eq!(
        listing_roots(&state),
        vec![alpha_id.clone()],
        "only the carrier is read; beta is never asked for a branch it lacks"
    );

    state.ui.log_roots = RootFilter::Only(vec![beta_id.clone()]);
    assert_eq!(
        listing_roots(&state),
        Vec::<RootId>::new(),
        "a selection whose set holds no carrier reads nothing"
    );

    state.ui.log_roots = RootFilter::Only(vec![alpha_id.clone(), beta_id]);
    assert_eq!(listing_roots(&state), vec![alpha_id]);
}

#[test]
fn an_empty_selection_names_nothing_rather_than_everything() {
    let (_tmp, mut state) = pair("alpha", repo_with_one_commit);
    state.ui.log_roots = RootFilter::Only(vec![]);
    assert_eq!(
        listing_roots(&state),
        Vec::<RootId>::new(),
        "`Only([])` is no repositories, not a rest state"
    );
    assert_eq!(
        log_states::commit_list(&state, true),
        LogState::FilteredOut,
        "and the empty read set answers instead of spinning forever"
    );
}

#[test]
fn ticking_up_to_a_full_set_collapses_it_back_to_all() {
    let dir = tempfile::tempdir().expect("hand roots dir");
    let alpha = hand_root(dir.path(), "alpha", &["main"]);
    let beta = hand_root(dir.path(), "beta", &["main"]);
    let registered = vec![alpha.clone(), beta.clone()];
    let alpha_id = alpha.id.clone();
    let beta_id = beta.id.clone();

    let mut filter = RootFilter::Only(vec![alpha_id.clone()]);
    filter.toggle(&beta_id, &registered);
    assert_eq!(
        filter,
        RootFilter::All,
        "a set that grew to everything is `All` again"
    );

    let mut from_all = RootFilter::All;
    from_all.toggle(&alpha_id, &registered);
    assert_eq!(
        from_all,
        RootFilter::Only(vec![beta_id]),
        "unchecking one box from `All` names the rest explicitly"
    );
}

#[test]
fn a_narrowed_selection_does_not_adopt_a_newly_registered_root() {
    let dir = tempfile::tempdir().expect("hand roots dir");
    let alpha = hand_root(dir.path(), "alpha", &["main"]);
    let beta = hand_root(dir.path(), "beta", &["main"]);
    let gamma = hand_root(dir.path(), "gamma", &["main"]);
    let before = vec![alpha.clone(), beta.clone()];
    let after = vec![alpha, beta, gamma.clone()];

    let narrowed = RootFilter::Only(vec![gamma.id.clone()]);
    assert!(
        narrowed.selected(&before).is_empty(),
        "an id that is not registered yet is pruned at read"
    );
    assert_eq!(narrowed.selected(&after).len(), 1);
    assert_eq!(
        RootFilter::All.selected(&after).len(),
        3,
        "`All` adopts the later-attached repository; a checked set does not"
    );
}

#[test]
fn a_fold_is_recorded_per_group() {
    assert_eq!(
        GROUP_TABLE
            .iter()
            .map(|(group, spec)| (*group, spec.starts_open))
            .collect::<Vec<(RefGroup, bool)>>(),
        vec![
            (RefGroup::Local, true),
            (RefGroup::Remote, false),
            (RefGroup::Tag, false),
        ],
        "Local opens and the other two start closed"
    );

    let tmp = tempfile::tempdir().expect("temp project dir");
    let project = tmp.path().to_path_buf();
    let alpha = repo_with_every_group(&project, "alpha");
    let beta = repo_with_every_group(&project, "beta");
    let mut state = AppState::for_roots(&project, &[alpha, beta]);

    assert_eq!(
        bands(&mut state)
            .iter()
            .map(|b| b.open)
            .collect::<Vec<bool>>(),
        vec![true, false, false],
        "the defaults come from the table, not from the painter"
    );
    state.ui.log_ref_folds.insert(RefGroup::Remote, Fold::Open);
    let all = bands(&mut state);
    assert_eq!(
        all.iter().map(|b| b.open).collect::<Vec<bool>>(),
        vec![true, true, false],
        "one fold per kind; there is no repository axis to key, and opening the \
         remote band opens the union's one remote band"
    );
    assert_eq!(
        all.iter().map(|b| b.revs.len()).collect::<Vec<usize>>(),
        vec![1, 2, 1],
        "a folded band keeps its rows, so the header still owes its count"
    );
}

#[test]
fn a_tag_scope_survives_an_operation_that_empties_the_tag_cache() {
    let (_tmp, mut state) = pair("alpha", repo_with_every_group);
    let alpha_id = root_named(&state, "alpha");
    state.ui.log_ref_scope = Some("v1".to_owned());

    state.ensure_ref_evidence();
    assert_eq!(
        listing_roots(&state),
        vec![alpha_id.clone()],
        "the tag carrier is found once its evidence is warm"
    );

    state.caches.invalidate(&Affected::Root(alpha_id.clone()));
    assert_eq!(
        listing_roots(&state),
        Vec::<RootId>::new(),
        "the emptied cache makes the derived carrier set silently false — that is the hole"
    );

    state.ensure_ref_evidence();
    assert_eq!(
        listing_roots(&state),
        vec![alpha_id],
        "re-warming before the carriers are read restores the evidence in the same frame"
    );
    assert_eq!(
        log_states::commit_list(&state, true),
        LogState::LoadingCommits,
        "and the pane reads as loading, not as `No commits matching filters`"
    );
}

#[test]
fn rev_of_qualifies_a_bare_remote_name_and_never_doubles_a_qualified_one() {
    let (_tmp, state) = pair("alpha", repo_with_a_published_and_deleted_branch);
    let rows = &state.multi.roots[0].branches;
    let bare = rows
        .iter()
        .find(|b| b.kind == BranchKind::Remote && b.name == "solo")
        .expect("`solo` is published, so the listing holds it as a remote row");
    assert_eq!(
        bare.remote.as_deref(),
        Some("origin"),
        "the parser puts the remote in its own field and the tail in `name`"
    );
    assert_eq!(
        rev_of(bare),
        "origin/solo",
        "the rev a remote row must hand the read"
    );

    let local = rows
        .iter()
        .find(|b| b.kind == BranchKind::Local && b.name == "main")
        .expect("`main` is still checked out");
    assert_eq!(rev_of(local), "main", "a local row's name is its rev");

    assert_eq!(
        rev_of(&remote_row("origin/main", Some("origin"))),
        "origin/main",
        "a name already carrying its remote is not doubled"
    );
    assert_eq!(
        rev_of(&remote_row("main", None)),
        "main",
        "a remote row with no remote invents nothing"
    );
}

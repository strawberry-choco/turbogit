//! Seam-2 contract tests for the branch tree component (branch-tree-view
//! execution plan, "Pin the component contract").
//!
//! These run the component through the bare-fixture harness made possible by
//! the generified test-support helpers: a fixture view model, a fixture props
//! block with a fixed timestamp, and assertions on both what is painted and
//! which events come back — no git, no temporary repository, no application
//! state, no worker channel. "The component is dumb" is a verified claim, not
//! an assumption.
//!
//! The fixture applies a minimal caller policy between frames (it flips the
//! tree-state fields the events name), exactly as a real surface would — so
//! collapse/selection assertions exercise the whole props-in / paint-and-
//! events-out round trip.
//!
//! Painted assertions stay substring-safe and position-anchored: a branch name
//! often paints both in a row and in a repo-header chip, so galleys are
//! filtered by exact text, font family, and vertical position rather than
//! trusting the first match. The fixed timestamp prop is what makes the
//! painted staleness text deterministic.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_kittest::{Harness, Node};
use test_support::harness::{PaintedGalley, filled_circles, painted_galleys, painted_text, settle};
use turbogit_app::state::TreeState;
use turbogit_domain::model::{Branch, BranchKind, Root, RootId};
use turbogit_ui::theme::{Palette, configure_style, install_fonts};
use turbogit_ui::ui::branch_tree_view::{TreeEvent, TreeGroup, TreeProps, branch_tree};
use turbogit_ui::ui::branches_tree::{BranchView, build_branch_view};

// --- fixture model builders (same shape as the pure-builder suite) ------------

/// The fixed "now" every fixture renders against — painted staleness and
/// freshness hints are therefore deterministic. 2026-06-01T12:00:00Z.
fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(1_780_315_200, 0).expect("valid now")
}

fn local(name: &str, favorite: bool, ahead: usize, behind: usize) -> Branch {
    Branch {
        name: name.to_string(),
        kind: BranchKind::Local,
        tracking: if ahead + behind > 0 {
            Some("origin/main".to_string())
        } else {
            None
        },
        favorite,
        protected: false,
        exists: true,
        ahead,
        behind,
        gone: false,
        // Fresh relative to `now` (5 days old) — nothing dimmed by default.
        last_touched: Some(now() - chrono::Duration::days(5)),
        tip: None,
        remote: None,
    }
}

fn stale_local(name: &str) -> Branch {
    let mut b = local(name, false, 0, 0);
    // ~3 months before `now` — well past the 28-day staleness threshold.
    b.last_touched = Some(now() - chrono::Duration::days(90));
    b
}

fn remote(name: &str, remote: &str) -> Branch {
    Branch {
        name: name.to_string(),
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
        remote: Some(remote.to_string()),
    }
}

fn root_with(id: &str, branches: &[Branch], current: Option<&str>) -> Root {
    Root {
        id: RootId(Arc::from(PathBuf::from(id))),
        path: PathBuf::from(id),
        remotes: vec![turbogit_domain::model::Remote {
            name: "origin".to_string(),
            fetch_url: None,
            push_url: None,
        }],
        branches: branches.to_vec(),
        current_branch: current.map(|s| s.to_string()),
        head: Some("abc".to_string()),
        status: turbogit_domain::model::RootStatus::default(),
    }
}

fn alpha_root() -> Root {
    root_with(
        "/alpha",
        &[
            local("main", false, 0, 0),
            stale_local("stale-branch"),
            local("starred", true, 0, 0),
            local("feature/a", false, 0, 0),
            local("feature/b", false, 0, 0),
            remote("origin/main", "origin"),
            remote("origin/remote-only", "origin"),
        ],
        Some("main"),
    )
}

fn beta_root() -> Root {
    // beta's checked-out branch is `wip` — the emphasis marker needs a current
    // branch that is unambiguous across the two repos.
    root_with(
        "/beta",
        &[local("main", false, 0, 0), local("wip", false, 2, 1)],
        Some("wip"),
    )
}

fn alpha_tags() -> HashMap<RootId, Vec<(String, turbogit_domain::model::RefState)>> {
    let mut tags = HashMap::new();
    tags.insert(
        RootId(Arc::from(PathBuf::from("/alpha"))),
        vec![(
            "v1.0".to_string(),
            turbogit_domain::model::RefState::Default,
        )],
    );
    tags
}

/// Everything the fixture render closure needs, owned (the harness state must
/// be `'static`). The component receives borrows into this each frame.
struct Fixture {
    roots: Vec<Root>,
    tags: HashMap<RootId, Vec<(String, turbogit_domain::model::RefState)>>,
    tree: TreeState,
    filter: String,
    repo_filter: Option<RootId>,
    busy: bool,
    merge_in_progress: bool,
    last_fetch: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
    allows_rename: bool,
    shows_row_actions: bool,
    /// Mirrors `TreeProps::collapse_remotes_by_default` (false = Branches-style
    /// expanded-by-default, true = Log-pane-style collapsed-by-default).
    collapse_remotes_by_default: bool,
    /// The events returned by the last completed frame.
    events: Vec<TreeEvent>,
}

impl Fixture {
    fn two_repos() -> Self {
        Self {
            roots: vec![alpha_root(), beta_root()],
            tags: alpha_tags(),
            tree: TreeState::default(),
            filter: String::new(),
            repo_filter: None,
            busy: false,
            merge_in_progress: false,
            last_fetch: None,
            now: now(),
            allows_rename: true,
            shows_row_actions: true,
            collapse_remotes_by_default: false,
            events: Vec::new(),
        }
    }

    fn alpha_only() -> Self {
        let mut fx = Self::two_repos();
        fx.roots = vec![alpha_root()];
        fx
    }

    fn empty_repo() -> Self {
        let mut empty = root_with("/empty", &[], None);
        // A repo with no branches and no HEAD is the genuinely-empty state;
        // an unborn current branch does not count as data.
        empty.head = None;
        Self {
            roots: vec![empty],
            tags: HashMap::new(),
            ..Self::two_repos()
        }
    }

    /// The view model, rebuilt each frame exactly as the surfaces do.
    fn view(&self) -> BranchView {
        build_branch_view(&self.roots, &self.tags, self.tree.show_remotes)
    }

    fn has_any_data(&self) -> bool {
        self.roots
            .iter()
            .any(|r| !r.branches.is_empty() || r.head.is_some())
    }

    /// The minimal caller policy (plan D4): the surface applies the tree's
    /// events to the state block between painting and any dependent surface.
    fn apply(&mut self, event: &TreeEvent) {
        match event {
            TreeEvent::GroupToggled(TreeGroup::Local) => {
                self.tree.groups.local = !self.tree.groups.local;
            }
            TreeEvent::GroupToggled(TreeGroup::Tags) => {
                self.tree.groups.tags = !self.tree.groups.tags;
            }
            TreeEvent::RemoteToggled { root, remote } => {
                let key = (root.clone(), remote.clone());
                if !self.tree.collapsed_remotes.remove(&key) {
                    self.tree.collapsed_remotes.insert(key);
                }
            }
            TreeEvent::RemotesVisibleChanged { visible } => {
                self.tree.show_remotes = *visible;
            }
            TreeEvent::RowClicked { root, branch } => {
                if self.tree.selected_root.as_ref() == Some(root)
                    && self.tree.selected.as_deref() == Some(branch.as_str())
                {
                    self.tree.selected = None;
                    self.tree.selected_root = None;
                } else {
                    self.tree.selected = Some(branch.clone());
                    self.tree.selected_root = Some(root.clone());
                }
                self.tree.overflow = None;
            }
            _ => {}
        }
    }
}

/// A bare-fixture harness (seam 2): renders only the component, no shell.
fn fixture_harness(fx: Fixture) -> Harness<'static, Fixture> {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, fx| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            let view = fx.view();
            let props = TreeProps {
                view: &view,
                filter: &fx.filter,
                repo_filter: fx.repo_filter.as_ref(),
                tags_by_root: &fx.tags,
                multi_repo: fx.roots.len() > 1,
                busy: fx.busy,
                has_any_data: fx.has_any_data(),
                merge_in_progress: fx.merge_in_progress,
                last_fetch: fx.last_fetch,
                now: fx.now,
                allows_rename: fx.allows_rename,
                shows_row_actions: fx.shows_row_actions,
                id_salt: "fixture_tree",
                full_height: true,
                collapse_remotes_by_default: fx.collapse_remotes_by_default,
            };
            let events = branch_tree(ui, &props, &mut fx.tree);
            for event in &events {
                fx.apply(event);
            }
            fx.events.extend(events);
        },
        fx,
    );
    harness.set_size(egui::vec2(720.0, 620.0));
    harness
}

/// The `Role::Button` node labelled exactly `label`. Picks the topmost match —
/// two repos each paint a "Local" / "Tags" / "Fetch" button.
#[track_caller]
fn button<'t>(harness: &'t Harness<'_, Fixture>, label: &'t str) -> Node<'t> {
    let mut nodes: Vec<Node<'t>> = harness
        .query_all_by_label_contains(label)
        .filter(|n| {
            n.accesskit_node().role() == Role::Button
                && n.accesskit_node().label().is_some_and(|l| l == label)
        })
        .collect();
    nodes.sort_by(|a, b| a.rect().top().total_cmp(&b.rect().top()));
    nodes.into_iter().next().unwrap_or_else(|| {
        panic!(
            "no button labelled {label:?}; painted: {:?}",
            painted_text(harness)
        )
    })
}

/// Click a labelled button and return the events the component emitted on
/// that frame — read before any further `step` can overwrite them.
#[track_caller]
fn click_events(harness: &mut Harness<'_, Fixture>, label: &str) -> Vec<TreeEvent> {
    button(harness, label).click();
    harness.step();
    harness.state().events.clone()
}

/// All text galleys painting exactly `text` (rows vs chips disambiguate by
/// their vertical position).
fn galleys_for(harness: &Harness<'_, Fixture>, text: &str) -> Vec<PaintedGalley> {
    painted_galleys(harness)
        .into_iter()
        .filter(|g| g.text == text)
        .collect()
}

// --- rows per repository ------------------------------------------------------

#[test]
fn rows_render_per_repository_with_status_dots() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let texts = painted_text(&h);
    assert!(texts.iter().any(|t| t.contains("alpha")), "{texts:#?}");
    assert!(texts.iter().any(|t| t.contains("beta")));
    assert!(texts.iter().any(|t| t.contains("wip")));

    // Each repo section header paints one 4px status dot (sidebar dots are
    // 3.5px; the shell is not involved here anyway).
    let repo_dots = filled_circles(&h)
        .into_iter()
        .filter(|(_, r, _)| (*r - 4.0).abs() < f32::EPSILON)
        .count();
    assert_eq!(repo_dots, 2, "one status dot per repo header");
}

// --- directory grouping and counts --------------------------------------------

#[test]
fn directory_grouping_shows_derived_count() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    let texts = painted_text(&h);
    assert!(texts.iter().any(|t| t.contains("feature/")), "{texts:#?}");
    // The count is derived from the two leaves beneath `feature/`.
    assert!(texts.iter().any(|t| t == "2"), "{texts:#?}");
}

// --- prefix stripping inside a remote group -----------------------------------

#[test]
fn remote_group_leaf_strips_the_remote_prefix() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = true;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let texts = painted_text(&h);
    assert!(
        texts.iter().any(|t| t.contains("remote-only")),
        "{texts:#?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("origin/remote-only")),
        "the remote prefix must not re-print inside its group: {texts:#?}"
    );
}

// --- the active-branch marker -------------------------------------------------

#[test]
fn current_branch_reads_in_the_emphasis_color() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    // The current branch's row reads in soft blue in the monospace data face.
    let active = galleys_for(&h, "wip");
    assert!(
        active
            .iter()
            .any(|g| g.color == Palette::STATE_INFO && g.family == egui::FontFamily::Monospace),
        "wip row must read STATE_INFO in the data face: {active:#?}"
    );
    // A plain local row does not.
    let plain = galleys_for(&h, "starred");
    assert!(
        plain.iter().all(|g| g.color != Palette::STATE_INFO),
        "a plain local row must not read as active: {plain:#?}"
    );
}

// --- favourite pinning ----------------------------------------------------------

#[test]
fn favorite_pins_above_the_current_branch_row() {
    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let starred_y = galleys_for(&h, "starred")[0].pos.y;
    // "main" paints twice in alpha's section: the header chip first, the row
    // below. Pinning means some "main" galley sits *below* the favorite.
    let main_ys: Vec<f32> = galleys_for(&h, "main").iter().map(|g| g.pos.y).collect();
    assert!(
        main_ys.iter().any(|y| *y > starred_y),
        "favorite `starred` ({starred_y}) must sit above a `main` row (chips+rows at {main_ys:?})"
    );
}

// --- group toggles: events and collapse ---------------------------------------

#[test]
fn tag_group_toggle_emits_event_and_opens_the_group() {
    let mut h = fixture_harness(Fixture::alpha_only());
    settle(&mut h);
    assert!(
        !painted_text(&h).iter().any(|t| t.contains("v1.0")),
        "tags start collapsed (TreeState::default)"
    );

    let events = click_events(&mut h, "Tags");
    assert!(
        events.contains(&TreeEvent::GroupToggled(TreeGroup::Tags)),
        "tag header click must emit GroupToggled(Tags): {events:#?}"
    );
    settle(&mut h);
    assert!(
        painted_text(&h).iter().any(|t| t.contains("v1.0")),
        "the tags group opens after the caller applies the event"
    );
}

#[test]
fn local_group_toggle_emits_event_and_collapses() {
    let mut h = fixture_harness(Fixture::alpha_only());
    settle(&mut h);

    let events = click_events(&mut h, "Local");
    assert!(
        events.contains(&TreeEvent::GroupToggled(TreeGroup::Local)),
        "{events:#?}"
    );
    settle(&mut h);
    assert!(
        !painted_text(&h).iter().any(|t| t.contains("starred")),
        "local rows hide while the group is collapsed"
    );
}

// --- per-remote collapse --------------------------------------------------------

#[test]
fn remote_header_toggle_emits_event_and_collapses_that_remote() {
    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = true;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let events = click_events(&mut h, "origin");
    assert!(
        events.contains(&TreeEvent::RemoteToggled {
            root: RootId(Arc::from(PathBuf::from("/alpha"))),
            remote: "origin".to_string(),
        }),
        "{events:#?}"
    );
    settle(&mut h);
    assert!(
        !painted_text(&h).iter().any(|t| t.contains("remote-only")),
        "the collapsed remote's branches hide"
    );
    assert!(
        painted_text(&h).iter().any(|t| t.contains("origin")),
        "the collapsed remote's header stays"
    );
}

/// The Log pane's tree starts every remote group collapsed: the header rows
/// paint, their branches hide until a header click expands them — and a second
/// click re-collapses.
#[test]
fn remote_groups_start_collapsed_when_default_is_collapsed() {
    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = true;
    fx.collapse_remotes_by_default = true;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    assert!(
        !painted_text(&h).iter().any(|t| t.contains("remote-only")),
        "remote branches are collapsed by default"
    );
    assert!(
        painted_text(&h).iter().any(|t| t.contains("origin")),
        "the remote header stays visible"
    );

    button(&h, "origin").click();
    settle(&mut h);
    assert!(
        painted_text(&h).iter().any(|t| t.contains("remote-only")),
        "a header click expands the group"
    );

    button(&h, "origin").click();
    settle(&mut h);
    assert!(
        !painted_text(&h).iter().any(|t| t.contains("remote-only")),
        "a second click re-collapses the group"
    );
}

// --- the remotes-visible switch -------------------------------------------------

#[test]
fn remote_rollup_click_emits_the_visibility_switch() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    // Collapsed by default: the rollup states the real hidden counts.
    let texts = painted_text(&h);
    assert!(texts.iter().any(|t| t.contains("Remote")), "{texts:#?}");
    assert!(
        texts.iter().any(|t| t.contains("1 remotes · 2 branches")),
        "the rollup count is derived, never estimated: {texts:#?}"
    );

    let events = click_events(&mut h, "Remote");
    assert!(
        events.contains(&TreeEvent::RemotesVisibleChanged { visible: true }),
        "{events:#?}"
    );
    settle(&mut h);
    assert!(
        painted_text(&h).iter().any(|t| t.contains("origin")),
        "the remote groups render after the caller applies the switch"
    );
}

// --- search filtering -------------------------------------------------------------

#[test]
fn search_filters_the_tree_live() {
    let mut fx = Fixture::two_repos();
    fx.filter = "wip".to_string();
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let texts = painted_text(&h);
    assert!(texts.iter().any(|t| t.contains("wip")), "{texts:#?}");
    assert!(!texts.iter().any(|t| t.contains("starred")));
    assert!(!texts.iter().any(|t| t.contains("v1.0")));
}

// --- no-match state: the dead end becomes the next intent ------------------------

#[test]
fn no_match_state_offers_to_create_the_typed_name() {
    let mut fx = Fixture::two_repos();
    fx.filter = "zzz".to_string();
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let texts = painted_text(&h);
    assert!(
        texts.iter().any(|t| t.contains("no branch called zzz")),
        "{texts:#?}"
    );
    let events = click_events(&mut h, "Create \"zzz\"");
    assert_eq!(
        events,
        vec![TreeEvent::CreateBranchRequested {
            name: "zzz".to_string()
        }],
        "the create event carries the typed name"
    );
}

// --- empty state -------------------------------------------------------------------

#[test]
fn empty_state_offers_one_create_action() {
    let mut h = fixture_harness(Fixture::empty_repo());
    settle(&mut h);

    let texts = painted_text(&h);
    assert!(
        texts
            .iter()
            .any(|t| t.contains("This repo has no branches yet")),
        "{texts:#?}"
    );
    let events = click_events(&mut h, "Create the first branch");
    assert_eq!(
        events,
        vec![TreeEvent::CreateBranchRequested {
            name: String::new()
        }],
        "the empty state's create event carries no typed name"
    );
}

#[test]
fn reading_state_shows_while_a_scan_is_in_flight() {
    let mut fx = Fixture::empty_repo();
    fx.busy = true;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let texts = painted_text(&h);
    assert!(
        texts.iter().any(|t| t.contains("reading branches…")),
        "{texts:#?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("Create")),
        "a reading state offers no create action yet"
    );
}

// --- the disabled rename capability ----------------------------------------------

#[test]
fn rename_capability_disabled_never_edits_inline() {
    let mut fx = Fixture::two_repos();
    fx.allows_rename = false;
    fx.tree.selected = Some("wip".to_string());
    fx.tree.selected_root = Some(RootId(Arc::from(PathBuf::from("/beta"))));
    fx.tree.renaming = Some("wip".to_string());
    fx.tree.rename_draft = "renamed-wip".to_string();
    let mut h = fixture_harness(fx);
    settle(&mut h);

    assert!(
        !painted_text(&h).iter().any(|t| t.contains("renamed-wip")),
        "without the capability the row paints, never the editor"
    );
    assert!(
        painted_text(&h).iter().any(|t| t.contains("wip")),
        "the row still paints"
    );
}

#[test]
fn rename_editor_commits_the_draft_as_an_event() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    fx.tree.selected = Some("wip".to_string());
    fx.tree.selected_root = Some(RootId(Arc::from(PathBuf::from("/beta"))));
    fx.tree.renaming = Some("wip".to_string());
    fx.tree.rename_draft = "renamed-wip".to_string();
    let mut h = fixture_harness(fx);
    settle(&mut h);

    assert!(
        painted_text(&h).iter().any(|t| t.contains("renamed-wip")),
        "with the capability the draft paints on the row"
    );
    // Commit via the keyboard path (Enter on the editor). The editor's
    // Apply/Cancel buttons overflow their row today — `widgets::text_input`
    // consumes the full row width — so a click cannot reach them inside this
    // bare-fixture window; Enter is the pixel-frozen commit gesture.
    h.key_press(egui::Key::Enter);
    h.step();
    h.step();
    let events = h.state().events.clone();
    assert!(
        events.contains(&TreeEvent::RenameCommitted {
            root: RootId(Arc::from(PathBuf::from("/beta"))),
            old: "wip".to_string(),
            new: "renamed-wip".to_string(),
        }),
        "{events:#?}"
    );
}

// --- row activation carries the owning repository --------------------------------

#[test]
fn row_click_emits_owner_and_branch_not_a_bare_name() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    // Two repositories both have a `main`; `wip` exists only in beta. Click it.
    let events = click_events(&mut h, "wip");
    assert!(
        events.contains(&TreeEvent::RowClicked {
            root: RootId(Arc::from(PathBuf::from("/beta"))),
            branch: "wip".to_string(),
        }),
        "a row click reports its owning repository: {events:#?}"
    );
}

#[test]
fn checkout_row_action_activation_carries_owner_and_branch() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    // Hover the row so its per-row action appears, then click Checkout.
    let row = button(&h, "wip");
    row.hover();
    h.step();
    let events = click_events(&mut h, "Checkout in beta");
    assert!(
        events.contains(&TreeEvent::RowActivated {
            root: RootId(Arc::from(PathBuf::from("/beta"))),
            branch: "wip".to_string(),
        }),
        "activation must carry the owning repository, not a bare name: {events:#?}"
    );
}

// --- the repository header's Fetch request ----------------------------------------

#[test]
fn repo_header_fetch_emits_a_fetch_request() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    // Both repos paint a Fetch button; `button` picks alpha's (the topmost).
    button(&h, "Fetch").click();
    h.step();
    let events = h.state().events.clone();
    assert!(
        events.contains(&TreeEvent::FetchRequested {
            root: RootId(Arc::from(PathBuf::from("/alpha"))),
        }),
        "{events:#?}"
    );
}

// --- the repo scope narrows the tree ----------------------------------------------

#[test]
fn repo_scope_narrows_to_one_section() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    fx.repo_filter = Some(RootId(Arc::from(PathBuf::from("/beta"))));
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let texts = painted_text(&h);
    assert!(texts.iter().any(|t| t.contains("wip")), "{texts:#?}");
    assert!(
        !texts.iter().any(|t| t.contains("starred")),
        "alpha's branches are out of scope"
    );
    assert!(
        !texts.iter().any(|t| t.contains("alpha")),
        "the narrowed tree never paints another repo's section"
    );
}

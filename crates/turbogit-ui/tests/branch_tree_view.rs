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
use egui::{Color32, Pos2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_kittest::{Harness, Node};
use test_support::harness::{
    PaintedGalley, filled_circles, filled_rects, painted_galleys, painted_text, settle,
};
use turbogit_app::state::TreeState;
use turbogit_domain::model::{Branch, BranchKind, Root, RootId};
use turbogit_ui::theme::{Palette, configure_style, install_fonts};
use turbogit_ui::ui::branch_tree_view::{
    TreeEvent, TreeGroup, TreeProps, branch_tree, remotes_revealed,
};
use turbogit_ui::ui::branches_tree::{BranchView, build_branch_view};
use turbogit_ui::ui::components::BRANCH_ROW_H;

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
        build_branch_view(&self.roots, &self.tags, &|root| {
            remotes_revealed(&self.tree, root)
        })
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
            TreeEvent::RemoteRevealToggled { root, revealed } => {
                if *revealed {
                    self.tree.remotes_revealed.insert(root.clone());
                } else {
                    self.tree.remotes_revealed.remove(root);
                }
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
    fixture_harness_at(fx, 720.0)
}

/// The same fixture at an explicit width, for the narrow-list cases.
fn fixture_harness_at(fx: Fixture, width: f32) -> Harness<'static, Fixture> {
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
    harness.set_size(egui::vec2(width, 620.0));
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

// --- group labels read as structure --------------------------------------------

/// A section header's count is a badge beside the label, not characters inside
/// the label string, and the header sits on a band of its own.
#[test]
fn a_section_header_paints_its_count_as_a_badge_on_a_band() {
    use turbogit_ui::ui::components::{RowState, current_row_fill, row_fill};

    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    // Five local branches: the label alone, and the count as its own galley.
    let label = galleys_for(&h, "LOCAL");
    assert_eq!(
        label.len(),
        1,
        "the label paints without its count inside it"
    );
    assert!(
        painted_text(&h).iter().all(|t| t != "LOCAL 5"),
        "the count is no longer part of the label string: {:?}",
        painted_text(&h)
    );
    let badge = galleys_for(&h, "5")
        .into_iter()
        .find(|g| (g.pos.y - label[0].pos.y).abs() < 12.0 && g.pos.x > label[0].rect.right())
        .expect("the count badge sits beside the label");
    assert!(
        badge.rect.left() - label[0].rect.right() < 24.0,
        "the badge belongs to the label, not to the far end of the strip: {:?}",
        badge.rect
    );

    let band = filled_rects(&h)
        .into_iter()
        .find(|(rect, _)| {
            rect.contains(Pos2::new(rect.center().x, label[0].pos.y + 4.0)) && rect.width() > 600.0
        })
        .map(|(_, fill)| fill)
        .expect("a full-width band under the section label");
    assert_ne!(
        band,
        Palette::SURFACE,
        "a section band is not the repo band"
    );
    assert_ne!(
        band,
        row_fill(RowState::Hover),
        "a section band is not a row hover fill"
    );
    assert_ne!(
        band,
        current_row_fill(RowState::Default),
        "a section band is not a current row's band"
    );
}

/// A subgroup, a section header and a branch row are distinguishable by painted
/// geometry — the defect this closes is that all three read as content.
#[test]
fn a_subgroup_a_section_and_a_row_are_distinguishable_by_geometry() {
    use test_support::harness::painted_paths;

    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let section = &galleys_for(&h, "LOCAL")[0];
    let dir = &galleys_for(&h, "feature/")[0];
    let child = &galleys_for(&h, "a")[0];

    // The subgroup prints its segment as data, with a folder glyph before it.
    assert_eq!(
        dir.family,
        egui::FontFamily::Monospace,
        "a directory segment is data, like a branch name"
    );
    assert!(
        painted_paths(&h).into_iter().any(|(r, _)| {
            (r.center().y - (dir.pos.y + 6.0)).abs() < 12.0
                && r.right() <= dir.pos.x + 1.0
                && r.left() >= dir.pos.x - 24.0
        }),
        "the subgroup carries no folder glyph"
    );

    // Only the section header sits on a band.
    let banded = |y: f32| {
        filled_rects(&h).into_iter().any(|(rect, fill)| {
            fill == Palette::SECTION_BG
                && rect.width() > 600.0
                && rect.top() <= y
                && y <= rect.bottom()
        })
    };
    assert!(banded(section.pos.y + 4.0), "the section header bands");
    assert!(
        !banded(dir.pos.y + 4.0),
        "a subgroup is not the same kind of strip"
    );

    // A subgroup hangs at its section's inset and is told apart from it by the
    // band; a branch row is told apart by hanging further in.
    assert!(
        child.pos.x > dir.pos.x + 1.0,
        "the indent step between a subgroup and its children is not visible: \
         child {:?} vs subgroup {:?}",
        child.pos.x,
        dir.pos.x
    );
    assert!(
        child.pos.x > section.pos.x + 1.0,
        "a branch row does not line up with its group label: child {:?} vs label {:?}",
        child.pos.x,
        section.pos.x
    );
}

/// The collapsed rollup sits at `LOCAL`'s level, so it takes the same casing.
#[test]
fn the_collapsed_rollup_reads_as_a_section_label() {
    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    assert_eq!(
        galleys_for(&h, "REMOTE").len(),
        1,
        "the rollup is cased like LOCAL and TAGS: {:?}",
        painted_text(&h)
    );
    assert!(
        painted_text(&h).iter().all(|t| t != "Remote"),
        "no capitalised `Remote` label is painted at section level"
    );
}

// --- the repo header's summary -------------------------------------------------

/// A header says what state its repository is in, in words, with the current
/// branch's counts when there are any.
#[test]
fn the_repo_header_names_its_status_in_words() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);
    let texts = painted_text(&h);

    // alpha: nothing to report.
    assert!(
        texts.iter().any(|t| t == "in sync"),
        "a clean repo says `in sync`: {texts:#?}"
    );
    // beta: its current branch `wip` is 2 ahead and 1 behind.
    assert!(
        texts.iter().any(|t| t == "2 ahead · 1 behind · diverged"),
        "the counts and the state both belong to the header summary: {texts:#?}"
    );
}

/// A header is the top of a block, so it carries a resting band that is neither
/// a row's hover fill nor a current row's band.
#[test]
fn the_repo_header_paints_a_resting_band() {
    use turbogit_ui::ui::components::{RowState, current_row_fill, row_fill};

    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    let (dot, _, _) = *filled_circles(&h)
        .iter()
        .find(|(_, r, _)| (*r - 4.0).abs() < f32::EPSILON)
        .expect("a repo header status dot");
    let center_y = dot.y;
    let band = filled_rects(&h)
        .into_iter()
        .find(|(rect, _)| {
            rect.contains(Pos2::new(rect.center().x, center_y)) && rect.width() > 600.0
        })
        .map(|(_, fill)| fill)
        .expect("a full-width band on the header line");
    assert_ne!(band, Color32::TRANSPARENT, "the header strip is filled");
    assert_ne!(
        band,
        row_fill(RowState::Hover),
        "a header band is not a row hover fill"
    );
    assert_ne!(
        band,
        current_row_fill(RowState::Default),
        "a header band is not a current row's band"
    );
}

/// The current-branch pill is part of the repo's identity, so it rides the name
/// rather than trailing the strip.
#[test]
fn the_current_branch_pill_sits_with_the_repo_name() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);
    let galleys = painted_galleys(&h);

    let name = galleys
        .iter()
        .find(|g| g.text == "beta")
        .expect("the repo name");
    let pill = galleys
        .iter()
        .find(|g| g.text == "wip")
        .expect("the current-branch pill");
    assert!(
        pill.pos.x > name.rect.right() && pill.pos.x - name.rect.right() < 24.0,
        "the pill sits directly after the name: name {:?}, pill {:?}",
        name.rect,
        pill.pos
    );
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

// --- the row's zones ----------------------------------------------------------

/// The tracking branch sits in a zone of its own, so it lines up down the list
/// instead of drifting to wherever each name happens to end.
#[test]
fn the_tracking_zone_has_one_left_edge_regardless_of_name_length() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    fx.roots = vec![root_with(
        "/track",
        &[
            local("a", false, 1, 0),
            local("a-name-long-enough-to-push-an-inline-upstream", false, 1, 0),
        ],
        Some("a"),
    )];
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let xs: Vec<f32> = galleys_for(&h, "origin/main")
        .iter()
        .map(|g| g.pos.x)
        .collect();
    assert_eq!(xs.len(), 2, "both rows track origin/main: {xs:?}");
    assert!(
        (xs[0] - xs[1]).abs() < 0.5,
        "the tracking column must not drift with the name's length: {xs:?}"
    );
}

/// The ⋯ column belongs to every row at rest, and every row leads with an icon
/// so the names line up — local and remote alike.
#[test]
fn every_row_leads_with_an_icon_and_carries_its_overflow_at_rest() {
    use test_support::harness::painted_paths;

    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = true;
    let mut h = fixture_harness(fx);
    settle(&mut h);
    // Nothing was ever hovered: `settle` only steps frames.

    let rows = row_buttons(&h);
    assert!(rows > 0, "the fixture paints branch rows");
    let overflows = count_label(&h, "More actions");
    assert_eq!(
        overflows, rows,
        "one ⋯ per branch row, at rest ({overflows} overflow vs {rows} rows)"
    );

    // Every row leads with an icon stroked in the slot before its name — local
    // and remote alike — and rows at the same depth share one name edge.
    for name in ["starred", "remote-only"] {
        let galley = galleys_for(&h, name)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("{name} paints no name"));
        let row = h
            .get_all_by_role(Role::Button)
            .find(|n| {
                (n.rect().height() - BRANCH_ROW_H).abs() < 0.5
                    && n.rect().width() > 500.0
                    && n.rect().contains(galley.pos)
            })
            .expect("the row the name paints on")
            .rect();
        let paths = painted_paths(&h)
            .into_iter()
            .filter(|(r, _)| {
                (r.center().y - (galley.pos.y + 6.0)).abs() < 12.0
                    && r.left() >= row.left()
                    && r.right() <= galley.pos.x + 1.0
            })
            .count();
        assert!(
            paths > 0,
            "`{name}` paints no leading icon in the slot before its name"
        );
    }

    let edges: Vec<f32> = ["starred", "stale-branch"]
        .iter()
        .map(|name| {
            galleys_for(&h, name)
                .into_iter()
                .next()
                .unwrap_or_else(|| panic!("{name} paints no name"))
                .pos
                .x
        })
        .collect();
    assert!(
        (edges[0] - edges[1]).abs() < 0.5,
        "the name column has one fixed left edge down the list: {edges:?}"
    );
}

/// The Log pane renders the same rows without the action column; the zones stay
/// lined up all the same.
#[test]
fn the_log_panes_rows_have_no_overflow_yet_still_align() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    fx.shows_row_actions = false;
    fx.roots = vec![root_with(
        "/track",
        &[
            local("a", false, 1, 0),
            local("a-name-long-enough-to-push-an-inline-upstream", false, 1, 0),
        ],
        Some("a"),
    )];
    let mut h = fixture_harness(fx);
    settle(&mut h);

    assert_eq!(
        count_label(&h, "More actions"),
        0,
        "no ⋯ where row actions are not shown"
    );

    let xs: Vec<f32> = galleys_for(&h, "origin/main")
        .iter()
        .map(|g| g.pos.x)
        .collect();
    assert_eq!(xs.len(), 2, "both rows track origin/main: {xs:?}");
    assert!(
        (xs[0] - xs[1]).abs() < 0.5,
        "the tracking zone keeps one left edge without the action column: {xs:?}"
    );
}

/// The failure mode the column layout exists to remove: at a width where
/// nothing fits, zones must still not sit on top of each other, and no run of
/// text may escape the row.
#[test]
fn a_narrow_list_keeps_every_zone_inside_the_row() {
    const NARROW: f32 = 300.0;
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    fx.roots = vec![root_with(
        "/track",
        &[
            local("a", false, 1, 1),
            local("a-name-long-enough-to-push-an-inline-upstream", false, 1, 1),
        ],
        Some("a"),
    )];
    let mut h = fixture_harness_at(fx, NARROW);
    settle(&mut h);

    let rows: Vec<egui::Rect> = h
        .get_all_by_role(Role::Button)
        .filter(|n| {
            (n.rect().height() - BRANCH_ROW_H).abs() < 0.5
                && n.rect().width() > 200.0
                && !matches!(
                    n.accesskit_node().label().as_deref(),
                    Some("Local") | Some("Tags") | Some("Remote")
                )
        })
        .map(|n| n.rect())
        .collect();
    assert_eq!(rows.len(), 2, "both branches paint a row");

    let galleys = painted_galleys(&h);
    for row in &rows {
        let mut on_row: Vec<&PaintedGalley> =
            galleys.iter().filter(|g| row.contains(g.pos)).collect();
        on_row.sort_by(|a, b| a.rect.left().total_cmp(&b.rect.left()));
        for pair in on_row.windows(2) {
            assert!(
                pair[0].rect.right() <= pair[1].rect.left() + 0.5,
                "in a {NARROW}px list `{}` runs into `{}`: {:?}",
                pair[0].text,
                pair[1].text,
                on_row.iter().map(|g| (&g.text, g.rect)).collect::<Vec<_>>()
            );
        }
        for g in &on_row {
            assert!(
                g.rect.right() <= row.right() + 0.5,
                "`{}` escapes its row in a {NARROW}px list: {:?}",
                g.text,
                g.rect
            );
        }
    }
}

/// How many Buttons are branch rows: a full-width row-height strip that is not a
/// group or section header (a remote group header is full width too, but rides a
/// shorter section line).
fn row_buttons(h: &Harness<'_, Fixture>) -> usize {
    h.get_all_by_role(Role::Button)
        .filter(|n| {
            n.rect().width() > 500.0
                && (n.rect().height() - BRANCH_ROW_H).abs() < 0.5
                && !matches!(
                    n.accesskit_node().label().as_deref(),
                    Some("Local") | Some("Tags") | Some("Remote")
                )
        })
        .count()
}

/// How many Buttons carry exactly `label`.
fn count_label(h: &Harness<'_, Fixture>, label: &str) -> usize {
    h.get_all_by_role(Role::Button)
        .filter(|n| n.accesskit_node().label().as_deref() == Some(label))
        .count()
}

// --- the active-branch marker -------------------------------------------------

/// One `current` badge per repository section — the row's answer to "where am I
/// right now", rendered by a single component rather than per call site.
#[test]
fn one_current_badge_per_repo_section() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    // alpha is on `main`, beta on `wip`.
    assert_eq!(
        galleys_for(&h, "current").len(),
        2,
        "one current badge per repo section: {:?}",
        painted_text(&h)
    );

    let mut h = fixture_harness(Fixture::alpha_only());
    settle(&mut h);
    assert_eq!(
        galleys_for(&h, "current").len(),
        1,
        "a single section paints a single current badge"
    );
}

/// The current row answers "where am I" with a band across the whole list plus
/// its badge — not with tinted name ink, which was a third marker for the same
/// fact.
#[test]
fn current_row_bands_the_whole_list_and_keeps_plain_name_ink() {
    const LIST_W: f32 = 720.0;
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    // beta's current branch is `wip`; it paints twice — the repo header's chip
    // above, the row below.
    let row = galleys_for(&h, "wip")
        .into_iter()
        .max_by(|a, b| a.pos.y.total_cmp(&b.pos.y))
        .expect("the current branch's row paints its name");
    assert_ne!(
        row.color,
        Palette::STATE_INFO,
        "the current row's name must stop carrying the marker in its ink: {row:#?}"
    );

    let center_y = row.pos.y + 6.0;
    let spanning = filled_rects(&h)
        .into_iter()
        .filter(|(rect, _)| rect.top() <= center_y && center_y <= rect.bottom())
        .map(|(rect, _)| rect.width())
        .max_by(f32::total_cmp)
        .unwrap_or_default();
    assert!(
        spanning > LIST_W * 0.85,
        "a current row's fill must span the list, not hug its text \
         (widest rect at y {center_y}: {spanning} of {LIST_W})"
    );
}

/// The header's current-branch chip and the row's `current` badge are one
/// component, so the same fact looks the same in both places.
#[test]
fn header_chip_and_row_badge_are_the_same_treatment() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    let badge = galleys_for(&h, "current")
        .into_iter()
        .next()
        .expect("a current badge");
    // beta's chip is the *upper* `wip`: the repo header sits above its row.
    let chip = galleys_for(&h, "wip")
        .into_iter()
        .min_by(|a, b| a.pos.y.total_cmp(&b.pos.y))
        .expect("the repo header's current-branch chip");
    assert_eq!(
        chip.color, badge.color,
        "one ink for one fact: chip {chip:#?} vs badge {badge:#?}"
    );

    let fill_at = |y: f32| {
        filled_rects(&h)
            .into_iter()
            .filter(|(rect, _)| rect.top() <= y && y <= rect.bottom())
            .min_by_key(|(rect, _)| rect.width() as i64)
            .map(|(_, fill)| fill)
    };
    let chip_fill = fill_at(chip.pos.y + 6.0).expect("the chip paints a fill");
    let badge_fill = fill_at(badge.pos.y + 6.0).expect("the badge paints a fill");
    assert_eq!(
        chip_fill, badge_fill,
        "the chip and the badge share one fill"
    );
}

/// A row that is both current and diverged keeps the status it also carries:
/// the band must not swallow the sync chips.
#[test]
fn a_current_row_keeps_its_status_chips() {
    use turbogit_ui::ui::components::{SyncKind, sync_bg};

    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    // beta's `wip` is current *and* 2 ahead / 1 behind.
    let row = galleys_for(&h, "wip")
        .into_iter()
        .max_by(|a, b| a.pos.y.total_cmp(&b.pos.y))
        .expect("the current row");
    let center_y = row.pos.y + 6.0;
    let fills: Vec<egui::Color32> = filled_rects(&h)
        .into_iter()
        .filter(|(rect, _)| rect.top() <= center_y && center_y <= rect.bottom())
        .map(|(_, fill)| fill)
        .collect();
    assert!(
        fills.contains(&sync_bg(SyncKind::Ahead)),
        "the ahead chip must survive the current band: {fills:#?}"
    );
    assert!(
        fills.contains(&sync_bg(SyncKind::Behind)),
        "the behind chip must survive the current band: {fills:#?}"
    );
}

/// Revealing remote rows must not multiply the marker: one current branch is
/// one badge, per repository section.
#[test]
fn revealing_remotes_does_not_add_a_second_current_badge() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = true;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    assert_eq!(
        galleys_for(&h, "current").len(),
        2,
        "still one badge per section with remotes open: {:?}",
        painted_text(&h)
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

/// Revealing one repository's remotes is that repository's own state.
#[test]
fn revealing_remotes_leaves_the_other_repository_rolled_up() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    // Both repos start rolled up, and no remote group is revealed.
    assert!(
        painted_text(&h)
            .iter()
            .any(|t| t == "1 remote · 0 branches"),
        "beta's own rollup line: {:?}",
        painted_text(&h)
    );
    assert!(galleys_for(&h, "origin").is_empty());

    // The topmost rollup is alpha's.
    button(&h, "Remote").click();
    settle(&mut h);

    assert_eq!(
        galleys_for(&h, "origin").len(),
        1,
        "the clicked repo reveals its remote groups"
    );
    assert!(
        painted_text(&h)
            .iter()
            .any(|t| t == "1 remote · 0 branches"),
        "the other repo still paints its rollup line: {:?}",
        painted_text(&h)
    );
}

/// The reveal has a way back: one click on the header a revealed repo was given
/// restores its rollup line.
#[test]
fn one_click_returns_a_revealed_repository_to_its_rollup() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    button(&h, "Remote").click();
    settle(&mut h);
    assert!(!galleys_for(&h, "origin").is_empty(), "revealed");

    // The revealed repo is the only one still offering a `Remote` button whose
    // click means "hide again": its header sits above beta's rollup.
    button(&h, "Remote").click();
    settle(&mut h);
    assert!(
        galleys_for(&h, "origin").is_empty(),
        "the click hid the remote groups"
    );
    assert!(
        painted_text(&h)
            .iter()
            .any(|t| t == "1 remote · 2 branches"),
        "and the rollup line came back: {:?}",
        painted_text(&h)
    );
    assert!(
        h.state().tree.remotes_revealed.is_empty(),
        "and left nothing revealed"
    );
}

/// Revealing a repository does not disturb the per-remote collapse that already
/// works inside it.
#[test]
fn per_remote_collapse_still_works_inside_a_revealed_repository() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    fx.tree
        .remotes_revealed
        .insert(RootId(Arc::from(PathBuf::from("/alpha"))));
    let mut h = fixture_harness(fx);
    settle(&mut h);

    assert!(
        !galleys_for(&h, "remote-only").is_empty(),
        "the revealed group is open"
    );
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
        galleys_for(&h, "remote-only").is_empty(),
        "collapsing that remote still hides its branches"
    );
}

/// A reveal is tree state, so searching and clearing must not lose it.
#[test]
fn a_reveal_survives_a_filter_round_trip() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    fx.tree
        .remotes_revealed
        .insert(RootId(Arc::from(PathBuf::from("/alpha"))));
    let mut h = fixture_harness(fx);
    settle(&mut h);

    h.state_mut().filter = "starred".to_string();
    settle(&mut h);
    h.state_mut().filter = String::new();
    settle(&mut h);

    assert_eq!(
        h.state().tree.remotes_revealed.len(),
        1,
        "the reveal is untouched by filtering"
    );
    assert!(
        !galleys_for(&h, "origin").is_empty(),
        "and the repo is still revealed afterwards: {:?}",
        painted_text(&h)
    );
}

// --- the remotes-visible switch -------------------------------------------------

#[test]
fn remote_rollup_click_emits_the_visibility_switch() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    // Collapsed by default: the rollup states the real hidden counts.
    let texts = painted_text(&h);
    assert!(texts.iter().any(|t| t.contains("REMOTE")), "{texts:#?}");
    assert!(
        texts.iter().any(|t| t.contains("1 remote · 2 branches")),
        "the rollup count is derived, never estimated: {texts:#?}"
    );

    let events = click_events(&mut h, "Remote");
    assert!(
        events.contains(&TreeEvent::RemoteRevealToggled {
            root: RootId(Arc::from(PathBuf::from("/alpha"))),
            revealed: true,
        }),
        "the rollup reveals the repository it belongs to: {events:#?}"
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

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
use turbogit_domain::model::{Branch, BranchKind, Root, RootId, Upstream};
use turbogit_ui::theme::{Palette, configure_style, install_fonts};
use turbogit_ui::ui::branch_tree_view::{
    TreeEvent, TreeGroup, TreeProps, branch_tree, remotes_revealed,
};
use turbogit_ui::ui::branches_tree::{BranchView, build_branch_view};
use turbogit_ui::ui::components::{BRANCH_ROW_H, current_row_fill, row_ink, sync_ink};

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
            Upstream::from_git_ref("origin/main")
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
    allows_context_menu: bool,
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
            allows_context_menu: true,
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
                allows_context_menu: fx.allows_context_menu,
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

/// Each repository paints its own block, and its band's state reaches the
/// screen as exactly **one mark**: the leading dot of the mark pair beside the
/// band summary.
///
/// **Why the dot moved.** The band used to carry a second, heading status dot on
/// its left *and* a coloured summary on its right. Two dots in one band, in the
/// same colour, saying the same thing, is one of them redundant — and R6 asks
/// for one. The mark pair's dot is that dot now, so "one status dot per repo
/// header" still holds, counted on the band's own line rather than at a radius
/// that a row's inline mark could collide with.
#[test]
fn rows_render_per_repository_with_one_state_mark_each() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let texts = painted_text(&h);
    assert!(texts.iter().any(|t| t.contains("alpha")), "{texts:#?}");
    assert!(texts.iter().any(|t| t.contains("beta")));
    assert!(texts.iter().any(|t| t.contains("wip")));

    // The band is located by its own name, and its mark is the dot on the same
    // line — a *circle*, at the mark pair's own radius, in the one map's colour.
    for (repo, color) in [
        ("alpha", Palette::AHEAD),
        ("beta", Palette::STATUS_DIVERGED),
    ] {
        let name = galleys_for(&h, repo)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("the `{repo}` band names its repository"));
        let marks: Vec<_> = filled_circles(&h)
            .into_iter()
            .filter(|(center, r, _)| {
                (center.y - name.pos.y).abs() < 14.0
                    && *r == turbogit_ui::ui::components::STATE_DOT_R
            })
            .collect();
        assert_eq!(
            marks.len(),
            1,
            "one state mark on the `{repo}` band's own line, found {marks:?}"
        );
        assert_eq!(
            marks[0].2, color,
            "…in that repository's state colour, from `RepoState::color()`"
        );
    }
}

// --- group labels read as structure --------------------------------------------

/// A section header's count is a chip of its own on the band's **trailing**
/// edge, and the header sits on a band of its own.
///
/// **Why the badge moved.** The count used to sit immediately beside the
/// uppercase label, which is where a reader looks for a word's continuation and
/// therefore made a *label* and its *count* read as one string. Ticket 16
/// requires the scope band to carry the count right-aligned rather than
/// interpolated into the label, and the count to be the shared **count chip**,
/// not the v1 9px proportional badge. The assertion that moved is the
/// "belongs to the label, not to the far end of the strip" one: it is now the
/// opposite claim, stated positively — the count is at the far end, in the
/// chip's own fill and ink, and the label string still never contains it.
#[test]
fn a_section_header_right_aligns_its_count_as_a_chip_on_a_band() {
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
    let chip_text = galleys_for(&h, "5")
        .into_iter()
        .find(|g| (g.pos.y - label[0].pos.y).abs() < 12.0)
        .expect("the count paints beside the label");
    // Right-aligned: the count's chip ends one `PAD_LIST` from the band's own
    // right edge, and it is nowhere near the label.
    let band_right = filled_rects(&h)
        .into_iter()
        .filter(|(rect, fill)| {
            *fill == Palette::SECTION_BG
                && rect.width() > 600.0
                && rect.top() <= label[0].pos.y + 4.0
                && label[0].pos.y + 4.0 <= rect.bottom()
        })
        .map(|(rect, _)| rect.right())
        .next()
        .expect("the section band");
    // …and it is the shared count chip: the raised fill, the secondary ink, the
    // compact chip radius, at the chip's own height.
    let chip = filled_rects(&h)
        .into_iter()
        .find(|(rect, fill)| {
            *fill == Palette::RAISED && rect.contains(chip_text.pos) && rect.width() < 40.0
        })
        .map(|(rect, _)| rect)
        .expect("the count is the shared count chip's fill, not a label colour");
    assert!(
        (band_right - chip.right() - turbogit_ui::ui::components::PAD_LIST).abs() < 8.0,
        "the count is right-aligned on the band's trailing edge — one inset, \
         plus at most the gap reserved for an absent trailing action: chip \
         {chip:?} vs band right {band_right}"
    );
    assert!(
        chip_text.pos.x > label[0].rect.right() + 24.0,
        "and it is not glued to the label the way a label's own count would be: \
         {:?} vs {:?}",
        chip_text.pos,
        label[0].rect
    );
    assert_eq!(chip_text.color, Palette::INK_2, "the count chip's own ink");
    assert!(
        (chip.height() - turbogit_ui::ui::widgets::CHIP_HEIGHT).abs() < 0.01,
        "at the shared chip height, not a 9px badge"
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

/// A band is the top of a block, so it carries a resting fill that is neither a
/// row's hover fill nor a current row's band.
#[test]
fn the_repo_header_paints_a_resting_band() {
    use turbogit_ui::ui::components::{RowState, current_row_fill, row_fill};

    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    // Located by the repository's own name: the band's leading status dot is
    // gone (the mark pair's dot leads the state summary instead), so the band is
    // found the way a reader finds it.
    let band_y = galleys_for(&h, "alpha")[0].pos.y + 6.0;
    let band = filled_rects(&h)
        .into_iter()
        .find(|(rect, _)| rect.contains(Pos2::new(rect.center().x, band_y)) && rect.width() > 600.0)
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

/// Every row leads with an icon so the names line up — local and remote
/// alike — and the names share one left edge down the list.
#[test]
fn every_row_leads_with_an_icon_at_its_name_edge() {
    use test_support::harness::painted_paths;

    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = true;
    let mut h = fixture_harness(fx);
    settle(&mut h);
    // Nothing was ever hovered: `settle` only steps frames.

    assert!(row_buttons(&h) > 0, "the fixture paints branch rows");

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

/// One `Current` marker per repository section — the row's answer to "where am
/// I right now". It is a word in the accent ink on no fill, so a current row
/// carries no second filled shape (see
/// `the_bands_head_ref_is_a_ref_chip_and_the_rows_marker_is_a_word`).
#[test]
fn one_current_marker_per_repo_section() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    // alpha is on `main`, beta on `wip`.
    assert_eq!(
        galleys_for(&h, "Current").len(),
        2,
        "one current marker per repo section: {:?}",
        painted_text(&h)
    );

    let mut h = fixture_harness(Fixture::alpha_only());
    settle(&mut h);
    assert_eq!(
        galleys_for(&h, "Current").len(),
        1,
        "a single section paints a single current marker"
    );
}

/// The current row answers "where am I" with a band across the whole list plus
/// its marker — not with tinted name ink, which was a third marker for the same
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

/// The repository band's head ref is a **neutral ref chip**, and the current
/// row's marker is a **word in the accent text ink** — two different treatments
/// on purpose.
///
/// **Why the assertion moved.** This test used to hold the band chip and the row
/// badge together ("one component, so the same fact looks the same in both
/// places"), because the head ref and the row's marker were both the
/// brand-filled `PillKind::Current`. Ticket 16 splits them: the band names a ref
/// (a neutral ref chip, so the band's blue is gone), the row states "this is the
/// current ref" (the accent text ink, on no fill, because a filled marker there
/// would be the same opaque value a chosen row takes). The proof is therefore
/// two — the band chip's fill is the raised-on-card surface, and the marker
/// paints no chip-sized rect under it at all.
#[test]
fn the_bands_head_ref_is_a_ref_chip_and_the_rows_marker_is_a_word() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    let marker = galleys_for(&h, "Current")
        .into_iter()
        .next()
        .expect("the current row's marker");
    assert_eq!(
        marker.color,
        Palette::ACCENT_TEXT,
        "the current marker is the accent *text* ink, which is the ink the \
         current chip would have used: {marker:#?}"
    );
    // beta's band chip is the *upper* `wip`: the band sits above its row.
    let chip = galleys_for(&h, "wip")
        .into_iter()
        .min_by(|a, b| a.pos.y.total_cmp(&b.pos.y))
        .expect("the repo band's head ref");
    // The **innermost** fill under a galley: of the rects containing this text,
    // the smallest, which is the chip the text is printed in rather than the row
    // band or the chip beside it. The 2px brand rail is a rail, not a chip, and
    // reading it as one would answer the wrong question.
    let fill_under = |pos: Pos2| {
        filled_rects(&h)
            .into_iter()
            .filter(|(rect, _)| rect.contains(pos))
            .min_by_key(|(rect, _)| (rect.width() * rect.height()) as i64)
            .map(|(_, fill)| fill)
    };
    assert_eq!(
        fill_under(chip.pos),
        Some(Palette::RAISED_ON_CARD),
        "the band's head ref is a neutral ref chip, never a brand fill"
    );
    // The marker paints no chip under it at all: the only rects at its origin
    // are the row's own band and the rail, so no row carries a second filled
    // shape.
    let under_marker: Vec<egui::Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(rect, _)| rect.contains(marker.pos))
        .map(|(rect, _)| rect)
        .collect();
    assert!(
        under_marker
            .iter()
            .all(|r| r.width() > 200.0 || r.width() <= 4.0),
        "the current marker is a word, not a second filled shape on the row: \
         painted under it at {under_marker:?}"
    );
}

/// A row that is both current and carrying sync state keeps the state it also
/// carries: the band must not swallow the words.
///
/// **Why the assertion moved.** It used to assert the *tinted chip fills*
/// (`sync_bg(Ahead)` / `sync_bg(Behind)`) survived the band. Ticket 16 deletes
/// that tint entirely — a repository state behind a fill is an R6 violation and
/// the last misuse of the reserved counter orange in the tree — so the words are
/// now painted by the shared mark pair and the assertion is on the words and
/// their ink. The rule the old assertion was reaching for (the band never hides
/// what the row says) is the same rule; the shape of what it says changed.
#[test]
fn a_current_row_keeps_its_state_words() {
    use turbogit_ui::ui::components::SyncKind;

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
    for (label, kind) in [("2 ahead", SyncKind::Ahead), ("1 behind", SyncKind::Behind)] {
        let words = galleys_for(&h, label)
            .into_iter()
            .find(|g| (g.pos.y - row.pos.y).abs() < 4.0)
            .unwrap_or_else(|| panic!("`{label}` states the row's state beside it"));
        assert!(
            (words.pos.y - center_y).abs() < 20.0,
            "`{label}` paints on the current row: {words:#?}"
        );
        assert_eq!(
            words.color,
            sync_ink(kind),
            "the state word wears the one repository-state map's ink"
        );
    }
    // …and neither state ink is ever a *fill* anywhere in the frame.
    let state_fills: Vec<egui::Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(_, fill)| {
            *fill == sync_ink(SyncKind::Ahead) || *fill == sync_ink(SyncKind::Behind)
        })
        .map(|(rect, _)| rect)
        .collect();
    assert!(
        state_fills.is_empty(),
        "a state is coloured text or a leading dot, never a chip fill: \
         {state_fills:#?}"
    );
}

/// Revealing remote rows must not multiply the marker: one current branch is
/// one marker, per repository section.
#[test]
fn revealing_remotes_does_not_add_a_second_current_marker() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = true;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    assert_eq!(
        galleys_for(&h, "Current").len(),
        2,
        "still one marker per section with remotes open: {:?}",
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
fn the_rename_editors_buttons_stay_inside_its_row() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    fx.tree.selected = Some("wip".to_string());
    fx.tree.selected_root = Some(RootId(Arc::from(PathBuf::from("/beta"))));
    fx.tree.renaming = Some("wip".to_string());
    fx.tree.rename_draft = "renamed-wip".to_string();
    let mut h = fixture_harness(fx);
    settle(&mut h);

    // The list's own right edge, read off a sibling row at full width.
    let row_right = button(&h, "main").rect().right();
    let apply = button(&h, "Apply rename");
    assert!(
        apply.rect().right() <= row_right + 0.5,
        "Apply must sit inside the row (its edge {:?} vs the row's {row_right:?})",
        apply.rect()
    );
    // …and so it answers a click: the button is the mouse path to committing.
    apply.click();
    h.step();
    assert!(
        h.state()
            .events
            .iter()
            .any(|e| matches!(e, TreeEvent::RenameCommitted { new, .. }
                if new == "renamed-wip")),
        "{:?}",
        h.state().events
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
    // Commit via the keyboard path (Enter on the editor), which the row's
    // Apply button also answers — see
    // `the_rename_editors_buttons_stay_inside_its_row` for the mouse path.
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
fn double_clicking_a_row_emits_activation_with_its_owner() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    // Two presses 0.1 s of *simulated* time apart. The harness's own step
    // advances the clock by more than egui's 0.3 s double-click window, so
    // the input time is stated here rather than left to the frame cadence.
    for i in 0..2 {
        *h.input_mut() = egui::RawInput {
            time: Some(1.0 + 0.1 * i as f64),
            ..Default::default()
        };
        button(&h, "wip").click();
        h.step();
    }
    let events = h.state().events.clone();
    assert!(
        events.contains(&TreeEvent::RowActivated {
            root: RootId(Arc::from(PathBuf::from("/beta"))),
            branch: "wip".to_string(),
        }),
        "double-click activates, and names its repository: {events:#?}"
    );
}

/// The row's status badges are its own, permanently: a hover must not blank
/// them. The context menu is the only place a row's actions live.
#[test]
fn a_hovered_row_keeps_painting_its_sync_badges() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    assert_eq!(
        galleys_for(&h, "2 ahead").len(),
        1,
        "the unhovered row states its sync state"
    );
    button(&h, "wip").hover();
    h.step();
    h.step();
    for label in ["2 ahead", "1 behind"] {
        assert_eq!(
            galleys_for(&h, label).len(),
            1,
            "hovering must not take the badges away: {:?}",
            painted_text(&h)
        );
    }
    assert_eq!(
        count_label(&h, "Checkout in beta"),
        0,
        "and no button takes their slot"
    );
}

// --- 16: the row's own vocabulary — fill, rail, name chip, state words --------
//
// R1/R4/R6 at the render seam. Every claim here is made against **values** and
// against **geometry**, never against "it looks blue": the row-selected token
// and the selection token are close in hue, so a hue-shaped assertion passes on
// both and proves nothing. And every galley is located by its row's rect,
// because a branch name paints more than once in a frame.

/// The rect painted with `fill` in a row-height band, or `None`.
///
/// Row bands are found by their own fill rather than by counting rows: this is
/// the assertion that says *which* row it is about, so deriving it from the row
/// order would make the geometry depend on the thing being checked.
fn row_band(harness: &Harness<'_, Fixture>, fill: egui::Color32, who: &str) -> egui::Rect {
    let hits: Vec<egui::Rect> = filled_rects(harness)
        .into_iter()
        .filter(|(rect, painted)| {
            *painted == fill && (rect.height() - BRANCH_ROW_H).abs() < 0.5 && rect.width() > 500.0
        })
        .map(|(rect, _)| rect)
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one {who} row band in {fill:?}, found {hits:?}"
    );
    hits.into_iter().next().expect("one row band")
}

/// The 2px brand rail at `row`'s leading edge, if the row carries one.
///
/// Matched on the leading edge **and** the row's own vertical band, because a
/// marked list has more than one rail in it and a rail that merely shares a
/// height would answer about a different row.
fn rail_at(harness: &Harness<'_, Fixture>, row: egui::Rect) -> Option<egui::Rect> {
    filled_rects(harness)
        .into_iter()
        .find(|(rect, fill)| {
            *fill == Palette::BRAND
                && (rect.top() - row.top()).abs() < 0.5
                && (rect.height() - row.height()).abs() < 0.5
                && (rect.left() - row.left()).abs() < 0.5
        })
        .map(|(rect, _)| rect)
}

/// The first painted text origin inside `row`, left to right.
fn first_text_origin(harness: &Harness<'_, Fixture>, row: egui::Rect) -> f32 {
    let mut on_row: Vec<PaintedGalley> = painted_galleys(harness)
        .into_iter()
        .filter(|g| row.contains(g.pos))
        .collect();
    on_row.sort_by(|a, b| a.pos.x.total_cmp(&b.pos.x));
    on_row
        .first()
        .map(|g| g.pos.x)
        .unwrap_or_else(|| panic!("no text painted inside {row:?}"))
}

/// **The selected and the current row both take a 2px brand rail at the leading
/// edge** — the one accent rail, from the one painter, and never hand-rolled.
///
/// The two are picked apart by their *fills*, which is the point: the selected
/// row takes the list-row fill, the current row takes the current ref's own
/// resting band, and each answers with a value rather than a hue.
#[test]
fn the_selected_and_current_rows_take_the_row_selected_fill_and_a_brand_rail() {
    use turbogit_ui::ui::components::RowState;

    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    // Select a plain row, so the frame holds a selected row *and* the current
    // row at once and both claims are about a real paint.
    let events = click_events(&mut h, "starred");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, TreeEvent::RowClicked { .. })),
        "{events:#?}"
    );
    settle(&mut h);

    let selected = row_band(&h, Palette::ROW_SELECTED, "the selected list row");
    let current = row_band(
        &h,
        current_row_fill(RowState::Default),
        "the current ref's resting band",
    );
    assert_ne!(selected, current, "two different rows, two different fills");

    for (row, who) in [(selected, "selected"), (current, "current")] {
        let rail = rail_at(&h, row)
            .unwrap_or_else(|| panic!("the {who} row takes no brand rail at its leading edge"));
        assert_eq!(
            rail.width(),
            turbogit_ui::theme::RAIL_WIDTH,
            "the {who} row's rail is the token width, drawn at the leading edge"
        );
        assert_eq!(rail.left(), row.left(), "flush with the row's leading edge");
        assert!(
            (rail.top() - row.top()).abs() < 0.5 && (rail.bottom() - row.bottom()).abs() < 0.5,
            "the {who} row's rail is the row's full height: rail {rail:?} vs row {row:?}"
        );
    }
}

/// The rail is **paint, not layout**. A selected row's first text origin is an
/// unselected row's, origin for origin — which is the assertion a rail reserved
/// as padding breaks.
#[test]
fn selecting_a_row_does_not_move_its_first_text() {
    use turbogit_ui::ui::components::RowState;

    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let before = row_band(
        &h,
        current_row_fill(RowState::Default),
        "the current ref's resting band",
    );
    assert!(
        !filled_rects(&h)
            .iter()
            .any(|(rect, f)| *f == Palette::ROW_SELECTED && rect.width() > 500.0),
        "nothing is selected yet, so no row takes the selected-row fill"
    );
    let name_before = first_text_origin(&h, before);

    click_events(&mut h, "starred");
    settle(&mut h);

    let after = row_band(
        &h,
        current_row_fill(RowState::Default),
        "the current ref's resting band",
    );
    let selected = row_band(&h, Palette::ROW_SELECTED, "the selected list row");
    assert_eq!(
        before, after,
        "selecting a row must not resize or move any other row"
    );
    assert_eq!(
        first_text_origin(&h, after),
        name_before,
        "and must not move the current row's content"
    );
    assert_eq!(
        first_text_origin(&h, selected) - selected.left(),
        first_text_origin(&h, after) - after.left(),
        "a selected row's first text origin, measured from its own row, is an \
         unselected row's: the rail is paint, not padding"
    );
}

/// **No list row in this pane paints a solid selection-token fill** — not at
/// rest, not hovered, and not when the *current* row is the selected one, which
/// is the state in which the current-ref band used to be reached.
#[test]
fn no_list_row_in_the_tree_paints_the_selection_token() {
    use turbogit_ui::ui::components::RowState;

    // (1) The current row selected: the exact state whose band was the selection
    // token before this ticket.
    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);
    click_events(&mut h, "main");
    settle(&mut h);
    assert!(
        h.state().tree.selected.as_deref() == Some("main"),
        "the current row is the selected one"
    );

    for (token, who) in [
        (Palette::SELECTION, "the selection token"),
        (Palette::selection_bg(), "the focus band composite"),
    ] {
        let hits: Vec<egui::Rect> = filled_rects(&h)
            .into_iter()
            .filter(|(_, fill)| *fill == token)
            .map(|(rect, _)| rect)
            .collect();
        assert!(
            hits.is_empty(),
            "{who} painted as a row band at {hits:?}: a chosen row takes \
             ROW_SELECTED, and the current ref's resting band is {:?}",
            current_row_fill(RowState::Default)
        );
    }
    // …and the row that *is* the selected current row takes the list-row fill, so
    // "the current ref" and "the chosen row" cannot be two different blues.
    let selected = row_band(&h, Palette::ROW_SELECTED, "the selected list row");
    assert!(
        rail_at(&h, selected).is_some(),
        "…and it keeps its rail, so the current row is still marked as current"
    );
}

/// **No row carries two filled pills.** A row's chips are counted at the shared
/// chip height, so this is a statement about *shape* first: whatever fills a row
/// paints, at most one of them is a chip.
#[test]
fn no_branch_row_carries_two_filled_pills() {
    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let chip_h = turbogit_ui::ui::widgets::CHIP_HEIGHT;
    for row in row_buttons_rects(&h) {
        let chips: Vec<(egui::Rect, egui::Color32)> = filled_rects(&h)
            .into_iter()
            .filter(|(rect, _)| (rect.height() - chip_h).abs() < 0.01 && row.contains_rect(*rect))
            .collect();
        assert!(
            chips.len() <= 1,
            "row {row:?} carries {} filled pills at {chips:?}: the name is a ref \
             chip and the current marker is a word, so a row has at most one \
             filled shape",
            chips.len()
        );
        // And the one it may carry is the ref chip, on the raised-on-card fill.
        for (rect, fill) in chips {
            assert_eq!(
                fill,
                Palette::RAISED_ON_CARD,
                "the only filled shape on a row is a ref chip, and a ref chip \
                 fills the raised-on-card surface: {rect:?} filled {fill:?}"
            );
        }
    }
}

/// **The ref chip's fill is the raised-on-card surface**, at the shared chip
/// height, in the data face, inked in the row's own ink — and the pane paints no
/// brand-filled chip at all, so the one blue object left is the New Branch
/// button.
#[test]
fn a_branch_name_is_painted_on_the_raised_on_card_ref_chip_fill() {
    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let name = galleys_for(&h, "starred")
        .into_iter()
        .next()
        .expect("the row's name paints");
    let chip = filled_rects(&h)
        .into_iter()
        .filter(|(rect, _)| rect.contains(name.pos))
        .min_by_key(|(rect, _)| (rect.width() * rect.height()) as i64)
        .expect("the name sits on a filled shape");
    assert_eq!(
        chip.1,
        Palette::RAISED_ON_CARD,
        "a branch name is a ref chip, and a ref chip fills the raised-on-card \
         surface: {chip:?}"
    );
    assert_eq!(
        chip.0.height(),
        turbogit_ui::ui::widgets::CHIP_HEIGHT,
        "at the shared chip height"
    );
    assert_eq!(
        name.color,
        row_ink(false),
        "and in the row's own ink — a name is identity, not state"
    );
    assert_eq!(
        name.family,
        egui::FontFamily::Monospace,
        "in the data face, because it holds a ref name"
    );

    // The whole pane: no chip anywhere fills the brand token.
    let brand_chips: Vec<egui::Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::BRAND)
        .map(|(rect, _)| rect)
        .filter(|rect| (rect.height() - turbogit_ui::ui::widgets::CHIP_HEIGHT).abs() < 0.01)
        .collect();
    assert!(
        brand_chips.is_empty(),
        "no chip in the pane fills the brand token: {brand_chips:?}. The current \
         chip is the only one allowed near the accent, and this row is not it."
    );
}

/// Every full-width branch row's rect.
fn row_buttons_rects(harness: &Harness<'_, Fixture>) -> Vec<egui::Rect> {
    harness
        .get_all_by_role(Role::Button)
        .filter(|n| {
            n.rect().width() > 500.0
                && (n.rect().height() - BRANCH_ROW_H).abs() < 0.5
                && !matches!(
                    n.accesskit_node().label().as_deref(),
                    Some("Local") | Some("Tags") | Some("Remote")
                )
        })
        .map(|n| n.rect())
        .collect()
}

// --- the pure builder stays pure ------------------------------------------------

/// **The pure branches tree builder is git-free and styling-free.** The claim
/// was a doc comment and a convention; a renderer change is exactly the kind of
/// edit that breaks it by accident, so it is asserted.
///
/// Scoped, deliberately, to what the contract actually says: **no egui
/// reference at all**, **no import of any `theme` item but the one
/// `RepoState` re-export**, and **no `Palette::*` reference**. The re-export is
/// allowed because it carries an enum, not a colour: the builder derives a
/// repository's *state* and hands the renderer a value to look up, which keeps
/// the git-free property intact while leaving every colour decision in the
/// renderer. It is the one crate-local reference the module is allowed, so a
/// second one fails here.
#[test]
fn the_pure_branches_tree_builder_holds_no_egui_and_no_styling() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/branches_tree.rs"),
    )
    .expect("read the pure builder");

    // (1) No egui. Not `egui::`, not a `Painter`, not a colour type, not a
    // `Sense`: the builder has no window to draw in and no state to react to.
    for forbidden in ["egui", "Painter", "Color32", "Sense", "Rect", "Painter"] {
        assert!(
            !src.contains(forbidden),
            "the pure builder must hold no `{forbidden}` reference: it has no \
             window, no painter and no input, and every one of those would take \
             it a layer it does not belong to"
        );
    }
    // (2) The only `theme` reference is the one permitted enum re-export.
    let theme_refs: Vec<&str> = src
        .lines()
        .filter(|line| line.contains("theme"))
        .map(|line| line.trim())
        .collect();
    assert_eq!(
        theme_refs,
        ["pub use crate::theme::RepoState as RepoStatus;"],
        "the builder's one styling-adjacent reference is the repository-state \
         enum it re-exports. Anything else it reaches into the theme for is a \
         colour decision made in the wrong layer — found at {theme_refs:?}"
    );
    // (3) No `Palette::*` at all: the enum carries no colour, and a value that
    // carries a colour is a rendering decision, not a tree.
    assert!(
        !src.contains("Palette"),
        "the pure builder names no Palette item: the state enum is re-exported \
         so the *renderer* decides what a state looks like"
    );
    // (4) And it is still git-free: no engine, no `GitExecutor`, no `dispatch`.
    for forbidden in [
        "GitExecutor",
        "dispatch",
        "turbogit_engine",
        "turbogit_services",
    ] {
        assert!(
            !src.contains(forbidden),
            "the pure builder must stay git-free: `{forbidden}` would put it \
             across the engine seam"
        );
    }
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

// --- right-click asks for the context menu (branch-context-menu ticket 03) ----

/// A right-click on a branch row reports the owning repository and the
/// branch, exactly like every other row event.
#[test]
fn a_right_click_on_a_row_asks_for_the_context_menu() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    button(&h, "wip").click_secondary();
    h.step();
    let events = h.state().events.clone();
    assert!(
        events.contains(&TreeEvent::ContextMenuRequested {
            root: RootId(Arc::from(PathBuf::from("/beta"))),
            branch: "wip".to_string(),
        }),
        "a right-click reports its row: {events:#?}"
    );
}

/// The capability is a prop: a surface that does not allow the menu (the Git
/// Log pane today) emits nothing on a right-click.
#[test]
fn a_surface_without_the_capability_emits_nothing_on_right_click() {
    let mut fx = Fixture::two_repos();
    fx.allows_context_menu = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    button(&h, "wip").click_secondary();
    h.step();
    let events = h.state().events.clone();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, TreeEvent::ContextMenuRequested { .. })),
        "the shared component must not open a menu the surface forbade: {events:#?}"
    );
}

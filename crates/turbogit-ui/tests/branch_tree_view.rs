use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use egui::accesskit::Role;
use egui::{Color32, Pos2, Shape};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use egui_kittest::{Harness, Node};
use test_support::harness::{
    PaintedGalley, filled_circles, filled_rects, painted_galleys, painted_text, settle,
};
use turbogit_app::state::TreeState;
use turbogit_domain::model::{Branch, BranchKind, Root, RootId, Upstream};
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::branch_tree_view::{
    TreeEvent, TreeGroup, TreeProps, branch_tree, remotes_revealed,
};
use turbogit_ui::ui::branches_tree::{BranchView, build_branch_view};
use turbogit_ui::ui::components::{BRANCH_ROW_H, current_row_fill, row_ink, sync_ink};

fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(1_780_315_200, 0).expect("valid now")
}

fn local(name: &str, favorite: bool, ahead: usize, behind: usize) -> Branch {
    Branch {
        name: name.to_string(),
        kind: BranchKind::Local,
        tracking: if turbogit_ui::theme::RepoState::has_drift(ahead, behind) {
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
        last_touched: Some(now() - chrono::Duration::days(5)),
        tip: None,
        remote: None,
    }
}

fn stale_local(name: &str) -> Branch {
    let mut b = local(name, false, 0, 0);
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
    collapse_remotes_by_default: bool,
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
        empty.head = None;
        Self {
            roots: vec![empty],
            tags: HashMap::new(),
            ..Self::two_repos()
        }
    }

    fn alpha_with_remotes() -> Self {
        let mut fx = Self::alpha_only();
        fx.tree.show_remotes = true;
        fx
    }

    fn alpha_revealed_in_two_repos() -> Self {
        let mut fx = Self::two_repos();
        fx.tree
            .remotes_revealed
            .insert(RootId(Arc::from(PathBuf::from("/alpha"))));
        fx
    }

    fn view(&self) -> BranchView {
        build_branch_view(
            &self.roots,
            &self.tags,
            &std::collections::HashMap::new(),
            &|root| remotes_revealed(&self.tree, root),
        )
    }

    fn has_any_data(&self) -> bool {
        self.roots
            .iter()
            .any(|r| !r.branches.is_empty() || r.head.is_some())
    }

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

fn fixture_harness(fx: Fixture) -> Harness<'static, Fixture> {
    fixture_harness_at(fx, 720.0)
}

fn fixture_harness_at(fx: Fixture, width: f32) -> Harness<'static, Fixture> {
    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, fx| {
            turbogit_ui::theme::configure_style(ui.ctx());
            if !fonts_installed {
                turbogit_ui::theme::install_fonts(ui.ctx());
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

#[track_caller]
fn click_events(harness: &mut Harness<'_, Fixture>, label: &str) -> Vec<TreeEvent> {
    button(harness, label).click();
    harness.step();
    harness.state().events.clone()
}

fn galleys_for(harness: &Harness<'_, Fixture>, text: &str) -> Vec<PaintedGalley> {
    painted_galleys(harness)
        .into_iter()
        .filter(|g| g.text == text)
        .collect()
}

fn accent_paths(harness: &Harness<'_, Fixture>) -> Vec<(egui::Rect, egui::Stroke)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Path(path)
                if path.stroke.color == egui::epaint::ColorMode::Solid(Palette::ACCENT_TEXT) =>
            {
                Some((
                    path.visual_bounding_rect(),
                    egui::Stroke::new(path.stroke.width, Palette::ACCENT_TEXT),
                ))
            }
            _ => None,
        })
        .collect()
}

fn painted_contains(harness: &Harness<'_, Fixture>, needle: &str) -> bool {
    painted_text(harness).iter().any(|t| t.contains(needle))
}

#[test]
fn rows_render_per_repository_with_their_state_in_words_alone() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let texts = painted_text(&h);
    assert!(texts.iter().any(|t| t.contains("alpha")), "{texts:#?}");
    assert!(texts.iter().any(|t| t.contains("beta")));
    assert!(texts.iter().any(|t| t.contains("wip")));

    for repo in ["alpha", "beta"] {
        let name = galleys_for(&h, repo)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("the `{repo}` band names its repository"));
        let dots: Vec<_> = filled_circles(&h)
            .into_iter()
            .filter(|(center, r, _)| {
                (center.y - name.pos.y).abs() < 14.0
                    && *r == turbogit_ui::ui::components::STATE_DOT_R
            })
            .collect();
        assert_eq!(
            dots.len(),
            0,
            "no state dot on the `{repo}` band's own line, found {dots:?}"
        );
    }
    let beta_band = galleys_for(&h, "beta")
        .into_iter()
        .next()
        .expect("the beta band names its repository")
        .pos
        .y;
    let summary: Vec<_> = painted_galleys(&h)
        .into_iter()
        .filter(|g| g.text.contains("diverged") && (g.pos.y - beta_band).abs() < 14.0)
        .collect();
    assert_eq!(
        summary.len(),
        1,
        "beta states its state once, on its own band: {summary:#?}"
    );
    assert_eq!(
        summary[0].color,
        Palette::STATUS_DIVERGED,
        "…in that repository's state colour, from `RepoState::color()`"
    );
}

#[test]
fn a_section_header_right_aligns_its_count_as_a_chip_on_a_band() {
    use turbogit_ui::ui::components::{RowState, current_row_fill, row_fill};

    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

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

#[test]
fn a_nested_level_hangs_one_icon_width() {
    use turbogit_ui::ui::components::KIT_ICON;

    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let base = galleys_for(&h, "stale-branch")
        .into_iter()
        .next()
        .expect("a depth-0 branch row");
    let child = galleys_for(&h, "a")
        .into_iter()
        .next()
        .expect("a row nested one level deep");
    let step = child.pos.x - base.pos.x;

    assert!(
        (step - KIT_ICON).abs() < 0.6,
        "a level hangs exactly one icon width: step {step}, KIT_ICON {KIT_ICON}"
    );
}

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

#[test]
fn the_repo_header_names_its_status() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);
    let texts = painted_text(&h);

    assert!(
        !texts.iter().any(|t| t == "in sync"),
        "a clean repo's band says nothing: {texts:#?}"
    );
    assert!(
        texts.iter().any(|t| t == "↑2 · ↓1 · diverged"),
        "the counts and the state both belong to the header summary: {texts:#?}"
    );
}

#[test]
fn the_repo_header_paints_a_resting_band() {
    use turbogit_ui::ui::components::{RowState, current_row_fill, row_fill};

    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

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

#[test]
fn directory_grouping_shows_derived_count() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    let texts = painted_text(&h);
    assert!(texts.iter().any(|t| t.contains("feature/")), "{texts:#?}");
    assert!(texts.iter().any(|t| t == "2"), "{texts:#?}");
}

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

#[test]
fn every_row_leads_with_an_icon_at_its_name_edge() {
    use test_support::harness::painted_paths;

    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = true;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    assert!(row_buttons(&h) > 0, "the fixture paints branch rows");

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

fn count_label(h: &Harness<'_, Fixture>, label: &str) -> usize {
    h.get_all_by_role(Role::Button)
        .filter(|n| n.accesskit_node().label().as_deref() == Some(label))
        .count()
}

type CurrentMarkerRow = (&'static str, fn() -> Fixture, bool, usize);

const CURRENT_MARKERS: [CurrentMarkerRow; 3] = [
    (
        "two repositories, remotes rolled up",
        Fixture::two_repos,
        false,
        2,
    ),
    ("one repository", Fixture::alpha_only, false, 1),
    (
        "two repositories with their remotes revealed",
        Fixture::two_repos,
        true,
        2,
    ),
];

#[test]
fn one_current_marker_per_repo_section() {
    for &(case, fixture, show_remotes, expected) in &CURRENT_MARKERS {
        let mut fx = fixture();
        fx.tree.show_remotes = show_remotes;
        let mut h = fixture_harness(fx);
        settle(&mut h);

        assert_eq!(
            accent_paths(&h).len(),
            expected,
            "{case}: one current marker per repo section: {:?}",
            painted_text(&h)
        );
    }
}

#[test]
fn current_row_bands_the_whole_list_and_keeps_plain_name_ink() {
    const LIST_W: f32 = 720.0;
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

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

#[test]
fn the_bands_head_ref_is_a_ref_chip_and_the_rows_marker_is_an_icon() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

    let marker_rect = accent_paths(&h)
        .into_iter()
        .next()
        .expect("the current row's marker, stroked in the accent text ink")
        .0;
    let chip = galleys_for(&h, "wip")
        .into_iter()
        .min_by(|a, b| a.pos.y.total_cmp(&b.pos.y))
        .expect("the repo band's head ref");
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
    let under_marker: Vec<egui::Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(rect, _)| rect.contains(marker_rect.center()))
        .map(|(rect, _)| rect)
        .collect();
    assert!(
        under_marker
            .iter()
            .all(|r| r.width() > 200.0 || r.width() <= 4.0),
        "the current marker is an icon, not a second filled shape on the row: \
         painted under it at {under_marker:?}"
    );
}

#[test]
fn a_current_row_keeps_its_state_marks() {
    use turbogit_ui::ui::components::SyncKind;

    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let row = galleys_for(&h, "wip")
        .into_iter()
        .max_by(|a, b| a.pos.y.total_cmp(&b.pos.y))
        .expect("the current row");
    let center_y = row.pos.y + 6.0;
    for (label, kind) in [("↑2", SyncKind::Ahead), ("↓1", SyncKind::Behind)] {
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
            "the state mark wears the one repository-state map's ink"
        );
        assert_eq!(
            words.family,
            egui::FontFamily::Monospace,
            "the badge is one mark, so one face must own both its glyphs; in the \
             chrome face the arrow only arrives through a fallback cut"
        );
    }
    let state_dots: Vec<_> = filled_circles(&h)
        .into_iter()
        .filter(|(center, radius, fill)| {
            (center.y - row.pos.y).abs() < 20.0
                && (*radius - turbogit_ui::ui::components::STATE_DOT_R).abs() < f32::EPSILON
                && (*fill == sync_ink(SyncKind::Ahead) || *fill == sync_ink(SyncKind::Behind))
        })
        .collect();
    assert!(
        state_dots.is_empty(),
        "no leading dot on a row's state, only its coloured words: {state_dots:#?}"
    );
    let state_fills: Vec<egui::Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(_, fill)| {
            *fill == sync_ink(SyncKind::Ahead) || *fill == sync_ink(SyncKind::Behind)
        })
        .map(|(rect, _)| rect)
        .collect();
    assert!(
        state_fills.is_empty(),
        "a state is coloured text, never a chip fill: {state_fills:#?}"
    );
}

#[test]
fn favorite_pins_above_the_current_branch_row() {
    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    let starred_y = galleys_for(&h, "starred")[0].pos.y;
    let main_ys: Vec<f32> = galleys_for(&h, "main").iter().map(|g| g.pos.y).collect();
    assert!(
        main_ys.iter().any(|y| *y > starred_y),
        "favorite `starred` ({starred_y}) must sit above a `main` row (chips+rows at {main_ys:?})"
    );
}

type GroupToggleRow = (
    &'static str,
    &'static str,
    TreeGroup,
    &'static str,
    bool,
    bool,
);

const GROUP_TOGGLES: [GroupToggleRow; 2] = [
    (
        "Tags opens on the first click",
        "Tags",
        TreeGroup::Tags,
        "v1.0",
        false,
        true,
    ),
    (
        "Local collapses on the first click",
        "Local",
        TreeGroup::Local,
        "starred",
        true,
        false,
    ),
];

#[test]
fn a_group_header_click_emits_its_own_group_and_applies_it() {
    for &(case, label, group, needle, before, after) in &GROUP_TOGGLES {
        let mut h = fixture_harness(Fixture::alpha_only());
        settle(&mut h);

        assert_eq!(
            painted_contains(&h, needle),
            before,
            "{case}: before the click, `{needle}` painted = {before}: {:?}",
            painted_text(&h)
        );

        let events = click_events(&mut h, label);
        assert!(
            events.contains(&TreeEvent::GroupToggled(group)),
            "{case}: the `{label}` header click must emit GroupToggled({group:?}): \
             {events:#?}"
        );
        settle(&mut h);

        assert_eq!(
            painted_contains(&h, needle),
            after,
            "{case}: after the caller applies the event, `{needle}` painted = \
             {after}: {:?}",
            painted_text(&h)
        );
    }
}

type PerRemoteCollapseRow = (&'static str, fn() -> Fixture);

const PER_REMOTE_COLLAPSE: [PerRemoteCollapseRow; 2] = [
    ("remotes open at the top level", Fixture::alpha_with_remotes),
    (
        "inside an already-revealed repository",
        Fixture::alpha_revealed_in_two_repos,
    ),
];

#[test]
fn remote_header_toggle_emits_event_and_collapses_that_remote() {
    for &(case, fixture) in &PER_REMOTE_COLLAPSE {
        let mut h = fixture_harness(fixture());
        settle(&mut h);
        assert!(
            !galleys_for(&h, "remote-only").is_empty(),
            "{case}: the remote's branches are open before the header is clicked"
        );

        let events = click_events(&mut h, "origin");
        assert!(
            events.contains(&TreeEvent::RemoteToggled {
                root: RootId(Arc::from(PathBuf::from("/alpha"))),
                remote: "origin".to_string(),
            }),
            "{case}: the `origin` header click must emit RemoteToggled for its own \
             (root, remote): {events:#?}"
        );
        settle(&mut h);
        assert!(
            galleys_for(&h, "remote-only").is_empty(),
            "{case}: the collapsed remote's branches hide"
        );
        assert!(
            painted_contains(&h, "origin"),
            "{case}: the collapsed remote's header stays"
        );
    }
}

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

#[test]
fn revealing_remotes_leaves_the_other_repository_rolled_up() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    assert!(
        painted_text(&h)
            .iter()
            .any(|t| t == "1 remote · 0 branches"),
        "beta's own rollup line: {:?}",
        painted_text(&h)
    );
    assert!(galleys_for(&h, "origin").is_empty());

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

#[test]
fn one_click_returns_a_revealed_repository_to_its_rollup() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    button(&h, "Remote").click();
    settle(&mut h);
    assert!(!galleys_for(&h, "origin").is_empty(), "revealed");

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

#[test]
fn remote_rollup_click_emits_the_visibility_switch() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

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

type CreateDeadEndRow = (
    &'static str,
    fn() -> Fixture,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

const CREATE_DEAD_ENDS: [CreateDeadEndRow; 2] = [
    (
        "a filter nothing matches",
        Fixture::two_repos,
        "zzz",
        "no branch called zzz",
        "Create \"zzz\"",
        "zzz",
    ),
    (
        "a repository with no branches at all",
        Fixture::empty_repo,
        "",
        "This repo has no branches yet",
        "Create the first branch",
        "",
    ),
];

#[test]
fn a_dead_end_names_itself_and_offers_to_create_that_name() {
    for &(case, fixture, filter, painted, button, name) in &CREATE_DEAD_ENDS {
        let mut fx = fixture();
        fx.filter = filter.to_string();
        let mut h = fixture_harness(fx);
        settle(&mut h);

        let texts = painted_text(&h);
        assert!(
            texts.iter().any(|t| t.contains(painted)),
            "{case}: the dead end says which name it found nothing for: {texts:#?}"
        );
        let events = click_events(&mut h, button);
        assert_eq!(
            events,
            vec![TreeEvent::CreateBranchRequested {
                name: name.to_string()
            }],
            "{case}: the create event carries the name the dead end stated — the \
             typed one for a no-match, and none at all for the empty state"
        );
    }
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

    let row_right = button(&h, "main").rect().right();
    let apply = button(&h, "Apply rename");
    assert!(
        apply.rect().right() <= row_right + 0.5,
        "Apply must sit inside the row (its edge {:?} vs the row's {row_right:?})",
        apply.rect()
    );
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

#[test]
fn row_click_emits_owner_and_branch_not_a_bare_name() {
    let mut h = fixture_harness(Fixture::two_repos());
    settle(&mut h);

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

#[test]
fn a_hovered_row_keeps_painting_its_sync_badges() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

    assert_eq!(
        galleys_for(&h, "↑2").len(),
        1,
        "the unhovered row states its sync state"
    );
    button(&h, "wip").hover();
    h.step();
    h.step();
    for label in ["↑2", "↓1"] {
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

#[test]
fn the_selected_and_current_rows_take_the_row_selected_fill_and_a_brand_rail() {
    use turbogit_ui::ui::components::RowState;

    let mut fx = Fixture::alpha_only();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

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

#[test]
fn no_list_row_in_the_tree_paints_the_selection_token() {
    use turbogit_ui::ui::components::RowState;

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
    let selected = row_band(&h, Palette::ROW_SELECTED, "the selected list row");
    assert!(
        rail_at(&h, selected).is_some(),
        "…and it keeps its rail, so the current row is still marked as current"
    );
}

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

#[test]
fn the_pure_branches_tree_builder_holds_no_egui_and_no_styling() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/branches_tree.rs"),
    )
    .expect("read the pure builder");

    for forbidden in ["egui", "Painter", "Color32", "Sense", "Rect", "Painter"] {
        assert!(
            !src.contains(forbidden),
            "the pure builder must hold no `{forbidden}` reference: it has no \
             window, no painter and no input, and every one of those would take \
             it a layer it does not belong to"
        );
    }
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
    assert!(
        !src.contains("Palette"),
        "the pure builder names no Palette item: the state enum is re-exported \
         so the *renderer* decides what a state looks like"
    );
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

#[test]
fn repo_header_fetch_emits_a_fetch_request() {
    let mut fx = Fixture::two_repos();
    fx.tree.show_remotes = false;
    let mut h = fixture_harness(fx);
    settle(&mut h);

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

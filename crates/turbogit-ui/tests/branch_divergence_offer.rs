use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Color32, Rect, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use test_support::git_seed::git;
use test_support::harness::{
    filled_rects, painted_galleys, painted_text, shell_harness_over_unstyled,
};
use turbogit_app::state::{AppState, Tab};
use turbogit_services::bulk_ops::{BulkOp, BulkPlan};
use turbogit_ui::theme::Palette;

fn temp_repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q", "-b", "main"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    let bare = parent.join(format!("{name}.origin"));
    let _ = std::fs::remove_dir_all(&bare);
    git(
        parent,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    git(&path, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&path, &["push", "-q", "origin", "main"]);
    path
}

fn diverging_project(tag: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join(tag);
    let frontend = project.join("frontend");
    std::fs::create_dir_all(&frontend).unwrap();
    let alpha = temp_repo(&frontend, "alpha");
    let broken = temp_repo(&frontend, "broken");
    git(&broken, &["branch", "feature"]);
    (tmp, project, alpha, broken)
}

fn rid(path: &Path) -> turbogit_domain::model::RootId {
    turbogit_domain::model::RootId(path.to_path_buf().into())
}

fn offer_harness(
    tag: &str,
) -> (
    tempfile::TempDir,
    PathBuf,
    PathBuf,
    PathBuf,
    Harness<'static, AppState>,
) {
    let (tmp, project, alpha, broken) = diverging_project(tag);
    let mut state = AppState::for_roots(&project, &[alpha.clone(), broken.clone()]);
    state.ui.repo_selection = vec![rid(&alpha), rid(&broken)].into_iter().collect();
    state.ui.show_status_bar = true;
    state.ui.tab = Tab::Branches;

    let plan = BulkPlan {
        op: BulkOp::CreateBranch,
        roots: vec![rid(&alpha), rid(&broken)],
        rebase: false,
        branch: "feature/x".to_string(),
        command: String::new(),
    };
    state.run_bulk_confirmed(plan);

    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        state.drain_events();
        if state.ui.branch_divergence.is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "the offer never armed");
        std::thread::sleep(Duration::from_millis(10));
    }
    state.ui.bulk_run = None;
    state.drain_events();

    let mut harness = shell_harness_over_unstyled(state, Vec2::new(1280.0, 860.0), 600);
    harness.step();
    harness.step();
    (tmp, project, alpha, broken, harness)
}

fn offer_rect(h: &Harness<'_, AppState>) -> Rect {
    let nodes: Vec<egui_kittest::Node<'_>> = h.query_all_by_label("Branches diverged").collect();
    assert!(!nodes.is_empty(), "the offer window is not queryable");
    let frame = nodes
        .iter()
        .map(|n| n.rect())
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .expect("at least one node rect");
    assert!(
        frame.height() > 40.0,
        "the offer frame must be a window, not a title strip: {frame:?}"
    );
    frame
}

/// Assert `needle` paints inside the offer's own frame.
///
/// Each galley's whitespace is folded before the match, because a wrapped label
/// carries `\n` at every break: a raw `contains` would miss wording that is
/// plainly on screen, and the absence assert would then fail for the wrong reason.
#[track_caller]
fn offer_has(h: &Harness<'_, AppState>, needle: &str) -> bool {
    let region = offer_rect(h);
    painted_galleys(h).into_iter().any(|g| {
        let folded = g.text.split_whitespace().collect::<Vec<_>>().join(" ");
        folded.contains(needle) && region.contains(g.pos)
    })
}

#[track_caller]
fn offer_text(h: &Harness<'_, AppState>) -> Vec<String> {
    let region = offer_rect(h);
    painted_galleys(h)
        .into_iter()
        .filter(|g| region.contains(g.pos))
        .map(|g| g.text)
        .collect()
}

#[track_caller]
fn painted_head_ref(h: &Harness<'_, AppState>, repo: &str) -> String {
    let galleys = painted_galleys(h);
    let head = galleys
        .iter()
        .find(|g| g.text == repo)
        .unwrap_or_else(|| panic!("`{repo}` paints no repo line; {:?}", painted_text(h)));
    let mut on_the_line: Vec<&_> = galleys
        .iter()
        .filter(|g| {
            (g.pos.y - head.pos.y).abs() < 12.0
                && g.pos.x > head.rect.right()
                && g.pos.x - head.rect.right() < 260.0
                && g.text != repo
        })
        .collect();
    assert!(
        !on_the_line.is_empty(),
        "no chip sits beside `{repo}`'s repo line; {:?}",
        painted_text(h)
    );
    on_the_line.sort_by_key(|g| (g.pos.x * 100.0) as i64);
    on_the_line[0].text.clone()
}

fn button_fill(h: &Harness<'_, AppState>, label: &str) -> Color32 {
    let rect = h.get_by_label(label).rect();
    filled_rects(h)
        .into_iter()
        .filter(|(r, _)| {
            (r.min.x - rect.min.x).abs() < 1.5
                && (r.min.y - rect.min.y).abs() < 1.5
                && (r.max.x - rect.max.x).abs() < 1.5
                && (r.max.y - rect.max.y).abs() < 1.5
        })
        .map(|(_, fill)| fill)
        .next()
        .unwrap_or_else(|| panic!("`{label}` paints no filled rect at its own hit rect"))
}

#[test]
fn the_offer_names_both_states_and_the_rollback_verb_with_the_root_identifiers() {
    let (_tmp, _project, _alpha, _broken, harness) = offer_harness("offer-words");
    assert!(
        offer_has(&harness, "has succeeded for the following 1 repository"),
        "the partial-success sentence must pluralise on the root count; painted {:?}",
        offer_text(&harness)
    );
    assert!(
        offer_has(&harness, "alpha") && offer_has(&harness, "now on feature/x"),
        "the changed root and the branch it moved to; painted {:?}",
        offer_text(&harness)
    );
    assert!(
        offer_has(&harness, "broken") && offer_has(&harness, "still on main"),
        "the failed root and the branch it never left; painted {:?}",
        offer_text(&harness)
    );
    assert!(
        offer_has(&harness, "so the branches do not diverge"),
        "the offer must say why; painted {:?}",
        offer_text(&harness)
    );
    assert!(
        offer_has(&harness, "Roll back"),
        "the rollback verb names itself; painted {:?}",
        offer_text(&harness)
    );
}

#[test]
fn the_two_controls_are_distinguishable_by_fill() {
    let (_tmp, _project, _alpha, _broken, harness) = offer_harness("offer-fills");

    let rollback = button_fill(&harness, "Roll back 1 repository");
    let keep = button_fill(&harness, "Keep divergent");

    assert_eq!(
        rollback,
        Palette::ACCENT,
        "the repair carries the brand fill, as every affirmative control in this app does"
    );
    assert_ne!(
        rollback, keep,
        "two identical rects would make the risky answer indistinguishable from the repair — \
         the exact failure the reference avoids by naming the reason in its sentence"
    );
    assert_eq!(
        keep,
        Palette::RAISED,
        "the keep-control is the kit's secondary fill, a surface tone rather than an action tone"
    );
}

#[test]
fn accepting_moves_the_painted_branch_back_and_clears_the_offer() {
    let (_tmp, _project, _alpha, _broken, mut harness) = offer_harness("offer-accept");
    assert_eq!(
        painted_head_ref(&harness, "alpha"),
        "feature/x",
        "the run landed in alpha, and the surface says so before the offer is answered"
    );

    harness.get_by_label("Roll back 1 repository").click();
    for _ in 0..12 {
        harness.step();
    }

    assert_eq!(
        painted_head_ref(&harness, "alpha"),
        "main",
        "the rolled-back root's branch name returns to its pre-operation value on the surface"
    );
    let texts = painted_text(&harness);
    assert!(
        !texts.iter().any(|t| t.contains("Roll back 1 repository")),
        "an answered offer must not stay painted; {texts:#?}"
    );
    assert!(
        harness.state().ui.branch_divergence.is_none(),
        "the app drops the offer with the answer, not with the next frame"
    );
}

#[test]
fn declining_leaves_the_divergence_painted_and_nothing_of_the_offer() {
    let (_tmp, _project, alpha, _broken, mut harness) = offer_harness("offer-decline");

    harness.get_by_label("Keep divergent").click();
    for _ in 0..12 {
        harness.step();
    }

    let texts = painted_text(&harness);
    assert!(
        !texts.iter().any(|t| t.contains("Roll back")),
        "the offer is gone from the surface; {texts:#?}"
    );
    assert!(
        !texts
            .iter()
            .any(|t| t.contains("so the branches do not diverge")),
        "and with it its reason; {texts:#?}"
    );
    assert_eq!(painted_head_ref(&harness, "alpha"), "feature/x");
    assert_eq!(painted_head_ref(&harness, "broken"), "main");
    assert_eq!(
        git(&alpha, &["branch", "--show-current"]).trim(),
        "feature/x",
        "declining touches no repository"
    );
    assert!(
        harness.state().ui.branch_divergence.is_none(),
        "a decline is an answer, so the app stops holding the offer"
    );
}

#[test]
fn the_offer_paints_both_answers_on_every_frame_it_is_held() {
    let (_tmp, _project, _alpha, _broken, mut harness) = offer_harness("offer-both");
    for _ in 0..4 {
        harness.step();
    }
    let texts = painted_text(&harness);
    for label in ["Roll back 1 repository", "Keep divergent"] {
        assert!(
            texts.iter().any(|t| t == label),
            "`{label}` is not on the surface on a later frame; {texts:#?}"
        );
    }
}

#[test]
fn an_offer_whose_roots_left_the_project_stops_painting() {
    let (_tmp, _project, _alpha, _broken, mut harness) = offer_harness("offer-stale");
    assert!(offer_has(&harness, "Roll back"));

    let elsewhere = tempfile::tempdir().expect("other workspace");
    temp_repo(elsewhere.path(), "someone-else");
    harness.state_mut().attach_workspace(elsewhere.path());
    for _ in 0..6 {
        harness.step();
    }

    let texts = painted_text(&harness);
    assert!(
        !texts.iter().any(|t| t.contains("Roll back 1 repository")),
        "the offer of a project nobody has open goes with the project; {texts:#?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("Branches diverged")),
        "and its window with it; {texts:#?}"
    );
}

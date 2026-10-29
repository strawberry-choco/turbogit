use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use test_support::harness::{
    KITTEST_DEFAULT_BOX, assert_not_painted, assert_painted, filled_rects, galley_origin,
    painted_galleys, shell_harness_over_unstyled,
};

use egui::{Color32, Key, Modifiers, Pos2, Rect, Shape, accesskit::Role};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use test_support::git_seed::git;
use test_support::wait_until;
use turbogit_app::state::AppState;
use turbogit_domain::model::RootId;
use turbogit_ui::theme::Palette;

fn git_unchecked(repo: &Path, args: &[&str]) {
    let _ = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output();
}

struct Repo {
    path: PathBuf,
}

fn temp_repo(parent: &Path, name: &str) -> Repo {
    let path = parent.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("base.txt"), "base\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", "init"]);
    Repo { path }
}

impl Repo {
    fn branch(&self) -> String {
        git(&self.path, &["rev-parse", "--abbrev-ref", "HEAD"])
            .trim()
            .to_string()
    }
}

/// Seed `conf.txt` with TWO well-separated conflict hunks on `repo`'s
/// default branch: ours rewrites lines 1 and 9 to `MAIN-*`, theirs to
/// `SIDE-*`. The seven unchanged lines between the edits keep git from
/// coalescing both hunks into a single conflict region.
fn seed_two_conflicts(repo: &Path, branch: &str) {
    std::fs::write(
        repo.join("conf.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "base conf"]);
    git(repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(
        repo.join("conf.txt"),
        "SIDE-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "side commit"]);
    git(repo, &["checkout", "-q", branch]);
    std::fs::write(
        repo.join("conf.txt"),
        "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nMAIN-nine\nten\n",
    )
    .unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "main commit"]);
    git_unchecked(repo, &["merge", "--no-edit", "side"]);
}

const COMPOSED: &str = "MAIN-one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nSIDE-nine\nten\n";

fn app_state(roots: &[PathBuf]) -> AppState {
    let dir = roots
        .first()
        .and_then(|r| r.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    AppState::for_roots(&dir, roots)
}

/// Headless harness driving the full app UI with event draining per frame.
///
/// Unstyled: the panes' tints and widths are read off the frame, and the dark
/// tokens plus embedded JetBrains Mono would re-measure it in another face.
///
/// `max_steps` is 1024, not kittest's default of 4: this suite calls
/// `Harness::run()`, and `run()`, unlike `step()`, enforces the budget, by
/// panicking past it.
fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, KITTEST_DEFAULT_BOX, 1024)
}

fn open_merge_editor(h: &mut Harness<'_, AppState>) {
    h.get_by_label("Merge…").click();
    h.run();
}

fn apply_button<'h>(h: &'h Harness<'_, AppState>) -> egui_kittest::Node<'h> {
    h.get_by_label("Apply")
}

fn tint_over_bg(accent: Color32, t: f32) -> Color32 {
    let bg = Palette::BG;
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgb(
        mix(bg.r(), accent.r()),
        mix(bg.g(), accent.g()),
        mix(bg.b(), accent.b()),
    )
}

fn yours_bg() -> Color32 {
    tint_over_bg(Palette::STATE_INFO, 0.12)
}

fn theirs_bg() -> Color32 {
    tint_over_bg(Palette::STATE_ERROR, 0.12)
}

fn marker_bg() -> Color32 {
    tint_over_bg(Palette::STATE_WARNING, 0.15)
}

fn origin_of(harness: &Harness<'_, AppState>, text: &str) -> Option<Pos2> {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text().trim_end() == text => Some(shape.pos),
            _ => None,
        })
}

fn header_band(harness: &Harness<'_, AppState>, label: &str) -> Rect {
    let pos = galley_origin(harness, label)
        .unwrap_or_else(|| panic!("{label} header label was not painted"));
    filled_rects(harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::RULE_STRUCTURAL && r.left() <= pos.x && pos.x <= r.right())
        .min_by_key(|(r, _)| ((r.top() - pos.y).abs() * 10.0) as i64)
        .map(|(r, _)| r)
        .unwrap_or_else(|| panic!("{label} header band was not painted"))
}

#[test]
fn merge_editor_renders_three_equal_panes_with_tinted_blocks() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "panes-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_merge_editor(&mut h);

    assert_painted(&h, "Local (Yours)");
    assert_painted(&h, "Result");
    assert_painted(&h, "Incoming (Theirs)");

    for label in ["Apply", "Cancel"] {
        assert_eq!(
            h.get_by_label(label).accesskit_node().role(),
            Role::Button,
            "{label} must remain an accessible button"
        );
    }

    assert_painted(&h, "MAIN-one");
    assert_painted(&h, "SIDE-one");
    assert_painted(&h, "<< unresolved >>");

    assert_painted(&h, "2 conflicts remaining");

    let local = header_band(&h, "Local (Yours)");
    let result = header_band(&h, "Result");
    let incoming = header_band(&h, "Incoming (Theirs)");
    assert!(
        (local.width() - result.width()).abs() <= 6.0
            && (result.width() - incoming.width()).abs() <= 6.0,
        "panes must be equal width, got widths {}/{}/{}",
        local.width(),
        result.width(),
        incoming.width()
    );

    let ours_pos = origin_of(&h, "MAIN-one").expect("ours text painted");
    let theirs_pos = origin_of(&h, "SIDE-one").expect("theirs text painted");
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == yours_bg() && r.contains(ours_pos)),
        "yours section must be tinted behind its text"
    );
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == theirs_bg() && r.contains(theirs_pos)),
        "theirs section must be tinted behind its text"
    );

    assert!(
        filled_rects(&h).iter().any(|(_, c)| *c == marker_bg()),
        "conflict marker strips must be painted"
    );

    let result_pos = galley_origin(&h, "Result").expect("Result header painted");
    assert!(
        filled_rects(&h).iter().any(|(r, c)| {
            *c == Palette::BRAND
                && (r.width() - turbogit_ui::theme::RAIL_WIDTH).abs() < 0.01
                && r.right() <= result_pos.x
                && r.top() <= result_pos.y
                && result_pos.y <= r.bottom()
        }),
        "Result pane must carry the shared 2pt BRAND focus rail beside its header, \
         beside {result_pos:?}: {:?}",
        filled_rects(&h)
            .into_iter()
            .filter(|(_, c)| *c == Palette::BRAND)
            .collect::<Vec<_>>()
    );
}

#[test]
fn accept_buttons_resolve_blocks_update_read_only_result_and_counter() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "resolve-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_merge_editor(&mut h);
    assert_eq!(
        h.get_by_label("Accept Yours 1").accesskit_node().role(),
        Role::Button,
        "per-block resolution must remain an accessible button"
    );

    h.get_by_label("Accept Yours 1").click();
    h.run();
    assert_painted(&h, "1 conflict remaining");
    assert_eq!(h.state().ui.conflict_res[0], Some(0));

    h.get_by_label("Accept Theirs 2").click();
    h.run();
    assert_painted(&h, "0 conflicts remaining");
    assert_eq!(
        h.state().ui.conflict_res,
        vec![Some(0), Some(1)],
        "per-block choices must land in conflict_res"
    );
    assert_eq!(
        h.state().ui.conflict_text,
        COMPOSED,
        "resolutions must compose the read-only Result"
    );
    assert_not_painted(&h, "<< unresolved >>");

    assert!(
        h.query_by_label(COMPOSED).is_none(),
        "the composed result must not be a single (editable) widget value"
    );
    let before = h.state().ui.conflict_text.clone();
    h.key_press_modifiers(Modifiers::NONE, Key::A);
    h.run();
    assert_eq!(
        h.state().ui.conflict_text,
        before,
        "free-text editing is deferred: input must not change the buffer"
    );
}

#[test]
fn apply_is_gated_until_all_blocks_resolved() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "gate-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_merge_editor(&mut h);

    assert!(
        apply_button(&h).accesskit_node().is_disabled(),
        "Apply must start disabled while conflicts remain"
    );

    h.get_by_label("Accept Yours 1").click();
    h.run();
    assert!(
        apply_button(&h).accesskit_node().is_disabled(),
        "Apply must stay disabled while any block is unresolved"
    );

    h.get_by_label("Accept Theirs 2").click();
    h.run();
    assert!(
        !apply_button(&h).accesskit_node().is_disabled(),
        "Apply must enable at zero conflicts remaining"
    );
}

#[test]
fn apply_writes_composed_result_and_clears_conflict_state() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "apply-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_merge_editor(&mut h);

    h.get_by_label("Accept Yours 1").click();
    h.get_by_label("Accept Theirs 2").click();
    h.run();
    apply_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || {
            std::fs::read_to_string(repo.path.join("conf.txt"))
                .map(|s| s == COMPOSED)
                .unwrap_or(false)
                && !git(&repo.path, &["status", "--porcelain"])
                    .lines()
                    .any(|l| l.starts_with("UU"))
        }),
        "Apply must write the composed result and clear the unmerged state, got:\n{}",
        git(&repo.path, &["status", "--porcelain"])
    );

    let root_id = RootId(repo.path.clone().into());
    let start = Instant::now();
    loop {
        h.run();
        let cleared = h.state().ui.conflict_open.is_none()
            && h.state()
                .multi
                .by_id(&root_id)
                .map(|r| r.status.conflicted.is_empty())
                .unwrap_or(false);
        if cleared {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "conflict state must clear after Apply (open={:?})",
            h.state().ui.conflict_open
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn ordinal_three_places_theirs_before_ours_in_the_read_only_result() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "both-theirs-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_merge_editor(&mut h);

    h.state_mut().ui.conflict_res[0] = Some(3);
    h.run();

    let cell = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text.contains("SIDE-one") && g.text.contains("MAIN-one"))
        .unwrap_or_else(|| panic!("no Result cell carries both sides of conflict 1"));
    let theirs = cell.text.find("SIDE-one").expect("theirs in the cell");
    let ours = cell.text.find("MAIN-one").expect("ours in the cell");
    assert!(
        theirs < ours,
        "ordinal 3 must read theirs before ours in the read-only Result, got:\n{}",
        cell.text
    );

    assert_painted(&h, "1 conflict remaining");
}

#[test]
fn ignore_keeps_both_sides_and_counts_as_resolved() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "ignore-repo");
    let branch = repo.branch();
    seed_two_conflicts(&repo.path, &branch);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    open_merge_editor(&mut h);

    h.get_by_label("Ignore 1").click();
    h.run();

    assert_painted(&h, "1 conflict remaining");
    assert_eq!(h.state().ui.conflict_res[0], Some(2));
    let text = &h.state().ui.conflict_text;
    assert!(
        text.contains("MAIN-one") && text.contains("SIDE-one"),
        "Ignore must keep both sides of the block, got {text:?}"
    );
}

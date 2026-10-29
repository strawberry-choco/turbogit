use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use std::path::{Path, PathBuf};
use test_support::git_seed::git;
use test_support::harness::{
    assert_not_painted, assert_painted, click_with_modifiers, filled_rects, galley_origin,
    painted_galleys, painted_ink, painted_paths, painted_text, shell_harness_over_unstyled,
    stroked_rects,
};
use test_support::wait_until;
use turbogit_app::state::{AppState, CommitSubTab, Dialog};
use turbogit_domain::model::{ChangeRef, RootId};
use turbogit_ui::theme::Palette;
use turbogit_ui::ui::widgets::{CHIP_HEIGHT, PANE_HEADER_HEIGHT};

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
    fn subjects(&self) -> Vec<String> {
        git(&self.path, &["log", "--format=%s"])
            .lines()
            .map(str::to_string)
            .collect()
    }
    fn commit_count(&self) -> usize {
        git(&self.path, &["rev-list", "--count", "HEAD"])
            .trim()
            .parse()
            .unwrap()
    }
}

fn seed_changes(repo: &Path) {
    std::fs::write(repo.join("base.txt"), "modified content\n").unwrap();
    std::fs::write(repo.join("added.txt"), "new tracked file\n").unwrap();
    git(repo, &["add", "added.txt"]);
    std::fs::write(repo.join("untracked.txt"), "untracked\n").unwrap();
}

fn seed_conflict(repo: &Path, branch: &str) {
    git(repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("conf.txt"), "side\n").unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "side commit"]);
    git(repo, &["checkout", "-q", branch]);
    std::fs::write(repo.join("conf.txt"), "main line\n").unwrap();
    git(repo, &["add", "conf.txt"]);
    git(repo, &["commit", "-q", "-m", "main commit"]);
    git_unchecked(repo, &["merge", "--no-edit", "side"]); // expected to conflict
}

fn change(repo: &Path, rel: &str) -> ChangeRef {
    ChangeRef::new(RootId(repo.to_path_buf().into()), rel)
}

fn app_state(roots: &[PathBuf]) -> AppState {
    let dir = roots
        .first()
        .and_then(|r| r.parent())
        .map(Path::to_path_buf)
        .unwrap_or_default();
    AppState::for_roots(&dir, roots)
}

fn harness(state: AppState) -> Harness<'static, AppState> {
    // Generous width so the Commit window's two zones (fixed-width commit
    // panel + diff preview) fit without clipping the multi-root select-all
    // rows. `1024` is the stated `max_steps` rather than kittest's default of 4,
    // because `Harness::run` PANICS once a run exceeds it.
    let mut harness = shell_harness_over_unstyled(state, egui::vec2(1280.0, 800.0), 1024);
    test_support::harness::settle(&mut harness);
    harness
}

fn commit_action_button<'h>(h: &'h Harness<'_, AppState>) -> egui_kittest::Node<'h> {
    h.get_by_label("Commit changes")
}

fn open_commit_split_dropdown(h: &Harness<'_, AppState>) {
    h.get_by_label("Commit split options").click();
}

fn commit_button_is_disabled(h: &Harness<'_, AppState>) -> bool {
    commit_action_button(h).accesskit_node().is_disabled()
}

fn changes_title_galley(h: &Harness<'_, AppState>) -> test_support::harness::PaintedGalley {
    painted_galleys(h)
        .into_iter()
        .find(|g| g.text == "CHANGES")
        .unwrap_or_else(|| {
            panic!(
                "the changes card must be headed by the shared pane header's own \
                 title, `CHANGES`. No such text painted, so the heading is \
                 something else — a `CHANGES (n)` title is the count interpolated \
                 into the heading, and the count belongs in a count chip BESIDE it."
            )
        })
}

fn changes_header_rule(h: &Harness<'_, AppState>) -> egui::Rect {
    let title = changes_title_galley(h);
    filled_rects(h)
        .into_iter()
        .filter(|(rect, fill)| {
            *fill == Palette::RULE_STRUCTURAL && (rect.height() - 1.0).abs() < 0.01
        })
        .map(|(rect, _)| rect)
        .filter(|rect| rect.top() >= title.pos.y + turbogit_ui::theme::TYPE_SECTION)
        .min_by(|a, b| a.top().partial_cmp(&b.top()).expect("finite rule top"))
        .unwrap_or_else(|| {
            panic!(
                "the changes header paints no structural hairline under its title at {:?}",
                title.pos
            )
        })
}

fn changes_header_band(h: &Harness<'_, AppState>) -> egui::Rect {
    let rule = changes_header_rule(h);
    egui::Rect::from_min_max(
        egui::pos2(rule.left(), rule.top() - PANE_HEADER_HEIGHT),
        rule.max,
    )
}

fn button_labels_in(band: egui::Rect, h: &Harness<'_, AppState>) -> Vec<String> {
    let mut labels: Vec<String> = h
        .get_all_by_role(egui::accesskit::Role::Button)
        .filter(|node| band.contains_rect(node.rect()))
        .filter_map(|node| node.accesskit_node().label())
        .collect();
    labels.sort();
    labels
}

fn changes_count_chip(h: &Harness<'_, AppState>) -> (egui::Rect, egui::CornerRadius) {
    let band = changes_header_band(h);
    let hits: Vec<_> = painted_rects(h)
        .into_iter()
        .filter(|(rect, fill, _, radius)| {
            *fill == Palette::RAISED
                && (rect.height() - CHIP_HEIGHT).abs() < 0.01
                && *radius == egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS)
                && band.contains_rect(*rect)
        })
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one count chip in the changes header, found {hits:#?}"
    );
    let (rect, _, _, radius) = hits.into_iter().next().expect("one count chip");
    (rect, radius)
}

fn stroked_rects_including_vec(h: &Harness<'_, AppState>) -> Vec<(egui::Rect, egui::Color32, f32)> {
    let mut out = Vec::new();
    for clipped in &h.output().shapes {
        let members: Vec<&egui::Shape> = match &clipped.shape {
            egui::Shape::Rect(rs) => {
                out.push((rs.rect, rs.stroke.color, rs.stroke.width));
                continue;
            }
            egui::Shape::Vec(shapes) => shapes.iter().collect(),
            _ => continue,
        };
        for shape in members {
            if let egui::Shape::Rect(rs) = shape {
                out.push((rs.rect, rs.stroke.color, rs.stroke.width));
            }
        }
    }
    out
}

#[derive(Debug, PartialEq, Eq, Clone)]
struct InertSnapshot {
    subtab: CommitSubTab,
    dialog: Option<Dialog>,
    confirm_open: bool,
    message: String,
    amend: bool,
    selected: Vec<PathBuf>,
    expanded: Vec<String>,
    filter: String,
    preview: Option<ChangeRef>,
    toast_kind: Option<turbogit_app::state::ToastKind>,
    toast_message: Option<String>,
    busy: bool,
    pane_generation: u64,
    last_error: Option<String>,
}

fn inert_snapshot(h: &Harness<'_, AppState>) -> InertSnapshot {
    let s = h.state();
    let mut expanded: Vec<String> = s.ui.changes_expanded.iter().map(|id| id.name()).collect();
    expanded.sort();
    let mut selected: Vec<PathBuf> = s.ui.selected.iter().cloned().collect();
    selected.sort();
    InertSnapshot {
        subtab: s.ui.commit_subtab,
        dialog: s.ui.dialog,
        confirm_open: s.ui.confirm.is_some(),
        message: s.ui.commit_message.clone(),
        amend: s.ui.amend,
        selected,
        expanded,
        filter: s.ui.file_filter.clone(),
        preview: s.ui.preview_change.clone(),
        toast_kind: s.ui.toast.as_ref().map(|t| t.kind),
        toast_message: s.ui.toast.as_ref().map(|t| t.message.clone()),
        busy: s.ui.busy,
        pane_generation: s.ui.pane_generation,
        last_error: s.last_error.clone(),
    }
}

#[test]
fn canonical_groups_count_badges_and_status_rows_paint() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "paint-repo");
    let branch = repo.branch();
    seed_changes(&repo.path);
    seed_conflict(&repo.path, &branch);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    h.get_by_label("base.txt");
    h.get_by_label("added.txt");
    h.get_by_label("untracked.txt");
    h.get_by_label("Merge conflicts (1)");
    h.get_by_label("C conf.txt");

    assert!(
        h.query_by_label("Select all paint-repo").is_none(),
        "select-all must only appear for multi-root projects"
    );
}

#[test]
fn the_file_row_checkbox_stages_and_unstages_the_whole_file() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "toggle-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    assert!(
        !staged_contains(&repo.path, "base.txt"),
        "nothing is staged yet"
    );

    h.get_by_label("Stage base.txt (unstaged)").click();
    h.run();
    assert!(
        wait_until(15_000, || staged_contains(&repo.path, "base.txt")),
        "ticking the box stages the whole file"
    );

    h.run();
    h.get_by_label("Unstage base.txt (staged)").click();
    h.run();
    assert!(
        wait_until(15_000, || !staged_contains(&repo.path, "base.txt")),
        "ticking it again un-stages the whole file"
    );
}

fn staged_contains(repo: &Path, needle: &str) -> bool {
    git(repo, &["diff", "--cached", "--name-only"]).contains(needle)
}

fn stage_label_of(name: &str) -> String {
    format!("Stage {name} (unstaged)")
}

#[track_caller]
fn stage_through_checkbox(h: &mut Harness<'_, AppState>, repo: &Path, name: &str) {
    h.get_by_label(&stage_label_of(name)).click();
    h.run();
    assert!(
        wait_until(15_000, || staged_contains(repo, name)),
        "the checkbox must have staged {name}"
    );
    h.run();
}

const COMMIT_ALL_NOTE: &str = "Nothing is staged";

#[track_caller]
fn accent_halves(h: &Harness<'_, AppState>) -> Vec<egui::Rect> {
    let button = egui::Rect::from_min_max(
        h.get_by_label("Commit changes").rect().min,
        h.get_by_label("Commit split options").rect().max,
    );
    filled_rects(h)
        .into_iter()
        .filter(|(rect, fill)| *fill == Palette::ACCENT && button.contains_rect(*rect))
        .map(|(rect, _)| rect)
        .collect()
}

#[test]
fn commit_is_disabled_until_the_index_holds_something_and_then_enables() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "index-gate-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    h.state_mut().ui.commit_message = "commit everything".into();
    h.run();
    assert!(
        commit_button_is_disabled(&h),
        "an unstaged working tree must not be committable"
    );
    assert!(
        !painted_text(&h).iter().any(|t| t.contains(COMMIT_ALL_NOTE)),
        "the card must not promise a whole-index commit; painted text: {:?}",
        painted_text(&h)
    );
    assert_eq!(
        accent_halves(&h).len(),
        0,
        "a gated Commit paints no accent fill; the tinted disabled fill is a \
         different colour by design"
    );

    h.get_by_label("Stage base.txt (unstaged)").click();
    h.run();
    assert!(
        wait_until(15_000, || !git(
            &repo.path,
            &["diff", "--cached", "--name-only"]
        )
        .trim()
        .is_empty()),
        "the checkbox must have staged the file"
    );
    h.run();
    assert!(
        !commit_button_is_disabled(&h),
        "the index now holds a file, so Commit is committable"
    );
    assert_eq!(
        accent_halves(&h).len(),
        2,
        "the enabled Commit control paints its two brand halves"
    );
}

#[test]
fn browsing_the_file_list_never_makes_a_commit_possible() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "browse-gate-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.state_mut().ui.commit_message = "browsing only".into();
    h.run();

    h.get_by_label("base.txt").click();
    h.run();

    assert!(
        !h.state().ui.selected.is_empty(),
        "the click selected the row"
    );
    assert!(
        git(&repo.path, &["diff", "--cached", "--name-only"])
            .trim()
            .is_empty(),
        "and staged nothing: git's own index is the answer"
    );
    assert!(
        commit_button_is_disabled(&h),
        "Commit stays gated on the index, not on the selection"
    );
}

#[test]
fn an_empty_message_still_refuses_with_a_full_index() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "index-message-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("Stage base.txt (unstaged)").click();
    h.run();
    assert!(
        wait_until(15_000, || !git(
            &repo.path,
            &["diff", "--cached", "--name-only"]
        )
        .trim()
        .is_empty()),
        "the file reached the index"
    );
    h.run();
    assert!(
        commit_button_is_disabled(&h),
        "an empty message must stay refused whatever the index holds"
    );

    h.state_mut().ui.commit_message = "now it may commit".into();
    h.run();
    assert!(
        !commit_button_is_disabled(&h),
        "the two gates are independent: the message gate still governs"
    );
}

#[test]
fn a_clean_tree_states_the_clean_tree_sentence() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "clean-repo");

    let h = harness(app_state(std::slice::from_ref(&repo.path)));
    assert!(
        painted_text(&h)
            .iter()
            .any(|t| t.contains("Nothing to commit, working tree is clean.")),
        "a clean tree must say so in words; painted text: {:?}",
        painted_text(&h)
    );
}

#[test]
fn commit_executes_for_valid_input() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "commit-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    stage_through_checkbox(&mut h, &repo.path, "base.txt");
    h.state_mut().ui.commit_message = "issue11: real commit".into();
    h.run();
    commit_action_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"issue11: real commit".to_string())),
        "commit should land in the temp repository, got {:?}",
        repo.subjects()
    );
}

#[test]
fn amend_commits_with_amend_flag() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "amend-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    let before = repo.commit_count();
    assert_eq!(before, 1);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    for _ in 0..5 {
        h.run();
    }
    // One click per frame: kittest delivers queued events only at the next
    // run, so two clicks without an intervening run would land together and
    // only the release target would register.
    stage_through_checkbox(&mut h, &repo.path, "base.txt");
    h.get_by_label("Amend").click();
    h.run();
    assert!(h.state().ui.amend, "the Amend checkbox must toggle");
    h.state_mut().ui.commit_message = "amended subject".into();
    h.run();
    commit_action_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"amended subject".to_string())
            && repo.commit_count() == before),
        "amend must rewrite HEAD without adding a commit (count={}, subjects={:?})",
        repo.commit_count(),
        repo.subjects()
    );
}

#[test]
fn amend_click_seeds_the_full_head_message_into_the_painted_well() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "seed-repo");
    std::fs::write(repo.path.join("seeded.txt"), "work\n").unwrap();
    git(&repo.path, &["add", "seeded.txt"]);
    git(
        &repo.path,
        &[
            "commit",
            "-q",
            "-m",
            "prefill subject",
            "-m",
            "prefill body",
        ],
    );
    std::fs::write(repo.path.join("base.txt"), "next change\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    stage_through_checkbox(&mut h, &repo.path, "base.txt");
    h.get_by_label("Amend").click();
    h.run();

    assert!(h.state().ui.amend, "the click still toggles amend");
    assert_eq!(
        h.state().ui.commit_message,
        "prefill subject\n\nprefill body",
        "the seed is the full head message, body included"
    );
    assert_painted(&h, "prefill subject");
    assert_painted(&h, "prefill body");
}

#[test]
fn amend_click_leaves_a_typed_draft_untouched() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "draft-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    stage_through_checkbox(&mut h, &repo.path, "base.txt");
    h.get_by_label("Commit message").focus();
    h.get_by_label("Commit message").type_text("my typed draft");
    h.run();
    h.get_by_label("Amend").click();
    h.run();

    assert_eq!(
        h.state().ui.commit_message,
        "my typed draft",
        "a non-blank buffer is the user's and survives the toggle"
    );
}

#[test]
fn amend_click_with_disagreeing_heads_leaves_the_buffer_empty() {
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "disagree-a");
    let b = temp_repo(parent.path(), "disagree-b");
    std::fs::write(b.path.join("head.txt"), "head work\n").unwrap();
    git(&b.path, &["add", "head.txt"]);
    git(&b.path, &["commit", "-q", "-m", "b-only head"]);
    std::fs::write(a.path.join("base.txt"), "modified a\n").unwrap();
    std::fs::write(b.path.join("base.txt"), "modified b\n").unwrap();

    let mut h = harness(app_state(&[a.path.clone(), b.path.clone()]));
    stage_through_checkbox(&mut h, &a.path, "base.txt");
    h.get_by_label("Toggle disagree-b").click();
    h.run();
    stage_through_checkbox(&mut h, &b.path, "base.txt");
    assert_eq!(
        h.state().commit_targets().len(),
        2,
        "both checked repos hold a staged change"
    );

    h.get_by_label("Amend").click();
    h.run();

    assert!(h.state().ui.amend, "the toggle itself still happens");
    assert_eq!(
        h.state().ui.commit_message,
        "",
        "heads that disagree must withhold the seed and stay silent"
    );
}

#[test]
fn amend_off_edit_on_keeps_the_edit() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "edge-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    stage_through_checkbox(&mut h, &repo.path, "base.txt");

    h.get_by_label("Amend").click();
    h.run();
    assert_eq!(
        h.state().ui.commit_message,
        "init",
        "the blank buffer seeds on the way on"
    );

    h.get_by_label("Amend").click();
    h.run();
    assert!(!h.state().ui.amend, "the second click turns amend off");

    h.state_mut().ui.commit_message = "init, edited".into();
    h.run();
    h.get_by_label("Amend").click();
    h.run();

    assert!(
        h.state().ui.amend,
        "the third click is a false→true edge again"
    );
    assert_eq!(
        h.state().ui.commit_message,
        "init, edited",
        "an edited buffer is not blank, so the re-toggle must not overwrite it"
    );
}

#[test]
fn amend_click_seeds_two_agreeing_roots_and_unchecking_one_still_seeds() {
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "agree-a");
    let b = temp_repo(parent.path(), "agree-b");
    std::fs::write(a.path.join("base.txt"), "modified a\n").unwrap();
    std::fs::write(b.path.join("base.txt"), "modified b\n").unwrap();

    let mut h = harness(app_state(&[a.path.clone(), b.path.clone()]));
    stage_through_checkbox(&mut h, &a.path, "base.txt");
    h.get_by_label("Toggle agree-b").click();
    h.run();
    stage_through_checkbox(&mut h, &b.path, "base.txt");

    h.get_by_label("Amend").click();
    h.run();
    assert_eq!(
        h.state().ui.commit_message,
        "init",
        "two agreeing heads seed the shared message"
    );

    h.get_by_label("Amend").click();
    h.run();
    h.state_mut().ui.commit_message = String::new();
    h.state_mut()
        .ui
        .commit_repo_unchecks
        .insert(RootId(b.path.clone().into()));
    h.run();
    assert_eq!(
        h.state().commit_targets().len(),
        1,
        "agree-b is unchecked, agree-a alone votes"
    );

    h.get_by_label("Amend").click();
    h.run();
    assert_eq!(
        h.state().ui.commit_message,
        "init",
        "the one checked repo still seeds on its own"
    );
}

#[test]
fn commit_and_push_chains_commit_then_opens_push_dialog() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "push-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    stage_through_checkbox(&mut h, &repo.path, "base.txt");
    h.state_mut().ui.commit_message = "push chain subject".into();
    h.run();
    open_commit_split_dropdown(&h);
    h.run();
    h.get_by_label("Commit and Push...").click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"push chain subject".to_string())),
        "commit-and-push should first perform the commit"
    );
    assert_eq!(
        h.state().ui.dialog,
        Some(Dialog::Push),
        "commit-and-push must open the push dialog afterwards"
    );
}

fn repo_tracking_origin(scratch: &Path) -> (Repo, tempfile::TempDir) {
    let remote = tempfile::tempdir().expect("temp bare remote");
    git(remote.path(), &["init", "-q", "--bare", "."]);
    let repo = temp_repo(scratch, "tracking-repo");
    git(
        &repo.path,
        &[
            "remote",
            "add",
            "origin",
            &remote.path().display().to_string(),
        ],
    );
    git(&repo.path, &["push", "-q", "-u", "origin", &repo.branch()]);
    (repo, remote)
}

#[test]
fn commit_and_push_dialog_lists_the_commit_the_chain_just_landed() {
    let parent = tempfile::tempdir().unwrap();
    let (repo, _remote) = repo_tracking_origin(parent.path());
    std::fs::write(repo.path.join("base.txt"), "chained\n").unwrap();

    let cfg = tempfile::tempdir().unwrap();
    let cfg_path = cfg.path().to_path_buf();
    std::mem::forget(cfg);
    let state = AppState::launch_in(Some(parent.path().to_path_buf()), Some(cfg_path));
    assert_eq!(
        state.multi.roots.len(),
        1,
        "the fixture must register exactly the repository under test"
    );

    let mut h = harness(state);
    stage_through_checkbox(&mut h, &repo.path, "base.txt");
    h.state_mut().ui.commit_message = "chained subject".into();
    h.run();
    open_commit_split_dropdown(&h);
    h.run();
    h.get_by_label("Commit and Push...").click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"chained subject".to_string())),
        "the commit must land before the dialog is judged"
    );
    assert_eq!(
        h.state().ui.dialog,
        Some(Dialog::Push),
        "commit-and-push must open the push dialog"
    );
    assert_not_painted(&h, "No outgoing commits.");
    assert_painted(&h, "tracking-repo — 1 commits ahead");
}

fn seed_tracked(repo: &Path, name: &str) {
    std::fs::write(repo.join(name), "tracked\n").unwrap();
    git(repo, &["add", name]);
    git(repo, &["commit", "-q", "-m", &format!("track {name}")]);
    std::fs::write(repo.join(name), "modified\n").unwrap();
}

#[test]
fn repo_groups_render_collapsible_one_tree_with_focus() {
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "repo-a");
    let b = temp_repo(parent.path(), "repo-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");

    let h = harness(app_state(&[a.path.clone(), b.path.clone()]));

    h.get_by_label("Toggle repo-a");
    h.get_by_label("Toggle repo-b");

    h.get_by_label("a.txt");
    assert!(
        h.query_by_label("b.txt").is_none(),
        "non-focused repo-b must be collapsed by default"
    );
}

#[test]
fn manual_toggle_expands_collapsed_repo_group_and_collapses_again() {
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "repo-a");
    let b = temp_repo(parent.path(), "repo-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");
    let b_id = turbogit_domain::model::RootId(b.path.clone().into());

    let mut h = harness(app_state(&[a.path.clone(), b.path.clone()]));
    assert!(
        h.query_by_label("b.txt").is_none(),
        "repo-b starts collapsed"
    );
    assert!(!h.state().ui.changes_expanded.contains(&b_id));

    h.get_by_label("Toggle repo-b").click();
    h.run();
    h.get_by_label("b.txt");
    assert!(
        h.state().ui.changes_expanded.contains(&b_id),
        "expanded group must be recorded in changes_expanded"
    );

    h.get_by_label("Toggle repo-b").click();
    h.run();
    assert!(
        h.query_by_label("b.txt").is_none(),
        "toggling again must collapse the group"
    );
    assert!(!h.state().ui.changes_expanded.contains(&b_id));
}

#[test]
fn sidebar_selection_refocuses_tree_collapsing_other_groups() {
    let parent = tempfile::tempdir().unwrap();
    let shared = parent.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    let a = temp_repo(&shared, "repo-a");
    let b = temp_repo(&shared, "repo-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");

    let mut h = harness(AppState::for_roots(
        parent.path(),
        &[a.path.clone(), b.path.clone()],
    ));
    assert_eq!(
        h.state().selected_root,
        Some(turbogit_domain::model::RootId(a.path.clone().into())),
        "first registered root starts focused"
    );

    h.get_by_label("Toggle repo-b").click();
    h.run();
    h.get_by_label("b.txt");
    h.get_by_label("repo-b").click();
    h.run();

    assert_eq!(
        h.state().selected_root,
        Some(turbogit_domain::model::RootId(b.path.clone().into()))
    );
    h.get_by_label("b.txt");
    assert!(
        h.query_by_label("a.txt").is_none(),
        "selecting repo-b must collapse repo-a's group"
    );
    assert!(
        h.state().ui.changes_expanded.is_empty(),
        "manual expansions reset when the focus moves"
    );
}

#[test]
fn commit_window_layout_is_fixed_panel_plus_flexible_preview() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "two-zone-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.run();

    assert_eq!(
        turbogit_ui::ui::commit_window::COMMIT_PANEL_WIDTH,
        340.0,
        "the commit column keeps its width. The header shed two icons and gave \
         the filter its own row; it did not borrow width from anywhere."
    );

    let changes_card = card_rect(&h, changes_title_galley(&h).pos);
    assert_eq!(
        changes_card.width(),
        turbogit_ui::ui::commit_window::COMMIT_PANEL_WIDTH,
        "the changes card must fill the {}-wide commit column, got {changes_card:?}",
        turbogit_ui::ui::commit_window::COMMIT_PANEL_WIDTH
    );

    let path = h.get_by_label("Previewing base.txt").rect();
    let preview = card_rect(&h, path.center());
    let expected_x = changes_card.left()
        + turbogit_ui::ui::commit_window::COMMIT_PANEL_WIDTH
        + turbogit_ui::theme::PANEL_PADDING;
    assert!(
        (preview.left() - expected_x).abs() < 0.5,
        "the diff preview must start one panel padding right of the fixed commit \
         panel (x={expected_x}), got x={}",
        preview.left()
    );
}

#[test]
fn the_commit_card_docks_under_the_changes_list() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "msg-dock-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    let changes_card = card_rect(&h, changes_title_galley(&h).pos);
    let msg_origin =
        galley_origin(&h, "Commit message:").expect("the commit message label must paint");
    let commit_card = card_rect(&h, msg_origin);

    assert!(
        changes_card.top() < commit_card.top(),
        "the changes card is the pane's top and the commit card docks beneath it: \
         changes {changes_card:?}, commit {commit_card:?}"
    );
    assert!(
        (commit_card.top() - changes_card.bottom() - turbogit_ui::theme::PANEL_PADDING).abs() < 0.5,
        "exactly one panel padding separates the cards: changes {changes_card:?}, \
         commit {commit_card:?}"
    );

    let amend_origin =
        galley_origin(&h, "Amend").expect("the Amend option must paint with the message box");
    assert!(
        commit_card.contains(amend_origin),
        "Amend rides the docked commit card: card {commit_card:?}, amend {amend_origin:?}"
    );

    let labels = painted_text(&h);
    assert_eq!(
        labels
            .iter()
            .filter(|t| t.as_str() == "Commit message:")
            .count(),
        1,
        "exactly one commit message box must paint"
    );
}

#[test]
fn staging_section_headers_and_stage_all_actions_are_removed() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "sectionless-row");
    seed_changes(&repo.path);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    h.get_by_label("base.txt");
    h.get_by_label("added.txt");
    h.get_by_label("untracked.txt");

    let text = painted_text(&h).join("\n");
    assert!(
        !text.contains("UNSTAGED") && !text.contains("STAGED"),
        "staging section headers must be removed, got:\n{text}"
    );
    assert!(
        !text.contains("Stage all") && !text.contains("Unstage all"),
        "stage-all / unstage-all actions must be removed, got:\n{text}"
    );
    assert!(
        !text.contains("Unversioned Files"),
        "untracked files share the one list; their group is gone, got:\n{text}"
    );
}

#[test]
fn merge_conflicts_group_survives_the_section_removal() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "conflict-kept");
    let branch = repo.branch();
    seed_changes(&repo.path);
    seed_conflict(&repo.path, &branch);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    h.get_by_label("Merge conflicts (1)");
    h.get_by_label("C conf.txt");
}

#[test]
fn single_primary_commit_split_button_with_grey_shelve_stash() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "split-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    stage_through_checkbox(&mut h, &repo.path, "base.txt");
    h.state_mut().ui.commit_message = "split subject".into();
    h.run();

    let commit = commit_action_button(&h);
    assert!(
        !commit.accesskit_node().is_disabled(),
        "the primary Commit action must be enabled with a message and a non-empty index"
    );

    assert_not_painted(&h, "Commit and Push");

    let row_y = commit.rect().center().y;
    for label in ["Shelve…", "Stash…"] {
        let n = h.get_by_label(label);
        assert!(
            (n.rect().center().y - row_y).abs() < 14.0,
            "{label} must sit beside the primary Commit action (y≈{row_y}), got {}",
            n.rect().center().y
        );
        assert!(
            n.rect().left() > commit.rect().right(),
            "{label} to the right"
        );
    }
}

#[test]
fn commit_split_dropdown_offers_commit_and_push_only() {
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "dd-a");
    let b = temp_repo(parent.path(), "dd-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");

    let mut h = harness(app_state(&[a.path.clone(), b.path.clone()]));
    h.get_by_label("Stage a.txt (unstaged)").click();
    h.run();
    assert!(
        wait_until(15_000, || staged_contains(&a.path, "a.txt")),
        "the checkbox staged the file"
    );
    h.state_mut().ui.commit_message = "dd subject".into();
    h.run();

    open_commit_split_dropdown(&h);
    h.run();

    h.get_by_label("Commit and Push...");
    assert_not_painted(&h, "Also commit on");
}

#[test]
fn diff_preview_reflects_selected_file() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "preview-repo");
    seed_changes(&repo.path);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    h.get_by_label("base.txt").click();
    h.run();
    assert_eq!(
        h.state().ui.preview_change,
        Some(change(&repo.path, "base.txt"))
    );
    h.get_by_label("Previewing base.txt");

    h.get_by_label("added.txt").click();
    h.run();
    assert_eq!(
        h.state().ui.preview_change,
        Some(change(&repo.path, "added.txt"))
    );
    h.get_by_label("Previewing added.txt");
}

#[test]
fn diff_header_shows_path_and_nav_on_selection() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "header-repo");
    std::fs::write(repo.path.join("base.txt"), "a\nb\nc\n").unwrap();
    git(&repo.path, &["add", "base.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "three lines"]);
    std::fs::write(repo.path.join("base.txt"), "a\nx\ny\nc\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    assert_not_painted(&h, "Modified");
    assert!(h.query_by_label("Previewing base.txt").is_none());

    h.get_by_label("base.txt").click();
    h.run();

    h.get_by_label("Previewing base.txt");
    h.get_by_label("Previous change");
    h.get_by_label("Next change");

    assert_not_painted(&h, "Modified");
    assert_not_painted(&h, "Unified diff");
    assert_not_painted(&h, "Side-by-side diff");

    assert!(
        wait_until(15_000, || {
            h.run();
            let text = painted_text(&h).join("\n");
            text.contains("+2") && text.contains("\u{2212}1")
        }),
        "the diff toolbar must show the +2 −1 change-size stats once the diff \
         is computed"
    );
}

#[test]
fn header_nav_buttons_walk_previous_and_next_changed_files() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "nav-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    std::fs::write(repo.path.join("other.txt"), "edited\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.run();
    assert_eq!(
        h.state().ui.preview_change,
        Some(change(&repo.path, "base.txt"))
    );

    let prev = h.get_by_label("Previous change");
    assert!(
        prev.accesskit_node().is_disabled(),
        "Previous must be disabled on the first changed file"
    );
    h.get_by_label("Next change").click();
    h.run();
    assert_eq!(
        h.state().ui.preview_change,
        Some(change(&repo.path, "other.txt")),
        "Next must walk to the second changed file"
    );
    h.get_by_label("Previewing other.txt");

    let next = h.get_by_label("Next change");
    assert!(
        next.accesskit_node().is_disabled(),
        "Next must be disabled on the last changed file"
    );
    h.get_by_label("Previous change").click();
    h.run();
    assert_eq!(
        h.state().ui.preview_change,
        Some(change(&repo.path, "base.txt")),
        "Previous must walk back to the first changed file"
    );
}

#[test]
fn sub_tab_strip_switches_active_sub_tab_and_restores_local_changes() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "subtab-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    h.get_by_label("Local Changes");
    h.get_by_label("Shelf");
    h.get_by_label("Stash");
    assert!(
        h.query_by_label("Unversioned Files").is_none(),
        "the Unversioned Files sub-tab must be removed (issue 04)"
    );
    assert_eq!(h.state().ui.commit_subtab, CommitSubTab::LocalChanges);
    h.get_by_label("base.txt");

    h.get_by_label("Shelf").click();
    h.run();
    assert_eq!(h.state().ui.commit_subtab, CommitSubTab::Shelf);

    h.get_by_label("Local Changes").click();
    h.run();
    assert_eq!(h.state().ui.commit_subtab, CommitSubTab::LocalChanges);
    h.get_by_label("base.txt");
}

#[test]
fn a_sub_tab_click_switches_the_pane_on_the_same_frame() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "same-frame-subtab");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("Shelf").click();
    h.step();

    assert_eq!(h.state().ui.commit_subtab, CommitSubTab::Shelf);
    let painted: Vec<String> = painted_text(&h);
    assert!(
        painted.iter().any(|t| t == "SHELVES"),
        "the Shelf pane paints on the same frame as the click, got {painted:?}"
    );
    assert!(
        !painted.iter().any(|t| t == "base.txt"),
        "the changelist is gone the same frame: the body did not lag a click"
    );
}

#[test]
fn shelf_and_stash_replace_the_placeholder_with_their_own_panes() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "real-panes-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    assert_not_painted(&h, "arrives in a later phase");

    h.get_by_label("Shelf").click();
    h.run();
    assert_eq!(h.state().ui.commit_subtab, CommitSubTab::Shelf);
    assert_painted(&h, "SHELVES");
    assert_not_painted(&h, "arrives in a later phase");
    assert_not_painted(&h, "UNSTAGED");
    assert_not_painted(&h, "base.txt");

    h.get_by_label("Stash").click();
    h.run();
    assert_eq!(h.state().ui.commit_subtab, CommitSubTab::Stash);
    assert_painted(&h, "STASHES");
    assert_not_painted(&h, "arrives in a later phase");
    assert_not_painted(&h, "base.txt");
}

#[test]
fn untracked_files_join_the_one_flat_list_and_their_box_commits_them() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "untracked-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    std::fs::write(repo.path.join("untracked.txt"), "untracked\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    assert_eq!(h.state().ui.commit_subtab, CommitSubTab::LocalChanges);

    h.get_by_label("base.txt");
    h.get_by_label("untracked.txt");
    h.get_by_label(&stage_label_of("untracked.txt"));
    assert!(
        h.query_by_label("Unversioned Files (1)").is_none(),
        "the separate untracked group is gone; its files are in the one list"
    );

    h.get_by_label(&stage_label_of("untracked.txt")).click();
    h.run();
    assert!(
        wait_until(15_000, || staged_contains(&repo.path, "untracked.txt")),
        "the checkbox staged the untracked file"
    );
    h.state_mut().ui.commit_message = "include untracked".into();
    h.run();
    commit_action_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"include untracked".to_string())),
        "commit should land in the temp repository, got {:?}",
        repo.subjects()
    );
    let last_files = git(&repo.path, &["show", "--name-only", "--format=", "HEAD"]);
    assert!(
        last_files.lines().any(|l| l == "untracked.txt"),
        "the previously-untracked file must be committed, got {last_files:?}"
    );
    assert!(
        !last_files.lines().any(|l| l == "base.txt"),
        "the unstaged modification must stay out of the commit, got {last_files:?}"
    );
}

#[test]
fn file_rows_paint_checkbox_letter_coloured_name_and_dim_location() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "row-layout");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    std::fs::create_dir_all(repo.path.join("src")).unwrap();
    std::fs::write(repo.path.join("src/app.rs"), "fn main() {}\n").unwrap();
    git(&repo.path, &["add", "src/app.rs"]);
    git(&repo.path, &["commit", "-q", "-m", "track app"]);
    std::fs::write(repo.path.join("src/app.rs"), "fn main() { println!(); }\n").unwrap();
    std::fs::write(repo.path.join("untracked.txt"), "new\n").unwrap();

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    h.get_by_label("base.txt");
    h.get_by_label("app.rs");
    h.get_by_label("untracked.txt");
    h.get_by_label("Stage base.txt (unstaged)");
    h.get_by_label("Stage src/app.rs (unstaged)");
    h.get_by_label("Stage untracked.txt (unstaged)");

    let text = painted_text(&h).join("\n");
    assert!(
        text.contains("src/"),
        "rows must paint the dim location, got:\n{text}"
    );
    assert!(
        !text.contains("hunk"),
        "rows must not paint hunk counts, got:\n{text}"
    );
}

#[test]
fn clicking_a_row_selects_and_previews_it_and_leaves_the_checkbox_alone() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "select-row");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    std::fs::write(repo.path.join("other.txt"), "edit\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    let p = repo.path.join("base.txt");
    assert!(!h.state().ui.selected.contains(&p));

    h.get_by_label("base.txt").click();
    h.run();
    assert!(
        h.state().ui.selected.contains(&p),
        "a clicked row is selected"
    );
    assert_eq!(
        h.state().ui.preview_change,
        Some(change(&repo.path, "base.txt")),
        "a clicked row drives the diff preview"
    );

    let row_origin = galley_origin(&h, "base.txt").expect("row paints its filename");
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(r, c)| *c == Palette::ROW_SELECTED && r.contains(row_origin)),
        "the selected row must paint the list-row selection fill"
    );

    h.get_by_label("Stage base.txt (unstaged)");
    assert!(
        !staged_contains(&repo.path, "base.txt"),
        "a row click must not stage: `git diff --cached` is the answer"
    );
}

#[test]
fn the_checkbox_toggles_the_index_without_moving_the_selection() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "checkbox-vs-select");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    std::fs::write(repo.path.join("other.txt"), "edit\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    let p = repo.path.join("base.txt");

    h.get_by_label("other.txt").click();
    h.run();
    assert_eq!(
        h.state().ui.preview_change,
        Some(change(&repo.path, "other.txt"))
    );

    h.get_by_label("Stage base.txt (unstaged)").click();
    h.run();
    assert!(
        wait_until(15_000, || staged_contains(&repo.path, "base.txt")),
        "the box staged its own file"
    );
    assert!(
        !h.state().ui.selected.contains(&p),
        "the checkbox must not select the row it belongs to"
    );
    assert_eq!(
        h.state().ui.preview_change,
        Some(change(&repo.path, "other.txt")),
        "the checkbox must not move the preview either"
    );
}

#[test]
fn the_changes_header_is_the_shared_pane_header_with_three_controls() {
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "repo-a");
    let b = temp_repo(parent.path(), "repo-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");

    let h = harness(app_state(&[a.path.clone(), b.path.clone()]));

    let painted = painted_text(&h);
    assert!(
        !painted.iter().any(|t| t.contains("Changes (")),
        "the file count must not be characters inside the heading text; painted: {painted:?}"
    );
    assert!(
        painted.contains(&"CHANGES".to_owned()),
        "the heading must paint as the bare pane title `CHANGES`, with the count \
         beside it; painted: {painted:?}"
    );

    let title = changes_title_galley(&h);
    assert_eq!(painted_ink(&h, "CHANGES"), Some(Palette::INK_3));
    assert_eq!(
        painted_font(&h, "CHANGES").size,
        turbogit_ui::theme::TYPE_SECTION,
        "the heading is the shared 9px pane title, not a body-size label"
    );

    let (chip, radius) = changes_count_chip(&h);
    assert_eq!(
        radius,
        egui::CornerRadius::same(turbogit_ui::theme::CHIP_RADIUS)
    );
    assert!(
        title.rect.right() <= chip.left(),
        "the count chip sits BESIDE the heading, not inside it: title {:?}, chip {chip:?}",
        title.rect
    );
    let chip_ink = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "2" && chip.contains_rect(g.rect))
        .expect("the count paints inside its chip")
        .color;
    assert_eq!(
        chip_ink,
        Palette::INK_2,
        "the count chip carries the shared secondary ink, not the muted one"
    );

    let band = changes_header_band(&h);
    assert_eq!(
        button_labels_in(band, &h),
        vec![
            "Changes header overflow",
            "Expand all groups",
            "Refresh changes",
            "Rollback",
        ],
        "the changes header's band holds the three live controls (refresh, \
         rollback, expand/collapse-all) plus the `⋯` that opens the overflow \
         menu the two inert ones moved into — and nothing else. A fifth name \
         here means the five-icon row is coming back."
    );
    assert!(
        h.query_by_label("Commit options").is_none(),
        "commit options is retired — it paints nowhere in the changes header"
    );
    assert!(
        h.query_by_label("Group by").is_none(),
        "group by paints no button in the header — it is a menu row now"
    );

    let rule = changes_header_rule(&h);
    assert!(
        (rule.top() - band.top() - PANE_HEADER_HEIGHT).abs() < 0.01
            && (rule.bottom() - band.bottom()).abs() < 0.01,
        "the hairline is the band's last pixel: band {band:?}, rule {rule:?}"
    );
    assert!(
        (rule.left() - title.rect.left()).abs() < 0.01,
        "the hairline starts where the title starts: rule {rule:?}, title {:?}",
        title.rect
    );

    let strokes: Vec<_> = stroked_rects(&h)
        .into_iter()
        .filter(|(rect, _, _)| band.intersects(*rect))
        .collect();
    assert!(
        strokes.is_empty(),
        "the pane header paints no stroke — a 1px outline here would be a second \
         way of saying the same thing as the one hairline, and the only surfaces \
         allowed a stroke are the ones that float. Found {strokes:?}"
    );

    assert_not_painted(&h, "Unstage selected");
}

#[test]
fn the_inert_header_controls_are_inert_overflow_items_that_dispatch_nothing() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "overflow-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    assert_not_painted(&h, "Advanced options");
    assert!(h.query_by_label("Commit options").is_none());
    assert!(h.query_by_label("Group by").is_none());

    let commits_before = repo.commit_count();
    let subjects_before = repo.subjects();

    h.get_by_label("Changes header overflow").click();
    h.run();

    let items: [(&str, &str); 1] = [("Group by", "No grouping surface yet (ADR-0010)")];
    let mut item_rects = Vec::new();
    for (label, _) in items {
        let node = h.get_by_label(label);
        assert!(
            node.accesskit_node().is_disabled(),
            "`{label}` is a not-yet-shipped flow (ADR-0010), so its row must say \
             so in the accessibility tree rather than pretending to be live"
        );
        let rect = node.rect();
        assert!(
            painted_paths(&h)
                .iter()
                .any(|(glyph, _)| rect.intersects(*glyph)),
            "`{label}` must keep its icon glyph inside its row {rect:?}"
        );
        item_rects.push(rect);
    }

    // The label is painted at 45 % of `INK`. Color32 stores premultiplied, so
    // the spec facts are the un-multiplied channels: INK's rgb, and an alpha of
    // 115 = round(255 × 0.45).
    for (label, _) in items {
        let galley = painted_galleys(&h)
            .into_iter()
            .find(|g| g.text == label)
            .unwrap_or_else(|| panic!("`{label}` is never hidden — a gated action stays rendered"));
        let [r, g, b, a] = galley.color.to_srgba_unmultiplied();
        assert_eq!(a, 115, "`{label}` paints at the shared 45 % disabled ink");
        let near = |x: u8, y: u8| (x as i32 - y as i32).abs() <= 2;
        assert!(
            near(r, 0xDF) && near(g, 0xE1) && near(b, 0xE5),
            "`{label}` keeps INK's hue at 45 %, got {r:#04x}/{g:#04x}/{b:#04x}",
        );
    }

    let border = stroked_rects_including_vec(&h)
        .into_iter()
        .find(|(rect, _, width)| {
            *width == 1.0 && item_rects.iter().all(|item| rect.contains_rect(*item))
        });
    assert!(
        border.is_some(),
        "an open menu floats above the card, so it wears the 1px hairline R2 \
         reserves for floating surfaces; a borderless menu would be a missed \
         rule, not a clean one. Item rects: {item_rects:?}"
    );

    h.hover_at(item_rects[0].center());
    let mut reason_painted = false;
    for _ in 0..20 {
        h.step();
        if painted_text(&h).iter().any(|t| t.contains(items[0].1)) {
            reason_painted = true;
            break;
        }
    }
    assert!(
        reason_painted,
        "hovering a gated item must state why it is gated (`{}`)",
        items[0].1
    );

    for (label, _) in items {
        if h.query_by_label(label).is_none() {
            h.get_by_label("Changes header overflow").click();
            h.run();
        }
        let before = inert_snapshot(&h);
        h.get_by_label(label).click();
        h.run();
        let after = inert_snapshot(&h);
        assert_eq!(
            before, after,
            "pressing `{label}` must dispatch nothing. `busy` and `pane_generation` \
             are in the snapshot because every `AppState::dispatch` sets `busy` \
             and every refresh bumps the generation — so a wired control cannot \
             pass this by being quiet."
        );
        assert_eq!(
            repo.commit_count(),
            commits_before,
            "pressing `{label}` must not call git"
        );
        assert_eq!(repo.subjects(), subjects_before);
    }
    assert!(
        h.state().ui.toast.is_none(),
        "a wired `Rollback` would have toasted \"Select files to discard.\""
    );
}

#[test]
fn expand_collapse_all_toggles_every_repo_group() {
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "repo-a");
    let b = temp_repo(parent.path(), "repo-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");
    let b_id = turbogit_domain::model::RootId(b.path.clone().into());

    let mut h = harness(app_state(&[a.path.clone(), b.path.clone()]));
    assert!(
        h.query_by_label("b.txt").is_none(),
        "repo-b starts collapsed"
    );

    h.get_by_label("Expand all groups").click();
    h.run();
    h.get_by_label("b.txt");
    assert!(
        h.state().ui.changes_expanded.contains(&b_id),
        "expand-all must record every non-focused group"
    );
    h.get_by_label("Collapse all groups").click();
    h.run();
    assert!(
        h.query_by_label("b.txt").is_none(),
        "collapse-all closes every non-focused group"
    );
    assert!(h.state().ui.changes_expanded.is_empty());
}

#[test]
fn rollback_icon_confirms_discard_and_toasts_when_empty() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "rollback-repo");
    seed_tracked(&repo.path, "a.txt");

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    h.get_by_label("Rollback").click();
    h.run();
    assert!(
        h.state().ui.confirm.is_none(),
        "rollback with an empty selection must not open the discard confirm"
    );

    h.get_by_label("a.txt").click();
    h.run();
    h.get_by_label("Rollback").click();
    h.run();
    match &h.state().ui.confirm {
        Some(turbogit_app::state::PendingConfirm::Discard { changes }) => {
            assert_eq!(changes.len(), 1, "one selected file enters the confirm");
        }
        None => panic!("expected a Discard confirm, got none"),
        Some(_) => panic!("expected a Discard confirm, got a different confirm"),
    }
}

#[test]
fn refresh_icon_refreshes_changes() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "refresh-repo");
    seed_tracked(&repo.path, "a.txt");

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    let gen_before = h.state().ui.pane_generation;

    h.get_by_label("Refresh changes").click();
    h.run();

    assert!(
        h.state().ui.pane_generation > gen_before,
        "refresh must dispatch the full scoped refresh (pane generation bump)"
    );
}

/// `(origin, size, line count)` of the painted galley carrying exactly
/// `text`.
///
/// The shared painted-text helpers expose a galley's text and origin only,
/// and egui keeps the *input* string on a wrapped galley
/// (`Galley::text` is documented as "the full, non-elided text of the input
/// job"), so a wrap is only observable from the galley's geometry: several
/// laid-out rows, none wider than the sheet's name column.
fn galley_metrics(h: &Harness<'_, AppState>, text: &str) -> (egui::Pos2, egui::Vec2, usize) {
    h.output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) if shape.galley.text() == text => {
                Some((shape.pos, shape.galley.size(), shape.galley.rows.len()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no painted galley carries {text:?}"))
}

fn selected_row_height(h: &Harness<'_, AppState>, origin: egui::Pos2) -> f32 {
    filled_rects(h)
        .into_iter()
        .find(|(r, c)| *c == Palette::ROW_SELECTED && r.contains(origin))
        .expect("the selected row paints the list-row selection fill (ticket 03)")
        .0
        .height()
}

#[test]
fn long_file_names_wrap_onto_a_continuation_line_and_grow_the_row() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "wrap-long-name");
    std::fs::create_dir_all(repo.path.join("src")).unwrap();
    let long = "an_extremely_long_file_name_that_cannot_fit_the_commit_panel_column.rs";
    let path = repo.path.join("src").join(long);
    std::fs::write(&path, "one\n").unwrap();
    git(&repo.path, &["add", "-A"]);
    git(&repo.path, &["commit", "-q", "-m", "track long name"]);
    std::fs::write(&path, "two\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    h.get_by_label(long);

    let (name_pos, name_size, name_lines) = galley_metrics(&h, long);
    assert!(
        name_lines >= 2,
        "a name too wide for its column must wrap onto the next line, \
         got {name_lines} line(s)"
    );

    let (loc_pos, ..) = galley_metrics(&h, "src/");
    assert!(
        loc_pos.x > name_pos.x,
        "the location column must stay right of the name (name x={}, location x={})",
        name_pos.x,
        loc_pos.x
    );
    assert!(
        (loc_pos.y - name_pos.y).abs() < 1.0,
        "the location must ride the name's first line (name y={}, location y={})",
        name_pos.y,
        loc_pos.y
    );
    assert!(
        name_size.x <= loc_pos.x - name_pos.x,
        "every wrapped line must stay inside the name column, left of the \
         location (name width={}, column={})",
        name_size.x,
        loc_pos.x - name_pos.x
    );

    h.get_by_label(long).click();
    h.run();
    let height = selected_row_height(&h, name_pos);
    assert!(
        height > turbogit_ui::theme::FILE_ROW_HEIGHT,
        "a wrapped row must grow past {} px, got {height}",
        turbogit_ui::theme::FILE_ROW_HEIGHT
    );
}

#[test]
fn renamed_rows_move_the_old_path_below_a_wrapped_new_name() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "wrap-rename");
    let long = "a_renamed_file_whose_new_name_cannot_fit_the_commit_panel_column.rs";
    std::fs::write(repo.path.join("old.txt"), "content\n").unwrap();
    git(&repo.path, &["add", "-A"]);
    git(&repo.path, &["commit", "-q", "-m", "track old name"]);
    git(&repo.path, &["mv", "old.txt", long]);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    let (name_pos, name_size, name_lines) = galley_metrics(&h, long);
    assert!(
        name_lines >= 2,
        "the renamed-to name must wrap, got {name_lines} line(s)"
    );
    let (orig_pos, ..) = galley_metrics(&h, "old.txt");
    assert!(
        orig_pos.y >= name_pos.y + name_size.y - 1.0,
        "the renamed-from path must follow the wrapped name on its own line \
         (name {}..{}, old path y={})",
        name_pos.y,
        name_pos.y + name_size.y,
        orig_pos.y
    );
}

#[test]
fn short_file_names_keep_the_single_line_row_height() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "wrap-short-name");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    let (origin, _, lines) = galley_metrics(&h, "base.txt");
    assert_eq!(lines, 1, "a short name must stay on one line");

    h.get_by_label("base.txt").click();
    h.run();
    assert_eq!(
        selected_row_height(&h, origin),
        turbogit_ui::theme::FILE_ROW_HEIGHT,
        "a single-line row keeps the 24 px file-row height"
    );
}

const CARD_FILL: egui::Color32 = egui::Color32::from_rgb(0x23, 0x25, 0x29);

fn painted_rects(
    h: &Harness<'_, AppState>,
) -> Vec<(egui::Rect, egui::Color32, egui::Stroke, egui::CornerRadius)> {
    h.output()
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Rect(rs) => Some((rs.rect, rs.fill, rs.stroke, rs.corner_radius)),
            _ => None,
        })
        .collect()
}

#[track_caller]
fn card_rect(h: &Harness<'_, AppState>, point: egui::Pos2) -> egui::Rect {
    let rects = painted_rects(h);
    let card = rects
        .iter()
        .filter(|(rect, fill, _, radius)| {
            *fill == CARD_FILL
                && *radius == egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS)
                && rect.contains(point)
        })
        .map(|(rect, _, _, _)| *rect)
        .next()
        .unwrap_or_else(|| {
            panic!("no card (a {CARD_FILL:?} fill at CARD_RADIUS) contains {point:?}")
        });

    for (rect, _, stroke, _) in &rects {
        if stroke.width > 0.0
            && stroke.color != egui::Color32::TRANSPARENT
            && (rect.min - card.min).length() < 0.5
            && (rect.max - card.max).length() < 0.5
        {
            panic!(
                "card {card:?} must paint no stroke, but {rect:?} strokes {stroke:?} on its edge"
            );
        }
    }
    card
}

#[test]
fn commit_message_controls_sit_inside_one_bordered_card() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-msg-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    let message = galley_origin(&h, "Commit message:").expect("the message label paints");
    let amend = galley_origin(&h, "Amend").expect("the Amend option paints");
    let card = card_rect(&h, message);
    assert!(
        card.contains(amend),
        "the message label and Amend must sit in the same card, card is {card:?}"
    );
}

#[test]
fn commit_action_row_is_the_commit_card_s_pinned_footer() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-footer-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.state_mut().ui.commit_message = "footer subject".into();
    h.run();

    let message = galley_origin(&h, "Commit message:").expect("the message label paints");
    let card = card_rect(&h, message);
    let commit = commit_action_button(&h);
    assert!(
        card.contains_rect(commit.rect()),
        "the primary Commit action must sit inside the commit card {card:?}, \
         got {:?}",
        commit.rect()
    );
    let stash = h.get_by_label("Stash…");
    assert!(
        card.contains_rect(stash.rect()),
        "the whole action row is the footer, so `Stash…` is inside the card too, \
         got {:?}",
        stash.rect()
    );
    assert!(
        commit.rect().top() > message.y,
        "the action row sits below the message controls (message y={}, row top={})",
        message.y,
        commit.rect().top()
    );
}

#[test]
fn changes_tree_sits_in_a_card_with_its_toolbar_as_the_header() {
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "card-changes-a");
    let b = temp_repo(parent.path(), "card-changes-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");

    let h = harness(app_state(&[a.path.clone(), b.path.clone()]));

    let title = changes_title_galley(&h).pos;
    let card = card_rect(&h, title);

    let row = galley_origin(&h, "a.txt").expect("the focused repo's file row paints");
    assert!(
        card.contains(row),
        "the changes tree must sit inside the card {card:?} headed at {title:?}"
    );

    for label in [
        "Expand all groups",
        "Rollback",
        "Refresh changes",
        "Changes header overflow",
    ] {
        let rect = h.get_by_label(label).rect();
        assert!(
            card.contains_rect(rect),
            "`{label}` must sit inside the changes card {card:?}, got {rect:?}"
        );
        assert!(
            rect.bottom() <= row.y,
            "`{label}` belongs to the header band, above the first row \
             (row y={}, control bottom={})",
            row.y,
            rect.bottom()
        );
    }
}

fn filter_frame(h: &Harness<'_, AppState>) -> egui::Rect {
    let edit = h.get_by_label("Filter files").rect();
    let mut frames: Vec<egui::Rect> = filled_rects(h)
        .into_iter()
        .filter(|(rect, fill)| *fill == Palette::SURFACE_3 && rect.contains_rect(edit))
        .map(|(rect, _)| rect)
        .collect();
    frames.sort_by(|a, b| b.width().partial_cmp(&a.width()).expect("finite width"));
    frames
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("the filter paints no SURFACE_3 input frame around {edit:?}"))
}

#[test]
fn the_file_filter_gets_its_own_full_width_row_under_the_header() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-filter-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    let title = changes_title_galley(&h).pos;
    let card = card_rect(&h, title);
    let rule = changes_header_rule(&h);
    let row = h.get_by_label("base.txt").rect();
    let frame = filter_frame(&h);

    assert!(
        card.contains_rect(frame),
        "the file filter must sit inside the changes card {card:?}, got {frame:?}"
    );

    assert!(
        frame.top() >= rule.bottom(),
        "the filter shares no pixel with the header band: it starts below the \
         hairline (band bottom {}, filter top {})",
        rule.bottom(),
        frame.top()
    );

    assert!(
        frame.bottom() <= row.top() + 1.0,
        "the filter must stay above the list it filters (filter {frame:?}, row {row:?})"
    );

    let pad = turbogit_ui::theme::PANEL_PADDING;
    let inner_width = card.width() - 2.0 * pad;
    assert!(
        (frame.width() - inner_width).abs() < 1.0,
        "the filter must span the card's whole inner row ({inner_width:.1}px), \
         got {:.1}px — a narrower one means it is still sharing a row with \
         something",
        frame.width()
    );
    assert!(
        frame.width() >= 96.0,
        "`Filter files` must stay wide enough to be usable, got {:.1}px ({frame:?})",
        frame.width()
    );

    assert!(
        frame.height() >= turbogit_ui::ui::components::CLICK_TARGET_MIN,
        "the filter's row must render at full height (>= {}px), got {:.1}px",
        turbogit_ui::ui::components::CLICK_TARGET_MIN,
        frame.height()
    );
    assert!(
        frame.height() > rule.bottom() - rule.top(),
        "the filter's row is taller than the 1px hairline it sits under, so the \
         row is a control rather than a rule"
    );
}

#[test]
fn preview_is_a_card_headered_by_the_path_and_navigation() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-preview-repo");
    std::fs::write(repo.path.join("base.txt"), "a\nb\nc\n").unwrap();
    git(&repo.path, &["add", "base.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "three lines"]);
    std::fs::write(repo.path.join("base.txt"), "a\nx\ny\nc\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.run();

    let path = h.get_by_label("Previewing base.txt").rect();
    let card = card_rect(&h, path.center());
    assert!(
        card.contains_rect(path),
        "the previewed path must sit inside the preview card {card:?}, got {path:?}"
    );
    assert!(
        galley_origin(&h, "Preview").is_none(),
        "the header must not paint a `Preview` title beside the path"
    );

    for label in ["Previous change", "Next change"] {
        let rect = h.get_by_label(label).rect();
        assert!(
            card.contains_rect(rect),
            "`{label}` must sit inside the preview card {card:?}, got {rect:?}"
        );
        assert!(
            (rect.center().y - path.center().y).abs() < 2.0,
            "`{label}` and the path are centred on one band line \
             (path centre {}, control centre {})",
            path.center().y,
            rect.center().y
        );
    }

    assert_not_painted(&h, "Modified");
    assert_not_painted(&h, "Unified diff");
    assert_not_painted(&h, "Side-by-side diff");
}

#[test]
fn expanded_repo_group_paints_one_indent_guide_over_its_file_block() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "guide-repo");
    seed_tracked(&repo.path, "base.txt");
    seed_tracked(&repo.path, "other.txt");

    let h = harness(app_state(std::slice::from_ref(&repo.path)));

    let first = h.get_by_label("base.txt").rect();
    let last = h.get_by_label("other.txt").rect();
    let block_top = first.top().min(last.top());
    let block_bottom = first.bottom().max(last.bottom());

    let guides: Vec<egui::Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(_, c)| *c == Palette::LINE_SUBTLE)
        .map(|(r, _)| r)
        .filter(|r| r.width() <= 2.0 && r.height() >= turbogit_ui::theme::FILE_ROW_HEIGHT)
        .collect();
    let guide = guides
        .iter()
        .find(|r| r.top() <= block_top + 1.0 && r.bottom() >= block_bottom - 1.0)
        .unwrap_or_else(|| {
            panic!(
                "no vertical LINE_SUBTLE guide spans the file block \
                 y={block_top}..{block_bottom}; guides: {guides:?}"
            )
        });

    for name in ["base.txt", "other.txt"] {
        let checkbox = h.get_by_label(&stage_label_of(name)).rect();
        assert!(
            guide.center().x >= checkbox.right() && guide.center().x <= checkbox.right() + 8.0,
            "the guide must sit in the gutter after `{name}`'s checkbox \
             (checkbox right {}, guide x {})",
            checkbox.right(),
            guide.center().x
        );
    }
}

#[test]
fn commit_severity_messages_use_the_shared_semantic_inks() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "sev");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));

    h.state_mut().last_error = Some("push failed".into());
    h.run();
    assert_eq!(
        painted_ink_of(&h, "push failed"),
        Some(Palette::STATE_ERROR),
        "the commit error banner must render in the shared error ink"
    );
}

fn painted_ink_of(h: &Harness<'_, AppState>, needle: &str) -> Option<egui::Color32> {
    painted_galleys(h)
        .into_iter()
        .find(|g| g.text.contains(needle))
        .map(|g| g.color)
}

#[test]
fn file_row_name_renders_at_the_shared_body_size() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "type-role");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.run();

    let name_size = {
        let mut sizes = Vec::new();
        for clipped in &h.output().shapes {
            if let egui::Shape::Text(shape) = &clipped.shape {
                let text = shape.galley.text();
                if text.contains("base.txt") {
                    sizes.push(
                        shape
                            .galley
                            .job
                            .sections
                            .first()
                            .map_or(0.0, |s| s.format.font_id.size),
                    );
                }
            }
        }
        sizes
    };
    assert!(
        !name_size.is_empty(),
        "the commit file row must paint its name"
    );
    assert!(
        name_size
            .iter()
            .any(|s| (*s - turbogit_ui::theme::TYPE_BODY).abs() < 0.01),
        "the file-row name must render at the shared body size (TYPE_BODY={}); got {name_size:?}",
        turbogit_ui::theme::TYPE_BODY
    );
}

fn changes_card(h: &Harness<'_, AppState>) -> egui::Rect {
    card_rect(h, changes_title_galley(h).pos)
}

fn row_bands(h: &Harness<'_, AppState>, card: egui::Rect) -> Vec<(egui::Rect, egui::Color32)> {
    filled_rects(h)
        .into_iter()
        .filter(|(rect, _)| {
            *rect != card
                && card.intersects(*rect)
                && rect.width() > 100.0
                && rect.height() >= turbogit_ui::theme::FILE_ROW_HEIGHT - 0.5
        })
        .collect()
}

#[track_caller]
fn band_containing(
    h: &Harness<'_, AppState>,
    card: egui::Rect,
    fill: egui::Color32,
    point: egui::Pos2,
) -> egui::Rect {
    let hits: Vec<egui::Rect> = row_bands(h, card)
        .into_iter()
        .filter(|(_, painted)| *painted == fill)
        .map(|(rect, _)| rect)
        .filter(|rect| rect.contains(point))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one {fill:?} row band containing {point:?}, found {hits:?}"
    );
    hits.into_iter().next().expect("one band")
}

#[test]
fn a_selected_file_row_paints_the_row_selected_fill_and_a_leading_accent_rail() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "row-selected-fill");
    seed_tracked(&repo.path, "base.txt");
    seed_tracked(&repo.path, "other.txt");

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.run();

    let card = changes_card(&h);
    let origin = galley_origin(&h, "base.txt").expect("the selected row paints its name");
    let band = band_containing(&h, card, Palette::ROW_SELECTED, origin);

    assert_eq!(
        turbogit_ui::ui::components::row_fill(turbogit_ui::ui::components::RowState::RowSelected),
        Palette::ROW_SELECTED,
        "a chosen list row's fill is the row-selected token, read from the \
         row-fill decision rather than restated here"
    );
    assert_ne!(
        Palette::ROW_SELECTED,
        Palette::SELECTION,
        "the current-ref band is not the list-row selection fill"
    );
    assert!(
        (band.height() - turbogit_ui::theme::FILE_ROW_HEIGHT).abs() < 0.01,
        "the selected band is exactly one file row tall, got {band:?}"
    );

    let rails: Vec<egui::Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(rect, fill)| {
            *fill == Palette::BRAND
                && card.intersects(*rect)
                && (rect.width() - turbogit_ui::theme::RAIL_WIDTH).abs() < 0.01
        })
        .map(|(rect, _)| rect)
        .collect();
    assert_eq!(
        rails.len(),
        1,
        "exactly one rail paints in the changes list, and it belongs to the \
         chosen row: {rails:?}"
    );
    let rail = rails[0];
    assert!(
        (rail.left() - band.left()).abs() < 0.01,
        "the rail is paint at the row's own leading edge, not a reserved \
         gutter beside it: rail {rail:?} vs band {band:?}"
    );
    assert!(
        (rail.height() - band.height()).abs() < 0.01,
        "the rail spans the chosen row's full height: rail {rail:?} vs band {band:?}"
    );

    for (rect, fill) in row_bands(&h, card) {
        assert_ne!(
            fill,
            Palette::SELECTION,
            "no list row fills with the current-ref band: {rect:?} is {fill:?}"
        );
        assert_ne!(
            fill,
            Palette::BRAND,
            "the accent is the rail, the primary action and the active tab \
             underline — never a row's own fill: {rect:?} is {fill:?}"
        );
    }
}

#[test]
fn a_selected_file_rows_text_origin_equals_an_unselected_rows() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "row-origin");
    seed_tracked(&repo.path, "base.txt");
    seed_tracked(&repo.path, "other.txt");

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.run();
    h.get_by_label("other.txt").hover();
    h.run();

    let card = changes_card(&h);
    let (selected, comparison) = (
        band_containing(
            &h,
            card,
            Palette::ROW_SELECTED,
            galley_origin(&h, "base.txt").expect("the chosen row paints its name"),
        ),
        band_containing(
            &h,
            card,
            Palette::RAISED_ON_CARD,
            galley_origin(&h, "other.txt").expect("the comparison row paints its name"),
        ),
    );
    let name_in = |name: &str| {
        painted_galleys(&h)
            .into_iter()
            .find(|g| g.text == name)
            .unwrap_or_else(|| panic!("no painted galley carries {name:?}"))
    };
    let (chosen, plain) = (name_in("base.txt"), name_in("other.txt"));

    assert_eq!(
        chosen.pos.x - selected.left(),
        plain.pos.x - comparison.left(),
        "a chosen row's name starts the same distance from its own leading edge \
         as an unchosen row's: the rail is paint, never reserved padding \
         (chosen {:?} in {selected:?}, plain {:?} in {comparison:?})",
        chosen.pos,
        plain.pos
    );
    assert_eq!(
        chosen.pos.x, plain.pos.x,
        "both rows share one name column, selected or not"
    );
    assert_eq!(
        selected.size(),
        comparison.size(),
        "choosing a row does not resize it either"
    );
}

fn file_row_inks(
    h: &Harness<'_, AppState>,
    name: &str,
    card: egui::Rect,
) -> Vec<(String, [u8; 4])> {
    let checkbox = h.get_by_label(&stage_label_of(name)).rect();
    let grow = (turbogit_ui::theme::FILE_ROW_HEIGHT - checkbox.height()) / 2.0;
    let row = egui::Rect::from_min_max(
        egui::pos2(checkbox.left() - 24.0, checkbox.top() - grow),
        egui::pos2(card.right() - 24.0, checkbox.bottom() + grow),
    );
    let mut inks: Vec<(String, [u8; 4])> = painted_galleys(h)
        .into_iter()
        .filter(|g| row.contains(g.pos))
        .map(|g| (g.text, g.color.to_array()))
        .collect();
    inks.sort();
    inks
}

#[test]
fn selecting_a_file_row_changes_no_ink_in_it() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "row-ink");
    std::fs::create_dir_all(repo.path.join("src")).unwrap();
    std::fs::write(repo.path.join("src/app.rs"), "fn main() {}\n").unwrap();
    git(&repo.path, &["add", "src/app.rs"]);
    git(&repo.path, &["commit", "-q", "-m", "track app"]);
    std::fs::write(repo.path.join("src/app.rs"), "fn main() { println!(); }\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    let card = changes_card(&h);
    let before = file_row_inks(&h, "src/app.rs", card);
    assert!(
        before.iter().any(|(text, _)| text == "app.rs"),
        "the row paints its name before selection, got {before:?}"
    );

    h.get_by_label("app.rs").click();
    h.run();

    let after = file_row_inks(&h, "src/app.rs", card);
    assert_eq!(
        before, after,
        "selecting a row must not re-ink any of its text: the row is the \
         same kind of object chosen or not"
    );

    let ink_of = |text: &str| {
        after
            .iter()
            .find(|(painted, _)| painted == text)
            .map(|(_, colour)| {
                egui::Color32::from_rgba_unmultiplied(colour[0], colour[1], colour[2], colour[3])
            })
            .unwrap_or_else(|| panic!("{text:?} is not painted in the selected row, got {after:?}"))
    };
    assert_eq!(
        ink_of("app.rs"),
        Palette::INK,
        "a file name is the row's answer, and answers at the primary ink"
    );
    assert_eq!(
        ink_of("src/"),
        Palette::INK_3,
        "the location suffix is the furniture around the name, and dims to \
         the muted step"
    );
    assert_eq!(
        ink_of("M"),
        turbogit_ui::theme::Palette::STATUS_MODIFIED,
        "the status letter keeps its semantic hue — it is state, not ramp"
    );
}

/// **A long name wraps: the row grows, and the painted galley grows with it.**
///
/// Asserted from geometry only, laid-out rows, galley extent, band height.
/// `Galley::text()` returns the *unwrapped* input, so a wrap test written
/// against it reads as though nothing wrapped even when the row grew
/// correctly; the galley is located by that accessor and never judged by it.
#[test]
fn a_long_file_name_grows_its_galley_and_its_row() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "wrap-galley");
    let long = "an_extremely_long_file_name_that_cannot_fit_the_commit_panel_column.rs";
    std::fs::create_dir_all(repo.path.join("src")).unwrap();
    std::fs::write(repo.path.join("src").join(long), "one\n").unwrap();
    std::fs::write(repo.path.join("src/short.rs"), "two\n").unwrap();
    git(&repo.path, &["add", "-A"]);
    git(&repo.path, &["commit", "-q", "-m", "track both"]);
    std::fs::write(repo.path.join("src").join(long), "one edited\n").unwrap();
    std::fs::write(repo.path.join("src/short.rs"), "two edited\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.run();

    let (_, long_size, long_rows) = galley_metrics(&h, long);
    let (_, short_size, short_rows) = galley_metrics(&h, "short.rs");
    assert!(
        long_rows >= 2,
        "a name too wide for its column wraps onto continuation lines, got \
         {long_rows} laid-out row(s)"
    );
    assert_eq!(
        short_rows, 1,
        "the short name in the same column stays on one line, or the two are \
         not being compared at the same width"
    );
    assert!(
        long_size.y > short_size.y,
        "the painted galley's own extent grows with the wrap: long {long_size:?} \
         vs short {short_size:?}"
    );

    h.get_by_label(long).click();
    click_with_modifiers(&mut h, "short.rs", egui::Modifiers::COMMAND);

    let card = changes_card(&h);
    let height_of = |name: &str| {
        band_containing(
            &h,
            card,
            Palette::ROW_SELECTED,
            galley_origin(&h, name).unwrap_or_else(|| panic!("{name:?} paints its name")),
        )
        .height()
    };
    let (long_h, short_h) = (height_of(long), height_of("short.rs"));
    assert!(
        long_h > turbogit_ui::theme::FILE_ROW_HEIGHT,
        "a wrapped row grows past the single-line height, got {long_h}"
    );
    assert!(
        (short_h - turbogit_ui::theme::FILE_ROW_HEIGHT).abs() < 0.01,
        "and the common case keeps exactly the file-row height, got {short_h}"
    );
}

#[test]
fn group_headers_sit_on_the_section_band_with_no_nested_card() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "group-band");
    let branch = repo.branch();
    let name = repo
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    seed_tracked(&repo.path, "base.txt");
    std::fs::write(repo.path.join("untracked.txt"), "untracked\n").unwrap();
    seed_conflict(&repo.path, &branch);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));
    let card = changes_card(&h);

    for label in [format!("Toggle {name}"), "Merge conflicts (1)".to_owned()] {
        let header = h.get_by_label(&label).rect();
        let bands: Vec<egui::Rect> = filled_rects(&h)
            .into_iter()
            .filter(|(rect, fill)| *fill == Palette::SECTION_BG && rect.contains(header.center()))
            .map(|(rect, _)| rect)
            .collect();
        assert_eq!(
            bands.len(),
            1,
            "`{label}` paints exactly one section band under itself, found {bands:?}"
        );
        let band = bands[0];
        let radii: Vec<(egui::Color32, egui::CornerRadius)> = painted_rects(&h)
            .into_iter()
            .filter(|(rect, fill, _, _)| *rect == band && *fill != Palette::SECTION_BG)
            .map(|(_, fill, _, radius)| (fill, radius))
            .collect();
        assert!(
            radii.iter().all(|(fill, _)| *fill
                == turbogit_ui::ui::components::row_fill(
                    turbogit_ui::ui::components::RowState::FocusSelected
                )),
            "the only fill over a group band is the shared focus row state, so \
             a group cannot grow a second surface of its own: {radii:?} at {band:?}"
        );
        assert_eq!(
            painted_rects(&h)
                .into_iter()
                .filter(|(rect, fill, _, _)| *rect == band && *fill == Palette::SECTION_BG)
                .map(|(_, _, _, radius)| radius)
                .collect::<Vec<_>>(),
            vec![egui::CornerRadius::ZERO],
            "`{label}`'s band is square: a group is scaffolding on the card, not \
             a smaller card inside it"
        );
        assert!(
            (band.height() - turbogit_ui::theme::GROUP_ROW_HEIGHT).abs() < 0.01,
            "`{label}`'s band is the shared group-row height, got {band:?}"
        );
        assert!(
            band.width() >= header.width() - 0.5,
            "`{label}`'s band spans its whole header row (band {band:?}, header \
             {header:?}) — a band narrower than the row it heads is a chip"
        );
    }

    let nested: Vec<egui::Rect> = painted_rects(&h)
        .into_iter()
        .filter(|(rect, fill, _, radius)| {
            *rect != card
                && *fill == CARD_FILL
                && *radius == egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS)
                && card.contains_rect(*rect)
        })
        .map(|(rect, _, _, _)| rect)
        .collect();
    assert!(
        nested.is_empty(),
        "a group is a band, not a nested card: {nested:?} inside {card:?}"
    );

    let header = h.get_by_label("Merge conflicts (1)").rect();
    let first_row = h.get_by_label("C conf.txt").rect();
    let gap = first_row.top() - header.bottom();
    assert!(
        (-0.5..=turbogit_ui::theme::ITEM_SPACING.y).contains(&gap),
        "the list is separated from the group band by spacing or a hairline, \
         not by a border: gap {gap} between {header:?} and {first_row:?}"
    );
    let between: Vec<(egui::Rect, egui::Color32, f32)> = stroked_rects(&h)
        .into_iter()
        .filter(|(rect, _, _)| {
            rect.width() > 100.0
                && rect.top() >= header.bottom() - 0.5
                && rect.bottom() <= first_row.top() + 0.5
        })
        .collect();
    assert!(
        between.is_empty(),
        "nothing strokes the seam between a group header and its rows: {between:?}"
    );
}

#[test]
fn the_changes_list_reads_on_the_ink_ramp_and_never_the_dim_step() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "list-ink-ramp");
    let branch = repo.branch();
    let name = repo
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    seed_tracked(&repo.path, "base.txt");
    std::fs::create_dir_all(repo.path.join("src")).unwrap();
    std::fs::write(repo.path.join("src/app.rs"), "fn main() {}\n").unwrap();
    git(&repo.path, &["add", "src/app.rs"]);
    git(&repo.path, &["commit", "-q", "-m", "track app"]);
    std::fs::write(repo.path.join("src/app.rs"), "fn main() { println!(); }\n").unwrap();
    std::fs::write(repo.path.join("untracked.txt"), "untracked\n").unwrap();
    seed_conflict(&repo.path, &branch);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));
    let card = changes_card(&h);

    let ink_of = |text: &str| painted_ink(&h, text);
    assert_eq!(
        ink_of(&name),
        Some(Palette::INK),
        "a group header's name is the group's answer, and answers at the \
         primary ink"
    );
    assert_eq!(
        ink_of("base.txt"),
        Some(Palette::INK),
        "a file name reads at the primary ink — the status letter beside it \
         carries the state, so the name does not have to"
    );
    assert_eq!(
        ink_of("src/"),
        Some(Palette::INK_3),
        "the dim location suffix is the furniture around the name, and takes \
         the muted step — `INK_3`, never `INK_4`"
    );

    let dim: Vec<String> = painted_galleys(&h)
        .into_iter()
        .filter(|g| card.contains(g.pos) && g.color == Palette::INK_4)
        .map(|g| g.text)
        .collect();
    assert!(
        dim.is_empty(),
        "`INK_4` is sub-AA on every surface in the palette and has no legal \
         surface at all; nothing in the changes list may paint it: {dim:?}"
    );
}

fn commit_window_fn_body(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/commit_window.rs");
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));

    let mut stripped = String::with_capacity(src.len());
    let chars: Vec<char> = src.chars().collect();
    let (mut i, mut in_line, mut in_block, mut in_string) = (0usize, false, false, false);
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        if in_line {
            if c == '\n' {
                in_line = false;
                stripped.push('\n');
            } else {
                stripped.push(' ');
            }
        } else if in_block {
            if c == '*' && next == '/' {
                in_block = false;
                stripped.push_str("  ");
                i += 2;
                continue;
            }
            stripped.push(if c == '\n' { '\n' } else { ' ' });
        } else if in_string {
            if c == '\\' {
                stripped.push(c);
                if let Some(escaped) = chars.get(i + 1) {
                    stripped.push(*escaped);
                }
                i += 2;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            stripped.push(c);
        } else if c == '/' && next == '/' {
            in_line = true;
            stripped.push_str("  ");
            i += 2;
            continue;
        } else if c == '/' && next == '*' {
            in_block = true;
            stripped.push_str("  ");
            i += 2;
            continue;
        } else {
            if c == '"' {
                in_string = true;
            }
            stripped.push(c);
        }
        i += 1;
    }

    let declaration = format!("fn {name}(");
    let start = stripped
        .find(&declaration)
        .unwrap_or_else(|| panic!("commit_window.rs declares no `{declaration}`"));
    let rest = &stripped[start..];
    let mut depth = 0i32;
    let mut end = rest.len();
    for (offset, c) in rest.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = offset + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    rest[..end].to_owned()
}

#[test]
fn the_changes_list_reaches_its_fill_and_rail_through_the_shared_row_shell() {
    for name in ["change_row", "conflict_row", "paint_checkbox"] {
        let body = commit_window_fn_body(name);
        assert!(
            !body.contains("Palette::SELECTION"),
            "`{name}` must never name the current-ref band; a chosen list row \
             takes the row-selected fill, and the negative rule is written at \
             the site that chooses the fill:\n{body}"
        );
        assert!(
            !body.contains("Palette::BRAND"),
            "`{name}` must never name the brand fill; the accent belongs to the \
             primary action, the active tab underline and the row's rail:\n{body}"
        );
        assert!(
            !body.contains("paint_rail("),
            "`{name}` must reach the rail through `components::row_shell`, so \
             there is one rail painter and one rail width:\n{body}"
        );
    }

    for name in ["change_row", "conflict_row"] {
        let body = commit_window_fn_body(name);
        assert!(
            body.contains("components::row_shell("),
            "`{name}` paints its row's shell through `components::row_shell`, \
             which is where the fill and the rail are decided:\n{body}"
        );
        assert!(
            body.contains("components::RowShell::Railed"),
            "`{name}` asks for the railed variant, so a chosen row is marked by \
             the shared accent rather than by a local bar:\n{body}"
        );
    }
}

fn painted_font<S>(h: &Harness<'_, S>, needle: &str) -> egui::FontId {
    h.output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text() == needle => text
                .galley
                .job
                .sections
                .first()
                .map(|s| s.format.font_id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no painted galley carries {needle:?}"))
}

#[track_caller]
fn commit_card(h: &Harness<'_, AppState>) -> egui::Rect {
    let heading = galley_origin(h, "Commit message:")
        .unwrap_or_else(|| panic!("the commit card's heading `Commit message:` must paint"));
    card_rect(h, heading)
}

#[track_caller]
fn message_well(h: &Harness<'_, AppState>, card: egui::Rect) -> egui::Rect {
    let inner = card.width() - 2.0 * turbogit_ui::theme::PANEL_PADDING;
    let hits: Vec<egui::Rect> = filled_rects(h)
        .into_iter()
        .map(|(rect, _)| rect)
        .filter(|rect| (rect.width() - inner).abs() < 1.0 && card.contains_rect(*rect))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one full-inner-width rect on the commit card {card:?} \
         (the message well), found {hits:?}"
    );
    hits.into_iter().next().expect("one well")
}

#[track_caller]
fn meta_row(h: &Harness<'_, AppState>, card: egui::Rect) -> egui::Rect {
    let amend = h.get_by_label("Amend").rect();
    assert!(
        card.contains_rect(amend),
        "the meta-row control must sit inside the commit card {card:?}, got {amend:?}"
    );

    let inner = card.width() - 2.0 * turbogit_ui::theme::PANEL_PADDING;
    assert!(
        (amend.right() - (card.left() + turbogit_ui::theme::PANEL_PADDING) - inner).abs() < 1.0,
        "the meta row's control is right-aligned to the card's inner edge \
         (x={:.1}), got {:.1}",
        card.left() + turbogit_ui::theme::PANEL_PADDING,
        amend.right()
    );

    amend
}

#[track_caller]
fn commit_control_parts(h: &Harness<'_, AppState>) -> (egui::Rect, egui::Rect, egui::Rect) {
    let label = h.get_by_label("Commit changes").rect();
    let chevron = h.get_by_label("Commit split options").rect();
    let button = egui::Rect::from_min_max(label.min, chevron.max);

    let brand: Vec<egui::Rect> = filled_rects(h)
        .into_iter()
        .filter(|(rect, fill)| *fill == Palette::ACCENT && button.contains_rect(*rect))
        .map(|(rect, _)| rect)
        .collect();
    assert_eq!(
        brand.len(),
        2,
        "the Commit control paints exactly two filled halves (the label half and \
         the chevron half), found {brand:?} inside {button:?}. A third brand rect \
         here is a second control, not a split."
    );
    let rule: Vec<egui::Rect> = stroked_rects(h)
        .into_iter()
        .filter(|(rect, _, width)| (*width - 1.0).abs() < 0.01 && button.contains_rect(*rect))
        .map(|(rect, _, _)| rect)
        .collect();
    match rule.len() {
        1 => {}
        0 => panic!(
            "the Commit control paints no 1px rule between its halves ({button:?}). \
             A gap with nothing in it is the old separate-chevron-button reading: two \
             carets glued together. One hairline, on the button, is what makes the \
             two halves one control."
        ),
        n => panic!(
            "the Commit control carries {n} 1px strokes ({rule:?}) inside {button:?}. \
             One rule, between the halves. A stroke around the button's own edge is a \
             bordered button, which the R2 rules forbid."
        ),
    }
    let mut brand = brand;
    brand.sort_by(|a, b| a.left().partial_cmp(&b.left()).expect("finite left"));
    (brand[0], brand[1], rule[0])
}

#[track_caller]
fn changes_card_above(h: &Harness<'_, AppState>, card: egui::Rect) -> egui::Rect {
    let mut above: Vec<egui::Rect> = painted_rects(h)
        .into_iter()
        .filter(|(rect, fill, _, radius)| {
            *fill == CARD_FILL
                && *radius == egui::CornerRadius::same(turbogit_ui::theme::CARD_RADIUS)
                && (rect.width() - COMMIT_PANEL_WIDTH).abs() < 0.5
                && rect.bottom() <= card.top() + 1.0
        })
        .map(|(rect, _, _, _)| rect)
        .collect();
    assert_eq!(
        above.len(),
        1,
        "the file list's own card must still be painted in the column above the \
         commit card, which is docked with its bottom at y={:.1}. Found {above:?}. \
         A message well that grows until the file list is off the screen has \
         solved the message at the list's expense, and the list is what the well \
         shares the fixed column with.",
        card.bottom()
    );
    above.remove(0)
}

#[track_caller]
fn commit_column_bottom(h: &Harness<'_, AppState>) -> f32 {
    let bottoms: Vec<f32> = filled_rects(h)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::SIDEBAR)
        .map(|(rect, _)| rect.bottom())
        .collect();
    assert_eq!(
        bottoms.len(),
        1,
        "the shell's own sidebar surface is how this suite reads the work area's \
         bottom edge (the commit column is clipped to it), expected exactly one, \
         found {bottoms:?}"
    );
    bottoms[0]
}

const COMMIT_PANEL_WIDTH: f32 = turbogit_ui::ui::commit_window::COMMIT_PANEL_WIDTH;

#[test]
fn the_commit_card_is_a_well_one_meta_row_and_one_commit_control() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-three-things");
    seed_changes(&repo.path);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.state_mut().ui.commit_message = "three things".into();
    h.run();

    let card = commit_card(&h);
    let well = message_well(&h, card);
    let row = meta_row(&h, card);
    let commit = h.get_by_label("Commit changes").rect();
    let chevron = h.get_by_label("Commit split options").rect();

    assert!(
        card.contains(egui::pos2(well.left(), well.top())),
        "the well must be inside the commit card {card:?}, got {well:?}"
    );
    assert!(
        row.top() >= well.bottom() - 0.5,
        "the meta row sits under the well (well {well:?}, row {row:?})"
    );
    assert!(
        commit.top() > row.bottom(),
        "the Commit control is the card's action row, below the meta row: \
         meta row {row:?}, Commit {commit:?}. A second primary beside the \
         message controls is the reading the ticket rules out."
    );
    assert!(
        card.contains_rect(chevron),
        "the chevron half is part of the Commit control and lives in the card: \
         {chevron:?} in {card:?}"
    );

    assert!(
        (card.width() - COMMIT_PANEL_WIDTH).abs() < 0.5,
        "the commit card fills the fixed {COMMIT_PANEL_WIDTH}-wide column, got {:.1}",
        card.width()
    );
}

#[test]
fn the_commit_control_is_one_content_width_split_button_with_a_single_rule() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-split");
    seed_changes(&repo.path);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.state_mut().ui.commit_message = "split subject".into();
    h.run();

    let card = commit_card(&h);
    let (left, right, rule) = commit_control_parts(&h);

    assert_eq!(
        left.right(),
        rule.left(),
        "the label half must run right up to the rule — a gap here is the seam \
         that made the old control read as two buttons: {left:?} {rule:?}"
    );
    assert_eq!(
        right.left(),
        rule.right(),
        "the chevron half must start at the rule's other edge: {rule:?} {right:?}"
    );
    assert_eq!(
        rule.width(),
        1.0,
        "the rule between the halves is one hairline, got {:.1}pt",
        rule.width()
    );
    assert_eq!(
        rule.height(),
        left.height(),
        "the rule spans the control's full height, so the halves read as one \
         shape split rather than as two of different sizes: {rule:?} vs {left:?}"
    );

    assert_eq!(left.height(), right.height());
    assert!(
        (left.height() - 32.0).abs() < 0.5,
        "the Commit control takes the shared primary's 32pt, got {:.1}pt",
        left.height()
    );

    let inner = card.width() - 2.0 * turbogit_ui::theme::PANEL_PADDING;
    let total = right.right() - left.left();
    assert!(
        total < inner * 0.6,
        "the Commit control is content-width: it measures {:.1}pt of the {inner:.1}pt \
         row it sits in. A stretched primary is a different control, and a wider \
         one is how this card solves its layout by giving back the width the \
         ticket said it would keep.",
        total
    );
    let commit = h.get_by_label("Commit changes").rect();
    for label in ["Shelve\u{2026}", "Stash\u{2026}"] {
        let r = h.get_by_label(label).rect();
        assert!(
            r.left() > right.right(),
            "`{label}` sits to the right of the Commit control, got {r:?} vs {right:?}"
        );
        assert!(
            (r.center().y - commit.center().y).abs() < 1.0,
            "`{label}` is on the Commit control's own row: {r:?} vs {commit:?}"
        );
    }
}

#[test]
fn the_alternatives_menu_still_opens_from_the_chevron_half() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-menu");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    let before = repo.commit_count();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    stage_through_checkbox(&mut h, &repo.path, "base.txt");
    h.state_mut().ui.commit_message = "menu subject".into();
    h.run();

    let label = h.get_by_label("Commit changes").rect();
    let chevron = h.get_by_label("Commit split options").rect();
    assert!(
        chevron.left() >= label.right() - 1.0,
        "the chevron half is the second half of the control, to the right of the \
         label half: {chevron:?} vs {label:?}"
    );
    assert!(
        (chevron.height() - label.height()).abs() < 0.5,
        "both halves are the same height, or they read as two controls of \
         different sizes: {chevron:?} vs {label:?}"
    );

    open_commit_split_dropdown(&h);
    h.run();
    h.get_by_label("Commit and Push...").click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"menu subject".to_string())
            && repo.commit_count() == before + 1),
        "the chevron half still opens a working alternatives menu: subjects={:?} \
         count={} (was {before})",
        repo.subjects(),
        repo.commit_count()
    );
    assert_eq!(h.state().ui.dialog, Some(Dialog::Push));
}

#[test]
fn the_well_is_about_76px_and_the_file_list_still_gets_rows() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-heights");
    seed_changes(&repo.path);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));
    let card = commit_card(&h);
    let well = message_well(&h, card);
    let changes = changes_card_above(&h, card);
    let column_bottom = commit_column_bottom(&h);

    assert!(
        (well.height() - turbogit_ui::ui::commit_window::MESSAGE_WELL_HEIGHT).abs() <= 4.0,
        "the message well renders at about {:.0}pt ({} rows of body text in its \
         own padding), got {:.1}pt",
        turbogit_ui::ui::commit_window::MESSAGE_WELL_HEIGHT,
        turbogit_ui::ui::commit_window::MESSAGE_WELL_HEIGHT,
        well.height()
    );

    let remaining = column_bottom - changes.top();
    assert!(
        remaining >= 3.0 * turbogit_ui::theme::FILE_ROW_HEIGHT,
        "the file list keeps at least three rows of the {:.0}pt the column has \
         left after the well: {remaining:.1}pt of column, list card {changes:?}, \
         column bottom {column_bottom:.1}. A well that grows until the list is \
         one row solves the message at the list's expense.",
        remaining
    );

    let names = ["base.txt", "added.txt", "untracked.txt"];
    for name in names {
        let g = painted_galleys(&h)
            .into_iter()
            .find(|g| g.text == name)
            .unwrap_or_else(|| panic!("the file list must still paint `{name}`"));
        let clipped = h
            .output()
            .shapes
            .iter()
            .find(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text() == name))
            .map(|c| c.clip_rect)
            .expect("the row's painted shape");
        assert!(
            changes.contains(g.pos) && clipped.contains_rect(g.rect),
            "`{name}` must be inside the changes card {changes:?} and inside the \
             clip rect {clipped:?} that bounds it: painted {}",
            g.rect
        );
    }
}

#[test]
fn the_well_and_amend_paint_the_raised_on_card_fill() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-raised");
    seed_changes(&repo.path);

    let h = harness(app_state(std::slice::from_ref(&repo.path)));
    let card = commit_card(&h);

    for (who, rect) in [
        ("the message well", message_well(&h, card)),
        ("Amend's checkbox", {
            let node = h.get_by_label("Amend").rect();
            egui::Rect::from_center_size(
                egui::pos2(node.left() + 8.0, node.center().y),
                egui::vec2(16.0, 16.0),
            )
        }),
    ] {
        let on_it: Vec<(egui::Rect, egui::Color32)> = filled_rects(&h)
            .into_iter()
            .filter(|(painted, _)| card.contains_rect(*painted) && rect.intersects(*painted))
            .collect();
        assert!(
            on_it.iter().any(|(painted, fill)| {
                *fill == Palette::RAISED_ON_CARD && rect.contains_rect(*painted)
            }),
            "{who} must paint the RAISED_ON_CARD fill over {rect:?} — the card is a \
             content-surface card, so a raised control on it takes the SECOND rung \
             of the ladder. Painted there: {on_it:?}"
        );
        assert!(
            !on_it.iter().any(|(_, fill)| *fill == Palette::RAISED),
            "{who} must not paint the raised-on-BACKGROUND fill (Palette::RAISED, \
             #2B2D30) on a content-surface card: it measures 1.1:1 against \
             CONTENT_BG and is invisible there. Painted there: {on_it:?}"
        );
    }

    assert_ne!(Palette::RAISED, Palette::RAISED_ON_CARD);
}

#[test]
fn the_commit_control_is_the_only_brand_filled_control_on_the_card() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-brand");
    seed_changes(&repo.path);

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    stage_through_checkbox(&mut h, &repo.path, "base.txt");
    h.state_mut().ui.commit_message = "brand subject".into();
    h.run();

    let card = commit_card(&h);
    let label = h.get_by_label("Commit changes").rect();
    let chevron = h.get_by_label("Commit split options").rect();
    let control = egui::Rect::from_min_max(label.min, chevron.max);

    let mut brand: Vec<egui::Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(rect, fill)| *fill == Palette::ACCENT && card.contains_rect(*rect))
        .map(|(rect, _)| rect)
        .collect();
    brand.sort_by(|a, b| a.left().partial_cmp(&b.left()).expect("finite left"));
    assert_eq!(
        brand.len(),
        2,
        "the card's brand fill is the Commit control and nothing else: expected its \
         two halves, found {brand:?} on the card {card:?}"
    );
    for rect in &brand {
        assert!(
            control.contains_rect(*rect),
            "a brand-filled control is the card's only primary; {rect:?} is outside \
             the Commit control {control:?}"
        );
    }
    assert_eq!(
        Palette::ACCENT,
        Palette::BRAND,
        "one accent token, one name"
    );
}

#[test]
fn a_typed_message_is_editable_and_commits() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-typed");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    let before = repo.commit_count();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    stage_through_checkbox(&mut h, &repo.path, "base.txt");

    h.get_by_label("Commit message").focus();
    h.get_by_label("Commit message")
        .type_text("typed from the harness");
    h.run();
    assert_eq!(
        h.state().ui.commit_message,
        "typed from the harness",
        "the message well must still be editable"
    );

    commit_action_button(&h).click();
    h.run();
    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"typed from the harness".to_string())
            && repo.commit_count() == before + 1),
        "the Commit control must still dispatch the same commit: subjects={:?} \
         count={} (was {before})",
        repo.subjects(),
        repo.commit_count()
    );
}

#[test]
fn the_preview_card_paints_one_change_size_pair() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "card-counts");
    std::fs::write(repo.path.join("base.txt"), "a\nb\nc\n").unwrap();
    git(&repo.path, &["add", "base.txt"]);
    git(&repo.path, &["commit", "-q", "-m", "three lines"]);
    std::fs::write(repo.path.join("base.txt"), "a\nx\ny\nc\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    h.get_by_label("base.txt").click();
    h.run();

    assert!(
        wait_until(15_000, || {
            h.run();
            let text = painted_text(&h);
            text.iter().any(|t| t.contains("+2")) && text.iter().any(|t| t.contains("\u{2212}1"))
        }),
        "the change-size stats must paint once the diff lands: {:?}",
        painted_text(&h)
    );

    let card = card_rect(&h, h.get_by_label("Previewing base.txt").rect().center());
    let pairs: Vec<(String, egui::Rect)> = painted_galleys(&h)
        .into_iter()
        .filter(|g| card.contains(g.pos))
        .filter(|g| {
            g.text
                .strip_prefix('+')
                .or_else(|| g.text.strip_prefix('\u{2212}'))
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
        })
        .map(|g| (g.text, g.rect))
        .collect();
    let added: Vec<&str> = pairs
        .iter()
        .filter(|(t, _)| t.starts_with('+'))
        .map(|(t, _)| t.as_str())
        .collect();
    let removed: Vec<&str> = pairs
        .iter()
        .filter(|(t, _)| t.starts_with('\u{2212}'))
        .map(|(t, _)| t.as_str())
        .collect();
    assert_eq!(
        added,
        ["+2"],
        "the card paints exactly one `+N`, the file's own count: {pairs:?}"
    );
    assert_eq!(
        removed,
        ["\u{2212}1"],
        "the card paints exactly one `−M`, the file's own count: {pairs:?}"
    );
}

fn hooked_repo(parent: &Path, name: &str) -> Repo {
    let repo = temp_repo(parent, name);
    let hook = repo.path.join(".git").join("hooks").join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    repo
}

#[test]
fn commit_options_toggle_skips_hooks_and_unchecks_after_the_commit() {
    let parent = tempfile::tempdir().unwrap();
    let repo = hooked_repo(parent.path(), "hooked-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    stage_through_checkbox(&mut h, &repo.path, "base.txt");
    h.state_mut().ui.commit_message = "no-verify subject".into();
    h.run();

    h.get_by_label("Commit card options").click();
    h.run();
    h.get_by_label("Do not run git commit hooks").click();
    h.run();
    assert!(
        h.state().ui.commit_no_verify,
        "the menu item must check the card's one-shot toggle"
    );

    commit_action_button(&h).click();
    h.run();

    assert!(
        wait_until(15_000, || repo
            .subjects()
            .contains(&"no-verify subject".to_string())),
        "the commit must land past a failing pre-commit hook, i.e. with \
         --no-verify; got {:?}",
        repo.subjects()
    );
    assert!(
        !h.state().ui.commit_no_verify,
        "a successful commit must uncheck the toggle"
    );
}

#[test]
fn a_staged_repo_is_a_commit_target_by_default() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "checklist-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();
    let id = RootId(repo.path.clone().into());

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    assert!(
        h.state().commit_targets().is_empty(),
        "nothing is staged, so there are no targets yet"
    );
    assert!(
        h.state().ui.commit_repo_unchecks.is_empty(),
        "and nothing has been unchecked by hand"
    );

    stage_through_checkbox(&mut h, &repo.path, "base.txt");
    let targets: Vec<RootId> = h
        .state()
        .commit_targets()
        .iter()
        .map(|r| r.id.clone())
        .collect();
    assert_eq!(
        targets,
        vec![id],
        "staging a file makes its repo a commit target with no separate check"
    );
}

#[test]
fn commit_options_kebab_offers_the_multi_repo_checklist() {
    let parent = tempfile::tempdir().unwrap();
    let staged = temp_repo(parent.path(), "checklist-staged");
    let clean = temp_repo(parent.path(), "checklist-clean");
    std::fs::write(staged.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(&[staged.path.clone(), clean.path.clone()]));
    stage_through_checkbox(&mut h, &staged.path, "base.txt");

    h.get_by_label("Commit card options").click();
    h.run();
    assert_painted(&h, "Commit on multiple repos");

    let staged_before = h.get_all_by_label("checklist-staged").count();
    let clean_before = h.get_all_by_label("checklist-clean").count();
    // The submenu trigger's accessible label carries egui's `⏵` arrow, so it is
    // addressed by substring.
    h.get_by_label_contains("Commit on multiple repos").click();
    h.run();
    assert_eq!(
        h.get_all_by_label("checklist-staged").count(),
        staged_before + 1,
        "the checklist must add one row for the repo with staged content"
    );
    assert_eq!(
        h.get_all_by_label("checklist-clean").count(),
        clean_before,
        "the checklist must not list a repo with nothing staged"
    );
}

#[test]
fn primary_commit_commits_every_staged_repo_with_the_same_message() {
    let parent = tempfile::tempdir().unwrap();
    let a = temp_repo(parent.path(), "multi-a");
    let b = temp_repo(parent.path(), "multi-b");
    seed_tracked(&a.path, "a.txt");
    seed_tracked(&b.path, "b.txt");

    let mut h = harness(app_state(&[a.path.clone(), b.path.clone()]));
    stage_through_checkbox(&mut h, &a.path, "a.txt");
    h.get_by_label("Toggle multi-b").click();
    h.run();
    stage_through_checkbox(&mut h, &b.path, "b.txt");

    assert!(
        h.state().ui.commit_repo_unchecks.is_empty(),
        "nothing has been unchecked by hand"
    );
    assert_eq!(
        h.state().commit_targets().len(),
        2,
        "both staged repos are commit targets"
    );

    h.state_mut().ui.commit_message = "multi subject".into();
    h.run();
    commit_action_button(&h).click();
    h.step();
    h.step();

    assert!(
        wait_until(15_000, || a
            .subjects()
            .contains(&"multi subject".to_string())),
        "checked repo a must be committed; got {:?}",
        a.subjects()
    );
    assert!(
        wait_until(15_000, || b
            .subjects()
            .contains(&"multi subject".to_string())),
        "checked repo b must be committed with the same message; got {:?}",
        b.subjects()
    );
}

#[test]
fn unchecking_the_repo_keeps_it_out_of_the_commit() {
    let parent = tempfile::tempdir().unwrap();
    let repo = temp_repo(parent.path(), "uncheck-repo");
    std::fs::write(repo.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&repo.path)));
    stage_through_checkbox(&mut h, &repo.path, "base.txt");
    let id = RootId(repo.path.clone().into());
    h.state_mut().ui.commit_repo_unchecks.insert(id);
    h.state_mut().ui.commit_message = "unchecked subject".into();
    h.run();
    assert!(
        h.state().commit_targets().is_empty(),
        "the only staged repo is unchecked, so there are no fan-out targets"
    );

    commit_action_button(&h).click();
    h.run();
    assert!(
        !repo.subjects().contains(&"unchecked subject".to_string()),
        "an unchecked repo must not commit; got {:?}",
        repo.subjects()
    );
}

#[test]
fn unchecking_a_repo_in_the_checklist_keeps_the_menu_open() {
    let parent = tempfile::tempdir().unwrap();
    let staged = temp_repo(parent.path(), "checklist-toggle");
    std::fs::write(staged.path.join("base.txt"), "modified\n").unwrap();

    let mut h = harness(app_state(std::slice::from_ref(&staged.path)));
    stage_through_checkbox(&mut h, &staged.path, "base.txt");
    let id = RootId(staged.path.clone().into());
    assert!(
        h.state().commit_targets().iter().any(|r| r.id == id),
        "the staged repo starts as a commit target"
    );

    h.get_by_label("Commit card options").click();
    h.run();
    h.get_by_label_contains("Commit on multiple repos").click();
    h.run();

    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "checklist-toggle")
        .click();
    h.run();

    let _still_open = h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "checklist-toggle");
    assert!(
        h.state().ui.commit_repo_unchecks.contains(&id),
        "the toggle must record an explicit uncheck"
    );
    assert!(
        h.state().commit_targets().is_empty(),
        "an unchecked repo is no longer a commit target"
    );
}

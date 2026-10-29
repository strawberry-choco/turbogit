use egui::{Key, Modifiers, Pos2, Rect, Shape, Vec2};
use egui_kittest::{Harness, kittest::NodeT, kittest::Queryable};
use tempfile::TempDir;
use test_support::git_seed::{self, git, repo_with_one_commit};
use test_support::harness::{
    assert_painted, filled_rects, galley_origin, painted_galleys, painted_text, settle,
    shell_harness, shell_harness_over,
};
use turbogit_app::events::{AppEvent, LogBatchMode};
use turbogit_app::state::{AppState, Dialog, Tab};
use turbogit_domain::model::{ChangeRef, LogOpts, RootId, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_ui::theme::Palette;

#[derive(Debug, PartialEq)]
struct ModalState {
    tab: Tab,
    dialog: Option<Dialog>,
    vcs_popup: bool,
    command_palette: bool,
    branches_popup: bool,
    settings_open: bool,
}

fn modal_state(harness: &Harness<'_, AppState>) -> ModalState {
    let s = harness.state();
    ModalState {
        tab: s.ui.tab,
        dialog: s.ui.dialog,
        vcs_popup: s.ui.vcs_popup,
        command_palette: s.ui.command_palette,
        branches_popup: s.ui.branches_popup,
        settings_open: s.ui.settings_open,
    }
}

fn brand_rings(harness: &Harness<'_, AppState>) -> Vec<Rect> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rs) if rs.stroke.width >= 1.0 && rs.stroke.color == Palette::BRAND => {
                Some(rs.rect)
            }
            _ => None,
        })
        .collect()
}

#[track_caller]
fn assert_ring_covers(harness: &Harness<'_, AppState>, point: Pos2, what: &str) {
    let rings = brand_rings(harness);
    assert!(
        rings.iter().any(|r| r.contains(point)),
        "{what}: no BRAND focus ring covers {point:?}; rings painted: {rings:?}"
    );
}

fn viewport(w: f32, h: f32) -> Rect {
    Rect::from_min_size(Pos2::ZERO, Vec2::new(w, h))
}

#[track_caller]
fn assert_visible(harness: &Harness<'_, AppState>, label: &str, vp: Rect, what: &str) -> Rect {
    let rect = harness.get_by_label(label).rect();
    assert!(
        vp.intersects(rect),
        "{what}: `{label}` sits outside the viewport (rect {rect:?}, viewport {vp:?})"
    );
    rect
}

fn settle_long(harness: &mut Harness<'_, AppState>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut prev = String::new();
    while std::time::Instant::now() < deadline {
        harness.step();
        let fingerprint = format!(
            "{:?}|busy={}",
            painted_text(harness),
            harness.state().ui.busy
        );
        if fingerprint == prev && !harness.state().ui.busy && !harness.state().read_pending() {
            return;
        }
        prev = fingerprint;
    }
    panic!("layout did not settle within 15s");
}

/// Run exactly `n` frames. Scroll actions (`scroll_to_me`, `scroll_*`) are
/// applied by egui on the pass AFTER the one consuming the request, so a
/// text-fingerprint settle can legitimately exit before the scroll lands,
/// callers follow scroll actions with a few fixed steps instead.
fn steps(harness: &mut Harness<'_, AppState>, n: usize) {
    for _ in 0..n {
        harness.step();
    }
}

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
    alpha: PathBuf,
    c1: String,
    c2: String,
}

use std::path::{Path, PathBuf};

fn commit_file(dir: &Path, name: &str, msg: &str) -> String {
    git_seed::commit(dir, name, msg, msg);
    git_seed::git(dir, &["rev-parse", "HEAD"])
        .trim()
        .to_string()
}

fn seeded_project() -> Seed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    std::fs::create_dir_all(&alpha).expect("alpha dir");

    git(&alpha, &["init", "-b", "main"]);
    git(&alpha, &["config", "user.email", "test@example.com"]);
    git(&alpha, &["config", "user.name", "Test"]);
    let c1 = commit_file(&alpha, "file.txt", "alpha: initial commit");
    let c2 = commit_file(&alpha, "file.txt", "alpha: second commit");

    std::fs::write(alpha.join("file.txt"), "alpha: second commit\nlocal edit\n")
        .expect("working-tree edit");

    Seed {
        _tmp: tmp,
        project,
        alpha,
        c1,
        c2,
    }
}

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

fn polish_harness(seed: &Seed, size: (f32, f32), tab: Tab) -> Harness<'static, AppState> {
    let mut state = AppState::new(seed.project.clone());
    assert!(
        !state.multi.roots.is_empty(),
        "seeded repo root must be discovered"
    );
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    for root in state.multi.roots.clone() {
        let commits = engine
            .log(&root.path, &LogOpts::default())
            .expect("seeded log");
        state
            .tx
            .send(AppEvent::LogLoaded {
                root: root.id.clone(),
                commits: Ok(commits),
                mode: LogBatchMode::Replace,
            })
            .expect("send LogLoaded");
    }
    state.drain_events();
    if let Some(root) = state.multi.roots.first().cloned()
        && let Some(head) = state.caches.log(&root.id).and_then(|c| c.first().cloned())
    {
        state.ui.selected_commit = Some(head.id.clone());
    }
    state.ui.tab = tab;

    let mut harness = shell_harness_over(state, egui::vec2(size.0, size.1));
    settle_long(&mut harness);
    harness
}

#[derive(Clone, Copy)]
enum Shell {
    Welcome,
    WelcomeWithSettings,
    Seeded { tab: Tab },
    SeededWithPreview { tab: Tab },
}

impl Shell {
    fn seeds_a_repo(self) -> bool {
        !matches!(self, Shell::Welcome | Shell::WelcomeWithSettings)
    }
}

#[derive(Clone, Copy)]
enum Commit {
    Older,
    Head,
}

#[derive(Clone, Copy)]
enum Node {
    Label(&'static str),
    CommitRow {
        which: Commit,
        subject: &'static str,
    },
    SettingsCategoryButton,
}

impl Node {
    fn seed<'s>(&self, seed: Option<&'s Seed>) -> &'s Seed {
        seed.expect("a commit row's shell seeds a repository")
    }

    fn focus(&self, harness: &mut Harness<'static, AppState>, seed: Option<&Seed>) {
        use egui::accesskit::Role;
        match self {
            Node::Label(label) => harness.get_by_label(label).focus(),
            Node::CommitRow { which, subject } => harness
                .get_by_label(&commit_row_label(self.seed(seed), *which, subject))
                .focus(),
            Node::SettingsCategoryButton => harness
                .get_all_by_label("General")
                .find(|n| n.accesskit_node().role() == Role::Button)
                .unwrap()
                .focus(),
        }
    }

    #[track_caller]
    fn center(&self, harness: &Harness<'static, AppState>, seed: Option<&Seed>) -> Pos2 {
        use egui::accesskit::Role;
        match self {
            Node::Label(label) => harness.get_by_label(label).rect().center(),
            Node::CommitRow { which, subject } => harness
                .get_by_label(&commit_row_label(self.seed(seed), *which, subject))
                .rect()
                .center(),
            Node::SettingsCategoryButton => harness
                .get_all_by_label("General")
                .find(|n| n.accesskit_node().role() == Role::Button)
                .unwrap()
                .rect()
                .center(),
        }
    }
}

fn commit_row_label(seed: &Seed, which: Commit, subject: &str) -> String {
    let sha = match which {
        Commit::Older => &seed.c1,
        Commit::Head => &seed.c2,
    };
    format!("{} {subject}", short(sha))
}

#[derive(Clone, Copy)]
enum Settle {
    Frames,
    Stable,
}

struct FocusRing<'a> {
    case: &'a str,
    shell: Shell,
    settle: Settle,
    widgets: &'a [Focus],
}

struct Focus {
    node: Node,
    what: &'static str,
}

const FOCUS_RINGS: &[FocusRing<'static>] = &[
    FocusRing {
        case: "vocabulary_button_paints_brand_focus_ring_when_focused",
        shell: Shell::Seeded { tab: Tab::Commit },
        settle: Settle::Frames,
        widgets: &[Focus {
            node: Node::Label("Refresh changes"),
            what: "focused commit-window refresh button",
        }],
    },
    FocusRing {
        case: "text_input_paints_brand_focus_ring_when_focused",
        shell: Shell::Welcome,
        settle: Settle::Frames,
        widgets: &[Focus {
            node: Node::Label("Repository URL"),
            what: "focused clone URL input",
        }],
    },
    FocusRing {
        case: "tab_items_paint_brand_focus_rings",
        shell: Shell::Seeded { tab: Tab::Commit },
        settle: Settle::Stable,
        widgets: &[
            Focus {
                node: Node::Label("Log"),
                what: "focused shell tab item",
            },
            Focus {
                node: Node::Label("Changes"),
                what: "focused shell tab item",
            },
        ],
    },
    FocusRing {
        case: "welcome_action_card_paints_brand_focus_ring",
        shell: Shell::Welcome,
        settle: Settle::Frames,
        widgets: &[Focus {
            node: Node::Label("Open Project"),
            what: "focused welcome action card",
        }],
    },
    FocusRing {
        case: "log_rows_paint_brand_focus_rings",
        shell: Shell::Seeded { tab: Tab::Log },
        settle: Settle::Stable,
        widgets: &[
            Focus {
                node: Node::CommitRow {
                    which: Commit::Head,
                    subject: "alpha: second commit",
                },
                what: "focused commit row",
            },
            Focus {
                node: Node::CommitRow {
                    which: Commit::Older,
                    subject: "alpha: initial commit",
                },
                what: "focused older commit row",
            },
            Focus {
                node: Node::Label("file.txt"),
                what: "focused changed-file row",
            },
            Focus {
                node: Node::Label("All roots"),
                what: "focused roots-filter row",
            },
        ],
    },
    FocusRing {
        case: "diff_toolbar_controls_paint_brand_focus_rings",
        shell: Shell::SeededWithPreview { tab: Tab::Commit },
        settle: Settle::Stable,
        widgets: &[
            Focus {
                node: Node::Label("Unified"),
                what: "focused diff control Unified",
            },
            Focus {
                node: Node::Label("Next hunk"),
                what: "focused diff control Next hunk",
            },
        ],
    },
    FocusRing {
        case: "settings_category_row_paints_brand_focus_ring",
        shell: Shell::WelcomeWithSettings,
        settle: Settle::Frames,
        widgets: &[Focus {
            node: Node::SettingsCategoryButton,
            what: "focused settings category row",
        }],
    },
];

fn open_shell(shell: Shell, seed: Option<&Seed>) -> Harness<'static, AppState> {
    let repo = || seed.expect("a seeded shell seeds a repository");
    match shell {
        Shell::Welcome => {
            let (mut harness, _project) = shell_harness();
            settle(&mut harness);
            harness
        }
        Shell::WelcomeWithSettings => {
            let (mut harness, _project) = shell_harness();
            settle(&mut harness);
            harness.state_mut().ui.settings_open = true;
            settle(&mut harness);
            harness
        }
        Shell::Seeded { tab } => polish_harness(repo(), (1024.0, 768.0), tab),
        Shell::SeededWithPreview { tab } => {
            let mut harness = polish_harness(repo(), (1024.0, 768.0), tab);
            let alpha = repo().alpha.clone();
            harness.state_mut().ui.preview_change = Some(ChangeRef::new(
                RootId(alpha.to_path_buf().into()),
                alpha.join("file.txt"),
            ));
            settle_long(&mut harness);
            assert_painted(&harness, "Side-by-Side");
            harness
        }
    }
}

#[test]
fn every_focused_surface_paints_a_brand_ring_over_itself() {
    for row in FOCUS_RINGS {
        println!("focus-ring case: {}", row.case);

        let seed = row.shell.seeds_a_repo().then(seeded_project);
        let mut harness = open_shell(row.shell, seed.as_ref());

        for focus in row.widgets {
            focus.node.focus(&mut harness, seed.as_ref());
            match row.settle {
                Settle::Frames => settle(&mut harness),
                Settle::Stable => settle_long(&mut harness),
            }
            let center = focus.node.center(&harness, seed.as_ref());
            assert_ring_covers(&harness, center, focus.what);
        }
    }
}

#[test]
fn only_one_focus_ring_is_visible_at_a_time() {
    let seed = seeded_project();
    let mut harness = polish_harness(&seed, (1024.0, 768.0), Tab::Commit);

    harness.get_by_label("Refresh changes").focus();
    settle(&mut harness);

    let rings = brand_rings(&harness);
    assert_eq!(
        rings.len(),
        1,
        "exactly one focus ring may be painted; got {rings:?}"
    );
}

#[test]
fn shell_chrome_holds_at_small_window_sizes() {
    let seed = seeded_project();
    let harness = polish_harness(&seed, (560.0, 420.0), Tab::Commit);

    let vp = viewport(560.0, 420.0);
    assert_visible(&harness, "Changes", vp, "tab strip");
    assert_visible(&harness, "Log", vp, "tab strip");

    let status_top = 420.0 - 24.0;
    let status_bands: Vec<_> = filled_rects(&harness)
        .into_iter()
        .filter(|(r, c)| *c == Palette::SURFACE && r.top() >= status_top - 12.0)
        .collect();
    assert!(
        status_bands
            .iter()
            .any(|(r, _)| (r.height() - 24.0).abs() <= 4.0 && r.bottom() >= 420.0 - 10.0),
        "status bar must pin to the window bottom at 560x420; bands: {status_bands:?}"
    );
}

#[test]
fn log_panes_stay_reachable_at_small_window_sizes() {
    let seed = seeded_project();
    let mut harness = polish_harness(&seed, (1024.0, 768.0), Tab::Log);
    let row_label = format!("{} {}", short(&seed.c2), "alpha: second commit");

    for (w, h) in [(720.0f32, 480.0f32), (600.0, 400.0)] {
        harness.set_size(egui::vec2(w, h));
        settle_long(&mut harness);
        let vp = viewport(w, h);
        let what = &format!("at {w}x{h}");

        let search = assert_visible(&harness, "Search branches", vp, what);
        assert!(
            search.width() >= 60.0,
            "{what}: branches pane collapsed (search input {search:?})"
        );
        assert_visible(&harness, "Search commits", vp, what);
        assert_visible(&harness, &row_label, vp, what);
        assert_visible(&harness, "COMMIT DETAILS", vp, what);
        assert_painted(&harness, "CHANGED FILES");

        let hash_pos = galley_origin(&harness, &short(&seed.c2))
            .unwrap_or_else(|| panic!("{what}: newest commit hash not painted"));
        assert!(
            vp.contains(hash_pos),
            "{what}: commit hash painted at {hash_pos:?} outside {vp:?} — graph pane collapsed"
        );
    }
}

#[test]
fn welcome_page_stays_reachable_at_small_window_sizes() {
    let (mut harness, _project) = shell_harness();
    harness.set_size(egui::vec2(520.0, 440.0));
    settle(&mut harness);

    let vp = viewport(520.0, 440.0);

    for card in [
        "Open Project",
        "Initialize Repository",
        "Attach Workspace Root",
    ] {
        harness.get_by_label(card).scroll_to_me();
        steps(&mut harness, 4);
        settle(&mut harness);
        assert_visible(&harness, card, vp, "welcome cards");
    }

    harness.get_by_label("Repository URL").scroll_to_me();
    steps(&mut harness, 4);
    settle(&mut harness);
    assert_visible(&harness, "Repository URL", vp, "scrolled-to clone input");

    harness.get_by_label("Attach Workspace Root").scroll_to_me();
    steps(&mut harness, 6);
    settle(&mut harness);
    let label = painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "No recent projects yet.")
        .unwrap_or_else(|| panic!("recents column content not painted at {vp:?}"));
    assert!(
        label.rect.max.x <= vp.max.x + 1.0 && label.rect.min.x >= vp.min.x - 1.0,
        "recents column painted outside the viewport width ({:?} vs {vp:?})",
        label.rect
    );
}

#[test]
fn settings_modal_fits_small_heights() {
    let (mut harness, _project) = shell_harness();
    harness.set_size(egui::vec2(900.0, 480.0));
    settle(&mut harness);

    harness.state_mut().ui.settings_open = true;
    steps(&mut harness, 4);
    settle(&mut harness);

    let vp = viewport(900.0, 480.0);
    for button in ["Apply", "Cancel", "Restore defaults"] {
        assert_visible(&harness, button, vp, "settings footer");
    }

    harness.get_by_label("Appearance").click();
    steps(&mut harness, 4);
    settle(&mut harness);
    harness.get_by_label("Date format").scroll_to_me();
    steps(&mut harness, 4);
    settle(&mut harness);
    assert_visible(&harness, "Date format", vp, "scrolled-to setting row");
}

#[test]
fn ctrl_k_returns_to_the_commit_tool_window() {
    let seed = seeded_project();
    let mut harness = polish_harness(&seed, (1024.0, 768.0), Tab::Commit);
    harness.get_by_label("Log").click();
    settle_long(&mut harness);
    assert_eq!(harness.state().ui.tab, Tab::Log);

    harness.key_press_modifiers(Modifiers::CTRL, Key::K);
    settle_long(&mut harness);

    assert_eq!(harness.state().ui.tab, Tab::Commit);
}

#[test]
fn ctrl_shift_k_opens_the_push_dialog() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    assert_eq!(harness.state().ui.dialog, None);

    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::K);
    settle(&mut harness);
    assert_eq!(harness.state().ui.dialog, Some(Dialog::Push));
    assert_painted(&harness, "PUSH SCOPE");
    assert_painted(&harness, "This repo");
    assert_painted(&harness, "Force push (--force-with-lease)");
}
#[test]
fn ctrl_t_rescans_without_disturbing_shell_state() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.key_press_modifiers(Modifiers::CTRL, Key::T);
    settle(&mut harness);

    let after = modal_state(&harness);
    assert_eq!(after.tab, Tab::Commit);
    assert_eq!(after.dialog, None);
    assert!(!after.command_palette && !after.vcs_popup && !after.branches_popup);
}

#[test]
fn ctrl_shift_a_opens_the_command_palette() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::A);
    settle(&mut harness);

    assert!(harness.state().ui.command_palette);
    assert_painted(&harness, "Find Action");
}

#[test]
fn alt_backtick_opens_the_vcs_operations_popup() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.key_press_modifiers(Modifiers::ALT, Key::Backtick);
    settle(&mut harness);

    assert!(harness.state().ui.vcs_popup);
    assert_painted(&harness, "VCS Operations");
}

#[test]
fn frozen_shortcuts_fire_while_a_text_field_has_focus() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    harness.get_by_label("Repository URL").focus();
    settle(&mut harness);
    harness.key_press_modifiers(Modifiers::CTRL, Key::K);
    settle(&mut harness);
    assert_eq!(harness.state().ui.tab, Tab::Commit);

    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::A);
    settle(&mut harness);
    harness.key_press_modifiers(Modifiers::CTRL, Key::K);
    settle(&mut harness);
    let s = harness.state();
    assert_eq!(s.ui.tab, Tab::Commit);
    assert!(s.ui.command_palette, "palette must stay open across Ctrl+K");
}

#[test]
fn unmodified_keys_never_trigger_frozen_shortcuts() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);
    let before = modal_state(&harness);

    for key in [Key::K, Key::T, Key::A, Key::Backtick] {
        harness.key_press(key);
    }
    settle(&mut harness);

    assert_eq!(
        modal_state(&harness),
        before,
        "unmodified keys must not dispatch frozen shortcuts"
    );
}

#[test]
fn alt_backtick_and_ctrl_shift_k_coexist_without_conflict() {
    let (mut harness, _project) = shell_harness();
    settle(&mut harness);

    harness.key_press_modifiers(Modifiers::ALT, Key::Backtick);
    settle(&mut harness);
    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::K);
    settle(&mut harness);

    let s = harness.state();
    assert!(s.ui.vcs_popup, "VCS popup must remain open");
    assert_eq!(s.ui.dialog, Some(Dialog::Push));
}

const FLOAT_STROKE_WIDTH: f32 = 1.0;

/// Every stroked rectangle in the last frame, as `(rect, colour, width)`.
///
/// **Recursive**, and that is not an optimisation. A `egui::Window`, which is
/// every dialog and every popup, paints its frame as a `Shape::Vec` holding
/// `[shadow, RectShape]`, so a top-level-only scan sees a window's *content*
/// borders and never its chrome. `tests/feedback_chrome.rs` hit the same thing
/// and works around it in its own `popup_chrome`; the workaround belongs here
/// instead, once, so every floating-surface assertion in the workspace can use
/// it.
fn strokes(harness: &Harness<'_, AppState>) -> Vec<(Rect, egui::Color32, f32)> {
    fn walk(shape: &Shape, out: &mut Vec<(Rect, egui::Color32, f32)>) {
        match shape {
            Shape::Rect(r)
                if r.stroke.color != egui::Color32::TRANSPARENT && r.stroke.width > 0.0 =>
            {
                out.push((r.rect, r.stroke.color, r.stroke.width));
            }
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in harness.output().shapes.iter() {
        walk(&clipped.shape, &mut out);
    }
    out
}

fn float_chrome_at(harness: &Harness<'_, AppState>, probe: Pos2) -> Option<Rect> {
    strokes(harness)
        .into_iter()
        .find(|(rect, color, width)| {
            *color == Palette::LINE
                && (*width - FLOAT_STROKE_WIDTH).abs() < 0.01
                && rect.contains(probe)
        })
        .map(|(rect, _, _)| rect)
}

fn project() -> (TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_one_commit(tmp.path(), "alpha");
    (tmp, repo)
}

fn project_harness(state: AppState) -> Harness<'static, AppState> {
    let mut h = shell_harness_over(state, Vec2::new(1024.0, 768.0));
    settle(&mut h);
    h
}

#[test]
fn every_floating_surface_keeps_its_one_pixel_stroke() {
    use egui_kittest::kittest::Queryable as _;

    let mut checked: Vec<(&'static str, Rect)> = Vec::new();

    {
        let (_tmp, repo) = project();
        let mut state = AppState::for_roots(_tmp.path(), &[repo]);
        state.ui.dialog = Some(Dialog::NewBranch);
        let mut h = project_harness(state);
        settle(&mut h);
        let button = h.get_by_label("Create").rect();
        let chrome = float_chrome_at(&h, button.center()).unwrap_or_else(|| {
            panic!(
                "the New Branch dialog must keep its 1px stroke; strokes: {:#?}",
                strokes(&h)
            )
        });
        assert!(
            chrome.width() > 200.0 && chrome.height() > 100.0,
            "the float chrome is a dialog-sized surface, not a control border: \
             {chrome:?}"
        );
        checked.push(("dialog (New Branch)", chrome));
    }

    {
        let (harness, _tmp) = shell_harness();
        let mut h = harness;
        h.key_press_modifiers(Modifiers::ALT, Key::Backtick);
        settle(&mut h);
        let button = h.get_by_label("Refresh").rect();
        let chrome = float_chrome_at(&h, button.center()).unwrap_or_else(|| {
            panic!(
                "the VCS popup must keep its 1px stroke; strokes: {:#?}",
                strokes(&h)
            )
        });
        checked.push(("popup (VCS operations)", chrome));
    }

    {
        let (harness, _tmp) = shell_harness();
        let mut h = harness;
        h.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::A);
        settle(&mut h);
        let button = h.get_by_label("Refresh").rect();
        let chrome = float_chrome_at(&h, button.center()).unwrap_or_else(|| {
            panic!(
                "the command palette must keep its 1px stroke; strokes: {:#?}",
                strokes(&h)
            )
        });
        checked.push(("popup (command palette)", chrome));
    }

    {
        let mut h = shell_harness().0;
        h.state_mut().ui.settings_open = true;
        settle(&mut h);
        let button = h.get_by_label("Apply").rect();
        let chrome = float_chrome_at(&h, button.center()).unwrap_or_else(|| {
            panic!(
                "the settings modal must keep its 1px stroke; strokes: {:#?}",
                strokes(&h)
            )
        });
        checked.push(("dialog (settings)", chrome));
    }

    assert_eq!(
        checked.len(),
        4,
        "every floating surface in this table was probed: {checked:?}"
    );
    for (what, rect) in &checked {
        assert!(
            rect.width() > 0.0 && rect.height() > 0.0,
            "{what} is a real surface: {rect:?}"
        );
    }
}

#[test]
fn the_commit_and_branch_context_menus_keep_the_float_stroke() {
    use egui_kittest::kittest::Queryable as _;
    use test_support::harness::right_click_row;

    {
        let (tmp, repo) = project();
        std::fs::write(repo.join("second.txt"), "second\n").unwrap();
        for args in [vec!["add", "."], vec!["commit", "-q", "-m", "second"]] {
            let out = std::process::Command::new("git")
                .args(&args)
                .current_dir(&repo)
                .output()
                .unwrap();
            assert!(out.status.success());
        }
        let mut state = AppState::for_roots(tmp.path(), &[repo]);
        state.ui.tab = Tab::Log;
        let mut h = project_harness(state);
        settle(&mut h);
        right_click_row(&mut h, "second");
        settle(&mut h);
        let item = h.get_by_label("Copy hash").rect();
        let chrome = float_chrome_at(&h, item.center()).unwrap_or_else(|| {
            panic!(
                "the commit context menu must keep its 1px stroke; strokes: {:#?}",
                strokes(&h)
            )
        });
        assert!(
            chrome.contains_rect(item),
            "the stroke is the menu's own frame, around its items: {chrome:?} vs \
             {item:?}"
        );
    }
}

#[test]
fn every_content_card_paints_no_stroke_and_the_floating_one_does() {
    let (harness, _tmp) = shell_harness();
    let mut h = harness;
    h.set_size(Vec2::new(1024.0, 900.0));
    settle(&mut h);

    let cards = [
        "Open Project",
        "Initialize Repository",
        "Attach Workspace Root",
    ];
    for card in cards {
        let node = h.get_by_label(card);
        let rect = node.rect();
        let overlapping: Vec<_> = strokes(&h)
            .into_iter()
            .filter(|(stroke, _, _)| stroke.intersect(rect).is_positive())
            .collect();
        assert!(
            overlapping.is_empty(),
            "the `{card}` card is a content region and paints no stroke (R2: a \
             stroke means the surface floats); but {overlapping:#?} overlap \
             {rect:?}"
        );
    }
    let default = turbogit_ui::ui::widgets::CardFrame::default();
    assert!(
        !default.bordered,
        "the shared card paints no stroke by default"
    );
    assert!(
        turbogit_ui::ui::widgets::CardFrame::default()
            .bordered()
            .bordered,
        "…and the bordered variant is a separate, opt-in frame"
    );

    h.state_mut().ui.show_changelog = true;
    settle(&mut h);
    let title = h.get_by_label("What's New").rect();
    let chrome = strokes(&h)
        .into_iter()
        .find(|(rect, color, width)| {
            *color == Palette::LINE
                && (*width - FLOAT_STROKE_WIDTH).abs() < 0.01
                && rect.contains(title.center())
        })
        .map(|(rect, _, _)| rect)
        .unwrap_or_else(|| {
            panic!(
                "the changelog dialog floats and so keeps its 1px stroke; \
                 strokes: {:#?}",
                strokes(&h)
            )
        });
    assert!(
        chrome.width() >= 420.0,
        "the floating card is the changelog dialog's own surface: {chrome:?}"
    );
}

#[test]
fn every_dialog_separates_its_body_from_its_actions_with_the_footer_rule() {
    use egui_kittest::kittest::Queryable as _;

    let cases: [(Dialog, &str); 9] = [
        (Dialog::NewBranch, "Start from:"),
        (Dialog::Merge, "Allow unrelated histories"),
        (Dialog::Rebase, "Autosquash fixup commits"),
        (Dialog::Tag, "Annotated"),
        (Dialog::Shelve, "Shelf name:"),
        (Dialog::Stash, "Keep index"),
        (Dialog::RenameBranch, "to:"),
        (Dialog::CompareBranches, "Swap Branches"),
        (Dialog::CherryPickTarget, "Apply the selected commit"),
    ];

    for (dialog, tell) in cases {
        let (_tmp, repo) = project();
        let mut state = AppState::for_roots(_tmp.path(), &[repo]);
        state.ui.dlg.rebase_onto = "main".to_string();
        state.ui.dlg.rebase_mode = turbogit_domain::model::RebaseMode::Standard;
        state.ui.dlg.compare_left = "main".to_string();
        state.ui.dlg.compare_right = "feature".to_string();
        state.ui.dlg.rename_branch_name = "old".to_string();
        state.ui.dialog = Some(dialog);
        let mut h = project_harness(state);
        settle(&mut h);

        assert_painted(&h, tell);

        let rules: Vec<Rect> = filled_rects(&h)
            .into_iter()
            .filter(|(rect, color)| {
                *color == Palette::RULE_FOOTER && (rect.height() - 1.0).abs() < 0.01
            })
            .map(|(rect, _)| rect)
            .collect();
        assert_eq!(
            rules.len(),
            1,
            "the {dialog:?} dialog's body is closed off from its action slot by \
             exactly one `RULE_FOOTER` hairline; the frame painted {rules:?}"
        );
        let rule = rules[0];
        assert!(
            rule.width() > 100.0,
            "the footer rule spans the dialog's body, not one control: {rule:?}"
        );
        let chrome = strokes(&h)
            .into_iter()
            .filter(|(rect, color, width)| {
                *color == Palette::LINE
                    && (*width - FLOAT_STROKE_WIDTH).abs() < 0.01
                    && rect.contains(rule.center())
            })
            .map(|(rect, _, _)| rect)
            .min_by(|a, b| {
                (a.width() * a.height())
                    .partial_cmp(&(b.width() * b.height()))
                    .expect("finite rects")
            })
            .unwrap_or_else(|| {
                panic!("the {dialog:?} dialog floats, so its footer rule is inside its frame")
            });
        assert!(
            chrome.width() > rule.width(),
            "the rule spans the dialog's body inside its own frame: chrome \
             {chrome:?}, rule {rule:?}"
        );
        let action_labels: &[&str] = match dialog {
            Dialog::NewBranch => &["Create", "Cancel"],
            Dialog::Merge => &["Merge", "Cancel"],
            Dialog::Rebase => &["Start rebase", "Cancel"],
            Dialog::Tag => &["Create tag", "Cancel"],
            Dialog::Shelve => &["Shelve selected", "Cancel"],
            Dialog::Stash => &["Stash", "Cancel"],
            Dialog::RenameBranch => &["Rename", "Cancel"],
            Dialog::CompareBranches => &["Swap Branches", "Close"],
            Dialog::CherryPickTarget => &["Cancel"],
            other => panic!("the footer-rule table has no row for {other:?}"),
        };
        for label in action_labels {
            let button = h
                .get_all_by_role(egui::accesskit::Role::Button)
                .find(|n| {
                    n.accesskit_node().label().as_deref() == Some(*label)
                        && n.rect().top() >= rule.bottom() - 1.0
                        && n.rect().intersect(chrome) == n.rect()
                })
                .map(|n| n.rect());
            let button = button.unwrap_or_else(|| {
                panic!(
                    "the {dialog:?} dialog's action slot sits on the far side of \
                     its footer rule from the body: `{label}` was not painted \
                     below the rule {rule:?} inside the dialog's frame {chrome:?}"
                )
            });
            assert!(
                button.top() >= rule.bottom() - 1.0,
                "the {dialog:?} dialog's action slot is separated from its body \
                 by the footer rule: `{label}` at {button:?} against a rule at \
                 {rule:?}"
            );
            assert!(
                chrome.contains_rect(button),
                "the action is inside the dialog's own frame: {button:?} against \
                 {chrome:?}"
            );
        }
    }
}

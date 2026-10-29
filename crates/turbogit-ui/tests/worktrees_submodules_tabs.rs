use egui::Rect;
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use test_support::git_seed::git;
use test_support::harness::{
    KITTEST_DEFAULT_BOX, PaintedGalley, assert_painted, filled_circles, filled_rects,
    painted_galleys, painted_ink, painted_text, settle, shell_harness_over_unstyled,
};
use test_support::wcag::contrast;
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{RootId, Submodule, SubmoduleState, Worktree};
use turbogit_ui::theme::Palette;

/// Kept local, not `git_seed::repo_with_one_commit`: the canonicalize() is
/// load-bearing on macOS, where a temp dir is a symlink and the engine filters
/// the main worktree by path comparison.
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
    path.canonicalize().unwrap()
}

fn scratch(tag: &str) -> PathBuf {
    let parent = test_support::scratch_root(tag);
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    parent
}

fn harness(state: AppState) -> Harness<'static, AppState> {
    shell_harness_over_unstyled(state, KITTEST_DEFAULT_BOX, 1024)
}

fn step_until(h: &mut Harness<'_, AppState>, mut pred: impl FnMut(&Harness<'_, AppState>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        h.step();
        if pred(h) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("condition not met within 10s");
}

fn step_briefly(
    h: &mut Harness<'_, AppState>,
    mut pred: impl FnMut(&Harness<'_, AppState>) -> bool,
) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        h.step();
        if pred(h) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn add_worktree(repo: &Path, parent: &Path, name: &str, branch: &str) -> PathBuf {
    git(repo, &["branch", branch]);
    let wt = parent.join(name);
    git(repo, &["worktree", "add", wt.to_str().unwrap(), branch]);
    wt.canonicalize().unwrap()
}

fn painted_contains(h: &Harness<'_, AppState>, needle: &str) -> bool {
    painted_text(h).iter().any(|t| t.contains(needle))
}

fn worktree(root: &RootId, path: PathBuf, branch: &str, dirty: Option<bool>) -> Worktree {
    Worktree {
        path,
        branch: branch.to_string(),
        dirty,
        root: root.clone(),
    }
}

fn pane_harness(
    tag: &str,
    seed: impl FnOnce(&mut AppState, &Path),
    show: fn(&mut egui::Ui, &mut AppState),
) -> Harness<'static, AppState> {
    let parent = scratch(tag);
    let repo = temp_repo(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, std::slice::from_ref(&repo));
    seed(&mut state, &parent);

    let mut fonts_installed = false;
    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 620.0))
        .build_ui_state(
            move |ui, state| {
                turbogit_ui::theme::configure_style(ui.ctx());
                if !fonts_installed {
                    turbogit_ui::theme::install_fonts(ui.ctx());
                    fonts_installed = true;
                }
                egui::CentralPanel::default().show(ui, |ui| {
                    show(ui, state);
                });
            },
            state,
        );
    settle(&mut harness);
    harness
}

fn worktrees_pane_harness(
    tag: &str,
    worktrees: &[(&str, &str, Option<bool>)],
) -> Harness<'static, AppState> {
    pane_harness(
        tag,
        |state, parent| {
            let root = state.selected_root.clone().expect("one discovered root");
            state.caches.store_worktrees(
                root.clone(),
                worktrees
                    .iter()
                    .map(|(dir, branch, dirty)| worktree(&root, parent.join(dir), branch, *dirty))
                    .collect(),
            );
        },
        turbogit_ui::ui::worktrees::show,
    )
}

fn submodule(
    root: &RootId,
    path: &str,
    head: Option<&str>,
    recorded: Option<&str>,
    state: SubmoduleState,
) -> Submodule {
    Submodule {
        path: path.into(),
        head: head.map(str::to_owned),
        recorded: recorded.map(str::to_owned),
        state,
        root: root.clone(),
    }
}

fn submodules_pane_harness(
    tag: &str,
    seed: impl FnOnce(&RootId) -> Vec<Submodule>,
) -> Harness<'static, AppState> {
    pane_harness(
        tag,
        |state, _parent| {
            let root = state.selected_root.clone().expect("one discovered root");
            let subs = seed(&root);
            state.caches.store_submodules(root.clone(), subs);
        },
        turbogit_ui::ui::submodules::show,
    )
}

fn state_cells(h: &Harness<'_, AppState>) -> Vec<PaintedGalley> {
    status_cells(h, "STATE")
}

fn status_cells(h: &Harness<'_, AppState>, label: &str) -> Vec<PaintedGalley> {
    let state_x = painted_galleys(h)
        .into_iter()
        .find(|g| g.text == label)
        .map(|g| g.pos.x)
        .expect("the state column is labelled");
    let header_bottom = painted_galleys(h)
        .into_iter()
        .find(|g| g.text == label)
        .map(|g| g.rect.bottom())
        .expect("the state column is labelled");
    let mut cells: Vec<PaintedGalley> = painted_galleys(h)
        .into_iter()
        .filter(|g| (state_x..state_x + 200.0).contains(&g.pos.x) && g.pos.y > header_bottom)
        .collect();
    cells.sort_by(|a, b| a.pos.y.total_cmp(&b.pos.y));
    cells
}

struct HeaderBand {
    title: PaintedGalley,
    rule: Rect,
}

fn pane_header_band(h: &Harness<'_, AppState>, title: &str) -> HeaderBand {
    let rule = filled_rects(h)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::RULE_STRUCTURAL)
        .map(|(rect, _)| rect)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap_or_else(|| panic!("the pane header underlines itself in RULE_STRUCTURAL"));
    let title = painted_galleys(h)
        .into_iter()
        .find(|g| g.text == title)
        .unwrap_or_else(|| panic!("the pane header's title is {title}"));
    assert!(
        title.rect.bottom() <= rule.top() + 0.01,
        "the title must sit in the header band above its own rule: title {:?}, rule {:?}",
        title.rect,
        rule
    );
    HeaderBand { title, rule }
}

fn worktrees_header_harness() -> Harness<'static, AppState> {
    worktrees_pane_harness(
        "wt17-header",
        &[("wt-a", "feature-a", None), ("wt-b", "feature-b", None)],
    )
}

fn submodules_header_harness() -> Harness<'static, AppState> {
    diverged_and_matching("sub18-header")
}

struct PaneHeader {
    case: &'static str,
    title: &'static str,
    harness: fn() -> Harness<'static, AppState>,
    primary: Option<&'static str>,
    count_must_not_wear: Option<(&'static str, egui::Color32)>,
}

const PANE_HEADERS: [PaneHeader; 2] = [
    PaneHeader {
        case: "Worktrees",
        title: "WORKTREES",
        harness: worktrees_header_harness,
        primary: Some("Add worktree"),
        count_must_not_wear: None,
    },
    PaneHeader {
        case: "Submodules",
        title: "SUBMODULES",
        harness: submodules_header_harness,
        primary: None,
        count_must_not_wear: Some(("COUNTER", Palette::COUNTER)),
    },
];

#[test]
fn both_pane_headers_are_the_shared_pane_header_with_a_neutral_count_chip() {
    for row in &PANE_HEADERS {
        let case = row.case;
        let h = (row.harness)();
        let header = pane_header_band(&h, row.title);
        assert!(
            header.title.rect.top()
                >= header.rule.top() - turbogit_ui::ui::widgets::PANE_HEADER_HEIGHT,
            "{case}: the title sits in the shared 28px band above the rule: title \
             {:?}, rule {:?}",
            header.title.rect,
            header.rule
        );

        assert_eq!(
            painted_ink(&h, row.title),
            Some(Palette::INK_3),
            "{case}: a pane title is the shared 9px muted mark; an ad-hoc band is \
             what this ratchet exists to catch"
        );
        assert_eq!(
            painted_font_size(&h, row.title),
            turbogit_ui::theme::TYPE_SECTION,
            "{case}: the pane title is the shared pane-title type size"
        );

        let count = painted_galleys(&h)
            .into_iter()
            .find(|g| g.text == "2" && g.rect.bottom() <= header.rule.top())
            .unwrap_or_else(|| {
                panic!(
                    "{case}: the header's count chip carries the pane's own count (2 \
                     here); painted in the header band: {:#?}",
                    painted_galleys(&h)
                        .iter()
                        .filter(|g| g.rect.bottom() <= header.rule.top())
                        .map(|g| (&g.text, g.rect))
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(
            count.color,
            Palette::INK_2,
            "{case}: a count chip is secondary ink on the raised fill; the accent is \
             not a counter's to wear"
        );
        assert_eq!(
            count.family,
            egui::FontFamily::Monospace,
            "{case}: counts are data: the count chip's face is the data face"
        );
        assert!(
            filled_rects(&h)
                .iter()
                .any(|(rect, fill)| *fill == Palette::RAISED && rect.contains_rect(count.rect)),
            "{case}: the count sits on a `RAISED` fill — the count chip's own raised \
             role, and not the ref chip's raised-on-card one, which is a different \
             value"
        );

        if let Some(label) = row.primary {
            let add = h.get_by_label(label);
            assert!(
                add.rect().center().x > 600.0,
                "{case}: the `{label}` control sits in the header's right-aligned \
                 action slot: {:?}",
                add.rect()
            );
            assert!(
                filled_rects(&h)
                    .iter()
                    .any(|(rect, fill)| *fill == Palette::BRAND && rect.intersects(add.rect())),
                "{case}: the control is the pane's one **primary** action, so it \
                 fills with BRAND; the compact ghost it replaces filled with nothing"
            );
        }
        if let Some((token, tone)) = row.count_must_not_wear {
            let reserved: Vec<Rect> = filled_rects(&h)
                .into_iter()
                .filter(|(_, fill)| *fill == tone)
                .map(|(rect, _)| rect)
                .collect();
            assert!(
                reserved.is_empty(),
                "{case}: the reserved {token} means dirt and unpulled; a count of \
                 this pane's list is neither, and the count chip exists so this \
                 number does not have to borrow it. {token}-filled rects: \
                 {reserved:?}"
            );
        }
    }
}

#[test]
fn worktree_columns_are_labelled_once_and_every_row_starts_where_the_header_does() {
    let h = worktrees_pane_harness("wt17-columns", &[("wt-a", "feature-a", Some(false))]);

    for label in ["PATH", "BRANCH", "HEAD", "STATE", "ACTIONS"] {
        let n = painted_galleys(&h)
            .iter()
            .filter(|g| g.text == label)
            .count();
        assert_eq!(
            n, 1,
            "`{label}` is labelled exactly once, by the shared row"
        );
    }

    let path = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text.ends_with("wt-a"))
        .expect("the row paints its path");
    let chip = filled_rects(&h)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::RAISED_ON_CARD)
        .min_by_key(|(rect, _)| (rect.width() * rect.height()) as i64)
        .map(|(rect, _)| rect)
        .unwrap_or_else(|| panic!("the row paints its branch as a ref chip"));
    let dot = filled_circles(&h)
        .into_iter()
        .find(|(_centre, radius, _)| *radius == turbogit_ui::ui::components::STATE_DOT_R)
        .map(|(centre, radius, _)| centre.x - radius)
        .unwrap_or_else(|| panic!("the row paints its state as the shared mark pair"));

    for (label, cell_x) in [
        ("PATH", path.rect.left()),
        ("BRANCH", chip.left()),
        ("STATE", dot),
    ] {
        let header_x = painted_galleys(&h)
            .into_iter()
            .find(|g| g.text == label)
            .unwrap_or_else(|| panic!("`{label}` is labelled"))
            .pos
            .x;
        assert!(
            (header_x - cell_x).abs() < 0.01,
            "the {label} label is not over its cell: header at {header_x}, row at \
             {cell_x}. The header and the rows must read the same column table, so \
             a column cannot be narrow in the header and wide in the rows — and \
             nothing notices until the frame is on screen."
        );
    }

    let actions_x = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "ACTIONS")
        .expect("ACTIONS is labelled")
        .rect
        .right();
    let remove = h.get_by_label("Remove wt-a");
    assert!(
        (actions_x - remove.rect().right()).abs() < 0.01,
        "a trailing column's label ends where its cell ends: label at {actions_x}, \
         remove at {:?}",
        remove.rect()
    );
}

#[test]
fn the_head_cell_is_empty_rather_than_filled_with_a_placeholder() {
    let h = worktrees_pane_harness("wt17-head-empty", &[("wt-a", "feature-a", Some(false))]);

    let head = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "HEAD")
        .expect("HEAD is labelled");
    let state_label = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "STATE")
        .expect("STATE is labelled");
    let band = head.rect.left()..state_label.rect.left();

    let in_head_band: Vec<String> = painted_galleys(&h)
        .into_iter()
        .filter(|g| band.contains(&g.pos.x) && g.pos.y > head.rect.bottom())
        .map(|g| g.text)
        .collect();
    assert!(
        in_head_band.is_empty(),
        "the HEAD cell paints nothing: an empty cell is a real state, and a \
         placeholder glyph would read as a value the pane knows. Found: \
         {in_head_band:?}"
    );
}

#[test]
fn the_worktree_path_is_the_primary_column_in_the_data_face_at_its_own_width() {
    let h = worktrees_pane_harness("wt17-path", &[("wt-a", "feature-a", Some(false))]);

    let painted = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text.ends_with("wt-a"))
        .expect("the row paints its path in full");

    assert_eq!(
        painted.color,
        Palette::INK,
        "the path is the row's **primary** column: primary ink, so the reader can \
         tell one row from another without reading anything else"
    );
    assert_eq!(
        painted.family,
        egui::FontFamily::Monospace,
        "a path is data, so it takes the data face — the same rule the ref chip \
         and the hash chip follow"
    );
    assert_ne!(
        painted.family,
        egui::FontFamily::Proportional,
        "the path must not fall back to the chrome face"
    );

    let branch = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "BRANCH")
        .expect("BRANCH is labelled");
    assert!(
        painted.rect.right() <= branch.rect.left(),
        "the path takes the width the other columns leave it: its measured right \
         edge is {:?}, past the BRANCH column at {:?}. Asserted from geometry \
         because a galley's text accessor returns the unwrapped input — a path \
         that elided to fit and a path that overflowed read identically through \
         `.text`.",
        painted.rect,
        branch.rect
    );
    let full = ui_text_width(&h, &painted.text);
    assert!(
        full > painted.rect.width() + 1.0,
        "the path column is narrower than this path, so the claim above is the \
         elision case and not a coincidence: laid out in full it is {full}pt, \
         painted at {:?}",
        painted.rect
    );
}

#[test]
fn the_worktree_branch_is_the_neutral_ref_chip_and_not_the_brand_token() {
    let h = worktrees_pane_harness("wt17-branch", &[("wt-a", "feature-a", Some(false))]);

    let branch = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "feature-a")
        .unwrap_or_else(|| panic!("the branch name paints"));

    let add = h.get_by_label("Add worktree");
    let competing: Vec<Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(rect, fill)| {
            *fill == Palette::BRAND
                && (rect.min.x - add.rect().min.x).abs() > 1.0
                && (rect.width() - add.rect().width()).abs() > 1.0
        })
        .map(|(rect, _)| rect)
        .collect();
    assert!(
        competing.is_empty(),
        "no worktree row paints a blue: the pane's one brand fill is the Add \
         primary in the header, and these brand-filled rects are not it: \
         {competing:?}"
    );

    let fill = filled_rects(&h)
        .into_iter()
        .filter(|(rect, _)| rect.contains_rect(branch.rect))
        .min_by_key(|(rect, _)| (rect.width() * rect.height()) as i64)
        .map(|(_, fill)| fill)
        .unwrap_or_else(|| {
            panic!(
                "the branch sits on a filled chip rect; the ref chip is the only \
                 thing in a worktree row that fills its background"
            )
        });

    assert_eq!(
        fill,
        Palette::RAISED_ON_CARD,
        "a ref chip fills the raised-on-card surface: it steps up from the \
         surface it sits on, which is what makes a control visible on a card at all"
    );
    assert_ne!(
        fill,
        Palette::BRAND,
        "a ref chip is **neutral** (R6). A worktree branch painted in the brand is \
         the canonical R1 violation: a worktree row must not add a blue that \
         competes with the pane's one primary action."
    );
    assert_eq!(
        branch.color,
        Palette::INK_2,
        "the ref chip's ink is secondary — the muted step is not legal on a raised \
         surface, and the constructor is the place that rule would break"
    );
    assert_ne!(
        branch.color,
        Palette::BRAND,
        "the branch name's own ink must not be the brand either"
    );
}

#[test]
fn an_unprobed_worktree_renders_a_visible_probing_state_and_the_cell_is_never_empty() {
    let h = worktrees_pane_harness(
        "wt17-probing",
        &[("wt-a", "feature-a", None), ("wt-b", "feature-b", None)],
    );

    let cells = state_cells(&h);
    assert_eq!(
        cells.len(),
        2,
        "the state cell of a worktree in the list is never empty: a blank cell is \
         ambiguous between `clean` and `not known yet`, and `clean` is the one \
         word in this column a reader is entitled to act on. Painted: {:?}",
        cells.iter().map(|g| &g.text).collect::<Vec<_>>()
    );
    for cell in &cells {
        assert!(
            cell.rect.width() > 1.0 && cell.rect.height() > 1.0,
            "the state cell paints a real mark, not a zero-extent artefact: {cell:?}"
        );
    }

    for cell in &cells {
        assert!(
            cell.text.starts_with("probing"),
            "a worktree whose probe has not returned says so; painted in its state \
             cell: {:?}",
            cell.text
        );
    }
}

#[test]
fn only_a_settled_clean_probe_renders_the_word_clean() {
    let h = worktrees_pane_harness(
        "wt17-clean-only",
        &[
            ("wt-clean", "feature-clean", Some(false)),
            ("wt-dirty", "feature-dirty", Some(true)),
            ("wt-probing", "feature-probing", None),
        ],
    );

    let cells = state_cells(&h);
    let words: Vec<&str> = cells.iter().map(|g| g.text.as_str()).collect();
    assert_eq!(
        words,
        ["clean", "dirty", "probing…"],
        "one state word per row, in row order, and the unsettled row is not the \
         one claiming `clean`"
    );
    assert_eq!(
        words.iter().filter(|w| **w == "clean").count(),
        1,
        "`clean` is the only thing that renders the word clean, and exactly one \
         row's probe settled clean here: {words:?}"
    );
    let probing = cells
        .iter()
        .find(|g| g.pos.y > cells[0].pos.y && g.pos.y < cells[2].pos.y.max(cells[0].pos.y))
        .or_else(|| cells.iter().find(|g| g.text.starts_with("probing")))
        .expect("the unsettled row says it is still asking");
    assert_ne!(
        probing.text, "clean",
        "`clean` is the only thing that renders the word clean: a row whose probe \
         has not returned must never fall back to the word a reader acts on"
    );
}

#[test]
fn remove_is_a_quiet_row_action_and_still_answers_to_its_name() {
    let h = worktrees_pane_harness("wt17-quiet-remove", &[("wt-a", "feature-a", Some(false))]);

    let remove = h.get_by_label("Remove wt-a");
    assert!(
        remove.rect().height() >= 24.0,
        "the quiet row action still occupies a clickable target: {:?}",
        remove.rect()
    );

    let label = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "Remove wt-a")
        .expect("the row action paints its name");
    let plate: Vec<Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(rect, _)| {
            (rect.min.x - remove.rect().min.x).abs() < 1.0
                && (rect.min.y - remove.rect().min.y).abs() < 1.0
                && (rect.width() - remove.rect().width()).abs() < 1.0
                && (rect.height() - remove.rect().height()).abs() < 1.0
        })
        .map(|(rect, _)| rect)
        .collect();
    assert!(
        plate.is_empty(),
        "Remove is a quiet row action, not a full button: a quiet action fills \
         nothing at rest, and a full button fills a plate at exactly the control's \
         own rect. Found a plate at: {plate:?}"
    );

    assert_ne!(
        label.color,
        Palette::BRAND,
        "the row's remove verb must not wear the accent; the pane's one blue is \
         the Add primary"
    );
    assert_eq!(
        label.color,
        Palette::INK_2,
        "a quiet action is the content's secondary ink at rest"
    );
}

#[test]
fn the_new_worktree_dialog_keeps_two_distinct_accessibility_labels_on_its_inputs() {
    let parent = scratch("wt17-dialog-labels");
    let repo = temp_repo(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, std::slice::from_ref(&repo));
    state.ui.tab = Tab::Worktrees;
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Add worktree").click();
    settle(&mut h);

    let path = h.get_by_label("Worktree path input");
    let branch = h.get_by_label("Worktree branch input");
    assert_ne!(
        format!("{:?}", path.rect()),
        format!("{:?}", branch.rect()),
        "the two labelled inputs are two fields, not one node reached twice"
    );

    let text_inputs = h
        .query_all_by_role(egui::accesskit::Role::TextInput)
        .count();
    assert!(
        text_inputs >= 2,
        "the dialog keeps both of its text inputs; found {text_inputs}"
    );
}

fn painted_font_size<S>(harness: &Harness<'_, S>, needle: &str) -> f32 {
    harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text() == needle => text
                .galley
                .job
                .sections
                .first()
                .map(|s| s.format.font_id.size),
            _ => None,
        })
        .unwrap_or_else(|| panic!("`{needle}` was not painted"))
}

fn ui_text_width<S>(harness: &Harness<'_, S>, text: &str) -> f32 {
    let font_id = harness
        .output()
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(t) if t.galley.text() == text => t
                .galley
                .job
                .sections
                .first()
                .map(|s| s.format.font_id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("`{text}` was not painted"));
    let job = egui::text::LayoutJob {
        text: text.to_owned(),
        sections: vec![egui::text::LayoutSection {
            leading_space: 0.0,
            byte_range: egui::text::ByteIndex(0)..egui::text::ByteIndex(text.len()),
            format: egui::text::TextFormat {
                font_id,
                ..Default::default()
            },
        }],
        ..Default::default()
    };
    harness.ctx.fonts_mut(|f| f.layout_job(job).size().x)
}

#[track_caller]
fn case_asserts_painted(case: &str, h: &Harness<'_, AppState>, needle: &str, painted: bool) {
    let texts = painted_text(h);
    let seen = texts.iter().any(|t| t.contains(needle));
    assert_eq!(
        seen, painted,
        "{case}: `{needle}` painted = {painted}; painted text:\n{texts:#?}"
    );
}

struct ListArrival {
    case: &'static str,
    tag: &'static str,
    worktree: Option<&'static str>,
    dirty: bool,
    arrived: &'static str,
    then_waits: Option<&'static str>,
    paints: &'static [&'static str],
    gone: &'static [&'static str],
}

const LIST_ARRIVALS: [ListArrival; 4] = [
    ListArrival {
        case: "the cheap list alone: the row paints, and no probe has run",
        tag: "wt-sub-tabs-list",
        worktree: Some("wt-feature"),
        dirty: true,
        arrived: "wt-feature",
        then_waits: None,
        paints: &["wt-feature", "feature", "Add worktree"],
        gone: &[],
    },
    ListArrival {
        case: "the list, then the window-open probe fills the dirty label in",
        tag: "wt-sub-tabs-dirty-probe",
        worktree: Some("wt-feature"),
        dirty: true,
        arrived: "wt-feature",
        then_waits: Some("dirty"),
        paints: &["wt-feature", "feature"],
        gone: &[],
    },
    ListArrival {
        case: "a clean row: the loading text is gone, with no probe in the path",
        tag: "wt-sub-tabs-loading",
        worktree: Some("wt-feature"),
        dirty: false,
        arrived: "wt-feature",
        then_waits: None,
        paints: &[],
        gone: &["Loading worktrees"],
    },
    ListArrival {
        case: "an empty list: the empty state appears, with no slow scan first",
        tag: "wt-sub-tabs-empty",
        worktree: None,
        dirty: false,
        arrived: "No linked worktrees",
        then_waits: None,
        paints: &["Add worktree"],
        gone: &[],
    },
];

#[test]
fn the_worktrees_list_lands_before_anything_is_probed() {
    for row in &LIST_ARRIVALS {
        let parent = scratch(row.tag);
        let repo = temp_repo(&parent, "alpha");
        if let Some(name) = row.worktree {
            let wt = add_worktree(&repo, &parent, name, "feature");
            if row.dirty {
                std::fs::write(wt.join("base.txt"), "changed\n").unwrap();
            }
        }

        let mut state = AppState::for_roots(&parent, &[repo]);
        state.ui.tab = Tab::Worktrees;
        let mut h = harness(state);
        settle(&mut h);

        step_until(&mut h, |h| painted_contains(h, row.arrived));
        if let Some(needle) = row.then_waits {
            step_until(&mut h, |h| painted_contains(h, needle));
        }
        for needle in row.paints {
            case_asserts_painted(row.case, &h, needle, true);
        }
        for needle in row.gone {
            case_asserts_painted(row.case, &h, needle, false);
        }
    }
}

#[test]
fn worktrees_header_keeps_add_action_in_the_right_aligned_action_slot() {
    let parent = scratch("wt-sub-tabs-header");
    let repo = temp_repo(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Worktrees;
    let mut h = harness(state);
    settle(&mut h);

    assert_painted(&h, "WORKTREES");
    let add = h.get_by_label("Add worktree");
    assert!(
        add.rect().center().x > 600.0,
        "shared header action must sit in the right slot: {:?}",
        add.rect()
    );
}

#[test]
fn worktrees_badge_counts_without_any_dirty_probe() {
    let parent = scratch("wt-sub-tabs-list-first");
    let repo = temp_repo(&parent, "alpha");
    let wt = add_worktree(&repo, &parent, "wt-feature", "feature");
    std::fs::write(wt.join("base.txt"), "changed\n").unwrap();
    let state = AppState::for_roots(&parent, &[repo]);
    let mut h = harness(state);

    step_until(&mut h, |h| painted_contains(h, "Worktrees 1"));
    let id = h.state_mut().selected_root.clone().unwrap();
    assert!(
        h.state_mut()
            .caches
            .worktrees(&id)
            .unwrap()
            .iter()
            .all(|w| w.dirty.is_none()),
        "the badge / eager-fill path never runs a dirty probe"
    );
}

#[test]
fn worktrees_tab_add_action_creates_a_worktree_end_to_end() {
    let parent = scratch("wt-sub-tabs-add");
    let repo = temp_repo(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, std::slice::from_ref(&repo));
    state.ui.tab = Tab::Worktrees;
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Add worktree").click();
    settle(&mut h);
    h.get_by_label("Worktree path input").focus();
    h.get_by_label("Worktree path input")
        .type_text("wt-created");
    h.get_by_label("Worktree branch input").focus();
    h.get_by_label("Worktree branch input")
        .type_text("created-branch");
    settle(&mut h);
    assert_painted(&h, "wt-created");
    assert_painted(&h, "created-branch");
    let created = repo.join("wt-created");
    h.get_by_label("Create").click();

    step_until(&mut h, |h| {
        created.exists() && painted_contains(h, "wt-created")
    });
}

#[test]
fn worktrees_tab_remove_action_removes_a_worktree_end_to_end() {
    let parent = scratch("wt-sub-tabs-remove");
    let repo = temp_repo(&parent, "alpha");
    let wt = add_worktree(&repo, &parent, "wt-feature", "feature");
    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Worktrees;
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "Remove wt-feature"));

    h.get_by_label("Remove wt-feature").click();
    settle(&mut h);
    assert_painted(&h, "Remove worktree");
    h.get_by_label("OK").click();

    step_until(&mut h, |h| {
        !wt.exists() && !painted_contains(h, "wt-feature")
    });
}

fn super_with_submodule(parent: &Path, name: &str) -> PathBuf {
    let child_src = parent.join(format!("{name}-child-src"));
    std::fs::create_dir_all(&child_src).unwrap();
    git(&child_src, &["init", "-q", "-b", "main"]);
    git(&child_src, &["config", "user.email", "test@example.com"]);
    git(&child_src, &["config", "user.name", "Test"]);
    std::fs::write(child_src.join("c.txt"), "one\n").unwrap();
    git(&child_src, &["add", "."]);
    git(&child_src, &["commit", "-q", "-m", "c1"]);

    let repo = parent.join(name);
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(
        &repo,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            &format!("../{name}-child-src"),
            "child",
        ],
    );
    git(&repo, &["commit", "-q", "-m", "add child"]);
    repo.canonicalize().unwrap()
}

#[test]
fn submodules_tab_lists_submodules_with_status_and_actions() {
    let parent = scratch("wt-sub-tabs-sub-list");
    let repo = super_with_submodule(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, std::slice::from_ref(&repo));
    state.ui.tab = Tab::Submodules;
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "child"));

    assert_painted(&h, "child");
    assert_painted(&h, "Up to date");
    assert_painted(&h, "Update child");
    assert_painted(&h, "Deinit child");
}

#[test]
fn submodules_tab_renders_unicode_commit_references_without_splitting_them() {
    let parent = scratch("wt-sub-tabs-unicode-ref");
    let repo = temp_repo(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, std::slice::from_ref(&repo));
    state.ui.tab = Tab::Submodules;
    let root = state.selected_root.clone().expect("selected root");
    state.caches.store_submodules(
        root.clone(),
        vec![Submodule {
            path: "child".into(),
            head: Some("界界界界界界界界".to_string()),
            recorded: None,
            state: SubmoduleState::UpToDate,
            root,
        }],
    );
    let mut h = harness(state);
    settle(&mut h);

    assert_painted(&h, "界界界界界界界");
    assert!(!painted_contains(&h, "界界界界界界界界"));
}

#[test]
fn submodules_tab_shows_needs_update_with_pinned_vs_recorded() {
    let parent = scratch("wt-sub-tabs-sub-ahead");
    let repo = super_with_submodule(&parent, "alpha");
    let sub_wc = repo.join("child");
    std::fs::write(sub_wc.join("c.txt"), "two\n").unwrap();
    git(&sub_wc, &["add", "."]);
    git(&sub_wc, &["commit", "-q", "-m", "c2"]);

    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Submodules;
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "Needs update"));

    assert_painted(&h, "Needs update");
    let out_x = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "CHECKED OUT")
        .expect("CHECKED OUT is labelled")
        .pos
        .x;
    let recorded_x = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "RECORDED")
        .expect("RECORDED is labelled")
        .pos
        .x;
    let in_columns: Vec<PaintedGalley> = painted_galleys(&h)
        .into_iter()
        .filter(|g| {
            (out_x..recorded_x - 8.0).contains(&g.pos.x)
                || (recorded_x..recorded_x + 100.0).contains(&g.pos.x)
        })
        .filter(|g| g.family == egui::FontFamily::Monospace)
        .collect();
    assert!(
        in_columns.len() >= 2,
        "a diverged submodule paints both of its commits, one in each column; \
         the monospaced ref cells found: {:?}",
        in_columns.iter().map(|g| &g.text).collect::<Vec<_>>()
    );
}

#[test]
fn submodules_tab_update_action_checks_the_recorded_commit_out() {
    let parent = scratch("wt-sub-tabs-sub-update");
    let repo = super_with_submodule(&parent, "alpha");
    let sub_wc = repo.join("child");
    std::fs::write(sub_wc.join("c.txt"), "two\n").unwrap();
    git(&sub_wc, &["add", "."]);
    git(&sub_wc, &["commit", "-q", "-m", "c2"]);

    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Submodules;
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "Update child"));

    h.get_by_label("Update child").click();
    step_until(&mut h, |h| painted_contains(h, "Up to date"));
}

#[test]
fn submodules_tab_deinit_action_uninitializes_the_submodule() {
    let parent = scratch("wt-sub-tabs-sub-deinit");
    let repo = super_with_submodule(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Submodules;
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "Deinit child"));

    h.get_by_label("Deinit child").click();
    settle(&mut h);
    assert_painted(&h, "De-init submodule");
    h.get_by_label("OK").click();
    step_until(&mut h, |h| painted_contains(h, "Uninitialized"));
}

#[test]
fn submodules_header_uses_shared_title_band_with_an_empty_action_slot() {
    let parent = scratch("sub-header");
    let repo = temp_repo(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Submodules;
    let mut h = harness(state);
    settle(&mut h);

    assert_painted(&h, "SUBMODULES");
    assert!(
        h.query_all_by_label("Add submodule").next().is_none(),
        "the empty header action slot must not invent a control"
    );
}

#[test]
fn tab_badges_show_live_worktree_and_submodule_counts() {
    let parent = scratch("wt-sub-tabs-badges");
    let repo = temp_repo(&parent, "alpha");
    add_worktree(&repo, &parent, "wt-a", "feature-a");
    add_worktree(&repo, &parent, "wt-b", "feature-b");
    let state = AppState::for_roots(&parent, &[repo]);
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "Worktrees 2"));
}

fn diverged_and_matching(tag: &str) -> Harness<'static, AppState> {
    submodules_pane_harness(tag, |root| {
        vec![
            submodule(
                root,
                "vendor/libs/alpha",
                Some("a1b2c3d"),
                Some("e4f5a6b"),
                SubmoduleState::NeedsUpdate,
            ),
            submodule(
                root,
                "vendor/libs/beta",
                Some("1122334"),
                Some("1122334"),
                SubmoduleState::UpToDate,
            ),
        ]
    })
}

fn header_x(h: &Harness<'_, AppState>, label: &str) -> f32 {
    painted_galleys(h)
        .into_iter()
        .find(|g| g.text == label)
        .unwrap_or_else(|| panic!("`{label}` is labelled exactly once by the shared row"))
        .pos
        .x
}

fn row_galleys(
    h: &Harness<'_, AppState>,
    row_centre: f32,
    x_range: std::ops::Range<f32>,
    tolerance: f32,
) -> Vec<PaintedGalley> {
    painted_galleys(h)
        .into_iter()
        .filter(|g| {
            x_range.contains(&g.pos.x)
                && (g.pos.y + g.rect.height() / 2.0 - row_centre).abs() <= tolerance
        })
        .collect()
}

fn status_dots(h: &Harness<'_, AppState>) -> Vec<(egui::Pos2, egui::Color32)> {
    let mut dots: Vec<(egui::Pos2, egui::Color32)> = filled_circles(h)
        .into_iter()
        .filter(|(_, radius, _)| (*radius - turbogit_ui::ui::components::STATE_DOT_R).abs() < 0.01)
        .map(|(centre, _, colour)| (centre, colour))
        .collect();
    dots.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));
    dots
}

fn column_labels(h: &Harness<'_, AppState>) -> [(&'static str, f32); 5] {
    [
        ("PATH", header_x(h, "PATH")),
        ("CHECKED OUT", header_x(h, "CHECKED OUT")),
        ("RECORDED", header_x(h, "RECORDED")),
        ("STATUS", header_x(h, "STATUS")),
        ("ACTIONS", header_x(h, "ACTIONS")),
    ]
}

#[test]
fn the_submodules_columns_are_labelled_once_and_every_row_starts_where_the_header_does() {
    let h = diverged_and_matching("sub18-columns");
    let [
        (_, path_x),
        (_, out_x),
        (_, recorded_x),
        (_, status_x),
        (_, _),
    ] = column_labels(&h);

    for label in ["PATH", "CHECKED OUT", "RECORDED", "STATUS", "ACTIONS"] {
        let n = painted_galleys(&h)
            .iter()
            .filter(|g| g.text == label)
            .count();
        assert_eq!(
            n, 1,
            "`{label}` is labelled exactly once, by the shared row"
        );
    }

    let anchored = [
        ("PATH", path_x),
        ("CHECKED OUT", out_x),
        ("RECORDED", recorded_x),
        ("STATUS", status_x),
    ];
    for pair in anchored.windows(2) {
        assert!(
            (pair[0].1 - pair[1].1).abs() > 1.0,
            "{:?} and {:?} are declared at the same x ({}): two columns sharing \
             one origin are two columns wearing one name",
            pair[0],
            pair[1],
            pair[0].1
        );
    }

    let dots = status_dots(&h);
    assert_eq!(dots.len(), 2, "one status mark per row, in row order");

    let row = dots[0].0.y;
    let path_cell = row_galleys(&h, row, path_x - 4.0..out_x - 8.0, 6.0)
        .into_iter()
        .find(|g| g.family == egui::FontFamily::Monospace)
        .unwrap_or_else(|| panic!("the row paints its path"));
    let out_cell = row_galleys(&h, row, out_x - 4.0..recorded_x - 8.0, 6.0)
        .into_iter()
        .find(|g| g.family == egui::FontFamily::Monospace)
        .unwrap_or_else(|| panic!("the diverged row paints its checked-out commit"));
    let recorded_cell = row_galleys(&h, row, recorded_x - 4.0..recorded_x + 100.0, 6.0)
        .into_iter()
        .find(|g| g.family == egui::FontFamily::Monospace)
        .unwrap_or_else(|| panic!("the diverged row paints its recorded commit"));

    let dot_x = dots[0].0.x - turbogit_ui::ui::components::STATE_DOT_R;
    for (label, header_at, cell_at) in [
        ("PATH", path_x, path_cell.rect.left()),
        ("CHECKED OUT", out_x, out_cell.rect.left()),
        ("RECORDED", recorded_x, recorded_cell.rect.left()),
        ("STATUS", status_x, dot_x),
    ] {
        assert!(
            (header_at - cell_at).abs() < 0.01,
            "the {label} label is not over its cell: header at {header_at}, row at \
             {cell_at}. The header and the rows must read the same column table, so \
             a column cannot be narrow in the header and wide in the rows — and \
             nothing notices until the frame is on screen."
        );
    }

    let actions_x = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "ACTIONS")
        .expect("ACTIONS is labelled")
        .rect
        .right();
    let deinit = h.get_by_label("Deinit alpha");
    assert!(
        (actions_x - deinit.rect().right()).abs() < 0.01,
        "a trailing column's label ends where its cell ends: label at {actions_x}, \
         deinit at {:?}",
        deinit.rect()
    );

    let row = dots[1].0.y;
    let path_cell = row_galleys(&h, row, path_x - 4.0..out_x - 8.0, 6.0)
        .into_iter()
        .find(|g| g.family == egui::FontFamily::Monospace)
        .unwrap_or_else(|| panic!("the second row paints its path"));
    let out_cell = row_galleys(&h, row, out_x - 4.0..recorded_x - 8.0, 6.0)
        .into_iter()
        .find(|g| g.family == egui::FontFamily::Monospace)
        .unwrap_or_else(|| panic!("the second row paints its checked-out commit"));
    for (label, header_at, cell_at) in [
        ("PATH", path_x, path_cell.rect.left()),
        ("CHECKED OUT", out_x, out_cell.rect.left()),
    ] {
        assert!(
            (header_at - cell_at).abs() < 0.01,
            "row 2's {label} cell is not under its header: header at {header_at}, \
             row at {cell_at}"
        );
    }
    assert!(
        (status_x - (dots[1].0.x - turbogit_ui::ui::components::STATE_DOT_R)).abs() < 0.01,
        "row 2's status cell is not under its header: header at {status_x}, row at {}",
        dots[1].0.x - turbogit_ui::ui::components::STATE_DOT_R
    );
}

#[test]
fn the_two_commit_columns_are_two_columns_and_only_the_side_that_moved_is_coloured() {
    use turbogit_ui::theme::RepoState;
    let h = diverged_and_matching("sub18-two-commit-columns");
    let [(_, _), (_, out_x), (_, recorded_x), ..] = column_labels(&h);
    let dots = status_dots(&h);

    assert!(
        (out_x - recorded_x).abs() > 1.0,
        "CHECKED OUT and RECORDED are two columns at {out_x} and {recorded_x}; a \
         single cell holding one sentence is what this ticket replaced"
    );

    let row = dots[0].0.y;
    let out_cell = row_galleys(&h, row, out_x - 4.0..recorded_x - 8.0, 6.0)
        .into_iter()
        .find(|g| g.family == egui::FontFamily::Monospace)
        .expect("the diverged row paints its checked-out commit");
    let recorded_cell = row_galleys(&h, row, recorded_x - 4.0..recorded_x + 100.0, 6.0)
        .into_iter()
        .find(|g| g.family == egui::FontFamily::Monospace)
        .expect("the diverged row paints its recorded commit");
    assert_eq!(
        out_cell.text, "a1b2c3d",
        "the CHECKED OUT column carries the checkout, not the record"
    );
    assert_eq!(
        recorded_cell.text, "e4f5a6b",
        "the RECORDED column carries the record, not the checkout"
    );
    assert!(
        !out_cell.text.contains("recorded") && !out_cell.text.contains("pinned"),
        "the sentence that used to hold both values is gone, words and all: {:?}",
        out_cell.text
    );

    assert_eq!(
        out_cell.color,
        RepoState::Diverged.color(),
        "the checkout is the side that moved off the record, so it is the side \
         that gets the divergence colour — and that colour is the one repository- \
         state map's answer, not a literal written into the view"
    );
    assert_eq!(
        recorded_cell.color,
        Palette::INK_2,
        "the recorded side did not move: colouring it too would say nothing, and \
         the divergence would stop being a direction"
    );
    assert_ne!(
        out_cell.color, recorded_cell.color,
        "a divergence the reader cannot see is a column split for its own sake"
    );

    let row = dots[1].0.y;
    let out_cell = row_galleys(&h, row, out_x - 4.0..recorded_x - 8.0, 6.0)
        .into_iter()
        .find(|g| g.family == egui::FontFamily::Monospace)
        .expect("the matching row paints its one ref");
    assert_eq!(
        out_cell.text, "1122334",
        "when the two commits agree, the short ref lives in CHECKED OUT"
    );
    let in_recorded: Vec<String> = row_galleys(&h, row, recorded_x - 4.0..recorded_x + 100.0, 6.0)
        .into_iter()
        .map(|g| g.text)
        .collect();
    assert!(
        in_recorded.is_empty(),
        "when the two commits agree the RECORDED cell paints nothing at all: \
         printing the same ref twice reads as two facts about two different \
         things when there is one. Found: {in_recorded:?}"
    );
    assert_eq!(
        out_cell.color,
        Palette::INK_2,
        "an agreed commit is not a divergence, so it wears no state colour"
    );
}

/// The flag is asserted by driving the real action and reading the refetched cache:
/// `git submodule update` without `--init` has no working copy and would leave the row
/// uninitialised with no head.
#[test]
fn an_uninitialised_submodule_offers_init_and_still_runs_an_init() {
    use turbogit_ui::theme::RepoState;
    let parent = scratch("sub18-init");
    let repo = super_with_submodule(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, std::slice::from_ref(&repo));
    state.ui.tab = Tab::Submodules;
    let root = state.selected_root.clone().expect("selected root");
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "Deinit child"));

    assert_painted(&h, "Update child");
    assert!(
        h.query_all_by_label("Init child").next().is_none(),
        "an initialised submodule's action is an update, not an init"
    );

    h.get_by_label("Deinit child").click();
    settle(&mut h);
    h.get_by_label("OK").click();
    step_until(&mut h, |h| painted_contains(h, "Uninitialized"));

    assert_painted(&h, "Init child");
    assert!(
        h.query_all_by_label("Init child").count() == 1,
        "the accessibility label says `Init` too: a screen-reader user is owed \
         the same word a sighted user reads"
    );
    assert!(
        !painted_contains(&h, "Update child"),
        "the update verb is gone from an uninitialised submodule's row — the \
         operation was always an init and the label now says so"
    );
    assert_painted(&h, "Deinit child");

    h.get_by_label("Init child").click();
    assert!(
        step_briefly(&mut h, |h| {
            h.state()
                .caches
                .submodules(&root)
                .is_some_and(|subs| !subs.is_empty() && subs[0].head.is_some())
        }),
        "the Init action must initialise the submodule. `git submodule update` \
         without `--init` has no working copy to update, so the refetched cache \
         still reports no head at all and the row never leaves the `Init` verb."
    );
    let subs = h
        .state()
        .caches
        .submodules(&root)
        .expect("refetched submodules");
    assert_eq!(
        subs[0].state,
        SubmoduleState::UpToDate,
        "the Init action initialised the submodule: an operation run with \
         `init: false` has no working copy to update and leaves it uninitialised"
    );
    assert_eq!(
        subs[0].head, subs[0].recorded,
        "an init checks the recorded commit out, so the checkout and the record \
         agree afterwards"
    );
    assert!(
        step_briefly(&mut h, |h| h.query_all_by_label("Update child").count()
            == 1),
        "once the submodule is initialised again, its action is an update again"
    );

    assert_ne!(
        RepoState::Clean.color(),
        RepoState::Uninitialized.color(),
        "a clean submodule and an uninitialised one must not look alike"
    );
}

#[test]
fn the_status_word_is_a_leading_dot_with_no_background_and_the_one_maps_colour() {
    use turbogit_ui::theme::RepoState;
    let h = submodules_pane_harness("sub18-status-mark", |root| {
        vec![
            submodule(
                root,
                "libs/clean",
                Some("aaaaaaa"),
                Some("aaaaaaa"),
                SubmoduleState::UpToDate,
            ),
            submodule(
                root,
                "libs/diverged",
                Some("bbbbbbb"),
                Some("ccccccc"),
                SubmoduleState::NeedsUpdate,
            ),
            submodule(
                root,
                "libs/absent",
                None,
                Some("ddddddd"),
                SubmoduleState::Uninitialized,
            ),
            submodule(
                root,
                "libs/conflicted",
                Some("eeeeeee"),
                Some("eeeeeee"),
                SubmoduleState::Conflicted,
            ),
        ]
    });

    let status_x = header_x(&h, "STATUS");
    let dots = status_dots(&h);
    assert_eq!(
        dots.len(),
        4,
        "every submodule row leads its status with the shared mark: {:?}",
        dots
    );

    let expected = [
        ("Up to date", RepoState::Clean.color()),
        ("Needs update", RepoState::Diverged.color()),
        ("Uninitialized", RepoState::Uninitialized.color()),
        ("Conflicts", RepoState::Conflict.color()),
    ];
    for (i, (word, colour)) in expected.iter().enumerate() {
        let (centre, painted_colour) = dots[i];
        assert_eq!(
            painted_colour, *colour,
            "row {i}'s state dot is not the one repository-state map's answer for \
             `{word}`: a literal written into the view is the second opinion this \
             ratchet exists to catch"
        );
        let cell = row_galleys(&h, centre.y, status_x..status_x + 200.0, 6.0);
        let painted_word = cell.iter().find(|g| g.text == *word).unwrap_or_else(|| {
            panic!(
                "`{word}` paints in its own status cell; the cell painted {:?}",
                cell.iter().map(|g| &g.text).collect::<Vec<_>>()
            )
        });
        assert_eq!(
            painted_word.color, *colour,
            "the status word and its leading dot are one mark in one colour"
        );
        assert!(
            painted_word.rect.left() > centre.x && painted_word.rect.left() - centre.x < 24.0,
            "the dot leads the word it belongs to: dot at {centre:?}, word at {:?}",
            painted_word.rect
        );

        let covering: Vec<(egui::Rect, egui::Color32)> = filled_rects(&h)
            .into_iter()
            .filter(|(rect, _)| rect.width() < 600.0 && rect.contains(painted_word.pos))
            .collect();
        assert!(
            covering.is_empty(),
            "`{word}` paints **no background at all**: no chip fill, no chip \
             radius, no chip geometry. Found fills covering the word at {painted_word:?}: \
             {covering:?}"
        );
        let band = egui::Rect::from_min_max(
            egui::pos2(status_x - 4.0, painted_word.rect.top() - 8.0),
            egui::pos2(
                painted_word.rect.right() + 4.0,
                painted_word.rect.bottom() + 8.0,
            ),
        );
        let plates: Vec<(egui::Rect, egui::Color32)> = filled_rects(&h)
            .into_iter()
            .filter(|(rect, _)| rect.width() < 600.0 && rect.intersects(band))
            .collect();
        assert!(
            plates.is_empty(),
            "the only mark beside `{word}` is the dot: a chip would put a filled \
             rect in this band. Found: {plates:?}"
        );
    }

    assert_ne!(
        expected[0].1, expected[1].1,
        "a clean submodule and a diverged one must never share a colour"
    );
    assert_ne!(
        dots[0].1, dots[1].1,
        "…and the painted dots prove it on screen, not just in the mapping"
    );

    let uninitialised = dots[2].1;
    assert_eq!(
        uninitialised,
        Palette::INK_3,
        "an uninitialised submodule wears the muted ink: it is work the user is \
         owed, not a severity, and the muted step is the one ink that says so"
    );
    assert_ne!(
        uninitialised,
        Palette::COUNTER,
        "an uninitialised submodule must never wear the reserved counter orange: \
         that tone is documented as dirt and unpushed counts, and borrowing it \
         puts a missing working copy in the same colour as `N commits incoming`"
    );
    assert_ne!(
        uninitialised,
        Palette::AHEAD,
        "an uninitialised submodule must never wear clean's ahead green — the \
         working copy is absent, which is the opposite of in sync"
    );
}

#[test]
fn a_submodule_deinitialised_through_the_real_action_paints_the_muted_ink_on_a_legal_surface() {
    use turbogit_ui::theme::RepoState;
    let parent = scratch("sub19-uninit-ink");
    let repo = super_with_submodule(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, std::slice::from_ref(&repo));
    state.ui.tab = Tab::Submodules;
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "Deinit child"));

    h.get_by_label("Deinit child").click();
    settle(&mut h);
    h.get_by_label("OK").click();
    step_until(&mut h, |h| painted_contains(h, "Uninitialized"));

    let root = h.state().selected_root.clone().expect("selected root");
    let subs = h
        .state()
        .caches
        .submodules(&root)
        .expect("refetched submodules");
    assert_eq!(
        subs[0].state,
        SubmoduleState::Uninitialized,
        "the row under test is one git itself reported as uninitialised"
    );
    assert!(
        subs[0].head.is_none(),
        "…with no working copy at all, which is the fact the colour is about"
    );

    let word = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "Uninitialized")
        .expect("the uninitialised row paints its status word");
    let (centre, painted) = status_dots(&h)
        .into_iter()
        .find(|(c, _)| (c.y - word.rect.center().y).abs() < 8.0)
        .unwrap_or_else(|| {
            panic!(
                "the `Uninitialized` word is led by the shared mark; the dots \
                 painted were {:?}",
                status_dots(&h)
            )
        });
    assert_eq!(
        painted,
        Palette::INK_3,
        "an uninitialised submodule's dot at {centre:?} must be the muted ink, \
         the one state that is work owed rather than a severity"
    );
    assert_eq!(
        painted,
        RepoState::Uninitialized.color(),
        "…and it is the one map's answer for that arm, read back off the screen"
    );
    for (token, tone, why) in [
        (
            "COUNTER",
            Palette::COUNTER,
            "the reserved counter orange means dirt and unpushed counts, so a \
             missing working copy in it reads as `N commits incoming`",
        ),
        (
            "AHEAD",
            Palette::AHEAD,
            "clean's ahead green, and an absent checkout is not in sync",
        ),
        (
            "STATUS_DIVERGED",
            Palette::STATUS_DIVERGED,
            "the error red, which is a conflict or a divergence and this is \
             neither",
        ),
    ] {
        assert_ne!(
            painted, tone,
            "an uninitialised submodule must never wear {token}: {why}"
        );
    }

    let ctx = egui::Context::default();
    turbogit_ui::theme::configure_style(&ctx);
    let panel_fill = ctx.style_of(egui::Theme::Dark).visuals.panel_fill;
    assert_eq!(
        panel_fill,
        Palette::BG,
        "the central panel's fill is the app background, which is one of the four \
         surfaces `INK_3` is legal on"
    );
    let ratio = contrast(painted, panel_fill);
    println!("INK_3 on the submodules pane's own fill: {ratio:.3}:1");
    assert!(
        ratio >= 4.5,
        "INK_3 must clear AA on the surface the dot lands on, got {ratio:.3}:1 on \
         {panel_fill:?}"
    );

    for (token, fill) in [
        ("SURFACE", Palette::SURFACE),
        ("SURFACE_2", Palette::SURFACE_2),
        ("SURFACE_3", Palette::SURFACE_3),
        ("SELECTION", Palette::SELECTION),
        ("ROW_SELECTED", Palette::ROW_SELECTED),
    ] {
        let covering: Vec<egui::Rect> = filled_rects(&h)
            .into_iter()
            .filter(|(rect, painted_fill)| *painted_fill == fill && rect.contains(centre))
            .map(|(rect, _)| rect)
            .collect();
        assert!(
            covering.is_empty(),
            "INK_3 is not legal on a raised or selected fill, and a {token} band \
             is painted under the uninitialised dot at {centre:?}: {covering:?}"
        );
    }
}

#[test]
fn no_submodule_row_paints_a_selection_fill_and_every_row_starts_at_one_origin() {
    let h = diverged_and_matching("sub18-no-selection");
    let [(_, path_x), ..] = column_labels(&h);

    for (token, fill) in [
        ("ROW_SELECTED", Palette::ROW_SELECTED),
        ("SELECTION", Palette::SELECTION),
    ] {
        let bands: Vec<egui::Rect> = filled_rects(&h)
            .into_iter()
            .filter(|(_, painted)| *painted == fill)
            .map(|(rect, _)| rect)
            .collect();
        assert!(
            bands.is_empty(),
            "a submodule row paints no solid {token} fill — a chosen row in a \
             list takes ROW_SELECTED, and this pane has no chosen row at all. \
             Found {token} bands: {bands:?}"
        );
    }

    let dots = status_dots(&h);
    assert_eq!(dots.len(), 2, "two rows, two origins to compare");
    for (i, (centre, _)) in dots.iter().enumerate() {
        let path = row_galleys(&h, centre.y, path_x - 4.0..path_x + 200.0, 6.0)
            .into_iter()
            .find(|g| g.family == egui::FontFamily::Monospace)
            .unwrap_or_else(|| panic!("row {i} paints its path"));
        assert!(
            (path.rect.left() - path_x).abs() < 0.01,
            "row {i}'s first text origin is not the column's: header at {path_x}, \
             row at {}",
            path.rect.left()
        );
    }
}

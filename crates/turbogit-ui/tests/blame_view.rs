use std::path::{Path, PathBuf};

use egui::{Id, Pos2, Rect, Shape};
use egui_kittest::{Harness, kittest::Queryable};
use tempfile::TempDir;
use test_support::git_seed::{commit, git};
use test_support::harness::{
    assert_not_painted, assert_painted, click_menu_item, filled_rects, painted_text,
    right_click_row, settle_reads,
};
use test_support::srcscan;
use turbogit_app::events::{AppEvent, LogBatchMode};
use turbogit_app::keyed_read::Keyed;
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::error::TgError;
use turbogit_domain::model::{LogOpts, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_ui::theme::{configure_style, install_fonts};

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
    c3: String,
    c2: String,
    c1: String,
}

fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    commit(dir, name, body, msg);
    git(dir, &["rev-parse", "HEAD"]).trim().to_owned()
}

fn seeded_project() -> Seed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    std::fs::create_dir_all(&alpha).expect("alpha dir");
    git(&alpha, &["init", "-b", "main"]);
    git(&alpha, &["config", "user.email", "test@example.com"]);
    git(&alpha, &["config", "user.name", "Test"]);
    let c1 = commit_file(
        &alpha,
        "file.txt",
        "alpha: initial commit\n",
        "alpha: initial commit",
    );
    let c2 = commit_file(
        &alpha,
        "file.txt",
        "alpha: initial commit\nalpha: second commit\n",
        "alpha: second commit",
    );
    let c3 = commit_file(
        &alpha,
        "README.md",
        "alpha: docs commit",
        "alpha: docs commit",
    );
    Seed {
        _tmp: tmp,
        project,
        c3,
        c2,
        c1,
    }
}

fn log_harness(seed: &Seed) -> Harness<'static, AppState> {
    let mut state = AppState::new(seed.project.clone());
    assert_eq!(state.multi.roots.len(), 1, "one root discovered");
    let engine = CliExecutor {
        settings: VcsSettings::default(),
    };
    for root in state.multi.roots.clone() {
        let commits = engine.log(&root.path, &LogOpts::default()).expect("log");
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
    state.ui.tab = Tab::Log;

    let mut fonts_installed = false;
    let mut harness = Harness::new_ui_state(
        move |ui, state| {
            configure_style(ui.ctx());
            if !fonts_installed {
                install_fonts(ui.ctx());
                fonts_installed = true;
            }
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(
        1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH,
        800.0,
    ));
    settle_reads(&mut harness);
    harness
}

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

fn blame_window(harness: &Harness<'_, AppState>) -> Rect {
    // A title-derived egui window id wraps the title in `Some` (egui 0.36's
    // `Atoms::text()` is an `Option<&str>`), so the id is NOT `Id::new("Blame")`.
    harness
        .ctx
        .memory(|m| m.area_rect(Id::new(Some("Blame"))))
        .expect("the blame window is open")
}

/// The shapes the blame window's own layer painted. The window floats above
/// the log, so its rect also covers the table's galleys painted underneath
/// it, and a rect-containment test alone cannot tell the two apart, every
/// layer clips its shapes to itself, so a clip rect inside the window names
/// a shape the window painted, whatever it overlaps.
fn window_shapes<'h>(
    harness: &'h Harness<'_, AppState>,
    window: Rect,
) -> Vec<&'h egui::epaint::ClippedShape> {
    harness
        .output()
        .shapes
        .iter()
        .filter(|clipped| window.contains_rect(clipped.clip_rect))
        .collect()
}

fn window_galleys(
    harness: &Harness<'_, AppState>,
    window: Rect,
) -> Vec<test_support::harness::PaintedGalley> {
    window_shapes(harness, window)
        .into_iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(shape) => Some(test_support::harness::PaintedGalley {
                text: shape.galley.text().to_owned(),
                pos: shape.pos,
                rect: Rect::from_min_size(shape.pos, shape.galley.size()),
                color: shape
                    .override_text_color
                    .or_else(|| shape.galley.job.sections.first().map(|s| s.format.color))
                    .unwrap_or(egui::Color32::TRANSPARENT),
                family: shape
                    .galley
                    .job
                    .sections
                    .first()
                    .map(|s| s.format.font_id.family.clone())
                    .unwrap_or(egui::FontFamily::Proportional),
            }),
            _ => None,
        })
        .collect()
}

fn window_fills(harness: &Harness<'_, AppState>, window: Rect, color: egui::Color32) -> Vec<Rect> {
    window_shapes(harness, window)
        .into_iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rect) if rect.fill == color => Some(rect.rect),
            _ => None,
        })
        .collect()
}

fn blame_row_in_window(harness: &Harness<'_, AppState>, label: &str) -> Rect {
    let window = blame_window(harness);
    harness
        .get_all_by_label(label)
        .map(|row| row.rect())
        .find(|rect| window.contains_rect(*rect))
        .unwrap_or_else(|| panic!("`{label}` must paint inside the blame window"))
}

fn select_commit_and_file(harness: &mut Harness<'_, AppState>, seed: &Seed) {
    let row_label = format!("{} alpha: second commit", short(&seed.c2));
    harness.get_by_label(&row_label).click();
    settle_reads(harness);
    harness.get_by_label("file.txt").click();
    settle_reads(harness);
}

#[test]
fn blame_renders_its_keyed_read_waiting_message_through_the_shared_presenter() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    harness.state_mut().ui.blame = Some(turbogit_app::state::BlameTarget {
        root: harness
            .state()
            .selected_root
            .clone()
            .expect("selected root"),
        path: seed.project.join("alpha/file.txt"),
        rev: seed.c2.clone(),
    });
    // Two steps, not one: the window's first frame after opening is egui's
    // fade-in at zero opacity, which paints nothing at all, so the body (and
    // the keyed read that drives it) first runs on frame two. A third frame
    // would race the real worker's BlameReady.
    harness.step();
    harness.step();
    assert_painted(&harness, "Computing blame…");
}

#[test]
fn blame_renders_its_keyed_read_failure_message_through_the_shared_presenter() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    let target = turbogit_app::state::BlameTarget {
        root: harness
            .state()
            .selected_root
            .clone()
            .expect("selected root"),
        path: seed.project.join("alpha/file.txt"),
        rev: seed.c2.clone(),
    };
    harness.state_mut().ui.blame = Some(target.clone());
    harness
        .state_mut()
        .tx
        .send(AppEvent::BlameReady {
            key: target.key(),
            result: Err(TgError::Other("blame read failed".into())),
        })
        .expect("send blame failure");
    settle_reads(&mut harness);
    assert_painted(&harness, "blame read failed");
}

#[test]
fn blame_renders_a_page_owned_empty_message_for_an_empty_read() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    let target = turbogit_app::state::BlameTarget {
        root: harness
            .state()
            .selected_root
            .clone()
            .expect("selected root"),
        path: seed.project.join("alpha/file.txt"),
        rev: seed.c2.clone(),
    };
    harness.state_mut().ui.blame = Some(target.clone());
    harness
        .state_mut()
        .tx
        .send(AppEvent::BlameReady {
            key: target.key(),
            result: Ok(Vec::new()),
        })
        .expect("send empty blame result");
    settle_reads(&mut harness);

    assert_painted(&harness, "No blame lines for this revision.");
    assert_not_painted(&harness, "Computing blame…");
    assert_not_painted(&harness, "blame read failed");
    assert_not_painted(&harness, "(no differences)");
}

#[test]
fn blame_opens_from_the_footer_and_paints_per_line_attribution() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    select_commit_and_file(&mut harness, &seed);

    assert_painted(&harness, "Open diff");
    assert_painted(&harness, "Blame");
    assert_painted(&harness, "Full path history");

    harness.get_by_label("Blame").click();
    settle_reads(&mut harness);

    assert!(harness.state().ui.blame.is_some(), "blame target recorded");
    assert_painted(&harness, "LINE");
    assert_painted(&harness, "file.txt");
    assert_painted(&harness, "alpha: docs commit");

    assert_painted(&harness, &short(&seed.c1));
    assert_painted(&harness, &short(&seed.c2));
    assert_painted(&harness, "Test");
    assert!(
        painted_text(&harness).iter().any(|t| t.ends_with("s ago")),
        "a relative age must be painted; painted text:\n{:#?}",
        painted_text(&harness)
    );
    assert_painted(&harness, "alpha: initial commit");
    assert_painted(&harness, "alpha: second commit");

    harness.get_by_label("Close").click();
    settle_reads(&mut harness);
    assert_eq!(harness.state().ui.blame, None, "close drops the target");
    assert_not_painted(&harness, "LINE");
    assert_painted(&harness, "alpha: docs commit");
}

fn centre_card(harness: &Harness<'_, AppState>) -> Rect {
    filled_rects(harness)
        .into_iter()
        .filter(|(rect, fill)| {
            *fill == turbogit_ui::theme::Palette::CONTENT_BG && rect.height() > 100.0
        })
        .max_by_key(|(rect, _)| rect.width().round() as i64)
        .map(|(rect, _)| rect)
        .expect("the log body's centre column paints a card")
}

#[test]
fn blame_floats_above_the_still_painted_commit_table() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_commit_and_file(&mut harness, &seed);
    harness.get_by_label("Blame").click();
    settle_reads(&mut harness);

    let card = centre_card(&harness);

    let docs_row = harness
        .get_by_label(&format!("{} alpha: docs commit", short(&seed.c3)))
        .rect();
    assert!(
        card.contains_rect(docs_row),
        "the graph's docs row paints in the table's card while the blame \
         window is open: row {docs_row:?}, card {card:?}"
    );

    let window = blame_window(&harness);
    for (id, content) in [
        (&seed.c1, "alpha: initial commit"),
        (&seed.c2, "alpha: second commit"),
    ] {
        let label = blame_row_label(id, content);
        let rows: Vec<Rect> = harness
            .get_all_by_label(&label)
            .map(|row| row.rect())
            .collect();
        assert_eq!(
            rows.len(),
            2,
            "one `{content}` row in the table and one in the window: {rows:?}"
        );
        assert!(
            rows.iter().any(|r| card.contains_rect(*r)),
            "the table's copy of `{content}` stays in its card: {rows:?} vs {card:?}"
        );
        assert!(
            rows.iter()
                .any(|r| window.contains_rect(*r) && !card.contains_rect(*r)),
            "the window's copy of `{content}` paints outside the table's card: \
             {rows:?} vs window {window:?}"
        );
    }
}

#[test]
fn lines_from_the_blamed_commit_are_highlighted() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_commit_and_file(&mut harness, &seed);

    harness.get_by_label("Blame").click();
    settle_reads(&mut harness);

    let window = blame_window(&harness);
    let tint = turbogit_ui::theme::Palette::selection_bg();
    let in_tint = |harness: &Harness<'_, AppState>, text: &str| {
        let origins: Vec<Pos2> = window_galleys(harness, window)
            .into_iter()
            .filter(|g| g.text == text)
            .map(|g| g.pos)
            .collect();
        assert!(
            !origins.is_empty(),
            "`{text}` must be painted inside the blame window"
        );
        filled_rects(harness)
            .iter()
            .any(|(r, c)| *c == tint && origins.iter().any(|p| r.contains(*p)))
    };
    assert!(
        in_tint(&harness, "alpha: second commit"),
        "line 2 must sit on a selection-tinted rect"
    );
    assert!(
        !in_tint(&harness, "alpha: initial commit"),
        "line 1 (a different commit's line) must stay unhighlighted"
    );
}

#[test]
fn clicking_a_blamed_line_navigates_to_its_commit_in_the_log() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_commit_and_file(&mut harness, &seed);

    harness.get_by_label("Blame").click();
    settle_reads(&mut harness);

    let row_label = blame_row_label(&seed.c1, "alpha: initial commit");
    let window = blame_window(&harness);
    let blame_copy = harness
        .get_all_by_label(&row_label)
        .find(|row| window.contains_rect(row.rect()))
        .expect("the blame window paints the c1 line");
    blame_copy.click();
    settle_reads(&mut harness);

    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(seed.c1.as_str()),
        "the line's commit must become the log selection"
    );
    assert_eq!(harness.state().ui.blame, None, "blame window closed");
    assert_painted(&harness, "HASH");
}

#[test]
fn context_menu_opens_blame_and_the_path_scope_survives_it() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    let c2_label = format!("{} alpha: second commit", short(&seed.c2));
    harness.get_by_label(&c2_label).click();
    settle_reads(&mut harness);
    right_click_row(&mut harness, "file.txt");
    click_menu_item(&mut harness, "Show blame", "Show history for file…");
    settle_reads(&mut harness);
    assert!(harness.state().ui.log_path_scope.is_some());
    assert_not_painted(&harness, "alpha: docs commit");

    harness.get_by_label(&c2_label).click();
    settle_reads(&mut harness);
    harness.get_by_label("file.txt").click();
    settle_reads(&mut harness);
    harness.get_by_label("Blame").click();
    settle_reads(&mut harness);
    assert!(harness.state().ui.blame.is_some());
    assert_painted(&harness, "alpha: second commit");

    harness.get_by_label("Close").click();
    settle_reads(&mut harness);
    assert!(harness.state().ui.log_path_scope.is_some());
    assert_painted(&harness, "alpha: initial commit");
    assert_not_painted(&harness, "alpha: docs commit");

    harness.get_by_label(&c2_label).click();
    settle_reads(&mut harness);
    harness.get_by_label("file.txt").click_secondary();
    settle_reads(&mut harness);
    harness.get_by_label("Show blame").click();
    settle_reads(&mut harness);
    assert!(
        harness.state().ui.blame.is_some(),
        "context menu opens blame"
    );
}

#[test]
fn the_blame_surface_labels_its_columns_with_the_shared_column_chrome() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_commit_and_file(&mut harness, &seed);
    harness.get_by_label("Blame").click();
    settle_reads(&mut harness);

    let window = blame_window(&harness);
    let galleys = window_galleys(&harness, window);
    for label in BLAME_LABELS {
        let painted = galleys.iter().find(|g| g.text == label).unwrap_or_else(|| {
            panic!(
                "the blame window must label its `{label}` column; painted in the window: {:#?}",
                galleys.iter().map(|g| &g.text).collect::<Vec<_>>()
            )
        });
        assert_eq!(
            painted.color,
            turbogit_ui::theme::Palette::INK_3,
            "the `{label}` column header is a label the user reads, so it takes the \
             muted step — the same one the commit table's headers wear"
        );
        assert_ne!(
            painted.color,
            turbogit_ui::theme::Palette::INK_4,
            "9px is normal-size text; the dim step is 3.2:1 and is reserved for \
             placeholders, dim path suffixes and hatches"
        );
    }

    let top = galleys
        .iter()
        .find(|g| g.text == BLAME_LABELS[0])
        .expect("the first column is labelled")
        .pos
        .y;
    let first_row = blame_row_in_window(
        &harness,
        &blame_row_label(&seed.c1, "alpha: initial commit"),
    );
    let rules: Vec<Rect> = window_fills(
        &harness,
        window,
        turbogit_ui::theme::Palette::RULE_STRUCTURAL,
    )
    .into_iter()
    .filter(|r| (r.height() - 1.0).abs() < 0.01)
    .filter(|r| r.top() >= top && r.bottom() <= first_row.top())
    .collect();
    assert_eq!(
        rules.len(),
        1,
        "the blame window wears the column header's hairline above its rows, and \
         nothing else: {rules:?} in window {window:?}"
    );
}

const BLAME_LABELS: [&str; 4] = ["HASH", "AUTHOR", "LINE", "AGE"];

fn blame_row_label(id: &str, content: &str) -> String {
    format!("{} {content}", short(id))
}

#[test]
fn the_blame_view_adopts_the_commit_tables_row_height_and_column_offsets() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_commit_and_file(&mut harness, &seed);

    let blame_src = srcscan::code_only(include_str!("../src/ui/blame_view.rs"));
    for constant in [
        "CommitTable::ROW_HEIGHT",
        "CommitTable::HASH",
        "CommitTable::AUTHOR",
        "CommitTable::MESSAGE",
        "CommitTable::DATE_RIGHT_PAD",
    ] {
        assert!(
            blame_src.contains(constant),
            "`blame_view.rs` must read `{constant}` from the commit table rather \
             than declaring its own: a second table of offsets is a second \
             answer to questions one constant already answers"
        );
    }
    for own_geometry in [
        "const HASH_X",
        "const AUTHOR_X",
        "const AGE_X",
        "const CONTENT_X",
        "const ROW_HEIGHT",
    ] {
        assert!(
            !blame_src.contains(own_geometry),
            "`blame_view.rs` declares `{own_geometry}` again: the blame window's \
             rows are the commit table's rows. Read the geometry from \
             `log_window::CommitTable`."
        );
    }
    assert!(
        blame_src.contains("paint_log_row"),
        "…and its rows must go through the commit table's row painter, so the band \
         and the rail are the same decision in both surfaces"
    );

    harness.get_by_label("Blame").click();
    settle_reads(&mut harness);
    let rows = [
        blame_row_in_window(
            &harness,
            &blame_row_label(&seed.c1, "alpha: initial commit"),
        ),
        blame_row_in_window(&harness, &blame_row_label(&seed.c2, "alpha: second commit")),
    ];
    for (name, row) in rows.iter().enumerate() {
        assert_eq!(
            row.height(),
            turbogit_ui::theme::FILE_ROW_HEIGHT,
            "the blame row no. {name} is the commit table's row height. This is \
             asserted against the token the commit table's rows allocate, NOT \
             against a restated number: a hard-coded height is a second answer \
             to a question one constant already answers."
        );
    }

    let window = blame_window(&harness);
    let galleys = window_galleys(&harness, window);
    let in_row = |row: &Rect, pos: Pos2| row.top() - 4.0 <= pos.y && pos.y < row.bottom();
    let cell_x = |row: &Rect, text: &str| -> f32 {
        galleys
            .iter()
            .find(|g| g.text == text && in_row(row, g.pos))
            .unwrap_or_else(|| panic!("`{text}` paints inside the row {row:?}"))
            .pos
            .x
    };
    let trailing_right = |row: &Rect| -> f32 {
        galleys
            .iter()
            .filter(|g| in_row(row, g.pos))
            .map(|g| g.rect.right())
            .fold(f32::MIN, f32::max)
    };
    for (name, row, hash, content) in [
        ("first", &rows[0], short(&seed.c1), "alpha: initial commit"),
        ("second", &rows[1], short(&seed.c2), "alpha: second commit"),
    ] {
        assert!(
            cell_x(row, &hash) < cell_x(row, "Test"),
            "the {name} row's hash cell leads its author cell: {row:?}"
        );
        assert!(
            cell_x(row, "Test") < cell_x(row, content),
            "the {name} row's author cell leads its line cell: {row:?}"
        );
    }
    for (cell, first, second) in [
        (
            "hash",
            cell_x(&rows[0], &short(&seed.c1)),
            cell_x(&rows[1], &short(&seed.c2)),
        ),
        ("author", cell_x(&rows[0], "Test"), cell_x(&rows[1], "Test")),
        (
            "line",
            cell_x(&rows[0], "alpha: initial commit"),
            cell_x(&rows[1], "alpha: second commit"),
        ),
    ] {
        assert!(
            (first - second).abs() < 0.5,
            "both rows' {cell} cells line up — one table, one set of offsets: \
             {first} vs {second} in rows {rows:?}"
        );
    }
    let inset = [
        rows[0].right() - trailing_right(&rows[0]),
        rows[1].right() - trailing_right(&rows[1]),
    ];
    assert!(
        (inset[0] - inset[1]).abs() < 0.5,
        "the trailing age cell ends at the same inset from every row's right \
         edge: {:?} in rows {rows:?}",
        inset
    );
}

#[test]
fn the_blame_view_paints_the_same_fills_and_rail_as_the_commit_table() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_commit_and_file(&mut harness, &seed);

    let commit_row = harness
        .get_by_label(&format!("{} alpha: second commit", short(&seed.c2)))
        .rect();
    let commit_band = filled_rects(&harness)
        .into_iter()
        .find(|(rect, color)| {
            *color == turbogit_ui::theme::Palette::selection_bg() && *rect == commit_row
        })
        .map(|(rect, _)| rect)
        .expect("a chosen commit row paints the shared selected band");
    let commit_rail = filled_rects(&harness)
        .into_iter()
        .find(|(rect, color)| {
            *color == turbogit_ui::theme::Palette::BRAND
                && (rect.width() - turbogit_ui::theme::RAIL_WIDTH).abs() < 0.01
                && rect.top() >= commit_row.top()
                && rect.bottom() <= commit_row.bottom()
        })
        .map(|(rect, _)| rect)
        .expect("a chosen commit row paints the one accent rail");

    harness.get_by_label("Blame").click();
    settle_reads(&mut harness);

    let chosen = blame_row_in_window(&harness, &blame_row_label(&seed.c2, "alpha: second commit"));
    let plain = blame_row_in_window(
        &harness,
        &blame_row_label(&seed.c1, "alpha: initial commit"),
    );

    let bands: Vec<Rect> = filled_rects(&harness)
        .into_iter()
        .filter(|(rect, color)| {
            *color == turbogit_ui::theme::Palette::selection_bg() && *rect == chosen
        })
        .map(|(rect, _)| rect)
        .collect();
    assert_eq!(
        bands,
        vec![chosen],
        "the line the blamed commit introduced takes the commit table's own \
         selected band at its own rect: painted {bands:?} for row {chosen:?} \
         (the commit table's chosen row band was {commit_band:?})"
    );
    assert_eq!(
        chosen.height(),
        commit_row.height(),
        "…and a row is the same height"
    );

    let rails: Vec<Rect> = filled_rects(&harness)
        .into_iter()
        .filter(|(rect, color)| {
            *color == turbogit_ui::theme::Palette::BRAND
                && (rect.width() - turbogit_ui::theme::RAIL_WIDTH).abs() < 0.01
                && (chosen.contains_rect(*rect) || plain.contains_rect(*rect))
        })
        .map(|(rect, _)| rect)
        .collect();
    assert_eq!(
        rails.len(),
        1,
        "one blamed-at line carries exactly one rail, and the line from a different \
         commit carries none: {rails:?} across rows {chosen:?} and {plain:?}"
    );
    let rail = rails[0];
    assert_eq!(rail.left(), chosen.left(), "at the row's leading edge");
    assert_eq!(rail.top(), chosen.top());
    assert_eq!(rail.height(), chosen.height(), "for the row's full height");
    assert_eq!(
        (rail.left() - chosen.left(), rail.width(), rail.height()),
        (
            commit_rail.left() - commit_row.left(),
            commit_rail.width(),
            commit_rail.height()
        ),
        "the blame view's rail is the commit table's rail at its own row's \
         leading edge — the window floats elsewhere, so the offset is the \
         comparison, not the absolute x: {rail:?} in row {chosen:?} vs \
         {commit_rail:?} in row {commit_row:?}. A rail the blame view sized \
         for itself is a second answer to what a chosen row wears."
    );

    assert!(
        !plain.contains_rect(rail),
        "a line from a different commit must not carry the rail: {plain:?} vs {rail:?}"
    );
    assert!(
        !filled_rects(&harness).iter().any(|(rect, color)| *color
            == turbogit_ui::theme::Palette::selection_bg()
            && *rect == plain),
        "…nor the band"
    );

    let galleys = test_support::harness::painted_galleys(&harness);
    let hash = galleys
        .iter()
        .find(|g| g.text == short(&seed.c2) && chosen.contains(g.pos))
        .expect("the blamed line paints a commit hash");
    let author = galleys
        .iter()
        .find(|g| g.text == "Test" && chosen.contains(g.pos))
        .expect("the blamed line paints an author");
    let content = galleys
        .iter()
        .find(|g| g.text == "alpha: second commit" && chosen.contains(g.pos))
        .expect("the blamed line paints its source content");
    assert_eq!(
        hash.color,
        turbogit_ui::theme::Palette::LINK,
        "the blame view's hash cell wears the commit table's hash ink (`LINK`), not \
         the accent: an information cell is not a call to action"
    );
    assert_eq!(
        author.color,
        turbogit_ui::theme::Palette::INK_2,
        "…and its author cell the commit table's secondary step"
    );
    assert_eq!(
        content.color,
        turbogit_ui::theme::Palette::INK,
        "…and its content cell the commit table's primary step"
    );
}

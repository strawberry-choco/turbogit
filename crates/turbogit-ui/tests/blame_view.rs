//! Issue 18 — Blame view: reachable from the changed-files pane footer
//! ("Open diff · Blame · Full path history", screen 09) and from the
//! changed-file context menu; per-line commit/author/age attribution with
//! the blamed-at commit's lines highlighted; clicking a line's commit
//! navigates to it in the log.
//!
//! Headless egui_kittest harness driving [`turbogit_ui::ui::render`] over
//! the seeded single-focus project from `git_log.rs`'s fixture shape, with
//! the worker-event channel drained every frame (production parity).

use std::path::{Path, PathBuf};

use egui::{Color32, Pos2, Rect, Shape};
use egui_kittest::{Harness, kittest::Queryable};
use tempfile::TempDir;
use test_support::harness::{click_menu_item, right_click_row};
use turbogit_app::events::{AppEvent, LogBatchMode};
use turbogit_app::keyed_read::Keyed;
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::error::TgError;
use turbogit_domain::model::{LogOpts, VcsSettings};
use turbogit_engine::cli::CliExecutor;
use turbogit_engine_api::GitExecutor;
use turbogit_ui::theme::{configure_style, install_fonts};

// --- helpers (mirrors tests/git_log.rs) --------------------------------------

fn painted_text(harness: &Harness<'_, AppState>) -> Vec<String> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

#[track_caller]
fn assert_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was not painted; painted text:\n{texts:#?}"
    );
}

#[track_caller]
fn assert_not_painted(harness: &Harness<'_, AppState>, needle: &str) {
    let texts = painted_text(harness);
    assert!(
        !texts.iter().any(|t| t.contains(needle)),
        "`{needle}` was unexpectedly painted; painted text:\n{texts:#?}"
    );
}

fn filled_rects(harness: &Harness<'_, AppState>) -> Vec<(Rect, Color32)> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Rect(rect_shape) if rect_shape.fill != Color32::TRANSPARENT => {
                Some((rect_shape.rect, rect_shape.fill))
            }
            _ => None,
        })
        .collect()
}

/// Step frames until the painted output stabilizes and no keyed read is
/// pending — diff or blame, the gate does not say which (mirrors
/// tests/diff_viewer.rs's settle).
pub fn settle(harness: &mut Harness<'_, AppState>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut prev = String::new();
    while std::time::Instant::now() < deadline {
        harness.step();
        let fingerprint = format!(
            "{:?}|read={}",
            painted_text(harness),
            harness.state().read_pending(),
        );
        if fingerprint == prev && !harness.state().read_pending() {
            return;
        }
        prev = fingerprint;
    }
    panic!("log/blame layout did not settle within 15s");
}

// --- fixture -------------------------------------------------------------------

struct Seed {
    _tmp: TempDir,
    project: PathBuf,
    /// HEAD~1 — rewrote `file.txt` to one line ("alpha: second commit").
    c2: String,
    /// HEAD~2 — created `file.txt` ("alpha: initial commit").
    c1: String,
}

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawning git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn commit_file(dir: &Path, name: &str, body: &str, msg: &str) -> String {
    std::fs::write(dir.join(name), body).expect("writing work file");
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-m", msg]);
    run_git(dir, &["rev-parse", "HEAD"]).trim().to_string()
}

/// One root `alpha`: c1 creates file.txt (one line), c2 rewrites it to two
/// lines, c3 is docs-only. Blame at c2 attributes line 1 → c1, line 2 → c2.
fn seeded_project() -> Seed {
    let tmp = tempfile::tempdir().expect("tempdir");
    let project = tmp.path().join("project");
    let alpha = project.join("alpha");
    std::fs::create_dir_all(&alpha).expect("alpha dir");
    run_git(&alpha, &["init", "-b", "main"]);
    run_git(&alpha, &["config", "user.email", "test@example.com"]);
    run_git(&alpha, &["config", "user.name", "Test"]);
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
    let _c3 = commit_file(
        &alpha,
        "README.md",
        "alpha: docs commit",
        "alpha: docs commit",
    );
    Seed {
        _tmp: tmp,
        project,
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
            state.drain_events(); // production parity with app.rs
            turbogit_ui::ui::render(ui, state);
        },
        state,
    );
    harness.set_size(egui::vec2(
        1280.0 + turbogit_ui::ui::sidebar::SIDEBAR_WIDTH,
        800.0,
    ));
    settle(&mut harness);
    harness
}

fn short(id: &str) -> String {
    id[..7.min(id.len())].to_string()
}

/// Select the commit row, then its changed-file entry, landing on the
/// changed-files pane with the footer links enabled.
fn select_commit_and_file(harness: &mut Harness<'_, AppState>, seed: &Seed) {
    let row_label = format!("{} alpha: second commit", short(&seed.c2));
    harness.get_by_label(&row_label).click();
    settle(harness);
    harness.get_by_label("file.txt").click();
    settle(harness);
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
    harness.step();
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
    harness.step();

    assert_painted(&harness, "No blame lines for this revision.");
    assert_not_painted(&harness, "Computing blame…");
    assert_not_painted(&harness, "blame read failed");
    assert_not_painted(&harness, "(no differences)");
}

// --- Cycle 3: footer entry + per-line attribution ------------------------------

#[test]
fn blame_opens_from_the_footer_and_paints_per_line_attribution() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    select_commit_and_file(&mut harness, &seed);

    // Footer links (screen 09) sit under the changed-files pane, acting on
    // the selected file.
    assert_painted(&harness, "Open diff");
    assert_painted(&harness, "Blame");
    assert_painted(&harness, "Full path history");

    harness.get_by_label("Blame").click();
    settle(&mut harness);

    // The blame surface replaced the graph pane and records its target.
    assert!(harness.state().ui.blame.is_some(), "blame target recorded");
    assert_painted(&harness, "Close blame");
    assert_painted(&harness, "file.txt");

    // Per-line attribution: both introducing commits' hashes, the author,
    // a relative age, and every line's content.
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

    // Closing blame returns to the commit graph.
    harness.get_by_label("Close blame").click();
    settle(&mut harness);
    assert_eq!(harness.state().ui.blame, None, "close drops the target");
    assert_not_painted(&harness, "Close blame");
    assert_painted(&harness, "HASH");
}

// --- Cycle 4: current commit's lines highlighted -------------------------------

/// The card the commit table renders into, and the one the blame view replaces it
/// in — the **widest** tall `CONTENT_BG` rect in the log body.
///
/// Widest rather than named, because that is what makes it the middle column: the
/// branches card is a fixed 210pt and the right column a fixed 344pt, so the
/// leftover column is always the one that takes the space. This is the fixture's
/// single-root shape, so there is no ambiguity to resolve.
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

/// **Blame renders into the commit table's card — the same card, at the same
/// rect, with its rows inside it.**
///
/// The log's three columns are separated by ten points of app background and the
/// commit table is a card like the other two, so the centre column is now a
/// *region with edges*. That raises a question this file did not have to answer
/// before: the blame view replaces the commit table in that slot, so it has to
/// land in the same card rather than paint a region of its own that happens to
/// overlap. The two existing ratchets here — shared row height, shared column
/// offsets, shared band and rail — all compare blame against the commit table
/// *relative to one another*, which is exactly the kind of comparison that would
/// stay green if both views moved a card apart. This one is absolute: the card
/// is read off the paint before blame opens and again after, and the two rects
/// have to be the same one.
#[test]
fn blame_fills_the_commit_tables_card() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    let table_card = centre_card(&harness);

    select_commit_and_file(&mut harness, &seed);
    harness.get_by_label("Blame").click();
    settle(&mut harness);

    let blame_card = centre_card(&harness);
    assert_eq!(
        blame_card, table_card,
        "blame REPLACES the commit table in the same region, so it fills the same card: \
         the commit table's card was {table_card:?} and blame's is {blame_card:?}. A view \
         that painted a region of its own over the slot would be a second surface in the \
         one place the frame draws a single one."
    );
    for (id, content) in [
        (&seed.c1, "alpha: initial commit"),
        (&seed.c2, "alpha: second commit"),
    ] {
        let row = harness.get_by_label(&blame_row_label(id, content)).rect();
        assert!(
            blame_card.contains_rect(row),
            "the `{content}` blame row is inside the card it replaced the commit table \
             in: row {row:?}, card {blame_card:?}"
        );
    }
}

/// Every paint-time origin of a galley painting exactly `text` — the same
/// string can appear in several panes (blame row + details message).
fn galley_origins(harness: &Harness<'_, AppState>, text: &str) -> Vec<Pos2> {
    harness
        .output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(shape) if shape.galley.text() == text => Some(shape.pos),
            _ => None,
        })
        .collect()
}

#[test]
fn lines_from_the_blamed_commit_are_highlighted() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_commit_and_file(&mut harness, &seed);

    harness.get_by_label("Blame").click();
    settle(&mut harness);

    // Line 2 was introduced by the blamed commit (c2): it carries the
    // translucent selection tint; line 1 (c1's) does not.
    let tint = turbogit_ui::theme::Palette::selection_bg();
    let in_tint = |harness: &Harness<'_, AppState>, text: &str| {
        let origins = galley_origins(harness, text);
        assert!(!origins.is_empty(), "`{text}` must be painted");
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

// --- Cycle 5: clicking a line's commit navigates to the log --------------------

#[test]
fn clicking_a_blamed_line_navigates_to_its_commit_in_the_log() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_commit_and_file(&mut harness, &seed);

    harness.get_by_label("Blame").click();
    settle(&mut harness);

    // Click line 1 (introduced by c1) → the log selects c1 and the blame
    // view closes.
    let row_label = format!("{} alpha: initial commit", short(&seed.c1));
    harness.get_by_label(&row_label).click();
    settle(&mut harness);

    assert_eq!(
        harness.state().ui.selected_commit.as_deref(),
        Some(seed.c1.as_str()),
        "the line's commit must become the log selection"
    );
    assert_eq!(harness.state().ui.blame, None, "blame view closed");
    assert_painted(&harness, "HASH");
}

// --- Cycle 6: context-menu entry + path-scope respect --------------------------

#[test]
fn context_menu_opens_blame_and_the_path_scope_survives_it() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);

    // Scope the log to file.txt via its changed-file menu, on the shared host.
    let c2_label = format!("{} alpha: second commit", short(&seed.c2));
    harness.get_by_label(&c2_label).click();
    settle(&mut harness);
    right_click_row(&mut harness, "file.txt");
    click_menu_item(&mut harness, "Show blame", "Show history for file…");
    settle(&mut harness);
    assert!(harness.state().ui.log_path_scope.is_some());
    assert_not_painted(&harness, "alpha: docs commit");

    // Open blame on the scoped file from the footer, on a scoped commit.
    harness.get_by_label(&c2_label).click();
    settle(&mut harness);
    harness.get_by_label("file.txt").click();
    settle(&mut harness);
    harness.get_by_label("Blame").click();
    settle(&mut harness);
    assert!(harness.state().ui.blame.is_some());
    assert_painted(&harness, "alpha: second commit");

    // Back to the log: the scope is untouched — the graph still lists only
    // commits touching file.txt.
    harness.get_by_label("Close blame").click();
    settle(&mut harness);
    assert!(harness.state().ui.log_path_scope.is_some());
    assert_painted(&harness, "alpha: initial commit");
    assert_not_painted(&harness, "alpha: docs commit");

    // The context menu offers blame directly on a changed file.
    harness.get_by_label(&c2_label).click();
    settle(&mut harness);
    harness.get_by_label("file.txt").click_secondary();
    settle(&mut harness);
    harness.get_by_label("Show blame").click();
    settle(&mut harness);
    assert!(
        harness.state().ui.blame.is_some(),
        "context menu opens blame"
    );
}

// --- The blame surface's column chrome ------------------------------------------

/// **The blame surface's columns are labelled by the shared column chrome.**
///
/// The blame view *replaces* the commit table in the same slot, so its header is
/// the same header: four labels in the one tracked type in the muted ink, over
/// one structural hairline. It used to be three hand-laid galleys at the 11px
/// control size with no rule under them at all — a header no other pane could be
/// compared against, which is what "switching to blame is a visual reset" looks
/// like when only the header is counted.
#[test]
fn the_blame_surface_labels_its_columns_with_the_shared_column_chrome() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_commit_and_file(&mut harness, &seed);
    harness.get_by_label("Blame").click();
    settle(&mut harness);

    let galleys = test_support::harness::painted_galleys(&harness);
    for label in BLAME_LABELS {
        let painted = galleys.iter().find(|g| g.text == label).unwrap_or_else(|| {
            panic!(
                "the blame surface must label its `{label}` column; painted: {:#?}",
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

    // One structural hairline under them, and no second rule in the same band.
    let top = galleys
        .iter()
        .find(|g| g.text == BLAME_LABELS[0])
        .expect("the first column is labelled")
        .pos
        .y;
    let rules: Vec<Rect> = filled_rects(&harness)
        .into_iter()
        .filter(|(_, fill)| *fill == turbogit_ui::theme::Palette::RULE_STRUCTURAL)
        .map(|(rect, _)| rect)
        .filter(|r| (r.height() - 1.0).abs() < 0.01 && r.top() >= top)
        // Scoped to the blame surface's own column of the frame. The sidebar
        // wears the same shared pane header and paints the same hairline, so a
        // frame-wide count here describes how many pane headers the window
        // happens to show rather than what the blame surface is wearing.
        .filter(|r| r.left() >= turbogit_ui::ui::sidebar::SIDEBAR_WIDTH)
        .collect();
    assert_eq!(
        rules.len(),
        2,
        "the blame surface wears the shared pane header's hairline and the column \
         header's, and nothing else: {rules:?}"
    );
}

/// The blame surface's column labels, in reading order.
///
/// `HASH` and `AUTHOR` are spelled the commit table spells them, because they
/// mean the same things; the wide column holds a source line and the trailing one
/// a relative age, so those two say what they hold.
const BLAME_LABELS: [&str; 4] = ["HASH", "AUTHOR", "LINE", "AGE"];

/// The header label painting `text` in `harness`'s centre pane.
fn blame_header(harness: &Harness<'_, AppState>, text: &str) -> Pos2 {
    test_support::harness::painted_galleys(harness)
        .into_iter()
        .find(|g| g.text == text && g.pos.x >= turbogit_ui::ui::sidebar::SIDEBAR_WIDTH)
        .map(|g| g.pos)
        .unwrap_or_else(|| panic!("the blame surface labels `{text}`"))
}

/// The one row label the blame surface publishes for a blamed line.
fn blame_row_label(id: &str, content: &str) -> String {
    format!("{} {content}", short(id))
}

/// The blame view's source with every comment blanked, one space per character
/// so line structure survives — **and every line of code kept**.
///
/// The same reasoning as `git_log.rs`'s `code_only`: a scan for `CommitTable::*`
/// has to be about what the module *calls*, or this file's own documentation —
/// which names every one of those constants while explaining why — would satisfy
/// the ratchet forever.
///
/// The "kept" half matters as much as the "blanked" half. A helper that blanks
/// the code as well as the comments returns a file of newlines and spaces, and
/// then every `!scan.contains(needle)` over it is true for the uninteresting
/// reason that the needle cannot be in there at all. That is not a ratchet; it
/// is a comment about one. The mutation that caught it here replaced
/// `CommitTable::ROW_HEIGHT` with a literal `20.0` and left the file full of prose
/// about `CommitTable::ROW_HEIGHT` — the broken scan passed it.
fn code_only(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let (mut i, mut block, mut line) = (0usize, false, false);
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or('\0');
        if block {
            if c == '*' && next == '/' {
                block = false;
                out.push(' ');
                out.push(' ');
                i += 2;
                continue;
            }
        } else if line {
            if c == '\n' {
                line = false;
                out.push('\n');
                i += 1;
                continue;
            }
        } else if c == '/' && next == '/' {
            line = true;
            out.push(' ');
            out.push(' ');
            i += 2;
            continue;
        } else if c == '/' && next == '*' {
            block = true;
            out.push(' ');
            out.push(' ');
            i += 2;
            continue;
        }
        out.push(if c == '\n' || !(block || line) {
            c
        } else {
            ' '
        });
        i += 1;
    }
    out.into_iter().collect()
}

/// **The blame view's row height and cell offsets are the commit table's, read
/// from its own published constants — not this module's literals.**
///
/// Three seams, and the middle one is the one the ticket asks for.
///
/// The **height** is asserted against `theme::FILE_ROW_HEIGHT`, which *is* the
/// commit table's row-height constant (a commit row allocates exactly that), and
/// deliberately not against the number 20.0: restating the number would let a
/// hard-coded blame height pass as long as somebody updated two places, and the
/// whole claim is that there is only one place.
///
/// The **source** seam is what actually forbids the hard-coding: `blame_view.rs`
/// must name `CommitTable::ROW_HEIGHT` (and `CommitTable::HASH` / `AUTHOR` /
/// `MESSAGE` / `DATE_RIGHT_PAD`), must not define a cell offset of its own, and
/// must measure its cells from the commit table's own
/// `table_content_left`/`shows_root_gutter` — which is how the **ROOTS column**
/// lines up: on a multi-root listing this view reserves the gutter the column
/// owns rather than painting over it or inventing its own. The blame fixture
/// here is single-root, so that half is a source claim rather than a painted
/// one, and it is stated as such rather than glossed.
///
/// The **offsets** are then compared end to end: the blame surface's header
/// labels are read off the paint, the commit table's are read off the paint in
/// the same harness, and the three leading columns must be at the same x in
/// both — with the commit table's own header row as the control.
#[test]
fn the_blame_view_adopts_the_commit_tables_row_height_and_column_offsets() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_commit_and_file(&mut harness, &seed);

    // ---- the source seam: no private geometry ----------------------------
    let blame_src = code_only(include_str!("../src/ui/blame_view.rs"));
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
             than declaring its own: a second table of offsets is what made \
             switching to blame a visual reset"
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
            "`blame_view.rs` declares `{own_geometry}` again: the blame view \\
             replaces the commit table in the same slot, so its geometry is the \\
             commit table's. Read it from `log_window::CommitTable`."
        );
    }
    assert!(
        blame_src.contains("table_content_left") && blame_src.contains("shows_root_gutter"),
        "the blame view must measure its cells from the commit table's own content \\
         edge and its own ROOTS-gutter predicate, so the ROOTS column lines up with \\
         it instead of the two views disagreeing by 5px"
    );
    assert!(
        blame_src.contains("paint_log_row"),
        "…and its rows must go through the commit table's row painter, so the band \\
         and the rail are the same decision in both views"
    );

    // ---- the row height, against the shared constant ----------------------
    harness.get_by_label("Blame").click();
    settle(&mut harness);
    let first = harness
        .get_by_label(&blame_row_label(&seed.c1, "alpha: initial commit"))
        .rect();
    let second = harness
        .get_by_label(&blame_row_label(&seed.c2, "alpha: second commit"))
        .rect();
    for (name, row) in [("first", first), ("second", second)] {
        assert_eq!(
            row.height(),
            turbogit_ui::theme::FILE_ROW_HEIGHT,
            "the {name} blame row is the commit table's row height. This is asserted \\
             against the token the commit table's rows allocate, NOT against the \\
             20.0 this view used to hard-code: a hard-coded height is a second \\
             answer to a question one constant already answers."
        );
    }

    // ---- the offsets, against the commit table's own header row ----------
    let blame_hash = blame_header(&harness, "HASH");
    let blame_author = blame_header(&harness, "AUTHOR");
    let blame_line = blame_header(&harness, "LINE");
    let blame_age_label_right = test_support::harness::painted_galleys(&harness)
        .into_iter()
        .find(|g| g.text == "AGE")
        .expect("the blame surface labels its trailing column")
        .rect
        .right();
    // The blame row's own trailing cell, measured before switching away.
    let blame_row = harness
        .get_by_label(&blame_row_label(&seed.c1, "alpha: initial commit"))
        .rect();
    let blame_age = test_support::harness::painted_galleys(&harness)
        .into_iter()
        .find(|g| blame_row.contains(g.pos) && g.pos.x > blame_row.left() + 200.0)
        .expect("the blame row paints a trailing age cell");

    // The control: the commit table's own header and row, one closing-the-view
    // click away, in the same harness and the same window.
    harness.get_by_label("Close blame").click();
    settle(&mut harness);
    let table_label = |label: &str| -> test_support::harness::PaintedGalley {
        test_support::harness::painted_galleys(&harness)
            .into_iter()
            .find(|g| g.text == label && g.pos.x >= turbogit_ui::ui::sidebar::SIDEBAR_WIDTH)
            .unwrap_or_else(|| panic!("the commit table labels its `{label}` column"))
    };
    let commit_row = harness
        .get_by_label(&format!("{} alpha: second commit", short(&seed.c2)))
        .rect();
    let commit_date = test_support::harness::painted_galleys(&harness)
        .into_iter()
        .find(|g| commit_row.contains(g.pos) && g.pos.x > commit_row.left() + 200.0)
        .expect("the commit row paints a trailing date cell");

    for (label, blame, control) in [
        ("HASH", blame_hash, table_label("HASH").pos),
        ("AUTHOR", blame_author, table_label("AUTHOR").pos),
        ("LINE / MESSAGE", blame_line, table_label("MESSAGE").pos),
    ] {
        assert_eq!(
            blame.x, control.x,
            "the blame `{label}` column starts where the commit table's does: {} vs \
             {}. Blame REPLACES the commit table in the same slot, so a column that \
             moved when the view did is a visual reset at the moment of switching.",
            blame.x, control.x
        );
    }
    // The trailing column shares an EDGE on both sides, from the same inset.
    assert_eq!(
        blame_age_label_right,
        table_label("DATE").rect.right(),
        "the two trailing columns' labels end where the same inset puts them: \
         blame AGE ends at {}, the commit table's DATE at {}",
        blame_age_label_right,
        table_label("DATE").rect.right()
    );
    assert_eq!(
        (blame_row.right() - blame_age.rect.right()).abs(),
        (commit_row.right() - commit_date.rect.right()).abs(),
        "the trailing CELL ends on the same inset in both views — the commit \
         table's `DATE_RIGHT_PAD`, read from `CommitTable::DATE_RIGHT_PAD` rather \
         than restated: blame {} vs commit {}",
        blame_row.right() - blame_age.rect.right(),
        commit_row.right() - commit_date.rect.right()
    );
}

/// **The blame view's rows paint the same fills and the same rail as the commit
/// table's rows.**
///
/// Blame used to paint a bare `selection_bg()` tint and a hand-picked
/// `SURFACE_2` hover — a second answer to "what does a chosen row look like", in
/// the region the commit table had already answered. Both now go through the
/// commit table's own `paint_log_row`, so the assertion is that a blamed-at line
/// takes the selected band *and* the one 2px accent rail at its leading edge, at
/// its full height, in the accent — and that a line from a different commit takes
/// neither.
///
/// The comparison is against the **commit table's** chosen row in the same
/// harness, not against the token names, so "the same fills" is arithmetic: the
/// same band colour at the row's own rect, the same rail width, the same leading
/// edge, the same span.
#[test]
fn the_blame_view_paints_the_same_fills_and_rail_as_the_commit_table() {
    let seed = seeded_project();
    let mut harness = log_harness(&seed);
    select_commit_and_file(&mut harness, &seed);

    // The control, first: what a chosen commit row looks like in this frame.
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
    settle(&mut harness);

    // The blamed-at line is the "chosen" row of this view: c2 introduced the
    // second line of the file.
    let chosen = harness
        .get_by_label(&blame_row_label(&seed.c2, "alpha: second commit"))
        .rect();
    let plain = harness
        .get_by_label(&blame_row_label(&seed.c1, "alpha: initial commit"))
        .rect();

    // The band: the same colour, at the row's own rect.
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

    // The rail: the one painter's output — token width, at the leading edge, for
    // the row's full height, in the accent. And exactly one of them, for the one
    // line that is "chosen".
    //
    // Scoped to the blame surface's own rows rather than to the frame: the
    // branches pane and the changed-files pane keep their own chosen-row rails in
    // the same frame, and a frame-wide count would be counting the window's
    // selections rather than this view's.
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
        (rail.left(), rail.width(), rail.height()),
        (
            commit_rail.left(),
            commit_rail.width(),
            commit_rail.height()
        ),
        "the blame view's rail is the commit table's rail, to the pixel: {rail:?} \
         vs {commit_rail:?}. A rail the blame view sized for itself is a visual \
         reset at the moment of switching."
    );

    // …and the line from a different commit takes neither the band nor the rail.
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

    // The cell inks, from the commit table's own two ink functions: the blamed
    // line is a row of this table, so its text wears the table's inks.
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

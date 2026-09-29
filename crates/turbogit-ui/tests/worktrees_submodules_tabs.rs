//! Issue 14 — Worktrees & Submodules tool tabs.
//!
//! The Worktrees and Submodules center tabs become real browsers over the
//! focused root: worktree path / branch / dirty status with add & remove
//! actions; submodule path / pinned vs recorded commit / status with update
//! & deinit actions. Tab badges paint live counts (screen 01 "Worktrees 3").
//!
//! Tests drive the real [`turbogit_ui::ui::render`] through `egui_kittest`
//! over temporary git repositories (CONTEXT.md "Headless harness") and
//! assert only on public surfaces: painted labels, on-disk git effects, and
//! public `AppState` transitions.
use egui::Rect;
use egui_kittest::{Harness, kittest::Queryable};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use test_support::harness::{
    PaintedGalley, assert_painted, filled_circles, filled_rects, painted_galleys, painted_ink,
    painted_text, settle,
};
use turbogit_app::state::{AppState, Tab};
use turbogit_domain::model::{RootId, Submodule, SubmoduleState, Worktree};
use turbogit_ui::theme::Palette;

/// Run `git` in `repo`, asserting success, and return stdout.
fn git(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git invocation");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 stdout")
}

/// An initialized temp repository with one base commit on the default
/// branch, under a deterministic scratch parent.
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

/// A fresh scratch parent under the workspace's `.scratch` so path
/// basenames in assertions are stable.
fn scratch(tag: &str) -> PathBuf {
    let parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join(format!(".scratch/{tag}"));
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).unwrap();
    parent
}

/// Headless harness driving the full app UI; mirrors `workspace_shell_frame`'s.
fn harness(state: AppState) -> Harness<'static, AppState> {
    Harness::builder().with_max_steps(1024).build_ui_state(
        |ui, state| {
            state.drain_events();
            turbogit_ui::ui::render(ui, state);
        },
        state,
    )
}

/// Step frames until `pred` holds or the deadline passes — for async op /
/// refresh cycles a plain `settle` cannot wait for (and for on-disk effects
/// settle cannot observe at all).
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

/// Step frames for a bounded while and report whether `pred` came true.
///
/// The sibling of [`step_until`] for a wait whose *failure* is the thing under
/// test. `step_until` panics with "condition not met", which is the right
/// message when the thing being waited for is incidental; it is the wrong
/// message when the thing being waited for **is** the assertion — a bounded
/// wait that returns a bool lets the caller fail on the claim it was making,
/// in words about the claim, instead of timing out on the answer it expected.
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

/// A linked worktree of `repo` on an existing branch, canonicalized.
fn add_worktree(repo: &Path, parent: &Path, name: &str, branch: &str) -> PathBuf {
    git(repo, &["branch", branch]);
    let wt = parent.join(name);
    git(repo, &["worktree", "add", wt.to_str().unwrap(), branch]);
    wt.canonicalize().unwrap()
}

/// True when some painted galley contains `needle`.
fn painted_contains(h: &Harness<'_, AppState>, needle: &str) -> bool {
    painted_text(h).iter().any(|t| t.contains(needle))
}

// -- Ticket 17 — the Worktrees pane's designed state --------------------------
//
// Two harnesses, on purpose.
//
// The **pane harness** below renders `turbogit_ui::ui::worktrees::show` — the
// pane's own entry point, the same one `widget_library.rs` measures the shared
// chrome through — over a cache this file seeds by hand, and it never drains
// the event pump. That is what makes the painted claims deterministic: the
// shell is what dispatches the per-worktree dirty probes, so with no shell no
// probe can land, and a row's STATE cell stays in the **unknown** state for as
// long as the test needs it to. A ratchet about `probing…` that raced a worker
// thread would be a ratchet that sometimes proves nothing.
//
// The **shell harness** (the file's existing `harness`) drives the real
// `turbogit_ui::ui::render` and still owns everything behavioural: adding a
// worktree end to end, removing one end to end, and the dialog's
// accessibility labels. Conformance does not get to skip the production path
// to make itself comfortable.
//
// Nothing below reaches into the view's internals: every claim is read off
// painted output (galleys, fills, filled circles) or off the accessibility
// tree, because those are the two surfaces a user actually meets.

/// One seeded worktree, in whatever probe state the test needs it in.
fn worktree(root: &RootId, path: PathBuf, branch: &str, dirty: Option<bool>) -> Worktree {
    Worktree {
        path,
        branch: branch.to_string(),
        dirty,
        root: root.clone(),
    }
}

/// A settled pane harness over one pane's own entry point, over a cache this
/// file seeds by hand.
///
/// `seed` fills the cache (and may read the scratch `parent` for paths);
/// `show` is the pane's `show` function itself, so the pane under test is the
/// one the shell calls and not a stand-in for it. Nothing here drains the event
/// pump: the pane is a pure view over the cache, and the shell — the only thing
/// that dispatches the per-row probes — is not being rendered.
///
/// The repository is a real one because `AppState::for_roots` discovers it; the
/// cached list is the fixture's, so the pane has nothing left to fetch and
/// nothing left to probe.
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
                // Deliberately NOT `state.drain_events()`: the pane is a pure
                // view over the cache, and the shell — the only thing that
                // dispatches a dirty probe — is not being rendered here.
                egui::CentralPanel::default().show(ui, |ui| {
                    show(ui, state);
                });
            },
            state,
        );
    settle(&mut harness);
    harness
}

/// A settled Worktrees pane harness over exactly `worktrees`, in exactly the
/// probe states given — [`pane_harness`] specialised to that pane's cache.
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

/// One seeded submodule, in whatever lifecycle state the test needs it in.
///
/// `head` and `recorded` are the checked-out and recorded commits, exactly as
/// the engine reports them: `None` for a submodule with no working copy, or for
/// one the index carries no gitlink for.
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

/// A settled Submodules pane harness over exactly the submodules given.
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

/// Every state cell painted below the column-header row, in reading order.
///
/// Scoped **by position**: the path and the state word paint in the same band,
/// and the same word appears on more than one row as soon as the fixture has
/// two worktrees in it, so a state cell is found by its column and its being
/// *under* the header — never by its string alone.
fn state_cells(h: &Harness<'_, AppState>) -> Vec<PaintedGalley> {
    status_cells(h, "STATE")
}

/// [`state_cells`] for a pane whose state column carries its own label — the
/// submodules pane calls the same column `STATUS`. The column is located from
/// that label's own painted origin, so a cell is never found by its string.
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

/// The pane header's own band for the pane whose title is `title`: the title
/// galley and its 1px hairline — two of the three things R7 says a pane header
/// is, the third being the action slot, which is asserted where it is used.
///
/// Scoped by the rule's rect rather than by "the first few galleys", so a row
/// that moved up the screen cannot be mistaken for the header.
struct HeaderBand {
    title: PaintedGalley,
    rule: Rect,
}

fn header_band(h: &Harness<'_, AppState>) -> HeaderBand {
    pane_header_band(h, "WORKTREES")
}

/// [`header_band`] for the pane whose own title is `title`. The submodules pane
/// has the same header with a different word in it, and a helper that hard-coded
/// `WORKTREES` would make that word unassertable.
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

// --- Header ------------------------------------------------------------------

/// **The tab's header is the shared pane header**: a pane title, a count chip
/// carrying the number of worktrees, and the Add-worktree control in the
/// right-aligned action slot.
///
/// Asserted as three separate facts on purpose. The title's ink and size are
/// the shared pane-title mark (R7), so this cannot be satisfied by an ad-hoc
/// band that happens to say WORKTREES. The count chip is the shared **count
/// chip** — `RAISED` fill, `INK_2` ink, the chip radius, and a monospaced
/// number — so it is the vocabulary's number and not a second counter. And the
/// Add control is a **primary** (a `BRAND` fill) rather than the compact ghost
/// it replaces, which is what makes it the pane's one blue.
#[test]
fn worktrees_pane_header_is_the_shared_pane_header_with_a_count_chip_and_the_add_primary() {
    let h = worktrees_pane_harness(
        "wt17-header",
        &[("wt-a", "feature-a", None), ("wt-b", "feature-b", None)],
    );
    let header = header_band(&h);
    assert!(
        header.title.rect.top() >= header.rule.top() - turbogit_ui::ui::widgets::PANE_HEADER_HEIGHT,
        "the title sits in the shared 28px band above the rule: title {:?}, rule {:?}",
        header.title.rect,
        header.rule
    );

    // The title is the shared pane-title mark: 9px, muted ink, chrome face.
    assert_eq!(
        painted_ink(&h, "WORKTREES"),
        Some(Palette::INK_3),
        "a pane title is the shared 9px muted mark; an ad-hoc band is what this \
         ratchet exists to catch"
    );
    assert_eq!(
        painted_font_size(&h, "WORKTREES"),
        turbogit_ui::theme::TYPE_SECTION,
        "the pane title is the shared pane-title type size"
    );

    // The count chip: the number of worktrees, in the count chip's own colours.
    // Located by the header **band**, not by "a `1` somewhere", so a row that
    // grew a chip could not satisfy this and a wrong count could not either.
    let count = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "2" && g.rect.bottom() <= header.rule.top())
        .unwrap_or_else(|| {
            panic!(
                "the header's count chip carries the number of worktrees (2 here); \
                 painted in the header band: {:#?}",
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
        "a count chip is secondary ink on the raised fill; the accent is not a \
         counter's to wear"
    );
    assert_eq!(
        count.family,
        egui::FontFamily::Monospace,
        "counts are data: the count chip's face is the data face"
    );
    let chip = filled_rects(&h)
        .into_iter()
        .find(|(rect, fill)| *fill == Palette::RAISED && rect.contains_rect(count.rect))
        .map(|(rect, _)| rect);
    assert!(
        chip.is_some(),
        "the count sits on a `RAISED` fill — the count chip's own raised role, and \
         not the ref chip's raised-on-card one, which is a different value"
    );

    // The Add control is a **primary** in the right-aligned slot: it carries a
    // solid brand fill, which is the whole reason no row may paint a blue.
    let add = h.get_by_label("Add worktree");
    assert!(
        add.rect().center().x > 600.0,
        "the add control sits in the header's right-aligned action slot: {:?}",
        add.rect()
    );
    assert!(
        filled_rects(&h)
            .iter()
            .any(|(rect, fill)| *fill == Palette::BRAND && rect.intersects(add.rect())),
        "the add control is the pane's one **primary** action, so it fills with \
         BRAND; the compact ghost it replaces filled with nothing"
    );
}

// --- Column chrome -----------------------------------------------------------

/// **One shared column-header row labels PATH, BRANCH, HEAD, STATE and
/// ACTIONS, and every data row's columns start where the header's do.**
///
/// The header is read from the shared `column_header`'s own paint and the row
/// from its own cells, then compared. That is the claim the whole table exists
/// to make: a column cannot be narrow in the header and wide in the rows,
/// because there is one number for both.
#[test]
fn worktree_columns_are_labelled_once_and_every_row_starts_where_the_header_does() {
    let h = worktrees_pane_harness("wt17-columns", &[("wt-a", "feature-a", Some(false))]);

    // All five labels exist, and each exists **once** — a column labelled twice
    // is two columns wearing one name, and a reader cannot tell which is which.
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

    // The four columns that carry a value, compared label-against-cell: the
    // header's label x against the row's own painted origin for that column.
    // The origins are compared, never a width, so a column that moved on one
    // side only is caught.
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
    // The state cell's origin is the mark pair's own rect, whose **dot** is one
    // `STATE_DOT_R` in from its left edge.
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

    // The trailing ACTIONS column shares its **right** edge with the header's
    // label for it, because a right-aligned control is placed by where it ends.
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

/// **The head cell is empty, and an empty cell is a real state.**
///
/// The HEAD column is declared and labelled, and the cached `Worktree` carries
/// no commit for the branch it has checked out — so the cell paints nothing.
/// This ratchet is the negative of that: a placeholder glyph there would read
/// as a value the pane knows, which is the failure an "empty" cell invites.
#[test]
fn the_head_cell_is_empty_rather_than_filled_with_a_placeholder() {
    let h = worktrees_pane_harness("wt17-head-empty", &[("wt-a", "feature-a", Some(false))]);

    // HEAD's band on screen, taken from the label the header painted for it.
    let head = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "HEAD")
        .expect("HEAD is labelled");
    let state_label = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "STATE")
        .expect("STATE is labelled");
    let band = head.rect.left()..state_label.rect.left();

    // Nothing at all paints inside HEAD's band on any row: no dash, no ellipsis,
    // no "—", no zero-width string. The band is empty, and that is the correct
    // rendering of "this pane has no head for this worktree".
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

// --- The path column ---------------------------------------------------------

/// **The path is the primary column**: primary ink, the monospaced data face,
/// and it takes the width the other columns leave it.
///
/// The width claim is read from **geometry**, never from the galley's text: a
/// galley's text accessor returns the *unwrapped input*, so a path that elided
/// to fit its column looks identical through `.text` and only its measured size
/// can tell the reader it did. The fixture's path is longer than the PATH column
/// is wide precisely so the claim has something to bite on.
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

    // The width: the path's own measured extent must stay inside the gap the
    // other columns leave it, so a deep path cannot push BRANCH out of line.
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
    // And it really did have to elide: the fixture path is far longer than the
    // gap, so this row is the elision case and not the "it happened to fit" one.
    let full = ui_text_width(&h, &painted.text);
    assert!(
        full > painted.rect.width() + 1.0,
        "the path column is narrower than this path, so the claim above is the \
         elision case and not a coincidence: laid out in full it is {full}pt, \
         painted at {:?}",
        painted.rect
    );
}

// --- The branch column -------------------------------------------------------

/// **The branch is the shared ref chip, filling the raised-on-card surface and
/// not the brand token.**
///
/// Asserted from the filled rect *under* the branch text rather than from the
/// galley's colour: the chip's galley is laid out in the chip's ink, and the
/// ink a reader sees is applied at paint time. Reading the fill is also the
/// claim — "which surface does this control sit on" is a question about a
/// rect, not about a string.
#[test]
fn the_worktree_branch_is_the_neutral_ref_chip_and_not_the_brand_token() {
    let h = worktrees_pane_harness("wt17-branch", &[("wt-a", "feature-a", Some(false))]);

    let branch = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "feature-a")
        .unwrap_or_else(|| panic!("the branch name paints"));

    // The whole point, stated once over the whole frame first: the pane's only
    // blue is its one primary action. A brand fill anywhere else in the pane —
    // the ref chip below most obviously — is a second invitation to act, and
    // this is the ratchet that fires when a worktree row starts painting one.
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

    // Then the specific claim, read from the filled rect *under* the branch text
    // rather than from the galley's colour: the chip's galley is laid out in the
    // chip's ink, and the ink a reader sees is applied at paint time. Reading
    // the fill is also the claim — "which surface does this control sit on" is
    // a question about a rect, not about a string.
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

// --- The state column --------------------------------------------------------

/// **A worktree whose probe has not returned renders a visible probing state,
/// and the state cell is never empty.**
///
/// The ratchet is the non-empty one, and it is deliberately geometric: the word
/// is multi-byte and the ellipsis makes a string comparison brittle, so what is
/// asserted is that the STATE cell of every row carries a painted mark with a
/// real extent. An empty arm cannot satisfy it — there is nothing to find, and
/// the row's band would simply have nothing in it.
#[test]
fn an_unprobed_worktree_renders_a_visible_probing_state_and_the_cell_is_never_empty() {
    // Two rows, both unprobed. The claim is about *every* row in the list, so
    // the fixture has to have more than one or "never empty" is one sample.
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

    // And what it renders says the pane is still asking. Matched on the leading
    // ASCII: the ellipsis is a multi-byte character and is not what this claim
    // is about, so the assertion must not be about reproducing it.
    for cell in &cells {
        assert!(
            cell.text.starts_with("probing"),
            "a worktree whose probe has not returned says so; painted in its state \
             cell: {:?}",
            cell.text
        );
    }
}

/// **The word "clean" is the only thing that renders the word clean.**
///
/// Three rows side by side in one frame — one settled clean, one settled dirty,
/// one still probing — because that is the only way the claim is checkable at
/// all. The clean row says `clean`; the probing row does not. The word a reader
/// acts on is never the fallback for a missing answer.
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
    // And the row that has NOT settled must not be the one saying it. Asserted
    // positionally, because a state cell found by its string would be the very
    // thing this ratchet is checking.
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

// --- The row action ----------------------------------------------------------

/// **Remove is a quiet row action rather than a full button, and it still
/// raises the same remove confirmation it raised before.**
///
/// Two claims, and they are independent. The *quiet* one is read off paint: a
/// quiet row action fills **nothing** at rest, where a full button fills a
/// plate the size of the control around its own label. The *same confirmation*
/// one is behavioural, and it is the pre-existing end-to-end removal test above
/// that owns it — this asserts the label the control still answers to, so the
/// quiet treatment cannot quietly rename the verb out from under it.
#[test]
fn remove_is_a_quiet_row_action_and_still_answers_to_its_name() {
    let h = worktrees_pane_harness("wt17-quiet-remove", &[("wt-a", "feature-a", Some(false))]);

    // The control is still a control: a named Button node with a hit target, so
    // a keyboard or a screen reader reaches the row action by its own name.
    let remove = h.get_by_label("Remove wt-a");
    assert!(
        remove.rect().height() >= 24.0,
        "the quiet row action still occupies a clickable target: {:?}",
        remove.rect()
    );

    // A quiet action paints **no plate**: nothing is filled at the control's own
    // geometry at rest, where a full button fills exactly that rect.
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

    // Quiet means quiet in ink too: the content's secondary step, not the accent
    // — a destructive verb painted as an invitation is asking to be pressed.
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

// --- The dialog --------------------------------------------------------------

/// **The new-worktree dialog's two text inputs remain individually labelled for
/// accessibility, with distinct labels.**
///
/// Asserted by the **widget info the text edits register**, not by the visible
/// label text: the visible labels are the dialog's own wording and may
/// legitimately change, while the accessible name is the contract — a screen
/// reader reading this dialog must be able to tell the path field from the
/// branch field, and the only way it can is if the two names differ.
#[test]
fn the_new_worktree_dialog_keeps_two_distinct_accessibility_labels_on_its_inputs() {
    // The **real** shell, not the pane harness: the dialog is the shell's, and
    // the claim is about a user reaching it the way a user reaches it.
    let parent = scratch("wt17-dialog-labels");
    let repo = temp_repo(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, std::slice::from_ref(&repo));
    state.ui.tab = Tab::Worktrees;
    let mut h = harness(state);
    settle(&mut h);

    h.get_by_label("Add worktree").click();
    settle(&mut h);

    // Both inputs are reachable by their own names…
    let path = h.get_by_label("Worktree path input");
    let branch = h.get_by_label("Worktree branch input");
    // …and they are *different* inputs. Two nodes answering to one name is a
    // screen reader reading the same field twice, and it is the failure this
    // ratchet exists to catch: the labels are attached to the text edits
    // themselves rather than to the visible label above them, precisely so the
    // two can be told apart.
    assert_ne!(
        format!("{:?}", path.rect()),
        format!("{:?}", branch.rect()),
        "the two labelled inputs are two fields, not one node reached twice"
    );

    // And each is a text input, not a button that happens to be named.
    let text_inputs = h
        .query_all_by_role(egui::accesskit::Role::TextInput)
        .count();
    assert!(
        text_inputs >= 2,
        "the dialog keeps both of its text inputs; found {text_inputs}"
    );
}

/// The size a string was laid out at, which is where a type-scale claim is read
/// from — [`PaintedGalley`] reports the family but not the size.
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

/// How wide `text` would lay out **unwrapped**, in the face it was painted in.
///
/// The counterpart to a galley's measured width, and the only honest way to ask
/// "did this elide?": a galley's `text` accessor hands back the *unwrapped
/// input*, so a string that fitted and a string that was clipped look identical
/// through it. This lays the same string out again with no width limit and
/// returns what it would have taken.
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

// -- Cycle A — Worktrees tab body -------------------------------------------

#[test]
fn worktrees_tab_lists_worktrees_with_branch_without_waiting_on_dirty() {
    let parent = scratch("wt-sub-tabs-list");
    let repo = temp_repo(&parent, "alpha");
    let wt = add_worktree(&repo, &parent, "wt-feature", "feature");
    std::fs::write(wt.join("base.txt"), "changed\n").unwrap();

    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Worktrees;
    let mut h = harness(state);
    settle(&mut h);
    // The tab's data loads asynchronously (event pump); step until it paints.
    step_until(&mut h, |h| painted_contains(h, "wt-feature"));

    // The tab paints a real browser from the cheap list: the worktree row
    // (path + branch) renders, and the add affordance. The dirty label is
    // filled in by the window-open probe (see the dirty-probe tests below).
    assert_painted(&h, "wt-feature");
    assert_painted(&h, "feature");
    assert_painted(&h, "Add worktree");
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

// -- Ticket 03 — visibility-gated dirty probes fill the rows in ---------------

/// Opening the Worktrees window starts per-worktree probes: each row's
/// clean/dirty label appears as its own probe resolves — here the single
/// dirty worktree's label paints once the probe settles.
#[test]
fn worktrees_tab_dirty_flags_fill_in_from_window_open_probes() {
    let parent = scratch("wt-sub-tabs-dirty-probe");
    let repo = temp_repo(&parent, "alpha");
    let wt = add_worktree(&repo, &parent, "wt-feature", "feature");
    std::fs::write(wt.join("base.txt"), "changed\n").unwrap();

    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Worktrees;
    let mut h = harness(state);

    step_until(&mut h, |h| painted_contains(h, "wt-feature"));
    // The dirty label is probe-driven: it paints as the window-open probe
    // settles for this row.
    step_until(&mut h, |h| painted_contains(h, "dirty"));
    assert_painted(&h, "wt-feature");
    assert_painted(&h, "feature");
}

// -- Ticket 03 — the list lands first: badge and rows before any probe --

/// The tab-strip badge derives from the cheap list alone — no dirty
/// computation anywhere in the path. Staying off the Worktrees tab, the
/// badge shows the count of a DIRTY worktree while the cached rows keep
/// their unknown probe state: the badge and eager fill never probe.
#[test]
fn worktrees_badge_counts_without_any_dirty_probe() {
    let parent = scratch("wt-sub-tabs-list-first");
    let repo = temp_repo(&parent, "alpha");
    let wt = add_worktree(&repo, &parent, "wt-feature", "feature");
    std::fs::write(wt.join("base.txt"), "changed\n").unwrap();
    let state = AppState::for_roots(&parent, &[repo]);
    // Stay off the Worktrees window: the badge must not wait on (or start)
    // any probe.
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

/// "Loading worktrees…" is tied to the LIST, not the (never-run) probe: once
/// the list lands, the rows paint and the loading text is gone.
#[test]
fn worktrees_loading_text_disappears_once_the_list_arrives() {
    let parent = scratch("wt-sub-tabs-loading");
    let repo = temp_repo(&parent, "alpha");
    add_worktree(&repo, &parent, "wt-feature", "feature");
    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Worktrees;
    let mut h = harness(state);

    step_until(&mut h, |h| painted_contains(h, "wt-feature"));
    assert!(
        !painted_contains(&h, "Loading worktrees"),
        "the loading state is gone once the list has arrived"
    );
}

/// The "No linked worktrees" empty state appears once the (empty) list
/// arrives — no slow scan precedes it.
#[test]
fn worktrees_empty_state_shows_once_the_list_arrives_empty() {
    let parent = scratch("wt-sub-tabs-empty");
    let repo = temp_repo(&parent, "alpha");
    let mut state = AppState::for_roots(&parent, &[repo]);
    state.ui.tab = Tab::Worktrees;
    let mut h = harness(state);

    step_until(&mut h, |h| painted_contains(h, "No linked worktrees"));
    assert_painted(&h, "Add worktree");
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

    // End to end: the worktree exists on disk AND the (refreshed, refetched)
    // tab lists it — wait for the painted row, not just the directory (git
    // creates it mid-op, long before the completion refresh lands).
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

/// A superproject with one locally-added submodule; returns the
/// canonicalized superproject root.
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

// -- Cycle B — Submodules tab body -------------------------------------------

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

/// The Submodules tab still reports a diverged submodule — and the way it says
/// so is now **two columns** rather than one sentence.
///
/// **Which assertion moved, and why.** This test used to end with
/// `assert_painted(&h, "recorded")`: the commit summary was one galley reading
/// `pinned a1b2c3d → recorded e4f5a6b`, and "recorded" was how the assertion
/// knew the record was on screen. Ticket 18 splits that sentence into the
/// CHECKED OUT and RECORDED columns, so the *word* `recorded` is gone from the
/// view by design — the column header says what the cell is, and the sentence
/// was saying twice what the header says once. The claim is kept and restated
/// as what replaced it: **both** commits are painted, each at its own column's
/// origin, which is strictly more than the old assertion could check (it could
/// not tell which side moved, and it could not tell a half-empty cell from a
/// whole one). Nothing was dropped — `Needs update` is still asserted, and the
/// geometric form of the divergence claim is now in
/// `the_two_commit_columns_are_two_columns_and_only_the_side_that_moved_is_coloured`.
#[test]
fn submodules_tab_shows_needs_update_with_pinned_vs_recorded() {
    let parent = scratch("wt-sub-tabs-sub-ahead");
    let repo = super_with_submodule(&parent, "alpha");
    // Move the submodule's HEAD off the recorded commit.
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
    // Both sides are on screen, each under its own column's header: the
    // checked-out commit and the recorded one, in the columns the shared
    // header labelled, rather than in one sentence that put them in a string.
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

// -- Cycle C — tab-strip badges ----------------------------------------------

/// The Worktrees tab badge paints the live count of the focused root's
/// linked worktrees (screen 01 "Worktrees 3"), without visiting the tab.
#[test]
fn tab_badges_show_live_worktree_and_submodule_counts() {
    let parent = scratch("wt-sub-tabs-badges");
    let repo = temp_repo(&parent, "alpha");
    add_worktree(&repo, &parent, "wt-a", "feature-a");
    add_worktree(&repo, &parent, "wt-b", "feature-b");
    let state = AppState::for_roots(&parent, &[repo]);
    // Stay on Changes: the badge must be live, not visit-driven.
    let mut h = harness(state);
    settle(&mut h);
    step_until(&mut h, |h| painted_contains(h, "Worktrees 2"));
}

// -- Ticket 18 — the Submodules pane's designed state -------------------------
//
// The same two-harness split the Worktrees block above is built on, for the same
// reason. The **pane harness** (`submodules_pane_harness`) renders
// `turbogit_ui::ui::submodules::show` over a cache this file seeds by hand, so
// every submodule is in exactly the lifecycle state the claim needs and nothing
// races a worker thread. The **shell harness** (the file's `harness`) still owns
// everything behavioural: the deinit round trip, the Init action, and the state
// the action produces are all driven through the real `ui::render` over a real
// superproject on disk.
//
// Nothing below reaches into the view's internals. Every claim is read off
// painted output — galleys, filled rects, filled circles — or off the
// accessibility tree, because those are the two surfaces a user meets.

/// The two-row fixture most of the block shares: one submodule whose checkout
/// has moved off its record, one whose two commits agree.
///
/// Both paths are deep enough to be a path and not a word, and the two rows
/// differ in the only two ways the pane can tell them apart — which is what lets
/// one frame answer "what does a divergence look like" and "what does an
/// agreement look like" at once.
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

/// The x a column's header label was painted at.
fn header_x(h: &Harness<'_, AppState>, label: &str) -> f32 {
    painted_galleys(h)
        .into_iter()
        .find(|g| g.text == label)
        .unwrap_or_else(|| panic!("`{label}` is labelled exactly once by the shared row"))
        .pos
        .x
}

/// Every galley painted on the row whose status dot sits at `row_centre`, whose
/// painted centre falls within `tolerance` of it.
///
/// Scoped **by position, never by string**: this block seeds rows whose paths
/// and states repeat across the fixture, and the whole point of several of these
/// ratchets is that a cell found by its own value is the thing being checked.
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

/// The status dot of every row in the pane, in row order, with its colour.
///
/// The mark pair's dot is the pane's only circle, so it is also the honest
/// anchor for "which band is this row": every cell in a row is centred against
/// it, and a row whose dot cannot be found is a row whose status cell is empty.
fn status_dots(h: &Harness<'_, AppState>) -> Vec<(egui::Pos2, egui::Color32)> {
    let mut dots: Vec<(egui::Pos2, egui::Color32)> = filled_circles(h)
        .into_iter()
        .filter(|(_, radius, _)| (*radius - turbogit_ui::ui::components::STATE_DOT_R).abs() < 0.01)
        .map(|(centre, _, colour)| (centre, colour))
        .collect();
    dots.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));
    dots
}

/// The header-label x of each of the pane's five columns, in table order.
fn column_labels(h: &Harness<'_, AppState>) -> [(&'static str, f32); 5] {
    [
        ("PATH", header_x(h, "PATH")),
        ("CHECKED OUT", header_x(h, "CHECKED OUT")),
        ("RECORDED", header_x(h, "RECORDED")),
        ("STATUS", header_x(h, "STATUS")),
        ("ACTIONS", header_x(h, "ACTIONS")),
    ]
}

// --- Header ------------------------------------------------------------------

/// **The tab's header is the shared pane header, and its new count chip counts
/// submodules — and does not wear the reserved counter orange.**
///
/// The count chip is a *new element* in this pane: the header had no count
/// anywhere, and it takes one here. What it counts is the pane's own list, and
/// the assertion is deliberately picky about that: the fixture has **two**
/// submodules and **one** of them is `Needs update`, so a chip saying `2` is a
/// count of submodules and a chip saying `1` would be a count of something
/// interesting. And it is the count chip's own treatment — a `RAISED` fill with
/// secondary monospaced ink — so the number is the shared vocabulary's and not a
/// second counter borrowing the reserved orange, which means dirt and unpulled.
#[test]
fn the_submodules_header_is_the_shared_pane_header_counting_submodules_in_the_neutral_count_chip() {
    let h = diverged_and_matching("sub18-header");

    let header = pane_header_band(&h, "SUBMODULES");
    assert!(
        header.title.rect.top() >= header.rule.top() - turbogit_ui::ui::widgets::PANE_HEADER_HEIGHT,
        "the title sits in the shared 28px band above the rule: title {:?}, rule {:?}",
        header.title.rect,
        header.rule
    );
    assert_eq!(
        painted_ink(&h, "SUBMODULES"),
        Some(Palette::INK_3),
        "a pane title is the shared 9px muted mark; an ad-hoc band is what this \
         ratchet exists to catch"
    );
    assert_eq!(
        painted_font_size(&h, "SUBMODULES"),
        turbogit_ui::theme::TYPE_SECTION,
        "the pane title is the shared pane-title type size"
    );

    // The count, located by the header **band** so a row's own text could not
    // satisfy it and a wrong number could not either.
    let count = painted_galleys(&h)
        .into_iter()
        .find(|g| g.text == "2" && g.rect.bottom() <= header.rule.top())
        .unwrap_or_else(|| {
            panic!(
                "the header's count chip carries the number of submodules (2 here); \
                 painted in the header band: {:#?}",
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
        "a count chip is secondary ink on the raised fill — the accent and the \
         reserved counter orange are not a counter's to wear"
    );
    assert_eq!(
        count.family,
        egui::FontFamily::Monospace,
        "counts are data: the count chip's face is the data face"
    );
    assert!(
        filled_rects(&h)
            .into_iter()
            .any(|(rect, fill)| fill == Palette::RAISED && rect.contains_rect(count.rect)),
        "the count sits on the `RAISED` fill — the count chip's own raised role, \
         and not the ref chip's raised-on-card one, which is a different value"
    );
    // The reserved orange, stated as the negative it is: nothing in this pane
    // fills with it. A pane count is not a dirt or unpulled count.
    let counter: Vec<Rect> = filled_rects(&h)
        .into_iter()
        .filter(|(_, fill)| *fill == Palette::COUNTER)
        .map(|(rect, _)| rect)
        .collect();
    assert!(
        counter.is_empty(),
        "the reserved counter orange means dirt and unpulled; a count of \
         submodules is neither, and the count chip exists so this number does \
         not have to borrow it. Counter-filled rects: {counter:?}"
    );
}

// --- Column chrome -----------------------------------------------------------

/// **One shared column-header row labels PATH, CHECKED OUT, RECORDED, STATUS and
/// ACTIONS, and every data row's column starts where the header's does.**
///
/// Each label is read from the shared `column_header`'s own paint and each cell
/// from its own painted origin, then compared — for **every row**, not a sample.
/// That is the claim the whole table exists to make: a column cannot be narrow
/// in the header and wide in the rows, because there is one number for both.
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

    // All five labels exist, and each exists **once** — a column labelled twice
    // is two columns wearing one name.
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

    // The four left-anchored columns are four columns and not two wearing two
    // names — asserted before any cell is looked for, so a table that gave two
    // of them one origin says so in a sentence rather than as a missing cell.
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

    // Row 0 is the diverged submodule, so all four value columns are occupied on
    // it and every origin can be compared against the header's.
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

    // The status cell's origin is the mark pair's own rect, whose **dot** is one
    // `STATE_DOT_R` in from its left edge.
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

    // The trailing ACTIONS column shares its **right** edge with the header's
    // label for it, because a right-aligned control is placed by where it ends.
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

    // And the *second* row's cells too, so "every row" is two rows rather than
    // one. Row 1's RECORDED cell is empty on purpose — the next ratchet is about
    // that — so only the two occupied columns are compared here.
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

// --- The two commit columns --------------------------------------------------

/// **The checked-out and recorded commits are two columns; a divergence colours
/// the side that moved; and an agreement leaves the second column empty.**
///
/// All three claims in one frame, because they are one decision: the two
/// columns exist so a divergence is *visible*, and a divergence that cannot be
/// seen is a column split for its own sake.
///
/// The distinction between the two sides is read from painted ink, and the
/// expected value is not spelled out here — it is
/// `RepoState::Diverged.color()`, read through the same one map the view is
/// required to use. Writing `Palette::STATUS_DIVERGED` here instead would let a
/// view-local literal pass while the two answers drifted apart, which is the
/// failure the ratchet exists to catch.
#[test]
fn the_two_commit_columns_are_two_columns_and_only_the_side_that_moved_is_coloured() {
    use turbogit_ui::theme::RepoState;
    let h = diverged_and_matching("sub18-two-commit-columns");
    let [(_, _), (_, out_x), (_, recorded_x), ..] = column_labels(&h);
    let dots = status_dots(&h);

    // -- they are two columns, not one sentence --------------------------------
    assert!(
        (out_x - recorded_x).abs() > 1.0,
        "CHECKED OUT and RECORDED are two columns at {out_x} and {recorded_x}; a \
         single cell holding one sentence is what this ticket replaced"
    );

    // -- the diverged row: both values, each in its own column ------------------
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

    // -- …and the side that moved is the distinguished one ----------------------
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

    // -- the matching row: one column occupied, one empty ------------------------
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

// --- The row action ----------------------------------------------------------

/// **The action on an uninitialised submodule is labelled `Init`, and pressing it
/// still performs an init — read off the state that press produces, not off the
/// source.**
///
/// The label and the flag are two separate claims and both matter. The label
/// tells the user what the press will do; the flag is what it actually does, and
/// it was already `true` before this ticket — only the word was wrong. So the
/// behavioural half is asserted by **driving the real action** through the shell
/// and then reading the refetched cache: a submodule that was deinitialised and
/// comes back `UpToDate` with a head equal to its record was *initialised*. Had
/// the flag been flipped to `false`, `git submodule update` would have had no
/// working copy to update and the submodule would still read `Uninitialized`
/// with no head — which is exactly what the assertion below rules out.
///
/// The label is asserted from the **painted text and the accessibility label
/// together**, so a screen-reader user and a sighted user read the same word.
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

    // An initialised submodule keeps the update verb, and the deinit action is
    // still offered on it — this is the state the ticket changes nothing about.
    assert_painted(&h, "Update child");
    assert!(
        h.query_all_by_label("Init child").next().is_none(),
        "an initialised submodule's action is an update, not an init"
    );

    // …and the deinit action still behaves as before.
    h.get_by_label("Deinit child").click();
    settle(&mut h);
    h.get_by_label("OK").click();
    step_until(&mut h, |h| painted_contains(h, "Uninitialized"));

    // Now the claim: the control says what it will do, in both surfaces a user
    // meets. Scoped by position is not possible here (both verbs carry the same
    // submodule name), so the exact labels are what is asserted.
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
    // Deinit is still there: renaming one action must not remove the other.
    assert_painted(&h, "Deinit child");

    // **The flag**, read off the state the press produces.
    h.get_by_label("Init child").click();
    // A *bounded* wait, deliberately: the thing being waited for is the thing
    // being asserted, so the failure has to be a sentence about the state and
    // not a ten-second timeout on the answer this test expected.
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
    // …and the row is back to offering the update verb, which is the same fact
    // as the one above, read off the surface the user pressed.
    assert!(
        step_briefly(&mut h, |h| h.query_all_by_label("Update child").count()
            == 1),
        "once the submodule is initialised again, its action is an update again"
    );

    // The neutral reading of the state, asserted on the same object so the
    // `Uninitialized` arm of the one map is pinned by something observable.
    assert_ne!(
        RepoState::Clean.color(),
        RepoState::Uninitialized.color(),
        "a clean submodule and an uninitialised one must not look alike"
    );
}

// --- The status mark pair ----------------------------------------------------

/// **The status word is a leading dot and coloured text with no background at
/// all — and its colour comes from the one repository-state map.**
///
/// Four rows in one frame, one per lifecycle state, because the claim is about
/// every state the pane can render and a sample of one proves nothing. For each
/// row the dot's colour is compared against `RepoState::….color()` read through
/// the shared map — **not** against a literal spelled here, because a literal in
/// the test would let a second opinion in the view pass.
///
/// The negative is the load-bearing half: a render seam can only prove "no
/// background" at the sites a test enumerates, so it is enumerated at the site
/// that matters — the status word's own paint origin — where **no filled rect
/// covers it**, and the only mark beside it is a filled **circle** of the dot's
/// radius rather than a rounded rect. A chip is a bounded container; a dot is a
/// mark, and the difference is the whole claim.
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

    // Each row's expected colour, read through the shared map rather than
    // written out — the ratchet is against a *second* map, and a literal here
    // would be the second map.
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
        // The word is beside the dot, and is the **same** colour as it: a dot and
        // a word disagreeing about the state they are both talking about is the
        // failure the single map prevents.
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
        // The dot leads: it is inside the cell and the word follows it.
        assert!(
            painted_word.rect.left() > centre.x && painted_word.rect.left() - centre.x < 24.0,
            "the dot leads the word it belongs to: dot at {centre:?}, word at {:?}",
            painted_word.rect
        );

        // -- the negative: no background at all --------------------------------
        let covering: Vec<(egui::Rect, egui::Color32)> = filled_rects(&h)
            .into_iter()
            // A full-width surface is the pane behind the row, not a mark in it;
            // everything narrower than that, covering the word, would be a plate.
            .filter(|(rect, _)| rect.width() < 600.0 && rect.contains(painted_word.pos))
            .collect();
        assert!(
            covering.is_empty(),
            "`{word}` paints **no background at all**: no chip fill, no chip \
             radius, no chip geometry. Found fills covering the word at {painted_word:?}: \
             {covering:?}"
        );
        // Nothing at all is filled in the mark's own band — the dot's cell and
        // the word beside it — which is exactly where a chip would put its
        // rect. The band is the mark's own extent and no more, so a control on
        // the far side of the row cannot be mistaken for one.
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

    // A clean submodule and a diverged one never look alike — the whole reason
    // the column has a dot to scan.
    assert_ne!(
        expected[0].1, expected[1].1,
        "a clean submodule and a diverged one must never share a colour"
    );
    assert_ne!(
        dots[0].1, dots[1].1,
        "…and the painted dots prove it on screen, not just in the mapping"
    );

    // The **uninitialised** row is the one this column could not say before, so
    // its colour is named here rather than only read through the map: the map
    // entry is pinned in `design_tokens.rs`, and this says the *view* wears it.
    //
    // The two negatives are the whole point. `COUNTER` is the reserved
    // counter orange and means dirt and unpushed counts — "N commits incoming"
    // — which is a different fact from "there is no working copy here at all";
    // borrowing it puts a missing checkout in the same colour as incoming
    // commits. `AHEAD` is clean's green, and a submodule with no checkout is not
    // clean. And the error red belongs to a conflict or a divergence, which this
    // is not, so the row must not wear a severity at all.
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

/// **A submodule deinitialised through the real action paints the muted ink,
/// and the ink is legible on the surface it lands on.**
///
/// The sibling above reads a *seeded* row, so it proves the view's translation
/// but not that a real deinit reaches it. This one drives the production path —
/// the pane's own `Deinit` control, the confirmation dialog, `OK`, and the
/// refetched cache — and only then reads the painted dot, so the state being
/// asserted is a state git actually reported rather than one a fixture declared.
///
/// The legibility half is the load-bearing addition. `INK_3`'s contract is not
/// "AA everywhere": it is legal on the app, panel, content and sidebar surfaces
/// and **not** on a raised or selected one, so a new consumer of it has to
/// answer "which surface is this on". The answer is measured rather than
/// asserted by name: the dot is painted inside the tool pane, which is a
/// `Ui::new_child` of the central panel and so inherits `panel_fill` — and
/// `INK_3` on that fill is asserted here to clear 4.5:1, which is the same
/// number the ramp block in `theme.rs` states for the app background.
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

    // The real action: press the pane's own Deinit control and confirm it.
    h.get_by_label("Deinit child").click();
    settle(&mut h);
    h.get_by_label("OK").click();
    step_until(&mut h, |h| painted_contains(h, "Uninitialized"));

    // …and the state the pane is now showing came from the refetched cache, not
    // from a local flag flipped by the press.
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

    // The painted dot, located on the row that carries the word rather than by
    // its index, so a second row appearing cannot silently move the answer.
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

    // The legality contract, measured at the surface the dot actually lands on.
    //
    // **`INK_3` is not legal everywhere**, so "which surface is this on" has to
    // be answered rather than assumed. The answer has two halves and both are
    // checked here. The first is what the dot sits *inside*: the submodules pane
    // is a `Ui::new_child` of the shell's central panel and paints no fill of
    // its own, and `configure_style` maps `panel_fill` to `Palette::BG` — read
    // off the configured context rather than off a painted rect, because the
    // harness's own backing rect is egui's stock dark fill and is not a token
    // this app chose. The second is that nothing *else* is between the dot and
    // that fill: a raised or selected band covering the dot is precisely the
    // case the ramp's narrowing exists to catch, and it is a negative that only
    // the painted frame can answer.
    let ctx = egui::Context::default();
    turbogit_ui::theme::configure_style(&ctx);
    let panel_fill = ctx.style_of(egui::Theme::Dark).visuals.panel_fill;
    assert_eq!(
        panel_fill,
        Palette::BG,
        "the central panel's fill is the app background, which is one of the four \
         surfaces `INK_3` is legal on"
    );
    let ratio = relative_luminance_contrast(painted, panel_fill);
    println!("INK_3 on the submodules pane's own fill: {ratio:.3}:1");
    assert!(
        ratio >= 4.5,
        "INK_3 must clear AA on the surface the dot lands on, got {ratio:.3}:1 on \
         {panel_fill:?}"
    );

    // …and no raised or selected band is painted over it. `INK_3` is 4.20:1 on
    // `SURFACE`, 3.81 on `SURFACE_2`, 3.23 on `SURFACE_3`, 3.01 on `SELECTION`
    // and 3.76 on `ROW_SELECTED` — sub-AA on every one of them — so a band of
    // any of those under this dot is the contract being broken, and the caller
    // that breaks it must step up to `INK_2` rather than the ramp being relaxed.
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

/// WCAG relative luminance of a painted colour, as egui stored it.
///
/// A copy of the ratio the token suite measures with, so the legality claim at
/// the *call site* uses the same arithmetic the ramp's own numbers came from
/// rather than a second formula that could disagree with the first by a
/// rounding step.
fn relative_luminance_contrast(a: egui::Color32, b: egui::Color32) -> f64 {
    let lin = |c: u8| {
        let v = f64::from(c) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let lum = |c: egui::Color32| 0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b());
    let (la, lb) = (lum(a), lum(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

// --- The row's selection -----------------------------------------------------

/// **No submodule row paints a selection-token fill, and every row's first text
/// starts at the same origin.**
///
/// The pane has **no chosen-row state** — there is nothing in the view that
/// selects a submodule, and this ticket does not invent one — so the criterion
/// is asserted in the two forms it can take. The first is the negative that
/// matters: neither `ROW_SELECTED` nor the heavier `SELECTION` appears anywhere
/// in the pane, which is the claim a solid band behind running text would break.
///
/// The second is the stronger available form of "a selected row's first text
/// origin equals an unselected row's": with no selected row to compare, **every**
/// row is compared instead, and all of them paint PATH at one origin — the
/// header's. A selection treatment that arrives later and moves the text would
/// have to move it away from that number to be caught here, and moving it is
/// exactly the regression this pins.
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

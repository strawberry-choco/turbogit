//! Chinese content must paint real glyphs in the commit view, not egui's
//! notdef box.
//!
//! The report was that a Chinese commit subject showed up as a row of empty
//! boxes. The strings themselves reach the frame intact — the git read, the
//! display model and the galley all carry the right characters — so a
//! painted-text assertion cannot see this defect: `assert_painted` matches the
//! galley's *string*, and a galley full of boxes carries the string just the
//! same. What has to be measured is whether the face chain the shell installs
//! can actually display each character, which is what these two tests do.
//!
//! The first pins the font stack itself; the second drives the real commit
//! view over a seeded repository and checks every CJK-bearing galley in the
//! frame, in the family that galley was laid out with.

use egui::{FontFamily, FontId};
use std::path::{Path, PathBuf};
use test_support::git_seed::git;
use test_support::harness::{painted_galleys, settle, shell_harness_over};
use turbogit_app::state::{AppState, Tab};
use turbogit_ui::theme::font_definitions;

/// The commit subject the defect report named, in the script it named.
const SUBJECT: &str = "修复提交视图无法显示中文";

/// Characters outside everything the design stack leads with, i.e. the ones a
/// reader sees as boxes when no fallback face covers them.
fn uncovered_script(text: &str) -> Vec<char> {
    text.chars().filter(|c| *c > '\u{2e80}').collect()
}

/// A repository whose root name, commit subject and file name are all Chinese.
fn repo_with_chinese(parent: &Path) -> PathBuf {
    let path = parent.join("中文仓库");
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.email", "test@example.com"]);
    git(&path, &["config", "user.name", "Test"]);
    std::fs::write(path.join("说明.md"), "第一行\n").unwrap();
    git(&path, &["add", "."]);
    git(&path, &["commit", "-q", "-m", SUBJECT]);
    path
}

/// Every face chain registered by the design stack.
fn chains() -> Vec<FontFamily> {
    font_definitions().families.keys().cloned().collect()
}

#[test]
fn every_font_chain_can_display_chinese() {
    let mut fonts = egui::epaint::text::Fonts::new(
        egui::epaint::text::TextOptions::default(),
        font_definitions(),
    );
    // Han only. epaint answers `has_glyph` false for any character that resolves
    // to the chain's replacement face, and a chain whose leading face is also its
    // `?` carrier reports false for the Latin it renders perfectly well.
    let probe = "中文仓库说明";
    let mut uncovered = Vec::new();
    for family in chains() {
        if !fonts.has_glyphs(&FontId::new(12.0, family.clone()), probe) {
            uncovered.push(family);
        }
    }
    assert!(
        uncovered.is_empty(),
        "these chains in the design font stack cannot display {probe:?}: {uncovered:?}. \
         A character no face in the chain covers paints as an empty box."
    );
}

#[test]
fn the_commit_view_paints_chinese_in_real_glyphs() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_chinese(tmp.path());
    let state = AppState::for_roots(tmp.path(), std::slice::from_ref(&repo));
    let mut h = shell_harness_over(state, egui::vec2(1280.0, 800.0));
    settle(&mut h);
    h.state_mut().ui.tab = Tab::Log;
    settle(&mut h);
    let head = git(&repo, &["rev-parse", "HEAD"]).trim().to_owned();
    h.state_mut().ui.selected_commit = Some(head);
    // The changed-file rows of the selected commit are a worker round-trip.
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(25));
        h.step();
    }

    let galleys = painted_galleys(&h);
    let painted: Vec<&str> = galleys.iter().map(|g| g.text.as_str()).collect();
    assert!(
        painted.contains(&SUBJECT),
        "the commit view must carry the seeded subject into the frame at all; \
         it painted {painted:?}"
    );

    let mut boxes: Vec<(String, FontFamily, Vec<char>)> = Vec::new();
    for galley in &galleys {
        let probe = uncovered_script(&galley.text);
        if probe.is_empty() {
            continue;
        }
        let font_id = FontId::new(12.0, galley.family.clone());
        let covered = h.ctx.fonts_mut(|f| f.has_glyphs(&font_id, &galley.text));
        if !covered {
            boxes.push((galley.text.clone(), galley.family.clone(), probe));
        }
    }
    assert!(
        boxes.is_empty(),
        "the commit view laid these strings out in a face chain that cannot \
         display them, so they paint as empty boxes: {boxes:?}"
    );
}

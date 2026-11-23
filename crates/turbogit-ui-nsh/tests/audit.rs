//! The N6 audit: accessibility names and roles, keyboard dismissal, reduced
//! motion, the 200 percent scale, the empty and detached states, and the
//! token literal scan. A gap this file cannot prove (a painted focus ring,
//! a stroke probe, a native screen reader) is named in the N6 report rather
//! than asserted here.

use std::path::{Path, PathBuf};

use nsh_core::Checked;
use nsh_engine::Size;
use nsh_engine::geometry::Points;
use support::{Mounted, Project, for_roots, shows_part, wait_for_part};

mod support;

fn shell(project: &Project) -> Mounted {
    let repo = support::seeded_repo(project.path(), "alpha");
    for_roots(project.path(), std::slice::from_ref(&repo))
}

fn open(app: &mut Mounted, id: &str, dialog: &str) {
    app.press(id).unwrap();
    app.wait_for(dialog).unwrap();
}

fn assert_no_horizontal_clip(app: &Mounted, what: &str) {
    let width = 1280.0;
    for node in app.screen().layout_nodes() {
        if node.hidden || node.bounds.size.width.0 <= 0.0 {
            continue;
        }
        let right = node.bounds.origin.x.0 + node.bounds.size.width.0;
        assert!(
            right <= width + 1.0,
            "{what}: {:?} reaches {right} at the 200 percent scale",
            node.test_id
        );
    }
}

#[test]
fn every_n6_control_has_a_role_and_a_name() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();

    open(&mut app, "id:toolbar:settings", "id:dialog:settings");
    assert!(
        app.screen().matches("role:dialog[Settings]").unwrap(),
        "the settings modal is a named dialog"
    );
    assert!(
        app.screen()
            .matches("role:switch[Restore workspace context on branch switch]")
            .unwrap(),
        "the switch carries its own name"
    );
    assert!(
        app.screen()
            .matches("role:button[Restore defaults]")
            .unwrap(),
        "Restore defaults is a named button"
    );
    app.press("id:settings:tab:git").unwrap();
    app.wait_for("id:settings:git-executable").unwrap();
    assert!(
        app.screen().matches("label:Git executable").unwrap(),
        "the git field names itself"
    );
    app.press("id:dialog:settings:cancel").unwrap();
    app.wait_for_gone("id:dialog:settings", std::time::Duration::from_secs(5))
        .unwrap();

    open(&mut app, "id:toolbar:shortcuts", "id:dialog:shortcuts");
    assert!(
        app.screen().matches("role:dialog[Shortcuts]").unwrap(),
        "the shortcuts window is a named dialog"
    );
    assert!(
        app.screen().matches("id:shortcuts:table").unwrap(),
        "the table wrapper names the table"
    );
    app.press("id:dialog:shortcuts:cancel").unwrap();
    app.wait_for_gone("id:dialog:shortcuts", std::time::Duration::from_secs(5))
        .unwrap();

    open(&mut app, "id:toolbar:palette", "id:dialog:command-palette");
    assert!(
        app.screen().matches("role:dialog[Find Action]").unwrap(),
        "the palette is a named dialog"
    );
    assert!(
        app.screen().matches("label:Search actions").unwrap(),
        "the search field names itself"
    );
    app.press("id:dialog:command-palette:cancel").unwrap();
    app.wait_for_gone(
        "id:dialog:command-palette",
        std::time::Duration::from_secs(5),
    )
    .unwrap();
}

#[test]
fn a_blame_row_is_a_named_button_and_the_dialog_is_named() {
    let project = Project::new();
    let repo = project.path().join("alpha");
    std::fs::create_dir_all(&repo).unwrap();
    test_support::git_seed::git(&repo, &["init", "-q", "-b", "main"]);
    test_support::git_seed::git(&repo, &["config", "user.email", "t@t"]);
    test_support::git_seed::git(&repo, &["config", "user.name", "Test"]);
    test_support::git_seed::commit(&repo, "file.txt", "one\ntwo\n", "one");
    let commit = test_support::git_seed::git(&repo, &["rev-parse", "HEAD"])
        .trim()
        .to_string();
    let project_dir = project.path().to_path_buf();
    let mut app = support::for_roots_seeded(&project_dir, std::slice::from_ref(&repo), {
        let path = repo.join("file.txt");
        move |s| {
            let root = s.multi.roots[0].id.clone();
            s.ui.blame = Some(turbogit_app::state::BlameTarget {
                root,
                path,
                rev: commit,
            });
        }
    });
    app.first_render().unwrap();
    app.wait_for("id:dialog:blame").unwrap();
    app.wait_for("id:blame:row:1").unwrap();
    assert!(
        app.screen().matches("role:dialog[Blame]").unwrap(),
        "the blame dialog is named"
    );
    assert!(
        app.screen().count("role:button").unwrap() >= 3,
        "the rows and the dismiss action are buttons"
    );
}

#[test]
fn reduced_motion_keeps_the_state_change() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();
    app.set_environment("reduced motion", |env| {
        env.reduce_motion = true;
    })
    .unwrap();
    open(&mut app, "id:toolbar:settings", "id:dialog:settings");
    app.press("id:settings:restore-workspace").unwrap();
    let state = app
        .screen()
        .accessibility_state("id:settings:restore-workspace")
        .unwrap();
    assert_eq!(
        state.checked,
        Some(Checked::True),
        "the switch reports its new state with reduced motion on"
    );
}

#[test]
fn a_200_percent_scale_leaves_no_n6_dialog_clipped() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();
    app.set_environment("font scale 200%", |env| env.font_scale = 2.0)
        .unwrap();
    app.set_viewport(Size::new(Points(1280.0), Points(820.0)))
        .unwrap();
    app.first_render().unwrap();

    for (open_id, dialog) in [
        ("id:toolbar:settings", "id:dialog:settings"),
        ("id:toolbar:shortcuts", "id:dialog:shortcuts"),
        ("id:toolbar:palette", "id:dialog:command-palette"),
    ] {
        open(&mut app, open_id, dialog);
        assert_no_horizontal_clip(&app, dialog);
        app.press_key("escape").unwrap();
        app.wait_for_gone(dialog, std::time::Duration::from_secs(5))
            .unwrap();
    }
}

#[test]
fn an_unborn_repository_still_opens_settings() {
    let project = Project::new();
    let repo = project.path().join("empty");
    std::fs::create_dir_all(&repo).unwrap();
    test_support::git_seed::git(&repo, &["init", "-q", "-b", "main"]);
    let mut app = for_roots(project.path(), std::slice::from_ref(&repo));
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();
    open(&mut app, "id:toolbar:settings", "id:dialog:settings");
    assert!(shows_part(
        &app,
        "Restore workspace context on branch switch"
    ));
}

#[test]
fn a_detached_head_still_paints_the_shell() {
    let project = Project::new();
    let repo = support::seeded_repo(project.path(), "alpha");
    test_support::git_seed::git(&repo, &["checkout", "-q", "--detach"]);
    let mut app = for_roots(project.path(), std::slice::from_ref(&repo));
    app.first_render().unwrap();
    app.wait_for("id:status-bar").unwrap();
    assert!(app.screen().shows("1 total"));
}

#[test]
fn a_lifecycle_verb_with_nothing_in_flight_explains_itself() {
    let project = Project::new();
    let mut app = shell(&project);
    app.first_render().unwrap();
    app.wait_for("text:1 total").unwrap();

    open(&mut app, "id:toolbar:palette", "id:dialog:command-palette");
    app.type_text("label:Search actions", "Continue Operation")
        .unwrap();
    app.press("text:Continue Operation").unwrap();
    wait_for_part(&mut app, "Nothing in flight to Continue").unwrap();
}

#[test]
fn no_literal_color_or_radius_lives_outside_the_token_layer() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    assert!(files.len() > 10, "the scan reached the crate sources");
    for file in files {
        if file.ends_with("src/theme.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for (at, line) in text.lines().enumerate() {
            let where_ = format!("{}:{}", file.display(), at + 1);
            assert!(
                !line.contains("AdaptiveColor::rgb") && !line.contains("Rgba::rgb"),
                "{where_}: a color literal outside the token layer: {line}"
            );
            assert!(
                !has_hex_color(line),
                "{where_}: a hex color outside the token layer: {line}"
            );
            assert!(
                !radius_literal(line),
                "{where_}: a radius literal outside the token layer: {line}"
            );
        }
    }
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn has_hex_color(line: &str) -> bool {
    let bytes = line.as_bytes();
    for (at, byte) in bytes.iter().enumerate() {
        if *byte == b'#' && bytes.len() >= at + 7 {
            let digits = &line[at + 1..at + 7];
            if digits.chars().all(|c| c.is_ascii_hexdigit()) {
                return true;
            }
        }
    }
    false
}

fn radius_literal(line: &str) -> bool {
    line.match_indices("rounded(px(").any(|(at, _)| {
        line[at + "rounded(px(".len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    })
}

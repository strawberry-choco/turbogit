//! Issue #26 — Settings, Git page: the git-executable row shows a live
//! version badge. `resolve_git_version` is the seam: it runs the resolved
//! `<git> --version` (settings override, else PATH) and reports the parsed
//! version, or an error when the executable cannot be run.

use turbogit_domain::model::VcsSettings;
use turbogit_engine::resolve_git_version;

#[test]
fn resolves_the_version_of_git_on_path() {
    let v = resolve_git_version(&VcsSettings::default()).expect("git on PATH must resolve");
    assert!(
        !v.is_empty() && v.chars().next().is_some_and(|c| c.is_ascii_digit()),
        "version must be the numeric git version, got {v:?}"
    );
}

#[test]
fn resolves_the_version_of_an_explicit_git_executable() {
    let settings = VcsSettings {
        git_executable: "git".to_string(),
        ..VcsSettings::default()
    };
    let v = resolve_git_version(&settings).expect("explicit git must resolve");
    assert!(
        v.chars().next().is_some_and(|c| c.is_ascii_digit()),
        "got {v:?}"
    );
}

#[test]
fn reports_an_error_for_a_broken_executable_path() {
    let settings = VcsSettings {
        git_executable: "/nonexistent/turbogit-missing-git".to_string(),
        ..VcsSettings::default()
    };
    let err = resolve_git_version(&settings).expect_err("missing binary must error");
    assert!(!err.to_string().is_empty());
}

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

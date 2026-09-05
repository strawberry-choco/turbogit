//! Issue 31 — tag dialog upgrade (screen 16).
//!
//! The tag dialog's domain rules live in [`tag_service`]: live tag-name
//! validation (the dialog's "✓ valid" / reason line), tag creation through
//! the executor port, and the push-immediately outcome reporting.

use std::path::{Path, PathBuf};

use turbogit_domain::error::TgError;
use turbogit_domain::model::{Remote, TagSpec};
use turbogit_engine::fake::{Call, FakeExecutor};
use turbogit_services::tag_service;

// ------------------------------------------------------------ validate_name --

#[test]
fn a_well_formed_new_name_is_valid() {
    assert_eq!(tag_service::validate_name("v0.9.0", &[]), Ok(()));
    assert_eq!(
        tag_service::validate_name("release-1.2", &["other".to_string()]),
        Ok(())
    );
}

#[test]
fn an_empty_name_is_rejected_with_a_reason() {
    assert!(tag_service::validate_name("", &[]).is_err());
    assert!(tag_service::validate_name("   ", &[]).is_err());
}

#[test]
fn a_duplicate_name_is_rejected() {
    let existing = vec!["v0.9.0".to_string()];
    let err = tag_service::validate_name("v0.9.0", &existing).unwrap_err();
    assert!(err.contains("v0.9.0"), "reason should name the tag: {err}");
}

#[test]
fn illegal_names_are_rejected_with_the_offending_rule() {
    // Spaces and git-forbidden characters.
    for bad in [
        "has space",
        "a~b",
        "a^b",
        "a:b",
        "a?b",
        "a*b",
        "a[b",
        "a\\b",
    ] {
        let err = tag_service::validate_name(bad, &[]).unwrap_err();
        assert!(!err.is_empty(), "'{bad}' should name the violated rule");
    }
    // Structural rules: leading dot, "..", trailing dot, ".lock" suffix,
    // "@{".
    for bad in [".hidden", "a..b", "trailing.", "v1.lock", "a@{b"] {
        assert!(
            tag_service::validate_name(bad, &[]).is_err(),
            "'{bad}' should be rejected"
        );
    }
}

// --------------------------------------------------- create / push helpers ----

fn fake_with_remote(root: &Path, name: &str) -> FakeExecutor {
    let mut v = FakeExecutor::new();
    v.remotes.insert(
        root.to_path_buf(),
        vec![Remote {
            name: name.to_string(),
            fetch_url: Some(String::new()),
            push_url: None,
        }],
    );
    v
}

#[test]
fn create_dispatches_the_spec_through_the_port() {
    let root = PathBuf::from("/repo");
    let v = FakeExecutor::new();
    let spec = TagSpec {
        name: "v0.9.0".into(),
        target: Some("f4e2a91".into()),
        message: Some("Release 0.9.0".into()),
        tagger: Some("Stink Ma <stink@turbogit.dev>".into()),
        sign: true,
    };
    tag_service::create(&v, &root, &spec).unwrap();
    assert_eq!(*v.calls.lock().unwrap(), vec![Call::TagCreate { spec }]);
}

#[test]
fn push_new_pushes_to_the_first_remote_and_reports_it() {
    let root = PathBuf::from("/repo");
    let v = fake_with_remote(&root, "origin");
    let remote = tag_service::push_new(&v, &root, "v0.9.0").unwrap();
    assert_eq!(remote, "origin");
    assert!(
        v.calls.lock().unwrap().iter().any(|c| matches!(
            c,
            Call::TagPush { remote, name, .. }
                if remote == "origin" && name.as_deref() == Some("v0.9.0")
        )),
        "expected a TagPush of v0.9.0 to origin, got {:?}",
        v.calls
    );
}

#[test]
fn push_new_without_a_remote_is_a_clear_error() {
    let root = PathBuf::from("/repo");
    let v = FakeExecutor::new();
    let err = tag_service::push_new(&v, &root, "v0.9.0").unwrap_err();
    assert!(matches!(err, TgError::Other(ref m) if m.contains("remote")));
}

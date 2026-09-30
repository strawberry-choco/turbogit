//! deepen-git-engine issue 15 — the port names its one raw caller.
//!
//! The branch's opening question was whether `run_raw` was a designed escape
//! hatch or an accident, and the census answer was "mostly accidents, nine of
//! them". This suite is the part of that answer that has to stay true: exactly
//! **one** place outside the engine runs arbitrary git, and it is the bulk
//! operations grid's user-typed custom command — a feature whose product is
//! *running what the user typed*, so it cannot be raised to an answer.
//!
//! It reads the workspace's own source, which is the only way to assert a rule
//! about code that does not exist yet: a second call site fails here, in the
//! layer that added it, with the file and line named.

use std::path::Path;
use std::path::PathBuf;

/// Call sites of the port's raw escape, as `crate/file.rs:line`.
fn raw_call_sites(root: &Path) -> Vec<String> {
    let mut sites = Vec::new();
    for (krate, path) in test_support::srcscan::crate_sources(root) {
        // Only the crates that *use* the port. The engine crates own the two
        // implementations — the CLI adapter execs git, which is the whole point
        // of it — and `test-support`'s wrapper forwards every method it
        // implements, which decides nothing about what may be run.
        if !matches!(
            krate.as_str(),
            "turbogit-services" | "turbogit-app" | "turbogit-ui"
        ) {
            continue;
        }
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).expect("source readable");
        for (no, line) in text.lines().enumerate() {
            // A doc comment quoting the call is not a call.
            let quoted_in_docs = line.trim_start().starts_with("///");
            if line.contains(".run_raw(") && !quoted_in_docs {
                sites.push(format!("{krate}/{file}:{}", no + 1));
            }
        }
    }
    sites
}

#[test]
fn exactly_one_production_caller_runs_arbitrary_git() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sites = raw_call_sites(&root);
    assert_eq!(
        sites.len(),
        1,
        "`run_raw` is the port's one raw escape; a second production call site \
         means a typed question got a command line instead of an answer. \
         Found: {sites:?}"
    );
    // The one caller, named rather than line-numbered: lines move.
    assert_eq!(
        sites[0].split(':').next().expect("the site names its file"),
        "turbogit-services/bulk_ops.rs",
        "the only thing allowed to run arbitrary git is the custom-command bulk \
         operation, whose product genuinely is that",
    );
}

#[test]
fn its_owner_is_the_custom_command_arm() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(root.join("crates/turbogit-services/src/bulk_ops.rs"))
        .expect("bulk_ops.rs readable");
    let at = source
        .find(".run_raw(")
        .expect("the one call site is still there");
    // The call must sit inside the `BulkOp::Custom` arm — the name is what the
    // contract on the port points at.
    let arm = source[..at]
        .rsplit_once("BulkOp::Custom")
        .map(|(_, rest)| rest.to_string())
        .expect("the call follows the Custom arm");
    assert!(
        !arm.contains("BulkOp::"),
        "no other arm reached the raw escape:\n{arm}"
    );
    assert!(
        arm.contains("parse_command"),
        "the Custom arm still runs the typed text through the parser that owns \
         the leading-`git` rule:\n{arm}"
    );
}

/// The other half of the same rule: the monitor's echo is produced by the
/// parser's own function, not by a second reading of the typed text.
#[test]
fn the_run_view_asks_the_parser_how_to_echo() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let view = std::fs::read_to_string(root.join("crates/turbogit-app/src/bulk_run_view.rs"))
        .expect("bulk_run_view.rs readable");
    assert!(
        view.contains("bulk_ops::echo_command"),
        "the monitor echoes a custom command through the rule that owns it"
    );
    assert!(
        !view.contains("starts_with(\"git \")"),
        "the view re-derived the leading-`git` rule again, and the two readings \
         can disagree about what the engine was given"
    );
}

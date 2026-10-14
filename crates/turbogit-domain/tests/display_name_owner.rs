//! The path display-name owner: what a path calls itself, and the two ways a
//! caller can be wrong about a name that is not UTF-8.
//!
//! The owner is the answer thirty-odd surfaces used to re-derive by hand, and
//! the parameter it exists for is the *placeholder*, not the rule: a surface
//! chooses which sentence it shows when a path has no name, and that choice
//! belongs to the surface. What belongs here is the one question they all asked
//! — the final component, or the fallback — and the one thing they disagreed
//! about, which is whether a non-UTF-8 component survives. Every assertion is
//! about the string a caller receives, because a test that failed when a
//! closure became an argument would have tested the refactor.

use std::path::{MAIN_SEPARATOR_STR, Path, PathBuf};
use std::sync::Arc;

use turbogit_domain::model::{DisplayName, RootId};

/// A path with no final component at all — the filesystem root, spelled with
/// the platform's own separator so the shape is the one every platform has.
fn nameless() -> PathBuf {
    PathBuf::from(MAIN_SEPARATOR_STR)
}

/// A directory whose name is not valid UTF-8, built from raw bytes on the
/// platform's `OsStr` extension. A string literal cannot hold one: the loss
/// under test *is* the byte sequence no literal can spell.
#[cfg(unix)]
fn non_utf8_named() -> PathBuf {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    PathBuf::from(OsStr::from_bytes(b"/w/bad\xffname"))
}

/// The same shape on Windows, where an unpaired surrogate is well-formed UTF-16
/// and not UTF-8 — the platform's own way of naming a directory no text can.
#[cfg(windows)]
fn non_utf8_named() -> PathBuf {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt as _;

    // `\w\` then 0xD800: a high surrogate with no low surrogate after it.
    PathBuf::from(OsString::from_wide(&[0x5C, 0x77, 0x5C, 0xD800]))
}

#[test]
fn a_named_path_returns_its_final_component_from_both_entries() {
    let path = Path::new("/w/ui");
    assert_eq!(path.display_name_or("?"), "ui");
    assert_eq!(path.display_name_lossy_or("?"), "ui");
}

#[test]
fn a_path_with_no_final_component_returns_the_supplied_fallback() {
    let path = nameless();
    // Two fallbacks, two answers: the parameter is the whole of what the caller
    // decides, and a caller that passes its own wording gets its own wording.
    assert_eq!(path.display_name_or("<workspace>"), "<workspace>");
    assert_eq!(path.display_name_lossy_or("<repo>"), "<repo>");
    assert_eq!(path.display_name_or("<repo>"), "<repo>");
    assert_ne!(
        path.display_name_or("<repo>"),
        path.display_name_or("Project")
    );
}

#[test]
fn one_path_kept_by_the_lossy_entry_and_replaced_by_the_strict_one() {
    let path = non_utf8_named();

    // Kept, mangled but present: the fourteen sites that decode rather than
    // drop keep a name the user can still recognise.
    let lossy = path.display_name_lossy_or("?");
    assert!(
        lossy.contains("bad"),
        "the name survives, mangled: {lossy:?}"
    );
    assert!(
        lossy.contains('\u{FFFD}'),
        "mangled, not silently repaired: {lossy:?}"
    );

    // Replaced, for the same path, on the same call shape: the sixteen sites
    // that ask for text show their placeholder instead. One input, two facts —
    // the contrast is the reason both entries exist.
    assert_eq!(path.display_name_or("?"), "?");

    // The two entries are answers to different questions, not a faster and a
    // slower spelling of one.
    assert_ne!(lossy, path.display_name_or("?"));
}

#[test]
fn root_id_name_keeps_the_whole_path_as_its_fallback() {
    let named = RootId(Arc::from(PathBuf::from("/w/ui")));
    assert_eq!(named.name(), "ui");

    // The fallback is `RootId`'s own, not a caller's: seventeen existing callers
    // depend on a nameless root still printing its whole path.
    let nameless_root = RootId(Arc::from(nameless()));
    assert_eq!(
        nameless_root.name(),
        nameless_root.as_path().to_string_lossy()
    );
    assert_eq!(nameless_root.name(), "/".to_string());
}

#[test]
fn utf8_name_reports_presence_rather_than_a_label() {
    let named = Path::new("/w/ui");
    assert_eq!(named.utf8_name(), Some("ui"));
    assert_eq!(nameless().utf8_name(), None);
    // Borrowed, not a fresh `String`: the two sites that consume this `None`
    // as "no readable name" are consuming presence, and an allocated label
    // could not tell them apart from a real name.
    assert_eq!(non_utf8_named().utf8_name(), None);

    let buf = PathBuf::from("/w/ui");
    assert_eq!(buf.utf8_name(), Some("ui"));
    assert_eq!(buf.display_name_or("?"), "ui");
    assert_eq!(buf.display_name_lossy_or("?"), "ui");

    let root = RootId(Arc::from(PathBuf::from("/w/ui")));
    assert_eq!(root.utf8_name(), Some("ui"));
    assert_eq!(root.display_name_or("<repo>"), "ui");
    assert_eq!(root.display_name_lossy_or("<repo>"), "ui");
}

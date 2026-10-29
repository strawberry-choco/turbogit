use std::path::{MAIN_SEPARATOR_STR, Path, PathBuf};
use std::sync::Arc;

use turbogit_domain::model::{DisplayName, RootId};

fn nameless() -> PathBuf {
    PathBuf::from(MAIN_SEPARATOR_STR)
}

#[cfg(unix)]
fn non_utf8_named() -> PathBuf {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;

    PathBuf::from(OsStr::from_bytes(b"/w/bad\xffname"))
}

#[cfg(windows)]
fn non_utf8_named() -> PathBuf {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt as _;

    PathBuf::from(OsString::from_wide(&[
        0x5C, 0x77, 0x5C, 0x62, 0x61, 0x64, 0xD800,
    ]))
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

    let lossy = path.display_name_lossy_or("?");
    assert!(
        lossy.contains("bad"),
        "the name survives, mangled: {lossy:?}"
    );
    assert!(
        lossy.contains('\u{FFFD}'),
        "mangled, not silently repaired: {lossy:?}"
    );

    assert_eq!(path.display_name_or("?"), "?");

    assert_ne!(lossy, path.display_name_or("?"));
}

#[test]
fn root_id_name_keeps_the_whole_path_as_its_fallback() {
    let named = RootId(Arc::from(PathBuf::from("/w/ui")));
    assert_eq!(named.name(), "ui");

    let nameless_root = RootId(Arc::from(nameless()));
    assert_eq!(
        nameless_root.name(),
        nameless_root.as_path().to_string_lossy()
    );
    assert_eq!(nameless_root.name(), MAIN_SEPARATOR_STR.to_string());
}

#[test]
fn utf8_name_reports_presence_rather_than_a_label() {
    let named = Path::new("/w/ui");
    assert_eq!(named.utf8_name(), Some("ui"));
    assert_eq!(nameless().utf8_name(), None);
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

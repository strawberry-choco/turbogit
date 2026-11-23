use std::path::{Path, PathBuf};

use test_support::srcscan::{self, Strings};

fn ui_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../turbogit-ui/src")
}

fn ui_source(rel: &str) -> String {
    srcscan::read_source(ui_src().join(rel))
}

fn real_line(rel: &str, needle: &str) -> usize {
    let src = ui_source(rel);
    src.lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{rel} no longer contains {needle:?}"))
        + 1
}

#[test]
fn it_finds_a_named_symbol_in_a_named_file() {
    let row = ui_source("ui/row.rs");
    let declared = srcscan::declared_fns(&row);
    assert!(
        declared.contains(&"state_of".to_owned()),
        "`state_of` is declared in ui/row.rs today; the scanner found {declared:?}"
    );

    let at = real_line("ui/row.rs", "pub fn state_of(");
    let line = row.lines().nth(at - 1).expect("the declaration line");
    assert!(
        srcscan::declares(line, "state_of"),
        "line {at} of ui/row.rs declares `state_of`: {line}"
    );
    assert!(
        !srcscan::declares(line, "state"),
        "`state` is not declared on ui/row.rs's `state_of` line"
    );

    let body = srcscan::fn_code(&ui_source("ui/card.rs"), "card");
    assert!(
        body.contains("impl IntoElement"),
        "`card` returns an element; the body the scanner returned does not say \
         so:\n{body}"
    );
    assert!(
        body.trim_end().ends_with('}'),
        "`card`'s body was read to its closing brace:\n{body}"
    );

    assert!(
        !srcscan::word_offsets(&ui_source("ui/mod.rs"), "row").is_empty(),
        "ui/mod.rs re-exports `row`"
    );

    let sites = srcscan::call_sites(ui_src(), "state_of");
    assert!(
        sites.len() >= 2,
        "`state_of` is called from at least two places; the scanner found {sites:?}"
    );
    assert!(
        srcscan::fns_using(&row, "RowState").contains(&"row_fill".to_owned()),
        "`row_fill` reads `RowState`"
    );
}

#[test]
fn it_does_not_find_a_name_that_was_never_declared() {
    let never = "turbogit_git_state_of_that_was_never_written";
    let row = ui_source("ui/row.rs");
    assert!(
        !srcscan::declared_fns(&row).iter().any(|name| name == never),
        "a name that was never declared came back as a declaration"
    );
    assert!(
        srcscan::items_naming(&row, never).is_empty(),
        "a name that was never declared matched a module item"
    );
    assert!(
        srcscan::fns_using(&row, never).is_empty(),
        "a name that was never declared matched a function"
    );
    assert!(
        srcscan::call_sites(ui_src(), never).is_empty(),
        "a name that was never declared matched a call site"
    );
    assert!(
        !srcscan::declared_fns(&row).contains(&"card".to_owned()),
        "`card` belongs to ui/card.rs, not to the row module"
    );
}

#[test]
fn the_projection_preserves_every_line_and_every_column() {
    for rel in [
        "theme.rs",
        "ui/button.rs",
        "ui/row.rs",
        "ui/mod.rs",
        "ui/card.rs",
        "screens/log.rs",
        "screens/shell.rs",
        "screens/settings.rs",
    ] {
        let src = ui_source(rel);
        for mode in [Strings::Keep, Strings::Blank] {
            let blanked = srcscan::blank_comments(&src, mode);
            assert_eq!(
                blanked.chars().count(),
                src.chars().count(),
                "{rel} in {mode:?} changed length; every column after the first \
                 masked byte would be wrong"
            );
            assert_eq!(
                blanked.lines().count(),
                src.lines().count(),
                "{rel} in {mode:?} changed its line count"
            );
        }
    }
}

#[test]
fn a_block_comment_opener_inside_a_string_literal_is_not_one() {
    let rel = "screens/settings.rs";
    let at = real_line(rel, r#""release/*""#);
    let src = ui_source(rel);

    let later = real_line(rel, "fn appearance_page(");
    assert!(
        later > at,
        "the probe is only meaningful while `appearance_page` is after the string"
    );
    let line = src.lines().nth(later - 1).expect("the later declaration");
    assert!(
        srcscan::declares(line, "appearance_page"),
        "`appearance_page` is declared at {rel}:{later} and the scanner still sees \
         it although screens/settings.rs:{at} holds `\"release/*\"`"
    );
    for (mode, what) in [
        (Strings::Keep, "code_with_literals"),
        (Strings::Blank, "code_only"),
    ] {
        let blanked = srcscan::blank_comments(&src, mode);
        assert!(
            blanked.contains("fn appearance_page("),
            "{what} lost `fn appearance_page(` after the `\"release/*\"` literal at \
             {rel}:{at}"
        );
    }
}

#[test]
fn a_line_comment_opener_inside_a_string_literal_is_not_one() {
    const FIXTURE: &str = "\
fn hint() -> &'static str {
    \"https://example.com\"
}
fn after_the_literal() -> u8 { 7 }
";
    let blanked = srcscan::code_with_literals(FIXTURE);
    assert!(
        blanked.contains("\"https://example.com\""),
        "a `//` inside a string literal is not a comment: {blanked}"
    );
    assert!(
        blanked.contains("fn after_the_literal("),
        "the declaration after the literal survived: {blanked}"
    );
}

#[test]
fn a_lifetime_is_not_a_char_literal() {
    let rel = "screens/log.rs";
    let src = ui_source(rel);
    let at = real_line(rel, "fn find_commit<'a>(");
    assert!(
        src.lines()
            .nth(at - 1)
            .expect("the signature")
            .contains("<'a>"),
        "screens/log.rs:{at} still carries the `'a` lifetime"
    );
    let blanked = srcscan::blank_lines(&src, Strings::Blank);
    for (name, needle) in [
        ("bind_log_keys", "fn bind_log_keys("),
        ("move_selection", "fn move_selection("),
    ] {
        let line = real_line(rel, needle);
        assert!(
            blanked[line - 1].contains(name),
            "`{name}` is declared at {rel}:{line}, well after the `'a` lifetime on \
             {rel}:{at}; a lifetime must not open a char literal"
        );
    }
}

#[test]
fn a_byte_char_literal_is_not_a_comment_or_a_string() {
    const FIXTURE: &str = "\
fn shift(c: char) -> char {
    match c {
        b'M' => b'L',
        _ => c,
    }
}
fn sample_cubic() -> u8 { 7 }
";
    for (mode, what) in [
        (Strings::Keep, "code_with_literals"),
        (Strings::Blank, "code_only"),
    ] {
        let blanked = srcscan::blank_comments(FIXTURE, mode);
        assert!(
            blanked.contains("fn sample_cubic("),
            "{what} lost `fn sample_cubic(` after the byte char literals"
        );
    }
    assert!(
        srcscan::code_only(FIXTURE).contains("b'M' => b'L',"),
        "the implicit-repetition arm survived blanking"
    );
}

#[test]
fn a_raw_string_body_is_not_scanned() {
    const FIXTURE: &str = r####"
const BODY: &str = r#"a "quoted" // not a comment
/* not a comment either */ still " code "#
pub fn declared_after_the_raw_string() -> u8 { 7 }
"####;
    let lines = srcscan::blank_lines(FIXTURE, Strings::Keep);
    assert!(
        lines[3].contains("pub fn declared_after_the_raw_string("),
        "the declaration after a raw string is found: {:?}",
        lines
    );
    let blanked = srcscan::code_only(FIXTURE);
    assert!(
        !blanked.contains("not a comment"),
        "comment-looking text inside a raw string is still a string literal: \
         {blanked}"
    );
    assert!(
        !blanked.contains("quoted"),
        "the raw string's own content is not code: {blanked}"
    );
}

#[test]
fn a_lifetime_beside_a_comment_is_not_a_char_literal() {
    const FIXTURE: &str = "\
/// takes a lifetime
pub fn borrowed<'a>(rows: &mut Vec<&'a str>) -> usize {
    let first: &'a str = rows[0]; // borrow the head
    let n = first.len();
    n
}
pub fn after_the_comment() -> u8 {
    7
}
";
    let lines = srcscan::blank_lines(FIXTURE, Strings::Keep);
    assert!(
        lines[1].contains("pub fn borrowed"),
        "the declaration carrying the `<'a>` lifetime survived: {:?}",
        lines[1]
    );
    assert!(
        lines[2].contains("rows[0];"),
        "the code before the comment on the lifetime's own line survived: {:?}",
        lines[2]
    );
    assert!(
        !lines[2].contains("borrow the head"),
        "the comment on the same line as the lifetime is still a comment: {:?}",
        lines[2]
    );
    assert!(
        !lines[0].contains("takes a lifetime"),
        "the doc comment is blanked"
    );
    let names: Vec<String> = srcscan::top_level_items(FIXTURE)
        .into_iter()
        .map(|item| item.name)
        .collect();
    assert_eq!(names, ["borrowed", "after_the_comment"]);
    assert!(
        srcscan::code_only(FIXTURE).contains("let n = first.len();"),
        "the statement after the commented line is code, not literal content"
    );
}

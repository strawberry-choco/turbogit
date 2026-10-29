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
    let rows = ui_source("ui/widgets/rows.rs");
    let declared = srcscan::declared_fns(&rows);
    assert!(
        declared.contains(&"paint_row".to_owned()),
        "`paint_row` is declared in ui/widgets/rows.rs today; the scanner found \
         {declared:?}"
    );

    let at = real_line("ui/widgets/rows.rs", "pub fn paint_row(");
    let line = rows.lines().nth(at - 1).expect("the declaration line");
    assert!(
        srcscan::declares(line, "paint_row"),
        "line {at} of ui/widgets/rows.rs declares `paint_row`: {line}"
    );
    assert!(
        !srcscan::declares(line, "paint_rail"),
        "`paint_rail` is not declared on ui/widgets/rows.rs's `paint_row` line"
    );

    let body = srcscan::fn_code(&ui_source("ui/components.rs"), "row_shell");
    assert!(
        body.contains("rect: egui::Rect"),
        "`row_shell` takes the caller's rect; the body the scanner returned does \
         not say so:\n{body}"
    );
    assert!(
        body.trim_end().ends_with('}'),
        "`row_shell`'s body was read to its closing brace:\n{body}"
    );

    assert!(
        !srcscan::word_offsets(&ui_source("ui/widgets/mod.rs"), "paint_row").is_empty(),
        "ui/widgets/mod.rs re-exports `paint_row`"
    );

    let sites = srcscan::call_sites(ui_src(), "paint_row");
    assert!(
        sites.len() >= 2,
        "`paint_row` is called from at least two screens; the scanner found {sites:?}"
    );
    assert!(
        srcscan::fns_using(&rows, "RowState").contains(&"paint_row".to_owned()),
        "`paint_row` reads `RowState`"
    );
}

#[test]
fn it_does_not_find_a_name_that_was_never_declared() {
    let never = "turbogit_git_paint_row_that_was_never_written";
    let rows = ui_source("ui/widgets/rows.rs");
    assert!(
        !srcscan::declared_fns(&rows)
            .iter()
            .any(|name| name == never),
        "a name that was never declared came back as a declaration"
    );
    assert!(
        srcscan::items_naming(&rows, never).is_empty(),
        "a name that was never declared matched a module item"
    );
    assert!(
        srcscan::fns_using(&rows, never).is_empty(),
        "a name that was never declared matched a function"
    );
    assert!(
        srcscan::call_sites(ui_src(), never).is_empty(),
        "a name that was never declared matched a call site"
    );
    assert!(
        !srcscan::declared_fns(&rows).contains(&"row_shell".to_owned()),
        "`row_shell` belongs to ui/components.rs, not to the row module"
    );
}

#[test]
fn the_projection_preserves_every_line_and_every_column() {
    for rel in [
        "theme.rs",
        "ui/components.rs",
        "ui/log_window.rs",
        "ui/blame_view.rs",
        "ui/settings_modal.rs",
        "ui/icons.rs",
        "ui/widgets/rows.rs",
        "ui/widgets/mod.rs",
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
    let rel = "ui/settings_modal.rs";
    let at = real_line(rel, r#""release/*""#);
    let src = ui_source(rel);

    let later = real_line(rel, "fn pattern_chip(");
    assert!(
        later > at,
        "the probe is only meaningful while `pattern_chip` is after the string"
    );
    let line = src.lines().nth(later - 1).expect("the later declaration");
    assert!(
        srcscan::declares(line, "pattern_chip"),
        "`pattern_chip` is declared at {rel}:{later} and the scanner still sees it \
         although ui/settings_modal.rs:{at} holds `\"release/*\"`"
    );
    for (mode, what) in [
        (Strings::Keep, "code_with_literals"),
        (Strings::Blank, "code_only"),
    ] {
        let blanked = srcscan::blank_comments(&src, mode);
        assert!(
            blanked.contains("fn pattern_chip("),
            "{what} lost `fn pattern_chip(` after the `\"release/*\"` literal at \
             {rel}:{at}"
        );
    }
}

#[test]
fn a_line_comment_opener_inside_a_string_literal_is_not_one() {
    let rel = "ui/remotes_dialog.rs";
    let at = real_line(rel, "https://");
    let src = srcscan::code_with_literals(&ui_source(rel));
    assert!(
        src.lines()
            .nth(at - 1)
            .expect("the hint line")
            .contains(r#".hint_text("https://…")"#),
        "ui/remotes_dialog.rs:{at} still holds its whole hint literal after \
         comment blanking; a `//` inside a string literal is not a comment, and a \
         scanner that believed it would blank the closing quote too"
    );
}

#[test]
fn a_lifetime_is_not_a_char_literal() {
    let rel = "ui/branch_tree_view.rs";
    let src = ui_source(rel);
    let at = real_line(rel, "pub fn branch_tree(");
    assert!(
        src.lines()
            .nth(at - 1)
            .expect("the signature")
            .contains("<'_>"),
        "ui/branch_tree_view.rs:{at} still carries the `'_` lifetime"
    );
    let blanked = srcscan::blank_lines(&src, Strings::Blank);
    for (name, needle) in [
        ("keyboard_order", "pub fn keyboard_order<'a>("),
        ("warm_tags", "pub fn warm_tags("),
    ] {
        let line = real_line(rel, needle);
        assert!(
            blanked[line - 1].contains(name),
            "`{name}` is declared at {rel}:{line}, well after the `'_` lifetime on \
             {rel}:{at}; a lifetime must not open a char literal"
        );
    }
    let ch = real_line(rel, "std::iter::once(");
    assert!(
        blanked[ch - 1].contains("std::iter::once("),
        "the char literal at {rel}:{ch} is still code after blanking"
    );
}

#[test]
fn a_byte_char_literal_is_not_a_comment_or_a_string() {
    let rel = "ui/icons.rs";
    let at = real_line(rel, "let mut last_cmd = b'");
    let src = ui_source(rel);
    let later = real_line(rel, "fn sample_cubic(");
    assert!(later > at, "the probe is only meaningful while it is after");
    for (mode, what) in [
        (Strings::Keep, "code_with_literals"),
        (Strings::Blank, "code_only"),
    ] {
        let blanked = srcscan::blank_comments(&src, mode);
        assert!(
            blanked.contains("fn sample_cubic("),
            "{what} lost `fn sample_cubic(` after the byte char literals at \
             {rel}:{at}"
        );
    }
    assert!(
        srcscan::code_only(&src).contains("b'M' => b'L',"),
        "the implicit-repetition arm survived blanking"
    );
}

#[test]
fn a_raw_string_body_is_not_scanned() {
    let real = Path::new(env!("CARGO_MANIFEST_DIR")).join("../turbogit-ui/tests/design_tokens.rs");
    let src = srcscan::read_source(&real);
    let open = src
        .lines()
        .position(|l| l.contains("r#\""))
        .expect("design_tokens.rs carries a raw string")
        + 1;
    let blanked = srcscan::blank_lines(&src, Strings::Keep);
    for (what, line) in [
        ("the closing delimiter", "restore_workspace: false,"),
        ("the const's own terminator", "\"#;"),
        (
            "the declaration after it",
            "fn legacy_state_with_removed_theme_mode_still_loads(",
        ),
    ] {
        let at = src
            .lines()
            .position(|l| l.contains(line))
            .unwrap_or_else(|| panic!("design_tokens.rs no longer contains {line:?}"))
            + 1;
        assert!(
            blanked[at - 1].contains(line.trim()),
            "{what} at design_tokens.rs:{at} survived the raw string opened at \
             design_tokens.rs:{open}"
        );
    }

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

//! The source scanner's own self-check.
//!
//! The ratchets `srcscan` exists for are almost all `assert!(!scan.contains(x))`
//! or `assert_eq!(items.len(), 1)` over a set the scanner produced. **An empty
//! set satisfies every one of them.** A stripper with a bug that blanks a whole
//! file therefore produces a green run, and a green run is the *dangerous*
//! outcome here rather than the reassuring one: the mutation that motivated this
//! module replaced `CommitTable::ROW_HEIGHT` with a literal `20.0` while the file
//! stayed full of prose naming it, and the broken scan passed the ratchet meant
//! to catch exactly that.
//!
//! So this suite is deliberately two-sided:
//!
//! - **The positive halves** name a symbol that exists *right now* in a file that
//!   exists *right now*, and fail if the scanner cannot see it. These are the
//!   halves that catch a broken stripper; a scanner that matches nothing is red
//!   here, which is the whole point.
//! - **The negative half** names a symbol that was never declared, so the
//!   positive halves cannot be satisfied by a scan that returns everything.
//!
//! The hard-input cases are pinned to **real lines in this repository**, not to
//! an invented fixture: a synthetic one is easier than the thing that broke the
//! parser, which is the whole reason the four private copies this module
//! replaced each made different trade-offs. Where the tree has no occurrence the
//! test says so rather than pretending otherwise — see
//! [`a_lifetime_beside_a_comment_is_not_a_char_literal`].

use std::path::{Path, PathBuf};

use test_support::srcscan::{self, Strings};

/// The UI crate's `src` tree — what the structural ratchets read.
fn ui_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../turbogit-ui/src")
}

fn ui_source(rel: &str) -> String {
    srcscan::read_source(ui_src().join(rel))
}

/// The 1-based line `rel` really is, so a test that names a line fails loudly
/// when the file moves instead of silently scanning something else.
fn real_line(rel: &str, needle: &str) -> usize {
    let src = ui_source(rel);
    src.lines()
        .position(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("{rel} no longer contains {needle:?}"))
        + 1
}

// --- the positive half: a named symbol in a named file ------------------------

/// **The half that catches a broken stripper.** `paint_row` is the shared row
/// module's one public primitive and `row_shell` is the one painter the log's
/// rows and `paint_row` share; both exist today, and the structural ratchets in
/// `branch_component_kit.rs` name both. A scanner that cannot find them is
/// broken, whatever its negative assertions say.
#[test]
fn it_finds_a_named_symbol_in_a_named_file() {
    // 1. The declaration, read out of the module that owns it.
    let rows = ui_source("ui/widgets/rows.rs");
    let declared = srcscan::declared_fns(&rows);
    assert!(
        declared.contains(&"paint_row".to_owned()),
        "`paint_row` is declared in ui/widgets/rows.rs today; the scanner found \
         {declared:?}"
    );

    // 2. The same fact, per line, against the real declaration line.
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

    // 3. A function body, brace-counted from its declaration — the seam the
    //    "the row shell never measures a rect" ratchet is made against.
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

    // 4. The façade's re-export — a `pub use`, which is neither a `pub fn` head
    //    nor a `const` head, so the item walk cannot see it; the whole-identifier
    //    query is what reads a re-export list.
    assert!(
        !srcscan::word_offsets(&ui_source("ui/widgets/mod.rs"), "paint_row").is_empty(),
        "ui/widgets/mod.rs re-exports `paint_row`"
    );

    // 5. The call sites, over the whole tree — the query the "at least two
    //    screens" rule is written in.
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

// --- the negative half: a name that was never declared -----------------------

/// **The half that stops the positive halves from being satisfied by a scan that
/// returns everything.** A name that does not exist anywhere in the workspace
/// must come back empty; if it does not, every "exactly one function produces
/// this" ratchet built on this module is measuring nothing.
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
    // `row_shell` really does exist — in `ui/components.rs`, not in the row
    // module — so this is a scoping assertion rather than a global one.
    assert!(
        !srcscan::declared_fns(&rows).contains(&"row_shell".to_owned()),
        "`row_shell` belongs to ui/components.rs, not to the row module"
    );
}

// --- the column-preservation promise ------------------------------------------

/// **The projection is the same length as its input, in both modes.** This is the
/// invariant the whole column story rests on — a masked byte becomes a space, so
/// a line number and a column offset in a scan's output are the caller's to
/// report — and it is the one thing no `contains` assertion can see.
///
/// It earns its place by having been false: a version of this module copied the
/// whole tail of the file after every string literal, which every "does it
/// contain" assertion in the tree was perfectly happy with and which silently
/// duplicated the colour-literal table a ratchet counts. A length check is the
/// cheapest possible detector of that class.
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

// --- the stripper's hardest real inputs ---------------------------------------

/// **`"release/*"` — a block-comment opener with no closer, inside a string
/// literal.** `ui/settings_modal.rs` really does carry
/// `.hint_text("release/*")`, and a stripper that opens a block comment there
/// blanks the remaining 250 lines of the file: every declaration after it
/// disappears, and every negative assertion over the file becomes true for the
/// uninteresting reason that nothing is in it. This is the single most
/// destructive thing the module has to get right.
#[test]
fn a_block_comment_opener_inside_a_string_literal_is_not_one() {
    let rel = "ui/settings_modal.rs";
    let at = real_line(rel, r#""release/*""#);
    let src = ui_source(rel);

    // The declaration after it, by name, from the same file.
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
    // And the projection both ways: neither mode may eat the tail of the file.
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

/// **`"https://…"` — a line-comment opener inside a string literal**, in
/// `ui/remotes_dialog.rs`. Weaker than the case above (a stray line comment only
/// eats the tail of one line) but it is the shape that hides a closing quote,
/// and with a string-oblivious stripper the literal is left unterminated, so
/// everything *after* it on that line is masked too.
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

/// **`&TreeProps<'_>` is a lifetime, not a char literal.** `ui/branch_tree_view.rs`
/// carries `'_` on the signature of `pub fn branch_tree` (line 170) and `<'a>` on
/// `pub fn keyboard_order` (line 1353). `'_` is *syntactically* a char literal,
/// so a scanner that decides on the opening quote alone and then scans for a
/// closing one would treat everything between them as literal content — and a
/// mode that blanks literal content would then blank 1,200 lines of real code.
/// Both late declarations have to survive the blanking for that to be caught.
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
    // The char literal that *is* one, four hundred lines down, is still code
    // rather than the start of a string: `once('…')` in the same file.
    let ch = real_line(rel, "std::iter::once(");
    assert!(
        blanked[ch - 1].contains("std::iter::once("),
        "the char literal at {rel}:{ch} is still code after blanking"
    );
}

/// **`b'\0'` and `b'M' => b'L'` in `ui/icons.rs`.** Byte char literals with an
/// escape and a run of plain ones, interleaved with `//` comments. A scanner
/// that mistakes a `'` for a string opener loses the rest of the file to the
/// next `"`, and this is the file in the tree where that would happen.
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
    // The mapping arms are a run of four char literals on four lines; a scanner
    // that loses track of them loses the arm they belong to.
    assert!(
        srcscan::code_only(&src).contains("b'M' => b'L',"),
        "the implicit-repetition arm survived blanking"
    );
}

/// **A raw string whose body is not Rust.** `crates/turbogit-ui/tests/design_tokens.rs`
/// carries a real `r#"…"#` literal — a `state.ron` document, full of quotes of
/// its own — and the code that reads it starts on the very next line. A scanner
/// that does not know `r#"` opens a string at its `"` and then pairs the
/// remaining quotes off by accident, so what has to be checked is the tail.
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

    // The destructive shape, which the tree has no occurrence of: an odd number
    // of `"` inside the raw body, plus comment-looking text the scanner must not
    // act on. Spelled out rather than invented into a file, because the point is
    // the stripper's behaviour and not where the bytes came from.
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

/// **A lifetime beside a comment on one line.** The tree has no line that puts
/// both — every `&'a` in `crates/turbogit-ui/src` is followed by code, not by a
/// `//` — so this one case is spelled out rather than pinned, and the only
/// reason to include it is that it is the combination a lifetime/char-literal
/// bug produces: a scanner that consumed the `//` as literal content, or opened
/// a comment on a `'`.
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
    // The item walk reads heads by prefix, so it names both declarations and
    // spans the first through its column-0 `}`.
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

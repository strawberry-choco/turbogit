use turbogit_domain::git_path_quoting::{
    QuoteTrigger, escape, escape_c, find_quote, find_quote_c, needs_quote, unescape, unescape_c,
};

const RAW: &[&str] = &[
    "a/README.md",
    "a/two words/it is.txt",
    "a/we\ttabbed.txt",
    "a/we\"quoted.txt",
    "a/we\\ird.txt",
    "a/café.txt",
];

const QUOTED: &[(&str, &str)] = &[
    (r"a/we\ttabbed.txt", "a/we\ttabbed.txt"),
    (r#"a/we\"quoted.txt"#, "a/we\"quoted.txt"),
    (r"a/we\\ird.txt", "a/we\\ird.txt"),
    (r"a/caf\303\251.txt", "a/café.txt"),
];

const TABLE: &[(u8, bool, bool)] = &[
    (0x00, true, true),
    (0x1f, true, true),
    (0x20, false, true),
    (0x7e, false, false),
    (0x7f, true, false),
    (0x80, true, false),
    (0xff, true, false),
    (b'"', true, true),
    (b'\\', true, false),
    (b'\t', true, true),
    (b'a', false, false),
    (b'z', false, false),
];

#[test]
fn each_policy_has_an_answer_for_each_interesting_byte() {
    for (byte, git, pair) in TABLE {
        assert_eq!(
            needs_quote(&[*byte], QuoteTrigger::GitSpelling),
            *git,
            "git's own spelling, byte {byte:#04x}"
        );
        assert_eq!(
            needs_quote(&[*byte], QuoteTrigger::HeaderPair),
            *pair,
            "the header rewriter's two-token line, byte {byte:#04x}"
        );
    }
}

#[test]
fn the_two_policies_disagree_on_the_space_the_backslash_and_the_bytes_above_ascii() {
    let disagree: Vec<u8> = (0u8..=0xff)
        .filter(|&b| {
            needs_quote(&[b], QuoteTrigger::GitSpelling)
                != needs_quote(&[b], QuoteTrigger::HeaderPair)
        })
        .collect();
    assert_eq!(
        disagree,
        [0x20, 0x5c]
            .into_iter()
            .chain(0x7f..=0xff)
            .collect::<Vec<u8>>(),
        "a space, a backslash, and everything from DEL up"
    );
}

#[test]
fn a_space_is_quoted_only_where_a_freshly_written_diff_git_line_is_written() {
    let spaced = b"a/two words/it is.txt";
    assert!(
        needs_quote(spaced, QuoteTrigger::HeaderPair),
        "the rewriter is writing a new two-token line, so its sides must still split"
    );
    assert!(
        !needs_quote(spaced, QuoteTrigger::GitSpelling),
        "git's own spelling leaves a plain space unquoted, and the value's writer matches git"
    );
}

#[test]
fn a_backslash_is_quoted_only_where_gits_own_spelling_is_reproduced() {
    let slashed = b"a/we\\ird.txt";
    assert!(
        needs_quote(slashed, QuoteTrigger::GitSpelling),
        "the value's writer escapes a backslash rather than writing it raw"
    );
    assert!(
        !needs_quote(slashed, QuoteTrigger::HeaderPair),
        "the rewriter leaves it raw — a preserved spelling, not an oversight"
    );
}

#[test]
fn a_non_ascii_byte_is_quoted_only_where_gits_own_spelling_is_reproduced() {
    let accented = b"a/caf\xc3\xa9.txt";
    assert!(
        needs_quote(accented, QuoteTrigger::GitSpelling),
        "git octal-escapes every byte outside printable ASCII"
    );
    assert!(
        !needs_quote(accented, QuoteTrigger::HeaderPair),
        "the rewriter passes the raw bytes through"
    );
}

#[test]
fn no_byte_means_no_trigger_under_either_policy() {
    for trigger in [QuoteTrigger::GitSpelling, QuoteTrigger::HeaderPair] {
        assert!(
            !needs_quote(b"", trigger),
            "a path with no bytes is decided by the caller's own rule"
        );
        assert!(
            !needs_quote(b"README.md", trigger),
            "and an ordinary repository-relative name is never quoted"
        );
    }
}

#[test]
fn the_escape_table_spells_the_bytes_git_spells_them() {
    for (byte, spelled) in [
        (0x00, &b"\\000"[..]),
        (b'\t', b"\\t"),
        (b'\n', b"\\n"),
        (b'\r', b"\\r"),
        (b'"', b"\\\""),
        (b'\\', b"\\\\"),
        (0x7f, b"\\177"),
        (0x80, b"\\200"),
        (0xff, b"\\377"),
    ] {
        assert_eq!(
            escape(&[byte]),
            spelled,
            "byte {byte:#04x} and its spelling"
        );
    }
    for literal in [0x20, 0x7e, b'a'] {
        assert_eq!(
            escape(&[literal]),
            vec![literal],
            "byte {literal:#04x} is inside the printable range and passes through"
        );
    }
}

#[test]
fn the_string_escape_is_the_byte_escape_converted() {
    for path in RAW {
        assert_eq!(
            escape_c(path),
            String::from_utf8(escape(path.as_bytes())).expect("the table is ASCII"),
            "{path:?}: the two entries disagree"
        );
    }
}

#[test]
fn the_string_unescape_is_the_byte_unescape_behind_the_lossy_boundary() {
    for path in RAW {
        assert_eq!(
            unescape_c(path),
            String::from_utf8_lossy(&unescape(path.as_bytes())),
            "{path:?}: the boundary is the conversion, and nothing else"
        );
    }
    for (quoted, name) in QUOTED {
        assert_eq!(
            unescape_c(quoted),
            *name,
            "{quoted:?}: git's spelling resolves to its own name"
        );
        assert_eq!(
            unescape(quoted.as_bytes()),
            name.as_bytes(),
            "{quoted:?}: and the byte entry resolves it exactly"
        );
    }
}

#[test]
fn the_byte_unescape_is_exact_where_the_string_one_is_lossy() {
    assert_eq!(
        unescape(br"a/caf\303\251.txt"),
        b"a/caf\xc3\xa9.txt",
        "a non-UTF-8 name comes back as itself"
    );
    assert_eq!(
        unescape_c(r"a/caf\303\251.txt"),
        "a/café.txt",
        "and as the text it spells, once decoded"
    );
    assert_eq!(
        unescape(br"\377"),
        vec![0xff],
        "a byte that is not text stays one byte — the grammar is exact"
    );
    assert_eq!(
        unescape_c(r"\377"),
        "\u{fffd}",
        "and only the &str boundary substitutes for it"
    );
}

#[test]
fn both_find_quote_entries_return_the_same_index() {
    for path in RAW {
        assert_eq!(
            find_quote(path.as_bytes()),
            find_quote_c(path),
            "{path:?}: the two entries disagree"
        );
    }
    assert_eq!(
        find_quote(br#"a/x.txt""#),
        find_quote_c(r#"a/x.txt""#),
        "the closing quote of a token is where the split is"
    );
    assert_eq!(find_quote(br#"a/x.txt""#), Some(7));
    assert_eq!(
        find_quote(br#"a/we\"quoted.txt"#),
        None,
        r#"a backslash escapes the quote that follows it, so the token has not ended"#
    );
}

#[test]
fn each_pair_round_trips_its_own_input() {
    for path in RAW {
        assert_eq!(
            unescape(&escape(path.as_bytes())),
            path.as_bytes(),
            "{path:?}: the byte pair is exact"
        );
        assert_eq!(
            unescape_c(&escape_c(path)),
            *path,
            "{path:?}: the String pair returns the name it was given"
        );
    }
    for byte in 0u8..=0xff {
        assert_eq!(
            unescape(&escape(&[byte])),
            vec![byte],
            "byte {byte:#04x} survives the byte pair whatever it is"
        );
    }
}

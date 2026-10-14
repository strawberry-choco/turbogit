//! git's C-style path quoting: the two policies, the bytes each lists, and the
//! bytes on which they disagree.
//!
//! The grammar is observable only as bytes in and bytes out, so every assertion
//! here is about the answer a caller receives — never about which function
//! produced it or which module a symbol lives in. That is also why the byte sets
//! have a suite of their own: nothing above the grammar asserts them today, which
//! is how a `needs_quote` and a rewriter's private `needs_quote` came to disagree
//! for years without either naming the other.

use turbogit_domain::git_path_quoting::{QuoteTrigger, needs_quote};

/// One byte and what each policy says about it: `(byte, GitSpelling, HeaderPair)`.
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

/// Every byte of a path a header line can carry, answered under both policies.
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

/// The disagreement set, so a future edit that moves a boundary — a space that
/// becomes a trigger everywhere, a backslash quietly added to one policy — fails
/// by name instead of passing as a rewrite.
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

/// A path is one byte past its trigger that forces quoting — the other policy
/// writes it plainly, on purpose.
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

/// The row the module doc records as a preserved defect: the rewriter does not
/// list `\`, so the backslash does not force quoting there.
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

/// A non-ASCII byte is the third of the three: octal-escaped under git's
/// spelling, passed through untouched by the rewriter, which is what keeps a
/// non-UTF-8 name itself rather than a rewritten one.
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

/// An empty path is not a quote trigger in either policy: a rewriter that needs
/// one for a two-token line asks separately, because the line must keep two
/// tokens rather than because a byte in it demands quoting.
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

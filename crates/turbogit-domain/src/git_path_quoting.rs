//! git's C-style path quoting: the grammar a header line is written with and
//! resolved with, held once for the **Patch value**'s writer, the port's patch
//! reader, and the `Create patch` header rewriter.
//!
//! # Two policies, one grammar
//!
//! "Must this path be quoted" is not one question in this workspace — it is two,
//! and they disagree. They are named here rather than unified, because each one
//! exists for a caller and a caller who does not know which is which will
//! "correct" the other:
//!
//! | policy | a plain space | a backslash | a non-ASCII byte | who writes it |
//! |---|---|---|---|---|
//! | [`QuoteTrigger::GitSpelling`] | no | yes | yes (octal) | the **Patch value**'s writer |
//! | [`QuoteTrigger::HeaderPair`] | yes | no | no (raw byte passes through) | the `Create patch` header rewriter |
//!
//! `GitSpelling` exists because the value's writer must reproduce git's own text
//! byte for byte: git does not quote a plain space, and a space forced to quote
//! would break the byte-exact re-render every patch shape is held to.
//!
//! `HeaderPair` exists because that writer is not reproducing a line — it is
//! writing a **new** `diff --git` line, whose two sides a reader recovers by
//! splitting on ` b/`. A base-path rewrite can introduce a space into a path that
//! had none, so quoting there is what keeps the line two tokens.
//!
//! # The third row: `HeaderPair` does not list `\`
//!
//! That omission is a **preserved defect owing a human ruling**, not an oversight.
//! The rewriter quotes a path only when the token was already quoted or when this
//! set says so, so a backslash-bearing path with no space is written unquoted and
//! the escape table's backslash arm is unreachable for the one case it exists for.
//! The raw backslash then reaches `git apply` on a header line whose reader treats
//! `\` as an escape introducer. The byte set is held as it is — as a named row,
//! not a special case in any scan — until that ruling lands. Making the space
//! trigger uniform in either direction is a second ruling on the same question:
//! which artifact is authoritative.
//!
//! # The byte level is the grammar; the `String` level is a boundary
//!
//! Each piece below has exactly one implementation, over `&[u8]`; the `&str`
//! entries delegate to it and add a conversion and nothing else. The byte level is
//! the grammar because a patch is bytes on the wire and `ADR-0013` holds a
//! rewritten header line to the byte — a non-UTF-8 name must survive as itself,
//! never as replacement characters.
//!
//! The `String` entries carry one deliberate lossiness, `from_utf8_lossy` over
//! octal-produced bytes, because that is what a `&str` reader has always been
//! handed. It is a property of that interface, not of the grammar: the byte entry
//! beside it is exact.
//!
//! There is deliberately **no `&str` quote-trigger predicate**: the value's
//! writer reaches [`needs_quote`] only through its quote-and-escape entry, so a
//! second `&str` entry would be an interface nobody calls.

/// Which spelling of git's quoting a caller writes.
///
/// A parameter rather than a flag: the two answers this replaces disagreed on a
/// backslash and on every non-ASCII byte, not only on a space, so a boolean
/// ("does a space count?") cannot carry either set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuoteTrigger {
    /// git's own spelling — what the **Patch value**'s writer must match byte for
    /// byte.
    GitSpelling,
    /// the rewriter's two-token line — what a freshly written `diff --git` line
    /// must survive.
    HeaderPair,
}

impl QuoteTrigger {
    /// Whether this policy's byte set lists `byte`. One row per policy, written
    /// once in the workspace.
    fn forces(self, byte: u8) -> bool {
        match self {
            // A control byte, a non-ASCII byte, or one of the two characters the
            // quoting itself uses; a plain space is deliberately absent.
            Self::GitSpelling => !(0x20..0x7f).contains(&byte) || matches!(byte, b'"' | b'\\'),
            // A space because a `diff --git` line is read as two tokens, a quote
            // because it ends a quoted one, a control byte because a line reader
            // would not treat it as part of a path — and a backslash is not listed,
            // which is the preserved row the module doc names.
            Self::HeaderPair => byte == b' ' || byte == b'"' || byte < 0x20,
        }
    }
}

/// Whether any byte of `path` forces quoting under `trigger`.
pub fn needs_quote(path: &[u8], trigger: QuoteTrigger) -> bool {
    path.iter().any(|&b| trigger.forces(b))
}

/// git's C-style quoting of a path, byte for byte — the header rewriter's entry.
pub fn escape(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 4);
    for &b in bytes {
        match b {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b' '..=b'~' => out.push(b),
            // Three octal digits, one per byte: that is how a non-ASCII name comes
            // back from a reader as itself rather than as Latin-1 mojibake.
            other => out.extend_from_slice(format!("\\{other:03o}").as_bytes()),
        }
    }
    out
}

/// git's C-style quoting undone, byte for byte — the header rewriter's entry.
pub fn unescape(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'\\' {
            out.push(bytes[at]);
            at += 1;
            continue;
        }
        at += 1;
        let Some(&esc) = bytes.get(at) else { break };
        at += 1;
        match esc {
            b'"' => out.push(b'"'),
            b'\\' => out.push(b'\\'),
            b't' => out.push(b'\t'),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            d @ b'0'..=b'7' => {
                let mut byte = d - b'0';
                for _ in 0..2 {
                    if matches!(bytes.get(at), Some(b'0'..=b'7')) {
                        byte = byte * 8 + (bytes[at] - b'0');
                        at += 1;
                    } else {
                        break;
                    }
                }
                out.push(byte);
            }
            // git writes no other escape, but a name it did not write is still a
            // name: the introducer goes and the byte it introduced stays.
            other => out.push(other),
        }
    }
    out
}

/// The index of the quote that ends a quoted token, or `None` if the token runs
/// to the end of the bytes.
pub fn find_quote(bytes: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// git's C-style quoting of a path as text — the **Patch value**'s writer's entry.
///
/// The conversion is lossless by construction: [`escape`] turns every byte outside
/// printable ASCII into an escape, so nothing else can reach it.
pub fn escape_c(s: &str) -> String {
    String::from_utf8(escape(s.as_bytes())).expect("escaped text is printable ASCII")
}

/// git's C-style quoting undone for a `&str` reader, which is handed one lossy
/// conversion — the exact bytes are [`unescape`]'s to give.
pub fn unescape_c(s: &str) -> String {
    String::from_utf8_lossy(&unescape(s.as_bytes())).into_owned()
}

/// [`find_quote`] over the bytes of a `&str` — the port reader's entry.
pub fn find_quote_c(s: &str) -> Option<usize> {
    find_quote(s.as_bytes())
}

/// git's quoted spelling of a path under `trigger`, or the path itself when no
/// byte of it forces quoting — the **Patch value**'s writer's entry.
pub fn quote_c(path: &str, trigger: QuoteTrigger) -> String {
    if needs_quote(path.as_bytes(), trigger) {
        format!("\"{}\"", escape_c(path))
    } else {
        path.to_string()
    }
}

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

//! The diff viewer's two view policies, computed over text (parity of the
//! reference's `Ignore Differences` and `Highlighting Differences`).
//!
//! Both are pure functions over the **Git engine**'s patch value, and neither one
//! touches it: what git answered stays exactly as git answered it, so the text
//! partial staging composes from is the same text whether the viewer is showing
//! `None` or `Ignore imports and formatting` (`ADR-0022`). What these functions
//! decide is only which lines the viewer *counts as different* and how finely a
//! differing line's highlight is cut up.
//!
//! That is why the policy cannot ride on the engine's `-w` family of flags here.
//! Two of the five values (line split/join, import-block changes) have no git
//! flag at all, and the in-process producer (`similar`) and the CLI producer do
//! not agree on git's whitespace flags value for value — so a flag-per-value
//! design would answer a different question depending on which producer ran. One
//! implementation, applied after the patch value settles, is the only shape where
//! all five values mean the same thing on every path.

use std::ops::Range;

use similar::{DiffTag, TextDiff};
use turbogit_domain::model::{DiffHighlighting, DiffWhitespace};

// --- whitespace --------------------------------------------------------------

/// Prefixes of a line that make it an **import statement**, for
/// [`DiffWhitespace::IgnoreImportsAndFormatting`]. The reference resolves this in
/// a Java PSI tree; TurboGit has no parser, so the shape is named from the line
/// instead — a conservative list of the forms the languages in this app's diff
/// actually write imports in, each requiring a space or a quote after the keyword
/// so an identifier that merely starts with the keyword is not swept in.
const IMPORT_PREFIXES: [&str; 9] = [
    "use ", "pub use ", "mod ", "import ", "from ", "#include", "#import", "@import", "require(",
];

/// Whether the line is an import statement.
fn is_import_line(line: &str) -> bool {
    let t = line.trim_start();
    IMPORT_PREFIXES.iter().any(|p| t.starts_with(p))
}

/// The comparable form of one line under [`DiffWhitespace::IgnoreImportsAndFormatting`]:
/// whitespace outside a string literal dropped, whitespace inside one kept — the
/// one value in the ladder that has to know what a literal is, because the
/// reference documents that its whitespace stays significant.
fn literal_aware_key(line: &str) -> String {
    let mut key = String::with_capacity(line.len());
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in line.chars() {
        match quote {
            Some(q) => {
                key.push(c);
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' | '`' => {
                    quote = Some(c);
                    key.push(c);
                }
                _ if c.is_whitespace() => {}
                _ => key.push(c),
            },
        }
    }
    key
}

/// The comparable form of one line under `ws`: whitespace folded away per the
/// policy.
fn whitespace_key(ws: DiffWhitespace, line: &str) -> String {
    match ws {
        DiffWhitespace::None => line.to_owned(),
        DiffWhitespace::TrimWhitespace => line.trim().to_owned(),
        DiffWhitespace::IgnoreWhitespace | DiffWhitespace::IgnoreWhitespaceAndEmptyLines => {
            line.chars().filter(|c| !c.is_whitespace()).collect()
        }
        DiffWhitespace::IgnoreImportsAndFormatting => literal_aware_key(line),
    }
}

/// The whole run's comparable form: every line's key concatenated, so a change
/// that only splits or joins lines — or only adds or removes lines of nothing —
/// has the same key on both sides.
fn run_key(ws: DiffWhitespace, lines: &[&str]) -> String {
    lines
        .iter()
        .map(|l| whitespace_key(ws, l))
        .collect::<String>()
}

/// Whether a changed run of `dels` against `adds` is **considered equal** under
/// the whitespace policy, and therefore stops being a difference in the view.
///
/// The two weaker values compare line by line, which is what makes them weaker:
/// `Trim whitespaces` cannot hide a run that adds a line of nothing, and neither
/// can hide a line that was split in two, because their non-whitespace content
/// only matches once the whole run is read as one string.
pub fn considered_equal(ws: DiffWhitespace, dels: &[&str], adds: &[&str]) -> bool {
    match ws {
        DiffWhitespace::None => false,
        DiffWhitespace::TrimWhitespace | DiffWhitespace::IgnoreWhitespace => {
            dels.len() == adds.len()
                && dels
                    .iter()
                    .zip(adds)
                    .all(|(d, a)| whitespace_key(ws, d) == whitespace_key(ws, a))
        }
        DiffWhitespace::IgnoreWhitespaceAndEmptyLines => run_key(ws, dels) == run_key(ws, adds),
        DiffWhitespace::IgnoreImportsAndFormatting => {
            run_key(ws, dels) == run_key(ws, adds)
                || (dels.iter().chain(adds).all(|l| is_import_line(l))
                    && !dels.is_empty()
                    && !adds.is_empty())
        }
    }
}

// --- splitting a changed run ---------------------------------------------------

/// One part of a changed run under [`DiffHighlighting::SplitChanges`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chunk {
    /// A line the change carries along unchanged: the same text sits on both
    /// sides of the run, so it is not part of any change.
    Carried(usize, usize),
    /// One smaller change: the pairs of run-relative line indices that differ,
    /// `None` filling the side a pair line does not exist on.
    Block(Vec<(Option<usize>, Option<usize>)>),
}

/// Partition a changed run into the separate changes it is made of: lines whose
/// text appears on both sides are carried through, and the remaining lines group
/// into as few blocks as the two sides' order allows.
///
/// This is the reference's `Split changes` rationale read off the row model — "A
/// \n B and A X \n B X are treated as two changes instead of one": the pairing is
/// the same line-by-line alignment the run's own diff would have made had the
/// unchanged line not been swallowed by the run.
pub fn split_changes(dels: &[&str], adds: &[&str]) -> Vec<Chunk> {
    let diff = TextDiff::from_slices(dels, adds);
    let mut chunks = Vec::new();
    for op in diff.ops() {
        if op.tag() == DiffTag::Equal {
            chunks.extend(
                op.old_range()
                    .zip(op.new_range())
                    .map(|(d, a)| Chunk::Carried(d, a)),
            );
            continue;
        }
        let (old, new) = (op.old_range(), op.new_range());
        let pairs = old.len().max(new.len());
        let block = (0..pairs)
            .map(|i| (old.clone().nth(i), new.clone().nth(i)))
            .collect();
        chunks.push(Chunk::Block(block));
    }
    chunks
}

// --- highlighting -------------------------------------------------------------

/// Byte offset of each char boundary, plus the length of the line: a token the
/// caller names by **char** offsets is then a borrowed slice with no copying, and
/// the char offsets are what the painter addresses glyphs by.
fn char_bounds(line: &str) -> Vec<usize> {
    let mut bounds: Vec<usize> = line.char_indices().map(|(b, _)| b).collect();
    bounds.push(line.len());
    bounds
}

/// Tokenize a line into slices of itself, each one a word (alphanumeric plus
/// `_`) or a single other character, paired with the char offsets it covers.
fn words(line: &str) -> Vec<(&str, Range<usize>)> {
    let bounds = char_bounds(line);
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let is_word = chars[i].is_alphanumeric() || chars[i] == '_';
        let mut j = i + 1;
        if is_word {
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
        }
        out.push((&line[bounds[i]..bounds[j]], i..j));
        i = j;
    }
    out
}

/// Character tokens: one slice per char, one char offset per slice.
fn chars(line: &str) -> Vec<(&str, Range<usize>)> {
    let bounds = char_bounds(line);
    line.char_indices()
        .map(|(i, _)| (&line[bounds[i]..bounds[i + 1]], i..i + 1))
        .collect()
}

/// The runs of `new` that differ from `old` at `hl` granularity, in char offsets.
///
/// [`DiffHighlighting::Lines`] and [`DiffHighlighting::SplitChanges`] highlight a
/// whole row, which the caller expresses as a row band rather than a run list;
/// [`DiffHighlighting::None`] highlights nothing. All three answer the empty list
/// here so the caller has one rule to read.
pub fn highlight_runs(hl: DiffHighlighting, old: &str, new: &str) -> Vec<Range<usize>> {
    match hl {
        DiffHighlighting::None | DiffHighlighting::Lines | DiffHighlighting::SplitChanges => {
            Vec::new()
        }
        DiffHighlighting::Words => runs_over(words(old), words(new)),
        DiffHighlighting::Characters => runs_over(chars(old), chars(new)),
    }
}

/// The changed token ranges of the new side, widened to whole token boundaries
/// and merged where they touch, so two adjacent changed words are two runs and
/// not four.
fn runs_over(
    old_tokens: Vec<(&str, Range<usize>)>,
    new_tokens: Vec<(&str, Range<usize>)>,
) -> Vec<Range<usize>> {
    let old_refs: Vec<&str> = old_tokens.iter().map(|(s, _)| *s).collect();
    let new_refs: Vec<&str> = new_tokens.iter().map(|(s, _)| *s).collect();
    let diff = TextDiff::from_slices(&old_refs, &new_refs);
    let mut runs: Vec<Range<usize>> = Vec::new();
    for op in diff.ops() {
        if op.tag() == DiffTag::Equal {
            continue;
        }
        let range = op.new_range();
        // A pure deletion has no new-side range: nothing to highlight there.
        let (Some(first), Some(last)) = (
            new_tokens.get(range.start),
            range.end.checked_sub(1).and_then(|k| new_tokens.get(k)),
        ) else {
            continue;
        };
        let run = first.1.start..last.1.end;
        // Runs that touch are one run; two changed words with an unchanged
        // separator between them are two, because a highlight that swallows the
        // gap is claiming a difference in the gap.
        match runs.last_mut() {
            Some(prev) if run.start <= prev.end => prev.end = prev.end.max(run.end),
            _ => runs.push(run),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The five whitespace values must be distinguishable, or the policy has one
    /// value and five names. Each case is hidden by one value and shown by the
    /// one below it.
    #[test]
    fn whitespace_ladder_is_five_distinct_values() {
        use DiffWhitespace as W;
        let trailing = (&["gamma  "][..], &["gamma"][..]);
        let internal = (&["g amma"][..], &["gamma"][..]);
        let added_blank = (&[][..], &["  "][..]);
        let joined = (&["a b c"][..], &["a", "b c"][..]);
        let import_swap = (&["use std::fs;", "use std::io;"][..], &["use std::io;"][..]);
        let literal_space = (&["let s = \"a b\";"][..], &["let s = \"a  b\";"][..]);

        let shows = |ws: W, dels: &[&str], adds: &[&str]| !considered_equal(ws, dels, adds);
        // None: every difference is a difference.
        for case in [
            &trailing,
            &internal,
            &added_blank,
            &joined,
            &import_swap,
            &literal_space,
        ] {
            assert!(shows(W::None, case.0, case.1), "None shows {case:?}");
        }
        // Trim: trailing goes, internal stays.
        assert!(!shows(W::TrimWhitespace, trailing.0, trailing.1));
        assert!(shows(W::TrimWhitespace, internal.0, internal.1));
        assert!(shows(W::TrimWhitespace, added_blank.0, added_blank.1));
        // Ignore whitespaces: internal goes too, but a line of nothing added
        // is still a change of line count.
        assert!(!shows(W::IgnoreWhitespace, internal.0, internal.1));
        assert!(shows(W::IgnoreWhitespace, added_blank.0, added_blank.1));
        assert!(shows(W::IgnoreWhitespace, joined.0, joined.1));
        // …and empty lines plus split/join go with the next value up.
        assert!(!shows(
            W::IgnoreWhitespaceAndEmptyLines,
            added_blank.0,
            added_blank.1
        ));
        assert!(!shows(W::IgnoreWhitespaceAndEmptyLines, joined.0, joined.1));
        assert!(shows(
            W::IgnoreWhitespaceAndEmptyLines,
            import_swap.0,
            import_swap.1
        ));
        // …and imports with the top value, whose literal rule keeps a space
        // inside a string significant even though it drops the rest.
        assert!(!shows(
            W::IgnoreImportsAndFormatting,
            import_swap.0,
            import_swap.1
        ));
        assert!(shows(
            W::IgnoreImportsAndFormatting,
            literal_space.0,
            literal_space.1
        ));
        // The value below does drop that same literal whitespace.
        assert!(!shows(
            W::IgnoreWhitespaceAndEmptyLines,
            literal_space.0,
            literal_space.1
        ));
    }

    /// A line whose content, not merely its imports, changed stays a difference
    /// at the top of the ladder.
    #[test]
    fn import_rule_is_confined_to_import_lines() {
        assert!(
            !considered_equal(
                DiffWhitespace::IgnoreImportsAndFormatting,
                &["use std::fs;"][..],
                &["use std::fs;", "let x = 1;"][..]
            ),
            "one side has a non-import line, so the run is a real change"
        );
    }

    /// `Split changes` partitions a run by the lines it carries along.
    #[test]
    fn split_changes_partitions_a_run_into_smaller_changes() {
        let dels = &["alpha", "beta"];
        let adds = &["alpha", "BETA"];
        assert_eq!(
            split_changes(dels, adds),
            vec![Chunk::Carried(0, 0), Chunk::Block(vec![(Some(1), Some(1))])],
            "the carried line stops being part of the change"
        );
        // A run with nothing carried stays one block, paired positionally.
        assert_eq!(
            split_changes(&["a", "b"], &["c"]),
            vec![Chunk::Block(vec![(Some(0), Some(0)), (Some(1), None)])]
        );
        // An insertion at the end of a run is its own block.
        assert_eq!(
            split_changes(&["keep"], &["keep", "new"]),
            vec![Chunk::Carried(0, 0), Chunk::Block(vec![(None, Some(1))])]
        );
    }

    /// The run-count relationship the viewer's highlighting is judged by: None
    /// paints no run, Words one per changed word group, Characters per changed
    /// character group — and Words never cuts inside a word.
    #[test]
    fn highlighting_granularity_runs_by_count_and_extent() {
        use DiffHighlighting as H;
        let (old, new) = ("alpha beta gamma", "ALPHA beta GAMMA");
        assert_eq!(highlight_runs(H::None, old, new).len(), 0);
        assert_eq!(
            highlight_runs(H::Lines, old, new).len(),
            0,
            "whole-row highlighting is not a run list"
        );
        let words = highlight_runs(H::Words, old, new);
        assert_eq!(words.len(), 2, "two changed word groups: {words:?}");
        assert_eq!(
            words
                .iter()
                .map(|r| new.chars().skip(r.start).take(r.end - r.start).collect())
                .collect::<Vec<String>>(),
            vec!["ALPHA".to_owned(), "GAMMA".to_owned()],
            "the runs land on the changed words themselves"
        );
        // Characters cuts a word open when only parts of its letters changed, so
        // the same change becomes more runs than words can name.
        let (inner_old, inner_new) = ("alpha BeTa gamMA", "Alpha beta GAMMA");
        let words_inner = highlight_runs(H::Words, inner_old, inner_new);
        let chars_inner = highlight_runs(H::Characters, inner_old, inner_new);
        assert_eq!(words_inner.len(), 3, "every word differs: {words_inner:?}");
        assert!(
            chars_inner.len() > words_inner.len(),
            "character granularity is finer than word granularity: {chars_inner:?} vs \
             {words_inner:?}"
        );
        // An appended suffix is one whole word for Words but one character for
        // Characters: the width of the run is the difference.
        let w = highlight_runs(H::Words, "alpha", "alpha2");
        let c = highlight_runs(H::Characters, "alpha", "alpha2");
        assert_eq!(w, vec![0..6], "a changed word is replaced whole: {w:?}");
        assert_eq!(c, vec![5..6], "only the added character changed: {c:?}");
    }

    /// A pure insertion has nothing on the old side, and a pure deletion nothing
    /// on the new side — neither is a run, and neither panics.
    #[test]
    fn one_sided_rows_have_runs_only_on_the_side_that_has_text() {
        use DiffHighlighting as H;
        assert_eq!(highlight_runs(H::Words, "", "added words"), vec![0..11]);
        assert!(highlight_runs(H::Words, "removed words", "").is_empty());
        assert!(highlight_runs(H::Characters, "", "").is_empty());
    }
}

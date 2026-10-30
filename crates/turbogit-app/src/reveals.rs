//! Reveal intent: how many lines of one gap a user has asked to see, and the
//! one side's text those lines are read from. Nothing here knows about rows,
//! painting, or the diff model.

use std::sync::Arc;

/// Which edge of a gap a tap opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
}

impl Edge {
    fn index(self) -> usize {
        match self {
            Edge::Top => 0,
            Edge::Bottom => 1,
        }
    }
}

/// One file's text for a reveal, split so a revealed line is a slice.
///
/// `starts` is load-bearing: indexing a painted row with `lines().skip(n)`
/// rescans from byte zero, several dozen times a frame. Its length is also a
/// `Hidden::ToEof` gap's answer, so one artifact settles the tail's count,
/// content and numbering.
#[derive(Debug)]
pub struct SideText {
    text: Arc<str>,
    starts: Arc<[u32]>,
}

impl SideText {
    /// `None` refuses bytes that are not UTF-8. A revealed line is painted as
    /// text, so a side that cannot be decoded answers with no text rather than
    /// with a lossy one.
    pub fn new(bytes: Vec<u8>) -> Option<Self> {
        let text = String::from_utf8(bytes).ok()?;
        let mut starts = Vec::new();
        if !text.is_empty() {
            starts.push(0u32);
            for (at, byte) in text.as_bytes().iter().enumerate() {
                if *byte == b'\n' && at + 1 < text.len() {
                    starts.push(u32::try_from(at + 1).unwrap_or(u32::MAX));
                }
            }
        }
        Some(Self {
            text: Arc::from(text),
            starts: starts.into(),
        })
    }

    /// How many lines the side has, counting a final line with no newline and
    /// not counting the empty span after a terminated file.
    pub fn lines(&self) -> u32 {
        self.starts.len() as u32
    }

    /// The line's text with its newline removed, `None` when the number is out
    /// of the file. One-based, the way a gutter prints it.
    pub fn line(&self, one_based: u32) -> Option<&str> {
        let index = usize::try_from(one_based.checked_sub(1)?).ok()?;
        let start = *self.starts.get(index)? as usize;
        let end = self
            .starts
            .get(index + 1)
            .map_or(self.text.len(), |next| *next as usize);
        let raw = self.text.get(start..end)?;
        Some(raw.strip_suffix('\n').unwrap_or(raw))
    }
}

/// One file's text, in one of three states and no others, so a reveal count can
/// never outrun the text a pane can paint.
#[derive(Clone, Default)]
pub enum Ask {
    /// Nothing to paint: never asked, or asked and refused by a side that is not
    /// UTF-8.
    #[default]
    None,
    Waiting,
    Have(Arc<SideText>),
}

/// One edge's first reveal step, and the surface's collapse threshold beside it.
/// Public because the pane must draw "this gap is one tap from empty" from the
/// same number `tap` advances by, not from a copy that can drift.
pub const FIRST_STEP: u32 = 10;

/// The step saturates so a late tap on a huge gap cannot drop several hundred
/// rows into the frame the user is reading. The total never caps: a ceiling is
/// what would leave a strip over a gap that can reveal nothing.
const STEP_CEILING: u32 = 200;

/// Reveal intent for exactly one settled comparison: `[from_top, from_bottom]`
/// per gap, and one text per file section.
///
/// The key is `DiffTarget::key()` verbatim, because the commit preview and the
/// floating Diff window write this field against different patches in one frame,
/// and unkeyed counts move from one patch to the other.
#[derive(Default)]
pub struct Reveals {
    key: Option<String>,
    given: Vec<[u32; 2]>,
    sides: Vec<Ask>,
}

impl Reveals {
    /// Advances one edge and answers whether anything changed, which is what
    /// makes an exhaustion test stateable: a tap over a run that is already
    /// fully open changes nothing and says so.
    pub fn tap(&mut self, key: &str, gap: usize, edge: Edge, hidden: Option<u32>) -> bool {
        self.use_key(key);
        if self.given.len() <= gap {
            self.given.resize(gap + 1, [0, 0]);
        }
        let taken = self.given[gap][edge.index()];
        // `min(10 · 2ⁿ, 200)` for the nth tap is the same schedule as
        // `taken + 10` until the ceiling is reached, and `+ 200` after it: one
        // number is stored per edge, not a tap count beside it.
        let step = taken.saturating_add(FIRST_STEP).min(STEP_CEILING);
        let next = match hidden {
            Some(hidden) => taken.saturating_add(step).min(hidden),
            None => taken.saturating_add(step),
        };
        if next == taken {
            return false;
        }
        self.given[gap][edge.index()] = next;
        true
    }

    /// A mismatched key answers zeros: counts are intent for one comparison and
    /// no other, and a stale gap ordinal must not borrow another patch's lines.
    pub fn edges(&self, key: &str, gap: usize) -> [u32; 2] {
        if self.key.as_deref() != Some(key) {
            return [0, 0];
        }
        self.given.get(gap).copied().unwrap_or([0, 0])
    }

    /// Records that this file's text is owed, and answers whether the read still
    /// has to be dispatched. The kind admits one fetch at a time, so a second
    /// file asked while the first loads waits rather than losing the click.
    pub fn admit(&mut self, key: &str, file: usize) -> bool {
        self.use_key(key);
        if self.sides.len() <= file {
            self.sides.resize(file + 1, Ask::default());
        }
        if !matches!(self.sides[file], Ask::None) {
            return false;
        }
        self.sides[file] = Ask::Waiting;
        true
    }

    pub fn side(&self, key: &str, file: usize) -> Ask {
        if self.key.as_deref() != Some(key) {
            return Ask::None;
        }
        self.sides.get(file).cloned().unwrap_or_default()
    }

    /// Folds a settled side in. A key that is no longer this state's own is
    /// ignored, and `None` — a missing side or undecodable bytes — leaves the
    /// file with no text to paint rather than a wait that never ends.
    pub fn settle(&mut self, key: &str, file: usize, side: Option<Arc<SideText>>) {
        if self.key.as_deref() != Some(key) {
            return;
        }
        if self.sides.len() <= file {
            self.sides.resize(file + 1, Ask::default());
        }
        self.sides[file] = side.map_or(Ask::None, Ask::Have);
    }

    fn use_key(&mut self, key: &str) {
        if self.key.as_deref() != Some(key) {
            *self = Self::default();
            self.key = Some(key.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hidden(n: u32) -> Option<u32> {
        Some(n)
    }

    #[test]
    fn a_reveal_doubles_its_step_and_stops_at_what_is_hidden() {
        let mut r = Reveals::default();
        let mut totals = Vec::new();
        for _ in 0..4 {
            assert!(r.tap("k", 1, Edge::Top, hidden(9000)));
            totals.push(r.edges("k", 1)[0]);
        }
        assert_eq!(totals, vec![10, 30, 70, 150], "the step doubles per edge");

        assert!(r.tap("k", 0, Edge::Top, hidden(9)));
        assert_eq!(
            r.edges("k", 0)[0],
            9,
            "a run is opened to its length, no more"
        );
        assert!(
            !r.tap("k", 0, Edge::Top, hidden(9)),
            "an exhausted edge changes nothing, which is the exhaustion test"
        );
        assert!(
            !r.tap("k", 0, Edge::Bottom, hidden(0)),
            "a run that hides nothing opens no edge at all"
        );
    }

    #[test]
    fn an_unknown_tail_never_clamps_and_the_step_never_caps_the_total() {
        let mut r = Reveals::default();
        for _ in 0..8 {
            assert!(r.tap("k", 1, Edge::Bottom, None));
        }
        let opened = r.edges("k", 1)[1];
        assert!(opened > STEP_CEILING, "no total ceiling: {opened}");
        assert_eq!(r.edges("k", 1)[0], 0, "the other edge is its own count");
    }

    #[test]
    fn counts_belong_to_one_comparison() {
        let mut r = Reveals::default();
        assert!(r.tap("first", 0, Edge::Top, None));
        assert_eq!(r.edges("first", 0), [10, 0]);
        assert_eq!(r.edges("second", 0), [0, 0], "a stale key answers zeros");
        assert!(r.tap("second", 0, Edge::Top, None));
        assert_eq!(r.edges("first", 0), [0, 0], "and the older key is gone");
    }

    #[test]
    fn one_side_of_text_is_owed_once_and_lands_once() {
        let mut r = Reveals::default();
        assert!(r.admit("k", 0), "the first ask owes a read");
        assert!(!r.admit("k", 0), "the same file is not asked for twice");
        assert!(matches!(r.side("k", 0), Ask::Waiting));
        let text = Arc::new(SideText::new(b"one\n".to_vec()).expect("utf-8"));
        r.settle("k", 0, Some(Arc::clone(&text)));
        assert!(matches!(r.side("k", 0), Ask::Have(_)));
        r.settle("other", 0, None);
        assert!(
            matches!(r.side("k", 0), Ask::Have(_)),
            "another comparison's answer cannot blank this one"
        );
    }

    #[test]
    fn refused_text_leaves_nothing_to_paint() {
        let mut r = Reveals::default();
        assert!(r.admit("k", 2));
        r.settle("k", 2, None);
        assert!(matches!(r.side("k", 2), Ask::None), "not a wait forever");
    }

    #[test]
    fn a_side_indexes_its_lines_without_rescanning() {
        let text = SideText::new(b"alpha\nbeta\ngamma".to_vec()).expect("utf-8");
        assert_eq!(text.lines(), 3);
        assert_eq!(text.line(1), Some("alpha"));
        assert_eq!(text.line(3), Some("gamma"));
        assert_eq!(text.line(4), None);
        assert_eq!(text.line(0), None);

        let terminated = SideText::new(b"one\n".to_vec()).expect("utf-8");
        assert_eq!(
            terminated.lines(),
            1,
            "a terminated file ends its last line"
        );
        assert_eq!(terminated.line(1), Some("one"));

        let empty = SideText::new(Vec::new()).expect("utf-8");
        assert_eq!(empty.lines(), 0, "an empty file has no line to reveal");

        let crlf = SideText::new(b"one\r\ntwo\r\n".to_vec()).expect("utf-8");
        assert_eq!(
            crlf.line(1),
            Some("one\r"),
            "only the newline is trimmed, so a revealed row matches the patch's own \
             context text"
        );

        assert!(
            SideText::new(vec![b'x', 0xff, 0xfe]).is_none(),
            "bytes that are not UTF-8 are refused, not decoded lossily"
        );
    }
}

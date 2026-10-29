#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuoteTrigger {
    /// git's own spelling, what the **Patch value**'s writer must match byte for
    /// byte.
    GitSpelling,
    /// the rewriter's two-token line, what a freshly written `diff --git` line
    /// must survive.
    HeaderPair,
}

impl QuoteTrigger {
    fn forces(self, byte: u8) -> bool {
        match self {
            Self::GitSpelling => !(0x20..0x7f).contains(&byte) || matches!(byte, b'"' | b'\\'),
            Self::HeaderPair => byte == b' ' || byte == b'"' || byte < 0x20,
        }
    }
}

pub fn needs_quote(path: &[u8], trigger: QuoteTrigger) -> bool {
    path.iter().any(|&b| trigger.forces(b))
}

/// git's C-style quoting of a path, byte for byte, the header rewriter's entry.
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
            other => out.extend_from_slice(format!("\\{other:03o}").as_bytes()),
        }
    }
    out
}

/// git's C-style quoting undone, byte for byte, the header rewriter's entry.
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

/// The conversion is lossless by construction: [`escape`] turns every byte outside
/// printable ASCII into an escape, so nothing else can reach it.
pub fn escape_c(s: &str) -> String {
    String::from_utf8(escape(s.as_bytes())).expect("escaped text is printable ASCII")
}

/// git's C-style quoting undone for a `&str` reader, which is handed one lossy
/// conversion, the exact bytes are [`unescape`]'s to give.
pub fn unescape_c(s: &str) -> String {
    String::from_utf8_lossy(&unescape(s.as_bytes())).into_owned()
}

pub fn find_quote_c(s: &str) -> Option<usize> {
    find_quote(s.as_bytes())
}

pub fn quote_c(path: &str, trigger: QuoteTrigger) -> String {
    if needs_quote(path.as_bytes(), trigger) {
        format!("\"{}\"", escape_c(path))
    } else {
        path.to_string()
    }
}

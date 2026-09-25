//! Reading git's unified-diff format into the **Git engine**'s patch value.
//!
//! This is the one place in the workspace where git's diff *syntax* is
//! interpreted (`ADR-0022`). A diff answer arrives here as a [`Patch`] and
//! everything upstream — the display model, **Partial staging**, the hunk
//! listings the root caches hold — reads fields. Saying a value back in git's
//! syntax is the value's own business, in `turbogit-domain`, because the index
//! is fed bytes and the viewer paints labels.
//!
//! Lines before the first `diff --git` section (a `git log -p` commit header,
//! say) are skipped; an empty diff is an empty value, which is an answer.

use turbogit_domain::model::{
    Patch, PatchFile, PatchHeaderLine, PatchHunk, PatchLine, PatchLineKind,
};

/// The `Git engine`'s one reader of unified diff.
pub fn parse_patch(text: &str) -> Patch {
    let mut files: Vec<PatchFile> = Vec::new();
    let mut cursor = Lines {
        all: text.lines().collect(),
        at: 0,
    };

    while let Some(line) = cursor.next() {
        let Some((rest, combined)) = section_header(line) else {
            continue;
        };
        let (old_path, new_path) = if combined {
            // `diff --cc conf.txt` names one path: every parent's side and the
            // worktree's share it.
            let same = side(rest);
            (same.clone(), same)
        } else {
            let (old, new) = split_git_paths(rest);
            (old.unwrap_or_default(), new.unwrap_or_default())
        };
        let mut file = PatchFile {
            old_path,
            new_path,
            combined,
            headers: Vec::new(),
            hunks: Vec::new(),
        };

        while let Some(line) = cursor.peek() {
            // A new section opens; the outer loop reads its header.
            if section_header(line).is_some() {
                break;
            }
            if line.is_empty() {
                // A blank line here is context whose leading space `lines()`
                // dropped; only a hunk can own it.
                cursor.next();
                if let Some(hunk) = file.hunks.last_mut() {
                    hunk.lines.push(PatchLine {
                        kind: PatchLineKind::Context,
                        text: String::new(),
                        no_newline: false,
                    });
                }
                continue;
            }
            if let Some(header) = line.strip_prefix("@@@") {
                let header = header.to_string();
                cursor.next();
                file.hunks.push(parse_combined_hunk(&header, &mut cursor));
                continue;
            }
            if let Some(header) = line.strip_prefix("@@") {
                let header = header.to_string();
                cursor.next();
                file.hunks.push(parse_hunk(&header, &mut cursor));
                continue;
            }
            let line = line.to_string();
            cursor.next();
            let header = read_header(&line);
            // `+++` completes the `---` line that opened it; the pair is one
            // fact because a patch's two sides are one question.
            let plus = match &header {
                Some(PatchHeaderLine::Sources { new, .. }) if line.starts_with("+++ ") => {
                    Some(new.clone())
                }
                _ => None,
            };
            match (plus, file.headers.last_mut()) {
                (Some(new), Some(PatchHeaderLine::Sources { new: slot, .. })) => *slot = new,
                _ => {
                    if let Some(header) = header {
                        file.headers.push(header);
                    }
                }
            }
        }

        files.push(file);
    }

    Patch { files }
}

/// A section's opening line: `diff --git a/x b/y`, or git's **combined** view of
/// an unmerged path (`diff --cc x`, `diff --combined x`). The rest of the line,
/// and which of the two shapes it was.
fn section_header(line: &str) -> Option<(&str, bool)> {
    if let Some(rest) = line.strip_prefix("diff --git ") {
        return Some((rest, false));
    }
    for prefix in ["diff --cc ", "diff --combined "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some((rest, true));
        }
    }
    None
}

/// One section header line, as the fact it states. `None` for a line the patch
/// model does not name — it still reaches the display as the text it is.
fn read_header(line: &str) -> Option<PatchHeaderLine> {
    if let Some(v) = line.strip_prefix("index ") {
        let (pair, mode) = match v.split_once(' ') {
            Some((pair, mode)) => (pair, Some(mode.to_string())),
            None => (v, None),
        };
        let (old, new) = pair.split_once("..").unwrap_or((pair, pair));
        return Some(PatchHeaderLine::Index {
            old: old.to_string(),
            new: new.to_string(),
            mode,
        });
    }
    if let Some(v) = line.strip_prefix("similarity index ") {
        return v
            .trim_end_matches('%')
            .parse()
            .ok()
            .map(|percent| PatchHeaderLine::Similarity { percent });
    }
    if let Some(v) = line.strip_prefix("rename from ") {
        return Some(PatchHeaderLine::RenameFrom { path: unquote(v) });
    }
    if let Some(v) = line.strip_prefix("rename to ") {
        return Some(PatchHeaderLine::RenameTo { path: unquote(v) });
    }
    if let Some(v) = line.strip_prefix("new file mode ") {
        return Some(PatchHeaderLine::NewFile {
            mode: v.to_string(),
        });
    }
    if let Some(v) = line.strip_prefix("deleted file mode ") {
        return Some(PatchHeaderLine::DeletedFile {
            mode: v.to_string(),
        });
    }
    if let Some(v) = line.strip_prefix("old mode ") {
        return Some(PatchHeaderLine::OldMode {
            mode: v.to_string(),
        });
    }
    if let Some(v) = line.strip_prefix("new mode ") {
        return Some(PatchHeaderLine::NewMode {
            mode: v.to_string(),
        });
    }
    if line.starts_with("Binary files ") && line.ends_with(" differ") {
        return Some(PatchHeaderLine::Binary);
    }
    if let Some(v) = line.strip_prefix("--- ") {
        return Some(PatchHeaderLine::Sources {
            old: unquote(v),
            new: String::new(),
        });
    }
    if let Some(v) = line.strip_prefix("+++ ") {
        return Some(PatchHeaderLine::Sources {
            old: String::new(),
            new: unquote(v),
        });
    }
    None
}

/// `@@ -l,s +l,s @@ heading` plus its body lines, consuming through the hunk.
fn parse_hunk(after_at_prefix: &str, lines: &mut Lines<'_>) -> PatchHunk {
    let header = after_at_prefix.trim_start_matches('-');
    let (span, heading) = split_hunk_header(header);
    let mut hunk = PatchHunk {
        old_start: span.0,
        old_count: span.1,
        new_start: span.2,
        new_count: span.3,
        heading,
        lines: Vec::new(),
    };
    parse_hunk_body(&mut hunk, lines);
    hunk
}

/// `@@@ -l,s -l,s +l,s @@@ heading`, git's combined header for an unmerged
/// path: one range per parent, then the new side's. The value has two sides, so
/// this keeps the first parent's range and the new one — enough to place the
/// hunk, and the section says where it came from.
fn parse_combined_hunk(after_three_at: &str, lines: &mut Lines<'_>) -> PatchHunk {
    let mut old = (1usize, 1usize);
    let mut new = (1usize, 1usize);
    for token in after_three_at.split_whitespace() {
        let range = || {
            let v = token.strip_prefix(['-', '+']).unwrap_or(token);
            let (start, count) = v.split_once(',').unwrap_or((v, "1"));
            (start.parse().unwrap_or(1), count.parse().unwrap_or(1))
        };
        if token.starts_with('-') && old == (1, 1) {
            old = range();
        } else if token.starts_with('+') {
            new = range();
        }
    }
    let heading = after_three_at
        .rsplit_once("@@@")
        .map(|(_, rest)| rest.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let mut hunk = PatchHunk {
        old_start: old.0,
        old_count: old.1,
        new_start: new.0,
        new_count: new.1,
        heading,
        lines: Vec::new(),
    };
    parse_hunk_body(&mut hunk, lines);
    hunk
}

/// A hunk's body lines, consuming through the next header or section.
fn parse_hunk_body(hunk: &mut PatchHunk, lines: &mut Lines<'_>) {
    let mut pending_no_newline = false;
    while let Some(line) = lines.peek() {
        if line.starts_with("@@") || section_header(line).is_some() {
            break;
        }
        let line = line.to_string();
        lines.next();
        if line.starts_with('\\') {
            // The marker annotates the line it followed.
            match hunk.lines.last_mut() {
                Some(last) => last.no_newline = true,
                None => pending_no_newline = true,
            }
            continue;
        }
        let (kind, text) = match line.as_bytes().first() {
            Some(b'+') => (PatchLineKind::Added, line[1..].to_string()),
            Some(b'-') => (PatchLineKind::Removed, line[1..].to_string()),
            Some(b' ') => (PatchLineKind::Context, line[1..].to_string()),
            // A context line whose content is empty: git emits a bare space,
            // and `lines()` has already dropped it.
            _ => (PatchLineKind::Context, line.clone()),
        };
        hunk.lines.push(PatchLine {
            kind,
            text,
            no_newline: std::mem::take(&mut pending_no_newline),
        });
    }
}

/// The section's lines with a cursor: a hunk consumes its own body and hands
/// the rest back, which is what keeps this one pass over one format.
struct Lines<'a> {
    all: Vec<&'a str>,
    at: usize,
}

impl<'a> Lines<'a> {
    fn peek(&self) -> Option<&'a str> {
        self.all.get(self.at).copied()
    }

    fn next(&mut self) -> Option<&'a str> {
        let line = self.all.get(self.at).copied();
        if line.is_some() {
            self.at += 1;
        }
        line
    }
}

/// `-l,s +l,s @@` (whatever follows the first `@@`) → spans and heading.
fn split_hunk_header(after: &str) -> ((usize, usize, usize, usize), Option<String>) {
    let body = after.strip_prefix('@').unwrap_or(after);
    let mut tokens = body.split_whitespace();
    let parse = |v: &str| -> (usize, usize) {
        let v = v.strip_prefix(['-', '+']).unwrap_or(v);
        let (start, count) = v.split_once(',').unwrap_or((v, "1"));
        (start.parse().unwrap_or(1), count.parse().unwrap_or(1))
    };
    let (old_start, old_count) = parse(tokens.next().unwrap_or("-1,1"));
    let (new_start, new_count) = parse(tokens.next().unwrap_or("+1,1"));
    let heading = body
        .find("@@")
        .map(|i| &body[i + 2..])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    ((old_start, old_count, new_start, new_count), heading)
}

/// The paths on a `diff --git` line, git's quoted form resolved.
fn split_git_paths(rest: &str) -> (Option<String>, Option<String>) {
    let rest = rest.trim();
    if let Some(tail) = rest.strip_prefix(Q)
        && let Some(end) = find_quote(tail)
    {
        let old = side(&rest[..end + 2]);
        let after = rest[end + 2..].trim_start();
        return (Some(old), (!after.is_empty()).then(|| side(after)));
    }
    match rest.find(" b/") {
        Some(sep) => (Some(side(&rest[..sep])), Some(side(&rest[sep + 1..]))),
        None => (None, None),
    }
}

/// One path from a header line: git's quoting undone, then its `a/`/`b/` side
/// prefix dropped. Order matters — the prefix is inside the quotes.
fn side(raw: &str) -> String {
    let bare = unquote(raw.trim());
    bare.strip_prefix("a/")
        .or_else(|| bare.strip_prefix("b/"))
        .unwrap_or(&bare)
        .to_string()
}

/// Unescape one quoted-or-plain path as git wrote it in a header line.
fn unquote(raw: &str) -> String {
    let raw = raw.trim();
    if let Some(quoted) = raw.strip_prefix(Q).and_then(|b| b.strip_suffix(Q)) {
        return unescape_c(quoted);
    }
    raw.to_string()
}

fn find_quote(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
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

/// git's C-style path quoting: `\"`, `\\`, `\t`, `\n`, and octal escapes — one
/// per **byte**, which is how a non-ASCII name comes back as itself rather than
/// as Latin-1 mojibake.
fn unescape_c(s: &str) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    let mut bytes = s.bytes().peekable();
    while let Some(b) = bytes.next() {
        if b != b'\\' {
            out.push(b);
            continue;
        }
        match bytes.next() {
            Some(b'"') => out.push(b'"'),
            Some(b'\\') => out.push(b'\\'),
            Some(b't') => out.push(b'\t'),
            Some(b'n') => out.push(b'\n'),
            Some(b'r') => out.push(b'\r'),
            Some(d @ b'0'..=b'7') => {
                let mut byte = d - b'0';
                for _ in 0..2 {
                    if matches!(bytes.peek(), Some(b'0'..=b'7')) {
                        byte = byte * 8 + (bytes.next().expect("peeked") - b'0');
                    } else {
                        break;
                    }
                }
                out.push(byte);
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

const Q: char = '"';

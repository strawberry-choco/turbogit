//! Turning git's own patch bytes into the artifact `Create patch` hands over:
//! path rewriting, the counting repair pair, the file name and the encoding.
//!
//! The Git engine answers `GitExecutor::patch_bytes` with **bytes**, and this module
//! is the only place a created patch is edited before it lands. Everything here works
//! on the raw text rather than on a parsed `Patch`, for the reason ADR-0013 states for
//! partial staging: a diff's body is its file's content, which git does not promise is
//! text in any encoding, and a value that round-trips through `String` has already
//! replaced the bytes the `Encoding` field is then asked to write. Rewriting a
//! section's paths is a header-line operation, so only header lines are read and every
//! content line is copied through byte for byte.
//!
//! Three facts hold the shape of the surface:
//!
//! * **Base path** decides the directory the patch's paths are read from — they are
//!   written *relative to it*, which is why moving the base up out of the repository
//!   adds the repository's own name and moving it into a subdirectory drops that
//!   subdirectory.
//! * **Leading-directory removal** is a *count*, not an edit: `Remove Leading
//!   Directory` takes one segment off every path in the patch, `Restore Leading
//!   Directory` gives one back, and the number removed is what the dialog shows in
//!   square brackets. A patch that nearly applies is repaired by counting segments
//!   rather than by retyping a prefix, so [`removable_leading_directories`] is the
//!   bound that keeps the count meaningful.
//! * **The file name comes from the comment.** The reference's create step asks for a
//!   commit comment and states in the same breath that it will be used as the patch
//!   file name, so the name is a *derived* value: sanitized so that a comment can
//!   never select a directory, escape one, or name nothing.
//!
//! Patch paths are **bytes** throughout. Only the base directory and the project root
//! — typed by the developer or handed over by the OS — go through text, and they meet a
//! patch path only when the base sits above or below the root.

use std::path::{Component, Path, PathBuf};
use turbogit_domain::error::{TgError, TgResult};
use turbogit_domain::model::PatchEncoding;

/// The longest file name [`file_name`] derives from a comment, in bytes, before its
/// extension. A comment is free to be a paragraph; a name is not.
const NAME_LIMIT: usize = 48;

/// The bytes written for the patch file when a comment names nothing at all.
const FALLBACK_NAME: &str = "patch";

/// One component of a path this module assembles with. Owned bytes, because a rewritten
/// patch path is a *new* path: it borrows from git's output, from the base the
/// developer typed, or from both.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Part {
    /// The only absolute form a patch path ever carries — a leading `/`.
    Root,
    /// A parent step. Base relativization produces these, and removal never consumes
    /// one: the directories being counted are the ones *below* the base.
    Parent,
    /// A name.
    Name(Vec<u8>),
}

impl Part {
    fn is_name(&self) -> bool {
        matches!(self, Self::Name(_))
    }
}

/// When a rewritten path is written with git's C-style quoting.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Quoting {
    /// Quote if the result needs it. A `diff --git` line is read as two tokens, so a
    /// space the rewrite introduced would merge them into one wrong path.
    AsNeeded,
    /// Keep whatever quoting the line already had. `---`/`+++` and `rename from` carry
    /// one path, and a consumer that read the unquoted form must not be handed a quoted
    /// one and back.
    AsWritten,
}

/// A patch's paths, rewritten for one base directory and one removal count.
///
/// `base_path` is the `Patch File Settings` field as typed: empty, or the project
/// directory, leaves git's repo-relative paths exactly as they came; a directory above
/// the project makes every path carry the project's own name; a directory inside it
/// strips that prefix. `strip_leading` then drops that many leading names from every
/// path — what the dialog's bracketed count states, and what
/// [`removable_leading_directories`] bounds, so no press can take a path past the point
/// where it stops naming a file.
pub fn rewrite_paths(bytes: &[u8], root: &Path, base_path: &str, strip_leading: usize) -> Vec<u8> {
    let base_dir = base_dir(root, base_path);
    let base = path_parts(&base_dir);
    let root_parts = path_parts(root);
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        out.extend_from_slice(&rewrite_line(line, &base, &root_parts, strip_leading));
    }
    out
}

/// One line of the patch, its ending newline and all: paths rewritten when it is a path
/// header, copied verbatim when it is anything else — every content line, and every
/// header this surface has no business touching (`index`, mode, `Binary files`, `@@`).
fn rewrite_line(line: &[u8], base: &[Part], root: &[Part], strip: usize) -> Vec<u8> {
    let newline = line.ends_with(b"\n");
    let body = if newline {
        &line[..line.len() - 1]
    } else {
        line
    };
    let mut out: Vec<u8> = Vec::with_capacity(line.len());

    if let Some(rest) = after(body, b"diff --git ") {
        // git names both sides of a section on its opening line; the pair is one path
        // fact and is rewritten twice or not at all.
        let (old, new) = split_section_paths(rest);
        out.extend_from_slice(b"diff --git ");
        out.extend_from_slice(&path_field(
            &old,
            b"a/",
            base,
            root,
            strip,
            Quoting::AsNeeded,
        ));
        out.push(b' ');
        out.extend_from_slice(&path_field(
            &new,
            b"b/",
            base,
            root,
            strip,
            Quoting::AsNeeded,
        ));
    } else if let Some(rest) = after(body, b"--- ") {
        out.extend_from_slice(b"--- ");
        out.extend_from_slice(&null_or(rest, b"a/", base, root, strip));
    } else if let Some(rest) = after(body, b"+++ ") {
        out.extend_from_slice(b"+++ ");
        out.extend_from_slice(&null_or(rest, b"b/", base, root, strip));
    } else if let Some((label, bare)) = rename_line(body) {
        out.extend_from_slice(label.as_bytes());
        // These lines carry a bare path: no side marker to keep.
        out.extend_from_slice(&path_field(
            one_path(bare),
            b"",
            base,
            root,
            strip,
            Quoting::AsWritten,
        ));
    } else {
        out.extend_from_slice(body);
    }
    if newline {
        out.push(b'\n');
    }
    out
}

/// A `---`/`+++` side: `/dev/null` copied through, anything else rewritten. Turning
/// `/dev/null` into a relative path would make the patch create a file called
/// `dev/null`, which is a different artifact than the create or delete it states.
fn null_or(rest: &[u8], marker: &[u8], base: &[Part], root: &[Part], strip: usize) -> Vec<u8> {
    let path = one_path(rest);
    if path.starts_with(b"/dev/null") {
        return path.to_vec();
    }
    path_field(path, marker, base, root, strip, Quoting::AsWritten)
}

/// `rename from`/`to`, `copy from`/`to`, as the label and the rest of the line.
fn rename_line(body: &[u8]) -> Option<(&'static str, &[u8])> {
    for (label, prefix) in [
        ("rename from ", b"rename from ".as_slice()),
        ("rename to ", b"rename to ".as_slice()),
        ("copy from ", b"copy from ".as_slice()),
        ("copy to ", b"copy to ".as_slice()),
    ] {
        if body.starts_with(prefix) {
            return Some((label, &body[prefix.len()..]));
        }
    }
    None
}

/// One path field as the rewritten patch writes it: git's side marker, then the path,
/// quoted under `quoting`.
fn path_field(
    raw: &[u8],
    marker: &[u8],
    base: &[Part],
    root: &[Part],
    strip: usize,
    quoting: Quoting,
) -> Vec<u8> {
    let logical = without_side(raw, marker);
    let mut full = marker.to_vec();
    full.extend_from_slice(&rewritten(&logical, base, root, strip));
    let quote = raw.starts_with(b"\"")
        || (quoting == Quoting::AsNeeded && (needs_quote(&full) || full.is_empty()));
    if quote {
        let mut out = Vec::with_capacity(full.len() + 2);
        out.push(b'"');
        out.extend_from_slice(&escape(&full));
        out.push(b'"');
        out
    } else {
        full
    }
}

/// One patch path as this patch will write it: related to the base, then with the
/// counted directories taken off the front.
fn rewritten(logical: &[u8], base: &[Part], root: &[Part], strip: usize) -> Vec<u8> {
    let parts = parts_of(logical);
    // An absolute path in a patch is already stated from nowhere; a base has no claim on
    // it, and inventing one would move the file.
    if parts.first() == Some(&Part::Root) {
        return join(&drop_leading(&parts, strip));
    }
    let mut absolute: Vec<Part> = root.to_vec();
    absolute.extend(parts);
    join(&drop_leading(&relativize(base, &absolute), strip))
}

/// How many leading directories every path in this patch can lose.
///
/// The **minimum** over the patch's paths, because one removal is one removal on every
/// path: a count that left `README.md` with nothing to name would not be a shorter path
/// but a broken patch. A patch with no path header can lose nothing, which is why a
/// surface disables the button at this bound rather than clamping a press that would
/// mean something else.
pub fn removable_leading_directories(bytes: &[u8]) -> usize {
    let mut least: Option<usize> = None;
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        let body = strip_newline(line);
        let mut paths: Vec<Vec<u8>> = Vec::new();
        if let Some(rest) = after(body, b"diff --git ") {
            let (old, new) = split_section_paths(rest);
            paths.push(without_side(&old, b"a/"));
            paths.push(without_side(&new, b"b/"));
        } else if let Some(rest) = after(body, b"--- ").or_else(|| after(body, b"+++ ")) {
            paths.push(one_path(rest).to_vec());
        } else if let Some((_, bare)) = rename_line(body) {
            paths.push(one_path(bare).to_vec());
        }
        for path in paths {
            if path.is_empty() || path.starts_with(b"/dev/null") {
                continue;
            }
            let names = parts_of(&path).iter().filter(|p| p.is_name()).count();
            // A parent step is not a directory being counted, and the file's own name is
            // the last segment and never removable.
            let depth = names.saturating_sub(1);
            least = Some(least.map_or(depth, |d| d.min(depth)));
        }
    }
    least.unwrap_or(0)
}

/// The file name the reference's comment step derives: "enter a commit comment
/// (it will be used as the patch file name)".
///
/// Sanitized rather than used verbatim, because a comment is free text and a name is a
/// path: separators, parent steps and a name that is empty after trimming all have to be
/// resolved here, or the comment becomes a way of writing outside the directory the
/// dialog chose. An empty comment falls back to the artifact's own name rather than to a
/// dot file.
pub fn file_name(comment: &str) -> String {
    let mut name = String::with_capacity(NAME_LIMIT + 6);
    for ch in comment.lines().next().unwrap_or_default().trim().chars() {
        if name.len() >= NAME_LIMIT {
            break;
        }
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => name.push('_'),
            '.' if name.is_empty() || name.ends_with('.') => name.push('_'),
            c if c.is_whitespace() => name.push('_'),
            c => name.push(c),
        }
    }
    // A separator collapsed to `_` at the end survives trimming: `fix/` and `fix` name
    // the same patch, which is all a derived file name can mean.
    let name = name.trim_matches(|c| c == '_' || c == '.');
    let name = if name.is_empty() { FALLBACK_NAME } else { name };
    if name.ends_with(".patch") {
        name.to_string()
    } else {
        format!("{name}.patch")
    }
}

/// The patch bytes as they are written, after the `Encoding` field.
///
/// `Utf8` — the default — is **no transformation at all**, which is the whole reason a
/// patch travels as bytes: what git emitted is what lands. The other two decode first
/// and refuse rather than guess, because a BOM over undecodable bytes claims an encoding
/// for them, and ISO-8859-1 has no byte for a code point above its range. A refusal
/// names the encoding that asked for it.
pub fn encode(bytes: &[u8], encoding: PatchEncoding) -> TgResult<Vec<u8>> {
    match encoding {
        PatchEncoding::Utf8 => Ok(bytes.to_vec()),
        PatchEncoding::Utf8WithBom => {
            let mut out = vec![0xEF, 0xBB, 0xBF];
            out.extend_from_slice(decode(bytes, encoding)?.as_bytes());
            Ok(out)
        }
        PatchEncoding::Latin1 => {
            let text = decode(bytes, encoding)?;
            let mut out = Vec::with_capacity(text.len());
            for ch in text.chars() {
                match u8::try_from(ch) {
                    Ok(byte) => out.push(byte),
                    Err(_) => {
                        return Err(TgError::Other(format!(
                            "ISO-8859-1 has no byte for `{ch}` — keep the patch encoding \
                             at UTF-8"
                        )));
                    }
                }
            }
            Ok(out)
        }
    }
}

/// The patch as clipboard text.
///
/// A text clipboard holds characters, so this is the one place a created patch is
/// decoded — and the one place a non-UTF-8 patch can legitimately stop the clipboard
/// route. Losing bytes silently to make a copy succeed would hand the developer an
/// artifact differing from the file the same action writes.
pub fn clipboard_text(bytes: &[u8]) -> TgResult<String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| {
        TgError::Other(
            "this patch is not valid UTF-8, so it cannot go on the clipboard — create it \
             as a file instead"
                .to_string(),
        )
    })
}

/// Why a byte sequence could not be written in `encoding`.
fn decode(bytes: &[u8], encoding: PatchEncoding) -> TgResult<&str> {
    std::str::from_utf8(bytes).map_err(|_| {
        TgError::Other(format!(
            "{} needs the patch to be valid UTF-8, and this one is not — keep the \
             encoding at UTF-8 to write the bytes git produced",
            encoding.label()
        ))
    })
}

/// The base directory the patch's paths are read from: the field as typed, taken
/// relative to the project unless it is absolute. Empty is the project itself —
/// "normally, this is your project directory".
fn base_dir(root: &Path, base_path: &str) -> PathBuf {
    let typed = base_path.trim();
    if typed.is_empty() || typed == "." {
        return root.to_path_buf();
    }
    let candidate = Path::new(typed);
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }
    root.join(candidate)
}

/// A filesystem path as the components this module assembles with.
///
/// A parent step is **resolved against the name before it** rather than carried along:
/// the base directory is typed, and `..` is the normal way of saying "the directory above
/// the project". Leaving the step in place would relate a file to a directory that is not
/// the one the developer named. Names are taken as lossy text, because this is the
/// project's own location and the base the developer typed — the patch's paths never come
/// through here.
fn path_parts(path: &Path) -> Vec<Part> {
    let mut out = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::RootDir => out.push(Part::Root),
            Component::ParentDir => match out.last() {
                Some(Part::Name(_)) => {
                    out.pop();
                }
                _ => out.push(Part::Parent),
            },
            Component::Prefix(prefix) => out.push(Part::Name(
                prefix
                    .as_os_str()
                    .to_string_lossy()
                    .into_owned()
                    .into_bytes(),
            )),
            Component::Normal(name) => {
                out.push(Part::Name(name.to_string_lossy().into_owned().into_bytes()))
            }
        }
    }
    out
}

/// A patch path's bytes as components, with git's quoting resolved.
fn parts_of(raw: &[u8]) -> Vec<Part> {
    let raw = unquote(raw);
    let mut out = Vec::new();
    if raw.starts_with(b"/") {
        out.push(Part::Root);
    }
    for seg in raw.split(|b| *b == b'/') {
        match seg {
            b"" | b"." => {}
            b".." => out.push(Part::Parent),
            name => out.push(Part::Name(name.to_vec())),
        }
    }
    out
}

/// `to` seen from `from`, in `..` form — lexical, the rule git itself uses for a path it
/// has never opened. Both sides are already component lists.
fn relativize(from: &[Part], to: &[Part]) -> Vec<Part> {
    let shared = from
        .iter()
        .zip(to.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut out = vec![Part::Parent; from.len() - shared];
    out.extend(to[shared..].iter().cloned());
    out
}

/// The first `strip` leading names dropped, which is the removal the dialog counts.
/// Parent steps survive — they say where the base is, not what the file is called — and
/// the last name always survives, because a path with no name is not a shorter path but a
/// patch that names nothing.
fn drop_leading(parts: &[Part], strip: usize) -> Vec<Part> {
    let last_name = parts.iter().rposition(Part::is_name);
    let mut remaining = strip;
    let mut out: Vec<Part> = Vec::with_capacity(parts.len());
    for (i, part) in parts.iter().enumerate() {
        if remaining > 0 && part.is_name() && Some(i) != last_name {
            remaining -= 1;
            continue;
        }
        out.push(part.clone());
    }
    out
}

/// Components back into the bytes a patch line carries.
fn join(parts: &[Part]) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        match part {
            Part::Root => out.push(b'/'),
            Part::Parent => out.extend_from_slice(b"../"),
            Part::Name(name) => {
                out.extend_from_slice(name);
                out.push(b'/');
            }
        }
    }
    while out.last() == Some(&b'/') {
        out.pop();
    }
    out
}

/// A header line's rest after `prefix`, when the line opens with it.
fn after<'a>(line: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    if line.starts_with(prefix) {
        Some(&line[prefix.len()..])
    } else {
        None
    }
}

fn strip_newline(line: &[u8]) -> &[u8] {
    let end = line.len() - usize::from(line.ends_with(b"\n"));
    &line[..end]
}

/// One path token from a header line: its remainder with git's trailing tab comment
/// dropped (`--- a/x\t2024-01-01 10:00:00 +0100`) and any padding around it.
fn one_path(rest: &[u8]) -> &[u8] {
    let end = rest.iter().position(|b| *b == b'\t').unwrap_or(rest.len());
    trim(&rest[..end])
}

/// Both ends of a byte slice trimmed of the spaces and tabs git pads a header line with.
/// A patch path may *contain* a space — which is why quoting exists — but never begin or
/// end with one.
fn trim(bytes: &[u8]) -> &[u8] {
    let pad = |b: &u8| *b == b' ' || *b == b'\t';
    let start = bytes.iter().position(|b| !pad(b)).unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !pad(b))
        .map(|i| i + 1)
        .unwrap_or(start);
    &bytes[start..end]
}

/// The two paths on a `diff --git` line. A quoted pair splits on its closing quote; an
/// unquoted pair splits at git's own ` b/` marker, the rule the engine's reader already
/// uses.
fn split_section_paths(rest: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let rest = trim(rest);
    if let Some(inner) = after(rest, b"\"")
        && let Some(end) = find_quote(inner)
    {
        let old = unescape(&inner[..end]);
        let tail = trim(&inner[end + 1..]);
        let new = tail.strip_prefix(&[b'"'][..]).unwrap_or(tail);
        let new = new.strip_suffix(&[b'"'][..]).unwrap_or(new);
        return (old, unescape(new));
    }
    match rest.windows(3).position(|w| w == b" b/") {
        Some(sep) => (rest[..sep].to_vec(), rest[sep + 1..].to_vec()),
        None => (rest.to_vec(), rest.to_vec()),
    }
}

fn find_quote(bytes: &[u8]) -> Option<usize> {
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

/// git's C-style quoting undone — the same octal-and-escape grammar its reader resolves,
/// kept byte-exact so a non-UTF-8 name survives a rewrite of the line carrying it.
fn unescape(bytes: &[u8]) -> Vec<u8> {
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
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'\\' => out.push(b'\\'),
            b'"' => out.push(b'"'),
            d @ b'0'..=b'7' => {
                let mut value = d - b'0';
                for _ in 0..2 {
                    if matches!(bytes.get(at), Some(b'0'..=b'7')) {
                        value = value * 8 + (bytes[at] - b'0');
                        at += 1;
                    } else {
                        break;
                    }
                }
                out.push(value);
            }
            other => out.push(other),
        }
    }
    out
}

/// A header token's bytes with git's `a/`/`b/` side marker and any quoting removed.
fn without_side(raw: &[u8], marker: &[u8]) -> Vec<u8> {
    let bare = unquote(raw);
    if !marker.is_empty() && bare.starts_with(marker) {
        bare[marker.len()..].to_vec()
    } else {
        bare
    }
}

/// A quoted header token resolved to its bytes; anything else is already plain.
fn unquote(raw: &[u8]) -> Vec<u8> {
    if let Some(inner) = raw.strip_prefix(&[b'"'][..])
        && let Some(inner) = inner.strip_suffix(&[b'"'][..])
    {
        return unescape(inner);
    }
    raw.to_vec()
}

/// The bytes that force quoting on a header line: a space, a quote, or something a line
/// reader would not treat as part of a path.
fn needs_quote(path: &[u8]) -> bool {
    path.iter().any(|b| *b == b' ' || *b == b'"' || *b < 0x20)
}

/// git's C-style quoting of a path, byte for byte.
fn escape(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 4);
    for b in bytes {
        match b {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            other if *other < 0x20 || *other > 0x7e => {
                out.extend_from_slice(format!("\\{other:03o}").as_bytes());
            }
            other => out.push(*other),
        }
    }
    out
}

//! One comment- and string-aware scanner for the suites that assert
//! architectural ratchets by reading production source.
//!
//! Roughly forty tests in this workspace assert a rule about code that does not
//! exist yet, and they can only do that by reading the file: "one function
//! produces each of the three chips", "no screen paints a hard-coded rail width",
//! "the tool-window header is retired from the shared surface". Every one of them
//! needs the same three things, and this module is the one place they live:
//!
//! 1. **a comment-aware projection** — a rule about what the code *does* must not
//!    be satisfied by a doc comment describing the opposite, and a `//` inside a
//!    string literal must not hide a line from it;
//! 2. **two string modes**, because they are two different questions and not a
//!    duplicate to be merged — see [`Strings`];
//! 3. **column preservation** — a masked byte becomes a *space*, never a deletion,
//!    so a line number and a column offset in a scan's output are the caller's to
//!    report and a declaration cannot slide under a match because a comment above
//!    it got shorter.
//!
//! ## The failure mode this module is shaped around
//!
//! Almost every ratchet assertion is `assert!(!scan.contains(needle))` or
//! `assert_eq!(found.len(), 1)` over a set the scanner produced, so **an empty
//! result satisfies all of them**. A stripper with a bug that blanks a whole file
//! produces a green run, and the green run is the dangerous outcome: the mutation
//! that motivated this module replaced `CommitTable::ROW_HEIGHT` with a literal
//! `20.0` while the file stayed full of prose naming it, and the broken scan
//! passed the ratchet written to catch exactly that. `tests/srcscan_selfcheck.rs`
//! is the mitigation, and its *positive* halves are the part that matters.
//!
//! ## The inputs that broke the four copies this replaced
//!
//! All four are real occurrences in this repository, not hypotheticals:
//!
//! - `"release/*"` — a block-comment opener with **no closer** inside a string
//!   literal (`ui/settings_modal.rs`). A stripper that opens a block comment
//!   there blanks the next 250 lines.
//! - `"https://…"` — a line-comment opener inside a string literal
//!   (`ui/remotes_dialog.rs`).
//! - `&TreeProps<'_>` — a lifetime that is *syntactically* a char literal
//!   (`ui/branch_tree_view.rs`); `'a` is not a char.
//! - `b'\0'`, `b'M' => b'L'` — byte char literals, and `c == '"'` in
//!   `turbogit-services/src/bulk_ops.rs`, whose `"` would open a string that never
//!   closes.
//! - `r#"…"#` — a raw string whose body is not Rust at all.
//!
//! ## What this module deliberately does not do
//!
//! It does not parse Rust. It recognises the lexical shapes a comment can hide
//! behind and nothing more: a `pub fn` head is still matched by prefix, so
//! `pub async fn` and `pub const fn` are not *declaration* heads to
//! [`declares`], exactly as they were not to the copies this replaced. Widening
//! that is a policy change for the ratchets to make deliberately, not a parsing
//! fix to slip in here.

use std::path::{Path, PathBuf};

/// What a scan is allowed to see of a **string literal**.
///
/// Two modes because they answer two different questions, and a scan in the
/// wrong one cannot see the thing it is looking for:
///
/// - [`Strings::Keep`] — a column's label and a table's name *are* string
///   literals. A scan that blanked them could not see that a column exists at
///   all, so a claim about a literal ("the ROOTS label is an entry of the commit
///   table's own column table") has to be made against this projection.
/// - [`Strings::Blank`] — the seam for a claim about **identifiers and calls**.
///   A file that formats a command line into a string can otherwise satisfy a
///   ratchet about the commands it *runs*.
///
/// Comment blanking is identical in both; only the literal projection differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strings {
    /// String literals survive verbatim, quotes included.
    Keep,
    /// Every byte of every string literal is blanked; the delimiters survive, so
    /// a scan can still see that a literal is there.
    Blank,
}

/// `src` with every comment masked and, in [`Strings::Blank`], every string
/// literal masked — **one space per masked byte**, and a newline is never masked,
/// so every line number and every column offset in the result is the same as in
/// `src`.
///
/// The masking is what makes column preservation load-bearing rather than
/// incidental: a declaration's column is where a caller looked for it, and a
/// scanner that *deleted* comment bytes would move every column after the first
/// comment in the file.
///
/// Handles line comments, **nested** block comments, `//` and `/*` inside string
/// literals, raw strings (`r"…"`, `r#"…"#`), byte and C strings, and the
/// difference between a char literal and a lifetime.
pub fn blank_comments(src: &str, strings: Strings) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            out.push(' ');
            out.push(' ');
            i += 2;
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i = mask_block(&chars, i, &mut out);
        } else if let Some(literal) = Literal::at(&chars, i) {
            i = literal.copy_through(&chars, i, strings, &mut out);
        } else {
            out.push(c);
            i += 1;
        }
    }
    out.into_iter().collect()
}

/// [`blank_comments`] in the identifier-and-call projection.
pub fn code_only(src: &str) -> String {
    blank_comments(src, Strings::Blank)
}

/// [`blank_comments`] in the literal-and-identifier projection.
pub fn code_with_literals(src: &str) -> String {
    blank_comments(src, Strings::Keep)
}

/// [`blank_comments`] split into lines, for a scan that matches a declaration or
/// a call per line.
pub fn blank_lines(src: &str, strings: Strings) -> Vec<String> {
    blank_comments(src, strings)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// One source file, comment-masked and split into lines — the shape a
/// per-line declaration/call scan wants, and the reason a comment cannot be
/// counted as a declaration.
pub fn code_lines(path: impl AsRef<Path>) -> Vec<String> {
    blank_lines(&read_source(path.as_ref()), Strings::Keep)
}

/// Read a file as Rust source, naming it in the panic: a ratchet that cannot
/// read the file it is about must say which one.
pub fn read_source(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

/// Every `.rs` file under `dir`, recursively, in directory order.
///
/// Directory order, not sorted: the callers that care about order sort
/// afterwards, and a walk whose order changed would move a reported line number
/// for no reason.
pub fn rust_files(dir: impl AsRef<Path>) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir.as_ref(), &mut out);
    out
}

/// Every `.rs` file under `base` as `(path relative to `base`, source)`, sorted
/// by path.
///
/// The "read it at *run* time, not through a hand-maintained `include_str!` list"
/// walk: the claim such a scan makes is "nowhere", and a list of the files that
/// were checked when the ratchet was written is a list that silently stops
/// covering new modules. Panics if the walk found nothing, because an empty scan
/// satisfies every negative assertion in the tree.
pub fn sources_under(base: impl AsRef<Path>) -> Vec<(String, String)> {
    let base = base.as_ref();
    let mut out = Vec::new();
    for path in rust_files(base) {
        let rel = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push((rel, read_source(&path)));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(
        !out.is_empty(),
        "the source walk found nothing under {}",
        base.display()
    );
    out
}

/// Every `crates/*/src/**/*.rs` in the workspace, as `(crate name, path)`, sorted
/// by path.
///
/// The walk behind the port-discipline rule. It is here because the *walk* is
/// generic; **which crates the rule applies to is not** — that is a policy
/// decision and belongs beside the rule, not in a shared parser.
pub fn crate_sources(root: impl AsRef<Path>) -> Vec<(String, PathBuf)> {
    fn collect(dir: &Path, krate: &str, out: &mut Vec<(String, PathBuf)>) {
        for entry in std::fs::read_dir(dir).expect("read_dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect(&path, krate, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push((krate.to_string(), path));
            }
        }
    }
    let root = root.as_ref();
    let mut out = Vec::new();
    for crate_dir in std::fs::read_dir(root.join("crates"))
        .expect("crates/")
        .flatten()
        .filter(|e| e.path().is_dir())
    {
        let src = crate_dir.path().join("src");
        if src.is_dir() {
            let krate = crate_dir.file_name().to_string_lossy().into_owned();
            collect(&src, &krate, &mut out);
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Whether `line` **declares** `name` as a function, rather than calling one.
///
/// Prefix-matched, which is what the ratchets have always meant: a head is
/// `pub fn `, `pub(crate) fn ` or `fn ` and nothing else.
pub fn declares(line: &str, name: &str) -> bool {
    let trimmed = line.trim_start();
    for prefix in ["pub fn ", "pub(crate) fn ", "fn "] {
        if let Some(rest) = trimmed.strip_prefix(prefix)
            && let Some(ident) = rest.split('(').next()
        {
            return ident.trim() == name;
        }
    }
    false
}

/// Whether `line` **calls** `name`: the name followed by `(`, and not part of a
/// longer identifier — `paint_rail(` and `rows::paint_rail(` are calls,
/// `my_paint_rail(` and `paint_rail_rect(` are not.
pub fn calls(line: &str, name: &str) -> bool {
    let mut from = 0;
    while let Some(offset) = line[from..].find(name) {
        let at = from + offset;
        let after = line[at + name.len()..].chars().next();
        let before = line[..at].chars().next_back().unwrap_or(' ');
        if after == Some('(') && !before.is_alphanumeric() && before != '_' {
            return true;
        }
        from = at + name.len();
    }
    false
}

/// The 1-based lines of `src` at which `name` is **called and not declared**,
/// with comments masked — a doc comment cannot be a call site, and a mention of
/// a name inside a string literal cannot be one either.
pub fn call_lines(src: &str, name: &str) -> Vec<usize> {
    blank_lines(src, Strings::Keep)
        .iter()
        .enumerate()
        .filter(|(_, line)| !declares(line, name) && calls(line, name))
        .map(|(index, _)| index + 1)
        .collect()
}

/// Every call of `name` in the `.rs` files under `dir`, as `(path relative to
/// `dir`, 1-based line)`, in walk order.
pub fn call_sites(dir: impl AsRef<Path>, name: &str) -> Vec<(String, usize)> {
    let dir = dir.as_ref();
    let mut out = Vec::new();
    for path in rust_files(dir) {
        let rel = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        for line in call_lines(&read_source(&path), name) {
            out.push((rel.clone(), line));
        }
    }
    out
}

/// The `pub fn`s `src` declares, in source order — a module's public surface,
/// read out of the module rather than restated beside it.
pub fn declared_fns(src: &str) -> Vec<String> {
    blank_lines(src, Strings::Keep)
        .iter()
        .filter_map(|line| {
            line.trim_start()
                .strip_prefix("pub fn ")
                .and_then(|rest| rest.split('(').next())
                .map(|ident| ident.trim().to_owned())
        })
        .collect()
}

/// The raw source text of `fn <name>`, from the `fn` keyword through the first
/// column-zero `}` that follows it.
///
/// The span rule, deliberately: a column-zero terminator cannot be fooled by a
/// brace inside a string literal or a nested block, where a brace *count* can.
/// It also means a wrapped signature is not read whole, so use [`fn_code`] when
/// the claim is about a body rather than about a signature. Comments are
/// **kept** — this is the seam for a claim about the file's own reasoning; pair
/// it with [`code_with_literals`] for a claim about what the function calls.
pub fn fn_text(src: &str, name: &str) -> String {
    let start = src
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("no `fn {name}` in the source"));
    let rest = &src[start..];
    let end = rest
        .find("\n}\n")
        .unwrap_or_else(|| panic!("`fn {name}` has no closing brace at column zero"))
        + 3;
    rest[..end].to_owned()
}

/// [`fn_text`] with the comments masked — the seam for a claim about what a
/// function *calls*.
pub fn fn_text_code(src: &str, name: &str) -> String {
    code_with_literals(&fn_text(src, name))
}

/// The code of `fn <name>`, from its declaration line through its closing brace.
///
/// Comment-masked, so a claim about what a body *does* cannot be satisfied by a
/// doc comment describing the opposite, and a `//` inside a string literal cannot
/// hide a line from it. Brace-counted rather than matched on a column, so a
/// wrapped signature or a nested block is read whole; the count runs over the
/// [`Strings::Blank`] projection, so a `{}` inside a `format!` is not a block.
/// The declaration line's leading `pub` / `pub(crate)` is included, because the
/// claim being made is about the declaration as written.
pub fn fn_code(src: &str, name: &str) -> String {
    let lines = blank_lines(src, Strings::Blank);
    let start = lines
        .iter()
        .position(|line| declares(line, name))
        .unwrap_or_else(|| panic!("no `fn {name}` in the source"));
    let mut depth = 0i32;
    let mut opened = false;
    let mut body = String::new();
    for line in &lines[start..] {
        for c in line.chars() {
            match c {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        body.push_str(line);
        body.push('\n');
        if opened && depth == 0 {
            return body;
        }
    }
    panic!("`fn {name}` never closes");
}

/// One top-level item of a module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleItem {
    /// The name the head declares.
    pub name: String,
    /// The item's own lines, head to terminator.
    pub text: String,
    /// Whether the head was a function rather than a constant.
    pub is_fn: bool,
}

/// The name an item head declares, with the generic parameter list, the argument
/// list and a `const`'s type annotation dropped: `pane_header<R>` declares
/// `pane_header`, and `pub const PANE_HEADER_HEIGHT: f32 = 28.0;` declares
/// `PANE_HEADER_HEIGHT`.
fn declared_name(rest: &str) -> String {
    rest.split(['(', '<', ':'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// Every top-level item of `src`, in source order.
///
/// Reads `src` **as given**: the item's own text includes whatever comments sit
/// inside it, and a caller that wants a claim about code rather than about
/// commentary passes an already-masked projection. A doc comment is not itself an
/// item, because a head is a `pub fn` / `fn` / `pub const` / `const` line.
pub fn top_level_items(src: &str) -> Vec<ModuleItem> {
    let lines: Vec<&str> = src.lines().collect();
    // The declared name of a column-0 `pub fn` / `fn` / `pub const` / `const`
    // head, or `None` for any other line.
    let head_name = |line: &str| -> Option<(String, bool)> {
        let trimmed = line.trim_start();
        for prefix in ["pub fn ", "fn "] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                return Some((declared_name(rest), true));
            }
        }
        for prefix in ["pub const ", "const "] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                return Some((declared_name(rest), false));
            }
        }
        None
    };

    let mut items = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        // Column 0 only: an indented head belongs to the enclosing `impl`, and
        // an `impl` member is not a module-level item.
        let head = if line.starts_with(' ') || line.starts_with('\t') {
            None
        } else {
            head_name(line)
        };
        let Some((name, is_fn)) = head else {
            i += 1;
            continue;
        };
        // A one-line `const` ends with its own `;`; a `fn` or a block-bodied
        // `const` ends at the column-0 `}` (or `};`) that closes it.
        let end = if line.trim_end().ends_with(';') {
            Some(i)
        } else {
            lines
                .iter()
                .enumerate()
                .skip(i + 1)
                .find(|(_, tail)| matches!(tail.trim_end(), "}" | "};"))
                .map(|(j, _)| j)
        };
        let end = end.unwrap_or_else(|| {
            panic!(
                "module item `{name}` has no column-0 terminator; the structural \
                 ratchets assume the rustfmt layout, so they refuse to pass on a \
                 hand-rolled one"
            )
        });
        items.push(ModuleItem {
            name,
            text: lines[i..=end].join("\n"),
            is_fn,
        });
        i = end + 1;
    }
    items
}

/// Byte offsets at which `needle` occurs in `text` as a whole identifier, so a
/// search for `Palette::RAISED` does not also match `Palette::RAISED_ON_CARD`.
pub fn word_offsets(text: &str, needle: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut hits = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(needle) {
        let at = from + rel;
        let after = at + needle.len();
        let bounded_before = at == 0 || !is_ident(bytes[at - 1]);
        let bounded_after = after >= bytes.len() || !is_ident(bytes[after]);
        if bounded_before && bounded_after {
            hits.push(at);
        }
        from = at + needle.len();
    }
    hits
}

/// The names of every top-level item of `src` that names `needle` in its own
/// text, sorted — so a ratchet reads as a claim about a set rather than about an
/// incidental source order.
pub fn items_naming(src: &str, needle: &str) -> Vec<String> {
    let mut names: Vec<String> = top_level_items(src)
        .into_iter()
        .filter(|item| !word_offsets(&item.text, needle).is_empty())
        .map(|item| item.name)
        .collect();
    names.sort();
    names
}

/// The names of every **function** in `src` that *uses* `ident`, sorted.
///
/// The item that declares `ident` is excluded, so asking "who calls
/// `chip_with`" answers with the callers rather than with `chip_with` itself.
pub fn fns_using(src: &str, ident: &str) -> Vec<String> {
    let mut names: Vec<String> = top_level_items(src)
        .into_iter()
        .filter(|item| item.is_fn && item.name != ident)
        .filter(|item| !word_offsets(&item.text, ident).is_empty())
        .map(|item| item.name)
        .collect();
    names.sort();
    names
}

// --- the literal scanner ------------------------------------------------------

/// A Rust literal that a comment can hide inside, recognised lexically.
///
/// The point of recognising a char literal at all is the `"` in `c == '"'`: a
/// scanner that only knew about `"` would open a string there and never close
/// it. The point of *not* mis-reading a lifetime is the `'` in `&TreeProps<'_>`:
/// a char literal is exactly one character (or one escape) between quotes, and
/// `'_>` is two, so it is a lifetime and stays code.
enum Literal {
    /// `"…"` — optionally behind a `b` or `c` prefix.
    Plain { opener: usize },
    /// `r"…"`, `r#"…"#`, `br##"…"##`.
    Raw { hashes: usize, opener: usize },
    /// `'x'`, `'\n'`, `'\u{fffd}'`, `b'x'`.
    Char { opener: usize },
}

impl Literal {
    /// The literal opening at `chars[i]`, if any.
    fn at(chars: &[char], i: usize) -> Option<Self> {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        match c {
            'b' | 'c' => match next {
                Some('r') => Self::raw(chars, i + 1).map(|(hashes, opener)| Self::Raw {
                    hashes,
                    opener: opener + 1,
                }),
                Some('"') => Some(Self::Plain { opener: 2 }),
                Some('\'') => Some(Self::Char { opener: 2 }),
                _ => None,
            },
            'r' => Self::raw(chars, i).map(|(hashes, opener)| Self::Raw { hashes, opener }),
            '"' => Some(Self::Plain { opener: 1 }),
            '\'' => Some(Self::Char { opener: 1 }),
            _ => None,
        }
    }

    /// A raw string at `chars[i]` (which is the `r`), as `(hash count, opener
    /// length)` — the length being the `r`, the hashes and the opening quote.
    fn raw(chars: &[char], i: usize) -> Option<(usize, usize)> {
        let mut hashes = 0;
        while chars.get(i + 1 + hashes) == Some(&'#') {
            hashes += 1;
        }
        (chars.get(i + 1 + hashes) == Some(&'"')).then_some((hashes, 1 + hashes + 1))
    }

    /// Copy the literal at `chars[i]` into `out` under `strings`, and return the
    /// index just past it.
    ///
    /// A char literal's bytes are copied **verbatim in both modes**. Its content
    /// is at most one character, so it can never be an identifier or a call name
    /// and masking it would buy nothing — while masking it wrongly (by reading
    /// `'_` as the literal `'_'`) would blank real code. The delimiter is kept so
    /// a scan can still see that a literal is there.
    fn copy_through(
        &self,
        chars: &[char],
        i: usize,
        strings: Strings,
        out: &mut Vec<char>,
    ) -> usize {
        let opener = match *self {
            Self::Plain { opener } | Self::Raw { opener, .. } | Self::Char { opener } => opener,
        };
        let content_from = i + opener;
        let content_to = match *self {
            Self::Plain { .. } => plain_end(chars, content_from),
            Self::Raw { hashes, .. } => raw_end(chars, content_from, hashes),
            Self::Char { .. } => match char_end(chars, content_from) {
                Some(end) => end,
                // A lifetime, not a literal. The quote is code and **nothing else
                // is consumed**: a scanner that kept looking for a partner quote
                // would swallow the rest of the file, which is the whole failure
                // mode this module exists to not have.
                None => {
                    out.extend(chars[i..content_from].iter());
                    return content_from;
                }
            },
        };
        for c in &chars[i..content_from] {
            out.push(*c);
        }
        // `content_to` is one past the closing delimiter, so the content range
        // already includes it — which is why the delimiter survives
        // `Strings::Blank` alongside the opening one.
        for c in &chars[content_from..content_to] {
            out.push(mask_byte(*c, strings, matches!(self, Self::Char { .. })));
        }
        content_to
    }
}

/// One byte of a literal's content under `strings`.
///
/// Newlines are structure rather than content in every mode, so a multi-line raw
/// string keeps its line numbers whatever the projection. A char literal's
/// content is masked in neither mode: it is at most one character, so it can
/// never be an identifier or a call name, and masking it wrongly — by reading
/// `'_` as the literal `'_'` — would blank real code.
fn mask_byte(c: char, strings: Strings, is_char: bool) -> char {
    match (strings, c) {
        (_, '\n') => '\n',
        (Strings::Blank, _) if !is_char => ' ',
        (_, c) => c,
    }
}

/// The index just past a `"…"` literal's closing quote, honouring `\\` so a
/// `\"` does not close it early.
fn plain_end(chars: &[char], from: usize) -> usize {
    let mut i = from;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            '"' => return i + 1,
            _ => i += 1,
        }
    }
    chars.len()
}

/// The index just past a raw literal's closing `"` + `hashes` `#`.
fn raw_end(chars: &[char], from: usize, hashes: usize) -> usize {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == '"' && (0..hashes).all(|k| chars.get(i + 1 + k) == Some(&'#')) {
            return i + 1 + hashes;
        }
        i += 1;
    }
    chars.len()
}

/// The index just past a char literal's closing `'`, or `None` when what follows
/// the quote is **not** a char literal — the lifetime case.
///
/// One character between the quotes, or one escape: `'\n'`, `'\\'`,
/// `'\u{fffd}'`. A single `'` followed by anything else — `'_`, `'a` — opens no
/// literal, so the caller must not consume the rest of the file looking for a
/// partner quote.
fn char_end(chars: &[char], from: usize) -> Option<usize> {
    let mut i = from;
    if chars.get(i) == Some(&'\\') {
        i += 1;
        if chars.get(i) == Some(&'u') && chars.get(i + 1) == Some(&'{') {
            i += 2;
            while i < chars.len() && chars[i] != '}' {
                i += 1;
            }
            i += 1;
        } else {
            i += 1;
        }
    } else {
        match chars.get(i) {
            // An unterminated `'` is a lifetime's, not a literal's.
            None | Some('\'') | Some('\n') => return None,
            Some(_) => i += 1,
        }
    }
    (chars.get(i) == Some(&'\'')).then_some(i + 1)
}

/// Mask a `/* … */` comment, nested, and return the index just past it.
fn mask_block(chars: &[char], i: usize, out: &mut Vec<char>) -> usize {
    let mut depth = 0i32;
    let mut j = i;
    while j < chars.len() {
        let c = chars[j];
        if c == '/' && chars.get(j + 1) == Some(&'*') {
            depth += 1;
            out.push(' ');
            out.push(' ');
            j += 2;
        } else if c == '*' && chars.get(j + 1) == Some(&'/') {
            depth -= 1;
            out.push(' ');
            out.push(' ');
            j += 2;
            if depth == 0 {
                return j;
            }
        } else {
            out.push(if c == '\n' { '\n' } else { ' ' });
            j += 1;
        }
    }
    j
}

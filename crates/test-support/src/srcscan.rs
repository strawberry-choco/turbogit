use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strings {
    Keep,
    Blank,
}

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

pub fn code_only(src: &str) -> String {
    blank_comments(src, Strings::Blank)
}

pub fn code_with_literals(src: &str) -> String {
    blank_comments(src, Strings::Keep)
}

pub fn blank_lines(src: &str, strings: Strings) -> Vec<String> {
    blank_comments(src, strings)
        .lines()
        .map(str::to_owned)
        .collect()
}

pub fn code_lines(path: impl AsRef<Path>) -> Vec<String> {
    blank_lines(&read_source(path.as_ref()), Strings::Keep)
}

pub fn read_source(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

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

pub fn call_lines(src: &str, name: &str) -> Vec<usize> {
    blank_lines(src, Strings::Keep)
        .iter()
        .enumerate()
        .filter(|(_, line)| !declares(line, name) && calls(line, name))
        .map(|(index, _)| index + 1)
        .collect()
}

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

pub fn fn_text_code(src: &str, name: &str) -> String {
    code_with_literals(&fn_text(src, name))
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleItem {
    pub name: String,
    pub text: String,
    pub is_fn: bool,
}

fn declared_name(rest: &str) -> String {
    rest.split(['(', '<', ':'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned()
}

pub fn top_level_items(src: &str) -> Vec<ModuleItem> {
    let lines: Vec<&str> = src.lines().collect();
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
        let head = if line.starts_with(' ') || line.starts_with('\t') {
            None
        } else {
            head_name(line)
        };
        let Some((name, is_fn)) = head else {
            i += 1;
            continue;
        };
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

pub fn items_naming(src: &str, needle: &str) -> Vec<String> {
    let mut names: Vec<String> = top_level_items(src)
        .into_iter()
        .filter(|item| !word_offsets(&item.text, needle).is_empty())
        .map(|item| item.name)
        .collect();
    names.sort();
    names
}

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

enum Literal {
    Plain { opener: usize },
    Raw { hashes: usize, opener: usize },
    Char { opener: usize },
}

impl Literal {
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

    fn raw(chars: &[char], i: usize) -> Option<(usize, usize)> {
        let mut hashes = 0;
        while chars.get(i + 1 + hashes) == Some(&'#') {
            hashes += 1;
        }
        (chars.get(i + 1 + hashes) == Some(&'"')).then_some((hashes, 1 + hashes + 1))
    }

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
                None => {
                    out.extend(chars[i..content_from].iter());
                    return content_from;
                }
            },
        };
        for c in &chars[i..content_from] {
            out.push(*c);
        }
        for c in &chars[content_from..content_to] {
            out.push(mask_byte(*c, strings, matches!(self, Self::Char { .. })));
        }
        content_to
    }
}

fn mask_byte(c: char, strings: Strings, is_char: bool) -> char {
    match (strings, c) {
        (_, '\n') => '\n',
        (Strings::Blank, _) if !is_char => ' ',
        (_, c) => c,
    }
}

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
            None | Some('\'') | Some('\n') => return None,
            Some(_) => i += 1,
        }
    }
    (chars.get(i) == Some(&'\'')).then_some(i + 1)
}

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

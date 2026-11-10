//! Syntax colours for the Diff tab: each line's tokens by kind, worked out
//! once when a diff loads (off the UI thread), drawn in the theme's colours
//! (`theme::syntax`). Sublime grammars through syntect, pure Rust.

use std::ops::Range;
use std::sync::OnceLock;

use syntect::parsing::{ParseState, Scope, ScopeStack, ScopeStackOp, SyntaxReference, SyntaxSet};

use crate::github::{File, Line};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Keyword,
    String,
    Comment,
    Number,
    /// A function's or a type's name.
    Name,
}

/// A run of a line's bytes and what it is.
pub type Span = (Range<usize>, Kind);

/// Past this many diff lines a file stays plain: its colours are not worth
/// the wait.
const MAX_LINES: usize = 5000;

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// The grammar for a path, by extension or file name; `None` stays plain.
fn syntax_for(path: &str) -> Option<&'static SyntaxReference> {
    let set = syntaxes();
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map_or(name, |(_, ext)| ext);
    // The default grammars have no TypeScript: JavaScript's reads it well.
    let ext = match ext {
        "ts" | "tsx" | "mts" | "cts" | "jsx" | "mjs" | "cjs" => "js",
        "zsh" | "bash" => "sh",
        "yml" => "yaml",
        other => other,
    };
    set.find_syntax_by_extension(ext)
        .or_else(|| set.find_syntax_by_extension(name))
        .filter(|s| s.name != "Plain Text")
}

/// Whether `file` gets colours at all: a known language, not too long.
pub fn colours(file: &File) -> bool {
    file.lines.len() <= MAX_LINES && syntax_for(&file.path).is_some()
}

/// One side of a diff (old or new) read top to bottom, so a string or a
/// comment over several lines stays one.
struct Reader {
    syntax: &'static SyntaxReference,
    state: ParseState,
    stack: ScopeStack,
}

impl Reader {
    fn new(syntax: &'static SyntaxReference) -> Self {
        Self {
            syntax,
            state: ParseState::new(syntax),
            stack: ScopeStack::new(),
        }
    }

    /// A new hunk: what came before it is not in the diff.
    fn reset(&mut self) {
        *self = Self::new(self.syntax);
    }

    fn line(&mut self, text: &str) -> Vec<Span> {
        let with_newline = format!("{text}\n");
        let Ok(ops) = self.state.parse_line(&with_newline, syntaxes()) else {
            self.reset();
            return Vec::new();
        };
        let mut spans: Vec<Span> = Vec::new();
        let mut at = 0;
        let mut flush = |to: usize, stack: &ScopeStack, spans: &mut Vec<Span>| {
            let to = to.min(text.len());
            if at < to
                && let Some(kind) = kind_of(stack.as_slice())
            {
                match spans.last_mut() {
                    Some((range, k)) if *k == kind && range.end == at => range.end = to,
                    _ => spans.push((at..to, kind)),
                }
            }
            at = at.max(to);
        };
        for (pos, op) in ops {
            flush(pos, &self.stack, &mut spans);
            apply(&mut self.stack, &op);
        }
        flush(text.len(), &self.stack, &mut spans);
        spans
    }
}

fn apply(stack: &mut ScopeStack, op: &ScopeStackOp) {
    // A grammar error only costs this line its colours.
    let _ = stack.apply(op);
}

/// The innermost scope that names a kind wins.
fn kind_of(scopes: &[Scope]) -> Option<Kind> {
    scopes.iter().rev().find_map(|scope| {
        let name = scope.build_string();
        let is = |prefix: &str| name.starts_with(prefix);
        if is("comment") {
            Some(Kind::Comment)
        } else if is("string") {
            Some(Kind::String)
        } else if is("constant.numeric") || is("constant.language") {
            Some(Kind::Number)
        } else if is("keyword") || is("storage") {
            Some(Kind::Keyword)
        } else if is("entity.name") || is("support.function") || is("support.type") {
            Some(Kind::Name)
        } else {
            None
        }
    })
}

/// Each diff line's spans, in the file's order: removed lines read as the
/// old file, added ones as the new, unchanged ones as both. Empty when the
/// language is unknown or the file too long. With `head` (the whole new
/// file) both sides are read from the file's first line, so a hunk that
/// starts inside a docstring or a block comment reads as one; without it
/// each hunk starts fresh.
pub fn spans(file: &File, head: Option<&str>) -> Vec<Vec<Span>> {
    let Some(syntax) = syntax_for(&file.path).filter(|_| file.lines.len() <= MAX_LINES) else {
        return Vec::new();
    };
    if let Some(whole) = head.and_then(|head| whole(file, syntax, head)) {
        return whole;
    }
    let (mut old, mut new) = (Reader::new(syntax), Reader::new(syntax));
    file.lines
        .iter()
        .map(|line| match line {
            Line::Hunk(_) => {
                old.reset();
                new.reset();
                Vec::new()
            }
            Line::Removed(text) => old.line(text),
            Line::Added(text) => new.line(text),
            Line::Context(text) => {
                old.line(text);
                new.line(text)
            }
        })
        .collect()
}

/// Past this many lines a whole file is not read for its colours.
const MAX_FILE_LINES: usize = 20_000;

/// The spans read from the whole old and new file; `None` when the diff
/// does not fit `head` (then the hunks read on their own).
fn whole(file: &File, syntax: &'static SyntaxReference, head: &str) -> Option<Vec<Vec<Span>>> {
    let new: Vec<&str> = head.lines().collect();
    if new.len() > MAX_FILE_LINES {
        return None;
    }
    let old = old_file(file, &new)?;
    let read = |lines: &[&str]| {
        let mut reader = Reader::new(syntax);
        lines.iter().map(|l| reader.line(l)).collect::<Vec<_>>()
    };
    let (old, new) = (read(&old), read(&new));
    let (mut o, mut n) = (0, 0);
    let mut out = Vec::with_capacity(file.lines.len());
    for line in &file.lines {
        out.push(match line {
            Line::Hunk(h) => {
                (o, n) = hunk_starts(h)?;
                Vec::new()
            }
            Line::Removed(_) => {
                o += 1;
                old.get(o - 1)?.clone()
            }
            Line::Added(_) => {
                n += 1;
                new.get(n - 1)?.clone()
            }
            Line::Context(_) => {
                o += 1;
                n += 1;
                new.get(n - 1)?.clone()
            }
        });
    }
    Some(out)
}

/// Where a hunk starts on each side, as 0-based line indexes.
fn hunk_starts(hunk: &str) -> Option<(usize, usize)> {
    let start = |sign: char| -> Option<usize> {
        let part = hunk.split_whitespace().find(|p| p.starts_with(sign))?;
        let mut it = part[1..].split(',');
        let at: usize = it.next()?.parse().ok()?;
        let count: usize = it.next().map_or(Some(1), |c| c.parse().ok())?;
        // An empty range names the line before it.
        Some(if count == 0 { at } else { at.saturating_sub(1) })
    };
    Some((start('-')?, start('+')?))
}

/// The old file, rebuilt from the new one and the diff: outside the hunks
/// both are the same. `None` when the diff's lines are not in `new`.
fn old_file<'a>(file: &'a File, new: &[&'a str]) -> Option<Vec<&'a str>> {
    let mut old = Vec::with_capacity(new.len());
    let mut at = 0;
    for line in &file.lines {
        match line {
            Line::Hunk(h) => {
                let (_, from) = hunk_starts(h)?;
                old.extend_from_slice(new.get(at..from)?);
                at = from;
            }
            Line::Context(t) => {
                (*new.get(at)? == t.as_str()).then_some(())?;
                old.push(t.as_str());
                at += 1;
            }
            Line::Removed(t) => old.push(t.as_str()),
            Line::Added(t) => {
                (*new.get(at)? == t.as_str()).then_some(())?;
                at += 1;
            }
        }
    }
    old.extend_from_slice(new.get(at..)?);
    Some(old)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, lines: Vec<Line>) -> File {
        File {
            path: path.into(),
            added: 0,
            removed: 0,
            lines,
            spans: Vec::new(),
        }
    }

    fn kinds(text: &str, spans: &[Span]) -> Vec<(String, Kind)> {
        spans
            .iter()
            .map(|(r, k)| (text[r.clone()].to_owned(), *k))
            .collect()
    }

    #[test]
    fn python_reads_as_keywords_strings_and_comments() {
        let lines = [
            "def scale(pods):  # how many",
            "    return \"\"\"many",
            "lines\"\"\"",
        ];
        let f = file(
            "fleet/foreman.py",
            vec![
                Line::Hunk("@@ -1 +1 @@".into()),
                Line::Added(lines[0].into()),
                Line::Added(lines[1].into()),
                Line::Added(lines[2].into()),
            ],
        );
        let spans = spans(&f, None);
        assert!(spans[0].is_empty(), "a hunk header stays plain");
        let first = kinds(lines[0], &spans[1]);
        assert!(first.contains(&("def".into(), Kind::Keyword)), "{first:?}");
        assert!(first.contains(&("scale".into(), Kind::Name)), "{first:?}");
        assert!(
            first
                .iter()
                .any(|(t, k)| t.contains("# how many") && *k == Kind::Comment),
            "{first:?}"
        );
        // A string that runs over two lines is a string on both.
        assert!(
            kinds(lines[1], &spans[2])
                .iter()
                .any(|(t, k)| t.contains("many") && *k == Kind::String)
        );
        assert_eq!(kinds(lines[2], &spans[3])[0].1, Kind::String);
    }

    #[test]
    fn an_unknown_or_huge_file_stays_plain() {
        let f = file(
            "notes.unknownext",
            vec![Line::Added("def x(): pass".into())],
        );
        assert!(spans(&f, None).is_empty());
        let big = file("a.py", vec![Line::Added("x = 1".into()); MAX_LINES + 1]);
        assert!(spans(&big, None).is_empty());
        // TypeScript reads with JavaScript's grammar.
        assert!(syntax_for("ui/src/App.tsx").is_some());
    }

    #[test]
    fn a_hunk_inside_a_docstring_reads_as_the_whole_file_does() {
        // The hunk starts on the docstring's closing quotes: alone it would
        // read them as an opening, with the whole file it is the end.
        let head = "def f():\n    \"\"\"Doc\n    more\n    \"\"\"\n    return 1\n";
        let f = file(
            "a.py",
            vec![
                Line::Hunk("@@ -4,2 +4,2 @@".into()),
                Line::Context("    \"\"\"".into()),
                Line::Removed("    return 0".into()),
                Line::Added("    return 1".into()),
            ],
        );
        let alone = spans(&f, None);
        // Python's grammar reads a docstring as a comment.
        let quoted = |k: &Kind| matches!(k, Kind::String | Kind::Comment);
        assert!(alone[3].iter().any(|(_, k)| quoted(k)), "{alone:?}");
        let whole = spans(&f, Some(head));
        for (n, text) in [(2, "    return 0"), (3, "    return 1")] {
            let kinds = kinds(text, &whole[n]);
            assert!(
                kinds.contains(&("return".into(), Kind::Keyword)),
                "{kinds:?}"
            );
            assert!(!kinds.iter().any(|(_, k)| quoted(k)), "{kinds:?}");
        }
        // A diff that does not fit the file falls back to the hunks alone.
        assert_eq!(spans(&f, Some("x = 1\n")), alone);
    }
}

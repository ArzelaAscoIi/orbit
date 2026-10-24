//! The plain text editor pane: one text, a path to save it to. Paste a
//! snippet, fix its indentation (Tab, Shift+Tab on the selected lines),
//! copy it all or save it. Nothing more on purpose.
//! ponytail: the text lives in memory only; a restart loses what was not
//! saved. Persist it when someone misses it.

use std::path::PathBuf;

#[derive(Default)]
pub struct Editor {
    pub text: String,
    /// Where Save writes: absolute, or `~/...`.
    pub path: String,
    /// The last save: "saved 14:02" or why it failed.
    pub said: String,
}

/// The selected lines (chars `start..end`) moved right by four spaces, or
/// left by the indent they share (at most four), so Python keeps its
/// structure. A blank line is left alone. Returns the text and the new
/// selection, which covers the whole lines it changed.
pub fn shift(text: &str, start: usize, end: usize, left: bool) -> (String, usize, usize) {
    let (start, end) = (start.min(end), start.max(end));
    // Each line's first char, and its text without the newline.
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut at = 0;
    for line in text.split('\n') {
        lines.push((at, line));
        at += line.chars().count() + 1;
    }
    let first = lines.iter().rposition(|(s, _)| *s <= start).unwrap_or(0);
    // A selection that ends at a line's start does not take that line.
    let last = lines
        .iter()
        .rposition(|(s, _)| *s < end || (*s <= end && start == end))
        .unwrap_or(first)
        .max(first);
    let blank = |l: &str| l.trim().is_empty();
    let shared = lines[first..=last]
        .iter()
        .filter(|(_, l)| !blank(l))
        .map(|(_, l)| l.chars().take_while(|c| *c == ' ').count())
        .min()
        .unwrap_or(0)
        .min(4);
    let mut out = Vec::with_capacity(lines.len());
    for (i, (_, line)) in lines.iter().enumerate() {
        out.push(match (i >= first && i <= last && !blank(line), left) {
            (false, _) => (*line).to_owned(),
            (true, false) => format!("    {line}"),
            (true, true) => line.chars().skip(shared).collect(),
        });
    }
    let new_start = out[..first].iter().map(|l| l.chars().count() + 1).sum();
    let new_end = new_start
        + out[first..=last]
            .iter()
            .map(|l| l.chars().count() + 1)
            .sum::<usize>()
        - 1;
    (out.join("\n"), new_start, new_end)
}

/// Writes the text to `path` (`~` is the home folder), making its folder.
pub fn save(path: &str, text: &str) -> std::io::Result<PathBuf> {
    let path = path.trim();
    if path.is_empty() {
        return Err(std::io::Error::other("no path: type where to save it"));
    }
    let path = match path.strip_prefix("~/") {
        Some(rest) => crate::store::home()
            .map(|h| h.join(rest))
            .ok_or_else(|| std::io::Error::other("no home folder"))?,
        None => PathBuf::from(path),
    };
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pasted_snippet_loses_its_extra_indent_and_keeps_its_structure() {
        let pasted = "        def f():\n            return 1\n\n        f()";
        let (once, a, b) = shift(pasted, 0, pasted.chars().count(), true);
        assert_eq!(once, "    def f():\n        return 1\n\n    f()");
        assert_eq!((a, b), (0, once.chars().count()), "it stays selected");
        let (twice, ..) = shift(&once, 0, once.chars().count(), true);
        assert_eq!(twice, "def f():\n    return 1\n\nf()");
        let (flat, ..) = shift(&twice, 0, twice.chars().count(), true);
        assert_eq!(flat, twice, "nothing shared left to remove");
    }

    #[test]
    fn tab_moves_only_the_selected_lines_right() {
        let text = "a\nb\nc";
        // b selected, the selection ending at c's start.
        let (moved, a, b) = shift(text, 2, 4, false);
        assert_eq!(moved, "a\n    b\nc");
        assert_eq!((a, b), (2, 7));
        // No selection: the cursor's line.
        assert_eq!(shift(text, 5, 5, false).0, "a\nb\n    c");
    }

    #[test]
    fn save_writes_the_file_and_its_folder() {
        let dir = std::env::temp_dir().join(format!("orbit-editor-{}", std::process::id()));
        let file = dir.join("new/snippet.py");
        let saved = save(&file.to_string_lossy(), "print(1)\n").unwrap();
        assert_eq!(std::fs::read_to_string(saved).unwrap(), "print(1)\n");
        assert!(save("  ", "x").is_err(), "no path, no write");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! Every product decision in docs/decisions.md stays true: each has a
//! guard in docs/decision-guards.txt, and the test here fails when one has
//! none or when a guard no longer holds. Edits (an AI's included) that
//! drop a decided behaviour fail `make check` instead of slipping through.
//!
//! A guard line is `<key> => <guard>; <guard>; …`, the key being the
//! decision's `date · area · ` and the first 60 characters of what it says
//! (`key`). A guard is one of:
//! - `test:<name>`: a test with that name exists (and `make check` runs it);
//! - `code:<path>:<text>`: the file still contains that text, for what is
//!   drawn rather than computed (a label, a hint, a widget's anchor);
//! - `replaced:<why>`: a later decision changed it, named in the reason;
//! - `manual:<why>`: no automatic check fits, and why (said once, kept rare).
//!
//! A profile keeps its own decisions the same way, in
//! `profiles/<name>/decisions.md` and `decision-guards.txt` next to it,
//! checked when that profile is in the build.

#[cfg(test)]
mod tests {
    use std::path::Path;

    const DECISIONS: &str = include_str!("../docs/decisions.md");
    const GUARDS: &str = include_str!("../docs/decision-guards.txt");

    /// A decision line's key: `date · area · ` and the first 60 characters
    /// of the decision, so the guard file reads like the log.
    fn key(line: &str) -> Option<String> {
        let rest = line.strip_prefix("- ")?;
        let mut parts = rest.splitn(4, " · ");
        let (date, _who, area, text) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
        let text: String = text.chars().take(60).collect();
        Some(format!("{date} · {area} · {}", text.trim_end()))
    }

    fn decisions() -> Vec<String> {
        decisions_in(DECISIONS)
    }

    fn decisions_in(log: &str) -> Vec<String> {
        log.lines()
            .filter(|l| l.starts_with("- 20"))
            .filter_map(key)
            .collect()
    }

    /// Every test function's name in src/, as `#[test] fn name(`.
    fn test_names() -> Vec<String> {
        fn walk(dir: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    let lines: Vec<&str> = text.lines().collect();
                    for (i, line) in lines.iter().enumerate() {
                        let line = line.trim_start();
                        let Some(rest) = line.strip_prefix("fn ") else {
                            continue;
                        };
                        let tested = lines[..i]
                            .iter()
                            .rev()
                            .take_while(|l| {
                                let l = l.trim_start();
                                l.starts_with("#[") || l.starts_with("///")
                            })
                            .any(|l| l.trim_start().starts_with("#[test]"));
                        if tested {
                            out.push(rest.chars().take_while(|c| *c != '(').collect());
                        }
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
        out
    }

    #[test]
    fn every_decision_has_a_guard_that_holds() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let tests = test_names();
        let mut problems = problems(root, &tests, DECISIONS, GUARDS);
        for entry in std::fs::read_dir(root.join("profiles"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let dir = entry.path();
            if let Ok(log) = std::fs::read_to_string(dir.join("decisions.md")) {
                let guards =
                    std::fs::read_to_string(dir.join("decision-guards.txt")).unwrap_or_default();
                problems.extend(problems_in(root, &tests, &log, &guards, &dir));
            }
        }
        assert!(
            problems.is_empty(),
            "{} decision guard problems:\n{}",
            problems.len(),
            problems.join("\n")
        );
    }

    fn problems(root: &Path, tests: &[String], log: &str, guards: &str) -> Vec<String> {
        problems_in(root, tests, log, guards, Path::new("docs"))
    }

    /// What does not hold in one decision log and its guard file (`from`
    /// names it in the messages).
    fn problems_in(
        root: &Path,
        tests: &[String],
        log: &str,
        guards: &str,
        from: &Path,
    ) -> Vec<String> {
        let guarded: Vec<(&str, &str)> = guards
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .filter_map(|l| l.split_once(" => "))
            .collect();
        let mut problems = Vec::new();
        let keys = decisions_in(log);
        for decision in &keys {
            let Some((_, guards)) = guarded.iter().find(|(k, _)| k == decision) else {
                problems.push(format!("no guard: {decision}  ({})", from.display()));
                continue;
            };
            for guard in guards.split("; ").map(str::trim).filter(|g| !g.is_empty()) {
                let ok = if let Some(name) = guard.strip_prefix("test:") {
                    tests.iter().any(|t| t == name)
                } else if let Some(rest) = guard.strip_prefix("code:") {
                    rest.split_once(':').is_some_and(|(path, text)| {
                        std::fs::read_to_string(root.join(path))
                            .is_ok_and(|source| source.contains(text))
                    })
                } else if let Some(why) = guard
                    .strip_prefix("replaced:")
                    .or_else(|| guard.strip_prefix("manual:"))
                {
                    why.trim().len() > 10
                } else {
                    false
                };
                if !ok {
                    problems.push(format!("guard does not hold: {guard}  ({decision})"));
                }
            }
        }
        // A guard for a decision that is not in the log is a typo'd key.
        for (k, _) in &guarded {
            if !keys.iter().any(|d| d == k) {
                problems.push(format!("guard for no decision: {k}  ({})", from.display()));
            }
        }
        problems
    }

    #[test]
    fn a_decision_key_is_its_date_area_and_first_words() {
        let line = "- 2026-09-27 #1 · user · PRs · Every action GitHub supports on a PR is available in the app: approve, merge";
        assert_eq!(
            key(line).as_deref(),
            Some(
                "2026-09-27 #1 · PRs · Every action GitHub supports on a PR is available in the app"
            )
        );
        assert!(decisions().len() > 150);
    }
}

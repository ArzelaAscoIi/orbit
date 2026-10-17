//! What an attached terminal's screen says about its session, read the way
//! herdr reads Claude Code: the spinner in the terminal title, the "esc to
//! interrupt" line, the permission prompt, the prompt box. Only the live
//! bottom of the screen counts, never scrollback the user went back to.
//!
//! The hooks (status.rs) stay the word on what a session waits for; the
//! screen is faster (a spinner shows before the hook's line lands) and
//! covers sessions the app did not start, which have no hooks at all.

/// What the screen shows, strongest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    /// A spinner in the title, "esc to interrupt", background agents or MCP
    /// tasks still running.
    Working,
    /// A permission or question prompt waits for the user.
    Blocked,
    /// The prompt box, with nothing running above it.
    Prompt,
}

/// The screen's verdict from the terminal title and its last non-empty
/// lines (bottom first or last, order does not matter). `None`: a plain
/// shell, a viewer (the transcript, a picker) or nothing recognisable.
pub fn classify(title: &str, bottom: &[&str]) -> Option<Seen> {
    // Braille spinner frames up to Claude Code 2.1.227, half circles after.
    if let Some(first) = title.chars().next()
        && matches!(first, '\u{2800}'..='\u{28FF}' | '\u{25D0}'..='\u{25D3}')
        && title.chars().nth(1) == Some(' ')
    {
        return Some(Seen::Working);
    }
    let lines: Vec<String> = bottom.iter().map(|l| l.trim().to_lowercase()).collect();
    let any = |needle: &str| lines.iter().any(|l| l.contains(needle));
    // A viewer over the conversation says nothing about the turn.
    if any("showing detailed transcript") {
        return None;
    }
    if any("esc to interrupt") || any("mcp tasks still running") || any("mcp task still running") {
        return Some(Seen::Working);
    }
    if lines.iter().any(|l| {
        l.find("waiting for ")
            .map(|at| &l[at + "waiting for ".len()..])
            .and_then(|rest| rest.split_once(' '))
            .is_some_and(|(n, rest)| {
                n.parse::<u32>().is_ok_and(|n| n > 0) && rest.starts_with("background agent")
            })
    }) {
        return Some(Seen::Working);
    }
    if any("do you want to proceed?")
        || any("waiting for permission")
        || any("requests your input")
        || (any("esc to cancel") && (any("enter to confirm") || any("enter to select")))
    {
        return Some(Seen::Blocked);
    }
    if lines.iter().any(|l| l.starts_with('❯')) {
        return Some(Seen::Prompt);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_screen_says_working_blocked_or_prompt() {
        assert_eq!(classify("⠋ Thinking", &[]), Some(Seen::Working));
        assert_eq!(classify("◐ fixing tests", &["❯ "]), Some(Seen::Working));
        assert_eq!(classify("✳ Claude Code", &["❯ "]), Some(Seen::Prompt));
        assert_eq!(
            classify("x", &["✻ Cogitating… (12s · esc to interrupt)", "❯ "]),
            Some(Seen::Working)
        );
        assert_eq!(
            classify("x", &["* Waiting for 2 background agents to finish", "❯ "]),
            Some(Seen::Working)
        );
        assert_eq!(
            classify(
                "x",
                &[
                    "Bash command",
                    "Do you want to proceed?",
                    "❯ 1. Yes",
                    "2. No"
                ]
            ),
            Some(Seen::Blocked)
        );
        assert_eq!(
            classify("x", &["Which one?", "Enter to select · Esc to cancel"]),
            Some(Seen::Blocked)
        );
        assert_eq!(
            classify("x", &["Showing detailed transcript", "ctrl+o to toggle"]),
            None
        );
        assert_eq!(classify("", &["$ ls", "Cargo.toml"]), None);
        // The user typing the words into the prompt does not count: the
        // line has to start with the prompt marker to be a prompt, and a
        // blocker needs its own line shape.
        assert_eq!(
            classify("", &["❯ do you want to proceed? asks the dialog"]),
            Some(Seen::Blocked)
        );
    }
}

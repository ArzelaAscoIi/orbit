//! Test plans for tickets in Testing: how to check a merged change works,
//! step by step, in the UI or in a terminal.
//!
//! Claude Sonnet writes one per ticket (`anthropic::test_plan`), and it is
//! posted once as a comment on the ticket. The comment carries
//! `marker(issue id)`, a fixed id per ticket: before posting, the backend
//! reads every comment on the ticket and reuses one that has it, so a
//! plan is never posted twice. The comment is also where the app reads the
//! plan back from (`from_comment`), so what the tab shows and what the
//! ticket says are the same text.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Checked by clicking through the product.
    Ui,
    /// Checked with commands: backend, API, CLI, infrastructure.
    Terminal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub title: String,
    pub how: String,
    pub command: Option<String>,
    pub expect: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestPlan {
    pub summary: String,
    pub kind: Kind,
    /// For a UI plan, the page to open, when the ticket or diff names it.
    pub url: Option<String>,
    pub steps: Vec<Step>,
}

/// The page a UI plan opens: its path on `base` (the test app), whatever
/// host the plan names (older plans may say the production one). No path:
/// the app.
pub fn page(base: &str, url: Option<&str>) -> String {
    let base = base.trim_end_matches('/');
    let url = url.unwrap_or_default().trim();
    let path = match url.split_once("://") {
        Some((_, rest)) => rest.find('/').map_or("", |i| &rest[i..]),
        None => url,
    };
    match path.trim_start_matches('/') {
        "" => base.to_owned(),
        path => format!("{base}/{path}"),
    }
}

/// The id that marks a ticket's test plan comment. One per ticket.
pub fn marker(issue_id: &str) -> String {
    format!("mcr-test-plan:{issue_id}")
}

/// Claude's reply: `SUMMARY`, `KIND` and `STEP` lines, tab-separated.
pub fn from_reply(text: &str) -> Option<TestPlan> {
    let mut plan = TestPlan {
        summary: String::new(),
        kind: Kind::Terminal,
        url: None,
        steps: Vec::new(),
    };
    for line in text.lines() {
        let parts: Vec<&str> = line.split('\t').map(str::trim).collect();
        match parts.as_slice() {
            ["SUMMARY", summary, ..] => plan.summary = (*summary).to_owned(),
            ["URL", url, ..] => {
                plan.url = Some((*url).to_owned()).filter(|u| !u.is_empty() && u != "-");
            }
            ["KIND", kind, ..] => {
                plan.kind = if kind.eq_ignore_ascii_case("ui") {
                    Kind::Ui
                } else {
                    Kind::Terminal
                }
            }
            ["STEP", title, how, command, expect, ..] => plan.steps.push(Step {
                title: (*title).to_owned(),
                how: (*how).to_owned(),
                command: Some(command.trim_matches('`').to_owned())
                    .filter(|c| !c.is_empty() && c != "-"),
                expect: (*expect).to_owned(),
            }),
            _ => {}
        }
    }
    // The prompt asks for one or two; a longer reply is cut, not posted.
    plan.steps.truncate(MAX_STEPS);
    (!plan.steps.is_empty()).then_some(plan)
}

/// A test plan is read in a minute: at most this many steps.
pub const MAX_STEPS: usize = 2;

const HEADING: &str = "## Test plan";

/// The comment posted on the ticket. `from_comment` reads it back.
pub fn to_comment(plan: &TestPlan, issue_id: &str) -> String {
    let where_ = match plan.kind {
        Kind::Ui => "Where: in the UI",
        Kind::Terminal => "Where: in a terminal",
    };
    let mut out = format!("{HEADING}\n\n{}\n\n{where_}\n", plan.summary);
    if let Some(url) = &plan.url {
        out.push_str(&format!("Open: {url}\n"));
    }
    out.push('\n');
    for (i, step) in plan.steps.iter().enumerate() {
        out.push_str(&format!("{}. **{}**\n   {}\n", i + 1, step.title, step.how));
        if let Some(command) = &step.command {
            out.push_str(&format!("   ```sh\n   {command}\n   ```\n"));
        }
        out.push_str(&format!("   Expected: {}\n\n", step.expect));
    }
    out.push_str(&format!(
        "`{}` · written by {} with Claude Sonnet",
        marker(issue_id),
        crate::NAME
    ));
    out
}

/// A plan from its comment; `None` when the comment is not one, or was
/// edited out of shape.
pub fn from_comment(body: &str) -> Option<TestPlan> {
    let rest = body.trim_start().strip_prefix(HEADING)?;
    let mut plan = TestPlan {
        summary: String::new(),
        kind: Kind::Terminal,
        url: None,
        steps: Vec::new(),
    };
    let mut in_code = false;
    for line in rest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            if let Some(step) = plan.steps.last_mut() {
                step.command = Some(trimmed.to_owned());
            }
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with("`mcr-test-plan:") {
            continue;
        }
        if let Some(url) = trimmed.strip_prefix("Open: ") {
            plan.url = Some(url.to_owned());
        } else if let Some(where_) = trimmed.strip_prefix("Where: ") {
            plan.kind = if where_.contains("UI") {
                Kind::Ui
            } else {
                Kind::Terminal
            };
        } else if let Some(title) = step_title(trimmed) {
            plan.steps.push(Step {
                title: title.to_owned(),
                how: String::new(),
                command: None,
                expect: String::new(),
            });
        } else if let Some(step) = plan.steps.last_mut() {
            match trimmed.strip_prefix("Expected: ") {
                Some(expect) => step.expect = expect.to_owned(),
                None if step.how.is_empty() => step.how = trimmed.to_owned(),
                None => step.how = format!("{} {trimmed}", step.how),
            }
        } else if plan.summary.is_empty() {
            plan.summary = trimmed.to_owned();
        }
    }
    (!plan.steps.is_empty()).then_some(plan)
}

/// At most `max` bytes of `text`, cut at a character boundary.
pub fn clip(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// `3. **Title**` → `Title`.
fn step_title(line: &str) -> Option<&str> {
    let (number, rest) = line.split_once(". ")?;
    number.parse::<u32>().ok()?;
    rest.strip_prefix("**")?.strip_suffix("**")
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str = "https://staging.example.com";

    fn plan() -> TestPlan {
        TestPlan {
            summary: "Retries survive a pod restart.".into(),
            kind: Kind::Terminal,
            url: None,
            steps: vec![
                Step {
                    title: "Export a report".into(),
                    how: "Export the report as CSV.".into(),
                    command: Some("curl -X POST https://api/<WORKSPACE>/exports".into()),
                    expect: "A 200 with the CSV.".into(),
                },
                Step {
                    title: "Restart the pod".into(),
                    how: "Delete the pod while it answers.".into(),
                    command: None,
                    expect: "The query is retried, not failed.".into(),
                },
            ],
        }
    }

    #[test]
    fn the_comment_reads_back_as_the_same_plan_and_carries_the_id() {
        let body = to_comment(&plan(), "abc-123");
        assert!(body.contains(&marker("abc-123")));
        assert_eq!(from_comment(&body), Some(plan()));
        // Any other comment is not a plan.
        assert_eq!(from_comment("Looks good, merging."), None);
    }

    #[test]
    fn a_ui_plan_keeps_its_page() {
        let mut ui_plan = plan();
        ui_plan.kind = Kind::Ui;
        ui_plan.url = Some("https://app.example.com/reports".into());
        assert_eq!(from_comment(&to_comment(&ui_plan, "x")), Some(ui_plan));
        let reply = "SUMMARY\ts\nKIND\tui\nURL\thttps://a.b/c\nSTEP\tt\th\t-\te";
        assert_eq!(
            from_reply(reply).unwrap().url.as_deref(),
            Some("https://a.b/c")
        );
    }

    #[test]
    fn ui_plans_open_on_the_dev_app() {
        assert_eq!(page(APP, None), APP);
        assert_eq!(page(APP, Some("/reports")), format!("{APP}/reports"));
        assert_eq!(
            page(APP, Some("reports?tab=runs")),
            format!("{APP}/reports?tab=runs")
        );
        // A full URL keeps its path and moves to dev.
        assert_eq!(
            page(APP, Some("https://app.example.com/workspaces/x/reports")),
            format!("{APP}/workspaces/x/reports")
        );
        assert_eq!(page(APP, Some("https://app.example.com")), APP);
        // Another test app from settings.json, with or without a slash.
        assert_eq!(
            page("http://localhost:3000/", Some("/x")),
            "http://localhost:3000/x"
        );
    }

    #[test]
    fn claude_reply_becomes_a_plan() {
        let reply = "Sure.\nSUMMARY\tRetries survive a pod restart.\nKIND\tterminal\n\
            STEP\tExport a report\tExport the report as CSV.\t`curl -X POST https://api/<WORKSPACE>/exports`\tA 200 with the CSV.\n\
            STEP\tRestart the pod\tDelete the pod while it answers.\t-\tThe query is retried, not failed.\n\
            STEP\tbroken";
        assert_eq!(from_reply(reply), Some(plan()));
        assert_eq!(from_reply("no plan here"), None);
        // A reply longer than the prompt allows is cut to MAX_STEPS.
        let long = format!("{reply}\nSTEP\tThird\tDo more.\t-\tMore.");
        assert_eq!(from_reply(&long).unwrap().steps.len(), MAX_STEPS);
    }
}

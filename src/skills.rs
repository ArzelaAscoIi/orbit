//! The app's own skills, compiled in from `skills/`: every core capability,
//! with nothing company-specific. A profile fine-tunes one with
//! `<profile>/skills/<folder>/EXTRA.md`, appended to it (`with_extra`), and
//! adds skills of its own as `<profile>/skills/<name>/SKILL.md`. Skills live
//! in the repo and the profile only; nothing is synced with Linear.

/// How a session inside the app reports to it (`orbit --skill` prints it).
pub const ORBIT: &str = include_str!("../skills/orbit/SKILL.md");

pub const GENERIC: &[(&str, &str)] = &[
    ("orbit", ORBIT),
    ("triage", include_str!("../skills/triage/SKILL.md")),
    ("plan", include_str!("../skills/plan/SKILL.md")),
    ("build", include_str!("../skills/build/SKILL.md")),
    (
        "fix-pr-comments",
        include_str!("../skills/fix-pr-comments/SKILL.md"),
    ),
    ("spot", include_str!("../skills/spot/SKILL.md")),
    ("goals", include_str!("../skills/goals/SKILL.md")),
    ("find-work", include_str!("../skills/find-work/SKILL.md")),
    ("ship", include_str!("../skills/ship/SKILL.md")),
    ("fix-deploy", include_str!("../skills/fix-deploy/SKILL.md")),
    (
        "orchestrate",
        include_str!("../skills/orchestrate/SKILL.md"),
    ),
    ("profile", include_str!("../skills/profile/SKILL.md")),
    ("operate", include_str!("../skills/operate/SKILL.md")),
    ("mission", include_str!("../skills/mission/SKILL.md")),
    ("land", include_str!("../skills/land/SKILL.md")),
    (
        "orbit-improve",
        include_str!("../skills/orbit-improve/SKILL.md"),
    ),
];

/// A skill with the company's fine-tuning after it, under a heading that
/// says where it came from.
pub fn with_extra(body: &str, company: &str, extra: &str) -> String {
    let company = if company.trim().is_empty() {
        "Company"
    } else {
        company.trim()
    };
    format!(
        "{}\n\n## {company} specifics (from the profile)\n\n{}\n",
        body.trim_end(),
        extra.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_extra_comes_after_the_skill_under_its_own_heading() {
        let body = with_extra(
            "---\nname: plan\n---\n\n# Plan\n\n",
            "Acme",
            "\n### Repos\nweb\n",
        );
        assert_eq!(
            body,
            "---\nname: plan\n---\n\n# Plan\n\n## Acme specifics (from the profile)\n\n### Repos\nweb\n"
        );
        assert!(with_extra("# Plan", "", "x").contains("## Company specifics"));
    }

    #[test]
    fn every_shipped_skill_has_frontmatter_naming_its_folder() {
        for (folder, body) in GENERIC {
            assert!(
                body.starts_with(&format!("---\nname: {folder}\n")),
                "{folder}"
            );
        }
    }

    /// The bot's poller and this app's `story.rs` match these strings by
    /// prefix, so each skill must keep emitting them byte for byte.
    #[test]
    fn every_skill_keeps_the_markers_the_poller_parses() {
        let markers: &[(&str, &[&str])] = &[
            (
                "triage",
                &[
                    "🤖 Possible duplicate — I found a similar issue",
                    "🤖 I need a bit more info before I can route this",
                    "🤖 Thanks for the update!",
                    "🤖 Routed to **<SQUAD>**",
                    "🤖 Dispatched Build",
                    "*Consolidated by the bot…*",
                ],
            ),
            (
                "plan",
                &[
                    "🤖 Here's my implementation plan",
                    ">>> My analysis",
                    "**Bug** · repos: `",
                    "### Root Cause",
                    "### Bottleneck",
                    "### Entry Points",
                    "### How It Should Work (End-to-End)",
                    "### Existing Pattern to Follow",
                    "### Approach",
                    "### Implementation Plan",
                    "**Phase N — `repo`**",
                    "### Open Questions",
                    "### Performance Impact",
                    "### Acceptance Criteria",
                    "### What changed",
                    "🤖 Routed to **Blocked** — ",
                    "🤖 Assigned to ",
                ],
            ),
            (
                "build",
                &[
                    "🤖 Here's my implementation plan",
                    "**Phase N — \\`repo\\`**",
                    "🤖 Dispatched Build — building locally in Claude Code…",
                    "🤖 PR opened → <url> (<owner>/<repo>) — ",
                    "🤖 PR updated → <url> (<owner>/<repo>) — ",
                    "🤖 Build summary — ",
                ],
            ),
        ];
        for (folder, wanted) in markers {
            let (_, body) = GENERIC.iter().find(|(f, _)| f == folder).unwrap();
            for marker in *wanted {
                assert!(body.contains(marker), "{folder} lost {marker:?}");
            }
        }
    }
}

//! New ticket: type what is wrong or wanted, paste links, and get a
//! triaged ticket. Haiku reads the note (`intake_prompt`): bug or feature, title,
//! squad, area, priority, and whether it just makes sense. If it does, the
//! ticket is created right away; if not, it asks up to three short
//! questions first (how to reproduce, an idea how to fix it, which other
//! customers would benefit). Then Haiku writes the description in the
//! triage template (`DRAFT`, same sections as the triage skill's template), and
//! the ticket is created in the squad's Backlog, triaged, with priority,
//! labels and the routing comment.

/// What Haiku read from the note.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Intake {
    /// bug, feature, ux, question or chore.
    pub kind: String,
    pub title: String,
    pub team: String,
    pub area: String,
    /// P0 to P4, as in the triage skill.
    pub priority: String,
    /// It just makes sense: create it without asking.
    pub obvious: bool,
    pub questions: Vec<String>,
    /// The fields Laya decided instead of Haiku (laya.rs), and why Laya
    /// could not be asked, when it could not.
    pub by_laya: Vec<String>,
    pub laya_error: Option<String>,
}

/// The intake prompt, around `teams`: which team owns what, from the
/// profile (`intake.routing`) or the watched teams' own names.
pub fn intake_prompt(teams: &str) -> String {
    let product = match crate::profile::get().company.product.trim() {
        "" => "the product".to_owned(),
        product => product.to_owned(),
    };
    format!(
        "You turn a quick note about a problem or a wish for {product} into a triaged \
Linear ticket. The note may be rough and hold links. Reply in exactly this form, nothing else:\n\
KIND<TAB>bug, feature, ux, question or chore\n\
TITLE<TAB>ticket title, at most 10 words, plain, no prefix\n\
TEAM<TAB>one team key from the list below\n\
AREA<TAB>product area, 1 to 3 words\n\
PRIORITY<TAB>P0, P1, P2, P3 or P4\n\
OBVIOUS<TAB>yes or no\n\
ASK<TAB>one question (0 to 3 ASK lines)\n\
KIND: bug needs two signals of a defect (broken, error, crash, fails, 500, a repro, expected \
vs actual, a regression); a UX opinion is ux; a wish is feature. \
TEAM by area: {teams} \
PRIORITY: P0 many customers blocked or an outage, P1 one blocked or several intermittent, \
P2 one degraded, P3 a non-blocking bug, P4 cosmetic; a feature is P3 unless the note says \
otherwise. \
OBVIOUS is yes when it just makes sense: concrete, bounded, clear who is hit and what right \
looks like, no product, pricing or security decision in it. Then write no ASK lines. \
Otherwise ask only what the note and its links do not already answer, one short question per \
line, most useful first: for a bug, how to reproduce it or what was expected if that is \
missing, and whether they already have an idea how to fix it; for a feature that looks niche \
or specific to one customer, which other customers would benefit and how; for anything, who \
is affected if that is unclear."
    )
}

/// Which team owns what, for `intake_prompt`: the profile's routing, else
/// one line per watched team `(key, name)`; the default team last.
pub fn teams_text(watched: &[(String, String)]) -> String {
    teams_text_in(&crate::profile::get(), watched)
}

fn teams_text_in(profile: &crate::profile::Profile, watched: &[(String, String)]) -> String {
    let routing = match profile.intake.routing.trim() {
        "" => watched
            .iter()
            .map(|(key, name)| format!("{key} is {name}"))
            .collect::<Vec<_>>()
            .join("; "),
        routing => routing.to_owned(),
    };
    let default = match profile.intake.default_team.trim() {
        "" => watched
            .first()
            .map(|(key, _)| key.clone())
            .unwrap_or_default(),
        team => team.to_owned(),
    };
    if default.is_empty() {
        format!("{routing}.")
    } else {
        format!("{routing}. When unclear: {default}.")
    }
}

pub const DRAFT: &str = "Write the description of a Linear ticket from a note, its answers \
and its classification. Use these markdown sections, and leave out any that would be empty: \
### Problem, ### Customer / Business Impact, ### Expected Behaviour, ### How to Reproduce, \
### Acceptance Criteria (user-observable outcomes only), ### Additional Context (every link \
from the note, verbatim). A fix idea from the reporter goes verbatim into \
<details><summary>Possible solution from the reporter</summary>...</details>. Lean: the \
symptom, who is hit, the right outcome, minimal steps. Mark anything inferred in italics and \
invent nothing: no customers, numbers or errors the note does not give. Reply with the \
description only.";

/// An open ticket that looks like the same report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Duplicate {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub team: String,
    pub state: String,
    pub url: String,
    /// What the note and the ticket share: the error, component, or page.
    pub evidence: String,
}

pub const DEDUPE: &str = "You check whether a new report is already filed. You get the \
note and open tickets found by searching for it. A ticket is a duplicate only when it shares \
real evidence with the note: the same error text, the same component or page, the same \
symptom. A similar title alone is not enough. Reply with one line per duplicate, at most \
three, best first, and nothing else:\n\
DUP<TAB>ticket id<TAB>what they share, at most 14 words\n\
No duplicates: reply NONE.";

/// The note and the search results, for the duplicate check.
pub fn dedupe_input(note: &str, candidates: &[Duplicate]) -> String {
    let mut out = format!("Note:\n{note}\n\nOpen tickets:\n");
    for c in candidates {
        out.push_str(&format!(
            "- {} | {} | {} | {}\n",
            c.identifier, c.team, c.state, c.title
        ));
        if !c.evidence.is_empty() {
            out.push_str(&format!("  {}\n", crate::testplan::clip(&c.evidence, 500)));
        }
    }
    out
}

/// The candidates Haiku kept, with what they share; only ids it was shown.
pub fn from_dedupe_reply(text: &str, candidates: &[Duplicate]) -> Vec<Duplicate> {
    let mut kept = Vec::new();
    for line in text.lines() {
        let cells = crate::anthropic::cells(line);
        let parts: Vec<&str> = cells.iter().map(String::as_str).collect();
        if let ["DUP", id, shared, ..] = parts.as_slice()
            && let Some(c) = candidates.iter().find(|c| c.identifier == *id)
            && !kept
                .iter()
                .any(|k: &Duplicate| k.identifier == c.identifier)
        {
            kept.push(Duplicate {
                evidence: (*shared).to_owned(),
                ..c.clone()
            });
        }
    }
    kept.truncate(3);
    kept
}

/// The comment that adds the note to an existing ticket, the way triage
/// folds a duplicate in: who reported it again, and what they said. The
/// user's text is defused so it can never read as a bot command.
pub fn report_comment(reporter: &str, note: &str, answers: &[(String, String)]) -> String {
    let mut out = format!(
        "🤖 Reported again by {reporter}:\n\n> {}",
        note.trim().replace('\n', "\n> ")
    );
    for (question, answer) in answers {
        if !answer.trim().is_empty() {
            out.push_str(&format!("\n\n**{question}** {}", answer.trim()));
        }
    }
    defuse(&out)
}

/// `@Robo Build!` in pasted text becomes `(at)Robo Build!`: the bot's
/// poller runs any `@<anyone> <Command>!` in a comment from the user. Only
/// that shape is touched (an `@`, a name, space, a word, `!`); emails and
/// plain mentions stay as written.
pub fn defuse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, c) in text.char_indices() {
        if c == '@' && looks_like_command(&text[i + 1..]) {
            out.push_str("(at)");
        } else {
            out.push(c);
        }
    }
    out
}

/// After an `@`: a name, whitespace, then a word right before `!`.
fn looks_like_command(rest: &str) -> bool {
    let name = rest.find(char::is_whitespace).unwrap_or(rest.len());
    if name == 0 {
        return false;
    }
    let after = rest[name..].trim_start();
    let word = after
        .find(|c: char| !c.is_alphabetic())
        .unwrap_or(after.len());
    word > 0 && after[word..].starts_with('!')
}

/// Haiku's intake reply; `None` when it has no title to create from.
pub fn from_reply(text: &str) -> Option<Intake> {
    let mut intake = Intake::default();
    for line in text.lines() {
        let cells = crate::anthropic::cells(line);
        let parts: Vec<&str> = cells.iter().map(String::as_str).collect();
        let value = || parts.get(1).map(|v| (*v).to_owned()).unwrap_or_default();
        match parts.first().copied() {
            Some("KIND") => intake.kind = value().to_lowercase(),
            Some("TITLE") => intake.title = value(),
            Some("TEAM") => intake.team = value().to_uppercase(),
            Some("AREA") => intake.area = value(),
            Some("PRIORITY") => intake.priority = value().to_uppercase(),
            Some("OBVIOUS") => intake.obvious = value().eq_ignore_ascii_case("yes"),
            Some("ASK") if !value().is_empty() => intake.questions.push(value()),
            _ => {}
        }
    }
    intake.questions.truncate(3);
    if intake.obvious {
        intake.questions.clear();
    }
    (!intake.title.is_empty()).then_some(intake)
}

/// The note and its answers, for the draft.
pub fn draft_input(note: &str, intake: &Intake, answers: &[(String, String)]) -> String {
    let mut out = format!(
        "Kind: {}\nArea: {}\nPriority: {}\n\nNote:\n{note}\n",
        intake.kind, intake.area, intake.priority
    );
    for (question, answer) in answers {
        if !answer.trim().is_empty() {
            out.push_str(&format!("\nQ: {question}\nA: {}\n", answer.trim()));
        }
    }
    out
}

/// Linear's priority for a triage level: P0 urgent to P4 low.
pub fn linear_priority(level: &str) -> u8 {
    match level {
        "P0" => 1,
        "P1" => 2,
        "P2" => 3,
        _ => 4,
    }
}

/// The routing comment the triage skill leaves, so the ticket reads as
/// triaged. Never a bot command.
pub fn routing(intake: &Intake, identifier: &str) -> String {
    let profile = crate::profile::get();
    let tail = if intake.kind == "bug" {
        format!(
            "Bug filed: focus is on reproduction and root cause. Plan it with /{} {identifier}.",
            profile.skill("plan")
        )
    } else {
        "Next: an implementation plan.".to_owned()
    };
    let urgent = profile.intake.urgent_note.trim();
    let fire = if matches!(intake.priority.as_str(), "P0" | "P1") && !urgent.is_empty() {
        format!(" ⚠️ {urgent}")
    } else {
        String::new()
    };
    format!(
        "🤖 Routed to **{}** · **{}** · **{}**. {tail}{fire}",
        intake.team, intake.area, intake.priority
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intake_reads_a_reply_that_bends_the_form() {
        let tabs = "KIND\tbug\nTITLE\tReport export disconnects\nTEAM\tDEV\nPRIORITY\tP2\nOBVIOUS\tno\nASK\tWhich report?";
        let colons = "Here is the ticket:\n**KIND**: bug\n- TITLE: Report export disconnects\nTEAM DEV\nPRIORITY<TAB>P2\nOBVIOUS: no\nASK: Which report?";
        for reply in [tabs, colons] {
            let intake = from_reply(reply).expect(reply);
            assert_eq!(intake.kind, "bug");
            assert_eq!(intake.title, "Report export disconnects");
            assert_eq!(intake.team, "DEV");
            assert_eq!(intake.priority, "P2");
            assert_eq!(intake.questions, ["Which report?"]);
        }
    }

    #[test]
    fn an_obvious_note_asks_nothing_and_a_niche_one_asks() {
        let obvious = "KIND\tbug\nTITLE\tLogin button does nothing on Safari\nTEAM\tweb\n\
            AREA\tLogin\nPRIORITY\tp2\nOBVIOUS\tyes\nASK\tshould not be kept";
        let intake = from_reply(obvious).unwrap();
        assert_eq!(
            (intake.team.as_str(), intake.priority.as_str()),
            ("WEB", "P2")
        );
        assert!(intake.obvious && intake.questions.is_empty());

        let niche = "KIND\tfeature\nTITLE\tExport runs as Parquet\nTEAM\tECO\nAREA\tJobs\n\
            PRIORITY\tP3\nOBVIOUS\tno\nASK\tWhich other customers would benefit, and how?\n\
            ASK\tDo you have an idea how to build it?\nASK\tthree\nASK\tfour";
        let intake = from_reply(niche).unwrap();
        assert_eq!(intake.questions.len(), 3);
        assert_eq!(from_reply("nothing useful"), None);
    }

    #[test]
    fn duplicates_need_shared_evidence_and_a_report_is_never_a_command() {
        let found = |id: &str| Duplicate {
            identifier: id.into(),
            title: format!("title of {id}"),
            ..Duplicate::default()
        };
        let candidates = vec![found("WEB-861"), found("OPS-650")];
        let reply = "DUP\tWEB-861\tSame reload loop when switching to the report editor\n\
            DUP\tWEB-9999\tnot a candidate\nDUP\tWEB-861\ttwice";
        let kept = from_dedupe_reply(reply, &candidates);
        assert_eq!(kept.len(), 1);
        assert_eq!(
            kept[0].evidence,
            "Same reload loop when switching to the report editor"
        );
        assert!(from_dedupe_reply("NONE", &candidates).is_empty());

        let comment = report_comment("Sam", "It reloads.\nAlso @Robo Build! please", &[]);
        assert!(comment.starts_with("🤖 Reported again by Sam:"));
        assert!(comment.contains("(at)Robo Build!"));
        // Only the command shape is defused; emails and mentions stay.
        assert_eq!(
            defuse("mail jo.doe@acme.example, cc @lee"),
            "mail jo.doe@acme.example, cc @lee"
        );
        assert_eq!(
            defuse("@bot  iterate! the fix"),
            "(at)bot  iterate! the fix"
        );
        assert_eq!(defuse("@robo plan!"), "(at)robo plan!");
    }

    #[test]
    fn routing_reads_like_triage_and_is_never_a_command() {
        let intake = Intake {
            kind: "bug".into(),
            team: "ABC".into(),
            area: "Indexes".into(),
            priority: "P1".into(),
            ..Intake::default()
        };
        let comment = routing(&intake, "ABC-9");
        assert!(comment.starts_with("🤖 Routed to **ABC** · **Indexes** · **P1**."));
        // No urgent note without a profile that names one.
        assert!(!comment.contains("⚠️"));
        assert!(comment.contains("Plan it with /plan ABC-9."));
        assert!(!comment.contains('@'));
        assert_eq!(linear_priority("P0"), 1);
        assert_eq!(linear_priority("P4"), 4);
    }

    #[test]
    fn a_profile_routes_the_intake() {
        let teams = teams_text_in(
            &crate::profile::acme(),
            &[("ENG".into(), "Engineering".into())],
        );
        // The profile's table word for word, not the watched teams.
        assert!(teams.starts_with("WEB is the web app, dashboards, login; API is"));
        assert!(!teams.contains("ENG is Engineering"));
        assert!(teams.ends_with("When unclear: WEB."));
    }

    #[test]
    fn the_intake_lists_the_watched_teams_without_a_profile() {
        let teams = teams_text(&[
            ("OPS".into(), "Operations".into()),
            ("WEB".into(), "Web app".into()),
        ]);
        assert_eq!(
            teams,
            "OPS is Operations; WEB is Web app. When unclear: OPS."
        );
        let prompt = intake_prompt(&teams);
        assert!(prompt.contains("a wish for the product into"));
        assert!(prompt.contains("TEAM by area: OPS is Operations"));
    }
}

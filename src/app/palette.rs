//! The command palette (Cmd+K): one search over tickets, sessions, stories,
//! customers and every quick action, plus Linear's own search for tickets
//! that are not on the board. Enter runs the top result. The views are the
//! bottom bar's, not the palette's; tickets show once something is typed.
//!
//! `items` builds the list from the app as it is; `rank` orders it for a
//! query. Both are plain functions, so the list is always current and the
//! ranking is tested without a window.

use super::{Action, App, Tab, View};
use crate::story::{self, Lane, Stage};

/// One line in the palette.
#[derive(Clone)]
pub struct Item {
    pub section: &'static str,
    pub title: String,
    /// Second line, dim: stage, status, where it goes.
    pub detail: String,
    /// The shortcut that does the same, shown on the right.
    pub keys: &'static str,
    /// Extra words it answers to, beyond the title and detail.
    pub also: &'static str,
    /// Hidden text it answers to, lowercased: what a session is about.
    pub text: String,
    pub actions: Vec<Action>,
}

fn item(section: &'static str, title: &str, keys: &'static str, actions: Vec<Action>) -> Item {
    Item {
        section,
        title: title.to_owned(),
        detail: String::new(),
        keys,
        also: "",
        text: String::new(),
        actions,
    }
}

/// Everything the palette can do right now, in the order it shows with an
/// empty query: the selected ticket's actions, quick actions, then what the
/// board holds.
pub fn items(app: &App) -> Vec<Item> {
    // Scoped to a repo (Tab): only the user's open PRs there.
    if let Some(repo) = app.palette.as_ref().and_then(|p| p.repo.as_deref()) {
        return app
            .my_prs
            .iter()
            .filter(|pr| pr.repo == repo)
            .map(my_pr_item)
            .collect();
    }
    let mut out = Vec::new();

    // The selected ticket first: what you most likely want to do next.
    if let Some(card) = app.selected.as_deref().and_then(|id| app.card(id)) {
        let id = card.issue.id.clone();
        let name = &card.issue.identifier;
        if !card.pr_only() {
            out.push(item(
                "this ticket",
                &format!("Open {name} in Linear"),
                "",
                vec![Action::Open(card.issue.url.clone())],
            ));
        }
        if let Some(pr) = card.pr(app.diff_pr.as_deref()) {
            out.push(item(
                "this ticket",
                &format!("Open the PR {}#{}", pr.repo, pr.number),
                "o",
                vec![Action::Open(pr.url.clone())],
            ));
            out.push(item(
                "this ticket",
                &format!("Review the diff of {name}"),
                "",
                vec![Action::Tab(Tab::Diff)],
            ));
        }
        if card.stage == Stage::Testing {
            out.push(item(
                "this ticket",
                &format!("Test {name}"),
                "",
                vec![Action::Tab(Tab::Testing)],
            ));
        }
        // herdr users keep their sessions in herdr: start it there.
        if app.herdr_bin {
            out.push(Item {
                detail: "A herdr workspace of its own, Claude with the ticket's brief".into(),
                also: "herdr claude session terminal start",
                ..item(
                    "this ticket",
                    &format!("Start {name} in herdr"),
                    "",
                    vec![Action::HerdrStart(id.clone())],
                )
            });
        }
        let agent = app.claude.agent_of(&id);
        let started = app.claude.saved.contains_key(&id);
        let verb = if started { "Resume" } else { "Work with" };
        out.push(Item {
            also: "claude codex session",
            ..item(
                "this ticket",
                &format!("{verb} {} on {name}", agent.short()),
                "",
                vec![Action::Claude(id.clone())],
            )
        });
        // Another agent, this once, while nothing has started yet.
        if !started {
            for other in crate::claude::Agent::offered()
                .into_iter()
                .filter(|a| *a != agent)
            {
                out.push(Item {
                    also: "claude codex pi session",
                    ..item(
                        "this ticket",
                        &format!("Work with {} on {name}", other.short()),
                        "",
                        vec![Action::WithAgent(
                            other,
                            Box::new(Action::Claude(id.clone())),
                        )],
                    )
                });
            }
        }
        // Its Linear state, one line per state it could move to; once a PR
        // merged, never back to before where the ticket stands.
        let merged = card.prs.iter().any(|pr| pr.merged);
        for state in app.states.get(&card.issue.team.id).into_iter().flatten() {
            if state.id != card.issue.state.id && !(merged && behind(state, &card.issue.state)) {
                out.push(Item {
                    also: "state status move",
                    ..item(
                        "this ticket",
                        &format!("Move {name} to {}", state.name),
                        "",
                        vec![Action::SetState {
                            issue_id: id.clone(),
                            state_id: state.id.clone(),
                        }],
                    )
                });
            }
        }
    }

    // Only where the profile names the bot's poller workflow.
    if !crate::profile::get().bot.poller_workflow.is_empty() {
        out.push(Item {
            also: "bot poller wake dispatch kick work",
            ..item(
                "actions",
                &format!(
                    "{} break is over, WORK!",
                    crate::profile::get().bot_name("Bot")
                ),
                "",
                vec![Action::WakeBot],
            )
        });
    }
    out.extend([
        Item {
            also: "create file report bug issue feature",
            ..item("actions", "New ticket", "⌘T", vec![Action::NewTicket])
        },
        Item {
            also: "operator agent automate cli orbit ctl",
            ..item("actions", "Run the operator", "", vec![Action::Operate])
        },
        Item {
            also: "orchestrate autopilot board",
            ..item("actions", "Run the board", "", vec![Action::Orchestrate])
        },
        Item {
            also: "intake groom",
            ..item(
                "actions",
                "Triage with Claude",
                "",
                vec![Action::Show(View::Intake), Action::Triage],
            )
        },
        Item {
            also: "claude free scratch",
            ..item(
                "actions",
                "New session without a ticket",
                "",
                vec![Action::PaletteQuery("New session: ".into())],
            )
        },
        Item {
            also: "pane split vertical",
            ..item(
                "panes",
                "Split right",
                "⌘D",
                vec![Action::Split { across: true }],
            )
        },
        Item {
            also: "pane split horizontal",
            ..item(
                "panes",
                "Split down",
                "⌘⇧D",
                vec![Action::Split { across: false }],
            )
        },
        Item {
            also: "window empty",
            ..item("panes", "New tab", "⌘N", vec![Action::NewTab])
        },
        // Typed text names it (`Rename this tab: …`); this brings the
        // automatic name back.
        Item {
            also: "rename window title name tab automatic",
            ..item(
                "panes",
                "Rename tab back to its automatic name",
                "",
                vec![Action::RenameWindow(app.window, String::new())],
            )
        },
        Item {
            also: "terminal shell console tab",
            ..item("panes", "New shell window", "", vec![Action::ShellWindow])
        },
        Item {
            also: "text editor notepad snippet paste python file",
            ..item("panes", "New editor", "", vec![Action::EditorWindow])
        },
        Item {
            also: "web browser page url test ui localhost website",
            ..item("panes", "New browser", "", vec![Action::BrowserWindow])
        },
        Item {
            also: "terminal shell console pane split",
            ..item(
                "panes",
                "Shell beside this pane",
                "",
                vec![Action::NewShell],
            )
        },
        Item {
            also: "pane",
            ..item("panes", "Close pane", "⌘W", vec![Action::ClosePane(None)])
        },
        Item {
            also: "pane maximize focus",
            ..item("panes", "Zoom pane", "⌘⇧↩", vec![Action::ZoomPane])
        },
        Item {
            also: "fullscreen focus maximize session alone",
            ..item("panes", "Full screen", "⌘↩", vec![Action::FullScreen])
        },
        item("panes", "Next pane", "⌘]", vec![Action::CyclePane(1)]),
        Item {
            also: "undo restore closed history revert",
            ..item(
                "panes",
                "Reopen closed window",
                "⌘Z",
                vec![Action::ReopenWindow],
            )
        },
        Item {
            also: "preferences about json keys connections",
            ..item(
                "actions",
                "Settings",
                "⌘,",
                vec![Action::Show(View::Settings)],
            )
        },
        Item {
            also: "linear watch board",
            ..item(
                "actions",
                &format!("Teams: {}", app.teams.join(", ")),
                "",
                vec![Action::PickTeams],
            )
        },
        Item {
            also: "version update changelog",
            ..item(
                "actions",
                &format!("Releases (this is {})", crate::version()),
                "",
                vec![Action::Open(crate::RELEASES.into())],
            )
        },
        Item {
            also: "version upgrade install new release",
            ..item(
                "actions",
                &match &app.update {
                    Some(tag) => format!("Update Orbit to {tag}"),
                    None => format!("Update Orbit (this is {}, the newest)", crate::version()),
                },
                "",
                vec![Action::Update],
            )
        },
        if app.no_linear() {
            Item {
                also: "linear sign in log in tickets",
                ..item("actions", "Connect Linear", "", vec![Action::ConnectLinear])
            }
        } else {
            Item {
                also: "log out forget key",
                ..item("actions", "Sign out", "", vec![Action::SignOut])
            }
        },
        Item {
            also: "reload sync",
            ..item("actions", "Refresh the board", "", vec![Action::Refresh])
        },
        Item {
            also: "feedback improve bug idea ux pr fix the app",
            ..item("actions", "Improve Orbit", "⌘⇧F", vec![Action::Improve])
        },
        Item {
            also: "instance copy second another",
            ..item("actions", "New window", "", vec![Action::NewWindow])
        },
        Item {
            also: "ktlo alerts security ship merge approve route",
            ..item(
                "actions",
                &format!("Clear {}", crate::profile::get().ktlo.label),
                "",
                vec![Action::ClearKtlo],
            )
        },
        Item {
            also: "review approve merge ship bot prs",
            ..item(
                "actions",
                "Ship the bot's PRs",
                "",
                vec![Action::ShipReviews],
            )
        },
        Item {
            also: "backlog goals priorities pull next",
            ..item(
                "actions",
                "Find work in the backlog",
                "",
                vec![Action::FindWork],
            )
        },
        Item {
            also: "inbox notifications handle all triage reply review mentions",
            ..item(
                "actions",
                "Handle all my notifications",
                "",
                vec![Action::HandleInbox],
            )
        },
        Item {
            also: "help tour onboarding",
            ..item(
                "actions",
                "Start the tutorial",
                "",
                vec![Action::Tutorial(Some(0))],
            )
        },
        Item {
            also: "theme colour dark light",
            ..item(
                "theme",
                "Next theme",
                "⌘⇧T",
                vec![Action::SetTheme(app.theme.next())],
            )
        },
    ]);
    for choice in crate::theme::Choice::ALL {
        out.push(Item {
            also: "theme colour",
            ..item(
                "theme",
                &format!("Theme: {}", choice.name()),
                "",
                vec![Action::SetTheme(choice)],
            )
        });
    }

    // Tickets: the ones that need you first, as the board orders them.
    let mut cards: Vec<&story::Card> = app
        .cards
        .iter()
        .filter(|c| c.stage != Stage::Done)
        .collect();
    story::sort(&mut cards);
    let ktlo = format!(" · {}", crate::profile::get().ktlo.label);
    for card in cards {
        let lane = if card.lane == Lane::Product {
            ""
        } else {
            &ktlo
        };
        let ask = card.ask.map_or(String::new(), |a| {
            format!(" · {}", a.label().to_lowercase())
        });
        out.push(Item {
            section: "tickets",
            title: format!("{}  {}", card.issue.identifier, app.short_title(card)),
            detail: format!(
                "{}{ask}{lane} · {}",
                card.stage.label().to_lowercase(),
                card.story
            ),
            keys: "",
            also: "",
            // Its old identifiers: a ticket moved to another team still
            // answers to the id the user remembers.
            text: card.issue.previous_identifiers.join(" ").to_lowercase(),
            actions: vec![Action::Focus(card.issue.identifier.clone())],
        });
    }

    // Sessions: every one the app started, and the running ones it did not;
    // what waits on the user first (needs approval, your turn), then the
    // ones at work, then the idle and stopped.
    let mut sessions = Vec::new();
    for (key, saved) in &app.claude.saved {
        let status = app.claude.live_of(key).map_or("stopped", |l| l.label());
        let about = app.claude.about.get(&saved.session_id);
        let name = app.session_name(key, saved);
        sessions.push(Item {
            section: "sessions",
            title: name.clone(),
            // Claude's title only when it says what the name does not.
            detail: match about
                .and_then(|a| a.title.as_deref())
                .filter(|t| !name.contains(t) && !t.contains(saved.identifier.as_str()))
            {
                Some(title) => format!("{} · {status} · {title}", saved.agent.short()),
                None => format!("{} · {status}", saved.agent.short()),
            },
            keys: "",
            also: "claude codex session",
            text: about.map_or(String::new(), |a| a.text.clone()),
            actions: vec![Action::OpenSession(key.clone())],
        });
    }
    for row in app.claude.rows.values() {
        if row.running() && app.claude.key_of(&row.session_id).is_none() {
            let about = app.claude.about.get(&row.session_id);
            sessions.push(Item {
                section: "sessions",
                title: row
                    .name
                    .clone()
                    .or_else(|| about.and_then(|a| a.title.clone()))
                    .unwrap_or_else(|| row.folder().to_owned()),
                detail: row.label().to_owned(),
                keys: "",
                also: "claude session terminal",
                text: about.map_or(String::new(), |a| a.text.clone()),
                actions: vec![Action::OpenSession(format!("sid:{}", row.session_id))],
            });
        }
    }
    sessions.sort_by_key(|item| state_rank(&item.detail));
    out.extend(sessions);

    // herdr: its agents (a pick brings one forward in herdr), and using the
    // user's herdr setup.
    for agent in &app.herdr_agents {
        out.push(Item {
            detail: format!("{} · {} · {}", agent.kind, agent.workspace, agent.label()),
            also: "herdr agent session",
            ..item(
                "herdr",
                &format!("{} in herdr", agent.target()),
                "",
                vec![Action::HerdrFocus(agent.target().to_owned())],
            )
        });
    }
    if app.herdr_config.is_some() {
        out.push(Item {
            detail: "Theme, notifications, sounds, sidebar and prefix keys from your herdr config"
                .into(),
            also: "herdr config keys theme import",
            ..item(
                "herdr",
                "Use my herdr config",
                "",
                vec![Action::UseHerdrConfig],
            )
        });
    }
    if app.herdr_bin {
        out.push(Item {
            detail: "What needs you in a herdr popup; Linear and GitHub links open in Orbit".into(),
            also: "herdr plugin install link",
            ..item(
                "herdr",
                "Add Orbit's plugin to herdr",
                "",
                vec![Action::InstallHerdrPlugin],
            )
        });
    }
    let sound = app.settings.sound == Some(true);
    out.push(Item {
        detail:
            "A sound when a session needs approval, or finishes while Orbit is in the background"
                .into(),
        also: "sound audio beep chime herdr",
        ..item(
            "actions",
            if sound { "Sounds off" } else { "Sounds on" },
            "",
            vec![Action::SetSound(!sound)],
        )
    });

    // Stats: your week in Orbit and the team's in SigNoz; not on the bar,
    // which holds only what asks for action.
    out.push(Item {
        detail: "Your last 7 days in Orbit, and the team's numbers from SigNoz".into(),
        also: "stats statistics numbers velocity dashboard signoz week activity",
        ..item("actions", "Stats", "", vec![Action::Show(View::Stats)])
    });

    // Repositories, the browser's way: a repo's page here, or its GitHub
    // pages; typed, like tickets, and one line that lists them all.
    out.push(Item {
        also: "repositories repos github browse",
        ..item(
            "repos",
            "Repositories",
            "9",
            vec![Action::Show(View::Repos)],
        )
    });
    for slug in app.repos() {
        out.push(Item {
            detail: "repo page: open PRs, yours and to review, each opens here".into(),
            also: "repo repository github",
            ..item(
                "repos",
                &slug,
                "",
                vec![Action::Show(View::Repo(slug.clone()))],
            )
        });
        out.push(Item {
            also: "repo repository github pulls browser",
            ..item(
                "repos",
                &format!("Open {slug} pulls on GitHub"),
                "",
                vec![Action::Open(format!("https://github.com/{slug}/pulls"))],
            )
        });
    }

    // Every keyboard shortcut, found by the key or by what it does.
    out.push(Item {
        detail: "Type a key (⌘D) or what it does (split) to find one".into(),
        also: "keys keyboard shortcuts help bindings",
        ..item(
            "keys",
            "Keyboard shortcuts",
            "",
            vec![Action::PaletteQuery("shortcut ".into())],
        )
    });
    for (keys, what) in crate::ui::SHORTCUTS {
        out.push(Item {
            also: "shortcut",
            ..item("keys", what, keys, Vec::new())
        });
    }

    // Customers: where a group of tickets lives.
    for (name, asks, moving, shipped) in app.customer_list() {
        out.push(Item {
            section: "customers",
            title: name.clone(),
            detail: format!("{asks} need you · {moving} moving · {shipped} shipped"),
            keys: "",
            also: "customer",
            text: String::new(),
            actions: vec![Action::Show(View::Customer(name))],
        });
    }

    out.push(Item {
        detail:
            "Claude asks a few questions and writes the profile: teams, the bot, maintenance, style"
                .into(),
        also: "profile company configure setup onboarding",
        ..item(
            "profile",
            if crate::profile::effective(&app.settings.profile).is_empty() {
                "Set up a profile with Claude"
            } else {
                "Improve the profile with Claude"
            },
            "",
            vec![Action::SetUpProfile],
        )
    });
    out.push(Item {
        detail: "Import a profile from a folder, a file or a URL, or edit profile.json".into(),
        also: "profile company import configure",
        ..item(
            "profile",
            "Profile settings",
            "",
            vec![Action::Show(View::Settings)],
        )
    });
    out.extend(laya_items(
        app.laya_ready,
        matches!(app.laya_download, Some(Ok(_))),
        &app.settings.laya.mode,
    ));

    // settings.json `debug`: each environment, its panes after its setup.
    for env in &app.settings.debug {
        out.push(Item {
            section: "debug",
            title: format!("Debug {}", env.name),
            detail: env
                .panes
                .iter()
                .map(|tool| env.command(tool))
                .collect::<Vec<_>>()
                .join("  |  "),
            keys: "",
            also: "environment cluster debug shell",
            text: String::new(),
            actions: vec![Action::Debug(env.name.clone())],
        });
    }

    // Live incidents: investigate one, or open it.
    for incident in &app.incidents {
        out.push(Item {
            section: "incidents",
            title: format!("Investigate {}  {}", incident.reference, incident.name),
            detail: format!(
                "{} · {}",
                incident.severity.to_lowercase(),
                incident.status.to_lowercase()
            ),
            keys: "",
            also: "incident outage sev debug",
            text: String::new(),
            actions: vec![Action::Investigate(incident.id.clone())],
        });
    }

    // The user's open PRs: found by repo, number or title; Tab on a repo
    // lists only its PRs.
    out.extend(app.my_prs.iter().map(my_pr_item));

    // Linear's own search, for tickets not on the board.
    for found in &app.palette_found {
        if app
            .cards
            .iter()
            .any(|c| c.issue.identifier == found.identifier)
        {
            continue;
        }
        out.push(found_item(found));
    }
    // Linear not answered yet, or failed: say so, never an empty section.
    let query = app.palette.as_ref().map_or("", |p| p.query.trim());
    let (answered, error) = &app.palette_answered;
    let status = match error {
        Some(error) if answered == query => Some(format!("Linear search failed: {error}")),
        _ if (query.len() >= 3 || crate::linear::ticket_number(query).is_some())
            && answered != query =>
        {
            Some("Searching Linear…".to_owned())
        }
        _ => None,
    };
    if let Some(status) = status {
        out.push(Item {
            section: "linear",
            title: status,
            detail: String::new(),
            keys: "",
            also: "",
            text: query.to_lowercase(),
            actions: Vec::new(),
        });
    }
    // Run the board only where a profile brings an orchestrator.
    if crate::profile::get().orchestrator.is_none() {
        out.retain(|item| !matches!(item.actions.as_slice(), [Action::Orchestrate]));
    }
    out
}

fn my_pr_item(pr: &crate::github::MyPr) -> Item {
    let draft = if pr.draft { " · draft" } else { "" };
    Item {
        section: "your prs",
        title: format!("{}#{}  {}", pr.repo, pr.number, pr.title),
        detail: format!("updated {}{draft}", story::ago(&pr.at)),
        keys: "",
        also: "pr pull request mine",
        text: String::new(),
        actions: vec![Action::OpenPr {
            url: pr.url.clone(),
            title: pr.title.clone(),
            at: pr.at.clone(),
        }],
    }
}

/// The repo Tab scopes to for what is typed: one of the user's PRs' repos,
/// the exact name first, then one it starts with, then one containing it.
pub fn repo_for(query: &str, prs: &[crate::github::MyPr]) -> Option<String> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }
    let mut repos: Vec<&str> = prs.iter().map(|p| p.repo.as_str()).collect();
    repos.sort_unstable();
    repos.dedup();
    let find = |hit: &dyn Fn(&str) -> bool| {
        repos
            .iter()
            .find(|r| hit(&r.to_lowercase()))
            .map(|r| r.to_string())
    };
    find(&|r| r == query)
        .or_else(|| find(&|r| r.starts_with(&query)))
        .or_else(|| find(&|r| r.contains(&query)))
}

/// How well `query` matches: every word must appear; word starts, the
/// title, and an exact ticket id count most. `None` hides the item.
pub fn score(query: &str, item: &Item) -> Option<i32> {
    let title = item.title.to_lowercase();
    let hay = format!(
        "{title} {} {} {} {}",
        item.detail.to_lowercase(),
        item.also,
        item.section,
        item.text
    );
    let mut total = 0;
    for word in query.to_lowercase().split_whitespace() {
        let at = hay.find(word)?;
        let starts_word = at == 0 || !hay.as_bytes()[at - 1].is_ascii_alphanumeric();
        total += if title.starts_with(word) {
            40
        } else if at < title.len() && starts_word {
            25
        } else if at < title.len() {
            12
        } else if starts_word {
            8
        } else {
            3
        };
    }
    // A ticket id typed out ("abc-431") goes to the top, an old one too,
    // and so does a repository typed by name ("api"): its
    // page is the way in.
    let typed = query.trim().to_lowercase();
    if title.split_whitespace().next() == Some(typed.as_str())
        || item.text.split_whitespace().any(|t| t == typed)
        || (item.section == "repos" && title.rsplit('/').next() == Some(typed.as_str()))
    {
        total += 100;
    }
    Some(total)
}

/// How many recent picks lead the palette when nothing is typed.
const RECENT: usize = 5;

/// What a line is, for the mark beside it: Linear's, GitHub's, a session,
/// a view to go to, a key, or something Orbit does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Linear,
    GitHub,
    Session(String),
    Go,
    Key,
    Action,
}

pub fn kind(item: &Item) -> Kind {
    let url_kind = |url: &str| {
        if url.contains("github.com") {
            Some(Kind::GitHub)
        } else if url.contains("linear.app") {
            Some(Kind::Linear)
        } else {
            None
        }
    };
    match item.section {
        "tickets" | "linear" | "stories" => return Kind::Linear,
        "your prs" | "repos" => return Kind::GitHub,
        "keys" => return Kind::Key,
        _ => {}
    }
    match item.actions.first() {
        Some(Action::Open(url) | Action::Link(url)) => url_kind(url).unwrap_or(Kind::Action),
        Some(Action::OpenPr { .. } | Action::DiffPr(_)) => Kind::GitHub,
        Some(Action::OpenTicket { .. } | Action::Focus(_) | Action::Select(_)) => Kind::Linear,
        Some(Action::OpenSession(key) | Action::Attach(key)) => Kind::Session(key.clone()),
        Some(
            Action::Show(_)
            | Action::Sheet(_)
            | Action::GoWindow(_)
            | Action::Tab(_)
            | Action::SessionsFiltered(_),
        ) => Kind::Go,
        _ => Kind::Action,
    }
}

/// A group's heading in the palette.
pub fn section_name(section: &str) -> &str {
    match section {
        "recent" => "recent",
        "tickets" => "linear · your board",
        "linear" => "linear · search",
        "your prs" => "github · your PRs",
        "repos" => "github · repos",
        "actions" => "orbit · actions",
        "panes" => "orbit · panes and windows",
        "keys" => "keyboard shortcuts",
        "debug" => "orbit · debug",
        other => other,
    }
}

/// The user's picks in the palette: how often each line was run, and
/// when last (`palette.json`, read-modify-write).
pub type Picks = std::collections::HashMap<String, (u32, u64)>;
const PICKS: &str = "palette.json";

pub fn load_picks() -> Picks {
    crate::store::load(PICKS)
}

/// Counts one more pick of `title`; keeps the 200 latest.
pub fn record_pick(title: &str, now_ms: u64) -> Picks {
    let mut picks = load_picks();
    let entry = picks.entry(title.to_owned()).or_insert((0, 0));
    entry.0 += 1;
    entry.1 = now_ms;
    if picks.len() > 200 {
        let mut by_age: Vec<(String, u64)> =
            picks.iter().map(|(t, (_, at))| (t.clone(), *at)).collect();
        by_age.sort_by_key(|(_, at)| std::cmp::Reverse(*at));
        let keep: std::collections::HashSet<String> =
            by_age.into_iter().take(200).map(|(t, _)| t).collect();
        picks.retain(|t, _| keep.contains(t));
    }
    let _ = crate::store::save(PICKS, &picks);
    picks
}

/// Titles picked, newest first.
pub fn recent(picks: &Picks) -> Vec<String> {
    let mut all: Vec<(&String, u64)> = picks.iter().map(|(t, (_, at))| (t, *at)).collect();
    all.sort_by_key(|(_, at)| std::cmp::Reverse(*at));
    all.into_iter().map(|(t, _)| t.clone()).collect()
}

/// How much a line is lifted: picked often or lately, a ticket that asks
/// something of the user, a session waiting on them.
pub fn lift(picks: &Picks, now_ms: u64, asks: impl Fn(&str) -> bool, item: &Item) -> i32 {
    let mut lift = 0;
    if let Some((count, at)) = picks.get(&item.title) {
        lift += (*count).min(10) as i32 * 3;
        let age = now_ms.saturating_sub(*at);
        lift += if age < 3_600_000 {
            20
        } else if age < 86_400_000 {
            10
        } else {
            0
        };
    }
    if let Some(Action::OpenTicket { id, .. } | Action::Focus(id) | Action::Select(id)) =
        item.actions.first()
        && asks(id)
    {
        lift += 15;
    }
    if item.section == "sessions" {
        lift += match state_rank(&item.detail) {
            0 => 15,
            1 => 10,
            _ => 0,
        };
    }
    lift
}

/// Cmd+K's "New session: <what>", with an optional " in <folder>" at the
/// end: the session's name and the folder it works in.
pub fn new_session_parts(typed: &str) -> (String, Option<String>) {
    match typed.rsplit_once(" in ") {
        Some((name, folder))
            if !name.trim().is_empty()
                && (folder.starts_with('~')
                    || folder.starts_with('/')
                    || folder.starts_with('.')) =>
        {
            (name.trim().to_owned(), Some(folder.trim().to_owned()))
        }
        _ => (typed.trim().to_owned(), None),
    }
}

/// Where a session's state sorts it: what waits on the user first, as
/// herdr's priority order (blocked, done, working, idle).
fn state_rank(detail: &str) -> u8 {
    let state = |word: &str| detail.split(" · ").any(|part| part == word);
    if state("needs approval") {
        0
    } else if state("your turn") || state("failed") {
        1
    } else if state("working") {
        2
    } else if state("idle") {
        3
    } else {
        4
    }
}

/// The items for `query`, best first; the empty query keeps the list order
/// without the tickets (the sheet's For you is where they queue) and
/// without the shortcut lines (one line offers them).
#[cfg(test)]
pub fn rank(query: &str, items: Vec<Item>) -> Vec<Item> {
    rank_by(query, items, |_| 0, &[])
}

/// `rank` with what the user does: `boost` lifts lines (recent picks,
/// what waits on them), and with nothing typed the lines in `recent`
/// (titles, newest first) come first as their own group.
pub fn rank_by(
    query: &str,
    items: Vec<Item>,
    boost: impl Fn(&Item) -> i32,
    recent: &[String],
) -> Vec<Item> {
    if query.trim().is_empty() {
        let mut first: Vec<Item> = Vec::new();
        let mut rest: Vec<Item> = Vec::new();
        for item in items {
            if item.section != "tickets"
                && item.section != "linear"
                && recent.iter().take(RECENT).any(|t| *t == item.title)
                && !first.iter().any(|f| f.title == item.title)
            {
                first.push(Item {
                    section: "recent",
                    ..item
                });
            } else {
                rest.push(item);
            }
        }
        first.sort_by_key(|i| recent.iter().position(|t| *t == i.title));
        return first
            .into_iter()
            .chain(
                rest.into_iter()
                    .filter(|item| item.section != "tickets")
                    .filter(|item| item.section != "keys" || item.keys.is_empty())
                    .filter(|item| item.section != "repos" || !item.keys.is_empty()),
            )
            .collect();
    }
    let mut scored: Vec<(i32, usize, Item)> = items
        .into_iter()
        .enumerate()
        .filter_map(|(i, item)| score(query, &item).map(|s| (s + boost(&item), i, item)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, item)| item).collect()
}

/// Laya from the keyboard: its download while it is missing, and its mode.
fn laya_items(ready: bool, downloading: bool, mode: &str) -> Vec<Item> {
    const ALSO: &str = "laya model bug decision local offline haiku";
    let mut out = Vec::new();
    if !ready && !downloading {
        out.push(Item {
            detail: "840 MB, once, from Hugging Face; then it runs inside the app, offline".into(),
            also: ALSO,
            ..item("laya", "Download Laya", "", vec![Action::DownloadLaya])
        });
    }
    let modes = [
        ("off", "Haiku decides bug or not"),
        (
            "shadow",
            "Haiku decides, Laya is compared (Settings shows how often they agree)",
        ),
        (
            "use",
            "Laya decides bug or not where it is sure, Haiku otherwise",
        ),
    ];
    for (name, what) in modes {
        let now = if mode.trim() == name {
            " · on now"
        } else {
            ""
        };
        let later = if name != "off" && !ready {
            " · downloads Laya first"
        } else {
            ""
        };
        out.push(Item {
            detail: format!("{what}{now}{later}"),
            also: ALSO,
            ..item(
                "laya",
                &format!("Laya: {name}"),
                "",
                vec![Action::SetLayaMode(name.into())],
            )
        });
    }
    out
}

/// A ticket Linear's search found off the board: opens here, fetched from
/// Linear first (`Action::Link`), not in the browser.
fn found_item(found: &crate::capture::Duplicate) -> Item {
    Item {
        section: "linear",
        title: format!("{}  {}", found.identifier, found.title),
        detail: format!("{} · {} · not on your board", found.team, found.state),
        keys: "",
        also: "",
        text: String::new(),
        actions: vec![Action::Link(found.url.clone())],
    }
}

/// `state` comes before `now` on the board: not started yet, or an
/// earlier started column (Linear's `position`).
fn behind(state: &crate::linear::State, now: &crate::linear::State) -> bool {
    let rank = |kind: &str| match kind {
        "triage" | "backlog" | "unstarted" => 0,
        "started" => 1,
        _ => 2,
    };
    (rank(&state.kind), state.position) < (rank(&now.kind), now.position)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_merged_ticket_is_not_offered_a_move_back() {
        let state = |kind: &str, position: f64| crate::linear::State {
            id: format!("{kind}{position}"),
            name: String::new(),
            color: String::new(),
            position,
            kind: kind.into(),
        };
        let testing = state("started", 3.0);
        assert!(behind(&state("started", 2.0), &testing), "In Review");
        assert!(behind(&state("backlog", 0.0), &testing));
        assert!(!behind(&state("completed", 0.0), &testing), "Done");
        assert!(!behind(&state("started", 4.0), &testing));
    }

    #[test]
    fn each_line_shows_what_it_is_and_picks_come_first() {
        let line = |section, title: &str, action: Action| item(section, title, "", vec![action]);
        let pr = line(
            "this ticket",
            "Open the PR r#1",
            Action::Open("https://github.com/o/r/pull/1".into()),
        );
        assert_eq!(kind(&pr), Kind::GitHub);
        let ticket = line(
            "this ticket",
            "Open ABC-1 in Linear",
            Action::Open("https://linear.app/x/issue/ABC-1".into()),
        );
        assert_eq!(kind(&ticket), Kind::Linear);
        let debug = line("debug", "Debug dev", Action::Debug("dev".into()));
        assert_eq!(kind(&debug), Kind::Action);
        assert_eq!(
            kind(&line("actions", "Sessions", Action::Show(View::Sessions))),
            Kind::Go
        );
        // Nothing typed: the recent picks lead, newest first, as their own group.
        let items = vec![pr.clone(), ticket.clone(), debug.clone()];
        let recent = vec!["Debug dev".to_owned(), "Open the PR r#1".to_owned()];
        let ranked = rank_by("", items, |_| 0, &recent);
        assert_eq!(ranked[0].title, "Debug dev");
        assert_eq!(ranked[0].section, "recent");
        assert_eq!(ranked[1].title, "Open the PR r#1");
        // Typed: a line picked often and just now beats an equal match.
        let mut picks = Picks::new();
        picks.insert("Debug dev".into(), (5, 1_000));
        let lifted = lift(&picks, 2_000, |_| false, &debug);
        assert!(lifted >= 35, "{lifted}");
        assert_eq!(lift(&picks, 2_000, |_| false, &pr), 0);
    }

    use super::*;

    #[test]
    fn tab_scopes_to_the_repo_the_typed_text_names() {
        let pr = |repo: &str| crate::github::MyPr {
            url: format!("https://github.com/o/{repo}/pull/1"),
            repo: repo.into(),
            number: 1,
            title: String::new(),
            at: String::new(),
            draft: false,
        };
        let prs = [pr("web-ui"), pr("api"), pr("web")];
        assert_eq!(repo_for("web", &prs).as_deref(), Some("web"), "exact first");
        assert_eq!(repo_for("Ap", &prs).as_deref(), Some("api"));
        assert_eq!(repo_for("ui", &prs).as_deref(), Some("web-ui"));
        assert_eq!(repo_for("orbit", &prs), None);
        assert_eq!(repo_for(" ", &prs), None);
    }

    fn named(section: &'static str, title: &str, also: &'static str) -> Item {
        Item {
            also,
            ..item(section, title, "", vec![Action::Refresh])
        }
    }

    #[test]
    fn laya_offers_its_download_until_it_is_here_and_every_mode() {
        let titles = |items: Vec<Item>| items.into_iter().map(|i| i.title).collect::<Vec<_>>();
        assert_eq!(
            titles(laya_items(false, false, "off")),
            ["Download Laya", "Laya: off", "Laya: shadow", "Laya: use"]
        );
        assert!(!titles(laya_items(false, true, "off")).contains(&"Download Laya".into()));
        assert!(!titles(laya_items(true, false, "off")).contains(&"Download Laya".into()));
        let items = laya_items(true, false, "shadow");
        let shadow = items.iter().find(|i| i.title == "Laya: shadow").unwrap();
        assert!(shadow.detail.ends_with("on now"));
        let missing = laya_items(false, false, "off");
        let used = missing.iter().find(|i| i.title == "Laya: use").unwrap();
        assert!(used.detail.contains("downloads Laya first"));
        // Found by what it does, not only by name.
        assert_eq!(rank("local bug", laya_items(true, false, "off")).len(), 3);
    }

    #[test]
    fn a_typed_ticket_id_wins_and_every_word_must_match() {
        let items = vec![
            named("actions", "New ticket", "create file report bug"),
            named("tickets", "ABC-431  Add filter control to dashboard", ""),
            named("tickets", "ABC-4310  Something else", ""),
        ];
        let ranked = rank("abc-431", items);
        assert_eq!(ranked[0].title, "ABC-431  Add filter control to dashboard");

        let items = vec![
            named("actions", "New ticket", "create file report bug"),
            named("tickets", "ABC-431  Add filter control to dashboard", ""),
        ];
        // "report bug" answers to New ticket through its extra words.
        assert_eq!(rank("report bug", items)[0].title, "New ticket");

        let items = vec![named("actions", "Settings", "preferences")];
        assert!(rank("settings nothing", items).is_empty());
        // Nothing typed: the actions, not the ticket queue.
        let items = vec![
            named("tickets", "ABC-1  A", ""),
            named("actions", "Settings", ""),
        ];
        assert_eq!(rank(" ", items).len(), 1);
    }

    #[test]
    fn a_moved_ticket_answers_to_its_old_id() {
        let mut moved = named("tickets", "XYZ-768  Saved views lost their filters", "");
        moved.text = "qa-905 pri-905".into();
        let items = vec![named("tickets", "QA-9050  Something else", ""), moved];
        assert_eq!(
            rank("qa-905", items)[0].title,
            "XYZ-768  Saved views lost their filters"
        );
    }

    #[test]
    fn a_found_ticket_opens_in_orbit() {
        let url = "https://linear.app/acme/issue/ABC-894/x";
        let found = crate::capture::Duplicate {
            identifier: "ABC-894".into(),
            url: url.into(),
            ..Default::default()
        };
        assert!(matches!(&found_item(&found).actions[..], [Action::Link(u)] if u == url));
    }

    #[test]
    fn sessions_sort_by_what_waits_on_the_user_and_shortcuts_hide_until_asked() {
        assert!(state_rank("Claude · needs approval") < state_rank("Claude · your turn"));
        assert!(state_rank("Claude · your turn") < state_rank("Claude · working"));
        assert!(state_rank("Claude · working") < state_rank("Claude · idle"));
        assert!(state_rank("Claude · idle") < state_rank("Claude · stopped"));
        assert_eq!(
            state_rank("working on your turn"),
            4,
            "a title is not a state"
        );
        let items = vec![
            named("keys", "Keyboard shortcuts", "keys"),
            Item {
                also: "shortcut",
                ..item("keys", "Split right", "⌘D", Vec::new())
            },
        ];
        assert_eq!(rank("", items).len(), 1, "one line offers the shortcuts");
        let mut items = vec![
            named("repos", "Repositories", "repos"),
            named("repos", "acme/web", "repo"),
        ];
        items[0].keys = "9";
        assert_eq!(rank("", items).len(), 1, "one line offers the repos");
        // A repo typed by name beats a session that starts with it.
        let items = vec![
            named("sessions", "api#1124 fix the thing", ""),
            named("repos", "acme/api", "repo"),
        ];
        assert_eq!(rank("api", items)[0].title, "acme/api");
        let items = vec![
            named("keys", "Keyboard shortcuts", "keys"),
            Item {
                also: "shortcut",
                ..item("keys", "Split right", "⌘D", Vec::new())
            },
        ];
        let titles: Vec<String> = rank("shortcut", items)
            .into_iter()
            .map(|i| i.title)
            .collect();
        assert_eq!(titles, ["Keyboard shortcuts", "Split right"]);
    }

    #[test]
    fn a_new_session_line_may_name_its_folder() {
        assert_eq!(
            new_session_parts("fix the build in ~/Development/api"),
            ("fix the build".into(), Some("~/Development/api".into()))
        );
        assert_eq!(new_session_parts("log in bug"), ("log in bug".into(), None));
        assert_eq!(new_session_parts(" notes "), ("notes".into(), None));
    }

    #[test]
    fn title_starts_beat_matches_deep_in_the_detail() {
        let mut deep = named("tickets", "XYZ-9  Fix login", "");
        deep.detail = "planning · split reports".into();
        let items = vec![deep, named("panes", "Split right", "pane split vertical")];
        assert_eq!(rank("split", items)[0].title, "Split right");
    }
}

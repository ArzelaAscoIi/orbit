//! What the sheet shows (`bar::sheet`): the tickets the current view
//! lists, grouped (For you in columns), the customers, or the day's plan.
//! The row under the sheet's cursor is the selected one; a click shows the
//! ticket in an empty pane, else in its own window (`Action::Focus`).

use super::*;
use bar::Entry;

pub(super) fn show(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let cursor = bar::cursor(app);
    let cursor = cursor.as_ref();
    match app.view {
        View::Sessions => sessions::list(app, cursor, ui, actions),
        View::Customers => customers(app, cursor, ui, actions),
        View::ForYou => for_you(app, cursor, ui, actions),
        View::Inbox => inbox(app, cursor, ui, actions),
        View::Alerts => alerts(app, ui, actions),
        View::Repos => repos(app, cursor, ui, actions),
        View::Stats => stats(app, ui, actions),
        View::Repo(ref slug) => repo(app, slug, cursor, ui, actions),
        _ => tickets(app, cursor, ui, actions),
    }
}

/// Stats: your week in Orbit (this Mac's own logs) beside the team's from
/// SigNoz (the profile's dashboard, over its MCP gateway, no key).
fn stats(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let local = crate::stats::read(&crate::store::data_dir());
    let profile = crate::profile::get();
    let panel = ui.scope(|ui| {
        let subtitle = if profile.stats.mcp.is_empty() {
            "The last 7 days: you in Orbit"
        } else {
            "The last 7 days: you in Orbit, and the team in SigNoz"
        };
        title_bar(ui, "Stats", subtitle, "esc close");
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let line = |ui: &mut Ui, what: &str, value: String| {
                    ui.horizontal(|ui| {
                        ui.add_sized([260.0, 18.0], egui::Label::new(label(what)));
                        ui.label(RichText::new(value).family(medium()).color(p().text));
                    });
                };
                group_label(
                    ui,
                    "// you, in orbit  (this mac's activity log and session logs)",
                );
                pad(ui, |ui| {
                    line(
                        ui,
                        "actions",
                        format!(
                            "{} on {} days · {:.0}% by key",
                            local.actions,
                            local.days,
                            local.by_key * 100.0
                        ),
                    );
                    line(
                        ui,
                        "approved · merged",
                        format!("{} · {}", local.approvals, local.merges),
                    );
                    line(
                        ui,
                        "skills run · sessions opened",
                        format!("{} · {}", local.skills_run, local.sessions_opened),
                    );
                    line(
                        ui,
                        "your sessions: turns · asked you · PRs",
                        format!(
                            "{} · {} · {}",
                            local.turns, local.questions, local.prs_opened
                        ),
                    );
                    let gates = ["yours", "queue", "do", "look", "app"]
                        .iter()
                        .map(|g| format!("{g} {}", local.gates.get(*g).copied().unwrap_or(0)))
                        .collect::<Vec<_>>()
                        .join(" · ");
                    line(ui, "by gate", gates);
                    if !local.top.is_empty() {
                        let top = local
                            .top
                            .iter()
                            .map(|(e, n)| format!("{e} {n}"))
                            .collect::<Vec<_>>()
                            .join(" · ");
                        line(ui, "most taken decisions", top);
                    }
                });
                hairline(ui);
                group_label(ui, "// the team, in signoz  (the dashboard's repos)");
                pad(ui, |ui| {
                    if profile.stats.mcp.is_empty() {
                        ui.label(label(
                            "Team numbers need a SigNoz dashboard in your profile; until then, \
                             only your own.",
                        ));
                        return;
                    }
                    match &app.team_stats {
                        None => {
                            ui.label(label("asking SigNoz…"));
                        }
                        Some((_, Err(error))) => {
                            ui.add(egui::Label::new(label(error).color(p().danger)).wrap());
                        }
                        Some((at, Ok(team))) => {
                            let share = |n: f64| {
                                if team.merged > 0.0 {
                                    format!("{:.0}%", 100.0 * n / team.merged)
                                } else {
                                    "-".into()
                                }
                            };
                            line(
                                ui,
                                "PRs touched · merged",
                                format!("{:.0} · {:.0}", team.touched, team.merged),
                            );
                            line(
                                ui,
                                "merged by a bot · AI-assisted",
                                format!(
                                    "{:.0} ({}) · {:.0} ({})",
                                    team.merged_bot,
                                    share(team.merged_bot),
                                    team.merged_ai,
                                    share(team.merged_ai)
                                ),
                            );
                            let who = if team.login.is_empty() {
                                "you".to_owned()
                            } else {
                                format!("@{}", team.login)
                            };
                            line(
                                ui,
                                &format!("merged by {who}"),
                                format!("{:.0} ({})", team.mine_merged, share(team.mine_merged)),
                            );
                            let reviews = if team.my_reviews.is_empty() {
                                "none".to_owned()
                            } else {
                                team.my_reviews
                                    .iter()
                                    .map(|(s, n)| format!("{} {n:.0}", s.to_lowercase()))
                                    .collect::<Vec<_>>()
                                    .join(" · ")
                            };
                            line(ui, &format!("reviews by {who}"), reviews);
                            ui.add_space(6.0);
                            ui.label(label(&format!(
                                "asked SigNoz {}m ago, again when shown after 10m",
                                at.elapsed().as_secs() / 60
                            )));
                        }
                    }
                    if !profile.stats.dashboard.is_empty() {
                        ui.add_space(6.0);
                        if link(ui, label("the full dashboard in SigNoz ↗").underline()).clicked()
                        {
                            actions.push(Action::Open(profile.stats.dashboard.clone()));
                        }
                    }
                });
            });
    });
    mark(app, ui, "stats", panel.response.rect);
}

/// Repos: every repository the user's work touches, one row each with
/// how many of their PRs are open there and how many wait on their
/// review; a click opens the repo's page (`View::Repo`), the browser's way.
fn repos(app: &App, cursor: Option<&Entry>, ui: &mut Ui, actions: &mut Vec<Action>) {
    let repos: Vec<String> = app
        .repos()
        .into_iter()
        .filter(|slug| matches_all(slug, &app.sheet_query))
        .collect();
    let (inbox, _) = app.inbox();
    let panel = ui.scope(|ui| {
        title_bar(
            ui,
            "Repositories",
            "Your PRs', the board's PRs', the watched deploys' and the repos you pin",
            bar::hints(&app.view),
        );
        sheet_search(app, ui, "search repositories", actions);
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                for slug in &repos {
                    let mine = app
                        .my_prs
                        .iter()
                        .filter(|pr| crate::github::slug_of(&pr.url).as_deref() == Some(slug))
                        .count();
                    let review = inbox
                        .iter()
                        .filter(|i| i.ask == crate::inbox::Ask::Review)
                        .filter(|i| {
                            i.pr.as_deref().and_then(crate::github::slug_of).as_deref()
                                == Some(slug)
                        })
                        .count();
                    let selected = cursor == Some(&Entry::Repo(slug.clone()));
                    let response = row(ui, selected, p().signal, true, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(slug).family(medium()).color(p().text));
                            if review > 0 {
                                ui.label(label(&format!("{review} to review")).color(p().signal));
                            }
                            if mine > 0 {
                                ui.label(label(&format!("{mine} yours")));
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                for (what, path) in [("actions", "actions"), ("pulls", "pulls")] {
                                    if link(ui, label(&format!("{what} ↗")))
                                        .on_hover_text(format!("{slug}/{path} on GitHub"))
                                        .clicked()
                                    {
                                        actions.push(Action::Open(format!(
                                            "https://github.com/{slug}/{path}"
                                        )));
                                    }
                                    ui.add_space(10.0);
                                }
                            });
                        });
                    });
                    if selected {
                        bar::follow(ui, &response, slug);
                    }
                    if response.clicked() {
                        actions.push(Action::Show(View::Repo(slug.clone())));
                    }
                }
                if repos.is_empty() {
                    pad(ui, |ui| {
                        ui.label(label(
                            "No repositories yet: they come from your open PRs, the board's PRs, \
                             the watched deploys, or repos you pin (`repos` in Settings).",
                        ));
                    });
                }
            });
    });
    mark(app, ui, "repos", panel.response.rect);
}

/// A repo page's PRs in the order it draws them: waiting on your review,
/// yours, the rest; newest change first in each.
/// A search finds a PR by its number (`#12`, `12`), title, author (login
/// or name), and
/// state words: draft, approved, red, green, review, yours.
pub(super) fn repo_order<'a>(
    prs: &'a [crate::github::RepoPr],
    mine: &[crate::github::MyPr],
    query: &str,
) -> Vec<&'a crate::github::RepoPr> {
    let own = |pr: &crate::github::RepoPr| mine.iter().any(|m| m.url == pr.url);
    let mut out: Vec<&crate::github::RepoPr> = prs
        .iter()
        .filter(|pr| {
            let text = format!(
                "#{n} {n} {} {} {} {} {} {} {} {}",
                pr.title,
                pr.author,
                pr.name,
                if pr.draft { "draft" } else { "" },
                pr.review.replace('_', " "),
                pr.checks,
                if pr.review_requested {
                    "review requested"
                } else {
                    ""
                },
                if own(pr) { "yours mine" } else { "" },
                n = pr.number,
            );
            matches_all(&text, query)
        })
        .collect();
    out.sort_by_key(|pr| match (pr.review_requested, own(pr)) {
        (true, _) => 0,
        (_, true) => 1,
        _ => 2,
    });
    out
}

/// One repository's page: its open PRs grouped by who waits on whom, each
/// opening here as a PR card (diff, approve, merge, Claude), and the
/// GitHub pages a browser tab would have.
fn repo(app: &App, slug: &str, cursor: Option<&Entry>, ui: &mut Ui, actions: &mut Vec<Action>) {
    let fetched = app.repo_prs.get(slug);
    let panel = ui.scope(|ui| {
        let subtitle = match fetched {
            Some((at, Ok(prs))) => format!(
                "{} open pull requests · asked GitHub {}",
                prs.len(),
                match at.elapsed().as_secs() {
                    s if s < 60 => "just now".to_owned(),
                    s => format!("{}m ago", s / 60),
                }
            ),
            Some((_, Err(_))) => "GitHub did not answer".to_owned(),
            None => "asking GitHub…".to_owned(),
        };
        title_bar(ui, slug, &subtitle, bar::hints(&app.view));
        pad(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if link(ui, label("← repos").underline()).clicked() {
                    actions.push(Action::Show(View::Repos));
                }
                ui.add_space(10.0);
                for (what, path) in [
                    ("code", ""),
                    ("pulls", "/pulls"),
                    ("issues", "/issues"),
                    ("actions", "/actions"),
                ] {
                    if link(ui, label(&format!("{what} ↗")))
                        .on_hover_text(format!("https://github.com/{slug}{path}"))
                        .clicked()
                    {
                        actions.push(Action::Open(format!("https://github.com/{slug}{path}")));
                    }
                    ui.add_space(10.0);
                }
                if link(ui, label("refresh"))
                    .on_hover_text("Ask GitHub again")
                    .clicked()
                {
                    actions.push(Action::RefreshRepo(slug.to_owned()));
                }
            });
        });
        hairline(ui);
        if let Some((_, Err(error))) = fetched {
            pad(ui, |ui| {
                ui.add(egui::Label::new(label(error).color(p().danger)).wrap());
            });
        }
        let rect = sheet_search(
            app,
            ui,
            "search pull requests: number, title, author, draft, approved, red, yours",
            actions,
        );
        mark(app, ui, "repo-search", rect);
        let prs = fetched
            .and_then(|(_, prs)| prs.as_ref().ok())
            .map(|prs| repo_order(prs, &app.my_prs, &app.sheet_query))
            .unwrap_or_default();
        let own = |pr: &crate::github::RepoPr| app.my_prs.iter().any(|m| m.url == pr.url);
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                let mut group: Option<&str> = None;
                for pr in &prs {
                    let name = if pr.review_requested {
                        "to review"
                    } else if own(pr) {
                        "yours"
                    } else {
                        "open"
                    };
                    if group != Some(name) {
                        group = Some(name);
                        let n = prs
                            .iter()
                            .filter(|p| {
                                (p.review_requested, own(p)) == (pr.review_requested, own(pr))
                                    || (name == "open" && !p.review_requested && !own(p))
                            })
                            .count();
                        group_label(ui, &format!("// {name}  {n}"));
                    }
                    let selected = cursor == Some(&Entry::Pr(pr.url.clone()));
                    let response = row(ui, selected, p().signal, true, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(label(&format!("#{}", pr.number)));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&pr.title).family(medium()).color(p().text),
                                )
                                .truncate(),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(label(&story::ago(&pr.at)));
                                ui.add_space(8.0);
                                let (checks, color) = match pr.checks.as_str() {
                                    "green" => ("checks green", p().success),
                                    "red" => ("checks red", p().danger),
                                    "pending" => ("checks running", p().text_3),
                                    _ => ("", p().text_3),
                                };
                                if !checks.is_empty() {
                                    ui.label(label(checks).color(color));
                                    ui.add_space(8.0);
                                }
                                let review = match pr.review.as_str() {
                                    "APPROVED" => "approved",
                                    "CHANGES_REQUESTED" => "changes requested",
                                    "REVIEW_REQUIRED" => "review needed",
                                    _ => "",
                                };
                                if pr.draft {
                                    ui.label(label("draft"));
                                    ui.add_space(8.0);
                                } else if !review.is_empty() {
                                    ui.label(label(review));
                                    ui.add_space(8.0);
                                }
                                ui.label(label(&pr.author).color(p().text_2));
                            });
                        });
                    });
                    if selected {
                        bar::follow(ui, &response, &pr.url);
                    }
                    if response.clicked() {
                        actions.push(Action::OpenPr {
                            url: pr.url.clone(),
                            title: pr.title.clone(),
                            at: pr.at.clone(),
                        });
                    }
                }
                if prs.is_empty() && matches!(fetched, Some((_, Ok(_)))) {
                    pad(ui, |ui| {
                        ui.label(label(if app.sheet_query.trim().is_empty() {
                            "No open pull requests."
                        } else {
                            "No pull request matches."
                        }));
                    });
                }
            });
    });
    mark(app, ui, "repo", panel.response.rect);
}

/// For you: one column per thing to do, then what is assigned to you.
fn for_you(app: &App, cursor: Option<&Entry>, ui: &mut Ui, actions: &mut Vec<Action>) {
    let cards = app.visible();
    let rect = ui
        .scope(|ui| {
            header(app, &cards, ui, actions);
            setup(app, ui, actions);
            ui.columns(bar::FOR_YOU.len(), |columns| {
                for (i, (column, (ask, name))) in columns.iter_mut().zip(bar::FOR_YOU).enumerate() {
                    // Columns lay out justified, which spreads wrapped
                    // words apart: rows go top-down, left-aligned.
                    let ui = &mut column.new_child(
                        egui::UiBuilder::new()
                            .max_rect(column.max_rect())
                            .layout(Layout::top_down(Align::Min)),
                    );
                    let these: Vec<&Card> =
                        cards.iter().filter(|c| c.ask == ask).copied().collect();
                    group_label(ui, &format!("// {name}  {}", these.len()));
                    if ask == Some(story::Ask::Review) && !these.is_empty() {
                        let rect = pad(ui, |ui| {
                            let button = hover_hand(&ui.button("✳ Ship the bot's PRs")).on_hover_text(
                                "One Claude session reviews each PR here: a fix that works gets \
                                 your approval and is merged at the head you saw, one that needs \
                                 a person moves to its team. It asks you before anything",
                            );
                            if button.clicked() {
                                actions.push(Action::ShipReviews);
                            }
                        });
                        mark(app, ui, "ship-reviews", rect);
                    }
                    egui::ScrollArea::vertical()
                        .id_salt(("for-you", i))
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            for card in &these {
                                card_row(app, card, cursor, ui, actions);
                            }
                            if these.is_empty() && !app.loading {
                                pad(ui, |ui| {
                                    ui.label(label("nothing here"));
                                });
                            }
                        });
                }
            });
        })
        .response
        .rect;
    mark(app, ui, "list", rect);
}

/// One ticket row; the cursor's is selected and scrolled to when it moves
/// there. A click shows it in an empty pane, else in its own window.
fn card_row(
    app: &App,
    card: &Card,
    cursor: Option<&Entry>,
    ui: &mut Ui,
    actions: &mut Vec<Action>,
) {
    let selected = cursor == Some(&Entry::Ticket(card.issue.id.clone()));
    let (response, read) = ticket_card(
        ui,
        card,
        app.short_title(card),
        selected,
        app.claude.saved.get(&card.issue.id).map(|s| s.agent),
        app.selected.as_deref() == Some(card.issue.id.as_str())
            || app
                .panes
                .others()
                .iter()
                .any(|(_, v)| v.selected.as_deref() == Some(card.issue.id.as_str())),
        app.view == View::ForYou,
    );
    if read {
        actions.push(Action::MarkRead(card.issue.id.clone()));
    }
    if selected {
        mark(app, ui, "row", response.rect);
        bar::follow(ui, &response, &card.issue.id);
    }
    if response.clicked() {
        actions.push(Action::Focus(card.issue.id.clone()));
    }
    // Who else is on its open PRs already, under the row.
    let on_it: Vec<&String> = card
        .prs
        .iter()
        .filter(|p| p.open)
        .filter_map(|p| match app.gates.get(&p.url) {
            Some(Ok(gate)) => Some(&gate.on_it),
            _ => None,
        })
        .flatten()
        .collect();
    if !on_it.is_empty() {
        let names: Vec<&str> = on_it.iter().map(|s| s.as_str()).collect();
        Frame::new()
            .inner_margin(Margin {
                left: 22,
                right: 12,
                top: 0,
                bottom: 6,
            })
            .show(ui, |ui| {
                ui.add(egui::Label::new(label(&format!("on it: {}", names.join(", ")))).truncate());
            });
    }
}

fn tickets(app: &App, cursor: Option<&Entry>, ui: &mut Ui, actions: &mut Vec<Action>) {
    let cards = app.visible();
    let panel = ui.scope(|ui| {
        header(app, &cards, ui, actions);
        if app.view == View::Ktlo {
            incidents(app, ui, actions);
            if !cards.is_empty() {
                let rect = pad(ui, |ui| {
                    let text = format!("✳ Clear {} with Claude", profile_ktlo());
                    let button = hover_hand(&ui.button(text)).on_hover_text(
                        "One Claude session reads every ticket here: fixes that only need a \
                         review get approved and merged, the ones that need a person move to \
                         their team. It asks you before approving, merging or moving anything",
                    );
                    if button.clicked() {
                        actions.push(Action::ClearKtlo);
                    }
                });
                mark(app, ui, "clear-ktlo", rect);
                hairline(ui);
            }
        }
        if app.view == View::Intake {
            // Both side by side, like the other views' action rows.
            pad(ui, |ui| {
                ui.horizontal(|ui| {
            let label = if app.claude.saved.contains_key(crate::claude::TRIAGE) {
                "✳ Continue triage with Claude"
            } else {
                "✳ Triage with Claude"
            };
            let triage = primary(ui, label)
                .on_hover_text("Clusters the Triage column, folds duplicates, and proposes what to accept, park or cancel. Nothing is written until you approve.");
            mark(app, ui, "triage", triage.rect);
            if triage.clicked() {
                actions.push(Action::Triage);
            }
            let find = hover_hand(&ui.button("✳ Find work in the backlog")).on_hover_text(
                "One Claude session reads this week's goals and priority docs, ranks the \
                 backlog tickets that move them, and moves the ones you pick to Todo",
            );
            mark(app, ui, "find-work", find.rect);
            if find.clicked() {
                actions.push(Action::FindWork);
            }
                });
            });
            hairline(ui);
        }
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            // The batch review scrolls with the lanes under it.
            if app.view == View::Ktlo {
                let rect = ui.scope(|ui| batch(app, ui, actions)).response.rect;
                mark(app, ui, "batch-review", rect);
            }
            let mut group: Option<String> = None;
            let now = jiff::Timestamp::now();
            let fresh = |c: &Card| app.view == View::Intake && c.new_today(now);
            for card in &cards {
                // Section breaks: by story on Stories and Intake, by bucket
                // for a customer, by lane on KTLO.
                let heading = match &app.view {
                    _ if fresh(card) => Some("new today".to_owned()),
                    View::Intake => {
                        Some(app.short_story(&card.story_key, &card.story).to_owned())
                    }
                    View::Customer(_) => Some(story::bucket(card).to_owned()),
                    View::Ktlo => Some(card.lane.label().to_owned()),
                    _ => None,
                };
                if heading.is_some() && heading != group {
                    let n = cards
                        .iter()
                        .filter(|c| fresh(c) == fresh(card))
                        .filter(|c| match &app.view {
                            _ if fresh(card) => true,
                            View::Customer(_) => story::bucket(c) == story::bucket(card),
                            View::Ktlo => c.lane == card.lane,
                            _ => c.story_key == card.story_key,
                        })
                        .count();
                    let text = format!(
                        "// {}  {n}",
                        heading.as_deref().unwrap_or_default().to_lowercase()
                    );
                    if fresh(card) {
                        group_label_in(ui, label(&text).color(p().danger));
                    } else {
                        group_label(ui, &text);
                    }
                    group = heading;
                }
                card_row(app, card, cursor, ui, actions);
            }
            if cards.is_empty() && !app.loading {
                // What the view would hold, and where its tickets come from.
                ui.add_space(24.0);
                pad(ui, |ui| {
                    ui.label(RichText::new(empty_text(&app.view)).color(p().text_2));
                    if matches!(app.view, View::ForYou | View::Intake | View::Ktlo) {
                        ui.add_space(4.0);
                        let teams = app.settings.teams.join(", ");
                        ui.label(label(&format!("watching {teams} · Settings (⌘,) changes it")));
                    }
                });
            }
        });
    });
    mark(app, ui, "list", panel.response.rect);
}

/// The line an empty list shows: what would be here, plainly.
fn empty_text(view: &View) -> String {
    match view {
        View::ForYou => {
            "Nothing waits on you. Reviews, calls to make and tickets to verify land here.".into()
        }
        View::Intake => {
            "Triage is clear. New tickets on the teams you watch land here first.".into()
        }
        View::Ktlo => format!(
            "No {} work open. Security tickets, Dependabot PRs and alert sweeps land here.",
            profile_ktlo()
        ),
        View::Customer(_) => "No tickets for this customer.".into(),
        _ => "Nothing here.".into(),
    }
}

/// What the profile calls the maintenance lane: "Maintenance", or the
/// company's own word for it.
fn profile_ktlo() -> String {
    crate::profile::get().ktlo.label.clone()
}

fn header(app: &App, cards: &[&Card], ui: &mut Ui, actions: &mut Vec<Action>) {
    let shipped = cards
        .iter()
        .filter(|c| matches!(c.stage, Stage::Testing | Stage::Done))
        .count();
    let (title, subtitle) = match &app.view {
        View::ForYou => {
            let n = |ask| cards.iter().filter(|c| c.ask == Some(ask)).count();
            let yours = cards.iter().filter(|c| c.ask.is_none()).count();
            (
                "For you".to_owned(),
                format!(
                    "{} to review · {} to decide · {} to verify · {yours} assigned to you",
                    n(Ask::Review),
                    n(Ask::Decide),
                    n(Ask::Verify)
                ),
            )
        }
        View::Customer(name) => (
            name.clone(),
            format!("{shipped} of {} shipped", cards.len()),
        ),
        View::Intake => (
            "Intake".to_owned(),
            "Triage: nobody has looked at these yet".to_owned(),
        ),
        View::Ktlo => (
            crate::profile::get().ktlo.label.clone(),
            // Only a profile's bot owns them; a bot guessed from the board may not.
            match crate::profile::get().bot_name("") {
                "" => "Security, dependencies and alerts, kept out of product work".to_owned(),
                bot => format!(
                    "Security, dependencies and alerts, kept out of product work · {bot} owns them"
                ),
            },
        ),
        View::Sessions
        | View::Customers
        | View::Settings
        | View::Inbox
        | View::Alerts
        | View::Repos
        | View::Repo(_)
        | View::Stats => (String::new(), String::new()),
    };
    title_bar(ui, &title, &subtitle, bar::hints(&app.view));
    // For you says the one thing that matters right now, when Claude can.
    if app.view == View::ForYou && (app.has_anthropic_key() || app.brief_error.is_some()) {
        let rect = pad(ui, |ui| brief(app, ui, actions));
        mark(app, ui, "brief", rect);
        hairline(ui);
    }
    // The one line on what is going on, when Claude has written it.
    let digest = match &app.view {
        View::Customer(name) => app.digest(&format!("customer:{name}")),
        _ => None,
    };
    if digest.is_none() && !matches!(app.view, View::Customer(_)) {
        return;
    }
    pad(ui, |ui| {
        if let Some(line) = digest {
            ui.add(egui::Label::new(RichText::new(format!("✳ {line}")).color(p().text)).wrap());
        }
        if matches!(app.view, View::Customer(_)) {
            progress(ui, shipped, cards.len(), 3.0);
        }
    });
    hairline(ui);
}

fn ticket_card(
    ui: &mut Ui,
    card: &Card,
    title: &str,
    selected: bool,
    session: Option<crate::claude::Agent>,
    in_pane: bool,
    can_read: bool,
) -> (egui::Response, bool) {
    let mut read = false;
    let response = row(ui, selected, p().signal, true, |ui| {
        ui.horizontal(|ui| {
            status(ui, card.stage);
            ui.label(label(&card.issue.identifier));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if can_read {
                    read = link(ui, label("×"))
                        .on_hover_text("Mark read: off For you until it changes (x)")
                        .clicked();
                }
                ui.label(label(&story::ago(&card.issue.updated_at)));
            });
        });
        let shown =
            ui.add(egui::Label::new(RichText::new(title).family(medium()).color(p().text)).wrap());
        // The full title on hover only where the card shows a short name:
        // a tooltip repeating the title it is over just covers the chips.
        if let Some(full) = full_title_on_hover(title, &card.issue.title) {
            shown.on_hover_text(full);
        }
        if !card.why.is_empty() {
            let color = if card.ci == Ci::Red {
                p().danger
            } else {
                p().text_2
            };
            let why = RichText::new(&card.why).monospace().size(11.0).color(color);
            ui.add(egui::Label::new(why).wrap());
        }
        ui.horizontal_wrapped(|ui| {
            if let Some(agent) = session {
                chip(ui, &format!("✳ {}", agent.short()));
            }
            if in_pane {
                chip(ui, "◧ in a pane");
            }
            // Who it is for says more than where it was filed.
            match card.customers.first() {
                Some(customer) => chip(ui, customer),
                None => chip(ui, &card.origin.source),
            };
            if card.story_key != "loose" {
                chip(ui, &card.story);
            }
        });
    });
    (response, read)
}

/// The full title to show on hover over a card's title: only when the card
/// shows a short name, never the title it is already over.
fn full_title_on_hover<'a>(shown: &str, full: &'a str) -> Option<&'a str> {
    (shown != full).then_some(full)
}

/// Every customer with tickets on the board, one card each: the name,
/// what needs you, and Claude's line on what is going on for them.
fn customers(app: &App, cursor: Option<&Entry>, ui: &mut Ui, actions: &mut Vec<Action>) {
    let customers = app.customer_list();
    let panel = ui.scope(|ui| {
        title_bar(
            ui,
            "Customers",
            "What is going on, per customer",
            bar::hints(&app.view),
        );
        if !app.has_anthropic_key() {
            pad(ui, |ui| {
                ui.label(label(
                    "Add an Anthropic key to find customers in tickets and summarise them.",
                ));
            });
            hairline(ui);
        }
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                for (name, asks, moving, shipped) in &customers {
                    let selected = cursor == Some(&Entry::Customer(name.clone()));
                    let response = row(ui, selected, p().signal, true, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(name).family(medium()).color(p().text));
                            if *asks > 0 {
                                ui.label(
                                    RichText::new(format!("{asks} need you"))
                                        .monospace()
                                        .size(11.0)
                                        .color(p().signal),
                                );
                            }
                        });
                        let line = app
                            .digest(&format!("customer:{name}"))
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("{moving} moving · {shipped} shipped"));
                        ui.add(
                            egui::Label::new(RichText::new(line).small().color(p().text_2)).wrap(),
                        );
                    });
                    if selected {
                        bar::follow(ui, &response, name);
                    }
                    if response.clicked() {
                        actions.push(Action::Show(View::Customer(name.clone())));
                    }
                }
                if customers.is_empty() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("No customers on these tickets yet.").color(p().text_2),
                        );
                    });
                }
            });
    });
    mark(app, ui, "list", panel.response.rect);
}

/// What is not set up yet, at the top of For you, one line each with the
/// button that does it: the Anthropic key, and optionally a profile.
fn setup(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    if app.onboarding.is_some() {
        let rect = pad(ui, |ui| {
            ui.label(label("// getting started"));
            ui.add(
                egui::Label::new(
                    "Claude is working on your profile on the right: answer its questions. \
                     When it saves the profile, the session closes and Settings opens for \
                     your keys.",
                )
                .wrap(),
            );
        });
        mark(app, ui, "onboarding", rect);
        hairline(ui);
        return;
    }
    let new = app.new_skills();
    if !new.is_empty() {
        let rect = pad(ui, |ui| {
            ui.label(label("// new skills"));
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(new.join(", ")).family(medium()));
                ui.label(label("came after your profile was set up"));
                if hover_hand(&ui.button("✳ Update the profile"))
                    .on_hover_text(
                        "Claude asks only about the new skills: use the app's, or one of \
                         your own. Saving closes the session.",
                    )
                    .clicked()
                {
                    actions.push(Action::SetUpProfile);
                }
            });
        });
        mark(app, ui, "onboarding", rect);
        hairline(ui);
    }
    let key = !app.has_anthropic_key();
    let profile = crate::profile::effective(&app.settings.profile).is_empty();
    if !key && !profile {
        return;
    }
    pad(ui, |ui| {
        ui.label(label("// set up"));
        if key {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("An Anthropic key").family(medium()));
                ui.label(label("turns on briefs, New ticket's reading, test plans"));
                if hover_hand(&ui.button("Add it")).clicked() {
                    actions.push(Action::Show(View::Settings));
                }
            });
        }
        if profile {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("A profile (optional)").family(medium()));
                ui.label(label(
                    "teaches Orbit your team's bot, routing and conventions",
                ));
                if hover_hand(&ui.button("Set up with Claude")).clicked() {
                    actions.push(Action::SetUpProfile);
                }
            });
        }
    });
    hairline(ui);
}

/// "Right now": Claude's one line on the board. Click it to go there.
fn brief(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    // Without a key the set-up line under it says so, once.
    if !app.has_anthropic_key() {
        if let Some(error) = &app.brief_error {
            ui.add(egui::Label::new(RichText::new(error).small().color(p().danger)).wrap());
        }
        return;
    }
    let (text, color) = match (&app.brief, &app.brief_error) {
        (Some(line), _) => (format!("✳ {line}"), p().text),
        (None, Some(error)) => (error.clone(), p().danger),
        (None, None) => ("✳ Reading the board…".to_owned(), p().text_3),
    };
    let response = clickable(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
    });
    let response = hover_hand(&response).on_hover_text("Go to it");
    if response.clicked()
        && let Some(id) = app.brief.as_deref().and_then(|l| l.split(':').next())
    {
        actions.push(Action::Focus(id.trim().to_owned()));
    }
}

/// Linear's and GitHub's notifications in one list, only what asks
/// something (src/inbox.rs): grouped by what to do, one row per ticket or
/// PR, and one line for what was left out.
fn inbox(app: &App, cursor: Option<&Entry>, ui: &mut Ui, actions: &mut Vec<Action>) {
    let (items, old) = app.inbox();
    let panel = ui.scope(|ui| {
        title_bar(
            ui,
            "Notifications",
            "Linear and GitHub together, only what asks something of you",
            bar::hints(&app.view),
        );
        if !items.is_empty() {
            let rect = pad(ui, |ui| {
                let button = hover_hand(&ui.button("✳ Handle all my notifications")).on_hover_text(
                    "One Claude session gets every notification here with its link: it says \
                     what each asks, handles what it can, and asks you before posting, \
                     approving or merging anything",
                );
                if button.clicked() {
                    actions.push(Action::HandleInbox);
                }
            });
            mark(app, ui, "handle-inbox", rect);
        }
        if let Some(error) = &app.inbox_error {
            pad(ui, |ui| {
                ui.add(egui::Label::new(label(error).color(p().danger)).wrap());
            });
        }
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let mut group = None;
            for item in &items {
                if group != Some(item.ask) {
                    group = Some(item.ask);
                    let n = items.iter().filter(|i| i.ask == item.ask).count();
                    group_label(ui, &format!("// {}  {n}", item.ask.verb()));
                }
                let selected = cursor == Some(&Entry::Note(item.key.clone()));
                let ticket = item
                    .issue
                    .as_ref()
                    .and_then(|id| app.cards.iter().find(|c| &c.issue.id == id));
                let response = row(ui, selected, p().signal, true, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(card) = ticket {
                            ui.label(label(&card.issue.identifier));
                        }
                        ui.add(
                            egui::Label::new(RichText::new(&item.title).family(medium()).color(p().text))
                                .truncate(),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(label(&story::ago(&item.at)));
                            let mut from: Vec<&str> = item
                                .notes
                                .iter()
                                .map(|n| match n.source {
                                    crate::inbox::Source::Linear => "linear",
                                    crate::inbox::Source::GitHub => "github",
                                })
                                .collect();
                            from.dedup();
                            ui.label(label(&from.join("+")));
                        });
                    });
                    ui.add(egui::Label::new(RichText::new(&item.why).small().color(p().text_2)).truncate());
                });
                if selected {
                    bar::follow(ui, &response, &item.key);
                }
                if response.clicked() {
                    actions.push(Action::InboxOpen(item.key.clone()));
                }
            }
            if items.is_empty() {
                pad(ui, |ui| {
                    ui.label(label(if app.inbox_answered { "nothing asks for you" } else { "asking Linear and GitHub…" }));
                });
            }
            let left = app.inbox_skipped + old;
            if left > 0 {
                pad(ui, |ui| {
                    let text = format!(
                        "// {left} left out: status changes, CI, subscriptions, team requests, older than two weeks. Still unread there."
                    );
                    if link(ui, label(&text)).on_hover_text("GitHub's notifications").clicked() {
                        actions.push(Action::Open("https://github.com/notifications".into()));
                    }
                });
            }
        });
    });
    mark(app, ui, "notifications", panel.response.rect);
}

/// incident.io's live incidents, on top of the KTLO view: each with a way
/// to investigate it, and links to the incident and its Slack channel.
/// Without a key, one line on where to connect it.
/// The Alerts view: incident.io's alerts of the last day, firing first. A
/// click opens one: what it says, its attributes, and the two ways to start
/// on it, the profile's debug skill or an investigation with its panes.
fn alerts(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let firing = app.alerts.iter().filter(|a| a.firing()).count();
    let subtitle = format!("{firing} firing · {} in the last day", app.alerts.len());
    title_bar(ui, "Alerts", &subtitle, bar::hints(&app.view));
    // Two tabs: incident.io's alerts, and the deploy workflows (CI) behind them.
    let tab = egui::Id::new("alerts-tab");
    let deploys: bool = ui.data(|d| d.get_temp(tab)).unwrap_or(false);
    let red = app
        .releases
        .iter()
        .filter(|(_, run)| matches!(run, Ok(Some(run)) if run.failed()))
        .count();
    if !app.settings.releases.is_empty() {
        pad(ui, |ui| {
            ui.horizontal(|ui| {
                let name = |on: bool, text: String| {
                    let text = RichText::new(text).family(medium());
                    if on {
                        text.color(p().text).underline()
                    } else {
                        text.color(p().text_2)
                    }
                };
                if link(ui, name(!deploys, format!("alerts  {firing}"))).clicked() {
                    ui.data_mut(|d| d.insert_temp(tab, false));
                }
                ui.add_space(14.0);
                let mut deploy_text = name(deploys, format!("deploys  {red} red"));
                if red > 0 && !deploys {
                    deploy_text = deploy_text.color(p().danger);
                }
                let response = link(ui, deploy_text);
                mark(app, ui, "deploys-tab", response.rect);
                if response.clicked() {
                    ui.data_mut(|d| d.insert_temp(tab, true));
                }
            });
        });
        hairline(ui);
    }
    if deploys {
        let rect = egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.scope(|ui| releases(app, ui, actions)).response.rect
            })
            .inner;
        mark(app, ui, "releases", rect);
        return;
    }
    let rect = ui
        .scope(|ui| {
            if app.incident_key.is_none() {
                pad(ui, |ui| {
                    if link(
                        ui,
                        label("connect incident.io in settings to see its alerts here"),
                    )
                    .clicked()
                    {
                        actions.push(Action::Show(View::Settings));
                    }
                });
                return;
            }
            if let Some(error) = &app.alerts_error {
                pad(ui, |ui| {
                    ui.add(egui::Label::new(label(error).color(p().danger)).wrap());
                });
            }
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    if app.alerts.is_empty() && app.alerts_error.is_none() {
                        ui.add_space(24.0);
                        pad(ui, |ui| {
                            ui.label(RichText::new("No alerts in the last day.").color(p().text_2));
                        });
                    }
                    let id = egui::Id::new("alert-open");
                    let open: Option<String> = ui.data(|d| d.get_temp(id));
                    for alert in &app.alerts {
                        let is_open = open.as_deref() == Some(alert.id.as_str());
                        let color = if alert.firing() {
                            p().danger
                        } else {
                            p().text_3
                        };
                        let response = row(ui, is_open, color, true, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(label(&format!("■ {}", alert.status)).color(color));
                                // When it fired, on the user's clock, and how long it lasted.
                                let mut at = format!("fired {}", story::when(&alert.created_at));
                                if !alert.resolved_at.is_empty() {
                                    at.push_str(&format!(
                                        " · resolved {} ({})",
                                        story::when(&alert.resolved_at),
                                        story::took(&alert.created_at, &alert.resolved_at)
                                    ));
                                }
                                ui.label(label(&at));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(label(&story::ago(&alert.created_at)));
                                });
                            });
                            let title = RichText::new(&alert.title).family(medium());
                            let title = if alert.firing() {
                                title
                            } else {
                                title.color(p().text_2)
                            };
                            ui.add(egui::Label::new(title).wrap());
                            if !is_open {
                                let attrs: Vec<&str> = alert
                                    .attributes
                                    .iter()
                                    .take(3)
                                    .map(|(_, v)| v.as_str())
                                    .collect();
                                if !attrs.is_empty() {
                                    ui.label(label(&attrs.join(" · ")));
                                }
                                return;
                            }
                            alert_details(alert, ui, actions);
                        });
                        if response.clicked() {
                            let next = (!is_open).then(|| alert.id.clone());
                            ui.data_mut(|d| match next {
                                Some(next) => {
                                    d.insert_temp(id, next);
                                }
                                None => d.remove::<String>(id),
                            });
                        }
                    }
                });
        })
        .response
        .rect;
    mark(app, ui, "alerts", rect);
}

/// Releases to prod (settings.json `releases`): each workflow's latest run,
/// green, red or running; a red one has Fix it.
/// The deploys by their latest run, newest on top; one not read yet (or
/// never run) last, in settings order. RFC 3339 times sort as text.
fn newest_first<'a, T>(all: &'a [T], started: impl Fn(&'a T) -> &'a str) -> Vec<&'a T> {
    let mut sorted: Vec<&T> = all.iter().collect();
    sorted.sort_by(|a, b| started(b).cmp(started(a)));
    sorted
}

fn releases(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    group_label(ui, &format!("// deploys  {}", app.settings.releases.len()));
    let run_of = |release| {
        app.releases
            .iter()
            .find(|(r, _)| r == release)
            .map(|(_, run)| run)
    };
    let started = |release| match run_of(release) {
        Some(Ok(Some(run))) => run.created_at.as_str(),
        _ => "",
    };
    for release in newest_first(&app.settings.releases, started) {
        let run = run_of(release);
        pad(ui, |ui| {
            ui.horizontal(|ui| {
                let (state, color) = match run {
                    None => ("checking".to_owned(), p().text_3),
                    Some(Err(_)) => ("unreadable".to_owned(), p().danger),
                    Some(Ok(None)) => ("no runs".to_owned(), p().text_3),
                    Some(Ok(Some(run))) if run.failed() => ("■ failed".to_owned(), p().danger),
                    Some(Ok(Some(run))) if run.running() => ("■ running".to_owned(), p().signal),
                    Some(Ok(Some(run))) if run.conclusion == "success" => {
                        ("■ success".to_owned(), p().success)
                    }
                    // Cancelled, neutral: neither green nor red.
                    Some(Ok(Some(run))) => (format!("■ {}", run.conclusion), p().text_3),
                };
                ui.label(label(&state).color(color));
                ui.label(RichText::new(release.label()).family(medium()));
                ui.label(label(&release.repo));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(Ok(Some(run))) = run {
                        if ui.button("Run ↗").clicked() {
                            actions.push(Action::Open(run.url.clone()));
                        }
                        if run.failed() {
                            let fix = primary(ui, "✳ Fix it").on_hover_text(
                                "A Claude session on this failed run (fix-deploy): finds the cause and whose change it was, then opens a fix PR and pings them, or a revert PR assigned to them. Merges nothing.",
                            );
                            if fix.clicked() {
                                actions.push(Action::FixRelease(run.id));
                            }
                        }
                        ui.label(label(&story::ago(&run.created_at)));
                    }
                });
            });
            match run {
                Some(Ok(Some(run))) => {
                    ui.horizontal(|ui| {
                        // Who to talk to, beside a red run.
                        if run.failed() {
                            // When it failed, on the user's clock.
                            let at = if run.updated_at.is_empty() {
                                &run.created_at
                            } else {
                                &run.updated_at
                            };
                            ui.label(
                                label(&format!("failed {}", story::when(at))).color(p().danger),
                            );
                            if let Some(by) = run.by() {
                                ui.label(label(&format!("· by {by}")).color(p().danger));
                            }
                        }
                        ui.add(egui::Label::new(label(&run.title).color(p().text_2)).truncate());
                    });
                }
                Some(Err(error)) => {
                    ui.add(egui::Label::new(label(error).color(p().danger)).wrap());
                }
                _ => {}
            }
        });
        hairline(ui);
    }
}

/// An open alert: its description, every attribute, when it resolved, and
/// the quick actions.
fn alert_details(alert: &crate::incidents::Alert, ui: &mut Ui, actions: &mut Vec<Action>) {
    ui.add_space(6.0);
    if !alert.description.trim().is_empty() {
        markdown(ui, &tidy_links(&alert.description));
        ui.add_space(6.0);
    }
    for (name, value) in &alert.attributes {
        ui.horizontal_wrapped(|ui| {
            ui.label(label(name));
            ui.label(
                RichText::new(value)
                    .monospace()
                    .size(11.0)
                    .color(p().text_2),
            );
        });
    }
    if !alert.resolved_at.is_empty() {
        ui.label(label(&format!(
            "resolved {} ago",
            story::ago(&alert.resolved_at)
        )));
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        // The profile's alert skill (`skills.debugAlert`), when it names one.
        if let Some(skill) = crate::profile::get().skills.get("debugAlert") {
            let debug = primary(ui, &format!("✳ Debug with /{skill}")).on_hover_text(
                "A Claude session that runs this skill on the alert, in a window of its own",
            );
            if debug.clicked() {
                actions.push(Action::InvestigateAlert {
                    id: alert.id.clone(),
                    debug: true,
                });
            }
        }
        let investigate = hover_hand(&ui.button("✳ Investigate")).on_hover_text(
            "A Claude session with this alert as its brief, the settings.json investigate panes beside it",
        );
        if investigate.clicked() {
            actions.push(Action::InvestigateAlert { id: alert.id.clone(), debug: false });
        }
        if !alert.source_url.is_empty() && ui.button("Source ↗").clicked() {
            actions.push(Action::Open(alert.source_url.clone()));
        }
    });
}

/// Long links (an alert's SigNoz query is hundreds of characters) as a
/// markdown link named by its host: `<https://host/…>` or a bare one.
fn tidy_links(text: &str) -> String {
    text.lines()
        .map(|line| {
            line.split(' ')
                .map(|word| {
                    let url = word.trim_matches(|c| c == '<' || c == '>');
                    if url.starts_with("http") && !word.contains("](") {
                        let host = url.split('/').nth(2).unwrap_or(url);
                        format!("[{host} ↗]({url})")
                    } else {
                        word.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// KTLO's batch review: every open KTLO PR that waits on the user's
/// approval, one row each with its Diff and Approve, then Land approved for
/// the ones approved. Merges stay the user's click (Land approved is it),
/// each bound to the head shown.
fn batch(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let prs: Vec<(&Card, &story::Pr, &crate::github::Gate)> = app
        .ktlo()
        .flat_map(|c| c.prs.iter().map(move |p| (c, p)))
        .filter(|(_, p)| p.open && !app.merged.contains_key(&p.url))
        .filter_map(|(c, p)| match app.gates.get(&p.url) {
            Some(Ok(gate)) if gate.state == "OPEN" => Some((c, p, gate)),
            _ => None,
        })
        .collect();
    let to_approve: Vec<_> = prs.iter().filter(|(_, _, g)| g.can_approve).collect();
    let approved: Vec<_> = prs
        .iter()
        .filter(|(_, p, g)| g.review == "APPROVED" && !g.queued && !app.merging.contains(&p.url))
        .collect();
    landing(app, ui, actions);
    if to_approve.is_empty() && approved.is_empty() {
        return;
    }
    group_label(
        ui,
        &format!(
            "// batch review  {} to approve · {} approved",
            to_approve.len(),
            approved.len()
        ),
    );
    // Diff all: every PR's diff under its row, to read top to bottom.
    if !to_approve.is_empty() {
        pad(ui, |ui| {
            let (text, hover) = if app.diff_all {
                ("Hide diffs", "Back to one line per PR")
            } else {
                (
                    "Diff all",
                    "Every PR's diff under its row, to read and approve top to bottom",
                )
            };
            if ui.button(text).on_hover_text(hover).clicked() {
                actions.push(Action::DiffAll(!app.diff_all));
            }
        });
    }
    for (card, pr, gate) in &to_approve {
        pad(ui, |ui| {
            ui.horizontal(|ui| {
                let title = batch_title(&pr.repo, &card.issue.title);
                ui.add(egui::Label::new(RichText::new(title).family(medium())).truncate());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    super::ticket::approve(app, pr, None, ui, actions);
                    if ui
                        .button("Diff")
                        .on_hover_text("Opens its diff in a pane")
                        .clicked()
                    {
                        actions.push(Action::Link(pr.url.clone()));
                    }
                });
            });
            let checks = match gate.checks.as_str() {
                "SUCCESS" => ("checks green", p().success),
                "FAILURE" | "ERROR" => ("checks red", p().danger),
                "" => ("no checks", p().text_3),
                _ => ("checks running", p().text_3),
            };
            ui.horizontal(|ui| {
                ui.label(label(&format!("#{}", pr.number)));
                ui.label(label(checks.0).color(checks.1));
                if !gate.on_it.is_empty() {
                    ui.label(label(&format!("· on it: {}", gate.on_it.join(", "))).color(p().text));
                }
            });
            if app.diff_all {
                ui.add_space(6.0);
                inline_diff(app, &pr.url, ui);
            }
        });
        hairline(ui);
    }
    if !approved.is_empty() {
        // The same plan the click runs, so the count is what it lands.
        let plans: Vec<(String, String, crate::app::Land)> = approved
            .iter()
            .map(|(_, p, g)| {
                (
                    p.url.clone(),
                    g.head.clone(),
                    app.land_plan(&p.url, &g.head),
                )
            })
            .collect();
        let land: Vec<(String, String)> = plans
            .iter()
            .filter(|(_, _, plan)| plan.acts())
            .map(|(url, head, _)| (url.clone(), head.clone()))
            .collect();
        let on_way = plans
            .iter()
            .filter(|(_, _, plan)| *plan == crate::app::Land::Merging)
            .count();
        let held: Vec<&str> = plans
            .iter()
            .filter_map(|(_, _, plan)| match plan {
                crate::app::Land::Skip(why) => Some(why.as_str()),
                _ => None,
            })
            .collect();
        pad(ui, |ui| {
            ui.horizontal(|ui| {
                if !land.is_empty() {
                    let button = primary(ui, &format!("Land approved ({})", land.len()))
                        .on_hover_text(
                            "Merges the green ones now, sets Merge when ready where checks \
                             still run, and starts one session that makes the rest mergeable.",
                        );
                    if button.clicked() {
                        actions.push(Action::LandBatch(land.clone()));
                    }
                }
                if on_way > 0 {
                    ui.label(label(&format!("{on_way} on their way")).color(p().text_3));
                }
                if !held.is_empty() {
                    ui.label(
                        label(&format!("{} held: {}", held.len(), held.join(", ")))
                            .color(p().text_3),
                    );
                }
            });
        });
        hairline(ui);
    }
}

/// The landing sessions Land approved started, newest first, the last
/// three: what each is doing, and Open to watch it.
fn landing(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let mut sessions: Vec<(&String, &crate::claude::Saved)> = app
        .claude
        .saved
        .iter()
        // Older ones were saved as plain sessions, named by their title.
        .filter(|(_, s)| {
            s.identifier == crate::claude::LANDING
                || (s.title.starts_with("Land ")
                    && s.title.ends_with(&format!("{} PRs", profile_ktlo())))
        })
        .collect();
    if sessions.is_empty() {
        return;
    }
    sessions.sort_by(|a, b| b.1.started_at.cmp(&a.1.started_at));
    group_label(ui, "// landing");
    for (key, saved) in sessions.into_iter().take(3) {
        pad(ui, |ui| {
            ui.horizontal(|ui| {
                let state = app.claude.row_of(key).map_or("stopped", |r| r.label());
                let color = match state {
                    "working" => p().success,
                    "your turn" | "needs approval" => p().signal,
                    _ => p().text_3,
                };
                if state == "working" {
                    ui.add(egui::Spinner::new().size(11.0).color(p().success));
                } else {
                    ui.label(label(&format!("■ {state}")).color(color));
                }
                ui.label(RichText::new(&saved.title).family(medium()));
                ui.label(label(&story::ago(&saved.started_at)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .button("Open")
                        .on_hover_text("Shows the session in its window")
                        .clicked()
                    {
                        actions.push(Action::OpenSession(key.clone()));
                    }
                });
            });
        });
        hairline(ui);
    }
}

/// A batch review row's title: the repo, then the PR's (or its ticket's) title.
fn batch_title(repo: &str, title: &str) -> String {
    format!("{repo} - {title}")
}

/// A PR's diff, compact and unified: each file's path, then its lines in
/// their colours, the first 80 of the PR.
fn inline_diff(app: &App, url: &str, ui: &mut Ui) {
    use crate::github::Line;
    let files = match app.diffs.get(url) {
        None => {
            ui.label(label("loading the diff…"));
            return;
        }
        Some(Err(error)) => {
            ui.label(label(error).color(p().danger));
            return;
        }
        Some(Ok(files)) => files,
    };
    let mut shown = 0;
    for file in files {
        ui.label(
            label(&format!("{}  +{} −{}", file.path, file.added, file.removed)).color(p().text_2),
        );
        for line in &file.lines {
            if shown >= 80 {
                ui.label(label("… the rest on Diff"));
                return;
            }
            shown += 1;
            let (text, color, fill) = match line {
                Line::Hunk(t) => (t.clone(), p().text_3, Color32::TRANSPARENT),
                Line::Added(t) => (format!("+ {t}"), p().diff_text, p().diff_bg),
                Line::Removed(t) => (format!("− {t}"), p().text, p().signal_line),
                Line::Context(t) => (format!("  {t}"), p().text_3, Color32::TRANSPARENT),
            };
            Frame::new().fill(fill).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(
                    egui::Label::new(RichText::new(text).monospace().size(11.0).color(color))
                        .truncate(),
                );
            });
        }
    }
}

fn incidents(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    if app.incident_key.is_none() {
        pad(ui, |ui| {
            if link(
                ui,
                label("// incidents: connect incident.io in settings to see them here"),
            )
            .clicked()
            {
                actions.push(Action::Show(View::Settings));
            }
        });
        hairline(ui);
        return;
    }
    let rect = ui
        .scope(|ui| {
            group_label(ui, &format!("// incidents  {}", app.incidents.len()));
            if let Some(error) = &app.incidents_error {
                pad(ui, |ui| {
                    ui.add(egui::Label::new(label(error).color(p().danger)).wrap());
                });
            } else if app.incidents.is_empty() {
                pad(ui, |ui| {
                    ui.label(label("no live incidents"));
                });
            }
            for incident in &app.incidents {
                pad(ui, |ui| {
                    ui.horizontal(|ui| {
                        let sev = format!("■ {}", incident.severity.to_lowercase());
                        ui.label(label(&sev).color(p().danger));
                        ui.label(label(&incident.reference));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(label(&story::ago(&incident.created_at)));
                        });
                    });
                    ui.add(egui::Label::new(RichText::new(&incident.name).family(medium())).wrap());
                    let lead = incident.lead.as_deref().unwrap_or("no lead");
                    ui.label(label(&format!("{} · {lead}", incident.status.to_lowercase())));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if primary(ui, "✳ Investigate")
                            .on_hover_text("A Claude session with this incident as its brief, the settings.json investigate panes beside it")
                            .clicked()
                        {
                            actions.push(Action::Investigate(incident.id.clone()));
                        }
                        if ui.button("incident.io ↗").clicked() {
                            actions.push(Action::Open(incident.permalink.clone()));
                        }
                        if let Some(slack) = &incident.slack
                            && ui.button("Slack ↗").clicked()
                        {
                            actions.push(Action::Open(slack.clone()));
                        }
                    });
                });
                hairline(ui);
            }
        })
        .response
        .rect;
    mark(app, ui, "incidents", rect);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_deploy_is_on_top() {
        let runs = [
            ("web ui", "2026-10-13T11:00:00Z"),
            ("unread", ""),
            ("operator", "2026-10-13T16:29:00Z"),
            ("workers", "2026-10-13T16:23:00Z"),
        ];
        let order: Vec<&str> = newest_first(&runs, |r| r.1).iter().map(|r| r.0).collect();
        assert_eq!(order, ["operator", "workers", "web ui", "unread"]);
    }

    #[test]
    fn a_card_title_shows_the_full_title_on_hover_only_when_shortened() {
        let full = "Reduce report export memory use";
        assert_eq!(full_title_on_hover(full, full), None);
        assert_eq!(full_title_on_hover("SigNoz ingest", full), Some(full));
    }

    #[test]
    fn a_repo_page_search_finds_by_number_title_author_and_state_words() {
        let pr = |n: u64, author: &str, draft: bool, review: &str, checks: &str, asked: bool| {
            crate::github::RepoPr {
                url: format!("https://github.com/o/r/pull/{n}"),
                number: n,
                title: format!("change {n}"),
                author: author.into(),
                name: if author == "@carl" { "Carla Rossi" } else { "" }.into(),
                at: String::new(),
                draft,
                review: review.into(),
                checks: checks.into(),
                review_requested: asked,
            }
        };
        let prs = vec![
            pr(1, "@bob", true, "", "red", false),
            pr(2, "@me", false, "APPROVED", "green", false),
            pr(3, "@carl", false, "", "pending", true),
        ];
        let mine = vec![crate::github::MyPr {
            url: prs[1].url.clone(),
            repo: "r".into(),
            number: 2,
            title: String::new(),
            at: String::new(),
            draft: false,
        }];
        let numbers = |q: &str| -> Vec<u64> {
            repo_order(&prs, &mine, q)
                .iter()
                .map(|p| p.number)
                .collect()
        };
        // To review, yours, the rest.
        assert_eq!(numbers(""), [3, 2, 1]);
        assert_eq!(numbers("#2"), [2]);
        assert_eq!(numbers("change 1"), [1]);
        assert_eq!(numbers("bob"), [1]);
        assert_eq!(
            numbers("carla"),
            [3],
            "by the author's name, not only the login"
        );
        assert_eq!(numbers("draft"), [1]);
        assert_eq!(numbers("approved"), [2]);
        assert_eq!(numbers("red"), [1]);
        assert_eq!(numbers("green"), [2]);
        assert_eq!(numbers("yours"), [2]);
        assert_eq!(numbers("review"), [3]);
    }

    #[test]
    fn a_batch_row_names_the_repo_then_the_pr() {
        assert_eq!(
            batch_title("api", "ci: bump azure/setup-helm from 4 to 5"),
            "api - ci: bump azure/setup-helm from 4 to 5"
        );
    }

    #[test]
    fn a_long_link_reads_as_its_host() {
        assert_eq!(
            tidy_links("Traces: <https://signoz.example/traces?q=%257B>\nok"),
            "Traces: [signoz.example ↗](https://signoz.example/traces?q=%257B)\nok"
        );
        assert_eq!(tidy_links("[a](https://x)"), "[a](https://x)");
    }

    #[test]
    fn an_empty_view_says_what_would_be_there() {
        assert!(empty_text(&View::Intake).starts_with("Triage is clear"));
        assert!(empty_text(&View::ForYou).contains("waits on you"));
    }
}

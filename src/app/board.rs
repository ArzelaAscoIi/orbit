//! The optimistic layer: what the user changed, laid over Linear's last
//! answer until an answer fetched after the change arrives.

use std::collections::{HashMap, HashSet};

use super::{App, View};
use crate::claude::status;
use crate::linear::{Comment, Issue, State};
use crate::story::{self, Card};

/// A state the interface shows ahead of Linear.
#[derive(Debug)]
pub struct Shown {
    pub state_id: String,
    /// Moved to this team (by id) too; the state is one of its states.
    pub team_id: Option<String>,
    /// `None` while the change is in flight. Once Linear accepts it, the id
    /// of the first refresh fetched after that; older answers are stale.
    pub confirmed_by: Option<u64>,
}

/// A comment the interface shows ahead of Linear, held the same way.
pub struct Local {
    pub issue_id: String,
    pub comment: Comment,
    pub confirmed_by: Option<u64>,
}

impl App {
    /// Cards are Linear's last answer with everything the user is ahead of
    /// it on laid over the top.
    pub(super) fn rebuild(&mut self) {
        let mut issues = self.server.clone();
        for issue in &self.fetched {
            if !issues.iter().any(|i| i.id == issue.id) {
                issues.push(issue.clone());
            }
        }
        overlay(&mut issues, &self.shown, &self.states);
        for local in &self.locals {
            if let Some(issue) = issues.iter_mut().find(|i| i.id == local.issue_id)
                && !issue
                    .comments
                    .nodes
                    .iter()
                    .any(|c| c.id == local.comment.id)
            {
                issue.comments.nodes.push(local.comment.clone());
            }
        }
        let parents: HashSet<String> = issues
            .iter()
            .filter_map(|i| i.parent.as_ref().map(|p| p.id.clone()))
            .collect();
        self.cards = issues
            .into_iter()
            .map(|issue| story::card(issue, &parents))
            .collect();
        // Tickets without a Linear customer request get the customers
        // Claude read from them.
        for card in &mut self.cards {
            if card.customers.is_empty()
                && let Some((_, names)) = self.extracted.get(&card.issue.id)
            {
                card.customers = names
                    .iter()
                    .filter(|n| !story::is_ours(n))
                    .cloned()
                    .collect();
            }
            story::live(card, &self.gates);
            story::hand_off(card, &self.handed);
            if let Some(reports) = self.claude.status.get(&card.issue.id) {
                Self::session_overlay(card, reports);
            }
        }
    }

    /// What the ticket's own Claude session did, laid over Linear's answer
    /// the same way the user's actions are: its PR shows before Linear lists
    /// it, its question is an ask on the card, and its events join the
    /// timeline. Linear catches up behind; nothing here flickers away.
    fn session_overlay(card: &mut Card, reports: &[status::Report]) {
        use status::State;
        // Only while Linear lists none: a command's output can name any PR.
        if card.prs.is_empty()
            && let Some(report) = status::pr(reports)
            && let Some(pr) = story::pr_from_url(&report.text, &report.timestamp())
        {
            card.prs.push(pr);
        }
        if let Some(report) = status::current(reports)
            && report.recent()
            && report.state == State::NeedsDecision
        {
            card.ask = Some(story::Ask::Decide);
            card.why = format!("Claude asks: {}", report.text);
        }
        card.timeline.extend(status::events(reports));
        card.timeline
            .sort_by_cached_key(|e| e.at.parse::<jiff::Timestamp>().ok());
    }

    /// Drops holds that an answer fetched after the change has replaced.
    pub(super) fn settle(&mut self, request: u64) {
        prune(&mut self.shown, request);
        self.locals
            .retain(|l| l.confirmed_by.is_none_or(|first| request < first));
    }

    /// The cards the current view lists, in the order it lists them.
    /// Assigned to the viewer and still open: theirs to move.
    pub fn is_mine(&self, card: &Card) -> bool {
        // One the bot works is its to move, not the user's.
        !self.viewer.is_empty()
            && !card.bot_works
            && !matches!(card.stage, story::Stage::Done)
            && card
                .issue
                .assignee
                .as_ref()
                .is_some_and(|a| a.name == self.viewer)
    }

    /// For you shows it: it asks something or is yours, and was not marked
    /// read since it last changed.
    pub fn for_you_shows(&self, card: &Card) -> bool {
        (card.ask.is_some() || self.is_mine(card))
            && self.read.get(&card.issue.id) != Some(&story::read_key(card))
    }

    /// What For you lists, in its order, whatever the sheet shows: the
    /// first load and Alt+Down go by it.
    pub fn for_you(&self) -> Vec<&Card> {
        let mut cards: Vec<&Card> = self
            .cards
            .iter()
            .filter(|c| c.lane == story::Lane::Product && self.for_you_shows(c))
            .collect();
        story::sort(&mut cards);
        cards
    }

    /// Open KTLO work: the tickets off the product lane and Dependabot's PRs.
    pub fn ktlo(&self) -> impl Iterator<Item = &Card> {
        self.cards
            .iter()
            .chain(&self.pr_cards)
            .filter(|c| c.lane != story::Lane::Product && c.stage != story::Stage::Done)
    }

    pub fn visible(&self) -> Vec<&Card> {
        let mut cards: Vec<&Card> = self
            .cards
            .iter()
            .chain(
                self.pr_cards
                    .iter()
                    .filter(|c| c.lane == story::Lane::Dependencies),
            )
            // KTLO stays out of the product views and has one of its own.
            .filter(|c| (c.lane == story::Lane::Product) != (self.view == View::Ktlo))
            .filter(|c| match &self.view {
                // What asks for you, and what is yours to move: a team
                // without the bot still sees its own work here.
                View::ForYou => self.for_you_shows(c),
                View::Ktlo => c.stage != story::Stage::Done,
                View::Intake => c.untriaged(),
                View::Customer(name) => c.customers.contains(name),
                View::Sessions
                | View::Customers
                | View::Settings
                | View::Inbox
                | View::Alerts
                | View::Repos
                | View::Repo(_)
                | View::Stats => false,
            })
            .collect();
        story::sort(&mut cards);
        if let View::Customer(_) = self.view {
            let order = |c: &Card| match story::bucket(c) {
                "Needs you" => 0,
                "Moving" => 1,
                _ => 2,
            };
            cards.sort_by_key(|c| order(c));
        }
        if self.view == View::Ktlo {
            // One group per lane, security, then dependencies, then alerts;
            // in each, what nobody is on.
            let now = jiff::Timestamp::now();
            cards.sort_by_key(|c| {
                let stuck = story::unhandled(c, now);
                (
                    match c.lane {
                        story::Lane::Security => 0,
                        story::Lane::Dependencies => 1,
                        _ => 2,
                    },
                    !stuck,
                    !(stuck && story::pressing(c)),
                )
            });
        }
        if matches!(self.view, View::Intake) {
            // Keep each story's cards together, stories with work for you first.
            let mut first: HashMap<&str, usize> = HashMap::new();
            for (i, c) in cards.iter().enumerate() {
                first.entry(c.story_key.as_str()).or_insert(i);
            }
            cards.sort_by_key(|c| first[c.story_key.as_str()]);
        }
        if self.view == View::Intake {
            // Filed in the last day: on top, before the stories.
            let now = jiff::Timestamp::now();
            cards.sort_by_key(|c| !c.new_today(now));
        }
        cards
    }
}

/// Ends a hold once an answer fetched after the change has arrived.
fn prune(shown: &mut HashMap<String, Shown>, request: u64) {
    shown.retain(|_, s| s.confirmed_by.is_none_or(|first| request < first));
}

/// Shows the user's state changes wherever Linear has not caught up.
fn overlay(
    issues: &mut [Issue],
    shown: &HashMap<String, Shown>,
    states: &HashMap<String, Vec<State>>,
) {
    for issue in issues {
        let Some(s) = shown.get(&issue.id) else {
            continue;
        };
        if let Some(team) = &s.team_id {
            issue.team.id = team.clone();
        }
        if let Some(state) = states
            .get(&issue.team.id)
            .and_then(|team| team.iter().find(|st| st.id == s.state_id))
        {
            issue.state = state.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linear::{Nodes, TeamRef};

    fn state(id: &str) -> State {
        State {
            id: id.into(),
            name: id.into(),
            color: String::new(),
            position: 0.0,
            kind: "started".into(),
        }
    }

    fn issue(state_id: &str) -> Issue {
        Issue {
            id: "i".into(),
            identifier: "ABC-1".into(),
            title: "t".into(),
            url: String::new(),
            description: None,
            updated_at: String::new(),
            created_at: String::new(),
            priority: 0.0,
            previous_identifiers: vec![],
            state: state(state_id),
            team: TeamRef { id: "team".into() },
            assignee: None,
            creator: None,
            parent: None,
            project: None,
            attachments: Nodes { nodes: vec![] },
            needs: Nodes { nodes: vec![] },
            labels: Nodes { nodes: vec![] },
            children: Nodes { nodes: vec![] },
            comments: Nodes { nodes: vec![] },
        }
    }

    #[test]
    fn a_moved_ticket_shows_in_its_new_team_and_state_at_once() {
        let states = HashMap::from([
            ("team".to_owned(), vec![state("todo")]),
            ("bui".to_owned(), vec![state("bui-todo")]),
        ]);
        let shown = HashMap::from([(
            "i".to_owned(),
            Shown {
                state_id: "bui-todo".into(),
                team_id: Some("bui".into()),
                confirmed_by: None,
            },
        )]);
        let mut issues = vec![issue("todo")];
        overlay(&mut issues, &shown, &states);
        assert_eq!(issues[0].team.id, "bui");
        assert_eq!(issues[0].state.id, "bui-todo");
    }

    #[test]
    fn stale_answers_never_undo_a_state_change() {
        let states = HashMap::from([("team".to_owned(), vec![state("todo"), state("done")])]);
        let mut shown = HashMap::from([(
            "i".to_owned(),
            Shown {
                state_id: "done".into(),
                team_id: None,
                confirmed_by: None,
            },
        )]);
        let answer = |shown: &mut HashMap<String, Shown>, request| {
            let mut issues = vec![issue("todo")];
            prune(shown, request);
            overlay(&mut issues, shown, &states);
            issues[0].state.id.clone()
        };

        // In flight: any answer is stale.
        assert_eq!(answer(&mut shown, 1), "done");
        // Accepted; refresh 3 is the first fetched afterwards.
        shown.get_mut("i").unwrap().confirmed_by = Some(3);
        assert_eq!(answer(&mut shown, 2), "done");
        // Refresh 3 is the truth again, even if someone else moved it back.
        assert_eq!(answer(&mut shown, 3), "todo");
        assert!(shown.is_empty());
    }

    #[test]
    fn a_sessions_pr_and_question_show_before_linear_has_them() {
        use crate::claude::status::{Report, State};
        let now = std::time::SystemTime::now();
        let report = |state, text: &str| Report {
            at: now,
            state,
            text: text.into(),
        };
        let reports = [
            report(State::Working, "fix it"),
            report(State::Pr, "https://github.com/acme/web/pull/42"),
            report(State::NeedsDecision, "Squash or merge?"),
        ];
        let mut card = story::card(issue("todo"), &HashSet::new());
        assert!(card.prs.is_empty() && card.ask.is_none());
        App::session_overlay(&mut card, &reports);
        let pr = &card.prs[0];
        assert_eq!((pr.repo.as_str(), pr.number, pr.open), ("web", 42, true));
        assert_eq!(card.ask, Some(story::Ask::Decide));
        assert_eq!(card.why, "Claude asks: Squash or merge?");
        assert_eq!(
            card.timeline.iter().map(|e| e.verb).collect::<Vec<_>>(),
            ["opened a PR", "asks"]
        );
    }
}

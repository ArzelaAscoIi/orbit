//! Bridge between the UI thread and asynchronous work.
//!
//! egui runs on the main thread and must never block. A dedicated tokio
//! runtime runs every Linear request; the two sides talk through channels.
//! Every event wakes the interface with `request_repaint`, so the app stays
//! event-driven and idle when nothing is happening.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::linear::{Client, Comment, Home};

/// The review the user posted before, for a walkthrough of what changed
/// since and whether their comments were handled.
#[derive(Debug)]
pub struct Since {
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub token: Option<String>,
    pub base: String,
    pub head: String,
    pub comments: Vec<String>,
}

impl Since {
    pub async fn context(&self, http: &reqwest::Client) -> String {
        let changes = crate::github::compare(
            http,
            self.token.as_deref(),
            &self.owner,
            &self.repo,
            &self.base,
            &self.head,
        )
        .await
        .map_or_else(
            |error| format!("(could not load: {error})"),
            |files| crate::github::text(&files, 30_000),
        );
        // GitHub's threads carry the author's replies too; the comments
        // this app recorded stand in when they cannot be read.
        let (owner, repo, number) = (self.owner.clone(), self.repo.clone(), self.number);
        let threads =
            tokio::task::spawn_blocking(move || crate::github::threads(&owner, &repo, number))
                .await
                .ok()
                .and_then(Result::ok)
                .filter(|t| !t.is_empty());
        let comments = match threads {
            Some(threads) => threads,
            None if self.comments.is_empty() => "(none recorded)".to_owned(),
            None => self.comments.join("\n"),
        };
        format!(
            "\n\nThe reviewer reviewed this PR before, at commit {}. The review threads \
             on the PR, with the replies:\n{comments}\n\nWhat changed since that review:\n{changes}",
            &self.base[..7.min(self.base.len())]
        )
    }
}

pub enum Command {
    Refresh {
        request: u64,
        teams: Vec<String>,
    },
    SetState {
        issue_id: String,
        state_id: String,
    },
    /// Assigns the ticket to the user: a local build took it from the bot.
    TakeFromBot {
        issue_id: String,
    },
    /// The GitHub login of an org member by their name (github::login_of).
    FindLogin {
        org: String,
        name: String,
    },
    /// Asks this login to review the PR (github::request_review).
    RequestReview {
        url: String,
        login: String,
    },
    /// Puts a label on a PR (a review router's).
    AddLabel {
        url: String,
        label: String,
    },
    /// What people said on a PR (github::talk).
    Talk {
        url: String,
    },
    /// Moves the issue to another team, in that team's state; answered as
    /// a StateSet, since the hold is the same.
    MoveTeam {
        issue_id: String,
        team_id: String,
        state_id: String,
    },
    AddComment {
        issue_id: String,
        temp_id: String,
        body: String,
    },
    Brief {
        fingerprint: u64,
        key: String,
        board: String,
    },
    Shorten {
        key: String,
        items: Vec<(String, String)>,
    },
    Customers {
        key: String,
        items: Vec<(String, String)>,
    },
    Digests {
        key: String,
        items: Vec<(String, String)>,
    },
    /// `(issue id, subtitle, metadata)` for each ticket's note.
    SaveNotes(Vec<(String, String, serde_json::Value)>),
    Diff {
        url: String,
        owner: String,
        repo: String,
        number: u64,
        token: Option<String>,
    },
    /// Whether the PR may be merged now (src/github.rs).
    Gate {
        url: String,
    },
    /// Squash-merges the PR, or queues it, bound to the head the user saw green.
    Merge {
        url: String,
        head: String,
    },
    /// Merge when ready on at this head, or off (`None`).
    AutoMerge {
        url: String,
        head: Option<String>,
    },
    /// Approves the PR as the user, bound to the head the user saw, with
    /// their note as the review's comment when there is one.
    Approve {
        url: String,
        head: String,
        body: Option<String>,
    },
    /// The user's questions about a PR and Claude's answers as one short
    /// approval note (anthropic::approve_note).
    ApproveNote {
        url: String,
        key: String,
        asked: Vec<(String, String)>,
    },
    /// Closes the PR without merging.
    ClosePr {
        url: String,
    },
    /// Merges the base into the PR's branch, bound to the head the user saw.
    UpdateBranch {
        url: String,
        head: String,
    },
    /// Asks for an AI review of the user's PR (profile `github.aiReview`),
    /// noted as asked at `head`.
    AiReview {
        url: String,
        head: String,
        how: crate::profile::AiReview,
    },
    /// Posts the user's drafted comments, and their summary when written,
    /// as one review, bound to the head.
    Review {
        url: String,
        head: String,
        drafts: Vec<crate::github::Draft>,
        body: String,
        fix: Option<String>,
        changes: bool,
    },
    /// A draft PR marked ready for review.
    ReadyPr {
        url: String,
    },
    /// Laya's quick questions about a ticket's text (laya.rs `QUICK`).
    Quick {
        issue_id: String,
        hash: u64,
        text: String,
    },
    /// Laya on each session's last message (laya::asks), `(key, text)`.
    LayaAsks(Vec<(String, String)>),
    Guide {
        url: String,
        key: String,
        context: String,
        diff: String,
        /// A re-review: the earlier one to follow up on.
        since: Option<Since>,
    },
    /// The reviewer's own question about a PR (anthropic::ask_pr).
    AskPr {
        url: String,
        key: String,
        context: String,
        diff: String,
        question: String,
    },
    /// An image in a ticket's description, by URL (`Client::file`).
    File {
        url: String,
    },
    /// incident.io's live incidents, with the user's read-only key.
    Incidents {
        key: String,
    },
    /// incident.io's alerts of the last day.
    Alerts {
        key: String,
    },
    /// The latest run of each watched release workflow (releases.rs).
    Releases(Vec<crate::releases::Release>),
    /// Linear's and GitHub's notifications, both at once (src/inbox.rs).
    Inbox,
    /// Marks these notifications read: Linear ids, GitHub thread ids.
    InboxRead {
        linear: Vec<String>,
        github: Vec<String>,
    },
    /// The user's open PRs on every repo, for the palette's repo scope.
    MyPrs,
    /// Dependabot PRs on these repos or waiting on the user's review, for KTLO.
    DependabotPrs(Vec<String>),
    /// The open PRs of one repo (`owner/repo`), for its page (View::Repo).
    RepoPrs(String),
    /// Who may review on one repo (`owner/repo`): Request review's picks.
    Reviewers(String),
    /// herdr's agents (herdr.rs).
    Herdr,
    /// Brings a herdr agent forward.
    HerdrFocus(String),
    /// Starts a ticket's Claude in herdr: agent name, workspace label, the
    /// folder and the `claude` arguments.
    HerdrStart {
        name: String,
        label: String,
        cwd: std::path::PathBuf,
        args: Vec<String>,
    },
    /// Writes Orbit's plugin and links it into herdr.
    HerdrPlugin,
    /// The team's numbers from SigNoz for the Stats view (signoz.rs).
    TeamStats {
        mcp: String,
        repos: Vec<String>,
    },
    /// One ticket not on the board (by id or identifier), to open it here.
    Issue {
        id: String,
    },
    /// An `orbit ctl` write to Linear; `reply` gets ok only once Linear said yes.
    Write {
        write: crate::linear::Write,
        reply: std::sync::mpsc::Sender<serde_json::Value>,
    },
    /// The same, only to know its state (`orbit ctl sessions`): nothing opens.
    Lookup {
        id: String,
    },
    /// Runs the bot's poller workflow (`owner/repo:workflow.yml`): it looks now.
    KickBot(String),
    /// A mission hands a sub-ticket to the bot: to this team, in this state.
    Dispatch {
        identifier: String,
        team: String,
        state: String,
    },
    /// The ticket a PR opened without one is attached to, if any.
    PrTicket {
        url: String,
    },
    /// Linear's search for the command palette: open tickets on any team.
    Search {
        term: String,
    },
    /// A new ticket's note, read by Haiku (src/capture.rs).
    ReadNote {
        key: String,
        note: String,
        /// settings.json `laya`: whether to use it, and from what
        /// confidence; `None` when it is off.
        laya: Option<(bool, f64)>,
        /// Which team owns what (capture::teams_text).
        teams: String,
        /// Read already (a Work spot): only the duplicate check runs.
        known: Option<crate::capture::Intake>,
    },
    /// Downloads Laya's checkpoint (laya.rs), with progress.
    DownloadLaya,
    /// The newest release, if newer than this build (update.rs).
    CheckUpdate,
    /// Swaps release `tag` in for the running app (update.rs).
    Update {
        tag: String,
    },
    /// Fetches a profile bundle and unpacks it (profile.rs).
    ImportProfile {
        source: String,
    },
    /// Adds a new report to an existing ticket instead of filing it again.
    AddReport {
        issue_id: String,
        identifier: String,
        url: String,
        body: String,
    },
    /// Writes the description and creates the ticket, triaged, then leaves
    /// the routing comment.
    CreateTicket {
        /// Anthropic's, to draft the description; `None` files it as written.
        key: Option<String>,
        note: String,
        intake: crate::capture::Intake,
        answers: Vec<(String, String)>,
        /// Written already (a Work spot's): used as is, not drafted again.
        description: Option<String>,
    },
    /// The ticket's test plan: the one on the ticket, or a new one from
    /// Claude, posted once (src/testplan.rs).
    TestPlan {
        issue_id: String,
        key: String,
        context: String,
        /// `(owner, repo, number)` of the ticket's merged or open PRs, for
        /// their diffs.
        prs: Vec<(String, String, u64)>,
        token: Option<String>,
    },
}

pub enum Event {
    Loaded {
        request: u64,
        result: Result<Home, String>,
    },
    StateSet {
        issue_id: String,
        state_id: String,
        result: Result<(), String>,
    },
    TakenFromBot {
        result: Result<(), String>,
    },
    CommentAdded {
        temp_id: String,
        result: Result<Comment, String>,
    },
    Brief {
        fingerprint: u64,
        result: Result<String, String>,
    },
    Shortened {
        items: Vec<(String, String)>,
        result: Result<Vec<(String, String)>, String>,
    },
    Customers {
        items: Vec<(String, String)>,
        result: Result<Vec<(String, Vec<String>)>, String>,
    },
    Digests {
        items: Vec<(String, String)>,
        result: Result<Vec<(String, String)>, String>,
    },
    NotesSaved(Result<usize, String>),
    Diff {
        url: String,
        result: Result<Vec<crate::github::File>, String>,
    },
    Gate {
        url: String,
        result: Result<crate::github::Gate, String>,
    },
    /// The merge commit's SHA, or the PR in the merge queue.
    Merged {
        url: String,
        result: Result<crate::github::Merged, String>,
    },
    Approved {
        url: String,
        result: Result<(), String>,
    },
    /// The gate after Merge when ready went on or off.
    AutoMerged {
        url: String,
        result: Result<crate::github::Gate, String>,
    },
    BranchUpdated {
        url: String,
        result: Result<(), String>,
    },
    AiReviewAsked {
        url: String,
        head: String,
        result: Result<(), String>,
    },
    Reviewed {
        url: String,
        /// The ticket whose session fixes the comments once posted.
        fix: Option<String>,
        result: Result<(), String>,
    },
    Quick {
        issue_id: String,
        hash: u64,
        result: Result<Vec<crate::laya::Proposal>, String>,
    },
    Guided {
        url: String,
        result: Result<crate::anthropic::Guide, String>,
    },
    /// `(key, text hash)` asked, and Laya's `(key, yes, confidence)`.
    LayaAsks {
        asked: Vec<(String, u64)>,
        result: Result<Vec<(String, bool, f64)>, String>,
    },
    PrReady {
        url: String,
        result: Result<(), String>,
    },
    PrClosed {
        url: String,
        result: Result<(), String>,
    },
    Talk {
        url: String,
        result: Result<crate::github::Talk, String>,
    },
    LoginFound {
        name: String,
        result: Result<Option<String>, String>,
    },
    ReviewRequested {
        url: String,
        login: String,
        result: Result<(), String>,
    },
    Labeled {
        url: String,
        label: String,
        result: Result<(), String>,
    },
    AnsweredPr {
        url: String,
        result: Result<String, String>,
    },
    ApproveNote {
        url: String,
        result: Result<String, String>,
    },
    Incidents(Result<Vec<crate::incidents::Incident>, String>),
    Alerts(Result<Vec<crate::incidents::Alert>, String>),
    /// Each watched release with its latest run, or why it could not be read.
    Releases(
        Vec<(
            crate::releases::Release,
            Result<Option<crate::releases::Run>, String>,
        )>,
    ),
    File {
        url: String,
        result: Result<Vec<u8>, String>,
    },
    /// Each side on its own, so one failing still shows the other.
    Inbox {
        linear: Result<crate::inbox::Fetched, String>,
        github: Result<crate::inbox::Fetched, String>,
    },
    /// The notes whose marking read failed, to show again.
    InboxRead(Result<(), (Vec<String>, String)>),
    /// The ticket `Command::Issue` asked for.
    Issue(Result<Box<crate::linear::Issue>, String>),
    /// An `orbit ctl` write went through (true) or not: the board looks again.
    Wrote(bool),
    Looked {
        id: String,
        result: Result<Box<crate::linear::Issue>, String>,
    },
    Dispatched {
        identifier: String,
        result: Result<(), String>,
    },
    Kicked(Result<(), String>),
    /// The ticket `Command::PrTicket` found for the PR at `url`.
    PrTicket {
        url: String,
        result: Result<Option<Box<crate::linear::Issue>>, String>,
    },
    /// The user's open PRs (`Command::MyPrs`).
    MyPrs(Result<Vec<crate::github::MyPr>, String>),
    /// `Command::DependabotPrs`.
    DependabotPrs(Result<Vec<crate::github::MyPr>, String>),
    /// `Command::Herdr`.
    Herdr(Result<Vec<crate::herdr::Agent>, String>),
    /// What a herdr action did, or why it did not.
    HerdrDone(Result<String, String>),
    /// `Command::TeamStats`.
    TeamStats(Result<crate::signoz::Team, String>),
    /// `Command::RepoPrs`.
    RepoPrs {
        slug: String,
        result: Result<Vec<crate::github::RepoPr>, String>,
    },
    Reviewers {
        slug: String,
        result: Result<Vec<(String, String)>, String>,
    },
    /// What Linear's search found for the palette's `term`.
    Found {
        term: String,
        result: Result<Vec<crate::capture::Duplicate>, String>,
    },
    /// What Haiku read, and the open tickets that look like the same report.
    /// `(done, total)` bytes of Laya's download, then its end.
    LayaProgress(u64, u64),
    LayaDownloaded(Result<(), String>),
    UpdateFound(Result<Option<String>, String>),
    /// The installed app, to relaunch.
    Updated(Result<std::path::PathBuf, String>),
    ProfileImported {
        source: String,
        result: Result<Box<crate::profile::Profile>, String>,
    },
    NoteRead(Result<(crate::capture::Intake, Vec<crate::capture::Duplicate>), String>),
    /// `(identifier, url)` of the ticket the report was added to.
    ReportAdded(Result<(String, String), String>),
    /// `(identifier, url)` of the new ticket.
    TicketCreated(Result<(String, String), String>),
    /// The plan, and whether it was posted just now.
    TestPlan {
        issue_id: String,
        result: Result<(crate::testplan::TestPlan, bool), String>,
    },
}

pub struct Backend {
    commands: mpsc::UnboundedSender<Command>,
    events: std::sync::mpsc::Receiver<Event>,
}

impl Backend {
    pub fn spawn(key: &str, ctx: egui::Context) -> Self {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        // Demo mode: every answer from the fixtures (demo.rs), no network.
        if crate::demo::on() {
            std::thread::spawn(move || {
                while let Some(command) = command_rx.blocking_recv() {
                    if let Some(event) = crate::demo::answer(command) {
                        let _ = event_tx.send(event);
                        ctx.request_repaint();
                    }
                }
            });
            return Self {
                commands: command_tx,
                events: event_rx,
            };
        }
        let client = Arc::new(Client::new(key));
        std::thread::Builder::new()
            .name("backend".into())
            .spawn(move || {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime")
                    .block_on(run(client, command_rx, event_tx, ctx));
            })
            .expect("backend thread");
        Self {
            commands: command_tx,
            events: event_rx,
        }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

async fn run(
    client: Arc<Client>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: std::sync::mpsc::Sender<Event>,
    ctx: egui::Context,
) {
    let http = reqwest::Client::new();
    while let Some(command) = commands.recv().await {
        let client = client.clone();
        let http = http.clone();
        let events = events.clone();
        let ctx = ctx.clone();
        // Requests run concurrently; each answer carries enough to be matched
        // against what the interface currently shows.
        tokio::spawn(async move {
            let text = |error: anyhow::Error| error.to_string();
            let event = match command {
                Command::Refresh { request, teams } => Event::Loaded {
                    request,
                    result: client.home(&teams).await.map_err(text),
                },
                Command::TakeFromBot { issue_id } => Event::TakenFromBot {
                    result: client.assign_to_me(&issue_id).await.map_err(text),
                },
                Command::SetState { issue_id, state_id } => Event::StateSet {
                    result: client.set_state(&issue_id, &state_id).await.map_err(text),
                    issue_id,
                    state_id,
                },
                Command::MoveTeam {
                    issue_id,
                    team_id,
                    state_id,
                } => Event::StateSet {
                    result: client
                        .move_team(&issue_id, &team_id, &state_id)
                        .await
                        .map_err(text),
                    issue_id,
                    state_id,
                },
                Command::AddComment {
                    issue_id,
                    temp_id,
                    body,
                } => Event::CommentAdded {
                    result: client.add_comment(&issue_id, &body).await.map_err(text),
                    temp_id,
                },
                Command::Brief {
                    fingerprint,
                    key,
                    board,
                } => Event::Brief {
                    fingerprint,
                    result: crate::anthropic::write(&http, &key, &board)
                        .await
                        .map_err(text),
                },
                Command::Diff {
                    url,
                    owner,
                    repo,
                    number,
                    token,
                } => Event::Diff {
                    result: crate::github::diff(&http, token.as_deref(), &owner, &repo, number)
                        .await
                        .map_err(text),
                    url,
                },
                Command::Gate { url } => {
                    let u = url.clone();
                    Event::Gate {
                        result: tokio::task::spawn_blocking(move || crate::github::gate(&u))
                            .await
                            .map_err(anyhow::Error::from)
                            .and_then(|r| r)
                            .map_err(text),
                        url,
                    }
                }
                Command::AutoMerge { url, head } => {
                    let u = url.clone();
                    Event::AutoMerged {
                        result: tokio::task::spawn_blocking(move || {
                            crate::github::auto_merge(&u, head.as_deref())
                        })
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                        url,
                    }
                }
                Command::Merge { url, head } => {
                    let u = url.clone();
                    Event::Merged {
                        result: tokio::task::spawn_blocking(move || {
                            crate::github::merge(&u, &head)
                        })
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                        url,
                    }
                }
                Command::LayaAsks(texts) => Event::LayaAsks {
                    asked: texts
                        .iter()
                        .map(|(k, t)| (k.clone(), crate::store::text_hash(t)))
                        .collect(),
                    result: tokio::task::spawn_blocking(move || crate::laya::asks(&texts))
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                },
                Command::Quick {
                    issue_id,
                    hash,
                    text: ticket,
                } => Event::Quick {
                    result: tokio::task::spawn_blocking(move || crate::laya::quick(&ticket))
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                    issue_id,
                    hash,
                },
                Command::Review {
                    url,
                    head,
                    drafts,
                    body,
                    fix,
                    changes,
                } => {
                    let u = url.clone();
                    Event::Reviewed {
                        result: tokio::task::spawn_blocking(move || {
                            crate::github::review(&u, &head, &drafts, &body, changes)
                        })
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                        url,
                        fix,
                    }
                }
                Command::ApproveNote { url, key, asked } => Event::ApproveNote {
                    result: crate::anthropic::approve_note(&http, &key, &asked)
                        .await
                        .map_err(text),
                    url,
                },
                Command::Approve { url, head, body } => {
                    let u = url.clone();
                    Event::Approved {
                        result: tokio::task::spawn_blocking(move || {
                            crate::github::approve(&u, &head, body.as_deref())
                        })
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                        url,
                    }
                }
                Command::FindLogin { org, name } => {
                    let n = name.clone();
                    Event::LoginFound {
                        result: tokio::task::spawn_blocking(move || {
                            crate::github::login_of(&org, &n)
                        })
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                        name,
                    }
                }
                Command::AddLabel { url, label } => {
                    let (u, l) = (url.clone(), label.clone());
                    Event::Labeled {
                        result: tokio::task::spawn_blocking(move || {
                            crate::github::add_label(&u, &l)
                        })
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                        url,
                        label,
                    }
                }
                Command::RequestReview { url, login } => {
                    let (u, l) = (url.clone(), login.clone());
                    Event::ReviewRequested {
                        result: tokio::task::spawn_blocking(move || {
                            crate::github::request_review(&u, &l)
                        })
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                        url,
                        login,
                    }
                }
                Command::Talk { url } => {
                    let u = url.clone();
                    Event::Talk {
                        result: tokio::task::spawn_blocking(move || crate::github::talk(&u))
                            .await
                            .map_err(anyhow::Error::from)
                            .and_then(|r| r)
                            .map_err(text),
                        url,
                    }
                }
                Command::ReadyPr { url } => {
                    let u = url.clone();
                    Event::PrReady {
                        result: tokio::task::spawn_blocking(move || crate::github::ready(&u))
                            .await
                            .map_err(anyhow::Error::from)
                            .and_then(|r| r)
                            .map_err(text),
                        url,
                    }
                }
                Command::ClosePr { url } => {
                    let u = url.clone();
                    Event::PrClosed {
                        result: tokio::task::spawn_blocking(move || crate::github::close(&u))
                            .await
                            .map_err(anyhow::Error::from)
                            .and_then(|r| r)
                            .map_err(text),
                        url,
                    }
                }
                Command::UpdateBranch { url, head } => {
                    let u = url.clone();
                    Event::BranchUpdated {
                        result: tokio::task::spawn_blocking(move || {
                            crate::github::update_branch(&u, &head)
                        })
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                        url,
                    }
                }
                Command::AiReview { url, head, how } => {
                    let u = url.clone();
                    Event::AiReviewAsked {
                        result: tokio::task::spawn_blocking(move || {
                            crate::github::request_ai_review(&u, &how)
                        })
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                        url,
                        head,
                    }
                }
                Command::Guide {
                    url,
                    key,
                    mut context,
                    diff,
                    since,
                } => Event::Guided {
                    result: async {
                        let base = since.as_ref().map(|s| s.base.clone());
                        if let Some(s) = since {
                            context.push_str(&s.context(&http).await);
                        }
                        let mut guide =
                            crate::anthropic::guide(&http, &key, &context, &diff).await?;
                        guide.since = base;
                        Ok::<_, anyhow::Error>(guide)
                    }
                    .await
                    .map_err(text),
                    url,
                },
                Command::AskPr {
                    url,
                    key,
                    context,
                    diff,
                    question,
                } => Event::AnsweredPr {
                    result: crate::anthropic::ask_pr(&http, &key, &context, &diff, &question)
                        .await
                        .map_err(text),
                    url,
                },
                Command::TestPlan {
                    issue_id,
                    key,
                    context,
                    prs,
                    token,
                } => Event::TestPlan {
                    result: test_plan(&client, &http, &issue_id, &key, &context, prs, token)
                        .await
                        .map_err(text),
                    issue_id,
                },
                Command::ImportProfile { source } => Event::ProfileImported {
                    result: async {
                        let bundle = crate::profile::fetch(&http, &source).await?;
                        crate::profile::unpack(&bundle, &crate::profile::unpacked()).map(Box::new)
                    }
                    .await
                    .map_err(|e| format!("{e:#}")),
                    source,
                },
                Command::DownloadLaya => {
                    // A progress event per percent, not per chunk.
                    let last = std::sync::atomic::AtomicU64::new(u64::MAX);
                    let result = crate::laya::download(&http, |done, total| {
                        let percent = done * 100 / total.max(1);
                        if last.swap(percent, std::sync::atomic::Ordering::Relaxed) != percent
                            && events.send(Event::LayaProgress(done, total)).is_ok()
                        {
                            ctx.request_repaint();
                        }
                    })
                    .await;
                    Event::LayaDownloaded(result.map_err(|e| format!("{e:#}")))
                }
                Command::CheckUpdate => Event::UpdateFound(
                    tokio::task::spawn_blocking(crate::update::check)
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                ),
                Command::Update { tag } => Event::Updated(
                    tokio::task::spawn_blocking(move || crate::update::install(&tag))
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(|e| format!("{e:#}")),
                ),
                Command::Inbox => {
                    let (linear, github) = tokio::join!(
                        client.notifications(),
                        tokio::task::spawn_blocking(crate::inbox::github)
                    );
                    Event::Inbox {
                        linear: linear.map(|n| crate::inbox::from_linear(&n)).map_err(text),
                        github: github
                            .map_err(anyhow::Error::from)
                            .and_then(|r| r)
                            .map_err(text),
                    }
                }
                Command::InboxRead { linear, github } => {
                    let ids = linear.iter().chain(&github).cloned().collect::<Vec<_>>();
                    let gh = github.clone();
                    let result = match client.read_notifications(&linear).await {
                        Ok(()) => {
                            tokio::task::spawn_blocking(move || crate::inbox::github_read(&gh))
                                .await
                                .map_err(anyhow::Error::from)
                                .and_then(|r| r)
                        }
                        Err(error) => Err(error),
                    };
                    Event::InboxRead(result.map_err(|e| (ids, text(e))))
                }
                Command::File { url } => Event::File {
                    result: client.file(&url).await.map_err(text),
                    url,
                },
                Command::Incidents { key } => {
                    Event::Incidents(crate::incidents::live(&http, &key).await.map_err(text))
                }
                Command::Releases(releases) => {
                    let mut out = Vec::new();
                    for release in releases {
                        let asked = release.clone();
                        let run =
                            tokio::task::spawn_blocking(move || crate::releases::latest(&asked))
                                .await
                                .map_err(anyhow::Error::from)
                                .and_then(|r| r)
                                .map_err(text);
                        out.push((release, run));
                    }
                    Event::Releases(out)
                }
                Command::Alerts { key } => {
                    Event::Alerts(crate::incidents::alerts(&http, &key).await.map_err(text))
                }
                Command::Issue { id } => {
                    Event::Issue(client.issue(&id).await.map(Box::new).map_err(text))
                }
                Command::KickBot(target) => Event::Kicked(
                    tokio::task::spawn_blocking(move || crate::github::run_workflow(&target))
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                ),
                Command::Dispatch {
                    identifier,
                    team,
                    state,
                } => Event::Dispatched {
                    result: client
                        .dispatch(&identifier, &team, &state)
                        .await
                        .map_err(text),
                    identifier,
                },
                Command::Write { write, reply } => {
                    let result = client.write(&write).await.map_err(text);
                    let ok = result.is_ok();
                    let _ = reply.send(match result {
                        Ok(done) => serde_json::json!({"ok": true, "gate": "queue", "done": done}),
                        Err(error) => {
                            serde_json::json!({"ok": false, "gate": "queue", "error": error})
                        }
                    });
                    Event::Wrote(ok)
                }
                Command::Lookup { id } => Event::Looked {
                    result: client.issue(&id).await.map(Box::new).map_err(text),
                    id,
                },
                Command::PrTicket { url } => Event::PrTicket {
                    result: client
                        .issue_for_url(&url)
                        .await
                        .map(|i| i.map(Box::new))
                        .map_err(text),
                    url,
                },
                Command::MyPrs => Event::MyPrs(
                    tokio::task::spawn_blocking(crate::github::my_prs)
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                ),
                Command::DependabotPrs(repos) => Event::DependabotPrs(
                    tokio::task::spawn_blocking(move || crate::github::dependabot_prs(&repos))
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                ),
                Command::Herdr => Event::Herdr(
                    tokio::task::spawn_blocking(crate::herdr::read)
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                ),
                Command::HerdrFocus(target) => Event::HerdrDone(
                    tokio::task::spawn_blocking(move || {
                        crate::herdr::focus(&target).map(|()| String::new())
                    })
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(|r| r)
                    .map_err(text),
                ),
                Command::HerdrStart {
                    name,
                    label,
                    cwd,
                    args,
                } => Event::HerdrDone(
                    tokio::task::spawn_blocking(move || {
                        crate::herdr::start(&name, &label, &cwd, &args)
                            .map(|name| format!("Started in herdr as {name}"))
                    })
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(|r| r)
                    .map_err(text),
                ),
                Command::HerdrPlugin => Event::HerdrDone(
                    tokio::task::spawn_blocking(|| {
                        crate::herdr::install_plugin()
                            .map(|dir| format!("Plugin linked into herdr from {}", dir.display()))
                    })
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(|r| r)
                    .map_err(text),
                ),
                Command::TeamStats { mcp, repos } => {
                    let login = tokio::task::spawn_blocking(|| {
                        crate::claude::gh(&["api", "user", "--jq", ".login"])
                    })
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(|r| r)
                    .map(|l| l.trim().to_owned())
                    .unwrap_or_default();
                    Event::TeamStats(
                        crate::signoz::team(&http, &mcp, &repos, &login, "7d")
                            .await
                            .map_err(text),
                    )
                }
                Command::RepoPrs(slug) => {
                    let repo = slug.clone();
                    Event::RepoPrs {
                        slug,
                        result: tokio::task::spawn_blocking(move || crate::github::repo_prs(&repo))
                            .await
                            .map_err(anyhow::Error::from)
                            .and_then(|r| r)
                            .map_err(text),
                    }
                }
                Command::Reviewers(slug) => {
                    let repo = slug.clone();
                    Event::Reviewers {
                        slug,
                        result: tokio::task::spawn_blocking(move || {
                            crate::github::reviewers(&repo)
                        })
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|r| r)
                        .map_err(text),
                    }
                }
                Command::Search { term } => Event::Found {
                    result: client.find(&term).await.map_err(text),
                    term,
                },
                Command::ReadNote {
                    key,
                    note,
                    laya,
                    teams,
                    known,
                } => Event::NoteRead(
                    async {
                        let read_already = known.is_some();
                        let mut intake = match known {
                            Some(intake) => intake,
                            None => crate::anthropic::intake(&http, &key, &note, &teams).await?,
                        };
                        let laya = laya.filter(|_| !read_already);
                        // Laya's bug-or-not after Haiku's; if it cannot be
                        // asked, Haiku's intake stands (laya.rs).
                        if let Some((use_it, min)) = laya {
                            let text = note.clone();
                            let decided =
                                tokio::task::spawn_blocking(move || crate::laya::decide(&text))
                                    .await
                                    .map_err(|e| anyhow::anyhow!("Laya stopped: {e}"))
                                    .and_then(|r| r);
                            match decided {
                                Ok(decided) => {
                                    let compared =
                                        crate::laya::merge(&mut intake, &decided, use_it, min);
                                    if compared.used {
                                        intake.by_laya = vec!["kind".into()];
                                    }
                                    let log = crate::store::data_dir().join("laya-shadow.jsonl");
                                    let _ = crate::laya::log(&log, &compared);
                                }
                                Err(error) => intake.laya_error = Some(format!("{error:#}")),
                            }
                        }
                        // Searched by Haiku's title: the note is too rough to search by.
                        let found = client.search_open(&intake.title).await?;
                        let duplicates = if found.is_empty() {
                            Vec::new()
                        } else {
                            crate::anthropic::duplicates(&http, &key, &note, &found).await?
                        };
                        anyhow::Ok((intake, duplicates))
                    }
                    .await
                    .map_err(text),
                ),
                Command::AddReport {
                    issue_id,
                    identifier,
                    url,
                    body,
                } => Event::ReportAdded(
                    client
                        .add_comment(&issue_id, &body)
                        .await
                        .map(|_| (identifier, url))
                        .map_err(text),
                ),
                Command::CreateTicket {
                    key,
                    note,
                    intake,
                    answers,
                    description,
                } => Event::TicketCreated(
                    async {
                        use crate::capture;
                        let description = match (description, key) {
                            (Some(written), _) => written,
                            (None, Some(key)) => {
                                let input = capture::draft_input(&note, &intake, &answers);
                                crate::anthropic::ticket_description(&http, &key, &input).await?
                            }
                            (None, None) => note.clone(),
                        };
                        let (id, identifier, url) = client
                            .create_ticket(
                                &intake.team,
                                &intake.title,
                                &description,
                                capture::linear_priority(&intake.priority),
                                &intake.priority,
                                intake.kind == "bug",
                            )
                            .await?;
                        // Filed as written, nobody triaged it: no routing comment.
                        if !intake.kind.is_empty() {
                            client
                                .add_comment(&id, &capture::routing(&intake, &identifier))
                                .await?;
                        }
                        anyhow::Ok((identifier, url))
                    }
                    .await
                    .map_err(text),
                ),
                Command::SaveNotes(notes) => {
                    let mut saved = 0;
                    let mut failure = None;
                    for (issue_id, subtitle, metadata) in &notes {
                        match client.save_note(issue_id, subtitle, metadata).await {
                            Ok(()) => saved += 1,
                            Err(error) => failure = Some(text(error)),
                        }
                    }
                    Event::NotesSaved(failure.map_or(Ok(saved), Err))
                }
                Command::Customers { key, items } => Event::Customers {
                    result: crate::anthropic::customers(&http, &key, &items)
                        .await
                        .map_err(text),
                    items,
                },
                Command::Digests { key, items } => Event::Digests {
                    result: crate::anthropic::digests(&http, &key, &items)
                        .await
                        .map_err(text),
                    items,
                },
                Command::Shorten { key, items } => Event::Shortened {
                    result: crate::anthropic::shorten(&http, &key, &items)
                        .await
                        .map_err(text),
                    items,
                },
            };
            if events.send(event).is_ok() {
                ctx.request_repaint();
            }
        });
    }
}

/// The ticket's plan if it has one, else a new one from Claude, posted.
/// Linear is asked again right before posting, so a plan posted meanwhile
/// (another window, a quick retry) is reused, never doubled.
async fn test_plan(
    client: &Client,
    http: &reqwest::Client,
    issue_id: &str,
    key: &str,
    context: &str,
    prs: Vec<(String, String, u64)>,
    token: Option<String>,
) -> anyhow::Result<(crate::testplan::TestPlan, bool)> {
    use crate::testplan;
    let marker = testplan::marker(issue_id);
    let existing = |comment: crate::linear::Comment| {
        testplan::from_comment(&comment.body)
            .map(|plan| (plan, false))
            .ok_or_else(|| anyhow::anyhow!("The ticket has a test plan comment that no longer reads as one; fix or delete it in Linear."))
    };
    if let Some(comment) = client.find_comment(issue_id, &marker).await? {
        return existing(comment);
    }
    // Without a diff Claude still has the ticket and its plan. Several PRs
    // share the budget, each under its own heading.
    let limit = 60_000 / prs.len().max(1);
    let mut diff = String::new();
    for (owner, repo, number) in prs {
        if let Ok(files) = crate::github::diff(http, token.as_deref(), &owner, &repo, number).await
        {
            diff.push_str(&format!("\n##### {owner}/{repo}#{number}\n"));
            diff.push_str(&crate::github::text(&files, limit));
        }
    }
    let plan = crate::anthropic::test_plan(http, key, context, &diff).await?;
    if let Some(comment) = client.find_comment(issue_id, &marker).await? {
        return existing(comment);
    }
    client
        .add_comment(issue_id, &testplan::to_comment(&plan, issue_id))
        .await?;
    Ok((plan, true))
}

//! Application state. Views draw and emit `Action`s; `apply` runs them after
//! drawing, so no view mutates state while it is borrowed.
//!
//! The interface is optimistic: a changed state or a sent comment shows at
//! once, and Linear's answers catch up behind. An answer that still tells
//! the story from before the user's action is held back, never shown.
//!
//! - `actions`: what the user does (`apply`).
//! - `events`: what the backend answers (`handle`).
//! - `board`: the optimistic layer over Linear's last answer.
//! - `ai`: the Claude helpers (brief, short names, customers, digests) and
//!   the notes that share their results through Linear.
//! - `review`: hooks for screenshots and scripted views, off by default.

mod actions;
pub(crate) use actions::quiet_ms;
mod ai;
mod board;
mod control;
mod events;
pub mod palette;
pub mod panes;
pub(crate) mod review;
mod settings;

use std::collections::{HashMap, HashSet};

use crate::backend::{Backend, Command};
use crate::linear::{Issue, State};
use crate::secrets::{self, Secret};
use crate::store::{self, Settings};
use crate::story::{Card, Pr};
use board::{Local, Shown};

const REVIEWED: &str = "reviewed.json";

/// A review the user posted: the head it was at, and its comments as
/// `file:line: body`.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Reviewed {
    pub head: String,
    pub comments: Vec<String>,
}

/// A question the reviewer asked about a PR, and Claude's answer once in.
pub type Asked = (String, Option<Result<String, String>>);

/// Claude's answer lands on the question waiting for one (one at a time).
fn answer(asked: &mut [Asked], result: Result<String, String>) {
    if let Some(slot) = asked.iter_mut().find(|(_, a)| a.is_none()) {
        slot.1 = Some(result);
    }
}

/// Deserialize: `orbit ctl do` (control.rs) presses any of these by JSON.
#[derive(Clone, Debug, serde::Deserialize)]
pub enum Action {
    SignIn,
    /// Sign in through the browser (src/oauth.rs).
    SignInWithLinear,
    /// Forgets the stored key and returns to the login screen.
    SignOut,
    /// Saves the key typed into its connection row in Settings and starts
    /// using it.
    SaveKey(crate::secrets::Secret),
    /// Opens the team picker.
    PickTeams,
    /// Adds or removes a team from the picker's selection.
    ToggleTeam(String),
    /// Saves the picked teams and reloads the board.
    SaveTeams,
    Refresh,
    /// Opens the sheet on this view; Settings takes the pane area instead.
    Show(View),
    /// Opens or closes the sheet (i, Esc); closing also leaves Settings.
    Sheet(bool),
    /// Moves the sheet's cursor to this column and row.
    SheetAt(usize, usize),
    /// Shows this ticket (id) in a new window, or in a split beside the
    /// focused pane; the sheet closes.
    OpenTicket {
        id: String,
        split: bool,
    },
    /// Shows the next ticket For you lists in the active pane (Alt+Down).
    NextForYou,
    /// Shows this window of the strip (Cmd+1 to 9, Cmd+Shift+] and [).
    GoWindow(usize),
    Select(String),
    /// Shows a ticket by identifier in the active pane.
    Focus(String),
    Open(String),
    /// A link Cmd+clicked in a terminal: a ticket or PR Orbit knows opens
    /// here (a PR on its Diff tab), anything else in the browser.
    Link(String),
    Tab(Tab),
    /// Loads the shown PR's diff again.
    ReloadDiff,
    /// Asks GitHub again now for this PR's state (the ↻ beside its buttons).
    RefreshGate(String),
    /// Starts this PR's review over after new commits: the Diff tab, the
    /// diff loaded again, drafts dropped, a fresh walkthrough.
    ReviewAgain(String),
    /// Shows this PR, by URL, on the selected ticket's Diff tab.
    DiffPr(String),
    /// Merge when ready: GitHub merges or queues this PR once its checks
    /// pass, bound to `head`. Only offered while checks are all it waits on.
    AutoMerge {
        url: String,
        head: String,
    },
    /// Turns Merge when ready off again.
    CancelAutoMerge(String),
    /// Squash-merges this PR URL at this head commit, or adds it to the
    /// merge queue. Only offered when the gate is green, and checked again
    /// before merging.
    Merge {
        url: String,
        head: String,
    },
    /// A click on a quick action Laya proposed: logged as a click on
    /// `question` for `ticket` (laya-quick.jsonl), then `then` runs.
    #[serde(skip)]
    Quick {
        ticket: String,
        question: String,
        then: Box<Action>,
    },
    /// Asks for an AI review of the user's own PR, at this head, the way
    /// the profile says. Only from the user's click.
    AiReview {
        url: String,
        head: String,
    },
    /// Approves this PR URL at this head commit. Only offered when the
    /// gate says the user's review is missing, and checked again first.
    /// `body`: the user's approval note (approve_notes), if they kept one.
    Approve {
        url: String,
        head: String,
        body: Option<String>,
    },
    /// The Diff tab in one column (true) or side by side (false, the default).
    UnifiedDiff(bool),
    /// KTLO's batch review: every PR's diff under its row, or not.
    DiffAll(bool),
    /// The active Claude tab moves to a new pane on the right; its pane
    /// shows the ticket's walkthrough.
    SplitClaude,
    /// Marks a diff file viewed, or not (`ui::diff::viewed_key`).
    ViewedFile(String),
    /// Folds or opens one file of a PR's diff: (PR url, path).
    FoldFile(String, String),
    /// Folds or unfolds a folder of a PR's diff: (PR url, folder path).
    FoldFolder(String, String),
    /// Asks this GitHub login to review the PR.
    RequestReview {
        url: String,
        login: String,
    },
    /// Moves the ticket to another team (by id), in the new team's state of
    /// the same kind.
    MoveTeam {
        issue_id: String,
        team_id: String,
    },
    /// Starts one Claude session with every open notification and its link.
    HandleInbox,
    /// A session ranks the backlog by the week's goals (skills/find-work).
    FindWork,
    /// A session ships KTLO fixes and moves the rest to their teams (skills/ktlo).
    ClearKtlo,
    /// Wakes the bot now: its poller workflow runs, whatever its two-minute rest.
    WakeBot,
    /// The same over the bot's PRs waiting on the user's review (For you).
    ShipReviews,
    /// Closes this PR without merging; with `cancel`, the ticket behind it
    /// moves to this canceled state too.
    ClosePr {
        url: String,
        cancel: Option<(String, String)>,
    },
    /// Turns the user's questions about this PR and their answers into one
    /// short approval note.
    NoteForApproval(String),
    /// The approval note as the user edited it; `None` drops it.
    SetApprovalNote {
        url: String,
        note: Option<String>,
    },
    /// Merges the base into this PR's branch at this head (GitHub's Update
    /// branch). Only offered while the gate says it is behind.
    UpdateBranch {
        url: String,
        head: String,
    },
    /// Adds this inline comment to the review of this PR, or takes it
    /// back when it is there already.
    Draft {
        url: String,
        draft: crate::github::Draft,
    },
    /// Drops every drafted comment on this PR.
    DiscardDrafts(String),
    /// Posts the drafted comments on this PR as one Comment review at
    /// `head`; with `fix`, the ticket's session then fixes them.
    Review {
        url: String,
        head: String,
        fix: Option<String>,
        /// Request changes instead of commenting.
        changes: bool,
    },
    /// The user's own review from the Diff tab's Review menu: their summary
    /// (required to request changes) plus any drafted line comments, as
    /// one Comment or Request changes review at `head`. Only their click.
    ReviewSummary {
        url: String,
        head: String,
        body: String,
        changes: bool,
    },
    /// Marks a draft PR ready for review (`gh pr ready`).
    ReadyPr(String),
    /// A new empty tab in the strip (Cmd+N): For you, a shell or a ticket next.
    NewTab,
    /// Names window `i` in the strip; empty brings back the automatic name.
    RenameWindow(usize, String),
    /// Moves a window along the strip.
    MoveWindow {
        from: usize,
        to: usize,
    },
    /// Puts a label on a PR: the profile's review router (`reviewRoulette`).
    AddLabel {
        url: String,
        label: String,
    },
    /// Asks Claude to walk through the shown PR's diff.
    Guide,
    /// Shows one step of this PR's walkthrough, or every file (`None`).
    GuideStep(Option<(String, usize)>),
    /// Asks Claude this about the shown PR; the answer shows under the
    /// walkthrough.
    AskPr(String),
    /// Attaches to the ticket's Claude Code session, starting or resuming it.
    Claude(String),
    /// The same for the triage session.
    Triage,
    /// A session from the Sessions view: a ticket's, or triage.
    OpenSession(String),
    /// Stops a background session; its conversation is kept.
    StopSession(String),
    /// Closes a session: stops it if it runs and drops it from Orbit's
    /// sessions (Shift+X in Sessions, `orbit ctl close`). Claude Code keeps
    /// the conversation.
    CloseSession(String),
    /// Sends a new prompt to the Codex thread owned by this key.
    CodexPrompt {
        key: String,
        text: String,
    },
    /// Answers an app-server request after the user clicked in the session.
    CodexAnswer {
        key: String,
        response: serde_json::Value,
    },
    /// Applies the action with this agent for any session it starts, in
    /// place of Settings' default: "or Codex" next to a start.
    WithAgent(crate::claude::Agent, Box<Action>),
    /// Forgets a ticket's session and starts a new one.
    FreshSession(String),
    /// Runs a skill command in the ticket's Claude session, typed and sent
    /// (`submit`) or left in the prompt box to finish.
    RunSkill {
        issue_id: String,
        command: String,
        submit: bool,
    },
    SetState {
        issue_id: String,
        state_id: String,
    },
    SendComment,
    /// Loads who may review on a repo (`owner/repo`), once: Request
    /// review's suggestions.
    LoadReviewers(String),
    /// A line run from the palette, by its title: remembered, so recent
    /// and frequent picks come first next time.
    PalettePicked(String),
    /// Puts this text in the ticket's comment box, to read, edit and send:
    /// the bot's suggested reply to the requester (The ask).
    DraftComment(String),
    /// Puts a theme on screen and remembers it.
    SetTheme(crate::theme::Choice),
    /// Loads the ticket's test plan, or has Claude write and post one.
    TestPlan(String),
    /// Attaches a session, starting or resuming it, without changing what
    /// the ticket pane shows.
    Attach(String),
    /// Runs the bot's step remotely: posts its command (the profile's)
    /// on the ticket, which his poller picks up. Only from the step menu.
    AskBot {
        issue_id: String,
        command: crate::story::BotCommand,
    },
    /// Yolo: hands the ticket straight to the bot, whatever team and state
    /// it is in: the bot's team (profile `orchestrator.team`) in Todo, which
    /// is the bot's go. No triage, no plan first.
    Yolo(String),
    /// The New ticket window (src/capture.rs): open it, read the note,
    /// create the ticket, close it.
    NewTicket,
    /// The same, with the note already written (from the palette).
    NewTicketWith(String),
    /// Opens or closes the command palette (Cmd+K).
    TogglePalette,
    /// Closes one in-app toast (`App::toasts`), by its key.
    DismissToast(String),
    /// The sheet's search field changed to this.
    SheetQuery(String),
    /// Opens the Sessions view with this in its search (a state word from
    /// the bottom bar's counts: "your turn", "working").
    SessionsFiltered(String),
    /// Asks GitHub again for a repo page's PRs.
    RefreshRepo(String),
    /// Brings a herdr agent forward in herdr (herdr.rs), by name or pane.
    HerdrFocus(String),
    /// Starts the ticket's Claude session in herdr instead of in Orbit.
    HerdrStart(String),
    /// Copies the user's herdr config into Orbit's settings: theme,
    /// notifications, sounds, sidebar; its keys apply while `herdr` is on.
    UseHerdrConfig,
    /// Turns the herdr keys (prefix mode) on or off.
    SetHerdrKeys(bool),
    /// Writes Orbit's herdr plugin and links it into herdr.
    InstallHerdrPlugin,
    /// Sounds on or off (`settings.sound`).
    SetSound(bool),
    /// Opens the palette on this query (the shortcut list, from its line).
    PaletteQuery(String),
    /// Opens find on the screen (Cmd+F), or gives it the keyboard again.
    Find,
    ReadNote,
    CreateTicket,
    /// Adds the report to the duplicate at this index instead of filing it.
    AddToDuplicate(usize),
    CloseCapture,
    /// Starts the goals run (goals.rs): what the week is about, from what
    /// the user and their leads wrote down.
    FindGoals,
    /// Starts a fresh orchestrator run in the background, unless one is
    /// still working.
    Orchestrate,
    /// Opens or closes the clone's chat on the right (Cmd+J).
    ToggleChat,
    /// A new chat with the clone, shown in the chat panel.
    NewChat,
    /// Shows this chat (or clone run) in the chat panel.
    ChatShow(String),
    /// A clone session in a window of its own, marked `CLONE ·` in the sidebar.
    CloneWindow(String),
    /// Has the clone drive this parent ticket's sub-tickets through the
    /// team's bot, `parallel` at a time (mission.rs). The user's word for
    /// every move it makes on them.
    Drive {
        parent: String,
        parallel: usize,
        /// The initiative whose tickets these are (filed by its mission).
        #[serde(default)]
        initiative: Option<String>,
        /// Built here in Orbit sessions the clone runs: nothing goes to the
        /// bot, Orbit only follows and lists each sub-ticket's step.
        #[serde(default)]
        local: bool,
        /// The session that asked (`orbit ctl` reads `ORBIT_SESSION_KEY`):
        /// it owns the initiative, the card opens it to steer.
        #[serde(default)]
        session: Option<String>,
    },
    /// Opens the session that steers this initiative, by id: the one that
    /// drives it, else a fresh one with its log.
    Steer(String),
    /// Stops driving this parent ticket.
    StopDrive(String),
    /// Tells the clone what to do, in the user's words: a Claude run that
    /// researches, files the tickets and hands the parent to a mission.
    Instruct(String),
    /// Starts the operator (claude::operator): one Claude run that works
    /// Orbit through `orbit ctl` and says what it did.
    Operate,
    /// The hourly pass (clone panel): on starts one operator session whose
    /// /loop runs the operate skill every hour on the jobs picked; off
    /// stops it.
    Clone(bool),
    /// Picks or drops one of the hourly pass's jobs (claude::PASS_TASKS).
    PassTask(String, bool),
    /// Switches the autopilot (autopilot.rs) on or off: Run the board every
    /// hour, off by itself after eight runs.
    Autopilot(bool),
    /// Feedback on the app itself (Cmd+Shift+F): a fresh session with the
    /// orbit-improve skill in a worktree of the app's repo, which opens a PR.
    Improve,
    /// Starts a session with no ticket and shows it in the focused pane.
    NewSession {
        name: String,
        folder: Option<String>,
    },
    /// Splits the focused pane: to the right (`across`) or below.
    Split {
        across: bool,
    },
    /// Closes a window of the strip with all its panes (its × or a middle
    /// click); sessions keep running, shells end. The last one empties.
    CloseWindow(usize),
    /// Cmd+Z: the last closed window back where it was (its history keeps 20).
    ReopenWindow,
    /// Closes a pane (`None`: the focused one).
    ClosePane(Option<u64>),
    FocusPane(u64),
    CyclePane(i32),
    /// A shell in a new pane to the right.
    NewShell,
    /// A shell in a new window of the strip, or in the empty pane focused.
    ShellWindow,
    /// A plain text editor, in the empty pane focused or a new window.
    EditorWindow,
    /// Writes this editor pane's text to its path.
    SaveEditor(u64),
    /// Another copy of the app, in its own window.
    NewWindow,
    /// incident.io: save the pasted key, or forget it.
    /// Saves the Settings editor's text as settings.json and applies it.
    SaveSettings,
    /// Changes the agent used for new local sessions.
    SetLocalAgent(crate::claude::Agent),
    /// Puts the editor back to the file as it is.
    RevertSettings,
    /// Downloads Laya's checkpoint into the data folder (laya.rs).
    DownloadLaya,
    /// Installs the newer release in `App::update` and relaunches.
    Update,
    /// Uses the profile at `profile_input`: a folder in place, or a bundle
    /// (a `.json` file or URL) fetched and unpacked (profile.rs).
    ImportProfile,
    /// Back to no profile: the generic app.
    ClearProfile,
    /// Uses a profile this build carries, by name (profiles/ in the repo).
    UseProfile(String),
    /// Shows a file in Finder.
    Reveal(std::path::PathBuf),
    /// The local activity log on or off (activity.rs).
    SetActivityLog(bool),
    /// Saves the profile editor's text as profile.json and puts it in use.
    SaveProfile,
    RevertProfile,
    /// Saves Settings' bot & PRs fields: profile ones to the edits file
    /// over the profile (profile.rs `edits_file`), the test app to settings.json.
    SaveBotSetup,
    RevertBotSetup,
    /// A Claude session that asks the user and writes the profile (first
    /// run after the teams, Settings, or new skills on For you); saving
    /// profile.json ends it and opens Settings (settings.rs `follow_onboarding`).
    SetUpProfile,
    /// Sets settings.json `laya.mode` (`off`, `shadow`, `use`); an on mode
    /// downloads the checkpoint first if it is not here.
    #[serde(skip)]
    SetLayaMode(String),
    /// Plan my day: starts the spotter (the spot skill), in a mode from
    /// settings.json `work.modes` or none; it ranks the day by the week's
    /// goals and writes spots.json. Finds the goals first when there are
    /// none for this week.
    SpotWork(Option<String>),
    /// Fix now from the New ticket window: a local session does the note.
    FixNote,
    /// Files the note as written, no AI reading (works without a key).
    FileAsIs,
    /// A Claude session on the incident with this id, the investigate panes beside it.
    Investigate(String),
    /// A Claude session on the alert with this id: `debug` starts it with
    /// the profile's alert skill, else as an investigation with its panes.
    InvestigateAlert {
        id: String,
        debug: bool,
    },
    /// Fix it on a red release: a Claude session on that failed run, by run id.
    FixRelease(u64),
    /// Land approved on KTLO's batch review: these PRs (url, head seen).
    /// Each green one merges, one whose checks still run merges when ready,
    /// the rest get one session with a goal to make them mergeable.
    LandBatch(Vec<(String, String)>),
    /// Debugging an environment (`prod`, `dev`): k9s and Claude Code side
    /// by side; k9s after `assume <env>`, Claude started plain.
    Debug(String),
    ZoomPane,
    /// The focused pane alone, window full screen: no sidebar, bar or clone panel.
    FullScreen,
    PaneRatio(u64, f32),
    /// The embedded browser: open an address, go back, reload.
    /// `slot`: browser::TESTING, or a browser pane's id.
    BrowserGo(u64, String),
    BrowserBack(u64),
    BrowserReload(u64),
    /// Cmd+F in a browser: the find bar over its page.
    BrowserFindOpen(u64),
    BrowserFindClose(u64),
    /// The next (or previous) match of the text in the page.
    BrowserFind {
        slot: u64,
        text: String,
        backwards: bool,
    },
    /// A browser in the focused empty pane (Cmd+N, then B), else a new tab.
    BrowserWindow,
    /// Fetches an image a description shows, once (asked every frame until
    /// it is there, so not logged).
    LoadImage(String),
    /// Goes to a tutorial step, or ends the tutorial (`None`).
    Tutorial(Option<usize>),
    /// Opens an Inbox row, by key: its ticket in a new window, else its
    /// link; its notifications are marked read on both sides.
    InboxOpen(String),
    /// A PR with no ticket, as a card of its own in a new window
    /// (`story::pr_card`); `at` is when it last changed.
    OpenPr {
        url: String,
        title: String,
        at: String,
    },
    /// Marks an Inbox row's notifications read without opening it (x).
    InboxDone(String),
    /// Takes a ticket off For you until it changes (x).
    MarkRead(String),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub enum View {
    ForYou,
    Intake,
    Sessions,
    Customers,
    Customer(String),
    Settings,
    /// KTLO: security findings and alerts, kept out of product work.
    Ktlo,
    /// Linear's and GitHub's notifications in one list (src/inbox.rs).
    Inbox,
    /// incident.io's alerts of the last day, firing first (src/incidents.rs).
    Alerts,
    /// The user's week in Orbit (stats.rs) beside the team's numbers from
    /// SigNoz (signoz.rs), the 3 tab.
    Stats,
    /// Every repository the user's work touches (`App::repos`), one row
    /// each, the way a browser's bookmarks bar holds them.
    Repos,
    /// One repository's page: its open PRs, who waits on whom, each
    /// opening here as a PR card (github.rs `repo_prs`).
    Repo(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Tab {
    Walkthrough,
    /// The selected ticket's PR diff.
    Diff,
    Claude,
    /// The Intake view's own triage session; not tied to any ticket.
    Triage,
    /// How to test a ticket in Testing, step by step (src/testplan.rs).
    Testing,
}

/// Find on the screen (Cmd+F, ui/find.rs): what is typed, which match is
/// current, and where to scroll before the next frame draws.
#[derive(Default)]
pub struct Find {
    pub query: String,
    pub current: usize,
    pub jump: Option<egui::Rect>,
    pub focus: bool,
    /// How many matches the last frame found.
    pub matches: usize,
}

/// The command palette's state: what is typed, which result is picked, and
/// when the query last changed, so Linear is searched once typing pauses.
/// One in-app notification line (`App::toasts`).
#[derive(Clone, Debug)]
pub struct Toast {
    /// notify.rs's key: what it is about, and so where a click goes.
    pub key: String,
    pub line: String,
    pub at: std::time::Instant,
    /// What a program did through `orbit ctl` (app/control.rs), to follow
    /// along: drawn quieter than a toast that needs the user.
    pub agent: bool,
}

pub struct Palette {
    pub query: String,
    pub selected: usize,
    /// Tab on a repo's name: only the user's open PRs in it.
    pub repo: Option<String>,
    last: String,
    typed_at: std::time::Instant,
    searched: String,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            query: String::new(),
            selected: 0,
            repo: None,
            last: String::new(),
            typed_at: std::time::Instant::now(),
            searched: String::new(),
        }
    }
}

/// The New ticket window's state. The text fields borrow from it directly.
#[derive(Default)]
pub struct Capture {
    pub note: String,
    /// What Haiku read, once it has; the answers line up with its questions.
    pub intake: Option<crate::capture::Intake>,
    pub answers: Vec<String>,
    pub busy: bool,
    pub error: Option<String>,
    /// Open tickets that look like the same report; nothing is filed on its
    /// own while there are any.
    pub duplicates: Vec<crate::capture::Duplicate>,
    /// `(identifier, url)` once created, or once the report was added to an
    /// existing ticket (`added`).
    pub created: Option<(String, String)>,
    pub added: bool,
    /// Filled by a Work spot: its description, used as the ticket's as is,
    /// and its id, marked started once the ticket exists.
    pub description: Option<String>,
    pub spot: Option<String>,
    /// The team File as-is files into (no Anthropic key); empty: the
    /// default team (`App::default_team`).
    pub team: String,
}

/// A cached Claude result with the hash of the text it was made from, so
/// an edited source gets a new one.
type Cached<T> = HashMap<String, (u64, T)>;

/// A closed window: where it was, its tree, its shells' claude (pane id
/// to session id and folder) to resume.
pub type ClosedWindow = (
    usize,
    panes::Panes,
    std::collections::BTreeMap<u64, (String, String)>,
);

pub struct App {
    backend: Option<Backend>,
    /// The theme picked in Settings.
    pub theme: crate::theme::Choice,
    /// The tutorial's current step (src/ui/tutorial.rs), and the view to
    /// return to when it ends.
    pub tutorial: Option<usize>,
    tutorial_return: Option<(View, bool)>,
    pub key_input: String,
    pub viewer: String,
    /// Linear's last answer, before the optimistic layer.
    server: Vec<Issue>,
    /// Tickets not on the board that a link or a notification opened:
    /// fetched once and kept beside the board's own.
    fetched: Vec<Issue>,
    pub cards: Vec<Card>,
    pub states: HashMap<String, Vec<State>>,
    /// Every team in the workspace, `(id, key, name)`, key order: where a
    /// ticket can move.
    pub all_teams: Vec<(String, String, String)>,
    /// The sheet's view, and whether the sheet is open over the panes, with
    /// its cursor as (column, row).
    pub view: View,
    pub sheet: bool,
    pub sheet_at: (usize, usize),
    /// What the sheet's search field holds (Sessions, Repos, a repo's PRs):
    /// the list and the cursor's entries both filter by it. Cleared when
    /// the sheet moves to another view.
    pub sheet_query: String,
    pub tab: Tab,
    pub claude: crate::claude::Sessions,
    /// The session the Claude tab shows: a ticket id, or the triage key.
    pub session: Option<String>,
    /// The PR the Diff tab shows, by URL; `None`, or one not on the
    /// ticket, shows its lead PR (`Card::pr`).
    pub diff_pr: Option<String>,
    pub teams: Vec<String>,
    /// The viewer's own teams, `(key, name)`, to choose from.
    pub my_teams: Vec<(String, String)>,
    /// The picker is open, with this selection.
    pub picking: Option<Vec<String>>,
    pub selected: Option<String>,
    pub draft: String,
    pub error: Option<String>,
    pub loading: bool,
    pub loaded_at: Option<String>,
    /// Linear accepted the key: the first board load succeeded.
    pub verified: bool,
    /// Why the last key was not accepted, shown on the login screen.
    pub sign_in_error: Option<String>,
    /// A pasted key, kept until Linear accepts it and it is stored.
    pending_key: Option<String>,
    /// The browser sign-in's answer, while it runs.
    oauth: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    github_token: Option<String>,
    /// Keys being typed in Settings, by `secrets::name`.
    pub key_drafts: std::collections::BTreeMap<&'static str, String>,
    /// Diffs by PR URL, and the ones asked for; absent from both until a
    /// Diff tab shows the PR (`load_diffs`).
    pub diffs: HashMap<String, Result<Vec<crate::github::File>, String>>,
    /// What people said on each PR shown (github::talk), fetched with the diff.
    pub talk: HashMap<String, Result<crate::github::Talk, String>>,
    /// The Diff tab in one column instead of side by side.
    pub unified_diff: bool,
    /// KTLO's batch review shows each PR's diff under its row.
    pub diff_all: bool,
    /// Folders folded in a diff's file tree, as (PR url, folder path); their
    /// files leave the diff. ponytail: in memory, persist if folds should
    /// survive a restart.
    pub folded: HashSet<(String, String)>,
    /// Diff files marked viewed (`ui::diff::viewed_key`), in viewed.json,
    /// shared by every copy (read-modify-write). ponytail: never pruned.
    pub viewed: HashSet<String>,
    /// Diff files folded or opened by hand against their viewed mark.
    pub folded_files: HashSet<(String, String)>,
    /// GitHub logins by Linear name (`None`: not found, or not one), and
    /// the names being looked up.
    pub logins: HashMap<String, Option<String>>,
    logins_pending: HashSet<String>,
    /// `(PR URL, login)` asked to review here, shown at once.
    pub review_requested: HashSet<(String, String)>,
    /// Labels put on PRs from here (url, label), shown at once.
    pub labeled: HashSet<(String, String)>,
    /// PRs whose review the user handed to someone: url -> who
    /// ("@ana", "review roulette"), in handed.json (read-modify-write).
    pub handed: HashMap<String, String>,
    diff_pending: HashSet<String>,
    /// Merge gates by PR URL, asked on select and every refresh.
    pub gates: HashMap<String, Result<crate::github::Gate, String>>,
    /// PRs being merged, and the merge SHA once done, by PR URL.
    pub merging: HashSet<String>,
    pub merged: HashMap<String, String>,
    /// PRs the user closed here: closed at once, whatever Linear still says.
    pub closed: HashSet<String>,
    /// The head each Merge when ready was turned on at, by PR URL: a gate
    /// with another head turns it off (events.rs). This copy's only.
    pub auto_merge_heads: HashMap<String, String>,
    /// PRs whose approval is on its way to GitHub.
    pub approving: HashSet<String>,
    /// PRs whose branch update is on its way, by the head it was asked at:
    /// held until GitHub shows a new head.
    pub updating: HashMap<String, String>,
    /// AI reviews asked for by PR URL, at the head they were asked at, and
    /// the ones on their way. New commits since make it a re-request.
    pub ai_reviews: HashMap<String, String>,
    pub ai_asking: HashSet<String>,
    /// Review comments drafted per PR URL, posted together on Comment.
    /// Local to this copy: nothing is on GitHub until the user posts.
    pub drafts: HashMap<String, Vec<crate::github::Draft>>,
    /// The head commit each loaded diff shows, from the first gate answer
    /// after it arrived: what Approve and Comment are bound to.
    pub diff_heads: HashMap<String, String>,
    /// PRs whose walkthrough starts as soon as their diff is back.
    guide_after: HashSet<String>,
    /// The last review the user posted per PR URL (`reviewed.json`), so
    /// new commits after it are noticed, across restarts too.
    pub reviewed: HashMap<String, Reviewed>,
    /// PRs whose review is on its way to GitHub.
    pub reviewing: HashSet<String>,
    /// Laya's quick answers per issue id, with the hash of the text they
    /// answer (asked again only when it changes), and the ones on the way.
    pub quick: HashMap<String, (u64, Result<Vec<crate::laya::Proposal>, String>)>,
    /// Laya on each session's last message, by session key: `(text hash,
    /// waits on the user, confidence)`; and whether an ask is out.
    pub session_asks: HashMap<String, (u64, bool, f64)>,
    asks_pending: bool,
    pub quick_pending: HashSet<String>,
    /// Per quick question: `(id, asked, shown, clicked)` (Settings).
    pub quick_stats: Vec<(&'static str, usize, usize, usize)>,
    /// Walkthroughs by PR URL, made only when asked for.
    pub guides: HashMap<String, Result<crate::anthropic::Guide, String>>,
    pub guide_pending: HashSet<String>,
    /// The reviewer's own questions per PR and Claude's answers, `None`
    /// while one is being answered; kept for the session, never posted.
    pub pr_questions: HashMap<String, Vec<Asked>>,
    /// The note that goes with the user's approval, per PR: `None` while
    /// Claude writes it.
    pub approve_notes: HashMap<String, Option<Result<String, String>>>,
    /// The walkthrough step on screen and the PR it is of; `None`, or
    /// another PR's step, shows every file.
    pub guide_step: Option<(String, usize)>,
    /// Test plans by issue id, and the ones being fetched or written.
    pub test_plans: HashMap<String, Result<crate::testplan::TestPlan, String>>,
    pub test_plan_pending: HashSet<String>,
    /// The current window's split panes (panes.rs), and every window of the
    /// strip; `windows[window]` is stale while its tree is in `panes`. The
    /// shells in them by pane id.
    pub panes: panes::Panes,
    pub windows: Vec<panes::Panes>,
    pub window: usize,
    /// windows.json as this copy last wrote it (`App::save_windows`).
    windows_saved: String,
    /// When windows were last checked for a name to ask Haiku for.
    named_at: Option<std::time::Instant>,
    pub shells: HashMap<u64, crate::claude::Terminal>,
    /// Each editor pane's text and path (editor.rs), under the pane id.
    pub editors: HashMap<u64, crate::editor::Editor>,
    /// What a shell pane is for, for its header ("k9s · prod"); plain
    /// shells have none.
    pub shell_titles: HashMap<u64, String>,
    /// What Claude Code started by hand in a shell pane is doing, read off
    /// its screen (no hooks there): the sidebar's state for that pane.
    pub shell_states: HashMap<u64, &'static str>,
    /// When the shell panes' folders were last handed to
    /// `claude::trust_shell_folders` (every two seconds).
    shell_trusted: Option<std::time::Instant>,
    /// The command last posted for the bot, by issue id, for the line under
    /// the ticket's buttons.
    /// A command left for the bot, and when (a jiff timestamp).
    pub asked_bot: HashMap<String, (crate::story::BotCommand, String)>,
    /// For you cards marked read: issue id -> `story::read_key` then
    /// (`read.json`, read-modify-write). A card whose key moved shows again.
    pub read: HashMap<String, u64>,
    /// The New ticket window, while open.
    pub capture: Option<Capture>,
    /// The spotter's last answer (work.rs), the spots already started, and
    /// when its file was last looked at.
    pub spots: Option<Result<crate::work::Spots, String>>,
    pub spots_started: Vec<crate::work::Started>,
    spots_seen: store::Watch,
    /// settings.json as last read or saved, the editor's text, why the last
    /// save failed, and when the file was last looked at (settings.rs).
    pub settings: Settings,
    pub settings_text: String,
    settings_loaded: String,
    pub settings_error: Option<String>,
    /// laya-shadow.jsonl as `(compared, agreed)`, re-read with the settings;
    /// whether Laya's checkpoint is here, and its download while it runs
    /// (`(done, total)` bytes) or why it failed.
    pub laya_agreement: (usize, usize),
    pub laya_ready: bool,
    /// Whether the error on screen was logged already (activity.rs).
    logged_error: bool,
    /// Why the profile in settings.json could not be read, and a bundle
    /// import on its way (profile.rs).
    pub profile_error: Option<String>,
    pub profile_importing: bool,
    pub profile_input: String,
    /// The profile editor (settings.rs): its text, the file as last read,
    /// why the last save failed, and the file's time when last looked at.
    pub profile_text: String,
    profile_loaded: String,
    /// Settings' bot & PRs fields as typed (profile.rs `EDIT_FIELDS` path,
    /// or `testAppUrl`), and why the last save failed.
    pub bot_drafts: std::collections::BTreeMap<String, String>,
    pub bot_save_error: Option<String>,
    pub profile_save_error: Option<String>,
    profile_seen: store::Watch,
    /// profile.json as it was when the profile session started.
    pub onboarding: Option<store::Watch>,
    pub laya_download: Option<Result<(u64, u64), String>>,
    /// A newer release's tag (update.rs), whether it is being installed,
    /// and why the last check or install failed. Checked hourly.
    pub update: Option<String>,
    pub installing: bool,
    pub update_error: Option<String>,
    update_at: Option<std::time::Instant>,
    /// Laya is on but not here: downloaded once per run on its own.
    laya_fetched: bool,
    settings_seen: store::Watch,
    laya_seen: store::Watch,
    quick_seen: store::Watch,
    /// incident.io (src/incidents.rs): the key, the live incidents, and when
    /// they were last asked for.
    pub incident_key: Option<String>,
    pub incidents: Vec<crate::incidents::Incident>,
    /// Images from ticket descriptions by URL, decoded, and the ones asked for.
    pub images: HashMap<String, Result<egui::TextureHandle, String>>,
    images_asked: HashSet<String>,
    pub incidents_error: Option<String>,
    pub alerts: Vec<crate::incidents::Alert>,
    pub alerts_error: Option<String>,
    /// Each watched release workflow with its latest run (releases.rs).
    pub releases: Vec<(
        crate::releases::Release,
        Result<Option<crate::releases::Run>, String>,
    )>,
    releases_at: Option<std::time::Instant>,
    /// When the open PRs every window shows last had their gates asked.
    window_gates_at: Option<std::time::Instant>,
    /// PRs whose gate KTLO's batch review asked for once.
    gates_asked: HashSet<String>,
    incidents_at: Option<std::time::Instant>,
    /// The Inbox (src/inbox.rs): both sides' last notes, how many each left
    /// out, whether GitHub and anyone answered, why a side failed, the notes marked
    /// read here (hidden until a side stops sending them), when last asked.
    pub inbox_notes: Vec<crate::inbox::Note>,
    pub inbox_skipped: usize,
    pub inbox_github: bool,
    pub inbox_answered: bool,
    pub inbox_error: Option<String>,
    pub inbox_done: HashSet<String>,
    /// PRs opened from the Inbox that no ticket has (`story::pr_card`):
    /// found by `card` like a ticket, listed in no view. Until restart.
    /// Dependabot's (lane `Dependencies`) are listed on KTLO.
    pub pr_cards: Vec<Card>,
    inbox_at: Option<std::time::Instant>,
    dependabot_at: Option<std::time::Instant>,
    /// The command palette, while open, and what Linear's search found for
    /// its query (palette.rs).
    pub palette: Option<Palette>,
    pub find: Option<Find>,
    pub palette_found: Vec<crate::capture::Duplicate>,
    /// The user's open PRs, asked each time the palette opens.
    pub my_prs: Vec<crate::github::MyPr>,
    /// Each repo page's open PRs (`Command::RepoPrs`), with when they
    /// came; asked again when shown after a minute.
    pub repo_prs: HashMap<
        String,
        (
            std::time::Instant,
            Result<Vec<crate::github::RepoPr>, String>,
        ),
    >,
    /// The Stats view's team numbers and when they came (signoz.rs).
    pub team_stats: Option<(std::time::Instant, Result<crate::signoz::Team, String>)>,
    /// herdr is on the user's PATH (herdr.rs); its agents, asked every few
    /// seconds while it is; its config, read at start and on "Use".
    pub herdr_bin: bool,
    pub herdr_agents: Vec<crate::herdr::Agent>,
    herdr_asked: Option<std::time::Instant>,
    /// When the next cleanup pass runs (cleanup.rs): 2 minutes after
    /// start, then daily, off the UI thread.
    cleanup_due: std::time::Instant,
    /// When idle background sessions are looked at next (claude/idle.rs).
    idle_due: std::time::Instant,
    /// The last pass's line for Settings ("storage: … · freed … on …").
    pub cleanup_note: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    /// The PRs the user's Land click covers, by URL, with the head they saw
    /// (`Action::LandBatch`): queued again whenever GitHub says mergeable
    /// at that head, after a re-run or a drop from the merge queue.
    // ponytail: in memory; a restart forgets the batch, a file if that bites.
    pub landing: HashMap<String, String>,
    landing_at: Option<std::time::Instant>,
    /// The parents the clone drives (mission.rs), as last read, and when.
    pub missions: Vec<crate::mission::Initiative>,
    missions_at: Option<std::time::Instant>,
    /// What the user ran from the palette, how often and when (palette.rs).
    pub palette_picks: palette::Picks,
    /// The clone's chat panel on the right: open, and the chat it shows.
    pub chat_open: bool,
    /// The focused pane fills the window (`Action::FullScreen`).
    pub full_screen: bool,
    pub chat: Option<String>,
    /// Orbit's window has the focus (as of the last tick).
    front: bool,
    /// When the bot's poller was last run (an initiative's kick).
    kicked_at: Option<std::time::Instant>,
    /// Sub-tickets handed to the bot whose move Linear has not shown yet.
    pub dispatching: HashSet<String>,
    /// Who may review, by `owner/repo` (Request review), asked once a run.
    pub reviewers: HashMap<String, Result<Vec<(String, String)>, String>>,
    reviewers_asked: HashSet<String>,
    /// `orbit ctl` requests (src/control.rs), when this copy serves the socket.
    control: Option<std::sync::mpsc::Receiver<crate::control::Request>>,
    /// Tickets off the board looked up for their state (`orbit ctl
    /// sessions`), by the id or identifier asked; asked once each.
    pub looked_up: HashMap<String, Result<Issue, String>>,
    lookups_asked: HashSet<String>,
    pub herdr_config: Option<crate::herdr::Config>,
    /// What the last herdr action said (started, linked, or why not).
    pub herdr_note: Option<String>,
    /// The needs-you lines last written for herdr's plugin pane.
    needs_written: String,
    /// The menu bar icon, while settings.json `menuBar` is not false.
    #[cfg(target_os = "macos")]
    tray: Option<crate::tray::Tray>,
    /// What the Mac would be told, shown inside the app too (notify.rs):
    /// a few lines bottom right, each a click away from what it names,
    /// gone after ten seconds or a click on its ×.
    pub toasts: Vec<Toast>,
    /// Closed windows, newest last: where each was, its tree, its shells'
    /// claude to resume. Cmd+Z reopens the newest.
    pub closed_windows: Vec<ClosedWindow>,
    /// The agent's last `say` line: the same line again is not news.
    last_said: String,
    /// The query Linear last answered for the palette, and its error if it
    /// failed: "searching" shows until it names the current query.
    pub palette_answered: (String, Option<String>),
    /// The goals run's last answer and when its file was last looked at;
    /// `plan_after_goals`: Plan my day (in this mode) waits for the run it
    /// started.
    pub goals: Option<Result<crate::goals::Goals, String>>,
    goals_seen: store::Watch,
    pub plan_after_goals: Option<Option<String>>,
    /// The autopilot as last read from autopilot.json, and when.
    pub autopilot: crate::autopilot::Autopilot,
    autopilot_at: Option<std::time::Instant>,
    /// When what needs the user was last compared with notified.json.
    notified_at: Option<std::time::Instant>,
    /// The keys open at the last check (`notify::stable`): a line is said
    /// only once it held for two checks.
    notify_previous: Option<Vec<String>>,
    /// The day Plan my day last started the run itself: once a day, so a
    /// run that finds nothing does not start again on every plan.
    goals_tried: Option<String>,
    /// The Testing tab's embedded browser (src/browser.rs).
    pub browser: crate::browser::Browser,
    /// Each browser pane's own, by pane id.
    pub browsers: HashMap<u64, crate::browser::Browser>,
    anthropic_key: Option<String>,

    /// The "Right now" line, and the board fingerprint it describes.
    pub brief: Option<String>,
    brief_for: Option<u64>,
    pub brief_pending: bool,
    brief_asked: Option<std::time::Instant>,
    pub brief_error: Option<String>,
    /// Short names by issue id or `story:<key>`.
    short: Cached<String>,
    shorten_pending: bool,
    shorten_tried: HashSet<String>,
    /// Customers Claude read from tickets with no Linear customer request.
    extracted: Cached<Vec<String>>,
    customers_pending: bool,
    customers_tried: HashSet<String>,
    /// One line on what is going on, per `customer:<name>` and `story:<key>`.
    digests: Cached<String>,
    digests_pending: bool,
    shown: HashMap<String, Shown>,
    locals: Vec<Local>,
    /// When Linear was last asked, and which session reports the board
    /// already reflects; both drive the automatic refresh.
    refreshed: Option<std::time::Instant>,
    status_seen: u64,
    last_request: u64,
    applied_request: u64,
    next_temp: u64,
}

/// Laya's quick actions are for a ticket before any PR: only these are
/// asked about (App::load_quick) and show them (ui/ticket.rs).
/// The person who filed the ticket, when that is someone to ask for a
/// review: not the user, not a bot, and the ticket has an open PR in review.
pub fn reporter<'a>(card: &'a Card, viewer: &str) -> Option<&'a str> {
    let name = card.issue.creator.as_ref()?.name.as_str();
    let person = !name.is_empty()
        && name != viewer
        && crate::story::who(name, false, false) == crate::story::Who::Other;
    let review = card.stage == crate::story::Stage::Review && card.prs.iter().any(|p| p.open);
    (person && review && !card.pr_only()).then_some(name)
}

pub fn quick_fits(card: &Card) -> bool {
    use crate::story::Stage;
    matches!(card.stage, Stage::Intake | Stage::Planning) && card.prs.is_empty()
}

impl App {
    pub fn new(ctx: &egui::Context) -> Self {
        // Demo mode: Acme's sessions and initiative, before anything loads.
        crate::demo::seed();
        let named = store::load::<Settings>("settings.json").profile;
        let profile_error = crate::profile::load(&named)
            .map(crate::profile::set)
            .err()
            .map(|e| format!("{e:#}"));
        // Now over the profile's settings.
        let settings = store::load_settings();
        // Before the first frame, so the window never flashes another theme.
        crate::theme::fonts(ctx);
        crate::theme::follow(ctx, settings.theme);
        let (view, sheet) = review::start_view();
        let mut app = Self {
            backend: None,
            theme: settings.theme,
            tutorial: None,
            tutorial_return: None,
            key_input: String::new(),
            viewer: String::new(),
            server: Vec::new(),
            fetched: Vec::new(),
            cards: Vec::new(),
            states: HashMap::new(),
            all_teams: Vec::new(),
            view,
            sheet,
            sheet_at: (0, 0),
            sheet_query: String::new(),
            tab: Tab::Walkthrough,
            claude: crate::claude::Sessions::load(),
            session: None,
            diff_pr: None,
            teams: teams_from_env().unwrap_or_else(|| settings.teams.clone()),
            my_teams: Vec::new(),
            picking: None,
            selected: None,
            draft: String::new(),
            error: None,
            loading: false,
            loaded_at: None,
            verified: false,
            sign_in_error: None,
            pending_key: None,
            oauth: None,
            github_token: secrets::get(Secret::Github),
            key_drafts: Default::default(),
            diffs: HashMap::new(),
            talk: HashMap::new(),
            unified_diff: false,
            diff_all: false,
            folded: HashSet::new(),
            viewed: store::load::<Vec<String>>("viewed.json")
                .into_iter()
                .collect(),
            folded_files: HashSet::new(),
            logins: HashMap::new(),
            logins_pending: HashSet::new(),
            review_requested: HashSet::new(),
            labeled: HashSet::new(),
            handed: store::load("handed.json"),
            diff_pending: HashSet::new(),
            gates: HashMap::new(),
            merging: HashSet::new(),
            approving: HashSet::new(),
            updating: HashMap::new(),
            ai_reviews: HashMap::new(),
            ai_asking: HashSet::new(),
            drafts: HashMap::new(),
            diff_heads: HashMap::new(),
            guide_after: HashSet::new(),
            reviewed: store::load(REVIEWED),
            reviewing: HashSet::new(),
            quick: HashMap::new(),
            session_asks: HashMap::new(),
            asks_pending: false,
            quick_pending: HashSet::new(),
            quick_stats: Vec::new(),
            merged: HashMap::new(),
            closed: HashSet::new(),
            guides: HashMap::new(),
            guide_pending: HashSet::new(),
            pr_questions: HashMap::new(),
            approve_notes: HashMap::new(),
            guide_step: None,
            test_plans: HashMap::new(),
            test_plan_pending: HashSet::new(),
            browser: crate::browser::Browser::new(crate::browser::TESTING),
            browsers: HashMap::new(),
            panes: Default::default(),
            windows: vec![Default::default()],
            window: 0,
            windows_saved: String::new(),
            named_at: None,
            shells: HashMap::new(),
            editors: HashMap::new(),
            shell_titles: HashMap::new(),
            shell_states: HashMap::new(),
            shell_trusted: None,
            anthropic_key: secrets::get(Secret::Anthropic),

            brief: None,
            brief_for: None,
            brief_pending: false,
            brief_asked: None,
            brief_error: None,
            short: store::load("short-names.json"),
            shorten_pending: false,
            shorten_tried: HashSet::new(),
            extracted: store::load("customers.json"),
            customers_pending: false,
            customers_tried: HashSet::new(),
            digests: store::load("digests.json"),
            goals: None,
            goals_seen: Default::default(),
            plan_after_goals: None,
            autopilot: crate::autopilot::load(),
            autopilot_at: None,
            notified_at: None,
            notify_previous: None,
            goals_tried: None,
            capture: None,
            find: None,
            palette: None,
            incident_key: secrets::get(Secret::IncidentIo),
            spots: None,
            spots_started: Vec::new(),
            spots_seen: Default::default(),
            settings: settings.clone(),
            settings_text: String::new(),
            settings_loaded: String::new(),
            settings_error: None,
            laya_agreement: (0, 0),
            laya_ready: crate::laya::downloaded(&crate::laya::dir()),
            logged_error: false,
            profile_error,
            profile_importing: false,
            profile_input: String::new(),
            profile_text: String::new(),
            profile_loaded: String::new(),
            bot_drafts: Default::default(),
            bot_save_error: None,
            profile_save_error: None,
            profile_seen: Default::default(),
            onboarding: None,
            laya_download: None,
            laya_fetched: false,
            settings_seen: Default::default(),
            laya_seen: Default::default(),
            quick_seen: Default::default(),
            incidents: Vec::new(),
            images: HashMap::new(),
            images_asked: HashSet::new(),
            incidents_error: None,
            alerts: Vec::new(),
            alerts_error: None,
            releases: Vec::new(),
            releases_at: None,
            window_gates_at: None,
            gates_asked: HashSet::new(),
            incidents_at: None,
            inbox_notes: Vec::new(),
            inbox_skipped: 0,
            inbox_github: false,
            inbox_answered: false,
            inbox_error: None,
            inbox_done: HashSet::new(),
            pr_cards: Vec::new(),
            auto_merge_heads: HashMap::new(),
            inbox_at: None,
            dependabot_at: None,
            update: None,
            installing: false,
            update_error: None,
            update_at: None,
            palette_found: Vec::new(),
            my_prs: Vec::new(),
            repo_prs: HashMap::new(),
            team_stats: None,
            herdr_bin: crate::herdr::bin().is_some(),
            herdr_agents: Vec::new(),
            herdr_asked: None,
            cleanup_due: std::time::Instant::now() + std::time::Duration::from_secs(120),
            idle_due: std::time::Instant::now() + std::time::Duration::from_secs(120),
            cleanup_note: std::sync::Arc::new(std::sync::Mutex::new(crate::cleanup::note(
                &crate::store::data_dir(),
            ))),
            control: crate::control::serve(ctx.clone()),
            reviewers: HashMap::new(),
            missions: crate::mission::load(),
            missions_at: None,
            dispatching: HashSet::new(),
            kicked_at: None,
            front: false,
            chat_open: false,
            full_screen: false,
            chat: None,
            palette_picks: palette::load_picks(),
            landing: HashMap::new(),
            landing_at: None,
            reviewers_asked: HashSet::new(),
            looked_up: HashMap::new(),
            lookups_asked: HashSet::new(),
            herdr_config: crate::herdr::config(),
            herdr_note: None,
            needs_written: String::new(),
            #[cfg(target_os = "macos")]
            tray: None,
            toasts: Vec::new(),
            closed_windows: Vec::new(),
            last_said: String::new(),
            palette_answered: Default::default(),
            asked_bot: HashMap::new(),
            read: crate::store::load("read.json"),
            digests_pending: false,
            shown: HashMap::new(),
            locals: Vec::new(),
            refreshed: None,
            status_seen: 0,
            last_request: 0,
            applied_request: 0,
            next_temp: 0,
        };
        app.restore_windows(ctx);
        // A key from the environment or the Keychain signs in by itself;
        // it is already stored, so nothing is saved once it works.
        if let Some(key) = secrets::get(Secret::Linear) {
            app.key_input = key;
            app.apply(Action::SignIn, ctx);
            app.pending_key = None;
        }
        app.log("start", crate::VERSION, "", None);
        app
    }

    pub fn signed_in(&self) -> bool {
        self.verified
    }

    /// The browser sign-in is open; its tokens then sign in like a key.
    fn poll_oauth(&mut self, ctx: &egui::Context) {
        let Some(result) = self.oauth.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.oauth = None;
        match result {
            Ok(tokens) => {
                self.key_input = tokens;
                self.apply(Action::SignIn, ctx);
            }
            Err(error) => self.sign_in_error = Some(error),
        }
    }

    pub fn waiting_for_browser(&self) -> bool {
        self.oauth.is_some()
    }

    /// A key is being checked against Linear.
    pub fn checking(&self) -> bool {
        self.backend.is_some() && !self.verified
    }

    pub fn has_anthropic_key(&self) -> bool {
        self.anthropic_key.is_some()
    }

    pub fn has_github_token(&self) -> bool {
        self.github_token.is_some()
    }

    /// A saved session's name in lists: its identifier and its title, once.
    /// A PR session titled only by its number takes the PR's title from
    /// what the app knows (its card, the user's PRs, its gate), then
    /// Claude's title for the session.
    pub fn session_name(&self, key: &str, saved: &crate::claude::Saved) -> String {
        let id = saved.identifier.as_str();
        let from_key = crate::story::pr_url_of(key);
        let url = self
            .card(key)
            .and_then(|c| c.pr(None))
            .map(|p| p.url.as_str())
            .or(from_key.as_deref());
        let known = [
            Some(saved.title.as_str()),
            self.card(key).map(|c| c.issue.title.as_str()),
            url.and_then(|u| self.my_prs.iter().find(|p| p.url == u))
                .map(|p| p.title.as_str()),
            url.and_then(|u| self.gates.get(u))
                .and_then(|g| g.as_ref().ok())
                .map(|g| g.title.as_str()),
            self.claude
                .about
                .get(&saved.session_id)
                .and_then(|a| a.title.as_deref()),
        ];
        name_of(id, known.into_iter().flatten())
    }

    pub fn card(&self, id: &str) -> Option<&Card> {
        self.cards
            .iter()
            .chain(&self.pr_cards)
            .find(|c| c.issue.id == id)
    }

    /// A card by its id or its identifier (`XYZ-794`), as `orbit ctl` and
    /// the clone name tickets.
    pub fn card_named(&self, name: &str) -> Option<&Card> {
        self.card(name).or_else(|| {
            self.cards
                .iter()
                .chain(&self.pr_cards)
                .find(|c| c.issue.identifier == name)
        })
    }

    /// The PR's status as its row shows it (`Pr::state`).
    pub fn pr_state(&self, pr: &Pr) -> crate::story::PrState {
        if self.closed.contains(&pr.url) && !pr.merged {
            return crate::story::PrState::Closed;
        }
        let gate = self.gates.get(&pr.url).and_then(|g| g.as_ref().ok());
        pr.state(
            gate,
            self.merging.contains(&pr.url),
            self.merged.contains_key(&pr.url),
        )
    }

    fn send(&self, command: Command) {
        if let Some(backend) = &self.backend {
            backend.send(command);
        }
    }

    /// Every skill a session gets (skills.rs), as `(folder, SKILL.md)`: the
    /// app's own, each with the profile's EXTRA.md after it, and the
    /// profile's own skills.
    fn skill_files(&self) -> Vec<(String, String)> {
        let profile = crate::profile::get();
        let mut files: std::collections::BTreeMap<String, String> = crate::skills::GENERIC
            .iter()
            .map(|(folder, body)| ((*folder).to_owned(), (*body).to_owned()))
            .collect();
        let mut extras = Vec::new();
        if let Some(dir) = &profile.dir
            && let Ok(entries) = std::fs::read_dir(dir.join("skills"))
        {
            for entry in entries.flatten() {
                let folder = entry.file_name().to_string_lossy().into_owned();
                if let Ok(body) = std::fs::read_to_string(entry.path().join("SKILL.md")) {
                    files.insert(folder.clone(), body);
                }
                if let Ok(extra) = std::fs::read_to_string(entry.path().join("EXTRA.md")) {
                    extras.push((folder, extra));
                }
            }
        }
        // The company's fine-tuning goes after the skill it refines.
        for (folder, extra) in extras {
            if let Some(body) = files.get_mut(&folder) {
                *body = crate::skills::with_extra(body, &profile.company.name, &extra);
            }
        }
        files.into_iter().collect()
    }

    /// Every skill's folder name but the profile setup's own.
    fn skill_names(&self) -> Vec<String> {
        self.skill_files()
            .into_iter()
            .map(|(folder, _)| folder)
            .filter(|folder| folder != "profile")
            .collect()
    }

    /// Skills that came after the profile was last set up with Claude: an
    /// update of the app, or a skill added to the profile.
    pub fn new_skills(&self) -> Vec<String> {
        unseen(&self.skill_names(), self.settings.profile_skills.as_deref())
    }

    /// Asks Laya, while it is on, whether each session whose turn ended is
    /// waiting on the user, once per version of its last message: it
    /// nudges the Sessions order. Runs every frame, one ask out at a time.
    fn load_session_asks(&mut self) {
        if self.asks_pending || self.settings.laya.on().is_none() || !self.laya_ready {
            return;
        }
        let texts: Vec<(String, String)> = self
            .claude
            .rows
            .values()
            .filter(|row| matches!(row.label(), "your turn" | "idle"))
            .filter_map(|row| {
                let key = self
                    .claude
                    .key_of(&row.session_id)
                    .cloned()
                    .unwrap_or_else(|| format!("sid:{}", row.session_id));
                let text = row
                    .report
                    .as_ref()
                    .map(|r| r.text.clone())
                    .or_else(|| row.activity.as_ref().map(|a| a.last_line.clone()))
                    .filter(|t| !t.trim().is_empty())?;
                let hash = store::text_hash(&text);
                let known = self
                    .session_asks
                    .get(&key)
                    .is_some_and(|(h, ..)| *h == hash);
                (!known).then_some((key, text))
            })
            .collect();
        if !texts.is_empty() {
            self.asks_pending = true;
            self.send(Command::LayaAsks(texts));
        }
    }

    /// Asks Laya's quick questions about the selected ticket while Laya is
    /// on, once per version of its text. Runs every frame.
    fn load_quick(&mut self) {
        if self.settings.laya.on().is_none() || !self.laya_ready {
            return;
        }
        let Some(card) = self.selected.as_deref().and_then(|id| self.card(id)) else {
            return;
        };
        if !quick_fits(card) {
            return;
        }
        let issue = &card.issue;
        let last = issue.comments.nodes.last().map_or("", |c| c.body.as_str());
        let text = crate::laya::ticket_text(
            &issue.title,
            issue.description.as_deref().unwrap_or(""),
            last,
        );
        let hash = store::text_hash(&text);
        let id = issue.id.clone();
        if self.quick.get(&id).is_some_and(|(h, _)| *h == hash) || self.quick_pending.contains(&id)
        {
            return;
        }
        self.quick_pending.insert(id.clone());
        self.send(Command::Quick {
            issue_id: id,
            hash,
            text,
        });
    }

    /// The quick actions to show on this ticket: Laya on in `use` mode,
    /// and each yes at its question's `min` confidence.
    pub fn proposed(&self, issue_id: &str) -> Vec<&'static str> {
        if self.settings.laya.on() != Some(true) {
            return Vec::new();
        }
        let Some((_, Ok(answers))) = self.quick.get(issue_id) else {
            return Vec::new();
        };
        crate::laya::shown(answers)
    }

    /// Asks again whether the selected ticket's open PRs may be merged
    /// (a merged one once: who approved it), and looks up the GitHub login of the person who filed it (for
    /// Request review from them).
    fn load_gate(&mut self) {
        let Some(card) = self.selected.as_deref().and_then(|id| self.card(id)) else {
            return;
        };
        let org = crate::profile::get().github.org.clone();
        if let Some(name) = crate::app::reporter(card, &self.viewer).map(str::to_owned)
            && !org.is_empty()
            && !self.logins.contains_key(&name)
            && self.logins_pending.insert(name.clone())
        {
            self.send(Command::FindLogin { org, name });
        }
        let Some(card) = self.selected.as_deref().and_then(|id| self.card(id)) else {
            return;
        };
        for pr in card.prs.iter().filter(|pr| {
            (pr.open && !self.merged.contains_key(&pr.url))
                || (pr.merged && !self.gates.contains_key(&pr.url))
        }) {
            self.send(Command::Gate {
                url: pr.url.clone(),
            });
        }
    }

    /// Asks Linear's search for the palette's query once typing has paused
    /// for 300 ms, once per query.
    fn palette_search(&mut self, ctx: &egui::Context) {
        let Some(palette) = self.palette.as_mut() else {
            return;
        };
        if palette.query != palette.last {
            palette.last = palette.query.clone();
            palette.typed_at = std::time::Instant::now();
            palette.selected = 0;
            ctx.request_repaint_after(std::time::Duration::from_millis(320));
            return;
        }
        let term = palette.query.trim().to_owned();
        // A ticket number is worth asking for however short ("42").
        let short = term.len() < 3 && crate::linear::ticket_number(&term).is_none();
        if short || term == palette.searched || palette.repo.is_some() {
            return;
        }
        if palette.typed_at.elapsed() < std::time::Duration::from_millis(300) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        palette.searched = term.clone();
        self.send(Command::Search { term });
    }

    /// The active ticket pane's view (panes.rs).
    pub fn ticket_view(&self) -> panes::TicketView {
        panes::TicketView {
            selected: self.selected.clone(),
            tab: self.tab,
            session: self.session.clone(),
            diff_pr: self.diff_pr.clone(),
        }
    }

    /// Puts a ticket pane's kept view into `App`, as the active one.
    fn show_ticket_view(&mut self, view: panes::TicketView) {
        self.selected = view.selected;
        self.tab = view.tab;
        self.session = view.session;
        self.diff_pr = view.diff_pr;
    }

    /// Points the active pane at another ticket, which opens on its own
    /// lead PR, not the PR picked on the last one. A pane's kept view
    /// comes back through `show_ticket_view` instead, pick and all.
    fn select_ticket(&mut self, id: Option<String>) {
        if self.selected != id {
            self.diff_pr = None;
        }
        self.selected = id;
    }

    /// Asks for the diff of every PR a Diff tab shows, in any pane, that
    /// is neither here nor on its way. Runs every frame: the PR a tab
    /// shows follows the cards (`Card::pr`), so a refresh can change it.
    fn load_diffs(&mut self) {
        let others = self.panes.others();
        let views = others
            .iter()
            .map(|(_, v)| (v.selected.as_deref(), v.tab, v.diff_pr.as_deref()))
            .chain([(self.selected.as_deref(), self.tab, self.diff_pr.as_deref())]);
        let cards: Vec<&Card> = self.cards.iter().chain(&self.pr_cards).collect();
        let missing: Vec<Pr> = missing_diffs(&cards, views, |url| {
            self.diffs.contains_key(url) || self.diff_pending.contains(url)
        })
        .into_iter()
        .cloned()
        .collect();
        for pr in missing {
            self.fetch_diff(pr);
        }
    }

    /// Loads the diff the active pane shows again.
    fn reload_diff(&mut self) {
        let pr = self
            .selected
            .as_deref()
            .and_then(|id| self.card(id))
            .and_then(|c| c.pr(self.diff_pr.as_deref()))
            .cloned();
        if let Some(pr) = pr {
            self.fetch_diff(pr);
        }
    }

    /// The head the shown diff is at and the PR's head now, when they
    /// differ: new commits since the user looked.
    pub fn moved(&self, url: &str) -> Option<(&str, &str)> {
        let seen = self.diff_heads.get(url)?;
        let now = &self.gates.get(url)?.as_ref().ok()?.head;
        (seen != now).then_some((seen.as_str(), now.as_str()))
    }

    /// The user's last review of this PR and the head now, when new
    /// commits came after it.
    /// GitHub's word on the user's latest review stands in for a review
    /// made before this app kept any, or made on github.com.
    pub fn since_review(&self, url: &str) -> Option<(Reviewed, &str)> {
        let gate = self.gates.get(url)?.as_ref().ok()?;
        let reviewed = last_review(self.reviewed.get(url), gate)?;
        (reviewed.head != gate.head).then_some((reviewed, gate.head.as_str()))
    }

    /// New commits the user has not looked at, as (from, to): the diff is
    /// behind the PR, or the PR moved on since their review and no
    /// walkthrough of that follow-up is shown or on its way.
    pub fn changed(&self, url: &str) -> Option<(String, String)> {
        if let Some((seen, now)) = self.moved(url) {
            return Some((seen.to_owned(), now.to_owned()));
        }
        let (reviewed, now) = self.since_review(url)?;
        let followed_up = self
            .guides
            .get(url)
            .and_then(|g| g.as_ref().ok())
            .is_some_and(|g| g.since.as_deref() == Some(reviewed.head.as_str()));
        (!followed_up && !self.guide_pending.contains(url) && !self.guide_after.contains(url))
            .then(|| (reviewed.head, now.to_owned()))
    }

    /// Notes a review the user posted, at `head`, with its comments.
    /// Other copies write the file too: read-modify-write.
    fn remember_review(&mut self, url: &str, head: String, comments: Vec<String>) {
        let mut all: HashMap<String, Reviewed> = store::load(REVIEWED);
        all.insert(url.to_owned(), Reviewed { head, comments });
        let _ = store::save(REVIEWED, &all);
        self.reviewed = all;
    }

    fn fetch_diff(&mut self, pr: Pr) {
        self.diffs.remove(&pr.url);
        self.diff_heads.remove(&pr.url);
        self.diff_pending.insert(pr.url.clone());
        self.send(Command::Talk {
            url: pr.url.clone(),
        });
        self.send(Command::Diff {
            url: pr.url,
            owner: pr.owner,
            repo: pr.repo,
            number: pr.number,
            token: self.github_token.clone(),
        });
    }
}

/// The user's last review of a PR: the one this app posted, else the one
/// GitHub knows of (with no comments recorded).
fn last_review(local: Option<&Reviewed>, gate: &crate::github::Gate) -> Option<Reviewed> {
    match (local, &gate.reviewed_at) {
        (Some(local), _) => Some(local.clone()),
        (None, Some(head)) => Some(Reviewed {
            head: head.clone(),
            comments: Vec::new(),
        }),
        (None, None) => None,
    }
}

/// The PRs the Diff tabs among `views` (`(ticket id, tab, picked PR)`)
/// show whose diffs `have` says are neither loaded nor asked for, once each.
fn missing_diffs<'a, 'v>(
    cards: &[&'a Card],
    views: impl IntoIterator<Item = (Option<&'v str>, Tab, Option<&'v str>)>,
    have: impl Fn(&str) -> bool,
) -> Vec<&'a Pr> {
    let mut out: Vec<&Pr> = Vec::new();
    for (ticket, tab, picked) in views {
        let Some(pr) = ticket
            .filter(|_| tab == Tab::Diff)
            .and_then(|id| cards.iter().find(|c| c.issue.id == id))
            .and_then(|c| c.pr(picked))
        else {
            continue;
        };
        if !have(&pr.url) && !out.iter().any(|p| p.url == pr.url) {
            out.push(pr);
        }
    }
    out
}

/// `identifier  title` with the first title that says more than the
/// identifier; the identifier alone when none does.
/// A session Orbit did not start, by what it is about and where it runs:
/// Claude's own name ("myrepo-1a") alone says nothing.
fn outside_name(row: &crate::claude::Live, title: Option<&str>) -> String {
    let what = [title, row.name.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|t| !t.is_empty())
        .unwrap_or_else(|| row.folder());
    match row.host_name() {
        Some(host) => format!("{what} ({host})"),
        None => what.to_owned(),
    }
}

fn name_of<'a>(id: &str, titles: impl Iterator<Item = &'a str>) -> String {
    let mut titles = titles.map(str::trim);
    match titles.find(|t| !t.is_empty() && !t.contains(id) && *t != "untitled") {
        Some(title) => format!("{id}  {title}"),
        None => id.to_owned(),
    }
}

/// `ORBIT_TEAMS=ABC,XYZ` overrides the saved choice.
fn teams_from_env() -> Option<Vec<String>> {
    let value = crate::store::env_os("TEAMS")?.into_string().ok()?;
    Some(
        value
            .split(',')
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty())
            .collect(),
    )
}

impl App {
    /// The board follows the world without a click: Linear is asked again
    /// every 20 seconds, and at once when a session reports something (it
    /// just commented, opened a PR, or asked), so what Claude did shows up
    /// here and in Linear together.
    /// One line in the activity log for what the user did (activity.rs):
    /// the action's name, and where it went for Show and Tab. Dragging a
    /// divider is not an action worth a line.
    fn log_action(&self, action: &Action, via: &str) {
        let detail = match action {
            Action::Show(view) => crate::activity::name(view),
            Action::Tab(tab) => crate::activity::name(tab),
            Action::PaneRatio(..) | Action::LoadImage(_) => return,
            _ => String::new(),
        };
        let ctx = target(action).and_then(|t| self.situation(&t));
        self.log(&crate::activity::name(action), &detail, via, ctx);
    }

    fn log(&self, event: &str, detail: &str, via: &str, ctx: Option<serde_json::Value>) {
        if self.settings.activity_log == Some(false) {
            return;
        }
        let line = crate::activity::line(
            crate::activity::now_ms(),
            event,
            detail,
            &crate::activity::name(&self.view),
            &crate::activity::name(&self.tab),
            via,
            ctx,
        );
        crate::activity::append(&store::data_dir(), &line);
    }

    /// What a decision was taken on, as it stood when the user took it
    /// (activity.rs `ctx`): enums, flags and counts, the id hashed, never
    /// a title, a body or a name.
    fn situation(&self, target: &Target) -> Option<serde_json::Value> {
        use serde_json::json;
        let session = |key: &str| {
            let row = self.claude.row_of(key);
            json!({
                "state": row.map_or("none".to_owned(), |r| r.label().to_owned()),
                "agent": crate::activity::name(&self.claude.agent_of(key)),
                "reported": row.and_then(|r| r.report.as_ref()).map(|r| crate::activity::name(&r.state)),
            })
        };
        let gate = |url: &str| {
            self.gates.get(url).and_then(|g| g.as_ref().ok()).map(|g| {
                json!({
                    "green": g.green, "draft": g.draft, "queued": g.queued,
                    "review": g.review, "checks": g.checks, "branch": g.branch,
                    "own": g.own, "authorBot": g.author_bot, "canApprove": g.can_approve,
                    "openThreads": g.open_threads, "autoMerge": g.auto_merge,
                })
            })
        };
        let card_json = |card: &crate::story::Card| {
            json!({
                "lane": crate::activity::name(&card.lane),
                "stage": crate::activity::name(&card.stage),
                "ask": card.ask.map(|a| crate::activity::name(&a)),
                "ci": crate::activity::name(&card.ci),
                "prs": card.prs.len(),
                "openPrs": card.prs.iter().filter(|p| p.open).count(),
                "prOnly": card.pr_only(),
                "botWorks": card.bot_works,
                "mine": self.is_mine(card),
                "customers": card.customers.len(),
                "session": session(&card.issue.id),
            })
        };
        Some(match target {
            Target::Issue(id) => {
                let card = self.card(id)?;
                let mut ctx = json!({"kind": "ticket", "id": crate::activity::hashed(id)});
                ctx["card"] = card_json(card);
                if let Some(pr) = card.prs.iter().find(|p| p.open) {
                    ctx["gate"] = gate(&pr.url).unwrap_or_default();
                }
                ctx
            }
            Target::Pr(url) => {
                let mut ctx = json!({"kind": "pr", "id": crate::activity::hashed(url)});
                if let Some(card) = self
                    .cards
                    .iter()
                    .chain(&self.pr_cards)
                    .find(|c| c.prs.iter().any(|p| &p.url == url) || &c.issue.url == url)
                {
                    ctx["card"] = card_json(card);
                }
                ctx["gate"] = gate(url).unwrap_or_default();
                ctx
            }
            Target::Session(key) => json!({
                "kind": "session",
                "id": crate::activity::hashed(key),
                "ticket": self.card(key).is_some(),
                "session": session(key),
            }),
            Target::Inbox(key) => {
                let item = self.inbox().0.into_iter().find(|i| &i.key == key)?;
                json!({
                    "kind": "notification",
                    "id": crate::activity::hashed(key),
                    "ask": crate::activity::name(&item.ask),
                    "notes": item.notes.len(),
                    "fromGithub": item.notes.iter().any(|n| n.source == crate::inbox::Source::GitHub),
                    "fromLinear": item.notes.iter().any(|n| n.source == crate::inbox::Source::Linear),
                    "pr": item.pr.is_some(),
                    "ticket": item.issue.is_some(),
                })
            }
        })
    }

    /// Every two seconds: the spotter's file, when it changed (work.rs);
    /// then the best `now` spots start while fewer than `work.autoStart` run.
    fn follow_spots(&mut self, ctx: &egui::Context) {
        let folder = self.claude.folder(crate::claude::SPOTTER);
        let file = crate::work::spots_file(&folder);
        if self
            .spots_seen
            .changed(&file, std::time::Duration::from_secs(2))
        {
            self.spots = crate::work::read(&folder);
            self.spots_started = crate::work::started(&folder);
        }
        // A quick spot needs its ticket's card: only once the board is in.
        if !self.verified || self.loading || crate::read_only() {
            return;
        }
        let Some(Ok(spots)) = &self.spots else {
            return;
        };
        let running = self
            .spots_started
            .iter()
            .filter(|d| !d.session.is_empty() && self.claude.busy(&d.session))
            .count();
        let next: Vec<_> = crate::work::to_start(
            spots,
            &self.spots_started,
            running,
            self.settings.work.auto_start,
            &crate::today::date(),
        )
        .into_iter()
        .cloned()
        .collect();
        if next.is_empty() {
            return;
        }
        // Another copy may have started some since: done.json again.
        self.spots_started = crate::work::started(&folder);
        for spot in next {
            if !self.spots_started.iter().any(|d| d.id == spot.id) {
                self.start_spot(spot, false, ctx);
            }
        }
    }

    /// Every two seconds: the goals run's file, when it changed; the plan
    /// that waited for it goes ahead, and so it does when the run ended
    /// without writing one.
    fn follow_goals(&mut self, ctx: &egui::Context) {
        let folder = self.claude.folder(crate::claude::GOALS);
        let file = crate::goals::file(&folder);
        let wrote = self
            .goals_seen
            .changed(&file, std::time::Duration::from_secs(2));
        if wrote {
            self.goals = crate::goals::read(&folder);
        }
        let Some(mode) = self.plan_after_goals.clone() else {
            return;
        };
        let key = crate::claude::GOALS;
        let running = self.claude.starting.contains(key) || self.claude.busy(key);
        // The run may have written just before it ended: read it once more.
        if !running {
            self.goals = crate::goals::read(&folder);
        }
        let fresh = self
            .found_goals()
            .is_some_and(|g| crate::goals::current(g, &crate::today::date()));
        if fresh || !running {
            self.plan_after_goals = None;
            self.apply(Action::SpotWork(mode), ctx);
        }
    }

    /// The Inbox's rows, and how many old ones it left out.
    pub fn inbox(&self) -> (Vec<crate::inbox::Item>, usize) {
        let tickets = self
            .cards
            .iter()
            .flat_map(|c| c.prs.iter().map(|p| (p.url.clone(), c.issue.id.clone())))
            .collect();
        crate::inbox::combine(
            &self.inbox_notes,
            self.inbox_github,
            &tickets,
            &self.inbox_done,
        )
    }

    /// Hides an Inbox row at once and marks its notifications read on
    /// both sides; the row, when it was there.
    fn read_inbox(&mut self, key: &str) -> Option<crate::inbox::Item> {
        let item = self.inbox().0.into_iter().find(|i| i.key == key)?;
        let (mut linear, mut github) = (Vec::new(), Vec::new());
        for note in &item.notes {
            self.inbox_done.insert(note.id.clone());
            match note.source {
                _ if note.id.starts_with("rr:") => {}
                crate::inbox::Source::Linear => linear.push(note.id.clone()),
                crate::inbox::Source::GitHub => github.push(note.id.clone()),
            }
        }
        if !linear.is_empty() || !github.is_empty() {
            self.send(Command::InboxRead { linear, github });
        }
        Some(item)
    }

    /// The goals run's goals, when its file reads.
    pub fn found_goals(&self) -> Option<&crate::goals::Goals> {
        self.goals.as_ref().and_then(|g| g.as_ref().ok())
    }

    fn keep_fresh(&mut self, ctx: &egui::Context) {
        if !self.verified || self.loading || self.picking.is_some() {
            return;
        }
        let reported = self.claude.status_version != self.status_seen;
        let stale = self
            .refreshed
            .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(20));
        if reported || stale {
            self.status_seen = self.claude.status_version;
            self.apply(Action::Refresh, ctx);
        }
        // Live incidents, once a minute, only with a key.
        if let Some(key) = self.incident_key.clone()
            && self
                .incidents_at
                .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(60))
        {
            self.incidents_at = Some(std::time::Instant::now());
            self.send(Command::Alerts { key: key.clone() });
            self.send(Command::Incidents { key });
        }
        // KTLO's batch review and For you's rows need each open PR's gate
        // (who else is on it).
        if self.sheet && matches!(self.view, View::Ktlo | View::ForYou) {
            let cards: Vec<&Card> = match self.view {
                View::Ktlo => self.ktlo().collect(),
                _ => self.visible(),
            };
            let missing: Vec<String> = cards
                .into_iter()
                .flat_map(|c| c.prs.iter())
                .filter(|p| p.open && !self.gates.contains_key(&p.url))
                .map(|p| p.url.clone())
                .collect();
            for url in missing {
                if self.gates_asked.insert(url.clone()) {
                    self.send(Command::Gate { url });
                }
            }
        }
        // Every window's open PRs, once a minute: the sidebar's merging and
        // next move, and a pane behind the active one, follow GitHub too
        // (the active pane's are asked with every refresh).
        if self
            .window_gates_at
            .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(60))
        {
            self.window_gates_at = Some(std::time::Instant::now());
            let mut urls: Vec<String> = self
                .window_tickets()
                .iter()
                .filter_map(|id| self.card(id))
                .flat_map(|c| c.prs.iter())
                .filter(|p| p.open && !self.merged.contains_key(&p.url))
                .map(|p| p.url.clone())
                .collect();
            urls.sort();
            urls.dedup();
            for url in urls {
                self.send(Command::Gate { url });
            }
        }
        // The release workflows' latest runs, every two minutes (gh's rate limit).
        if !self.settings.releases.is_empty()
            && self
                .releases_at
                .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(120))
        {
            self.releases_at = Some(std::time::Instant::now());
            self.send(Command::Releases(self.settings.releases.clone()));
        }
        // Laya is on by default: fetch it once, in the background (840 MB,
        // resumed per file), the first time the app has signed in.
        if !self.laya_fetched && !self.laya_ready && self.settings.laya.on().is_some() {
            self.laya_fetched = true;
            self.apply(Action::DownloadLaya, ctx);
        }
        // The autopilot's file every half minute: its run when one is due
        // and none is working (another copy may run it too).
        if self
            .autopilot_at
            .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(30))
        {
            self.autopilot_at = Some(std::time::Instant::now());
            let key = crate::claude::ORCHESTRATOR;
            let now = crate::activity::now_ms() as u64;
            // The last run first: counted when it finished, stopped when it
            // sat an hour on a prompt, a Mac notification when it failed.
            if self.autopilot.pending
                && let Some(outcome) = self
                    .claude
                    .status
                    .get(key)
                    .and_then(|r| crate::autopilot::outcome(r, self.autopilot.last_ms))
                && (outcome == crate::autopilot::Outcome::Stuck || !self.claude.busy(key))
                && let Some(state) = crate::autopilot::settle(&outcome)
            {
                if outcome == crate::autopilot::Outcome::Stuck {
                    self.claude.stop(key);
                }
                if outcome != crate::autopilot::Outcome::Finished {
                    crate::notify::send(crate::autopilot::failure_line(&outcome, !state.on));
                }
                self.autopilot = state;
            }
            let idle = !self.claude.busy(key) && crate::profile::get().orchestrator.is_some();
            match idle.then(|| crate::autopilot::take(now)).flatten() {
                // The autopilot's run never asks: nobody is at the screen.
                Some(state) => {
                    self.autopilot = state;
                    if let Some(spec) = crate::claude::orchestrator(self.skill_files(), false) {
                        self.run_fresh(spec, ctx);
                    }
                }
                None => self.autopilot = crate::autopilot::load(),
            }
        }
        // The PRs a Land click covers, every minute until they land.
        if !self.landing.is_empty()
            && self
                .landing_at
                .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(60))
        {
            self.landing_at = Some(std::time::Instant::now());
            let urls: Vec<String> = self.landing.keys().cloned().collect();
            for url in urls {
                if !self.merging.contains(&url) {
                    self.send(Command::Gate { url });
                }
            }
        }
        // Missions, every two minutes: each parent read again, and what is
        // free to go handed to the bot.
        if self
            .missions_at
            .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(120))
        {
            self.missions_at = Some(std::time::Instant::now());
            self.missions = crate::mission::load();
            self.drive_missions(ctx);
        }
        // Notifications, once a minute: GitHub asks no more often.
        if self
            .inbox_at
            .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(60))
        {
            self.inbox_at = Some(std::time::Instant::now());
            self.send(Command::Inbox);
        }
        // KTLO's Dependabot PRs every five minutes: GitHub's search is rate limited.
        if self
            .dependabot_at
            .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(300))
        {
            self.dependabot_at = Some(std::time::Instant::now());
            // The repos the user deploys (settings.json `releases`).
            let mut repos: Vec<String> = self
                .settings
                .releases
                .iter()
                .map(|r| r.repo.clone())
                .collect();
            repos.sort();
            repos.dedup();
            self.send(Command::DependabotPrs(repos));
        }
        // A newer release, at start and then hourly.
        if self
            .update_at
            .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(3600))
        {
            self.update_at = Some(std::time::Instant::now());
            self.send(Command::CheckUpdate);
        }
    }
}

impl App {
    /// A PR-only card takes what GitHub says of its PR: the PR's title as
    /// its headline, its description, and done or canceled once it is
    /// merged or closed (it asks nothing then, and its session's turn is
    /// over: `mark_finished`).
    fn pr_known(&mut self, url: &str, gate: &crate::github::Gate) {
        let Some(i) = self
            .pr_cards
            .iter()
            .position(|c| c.prs.iter().any(|p| p.url == url))
        else {
            return;
        };
        let at = self.pr_cards[i].issue.updated_at.clone();
        if let Some(mut card) = crate::story::pr_card_from(url, &at, gate) {
            let old = &self.pr_cards[i];
            card.lane = old.lane;
            let same = old.issue.title == card.issue.title
                && old.issue.description == card.issue.description
                && old.stage == card.stage;
            if !same {
                self.pr_cards[i] = card;
            }
        }
    }

    /// A session whose ticket or PR is done has nothing left for the user:
    /// its finished turn reads "done", not "your turn". Claude Code's rows
    /// come fresh on every poll, so this runs every tick.
    /// Starts a cleanup pass when one is due (cleanup.rs), with the
    /// board's done and canceled tickets; the files are read and deleted
    /// on a thread of its own.
    /// Every ten minutes: Orbit's own background sessions nobody needs are
    /// stopped (claude/idle.rs). The hourly pass always runs on, and the
    /// board run while the autopilot is on.
    fn stop_idle_sessions(&mut self) {
        let now = std::time::Instant::now();
        if self.settings.keep_idle_sessions || now < self.idle_due || crate::read_only() {
            return;
        }
        self.idle_due = now + std::time::Duration::from_secs(600);
        let in_front: std::collections::HashSet<String> = self
            .claude
            .saved
            .keys()
            .filter(|key| self.session_in_window(key).is_some())
            .cloned()
            .collect();
        let mut protected = vec![crate::claude::OPERATOR];
        if self.autopilot.on || self.autopilot.pending {
            protected.push(crate::claude::ORCHESTRATOR);
        }
        let after = crate::claude::idle::idle_after(self.settings.idle_session_hours);
        self.claude.stop_idle(&in_front, &protected, after);
    }

    fn clean_up(&mut self) {
        let now = std::time::Instant::now();
        if self.settings.no_cleanup || now < self.cleanup_due || crate::read_only() {
            return;
        }
        self.cleanup_due = now + std::time::Duration::from_secs(24 * 60 * 60);
        let done = self
            .cards
            .iter()
            .filter(|c| matches!(c.issue.state.kind.as_str(), "completed" | "canceled"))
            .filter_map(|c| {
                let at: jiff::Timestamp = c.issue.updated_at.parse().ok()?;
                Some((c.issue.identifier.clone(), std::time::SystemTime::from(at)))
            })
            .collect();
        let note = self.cleanup_note.clone();
        std::thread::spawn(move || {
            let data = crate::store::data_dir();
            let facts = crate::cleanup::Facts {
                busy: crate::cleanup::busy(),
                done,
            };
            crate::cleanup::run(&data, &facts, false);
            *note
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = crate::cleanup::note(&data);
        });
    }

    fn mark_finished(&mut self) {
        let done: Vec<String> = self
            .claude
            .saved
            .iter()
            .filter(|(key, _)| {
                self.card(key)
                    .is_some_and(|c| c.stage == crate::story::Stage::Done)
            })
            .map(|(_, saved)| saved.session_id.clone())
            .collect();
        for id in done {
            if let Some(row) = self.claude.rows.get_mut(&id) {
                row.finished = true;
            }
        }
    }

    /// Every shell pane's screen, for a `claude` typed in it: working, a
    /// prompt waiting, or a turn that ended while the user looked elsewhere.
    fn glance_shells(&mut self) {
        // A claude typed in a shell pane starts without the workspace-trust
        // question in Orbit's own folders (every folder with settings.json
        // trustShellFolders); the folders are looked up off the UI thread.
        if self
            .shell_trusted
            .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(2))
        {
            self.shell_trusted = Some(std::time::Instant::now());
            let pids = self.shells.values().filter_map(|t| t.cwd_pid()).collect();
            crate::claude::trust_shell_folders(pids, self.settings.trust_shell_folders);
        }
        let looked = self.front.then_some(self.panes.focused);
        let states = &mut self.shell_states;
        states.retain(|id, _| self.shells.contains_key(id));
        for (id, terminal) in &mut self.shells {
            let before = states.get(id).copied();
            let listed = terminal.pid().and_then(|pid| self.claude.in_shell(pid));
            let seen = shell_seen(terminal.glance(), listed);
            match shell_state(before, seen, looked == Some(*id)) {
                Some(state) => states.insert(*id, state),
                None => states.remove(id),
            };
        }
    }

    /// The sessions whose terminal the user has in front of them right now
    /// (the Claude tab of a pane in the current window, attached, while the
    /// window is focused) are looked at: a turn that ended there is seen,
    /// and "your turn" counts only what they have not looked at yet.
    fn mark_seen(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().focused.unwrap_or(false)) {
            return;
        }
        let keys: Vec<String> = self
            .panes
            .ids()
            .into_iter()
            .filter_map(|id| {
                let view = match self.panes.surface(id)? {
                    panes::Surface::Ticket(_) if id == self.panes.active => self.ticket_view(),
                    panes::Surface::Ticket(view) => view.clone(),
                    _ => return None,
                };
                (view.tab == Tab::Claude).then_some(view.session.or(view.selected))?
            })
            .filter(|key| self.claude.attached.contains_key(key))
            .collect();
        for key in keys {
            self.claude.mark_seen(&key);
        }
    }

    /// Every few seconds: what needs the user, against what was said; the
    /// new lines go out as Mac notifications while Orbit is not in front.
    fn notify(&mut self, ctx: &egui::Context) {
        if self
            .notified_at
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(5))
        {
            return;
        }
        self.notified_at = Some(std::time::Instant::now());
        // Only a board that loaded says anything: an empty one would make
        // every open ask look new once it is back.
        if !self.verified || self.loading || self.cards.is_empty() {
            return;
        }
        let cards: Vec<&crate::story::Card> = self.cards.iter().collect();
        let failed: Vec<(&str, &crate::releases::Run)> = self
            .releases
            .iter()
            .filter_map(|(r, run)| Some((r.label(), run.as_ref().ok()?.as_ref()?)))
            .filter(|(_, run)| run.failed())
            .collect();
        // Every running session waiting on the user, by its name.
        let sessions: Vec<(String, String, String)> = self
            .claude
            .rows
            .values()
            .filter(|r| r.running())
            .map(|row| {
                let (key, name) = match self.claude.key_of(&row.session_id) {
                    Some(key) => (
                        key.clone(),
                        self.claude
                            .saved
                            .get(key)
                            .map_or_else(|| key.clone(), |saved| self.session_name(key, saved)),
                    ),
                    None => (
                        format!("sid:{}", row.session_id),
                        outside_name(
                            row,
                            self.claude
                                .about
                                .get(&row.session_id)
                                .and_then(|a| a.title.as_deref()),
                        ),
                    ),
                };
                (key, name, row.label().to_owned())
            })
            .collect();
        // herdr's agents waiting on the user notify like Orbit's sessions.
        let mut sessions = sessions;
        sessions.extend(self.herdr_agents.iter().map(|a| {
            (
                format!("herdr:{}", a.target()),
                format!("{} in herdr ({})", a.target(), a.workspace),
                a.label().to_owned(),
            )
        }));
        let open = crate::notify::open(
            &cards,
            &self.inbox().0,
            &self.alerts,
            &failed,
            &sessions,
            |c| self.short_title(c).to_owned(),
        );
        // What needs the user, for herdr's plugin pane (`orbit needs`).
        let needs: String = open.values().map(|l| format!("{l}\n")).collect();
        if needs != self.needs_written {
            let _ = std::fs::write(crate::herdr::needs_path(), &needs);
            self.needs_written = needs;
        }
        #[cfg(target_os = "macos")]
        self.menu_bar(&open, &sessions, ctx);
        let held = crate::notify::stable(&open, self.notify_previous.as_deref());
        self.notify_previous = Some(open.keys().cloned().collect());
        let new = crate::notify::take(&held);
        let front = ctx.input(|i| i.viewport().focused.unwrap_or(false));
        // A sound, as herdr plays one: a prompt that waits always, a turn
        // that ended only while Orbit is not in front.
        if self.settings.sound == Some(true) {
            let keys: Vec<&str> = new.iter().map(|(k, _)| k.as_str()).collect();
            if let Some(sound) = crate::notify::sound(&keys, front) {
                crate::notify::play(sound, &self.settings);
            }
        }
        if self.settings.notifications == Some(false) {
            return;
        }
        // Inside the app the same lines show as toasts, newest last, so
        // they are seen with Orbit in front too (the Mac says nothing then).
        let now = std::time::Instant::now();
        for (key, line) in &new {
            self.toasts.retain(|t| &t.key != key);
            self.toasts.push(Toast {
                key: key.clone(),
                line: line.clone(),
                at: now,
                agent: false,
            });
        }
        if front {
            return;
        }
        let lines: Vec<&String> = new.iter().map(|(_, line)| line).collect();
        // ponytail: a flood (back from a weekend) is one line, not twenty.
        match lines.as_slice() {
            [] => {}
            [one] => crate::notify::send((*one).clone()),
            [first, rest @ ..] => crate::notify::send(format!("{first} (+{} more)", rest.len())),
        }
    }

    /// The menu bar icon: what needs the user (`open`, counted beside it),
    /// the running sessions and the rest of For you; a line clicked opens
    /// here as its notification would.
    #[cfg(target_os = "macos")]
    fn menu_bar(
        &mut self,
        open: &std::collections::BTreeMap<String, String>,
        sessions: &[(String, String, String)],
        ctx: &egui::Context,
    ) {
        if self.settings.menu_bar == Some(false) {
            self.tray = None;
            return;
        }
        if self.tray.is_none() {
            self.tray = crate::tray::Tray::new(ctx);
        }
        let mut mine: Vec<&crate::story::Card> = self
            .cards
            .iter()
            .filter(|c| c.lane == crate::story::Lane::Product)
            .filter(|c| c.ask.is_none() && self.is_mine(c))
            .collect();
        crate::story::sort(&mut mine);
        let mine: Vec<(String, String)> = mine
            .iter()
            .take(15)
            .map(|c| {
                let text = format!("{} {}", c.issue.identifier, self.short_title(c));
                (c.issue.id.clone(), text)
            })
            .collect();
        let lines = crate::tray::lines(open, sessions, &mine);
        let title = if open.is_empty() {
            String::new()
        } else {
            open.len().to_string()
        };
        if let Some(tray) = &mut self.tray {
            tray.show(title, lines);
        }
    }

    /// Every repository the user's work touches, `owner/repo`, sorted:
    /// the pinned ones (settings.json `repos`), the watched deploys', the
    /// user's open PRs' and the board's PRs'.
    pub fn repos(&self) -> Vec<String> {
        let urls = self.my_prs.iter().map(|p| p.url.as_str()).chain(
            self.cards.iter().chain(&self.pr_cards).flat_map(|c| {
                c.prs
                    .iter()
                    .map(|p| p.url.as_str())
                    .chain(c.pr_only().then_some(c.issue.url.as_str()))
            }),
        );
        repos_from(
            &self.settings.repos,
            self.settings.releases.iter().map(|r| r.repo.as_str()),
            urls,
        )
    }
}

/// What a shell pane runs, from its screen and from `claude agents`: a
/// permission prompt on screen first, then Claude Code's busy (the screen
/// can show a `❯` box while it works: queued messages, no title spinner),
/// then the screen.
pub(crate) fn shell_seen(
    screen: Option<crate::claude::screen::Seen>,
    listed: Option<crate::claude::screen::Seen>,
) -> Option<crate::claude::screen::Seen> {
    use crate::claude::screen::Seen;
    match (screen, listed) {
        (Some(Seen::Blocked), _) => Some(Seen::Blocked),
        (_, Some(Seen::Working)) => Some(Seen::Working),
        (screen, listed) => screen.or(listed),
    }
}

/// A shell pane's state from its screen: "your turn" only after it was
/// seen working and until the user looks, so a shell prompt that happens to
/// start with ❯ never reads as Claude waiting.
pub(crate) fn shell_state(
    before: Option<&'static str>,
    seen: Option<crate::claude::screen::Seen>,
    looked: bool,
) -> Option<&'static str> {
    use crate::claude::screen::Seen;
    match seen {
        Some(Seen::Working) => Some("working"),
        Some(Seen::Blocked) => Some("needs approval"),
        Some(Seen::Prompt) if !looked && matches!(before, Some("working" | "your turn")) => {
            Some("your turn")
        }
        _ => None,
    }
}

impl eframe::App for App {
    /// Runs while the window is hidden too (eframe calls only this then):
    /// sessions, Linear, the files other runs write, and notifications keep
    /// going behind other windows.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.front = ctx.input(|i| i.viewport().focused.unwrap_or(false));
        self.claude.default_agent = self.settings.local_agent;
        self.claude.poll(ctx);
        self.claude.glance();
        self.glance_shells();
        self.mark_finished();
        self.clean_up();
        self.stop_idle_sessions();
        self.mark_seen(ctx);
        self.poll_oauth(ctx);
        if let Some(backend) = &self.backend {
            for event in backend.poll() {
                self.handle(event, ctx);
            }
        }
        self.toasts
            .retain(|t| t.at.elapsed() < std::time::Duration::from_secs(10));
        // herdr's agents every few seconds, while herdr is installed.
        if self.herdr_bin
            && self
                .herdr_asked
                .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(4))
        {
            self.herdr_asked = Some(std::time::Instant::now());
            self.send(Command::Herdr);
        }
        // A line clicked in the menu bar's menu: Orbit in front, it opened.
        #[cfg(target_os = "macos")]
        for id in crate::tray::clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            if let Some(action) = crate::notify::action_for(&id) {
                self.apply(action, ctx);
            }
        }
        // Links a herdr pane handed over (`orbit open`): opened here.
        for url in crate::herdr::take_opens(&crate::herdr::opens_path()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.apply(Action::Link(url), ctx);
        }
        if !self.toasts.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        self.follow_control(ctx);
        self.follow_settings(ctx);
        self.follow_spots(ctx);
        self.follow_goals(ctx);
        self.keep_fresh(ctx);
        self.notify(ctx);
        self.save_windows();
        // A window opened or changed since the last board load gets its
        // name without waiting for the next one.
        if self
            .named_at
            .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(3))
        {
            self.named_at = Some(std::time::Instant::now());
            self.maybe_shorten();
        }
        // Hidden, nothing else asks for the next tick.
        ctx.request_repaint_after(std::time::Duration::from_secs(5));
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        crate::theme::follow(&ctx, self.theme);
        self.palette_search(&ctx);
        review::tick(self, &ctx);
        review::simulate(self, &ctx);
        // Read before the views consume the keys: a click, else a key press.
        let via = ctx.input(|i| {
            let key = |e: &egui::Event| matches!(e, egui::Event::Key { pressed: true, .. });
            if i.pointer.any_click() {
                "click"
            } else if i.events.iter().any(key) {
                "key"
            } else {
                ""
            }
        });
        let mut actions = Vec::new();
        crate::ui::show(self, ui, &mut actions);
        for action in actions {
            self.log_action(&action, via);
            self.apply(action, &ctx);
        }
        // An error shows up: logged that it did, never what it says; what
        // it says goes to errors.log beside it, to debug from.
        let erred = self.error.is_some();
        if erred && !self.logged_error {
            self.log("error", "", "", None);
            if let Some(error) = &self.error {
                note_error(&crate::store::data_dir(), error);
            }
        }
        self.logged_error = erred;
        // After the actions, so a tab opened this frame asks this frame.
        self.load_diffs();
        self.load_quick();
        self.load_session_asks();
        // A native view draws over egui: hidden under the tutorial and the
        // New ticket window.
        let clear = self.tutorial.is_none()
            && self.capture.is_none()
            && self.palette.is_none()
            && !self.sheet;
        self.browser.follow(&ctx, frame, clear);
        for browser in self.browsers.values_mut() {
            browser.follow(&ctx, frame, clear);
        }
    }

    /// A focused web page takes the keys; the ones it hands back
    /// (browser.rs `take_keys`) arrive here as if pressed on the window,
    /// so the app's shortcuts work over a page too.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let browsers = std::iter::once(&mut self.browser).chain(self.browsers.values_mut());
        for (key, modifiers) in browsers.flat_map(|b| b.take_keys()) {
            for pressed in [true, false] {
                raw_input.events.push(egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers,
                });
            }
        }
    }
}

/// What an action is about, for its line in the activity log.
enum Target {
    Issue(String),
    Pr(String),
    Session(String),
    Inbox(String),
}

/// The ticket, PR, session or notification an action acts on, if any.
fn target(action: &Action) -> Option<Target> {
    use Action as A;
    Some(match action {
        A::Approve { url, .. }
        | A::Merge { url, .. }
        | A::AutoMerge { url, .. }
        | A::RequestReview { url, .. }
        | A::ClosePr { url, .. }
        | A::AddLabel { url, .. }
        | A::UpdateBranch { url, .. }
        | A::Draft { url, .. }
        | A::Review { url, .. }
        | A::AiReview { url, .. }
        | A::SetApprovalNote { url, .. }
        | A::OpenPr { url, .. } => Target::Pr(url.clone()),
        A::ReadyPr(url)
        | A::CancelAutoMerge(url)
        | A::NoteForApproval(url)
        | A::DiffPr(url)
        | A::ReviewAgain(url)
        | A::RefreshGate(url) => Target::Pr(url.clone()),
        A::RunSkill { issue_id, .. }
        | A::SetState { issue_id, .. }
        | A::MoveTeam { issue_id, .. }
        | A::AskBot { issue_id, .. } => Target::Issue(issue_id.clone()),
        A::Yolo(id) => Target::Issue(id.clone()),
        A::OpenTicket { id, .. } => Target::Issue(id.clone()),
        A::Focus(id) | A::Select(id) | A::TestPlan(id) | A::MarkRead(id) => {
            Target::Issue(id.clone())
        }
        A::Claude(key)
        | A::Attach(key)
        | A::OpenSession(key)
        | A::StopSession(key)
        | A::CloseSession(key)
        | A::FreshSession(key) => Target::Session(key.clone()),
        A::InboxOpen(key) | A::InboxDone(key) => Target::Inbox(key.clone()),
        _ => return None,
    })
}

/// `owner/repo` slugs from pins, deploys' repos and PR URLs, each once,
/// sorted; a URL that is not GitHub's is skipped.
pub fn repos_from<'a>(
    pinned: &[String],
    deploys: impl IntoIterator<Item = &'a str>,
    urls: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let mut out: Vec<String> = pinned
        .iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| s.contains('/'))
        .chain(deploys.into_iter().map(str::to_owned))
        .chain(urls.into_iter().filter_map(crate::github::slug_of))
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_busy_in_a_shell_reads_working_though_its_screen_shows_a_prompt() {
        use crate::claude::screen::Seen;
        assert_eq!(
            shell_seen(Some(Seen::Prompt), Some(Seen::Working)),
            Some(Seen::Working)
        );
        assert_eq!(
            shell_seen(Some(Seen::Blocked), Some(Seen::Working)),
            Some(Seen::Blocked)
        );
        assert_eq!(
            shell_seen(Some(Seen::Prompt), Some(Seen::Prompt)),
            Some(Seen::Prompt)
        );
        assert_eq!(shell_seen(None, Some(Seen::Prompt)), Some(Seen::Prompt));
        assert_eq!(shell_seen(None, None), None);
    }

    #[test]
    fn claude_in_a_shell_reports_its_state_and_a_plain_prompt_does_not() {
        use crate::claude::screen::Seen;
        assert_eq!(
            shell_state(None, Some(Seen::Working), true),
            Some("working")
        );
        assert_eq!(
            shell_state(None, Some(Seen::Blocked), true),
            Some("needs approval")
        );
        assert_eq!(
            shell_state(Some("working"), Some(Seen::Prompt), false),
            Some("your turn")
        );
        assert_eq!(
            shell_state(Some("your turn"), Some(Seen::Prompt), true),
            None,
            "looked at"
        );
        assert_eq!(
            shell_state(None, Some(Seen::Prompt), false),
            None,
            "a ❯ shell prompt"
        );
        assert_eq!(
            shell_state(Some("working"), None, false),
            None,
            "claude exited"
        );
    }

    /// Every Action variant has a gate in activity.rs `GATES`, and every
    /// gate names a real variant: the operator to come reads that table to
    /// know what it may do, so it is never out of date.
    #[test]
    fn every_action_has_a_gate() {
        let source = include_str!("mod.rs");
        let body = source
            .split("pub enum Action {")
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .unwrap();
        let variants: Vec<String> = body
            .lines()
            .filter(|l| l.starts_with("    ") && !l.starts_with("     "))
            .map(str::trim)
            .filter(|l| l.chars().next().is_some_and(char::is_uppercase))
            .map(|l| {
                l.chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect()
            })
            .collect();
        assert!(variants.len() > 100, "parsed {} variants", variants.len());
        let missing: Vec<&String> = variants
            .iter()
            .filter(|v| crate::activity::gate(v).is_none())
            .collect();
        assert!(missing.is_empty(), "actions without a gate: {missing:?}");
        let stale: Vec<&str> = crate::activity::GATES
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| !variants.iter().any(|v| v == n))
            .collect();
        assert!(stale.is_empty(), "gates for no action: {stale:?}");
    }

    #[test]
    fn repos_come_from_pins_deploys_and_pr_links_once_each() {
        let repos = repos_from(
            &["acme/web".into(), " acme/api ".into(), "junk".into()],
            ["acme/api", "acme/infra"],
            [
                "https://github.com/acme/web/pull/3",
                "https://github.com/other/tool/pull/1",
                "https://linear.app/acme/issue/A-1",
            ],
        );
        assert_eq!(repos, ["acme/api", "acme/infra", "acme/web", "other/tool"]);
    }

    #[test]
    fn a_session_is_named_once_by_the_first_title_that_says_more() {
        let id = "api#1197";
        let name = |titles: &[&str]| super::name_of(id, titles.iter().copied());
        // Titled only by its number: the PR's title from what the app knows.
        assert_eq!(
            name(&[id, "", "fix(workers): pin the worker tag"]),
            "api#1197  fix(workers): pin the worker tag"
        );
        // Claude's title repeating the identifier says nothing new.
        assert_eq!(name(&[id, "api#1197 api#1197"]), id);
        assert_eq!(name(&["ABC-9 fix", "x"]), "api#1197  ABC-9 fix");
        assert_eq!(name(&["untitled"]), id);
    }

    #[test]
    fn a_session_orbit_did_not_start_is_named_by_its_topic_and_host() {
        let row: crate::claude::Live = serde_json::from_str(
            r#"{"sessionId":"s","name":"myrepo-1a","cwd":"/Users/me/myrepo"}"#,
        )
        .unwrap();
        // Claude's own name says nothing; the title does.
        assert_eq!(
            super::outside_name(&row, Some("Fix the notification name")),
            "Fix the notification name"
        );
        assert_eq!(super::outside_name(&row, None), "myrepo-1a");
        let bare: crate::claude::Live =
            serde_json::from_str(r#"{"sessionId":"s","cwd":"/Users/me/myrepo"}"#).unwrap();
        assert_eq!(super::outside_name(&bare, Some("  ")), "myrepo");
    }

    #[test]
    fn an_answer_lands_on_the_waiting_question_only() {
        let mut asked: Vec<Asked> = vec![
            ("first".into(), Some(Ok("yes".into()))),
            ("second".into(), None),
        ];
        answer(&mut asked, Ok("no".into()));
        assert_eq!(asked[0].1, Some(Ok("yes".into())));
        assert_eq!(asked[1].1, Some(Ok("no".into())));
        answer(&mut asked, Ok("stray".into()));
        assert_eq!(asked[1].1, Some(Ok("no".into())));
    }

    #[test]
    fn the_reporter_is_asked_to_review_only_when_a_person_filed_it_and_it_is_in_review() {
        use crate::story::{Stage, tests::issue};
        let card = |creator: &str, stage: Stage, pr_open: bool| {
            let mut i = issue("In Review", "started", vec![]);
            i.creator = Some(crate::linear::User {
                name: creator.into(),
            });
            let mut c = crate::story::card(i, &std::collections::HashSet::new());
            c.stage = stage;
            c.prs = crate::story::pr_from_url("https://github.com/o/r/pull/1", "")
                .into_iter()
                .map(|mut p| {
                    p.open = pr_open;
                    p
                })
                .collect();
            c
        };
        let me = "Pat Doe";
        assert_eq!(
            reporter(&card("Sam Roe", Stage::Review, true), me),
            Some("Sam Roe")
        );
        assert_eq!(
            reporter(&card(me, Stage::Review, true), me),
            None,
            "not yourself"
        );
        assert_eq!(
            reporter(&card("acme-bot[bot]", Stage::Review, true), me),
            None
        );
        assert_eq!(reporter(&card("Sam Roe", Stage::Planning, true), me), None);
        assert_eq!(reporter(&card("Sam Roe", Stage::Review, false), me), None);
    }

    #[test]
    fn the_last_review_is_ours_else_githubs() {
        let gate = |at: Option<&str>| crate::github::Gate {
            head: "new".into(),
            green: false,
            why: String::new(),
            state: "OPEN".into(),
            draft: false,
            queued: false,
            open_threads: 0,
            merge_queue: false,
            can_approve: false,
            reviewed_at: at.map(str::to_owned),
            own: false,
            ..Default::default()
        };
        let ours = Reviewed {
            head: "mine".into(),
            comments: vec!["a.py:3: why?".into()],
        };
        assert_eq!(last_review(Some(&ours), &gate(Some("gh"))), Some(ours));
        assert_eq!(last_review(None, &gate(Some("gh"))).unwrap().head, "gh");
        assert_eq!(last_review(None, &gate(None)), None);
    }
    use crate::linear::Attachment;

    #[test]
    fn each_diff_tab_asks_once_for_the_pr_it_shows_now() {
        let url = |n: u64| format!("https://github.com/acme/web/pull/{n}");
        let mut issue = crate::story::tests::issue("In Review", "started", vec![]);
        for (n, status) in [(7, "inReview"), (9, "merged")] {
            issue.attachments.nodes.push(Attachment {
                url: url(n),
                title: "fix".into(),
                source_type: Some("github".into()),
                metadata: serde_json::json!({ "status": status, "repoName": "web",
                    "number": n, "reviews": [] }),
            });
        }
        let card = crate::story::card(issue, &HashSet::new());
        let cards = [&card];
        let (seven, nine) = (url(7), url(9));
        let gone = url(3);
        let views = [
            // The lead PR, the picked one, one picked but no longer linked
            // (the lead again), and a pane on another tab.
            (Some("i"), Tab::Diff, None),
            (Some("i"), Tab::Diff, Some(nine.as_str())),
            (Some("i"), Tab::Diff, Some(gone.as_str())),
            (Some("i"), Tab::Walkthrough, Some(seven.as_str())),
        ];
        let asked = |have: &[&str]| -> Vec<u64> {
            missing_diffs(&cards, views, |u| have.contains(&u))
                .iter()
                .map(|p| p.number)
                .collect()
        };
        assert_eq!(asked(&[]), [7, 9]);
        assert_eq!(asked(&[&seven]), [9]);
        assert!(asked(&[&seven, &nine]).is_empty());
    }
}

/// `known` minus `seen`; nothing is new before the first look.
fn unseen(known: &[String], seen: Option<&[String]>) -> Vec<String> {
    let Some(seen) = seen else {
        return Vec::new();
    };
    known
        .iter()
        .filter(|k| !seen.contains(k))
        .cloned()
        .collect()
}

/// One line per error the app showed, with when: activity.jsonl only says
/// that one did. ponytail: never trimmed, errors are rare (once per showing).
fn note_error(dir: &std::path::Path, error: &str) {
    use std::io::Write;
    let line = format!("{} {}\n", jiff::Timestamp::now(), error.replace('\n', " "));
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("errors.log"))
    {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod skills_seen_tests {
    #[test]
    fn an_error_says_what_it_was_in_errors_log() {
        let dir = std::env::temp_dir().join(format!("orbit-errors-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        super::note_error(&dir, "could not save the initiative:\ndisk full");
        let log = std::fs::read_to_string(dir.join("errors.log")).unwrap();
        assert!(
            log.ends_with(" could not save the initiative: disk full\n"),
            "{log}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_skill_is_new_only_after_a_first_look() {
        let known: Vec<String> = ["build", "spot", "acme-deploy"].map(String::from).into();
        assert!(super::unseen(&known, None).is_empty());
        let seen: Vec<String> = ["build"].map(String::from).into();
        assert_eq!(super::unseen(&known, Some(&seen)), ["spot", "acme-deploy"]);
        assert!(super::unseen(&known, Some(&known)).is_empty());
    }
}

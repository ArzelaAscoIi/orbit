//! The tutorial: a guided tour of every feature, one step at a time.
//!
//! A step names the view to show and an anchor: a place in the interface
//! that the panels record with `mark` while they draw. The overlay dims
//! everything but that place and puts a card with the explanation next to
//! it. A step whose anchor is not on screen (no ticket selected, nothing
//! needs you) shows its card in the middle instead, so the tour never
//! stalls.
//!
//! Rule: every user-facing feature has a step here. Adding or changing a
//! feature means adding or updating its step and its `mark`; the test at
//! the bottom fails when a step's anchor is not marked anywhere in `ui/`.

use super::*;
use egui::{Id, LayerId, Order, Pos2, Rect, Vec2};

pub struct Step {
    /// The place to highlight; `None` for a card in the middle.
    pub anchor: Option<&'static str>,
    /// The view the step shows in the sheet (Settings in place of the
    /// panes); `None` lowers the sheet, for the panes and the bars.
    pub view: Option<View>,
    pub title: &'static str,
    pub body: &'static str,
}

/// A step's text with the profile's names in: `{bot}` / `{Bot}` the team's
/// bot, `{fix}` the fix-comments command, `{board}` the orchestrator's board.
fn fill(text: &str) -> String {
    let profile = crate::profile::get();
    let bot = crate::story::bot();
    let mut capital = bot.chars();
    let bot_capital: String = capital
        .next()
        .map_or(String::new(), |c| c.to_uppercase().chain(capital).collect());
    let board = profile
        .orchestrator
        .as_ref()
        .map_or("team's board".to_owned(), |o| format!("{} board", o.team));
    text.replace("{bot}", &bot)
        .replace("{Bot}", &bot_capital)
        .replace("{fix}", &format!("/{}", profile.skill("fixComments")))
        .replace("{board}", &board)
        .replace("{ktlo}", &profile.ktlo.label)
}

const fn step(
    anchor: Option<&'static str>,
    view: Option<View>,
    title: &'static str,
    body: &'static str,
) -> Step {
    Step {
        anchor,
        view,
        title,
        body,
    }
}

pub const STEPS: &[Step] = &[
    step(
        None,
        None,
        "Welcome to Orbit",
        "{Bot} works your Linear tickets: triage, plan, build, open the PR. This app shows \
         what it is doing and, above all, what needs you. Claude Code runs per ticket in the \
         background. This tour takes about two minutes. Use the arrow keys or the buttons; \
         Esc ends it.",
    ),
    step(
        Some("strip"),
        None,
        "Windows",
        "The sidebar on the left has one row per window: a ticket, a shell or a session, with \
         its number, a short name (Haiku's, or yours), what it holds, and what its Claude session \
         is doing (a spinner while it works, your turn, needs approval, PR opened, failed; also for \
         claude typed into a shell). \
         Cmd+1 to 9 switch windows, Cmd+Shift+] and [ step to the next or previous, + new or \
         Cmd+N opens a new one: Enter starts Claude in it, T a shell, E a plain editor (line numbers, long lines scroll sideways), B a browser (Cmd+F finds in its page), or file a ticket. Drag a row to reorder; right-click \
         renames it or moves it up or down (Cmd+Shift+Up/Down for the current one); × or a \
         middle click closes it, its sessions keep running, and Cmd+Z reopens it (again for the one before); once every ticket in it is done it \
         reads \"done ×\"; otherwise it says your next move on its ticket in the same place \
         (fix comments, fix CI, merge, approve, verify, ...), also once its session finished. \"your order\" switches to needs you first. « collapses the sidebar \
         to the numbers and their states. The notifications sit on top, a live incident is \
         pinned first, and the windows come back as you left them when Orbit restarts.",
    ),
    step(
        Some("tabs"),
        None,
        "A pane",
        "Each pane's header has the ticket's id, its tabs and, on the right, its stage and its \
         PR with CI. Walk reads the ticket top to bottom. Diff shows the PR with an optional \
         Claude walkthrough: why the ticket needs the change, then step by step the lines that \
         do it, with questions under the characters they are about. Claude attaches the \
         ticket's own Claude Code session, which keeps running when you switch or quit; it \
         builds in a fresh git worktree per repo.",
    ),
    step(
        Some("terminal"),
        None,
        "Links and copying in a session",
        "Hold Cmd over a link Claude printed and the pointer turns into a hand; Cmd+click \
         opens it. A Linear ticket or a GitHub PR opens here in Orbit, a PR on its Diff tab; \
         anything else opens in the browser. Drag over text to select it and press Cmd+C to \
         copy it, in a session or a shell.",
    ),
    step(
        Some("panes"),
        None,
        "Splits",
        "Cmd+D splits the focused pane to the right, Cmd+Shift+D below; drag a divider to \
         resize. In a shell, the split is another shell (a claude typed there starts without the trust question, shows its state and comes back resumed after a restart); in a new empty pane Shell puts a shell there; the window's tab on top names \
         every pane in it. Each pane is a full ticket with its own tabs, so a second ticket, a shell or \
         nothing sits beside the first. Cmd+] moves between panes, Cmd+Shift+Enter zooms one, Cmd+Enter takes the focused session full screen, Esc leaves it.",
    ),
    step(
        Some("split-claude"),
        None,
        "Walkthrough beside the session",
        "Split, next to Stop on a ticket's Claude tab, puts the ticket's walkthrough on the \
         left and the session on the right, so you read where the ticket stands while Claude works.",
    ),
    step(
        Some("split-ticket"),
        None,
        "Claude beside the diff",
        "split in a ticket's header opens the same ticket again beside it. From the Claude tab \
         the copy shows the diff (the walkthrough without a PR), so you read the change while \
         Claude works on it. Cmd+D does the same from the keyboard.",
    ),
    step(
        Some("views"),
        None,
        "The bottom bar",
        "The views, each with a key and a count: 0 notifications, 1 for you, \
         4 customers, 5 sessions, 6 intake, 7 {ktlo}, 8 alerts, 9 repos. A count in the signal \
         colour waits on you; a red dot on {ktlo} is a live incident. On the right: sessions \
         working, sessions on your turn, when Linear last answered (click to ask now), and Cmd+K.",
    ),
    step(
        Some("notifications-badge"),
        None,
        "Notifications, where you look",
        "Top right, the strip counts your notifications and turns the signal colour while one \
         asks something of you; a click opens the Notifications view. It is its own view, apart \
         from For you: only what Linear and GitHub asked of you, nothing of the board.",
    ),
    step(
        Some("toasts"),
        None,
        "Toasts",
        "What newly asks for you shows bottom right inside the app for ten seconds: a review, a \
         mention, a session on your turn, a firing alert. Click one to go there, × closes it. \
         The Mac gets the same line while Orbit is not in front. A grey toast marked agent is \
         what a program did through orbit ctl: there to follow along, nothing to answer.",
    ),
    step(
        None,
        None,
        "Cmd+K: your PRs by repo",
        "Cmd+K searches tickets, sessions, actions and your open PRs. Type a repo's name and \
         press Tab: only your open PRs in that repo, newest first; type to narrow, Enter opens \
         one here. Backspace on the empty field goes back to everything.",
    ),
    step(
        Some("sheet"),
        Some(View::ForYou),
        "The sheet",
        "i raises the sheet over the panes, and so does a view's key (0 to 7, p); the bar stays \
         as its tabs, and the same keys switch them. j and k move, h and l change column, Enter opens the ticket as a new window, \
         Cmd+D as a split beside the focused pane, Esc closes. Alt+Down opens the next ticket \
         For you lists without the sheet.",
    ),
    step(
        Some("list"),
        Some(View::ForYou),
        "For you",
        "Only the tickets that need you, one column per thing to do: review the PR, make a \
         call, verify a merged fix, and what is assigned to you. A colleague's ask from Slack \
         (Linear Asks) waits under make a call until it is triaged, with who asked and how \
         urgent. The board keeps itself fresh every 20 seconds. The × on a card (or x) marks \
         it read: it leaves For you until something on it changes, a comment, its stage or a PR.",
    ),
    step(
        Some("row"),
        Some(View::ForYou),
        "A ticket",
        "Its stage, id and age on top, the title, one line on why it is here, and tags: \
         the customer, the story, and a Claude tag when it has a session. A click shows it \
         in an empty pane, else in its own window: it never replaces what a pane shows.",
    ),
    step(
        Some("brief"),
        Some(View::ForYou),
        "The brief",
        "Claude reads the board and writes the one thing that matters right now, on top of \
         For you. Click it to go there. It runs on your Claude Code login, or on an \
         Anthropic key when one is set in Settings.",
    ),
    step(
        Some("onboarding"),
        Some(View::ForYou),
        "Getting started",
        "On first open, after you sign in and pick teams, a Claude session asks a few \
         questions and writes your profile. When it saves, the session closes and Settings \
         opens on the connections, where the API keys go. When the app or your profile gets \
         new skills later, For you offers Update the profile, which asks about those only.",
    ),
    step(
        Some("list"),
        Some(View::Customers),
        "Customers",
        "What is going on per customer, with a one-line summary from Claude. Enter or a \
         click shows their tickets.",
    ),
    step(
        Some("list"),
        Some(View::Sessions),
        "Sessions",
        "Every Claude Code session on this Mac, most relevant first: waiting on your approval, \
         then your turn (a turn you have not looked at yet), then at work, then idle; ranked in \
         Plan my day and asking for you on the board lift one, a done ticket sinks, and a \
         finished session whose ticket or PR is done reads done. Each row says why, and its second \
         line is what the session says it is on (its own one-line summary), else its last words. \
         With Laya on it lifts sessions whose last message asks \
         you something (the Laya chip). The first five are now, the rest later. Enter or a click opens one \
         in a window of its own, or goes to the window already showing it; a stopped one \
         resumes. Orbit stops its own sessions that sit idle out of sight for half a day (an hour \
         once their ticket is done), so they hold no memory; opening one picks it up again. A session Orbit did not start says where it runs (\"in Ghostty\", \"in the background, not by Orbit\"); \
         clicking one running in a terminal app brings that app to the front. x stops it, Shift+X closes it: stopped and off the list, the conversation kept in Claude Code. A new session without a ticket is Cmd+K: New session: <what>, \
         with \"in <folder>\" at the end to pick its folder.",
    ),
    step(
        Some("guide-risk"),
        None,
        "Risk, not every line",
        "The walkthrough says what the PR could break outside its ticket, as risk low, medium \
         or high with the callers, data or services it touches and whether a test covers them. \
         When the ticket asked the wrong thing, a line under it says so and names the smaller \
         answer. Read these first; open the diff where the risk points.",
    ),
    step(
        Some("sub-tickets"),
        None,
        "Sub-tickets",
        "A ticket split into parts lists its sub-tickets right under its status, each with \
         its state and who has it, and how many are done; a sub-ticket names the ticket it is \
         part of. Click one to open it here, even when it is not on your board.",
    ),
    step(
        Some("clone-chat"),
        None,
        "Chat with your clone",
        "Cmd+J (or chat beside the clone switch) opens your clone on the right, like a coding \
         agent's panel in an editor: each chat is a full Claude Code session with your own \
         login that steers Orbit for you (read the board, press buttons, start missions). Above it every chat and clone run, newest first; + new chat starts another. \
         The clone switch says how many clones are working right now and how many missions run. \
         settings.json clone.skills and clone.instructions tune it.",
    ),
    step(
        Some("missions"),
        None,
        "Missions",
        "A mission is one parent ticket on top with its sub-tickets under it, driven through the \
         team's bot for days until each has a PR: a feature, a fix someone reported in Slack, a \
         parent to land. Start one from a parent ticket's Start mission, or from a goal in your \
         words with new mission (or Cmd+K Mission: …): your clone researches, files the parent \
         and sub-tickets, then Orbit drives them. The heading's − folds the list. Orbit tells you \
         once for each PR to review, block or merge, and when one sits with the bot for an hour. \
         A finished one stays under finished for two days, saying how it ended (the reviews \
         are yours, or stopped by you), then folds into earlier; its × takes it off. \
         Each row: how far, what is yours, then each sub-ticket's step in a word (building, \
         fixing comments, your review, merging, waiting on, done) with its session's note, else \
         what the bot did last and when; a breathing dot shows what runs right now (bot quiet: \
         nothing on it for 30 minutes; held: review cap: your reviews free the bot), and a \
         ticket the bot's team renamed says what it was. A click on the row opens the session \
         that drives it in the chat, to ask where it stands and steer it (one is started with \
         the log if none drove it); window opens it in a window of its own (green CLONE in the \
         sidebar), the × closes it. orbit ctl missions shows every move and why each sub-ticket \
         waits.",
    ),
    step(
        Some("mission"),
        None,
        "Start a mission on a parent",
        "On a ticket with sub-tickets, the row with Orbit's mark under the title starts a \
         mission: Orbit moves the sub-tickets to the bot's Todo two at a time, each once what \
         blocks it is merged, until each has a PR for you to review, and says every move. One \
         that goes quiet for 30 minutes where the bot waits for a word (plan done, no plan, a \
         red PR) gets the bot's Plan or Build once, and you hear it. While it runs the row says \
         how far it is; a click steers it, stop ends it.",
    ),
    step(
        Some("the-ask"),
        None,
        "The ask",
        "Before the plan, whether to build at all: who asked, the need behind it, the evidence, \
         a smaller answer, and the bot's verdict. Push back or Ask holds the ticket for your call: \
         Use this reply puts its suggested answer to the requester in the comment box, to edit \
         and send; to build it anyway, use the Build step's menu.",
    ),
    step(
        Some("clone"),
        None,
        "Your clone's hourly pass",
        "In the clone panel (Cmd+J), the hourly pass (+ unfolds it): tick the jobs it should do (alerts, \
         notifications, For you, your PRs, sessions, missions) and Start. One Claude Code \
         session then runs /loop 1h /operate on them, says what it did each hour, and never \
         approves or merges; watch shows it, stop ends it. Untick every job to pause the pass; a failed run shows red on its heading. The \
         button top right opens the panel.",
    ),
    step(
        Some("operator"),
        Some(View::Sessions),
        "The operator",
        "Run the operator hands Orbit to one Claude run that works it for you through orbit ctl: \
         alerts, notifications, For you, your PRs, sessions whose work is finished. Each button it \
         presses shows as a grey agent toast, and its own lines also reach you as Mac \
         notifications while Orbit is behind. It never approves, merges or changes what others \
         see; those land in its report for you.",
    ),
    step(
        Some("session-search"),
        Some(View::Sessions),
        "Search sessions",
        "Type a few words to find a session by what it is about: its title, its folder, \
         Claude's title for it and everything you asked in it (\"workers prod\"), or by its state \
         (\"your turn\", \"working\", \"needs approval\"). An unnamed \
         session shows Claude's title. Cmd+K finds sessions the same way, those waiting on you first.",
    ),
    step(
        Some("session-note"),
        None,
        "What a session is on",
        "Under a session's header, one line in its own words: every session can report a \
         one-line summary of what it is on (the orbit skill in its folder says how), and it \
         shows here and on its row in Sessions. Beside it the app says when a stopped session \
         could not be resumed and started fresh instead, so a lost conversation is never silent.",
    ),
    step(
        Some("orchestrate"),
        Some(View::Sessions),
        "Run the board",
        "One click hands the {board} to an unattended Claude run: it lands approved PRs, fixes \
         red ones, splits Blocked tickets, refills Todo and grooms Triage, one action at a time, \
         then reports what is ready for you. It never approves a PR. Click its status to watch.",
    ),
    step(
        Some("autopilot"),
        Some(View::Sessions),
        "Autopilot",
        "Beside Run the board: switch the autopilot on and the board run goes every hour, \
         the first at once, never while one is still working. It switches itself off after \
         eight finished runs, so turn it on in the morning. A run that fails, or waits an hour \
         on a prompt (then stopped), does not count and sends a Mac notification (a login \
         error says to run claude /login); three of those in a row switch it off.",
    ),
    step(
        None,
        None,
        "Mac notifications",
        "When a ticket starts asking for you (review, decide, verify), a review is requested \
         or you are assigned or mentioned in Linear, or a Claude session waits on you (your \
         turn, needs approval), your Mac shows one short line, only \
         while Orbit is not in front; activity on your own PRs never does, and a state that \
         holds for less than a few seconds never reaches the Mac. \
         Orbit keeps checking behind other windows. notifications: false in settings.json \
         turns them off.",
    ),
    step(
        None,
        None,
        "Menu bar",
        "The ring in the Mac's menu bar counts what needs you. Its menu lists those lines, every \
         running session with its state and the rest of For you; a click brings Orbit in front \
         with that ticket or session open. menuBar: false in settings.json takes it away.",
    ),
    step(
        Some("notifications"),
        Some(View::Inbox),
        "Notifications",
        "Press 0: Linear's and GitHub's notifications in one list, only what asks something of \
         you (a review, a mention, an assignment, an SLA at risk, activity on your PR). A PR and \
         its ticket are one row, however many sides told you. Enter opens it and marks it read \
         on both, x only marks it read. A PR with no ticket opens here like one: walkthrough, \
         diff, merge and approve, Claude; only Linear's parts are missing.",
    ),
    step(
        Some("stats"),
        Some(View::Stats),
        "Stats",
        "Cmd+K Stats, or your stats in Settings: your last seven days in Orbit from this Mac's own logs (actions, approvals, \
         merges, skills run, what your sessions did, your decisions by gate) beside the team's \
         from SigNoz: PRs touched and merged, how many by a bot or AI-assisted, yours, and your \
         reviews. SigNoz is read through the profile's gateway, no key needed.",
    ),
    step(
        Some("herdr"),
        None,
        "herdr",
        "With herdr installed, its agents show in the sidebar under // in herdr with herdr's own \
         states (needs approval, your turn, working); a click brings one forward in herdr and \
         its terminal to the front, and one waiting on you notifies like Orbit's sessions. \
         Cmd+K Start <ID> in herdr opens a ticket's Claude session as a herdr agent, with the \
         ticket's brief.",
    ),
    step(
        Some("herdr-settings"),
        Some(View::Settings),
        "Your herdr setup",
        "Settings, herdr: use my herdr config takes its theme, notifications, sounds and \
         sidebar order, and turns on herdr's keys (ctrl+b, then c, v, minus, z, n, p, 1 to 9, as \
         you set them). Add Orbit's plugin to herdr puts what needs you in a herdr popup and \
         opens Linear and GitHub links clicked in herdr here. Sounds: a prompt that waits always \
         sounds, a finished turn only while Orbit is in the background.",
    ),
    step(
        Some("repos"),
        Some(View::Repos),
        "Repositories",
        "Press 9: every repository your work touches (your open PRs, the board's PRs, the \
         watched deploys, and the repos you pin), with how many of your PRs are open there \
         and how many wait on your review. Click one for its page, or jump to its pulls or \
         actions on GitHub. Cmd+K finds a repo by name.",
    ),
    step(
        Some("repo"),
        None,
        "A repository's page",
        "A repo's open pull requests, grouped: waiting on your review, yours, the rest; each \
         with its author, checks, review state and age. Click one and it opens here as a PR \
         card: diff, approve, merge, Claude. code, pulls, issues and actions open the GitHub \
         pages; refresh asks GitHub again. The search on top (or /) filters by number, title, \
         author, or a word: draft, approved, red, green, yours, review.",
    ),
    step(
        Some("handle-inbox"),
        Some(View::Inbox),
        "Handle all my notifications",
        "One click hands every notification here to one Claude session, each with its link. \
         For each it says what it asks and your move; for a review, its verdict and the few \
         places you should look at, with why; for a question, a draft answer. It ends with \
         your moves as a checklist and asks before posting, approving or merging anything. \
         Also in Cmd+K.",
    ),
    step(
        Some("request-review"),
        None,
        "Request review",
        "Request review (under More, or the filled button on your own PR) opens a field beside \
         the buttons: type part of a name \
         or login and pick the person, only people who may review on that repo are offered, \
         so GitHub never refuses one. Roulette (when the profile names a router's label) adds \
         that label instead, and the router picks a reviewer.",
    ),
    step(
        Some("working"),
        None,
        "What runs",
        "Bottom right, \"your turn\" and \"working\" count the sessions waiting on you and the ones \
         at work. A finished turn counts as your turn until you have had its session in front \
         of you; then it is idle until the next one (a question or a permission prompt stays \
         yours until answered). The attached terminal's own screen is read too, so a spinner \
         or a permission prompt shows the moment it is drawn, even in a session Orbit did not \
         start. Click either count and Sessions opens searched for that state, the list behind \
         the number.",
    ),
    step(
        Some("preview"),
        None,
        "Preview",
        "On a PR in a repo that deploys previews (the profile names which), Preview ▾ lists \
         each kind of preview (a UI-only one, a full one, as the profile lists them); a pick \
         adds its label and the repo's workflow deploys it. Hover one to see what it deploys.",
    ),
    step(
        Some("batch-review"),
        Some(View::Ktlo),
        "Batch review",
        "On top of {ktlo}, every open {ktlo} PR that waits on your approval (Dependabot bumps on \
         the repos you deploy, CVE fixes), each as repo - title: Diff opens it, Diff all shows every \
         diff under its row, Approve approves it at the head shown. \
         Land approved then merges the green ones, sets Merge when ready where checks still \
         run, and starts one session that makes the rest mergeable. Orbit keeps at it: each \
         one goes back into the merge queue by itself once it is green at the head you saw, \
         also after the queue drops it; one that got new commits asks you to land it again.",
    ),
    step(
        Some("releases"),
        Some(View::Alerts),
        "Deploys",
        "Alerts' deploys tab (next to alerts, red when one failed): each deploy workflow from settings.json releases (repo and workflow) \
         with its latest run: green, red or running. A red one names whose change it ran (by \
         @login and the PR), sends a Mac notification and has \
         Fix it: a Claude session runs the fix-deploy skill on the failed run, finds why and \
         whose change broke it, then opens a fix PR and pings them, or a revert PR assigned to \
         them; it merges nothing, in a red RELEASE window.",
    ),
    step(
        Some("alerts"),
        Some(View::Alerts),
        "Alerts",
        "8 lists incident.io's alerts of the last day, firing first; the tab turns red while one \
         fires and a new one sends a Mac notification. Click an alert for its details, then \
         Debug runs the profile's debug skill on it, or Investigate starts a session with the \
         investigate panes beside it, in a red ALERT window of its own (an incident's is INCIDENT).",
    ),
    step(
        Some("triage"),
        Some(View::Intake),
        "Intake and triage",
        "Tickets still in Triage: nobody has looked at them yet. The ones filed in the last \
         day sit on top under a red \"new today\", and each new one sends a Mac notification \
         while Orbit is in the background. Triage with Claude opens its \
         session in a window of its own: it clusters them, folds duplicates, and proposes what to \
         accept, park or cancel. Continue triage runs it again in the same conversation. Nothing \
         is written until you approve.",
    ),
    step(
        Some("find-work"),
        Some(View::Intake),
        "Find work in the backlog",
        "The backlog is not in Intake. Find work starts a Claude session that reads this week's \
         goals and priority docs, ranks the backlog tickets that move them, and moves the ones \
         you pick to Todo. Also in Cmd+K.",
    ),
    step(
        Some("incidents"),
        Some(View::Ktlo),
        "Incidents",
        "With incident.io connected in Settings, live incidents sit on top of {ktlo}. \
         Investigate opens a Claude session with the incident as its brief (the tools your \
         profile names) and the settings.json investigate panes beside it; it posts nothing \
         without you.",
    ),
    step(
        Some("ship-reviews"),
        Some(View::ForYou),
        "Ship the bot's PRs",
        "The bot writes a fix in minutes; then it waits days for your review. Ship the bot's \
         PRs hands every PR in the review column to one Claude session: it reads each diff, \
         gives a verdict, and after you pick, approves at the head you saw and merges; what \
         needs a person moves to its team. Also in Cmd+K.",
    ),
    step(
        Some("clear-ktlo"),
        Some(View::Ktlo),
        "Clear {ktlo}",
        "One Claude session goes through every {ktlo} ticket. A fix that only needs a review \
         gets your approval and is merged at the head you saw; one that needs a person moves \
         to the team that owns it with a note on what to do. It asks you first; also in Cmd+K.",
    ),
    step(
        Some("list"),
        Some(View::Ktlo),
        "{ktlo}",
        "Security findings (a scanner's CVE tickets, the Security label), Dependabot PRs \
         waiting on your or your team's review, and the alerts {bot}'s sweeps file live here, out of For you and Intake, so they never eat product \
         time. What counts as {ktlo} comes from the profile. One nobody picked up for a day \
         (no owner, not started) goes to the top with Nobody on it, the tab's count turns red, \
         and an urgent or high one pings your Mac once.",
    ),
    step(
        None,
        None,
        "Command palette",
        "Cmd+K from anywhere: search tickets (on your board, and all of Linear), sessions, \
         and customers, or run an action: Run the board, \
         New ticket, New window (another copy of the app), Settings, the tutorial, themes, \
         teams, sign out, and the focused ticket's Linear state. Debug <env> opens, in a window of its own, the \
         settings.json debug panes, a tool's pane with the profile's views and plugins for it merged into \
         its config as it opens; Laya: off, shadow or use switches the built-in model. Keyboard \
         shortcuts lists every key; type a key (⌘D) or what it does (split) to find one. Each \
         line shows what it is: Linear's mark on tickets, GitHub's on PRs and repos, the agent's \
         on sessions, a ▸ and run on what Orbit does, go on a view; groups stand apart, \
         at most four lines each. What you ran lately and often comes first, as does what \
         waits on you. Paste a GitHub PR or Linear link (here, or Cmd+V anywhere outside a \
         field) and it opens in Orbit, the PR with its ticket.",
    ),
    step(
        None,
        None,
        "New ticket",
        "Cmd+T: type what is wrong or wanted and paste links. Haiku sorts it into bug or \
         feature, team, area and priority, and checks it is not filed already. If it is new \
         and just makes sense it is filed straight away, triaged; otherwise it asks up to three \
         short questions first. Laya, an open model built in, downloads once on its own \
         (Settings shows it) and double-checks whether it is a bug.",
    ),
    step(
        Some("copy-link"),
        None,
        "Copy link",
        "Beside Linear and GitHub, and beside each PR: Copy link puts the ticket's or the PR's \
         link on the clipboard, for Slack or a note.",
    ),
    step(
        Some("state"),
        None,
        "State and Linear",
        "Change the ticket's Linear state here, or from Cmd+K; the change shows at once. The \
         button with Linear's logo opens the ticket in Linear. o opens the PR waiting for you, \
         or the ticket when there is none.",
    ),
    step(
        Some("move-team"),
        None,
        "Move to another team",
        "Beside the state, the team: pick another (ENG to WEB or API) and the ticket moves \
         there at once, keeping its kind of state, Todo stays Todo. It gets the new team's \
         identifier once Linear answers.",
    ),
    step(
        Some("rail"),
        None,
        "Where it is",
        "Triage, plan, build, review, test, done. The current step is in the signal colour; \
         a blocked ticket shows it in red.",
    ),
    step(
        Some("your-move"),
        None,
        "Your move",
        "What the ticket needs from you, and the buttons to do it: open each linked PR (one row \
         per PR when there are several, each with its status: draft, open, approved, merging \
         while it waits in the merge queue, merged or closed), mark it done, \
         run {bot}'s next step (triage, plan, build, fix) here in the background (fix only while a PR has comments, failing checks or conflicts; Fix CI when only its checks fail, which touches nothing else); where the profile names its commands, the arrow beside it hands the step to {bot} instead. A ticket with no plan and no PR always has Plan too. A build run here on a ticket the bot holds makes it yours first, so the bot does not build it too. Or work on it with Claude. The line under the buttons says what the session is doing; the claude tab shows it.",
    ),
    step(
        Some("investigate"),
        None,
        "Investigate",
        "A ticket one of {bot}'s sweeps filed (a UX stuck finding, say) gets Investigate: here, \
         the ticket's Claude session checks the finding again and finds its cause, posting \
         nothing. Under the arrow, {bot} runs its own investigation instead, when the profile \
         names the command.",
    ),
    step(
        Some("fix-comments"),
        None,
        "Fix",
        "One Fix button for whatever blocks a PR: open review threads, red CI, changes asked \
         for, conflicts, a merge-queue drop. It says what it covers (Fix · 3 comments · CI) \
         and runs in the ticket's Claude session; comments alone run {fix}, which fixes, \
         replies and resolves each thread. Its ▼ runs one part alone.",
    ),
    step(
        Some("yolo"),
        None,
        "Yolo",
        "Yolo, next to the ticket's state, sends it straight to the bot's team in Todo, \
         whatever team or state it is in: the bot plans and builds it, no triage or plan from \
         you first. It shows on every open ticket the bot does not have yet.",
    ),
    step(
        Some("refresh-gate"),
        None,
        "Ask GitHub again",
        "↻ beside a PR's buttons asks GitHub now whether it merged, sits in the merge queue, \
         and how its checks and reviews stand, instead of waiting for the next look. The open \
         ticket's PRs are asked every 20 seconds anyway, and every window's open PRs once a \
         minute, so the sidebar's merging and next move follow GitHub too.",
    ),
    step(
        Some("fix-queue"),
        None,
        "Fix a merge-queue drop",
        "When a PR was taken out of the merge queue last time (a failure or by hand) and \
         nothing was committed since, Fix says merge queue. It runs in the \
         ticket's Claude session: finds the queue's failed run for this PR, reads the failed \
         jobs, fixes the cause on the PR's branch and pushes, or says it was a flake. Queuing it \
         again stays your click.",
    ),
    step(
        Some("laya-quick"),
        None,
        "Laya suggests",
        "With Laya on in use mode, it reads the open ticket in the background and proposes the \
         next step under your move: Plan it when the ticket says how to reproduce it and what \
         is expected, Ask for repro steps when a bug does not. Each click is logged next to \
         what it proposed, so Settings shows how often it was right.",
    ),
    step(
        Some("diff-viewed"),
        None,
        "Viewed files",
        "Tick viewed beside a file once you reviewed it: it folds and gets a check in the \
         tree, and opens again when a new commit changes it. A click on a file's path folds or \
         opens it either way.",
    ),
    step(
        None,
        None,
        "Find on screen",
        "Cmd+F finds text on whatever you are looking at, like a browser: a ticket, the diff, a \
         session. Matches are marked; Enter and Shift+Enter step through them, Esc closes.",
    ),
    step(
        Some("diff-tree"),
        None,
        "Folders beside the diff",
        "A diff with folders lists them on the side, like GitHub. Click a folder to fold it: \
         its files leave the diff, so a generated folder stays out of the way; click again to \
         bring them back.",
    ),
    step(
        Some("line-comments"),
        None,
        "Comments on the lines",
        "The Diff tab is side by side, old on the left and new on the right, each with its \
         line numbers (unified above the files for one column). It shows what people said: review threads on their lines (resolved ones \
         dimmed), beside an added or removed line in the empty column, else under it, and the conversation above the files, one line each. Click any code line to \
         comment on it; Add to review drafts it there, Comment in the box posts it at once, and \
         Comment under the files posts your drafts as one review (Request changes sends it as one). The walkthrough's suggestions sit at their lines too, one click to draft.",
    ),
    step(
        Some("ask-pr"),
        None,
        "Ask about the PR",
        "Under the walkthrough, ask Claude anything about the PR in your own words: is this \
         specific, or should it cover all of them? It answers in a few sentences from the \
         diff, the ticket and the step you are on, and remembers what you asked before. \
         Nothing is posted. Each question of the walkthrough is one line; open it for the \
         comment to the author.",
    ),
    step(
        Some("approval-note"),
        None,
        "A note with your approval",
        "Once you asked Claude about a PR, Note for the approval writes one short line from it \
         in your voice: makes sense, and what you checked. Edit it or drop it with ×; Approve \
         then posts it as the review's comment. Without questions an approval has no comment.",
    ),
    step(
        None,
        None,
        "Sent by orbit",
        "What you send from here says so: replies on a ticket, PR review comments and \
         approvals end with a short Sent by orbit line. Commits the sessions make name orbit \
         as co-author beside Claude, with the address your profile gives. The bot's own \
         commands are never changed.",
    ),
    step(
        Some("review-comments"),
        None,
        "Review comments",
        "Each question in the diff walkthrough comes with a comment ready to post: Ask posts it \
         as written, Suggest adds one of Claude's options. A click drafts it on its line, a \
         second takes it back. Comment posts them all as one review; Comment and fix also has \
         the ticket's Claude session fix them.",
    ),
    step(
        Some("review-again"),
        None,
        "Review the changes",
        "When a PR gets new commits after your review, for example from Comment and fix, the \
         diff tab and the claude tab offer Review the changes. One click loads the new diff; \
         the walkthrough first says for each of your comments whether it was handled, then \
         walks through what changed since. Review, fix, review is a loop.",
    ),
    step(
        Some("opened-by"),
        None,
        "Who opened it",
        "Next to the ticket's id, and in front of each PR's facts: opened by you in the \
         accent colour, by {bot} or another bot marked bot, anyone else by name. So you see at \
         once whose work you are reviewing.",
    ),
    step(
        Some("verdict"),
        None,
        "Can it be merged?",
        "Every open PR has one line of facts, each in its colour: ready to merge in green when \
         it is, then the three things a merge needs: approval, checks, and whether the branch \
         is up to date with its base. Red is what someone has to act on; anything else in the \
         way (a draft, the merge queue) is named once, on the same line.",
    ),
    step(
        Some("approve"),
        None,
        "Approve",
        "Shows next to a PR, and at the top of its diff, when GitHub is waiting for your review \
         and the PR is not yours. \
         One click approves it as you, at the commit you looked at; if new commits landed \
         since, it refuses and asks you to look again.",
    ),
    step(
        Some("review-menu"),
        None,
        "Comment or request changes",
        "Review ▾ beside Approve, at the top of a PR's diff: Comment, or Request changes (not \
         on your own PR). Write what you think and Send posts it as your review, with any line \
         comments you drafted, at the commit you looked at.",
    ),
    step(
        Some("ai-review"),
        None,
        "Request an AI review",
        "Your own PR cannot get your approval. While it has none, the button beside it (and at \
         the top of its diff) asks for an AI review the way your profile says: a comment such \
         as /ai-review, a label, or a reviewer. After new commits it offers Re-request.",
    ),
    step(
        Some("merge"),
        None,
        "Merge",
        "Shows next to each PR, and at the top of its diff, only when GitHub says it is clean: open, not a draft, not already \
         in the merge queue, every check green. \
         Where the base branch has a merge queue, one click adds it to the queue; without one, \
         click twice to squash-merge. Either is refused if new commits landed since. \
         Otherwise the verdict above the buttons says why not.",
    ),
    step(
        Some("merge-when-ready"),
        None,
        "Merge when ready",
        "When a PR is approved and only its checks are still running, Merge when ready (one \
         click into a merge queue, two otherwise) has GitHub merge it, or add it to the merge queue, once they pass, bound to \
         the commit you saw. New commits after that turn it off again; Turn off stops it.",
    ),
    step(
        Some("request-reporter"),
        None,
        "Request review from the reporter",
        "A ticket in review that a teammate filed offers Request review from them: they know \
         best whether it is fixed. It asks their GitHub account (found by their name in the \
         org) to review the PR, on your click.",
    ),
    step(
        Some("land-it"),
        None,
        "Land it",
        "A reviewed PR held back by a flaky or red check, or a branch behind its base, gets \
         Land it: a landing session keeps at it until the PR is merged, re-running flaky \
         checks, fixing real failures, updating the branch. Once GitHub says it is approved \
         and green, it merges through GitHub; it never approves or force-merges. It runs the \
         land skill, which you can also type in any session: /land <PR URL>.",
    ),
    step(
        Some("close-pr"),
        None,
        "Close a PR",
        "Close sits under More, last and in red: close just the PR, or, when a ticket is behind it, \
         close it and cancel the ticket in Linear too. Closed shows at once; GitHub can reopen \
         it.",
    ),
    step(
        Some("update-branch"),
        None,
        "Update branch",
        "Shows next to a PR when GitHub says its branch is behind the base, as GitHub's own \
         Update branch does. One click merges the base into the branch at the commit you saw; \
         the checks run again on the new head.",
    ),
    step(
        Some("testing-tab"),
        None,
        "Testing",
        "Every ticket has a test tab: a short plan to check the merged change. A UI \
         plan comes with a browser next to the steps; a backend plan with commands to copy. \
         Claude Sonnet writes it once and posts it on the ticket. Looks good moves the ticket \
         to done. While the PR is open in a repo that deploys previews, pick dev or the PR \
         preview above the browser: no preview yet offers its Preview buttons, a deploying \
         one says it is not ready, a ready one opens the plan's page on the preview.",
    ),
    step(
        Some("description"),
        None,
        "The ticket",
        "What the ticket is about, as it was filed, images included, right under your move. \
         Click an image to open it full size; fold the section away once you know it.",
    ),
    step(
        Some("origin"),
        None,
        "Where it came from",
        "The source, the customers, how often it was seen again, and who has it. Below: the \
         plan {bot} wrote, and what happened by day: each step of the work (planned, building, \
         a PR) with a mark and its time, the messages around it quiet and indented, long runs \
         of them folded. Click an event for the full comment.",
    ),
    step(
        Some("composer"),
        None,
        "Reply on the ticket",
        "Write a comment and send it with Cmd+Enter. It lands on the Linear ticket.",
    ),
    step(
        Some("profile"),
        Some(View::Settings),
        "Profile",
        "Everything specific to your company lives in a profile: the team's bot and its \
         commands, how tickets are routed, what counts as {ktlo}, the skills sessions get. \
         Set up with Claude asks you a few questions and writes it; or import one from a \
         folder, a file or a URL, and edit profile.json right here. Without one it is generic.",
    ),
    step(
        Some("bot-setup"),
        Some(View::Settings),
        "Bot & PRs",
        "Plain fields for what your team's bot and GitHub do: the bot's plan and build \
         commands put \"Remotely, <bot>\" under Plan and Build, and AI review, review roulette \
         and previews each add their button. Leave a field empty and its button stays hidden. \
         Your edits survive updates.",
    ),
    step(
        Some("connections"),
        Some(View::Settings),
        "Connections",
        "Cmd+, (or Settings in Cmd+K) opens Settings in place of the panes; Esc closes it. It \
         shows which tools are connected and what each one turns on. A feature whose tool is \
         not connected stays hidden or says what it needs. Paste a key to connect one; once it \
         is saved, \"change key\" under it replaces a rotated or wrong key without editing \
         settings.json.",
    ),
    step(
        Some("local-agent"),
        Some(View::Settings),
        "Claude Code, Codex or pi",
        "Pick the local agent new sessions run with: Claude Code in a terminal, your \
         installed Codex in a native pane with its answers, a prompt box and Allow or Decline \
         on every permission it asks for, or pi (when installed) in a terminal that asks before \
         what the session's rules do not allow. Every start is a split button: a click starts with the \
         default, the chevron beside it picks the agent for that one session. A session \
         keeps its agent. The Codex \
         pane logs what you asked, its answers and each command (a failed one opens its \
         output); a failed turn gets one card: what went wrong, the usage and resets when \
         that is why, what to do next, and Copy details. After a failure or a killed session, Start fresh (and its chevron) \
         gives the same brief to the agent you pick.",
    ),
    step(
        Some("update"),
        Some(View::Settings),
        "Update Orbit",
        "Orbit looks for a newer release when it starts and every hour after. When there is \
         one, Update in Settings (or Update Orbit in Cmd+K) downloads it with your gh login, \
         swaps it in and reopens the app. Sessions keep running across the restart. Below it, \
         one line says how much disk Orbit uses: old session folders are cleaned up on their own.",
    ),
    step(
        Some("themes"),
        Some(View::Settings),
        "Themes",
        "Pick a theme; it applies at once. Cmd+Shift+T cycles through them from anywhere, and \
         Cmd+K has each by name.",
    ),
    step(
        Some("settings-json"),
        Some(View::Settings),
        "settings.json",
        "Everything the app is configured with, as text: the Debug environments and their \
         commands, what Investigate opens, the test app, theme, teams and keys. Edit it and \
         press Cmd+S; a key typed here moves to the Keychain and reads SAVED.",
    ),
    step(
        Some("feedback"),
        None,
        "Feedback",
        "Something annoying, broken or missing in this app? Press Cmd+Shift+F or click \
         feedback in the bar. A Claude session reads your activity log and asks what should \
         be better. If the app already does it, it shows you how; otherwise it changes the \
         app and opens a PR for you to review.",
    ),
    step(
        Some("inbox"),
        None,
        "That's the tour",
        "i opens the inbox whenever you want to know what needs you. Start the tour again any \
         time from Cmd+K: Start the tutorial.",
    ),
];

/// Records where `anchor` is this frame, clipped to what is visible. When
/// the tutorial is on this anchor and it is scrolled out of view, scrolls
/// to it.
pub(super) fn mark(app: &App, ui: &Ui, anchor: &'static str, rect: Rect) {
    let active = app
        .tutorial
        .and_then(|i| STEPS.get(i))
        .is_some_and(|s| s.anchor == Some(anchor));
    if active && !ui.clip_rect().contains_rect(rect) {
        ui.scroll_to_rect(rect, Some(Align::Center));
    }
    let visible = rect.intersect(ui.clip_rect());
    let pass = ui.ctx().cumulative_pass_nr();
    ui.ctx()
        .data_mut(|d| d.insert_temp(Id::new(("tutorial", anchor)), (visible, pass)));
}

/// Where `anchor` was marked this frame, if it was.
fn marked(ctx: &egui::Context, anchor: &str) -> Option<Rect> {
    let (rect, pass) = ctx.data(|d| d.get_temp::<(Rect, u64)>(Id::new(("tutorial", anchor))))?;
    (pass == ctx.cumulative_pass_nr() && rect.is_positive()).then_some(rect)
}

/// The card's size before its first frame; after that, its measured size.
const CARD: Vec2 = Vec2::new(360.0, 200.0);

/// The card's top-left: right of the target, else below, else left, else
/// above; the middle of the screen when there is no target. Always on screen.
fn place(target: Option<Rect>, card: Vec2, screen: Rect) -> Pos2 {
    let gap = 14.0;
    let Some(t) = target else {
        return screen.center() - card / 2.0;
    };
    let candidates = [
        Pos2::new(t.right() + gap, t.top()),
        Pos2::new(t.left(), t.bottom() + gap),
        Pos2::new(t.left() - gap - card.x, t.top()),
        Pos2::new(t.left(), t.top() - gap - card.y),
    ];
    let fits = |p: &Pos2| screen.contains_rect(Rect::from_min_size(*p, card));
    let pos = candidates
        .into_iter()
        .find(fits)
        .unwrap_or_else(|| screen.center() - card / 2.0);
    let max = (screen.max - card).max(screen.min);
    pos.clamp(screen.min, max)
}

/// Draws the current step over everything, and takes the keys while on.
pub(super) fn overlay(app: &App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let Some(i) = app.tutorial else {
        return;
    };
    let Some(step) = STEPS.get(i) else {
        actions.push(Action::Tutorial(None));
        return;
    };
    let ctx = ui.ctx().clone();
    let screen = ctx.content_rect();
    let target = step
        .anchor
        .and_then(|a| marked(&ctx, a))
        .map(|r| r.expand(4.0));

    // Dim around the target, and swallow clicks on everything but the card.
    let layer = LayerId::new(Order::Foreground, Id::new("tutorial-scrim"));
    let painter = ctx.layer_painter(layer);
    let scrim = theme::scrim();
    match target {
        Some(t) => {
            for r in [
                Rect::from_min_max(screen.min, Pos2::new(screen.max.x, t.top())),
                Rect::from_min_max(Pos2::new(screen.min.x, t.bottom()), screen.max),
                Rect::from_min_max(
                    Pos2::new(screen.min.x, t.top()),
                    Pos2::new(t.left(), t.bottom()),
                ),
                Rect::from_min_max(
                    Pos2::new(t.right(), t.top()),
                    Pos2::new(screen.max.x, t.bottom()),
                ),
            ] {
                painter.rect_filled(r, 0.0, scrim);
            }
            painter.rect_stroke(
                t,
                0.0,
                Stroke::new(2.0, p().signal),
                egui::StrokeKind::Outside,
            );
        }
        None => {
            painter.rect_filled(screen, 0.0, scrim);
        }
    }
    egui::Area::new(Id::new("tutorial-block"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .show(&ctx, |ui| {
            ui.allocate_rect(screen, Sense::click());
        });

    let last = i + 1 == STEPS.len();
    let size_id = Id::new("tutorial-card-size");
    let size = ctx.data(|d| d.get_temp::<Vec2>(size_id)).unwrap_or(CARD);
    let card = egui::Area::new(Id::new("tutorial-card"))
        .order(Order::Tooltip)
        .fixed_pos(place(target, size, screen))
        .show(&ctx, |ui| {
            Frame::new()
                .fill(p().bg)
                .stroke(Stroke::new(1.0, p().signal))
                .inner_margin(PAD)
                .show(ui, |ui| {
                    ui.set_width(CARD.x - 2.0 * PAD as f32);
                    ui.horizontal(|ui| {
                        ui.label(label("▲ tutorial").color(p().signal));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(label(&format!("{} / {}", i + 1, STEPS.len())));
                        });
                    });
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(fill(step.title))
                            .size(16.0)
                            .family(semibold()),
                    );
                    let body = RichText::new(fill(step.body)).color(p().text_2);
                    ui.add(egui::Label::new(body).wrap());
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        let next = if last { "Done" } else { "Next" };
                        if primary(ui, next).clicked() {
                            actions.push(Action::Tutorial((!last).then_some(i + 1)));
                        }
                        if i > 0 && ui.button("Back").clicked() {
                            actions.push(Action::Tutorial(Some(i - 1)));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if !last && link(ui, label("end tour")).clicked() {
                                actions.push(Action::Tutorial(None));
                            }
                        });
                    });
                });
        });
    let measured = card.response.rect.size();
    ctx.data_mut(|d| d.insert_temp(size_id, measured));

    ui.input(|input| {
        if input.key_pressed(egui::Key::Escape) {
            actions.push(Action::Tutorial(None));
        } else if input.key_pressed(egui::Key::ArrowRight) || input.key_pressed(egui::Key::Enter) {
            actions.push(Action::Tutorial((!last).then_some(i + 1)));
        } else if input.key_pressed(egui::Key::ArrowLeft) && i > 0 {
            actions.push(Action::Tutorial(Some(i - 1)));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_name_what_the_profile_names() {
        // The tests' profile is the generic one.
        let text = fill("{Bot} runs {fix} on the {board}; ask {bot}.");
        assert!(!text.contains('{'));
        assert!(text.contains("; ask ") && text.ends_with('.'));
        for step in STEPS {
            for word in ["ABC-", "k9s", "Socket"] {
                assert!(!step.body.contains(word), "{:?} says {word}", step.title);
            }
        }
    }

    #[test]
    fn every_anchor_is_marked_in_the_ui() {
        let ui = [
            include_str!("bar.rs"),
            include_str!("list.rs"),
            include_str!("ticket.rs"),
            include_str!("sessions.rs"),
            include_str!("settings.rs"),
            include_str!("diff.rs"),
            include_str!("login.rs"),
            include_str!("panes.rs"),
            include_str!("testing.rs"),
            include_str!("mod.rs"),
        ]
        .concat();
        for anchor in STEPS.iter().filter_map(|s| s.anchor) {
            assert!(
                ui.contains(&format!("mark(app, ui, \"{anchor}\"")),
                "step anchor {anchor:?} is never marked"
            );
        }
    }

    #[test]
    fn card_stays_on_screen_and_beside_its_target() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0));
        // Room on the right: beside the target.
        let t = Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(200.0, 40.0));
        assert_eq!(place(Some(t), CARD, screen), Pos2::new(314.0, 100.0));
        // Target at the right edge: the card goes below, still on screen.
        let t = Rect::from_min_size(Pos2::new(1300.0, 100.0), Vec2::new(100.0, 40.0));
        let pos = place(Some(t), CARD, screen);
        assert!(screen.contains_rect(Rect::from_min_size(pos, CARD)));
        // No target: centred.
        assert_eq!(place(None, CARD, screen), screen.center() - CARD / 2.0);
    }
}

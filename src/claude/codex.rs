//! The local Codex app-server connection. One worker owns the daemon's
//! socket (JSON-RPC over WebSocket); the UI receives parsed events and
//! never waits on the CLI.

use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

use super::{Agent, Saved, Spec, shell};

pub(super) enum Command {
    Open {
        spec: Box<Spec>,
        saved: Option<Saved>,
        work: PathBuf,
    },
    Resume {
        key: String,
        saved: Saved,
    },
    Prompt {
        id: String,
        cwd: PathBuf,
        text: String,
        active_turn: Option<String>,
    },
    Interrupt {
        id: String,
        turn: String,
    },
    Answer {
        request: Value,
        response: Value,
    },
    List,
    Inspect(String),
    /// The account's usage windows and credits (`account/rateLimits/read`).
    Limits,
}

pub(super) enum Event {
    Started {
        key: String,
        saved: Saved,
        /// A first prompt went with it: a line typed meanwhile waits for
        /// that turn to end.
        prompted: bool,
    },
    Packet(Value),
    /// Connecting, or opening or resuming a thread, failed: retried later.
    Failed {
        key: String,
        error: String,
    },
    /// Codex refused a turn on a thread that is open (`turn/start`,
    /// `turn/steer`): that session's own trouble, not the connection's.
    TurnFailed {
        thread: String,
        error: String,
    },
    /// The daemon connection closed; threads must be resumed to be heard.
    Disconnected,
}

#[derive(Clone, Debug, Default)]
pub struct View {
    /// The conversation as it happened, by Codex's item id (an app note
    /// has none), so reading the thread again adds only what is new.
    pub log: Vec<(String, Entry)>,
    pub stream: String,
    pub draft: String,
    pub status: String,
    pub turn: Option<String>,
    pub pending: Option<Approval>,
    pub last_line: String,
    /// When the thread last changed, for "5m" and sorting.
    pub updated: Option<std::time::SystemTime>,
    /// The last error Codex gave for a turn.
    pub failure: Option<Failure>,
    /// That error is about usage or credits: the pane shows the limits.
    pub usage: bool,
    /// Codex's last message in full: a read-only run's JSON.
    pub answer: String,
}

#[derive(Clone, Debug)]
pub struct Approval {
    pub request: Value,
    pub method: String,
    pub params: Value,
    pub title: String,
    pub detail: String,
    pub question: bool,
}

pub(super) struct Client {
    tx: Sender<Command>,
    rx: Receiver<Event>,
    pub views: HashMap<String, View>,
    pub external: HashMap<String, Value>,
    /// The account's last `RateLimitSnapshot`, read after a failed turn.
    pub limits: Option<Value>,
}

impl Client {
    pub fn new() -> Self {
        let (tx, commands) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("codex-app-server".into())
            .spawn(move || worker(commands, events))
            .ok();
        Self {
            tx,
            rx,
            views: HashMap::new(),
            external: HashMap::new(),
            limits: None,
        }
    }

    pub fn send(&self, command: Command) {
        let _ = self.tx.send(command);
    }

    pub fn next(&self) -> Option<Event> {
        self.rx.try_recv().ok()
    }
}

enum Waiting {
    Start {
        key: String,
        spec: Box<Spec>,
        cwd: PathBuf,
        old: Option<Saved>,
    },
    Turn {
        id: String,
    },
    List,
    Read {
        id: String,
    },
    Inspect {
        id: String,
    },
    Limits,
}

/// How long the worker waits on the socket before looking at commands.
const TICK: Duration = Duration::from_millis(40);

type Socket = WebSocket<UnixStream>;

impl Command {
    /// The session a failed connection is reported under.
    fn key(&self) -> String {
        match self {
            Self::Open { spec, .. } => spec.key.clone(),
            Self::Resume { key, .. } => key.clone(),
            _ => String::new(),
        }
    }
}

/// Connects on the first command, and again on the next one after the
/// daemon went away. Threads live in the daemon, so they keep running
/// while the app is closed or reconnecting.
fn worker(commands: Receiver<Command>, events: Sender<Event>) {
    while let Ok(first) = commands.recv() {
        match connect() {
            Ok(socket) => serve(socket, first, &commands, &events),
            Err(error) => {
                let _ = events.send(Event::Failed {
                    key: first.key(),
                    error,
                });
                continue;
            }
        }
        let _ = events.send(Event::Disconnected);
    }
}

/// The daemon's control socket speaks JSON-RPC over WebSocket; `codex
/// app-server daemon start` starts it if needed and says where it listens.
fn connect() -> Result<Socket, String> {
    let output = shell::codex()
        .args(["app-server", "daemon", "start"])
        .output()
        .map_err(|error| format!("Codex is not installed or not on PATH: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(format!("Codex daemon: {error}"));
    }
    let path = socket_path(&String::from_utf8_lossy(&output.stdout))
        .ok_or("Codex daemon did not say where it listens")?;
    let stream =
        UnixStream::connect(&path).map_err(|error| format!("Codex daemon socket: {error}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| error.to_string())?;
    let (mut socket, _) = tungstenite::client("ws://localhost/", stream)
        .map_err(|error| format!("Codex daemon socket: {error}"))?;
    send(
        &mut socket,
        json!({ "id": 0, "method": "initialize", "params": { "clientInfo": {
            "name": "orbit", "title": "Orbit", "version": crate::VERSION
        }}}),
    )
    .map_err(|error| format!("Codex initialize: {error}"))?;
    loop {
        let packet = read(&mut socket).map_err(|error| format!("Codex initialize: {error}"))?;
        if let Some(packet) = packet
            && packet["id"] == 0
        {
            if let Some(error) = packet["error"]["message"].as_str() {
                return Err(format!("Codex initialize: {error}"));
            }
            break;
        }
    }
    send(
        &mut socket,
        json!({ "method": "initialized", "params": {} }),
    )
    .map_err(|error| error.to_string())?;
    socket
        .get_ref()
        .set_read_timeout(Some(TICK))
        .map_err(|error| error.to_string())?;
    Ok(socket)
}

/// The server requests the session pane answers; the rest are declined.
const ASKS: [&str; 4] = [
    "item/commandExecution/requestApproval",
    "item/fileChange/requestApproval",
    "item/permissions/requestApproval",
    "item/tool/requestUserInput",
];

/// A thread status (`ThreadStatus`) as the session list says it.
pub fn status(status: &Value) -> &'static str {
    let flag = |name: &str| {
        status["activeFlags"]
            .as_array()
            .is_some_and(|flags| flags.iter().any(|f| f == name))
    };
    match status["type"].as_str() {
        Some("active") if flag("waitingOnApproval") || flag("waitingOnUserInput") => {
            "needs approval"
        }
        Some("active") => "working",
        Some("systemError") => "failed",
        _ => "your turn",
    }
}

/// The daemon's JSON status line; anything else it prints is skipped.
fn socket_path(stdout: &str) -> Option<PathBuf> {
    stdout.lines().rev().find_map(|line| {
        let status: Value = serde_json::from_str(line).ok()?;
        status["socketPath"].as_str().map(PathBuf::from)
    })
}

fn send(socket: &mut Socket, value: Value) -> tungstenite::Result<()> {
    socket.send(Message::text(value.to_string()))
}

/// One packet, `None` for a non-text frame; a timeout is an `Io` error.
fn read(socket: &mut Socket) -> tungstenite::Result<Option<Value>> {
    match socket.read()? {
        Message::Text(text) => Ok(serde_json::from_str(&text).ok()),
        _ => Ok(None),
    }
}

fn timed_out(error: &tungstenite::Error) -> bool {
    matches!(error, tungstenite::Error::Io(e)
        if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}

struct Connection {
    socket: Socket,
    next_id: u64,
    waiting: HashMap<u64, Waiting>,
}

impl Connection {
    fn request(&mut self, method: &str, params: Value, job: Option<Waiting>) {
        self.next_id += 1;
        let id = self.next_id;
        let _ = send(
            &mut self.socket,
            json!({ "id": id, "method": method, "params": params }),
        );
        if let Some(job) = job {
            self.waiting.insert(id, job);
        }
    }
}

/// Until the socket closes or the app drops the client.
fn serve(socket: Socket, first: Command, commands: &Receiver<Command>, events: &Sender<Event>) {
    let mut connection = Connection {
        socket,
        next_id: 0,
        waiting: HashMap::new(),
    };
    let mut next = Some(first);
    loop {
        while let Some(command) = next.take().or_else(|| commands.try_recv().ok()) {
            connection.command(command, events);
        }
        match read(&mut connection.socket) {
            Ok(Some(packet)) => connection.packet(packet, events),
            Ok(None) => {}
            Err(error) if timed_out(&error) => {}
            Err(_) => return,
        }
        match commands.try_recv() {
            Ok(command) => next = Some(command),
            Err(mpsc::TryRecvError::Disconnected) => return,
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
}

impl Connection {
    fn packet(&mut self, packet: Value, events: &Sender<Event>) {
        let job = packet["id"]
            .as_u64()
            .filter(|_| packet.get("method").is_none())
            .and_then(|id| self.waiting.remove(&id));
        let Some(job) = job else {
            // An elicitation, a dynamic tool, a token refresh: the app
            // cannot answer them, and an open request would hold the turn.
            if let (Some(method), Some(id)) = (packet["method"].as_str(), packet.get("id"))
                && !ASKS.contains(&method)
            {
                let _ = send(
                    &mut self.socket,
                    json!({"id": id, "error": {"code": -32601,
                        "message": format!("Orbit does not handle {method}")}}),
                );
            }
            let _ = events.send(Event::Packet(packet));
            return;
        };
        match job {
            Waiting::Start {
                key,
                spec,
                cwd,
                old,
            } => {
                let Some(thread) = packet["result"]["thread"]["id"].as_str() else {
                    let error = packet["error"]["message"]
                        .as_str()
                        .unwrap_or("could not open the Codex thread");
                    let _ = events.send(Event::Failed {
                        key,
                        error: error.to_owned(),
                    });
                    return;
                };
                let thread = thread.to_owned();
                let prompt = if old.is_none() {
                    spec.prompt.clone()
                } else {
                    None
                };
                let saved = Saved {
                    agent: Agent::Codex,
                    identifier: spec.identifier,
                    title: spec.title,
                    session_id: thread.clone(),
                    cwd,
                    deny: match (spec.deny.is_empty(), &old) {
                        (true, Some(old)) => old.deny.clone(),
                        _ => spec.deny,
                    },
                    started_at: old
                        .map_or_else(|| jiff::Timestamp::now().to_string(), |s| s.started_at),
                };
                let _ = events.send(Event::Started {
                    key,
                    saved: saved.clone(),
                    prompted: prompt.is_some(),
                });
                self.request(
                    "thread/read",
                    json!({"threadId": thread, "includeTurns": true}),
                    Some(Waiting::Read { id: thread.clone() }),
                );
                if let Some(prompt) = prompt {
                    self.request(
                        "turn/start",
                        turn(&thread, &prompt, &saved.cwd),
                        Some(Waiting::Turn { id: thread }),
                    );
                }
            }
            Waiting::Turn { id } => {
                if let Some(error) = packet["error"]["message"].as_str() {
                    let _ = events.send(Event::TurnFailed {
                        thread: id,
                        error: error.to_owned(),
                    });
                }
            }
            Waiting::List => {
                let _ = events.send(Event::Packet(
                    json!({"method": "orbit/threadList", "params": packet["result"]}),
                ));
            }
            Waiting::Read { id } => {
                let _ = events.send(Event::Packet(json!({"method": "orbit/threadRead",
                    "params": {"threadId": id, "thread": packet["result"]["thread"]}})));
            }
            Waiting::Inspect { id } => self.request(
                "thread/read",
                json!({"threadId": id, "includeTurns": true}),
                Some(Waiting::Read { id }),
            ),
            Waiting::Limits => {
                let _ = events.send(Event::Packet(
                    json!({"method": "orbit/rateLimits", "params": packet["result"]}),
                ));
            }
        }
    }

    fn command(&mut self, command: Command, events: &Sender<Event>) {
        match command {
            Command::Open { spec, saved, work } => {
                let key = spec.key.clone();
                let cwd = saved
                    .as_ref()
                    .map_or_else(|| work.join(&spec.folder), |s| s.cwd.clone());
                if let Err(error) = write_skills(&cwd, &spec.skills) {
                    let _ = events.send(Event::Failed {
                        key,
                        error: error.to_string(),
                    });
                    return;
                }
                // A resume gets the brief and the policies again, as a
                // Claude session gets them on every start.
                let mut params = start_params(&spec, &cwd);
                let method = match &saved {
                    Some(old) => {
                        params["threadId"] = json!(old.session_id);
                        "thread/resume"
                    }
                    None => "thread/start",
                };
                self.request(
                    method,
                    params,
                    Some(Waiting::Start {
                        key,
                        spec,
                        cwd,
                        old: saved,
                    }),
                );
            }
            Command::Resume { key, saved } => {
                let spec = Spec {
                    key: key.clone(),
                    identifier: saved.identifier.clone(),
                    title: saved.title.clone(),
                    folder: String::new(),
                    context: String::new(),
                    prompt: None,
                    skills: Vec::new(),
                    allow: Vec::new(),
                    deny: Vec::new(),
                };
                self.request(
                    "thread/resume",
                    json!({"threadId": saved.session_id}),
                    Some(Waiting::Start {
                        key,
                        spec: Box::new(spec),
                        cwd: saved.cwd.clone(),
                        old: Some(saved),
                    }),
                );
            }
            Command::Prompt {
                id,
                cwd,
                text,
                active_turn,
            } => {
                let (method, params) = match active_turn {
                    Some(turn_id) => (
                        "turn/steer",
                        json!({"threadId": id, "expectedTurnId": turn_id,
                            "input": prompt_input(&text, &cwd)}),
                    ),
                    None => ("turn/start", turn(&id, &text, &cwd)),
                };
                self.request(method, params, Some(Waiting::Turn { id }));
            }
            Command::Interrupt { id, turn } => self.request(
                "turn/interrupt",
                json!({"threadId": id, "turnId": turn}),
                None,
            ),
            Command::Answer { request, response } => {
                let _ = send(
                    &mut self.socket,
                    json!({ "id": request, "result": response }),
                );
            }
            Command::List => self.request(
                "thread/list",
                json!({"limit": 100, "sourceKinds": ["cli", "vscode", "appServer"]}),
                Some(Waiting::List),
            ),
            Command::Inspect(id) => self.request(
                "thread/resume",
                json!({"threadId": id}),
                Some(Waiting::Inspect { id }),
            ),
            Command::Limits => self.request(
                "account/rateLimits/read",
                json!({"excludeResetCreditDetails": true}),
                Some(Waiting::Limits),
            ),
        }
    }
}

/// The app's and the profile's skills, where Codex finds a repo's own.
fn write_skills(cwd: &Path, skills: &[(String, String)]) -> std::io::Result<()> {
    std::fs::create_dir_all(cwd)?;
    for (name, body) in skills {
        let dir = cwd.join(".agents/skills").join(name);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("SKILL.md"), body)?;
    }
    Ok(())
}

/// Goals and the spotter only read: Codex answers with the JSON and the
/// app validates and saves it. Every approval goes to the user.
fn start_params(spec: &Spec, cwd: &Path) -> Value {
    let reading = matches!(spec.key.as_str(), super::GOALS | super::SPOTTER);
    let mut instructions = spec.context.clone();
    if reading {
        instructions.push_str(
            "\n\nFor this Codex run, do not write files, even where the skill says to write \
             one: the folder is read-only. Return only the JSON object the skill describes as \
             your final answer, with no code fence. Orbit validates and saves it. Never use a \
             tool that changes an external service.",
        );
    }
    if !spec.deny.is_empty() {
        // Codex has no per-run deny list; Orbit declines these when they
        // come up for approval (`denied`), and says so here up front.
        instructions.push_str(&format!(
            "\n\nNever run these in this session; Orbit declines them: {}.",
            spec.deny.join(", ")
        ));
    }
    // Commits carry the profile's co-author (the line Claude sessions get
    // from `attribution.commit`), else no trailer at all.
    match crate::profile::get().attribution.co_author() {
        Some(line) => instructions.push_str(&format!(
            "\n\nEnd every commit message with this line, and no other trailer: {line}"
        )),
        None => instructions
            .push_str("\n\nDo not add agent attribution or co-author trailers to commits."),
    }
    json!({
        "cwd": cwd,
        "developerInstructions": instructions,
        "approvalPolicy": "on-request",
        "approvalsReviewer": "user",
        "sandbox": if reading { "read-only" } else { "workspace-write" },
        "serviceName": "orbit"
    })
}

/// The deny rule a command Codex asks to run falls under, if any. A rule
/// `Bash(gh pr merge*)` matches wherever its parts appear in order, so a
/// wrapped `zsh -lc 'gh pr merge 1'` is declined too; MCP rules name no
/// command and are left to the instructions.
pub fn denied<'a>(command: &str, deny: &'a [String]) -> Option<&'a str> {
    deny.iter()
        .filter_map(|rule| rule.strip_prefix("Bash(")?.strip_suffix(')'))
        .find(|pattern| {
            let mut rest = command;
            pattern.split('*').filter(|p| !p.is_empty()).all(|part| {
                rest.find(part)
                    .map(|at| rest = &rest[at + part.len()..])
                    .is_some()
            })
        })
}

/// One step of the conversation, for the pane's log.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// What the user (or a button) sent.
    You(String),
    Codex(String),
    Command {
        command: String,
        /// `completed`, `failed`, `declined`, `inProgress`.
        status: String,
        exit: Option<i64>,
        ms: Option<u64>,
        output: String,
    },
    Tool {
        name: String,
        status: String,
        error: Option<String>,
    },
    /// What the app did on its own: a declined command.
    Note(String),
}

impl Entry {
    /// A command or tool that did not work: the log opens its output.
    pub fn failed(&self) -> bool {
        match self {
            Self::Command { status, exit, .. } => {
                status == "failed" || status == "declined" || exit.is_some_and(|e| e != 0)
            }
            Self::Tool { status, error, .. } => status == "failed" || error.is_some(),
            _ => false,
        }
    }
}

/// A `ThreadItem` as a log entry, with its id; `None` for what the log
/// does not show (reasoning, plans, compaction).
pub fn entry(item: &Value) -> Option<(String, Entry)> {
    let id = item["id"].as_str().unwrap_or_default().to_owned();
    let text = |v: &Value| v.as_str().map(str::to_owned);
    let entry = match item["type"].as_str()? {
        "userMessage" => Entry::You(
            item["content"]
                .as_array()?
                .iter()
                .filter_map(|c| c["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        "agentMessage" => Entry::Codex(text(&item["text"])?),
        "commandExecution" => Entry::Command {
            command: text(&item["command"])?,
            status: text(&item["status"]).unwrap_or_default(),
            exit: item["exitCode"].as_i64(),
            ms: item["durationMs"].as_u64(),
            output: text(&item["aggregatedOutput"]).unwrap_or_default(),
        },
        "mcpToolCall" | "dynamicToolCall" => Entry::Tool {
            name: match item["server"].as_str() {
                Some(server) => format!("{server}.{}", item["tool"].as_str()?),
                None => text(&item["tool"])?,
            },
            status: text(&item["status"]).unwrap_or_default(),
            error: item["error"]["message"]
                .as_str()
                .or_else(|| item["error"].as_str())
                .map(str::to_owned),
        },
        _ => return None,
    };
    Some((id, entry))
}

/// Why a turn failed, as Codex said it (`TurnError`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Failure {
    pub message: String,
    pub details: Option<String>,
    /// `codexErrorInfo`: `usageLimitExceeded`, `httpConnectionFailed`, …
    pub code: String,
    pub http: Option<u64>,
}

/// What kind of trouble a failure is, for its title and what to offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trouble {
    /// Out of usage or credits: the limits, and the other agent.
    Usage,
    /// The conversation outgrew the model: start it again.
    TooLong,
    /// The model could not be reached or answered with an error: retry.
    Unreachable,
    /// Codex's login is gone: `codex login`, then retry.
    SignedOut,
    Other,
}

impl Failure {
    pub fn trouble(&self) -> Trouble {
        let message = self.message.to_lowercase();
        match self.code.as_str() {
            "usageLimitExceeded" | "rateLimitExceeded" | "sessionBudgetExceeded" => Trouble::Usage,
            "contextWindowExceeded" => Trouble::TooLong,
            "unauthorized" => Trouble::SignedOut,
            "serverOverloaded"
            | "internalServerError"
            | "httpConnectionFailed"
            | "responseStreamConnectionFailed"
            | "responseStreamDisconnected"
            | "responseTooManyFailedAttempts" => Trouble::Unreachable,
            _ if ["credits", "usage limit", "rate limit", "quota"]
                .iter()
                .any(|w| message.contains(w)) =>
            {
                Trouble::Usage
            }
            _ => Trouble::Other,
        }
    }

    /// The card's title, in the user's terms.
    pub fn title(&self) -> &'static str {
        match self.trouble() {
            Trouble::Usage if self.message.to_lowercase().contains("credits") => "Out of credits",
            Trouble::Usage => "Out of usage",
            Trouble::TooLong => "The conversation is too long",
            Trouble::Unreachable => "Codex could not reach its model",
            Trouble::SignedOut => "Codex is signed out",
            Trouble::Other => "Codex failed",
        }
    }

    /// Everything, for Copy details.
    pub fn report(&self) -> String {
        let mut out = self.message.clone();
        if let Some(details) = &self.details {
            out.push_str(&format!("\n{details}"));
        }
        if !self.code.is_empty() {
            out.push_str(&format!("\ncode: {}", self.code));
        }
        if let Some(http) = self.http {
            out.push_str(&format!("\nhttp: {http}"));
        }
        out
    }
}

/// A turn's error (`TurnError`), when it has a message.
pub fn failure(error: &Value) -> Option<Failure> {
    let info = &error["codexErrorInfo"];
    // A plain code, or an object named by its one key.
    let (code, inner) = match info {
        Value::String(code) => (code.clone(), &Value::Null),
        Value::Object(map) => map
            .iter()
            .next()
            .map_or((String::new(), &Value::Null), |(k, v)| (k.clone(), v)),
        _ => (String::new(), &Value::Null),
    };
    Some(Failure {
        message: error["message"].as_str()?.to_owned(),
        details: error["additionalDetails"]
            .as_str()
            .filter(|d| !d.is_empty())
            .map(str::to_owned),
        code,
        http: inner["httpStatusCode"].as_u64(),
    })
}

/// Whether the error is about usage or credits, so the limits are worth
/// reading and showing.
pub fn about_usage(error: &Value) -> bool {
    failure(error).is_some_and(|f| f.trouble() == Trouble::Usage)
}

/// One usage window: how much is used and when it resets.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// "5 h", "weekly".
    pub span: String,
    pub used: i64,
    /// "Mon 19:29 (in 39m)".
    pub resets: Option<String>,
}

/// The account's usage for the pane: every window, then one line each for
/// the credits and why Codex stopped. `now` in Unix seconds; `at` formats
/// a reset time on the user's clock.
pub fn usage(limits: &Value, now: i64, at: impl Fn(i64) -> String) -> (Vec<Window>, Vec<String>) {
    let mut windows = Vec::new();
    for window in ["primary", "secondary"] {
        let window = &limits[window];
        let Some(used) = window["usedPercent"].as_i64() else {
            continue;
        };
        let span = match window["windowDurationMins"].as_i64() {
            Some(10_080) => "weekly".to_owned(),
            Some(mins) if mins % 1440 == 0 => format!("{} d", mins / 1440),
            Some(mins) if mins % 60 == 0 => format!("{} h", mins / 60),
            Some(mins) => format!("{mins} min"),
            None => "window".to_owned(),
        };
        let resets = window["resetsAt"].as_i64().map(|reset| {
            let left = (reset - now).max(0);
            let wait = match left {
                s if s < 3600 => format!("{}m", s / 60),
                s if s < 86_400 => format!("{}h {}m", s / 3600, s % 3600 / 60),
                s => format!("{}d {}h", s / 86_400, s % 86_400 / 3600),
            };
            format!("{} (in {wait})", at(reset))
        });
        windows.push(Window { span, used, resets });
    }
    let mut notes = Vec::new();
    let credits = &limits["credits"];
    if credits["unlimited"] == true {
        notes.push("credits: unlimited".into());
    } else if let Some(balance) = credits["balance"].as_str() {
        notes.push(format!("credits: {balance}"));
    } else if credits["hasCredits"] == false {
        notes.push("credits: none left".into());
    }
    if let Some(why) = limits["rateLimitReachedType"].as_str() {
        notes.push(format!("reason: {}", why.replace('_', " ")));
    }
    (windows, notes)
}

/// The JSON a read-only run answered with: bare, or in a code fence.
pub fn answer_json(text: &str) -> Option<Value> {
    let text = text.trim();
    let inner = text
        .strip_prefix("```json")
        .or_else(|| text.strip_prefix("```"))
        .and_then(|t| t.strip_suffix("```"))
        .unwrap_or(text);
    serde_json::from_str(inner.trim()).ok()
}

fn turn(thread: &str, prompt: &str, cwd: &std::path::Path) -> Value {
    json!({"threadId": thread, "input": prompt_input(prompt, cwd)})
}

fn prompt_input(prompt: &str, cwd: &std::path::Path) -> Vec<Value> {
    let Some(without_slash) = prompt.strip_prefix('/') else {
        return vec![json!({"type":"text", "text":prompt})];
    };
    let (name, rest) = without_slash
        .split_once([' ', '\n'])
        .unwrap_or((without_slash, ""));
    let path = cwd.join(".agents/skills").join(name).join("SKILL.md");
    if !path.is_file() {
        return vec![json!({"type":"text", "text":prompt})];
    }
    vec![
        json!({"type":"text", "text":format!("${name} {rest}")}),
        json!({"type":"skill", "name":name, "path":path}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_sends_the_prompt_to_the_saved_thread() {
        let turn = turn(
            "thread-1",
            "Please review this",
            std::path::Path::new("/tmp"),
        );
        assert_eq!(turn["threadId"], "thread-1");
        assert_eq!(turn["input"][0]["text"], "Please review this");
    }

    /// Needs Codex installed and logged in: `cargo test daemon -- --ignored`.
    #[test]
    #[ignore = "talks to the local Codex daemon"]
    fn the_local_daemon_lists_threads() {
        let mut socket = connect().unwrap();
        send(
            &mut socket,
            json!({"id": 1, "method": "thread/list", "params": {"limit": 1}}),
        )
        .unwrap();
        let listed = std::iter::repeat_with(|| read(&mut socket))
            .take(200)
            .find_map(|packet| packet.ok().flatten().filter(|p| p["id"] == 1))
            .unwrap();
        assert!(listed["result"]["data"].is_array(), "{listed}");
    }

    #[test]
    fn every_command_sent_while_the_worker_waits_reaches_the_daemon() {
        let (client, server) = UnixStream::pair().unwrap();
        let daemon = std::thread::spawn(move || {
            let mut socket = tungstenite::accept(server).unwrap();
            let mut methods = Vec::new();
            while methods.len() < 6 {
                if let Ok(Message::Text(text)) = socket.read() {
                    let packet: Value = serde_json::from_str(&text).unwrap();
                    methods.push(packet["method"].as_str().unwrap_or_default().to_owned());
                }
            }
            methods
        });
        let (socket, _) = tungstenite::client("ws://localhost/", client).unwrap();
        socket.get_ref().set_read_timeout(Some(TICK)).unwrap();
        let (tx, commands) = mpsc::channel();
        let (events, _rx) = mpsc::channel();
        let worker = std::thread::spawn(move || serve(socket, Command::List, &commands, &events));
        for turn in 0..5 {
            std::thread::sleep(Duration::from_millis(15));
            let (id, turn) = ("t".to_owned(), turn.to_string());
            tx.send(Command::Interrupt { id, turn }).unwrap();
        }
        let methods = daemon.join().unwrap();
        drop(tx);
        worker.join().unwrap();
        assert_eq!(methods.iter().filter(|m| *m == "turn/interrupt").count(), 5);
    }

    #[test]
    fn a_denied_command_is_found_inside_a_shell_wrapper() {
        let deny = [
            "Bash(gh pr merge*)".to_owned(),
            "Bash(gh api *-X*)".to_owned(),
            "mcp__x__y".to_owned(),
        ];
        assert_eq!(
            denied("/bin/zsh -lc 'gh pr merge 12'", &deny),
            Some("gh pr merge*")
        );
        assert_eq!(
            denied("gh api repos/a/b -X DELETE", &deny),
            Some("gh api *-X*")
        );
        assert_eq!(denied("gh pr view 12", &deny), None);
    }

    #[test]
    fn a_usage_failure_shows_each_window_and_when_it_resets() {
        let limits = json!({
            "primary": {"usedPercent": 100, "windowDurationMins": 300, "resetsAt": 11_000},
            "secondary": {"usedPercent": 54, "windowDurationMins": 10080, "resetsAt": 100_000},
            "credits": {"hasCredits": false, "unlimited": false, "balance": null},
            "rateLimitReachedType": "workspace_member_credits_depleted"
        });
        let (windows, notes) = usage(&limits, 1_000, |t| format!("@{t}"));
        assert_eq!(
            windows,
            [
                Window {
                    span: "5 h".into(),
                    used: 100,
                    resets: Some("@11000 (in 2h 46m)".into())
                },
                Window {
                    span: "weekly".into(),
                    used: 54,
                    resets: Some("@100000 (in 1d 3h)".into())
                },
            ]
        );
        assert_eq!(
            notes,
            [
                "credits: none left",
                "reason: workspace member credits depleted"
            ]
        );
    }

    #[test]
    fn a_failure_is_named_by_what_went_wrong() {
        let credits = failure(&json!({"message": "Your workspace is out of credits.",
            "codexErrorInfo": "other"}))
        .unwrap();
        assert_eq!(
            (credits.trouble(), credits.title()),
            (Trouble::Usage, "Out of credits")
        );
        let long =
            failure(&json!({"message": "x", "codexErrorInfo": "contextWindowExceeded"})).unwrap();
        assert_eq!(long.trouble(), Trouble::TooLong);
        let net = failure(
            &json!({"message": "stream closed", "additionalDetails": "retry later",
            "codexErrorInfo": {"responseStreamDisconnected": {"httpStatusCode": 502}}}),
        )
        .unwrap();
        assert_eq!(net.trouble(), Trouble::Unreachable);
        assert_eq!(
            net.report(),
            "stream closed\nretry later\ncode: responseStreamDisconnected\nhttp: 502"
        );
    }

    #[test]
    fn items_become_log_entries_and_a_failed_command_opens() {
        let (id, command) = entry(&json!({"id": "i1", "type": "commandExecution",
            "command": "cargo test", "status": "completed", "exitCode": 101,
            "durationMs": 2300, "aggregatedOutput": "1 failed"}))
        .unwrap();
        assert_eq!(id, "i1");
        assert!(command.failed());
        let (_, you) = entry(&json!({"id": "i2", "type": "userMessage",
            "content": [{"type": "text", "text": "Say OK"}]}))
        .unwrap();
        assert_eq!(you, Entry::You("Say OK".into()));
        assert!(entry(&json!({"id": "i3", "type": "reasoning"})).is_none());
    }

    #[test]
    fn a_fenced_answer_is_still_json() {
        assert_eq!(answer_json("```json\n{\"a\": 1}\n```").unwrap()["a"], 1);
        assert_eq!(answer_json(" {\"a\": 2} ").unwrap()["a"], 2);
        assert!(answer_json("Here you go").is_none());
    }

    #[test]
    fn the_socket_is_read_from_the_daemon_status_line() {
        let stdout =
            "WARNING: no aliases\n{\"status\":\"running\",\"socketPath\":\"/tmp/c.sock\"}\n";
        assert_eq!(socket_path(stdout), Some(PathBuf::from("/tmp/c.sock")));
        assert_eq!(socket_path("nothing"), None);
    }

    #[test]
    fn a_skill_command_becomes_a_skill_input() {
        let cwd = std::env::temp_dir().join(format!("orbit-codex-skill-{}", std::process::id()));
        write_skills(&cwd, &[("plan".into(), "---\nname: plan\n---".into())]).unwrap();
        let input = prompt_input("/plan DEV-1", &cwd);
        assert_eq!(input[0]["text"], "$plan DEV-1");
        assert_eq!(input[1]["type"], "skill");
        assert_eq!(prompt_input("/unknown", &cwd)[0]["text"], "/unknown");
        let _ = std::fs::remove_dir_all(&cwd);
    }

    #[test]
    fn a_thread_waiting_on_the_user_needs_them_and_an_idle_one_is_their_turn() {
        let active = json!({"type": "active", "activeFlags": ["waitingOnUserInput"]});
        assert_eq!(status(&active), "needs approval");
        assert_eq!(
            status(&json!({"type": "active", "activeFlags": []})),
            "working"
        );
        assert_eq!(status(&json!({"type": "idle"})), "your turn");
        assert_eq!(status(&json!({"type": "systemError"})), "failed");
    }

    #[test]
    fn goals_and_the_spotter_start_read_only() {
        let spec = |key: &str| Spec {
            key: key.into(),
            identifier: String::new(),
            title: String::new(),
            folder: String::new(),
            context: "brief".into(),
            prompt: None,
            skills: Vec::new(),
            allow: Vec::new(),
            deny: Vec::new(),
        };
        let cwd = Path::new("/tmp");
        assert_eq!(
            start_params(&spec(super::super::GOALS), cwd)["sandbox"],
            "read-only"
        );
        assert_eq!(
            start_params(&spec("DEV-1"), cwd)["sandbox"],
            "workspace-write"
        );
        assert_eq!(
            start_params(&spec("DEV-1"), cwd)["approvalsReviewer"],
            "user"
        );
    }
}

//! `orbit ctl`: another program presses Orbit's buttons. The first running
//! copy serves `<data>/orbit.sock` (mode 600); one JSON line in, one JSON
//! line out. The app answers on its UI thread (app/control.rs), so the
//! window shows each action as it happens, and every one lands in
//! activity.jsonl with `via: "cli"`: what a person did and what a program
//! did stay apart. Actions only the user may take (gate `Yours`: approve,
//! merge, a permission prompt) are refused here, never queued.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

/// One request, and where its answer goes.
pub struct Request {
    pub body: Value,
    pub reply: mpsc::Sender<Value>,
}

pub fn path() -> PathBuf {
    crate::store::data_dir().join("orbit.sock")
}

/// Serves the socket unless another copy already does (it answers a
/// connect). A file left by a copy that quit is removed first.
/// ponytail: the copy that serves quitting leaves the others unserved
/// until one restarts; hand over on quit if two copies matter.
pub fn serve(ctx: egui::Context) -> Option<mpsc::Receiver<Request>> {
    let path = path();
    if UnixStream::connect(&path).is_ok() {
        return None;
    }
    let _ = std::fs::remove_file(&path);
    // Only this user may reach the socket, even in the moment between bind
    // and the chmod below: its folder (the data folder, keys.json's too) is
    // private first.
    if let Some(dir) = path.parent() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    let listener = UnixListener::bind(&path).ok()?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (tx, ctx) = (tx.clone(), ctx.clone());
            std::thread::spawn(move || answer(stream, &tx, &ctx));
        }
    });
    Some(rx)
}

/// The longest request line read; a longer one is cut there (and then
/// fails to parse), so a client cannot fill memory.
const MAX_LINE: u64 = 64 * 1024;

fn read_request(reader: impl std::io::Read) -> Option<String> {
    let mut line = String::new();
    BufReader::new(reader.take(MAX_LINE))
        .read_line(&mut line)
        .ok()?;
    Some(line)
}

fn answer(stream: UnixStream, tx: &mpsc::Sender<Request>, ctx: &egui::Context) {
    let Some(line) = read_request(&stream) else {
        return;
    };
    let body =
        serde_json::from_str(&line).unwrap_or_else(|e| json!({"cmd": "", "error": e.to_string()}));
    let (reply, back) = mpsc::channel();
    if tx.send(Request { body, reply }).is_err() {
        return;
    }
    // The app wakes for it even while its window is hidden.
    ctx.request_repaint();
    let out = back
        .recv_timeout(Duration::from_secs(15))
        .unwrap_or_else(|_| json!({"ok": false, "error": "Orbit did not answer within 15s"}));
    let _ = writeln!(&stream, "{out}");
}

/// Sends one request to the running app and returns its answer.
pub fn ask(body: &Value) -> Result<Value, String> {
    let path = path();
    let mut stream = UnixStream::connect(&path)
        .map_err(|_| format!("Orbit is not running (nothing serves {})", path.display()))?;
    writeln!(stream, "{body}").map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(&stream)
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&line).map_err(|e| format!("{e}: {line}"))
}

pub const HELP: &str = "\
orbit ctl: drive the running Orbit app. Every command prints JSON. Every
action shows in Orbit's window as it happens, with a grey \"agent\" toast,
and is logged as done by a program (via \"cli\"), never as the user's own.

Read:
  orbit ctl state                 where Orbit is: view, tab, open ticket, counts
  orbit ctl tickets [all]         For you in its order (all: every board card)
  orbit ctl ticket <DEV-1|id>     one ticket: card, PRs with their gates, session
  orbit ctl sessions              every session: key, ticket, agent, state, and
                                  `work`: its ticket's Linear state or its PR's
                                  (open, merged, closed), `finished` when done;
                                  `run`: its last note, the question or approval
                                  it waits on (`asks`), its PR, why it failed
  orbit ctl inbox                 Notifications rows
  orbit ctl alerts                alerts of the last day, live incidents, release runs,
                                  and the sessions already on them
  orbit ctl prs                   the user's own open PRs, with their gates
  orbit ctl actions [name]        every action: gate, payload and what it does
  orbit ctl log [n]               the last n activity lines (default 20)

Act:
  orbit ctl do '<Action JSON>'    press one button, e.g. '{\"Show\":\"Sessions\"}',
                                  '\"Refresh\"', '{\"Select\":\"<ticket id>\"}'
  orbit ctl show <View>           ForYou, Intake, Sessions, Ktlo, Inbox, Alerts, Repos, ...
  orbit ctl open <DEV-1|id>       open a ticket in the active pane
  orbit ctl close <DEV-1|key|short id>
                                  close a session: stopped and off Orbit's list;
                                  Claude Code keeps the conversation

Linear (Orbit's own key, no connector needed; gate queue: only when the user
asked; ok true only once Linear confirmed):
  orbit ctl comment <DEV-1> <text…>
                                  a comment; text holding `@<name> <Word>!` (a bot
                                  command) is refused
  orbit ctl move <DEV-1> <state…> to the state of that name in the ticket's team,
                                  any case (\"in review\"); a wrong name lists them
  orbit ctl parent <DEV-1> <PARENT|none>
                                  put the ticket under PARENT, or take it out
  orbit ctl block <BLOCKER> <BLOCKED>
                                  a blocks relation: BLOCKED waits for BLOCKER

Missions (one parent ticket on top, its sub-tickets driven through the
team's bot, over days):
  orbit ctl instruct <text…>      a new mission from a goal in your words (a
                                  feature, a fix reported in Slack): research,
                                  file a parent with sub-tickets, then drive it
  orbit ctl mission <PARENT> [n] [id]
                                  a mission on a parent that has sub-tickets:
                                  moves them to the bot (Todo), n at a time
                                  (default 2), each once its blockers merged;
                                  Orbit waits on the bot and tells the user what
                                  is theirs (a PR to review, a block, a merge)
  orbit ctl follow <PARENT>        built here in Orbit sessions instead: nothing
                                  goes to the bot; the clone panel lists each
                                  sub-ticket's step (building, fixing comments,
                                  your review, merging, waiting on, done)
  orbit ctl mission stop <PARENT|id>
                                  stop it
  orbit ctl missions              each mission, where its sub-tickets stand, its log

Tell the user:
  orbit ctl say <text…>           one line on what you did and why: a grey agent
                                  toast, and a Mac notification while Orbit is
                                  behind other windows

Wait:
  orbit ctl wait <DEV-1|key> <state…> [--timeout 600]
                                  until the ticket's session label is one of
                                  the states (working, your turn, needs approval,
                                  done, stopped, failed); exits 1 on timeout

Gates (orbit ctl actions): look and do are yours to press; queue changes
what other people see, so press it only when the user asked for it; app is
Orbit's own settings; yours (approve, merge, a permission prompt) is refused:
only the user's click does it. Ticket ids in actions are Linear ids (the
`id` field), not identifiers like DEV-1; `orbit ctl actions <name>` gives a
payload to copy (`example`). An action naming a ticket, state or PR Orbit
does not know is refused with why, nothing pressed.
";

/// `orbit ctl <args>`: the exit code.
pub fn main(args: &[String]) -> i32 {
    let arg = |i: usize| args.get(i).map(String::as_str);
    let body = match (arg(0), arg(1)) {
        (None | Some("help" | "--help" | "-h"), _) => {
            print!("{HELP}");
            return 0;
        }
        (Some("log"), n) => {
            let n = n.and_then(|n| n.parse().ok()).unwrap_or(20);
            let text = std::fs::read_to_string(crate::store::data_dir().join("activity.jsonl"))
                .unwrap_or_default();
            let lines: Vec<&str> = text.lines().collect();
            for line in &lines[lines.len().saturating_sub(n)..] {
                println!("{line}");
            }
            return 0;
        }
        (Some("wait"), Some(key)) => return wait(key, &args[2..]),
        (Some("do"), Some(action)) => match serde_json::from_str::<Value>(action) {
            Ok(action) => json!({"cmd": "do", "action": action}),
            Err(e) => {
                eprintln!("not JSON: {e}");
                return 2;
            }
        },
        (Some("show"), Some(view)) => json!({"cmd": "do", "action": {"Show": view}}),
        (Some("open"), Some(id)) => json!({"cmd": "open", "id": id}),
        (Some("close"), Some(id)) => json!({"cmd": "close", "id": id}),
        // `drive`, `undrive` and `initiatives`: the old names, still taken.
        (Some("mission"), Some("stop")) | (Some("undrive"), Some(_)) => {
            let target = if arg(0) == Some("undrive") {
                arg(1)
            } else {
                arg(2)
            };
            let Some(target) = target else {
                eprintln!("orbit ctl mission stop <PARENT|id>");
                return 2;
            };
            json!({"cmd": "do", "action": {"StopDrive": target}})
        }
        (Some("mission" | "drive"), Some(parent)) => json!({"cmd": "do", "action": {"Drive": {
            "parent": parent,
            "parallel": arg(2).and_then(|n| n.parse::<usize>().ok()).unwrap_or(2),
            "initiative": arg(3),
            "session": std::env::var("ORBIT_SESSION_KEY").ok(),
        }}}),
        (Some("follow"), Some(parent)) => json!({"cmd": "do", "action": {"Drive": {
            "parent": parent, "parallel": 1, "local": true,
            "session": std::env::var("ORBIT_SESSION_KEY").ok(),
        }}}),
        (Some("instruct"), Some(_)) => {
            json!({"cmd": "do", "action": {"Instruct": args[1..].join(" ")}})
        }
        (Some("missions" | "initiatives"), _) => json!({"cmd": "missions"}),
        (Some("sessions"), _) => return print_answer(sessions()),
        (Some(cmd @ ("ticket" | "tickets" | "actions")), id) => json!({"cmd": cmd, "id": id}),
        (Some("prs"), _) => return print_answer(prs()),
        (Some(cmd @ ("state" | "inbox" | "alerts")), _) => json!({"cmd": cmd}),
        (Some("say"), Some(_)) => json!({"cmd": "say", "text": args[1..].join(" ")}),
        (Some("comment" | "move" | "parent" | "block"), _) => match write_of(args) {
            Some(write) => json!({"cmd": "write", "write": write}),
            None => {
                eprint!("{HELP}");
                return 2;
            }
        },
        _ => {
            eprint!("{HELP}");
            return 2;
        }
    };
    print_answer(ask(&body))
}

/// `comment <DEV-1> <text…>`, `move <DEV-1> <state…>`, `parent <DEV-1>
/// <PARENT|none>`, `block <BLOCKER> <BLOCKED>`: a Linear write.
fn write_of(args: &[String]) -> Option<crate::linear::Write> {
    use crate::linear::Write;
    let (verb, issue, rest) = (args.first()?, args.get(1)?.clone(), args.get(2..)?);
    let text = rest.join(" ");
    if text.trim().is_empty() {
        return None;
    }
    Some(match verb.as_str() {
        "comment" => Write::Comment { issue, body: text },
        "move" => Write::Move { issue, state: text },
        "parent" if rest.len() == 1 => Write::Parent {
            issue,
            parent: Some(text).filter(|p| !p.eq_ignore_ascii_case("none")),
        },
        "block" if rest.len() == 1 => Write::Block {
            blocker: issue,
            blocked: text,
        },
        _ => return None,
    })
}

fn print_answer(answer: Result<Value, String>) -> i32 {
    match answer {
        Ok(out) => {
            println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
            i32::from(out["ok"] == false)
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// The user's PRs, asked again while the app is still loading them or
/// their gates, at most 20 seconds.
fn prs() -> Result<Value, String> {
    let until = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let out = ask(&json!({"cmd": "prs"}))?;
        let pending = out["pending"] == true
            || out["prs"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|r| r["gate"]["pending"] == true));
        if !pending || std::time::Instant::now() > until {
            return Ok(out);
        }
        std::thread::sleep(Duration::from_millis(700));
    }
}

/// Every session with its work's state: asked again while the app is
/// still looking some up (Linear, GitHub), at most 20 seconds.
fn sessions() -> Result<Value, String> {
    let until = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let out = ask(&json!({"cmd": "sessions"}))?;
        let pending = out["sessions"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|r| r["work"]["pending"] == true));
        if !pending || std::time::Instant::now() > until {
            return Ok(out);
        }
        std::thread::sleep(Duration::from_millis(700));
    }
}

/// Polls the ticket's (or session key's) state until it is one of `rest`.
fn wait(key: &str, rest: &[String]) -> i32 {
    let mut timeout = 600u64;
    let mut states = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        if a == "--timeout" {
            timeout = it.next().and_then(|t| t.parse().ok()).unwrap_or(timeout);
        } else {
            states.push(a.as_str());
        }
    }
    let until = std::time::Instant::now() + Duration::from_secs(timeout);
    loop {
        let state = ask(&json!({"cmd": "ticket", "id": key})).map(|out| {
            out["session"]["state"]
                .as_str()
                .unwrap_or("none")
                .to_owned()
        });
        match state {
            Ok(state) if states.is_empty() || states.contains(&state.as_str()) => {
                println!("{}", json!({"ok": true, "state": state}));
                return 0;
            }
            Err(e) => {
                eprintln!("{e}");
                return 1;
            }
            Ok(state) if std::time::Instant::now() > until => {
                println!(
                    "{}",
                    json!({"ok": false, "state": state, "error": "timed out"})
                );
                return 1;
            }
            Ok(_) => std::thread::sleep(Duration::from_secs(3)),
        }
    }
}

/// Every action in the Action enum's own words: name, payload, docs. Read
/// from the source this binary was built from, so it never falls behind.
pub fn actions() -> Vec<(String, String, String)> {
    let src = include_str!("app/mod.rs");
    let body = src
        .split_once("pub enum Action {\n")
        .and_then(|(_, rest)| rest.split_once("\n}\n"))
        .map_or("", |(body, _)| body);
    let mut out = Vec::new();
    let mut doc = String::new();
    let mut lines = body.lines().peekable();
    while let Some(line) = lines.next() {
        let t = line.trim();
        if let Some(d) = t.strip_prefix("///") {
            doc.push_str(d.trim());
            doc.push(' ');
        } else if line.starts_with("    ")
            && !line.starts_with("     ")
            && t.starts_with(char::is_uppercase)
        {
            let name: String = t.chars().take_while(|c| c.is_alphanumeric()).collect();
            let mut shape = t[name.len()..].trim_end_matches(',').to_owned();
            // A struct variant: its fields up to the closing brace.
            if shape.trim() == "{" {
                for field in lines.by_ref() {
                    if field.trim().starts_with('}') {
                        break;
                    }
                    if !field.trim().starts_with("///") {
                        shape.push(' ');
                        shape.push_str(field.trim());
                    }
                }
                shape.push_str(" }");
            }
            out.push((name, shape.trim().to_owned(), doc.trim().to_owned()));
            doc.clear();
        } else if !t.starts_with("#[") {
            doc.clear();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_request_line_is_cut_at_64_kib() {
        let long = "x".repeat(200 * 1024);
        assert_eq!(
            super::read_request(long.as_bytes()).unwrap().len(),
            64 * 1024
        );
        let one = "{\"cmd\":\"state\"}\nrest";
        assert_eq!(
            super::read_request(one.as_bytes()).unwrap(),
            "{\"cmd\":\"state\"}\n"
        );
    }

    #[test]
    fn every_action_is_listed_with_its_payload() {
        let actions = super::actions();
        let names: Vec<&str> = actions.iter().map(|(n, ..)| n.as_str()).collect();
        for (name, _) in crate::activity::GATES {
            assert!(
                names.contains(name),
                "{name} missing from `orbit ctl actions`"
            );
        }
        assert_eq!(names.len(), crate::activity::GATES.len());
        let merge = actions.iter().find(|(n, ..)| n == "Merge").unwrap();
        assert_eq!(merge.1, "{ url: String, head: String, }");
        assert!(merge.2.contains("merge"));
    }

    #[test]
    fn linear_writes_are_read_from_names_not_ids() {
        use crate::linear::Write;
        let of = |s: &str| super::write_of(&s.split(' ').map(String::from).collect::<Vec<_>>());
        assert_eq!(
            of("comment DEV-1 PR is up"),
            Some(Write::Comment {
                issue: "DEV-1".into(),
                body: "PR is up".into()
            })
        );
        assert_eq!(
            of("move DEV-1 In Review"),
            Some(Write::Move {
                issue: "DEV-1".into(),
                state: "In Review".into()
            })
        );
        assert_eq!(
            of("parent DEV-2 none"),
            Some(Write::Parent {
                issue: "DEV-2".into(),
                parent: None
            })
        );
        assert_eq!(
            of("parent DEV-2 DEV-1"),
            Some(Write::Parent {
                issue: "DEV-2".into(),
                parent: Some("DEV-1".into())
            })
        );
        assert_eq!(
            of("block DEV-1 DEV-2"),
            Some(Write::Block {
                blocker: "DEV-1".into(),
                blocked: "DEV-2".into()
            })
        );
        assert_eq!(of("block DEV-1 DEV-2 DEV-3"), None);
        assert_eq!(of("move DEV-1"), None);
        assert_eq!(of("comment"), None);
        // What the app reads back off the socket.
        let sent = serde_json::to_value(of("move DEV-1 Todo").unwrap()).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({"op": "move", "issue": "DEV-1", "state": "Todo"})
        );
    }

    #[test]
    fn socket_round_trip() {
        let dir = std::env::temp_dir().join(format!("orbit-ctl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.sock");
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let stream = listener.incoming().next().unwrap().unwrap();
            super::answer(stream, &tx, &egui::Context::default());
        });
        std::thread::spawn(move || {
            let req: super::Request = rx.recv().unwrap();
            req.reply
                .send(serde_json::json!({"ok": true, "got": req.body}))
                .unwrap();
        });
        let mut s = std::os::unix::net::UnixStream::connect(&path).unwrap();
        use std::io::{BufRead, Write};
        writeln!(s, "{{\"cmd\":\"state\"}}").unwrap();
        let mut line = String::new();
        std::io::BufReader::new(&s).read_line(&mut line).unwrap();
        let out: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(out["got"]["cmd"], "state");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

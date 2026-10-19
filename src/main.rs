/// The product name: the window title, the data folder, the Keychain entry.
/// To rename the app, change it here and add the old one to `PREVIOUS_NAMES`
/// (the bundle's name and binary are in scripts/bundle-macos.sh and
/// Cargo.toml). On the first start under the new name a user's data folder
/// moves over (a link stays at the old path) and their Keychain entry is
/// copied.
pub const NAME: &str = "Orbit";

/// `ORBIT_READ_ONLY=1`: nothing leaves the app that could change anything
/// for anyone. Every Linear mutation is refused before it is sent, GitHub
/// merges are refused, and no Claude session or shell starts (they could
/// post or run commands). Simulated users run in it (scripts/simulate.sh).
pub fn read_only() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var_os("ORBIT_READ_ONLY").is_some_and(|v| !v.is_empty() && v != "0") || demo::on()
    })
}

/// Names this app had before, newest first: where to find a user's data
/// and keys from before a rename. A build names its old one with
/// `ORBIT_PREVIOUS_NAME`; a fresh install has none.
pub const PREVIOUS_NAMES: &[&str] = match option_env!("ORBIT_PREVIOUS_NAME") {
    Some(name) => &[name],
    None => &[],
};

/// This build's version, from Cargo.toml; releases are tagged `v<VERSION>`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The commit a release was built from; set by scripts/bundle-macos.sh.
pub const COMMIT: Option<&str> = option_env!("ORBIT_COMMIT");

/// The app's own repo: where releases come from and feedback PRs go
/// (`ORBIT_RELEASES_REPO` at build time, else Cargo.toml's; build.rs).
pub const REPO: &str = env!("ORBIT_REPO_URL");

/// Where releases are downloaded from.
pub const RELEASES: &str = concat!(env!("ORBIT_REPO_URL"), "/releases");

/// "v0.2.0 · 9126947", or "v0.2.0 · dev" for a local build.
pub fn version() -> String {
    format!("v{VERSION} · {}", COMMIT.unwrap_or("dev"))
}

/// The URL of the one attachment the app keeps per Linear ticket. Linear
/// treats an attachment URL as its key on an issue, so there is never more
/// than one. A team keeps its own (the profile's `linear.noteUrl`), so
/// everyone on it reads and writes the same note.
pub fn note_url() -> String {
    match profile::get().linear.note_url.trim() {
        "" => env!("CARGO_PKG_REPOSITORY").to_owned(),
        url => url.to_owned(),
    }
}

mod activity;
mod anthropic;
mod app;
mod autopilot;
mod backend;
mod browser;
mod capture;
mod claude;
mod cleanup;
mod control;
mod demo;
mod editor;
mod github;
mod goals;
mod guards;
mod herdr;
mod inbox;
mod incidents;
mod k9s;
mod laya;
mod linear;
mod mission;
mod notify;
mod oauth;
mod profile;
mod releases;
mod secrets;
mod signoz;
mod skills;
mod stats;
mod store;
mod story;
mod testplan;
mod theme;
mod today;
#[cfg(target_os = "macos")]
mod tray;
mod ui;
mod update;
mod work;

fn main() -> eframe::Result {
    // `orbit hook <log>`: a Claude Code hook in a session the
    // app started, appending one line about the session. No window.
    let args: Vec<String> = std::env::args().collect();
    if let [_, cmd, log] = args.as_slice()
        && cmd == "hook"
    {
        if let Err(error) = claude::status::hook(std::path::Path::new(log)) {
            eprintln!("{error:#}");
        }
        return Ok(());
    }
    // `orbit report <log> <state> <text…>`: a session the app started
    // says what it is on (a note on its row) or what it waits for. No window.
    if let [_, cmd, log, state, text @ ..] = args.as_slice()
        && cmd == "report"
    {
        let text = text.join(" ");
        if let Err(error) = claude::status::report(std::path::Path::new(log), state, &text) {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    // `orbit needs [--watch]`: what needs the user, as the running app last
    // wrote it (herdr.rs), for herdr's plugin pane. No window.
    if let [_, cmd, rest @ ..] = args.as_slice()
        && cmd == "needs"
    {
        let watch = rest.iter().any(|a| a == "--watch");
        loop {
            let text = std::fs::read_to_string(herdr::needs_path()).unwrap_or_default();
            if watch {
                print!("\x1b[2J\x1b[H");
            }
            if text.trim().is_empty() {
                println!("Nothing needs you in {NAME} right now (or {NAME} is not running).");
            } else {
                print!("{text}");
            }
            if !watch {
                return Ok(());
            }
            println!("\n(updates every few seconds; esc or ctrl+c closes)");
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
    }
    // `orbit open <url>`: hands a link to the running app (herdr's plugin,
    // a link clicked in a herdr pane). No window.
    if let [_, cmd, url] = args.as_slice()
        && cmd == "open"
    {
        if let Err(error) = herdr::queue_open(&herdr::opens_path(), url) {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    // `orbit ctl <cmd>`: drives the running app (control.rs). No window.
    if let [_, cmd, rest @ ..] = args.as_slice()
        && cmd == "ctl"
    {
        std::process::exit(control::main(rest));
    }
    // `orbit --skill`: the skill that teaches a session running inside the
    // app to report to it (skills/orbit), for other agents and for reading.
    if let [_, flag] = args.as_slice()
        && flag == "--skill"
    {
        print!("{}", skills::ORBIT);
        return Ok(());
    }
    // `orbit cleanup [--dry-run]`: one cleanup pass (cleanup.rs) from a
    // shell, without the board's done tickets. No window.
    // `orbit sessions-stop [--dry-run]`: Orbit's own idle background
    // sessions (claude/idle.rs), listed and stopped. No window.
    if let [_, cmd, rest @ ..] = args.as_slice()
        && cmd == "sessions-stop"
    {
        let dry = rest.iter().any(|a| a == "--dry-run");
        if let Err(error) = claude::stop_idle_cli(dry) {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    if let [_, cmd, rest @ ..] = args.as_slice()
        && cmd == "cleanup"
    {
        let dry = rest.iter().any(|a| a == "--dry-run");
        let facts = cleanup::Facts {
            busy: cleanup::busy(),
            done: Default::default(),
        };
        let data = store::data_dir();
        let (steps, freed) = cleanup::run(&data, &facts, dry);
        for step in &steps {
            println!(
                "{:<8} {:>9}  {}",
                format!("{:?}", step.kind).to_lowercase(),
                cleanup::gb(step.bytes),
                step.path.display()
            );
        }
        let total: u64 = steps.iter().map(|s| s.bytes).sum();
        if dry {
            println!(
                "{} steps, {} would be freed (dry run: nothing deleted)",
                steps.len(),
                cleanup::gb(total)
            );
        } else {
            println!("{} steps, {} freed", steps.len(), cleanup::gb(freed));
        }
        return Ok(());
    }
    // `orbit worktree <repos> <owner/repo> <branch>`: a ticket
    // session's clean checkout (src/claude/worktree.rs). No window.
    if let [_, cmd, repos, slug, branch] = args.as_slice()
        && cmd == "worktree"
    {
        if let Err(error) = claude::worktree::run(std::path::Path::new(repos), slug, branch) {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return Ok(());
    }
    // `ORBIT_SIZE=1100x700`: the window's size at start, for
    // reviewing narrow layouts (review.rs); 1440x900 otherwise.
    let size = std::env::var("ORBIT_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some([w.trim().parse().ok()?, h.trim().parse().ok()?])
        })
        .unwrap_or([1440.0, 900.0]);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(NAME)
            .with_inner_size(size),
        ..Default::default()
    };
    eframe::run_native(
        NAME,
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(&cc.egui_ctx)))),
    )
}

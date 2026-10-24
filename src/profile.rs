//! A profile: everything specific to one company or one person's way of
//! working, kept out of the app so the app itself is generic. Without one,
//! Orbit is a plain Linear client with Claude Code sessions; a profile adds
//! the team's bot and its commands, how tickets are routed, what counts as
//! KTLO, the skills its sessions get, and settings defaults.
//!
//! settings.json `profile` names it:
//! - nothing: the build's default (`profiles/default` in the repo), if any.
//! - `generic`: none at all.
//! - a built-in one by name (a folder under the repo's `profiles/`,
//!   compiled in by build.rs): used in place where the app was built;
//!   elsewhere its compiled copy is unpacked into `<data>/profiles/<name>/`.
//! - a folder, used in place: `profile.json`, `skills/<app skill>/EXTRA.md`
//!   (fine-tuning appended to the app's skill) and `skills/<name>/SKILL.md`
//!   (a skill of its own), `k9s/` (views and plugins for the debug panes).
//! - a `.json` file or an `https://` URL: a bundle, `profile.json` with the
//!   files embedded under `skillFiles` and `skillExtras`, unpacked into
//!   `<data>/profile/` by Import in Settings.
//!
//! Every field has a neutral default. A feature whose piece is missing stays
//! out of sight (no bot commands, no Run the board without an orchestrator).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Profile {
    /// Shown in Settings, e.g. "acme".
    pub name: String,
    pub company: Company,
    pub bot: Bot,
    pub github: Github,
    /// Where the Stats view gets the team's numbers (signoz.rs).
    pub stats: Stats,
    pub linear: Linear,
    pub intake: Intake,
    pub ktlo: Ktlo,
    /// Extra lines for the customer extraction prompt: aliases, internal
    /// names that are not customers.
    pub customers: String,
    /// How the user wants text written to them (Today, briefs), e.g. "short,
    /// the point first".
    pub style: String,
    /// Run the board: an unattended session that operates one team's board.
    pub orchestrator: Option<Orchestrator>,
    pub spotter: Spotter,
    /// Extra lines for an incident investigation's brief: the stack, skills
    /// and tools to use.
    pub investigate: String,
    /// House rules every job session keeps (Work's Fix now, New ticket's
    /// Fix now), e.g. "Changes go to dev before prod."
    pub rules: String,
    /// Role to skill folder: `triage`, `plan`, `build`, `fixComments`,
    /// `spot`, `orchestrate`. A role not named uses the app's own skill.
    /// `debugAlert` (no default): the skill an alert's Debug button runs;
    /// `fixDeploy`: the one a red deploy's Fix it runs.
    pub skills: BTreeMap<String, String>,
    /// In a bundle: the skills themselves, folder to SKILL.md.
    pub skill_files: BTreeMap<String, String>,
    /// In a bundle: the company's additions to the app's skills, folder to
    /// EXTRA.md (skills.rs `with_extra`).
    pub skill_extras: BTreeMap<String, String>,
    /// The folder's `k9s/`, file name to text: `aliases.yaml`, `views.yaml`,
    /// `plugins.yaml` entries merged into the user's k9s config when a debug
    /// pane opens k9s, every other file a script those plugins call (k9s.rs).
    pub k9s: BTreeMap<String, String>,
    /// settings.json values the profile brings (debug environments, test
    /// app, work modes): defaults under settings.json.
    pub settings: serde_json::Value,
    /// Who Orbit says it is: the name under what it posts for the user, and
    /// the email it co-authors its sessions' commits with.
    pub attribution: Attribution,
    /// Per person, by Linear email: any of the fields above, laid over the
    /// company's for that person only (their orchestrator, style, work
    /// modes). Everyone else gets the company profile.
    pub people: BTreeMap<String, serde_json::Value>,
    /// Where the profile's `skills/` folder is; not part of the file.
    #[serde(skip)]
    pub dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Attribution {
    /// "orbit"; empty: the app's name in lower case.
    pub name: String,
    /// "orbit@acme.example"; empty: commits get no co-author.
    pub email: String,
}

impl Attribution {
    pub fn name(&self) -> String {
        match self.name.trim() {
            "" => crate::NAME.to_lowercase(),
            name => name.to_owned(),
        }
    }

    /// What the user sends from here, with a last line saying so. Empty:
    /// only that line.
    pub fn signed(&self, body: &str) -> String {
        let line = format!("_Sent by {}_", self.name());
        match body.trim() {
            "" => line,
            body => format!("{body}\n\n{line}"),
        }
    }

    /// The commit trailer for the sessions' commits, when there is an email.
    pub fn co_author(&self) -> Option<String> {
        let email = self.email.trim();
        (!email.is_empty()).then(|| format!("Co-Authored-By: {} <{email}>", self.name()))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Company {
    /// "Acme".
    pub name: String,
    /// What tickets are about, for prompts: "Acme's analytics platform".
    pub product: String,
    /// Name prefixes of internal services and orgs, never customers.
    pub internal_prefixes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Bot {
    /// The team's bot, as it appears on Linear: "Robo". Empty: the author
    /// of most marker comments, else "the bot".
    pub name: String,
    /// Its comments start with this.
    pub marker: String,
    /// Comments that make it plan or build a ticket remotely, e.g.
    /// "@Bot Plan!". Empty: the step runs here only.
    pub plan_command: String,
    pub build_command: String,
    /// The comment that makes it investigate a sweep's ticket again, e.g.
    /// "@Bot Investigate!"; offered where a sweep says `investigate`.
    pub investigate_command: String,
    /// When it picks a command up, for the hint under the menu.
    pub poller_note: String,
    /// The GitHub Actions workflow that makes it look now, as
    /// `owner/repo:workflow.yml`: an initiative runs it after handing the
    /// bot a ticket, so the bot's loop starts at once. Empty: it waits.
    pub poller_workflow: String,
    /// The bot builds nothing new while this many of its team's tickets
    /// are In Review; a mission names it when a sub-ticket stalls. 0: none.
    pub review_cap: usize,
}

impl Default for Bot {
    fn default() -> Self {
        Self {
            name: String::new(),
            marker: "🤖".into(),
            plan_command: String::new(),
            build_command: String::new(),
            investigate_command: String::new(),
            poller_note: String::new(),
            poller_workflow: String::new(),
            review_cap: 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Github {
    /// The org most repos live in, for skills and examples.
    pub org: String,
    /// The bot's GitHub login, whose PRs count as the bot's.
    pub bot_login: String,
    /// How an AI review of the user's own PR is asked for, again after new
    /// commits. Empty: no button.
    pub ai_review: AiReview,
    /// The label that hands a PR to a review router, e.g.
    /// `review-roulette`. Empty: no Review roulette in Request review.
    pub review_roulette: String,
    /// A label that deploys a PR's preview, and the repos it does that in
    /// (e.g. `preview` on `web`). Empty: no Preview button.
    pub preview: Preview,
}

/// The Stats view's team numbers: a SigNoz MCP gateway reachable without
/// a key (the company's private network), the repos to count, the
/// dashboard to open for more. Empty `mcp`: the view shows only the
/// user's own activity.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stats {
    pub mcp: String,
    pub repos: Vec<String>,
    pub dashboard: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preview {
    /// One label, one Preview button.
    pub label: String,
    /// Several kinds of preview, a button each (e.g. `preview-ui`,
    /// `preview-full`); used instead of `label` when set.
    pub labels: Vec<PreviewLabel>,
    /// Repo names (no owner) whose PRs get the button.
    pub repos: Vec<String>,
    /// The words a PR comment carries once the preview is up, its address
    /// right after them (e.g. `Deployed preview environment to`).
    /// Empty: the Testing tab never offers the preview.
    pub ready: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PreviewLabel {
    pub label: String,
    /// The button's text, `Preview UI`.
    pub button: String,
    /// What that preview deploys, for the button's hover.
    pub hint: String,
}

impl Preview {
    /// The preview labels for a PR in `repo`, when that repo deploys previews.
    pub fn labels_for(&self, repo: &str) -> Vec<PreviewLabel> {
        if !self.repos.iter().any(|r| r == repo) {
            return Vec::new();
        }
        if !self.labels.is_empty() {
            return self.labels.clone();
        }
        (!self.label.is_empty())
            .then(|| PreviewLabel {
                label: self.label.clone(),
                button: "Preview".into(),
                hint: "the PR's preview gets deployed".into(),
            })
            .into_iter()
            .collect()
    }

    /// The preview's address from a PR's comments (oldest first): the
    /// first URL after `ready` in the latest comment that says it.
    pub fn url_in(&self, comments: &[String]) -> Option<String> {
        if self.ready.is_empty() {
            return None;
        }
        let comment = comments.iter().rev().find(|c| c.contains(&self.ready))?;
        let after = &comment[comment.find(&self.ready)? + self.ready.len()..];
        let url = &after[after.find("http")?..];
        let end = url
            .find(|c: char| c.is_whitespace() || matches!(c, ')' | ']' | '"' | '<' | '>'))
            .unwrap_or(url.len());
        Some(url[..end].to_owned())
    }
}

/// One way to ask for an AI review, the first one set: a comment
/// (`/ai-review`), a label (taken off and put back, so it fires again),
/// or a reviewer (requested again).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AiReview {
    pub comment: String,
    pub label: String,
    pub reviewer: String,
}

impl AiReview {
    /// What the button says it does, or `None` when nothing is set.
    pub fn how(&self) -> Option<String> {
        let (comment, label, reviewer) =
            (self.comment.trim(), self.label.trim(), self.reviewer.trim());
        if !comment.is_empty() {
            Some(format!("comments {comment} on the PR"))
        } else if !label.is_empty() {
            Some(format!("puts the {label} label on the PR (again)"))
        } else if !reviewer.is_empty() {
            Some(format!("requests a review from {reviewer} (again)"))
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Linear {
    /// A Linear OAuth app for Sign in with Linear. Empty: API key only.
    pub oauth_client_id: String,
    /// The URL every per-ticket note attachment is keyed by, shared with
    /// teammates. Empty: the app's own.
    pub note_url: String,
    /// Where the teams' weekly priorities live in Linear.
    pub priorities: CyclePriorities,
}

/// Linear -> team -> the cycle that covers the week -> its documents whose
/// title contains `title`: the weekly priorities Plan my day and the goals
/// run read before anything else.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CyclePriorities {
    /// Team keys. Empty: the teams the user watches.
    pub teams: Vec<String>,
    /// What a priority document's title contains. Empty: "prio".
    pub title: String,
}

impl CyclePriorities {
    pub fn title(&self) -> String {
        match self.title.trim() {
            "" => "prio".to_owned(),
            title => title.to_lowercase(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Intake {
    /// Which team owns what, for the New ticket intake: one line per team
    /// key. Empty: the watched teams, as Linear names them.
    pub routing: String,
    /// The team when the note fits none.
    pub default_team: String,
    /// Appended to the routing comment of a P0 or P1, e.g. "Ping #oncall.".
    pub urgent_note: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Ktlo {
    /// What the lane is called: "KTLO" (keep the lights on), "Maintenance".
    pub label: String,
    /// Tickets created by these users are security findings.
    pub security_creators: Vec<String>,
    /// Lowercase label names that mark a security finding.
    pub security_labels: Vec<String>,
    /// Lowercase title parts that mark one.
    pub security_titles: Vec<String>,
    /// Lowercase label names of alert tickets.
    pub alert_labels: Vec<String>,
    /// Title prefixes of tickets the bot's sweeps file, and the story each
    /// groups into; `alert` puts them in the KTLO alerts lane.
    pub sweeps: Vec<Sweep>,
}

impl Default for Ktlo {
    fn default() -> Self {
        Self {
            label: "Maintenance".into(),
            security_creators: Vec::new(),
            security_labels: vec!["security".into()],
            security_titles: vec![" cve:".into(), "cve-".into()],
            alert_labels: vec!["ktlo".into()],
            sweeps: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sweep {
    /// Lowercase, e.g. "[prod]".
    pub prefix: String,
    pub story: String,
    pub alert: bool,
    /// Its tickets offer Investigate: here in Claude, or the bot's
    /// `investigateCommand`.
    pub investigate: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Orchestrator {
    /// The team key whose board it operates.
    pub team: String,
    /// Extra tools it may use unattended, e.g. an observability MCP.
    pub allow: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Spotter {
    /// What the spotter should know about the company: teams, alert
    /// channels, where support asks land.
    pub context: String,
    /// Extra read tools it may use unattended (MCP tool names).
    pub tools: Vec<String>,
}

impl Profile {
    /// The bot's name for text, or `fallback` ("the bot") when unset.
    pub fn bot_name<'a>(&'a self, fallback: &'a str) -> &'a str {
        match self.bot.name.trim() {
            "" => fallback,
            name => name,
        }
    }

    /// The user's own company or one of its internal services: never a
    /// customer.
    pub fn is_ours(&self, name: &str) -> bool {
        let lower = name.trim().to_lowercase();
        (!self.company.name.is_empty() && lower == self.company.name.to_lowercase())
            || self
                .company
                .internal_prefixes
                .iter()
                .any(|p| !p.is_empty() && lower.starts_with(&p.to_lowercase()))
    }

    /// The skill folder for a role: the profile's name for it, else the
    /// app's own (`triage`, `plan`, ...).
    pub fn skill(&self, role: &str) -> String {
        self.skills
            .get(role)
            .cloned()
            .unwrap_or_else(|| default_skill(role).to_owned())
    }
}

/// The app's own skill folder for a role.
fn default_skill(role: &str) -> &str {
    match role {
        "fixComments" => "fix-pr-comments",
        "fixDeploy" => "fix-deploy",
        other => other,
    }
}

fn current() -> &'static RwLock<Arc<Profile>> {
    static CURRENT: std::sync::OnceLock<RwLock<Arc<Profile>>> = std::sync::OnceLock::new();
    CURRENT.get_or_init(|| RwLock::new(Arc::new(Profile::default())))
}

/// The profile in use.
pub fn get() -> Arc<Profile> {
    current().read().map(|p| p.clone()).unwrap_or_default()
}

/// Puts a profile in use.
pub fn set(profile: Profile) {
    if let Ok(mut slot) = current().write() {
        *slot = Arc::new(profile);
    }
}

mod built_in {
    include!(concat!(env!("OUT_DIR"), "/profiles.rs"));
}

/// settings.json `profile` for no profile at all.
pub const GENERIC: &str = "generic";

/// The profiles compiled into this build.
pub fn built_in() -> Vec<&'static str> {
    built_in::BUILT_IN.iter().map(|(name, _)| *name).collect()
}

/// What settings.json `profile` stands for: nothing is the build's
/// default, `generic` is none.
pub fn effective(source: &str) -> &str {
    match source.trim() {
        "" => built_in::DEFAULT.unwrap_or(""),
        GENERIC => "",
        source => source,
    }
}

/// The folder a source's profile lives in: a built-in one's (in the repo,
/// or unpacked), the folder itself, or the unpacked copy of a bundle.
/// `None` for no profile.
pub fn folder(source: &str) -> Option<PathBuf> {
    let source = effective(source);
    if source.is_empty() {
        None
    } else if let Some(dir) = built_in_folder(source) {
        Some(dir)
    } else if is_bundle(source) {
        Some(unpacked())
    } else {
        Some(expand(source))
    }
}

/// A built-in profile's folder: the repo's own when the app runs where it
/// was built, else its compiled copy, unpacked when it changed.
fn built_in_folder(name: &str) -> Option<PathBuf> {
    let (_, bundle) = built_in::BUILT_IN.iter().find(|(n, _)| *n == name)?;
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("profiles")
        .join(name);
    if repo.join("profile.json").is_file() {
        return Some(repo);
    }
    let dir = crate::store::data_dir().join("profiles").join(name);
    let stamp = dir.join(".bundle-hash");
    let hash = crate::store::text_hash(bundle).to_string();
    if std::fs::read_to_string(&stamp).ok().as_deref() != Some(hash.as_str()) {
        unpack(bundle, &dir).ok()?;
        let _ = std::fs::write(&stamp, &hash);
    }
    Some(dir)
}

/// Where an imported bundle is unpacked.
pub fn unpacked() -> PathBuf {
    crate::store::data_dir().join("profile")
}

/// The profile settings.json names: a folder in place, or the unpacked
/// copy of a bundle. Empty or missing: the generic default.
/// The user's own edits from Settings (`edits_file`) lie on top.
pub fn load(source: &str) -> Result<Profile> {
    // Demo mode is Acme, whatever settings.json names.
    if crate::demo::on() {
        return Ok(crate::demo::profile());
    }
    load_with(source, edits())
}

/// The profile without the user's Settings edits: what an edit is
/// compared against, so only real changes are kept.
pub fn load_base(source: &str) -> Result<Profile> {
    load_with(source, serde_json::Value::Null)
}

fn load_with(source: &str, edits: serde_json::Value) -> Result<Profile> {
    match folder(source) {
        None => {
            let mut base = serde_json::json!({});
            if edits.is_object() {
                merge(&mut base, edits);
            }
            Ok(serde_json::from_value(base)?)
        }
        Some(dir) => read_dir(&dir, edits),
    }
}

fn read_dir(dir: &Path, edits: serde_json::Value) -> Result<Profile> {
    let file = dir.join("profile.json");
    let text = std::fs::read_to_string(&file)
        .with_context(|| format!("no profile at {}", file.display()))?;
    let mut company: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("{} is not a profile", file.display()))?;
    let own = company["people"][viewer().to_lowercase()].clone();
    if own.is_object() {
        merge(&mut company, own);
    }
    if edits.is_object() {
        merge(&mut company, edits);
    }
    let mut profile =
        for_person(company, "").with_context(|| format!("{} is not a profile", file.display()))?;
    read_k9s(dir, &mut profile);
    profile.dir = Some(dir.to_owned());
    Ok(profile)
}

/// The bot & PRs fields Settings edits (ui/settings.rs), kept in
/// `<data>/profile-edits.json` and laid over whichever profile is in use,
/// or none. A file of its own in the data folder: an update that unpacks
/// a built-in profile again never wipes it, nothing in a repo is written,
/// and a plain install gets a bot without writing a profile. Only fields
/// that differ from the profile are kept, so a later profile version still
/// reaches the user everywhere else.
pub fn edits_file() -> PathBuf {
    crate::store::data_dir().join("profile-edits.json")
}

fn edits() -> serde_json::Value {
    // Tests never read the user's own edits.
    if cfg!(test) {
        return serde_json::Value::Null;
    }
    std::fs::read_to_string(edits_file())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// How a field is typed in Settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditKind {
    Text,
    /// Comma separated.
    List,
    /// The preview labels, one per line: `label: Button text`.
    Labels,
}

/// One field of Settings' bot & PRs section: where it sits in
/// profile.json (a JSON pointer), its name, and what it turns on.
pub struct EditField {
    pub path: &'static str,
    pub name: &'static str,
    pub what: &'static str,
    pub kind: EditKind,
}

const fn field(path: &'static str, name: &'static str, what: &'static str) -> EditField {
    EditField {
        path,
        name,
        what,
        kind: EditKind::Text,
    }
}

pub const EDIT_FIELDS: &[EditField] = &[
    field(
        "/bot/name",
        "bot name",
        "the team's bot as it appears on Linear, e.g. Robo",
    ),
    field(
        "/bot/planCommand",
        "plan command",
        "adds \"Remotely, <bot>\" under Plan's ▼, e.g. @Robo Plan!",
    ),
    field(
        "/bot/buildCommand",
        "build command",
        "adds \"Remotely, <bot>\" under Build's ▼",
    ),
    field(
        "/bot/investigateCommand",
        "investigate command",
        "adds \"Remotely, <bot>\" under a sweep's Investigate ▼",
    ),
    field(
        "/bot/pollerNote",
        "poller note",
        "when the bot picks a command up, said after asking it",
    ),
    field(
        "/github/botLogin",
        "bot's GitHub login",
        "PRs by this login count as the bot's",
    ),
    field(
        "/github/aiReview/comment",
        "AI review comment",
        "adds Request AI review: comments this on the PR, e.g. /ai-review",
    ),
    field(
        "/github/aiReview/label",
        "AI review label",
        "or: puts this label on the PR (again)",
    ),
    field(
        "/github/aiReview/reviewer",
        "AI reviewer",
        "or: requests a review from this login (again)",
    ),
    field(
        "/github/reviewRoulette",
        "review roulette label",
        "adds Review roulette under Request review: the label a review router picks up",
    ),
    EditField {
        path: "/github/preview/labels",
        name: "preview labels",
        what: "a Preview button each, one per line as `label: Button text`",
        kind: EditKind::Labels,
    },
    EditField {
        path: "/github/preview/repos",
        name: "preview repos",
        what: "repo names (no owner) whose PRs get the Preview buttons, comma separated",
        kind: EditKind::List,
    },
    field(
        "/github/preview/ready",
        "preview ready text",
        "the words a PR comment says once the preview is up; the Testing tab then offers it",
    ),
];

impl EditField {
    /// The field as typed in Settings, from a profile.
    pub fn text(&self, profile: &Profile) -> String {
        let value = serde_json::to_value(profile).unwrap_or_default();
        let at = value.pointer(self.path).cloned().unwrap_or_default();
        match self.kind {
            EditKind::Text => at.as_str().unwrap_or_default().to_owned(),
            EditKind::List => strings(&at).join(", "),
            EditKind::Labels => {
                let labels: Vec<PreviewLabel> = serde_json::from_value(at).unwrap_or_default();
                let one = profile.github.preview.label.trim();
                let lines: Vec<String> = if labels.is_empty() && !one.is_empty() {
                    vec![format!("{one}: Preview")]
                } else {
                    labels
                        .iter()
                        .map(|l| format!("{}: {}", l.label, l.button))
                        .collect()
                };
                lines.join("\n")
            }
        }
    }

    /// The JSON a typed text stands for; preview labels keep the hint the
    /// profile gives the same label.
    fn value(&self, text: &str, base: &Profile) -> serde_json::Value {
        match self.kind {
            EditKind::Text => text.trim().into(),
            EditKind::List => text
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .into(),
            EditKind::Labels => {
                let labels: Vec<PreviewLabel> = text
                    .lines()
                    .filter_map(|line| {
                        let (label, button) = line.split_once(':').unwrap_or((line, "Preview"));
                        let label = label.trim().to_owned();
                        let hint = base
                            .github
                            .preview
                            .labels
                            .iter()
                            .find(|l| l.label == label)
                            .map_or("the PR's preview gets deployed".to_owned(), |l| {
                                l.hint.clone()
                            });
                        (!label.is_empty()).then(|| PreviewLabel {
                            label,
                            button: button.trim().to_owned(),
                            hint,
                        })
                    })
                    .collect();
                serde_json::to_value(labels).unwrap_or_default()
            }
        }
    }
}

fn strings(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

/// The edits file after typing `typed` (path to text) over the profile
/// `base` (without edits): a field typed as the profile has it is dropped,
/// any other is kept, so the profile's own later changes still arrive.
pub fn edited(
    mut edits: serde_json::Value,
    base: &Profile,
    typed: &BTreeMap<String, String>,
) -> serde_json::Value {
    if !edits.is_object() {
        edits = serde_json::json!({});
    }
    for field in EDIT_FIELDS {
        let Some(text) = typed.get(field.path) else {
            continue;
        };
        let parts: Vec<&str> = field.path.trim_start_matches('/').split('/').collect();
        let (last, parents) = parts.split_last().unwrap();
        if text.trim() == field.text(base).trim() {
            let parent = format!("/{}", parents.join("/"));
            if let Some(map) = edits.pointer_mut(&parent).and_then(|v| v.as_object_mut()) {
                map.remove(*last);
            }
            continue;
        }
        let mut at = &mut edits;
        for part in parents {
            if !at[*part].is_object() {
                at[*part] = serde_json::json!({});
            }
            at = &mut at[*part];
        }
        at[*last] = field.value(text, base);
    }
    edits
}

/// A folder profile's `k9s/` files; a bundle carries them in its JSON.
fn read_k9s(dir: &Path, profile: &mut Profile) {
    if !profile.k9s.is_empty() {
        return;
    }
    for entry in std::fs::read_dir(dir.join("k9s"))
        .into_iter()
        .flatten()
        .flatten()
    {
        if let Ok(text) = std::fs::read_to_string(entry.path()) {
            profile
                .k9s
                .insert(entry.file_name().to_string_lossy().into_owned(), text);
        }
    }
}

/// The company profile with the person's own section (`people[email]`)
/// laid over it, key by key.
fn for_person(mut company: serde_json::Value, email: &str) -> Result<Profile> {
    let own = company["people"][email.to_lowercase()].clone();
    if own.is_object() {
        merge(&mut company, own);
    }
    Ok(serde_json::from_value(company)?)
}

fn merge(base: &mut serde_json::Value, over: serde_json::Value) {
    match (base, over) {
        (serde_json::Value::Object(base), serde_json::Value::Object(over)) => {
            for (key, value) in over {
                merge(base.entry(key).or_insert(serde_json::Value::Null), value);
            }
        }
        (base, over) => *base = over,
    }
}

static VIEWER: RwLock<String> = RwLock::new(String::new());

/// The signed-in user's Linear email, for their section of the profile.
/// `true` when it changed, so the profile is read again.
pub fn set_viewer(email: &str) -> bool {
    let email = email.to_lowercase();
    let mut slot = VIEWER.write().unwrap();
    let changed = *slot != email;
    *slot = email;
    changed
}

fn viewer() -> String {
    VIEWER.read().map(|v| v.clone()).unwrap_or_default()
}

/// A URL or a single JSON file: fetched and unpacked, not used in place.
pub fn is_bundle(source: &str) -> bool {
    source.starts_with("https://") || source.ends_with(".json")
}

pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => crate::store::home().map_or_else(|| PathBuf::from(path), |h| h.join(rest)),
        None => PathBuf::from(path),
    }
}

/// Writes a bundle's profile and skills into `dir`, replacing what was
/// there, and returns the profile.
pub fn unpack(bundle: &str, dir: &Path) -> Result<Profile> {
    let mut profile: Profile = serde_json::from_str(bundle).context("not a profile")?;
    // Checked before anything is removed: a bad bundle keeps the old one.
    if let Some(bad) = profile
        .skill_files
        .keys()
        .chain(profile.skill_extras.keys())
        .find(|f| f.is_empty() || f.contains(['/', '\\']) || f.starts_with('.'))
    {
        bail!("bad skill folder name {bad:?}");
    }
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    for (folder, body) in std::mem::take(&mut profile.skill_files) {
        let path = dir.join("skills").join(&folder);
        std::fs::create_dir_all(&path)?;
        std::fs::write(path.join("SKILL.md"), body)?;
    }
    for (folder, extra) in std::mem::take(&mut profile.skill_extras) {
        let path = dir.join("skills").join(&folder);
        std::fs::create_dir_all(&path)?;
        std::fs::write(path.join("EXTRA.md"), extra)?;
    }
    std::fs::create_dir_all(dir)?;
    crate::store::save_at(&dir.join("profile.json"), &profile)?;
    profile.dir = Some(dir.to_owned());
    Ok(profile)
}

/// Reads a bundle from a URL or a local file. Blocking for a file; the URL
/// goes through `http`.
pub async fn fetch(http: &reqwest::Client, source: &str) -> Result<String> {
    if source.starts_with("https://") {
        return Ok(http
            .get(source)
            .send()
            .await?
            .error_for_status()
            .context("the profile URL did not answer")?
            .text()
            .await?);
    }
    std::fs::read_to_string(expand(source)).with_context(|| format!("cannot read {source}"))
}

/// The test profile in tests/profiles/acme: a full company profile (bot,
/// KTLO rules, intake, skill extras, k9s, a person's section) for the tests
/// that check what a profile does.
#[cfg(test)]
pub fn acme() -> Profile {
    acme_as("")
}

/// The acme test profile as one person gets it, their own section on top.
#[cfg(test)]
pub fn acme_as(email: &str) -> Profile {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/profiles/acme");
    let text = std::fs::read_to_string(dir.join("profile.json")).unwrap();
    let mut profile = for_person(serde_json::from_str(&text).unwrap(), email).unwrap();
    read_k9s(&dir, &mut profile);
    profile.dir = Some(dir);
    profile
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_edits_keep_only_what_differs_from_the_profile() {
        let base = acme();
        let typed = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        // Typed as the profile has it: nothing kept.
        let same: BTreeMap<String, String> = EDIT_FIELDS
            .iter()
            .map(|f| (f.path.to_owned(), f.text(&base)))
            .collect();
        assert_eq!(
            edited(serde_json::Value::Null, &base, &same),
            serde_json::json!({})
        );
        // A changed command is kept and lands in the profile.
        let edits = edited(
            serde_json::Value::Null,
            &base,
            &typed(&[
                ("/bot/planCommand", " @Robo Think! "),
                ("/github/preview/repos", "web, api"),
            ]),
        );
        assert_eq!(edits["bot"]["planCommand"], "@Robo Think!");
        assert_eq!(
            edits["github"]["preview"]["repos"],
            serde_json::json!(["web", "api"])
        );
        let mut company = serde_json::to_value(&base).unwrap();
        merge(&mut company, edits.clone());
        let edited_profile: Profile = serde_json::from_value(company).unwrap();
        assert_eq!(edited_profile.bot.plan_command, "@Robo Think!");
        assert_eq!(edited_profile.bot.build_command, base.bot.build_command);
        // Typed back to the profile's own: the edit goes, the rest stays.
        let back = edited(
            edits,
            &base,
            &typed(&[("/bot/planCommand", &base.bot.plan_command)]),
        );
        assert!(back["bot"].get("planCommand").is_none());
        assert_eq!(
            back["github"]["preview"]["repos"],
            serde_json::json!(["web", "api"])
        );
    }

    #[test]
    fn a_plain_install_gets_a_bot_from_its_settings_edits_alone() {
        let generic = load_with(
            GENERIC,
            serde_json::json!({"bot": {"name": "Robo", "planCommand": "@Robo Plan!"}}),
        )
        .unwrap();
        assert_eq!(generic.bot.plan_command, "@Robo Plan!");
        assert!(generic.dir.is_none());
        assert_eq!(
            load_with(GENERIC, serde_json::Value::Null).unwrap(),
            Profile::default()
        );
    }

    #[test]
    fn preview_labels_are_typed_one_per_line_and_keep_their_hint() {
        let field = EDIT_FIELDS
            .iter()
            .find(|f| f.kind == EditKind::Labels)
            .unwrap();
        let mut base = Profile::default();
        base.github.preview.labels = vec![PreviewLabel {
            label: "preview-ui".into(),
            button: "Preview UI".into(),
            hint: "the UI only".into(),
        }];
        assert_eq!(field.text(&base), "preview-ui: Preview UI");
        let value = field.value("preview-ui: UI\npreview-full: Full", &base);
        let labels: Vec<PreviewLabel> = serde_json::from_value(value).unwrap();
        assert_eq!(labels[0].hint, "the UI only");
        assert_eq!(labels[1].button, "Full");
    }

    #[test]
    fn what_orbit_posts_is_signed_and_commits_get_a_co_author_with_an_email() {
        let generic = Attribution::default();
        assert_eq!(
            generic.signed("Looks right"),
            "Looks right\n\n_Sent by orbit_"
        );
        assert_eq!(generic.signed("  "), "_Sent by orbit_");
        assert_eq!(generic.co_author(), None);
        let acme = Attribution {
            name: "orbit".into(),
            email: "orbit@acme.example".into(),
        };
        assert_eq!(
            acme.co_author().as_deref(),
            Some("Co-Authored-By: orbit <orbit@acme.example>")
        );
    }

    #[test]
    fn an_ai_review_is_asked_the_first_way_set() {
        let mut how = AiReview::default();
        assert_eq!(how.how(), None);
        how.reviewer = "copilot".into();
        how.label = "ai-review".into();
        assert!(how.how().unwrap().contains("ai-review label"));
        how.comment = "/ai-review".into();
        assert_eq!(how.how().unwrap(), "comments /ai-review on the PR");
        let github: Github =
            serde_json::from_str(r#"{"aiReview": {"comment": "/ai-review"}}"#).unwrap();
        assert_eq!(github.ai_review.comment, "/ai-review");
    }

    #[test]
    fn a_preview_is_ready_once_a_comment_names_where_it_runs() {
        let preview = Preview {
            ready: "Deployed preview environment to".into(),
            ..Preview::default()
        };
        let said = vec![
            "Deploying preview environment".to_owned(),
            "## :rocket: Deployed preview environment to [https://pr-9.dev.example.com](https://pr-9.dev.example.com)".to_owned(),
        ];
        assert_eq!(
            preview.url_in(&said).as_deref(),
            Some("https://pr-9.dev.example.com")
        );
        // Still deploying: no address yet.
        assert_eq!(preview.url_in(&said[..1]), None);
        // No ready words in the profile: never a preview to test on.
        assert_eq!(Preview::default().url_in(&said), None);
    }

    #[test]
    fn a_profile_fine_tunes_the_apps_own_skills() {
        let p = acme();
        assert_eq!((p.name.as_str(), p.bot_name("")), ("acme", "Robo"));
        let k9s = &p.k9s;
        assert!(k9s.contains_key("views.yaml") && k9s.contains_key("k9s-rollup"));
        assert!(k9s["plugins.yaml"].contains("command: k9s-rollup"));
        // Every role runs the app's skill, and every EXTRA.md refines one
        // of them; its only mapping is the alert Debug button's skill.
        assert_eq!(
            p.skills.iter().collect::<Vec<_>>(),
            [(&"debugAlert".to_owned(), &"debug-alert".to_owned())]
        );
        for role in ["triage", "plan", "build", "spot", "orchestrate"] {
            assert_eq!(p.skill(role), role);
        }
        assert_eq!(p.skill("fixComments"), "fix-pr-comments");
        assert_eq!(p.skill("fixDeploy"), "fix-deploy");
        assert_eq!(p.github.review_roulette, "review-roulette");
        // Only the repos named preview, with every label as a button.
        let labels: Vec<String> = p
            .github
            .preview
            .labels_for("web")
            .into_iter()
            .map(|l| l.label)
            .collect();
        assert_eq!(labels, ["preview-ui", "preview-full"]);
        assert!(p.github.preview.labels_for("api").is_empty());
        let one = Preview {
            label: "preview".into(),
            repos: vec!["app".into()],
            ..Default::default()
        };
        assert_eq!(
            one.labels_for("app")[0].button,
            "Preview",
            "one label, one button"
        );
        let dir = p.dir.clone().unwrap().join("skills");
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let folder = entry.file_name().to_string_lossy().into_owned();
            assert!(
                crate::skills::GENERIC.iter().any(|(f, _)| *f == folder),
                "{folder} refines no skill of the app"
            );
            let extra = std::fs::read_to_string(entry.path().join("EXTRA.md")).unwrap();
            assert!(
                !extra.starts_with("---"),
                "{folder}: EXTRA.md has no frontmatter"
            );
        }
        assert_eq!(p.linear.oauth_client_id, "acme-oauth-client");
        assert_eq!(p.linear.note_url, "https://github.com/acme/orbit-notes");
        // Run the board and the style note are one person's, not everyone's.
        assert!(p.orchestrator.is_none() && p.style.is_empty());
        let sam = acme_as("sam@acme.example");
        assert_eq!(sam.orchestrator.as_ref().unwrap().team, "OPS");
        assert!(sam.style.contains("point first"));
        assert!(
            p.spotter
                .tools
                .iter()
                .any(|t| t == "mcp__metrics__search_logs")
        );
        assert_eq!(p.settings["testAppUrl"], "https://dev.acme.example");
        assert_eq!(p.settings["debug"][0]["setup"], "login prod");
    }

    #[test]
    fn a_profile_imports_as_a_bundle_too() {
        // A bundle: profile.json with its skill extras embedded, as a URL serves it.
        let mut bundle = acme();
        let dir = bundle.dir.take().unwrap();
        let extra = std::fs::read_to_string(dir.join("skills/triage/EXTRA.md")).unwrap();
        bundle.skill_extras.insert("triage".into(), extra.clone());
        let json = serde_json::to_string(&bundle).unwrap();
        let out = std::env::temp_dir().join(format!("orbit-acme-{}", std::process::id()));
        let unpacked = unpack(&json, &out).unwrap();
        assert_eq!(unpacked.skills, acme().skills);
        assert_eq!(
            std::fs::read_to_string(out.join("skills/triage/EXTRA.md")).unwrap(),
            extra
        );
        std::fs::remove_dir_all(&out).unwrap();
    }

    #[test]
    fn no_profile_is_generic_and_a_folder_is_read_in_place() {
        let generic = load(GENERIC).unwrap();
        assert_eq!(generic, Profile::default());
        assert_eq!(generic.bot_name("the bot"), "the bot");
        assert_eq!(generic.skill("triage"), "triage");
        assert_eq!(generic.skill("fixComments"), "fix-pr-comments");
        assert!(
            generic.orchestrator.is_none(),
            "no Run the board without one"
        );

        let dir = std::env::temp_dir().join(format!("orbit-profile-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("skills/acme-triage")).unwrap();
        std::fs::write(
            dir.join("profile.json"),
            r#"{"name":"acme","bot":{"name":"Robo","planCommand":"@Robo Plan!"},
                "skills":{"triage":"acme-triage"},"orchestrator":{"team":"OPS"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("skills/acme-triage/SKILL.md"),
            "# Triage the Acme way",
        )
        .unwrap();
        let acme = load(dir.to_str().unwrap()).unwrap();
        assert_eq!(
            (acme.bot_name("the bot"), acme.bot.marker.as_str()),
            ("Robo", "🤖")
        );
        assert_eq!(acme.skill("triage"), "acme-triage");
        assert_eq!(
            std::fs::read_to_string(dir.join("skills/acme-triage/SKILL.md")).unwrap(),
            "# Triage the Acme way"
        );
        assert_eq!(acme.orchestrator.unwrap().team, "OPS");
        assert!(load("/no/such/profile").is_err());
        // Nothing named: this build's default (the repo's profiles/default),
        // none at all in a build without one.
        let default = built_in::DEFAULT.unwrap_or("");
        assert_eq!(effective(""), default);
        assert_eq!(load("").unwrap().name, default);
        assert!(default.is_empty() || built_in().contains(&default));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_person_gets_their_own_section_over_the_company_profile() {
        let company = serde_json::json!({
            "name": "acme",
            "style": "",
            "bot": {"name": "Robo", "planCommand": "@Robo Plan!"},
            "people": {"sam@acme.example": {
                "style": "short, the point first",
                "orchestrator": {"team": "OPS"},
                "bot": {"planCommand": ""},
            }},
        });
        let sam = for_person(company.clone(), "Sam@Acme.example").unwrap();
        assert_eq!(sam.style, "short, the point first");
        assert_eq!(sam.orchestrator.unwrap().team, "OPS");
        assert_eq!(
            (sam.bot.name.as_str(), sam.bot.plan_command.as_str()),
            ("Robo", "")
        );
        // Anyone else: the company profile, no orchestrator.
        let other = for_person(company, "alex@acme.example").unwrap();
        assert!(other.orchestrator.is_none() && other.style.is_empty());
        assert_eq!(other.bot.plan_command, "@Robo Plan!");
    }

    #[test]
    fn a_bundle_unpacks_its_skills_and_refuses_paths() {
        assert!(is_bundle("https://example.com/p.json") && is_bundle("~/acme.json"));
        assert!(!is_bundle("~/acme-profile"));
        let dir = std::env::temp_dir().join(format!("orbit-bundle-{}", std::process::id()));
        let profile = unpack(
            r##"{"name":"acme","skillFiles":{"acme-plan":"# Plan"},"settings":{"testAppUrl":"https://staging.example.com"}}"##,
            &dir,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("skills/acme-plan/SKILL.md")).unwrap(),
            "# Plan"
        );
        assert!(
            profile.skill_files.is_empty(),
            "the files live on disk, not in the json"
        );
        let read = read_dir(&dir, serde_json::Value::Null).unwrap();
        assert_eq!(read.settings["testAppUrl"], "https://staging.example.com");
        assert!(unpack(r#"{"skillFiles":{"../evil":"x"}}"#, &dir).is_err());
        assert!(
            read_dir(&dir, serde_json::Value::Null).is_ok(),
            "a bad bundle leaves the old profile in place"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

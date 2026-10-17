//! An authenticated transport for the Linear GraphQL API.
//!
//! Every call goes through one `query` helper that adds the key, sends the
//! GraphQL document, and turns GraphQL errors into plain messages. The key is
//! never logged or included in an error.

use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::oauth::{self, Tokens};
use crate::secrets::{self, Secret};

const URL: &str = "https://api.linear.app/graphql";

#[derive(Clone, Debug, Deserialize)]
pub struct Nodes<T> {
    pub nodes: Vec<T>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct State {
    pub id: String,
    pub name: String,
    pub color: String,
    pub position: f64,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TeamRef {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct User {
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct IssueRef {
    pub id: String,
    pub identifier: String,
    pub title: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Named {
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub url: String,
    pub title: String,
    pub source_type: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    pub body: String,
    pub created_at: String,
    pub user: Option<User>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub url: String,
    pub description: Option<String>,
    pub updated_at: String,
    #[serde(default)]
    pub created_at: String,
    /// Linear's 0 (none), 1 (urgent) to 4 (low).
    #[serde(default)]
    pub priority: f64,
    #[serde(default)]
    pub previous_identifiers: Vec<String>,
    pub state: State,
    pub team: TeamRef,
    pub assignee: Option<User>,
    pub creator: Option<User>,
    pub parent: Option<IssueRef>,
    pub project: Option<Named>,
    pub attachments: Nodes<Attachment>,
    /// Linear customer requests linked to the ticket.
    #[serde(default = "no_needs")]
    pub needs: Nodes<Need>,
    #[serde(default = "no_labels")]
    pub labels: Nodes<Named>,
    /// Its sub-tickets, as Linear lists them.
    #[serde(default = "no_children")]
    pub children: Nodes<Child>,
    /// Oldest first.
    pub comments: Nodes<Comment>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Need {
    pub customer: Option<Named>,
}

fn no_needs() -> Nodes<Need> {
    Nodes { nodes: Vec::new() }
}

fn no_labels() -> Nodes<Named> {
    Nodes { nodes: Vec::new() }
}

fn no_children() -> Nodes<Child> {
    Nodes { nodes: Vec::new() }
}

/// A sub-ticket, as much as its parent shows of it.
#[derive(Clone, Debug, Deserialize)]
pub struct Child {
    pub identifier: String,
    /// Its ids before a team move (ABC-810 handed to the bot is DEV-270).
    #[serde(default, rename = "previousIdentifiers")]
    pub previous_identifiers: Vec<String>,
    pub title: String,
    pub url: String,
    /// Missions hand the bot sub-tickets in the order they were filed.
    #[serde(default, rename = "createdAt")]
    pub created_at: String,
    pub state: ChildState,
    pub assignee: Option<User>,
    #[serde(default)]
    pub team: Option<TeamKey>,
    /// What it blocks or relates to (missions read `blocks`).
    #[serde(default = "no_relations")]
    pub relations: Nodes<Relation>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TeamKey {
    pub key: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relation {
    #[serde(rename = "type")]
    pub kind: String,
    pub related_issue: RelatedIssue,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RelatedIssue {
    pub identifier: String,
}

fn no_relations() -> Nodes<Relation> {
    Nodes { nodes: Vec::new() }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ChildState {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Team {
    pub id: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub name: String,
    pub states: Nodes<State>,
}

/// A file Linear hosts, the only place the Linear key is sent besides the API.
fn is_linear_upload(url: &str) -> bool {
    url.starts_with("https://uploads.linear.app/")
}

/// An address a ticket's image may be fetched from: https, and not this
/// Mac or the local network (localhost, `.local`, private and link-local
/// IP literals).
fn fetchable(url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host
        .trim_matches(['[', ']'])
        .trim_end_matches('.')
        .to_lowercase();
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1]))
        }
        Ok(std::net::IpAddr::V6(ip)) => {
            let first = ip.segments()[0];
            !(ip.is_loopback()
                || ip.is_unspecified()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || ip.to_ipv4_mapped().is_some())
        }
        Err(_) => {
            host != "localhost"
                && !host.ends_with(".localhost")
                && !host.ends_with(".local")
                && !host.ends_with(".internal")
        }
    }
}

/// A GraphQL document that writes: its first word is `mutation`.
fn is_mutation(query: &str) -> bool {
    query.trim_start().split(['(', '{', ' ', '\n']).next() == Some("mutation")
}

#[derive(Clone, Debug)]
pub struct Home {
    pub viewer: String,
    /// Their email: which `people` section of the profile is theirs.
    pub viewer_email: String,
    /// The Linear workspace's name: the user's own company, never a customer.
    pub org: String,
    /// `(key, name)` of the teams the viewer belongs to, for choosing which
    /// to watch.
    pub my_teams: Vec<(String, String)>,
    pub issues: Vec<Issue>,
    pub teams: Vec<Team>,
}

pub struct Client {
    http: reqwest::Client,
    auth: Auth,
    url: String,
    /// The stored Linear secret as it is now, for when another window of
    /// the app refreshed the tokens first.
    stored: fn() -> Option<String>,
    /// Refuse every mutation (main.rs `read_only`).
    read_only: bool,
}

enum Auth {
    /// A ready `Authorization` header value.
    Header(String),
    /// Signed in with Linear; refreshed under the lock, one refresh at a time.
    OAuth(tokio::sync::Mutex<Tokens>),
}

impl Client {
    pub fn new(key: &str) -> Self {
        let key = key.trim();
        // Personal API keys are sent as is; OAuth access tokens need Bearer.
        let auth = if let Some(tokens) = Tokens::parse(key) {
            Auth::OAuth(tokio::sync::Mutex::new(tokens))
        } else if key.starts_with("lin_api_") {
            Auth::Header(key.to_owned())
        } else {
            Auth::Header(format!("Bearer {key}"))
        };
        Self {
            http: reqwest::Client::new(),
            auth,
            url: URL.to_owned(),
            stored: || secrets::fresh(Secret::Linear),
            read_only: crate::read_only(),
        }
    }

    /// The header for the next request, refreshing a stale OAuth token first
    /// and saving the new one where the old one was.
    async fn authorization(&self) -> Result<String> {
        let tokens = match &self.auth {
            Auth::Header(header) => return Ok(header.clone()),
            Auth::OAuth(tokens) => tokens,
        };
        let mut tokens = tokens.lock().await;
        if tokens.stale() {
            let url = self.url.replace("/graphql", "/oauth/token");
            match oauth::refresh(&self.http, &url, &tokens).await {
                Ok(new) => {
                    *tokens = new;
                    // Tests must never overwrite the real Keychain entry.
                    if !cfg!(test) {
                        let _ = secrets::set(Secret::Linear, &tokens.to_json());
                    }
                }
                // Another window may have refreshed first and spent the
                // refresh token; its new tokens are in the Keychain.
                Err(error) => match (self.stored)()
                    .and_then(|s| Tokens::parse(&s))
                    .filter(|theirs| !theirs.stale() && theirs.refresh != tokens.refresh)
                {
                    Some(theirs) => *tokens = theirs,
                    None => return Err(error),
                },
            }
        }
        Ok(format!("Bearer {}", tokens.access))
    }

    /// A client for a fake Linear in tests.
    #[cfg(test)]
    pub fn at(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            ..Self::new("lin_api_test")
        }
    }

    async fn query<T: DeserializeOwned>(&self, query: &str, variables: Value) -> Result<T> {
        // Read-only: every write to Linear is a mutation, refused here, the
        // one place all requests pass.
        if self.read_only && is_mutation(query) {
            bail!("read-only: not changing Linear");
        }
        let response = self
            .http
            .post(&self.url)
            .header(reqwest::header::AUTHORIZATION, self.authorization().await?)
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await
            .map_err(|error| anyhow!("network error: {}", error.without_url()))?;
        let status = response.status();
        let mut body: Value = response
            .json()
            .await
            .map_err(|_| anyhow!("Linear answered {status} without a readable body"))?;
        // Linear reports auth failures and rate limits as GraphQL errors.
        if let Some(message) = body["errors"][0]["message"].as_str() {
            bail!("{message}");
        }
        if !status.is_success() {
            bail!("Linear answered {status}");
        }
        Ok(serde_json::from_value(body["data"].take())?)
    }

    /// The watched teams' open work plus anything assigned to the viewer,
    /// with the last two weeks of finished tickets so stories show progress.
    /// One ticket by its id or identifier ("WEB-1099"), with what the board
    /// reads from it: a link to a ticket not on the board opens here too.
    pub async fn issue(&self, id: &str) -> Result<Issue> {
        #[derive(Deserialize)]
        struct Data {
            issue: Issue,
        }
        let query = format!("query($id: String!) {{ issue(id: $id) {{ {ISSUE} }} }}");
        let mut data: Data = self.query(&query, json!({ "id": id })).await?;
        data.issue
            .comments
            .nodes
            .sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(data.issue)
    }

    /// The ticket a PR is attached to, by the PR's URL: Linear's GitHub
    /// integration links them even when the ticket is not on the board.
    pub async fn issue_for_url(&self, url: &str) -> Result<Option<Issue>> {
        let query =
            "query($url: String!) { attachmentsForURL(url: $url) { nodes { issue { id } } } }";
        let data: Value = self.query(query, json!({ "url": url })).await?;
        match data["attachmentsForURL"]["nodes"][0]["issue"]["id"].as_str() {
            Some(id) => Ok(Some(self.issue(id).await?)),
            None => Ok(None),
        }
    }

    pub async fn home(&self, teams: &[String]) -> Result<Home> {
        #[derive(Deserialize)]
        struct TeamName {
            key: String,
            name: String,
        }
        #[derive(Deserialize)]
        struct Viewer {
            name: String,
            #[serde(default)]
            email: String,
            teams: Nodes<TeamName>,
            #[serde(default)]
            organization: Option<Org>,
        }
        #[derive(Deserialize)]
        struct Org {
            name: String,
        }
        #[derive(Deserialize)]
        struct Data {
            viewer: Viewer,
            issues: Nodes<Issue>,
            teams: Nodes<Team>,
        }
        let query = format!(
            "query($teams: [String!]!) {{
                viewer {{ name email organization {{ name }} teams {{ nodes {{ key name }} }} }}
                issues(first: 100, orderBy: updatedAt, filter: {{ and: [
                    {{ or: [ {{ team: {{ key: {{ in: $teams }} }} }},
                             {{ assignee: {{ isMe: {{ eq: true }} }} }} ] }},
                    {{ state: {{ type: {{ nin: [\"canceled\", \"duplicate\"] }} }} }},
                    {{ or: [ {{ completedAt: {{ null: true }} }},
                             {{ completedAt: {{ gt: \"-P14D\" }} }} ] }}
                ] }}) {{ nodes {{ {ISSUE} }} }}
                teams {{ nodes {{ id key name states {{ nodes {STATE} }} }} }}
            }}"
        );
        let mut data: Data = self.query(&query, json!({ "teams": teams })).await?;
        for issue in &mut data.issues.nodes {
            // ISO 8601 timestamps sort chronologically as strings.
            issue
                .comments
                .nodes
                .sort_by(|a, b| a.created_at.cmp(&b.created_at));
        }
        Ok(Home {
            viewer_email: data.viewer.email.clone(),
            org: data
                .viewer
                .organization
                .take()
                .map(|o| o.name)
                .unwrap_or_default(),
            my_teams: data
                .viewer
                .teams
                .nodes
                .into_iter()
                .map(|t| (t.key, t.name))
                .collect(),
            viewer: data.viewer.name,
            issues: data.issues.nodes,
            teams: data.teams.nodes,
        })
    }

    /// An image from a ticket's markdown. Linear's own uploads need the
    /// Linear key; it goes to uploads.linear.app only, never to another
    /// host a description links. Anyone who can write a ticket picks the
    /// address, so only https to a public host (`fetchable`, every
    /// redirect too), and at most 20 MB however the server answers.
    pub async fn file(&self, url: &str) -> Result<Vec<u8>> {
        const MAX: usize = 20 << 20;
        if !fetchable(url) {
            bail!("not an https address on a public host: {url}");
        }
        let files = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::custom(|hop| {
                if hop.previous().len() < 5 && fetchable(hop.url().as_str()) {
                    hop.follow()
                } else {
                    hop.stop()
                }
            }))
            .build()?;
        let mut request = files.get(url);
        if is_linear_upload(url) {
            request = request.header("Authorization", self.authorization().await?);
        }
        let mut response = request.send().await?.error_for_status()?;
        if response.content_length().is_some_and(|n| n > MAX as u64) {
            bail!("the file is over 20 MB");
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len() + chunk.len() > MAX {
                bail!("the file is over 20 MB");
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    /// Creates or updates the ticket's one Orbit attachment. Creating
    /// it again answers "duplicate url" when it already exists, so an
    /// existing one is updated by id.
    pub async fn save_note(&self, issue_id: &str, subtitle: &str, metadata: &Value) -> Result<()> {
        let url = crate::note_url();
        let found: Value = self
            .query(
                "query($id: String!, $url: String!) {
                    issue(id: $id) { attachments(filter: { url: { eq: $url } }) { nodes { id } } }
                }",
                json!({ "id": issue_id, "url": url }),
            )
            .await?;
        let saved = match found["issue"]["attachments"]["nodes"][0]["id"].as_str() {
            Some(id) => {
                let query = "mutation($id: String!, $input: AttachmentUpdateInput!) {
                    attachmentUpdate(id: $id, input: $input) { success }
                }";
                let input =
                    json!({ "title": crate::NAME, "subtitle": subtitle, "metadata": metadata });
                let data: Value = self
                    .query(query, json!({ "id": id, "input": input }))
                    .await?;
                data["attachmentUpdate"]["success"] == true
            }
            None => {
                let query = "mutation($input: AttachmentCreateInput!) {
                    attachmentCreate(input: $input) { success }
                }";
                let input = json!({ "input": {
                    "issueId": issue_id,
                    "url": url,
                    "title": crate::NAME,
                    "subtitle": subtitle,
                    "metadata": metadata,
                }});
                let data: Value = self.query(query, input).await?;
                data["attachmentCreate"]["success"] == true
            }
        };
        if !saved {
            bail!("Linear did not save the Orbit note");
        }
        Ok(())
    }

    pub async fn set_state(&self, issue_id: &str, state_id: &str) -> Result<()> {
        let query = "mutation($id: String!, $stateId: String!) {
            issueUpdate(id: $id, input: { stateId: $stateId }) { success }
        }";
        let data: Value = self
            .query(query, json!({ "id": issue_id, "stateId": state_id }))
            .await?;
        if data["issueUpdate"]["success"] != true {
            bail!("Linear did not accept the state change");
        }
        Ok(())
    }

    /// Assigns the issue to the viewer: a build run here takes it from the
    /// bot, so the bot's poller does not build it a second time.
    pub async fn assign_to_me(&self, issue_id: &str) -> Result<()> {
        let me: Value = self.query("query { viewer { id } }", json!({})).await?;
        let Some(me) = me["viewer"]["id"].as_str() else {
            bail!("Linear did not say who you are");
        };
        let query = "mutation($id: String!, $me: String!) {
            issueUpdate(id: $id, input: { assigneeId: $me }) { success }
        }";
        let data: Value = self
            .query(query, json!({ "id": issue_id, "me": me }))
            .await?;
        if data["issueUpdate"]["success"] != true {
            bail!("Linear did not take the ticket from the bot");
        }
        Ok(())
    }

    /// Moves the issue to another team, in `state_id`, one of that team's.
    /// Moves a ticket (id or identifier) to the team with this key, in its
    /// state of this name: a mission handing a sub-ticket to the bot.
    pub async fn dispatch(&self, issue: &str, team_key: &str, state_name: &str) -> Result<()> {
        let query = "query($key: String!) { teams(filter: { key: { eq: $key } }) { nodes { id \
            states { nodes { id name } } } } }";
        let data: Value = self.query(query, json!({ "key": team_key })).await?;
        let team = &data["teams"]["nodes"][0];
        let team_id = team["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("no team {team_key}"))?;
        let state_id = team["states"]["nodes"]
            .as_array()
            .and_then(|s| s.iter().find(|s| s["name"] == state_name))
            .and_then(|s| s["id"].as_str())
            .ok_or_else(|| anyhow::anyhow!("{team_key} has no state {state_name}"))?;
        self.move_team(issue, team_id, state_id).await
    }

    pub async fn move_team(&self, issue_id: &str, team_id: &str, state_id: &str) -> Result<()> {
        let query = "mutation($id: String!, $teamId: String!, $stateId: String!) {
            issueUpdate(id: $id, input: { teamId: $teamId, stateId: $stateId }) { success }
        }";
        let data: Value = self
            .query(
                query,
                json!({ "id": issue_id, "teamId": team_id, "stateId": state_id }),
            )
            .await?;
        if data["issueUpdate"]["success"] != true {
            bail!("Linear did not move the ticket");
        }
        Ok(())
    }

    /// The user's latest notifications, raw, for `inbox::from_linear`.
    pub async fn notifications(&self) -> Result<Vec<Value>> {
        let query = "{ notifications(first: 50) { nodes {
            id type title subtitle url readAt archivedAt snoozedUntilAt unsnoozedAt updatedAt
            ... on IssueNotification { issue { id } }
            ... on PullRequestNotification { pullRequest { url status } }
        } } }";
        let data: Value = self.query(query, json!({})).await?;
        Ok(data["notifications"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    /// Marks these notifications read, as the Inbox does on open or `x`.
    pub async fn read_notifications(&self, ids: &[String]) -> Result<()> {
        let query = "mutation($id: String!, $at: DateTime!) {
            notificationUpdate(id: $id, input: { readAt: $at }) { success }
        }";
        let at = jiff::Timestamp::now().to_string();
        for id in ids {
            let data: Value = self.query(query, json!({ "id": id, "at": at })).await?;
            if data["notificationUpdate"]["success"] != true {
                bail!("Linear did not mark the notification read");
            }
        }
        Ok(())
    }

    /// The palette's Linear search: a ticket number typed out ("761",
    /// "abc-761") finds that ticket on every team, done or not; then the
    /// open tickets Linear's search matches.
    pub async fn find(&self, term: &str) -> Result<Vec<crate::capture::Duplicate>> {
        let mut found = Vec::new();
        if let Some((team, number)) = ticket_number(term) {
            #[derive(Deserialize)]
            struct Key {
                key: String,
            }
            #[derive(Deserialize)]
            struct Row {
                id: String,
                identifier: String,
                title: String,
                url: String,
                state: Named,
                team: Key,
            }
            #[derive(Deserialize)]
            struct Data {
                issues: Nodes<Row>,
            }
            let mut filter = json!({ "number": { "eq": number } });
            if let Some(team) = team {
                filter["team"] = json!({ "key": { "eqIgnoreCase": team } });
            }
            let data: Data = self
                .query(
                    r#"query($filter: IssueFilter!) {
                        issues(first: 8, filter: $filter) {
                            nodes { id identifier title url state { name } team { key } }
                        }
                    }"#,
                    json!({ "filter": filter }),
                )
                .await?;
            found.extend(
                data.issues
                    .nodes
                    .into_iter()
                    .map(|r| crate::capture::Duplicate {
                        id: r.id,
                        identifier: r.identifier,
                        title: r.title,
                        team: r.team.key,
                        state: r.state.name,
                        url: r.url,
                        evidence: String::new(),
                    }),
            );
        }
        for hit in self.search_open(term).await? {
            if !found.iter().any(|f| f.identifier == hit.identifier) {
                found.push(hit);
            }
        }
        Ok(found)
    }

    /// Open tickets on any team matching `term`, for the duplicate check.
    pub async fn search_open(&self, term: &str) -> Result<Vec<crate::capture::Duplicate>> {
        #[derive(Deserialize)]
        struct Key {
            key: String,
        }
        #[derive(Deserialize)]
        struct Row {
            id: String,
            identifier: String,
            title: String,
            url: String,
            description: Option<String>,
            state: Named,
            team: Key,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            search_issues: Nodes<Row>,
        }
        let data: Data = self
            .query(
                r#"query($term: String!) {
                    searchIssues(term: $term, first: 8, filter: {
                        state: { type: { nin: ["completed", "canceled", "duplicate"] } } }) {
                        nodes { id identifier title url description state { name } team { key } }
                    }
                }"#,
                json!({ "term": term }),
            )
            .await?;
        Ok(data
            .search_issues
            .nodes
            .into_iter()
            .map(|r| crate::capture::Duplicate {
                id: r.id,
                identifier: r.identifier,
                title: r.title,
                team: r.team.key,
                state: r.state.name,
                url: r.url,
                // The description stands in for evidence until Haiku names it.
                evidence: r.description.unwrap_or_default(),
            })
            .collect())
    }

    /// Creates a triaged ticket: in the team's Backlog (never Triage), with
    /// the priority, the `Bug` label for a bug, and the `P#` label (team's
    /// own first, else the workspace's). Returns `(id, identifier, url)`.
    pub async fn create_ticket(
        &self,
        team_key: &str,
        title: &str,
        description: &str,
        priority: u8,
        level: &str,
        bug: bool,
    ) -> Result<(String, String, String)> {
        #[derive(Deserialize)]
        struct StateRow {
            id: String,
            name: String,
            #[serde(rename = "type")]
            kind: String,
        }
        #[derive(Deserialize)]
        struct TeamRow {
            id: String,
            states: Nodes<StateRow>,
        }
        #[derive(Deserialize)]
        struct TeamKey {
            key: String,
        }
        #[derive(Deserialize)]
        struct Label {
            id: String,
            name: String,
            team: Option<TeamKey>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Lookup {
            teams: Nodes<TeamRow>,
            issue_labels: Nodes<Label>,
        }
        // Two small queries in one: all labels at once is over Linear's limit.
        let lookup: Lookup = self
            .query(
                "query($key: String!, $names: [String!], $level: String!) {
                    teams(filter: { key: { eq: $key } }) { nodes { id states { nodes { id name type } } } }
                    issueLabels(first: 50, filter: { or: [
                        { name: { in: $names } }, { name: { startsWith: $level } } ] }) {
                        nodes { id name team { key } } }
                }",
                json!({ "key": team_key, "names": ["Bug", level], "level": level }),
            )
            .await?;
        let team = lookup
            .teams
            .nodes
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("No Linear team with the key {team_key}"))?;
        let backlog = team
            .states
            .nodes
            .iter()
            .find(|s| s.name == "Backlog")
            .or_else(|| team.states.nodes.iter().find(|s| s.kind == "backlog"))
            .ok_or_else(|| anyhow!("{team_key} has no Backlog state"))?;
        let ours = |l: &&Label| l.team.as_ref().is_none_or(|t| t.key == team_key);
        let pick = |wanted: &dyn Fn(&str) -> bool| {
            let fits: Vec<&Label> = lookup
                .issue_labels
                .nodes
                .iter()
                .filter(ours)
                .filter(|l| wanted(&l.name))
                .collect();
            // The team's own label first, else the workspace's.
            fits.iter()
                .find(|l| l.team.is_some())
                .or(fits.first())
                .map(|l| l.id.clone())
        };
        let mut labels: Vec<String> = pick(&|n| n.starts_with(level)).into_iter().collect();
        if bug {
            labels.extend(pick(&|n| n == "Bug"));
        }
        #[derive(Deserialize)]
        struct Created {
            id: String,
            identifier: String,
            url: String,
        }
        #[derive(Deserialize)]
        struct Payload {
            issue: Created,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            issue_create: Payload,
        }
        let data: Data = self
            .query(
                "mutation($input: IssueCreateInput!) {
                    issueCreate(input: $input) { issue { id identifier url } }
                }",
                json!({ "input": {
                    "teamId": team.id,
                    "title": title,
                    "description": description,
                    "priority": priority,
                    "stateId": backlog.id,
                    "labelIds": labels,
                } }),
            )
            .await?;
        let issue = data.issue_create.issue;
        Ok((issue.id, issue.identifier, issue.url))
    }

    /// The first comment on the issue whose body contains `needle`, reading
    /// every comment, archived ones too; the home query only has 25.
    pub async fn find_comment(&self, issue_id: &str, needle: &str) -> Result<Option<Comment>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct PageInfo {
            has_next_page: bool,
            end_cursor: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Page {
            nodes: Vec<Comment>,
            page_info: PageInfo,
        }
        #[derive(Deserialize)]
        struct IssueComments {
            comments: Page,
        }
        #[derive(Deserialize)]
        struct Data {
            issue: IssueComments,
        }
        let query = "query($id: String!, $after: String) {
            issue(id: $id) {
                comments(first: 250, after: $after, includeArchived: true) {
                    nodes { id body createdAt user { name } }
                    pageInfo { hasNextPage endCursor }
                }
            }
        }";
        let mut after: Option<String> = None;
        loop {
            let data: Data = self
                .query(query, json!({ "id": issue_id, "after": after }))
                .await?;
            let page = data.issue.comments;
            if let Some(found) = page.nodes.into_iter().find(|c| c.body.contains(needle)) {
                return Ok(Some(found));
            }
            if !page.page_info.has_next_page || page.page_info.end_cursor.is_none() {
                return Ok(None);
            }
            after = page.page_info.end_cursor;
        }
    }

    pub async fn add_comment(&self, issue_id: &str, body: &str) -> Result<Comment> {
        #[derive(Deserialize)]
        struct Created {
            comment: Comment,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            comment_create: Created,
        }
        let query = "mutation($issueId: String!, $body: String!) {
            commentCreate(input: { issueId: $issueId, body: $body }) {
                comment { id body createdAt user { name } }
            }
        }";
        let data: Data = self
            .query(query, json!({ "issueId": issue_id, "body": body }))
            .await?;
        Ok(data.comment_create.comment)
    }

    /// A ticket's id and identifier, from either: an unknown one is an error
    /// naming it.
    async fn ticket(&self, issue: &str) -> Result<(String, String)> {
        let query = "query($id: String!) { issue(id: $id) { id identifier } }";
        let data: Value = self
            .query(query, json!({ "id": issue }))
            .await
            .map_err(|e| anyhow!("no ticket {issue} in Linear ({e})"))?;
        let it = &data["issue"];
        match (it["id"].as_str(), it["identifier"].as_str()) {
            (Some(id), Some(identifier)) => Ok((id.to_owned(), identifier.to_owned())),
            _ => bail!("no ticket {issue} in Linear"),
        }
    }

    /// One `orbit ctl` write, done only once Linear said yes: what was done.
    pub async fn write(&self, write: &Write) -> Result<String> {
        write.check().map_err(|e| anyhow!(e))?;
        match write {
            Write::Comment { issue, body } => {
                let (id, identifier) = self.ticket(issue).await?;
                self.add_comment(&id, body).await?;
                Ok(format!("commented on {identifier}"))
            }
            Write::Move { issue, state } => {
                let query = "query($id: String!) { issue(id: $id) { id identifier \
                    team { key states { nodes { id name } } } } }";
                let data: Value = self
                    .query(query, json!({ "id": issue }))
                    .await
                    .map_err(|e| anyhow!("no ticket {issue} in Linear ({e})"))?;
                let it = &data["issue"];
                let states: Vec<(String, String)> = it["team"]["states"]["nodes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| Some((s["id"].as_str()?.into(), s["name"].as_str()?.into())))
                    .collect();
                let identifier = it["identifier"].as_str().unwrap_or(issue);
                let team = it["team"]["key"].as_str().unwrap_or("its team");
                let (state_id, name) =
                    state_named(&states, state).map_err(|e| anyhow!("{team}: {e}"))?;
                self.set_state(it["id"].as_str().unwrap_or(issue), state_id)
                    .await?;
                Ok(format!("{identifier} moved to {name}"))
            }
            Write::Parent { issue, parent } => {
                let (id, identifier) = self.ticket(issue).await?;
                let parent = match parent {
                    Some(p) => Some(self.ticket(p).await?),
                    None => None,
                };
                let query = "mutation($id: String!, $parent: String) {
                    issueUpdate(id: $id, input: { parentId: $parent }) { success }
                }";
                let data: Value = self
                    .query(
                        query,
                        json!({ "id": id, "parent": parent.as_ref().map(|p| &p.0) }),
                    )
                    .await?;
                if data["issueUpdate"]["success"] != true {
                    bail!("Linear did not change the parent");
                }
                Ok(match parent {
                    Some((_, p)) => format!("{identifier} is under {p}"),
                    None => format!("{identifier} has no parent"),
                })
            }
            Write::Block { blocker, blocked } => {
                let (a, blocker) = self.ticket(blocker).await?;
                let (b, blocked) = self.ticket(blocked).await?;
                let query = "mutation($a: String!, $b: String!) {
                    issueRelationCreate(input: { issueId: $a, relatedIssueId: $b, type: blocks }) { success }
                }";
                let data: Value = self.query(query, json!({ "a": a, "b": b })).await?;
                if data["issueRelationCreate"]["success"] != true {
                    bail!("Linear did not add the relation");
                }
                Ok(format!("{blocker} blocks {blocked}"))
            }
        }
    }
}

/// What `orbit ctl` writes to Linear with Orbit's own key: tickets by
/// identifier (or id), states by name, so the clone needs no connector.
#[derive(Clone, Debug, PartialEq, serde::Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Write {
    Comment {
        issue: String,
        body: String,
    },
    /// To the state of this name in the ticket's team, any case.
    Move {
        issue: String,
        state: String,
    },
    /// `None` takes the parent away.
    Parent {
        issue: String,
        parent: Option<String>,
    },
    /// A `blocks` relation: `blocker` blocks `blocked`.
    Block {
        blocker: String,
        blocked: String,
    },
}

impl Write {
    /// Refused before anything is sent: an empty part, a ticket blocking
    /// itself, a comment the bot's poller would run (CLAUDE.md Safety).
    pub fn check(&self) -> Result<(), String> {
        let parts: Vec<&str> = match self {
            Write::Comment { issue, body } => vec![issue, body],
            Write::Move { issue, state } => vec![issue, state],
            Write::Parent { issue, parent } => {
                vec![issue.as_str(), parent.as_deref().unwrap_or("none")]
            }
            Write::Block { blocker, blocked } => vec![blocker, blocked],
        };
        if parts.iter().any(|p| p.trim().is_empty()) {
            return Err("a ticket, a name or the text is missing; see orbit ctl help".into());
        }
        match self {
            Write::Comment { body, .. } if crate::story::has_bot_command(body) => Err(
                "refused: the text holds `@<name> <Word>!`, which the bot's poller runs as a command"
                    .into(),
            ),
            Write::Block { blocker, blocked } if blocker.eq_ignore_ascii_case(blocked) => {
                Err("a ticket cannot block itself".into())
            }
            Write::Parent {
                issue,
                parent: Some(p),
            } if issue.eq_ignore_ascii_case(p) => Err("a ticket cannot be its own parent".into()),
            _ => Ok(()),
        }
    }
}

/// The `(id, name)` of the state called `wanted` (any case), or an error
/// listing the states there are.
fn state_named<'a>(
    states: &'a [(String, String)],
    wanted: &str,
) -> Result<(&'a str, &'a str), String> {
    states
        .iter()
        .find(|(_, name)| name.eq_ignore_ascii_case(wanted.trim()))
        .map(|(id, name)| (id.as_str(), name.as_str()))
        .ok_or_else(|| {
            let names: Vec<&str> = states.iter().map(|(_, n)| n.as_str()).collect();
            format!(
                "no state \"{}\"; its states: {}",
                wanted.trim(),
                names.join(", ")
            )
        })
}

const STATE: &str = "{ id name color position type }";

/// What the board reads from a ticket, in `home` and `issue`.
const ISSUE: &str = "id identifier title url description updatedAt createdAt priority previousIdentifiers \
    state { id name color position type } team { id } \
    assignee { name } creator { name } \
    parent { id identifier title url } project { name } \
    children(first: 15) { nodes { identifier previousIdentifiers title url createdAt state { name type } assignee { name } team { key } \
      relations(first: 10) { nodes { type relatedIssue { identifier } } } } } \
    attachments(first: 15) { nodes { url title sourceType metadata } } \
    needs(first: 5) { nodes { customer { name } } } \
    labels(first: 10) { nodes { name } } \
    comments(first: 25) { nodes { id body createdAt user { name } } }";

/// "761" or "ABC-761" (any case, around spaces): the team key, if one,
/// and the number.
pub(crate) fn ticket_number(term: &str) -> Option<(Option<String>, u64)> {
    let term = term.trim();
    let (team, number) = match term.rsplit_once('-') {
        Some((team, number)) => (Some(team), number),
        None => (None, term),
    };
    let number = number.parse().ok()?;
    match team {
        Some(t) if t.is_empty() || !t.chars().all(|c| c.is_ascii_alphanumeric()) => None,
        team => Some((team.map(str::to_uppercase), number)),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_ticket_reads_its_sub_tickets_and_parent() {
        let issue: super::Issue = serde_json::from_value(serde_json::json!({
            "id": "1", "identifier": "ABC-769", "title": "t", "url": "u", "updatedAt": "",
            "state": {"id": "s", "name": "Backlog", "color": "", "position": 0.0, "type": "backlog"},
            "team": {"id": "t"},
            "parent": {"id": "0", "identifier": "ABC-1", "title": "p", "url": "https://linear.app/x/issue/ABC-1"},
            "attachments": {"nodes": []},
            "comments": {"nodes": []},
            "children": {"nodes": [{"identifier": "DEV-207", "title": "c", "url": "https://linear.app/x/issue/DEV-207",
                "state": {"name": "Todo", "type": "unstarted"}, "assignee": null}]}
        }))
        .unwrap();
        assert_eq!(issue.children.nodes[0].identifier, "DEV-207");
        assert_eq!(issue.children.nodes[0].state.kind, "unstarted");
        assert_eq!(
            issue.parent.unwrap().url,
            "https://linear.app/x/issue/ABC-1"
        );
    }

    use super::*;

    #[test]
    fn a_state_is_found_by_name_in_any_case_or_the_team_states_are_listed() {
        let states = [
            ("s1".to_owned(), "Todo".to_owned()),
            ("s2".to_owned(), "In Review".to_owned()),
        ];
        assert_eq!(state_named(&states, " in review "), Ok(("s2", "In Review")));
        let error = state_named(&states, "Doing").unwrap_err();
        assert!(
            error.contains("\"Doing\"") && error.contains("Todo, In Review"),
            "{error}"
        );
    }

    #[test]
    fn ctl_writes_refuse_bot_commands_and_nonsense_before_sending() {
        let comment = |body: &str| super::Write::Comment {
            issue: "DEV-1".into(),
            body: body.into(),
        };
        assert!(
            comment("PR is up: https://github.com/o/r/pull/1")
                .check()
                .is_ok()
        );
        assert!(
            comment("ready, @Robo Build!")
                .check()
                .unwrap_err()
                .contains("poller")
        );
        assert!(comment("  ").check().is_err());
        let block = |a: &str, b: &str| super::Write::Block {
            blocker: a.into(),
            blocked: b.into(),
        };
        assert!(block("DEV-1", "DEV-2").check().is_ok());
        assert!(block("DEV-1", "dev-1").check().is_err());
        let parent = |p: Option<&str>| super::Write::Parent {
            issue: "DEV-2".into(),
            parent: p.map(Into::into),
        };
        assert!(parent(None).check().is_ok());
        assert!(parent(Some("DEV-2")).check().is_err());
    }

    /// A move asks for the ticket's team states, then sets the one named;
    /// the answer comes only after Linear's `success`.
    #[tokio::test]
    async fn a_move_sets_the_state_named_in_the_tickets_team() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/graphql", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for answer in [
                r#"{"data":{"issue":{"id":"u1","identifier":"DEV-1","team":{"key":"DEV","states":{"nodes":[{"id":"s1","name":"Todo"},{"id":"s2","name":"In Review"}]}}}}}"#,
                r#"{"data":{"issueUpdate":{"success":true}}}"#,
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                bodies.push(read_request(&mut stream));
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
                stream.write_all(reply.as_bytes()).unwrap();
            }
            bodies
        });
        let done = Client::at(&url)
            .write(&super::Write::Move {
                issue: "DEV-1".into(),
                state: "in review".into(),
            })
            .await
            .unwrap();
        assert_eq!(done, "DEV-1 moved to In Review");
        let bodies = server.join().unwrap();
        assert!(bodies[1].contains(r#""stateId":"s2""#), "{}", bodies[1]);
    }

    #[test]
    fn a_typed_ticket_number_is_read_with_or_without_its_team() {
        assert_eq!(ticket_number("761"), Some((None, 761)));
        assert_eq!(ticket_number(" abc-761 "), Some((Some("ABC".into()), 761)));
        assert_eq!(ticket_number("builder 761"), None);
        assert_eq!(ticket_number("-761"), None);
        assert_eq!(ticket_number("builder"), None);
    }
    use std::io::{BufRead, BufReader, Read, Write};

    /// One HTTP request's body, read off a fake Linear's stream.
    fn read_request(stream: &mut std::net::TcpStream) -> String {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some(n) = line.to_lowercase().strip_prefix("content-length:") {
                length = n.trim().parse().unwrap();
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        String::from_utf8(body).unwrap()
    }

    #[tokio::test]
    async fn read_only_refuses_every_write_before_it_is_sent() {
        // Nothing listens on port 9: a request that got out would fail
        // with a network error, not this one.
        let client = Client {
            read_only: true,
            ..Client::at("http://127.0.0.1:9/graphql")
        };
        let error = client.add_comment("issue", "hello").await.unwrap_err();
        assert!(error.to_string().starts_with("read-only"), "{error}");
        let error = client.set_state("issue", "state").await.unwrap_err();
        assert!(error.to_string().starts_with("read-only"), "{error}");
    }

    #[test]
    fn ticket_images_come_only_from_public_https_hosts() {
        for ok in [
            "https://uploads.linear.app/a/b/c.png",
            "https://i.imgur.com/a.png",
            "https://8.8.8.8/a.png",
        ] {
            assert!(fetchable(ok), "{ok}");
        }
        for bad in [
            "http://i.imgur.com/a.png",
            "https://localhost/a.png",
            "https://printer.local/a.png",
            "https://127.0.0.1:8080/x",
            "https://10.0.0.5/x",
            "https://192.168.1.1/x",
            "https://169.254.169.254/latest/meta-data",
            "https://100.100.1.1/x",
            "https://[::1]/x",
            "https://[fd00::1]/x",
            "file:///etc/passwd",
            "nonsense",
        ] {
            assert!(!fetchable(bad), "{bad}");
        }
    }

    #[test]
    fn the_key_goes_to_linear_uploads_only() {
        assert!(is_linear_upload("https://uploads.linear.app/a/b/c.png"));
        for other in [
            "https://uploads.linear.app.evil.com/a.png",
            "http://uploads.linear.app/a.png",
            "https://i.imgur.com/a.png",
        ] {
            assert!(!is_linear_upload(other), "{other}");
        }
    }

    #[test]
    fn only_mutations_count_as_writes() {
        for write in [
            "mutation($id: String!) { issueUpdate(id: $id) { success } }",
            "  mutation { commentCreate(input: {}) { success } }",
            "mutation{attachmentCreate}",
        ] {
            assert!(is_mutation(write), "{write}");
        }
        for read in [
            "query { viewer { name } }",
            "{ issues { nodes { id } } }",
            "query($m: String!) { issue(id: $m) { title } }",
        ] {
            assert!(!is_mutation(read), "{read}");
        }
    }

    /// A stale OAuth token is refreshed once, then the new one is sent.
    #[tokio::test]
    async fn stale_oauth_token_is_refreshed_before_the_query() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for stream in listener.incoming().take(2) {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let (mut first, mut auth, mut length) = (String::new(), String::new(), 0);
                reader.read_line(&mut first).unwrap();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    let lower = line.to_lowercase();
                    if let Some(n) = lower.strip_prefix("content-length:") {
                        length = n.trim().parse().unwrap();
                    }
                    if lower.starts_with("authorization:") {
                        auth = line["authorization:".len()..].trim().to_owned();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let body = String::from_utf8(body).unwrap();
                let reply = if first.contains("/oauth/token") {
                    assert!(body.contains("grant_type=refresh_token"));
                    assert!(body.contains("refresh_token=old-refresh"));
                    r#"{"access_token":"new","refresh_token":"new-refresh","expires_in":86399}"#
                } else {
                    r#"{"data":{"viewer":{"id":"u"}}}"#
                };
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                )
                .unwrap();
                seen.push((first, auth));
            }
            seen
        });
        let stale = Tokens {
            access: "old".into(),
            refresh: "old-refresh".into(),
            expires: 0,
            client: "test-client".into(),
        };
        let client = Client {
            url: format!("{base}/graphql"),
            stored: || None,
            ..Client::new(&stale.to_json())
        };
        client
            .query::<Value>("query { viewer { id } }", json!({}))
            .await
            .unwrap();
        let seen = seen.join().unwrap();
        assert!(seen[0].0.starts_with("POST /oauth/token"));
        assert_eq!(seen[1].1, "Bearer new");
        let Auth::OAuth(tokens) = &client.auth else {
            panic!("not an OAuth client")
        };
        assert_eq!(tokens.lock().await.refresh, "new-refresh");
    }

    /// Two windows, one refresh token: the one that loses the refresh uses
    /// the tokens the other stored instead of failing.
    #[tokio::test]
    async fn a_refresh_lost_to_another_window_uses_its_tokens() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for stream in listener.incoming().take(2) {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let (mut first, mut auth, mut length) = (String::new(), String::new(), 0);
                reader.read_line(&mut first).unwrap();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    let lower = line.to_lowercase();
                    if let Some(n) = lower.strip_prefix("content-length:") {
                        length = n.trim().parse().unwrap();
                    }
                    if lower.starts_with("authorization:") {
                        auth = line["authorization:".len()..].trim().to_owned();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let (status, reply) = if first.contains("/oauth/token") {
                    ("400 Bad Request", r#"{"error":"invalid_grant"}"#)
                } else {
                    ("200 OK", r#"{"data":{"viewer":{"id":"u"}}}"#)
                };
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                )
                .unwrap();
                seen.push((first, auth));
            }
            seen
        });
        let stale = Tokens {
            access: "old".into(),
            refresh: "spent-refresh".into(),
            expires: 0,
            client: "test-client".into(),
        };
        let client = Client {
            url: format!("{base}/graphql"),
            stored: || {
                let theirs = Tokens {
                    access: "theirs".into(),
                    refresh: "their-refresh".into(),
                    expires: u64::MAX / 2,
                    client: "test-client".into(),
                };
                Some(theirs.to_json())
            },
            ..Client::new(&stale.to_json())
        };
        client
            .query::<Value>("query { viewer { id } }", json!({}))
            .await
            .unwrap();
        let seen = seen.join().unwrap();
        assert!(seen[0].0.starts_with("POST /oauth/token"));
        assert_eq!(seen[1].1, "Bearer theirs");
    }
}

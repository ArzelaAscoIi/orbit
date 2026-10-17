//! incident.io: the live incidents, pulled with a read-only API key and
//! shown at the top of the KTLO view, the alerts of the last day (the
//! Alerts view), and the brief an investigation session starts from.
//!
//! The app reads incidents over the REST API (`GET /v2/incidents`); the
//! incident.io MCP is for the Claude session that investigates one. No key,
//! no incidents: the feature stays out of sight (Settings, connections).

use anyhow::{Result, bail};
use serde_json::Value;

const URL: &str = "https://api.incident.io/v2/incidents";
const ALERTS: &str = "https://api.incident.io/v2/alerts";

/// An alert from an alert source (`GET /v2/alerts`): firing or resolved.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Alert {
    pub id: String,
    pub title: String,
    pub description: String,
    /// "firing" or "resolved".
    pub status: String,
    /// The alert in the system that raised it, when it has a link.
    pub source_url: String,
    pub created_at: String,
    pub resolved_at: String,
    /// The alert source's parsed attributes, as (name, value).
    pub attributes: Vec<(String, String)>,
}

impl Alert {
    pub fn firing(&self) -> bool {
        self.status == "firing"
    }
}

/// The alerts of the last 24 hours, firing first, newest first in each.
pub async fn alerts(http: &reqwest::Client, key: &str) -> Result<Vec<Alert>> {
    let since = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(24);
    let response = http
        .get(ALERTS)
        .bearer_auth(key)
        // 50 is the most a page may hold (more is a 422).
        .query(&[
            ("page_size", "50".to_owned()),
            ("created_at[gte]", since.to_string()),
        ])
        .send()
        .await?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        bail!("incident.io said {status}: {}", reason(&body));
    }
    Ok(alerts_from_json(&body))
}

/// Why incident.io refused: its first error's message, else its type.
fn reason(body: &Value) -> String {
    body["errors"][0]["message"]
        .as_str()
        .or(body["message"].as_str())
        .or(body["type"].as_str())
        .unwrap_or("no reason given")
        .to_owned()
}

/// The first message of an alert's session: the alert, where it came from,
/// and that nothing is posted without asking.
pub fn alert_brief(alert: &Alert, panes: bool) -> String {
    let attributes: Vec<String> = alert
        .attributes
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect();
    let resolved = match alert.resolved_at.as_str() {
        "" => String::new(),
        at => format!(" · resolved {at}"),
    };
    format!(
        "Investigate this incident.io alert: {title}\n\
         Status: {status} · fired {created}{resolved}\n\
         Source: {source}\nAttributes: {attributes}\n\n\
         {description}\n\n\
         Find out what fired it and whether it still matters: the logs, traces and metrics your \
         MCP tools reach, and related Linear tickets. Do not log in to or connect to a cluster \
         or cloud account from the shell.{how} Report here, short: what broke, since \
         when, why, and the fix. Do not post to incident.io, Slack or Linear without asking me \
         first.",
        title = alert.title,
        status = alert.status,
        created = alert.created_at,
        source = if alert.source_url.is_empty() {
            "none"
        } else {
            &alert.source_url
        },
        attributes = if attributes.is_empty() {
            "none".to_owned()
        } else {
            attributes.join(", ")
        },
        description = alert.description.trim(),
        // The profile's how-to names the panes; only when they are open.
        how = match crate::profile::get().investigate.trim() {
            how if panes && !how.is_empty() => format!(" {how}"),
            _ => String::new(),
        },
    )
}

/// Alerts from a `/v2/alerts` answer, firing first, newest first in each.
pub fn alerts_from_json(body: &Value) -> Vec<Alert> {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    // A value is a label, a literal or a catalog entry; arrays list theirs.
    let value = |v: &Value| -> Option<String> {
        v["label"]
            .as_str()
            .or(v["literal"].as_str())
            .or(v["catalog_entry"]["name"].as_str())
            .map(str::to_owned)
    };
    let mut alerts: Vec<Alert> = body["alerts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| {
            let attributes = a["attributes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|attr| {
                    let name = attr["attribute"]["name"].as_str()?.to_owned();
                    let one = value(&attr["value"]);
                    let many: Vec<String> = attr["array_value"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(value)
                        .collect();
                    let v = one.or_else(|| (!many.is_empty()).then(|| many.join(", ")))?;
                    Some((name, v))
                })
                .collect();
            Alert {
                id: text(&a["id"]),
                title: text(&a["title"]),
                description: text(&a["description"]),
                status: text(&a["status"]),
                source_url: text(&a["source_url"]),
                created_at: text(&a["created_at"]),
                resolved_at: text(&a["resolved_at"]),
                attributes,
            }
        })
        .filter(|a| !a.id.is_empty())
        .collect();
    alerts.sort_by(|a, b| (!a.firing(), &b.created_at).cmp(&(!b.firing(), &a.created_at)));
    alerts
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Incident {
    pub id: String,
    /// "INC-123".
    pub reference: String,
    pub name: String,
    pub severity: String,
    pub status: String,
    pub summary: String,
    pub permalink: String,
    pub lead: Option<String>,
    /// A link that opens the incident's Slack channel, when it has one.
    pub slack: Option<String>,
    pub created_at: String,
}

/// The live incidents: active ones, and ones still in triage.
pub async fn live(http: &reqwest::Client, key: &str) -> Result<Vec<Incident>> {
    let mut all = Vec::new();
    for category in ["live", "triage"] {
        let response = http
            .get(URL)
            .bearer_auth(key)
            .query(&[("status_category[one_of]", category), ("page_size", "50")])
            .send()
            .await?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or_default();
        if !status.is_success() {
            let why = body["type"]
                .as_str()
                .or(body["message"].as_str())
                .unwrap_or("no reason given");
            bail!("incident.io said {status}: {why}");
        }
        all.extend(from_json(&body));
    }
    all.dedup_by(|a, b| a.id == b.id);
    Ok(all)
}

/// Incidents from a `/v2/incidents` answer; fields that are missing stay empty.
pub fn from_json(body: &Value) -> Vec<Incident> {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    body["incidents"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| {
            let lead = i["incident_role_assignments"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|a| a["role"]["role_type"] == "lead")
                .and_then(|a| a["assignee"]["name"].as_str())
                .map(str::to_owned);
            let slack = i["slack_channel_id"]
                .as_str()
                .filter(|c| !c.is_empty())
                .map(|channel| match i["slack_team_id"].as_str() {
                    Some(team) => {
                        format!("https://slack.com/app_redirect?team={team}&channel={channel}")
                    }
                    None => format!("https://slack.com/app_redirect?channel={channel}"),
                });
            Incident {
                id: text(&i["id"]),
                reference: text(&i["reference"]),
                name: text(&i["name"]),
                severity: text(&i["severity"]["name"]),
                status: text(&i["incident_status"]["name"]),
                summary: text(&i["summary"]),
                permalink: text(&i["permalink"]),
                lead,
                slack,
                created_at: text(&i["created_at"]),
            }
        })
        .filter(|i| !i.id.is_empty())
        .collect()
}

/// The first message of an investigation session: the incident, where to
/// look, and that nothing is posted without asking.
pub fn brief(incident: &Incident) -> String {
    let lead = incident.lead.as_deref().unwrap_or("nobody yet");
    let slack = incident.slack.as_deref().unwrap_or("none");
    format!(
        "Investigate incident {reference}: {name}\n\
         Severity: {severity} · Status: {status} · Lead: {lead}\n\
         incident.io: {link}\nSlack channel: {slack}\n\n\
         Summary: {summary}\n\n\
         Read the incident's timeline and updates first (the incident.io MCP, or the link). \
         Then find the root cause from the logs, traces and metrics your MCP tools reach, and \
         related Linear tickets. Do not log in to or connect to a cluster or cloud account from \
         the shell; the panes next to this one are mine.{how} Report findings here, short: \
         what is broken, since when, why, and the fix. Do not post to the incident, Slack or \
         Linear without asking me first.",
        how = match crate::profile::get().investigate.trim() {
            "" => String::new(),
            how => format!(" {how}"),
        },
        reference = incident.reference,
        name = incident.name,
        severity = if incident.severity.is_empty() {
            "unset"
        } else {
            &incident.severity
        },
        status = incident.status,
        link = incident.permalink,
        summary = if incident.summary.is_empty() {
            "none written yet"
        } else {
            &incident.summary
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brief_of_incident() -> String {
        brief(&Incident {
            reference: "INC-1".into(),
            name: "x".into(),
            ..Default::default()
        })
    }

    #[test]
    fn alerts_read_firing_first_with_their_attributes() {
        let body: Value = serde_json::from_str(
            r#"{"alerts":[
              {"id":"01A","title":"old resolved","status":"resolved","created_at":"2026-10-01T08:00:00Z"},
              {"id":"01B","title":"PG::Error failed to connect","status":"firing",
               "description":"CPU over 75%","source_url":"https://grafana/x",
               "created_at":"2026-10-01T09:00:00Z",
               "attributes":[
                 {"attribute":{"name":"service"},"value":{"label":"Payments"}},
                 {"attribute":{"name":"teams"},"array_value":[{"literal":"a"},{"catalog_entry":{"name":"b"}}]}]},
              {"title":"no id, dropped"}]}"#,
        )
        .unwrap();
        let alerts = alerts_from_json(&body);
        assert_eq!(alerts.len(), 2);
        assert_eq!(alerts[0].title, "PG::Error failed to connect");
        assert_eq!(
            alerts[0].attributes,
            [
                ("service".into(), "Payments".into()),
                ("teams".into(), "a, b".into())
            ]
        );
        let brief = alert_brief(&alerts[0], false);
        assert!(brief.contains("MCP tools reach"));
        assert!(brief.contains("Do not log in to or connect to a cluster"));
        let incident = brief_of_incident();
        assert!(incident.contains("Do not log in to or connect to a cluster"));
        assert!(!incident.contains("you can reach, the panes"));
        assert!(brief.starts_with("Investigate this incident.io alert: PG::Error"));
        assert!(brief.contains("service: Payments, teams: a, b"));
        let refused: Value = serde_json::from_str(
            r#"{"type":"validation_error","errors":[{"message":"page_size must be lesser or equal than 50"}]}"#,
        )
        .unwrap();
        assert_eq!(
            reason(&refused),
            "page_size must be lesser or equal than 50"
        );
    }

    #[test]
    fn incidents_read_from_the_api_answer() {
        let body: Value = serde_json::from_str(
            r#"{"incidents":[{
                "id":"01FDAG4SAP5TYPT98WGR2N7W91","reference":"INC-123","name":"Search is down",
                "severity":{"name":"Major"},"incident_status":{"name":"Investigating","category":"live"},
                "summary":"Queries time out for EU customers.","permalink":"https://app.incident.io/acme/incidents/123",
                "slack_channel_id":"C0123456789","slack_team_id":"T0123456789",
                "created_at":"2026-09-24T10:00:00Z",
                "incident_role_assignments":[
                    {"role":{"role_type":"reporter"},"assignee":{"name":"Robin"}},
                    {"role":{"role_type":"lead"},"assignee":{"name":"Kim"}}]
            },{"name":"no id, dropped"}]}"#,
        )
        .unwrap();
        let incidents = from_json(&body);
        assert_eq!(incidents.len(), 1);
        let i = &incidents[0];
        assert_eq!(
            (i.reference.as_str(), i.severity.as_str()),
            ("INC-123", "Major")
        );
        assert_eq!(i.lead.as_deref(), Some("Kim"));
        assert_eq!(
            i.slack.as_deref(),
            Some("https://slack.com/app_redirect?team=T0123456789&channel=C0123456789")
        );
        let brief = brief(i);
        assert!(brief.contains("INC-123") && brief.contains(&i.permalink));
        assert!(brief.contains("without asking me first"));
    }
}

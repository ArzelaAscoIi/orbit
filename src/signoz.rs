//! SigNoz through its MCP gateway (the profile's `stats.mcp`), no API key:
//! the gateway sits on the company's private network, which is the access
//! control, and holds SigNoz's key itself. Three JSON-RPC posts per call:
//! `initialize` (gives a session id), `notifications/initialized`, then
//! `tools/call`. The Stats view (stats.rs) reads the engineering velocity
//! numbers the profile's dashboard shows, for the dashboard's repos.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

/// The team's PR numbers over the window, and the user's own share.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Team {
    pub touched: f64,
    pub merged: f64,
    pub merged_bot: f64,
    pub merged_ai: f64,
    /// PRs the user authored that merged.
    pub mine_merged: f64,
    /// The user's reviews by state (`APPROVED`, `COMMENTED`, ...).
    pub my_reviews: Vec<(String, f64)>,
    /// Who the numbers are about (`gh api user`).
    pub login: String,
}

const ACCEPT: &str = "application/json, text/event-stream";

/// Calls one SigNoz MCP tool and returns its result's text, parsed.
pub async fn call(http: &reqwest::Client, url: &str, tool: &str, args: Value) -> Result<Value> {
    let post = |body: Value, session: Option<&str>| {
        let mut request = http
            .post(url)
            .header("Content-Type", "application/json")
            .header("Accept", ACCEPT)
            .timeout(std::time::Duration::from_secs(60))
            .json(&body);
        if let Some(id) = session {
            request = request.header("Mcp-Session-Id", id);
        }
        request.send()
    };
    let init = post(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": crate::NAME, "version": crate::VERSION}}}),
        None,
    )
    .await
    .with_context(|| format!("SigNoz is not reachable at {url} (on the private network?)"))?;
    let session = init
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .context("SigNoz's gateway gave no session")?;
    post(
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        Some(&session),
    )
    .await?;
    let answer = post(
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
               "params": {"name": tool, "arguments": args}}),
        Some(&session),
    )
    .await?
    .text()
    .await?;
    result_text(&answer)
}

/// The tool's answer: the JSON-RPC result's first text, parsed; the body
/// may be plain JSON or one server-sent event.
fn result_text(body: &str) -> Result<Value> {
    let json = body
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .unwrap_or(body);
    let reply: Value = serde_json::from_str(json.trim()).context("SigNoz answered no JSON")?;
    if let Some(error) = reply.get("error") {
        bail!("SigNoz: {}", error["message"].as_str().unwrap_or("error"));
    }
    let text = reply["result"]["content"][0]["text"]
        .as_str()
        .context("SigNoz answered no text")?;
    serde_json::from_str(text).context("SigNoz's text is not JSON")
}

/// The rows of a scalar query: each group's labels joined, and its value.
fn rows(result: &Value) -> Vec<(String, f64)> {
    let Some(table) = result["data"]["data"]["results"][0]["data"].as_array() else {
        return Vec::new();
    };
    table
        .iter()
        .filter_map(|row| {
            let cells = row.as_array()?;
            let (value, labels) = cells.split_last()?;
            let label = labels
                .iter()
                .map(|l| l.as_str().map_or_else(|| l.to_string(), str::to_owned))
                .collect::<Vec<_>>()
                .join(" ");
            Some((label, value.as_f64()?))
        })
        .collect()
}

/// A single number, 0 when the query found nothing.
fn scalar(result: &Value) -> f64 {
    rows(result).first().map_or(0.0, |(_, v)| *v)
}

/// The filter all PR numbers share: the PR event logs, on these repos.
fn pr_filter(repos: &[String], extra: &str) -> String {
    let list = repos
        .iter()
        .map(|r| format!("'{}'", r.replace('\'', "")))
        .collect::<Vec<_>>()
        .join(",");
    let repos = if repos.is_empty() {
        String::new()
    } else {
        format!(" AND repo.full_name in ({list})")
    };
    format!("service.name = 'github-pr-events' AND pr.number EXISTS{repos}{extra}")
}

/// The dashboard's headline numbers over `window` (`7d`) and the user's.
pub async fn team(
    http: &reqwest::Client,
    url: &str,
    repos: &[String],
    login: &str,
    window: &str,
) -> Result<Team> {
    let login = login.replace('\'', "");
    let distinct = |extra: String| {
        call(
            http,
            url,
            "signoz_aggregate_logs",
            json!({"aggregation": "count_distinct", "aggregateOn": "pr.number",
                   "filter": pr_filter(repos, &extra), "timeRange": window}),
        )
    };
    let reviews = call(
        http,
        url,
        "signoz_aggregate_logs",
        json!({"aggregation": "count", "groupBy": "review.state",
               "filter": pr_filter(repos, &format!(
                   " AND review.state EXISTS AND review.author_login = '{login}'"))
                   .replace(" AND pr.number EXISTS", ""),
               "timeRange": window}),
    );
    let (touched, merged, bot, ai, mine, reviews) = tokio::join!(
        distinct(String::new()),
        distinct(" AND pr.merged = true".into()),
        distinct(" AND pr.merged = true AND pr.author_is_bot = true".into()),
        distinct(" AND pr.merged = true AND pr.ai_assisted = true".into()),
        distinct(format!(
            " AND pr.merged = true AND pr.author_login = '{login}'"
        )),
        reviews,
    );
    Ok(Team {
        touched: scalar(&touched?),
        merged: scalar(&merged?),
        merged_bot: scalar(&bot?),
        merged_ai: scalar(&ai?),
        mine_merged: scalar(&mine?),
        my_reviews: rows(&reviews?),
        login,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_answer_is_read_as_json_or_one_event() {
        let inner = r#"{"status":"success","data":{"type":"scalar","data":{"results":[{"queryName":"A","data":[[196]]}]}}}"#;
        let reply = json!({"jsonrpc": "2.0", "id": 2, "result": {"content": [{"type": "text", "text": inner}]}});
        let plain = result_text(&reply.to_string()).unwrap();
        assert_eq!(scalar(&plain), 196.0);
        let event = format!("event: message\ndata: {reply}\n\n");
        assert_eq!(scalar(&result_text(&event).unwrap()), 196.0);
        let error = json!({"jsonrpc": "2.0", "id": 2, "error": {"message": "no such tool"}});
        assert!(result_text(&error.to_string()).is_err());
    }

    #[test]
    fn grouped_rows_keep_their_labels_and_nothing_found_is_zero() {
        let grouped = json!({"data": {"data": {"results": [{"data": [["APPROVED", 12], ["COMMENTED", 3]]}]}}});
        assert_eq!(
            rows(&grouped),
            [("APPROVED".to_owned(), 12.0), ("COMMENTED".to_owned(), 3.0)]
        );
        assert_eq!(scalar(&json!({})), 0.0);
    }

    #[test]
    fn the_pr_filter_names_the_dashboards_repos_and_drops_quotes() {
        let f = pr_filter(&["o/a".into(), "o/b'".into()], " AND pr.merged = true");
        assert_eq!(
            f,
            "service.name = 'github-pr-events' AND pr.number EXISTS AND repo.full_name in ('o/a','o/b') AND pr.merged = true"
        );
        assert!(!pr_filter(&[], "").contains("repo.full_name"));
    }
}

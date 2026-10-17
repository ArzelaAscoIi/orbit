//! Releases to prod: the GitHub Actions workflows settings.json `releases`
//! names (repo + workflow), each with its latest run, read through `gh`.
//! A red one gets Fix it: a Claude session on the failed run.

use anyhow::Result;
use serde_json::Value;

/// One watched workflow (settings.json `releases`).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Release {
    /// `owner/repo`.
    pub repo: String,
    /// The workflow file (`deploy-prod.yaml`) or its name.
    pub workflow: String,
    /// What the Alerts view calls it; the workflow when empty.
    pub name: String,
}

impl Release {
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            &self.workflow
        } else {
            &self.name
        }
    }
}

/// A workflow run as `gh run list --json` gives it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Run {
    pub id: u64,
    pub title: String,
    /// `queued`, `in_progress`, `completed`.
    pub status: String,
    /// When completed: `success`, `failure`, `cancelled`, `timed_out`, …
    pub conclusion: String,
    pub url: String,
    pub branch: String,
    pub created_at: String,
    /// When it last changed: for a completed run, when it finished.
    pub updated_at: String,
    pub sha: String,
    /// Who changed what a red run ran: the PR's author (whoever merged it,
    /// for a bot's PR), else the commit's. Only looked up for a red run.
    pub responsible: Option<String>,
    /// The PR that brought the change in.
    pub pr: Option<u64>,
}

impl Run {
    /// Red: the run failed (a failed job, a timeout, a broken start).
    pub fn failed(&self) -> bool {
        matches!(
            self.conclusion.as_str(),
            "failure" | "timed_out" | "startup_failure"
        )
    }

    pub fn running(&self) -> bool {
        self.status != "completed"
    }
}

/// The latest run of `release` that did something: a skipped run (its
/// conditions did not hold) says nothing about the release.
pub fn latest(release: &Release) -> Result<Option<Run>> {
    let out = crate::claude::gh(&[
        "run",
        "list",
        "--repo",
        &release.repo,
        "--workflow",
        &release.workflow,
        "--limit",
        "10",
        "--json",
        "databaseId,displayTitle,status,conclusion,url,headBranch,createdAt,updatedAt,headSha",
    ])?;
    let mut run = runs(&out).into_iter().find(|r| r.conclusion != "skipped");
    if let Some(run) = run.as_mut().filter(|r| r.failed()) {
        // ponytail: best effort, a red run without a name still shows.
        (run.responsible, run.pr) = responsible(&release.repo, &run.sha).unwrap_or_default();
    }
    Ok(run)
}

/// Whose change commit `sha` is: its PR's author, or for a bot's PR the
/// person who merged it; with no PR, the commit's author.
fn responsible(repo: &str, sha: &str) -> Result<(Option<String>, Option<u64>)> {
    let pulls = crate::claude::gh(&["api", &format!("repos/{repo}/commits/{sha}/pulls")])?;
    let pulls: Value = serde_json::from_str(&pulls).unwrap_or_default();
    let pr = &pulls[0];
    if let Some(number) = pr["number"].as_u64() {
        let author = pr["user"]["login"].as_str().unwrap_or_default();
        if pr["user"]["type"] != "Bot" {
            return Ok((Some(author.to_owned()), Some(number)));
        }
        let full = crate::claude::gh(&["api", &format!("repos/{repo}/pulls/{number}")])?;
        let full: Value = serde_json::from_str(&full).unwrap_or_default();
        let merger = full["merged_by"]["login"].as_str().map(str::to_owned);
        return Ok((merger.or(Some(author.to_owned())), Some(number)));
    }
    let commit = crate::claude::gh(&["api", &format!("repos/{repo}/commits/{sha}")])?;
    let commit: Value = serde_json::from_str(&commit).unwrap_or_default();
    Ok((commit["author"]["login"].as_str().map(str::to_owned), None))
}

impl Run {
    /// Who to talk to about it, as the Alerts view and notifications say it.
    pub fn by(&self) -> Option<String> {
        let who = self.responsible.as_deref()?;
        Some(match self.pr {
            Some(n) => format!("@{who} (#{n})"),
            None => format!("@{who}"),
        })
    }
}

fn runs(json: &str) -> Vec<Run> {
    let body: Value = serde_json::from_str(json).unwrap_or_default();
    let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    body.as_array()
        .into_iter()
        .flatten()
        .map(|r| Run {
            id: r["databaseId"].as_u64().unwrap_or_default(),
            title: text(&r["displayTitle"]),
            status: text(&r["status"]),
            conclusion: text(&r["conclusion"]),
            url: text(&r["url"]),
            branch: text(&r["headBranch"]),
            created_at: text(&r["createdAt"]),
            updated_at: text(&r["updatedAt"]),
            sha: text(&r["headSha"]),
            responsible: None,
            pr: None,
        })
        .filter(|r| r.id != 0)
        .collect()
}

/// The first message of a Fix it session: the fix-deploy skill (the
/// profile's `fixDeploy`) on the failed run, with what the app knows of it.
pub fn brief(release: &Release, run: &Run) -> String {
    format!(
        "/{skill} {url}\n\n\
         The deploy {name} failed: {title}\n\
         Workflow: {workflow} in {repo} · run {id} on {branch}\n\
         Responsible: {by}\n\
         Investigate why it failed, find whose change broke it, then open a fix PR and ping them \
         on it, or open a revert PR assigned to them. Do not merge, and do not touch an \
         environment by hand, without asking me first.",
        skill = crate::profile::get().skill("fixDeploy"),
        name = release.label(),
        title = run.title,
        workflow = release.workflow,
        repo = release.repo,
        id = run.id,
        branch = run.branch,
        url = run.url,
        by = run.by().unwrap_or_else(|| "unknown, find it".to_owned()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_without_a_pr_names_its_commit_author() {
        let run = Run {
            responsible: Some("bo".into()),
            pr: None,
            ..Run::default()
        };
        assert_eq!(run.by().as_deref(), Some("@bo"));
        let release = Release {
            repo: "o/r".into(),
            workflow: "deploy.yaml".into(),
            name: "API prod".into(),
        };
        assert!(brief(&release, &run).contains("Responsible: @bo\n"));
        assert!(brief(&release, &Run::default()).contains("Responsible: unknown, find it"));
    }

    #[test]
    fn a_cancelled_run_is_neither_red_nor_running() {
        let run = |status: &str, conclusion: &str| Run {
            status: status.into(),
            conclusion: conclusion.into(),
            ..Run::default()
        };
        let cancelled = run("completed", "cancelled");
        assert!(!cancelled.failed() && !cancelled.running());
        assert!(run("completed", "failure").failed());
        assert!(run("completed", "timed_out").failed());
        assert!(run("completed", "startup_failure").failed());
        assert!(run("in_progress", "").running());
    }

    #[test]
    fn the_latest_run_that_did_something_counts() {
        let json = r#"[
            {"databaseId":3,"displayTitle":"skipped one","status":"completed","conclusion":"skipped"},
            {"databaseId":2,"displayTitle":"Deploy api","status":"completed","conclusion":"failure",
             "url":"https://github.com/o/r/actions/runs/2","headBranch":"main","createdAt":"2026-10-01T10:00:00Z"},
            {"databaseId":1,"status":"completed","conclusion":"success"}]"#;
        let all = runs(json);
        let latest = all.into_iter().find(|r| r.conclusion != "skipped").unwrap();
        assert_eq!(latest.id, 2);
        assert!(latest.failed() && !latest.running());
        let mut latest = latest;
        assert_eq!(latest.by(), None);
        (latest.responsible, latest.pr) = (Some("ana".into()), Some(7));
        assert_eq!(latest.by().as_deref(), Some("@ana (#7)"));
        let release = Release {
            repo: "o/r".into(),
            workflow: "deploy.yaml".into(),
            name: String::new(),
        };
        let brief = brief(&release, &latest);
        assert!(brief.starts_with("/fix-deploy https://github.com/o/r/actions/runs/2"));
        assert!(brief.contains("The deploy deploy.yaml failed: Deploy api"));
        assert!(brief.contains("Responsible: @ana (#7)"));
    }
}

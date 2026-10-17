//! Your own week in Orbit, from what this Mac already keeps: the activity
//! log (activity.rs: every action, its gate) and the sessions' status logs
//! (claude/status.rs: turns, questions, PRs). Nothing leaves the machine;
//! the Stats view puts it beside the team's numbers from SigNoz (signoz.rs).

use std::collections::BTreeMap;
use std::path::Path;

/// One window of the user's activity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Local {
    pub actions: usize,
    /// Days with at least one action.
    pub days: usize,
    /// Share of actions taken by key rather than mouse.
    pub by_key: f64,
    /// Actions per gate (activity::Gate): look, app, do, queue, yours.
    pub gates: BTreeMap<String, usize>,
    pub approvals: usize,
    pub merges: usize,
    pub skills_run: usize,
    pub sessions_opened: usize,
    /// What the sessions did, from their status logs.
    pub turns: usize,
    pub questions: usize,
    pub prs_opened: usize,
    /// The most used decisions (gate do, queue or yours), most first.
    pub top: Vec<(String, usize)>,
}

/// The week ending `now_ms` from the activity log's lines and the status
/// logs' lines (`<unix seconds>\t<state>\t<text>`).
pub fn week(activity: &str, status: &[String], now_ms: u128) -> Local {
    let from = now_ms.saturating_sub(7 * 24 * 3600 * 1000);
    let mut local = Local::default();
    let mut days = std::collections::BTreeSet::new();
    let mut keys = 0usize;
    let mut counted: BTreeMap<String, usize> = BTreeMap::new();
    for line in activity.lines() {
        let Ok(e) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let t = e["t"].as_u64().unwrap_or(0) as u128;
        let event = e["event"].as_str().unwrap_or_default();
        if t < from || t > now_ms || event == "error" || event == "start" {
            continue;
        }
        local.actions += 1;
        days.insert(t / 86_400_000);
        if e["via"] == "key" {
            keys += 1;
        }
        let gate = e["gate"].as_str().map(str::to_owned).unwrap_or_else(|| {
            crate::activity::gate(event)
                .map_or("other".into(), |g| crate::activity::name(&g).to_lowercase())
        });
        *local.gates.entry(gate.clone()).or_default() += 1;
        match event {
            "Approve" => local.approvals += 1,
            "Merge" | "AutoMerge" | "LandBatch" => local.merges += 1,
            "RunSkill" => local.skills_run += 1,
            "Claude" | "NewSession" | "OpenSession" | "FreshSession" => local.sessions_opened += 1,
            _ => {}
        }
        if matches!(gate.as_str(), "do" | "queue" | "yours") {
            *counted.entry(event.to_owned()).or_default() += 1;
        }
    }
    local.days = days.len();
    local.by_key = if local.actions == 0 {
        0.0
    } else {
        keys as f64 / local.actions as f64
    };
    let mut top: Vec<(String, usize)> = counted.into_iter().collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    top.truncate(6);
    local.top = top;
    let from_s = (from / 1000) as u64;
    for line in status {
        let mut parts = line.splitn(3, '\t');
        let (Some(at), Some(state)) = (parts.next(), parts.next()) else {
            continue;
        };
        if at.parse::<u64>().is_ok_and(|at| at >= from_s) {
            match state {
                "turn" => local.turns += 1,
                "needs-decision" => local.questions += 1,
                "pr" => local.prs_opened += 1,
                _ => {}
            }
        }
    }
    local
}

/// The week from the data folder: activity.jsonl and every status log.
pub fn read(dir: &Path) -> Local {
    let activity = std::fs::read_to_string(crate::activity::path(dir)).unwrap_or_default();
    let status: Vec<String> = std::fs::read_dir(dir.join("status"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .flat_map(|t| t.lines().map(str::to_owned).collect::<Vec<_>>())
        .collect();
    week(&activity, &status, crate::activity::now_ms())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_week_counts_decisions_days_and_what_sessions_did() {
        let day = 86_400_000u128;
        let now = 30 * day + day / 2;
        let line = |t: u128, event: &str, via: &str| {
            crate::activity::line(t, event, "", "ForYou", "Walkthrough", via, None)
        };
        let activity = [
            line(now - 1000, "Approve", "click"),
            line(now - 2000, "Merge", "click"),
            line(now - day - 5, "RunSkill", "key"),
            line(now - 2 * day, "GoWindow", "key"),
            line(now - 9 * day, "Approve", "click"), // older than the week
            "not json".to_owned(),
        ]
        .join("\n");
        let s = (now / 1000) as u64;
        let status = vec![
            format!("{}\tturn\tdone", s - 10),
            format!("{}\tpr\thttps://github.com/o/r/pull/1", s - 10),
            format!("{}\tneeds-decision\tSquash?", s - 10),
            format!("{}\tturn\told", s - 8 * 86_400),
        ];
        let w = week(&activity, &status, now);
        assert_eq!(w.actions, 4);
        assert_eq!(w.days, 3);
        assert_eq!((w.approvals, w.merges, w.skills_run), (1, 1, 1));
        assert_eq!(w.gates.get("yours"), Some(&2));
        assert_eq!(w.gates.get("look"), Some(&1));
        assert_eq!((w.turns, w.prs_opened, w.questions), (1, 1, 1));
        assert!((w.by_key - 0.5).abs() < 1e-9);
        assert!(
            w.top.iter().all(|(e, _)| e != "GoWindow"),
            "looking is not a decision"
        );
    }
}

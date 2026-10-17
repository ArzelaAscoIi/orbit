//! The Claude helpers that make the interface say less: the one-line brief,
//! short names, customers and digests. Each runs only when its input
//! changed, caches the result on disk, and shares per-ticket results with
//! teammates through one Linear attachment per ticket.

use std::collections::BTreeMap;

use super::App;
use crate::anthropic;
use crate::backend::Command;
use crate::secrets::{self, Secret};
use crate::store::{self, text_hash};
use crate::story::{self, Card};

const REJECTED: &str = "Anthropic did not accept that key. Paste it again.";

/// What Haiku is asked to name a window by, from its panes' parts: none
/// for a lone ticket (its id and `tab:` name do), an empty window or a
/// lone untitled shell.
fn to_name(parts: &[String], lone_ticket: bool) -> Option<String> {
    if parts.is_empty() || lone_ticket || parts == ["shell"] {
        return None;
    }
    Some(parts.join(" | "))
}

impl App {
    /// Starts using a pasted key: stores it and runs every helper.
    pub(super) fn enable_anthropic(&mut self, key: String) {
        if let Err(error) = secrets::set(Secret::Anthropic, &key) {
            self.brief_error = Some(format!("could not save the key: {error}"));
        }
        self.anthropic_key = Some(key);

        self.brief_error = None;
        self.brief_asked = None;
        self.shorten_tried.clear();
        self.customers_tried.clear();
        self.run_helpers();
    }

    pub(super) fn forget_anthropic_key(&mut self, why: Option<String>) {
        if let Err(error) = secrets::clear(Secret::Anthropic) {
            self.error = Some(format!("could not remove the Anthropic key: {error}"));
        }
        self.anthropic_key = None;
        self.brief = None;
        self.brief_for = None;
        self.brief_error = why;
    }

    /// After a board load: whatever is stale gets asked for again.
    pub(super) fn run_helpers(&mut self) {
        self.maybe_brief();
        self.maybe_shorten();
        self.maybe_customers();
    }

    /// Anthropic's answer to a helper failed; a rejected key is dropped.
    fn helper_failed(&mut self, error: String) {
        if error.contains("401") {
            self.forget_anthropic_key(Some(REJECTED.into()));
        } else {
            self.brief_error = Some(error);
        }
    }

    /// A ticket's title as the list shows it: Claude's short name when the
    /// title is long and one exists.
    pub fn short_title<'a>(&'a self, card: &'a Card) -> &'a str {
        self.short_for(&card.issue.id, &card.issue.title)
    }

    pub fn short_story<'a>(&'a self, key: &str, title: &'a str) -> &'a str {
        self.short_for(&format!("story:{key}"), title)
    }

    /// A ticket's window tab name from Haiku, when it gave one for this title.
    pub fn tab_title(&self, card: &Card) -> Option<&str> {
        self.short
            .get(&format!("tab:{}", card.issue.id))
            .filter(|(hash, _)| *hash == text_hash(&card.issue.title))
            .map(|(_, short)| short.as_str())
    }

    /// What Haiku names window `i` from: each pane's ticket, session or
    /// shell, one per part. None for a window with a fixed title, a lone
    /// ticket (its id and `tab:` name say it) or a lone untitled shell.
    pub fn window_desc(&self, i: usize) -> Option<String> {
        use super::panes::Surface;
        let panes = if i == self.window {
            &self.panes
        } else {
            self.windows.get(i)?
        };
        if panes.title.is_some() {
            return None;
        }
        let mut parts = Vec::new();
        let mut carded = false;
        for id in panes.ids() {
            let view = match panes.surface(id) {
                Some(Surface::Ticket(_)) if i == self.window && id == panes.active => {
                    self.ticket_view()
                }
                Some(Surface::Ticket(view)) => view.clone(),
                _ => {
                    parts.push(
                        self.shell_titles
                            .get(&id)
                            .map_or("shell", String::as_str)
                            .to_owned(),
                    );
                    continue;
                }
            };
            let card = view.selected.as_deref().and_then(|id| self.card(id));
            let saved = view
                .session
                .as_deref()
                .and_then(|k| self.claude.saved.get(k));
            match (card, saved) {
                (Some(card), _) => {
                    carded = true;
                    parts.push(format!("{} {}", card.issue.identifier, card.issue.title));
                }
                (None, Some(s)) => parts.push(format!("session {}", s.title)),
                _ => {}
            }
        }
        to_name(&parts, panes.single() && carded)
    }

    /// Window `i`'s name in the strip: its fixed title, else Haiku's name
    /// for what it holds, when there is one for exactly that.
    pub fn window_title(&self, i: usize) -> Option<String> {
        let panes = if i == self.window {
            &self.panes
        } else {
            self.windows.get(i)?
        };
        if let Some(title) = &panes.title {
            return Some(title.clone());
        }
        let desc = self.window_desc(i)?;
        let hash = text_hash(&desc);
        self.short
            .get(&format!("win:{hash:x}"))
            .filter(|(h, _)| *h == hash)
            .map(|(_, short)| short.clone())
    }

    /// The tickets every window shows, the current one's active pane included.
    pub(super) fn window_tickets(&self) -> Vec<String> {
        let mut ids = Vec::new();
        for (i, stashed) in self.windows.iter().enumerate() {
            let panes = if i == self.window {
                &self.panes
            } else {
                stashed
            };
            for pane in panes.tickets() {
                let selected = match panes.surface(pane) {
                    _ if i == self.window && pane == panes.active => self.selected.clone(),
                    Some(super::panes::Surface::Ticket(view)) => view.selected.clone(),
                    _ => None,
                };
                ids.extend(selected);
            }
        }
        ids
    }

    fn short_for<'a>(&'a self, key: &str, text: &'a str) -> &'a str {
        match self.short.get(key) {
            Some((hash, short)) if *hash == text_hash(text) => short,
            _ => text,
        }
    }

    pub fn digest(&self, key: &str) -> Option<&str> {
        self.digests.get(key).map(|(_, line)| line.as_str())
    }

    /// Every customer with tickets on the board: name, and how many of
    /// its tickets need you, are moving, and shipped. Most asks first.
    pub fn customer_list(&self) -> Vec<(String, usize, usize, usize)> {
        let mut by_name: BTreeMap<&str, (usize, usize, usize)> = BTreeMap::new();
        for card in &self.cards {
            for name in &card.customers {
                let entry = by_name.entry(name).or_default();
                match story::bucket(card) {
                    "Needs you" => entry.0 += 1,
                    "Moving" => entry.1 += 1,
                    _ => entry.2 += 1,
                }
            }
        }
        // The user's own company is not a customer (story::is_ours).
        let mut list: Vec<(String, usize, usize, usize)> = by_name
            .into_iter()
            .filter(|(name, _)| !story::is_ours(name))
            .map(|(name, (asks, moving, shipped))| (name.to_owned(), asks, moving, shipped))
            .collect();
        list.sort_by(|a, b| (b.1, b.2 + b.3).cmp(&(a.1, a.2 + a.3)).then(a.0.cmp(&b.0)));
        list
    }

    /// Asks Claude for a new brief when the board changed since the last
    /// one, at most every five minutes.
    fn maybe_brief(&mut self) {
        let Some(key) = self.anthropic_key.clone() else {
            return;
        };
        let fingerprint = anthropic::fingerprint(&self.cards);
        let recent = self
            .brief_asked
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(300));
        if self.brief_pending
            || self.brief_for == Some(fingerprint)
            || recent
            || self.cards.is_empty()
        {
            return;
        }
        self.brief_pending = true;
        self.brief_asked = Some(std::time::Instant::now());
        self.send(Command::Brief {
            fingerprint,
            key,
            board: anthropic::board(&self.cards),
        });
    }

    pub(super) fn on_brief(&mut self, fingerprint: u64, result: Result<String, String>) {
        self.brief_pending = false;
        match result {
            Ok(line) => {
                self.brief = Some(line);
                self.brief_for = Some(fingerprint);
                self.brief_error = None;
            }
            Err(error) => self.helper_failed(error),
        }
    }

    /// Asks Claude for short names of long titles it has not named yet.
    pub(super) fn maybe_shorten(&mut self) {
        let Some(key) = self.anthropic_key.clone() else {
            return;
        };
        if self.shorten_pending {
            return;
        }
        let needs = |k: &str, text: &str, limit: usize| {
            text.chars().count() > limit
                && !self.shorten_tried.contains(k)
                && self
                    .short
                    .get(k)
                    .is_none_or(|(hash, _)| *hash != text_hash(text))
        };
        let mut items: Vec<(String, String)> = Vec::new();
        for card in self.cards.iter().filter(|c| c.stage != story::Stage::Done) {
            if needs(&card.issue.id, &card.issue.title, 45) {
                items.push((card.issue.id.clone(), card.issue.title.clone()));
            }
            let story_key = format!("story:{}", card.story_key);
            if card.story_key != "loose"
                && needs(&story_key, &card.story, 22)
                && !items.iter().any(|(k, _)| *k == story_key)
            {
                items.push((story_key, card.story.clone()));
            }
        }
        // A window of several panes or a session: Haiku names it.
        for i in 0..self.windows.len() {
            if let Some(desc) = self.window_desc(i) {
                let key = format!("win:{:x}", text_hash(&desc));
                if needs(&key, &desc, 0) && !items.iter().any(|(k, _)| *k == key) {
                    items.insert(0, (key, desc));
                }
            }
        }
        // Every ticket open in a window gets a tab name, however short its title.
        for id in self.window_tickets() {
            let tab = format!("tab:{id}");
            if let Some(card) = self.card(&id)
                && needs(&tab, &card.issue.title, 0)
                && !items.iter().any(|(k, _)| *k == tab)
            {
                items.insert(0, (tab, card.issue.title.clone()));
            }
        }
        items.truncate(60);
        if items.is_empty() {
            return;
        }
        self.shorten_tried
            .extend(items.iter().map(|(k, _)| k.clone()));
        self.shorten_pending = true;
        self.send(Command::Shorten { key, items });
    }

    pub(super) fn on_shortened(
        &mut self,
        items: Vec<(String, String)>,
        result: Result<Vec<(String, String)>, String>,
    ) {
        self.shorten_pending = false;
        let names = match result {
            Ok(names) => names,
            Err(error) => return self.helper_failed(error),
        };
        for (key, short) in names {
            if let Some((_, text)) = items.iter().find(|(k, _)| *k == key) {
                self.short.insert(key, (text_hash(text), short));
            }
        }
        self.persist("short-names.json", &self.short.clone());
        let ids: Vec<String> = items
            .into_iter()
            .map(|(k, _)| k)
            .filter(|k| !["story:", "tab:", "win:"].iter().any(|p| k.starts_with(p)))
            .collect();
        self.write_notes(&ids);
        // Anything still long gets its turn in the next batch.
        self.maybe_shorten();
    }

    /// Asks Claude which customers the tickets without a Linear customer
    /// request are for.
    fn maybe_customers(&mut self) {
        let Some(key) = self.anthropic_key.clone() else {
            return;
        };
        if self.customers_pending {
            return;
        }
        let mut items: Vec<(String, String)> = Vec::new();
        for card in &self.cards {
            let linked = card.issue.needs.nodes.iter().any(|n| n.customer.is_some());
            if linked {
                continue;
            }
            let text = story::customer_text(card);
            let known = self
                .extracted
                .get(&card.issue.id)
                .is_some_and(|(hash, _)| *hash == text_hash(&text));
            if !known && !self.customers_tried.contains(&card.issue.id) {
                items.push((card.issue.id.clone(), text));
            }
        }
        items.truncate(50);
        if items.is_empty() {
            self.maybe_digests();
            return;
        }
        self.customers_tried
            .extend(items.iter().map(|(k, _)| k.clone()));
        self.customers_pending = true;
        self.send(Command::Customers { key, items });
    }

    pub(super) fn on_customers(
        &mut self,
        items: Vec<(String, String)>,
        result: Result<Vec<(String, Vec<String>)>, String>,
    ) {
        self.customers_pending = false;
        let found = match result {
            Ok(found) => found,
            Err(error) => return self.helper_failed(error),
        };
        for (id, names) in found {
            if let Some((_, text)) = items.iter().find(|(k, _)| *k == id) {
                self.extracted.insert(id, (text_hash(text), names));
            }
        }
        self.persist("customers.json", &self.extracted.clone());
        let ids: Vec<String> = items.into_iter().map(|(k, _)| k).collect();
        self.write_notes(&ids);
        self.rebuild();
        self.maybe_customers();
    }

    /// Asks Claude for one line per customer and story whose tickets
    /// changed since the last line was written.
    fn maybe_digests(&mut self) {
        let Some(key) = self.anthropic_key.clone() else {
            return;
        };
        if self.digests_pending {
            return;
        }
        let row = |c: &Card| {
            let title: String = c.issue.title.chars().take(70).collect();
            format!(
                "{} | {} | {} | {}",
                c.issue.identifier,
                c.stage.label(),
                c.ask.map_or("-", |a| a.label()),
                title
            )
        };
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for card in &self.cards {
            for name in &card.customers {
                groups
                    .entry(format!("customer:{name}"))
                    .or_default()
                    .push(row(card));
            }
            if card.story_key != "loose" && card.stage != story::Stage::Done {
                groups
                    .entry(format!("story:{}", card.story_key))
                    .or_default()
                    .push(row(card));
            }
        }
        let mut items: Vec<(String, String)> = groups
            .into_iter()
            .filter(|(k, rows)| k.starts_with("customer:") || rows.len() >= 2)
            .map(|(k, rows)| (k, rows.join("\n")))
            .filter(|(k, text)| {
                self.digests
                    .get(k)
                    .is_none_or(|(hash, _)| *hash != text_hash(text))
            })
            .collect();
        items.truncate(40);
        if items.is_empty() {
            return;
        }
        self.digests_pending = true;
        self.send(Command::Digests { key, items });
    }

    pub(super) fn on_digests(
        &mut self,
        items: Vec<(String, String)>,
        result: Result<Vec<(String, String)>, String>,
    ) {
        self.digests_pending = false;
        let lines = match result {
            Ok(lines) => lines,
            Err(error) => return self.helper_failed(error),
        };
        for (key, line) in lines {
            if let Some((_, text)) = items.iter().find(|(k, _)| *k == key) {
                self.digests.insert(key, (text_hash(text), line));
            }
        }
        self.persist("digests.json", &self.digests.clone());
    }

    fn persist<T: serde::Serialize>(&mut self, name: &str, value: &T) {
        if let Err(error) = store::save(name, value) {
            self.error = Some(format!("could not save {name}: {error}"));
        }
    }

    /// Takes the short names and customers stored in each ticket's Control
    /// Room note, so work a teammate's app already did is not done again.
    pub(super) fn read_notes(&mut self) {
        // Hashes are hex strings: JSON numbers past 2^53 lose precision.
        let hash_of =
            |v: &serde_json::Value| v.as_str().and_then(|h| u64::from_str_radix(h, 16).ok());
        for issue in &self.server {
            let Some(note) = issue
                .attachments
                .nodes
                .iter()
                .find(|a| a.url == crate::note_url())
            else {
                continue;
            };
            let meta = &note.metadata;
            if let (Some(hash), Some(text)) = (
                hash_of(&meta["short"]["hash"]),
                meta["short"]["text"].as_str(),
            ) && hash == text_hash(&issue.title)
            {
                self.short.insert(issue.id.clone(), (hash, text.to_owned()));
            }
            if let (Some(hash), Some(names)) = (
                hash_of(&meta["customers"]["hash"]),
                meta["customers"]["names"].as_array(),
            ) {
                let names = names
                    .iter()
                    .filter_map(|n| n.as_str().map(str::to_owned))
                    .collect();
                self.extracted.insert(issue.id.clone(), (hash, names));
            }
        }
    }

    /// Writes the notes whose content changed; unchanged ones are skipped,
    /// so Linear sees a write only when there is something new.
    fn write_notes(&mut self, issue_ids: &[String]) {
        let mut notes = Vec::new();
        for id in issue_ids {
            let Some(issue) = self.server.iter().find(|i| &i.id == id) else {
                continue;
            };
            let mut meta = serde_json::json!({ "v": 1 });
            let mut subtitle = String::new();
            if let Some((hash, text)) = self.short.get(id) {
                meta["short"] = serde_json::json!({ "hash": format!("{hash:x}"), "text": text });
                subtitle = text.clone();
            }
            if let Some((hash, names)) = self.extracted.get(id) {
                meta["customers"] =
                    serde_json::json!({ "hash": format!("{hash:x}"), "names": names });
                if !names.is_empty() {
                    subtitle = format!("{subtitle} · {}", names.join(", "))
                        .trim_start_matches(" · ")
                        .to_owned();
                }
            }
            let current = issue
                .attachments
                .nodes
                .iter()
                .find(|a| a.url == crate::note_url())
                .map(|a| &a.metadata);
            if current != Some(&meta) {
                notes.push((id.clone(), subtitle, meta));
            }
        }
        if !notes.is_empty() {
            self.send(Command::SaveNotes(notes));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haiku_names_windows_of_several_panes_and_sessions_only() {
        let parts = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(to_name(&parts(&["ABC-1 Fix login"]), true), None);
        assert_eq!(to_name(&parts(&["shell"]), false), None);
        assert_eq!(
            to_name(&parts(&["session Clean up IRSA"]), false).as_deref(),
            Some("session Clean up IRSA")
        );
        assert_eq!(
            to_name(&parts(&["ABC-1 Fix login", "k9s · prod"]), false).as_deref(),
            Some("ABC-1 Fix login | k9s · prod")
        );
    }
}

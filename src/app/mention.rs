//! `@` mentions in comments: suggests the project's members while typing and turns the picked
//! names into `@<id>` when the comment is sent.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::api::Member;

use super::message::Message;
use super::{App, Popup, TextTarget};

/// Most suggestions shown at once.
const MAX_SUGGESTIONS: usize = 6;
/// A longer query after `@` is no longer taken for a name.
const MAX_QUERY_CHARS: usize = 40;

#[derive(Default)]
pub struct Mentions {
    /// Active members of the project; `None` until loaded.
    pub members: Option<Vec<Member>>,
    /// The members were asked for and have not failed since.
    requested: bool,
    /// People picked from the suggestions, resolved first when a comment is sent, so two people
    /// with the same name are told apart.
    picked: Vec<Member>,
    /// Suggestions for the `@` before the cursor.
    pub picker: Option<MentionPicker>,
    /// Byte offset of the `@` whose suggestions esc closed.
    dismissed: Option<usize>,
}

pub struct MentionPicker {
    /// Byte offset of the `@` in the text.
    start: usize,
    pub matches: Vec<Member>,
    pub selected: usize,
}

impl Mentions {
    /// Forgets the members of the previous project.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// `text` with the mentioned names written as `@<id>`. A name matches a picked person first,
    /// then a member whose name or email is unique among the members.
    pub fn resolve(&self, text: &str) -> String {
        let members = self.members.as_deref().unwrap_or_default();
        let unique = |key: fn(&Member) -> &str, member: &&Member| {
            members
                .iter()
                .filter(|other| key(other).eq_ignore_ascii_case(key(member)))
                .count()
                == 1
        };
        let mut candidates: Vec<(&str, &str)> = Vec::new();
        for member in &self.picked {
            candidates.push((&member.display_name, &member.id));
        }
        for member in members {
            if unique(|m| &m.display_name, &member) {
                candidates.push((&member.display_name, &member.id));
            }
            if unique(|m| &m.unique_name, &member) {
                candidates.push((&member.unique_name, &member.id));
            }
        }

        let mut resolved = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find('@') {
            let (before, after) = rest.split_at(at);
            resolved.push_str(before);
            let after = &after[1..];
            let starts_word = resolved.chars().next_back().is_none_or(|c| !is_word(c));
            let name = starts_word
                .then(|| longest_name_at(after, &candidates))
                .flatten();
            match name {
                Some((len, id)) => {
                    resolved.push_str(&format!("@<{id}>"));
                    rest = &after[len..];
                }
                None => {
                    resolved.push('@');
                    rest = after;
                }
            }
        }
        resolved.push_str(rest);
        resolved
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.' || c == '-'
}

/// Length in `text` and id of the longest candidate name `text` starts with, ending at a word
/// boundary.
fn longest_name_at<'a>(text: &str, candidates: &[(&str, &'a str)]) -> Option<(usize, &'a str)> {
    candidates
        .iter()
        .filter(|(name, _)| !name.is_empty())
        .filter(|(name, _)| {
            text.get(..name.len())
                .is_some_and(|start| start.to_lowercase() == name.to_lowercase())
                && text[name.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric() && c != '_')
        })
        .max_by_key(|(name, _)| name.len())
        .map(|(name, id)| (name.len(), *id))
}

/// Offset of the `@` and the query typed after it, when the cursor is in a mention being typed.
fn query_before_cursor(before: &str) -> Option<(usize, &str)> {
    let start = before.rfind('@')?;
    if before[..start].chars().next_back().is_some_and(is_word) {
        return None;
    }
    let query = &before[start + 1..];
    let ends = query.starts_with([' ', '<']) || query.contains('\n');
    (!ends && query.chars().count() <= MAX_QUERY_CHARS).then_some((start, query))
}

/// Members whose name or email matches `query`: names starting with it first, then names with a
/// word starting with it, then the rest.
fn suggestions(members: &[Member], query: &str) -> Vec<Member> {
    let query = query.to_lowercase();
    let rank = |member: &Member| {
        let name = member.display_name.to_lowercase();
        if name.starts_with(&query) {
            Some(0)
        } else if name
            .split([' ', '.', '-'])
            .any(|word| word.starts_with(&query))
        {
            Some(1)
        } else if name.contains(&query) || member.unique_name.to_lowercase().contains(&query) {
            Some(2)
        } else {
            None
        }
    };
    let mut ranked: Vec<_> = members
        .iter()
        .filter_map(|member| rank(member).map(|rank| (rank, member)))
        .collect();
    // Stable, so each rank keeps the members' order by name.
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, member)| member.clone())
        .collect()
}

impl TextTarget {
    fn takes_mentions(&self) -> bool {
        matches!(
            self,
            TextTarget::WorkItemComment(_) | TextTarget::PrThread { .. }
        )
    }
}

impl App {
    /// Moves through or picks a suggestion; true when the key was taken.
    pub(super) fn handle_mention_key(&mut self, key: KeyEvent) -> bool {
        let Some(picker) = &mut self.mentions.picker else {
            return false;
        };
        if picker.matches.is_empty() {
            return false;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let last = picker.matches.len() - 1;
        match key.code {
            KeyCode::Down | KeyCode::Tab if key.modifiers.is_empty() => {
                picker.selected = if picker.selected == last {
                    0
                } else {
                    picker.selected + 1
                };
            }
            KeyCode::Char('n') if ctrl => picker.selected = (picker.selected + 1).min(last),
            KeyCode::Up | KeyCode::BackTab => {
                picker.selected = picker.selected.checked_sub(1).unwrap_or(last);
            }
            KeyCode::Char('p') if ctrl => picker.selected = picker.selected.saturating_sub(1),
            KeyCode::Enter if key.modifiers.is_empty() => self.pick_mention(),
            KeyCode::Esc => {
                self.mentions.dismissed = Some(picker.start);
                self.mentions.picker = None;
            }
            _ => return false,
        }
        true
    }

    /// Replaces the typed `@query` with the highlighted person's name.
    fn pick_mention(&mut self) {
        let Some(picker) = self.mentions.picker.take() else {
            return;
        };
        let Some(member) = picker.matches.into_iter().nth(picker.selected) else {
            return;
        };
        if let Some(Popup::Text { text, .. }) = &mut self.popup {
            text.replace_before_cursor(picker.start, &format!("@{} ", member.display_name));
        }
        if !self.mentions.picked.contains(&member) {
            self.mentions.picked.push(member);
        }
    }

    /// Shows the suggestions for the `@` before the cursor of a comment, loading the members first.
    pub(super) fn update_mentions(&mut self) {
        let query = match &self.popup {
            Some(Popup::Text { target, text }) if target.takes_mentions() => {
                query_before_cursor(text.split().0).map(|(start, query)| (start, query.to_string()))
            }
            _ => None,
        };
        let Some((start, query)) = query else {
            self.mentions.picker = None;
            self.mentions.dismissed = None;
            return;
        };
        if self.mentions.dismissed == Some(start) {
            self.mentions.picker = None;
            return;
        }
        self.load_members();
        let matches = self
            .mentions
            .members
            .as_deref()
            .map(|members| suggestions(members, &query))
            .unwrap_or_default();
        let selected = match &self.mentions.picker {
            Some(picker) if picker.start == start => {
                picker.selected.min(matches.len().saturating_sub(1))
            }
            _ => 0,
        };
        self.mentions.picker = Some(MentionPicker {
            start,
            matches,
            selected,
        });
    }

    /// Asks for the project's members once.
    pub(super) fn load_members(&mut self) {
        if self.mentions.members.is_some() || self.mentions.requested {
            return;
        }
        let (Some(api), Some(config)) = (self.api(), self.config.as_ref()) else {
            return;
        };
        self.mentions.requested = true;
        let project = config.project.clone();
        self.spawn(async move {
            let result = api.project_members().await;
            Message::ProjectMembers { project, result }
        });
    }

    pub(super) fn handle_project_members(
        &mut self,
        project: String,
        result: anyhow::Result<Vec<Member>>,
    ) {
        if self.config.as_ref().is_none_or(|c| c.project != project) {
            return;
        }
        match result {
            Ok(members) => {
                self.mentions.members = Some(members);
                self.update_mentions();
            }
            Err(err) => {
                // Typing the next `@` asks again. Only someone waiting for suggestions sees the
                // error; the load at startup fails quietly.
                self.mentions.requested = false;
                let err = err.context("failed to load the project's members");
                if self.mentions.picker.take().is_some() {
                    self.report_error(err);
                } else {
                    tracing::warn!("{err:#}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: &str, name: &str, email: &str) -> Member {
        Member {
            id: id.into(),
            display_name: name.into(),
            unique_name: email.into(),
        }
    }

    fn members() -> Vec<Member> {
        vec![
            member("ann-id", "Ann Berg", "ann@contoso.com"),
            member("bo-id", "Bo Lind", "bo@contoso.com"),
            member("bo2-id", "Bo Lind", "bo.lind@contoso.com"),
            member("annika-id", "Annika Berg", "annika@contoso.com"),
        ]
    }

    #[test]
    fn query_is_the_text_after_an_at_that_starts_a_word() {
        assert_eq!(query_before_cursor("hi @an"), Some((3, "an")));
        assert_eq!(query_before_cursor("@"), Some((0, "")));
        assert_eq!(query_before_cursor("(@Ann B"), Some((1, "Ann B")));
        assert_eq!(query_before_cursor("mail ann@contoso"), None);
        assert_eq!(query_before_cursor("hi @ "), None);
        assert_eq!(query_before_cursor("@ann\nok"), None);
        assert_eq!(query_before_cursor("@<ann-id>"), None);
        assert_eq!(query_before_cursor("no at"), None);
    }

    #[test]
    fn suggestions_rank_name_starts_then_word_starts_then_the_rest() {
        let names = |query| {
            suggestions(&members(), query)
                .into_iter()
                .map(|m| m.display_name)
                .collect::<Vec<_>>()
        };

        assert_eq!(names("berg"), ["Ann Berg", "Annika Berg"]);
        assert_eq!(names("ann"), ["Ann Berg", "Annika Berg"]);
        assert_eq!(names("lind@"), ["Bo Lind"]);
        assert_eq!(names("nik"), ["Annika Berg"]);
        assert!(names("zed").is_empty());
        assert_eq!(names("").len(), 4);
    }

    #[test]
    fn resolve_writes_unique_names_and_emails_as_ids() {
        let mentions = Mentions {
            members: Some(members()),
            ..Mentions::default()
        };

        assert_eq!(
            mentions.resolve("@Ann Berg and @annika berg, ping @bo@contoso.com!"),
            "@<ann-id> and @<annika-id>, ping @<bo-id>!"
        );
        // Same name twice: only the email tells them apart.
        assert_eq!(mentions.resolve("@Bo Lind"), "@Bo Lind");
        assert_eq!(
            mentions.resolve("mail ann@contoso.com"),
            "mail ann@contoso.com"
        );
        assert_eq!(mentions.resolve("@Ann Bergman"), "@Ann Bergman");
        assert_eq!(mentions.resolve("@Ann Berg."), "@<ann-id>.");
        assert_eq!(mentions.resolve("@<ann-id> @"), "@<ann-id> @");
    }

    #[test]
    fn a_picked_person_wins_over_a_same_named_one() {
        let mentions = Mentions {
            members: Some(members()),
            picked: vec![member("bo2-id", "Bo Lind", "bo.lind@contoso.com")],
            ..Mentions::default()
        };

        assert_eq!(mentions.resolve("@Bo Lind: hi"), "@<bo2-id>: hi");
    }
}

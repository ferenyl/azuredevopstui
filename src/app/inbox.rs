use std::collections::HashSet;

use crate::notify;

use super::message::Message;
use super::{App, Detail};

impl App {
    /// Inbox entries that showed up or changed status after the first load and are not notified yet.
    /// Marks them as notified. Empty when notifications are off.
    pub(super) fn new_inbox_entries(&mut self) -> Vec<Detail> {
        if !self.config.as_ref().is_some_and(|c| c.notifications) {
            return Vec::new();
        }
        let Some(notified) = &mut self.notified else {
            return Vec::new();
        };
        self.data
            .inbox()
            .into_iter()
            .map(|entry| entry.detail)
            .filter(|detail| {
                let status = self.data.status(detail);
                notified.insert(detail.key(), status.clone()).as_ref() != Some(&status)
            })
            .collect()
    }

    /// Sends a desktop notification for each new inbox entry.
    pub(super) fn notify_new_inbox(&mut self) {
        let entries = self.new_inbox_entries();
        let (summary, body, url) = match entries.as_slice() {
            [] => return,
            [detail] => {
                let (summary, title) = match detail {
                    Detail::PullRequest(pr) => (
                        format!("Pull request !{}", pr.pull_request_id),
                        pr.title.clone(),
                    ),
                    Detail::WorkItem(item) => (
                        format!("Mentioned in #{}", item.id),
                        item.fields.title.clone(),
                    ),
                    Detail::Build(build) => (
                        format!("Build {} failed", build.build_number),
                        build.definition.name.clone(),
                    ),
                };
                let status = self.data.status(detail);
                let body = if status.is_empty() {
                    title
                } else {
                    format!("{title}\n{status}")
                };
                (summary, body, self.detail_url(detail))
            }
            details => (
                format!("{} new items in your inbox", details.len()),
                details
                    .iter()
                    .map(|detail| format!("{} {}", detail.key(), detail.title()))
                    .collect::<Vec<_>>()
                    .join("\n"),
                None,
            ),
        };
        self.spawn(async move { Message::Notified(notify::send(summary, body, url).await) });
    }

    /// After the first load, remembers the current inbox as seen and notified.
    pub(super) fn track_inbox(&mut self) {
        if self.data.inbox_seen.is_none() && self.last_updated.is_some() {
            let inbox = self.data.inbox();
            let seen: HashSet<String> = inbox.iter().map(|e| e.detail.key()).collect();
            self.notified = Some(
                inbox
                    .iter()
                    .map(|e| (e.detail.key(), self.data.status(&e.detail)))
                    .collect(),
            );
            self.data.inbox_seen = Some(seen);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use crate::api::Mention;
    use crate::app::message::Message;
    use crate::app::{App, Detail, Screen};
    use crate::test_support::{config, work_item};

    fn mention(id: u32) -> Mention {
        Mention {
            item: work_item(id, "Mentioned", "Bug", "Active"),
            date: "2026-10-01T10:00:00Z".into(),
        }
    }

    fn notifying_app(notifications: bool) -> App {
        let mut config = config();
        config.notifications = notifications;
        let mut app = App::new(Some(config), None);
        app.screen = Screen::Main;
        app
    }

    fn keys(details: &[Detail]) -> Vec<String> {
        details.iter().map(Detail::key).collect()
    }

    /// Loads the inbox with mentions and starts tracking it, like the second refresh.
    fn loaded(app: &mut App, ids: &[u32]) {
        app.handle_message(Message::Mentions(Ok(ids
            .iter()
            .map(|id| mention(*id))
            .collect())));
        app.last_updated = Some(Instant::now());
        app.track_inbox();
    }

    #[test]
    fn first_load_is_not_notified() {
        let mut app = notifying_app(true);

        app.handle_message(Message::Mentions(Ok(vec![mention(20)])));
        let first = app.new_inbox_entries();
        app.last_updated = Some(Instant::now());
        app.track_inbox();
        let after_tracking = app.new_inbox_entries();

        assert!(first.is_empty());
        assert!(after_tracking.is_empty());
    }

    #[test]
    fn new_inbox_entry_is_notified_once() {
        let mut app = notifying_app(true);
        loaded(&mut app, &[20]);

        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)])));
        let first = app.new_inbox_entries();
        let second = app.new_inbox_entries();

        assert_eq!(keys(&first), ["#21"]);
        assert!(second.is_empty());
    }

    #[test]
    fn notified_entry_is_not_notified_after_refresh() {
        let mut app = notifying_app(true);
        loaded(&mut app, &[20]);
        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)])));
        app.new_inbox_entries();

        app.track_inbox();
        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)])));
        let entries = app.new_inbox_entries();

        assert!(entries.is_empty());
    }

    #[test]
    fn returning_entry_is_not_notified_again() {
        let mut app = notifying_app(true);
        loaded(&mut app, &[20]);
        app.handle_message(Message::Mentions(Ok(vec![mention(21)])));
        app.new_inbox_entries();
        app.handle_message(Message::Mentions(Ok(Vec::new())));
        app.new_inbox_entries();

        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)])));
        let entries = app.new_inbox_entries();

        assert!(entries.is_empty());
    }

    #[test]
    fn nothing_is_notified_when_notifications_are_off() {
        let mut app = notifying_app(false);
        loaded(&mut app, &[20]);

        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)])));
        let entries = app.new_inbox_entries();

        assert!(entries.is_empty());
    }
}

use std::collections::HashSet;

use chrono::{Local, NaiveTime};

use super::message::Message;
use super::{App, Detail};

impl App {
    /// Inbox entries that showed up or changed status after the first load and are not notified yet.
    /// Marks them as notified. Empty when notifications are off, a kind is disabled or it is quiet
    /// hours; entries seen then still count as notified, so they do not burst when quiet hours end.
    pub(super) fn new_inbox_entries(&mut self) -> Vec<Detail> {
        self.new_inbox_entries_at(Local::now().time())
    }

    pub(super) fn new_inbox_entries_at(&mut self, now: NaiveTime) -> Vec<Detail> {
        let Some(config) = self.config.as_ref().filter(|c| c.notifications) else {
            return Vec::new();
        };
        let notify = config.notify.clone();
        let quiet = notify.quiet_hours.as_ref().is_some_and(|q| q.contains(now));
        let Some(notified) = &mut self.notified else {
            return Vec::new();
        };
        self.data
            .inbox()
            .iter()
            .map(|entry| &entry.detail)
            .filter(|detail| {
                let status = self.data.status(detail);
                let changed =
                    notified.insert(detail.key(), status.clone()).as_ref() != Some(&status);
                let enabled = match detail {
                    Detail::Build(_) => notify.builds,
                    Detail::WorkItem(_) => notify.mentions,
                    Detail::PullRequest(_) => notify.pull_requests,
                };
                changed && enabled && !quiet
            })
            .cloned()
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
        let notifier = self.notifier.clone();
        self.spawn(async move { Message::Notified(notifier.send(summary, body, url).await) });
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

    use chrono::NaiveTime;

    use crate::api::{Mention, Partial};
    use crate::app::message::Message;
    use crate::app::{App, Detail, Screen};
    use crate::config::QuietHours;
    use crate::test_support::{config, work_item};

    fn noon() -> NaiveTime {
        NaiveTime::from_hms_opt(12, 0, 0).unwrap()
    }

    fn set_notify(app: &mut App, edit: impl FnOnce(&mut crate::config::NotifyConfig)) {
        let mut config = (**app.config.as_ref().unwrap()).clone();
        edit(&mut config.notify);
        app.config = Some(std::sync::Arc::new(config));
    }

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
            .collect::<Vec<_>>()
            .into())));
        app.last_updated = Some(Instant::now());
        app.track_inbox();
    }

    #[test]
    fn partial_load_shows_a_notice() {
        let mut app = notifying_app(true);

        app.handle_message(Message::Mentions(Ok(Partial {
            items: vec![mention(20)],
            failed: 2,
        })));

        assert_eq!(app.notice(), Some("2 work items skipped without comments"));
    }

    #[test]
    fn partial_load_notice_is_singular_for_one() {
        let mut app = notifying_app(true);

        app.handle_message(Message::Mentions(Ok(Partial {
            items: Vec::new(),
            failed: 1,
        })));

        assert_eq!(app.notice(), Some("1 work item skipped without comments"));
    }

    #[test]
    fn first_load_is_not_notified() {
        let mut app = notifying_app(true);

        app.handle_message(Message::Mentions(Ok(vec![mention(20)].into())));
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

        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)].into())));
        let first = app.new_inbox_entries();
        let second = app.new_inbox_entries();

        assert_eq!(keys(&first), ["#21"]);
        assert!(second.is_empty());
    }

    #[test]
    fn notified_entry_is_not_notified_after_refresh() {
        let mut app = notifying_app(true);
        loaded(&mut app, &[20]);
        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)].into())));
        app.new_inbox_entries();

        app.track_inbox();
        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)].into())));
        let entries = app.new_inbox_entries();

        assert!(entries.is_empty());
    }

    #[test]
    fn returning_entry_is_not_notified_again() {
        let mut app = notifying_app(true);
        loaded(&mut app, &[20]);
        app.handle_message(Message::Mentions(Ok(vec![mention(21)].into())));
        app.new_inbox_entries();
        app.handle_message(Message::Mentions(Ok(Vec::new().into())));
        app.new_inbox_entries();

        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)].into())));
        let entries = app.new_inbox_entries();

        assert!(entries.is_empty());
    }

    #[test]
    fn nothing_is_notified_when_notifications_are_off() {
        let mut app = notifying_app(false);
        loaded(&mut app, &[20]);

        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)].into())));
        let entries = app.new_inbox_entries();

        assert!(entries.is_empty());
    }

    #[test]
    fn disabled_kind_is_not_notified() {
        let mut app = notifying_app(true);
        set_notify(&mut app, |n| n.mentions = false);
        loaded(&mut app, &[20]);

        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)].into())));
        let entries = app.new_inbox_entries_at(noon());

        assert!(entries.is_empty());
    }

    #[test]
    fn other_kinds_stay_enabled_when_one_is_disabled() {
        let mut app = notifying_app(true);
        set_notify(&mut app, |n| n.builds = false);
        loaded(&mut app, &[20]);

        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)].into())));
        let entries = app.new_inbox_entries_at(noon());

        assert_eq!(keys(&entries), ["#21"]);
    }

    #[test]
    fn quiet_hours_suppress_without_a_burst_afterwards() {
        let mut app = notifying_app(true);
        set_notify(&mut app, |n| {
            n.quiet_hours = Some(QuietHours {
                start: "22:00".into(),
                end: "07:00".into(),
            });
        });
        loaded(&mut app, &[20]);
        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)].into())));

        let quiet = app.new_inbox_entries_at(NaiveTime::from_hms_opt(23, 0, 0).unwrap());
        let after = app.new_inbox_entries_at(noon());

        assert!(quiet.is_empty());
        assert!(after.is_empty());
    }

    #[test]
    fn outside_quiet_hours_entries_are_notified() {
        let mut app = notifying_app(true);
        set_notify(&mut app, |n| {
            n.quiet_hours = Some(QuietHours {
                start: "22:00".into(),
                end: "07:00".into(),
            });
        });
        loaded(&mut app, &[20]);
        app.handle_message(Message::Mentions(Ok(vec![mention(20), mention(21)].into())));

        let entries = app.new_inbox_entries_at(noon());

        assert_eq!(keys(&entries), ["#21"]);
    }
}

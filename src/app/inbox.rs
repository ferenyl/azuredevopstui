use super::message::Message;
use super::{App, Detail};
use crate::notify;
use std::collections::HashSet;

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

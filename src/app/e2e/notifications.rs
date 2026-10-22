//! Notification controls: which inbox entries would be notified. The real `Notifier` is never called.

use std::sync::Arc;

use chrono::NaiveTime;

use super::Harness;
use super::flows::{mention_601, mention_601_comment};
use crate::config::{NotifyConfig, QuietHours};

fn at(hour: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(hour, 0, 0).unwrap()
}

/// Connected harness with notifications on whose second refresh adds the mention of work item 601.
async fn with_new_mention(notify: NotifyConfig) -> Harness {
    let mut harness = Harness::new().await;
    let mut config = (*harness.config()).clone();
    config.notifications = true;
    config.notify = notify;
    harness.app.config = Some(Arc::new(config));
    harness.connect().await;
    harness.app.refresh();
    harness.settle().await;
    assert!(harness.app.new_inbox_entries_at(at(12)).is_empty());
    mention_601(&harness, 1).await;
    mention_601_comment(&harness).await;
    harness.app.refresh();
    harness.settle().await;
    harness
}

fn keys(app: &mut crate::app::App, hour: u32) -> Vec<String> {
    app.new_inbox_entries_at(at(hour))
        .iter()
        .map(crate::app::Detail::key)
        .collect()
}

#[tokio::test]
async fn new_mention_is_notified_by_default() {
    let mut harness = with_new_mention(NotifyConfig::default()).await;

    assert_eq!(keys(&mut harness.app, 12), ["#601"]);
}

#[tokio::test]
async fn disabled_mentions_are_not_notified() {
    let mut harness = with_new_mention(NotifyConfig {
        mentions: false,
        ..NotifyConfig::default()
    })
    .await;

    assert!(keys(&mut harness.app, 12).is_empty());
}

#[tokio::test]
async fn quiet_hours_suppress_notifications_without_a_burst_afterwards() {
    let mut harness = with_new_mention(NotifyConfig {
        quiet_hours: Some(QuietHours {
            start: "22:00".into(),
            end: "07:00".into(),
        }),
        ..NotifyConfig::default()
    })
    .await;

    assert!(keys(&mut harness.app, 23).is_empty());
    assert!(keys(&mut harness.app, 12).is_empty());
}

use std::sync::Arc;
use std::time::Duration;

use notify_rust::{Notification, Timeout};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const OPEN_ACTION: &str = "open";
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_WAITERS: usize = 4;

/// Shows desktop notifications, bounding how many blocking threads wait for a click at once.
#[derive(Clone)]
pub struct Notifier {
    waiters: Arc<Semaphore>,
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new(MAX_WAITERS)
    }
}

impl Notifier {
    fn new(max_waiters: usize) -> Self {
        Self {
            waiters: Arc::new(Semaphore::new(max_waiters)),
        }
    }

    fn try_wait_slot(&self) -> Option<OwnedSemaphorePermit> {
        self.waiters.clone().try_acquire_owned().ok()
    }

    /// Shows a desktop notification, with an open action when there is a `url`, and waits for it to close.
    /// Returns `url` when the open action was clicked. Above the waiter cap the notification is shown
    /// without the action and not waited for.
    pub async fn send(&self, summary: String, body: String, url: Option<String>) -> Option<String> {
        let slot = if url.is_some() {
            self.try_wait_slot()
        } else {
            None
        };
        let result = tokio::task::spawn_blocking(move || {
            let mut notification = Notification::new();
            notification
                .appname("azuredevopstui")
                .summary(&summary)
                .body(&body)
                .timeout(Timeout::Milliseconds(TIMEOUT.as_millis() as u32));
            if slot.is_some() {
                notification.action(OPEN_ACTION, "Open");
            }
            let handle = notification.show()?;
            let mut open = false;
            if slot.is_some() {
                handle.wait_for_action(|action| open = action == OPEN_ACTION);
            }
            Ok::<_, notify_rust::error::Error>(open)
        })
        .await;
        match result {
            Ok(Ok(open)) => url.filter(|_| open),
            Ok(Err(err)) => {
                tracing::warn!("failed to show notification: {err}");
                None
            }
            Err(err) => {
                tracing::warn!("notification task failed: {err}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Notifier;

    #[test]
    fn wait_slots_are_capped() {
        let notifier = Notifier::new(2);

        let first = notifier.try_wait_slot();
        let second = notifier.try_wait_slot();

        assert!(first.is_some());
        assert!(second.is_some());
        assert!(notifier.try_wait_slot().is_none());
    }

    #[test]
    fn dropped_slot_is_reusable() {
        let notifier = Notifier::new(1);
        let slot = notifier.try_wait_slot();
        assert!(notifier.try_wait_slot().is_none());

        drop(slot);

        assert!(notifier.try_wait_slot().is_some());
    }
}

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};

const MAX_ERRORS: usize = 50;
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPINNER_STEP: Duration = Duration::from_millis(100);

/// Actions in flight and the errors reported so far.
#[derive(Default)]
pub struct Activity {
    next_id: u64,
    pending: Vec<(u64, String)>,
    /// Set while `pending` is not empty, to animate the spinner.
    since: Option<Instant>,
    /// Newest last, at most `MAX_ERRORS`.
    errors: VecDeque<(DateTime<Local>, String)>,
}

impl Activity {
    /// Registers an action in flight; the id comes back with its result.
    pub fn start(&mut self, label: String) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        if self.pending.is_empty() {
            self.since = Some(Instant::now());
        }
        self.pending.push((id, label));
        id
    }

    /// Removes the action `id` and returns its label.
    pub fn finish(&mut self, id: u64) -> Option<String> {
        let index = self
            .pending
            .iter()
            .position(|(pending, _)| *pending == id)?;
        let (_, label) = self.pending.remove(index);
        if self.pending.is_empty() {
            self.since = None;
        }
        Some(label)
    }

    /// The newest action in flight and how many others are.
    pub fn latest(&self) -> Option<(&str, usize)> {
        let (_, label) = self.pending.last()?;
        Some((label, self.pending.len() - 1))
    }

    /// The braille frame of the spinner now.
    pub fn spinner(&self) -> &'static str {
        let elapsed = self.since.map(|since| since.elapsed()).unwrap_or_default();
        spinner_frame(elapsed)
    }

    pub fn push_error(&mut self, text: String) {
        if self.errors.len() == MAX_ERRORS {
            self.errors.pop_front();
        }
        self.errors.push_back((Local::now(), text));
    }

    /// Newest first.
    pub fn errors(&self) -> impl Iterator<Item = &(DateTime<Local>, String)> {
        self.errors.iter().rev()
    }

    pub fn error_count(&self) -> usize {
        self.errors.len()
    }
}

fn spinner_frame(elapsed: Duration) -> &'static str {
    let step = elapsed.as_millis() / SPINNER_STEP.as_millis();
    SPINNER[step as usize % SPINNER.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_are_added_and_removed_by_id() {
        let mut activity = Activity::default();
        let first = activity.start("assigning #1".into());
        let second = activity.start("voting on !2".into());

        assert_eq!(activity.latest(), Some(("voting on !2", 1)));
        assert_eq!(activity.finish(second).as_deref(), Some("voting on !2"));
        assert_eq!(activity.latest(), Some(("assigning #1", 0)));
        assert_eq!(activity.finish(second), None);
        assert_eq!(activity.finish(first).as_deref(), Some("assigning #1"));
        assert_eq!(activity.latest(), None);
    }

    #[test]
    fn errors_are_listed_newest_first_and_capped() {
        let mut activity = Activity::default();
        for n in 0..MAX_ERRORS + 5 {
            activity.push_error(format!("error {n}"));
        }

        assert_eq!(activity.error_count(), MAX_ERRORS);
        let texts: Vec<_> = activity.errors().map(|(_, text)| text.as_str()).collect();
        assert_eq!(texts.first(), Some(&"error 54"));
        assert_eq!(texts.last(), Some(&"error 5"));
    }

    #[test]
    fn spinner_advances_every_100_ms_and_wraps() {
        assert_eq!(spinner_frame(Duration::ZERO), "⠋");
        assert_eq!(spinner_frame(Duration::from_millis(99)), "⠋");
        assert_eq!(spinner_frame(Duration::from_millis(100)), "⠙");
        assert_eq!(spinner_frame(Duration::from_millis(1000)), "⠋");
    }
}

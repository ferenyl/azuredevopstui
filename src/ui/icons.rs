use ratatui::style::Color;

use crate::theme::Theme;

/// A marker in the lists; the help lists them by [`SECTIONS`].
pub struct Icon {
    pub symbol: &'static str,
    pub meaning: &'static str,
    /// Followed by a count, like `✎2`.
    pub counted: bool,
    color: fn(&Theme) -> Color,
}

impl Icon {
    pub fn color(&self, theme: &Theme) -> Color {
        (self.color)(theme)
    }
}

const fn icon(symbol: &'static str, meaning: &'static str, color: fn(&Theme) -> Color) -> Icon {
    Icon {
        symbol,
        meaning,
        counted: false,
        color,
    }
}

const fn counted(symbol: &'static str, meaning: &'static str, color: fn(&Theme) -> Color) -> Icon {
    Icon {
        counted: true,
        ..icon(symbol, meaning, color)
    }
}

pub const UNRESOLVED: Icon = counted("✎", "unresolved threads", |t| t.pr_waiting);
pub const WAITING: Icon = icon("◔", "waiting for author", |t| t.pr_waiting);
pub const REJECTED: Icon = icon("⊘", "rejected", |t| t.pr_rejected);
pub const CHECKS_FAILED: Icon = icon("✖", "checks failed", |t| t.build_failed);
pub const CONFLICTS: Icon = icon("⇄", "merge conflicts", |t| t.error);
pub const UNLINKED: Icon = icon("∅", "no linked work item", |t| t.pr_waiting);
pub const READY: Icon = icon("✔", "ready to merge", |t| t.pr_approved);

pub const NEEDS_VOTE: Icon = icon("◉", "your vote is missing", |t| t.pr_waiting);
pub const CHANGED_SINCE_VOTE: Icon = icon("↻", "pushed after your vote", |t| t.build_running);
pub const REPLIES: Icon = counted("↩", "replies to your threads", |t| t.comment_author);
pub const MENTIONS: Icon = counted("@", "threads mentioning you", |t| t.title);

pub const NEW: Icon = icon("•", "new in the inbox", |t| t.title);
pub const MENTIONED: Icon = icon("@", "work item mentions you", |t| t.title);

pub const RUNNING: Icon = icon("⟳", "running", |t| t.build_running);
pub const CANCELLING: Icon = icon("⟳", "cancelling", |t| t.muted);
pub const SUCCEEDED: Icon = icon("✔", "succeeded", |t| t.build_succeeded);
pub const PARTIALLY_SUCCEEDED: Icon = icon("◑", "partially succeeded", |t| t.pr_waiting);
pub const FAILED: Icon = icon("✖", "failed", |t| t.build_failed);
pub const CANCELED: Icon = icon("⊘", "canceled", |t| t.muted);
pub const COMPLETED: Icon = icon("–", "completed", |t| t.muted);
pub const QUEUED: Icon = icon("○", "queued", |t| t.muted);

/// The icons by where they show, as the help lists them.
pub const SECTIONS: [(&str, &[Icon]); 4] = [
    (
        "My PRs",
        &[
            UNRESOLVED,
            WAITING,
            REJECTED,
            CHECKS_FAILED,
            CONFLICTS,
            UNLINKED,
            READY,
        ],
    ),
    (
        "Others' PRs",
        &[NEEDS_VOTE, CHANGED_SINCE_VOTE, REPLIES, MENTIONS],
    ),
    ("Inbox", &[NEW, MENTIONED]),
    (
        "Pipelines",
        &[
            RUNNING,
            CANCELLING,
            SUCCEEDED,
            PARTIALLY_SUCCEEDED,
            FAILED,
            CANCELED,
            COMPLETED,
            QUEUED,
        ],
    ),
];

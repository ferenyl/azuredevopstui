use crate::api::PullRequest;
use crate::app::loads::Load;
use crate::app::message::Message;
use crate::app::{App, Detail, Popup, Selection};

/// Votes in the order they are offered.
pub(in crate::app) const VOTES: [(i32, &str); 5] = [
    (10, "Approve"),
    (5, "Approve with suggestions"),
    (-5, "Wait for author"),
    (-10, "Reject"),
    (0, "Reset vote"),
];

impl App {
    pub(in crate::app) fn open_vote_picker(&mut self) {
        let Some(Detail::PullRequest(pr)) = self.current_item() else {
            return;
        };
        let current = self.user.as_ref().and_then(|user| {
            pr.reviewers
                .iter()
                .find(|reviewer| reviewer.id.eq_ignore_ascii_case(&user.id))
                .map(|reviewer| reviewer.vote)
        });
        let mut selection =
            Selection::new(VOTES.iter().map(|(_, label)| (*label).into()).collect());
        selection.selected = VOTES
            .iter()
            .position(|(vote, _)| Some(*vote) == current)
            .unwrap_or(0);
        self.popup = Some(Popup::Vote { pr, selection });
    }

    pub(in crate::app) fn confirm_vote(&mut self, pr: Box<PullRequest>, index: usize) {
        let (Some((vote, _)), Some(api), Some(user)) =
            (VOTES.get(index).copied(), self.api(), self.user.as_ref())
        else {
            return;
        };
        let user_id = user.id.clone();
        let label = format!("voting on !{}", pr.pull_request_id);
        self.spawn_action(label, async move {
            let result = api.vote(&pr, &user_id, vote).await;
            Message::PullRequestUpdated {
                id: pr.pull_request_id,
                reload: vec![Load::MyPullRequests, Load::OtherPullRequests, Load::Detail],
                result,
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use super::VOTES;
    use crate::app::Popup;
    use crate::app::test_helpers::{app, me, open_pull_request, press};
    use crate::test_support::pull_request;

    #[test]
    fn vote_picker_lists_the_five_votes() {
        let mut app = app();
        app.user = Some(me());
        open_pull_request(&mut app);

        press(&mut app, KeyCode::Char('v'));

        let Some(Popup::Vote { selection, .. }) = &app.popup else {
            panic!("vote picker not open");
        };
        assert_eq!(selection.items.len(), VOTES.len());
        assert_eq!(selection.current(), Some("Approve"));
    }

    #[test]
    fn vote_picker_preselects_my_current_vote() {
        let mut app = app();
        app.user = Some(me());
        let mut pr = pull_request(1, "First", "me", "2026-10-01T10:00:00Z");
        pr.reviewers[0].id = "me-id".into();
        pr.reviewers[0].vote = -5;
        app.data.set_my_prs(vec![pr]);

        press(&mut app, KeyCode::Char('v'));

        let Some(Popup::Vote { selection, .. }) = &app.popup else {
            panic!("vote picker not open");
        };
        assert_eq!(selection.current(), Some("Wait for author"));
    }

    #[test]
    fn esc_cancels_the_vote() {
        let mut app = app();
        app.user = Some(me());
        open_pull_request(&mut app);
        press(&mut app, KeyCode::Char('v'));

        press(&mut app, KeyCode::Esc);

        assert!(app.popup.is_none());
    }

    #[test]
    fn votes_map_to_the_api_values() {
        let votes: Vec<i32> = VOTES.iter().map(|(vote, _)| *vote).collect();

        assert_eq!(votes, [10, 5, -5, -10, 0]);
    }
}

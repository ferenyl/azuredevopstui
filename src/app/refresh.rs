use std::time::{Duration, Instant};

use crate::api::PullRequest;
use crate::images::image_urls;
use crate::rich_text::RichText;

use super::loads::Load;
use super::message::Message;
use super::{App, Detail, DetailInfo, DetailTab, Panel, Screen};

const ALL: [Load; 6] = [
    Load::MyPullRequests,
    Load::Builds,
    Load::Mentions,
    Load::OtherPullRequests,
    Load::Detail,
    Load::Sprint,
];

impl App {
    /// Reloads the shown data, leaving requests already in flight to finish.
    pub(super) fn refresh(&mut self) {
        self.load_all(&ALL, false);
    }

    /// Like [`App::refresh`], but restarts requests in flight, whose data may predate a change.
    pub(super) fn reload(&mut self) {
        self.load_all(&ALL, true);
    }

    /// Restarts only `loads` after a mutation, whose effect earlier requests may predate.
    pub(super) fn reload_only(&mut self, loads: &[Load]) {
        self.load_all(loads, true);
    }

    fn load_all(&mut self, loads: &[Load], restart: bool) {
        let (Some(api), Some(config), Some(user)) =
            (self.api(), self.config.clone(), self.user.clone())
        else {
            return;
        };
        self.error = None;
        let full = loads.len() == ALL.len();
        if full {
            self.last_refresh = Instant::now();
            self.track_inbox();
        }
        self.data.show_approved = config.other_prs_filter.show_approved;
        self.data.show_drafts = config.other_prs_filter.show_drafts;
        let wanted =
            |app: &Self, load| loads.contains(&load) && (restart || !app.loads.is_loading(load));

        if wanted(self, Load::MyPullRequests) {
            let (a, user_id) = (api.clone(), user.id.clone());
            self.spawn_load(Load::MyPullRequests, async move {
                Message::MyPullRequests(a.my_pull_requests(&user_id).await)
            });
        }

        if wanted(self, Load::Builds) {
            let (a, user_id) = (api.clone(), user.id.clone());
            self.spawn_load(Load::Builds, async move {
                Message::Builds(a.my_builds(&user_id).await)
            });
        }

        if wanted(self, Load::Mentions) {
            let (a, user_id) = (api.clone(), user.id.clone());
            self.spawn_load(Load::Mentions, async move {
                Message::Mentions(a.mentioned_work_items(&user_id).await)
            });
        }

        if wanted(self, Load::OtherPullRequests) {
            self.spawn_load(Load::OtherPullRequests, async move {
                Message::OtherPullRequests(
                    api.other_pull_requests(&user.id, &config.other_prs_filter)
                        .await,
                )
            });
        }

        if let Some(detail) = self.detail.clone()
            && wanted(self, Load::Detail)
        {
            self.load_detail(&detail);
        }

        if wanted(self, Load::Sprint) {
            self.load_sprint();
        }

        let board_stale = loads.contains(&Load::Sprint) || loads.contains(&Load::SprintBoard);
        if self.board_view && board_stale && (restart || !self.loads.is_loading(Load::SprintBoard))
        {
            self.load_sprint_board();
        }
    }

    /// Shows the board lanes in place of the detail, or hides them again.
    pub(super) fn toggle_sprint_board(&mut self) {
        if self.board_view {
            self.board_view = false;
            self.loads.cancel(Load::SprintBoard);
            return;
        }
        self.close_detail();
        self.board_view = true;
        self.data.board_items = None;
        if self.board.is_none() {
            self.load_board();
        }
        self.load_sprint_board();
    }

    pub(super) fn load_sprint_board(&mut self) {
        let (Some(api), Some(config)) = (self.api(), &self.config) else {
            return;
        };
        let types = config.work_item_types.clone();
        let sprint = self.sprint.clone();
        self.spawn_load(Load::SprintBoard, async move {
            Message::SprintBoard(api.sprint_board_items(sprint.as_deref(), &types).await)
        });
    }

    /// Refreshes on the configured interval while the main view is idle. 0 disables it.
    pub(super) fn auto_refresh(&mut self) {
        let Some(interval) = self.config.as_ref().map(|c| c.refresh_interval_secs) else {
            return;
        };
        if interval > 0
            && matches!(self.screen, Screen::Main)
            && self.popup.is_none()
            && self.last_refresh.elapsed() >= Duration::from_secs(interval)
        {
            self.refresh();
        }
    }

    pub(super) fn load_sprint(&mut self) {
        let (Some(api), Some(config)) = (self.api(), &self.config) else {
            return;
        };
        let Some(ready_column) = config.ready_column.clone() else {
            return;
        };
        let types = config.work_item_types.clone();
        let sprint = self.sprint.clone();
        self.spawn_load(Load::Sprint, async move {
            Message::SprintWorkItems(
                api.sprint_work_items(sprint.as_deref(), &ready_column, &types)
                    .await,
            )
        });
    }

    pub(super) fn load_sprints(&self) {
        let Some(api) = self.api() else {
            return;
        };
        self.spawn(async move { Message::Sprints(api.iterations().await) });
    }

    /// Board columns, used for the sprint overview and moving items.
    pub(super) fn load_board(&self) {
        let Some(api) = self.api() else {
            return;
        };
        self.spawn(async move { Message::TeamBoard(api.board().await) });
    }

    pub(super) fn load_project_work_item_types(&self) {
        let Some(api) = self.api() else {
            return;
        };
        self.spawn(async move { Message::ProjectWorkItemTypes(api.work_item_types().await) });
    }

    pub(super) fn load_work_item_types(&mut self) {
        let Some(api) = self.api() else {
            return;
        };
        self.spawn(async move { Message::WorkItemTypes(api.work_item_types().await) });
    }

    pub(super) fn load_detail(&mut self, detail: &Detail) {
        let Some(api) = self.api() else {
            return;
        };
        match detail {
            Detail::PullRequest(pr) => {
                let (id, pr) = (pr.pull_request_id, pr.clone());
                self.spawn_load(Load::Detail, async move {
                    let result = api.pull_request_details(&pr).await;
                    Message::PullRequestDetails { id, result }
                });
            }
            Detail::WorkItem(item) => {
                let id = item.id;
                self.spawn_load(Load::Detail, async move {
                    let result = api.work_item_details(id).await;
                    Message::WorkItemDetails { id, result }
                });
            }
            Detail::Build(build) => {
                let id = build.id;
                self.spawn_load(Load::Detail, async move {
                    let result = api.build_timeline(id).await;
                    Message::BuildTimeline { id, result }
                });
            }
        }
    }

    pub fn loading_older_comments(&self) -> bool {
        self.loads.is_loading(Load::Comments)
    }

    /// Loads the next page of older comments when the end of the Comments tab is on screen.
    pub(super) fn load_older_comments(&mut self) {
        let (Some(Detail::WorkItem(item)), Some(DetailInfo::WorkItem(info))) =
            (&self.detail, &self.detail_info)
        else {
            return;
        };
        let Some(token) = info.comments_continuation.clone() else {
            return;
        };
        let at_end = self.detail_scroll >= self.detail_max_scroll.get();
        if self.detail_tab != DetailTab::Comments
            || !at_end
            || self.older_comments_failed
            || self.loads.is_loading(Load::Comments)
        {
            return;
        }
        let Some(api) = self.api() else {
            return;
        };
        let id = item.id;
        self.spawn_load(Load::Comments, async move {
            let result = api.older_comments(id, &token).await;
            Message::OlderComments { id, result }
        });
    }

    /// Starts downloading images in the shown detail that are not cached yet.
    pub(super) fn load_images(&self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let mut texts: Vec<&str> = Vec::new();
        let mut rich: Vec<&RichText> = Vec::new();
        if let Some(Detail::PullRequest(pr)) = &self.detail {
            texts.extend(pr.description.as_deref());
        }
        match &self.detail_info {
            Some(DetailInfo::PullRequest(info)) => {
                rich.extend(&info.description);
                rich.extend(
                    info.threads
                        .iter()
                        .flat_map(|thread| &thread.comments)
                        .map(|comment| &comment.text),
                );
            }
            Some(DetailInfo::WorkItem(info)) => {
                rich.extend(
                    [
                        &info.description,
                        &info.repro_steps,
                        &info.acceptance_criteria,
                    ]
                    .into_iter()
                    .flatten(),
                );
                rich.extend(info.comments.iter().map(|comment| &comment.text));
            }
            Some(DetailInfo::Build(_)) | None => {}
        }
        let urls = texts
            .into_iter()
            .flat_map(image_urls)
            .chain(rich.into_iter().flat_map(RichText::image_urls))
            .collect();
        for url in self.images.start_loading(urls) {
            let client = client.clone();
            self.spawn(async move {
                let result = match client.download(&url).await {
                    Ok(bytes) => tokio::task::spawn_blocking(move || {
                        image::load_from_memory(&bytes).map_err(anyhow::Error::from)
                    })
                    .await
                    .map_err(anyhow::Error::from)
                    .and_then(|result| result),
                    Err(err) => Err(err),
                };
                Message::Image { url, result }
            });
        }
    }

    pub(super) fn load_signals(&mut self, prs: &[PullRequest]) {
        let Some(api) = self.api() else {
            return;
        };
        for pr in prs {
            let (api, pr) = (api.clone(), pr.clone());
            self.spawn_load(Load::Signals(pr.pull_request_id), async move {
                let result = api.pull_request_checks(&pr).await;
                Message::PullRequestSignals {
                    id: pr.pull_request_id,
                    result,
                }
            });
        }
    }

    pub(super) fn open_selected(&mut self) {
        let Some(index) = self.focus.index() else {
            return;
        };
        let Some(detail) = self.data.detail(self.focus, self.selections[index]) else {
            return;
        };
        self.board_view = false;
        self.loads.cancel(Load::SprintBoard);
        self.load_detail(&detail);
        if let Some(seen) = &mut self.data.inbox_seen {
            seen.insert(detail.key());
        }
        self.detail = Some(detail);
        self.load_images();
        self.detail_info = None;
        self.loads.cancel(Load::Comments);
        self.older_comments_failed = false;
        self.clear_files();
        self.clear_build_log();
        self.detail_scroll = 0;
        self.link_index = None;
        self.detail_tab = DetailTab::Overview;
        self.detail_thread = None;
        self.scroll_to_thread = false;
    }

    /// Back to the sprint overview.
    pub(super) fn close_detail(&mut self) {
        self.loads.cancel(Load::Detail);
        self.detail = None;
        self.detail_info = None;
        self.clear_files();
        self.clear_build_log();
        self.detail_links.borrow_mut().clear();
        self.link_index = None;
        if self.focus == Panel::Detail {
            self.focus = self.last_left;
        }
    }

    pub(super) fn sync_detail(&mut self) {
        if let Some(fresh) = self.detail.as_ref().and_then(|d| self.data.find(d)) {
            self.detail = Some(fresh);
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use crate::api::{AdoClient, Iteration};
    use crate::app::loads::Load;
    use crate::app::test_helpers::{app, me, open_pull_request, press};
    use crate::app::{App, Detail, DetailInfo, DetailTab, Panel, Popup, SprintPicker};
    use crate::auth::Auth;
    use crate::test_support::{work_item, work_item_details};

    #[test]
    fn enter_opens_selected_item_on_overview() {
        let mut app = app();
        app.detail_tab = DetailTab::Checks;
        app.detail_scroll = 5;
        press(&mut app, KeyCode::Char('j'));

        press(&mut app, KeyCode::Enter);

        assert!(app.detail.as_ref().is_some_and(|d| d.is_pull_request(2)));
        assert_eq!(app.detail_tab, DetailTab::Overview);
        assert_eq!(app.detail_scroll, 0);
        assert!(app.detail_info.is_none());
    }

    #[test]
    fn b_toggles_the_board_and_closes_an_open_item() {
        let mut app = app();
        open_pull_request(&mut app);

        press(&mut app, KeyCode::Char('b'));

        assert!(app.board_view);
        assert!(app.detail.is_none());
        press(&mut app, KeyCode::Char('b'));
        assert!(!app.board_view);
    }

    #[test]
    fn esc_hides_the_board_but_first_closes_zoom() {
        let mut app = app();
        press(&mut app, KeyCode::Char('b'));
        press(&mut app, KeyCode::Char('z'));
        app.focus = Panel::Detail;

        press(&mut app, KeyCode::Esc);
        assert!(app.board_view);
        assert!(!app.zoomed);
        press(&mut app, KeyCode::Esc);

        assert!(!app.board_view);
    }

    #[test]
    fn opening_an_item_replaces_the_board() {
        let mut app = app();
        press(&mut app, KeyCode::Char('b'));

        press(&mut app, KeyCode::Enter);

        assert!(!app.board_view);
        assert!(app.detail.is_some());
    }

    #[tokio::test]
    async fn the_board_loads_only_while_it_is_shown() {
        let mut app = app();
        // Nothing listens there; the requests stay pending or fail, and are never handled.
        app.client = Some(AdoClient::with_base_url(
            Auth::from_pat("pat"),
            "http://127.0.0.1:9",
        ));
        app.sync_api();
        app.user = Some(me());

        press(&mut app, KeyCode::Char('r'));
        assert_eq!(app.loads.generation(Load::SprintBoard), None);

        press(&mut app, KeyCode::Char('b'));
        let started = app.loads.generation(Load::SprintBoard);
        assert!(started.is_some());
        assert!(app.data.board_items.is_none());

        press(&mut app, KeyCode::Char('r'));
        assert_eq!(app.loads.generation(Load::SprintBoard), started);
        app.reload();
        assert!(app.loads.generation(Load::SprintBoard) > started);

        press(&mut app, KeyCode::Char('b'));
        assert_eq!(app.loads.generation(Load::SprintBoard), None);
    }

    #[tokio::test]
    async fn switching_sprint_reloads_the_board() {
        let mut app = app();
        app.client = Some(AdoClient::with_base_url(
            Auth::from_pat("pat"),
            "http://127.0.0.1:9",
        ));
        app.sync_api();
        app.user = Some(me());
        press(&mut app, KeyCode::Char('b'));
        app.data.board_items = Some(Vec::new());
        let started = app.loads.generation(Load::SprintBoard);
        app.popup = Some(Popup::Sprint(SprintPicker::new(
            vec![Iteration {
                name: "Sprint 2".into(),
                path: "Team\\Sprint 2".into(),
                attributes: Default::default(),
            }],
            None,
        )));

        press(&mut app, KeyCode::Enter);

        assert!(app.data.board_items.is_none());
        assert!(app.loads.generation(Load::SprintBoard) > started);
    }

    #[tokio::test]
    async fn repeated_refresh_leaves_requests_in_flight_alone() {
        let mut app = app();
        // Nothing listens there; the requests stay pending or fail, and are never handled.
        app.client = Some(AdoClient::with_base_url(
            Auth::from_pat("pat"),
            "http://127.0.0.1:9",
        ));
        app.sync_api();
        app.user = Some(me());

        let loads = [
            Load::MyPullRequests,
            Load::OtherPullRequests,
            Load::Builds,
            Load::Mentions,
            Load::Sprint,
        ];
        press(&mut app, KeyCode::Char('r'));
        let started = loads.map(|load| app.loads.generation(load));
        press(&mut app, KeyCode::Char('r'));
        press(&mut app, KeyCode::Char('r'));

        assert!(started.iter().all(Option::is_some));
        assert_eq!(loads.map(|load| app.loads.generation(load)), started);
    }

    /// Work item 10 open on its Comments tab, scrolled to the end of what is drawn.
    fn on_comments(token: Option<&str>) -> App {
        let mut app = app();
        // Nothing listens there; the request stays pending or fails, and is never handled.
        app.client = Some(AdoClient::with_base_url(
            Auth::from_pat("pat"),
            "http://127.0.0.1:9",
        ));
        app.sync_api();
        app.detail = Some(Detail::WorkItem(Box::new(work_item(
            10,
            "Story",
            "User Story",
            "Active",
        ))));
        app.detail_info = Some(DetailInfo::WorkItem(Box::new(work_item_details(
            &["new"],
            token,
        ))));
        app.detail_tab = DetailTab::Comments;
        app.detail_max_scroll.set(0);
        app
    }

    #[tokio::test]
    async fn older_comments_load_at_the_end_of_the_comments() {
        let mut app = on_comments(Some("page2"));

        app.load_older_comments();

        assert!(app.loading_older_comments());
    }

    #[tokio::test]
    async fn scrolling_to_the_end_loads_older_comments() {
        let mut app = on_comments(Some("page2"));
        app.detail_max_scroll.set(5);
        app.focus = Panel::Detail;

        press(&mut app, KeyCode::Char('j'));
        assert!(!app.loading_older_comments());
        press(&mut app, KeyCode::Char('G'));

        assert!(app.loading_older_comments());
    }

    #[tokio::test]
    async fn older_comments_wait_while_more_is_to_scroll() {
        let mut app = on_comments(Some("page2"));
        app.detail_max_scroll.set(5);

        app.load_older_comments();

        assert!(!app.loading_older_comments());
    }

    #[tokio::test]
    async fn older_comments_need_a_token_and_the_comments_tab() {
        let mut app = on_comments(None);
        app.load_older_comments();
        assert!(!app.loading_older_comments());

        let mut app = on_comments(Some("page2"));
        app.detail_tab = DetailTab::Overview;
        app.load_older_comments();
        assert!(!app.loading_older_comments());
    }

    #[tokio::test]
    async fn older_comments_are_not_requested_twice_at_once() {
        let mut app = on_comments(Some("page2"));
        app.load_older_comments();
        let started = app.loads.generation(Load::Comments);

        app.load_older_comments();

        assert!(started.is_some());
        assert_eq!(app.loads.generation(Load::Comments), started);
    }

    #[tokio::test]
    async fn drawing_does_not_retry_failed_older_comments() {
        let mut app = on_comments(Some("page2"));
        app.older_comments_failed = true;

        app.load_older_comments();

        assert!(!app.loading_older_comments());
    }
}

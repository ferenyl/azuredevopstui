use std::time::{Duration, Instant};

use crate::api::PullRequest;
use crate::images::image_urls;
use crate::rich_text::RichText;

use super::message::Message;
use super::{App, Detail, DetailInfo, DetailTab, Panel, Screen};

impl App {
    pub(super) fn refresh(&mut self) {
        let (Some(client), Some(config), Some(user)) =
            (self.client.clone(), self.config.clone(), self.user.clone())
        else {
            return;
        };
        self.error = None;
        self.last_refresh = Instant::now();
        self.track_inbox();
        self.data.show_approved = config.other_prs_filter.show_approved;
        self.data.show_drafts = config.other_prs_filter.show_drafts;

        let (c, cfg, user_id) = (client.clone(), config.clone(), user.id.clone());
        self.spawn(async move {
            Message::MyPullRequests(
                c.my_pull_requests(&cfg.organization, &cfg.project, &user_id)
                    .await,
            )
        });

        let (c, cfg, user_id) = (client.clone(), config.clone(), user.id.clone());
        self.spawn(async move {
            Message::Builds(c.my_builds(&cfg.organization, &cfg.project, &user_id).await)
        });

        let (c, cfg, user_id) = (client.clone(), config.clone(), user.id.clone());
        self.spawn(async move {
            Message::Mentions(
                c.mentioned_work_items(&cfg.organization, &cfg.project, &cfg.team, &user_id)
                    .await,
            )
        });

        let (c, cfg, user_id) = (client.clone(), config.clone(), user.id);
        self.spawn(async move {
            Message::OtherPullRequests(
                c.other_pull_requests(
                    &cfg.organization,
                    &cfg.project,
                    &user_id,
                    &cfg.other_prs_filter,
                )
                .await,
            )
        });

        if let Some(detail) = &self.detail {
            self.load_detail(detail);
        }

        self.load_sprint();
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

    pub(super) fn load_sprint(&self) {
        let Some((client, config)) = self.connection() else {
            return;
        };
        let Some(ready_column) = config.ready_column.clone() else {
            return;
        };
        let sprint = self.sprint.clone();
        self.spawn(async move {
            Message::SprintWorkItems(
                client
                    .sprint_work_items(
                        &config.organization,
                        &config.project,
                        &config.team,
                        sprint.as_deref(),
                        &ready_column,
                        &config.work_item_types,
                    )
                    .await,
            )
        });
    }

    pub(super) fn load_sprints(&self) {
        let Some((client, config)) = self.connection() else {
            return;
        };
        self.spawn(async move {
            Message::Sprints(
                client
                    .iterations(&config.organization, &config.project, &config.team)
                    .await,
            )
        });
    }

    /// Board columns, used for the sprint overview and moving items.
    pub(super) fn load_board(&self) {
        let Some((client, config)) = self.connection() else {
            return;
        };
        self.spawn(async move {
            Message::TeamBoard(
                client
                    .board(&config.organization, &config.project, &config.team)
                    .await,
            )
        });
    }

    pub(super) fn load_work_item_types(&mut self) {
        let Some((client, config)) = self.connection() else {
            return;
        };
        self.spawn(async move {
            Message::WorkItemTypes(
                client
                    .work_item_types(&config.organization, &config.project)
                    .await,
            )
        });
    }

    pub(super) fn load_detail(&self, detail: &Detail) {
        let Some((client, config)) = self.connection() else {
            return;
        };
        match detail {
            Detail::PullRequest(pr) => {
                let (id, pr) = (pr.pull_request_id, pr.clone());
                self.spawn(async move {
                    let result = client
                        .pull_request_details(&config.organization, &config.project, &pr)
                        .await;
                    Message::PullRequestDetails { id, result }
                });
            }
            Detail::WorkItem(item) => {
                let id = item.id;
                self.spawn(async move {
                    let result = client
                        .work_item_details(&config.organization, &config.project, id)
                        .await;
                    Message::WorkItemDetails { id, result }
                });
            }
            Detail::Build(_) => {}
        }
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
                texts.extend(info.description.as_deref());
                texts.extend(
                    info.threads
                        .iter()
                        .flat_map(|thread| &thread.comments)
                        .filter_map(|comment| comment.content.as_deref()),
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
            None => {}
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

    pub(super) fn load_signals(&self, prs: &[PullRequest]) {
        let Some((client, config)) = self.connection() else {
            return;
        };
        for pr in prs {
            let (client, config, pr) = (client.clone(), config.clone(), pr.clone());
            self.spawn(async move {
                let result = client
                    .pull_request_details(&config.organization, &config.project, &pr)
                    .await;
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
        self.load_detail(&detail);
        if let Some(seen) = &mut self.data.inbox_seen {
            seen.insert(detail.key());
        }
        self.detail = Some(detail);
        self.load_images();
        self.detail_info = None;
        self.detail_scroll = 0;
        self.detail_tab = DetailTab::Overview;
    }

    /// Back to the sprint overview.
    pub(super) fn close_detail(&mut self) {
        self.detail = None;
        self.detail_info = None;
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

    use crate::app::DetailTab;
    use crate::app::test_helpers::{app, press};

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
}

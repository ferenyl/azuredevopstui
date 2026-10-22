use crossterm::event::KeyEvent;

use super::{Action, App, Detail, LEFT_PANELS, Panel, Popup, Retry, Screen, SetupStep, TextInput};

impl App {
    pub(super) fn handle_key(&mut self, key: KeyEvent) {
        if let Screen::Setup(step) = &mut self.screen
            && let Some(input) = step.input_mut()
            && input.handle_key(key, false)
        {
            return;
        }
        if self.handle_popup_key(key) {
            return;
        }
        if let Some(action) = Action::from_key(key) {
            self.apply(action);
        }
    }

    pub(super) fn apply(&mut self, action: Action) {
        if self.show_help {
            match action {
                Action::Cancel | Action::Help => self.show_help = false,
                Action::Quit => self.should_quit = true,
                _ => {}
            }
            return;
        }
        if self.apply_popup(action) {
            return;
        }
        match action {
            Action::Quit => self.should_quit = true,
            Action::Cancel => match self.screen {
                Screen::Setup(SetupStep::EnterPat {
                    cancelable: true, ..
                }) => self.screen = Screen::Main,
                Screen::Setup(_) => self.should_quit = true,
                Screen::Main => self.close_detail(),
            },
            Action::Help => self.show_help = matches!(self.screen, Screen::Main),
            Action::ChangeToken => match self.screen {
                Screen::Main => self.set_step(SetupStep::EnterPat {
                    input: TextInput::default(),
                    error: None,
                    cancelable: true,
                }),
                Screen::Setup(SetupStep::Failed {
                    retry: Retry::Authenticate,
                    ..
                }) if self.can_enter_pat() => self.set_step(SetupStep::EnterPat {
                    input: TextInput::default(),
                    error: None,
                    cancelable: false,
                }),
                Screen::Setup(_) => {}
            },
            Action::Up | Action::Down => match &mut self.screen {
                Screen::Main if self.focus == Panel::Detail => {
                    self.scroll_detail(action == Action::Down);
                }
                Screen::Main => self.move_selection(action == Action::Down),
                Screen::Setup(step) => {
                    if let Some(selection) = step.selection_mut() {
                        if action == Action::Up {
                            selection.previous();
                        } else {
                            selection.next();
                        }
                    }
                }
            },
            Action::Confirm => match self.screen {
                Screen::Main => self.open_selected(),
                Screen::Setup(_) => self.confirm_setup_step(),
            },
            Action::OpenInBrowser
            | Action::Copy
            | Action::CopyLink
            | Action::AssignToMe
            | Action::Unassign
            | Action::Tag
            | Action::Comment
            | Action::ChangeColumn
            | Action::Complete
            | Action::Sort
            | Action::Filter
            | Action::Sprint
            | Action::Toggle
            | Action::NextTab
            | Action::PrevTab
                if !matches!(self.screen, Screen::Main) => {}
            Action::OpenInBrowser => self.open_in_browser(),
            Action::Copy => self.copy_number(),
            Action::CopyLink => self.copy_link(),
            Action::Sort => self.open_sort_picker(),
            Action::Filter => match self.current_item() {
                _ if self.focus == Panel::Builds => self.open_build_filter(),
                Some(Detail::Build(_)) => self.open_build_filter(),
                Some(Detail::PullRequest(_)) => self.open_pr_filter(),
                _ => self.load_work_item_types(),
            },
            Action::Sprint => self.load_sprints(),
            Action::Toggle => {}
            Action::NextTab | Action::PrevTab => self.switch_tab(action == Action::NextTab),
            Action::AssignToMe => self.assign_to_me(),
            Action::ChangeColumn => self.change_column(false),
            Action::Complete => self.complete(),
            Action::Unassign => self.unassign(),
            Action::Tag => self.tag(),
            Action::Comment => self.open_comment(),
            Action::FocusLeft | Action::FocusRight | Action::FocusUp | Action::FocusDown => {
                if matches!(self.screen, Screen::Main) {
                    self.move_focus(action);
                }
            }
            Action::Reload => match &self.screen {
                Screen::Main => self.refresh(),
                Screen::Setup(SetupStep::Failed { .. }) => {
                    if let Screen::Setup(SetupStep::Failed { retry, .. }) =
                        std::mem::replace(&mut self.screen, Screen::Main)
                    {
                        self.retry(retry);
                    }
                }
                Screen::Setup(_) => {}
            },
        }
    }

    pub(super) fn move_focus(&mut self, action: Action) {
        match (action, self.focus.index()) {
            (Action::FocusLeft, None) => self.focus = self.last_left,
            (Action::FocusRight, Some(_)) => {
                self.last_left = self.focus;
                self.focus = Panel::Detail;
            }
            (Action::FocusUp, Some(index)) if index > 0 => self.focus = LEFT_PANELS[index - 1],
            (Action::FocusDown, Some(index)) if index + 1 < LEFT_PANELS.len() => {
                self.focus = LEFT_PANELS[index + 1];
            }
            _ => {}
        }
    }

    pub(super) fn move_selection(&mut self, down: bool) {
        let Some(index) = self.focus.index() else {
            return;
        };
        let len = self.data.len(self.focus);
        let selected = &mut self.selections[index];
        *selected = if down {
            (*selected + 1).min(len.saturating_sub(1))
        } else {
            selected.saturating_sub(1)
        };
    }

    pub(super) fn scroll_detail(&mut self, down: bool) {
        self.detail_scroll = if down {
            self.detail_scroll.saturating_add(1)
        } else {
            self.detail_scroll.saturating_sub(1)
        };
    }

    pub(super) fn switch_tab(&mut self, next: bool) {
        let Some(detail) = &self.detail else {
            return;
        };
        let tabs = detail.tabs();
        let index = tabs
            .iter()
            .position(|tab| *tab == self.detail_tab)
            .unwrap_or(0);
        let index = if next {
            (index + 1) % tabs.len()
        } else {
            (index + tabs.len() - 1) % tabs.len()
        };
        self.detail_tab = tabs[index];
        self.detail_scroll = 0;
    }

    pub(super) fn clamp_selections(&mut self) {
        for (index, panel) in LEFT_PANELS.iter().enumerate() {
            let len = self.data.len(*panel);
            self.selections[index] = self.selections[index].min(len.saturating_sub(1));
        }
    }

    pub(crate) fn handle_paste(&mut self, pasted: &str) {
        let pasted = pasted.replace("\r\n", "\n").replace('\r', "\n");
        if let Some(Popup::Comment { text, .. }) = &mut self.popup {
            text.insert_str(&pasted);
            return;
        }
        let line = pasted.lines().next().unwrap_or_default();
        match &mut self.popup {
            Some(Popup::BuildFilter { text }) => text.insert_str(line),
            Some(Popup::Tags(picker)) => {
                picker.query.insert_str(line);
                picker.filter();
            }
            Some(Popup::Sprint(picker)) => {
                picker.query.insert_str(line);
                picker.filter();
            }
            _ => {
                if let Screen::Setup(step) = &mut self.screen
                    && let Some(input) = step.input_mut()
                {
                    input.insert_str(line);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use crate::app::message::Message;
    use crate::app::test_helpers::{app, ctrl, open_pull_request, press};
    use crate::app::{App, DetailTab, Panel, Popup, Screen, SetupStep, TextInput};
    use crate::test_support::pull_request;

    #[test]
    fn focus_moves_between_boxes_and_stops_at_edges() {
        let mut app = app();

        ctrl(&mut app, 'k');
        assert_eq!(app.focus, Panel::Inbox);
        ctrl(&mut app, 'k');
        assert_eq!(app.focus, Panel::Inbox);
        for _ in 0..5 {
            ctrl(&mut app, 'j');
        }
        assert_eq!(app.focus, Panel::Builds);
    }

    #[test]
    fn focus_returns_to_last_left_box() {
        let mut app = app();
        ctrl(&mut app, 'j');
        ctrl(&mut app, 'j');

        ctrl(&mut app, 'l');
        assert_eq!(app.focus, Panel::Detail);
        ctrl(&mut app, 'h');

        assert_eq!(app.focus, Panel::OtherPrs);
    }

    #[test]
    fn selection_stays_within_list() {
        let mut app = app();

        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.selections[1], 0);
        for _ in 0..5 {
            press(&mut app, KeyCode::Char('j'));
        }
        assert_eq!(app.selections[1], 2);
    }

    #[test]
    fn selection_is_clamped_when_list_shrinks() {
        let mut app = app();
        app.selections[1] = 2;

        app.handle_message(Message::MyPullRequests(Ok(vec![pull_request(
            1,
            "Only",
            "me",
            "2026-10-01T10:00:00Z",
        )])));

        assert_eq!(app.selections[1], 0);
    }

    #[test]
    fn j_scrolls_detail_when_it_has_focus() {
        let mut app = app();
        open_pull_request(&mut app);
        ctrl(&mut app, 'l');

        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('k'));

        assert_eq!(app.detail_scroll, 1);
        assert_eq!(app.selections[1], 0);
    }

    #[test]
    fn tab_cycles_through_detail_tabs() {
        let mut app = app();
        open_pull_request(&mut app);
        app.detail_scroll = 3;

        press(&mut app, KeyCode::Tab);
        assert_eq!(app.detail_tab, DetailTab::Comments);
        assert_eq!(app.detail_scroll, 0);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.detail_tab, DetailTab::Checks);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.detail_tab, DetailTab::Overview);
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.detail_tab, DetailTab::Checks);
    }

    #[test]
    fn tab_without_detail_does_nothing() {
        let mut app = app();

        press(&mut app, KeyCode::Tab);

        assert_eq!(app.detail_tab, DetailTab::Overview);
    }

    #[test]
    fn help_popup_blocks_other_actions() {
        let mut app = app();

        press(&mut app, KeyCode::Char('?'));
        assert!(app.show_help);
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.selections[1], 0);
        press(&mut app, KeyCode::Esc);

        assert!(!app.show_help);
        assert!(!app.should_quit);
    }

    #[test]
    fn esc_in_setup_quits() {
        let mut app = App::new(None, None);

        press(&mut app, KeyCode::Esc);

        assert!(app.should_quit);
    }

    #[test]
    fn esc_in_cancelable_pat_dialog_returns_to_main() {
        let mut app = app();

        press(&mut app, KeyCode::Char('T'));
        assert!(matches!(
            app.screen,
            Screen::Setup(SetupStep::EnterPat { .. })
        ));
        press(&mut app, KeyCode::Esc);

        assert!(matches!(app.screen, Screen::Main));
        assert!(!app.should_quit);
    }

    #[test]
    fn text_input_takes_letters_instead_of_actions() {
        let mut app = app();
        press(&mut app, KeyCode::Char('T'));

        for c in "qj?".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Backspace);

        assert!(!app.should_quit);
        assert!(
            matches!(&app.screen, Screen::Setup(SetupStep::EnterPat { input, .. }) if input.as_str() == "qj")
        );
    }

    #[test]
    fn cursor_keys_edit_mid_text_in_setup_input() {
        let mut app = app();
        press(&mut app, KeyCode::Char('T'));
        for c in "ac".chars() {
            press(&mut app, KeyCode::Char(c));
        }

        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Char('b'));
        press(&mut app, KeyCode::Home);
        press(&mut app, KeyCode::Delete);

        assert!(
            matches!(&app.screen, Screen::Setup(SetupStep::EnterPat { input, .. }) if input.split() == ("", "bc"))
        );
    }

    #[test]
    fn cursor_keys_edit_mid_text_in_picker_query_and_refilter() {
        let mut app = app();
        app.popup = Some(Popup::Tags(crate::app::TagPicker::new(
            10,
            Vec::new(),
            vec!["backend".into(), "frontend".into()],
        )));
        for c in "bk".chars() {
            press(&mut app, KeyCode::Char(c));
        }

        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Char('c'));

        let Some(Popup::Tags(picker)) = &app.popup else {
            panic!("tag picker not open");
        };
        assert_eq!(picker.query.as_str(), "back");
        assert_eq!(picker.selection.items, ["backend", "back"]);
    }

    /// Real terminal events: press kind, with lock state or empty modifiers.
    fn real_keys(code: KeyCode) -> [KeyEvent; 2] {
        [
            KeyEvent::new_with_kind_and_state(
                code,
                KeyModifiers::NONE,
                KeyEventKind::Press,
                KeyEventState::NONE,
            ),
            KeyEvent::new_with_kind_and_state(
                code,
                KeyModifiers::NONE,
                KeyEventKind::Press,
                KeyEventState::NUM_LOCK,
            ),
        ]
    }

    fn type_and_move(app: &mut App, left: KeyEvent, home: KeyEvent) {
        for c in "ac".chars() {
            press(app, KeyCode::Char(c));
        }
        app.handle_key(left);
        press(app, KeyCode::Char('b'));
        app.handle_key(home);
        press(app, KeyCode::Char('_'));
    }

    #[test]
    fn real_arrow_events_move_the_cursor_in_every_input() {
        for (left, home) in real_keys(KeyCode::Left)
            .into_iter()
            .zip(real_keys(KeyCode::Home))
        {
            let mut pat = app();
            press(&mut pat, KeyCode::Char('T'));
            type_and_move(&mut pat, left, home);
            assert!(
                matches!(&pat.screen, Screen::Setup(SetupStep::EnterPat { input, .. }) if input.as_str() == "_abc")
            );

            let mut organization = App::new(None, None);
            organization.screen = Screen::Setup(SetupStep::EnterOrganization(TextInput::default()));
            type_and_move(&mut organization, left, home);
            assert!(
                matches!(&organization.screen, Screen::Setup(SetupStep::EnterOrganization(input)) if input.as_str() == "_abc")
            );

            let mut tag = app();
            tag.popup = Some(Popup::Tags(crate::app::TagPicker::new(
                10,
                Vec::new(),
                Vec::new(),
            )));
            type_and_move(&mut tag, left, home);
            assert!(
                matches!(&tag.popup, Some(Popup::Tags(picker)) if picker.query.as_str() == "_abc")
            );

            let mut sprint = app();
            sprint.popup = Some(Popup::Sprint(crate::app::SprintPicker::new(
                Vec::new(),
                None,
            )));
            type_and_move(&mut sprint, left, home);
            assert!(
                matches!(&sprint.popup, Some(Popup::Sprint(picker)) if picker.query.as_str() == "_abc")
            );

            let mut comment = app();
            comment.popup = Some(Popup::Comment {
                work_item_id: 10,
                text: TextInput::default(),
            });
            type_and_move(&mut comment, left, home);
            assert!(
                matches!(&comment.popup, Some(Popup::Comment { text, .. }) if text.as_str() == "_abc")
            );

            let mut filter = app();
            filter.popup = Some(Popup::BuildFilter {
                text: TextInput::default(),
            });
            type_and_move(&mut filter, left, home);
            assert!(
                matches!(&filter.popup, Some(Popup::BuildFilter { text }) if text.as_str() == "_abc")
            );
        }
    }

    #[test]
    fn delete_and_end_work_through_handle_key() {
        let mut app = app();
        app.popup = Some(Popup::Comment {
            work_item_id: 10,
            text: TextInput::new("abc"),
        });

        press(&mut app, KeyCode::Home);
        press(&mut app, KeyCode::Delete);
        press(&mut app, KeyCode::End);
        press(&mut app, KeyCode::Char('d'));

        assert!(matches!(
            &app.popup,
            Some(Popup::Comment { text, .. }) if text.as_str() == "bcd"
        ));
    }

    #[test]
    fn paste_inserts_at_the_cursor() {
        let mut app = app();
        app.popup = Some(Popup::Comment {
            work_item_id: 10,
            text: TextInput::new("ad"),
        });
        press(&mut app, KeyCode::Left);

        app.handle_paste("b\nc");

        assert!(matches!(
            &app.popup,
            Some(Popup::Comment { text, .. }) if text.split() == ("ab\nc", "d")
        ));
    }

    #[test]
    fn paste_into_comment_keeps_lines_and_triggers_no_actions() {
        let mut app = app();
        app.popup = Some(Popup::Comment {
            work_item_id: 10,
            text: TextInput::default(),
        });

        app.handle_paste("# Title\r\n\r\n- q item\r- j item\n```rust\nfn x() {}\n```");

        assert!(!app.should_quit);
        assert!(matches!(
            &app.popup,
            Some(Popup::Comment { text, .. })
                if text.as_str() == "# Title\n\n- q item\n- j item\n```rust\nfn x() {}\n```"
        ));
    }

    #[test]
    fn paste_into_setup_input_takes_first_line() {
        let mut app = app();
        press(&mut app, KeyCode::Char('T'));

        app.handle_paste("token\nq");

        assert!(!app.should_quit);
        assert!(
            matches!(&app.screen, Screen::Setup(SetupStep::EnterPat { input, .. }) if input.as_str() == "token")
        );
    }

    #[test]
    fn paste_without_input_is_ignored() {
        let mut app = app();

        app.handle_paste("q");

        assert!(!app.should_quit);
        assert!(app.popup.is_none());
    }

    #[test]
    fn q_quits_from_main() {
        let mut app = app();

        press(&mut app, KeyCode::Char('q'));

        assert!(app.should_quit);
    }
}

use crossterm::event::{KeyCode, KeyEvent};

use super::{
    Action, App, Detail, DetailTab, LEFT_PANELS, Panel, Popup, Retry, Screen, SetupStep, TextInput,
};

/// A cursor move in a list of rows, which stops at both ends.
#[derive(Debug, Clone, Copy)]
pub(super) enum Move {
    Line { down: bool },
    Top,
    Bottom,
    Page { down: bool, half: bool },
}

impl Move {
    /// The row reached from `current` in `len` rows, with `page` rows visible.
    pub(super) fn target(self, current: usize, len: usize, page: usize) -> usize {
        let last = len.saturating_sub(1);
        let (down, rows) = match self {
            Self::Top => return 0,
            Self::Bottom => return last,
            Self::Line { down } => (down, 1),
            Self::Page { down, half: false } => (down, page.max(1)),
            Self::Page { down, half: true } => (down, (page / 2).max(1)),
        };
        if down {
            current.saturating_add(rows).min(last)
        } else {
            current.min(last).saturating_sub(rows)
        }
    }
}

impl App {
    pub(super) fn handle_key(&mut self, key: KeyEvent) {
        if let Screen::Setup(step) = &mut self.screen
            && let Some(input) = step.input_mut()
            && input.handle_key(key, false)
        {
            return;
        }
        if self.handle_search_key(key) || self.handle_popup_key(key) {
            return;
        }
        if let Some(action) = self.keymap.action(key) {
            self.apply(action);
            if self
                .search
                .as_ref()
                .is_some_and(|(panel, _)| *panel != self.focus)
            {
                self.search = None;
            }
        }
    }

    /// Types into the search of a list or a setup picker; true when the key was taken.
    fn handle_search_key(&mut self, key: KeyEvent) -> bool {
        if let Screen::Setup(step) = &mut self.screen {
            return step
                .selection_mut()
                .is_some_and(|selection| selection.edit_query(key));
        }
        if self.popup.is_some() || self.show_help {
            return false;
        }
        let Some((panel, input)) = &mut self.search else {
            return false;
        };
        let panel = *panel;
        let Some(index) = panel.index() else {
            return false;
        };
        match key.code {
            KeyCode::Esc => {
                self.data.filters[index].clear();
                self.search = None;
            }
            KeyCode::Enter => self.search = None,
            _ if input.handle_key(key, false) => {
                self.data.filters[index] = input.as_str().to_string();
                self.selections[index] = 0;
            }
            _ => return false,
        }
        true
    }

    fn start_search(&mut self) {
        match &mut self.screen {
            Screen::Setup(step) => {
                if let Some(selection) = step.selection_mut() {
                    selection.start_query();
                }
            }
            Screen::Main => {
                if let Some(index) = self.focus.index() {
                    let input = TextInput::new(self.data.filters[index].as_str());
                    self.search = Some((self.focus, input));
                }
            }
        }
    }

    /// Esc in a setup picker first shows all items again.
    fn clear_setup_query(&mut self) -> bool {
        let Screen::Setup(step) = &mut self.screen else {
            return false;
        };
        match step.selection_mut() {
            Some(selection) if selection.query.is_some() => {
                selection.clear_query();
                true
            }
            _ => false,
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
            Action::Cancel if self.clear_setup_query() => {}
            Action::Cancel => match self.screen {
                Screen::Setup(SetupStep::EnterPat {
                    cancelable: true, ..
                }) => self.screen = Screen::Main,
                Screen::Setup(_) => self.go_back(),
                Screen::Main => {
                    if self.zoomed && self.detail.is_none() {
                        self.zoomed = false;
                    } else if !self.close_file_diff() {
                        self.close_detail();
                    }
                }
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
                    let step = Move::Line {
                        down: action == Action::Down,
                    };
                    if !self.jump_file_selection(step, 1) {
                        self.scroll_detail(action == Action::Down);
                    }
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
                Screen::Main if self.focus == Panel::Detail => self.open_file_diff(),
                Screen::Main => self.open_selected(),
                Screen::Setup(_) => self.confirm_setup_step(),
            },
            Action::OpenInBrowser
            | Action::Copy
            | Action::CopyLink
            | Action::NextLink
            | Action::PrevLink
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
            | Action::Vote
            | Action::NextThread
            | Action::PrevThread
            | Action::ThreadStatus
            | Action::PrActions
            | Action::SwitchProject
            | Action::Zoom
            | Action::Top
            | Action::Bottom
            | Action::PageDown
            | Action::PageUp
            | Action::HalfPageDown
            | Action::HalfPageUp
            | Action::JumpToPanel(_)
            | Action::Errors
                if !matches!(self.screen, Screen::Main) => {}
            Action::Search => self.start_search(),
            Action::SwitchProject => self.switch_project(),
            Action::Top => self.move_by(Move::Top),
            Action::Bottom => self.move_by(Move::Bottom),
            Action::PageDown => self.move_by(Move::Page {
                down: true,
                half: false,
            }),
            Action::PageUp => self.move_by(Move::Page {
                down: false,
                half: false,
            }),
            Action::HalfPageDown => self.move_by(Move::Page {
                down: true,
                half: true,
            }),
            Action::HalfPageUp => self.move_by(Move::Page {
                down: false,
                half: true,
            }),
            Action::JumpToPanel(number) => {
                if let Some(panel) = LEFT_PANELS.get(usize::from(number).wrapping_sub(1)) {
                    self.last_left = *panel;
                    self.focus = *panel;
                }
            }
            Action::OpenInBrowser => self.open_in_browser(),
            Action::Copy => self.copy_number(),
            Action::CopyLink => self.copy_link(),
            Action::NextLink | Action::PrevLink => self.select_link(action == Action::NextLink),
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
            Action::Vote => self.open_vote_picker(),
            Action::NextThread | Action::PrevThread => {
                self.move_thread(action == Action::NextThread)
            }
            Action::ThreadStatus => self.open_thread_status(),
            Action::PrActions => self.open_pr_actions(),
            Action::Zoom => self.zoomed = !self.zoomed,
            Action::Errors => self.popup = Some(Popup::Errors { first: 0 }),
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
        if len == 0 {
            return;
        }
        let selected = &mut self.selections[index];
        *selected = if down {
            (*selected + 1) % len
        } else {
            (*selected + len - 1) % len
        };
    }

    /// Jumps within the focused list, or the detail when it has focus; stops at the ends.
    fn move_by(&mut self, step: Move) {
        if self.focus == Panel::Detail {
            let page = usize::from(self.detail_height.get());
            if !self.jump_file_selection(step, page) {
                self.scroll_detail_by(step, page);
            }
        } else if let Some(index) = self.focus.index() {
            let len = self.data.len(self.focus);
            let selected = self.selections[index];
            self.selections[index] =
                step.target(selected, len, usize::from(self.list_height.get()));
        }
    }

    pub(super) fn scroll_detail(&mut self, down: bool) {
        self.scroll_detail_by(Move::Line { down }, 1);
    }

    fn scroll_detail_by(&mut self, step: Move, page: usize) {
        if self.scroll_to_thread {
            self.scroll_to_thread = false;
            self.detail_scroll = self.thread_row.get().unwrap_or(self.detail_scroll);
        }
        let max = self.detail_max_scroll.get();
        let current = usize::from(self.detail_scroll.min(max));
        self.detail_scroll = step.target(current, usize::from(max) + 1, page) as u16;
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
        self.select_tab(tabs[index]);
    }

    pub(super) fn select_tab(&mut self, tab: DetailTab) {
        self.detail_tab = tab;
        self.detail_scroll = 0;
        self.detail_thread = None;
        self.scroll_to_thread = false;
        self.link_index = None;
        if self.detail_tab == DetailTab::Files {
            self.load_files();
        }
    }

    pub(super) fn clamp_selections(&mut self) {
        for (index, panel) in LEFT_PANELS.iter().enumerate() {
            let len = self.data.len(*panel);
            self.selections[index] = self.selections[index].min(len.saturating_sub(1));
        }
    }

    pub(crate) fn handle_paste(&mut self, pasted: &str) {
        let pasted = pasted.replace("\r\n", "\n").replace('\r', "\n");
        if let Some(Popup::Text { text, .. }) = &mut self.popup {
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
    use crate::app::{App, DetailTab, Panel, Popup, Screen, SetupStep, TextInput, TextTarget};
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
    fn selection_wraps_around_the_list() {
        let mut app = app();

        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.selections[1], 2);
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.selections[1], 0);
    }

    #[test]
    fn g_and_shift_g_jump_to_the_ends_of_a_list() {
        let mut app = app();

        press(&mut app, KeyCode::Char('G'));
        assert_eq!(app.selections[1], 2);
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(app.selections[1], 0);
    }

    #[test]
    fn page_keys_move_a_page_and_stop_at_the_ends() {
        let mut app = app();
        app.list_height.set(2);

        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.selections[1], 2);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.selections[1], 2);
        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.selections[1], 0);
        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.selections[1], 0);
    }

    #[test]
    fn ctrl_d_and_u_move_half_a_page_and_do_not_unassign() {
        let mut app = app();
        app.list_height.set(4);

        ctrl(&mut app, 'd');
        assert_eq!(app.selections[1], 2);
        ctrl(&mut app, 'u');
        assert_eq!(app.selections[1], 0);
        assert!(app.popup.is_none());
    }

    #[test]
    fn number_keys_focus_the_numbered_box() {
        let mut app = app();

        press(&mut app, KeyCode::Char('1'));
        assert_eq!(app.focus, Panel::Inbox);
        press(&mut app, KeyCode::Char('6'));
        assert_eq!(app.focus, Panel::Builds);
        press(&mut app, KeyCode::Char('4'));
        assert_eq!(app.focus, Panel::OtherPrs);
        ctrl(&mut app, 'l');
        ctrl(&mut app, 'h');
        assert_eq!(app.focus, Panel::OtherPrs);
    }

    #[test]
    fn detail_jump_keys_scroll_within_the_content() {
        let mut app = app();
        open_pull_request(&mut app);
        ctrl(&mut app, 'l');
        app.detail_max_scroll.set(30);
        app.detail_height.set(10);

        press(&mut app, KeyCode::Char('G'));
        assert_eq!(app.detail_scroll, 30);
        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.detail_scroll, 20);
        ctrl(&mut app, 'u');
        assert_eq!(app.detail_scroll, 15);
        ctrl(&mut app, 'd');
        assert_eq!(app.detail_scroll, 20);
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(app.detail_scroll, 0);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.detail_scroll, 10);
        assert_eq!(app.selections[1], 0);
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
    fn scrolling_detail_stops_at_the_end_of_the_content() {
        let mut app = app();
        open_pull_request(&mut app);
        ctrl(&mut app, 'l');
        app.detail_max_scroll.set(2);

        for _ in 0..10 {
            press(&mut app, KeyCode::Char('j'));
        }
        assert_eq!(app.detail_scroll, 2);

        press(&mut app, KeyCode::Char('k'));
        assert_eq!(app.detail_scroll, 1);
    }

    #[test]
    fn scrolling_up_from_beyond_the_end_moves_immediately() {
        let mut app = app();
        open_pull_request(&mut app);
        ctrl(&mut app, 'l');
        app.detail_scroll = 9;
        app.detail_max_scroll.set(4);

        press(&mut app, KeyCode::Char('k'));

        assert_eq!(app.detail_scroll, 3);
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
        assert_eq!(app.detail_tab, DetailTab::Files);
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.detail_tab, DetailTab::Overview);
        press(&mut app, KeyCode::BackTab);
        assert_eq!(app.detail_tab, DetailTab::Files);
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
            comment.popup = Some(Popup::Text {
                target: TextTarget::WorkItemComment(10),
                text: TextInput::default(),
            });
            type_and_move(&mut comment, left, home);
            assert!(
                matches!(&comment.popup, Some(Popup::Text { text, .. }) if text.as_str() == "_abc")
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
        app.popup = Some(Popup::Text {
            target: TextTarget::WorkItemComment(10),
            text: TextInput::new("abc"),
        });

        press(&mut app, KeyCode::Home);
        press(&mut app, KeyCode::Delete);
        press(&mut app, KeyCode::End);
        press(&mut app, KeyCode::Char('d'));

        assert!(matches!(
            &app.popup,
            Some(Popup::Text { text, .. }) if text.as_str() == "bcd"
        ));
    }

    #[test]
    fn paste_inserts_at_the_cursor() {
        let mut app = app();
        app.popup = Some(Popup::Text {
            target: TextTarget::WorkItemComment(10),
            text: TextInput::new("ad"),
        });
        press(&mut app, KeyCode::Left);

        app.handle_paste("b\nc");

        assert!(matches!(
            &app.popup,
            Some(Popup::Text { text, .. }) if text.split() == ("ab\nc", "d")
        ));
    }

    #[test]
    fn paste_into_comment_keeps_lines_and_triggers_no_actions() {
        let mut app = app();
        app.popup = Some(Popup::Text {
            target: TextTarget::WorkItemComment(10),
            text: TextInput::default(),
        });

        app.handle_paste("# Title\r\n\r\n- q item\r- j item\n```rust\nfn x() {}\n```");

        assert!(!app.should_quit);
        assert!(matches!(
            &app.popup,
            Some(Popup::Text { text, .. })
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
    fn z_toggles_zoom_in_main_only() {
        let mut app = app();

        press(&mut app, KeyCode::Char('z'));
        assert!(app.zoomed);
        press(&mut app, KeyCode::Char('z'));
        assert!(!app.zoomed);

        let mut setup = App::new(None, None);
        press(&mut setup, KeyCode::Char('z'));
        assert!(!setup.zoomed);
    }

    #[test]
    fn zoom_follows_focus() {
        let mut app = app();
        press(&mut app, KeyCode::Char('z'));

        ctrl(&mut app, 'j');
        assert_eq!(app.left_focus(), Panel::MyWorkItems);
        ctrl(&mut app, 'l');

        assert!(app.zoomed);
        assert_eq!(app.focus, Panel::Detail);
        assert_eq!(app.left_focus(), Panel::MyWorkItems);
    }

    #[test]
    fn esc_closes_the_detail_before_unzooming() {
        let mut app = app();
        open_pull_request(&mut app);
        press(&mut app, KeyCode::Char('z'));

        press(&mut app, KeyCode::Esc);
        assert!(app.detail.is_none());
        assert!(app.zoomed);

        press(&mut app, KeyCode::Esc);
        assert!(!app.zoomed);
        assert!(!app.should_quit);
    }

    fn type_keys(app: &mut App, text: &str) {
        for c in text.chars() {
            press(app, KeyCode::Char(c));
        }
    }

    #[test]
    fn slash_filters_the_focused_list_live_and_takes_letters_as_text() {
        let mut app = app();
        app.selections[1] = 2;

        press(&mut app, KeyCode::Char('/'));
        type_keys(&mut app, "sec");

        assert!(!app.should_quit);
        assert_eq!(app.data.len(Panel::MyPrs), 1);
        assert_eq!(app.data.filters[1], "sec");
        assert_eq!(app.selections[1], 0);
        assert_eq!(app.data.len(Panel::OtherPrs), 1);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.data.filters[1], "se");
    }

    #[test]
    fn enter_keeps_the_filter_and_esc_clears_it() {
        let mut app = app();
        press(&mut app, KeyCode::Char('/'));
        type_keys(&mut app, "third");

        press(&mut app, KeyCode::Enter);
        assert!(app.search.is_none());
        assert_eq!(app.data.len(Panel::MyPrs), 1);
        assert!(app.detail.is_none());

        press(&mut app, KeyCode::Char('/'));
        assert!(matches!(&app.search, Some((_, input)) if input.as_str() == "third"));
        press(&mut app, KeyCode::Esc);

        assert!(app.search.is_none());
        assert_eq!(app.data.len(Panel::MyPrs), 3);
    }

    #[test]
    fn moving_focus_ends_the_search_but_keeps_the_filter() {
        let mut app = app();
        press(&mut app, KeyCode::Char('/'));
        type_keys(&mut app, "first");

        ctrl(&mut app, 'j');

        assert!(app.search.is_none());
        assert_eq!(app.data.len(Panel::MyPrs), 1);
        assert_eq!(app.focus, Panel::MyWorkItems);
    }

    #[test]
    fn slash_in_the_detail_does_nothing() {
        let mut app = app();
        open_pull_request(&mut app);
        ctrl(&mut app, 'l');

        press(&mut app, KeyCode::Char('/'));

        assert!(app.search.is_none());
    }

    #[test]
    fn slash_filters_a_setup_list_and_esc_clears_before_going_back() {
        let mut app = App::new(None, None);
        app.handle_message(Message::Organizations(Ok(vec![
            "alpha".into(),
            "beta".into(),
        ])));

        press(&mut app, KeyCode::Char('/'));
        type_keys(&mut app, "bet");
        let Screen::Setup(step) = &app.screen else {
            panic!("not in setup");
        };
        assert_eq!(step.selection().unwrap().visible(), ["beta"]);
        assert!(!app.should_quit);

        press(&mut app, KeyCode::Esc);
        let Screen::Setup(step) = &app.screen else {
            panic!("not in setup");
        };
        let selection = step.selection().unwrap();
        assert!(selection.query.is_none());
        assert_eq!(selection.current(), Some("beta"));
        assert!(!app.should_quit);
    }

    #[test]
    fn q_quits_from_main() {
        let mut app = app();

        press(&mut app, KeyCode::Char('q'));

        assert!(app.should_quit);
    }
}

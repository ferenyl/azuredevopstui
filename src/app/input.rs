use super::{Action, App, Detail, LEFT_PANELS, Panel, Popup, Retry, Screen, SetupStep};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

impl App {
    pub(super) fn handle_key(&mut self, key: KeyEvent) {
        if let Screen::Setup(step) = &mut self.screen
            && let Some(input) = step.input_mut()
        {
            match key.code {
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.push(c);
                    return;
                }
                KeyCode::Backspace => {
                    input.pop();
                    return;
                }
                _ => {}
            }
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
                    input: String::new(),
                    error: None,
                    cancelable: true,
                }),
                Screen::Setup(SetupStep::Failed {
                    retry: Retry::Authenticate,
                    ..
                }) if self.can_enter_pat() => self.set_step(SetupStep::EnterPat {
                    input: String::new(),
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
            text.push_str(&pasted);
            return;
        }
        let line = pasted.lines().next().unwrap_or_default();
        match &mut self.popup {
            Some(Popup::BuildFilter { text }) => text.push_str(line),
            Some(Popup::Tags(picker)) => {
                picker.query.push_str(line);
                picker.filter();
            }
            Some(Popup::Sprint(picker)) => {
                picker.query.push_str(line);
                picker.filter();
            }
            _ => {
                if let Screen::Setup(step) = &mut self.screen
                    && let Some(input) = step.input_mut()
                {
                    input.push_str(line);
                }
            }
        }
    }
}

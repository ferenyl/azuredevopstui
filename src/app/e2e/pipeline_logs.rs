use crossterm::event::KeyCode;

use super::Harness;
use super::flows::connected;
use crate::app::{DetailInfo, DetailTab, Panel};

const LOG_7: &str = "/contoso/Fabrikam/_apis/build/builds/501/logs/7";

/// Opens the failed run 501 and switches to its Timeline tab.
async fn on_timeline(harness: &mut Harness) {
    let index = Panel::Builds.index().expect("left panel");
    harness.app.focus = Panel::Builds;
    harness.app.selections[index] = 0;
    harness.press(KeyCode::Enter);
    harness
        .wait_for(|app| matches!(app.detail_info, Some(DetailInfo::Build(_))))
        .await;
    harness.press(KeyCode::Tab);
}

#[tokio::test]
async fn the_overview_of_a_failed_run_points_at_the_timeline() {
    let mut harness = connected().await;
    harness.app.focus = Panel::Builds;
    harness.press(KeyCode::Enter);
    harness.wait_for_text("tab: timeline and logs").await;
}

#[tokio::test]
async fn the_timeline_shows_stages_jobs_and_tasks_with_the_failed_one_marked() {
    let mut harness = connected().await;

    on_timeline(&mut harness).await;

    assert_eq!(harness.app.detail_tab, DetailTab::Timeline);
    let screen = harness.screen();
    let row = |text: &str| {
        screen
            .lines()
            .find(|line| line.contains(text))
            .unwrap_or_else(|| panic!("no {text:?} in\n{screen}"))
            .to_string()
    };
    assert!(screen.contains("Timeline"), "{screen}");
    let (stage, job) = (row("Continuous integration"), row("Build job"));
    let (install, test) = (row("npm install"), row("npm test"));
    assert!(install.contains('✔'), "{install}");
    assert!(test.contains('✖') && test.contains("(2 errors)"), "{test}");
    assert!(screen.contains("Cannot find module 'orders'"), "{screen}");
    // The phase between stage and job is folded away.
    assert_eq!(screen.matches("Build job").count(), 1, "{screen}");
    let column = |line: &str, text: &str| line[..line.find(text).unwrap()].chars().count();
    assert!(column(&job, "Build job") > column(&stage, "Continuous integration"));
    assert!(column(&test, "npm test") > column(&job, "Build job"));
}

#[tokio::test]
async fn enter_on_the_failed_task_shows_its_log_and_esc_goes_back() {
    let mut harness = connected().await;
    on_timeline(&mut harness).await;

    harness.ctrl('l');
    harness.press(KeyCode::Enter);
    harness.wait_for_text("Starting: npm test").await;

    assert_eq!(harness.ado.received("GET", "/builds/501/logs/7").await, 1);
    let screen = harness.screen();
    assert!(
        screen.contains("##[error]Cannot find module 'orders'"),
        "{screen}"
    );
    assert!(screen.contains("##[warning]deprecated flag"), "{screen}");
    assert!(screen.contains("npm ERR! code ELIFECYCLE"), "{screen}");
    assert!(!screen.contains("2026-10-20T09:31"), "{screen}");
    assert!(!screen.contains('\u{1b}'));

    harness.press(KeyCode::Esc);

    let screen = harness.screen();
    assert!(screen.contains("Continuous integration"), "{screen}");
    assert!(!screen.contains("Starting: npm test"), "{screen}");
    assert!(harness.app.detail.is_some());
}

#[tokio::test]
async fn g_shows_the_end_of_a_long_log() {
    let mut harness = connected().await;
    on_timeline(&mut harness).await;
    harness.ctrl('l');
    harness.press(KeyCode::Enter);
    harness.wait_for_text("Starting: npm test").await;

    // Scrolling needs the size of the pane from a frame.
    harness.screen_at(120, 9);
    harness.press(KeyCode::Char('G'));

    let screen = harness.screen_at(120, 9);
    assert!(screen.contains("Finishing: npm test"), "{screen}");
    assert!(!screen.contains("Starting: npm test"), "{screen}");
    harness.press(KeyCode::Char('g'));
    let screen = harness.screen_at(120, 9);
    assert!(screen.contains("Starting: npm test"), "{screen}");
}

#[tokio::test]
async fn a_log_that_fails_to_load_reports_and_stays_on_the_timeline() {
    let mut harness = connected().await;
    harness.ado.fail("GET", LOG_7, 404).await;
    on_timeline(&mut harness).await;
    harness.ctrl('l');

    harness.press(KeyCode::Enter);
    harness.wait_for(|app| app.error.is_some()).await;

    assert!(harness.app.log.is_none());
    assert!(harness.screen().contains("npm test"));
}

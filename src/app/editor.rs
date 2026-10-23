use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context, Result, bail};

use super::{App, Popup, TextInput};

static EDIT_COUNTER: AtomicU32 = AtomicU32::new(0);

/// The configured editor, else `$VISUAL`, `$EDITOR`, or what the platform has.
fn editor_command(configured: Option<&str>, visual: Option<&str>, editor: Option<&str>) -> String {
    [configured, visual, editor]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|command| !command.is_empty())
        .map_or_else(
            || if cfg!(windows) { "notepad" } else { "vi" }.to_string(),
            str::to_string,
        )
}

/// Lets `command` (split on spaces) edit `text` in a temporary file and returns the result.
pub(super) fn edit_externally(command: &str, text: &str) -> Result<String> {
    let mut parts = command.split_whitespace();
    let program = parts.next().context("no editor configured")?;
    let name = format!(
        "azuredevopstui-{}-{}.md",
        std::process::id(),
        EDIT_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let path = std::env::temp_dir().join(name);
    fs::write(&path, text).with_context(|| format!("failed to write {}", path.display()))?;
    let result = Command::new(program)
        .args(parts)
        .arg(&path)
        .status()
        .with_context(|| format!("failed to run {program}"))
        .and_then(|status| {
            if !status.success() {
                bail!("{program} exited with {status}");
            }
            fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))
        });
    let _ = fs::remove_file(&path);
    let edited = result?.replace("\r\n", "\n");
    Ok(edited.strip_suffix('\n').unwrap_or(&edited).to_string())
}

impl App {
    /// Edits `text` in the editor; the terminal must not be in use by the TUI meanwhile.
    pub(super) fn edit_in_editor(&mut self, text: &str) {
        let configured = self.config.as_ref().and_then(|c| c.editor.as_deref());
        let command = editor_command(
            configured,
            std::env::var("VISUAL").ok().as_deref(),
            std::env::var("EDITOR").ok().as_deref(),
        );
        match edit_externally(&command, text) {
            Ok(edited) => {
                if let Some(Popup::Text { text, .. }) = &mut self.popup {
                    *text = TextInput::new(edited);
                }
            }
            Err(err) => self.set_error(format!("editor failed: {err:#}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_command_prefers_config_then_visual_then_editor() {
        assert_eq!(editor_command(Some("hx"), Some("vim"), Some("nano")), "hx");
        assert_eq!(editor_command(None, Some("vim"), Some("nano")), "vim");
        assert_eq!(editor_command(Some(" "), None, Some("nano")), "nano");
        assert_eq!(
            editor_command(None, Some(""), None),
            if cfg!(windows) { "notepad" } else { "vi" }
        );
    }

    #[test]
    fn empty_command_is_an_error() {
        assert!(edit_externally("  ", "text").is_err());
    }

    #[cfg(unix)]
    mod unix {
        use std::os::unix::fs::PermissionsExt;

        use super::*;

        /// An executable that runs `body` with the file to edit as `$1`.
        fn script(name: &str, body: &str) -> String {
            let path = std::env::temp_dir().join(format!(
                "azuredevopstui-test-{}-{name}.sh",
                std::process::id()
            ));
            fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path.display().to_string()
        }

        #[test]
        fn returns_what_the_editor_wrote_without_the_final_newline() {
            let editor = script("write", r#"printf 'edited\nlines\n' > "$1""#);

            let edited = edit_externally(&editor, "original").unwrap();

            assert_eq!(edited, "edited\nlines");
        }

        #[test]
        fn passes_the_text_and_keeps_it_when_the_editor_changes_nothing() {
            let editor = script("keep", "true");

            assert_eq!(edit_externally(&editor, "a\n\nb").unwrap(), "a\n\nb");
        }

        #[test]
        fn failing_editor_is_an_error_and_removes_the_file() {
            let editor = script("fail", r#"echo "$1" > "$0.path"; exit 3"#);

            let err = edit_externally(&editor, "text").unwrap_err();

            assert!(err.to_string().contains("exited with"), "{err:#}");
            let recorded = fs::read_to_string(format!("{editor}.path")).unwrap();
            assert!(!std::path::Path::new(recorded.trim()).exists());
        }

        #[test]
        fn missing_editor_is_an_error() {
            let err = edit_externally("/nonexistent/editor", "text").unwrap_err();

            assert!(err.to_string().contains("failed to run"), "{err:#}");
        }
    }
}

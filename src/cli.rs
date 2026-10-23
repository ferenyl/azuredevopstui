use std::path::PathBuf;

use clap::{Parser, Subcommand};

const CONFIG_HELP: &str = "\
The config defaults to azuredevopstui/config.json in the user config directory:
~/.config on Linux, ~/Library/Application Support on macOS, %APPDATA% on Windows.";

/// Terminal UI for your Azure DevOps pull requests, work items and sprint board.
#[derive(Debug, Parser)]
#[command(name = "azuredevopstui", version, after_help = CONFIG_HELP)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    /// Use this config file instead of the default one
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,
    /// Switch to this organization (saved to the config like the in-app switch)
    #[arg(long = "org", value_name = "NAME")]
    pub organization: Option<String>,
    /// Switch to this project; without --org in the configured organization
    #[arg(long, value_name = "NAME")]
    pub project: Option<String>,
    /// Send a test desktop notification and print whether it was opened or closed
    #[arg(long)]
    pub test_notification: bool,
}

#[derive(Debug, PartialEq, Eq, Subcommand)]
pub enum Command {
    /// Check the config, the sign-in and the connection to Azure DevOps, then exit
    Doctor,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use clap::error::ErrorKind;

    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("azuredevopstui").chain(args.iter().copied()))
    }

    #[test]
    fn definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn no_arguments_runs_the_app() {
        let cli = parse(&[]).unwrap();

        assert_eq!(cli.command, None);
        assert_eq!(cli.config, None);
        assert_eq!(cli.organization, None);
        assert_eq!(cli.project, None);
        assert!(!cli.test_notification);
    }

    #[test]
    fn options_take_separate_or_equals_values() {
        let separate = parse(&["--config", "a.json", "--org", "o", "--project", "p"]).unwrap();
        let equals = parse(&["--config=a.json", "--org=o", "--project=p"]).unwrap();

        for cli in [separate, equals] {
            assert_eq!(cli.config, Some(PathBuf::from("a.json")));
            assert_eq!(cli.organization.as_deref(), Some("o"));
            assert_eq!(cli.project.as_deref(), Some("p"));
        }
    }

    #[test]
    fn doctor_takes_a_config() {
        let before = parse(&["--config", "a.json", "doctor"]).unwrap();
        let after = parse(&["doctor", "--config", "a.json"]).unwrap();

        for cli in [before, after] {
            assert_eq!(cli.command, Some(Command::Doctor));
            assert_eq!(cli.config, Some(PathBuf::from("a.json")));
        }
    }

    #[test]
    fn test_notification_is_a_flag() {
        assert!(parse(&["--test-notification"]).unwrap().test_notification);
    }

    #[test]
    fn help_and_version_stop_parsing() {
        for (args, kind) in [
            (["--help"], ErrorKind::DisplayHelp),
            (["-h"], ErrorKind::DisplayHelp),
            (["--version"], ErrorKind::DisplayVersion),
            (["-V"], ErrorKind::DisplayVersion),
        ] {
            assert_eq!(parse(&args).unwrap_err().kind(), kind);
        }
    }

    #[test]
    fn version_prints_the_crate_version() {
        let version = Cli::command().render_version();

        assert_eq!(
            version.trim(),
            format!("azuredevopstui {}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn unknown_or_incomplete_arguments_are_errors() {
        for args in [&["--bogus"][..], &["--config"], &["--org"], &["bogus"]] {
            let err = parse(args).unwrap_err();

            assert_eq!(err.exit_code(), 2, "{args:?}");
        }
    }

    #[test]
    fn readme_lists_every_flag_and_command() {
        let readme = include_str!("../README.md");
        let command = Cli::command();

        for long in command.get_arguments().filter_map(|arg| arg.get_long()) {
            assert!(
                readme.contains(&format!("--{long}")),
                "--{long} missing in README"
            );
        }
        for sub in command.get_subcommands() {
            assert!(
                readme.contains(&format!("azuredevopstui {}", sub.get_name())),
                "{} missing in README",
                sub.get_name()
            );
        }
    }
}

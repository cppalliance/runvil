use std::{ffi::OsString, path::PathBuf};

use clap::{Parser, Subcommand};
use gateway::{ProfileName, ServeOptions};

/// The parsed app options: the serve options plus how the main thread runs.
#[derive(Debug)]
pub(crate) struct AppOptions {
    /// What the launch does.
    pub command: LaunchCommand,

    /// What to serve. Under [`LaunchCommand::Diagnostics`] only the config path
    /// is meaningful: the report names it.
    pub serve: ServeOptions,

    /// Whether the system tray occupies the main thread (default).
    /// `--no-tray` keeps the headless Ctrl-C loop for servers and CI.
    pub tray: bool,

    /// Whether the launch came from the OS autostart entry (`--login`).
    pub login: bool,

    /// Whether to print the Settings handoff URL to stdout (`--print-url`).
    /// Implies the headless loop: the flag exists for tray-less
    /// environments.
    pub print_url: bool,
}

impl AppOptions {
    /// Constructs an [`AppOptions`] from command line args.
    ///
    /// Config paths are read as `OsString`, so non-UTF-8 paths survive.
    /// The config path
    /// (`--config PATH`, falling back to `RUNVIL_GATEWAY_CONFIG`) stays
    /// optional: with neither set, the gateway discovers or generates the
    /// boot config itself.
    /// `--profile NAME` is validated into a
    /// [`ProfileName`] at parse time.
    ///
    /// `--help` and `--version` also arrive as a [`clap::Error`]; its
    /// `use_stderr()` is false for them.
    pub(crate) fn from_args(args: impl IntoIterator<Item = OsString>) -> Result<Self, clap::Error> {
        let cli = Cli::try_parse_from(args)?;
        let env = std::env::var_os("RUNVIL_GATEWAY_CONFIG");

        let config_path = resolve_config_path(cli.config, env);

        Ok(match cli.command {
            Some(SubCommand::Diagnostics) => Self {
                command: LaunchCommand::Diagnostics,
                serve: ServeOptions::new(config_path, None),
                tray: false,
                login: false,
                print_url: false,
            },

            None => Self {
                command: LaunchCommand::Serve,
                // `--login`'s contract is absolute - a login launch never opens a
                // browser - so it wins over `--browser`.
                serve: ServeOptions::new(config_path, cli.profile)
                    .with_browser(cli.browser && !cli.login)
                    .with_supervised(cli.supervised),
                tray: !cli.no_tray,
                login: cli.login,
                print_url: cli.print_url,
            },
        })
    }
}

/// What this launch does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaunchCommand {
    /// Serve (the default and only serving mode).
    Serve,

    /// Prints the diagnostics report and exits.
    Diagnostics,
}

/// The command line as clap reads it
#[derive(Debug, Parser)]
#[command(
    name = "runvil-gateway",
    version,
    about = "Runvil inference gateway: routes OpenAI-shaped chat completions to a backend",
    after_help = r#"The gateway reads the first config it finds, in the following order.
    --config <PATH>
    RUNVIL_GATEWAY_CONFIG environment variable
    The directory containing the executable
    The current directory
    The user's home .runvil directory (~/.runvil or %USERPROFILE%\.runvil on Windows)
If none is found, a default config is generated in the .runvil directory."#,
    // `diagnostics` must come first: `--no-tray diagnostics` is a usage error, not a subcommand.
    args_conflicts_with_subcommands = true,
    disable_help_subcommand = true
)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is an independent command-line flag"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<SubCommand>,

    /// The config file
    #[arg(long, value_name = "PATH", global = true)]
    config: Option<PathBuf>,

    /// The profile to serve
    #[arg(long, value_name = "NAME", value_parser = ProfileName::parse)]
    profile: Option<ProfileName>,

    /// Run headless (Ctrl-C driven); for servers and CI
    #[arg(long)]
    no_tray: bool,

    /// The launch came from the OS autostart entry; never opens a browser
    #[arg(long)]
    login: bool,

    /// Print the Settings handoff URL once bound, then serve headless; with a
    /// gateway already running, print its URL instead
    #[arg(long)]
    print_url: bool,

    /// Open the Settings page in the default browser once bound; the
    /// installer's first run uses this
    #[arg(long)]
    browser: bool,

    /// The launch is a supervised child; internal only
    #[arg(long, hide = true, conflicts_with = "print_url")]
    supervised: bool,
}

#[derive(Debug, Subcommand)]
enum SubCommand {
    /// Print a JSON report of the state dir, config, logs, and gateway
    /// discovery file. Does not serve, rotate logs, or parse the config
    Diagnostics,
}

/// Resolves the config path: the `--config` flag wins, then the
/// `RUNVIL_GATEWAY_CONFIG` environment variable - but only when it
/// names an existing file. A stale env var warns and falls through to boot
/// discovery: ambient state rots in ways a typed CLI path does not, and a
/// forgotten variable must not hard-fail a first-run boot. A `--config`
/// path is deliberate, so a missing file there stays an error
/// downstream.
///
/// Tests pass both sources explicitly and never touch the process
/// environment (edition 2024 makes `set_var` unsafe); the existence check
/// touches only the paths the test itself creates.
fn resolve_config_path(from_cli: Option<PathBuf>, from_env: Option<OsString>) -> Option<PathBuf> {
    from_cli.or_else(|| {
        let path = PathBuf::from(from_env?);
        if path.is_file() {
            return Some(path);
        }
        tracing::warn!(
            path = %path.display(),
            "RUNVIL_GATEWAY_CONFIG names no file; falling back to discovery"
        );
        None
    })
}

#[cfg(test)]
#[path = "cli-tests.rs"]
mod tests;

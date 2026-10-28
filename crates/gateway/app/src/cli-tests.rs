//! Tests for the binary's command-line parsing.

use clap::CommandFactory as _;
use clap::error::ErrorKind;

use super::*;

fn args(items: &[&str]) -> Vec<OsString> {
    std::iter::once("runvil-gateway")
        .chain(items.iter().copied())
        .map(OsString::from)
        .collect()
}

/// The kind of the parse error for `items`; panics if they parse.
fn error_kind(items: &[&str]) -> ErrorKind {
    parse_args(args(items))
        .expect_err("the arguments are rejected")
        .kind()
}

#[test]
fn the_command_definition_is_consistent() {
    Cli::command().debug_assert();
}

#[test]
fn cli_path_wins_over_env() {
    let path = resolve_config_path(
        Some(PathBuf::from("cli.toml")),
        Some(OsString::from("env.toml")),
    );
    assert_eq!(path, Some(PathBuf::from("cli.toml")));
}

#[test]
fn env_path_is_the_fallback() {
    let file = tempfile::NamedTempFile::new().expect("temp config");
    let path = resolve_config_path(None, Some(file.path().as_os_str().to_os_string()));
    assert_eq!(path, Some(file.path().to_path_buf()));
}

#[test]
fn a_stale_env_path_falls_back_to_discovery() {
    let missing = PathBuf::from("definitely-not-here-env.toml");
    let path = resolve_config_path(None, Some(missing.into_os_string()));
    assert_eq!(
        path, None,
        "a stale env var warns and defers to boot discovery"
    );
}

#[test]
fn neither_path_set_defers_to_boot_discovery() {
    let path = resolve_config_path(None, None);
    assert_eq!(path, None, "the gateway discovers or generates the config");
}

#[test]
fn the_root_invocation_serves_with_discovery() {
    let invocation = parse_args(args(&[])).expect("the bare invocation parses");
    assert_eq!(
        invocation.serve.config_path, None,
        "no --config defers to boot discovery"
    );
    assert!(invocation.serve.profile.is_none());
    assert!(invocation.tray, "the tray is the default main loop");
    assert!(!invocation.login);
    assert!(!invocation.print_url);
    assert!(
        !invocation.serve.browser,
        "embedders and ordinary launches never open a browser"
    );
}

#[test]
fn the_serve_verb_is_rejected() {
    assert_eq!(
        error_kind(&["serve"]),
        ErrorKind::InvalidSubcommand,
        "the removed subcommand is a usage error, never an alias"
    );
}

#[test]
fn a_positional_config_path_is_rejected() {
    assert_eq!(
        error_kind(&["gateway.toml"]),
        ErrorKind::InvalidSubcommand,
        "the config path is --config PATH, never a positional"
    );
}

#[test]
fn the_config_flag_sets_the_path() {
    let invocation = parse_args(args(&["--config", "gateway.toml"])).expect("parse");
    assert_eq!(
        invocation.serve.config_path,
        Some(PathBuf::from("gateway.toml"))
    );
}

#[test]
fn the_config_flag_requires_a_value() {
    assert_eq!(error_kind(&["--config"]), ErrorKind::InvalidValue);
}

#[test]
fn the_config_flag_is_given_once() {
    assert_eq!(
        error_kind(&["--config", "a.toml", "--config", "b.toml"]),
        ErrorKind::ArgumentConflict
    );
}

#[test]
fn parses_path_and_profile() {
    let invocation =
        parse_args(args(&["--config", "gateway.toml", "--profile", "dev"])).expect("parse");
    assert_eq!(
        invocation.serve.profile.as_ref().map(ProfileName::as_str),
        Some("dev")
    );
    assert_eq!(
        invocation.serve.config_path,
        Some(PathBuf::from("gateway.toml"))
    );
}

#[test]
fn the_tray_is_default_and_login_is_off() {
    let invocation = parse_args(args(&["--config", "gateway.toml"])).expect("parse");
    assert!(invocation.tray, "the tray is the default main loop");
    assert!(!invocation.login);
}

#[test]
fn no_tray_selects_the_headless_loop() {
    let invocation = parse_args(args(&["--no-tray"])).expect("parse");
    assert!(!invocation.tray);
    assert!(!invocation.login);
}

#[test]
fn the_autostart_command_line_parses() {
    // The Run-key entry is `"<exe>" --login`; a login launch must
    // never fail on its own command line.
    let invocation = parse_args(args(&["--login"])).expect("parse");
    assert!(invocation.login);
    assert!(invocation.tray, "a login launch still shows the tray");
}

#[test]
fn print_url_parses_and_leaves_the_other_flags_alone() {
    let invocation = parse_args(args(&["--print-url"])).expect("parse");
    assert!(invocation.print_url);
    assert!(
        invocation.tray,
        "the flag is independent; the dispatch makes it headless"
    );
    assert!(!invocation.login);
}

#[test]
fn print_url_combines_with_no_tray_and_a_config_path() {
    let invocation = parse_args(args(&[
        "--config",
        "gateway.toml",
        "--no-tray",
        "--print-url",
    ]))
    .expect("parse");
    assert!(invocation.print_url);
    assert!(!invocation.tray);
    assert_eq!(
        invocation.serve.config_path,
        Some(PathBuf::from("gateway.toml"))
    );
}

#[test]
fn browser_parses_and_reaches_the_serve_options() {
    let invocation = parse_args(args(&["--browser"])).expect("parse");
    assert!(
        invocation.serve.browser,
        "the flag reaches the spawn hook through ServeOptions"
    );
    assert!(invocation.tray, "the flag is independent of the run loop");
}

#[test]
fn login_wins_over_browser() {
    let invocation = parse_args(args(&["--login", "--browser"])).expect("parse");
    assert!(
        !invocation.serve.browser,
        "a login launch never opens a browser"
    );
}

#[test]
fn missing_profile_defers_to_environment_or_state() {
    let invocation = parse_args(args(&["--config", "gateway.toml"])).expect("parse");
    assert!(invocation.serve.profile.is_none());
}

#[test]
fn invalid_profile_name_is_a_usage_error() {
    assert_eq!(
        error_kind(&["--config", "gateway.toml", "--profile", ""]),
        ErrorKind::ValueValidation
    );
}

#[test]
fn rejects_traversal_profile_name() {
    assert_eq!(
        error_kind(&["--config", "gateway.toml", "--profile", "../escape"]),
        ErrorKind::ValueValidation
    );
}

#[test]
fn rejects_an_unknown_argument() {
    assert_eq!(error_kind(&["frobnicate"]), ErrorKind::InvalidSubcommand);
}

#[test]
fn help_is_recognized() {
    for flag in ["--help", "-h"] {
        let error = parse_args(args(&[flag])).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::DisplayHelp, "{flag}");
        assert!(!error.use_stderr(), "{flag} prints to stdout and succeeds");
    }
}

#[test]
fn version_is_recognized() {
    for flag in ["--version", "-V"] {
        let error = parse_args(args(&[flag])).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::DisplayVersion, "{flag}");
        assert!(!error.use_stderr(), "{flag} prints to stdout and succeeds");
    }
}

#[test]
fn rejects_unknown_flag() {
    assert_eq!(
        error_kind(&["--profiles-dir", "x", "--profile", "dev"]),
        ErrorKind::UnknownArgument
    );
}

#[test]
fn diagnostics_is_the_only_subcommand() {
    let invocation = parse_args(args(&["diagnostics"])).expect("parses");
    assert_eq!(invocation.command, LaunchCommand::Diagnostics);
    assert_eq!(invocation.serve.config_path, None);
    let invocation = parse_args(args(&[])).expect("the bare invocation parses");
    assert_eq!(invocation.command, LaunchCommand::Serve);
}

#[test]
fn diagnostics_accepts_a_config_path() {
    let invocation =
        parse_args(args(&["diagnostics", "--config", "gateway.toml"])).expect("parses");
    assert_eq!(invocation.command, LaunchCommand::Diagnostics);
    assert_eq!(
        invocation.serve.config_path,
        Some(PathBuf::from("gateway.toml"))
    );
}

#[test]
fn diagnostics_rejects_serving_flags() {
    for rest in ["--no-tray", "--login", "--print-url", "--browser"] {
        assert_eq!(
            error_kind(&["diagnostics", rest]),
            ErrorKind::UnknownArgument,
            "diagnostics rejects {rest}"
        );
    }
}

#[test]
fn diagnostics_after_a_flag_is_not_a_subcommand() {
    assert_eq!(
        error_kind(&["--no-tray", "diagnostics"]),
        ErrorKind::ArgumentConflict,
        "the subcommand must come first"
    );
}

#[test]
fn supervised_option_parses_and_reaches_the_serve_options() {
    let invocation = parse_args(args(&["--supervised", "--no-tray"])).expect("should parse");

    assert!(invocation.serve.supervised);
    assert!(
        !invocation.tray,
        "--supervised leaves the tray choice to --no-tray"
    );
}

#[test]
fn supervised_option_does_not_override_no_tray_option() {
    let invocation = parse_args(args(&["--supervised"])).expect("should parse");

    assert!(invocation.tray, "--supervised should not set tray to false");

    let invocation = parse_args(args(&["--supervised", "--no-tray"])).expect("should parse");

    assert!(
        !invocation.tray,
        "--supervised should not override --no-tray"
    );
}

#[test]
fn a_launch_is_not_supervised_by_default() {
    let invocation = parse_args(args(&[])).expect("should parse");

    assert!(!invocation.serve.supervised);
}

#[test]
fn supervised_and_print_url_options_cannot_be_used_together() {
    let print_url = "--print-url";
    let kind = error_kind(&["--supervised", print_url]);

    assert_eq!(
        kind,
        ErrorKind::ArgumentConflict,
        "--supervised {print_url} cannot both be set"
    );
}

#[test]
fn diagnostics_has_its_own_help() {
    let error = parse_args(args(&["diagnostics", "--help"])).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    let help = error.to_string();
    assert!(help.contains("--config"), "{help}");
    assert!(!help.contains("--no-tray"), "{help}");
}

#[test]
fn the_help_text_hides_the_supervised_flag() {
    let help = Cli::command().render_long_help().to_string();
    assert!(!help.contains("--supervised"), "{help}");
}

#[test]
fn the_help_text_lists_all_flags() {
    let help = Cli::command().render_long_help().to_string();
    for flag in [
        "--config",
        "--profile",
        "--no-tray",
        "--login",
        "--print-url",
        "--browser",
        "diagnostics",
    ] {
        assert!(help.contains(flag), "the help names {flag}: {help}");
    }
}

#[test]
fn preferred_port_parses_and_reaches_the_serve_options() {
    let options = parse_args(args(&["--preferred-port", "8081"])).expect("should parse");

    assert_eq!(options.serve.preferred_port, Some(8081));
}

#[test]
fn a_launch_has_no_preferred_port_by_default() {
    let options = parse_args(args(&[])).expect("the bare invocation should parse");

    assert_eq!(options.serve.preferred_port, None);
}

#[test]
fn preferred_port_requires_a_value() {
    assert_eq!(error_kind(&["--preferred-port"]), ErrorKind::InvalidValue);
}

#[test]
fn preferred_port_rejects_a_value_that_is_not_a_port() {
    for value in ["http", "65536", "70000"] {
        assert_eq!(
            error_kind(&["--preferred-port", value]),
            ErrorKind::ValueValidation,
            "--preferred-port {value} is not a port"
        );
    }
}

#[test]
fn the_help_text_hides_the_preferred_port_flag() {
    let help = Cli::command().render_long_help().to_string();

    assert!(!help.contains("--preferred-port"), "{help}");
}

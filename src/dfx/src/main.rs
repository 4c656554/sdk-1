use crate::config::dfinity;
use crate::config::{dfx_version, dfx_version_str};
use crate::lib::environment::{Environment, EnvironmentImpl};
use crate::lib::error::DfxError;
use crate::lib::logger::{create_root_logger, LoggingMode};
use crate::lib::message::UserMessage;

use clap::{AppSettings, Clap};
use ic_http_agent::AgentError;
use semver::Version;
use slog::{Logger, error, info};
use std::path::PathBuf;
use url::Url;

mod commands;
mod config;
mod lib;
mod util;

const LOG_MODES: &[&str; 3] = &["file", "stderr", "tee"];

/// DFX global configuration options.
#[clap(
    author = "DFINITY USA Research LLC",
    global_setting = AppSettings::ColoredHelp,
    version = dfx_version_str(),
)]
#[derive(Clap, Clone, Debug)]
struct Args {

    /// Verbosity level.
    #[clap(long = "verbose", short = "v", parse(from_occurrences))]
    verbose: i64,

    /// Verbosity suppression level.
    #[clap(long = "quiet", short = "q", parse(from_occurrences))]
    quiet: i64,

    /// Log file.
    #[clap(long = "log-file", default_value = "log.txt", takes_value = true)]
    log_file: String,

    /// Log mode.
    #[clap(
        long = "log-mode",
        default_value = "stderr",
        possible_values = LOG_MODES,
        takes_value = true,
    )]
    log_mode: String,

    /// Command.
    #[clap(subcommand)]
    command: Command,
}

/// DFX commands.
#[derive(Clap, Clone, Debug)]
enum Command {

    /// Bootstrap command.
    #[clap(about = UserMessage::BootstrapCommand.to_str(), name = "bootstrap")]
    Bootstrap(dfinity::ConfigDefaultsBootstrap),

    /// Build command.
    #[clap(about = UserMessage::BuildCommand.to_str(), name = "build")]
    Build(dfinity::ConfigDefaultsBuild),

    /// Cache command.
    #[clap(about = UserMessage::CacheCommand.to_str(), name = "cache")]
    Cache(dfinity::ConfigDefaultsCache),

    /// Canister command.
    #[clap(about = UserMessage::CanisterCommand.to_str(), name = "canister")]
    Canister(dfinity::ConfigDefaultsCanister),

/*
    /// Config command.
    #[clap(about = UserMessage::ConfigCommand.to_str(), name = "config")]
    Config(dfinity::ConfigDefaultsConfig),

    /// IDE command.
    #[clap(about = UserMessage::IDECommand.to_str(), name = "_language-service")]
    IDE(dfinity::ConfigDefaultsIDE),

    /// New command.
    #[clap(about = UserMessage::NewCommand.to_str(), name = "new")]
    New(dfinity::ConfigDefaultsNew),

    /// Replica command.
    #[clap(about = UserMessage::ReplicaCommand.to_str(), name = "replica")]
    Replica(dfinity::ConfigDefaultsReplica),

    /// Start command.
    #[clap(about = UserMessage::StartCommand.to_str(), name = "start")]
    Start(dfinity::ConfigDefaultsStart),

    /// Stop command.
    #[clap(about = UserMessage::StopCommand.to_str(), name = "stop")]
    Stop(dfinity::ConfigDefaultsStop),

    /// Upgrade command.
    #[clap(about = UserMessage::UpgradeCommand.to_str(), name = "upgrade")]
    Upgrade(dfinity::ConfigDefaultsUpgrade),

*/
}

/// Initialize a logger.
fn init_logger(args: &Args) -> Logger {
    let level = args.verbose - args.quiet;
    let file = PathBuf::from(args.log_file.clone());
    let mode = match args.log_mode.as_str() {
        "file" => LoggingMode::File(file),
        "tee" => LoggingMode::Tee(file),
        _ => LoggingMode::Stderr,
        // TODO: Add support for stdout.
    };
    create_root_logger(level, mode)
}

/// Run a DFX command.
fn main() {
    // Parse command-line arguments.
    let args = Args::parse();
    // Configure execution environment.
    let progress_bar = args.verbose >= args.quiet;
    let logger = init_logger(&args);
    EnvironmentImpl::new()
        .map(|env| env.with_logger(logger).with_progress_bar(progress_bar))
        .map(move |env| {
            // Configure execution redirect if necessary.
            let version = env.get_version();
            maybe_redirect_dfx(version).map_or((), |_| unreachable!());
            // Execute command.
            let result = match args.command {
                Command::Bootstrap(cfg) => commands::bootstrap::exec(&env, &cfg),
                Command::Build(cfg) => commands::build::exec(&env, &cfg),
                Command::Cache(cfg) => commands::cache::exec(&env, cfg),
                Command::Canister(cfg) => commands::canister::exec(&env, cfg),
                // TODO: Implement remaining commands.
            };
            // Check if an error occurred.
            if let Err(err) = result {
                notify_err(err);
                std::process::exit(255)
            }
        });
}

/// Notify the user that an error occurred.
fn notify_err(err: DfxError) {
    match err {
        DfxError::BuildError(err) => {
            eprintln!("Build failed. Reason:");
            eprintln!("  {}", err);
        }
        DfxError::IdeError(msg) => {
            eprintln!("The Motoko Language Server returned an error:\n{}", msg);
        }
        DfxError::UnknownCommand(command) => {
            eprintln!("Unknown command: {}", command);
        }
        DfxError::ProjectExists => {
            eprintln!("Cannot create a new project because the directory already exists.");
        }
        DfxError::CommandMustBeRunInAProject => {
            eprintln!("Command must be run in a project directory (with a dfx.json file).");
        }
        DfxError::AgentError(AgentError::ClientError(code, message)) => {
            eprintln!("Client error (code {}): {}", code, message);
        }
        DfxError::Unknown(err) => {
            eprintln!("Unknown error: {}", err);
        }
        DfxError::ConfigPathDoesNotExist(config_path) => {
            eprintln!("Config path does not exist: {}", config_path);
        }
        DfxError::InvalidArgument(e) => {
            eprintln!("Invalid argument: {}", e);
        }
        DfxError::InvalidData(e) => {
            eprintln!("Invalid data: {}", e);
        }
        DfxError::LanguageServerFromATerminal => {
            eprintln!("The `_language-service` command is meant to be run by editors to start a language service. You probably don't want to run it from a terminal.\nIf you _really_ want to, you can pass the --force-tty flag.");
        }
        err => {
            eprintln!("An error occured:\n{:#?}", err);
        }
    }
}

/// In some cases, redirect the dfx execution to the proper version.
/// This will ALWAYS return None, OR WILL TERMINATE THE PROCESS. There is no Ok()
/// version of this (nor should there be).
///
/// Note: the right return type for communicating this would be [Option<!>], but since the
/// never type is experimental, we just assert on the calling site.
fn maybe_redirect_dfx(env_version: &Version) -> Option<()> {
    // Verify we're using the same version as the dfx.json, and if not just redirect the
    // call to the cache.
    if dfx_version() != env_version {
        // Show a warning to the user.
        if !is_warning_disabled("version_check") {
            eprintln!(
                concat!(
                    "Warning: The version of DFX used ({}) is different than the version ",
                    "being run ({}).\n",
                    "This might happen because your dfx.json specifies an older version, or ",
                    "DFX_VERSION is set in your environment.\n",
                    "We are forwarding the command line to the old version. To disable this ",
                    "warning, set the DFX_WARNING=-version_check environment variable.\n"
                ),
                env_version,
                dfx_version()
            );
        }
        match crate::config::cache::call_cached_dfx(env_version) {
            Ok(status) => std::process::exit(status.code().unwrap_or(0)),
            Err(e) => {
                eprintln!("Error when trying to forward to project dfx:\n{:?}", e);
                eprintln!("Installed executable: {}", dfx_version());
                std::process::exit(1)
            }
        };
    }
    None
}

fn is_warning_disabled(warning: &str) -> bool {
    std::env::var("DFX_WARNING")
        .unwrap_or_else(|_| "".to_string())
        .split(',')
        .filter(|w| w.starts_with('-'))
        .any(|w| w.chars().skip(1).collect::<String>().eq(warning))
}

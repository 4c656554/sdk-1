use crate::config::dfinity::ConfigDefaultsBootstrap;
use crate::config::{dfx_version, dfx_version_str};
use crate::lib::environment::{Environment, EnvironmentImpl};
use crate::lib::error::DfxError;
use crate::lib::logger::{create_root_logger, LoggingMode};
use crate::lib::message::UserMessage;
use clap::{AppSettings, Clap};
use ic_http_agent::AgentError;
use semver::Version;
use slog;
use std::path::PathBuf;

mod actors;
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
struct Opts {

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
    Bootstrap(ConfigDefaultsBootstrap),

/*
    /// Build command.
    #[clap(about = UserMessage::BuildCommand.to_str(), name = "build")]
    Build(ConfigDefaultsBuild),

    /// Cache command.
    #[clap(about = UserMessage::CacheCommand.to_str(), name = "cache")]
    Cache(ConfigDefaultsCache),

    /// Canister command.
    #[clap(about = UserMessage::CanisterCommand.to_str(), name = "canister")]
    Canister(ConfigDefaultsCanister),

    /// Config command.
    #[clap(about = UserMessage::ConfigCommand.to_str(), name = "config")]
    Config(ConfigDefaultsConfig),

    /// IDE command.
    #[clap(about = UserMessage::IDECommand.to_str(), name = "_language-service")]
    IDE(ConfigDefaultsIDE),

    /// New command.
    #[clap(about = UserMessage::NewCommand.to_str(), name = "new")]
    New(ConfigDefaultsNew),

    /// Replica command.
    #[clap(about = UserMessage::ReplicaCommand.to_str(), name = "replica")]
    Replica(ConfigDefaultsReplica),

    /// Start command.
    #[clap(about = UserMessage::StartCommand.to_str(), name = "start")]
    Start(ConfigDefaultsStart),

    /// Stop command.
    #[clap(about = UserMessage::StopCommand.to_str(), name = "stop")]
    Stop(ConfigDefaultsStop),

    /// Upgrade command.
    #[clap(about = UserMessage::UpgradeCommand.to_str(), name = "upgrade")]
    Upgrade(ConfigDefaultsUpgrade),
*/
}

/// Initialize a logger.
fn init_logger(opts: &Opts) -> slog::Logger {
    let level = opts.verbose - opts.quiet;
    let file = PathBuf::from(opts.log_file.clone());
    let mode = match opts.log_mode.as_str() {
        "file" => LoggingMode::File(file),
        "tee" => LoggingMode::Tee(file),
        _ => LoggingMode::Stderr,
    };
    create_root_logger(level, mode)
}

/// Run a DFX command.
fn main() {
    let opts: Opts = Opts::parse();
    let progress_bar = opts.verbose >= opts.quiet;
    let logger = init_logger(&opts);
    let result = EnvironmentImpl::new()
        .map(|env| env.with_logger(logger).with_progress_bar(progress_bar))
        .map(move |env| {
            let version = env.get_version();
            maybe_redirect_dfx(version).map_or((), |_| unreachable!());
            match opts.command {
                Command::Bootstrap(cfg) => commands::bootstrap::exec(&env, &cfg),
/*
                Command::Build(cfg) => commands::build::exec(&env, &cfg),
                Command::Cache(cfg) => commands::cache::exec(&env, &cfg),
                Command::Canister(cfg) => commands::canister::exec(&env, &cfg),
                Command::Config(cfg) => commands::config::exec(&env, &cfg),
                Command::IDE(cfg) => commands::ide::exec(&env, &cfg),
                Command::New(cfg) => commands::new::exec(&env, &cfg),
                Command::Replica(cfg) => commands::replica::exec(&env, &cfg),
                Command::Start(cfg) => commands::start::exec(&env, &cfg),
                Command::Stop(cfg) => commands::stop::exec(&env, &cfg),
                Command::Upgrade(cfg) => commands::upgrade::exec(&env, &cfg),
*/
            }
        });

    // Check if an error occurred.
    if let Err(err) = result {
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
        std::process::exit(255);
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

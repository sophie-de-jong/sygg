use std::{fs::OpenOptions, time::Duration};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgAction, Parser};
use crossterm::style::{Stylize, force_color_output};
use env_logger::{Builder, Target, WriteStyle};
use log::LevelFilter;
use sygg::{AlgebraEngine, EngineConfig};

mod repl;

use repl::ReplOptions;

pub const CARGO_PKG_NAME: &str = env!("CARGO_PKG_NAME");
pub const CARGO_PKG_AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
pub const CARGO_PKG_DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");
pub const VERSION_AND_GIT_HASH: &str = env!("VERSION_AND_GIT_HASH");

const HELP_TEMPLATE: &str = "\
{before-help}{name} {version}
{author-with-newline}{about-with-newline}
{usage-heading} {usage}

{all-args}";

#[derive(Debug, Parser)]
#[command(
    name = CARGO_PKG_NAME,
    version = VERSION_AND_GIT_HASH,
    author = CARGO_PKG_AUTHORS,
    about = CARGO_PKG_DESCRIPTION,
    help_template = HELP_TEMPLATE,
)]
struct Cli {
    /// Rule files to load at startup (one rule per line)
    #[arg(value_name = "RULES")]
    rules: Vec<PathBuf>,

    /// Rule file to load at startup (may be repeated; same as positional RULES)
    #[arg(short = 'r', long = "rules", value_name = "FILE")]
    rule_files: Vec<PathBuf>,

    /// Do not load the preset rule files
    #[arg(long)]
    no_default_rules: bool,

    /// Disable ANSI colours
    #[arg(long)]
    no_color: bool,

    /// Do not print the welcome banner
    #[arg(long)]
    no_banner: bool,

    /// Increase log verbosity: -v info, -vv debug, -vvv trace
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count)]
    verbose: u8,

    /// Maximum number of e-graph iterations
    #[arg(long, default_value_t = 15, value_name = "N")]
    max_iterations: usize,

    /// Maximum number of e-graph nodes
    #[arg(long, default_value_t = 20_000, value_name = "N")]
    max_nodes: usize,

    /// Time limit for e-graph saturation, in milliseconds
    #[arg(long, default_value_t = 750, value_name = "MS")]
    time_limit: u64,

    /// Maximum expression depth that rewrites may introduce
    #[arg(long, default_value_t = 32, value_name = "N")]
    max_depth: usize,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let use_color =
        !cli.no_color || std::io::stdout().is_terminal() || std::env::var_os("NO_COLOR").is_none();
    force_color_output(use_color);

    init_logging(cli.verbose);

    log::info!("starting {CARGO_PKG_NAME} {VERSION_AND_GIT_HASH}");
    if let Some(path) = repl::log_path() {
        log::debug!("log file: {}", path.display());
    }

    let mut engine = AlgebraEngine::new(EngineConfig {
        max_iterations: cli.max_iterations,
        max_nodes: cli.max_nodes,
        time_limit: Duration::from_millis(cli.time_limit),
        max_depth: cli.max_depth,
        load_default_rules: !cli.no_default_rules,
    });

    for path in cli.rules.iter().chain(&cli.rule_files) {
        match engine.load_rules_file(path) {
            Ok(count) => log::info!("loaded {count} rule(s) from {}", path.display()),
            Err(error) => {
                log::error!("failed to load rules from {}: {error}", path.display());
                eprintln!("{} {error}", "error:".red());
                return ExitCode::from(1);
            }
        }
    }

    let options = ReplOptions {
        banner: !cli.no_banner,
    };
    if let Err(error) = repl::run(&mut engine, options) {
        log::error!("REPL error: {error}");
        eprintln!("{} {error}", "error:".red());
        return ExitCode::from(1);
    }
    ExitCode::from(0)
}

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => LevelFilter::Warn,
        1 => LevelFilter::Info,
        2 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };

    let mut builder = Builder::new();
    builder.filter_level(level);
    builder.write_style(WriteStyle::Never);

    if let Some(path) = repl::log_path() {
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            eprintln!(
                "{} could not create log directory {}: {error}",
                "warning:".yellow(),
                parent.display()
            );
        }

        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => {
                builder.target(Target::Pipe(Box::new(file)));
            }
            Err(error) => {
                eprintln!(
                    "{} could not open log file {}: {error}; logging to stderr",
                    "warning:".yellow(),
                    path.display()
                );
            }
        }
    }

    if builder.try_init().is_err() {
        eprintln!("{} logger was already initialized", "warning:".yellow());
    }
}

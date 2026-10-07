use clap::{Args, Parser, Subcommand};
use log::info;
use simplelog::{ColorChoice, ConfigBuilder, LevelFilter, TermLogger, TerminalMode};
use std::env;
use std::path::PathBuf;
use utgen::{LLM, LlmConfig, analyze_project, collect_coverage, validate_repair};
use utgen::{comment_out_tests, gen_test_rate, gen_tests_project, llm_fix, rename_tests_to_bak};

/// Generate unit tests for a project
#[derive(Debug, Parser)]
struct Cli {
    /// Model configuration JSON; falls back to PALM_CONFIG. Used by gen and fix.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Preprocess the project, including renaming integration tests and commenting out unit tests
    PreProcess {
        #[command(flatten)]
        options: Opts,
    },
    /// Analyze the project to get branch constraints and context information
    Analyze {
        #[command(flatten)]
        options: Opts,
    },
    /// Run existing tests once and export coverage excluding test code (no model needed)
    Coverage {
        /// Standalone crate working copy
        #[arg(short, long)]
        project_dir: PathBuf,
    },
    /// Generate and run tests for the project to collect coverage data
    Gen {
        #[command(flatten)]
        options: Opts,
        /// Number of parallel test generation tasks
        #[arg(short, long, default_value = "128")]
        tasks: usize,
        /// Whether to generate integration tests
        #[arg(short, long)]
        integration: bool,
        /// Whether to provide requirements in prompt
        #[arg(short, long)]
        requirement: bool,
        /// Whether to provide context in prompt
        #[arg(short, long)]
        context: bool,
        /// Whether to generate oracle independently
        #[arg(short, long)]
        oracle: bool,
    },
    /// Fix and run tests for the project to collect coverage data
    Fix {
        #[command(flatten)]
        options: Opts,
        /// Number of parallel test fix tasks
        #[arg(short, long, default_value = "128")]
        tasks: usize,
    },
}

#[derive(Debug, Args)]
struct Opts {
    /// Path to the project directory, can be relative to the current directory
    #[arg(short, long)]
    project_dir: PathBuf,
    /// Crate directories, separated by commas; relative to the current directory
    #[arg(short, long, use_value_delimiter = true)]
    work_dir: Vec<PathBuf>,
}

fn init_log() {
    let log_config = ConfigBuilder::new()
        .set_location_level(LevelFilter::Error)
        .build();
    TermLogger::init(
        LevelFilter::Info,
        log_config,
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .unwrap();
}

fn get_dirs(options: Opts) -> std::io::Result<(PathBuf, Vec<PathBuf>)> {
    let current_dir = env::current_dir()?;
    let project_dir = current_dir.join(options.project_dir).canonicalize()?;
    let mut work_dirs = Vec::new();
    for work_dir in options.work_dir {
        work_dirs.push(current_dir.join(work_dir).canonicalize()?);
    }
    if work_dirs.is_empty() {
        work_dirs.push(project_dir.clone());
    }
    if !project_dir.is_dir() || work_dirs.iter().any(|dir| !dir.is_dir()) {
        return Err(std::io::Error::other(
            "Project and work paths must be directories",
        ));
    }
    Ok((project_dir, work_dirs))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_log();
    let cli = Cli::parse();
    info!("Command line arguments: {:?}", cli);

    match cli.command {
        // preprocess project
        Command::PreProcess { options } => {
            let (project_dir, work_dirs) = get_dirs(options)?;
            info!(
                "Preprocessing project at {}, with work directory(s) {:?}",
                project_dir.display(),
                work_dirs
            );

            // Rename integration tests
            for work_dir in work_dirs {
                if work_dir.join("tests").exists() && work_dir.join("tests.bak").exists() {
                    return Err(std::io::Error::other(
                        "tests.bak already exists; use a fresh working copy",
                    )
                    .into());
                }
                comment_out_tests(&work_dir)?;
                rename_tests_to_bak(&work_dir)?;
            }
        }
        // analyze project
        Command::Analyze { options } => {
            let (project_dir, work_dirs) = get_dirs(options)?;
            info!(
                "Analyzing project at {}, with work directory(s) {:?}",
                project_dir.display(),
                work_dirs
            );
            for work_dir in &work_dirs {
                if work_dirs.len() != 1 || work_dir != &project_dir {
                    return Err(std::io::Error::other(
                        "analyze currently supports one standalone crate; pass its root with -p",
                    )
                    .into());
                }
                analyze_project(work_dir)?;
            }
        }
        Command::Coverage { project_dir } => {
            let output = collect_coverage(&project_dir.canonicalize()?, true)?;
            use std::io::Write;
            std::io::stdout().write_all(&output.stdout)?;
            std::io::stderr().write_all(&output.stderr)?;
        }
        // generate unit tests
        Command::Gen {
            options,
            tasks,
            integration,
            requirement,
            context,
            oracle,
        } => {
            let llm = LLM::new(LlmConfig::load(cli.config.as_deref())?);
            let (project_dir, work_dirs) = get_dirs(options)?;
            info!(
                "Generating tests for project at {}, with work directory(s) {:?}",
                project_dir.display(),
                work_dirs
            );

            // Generate tests for each work directory
            for work_dir in work_dirs.iter() {
                gen_tests_project(
                    &llm,
                    &project_dir,
                    &work_dir,
                    tasks,
                    integration,
                    requirement,
                    context,
                    oracle,
                )
                .await?;
            }
            // Collect coverage rate and pass rate for each work directory
            for work_dir in work_dirs.iter() {
                gen_test_rate(&project_dir, &work_dir, integration, true);
            }
        }
        // fix unit tests
        Command::Fix { options, tasks } => {
            let llm = LLM::new(LlmConfig::load(cli.config.as_deref())?);
            let (project_dir, work_dirs) = get_dirs(options)?;
            info!(
                "Fixing tests for project at {}, with work directory(s) {:?}",
                project_dir.display(),
                work_dirs
            );
            for work_dir in work_dirs.iter() {
                validate_repair(&project_dir, work_dir)?;
                llm_fix(&llm, project_dir.clone(), work_dir.clone()).await;
            }
            //gen_test_rate_aggregated(&project_dir, false);
            for work_dir in work_dirs.iter() {
                gen_test_rate(&project_dir, &work_dir, false, false);
            }
        }
    }
    Ok(())
}

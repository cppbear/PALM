use clap::{Args, Parser, Subcommand};
use log::info;
use simplelog::{ColorChoice, ConfigBuilder, LevelFilter, TermLogger, TerminalMode};
use std::path::PathBuf;
use std::{
    env,
    num::{NonZeroU64, NonZeroUsize},
    time::Duration,
};
use utgen::{
    FunctionSelection, LLM, LlmConfig, analyze_project, collect_coverage, validate_repair,
};
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
        /// Maximum active function-generation tasks; compilation stays serial
        #[arg(short, long, default_value = "4")]
        tasks: NonZeroUsize,
        /// Timeout in seconds for each model request attempt (at most three attempts)
        #[arg(long, default_value = "180", value_name = "SECONDS")]
        request_timeout: NonZeroU64,
        /// Exact full function names, one per line (blank lines ignored)
        #[arg(long, value_name = "PATH")]
        functions_file: Option<PathBuf>,
        /// Maximum model request attempts for the whole invocation, including retries
        #[arg(long, value_name = "N")]
        max_requests: Option<NonZeroU64>,
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
        /// Maximum active function-repair tasks; compilation stays serial
        #[arg(short, long, default_value = "4")]
        tasks: NonZeroUsize,
        /// Timeout in seconds for each model request attempt (at most three attempts)
        #[arg(long, default_value = "180", value_name = "SECONDS")]
        request_timeout: NonZeroU64,
        /// Exact full function names, one per line (blank lines ignored)
        #[arg(long, value_name = "PATH")]
        functions_file: Option<PathBuf>,
        /// Maximum model request attempts for the whole invocation, including retries
        #[arg(long, value_name = "N")]
        max_requests: Option<NonZeroU64>,
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
            request_timeout,
            functions_file,
            max_requests,
            integration,
            requirement,
            context,
            oracle,
        } => {
            let (project_dir, work_dirs) = get_dirs(options)?;
            let functions = FunctionSelection::load(functions_file.as_deref(), &work_dirs)?;
            let llm = LLM::new(LlmConfig::load(cli.config.as_deref())?)
                .with_request_timeout(Duration::from_secs(request_timeout.get()))
                .with_max_requests(max_requests);
            info!(
                "Generating tests for project at {}, with work directory(s) {:?}",
                project_dir.display(),
                work_dirs
            );

            let generated = async {
                for work_dir in work_dirs.iter() {
                    gen_tests_project(
                        &llm,
                        &project_dir,
                        &work_dir,
                        &functions,
                        tasks.get(),
                        integration,
                        requirement,
                        context,
                        oracle,
                    )
                    .await?;
                }
                Ok::<_, std::io::Error>(())
            }
            .await;
            let recorded = if generated.is_ok() || llm.request_count() > 0 {
                llm.write_request_summary(
                    &project_dir.join("utgen/generation/gen-requests.json"),
                    serde_json::json!({
                        "command": "gen", "functions": functions.names(),
                        "project_dir": project_dir, "work_dirs": work_dirs,
                        "tasks": tasks, "integration": integration, "requirement": requirement,
                        "context": context, "oracle": oracle,
                        "candidate_status": if generated.is_ok() { "completed" } else { "failed" },
                        "error": generated.as_ref().err().map(ToString::to_string),
                    }),
                )
            } else {
                Ok(())
            };
            generated?;
            recorded?;
            // Collect coverage rate and pass rate for each work directory
            for work_dir in work_dirs.iter() {
                gen_test_rate(&project_dir, &work_dir, integration, true, &functions);
            }
        }
        // fix unit tests
        Command::Fix {
            options,
            tasks,
            request_timeout,
            functions_file,
            max_requests,
        } => {
            let (project_dir, work_dirs) = get_dirs(options)?;
            let functions = FunctionSelection::load(functions_file.as_deref(), &work_dirs)?;
            let llm = LLM::new(LlmConfig::load(cli.config.as_deref())?)
                .with_request_timeout(Duration::from_secs(request_timeout.get()))
                .with_max_requests(max_requests);
            info!(
                "Fixing tests for project at {}, with work directory(s) {:?}",
                project_dir.display(),
                work_dirs
            );
            let repaired = async {
                for work_dir in work_dirs.iter() {
                    validate_repair(&project_dir, work_dir)?;
                    llm_fix(
                        &llm,
                        project_dir.clone(),
                        work_dir.clone(),
                        &functions,
                        tasks.get(),
                    )
                    .await?;
                }
                Ok::<_, std::io::Error>(())
            }
            .await;
            let recorded = if repaired.is_ok() || llm.request_count() > 0 {
                llm.write_request_summary(
                    &project_dir.join("utgen/generation/fix-requests.json"),
                    serde_json::json!({
                        "command": "fix", "functions": functions.names(),
                        "project_dir": project_dir, "work_dirs": work_dirs, "tasks": tasks,
                        "candidate_status": if repaired.is_ok() { "completed" } else { "failed" },
                        "error": repaired.as_ref().err().map(ToString::to_string),
                    }),
                )
            } else {
                Ok(())
            };
            repaired?;
            recorded?;
            //gen_test_rate_aggregated(&project_dir, false);
            for work_dir in work_dirs.iter() {
                gen_test_rate(&project_dir, &work_dir, false, false, &functions);
            }
        }
    }
    Ok(())
}

use clap::{Args, Parser, Subcommand};
use log::info;
use utgen::{comment_out_tests, gen_test_rate, gen_tests_project, llm_fix, rename_tests_to_bak};
use utgen::gen_test_rate_aggregated;
use simplelog::{ColorChoice, ConfigBuilder, LevelFilter, TermLogger, TerminalMode};
use std::env;
use std::path::PathBuf;

/// Generate unit tests for a project
#[derive(Debug, Parser)]
struct Cli {
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
    /// Path to the work directory(s), separated by commas (e.g., dir1,dir2), can be relative to the project directory, default to the project directory
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

fn get_dirs(options: Opts) -> (PathBuf, Vec<PathBuf>) {
    let current_dir = env::current_dir().unwrap();
    let project_dir = current_dir
        .join(options.project_dir)
        .canonicalize()
        .unwrap();
    let mut work_dirs = Vec::new();
    for work_dir in options.work_dir {
        work_dirs.push(current_dir.join(work_dir).canonicalize().unwrap());
    }
    if work_dirs.is_empty() {
        work_dirs.push(project_dir.clone());
    }
    (project_dir, work_dirs)
}

#[tokio::main]
async fn main() {
    init_log();
    let cli = Cli::parse();
    info!("Command line arguments: {:?}", cli);

    match cli.command {
        // preprocess project
        Command::PreProcess { options } => {
            let (project_dir, work_dirs) = get_dirs(options);
            info!(
                "Preprocessing project at {}, with work directory(s) {:?}",
                project_dir.display(),
                work_dirs
            );

            // Rename integration tests
            rename_tests_to_bak(&project_dir).unwrap();

            // Comment out unit tests
            for work_dir in work_dirs {
                comment_out_tests(&work_dir).unwrap();
            }
        }
        // analyze project
        Command::Analyze { options } => {
            let (project_dir, work_dirs) = get_dirs(options);
            info!(
                "Analyzing project at {}, with work directory(s) {:?}",
                project_dir.display(),
                work_dirs
            );
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
            let (project_dir, work_dirs) = get_dirs(options);
            info!(
                "Generating tests for project at {}, with work directory(s) {:?}",
                project_dir.display(),
                work_dirs
            );

            // Generate tests for each work directory
            for work_dir in work_dirs.iter() {
                gen_tests_project(
                    &project_dir,
                    &work_dir,
                    tasks,
                    integration,
                    requirement,
                    context,
                    oracle,
                )
                .await;
            }
            // Collect coverage rate and pass rate for each work directory
            for work_dir in work_dirs.iter() {
                gen_test_rate(&project_dir, &work_dir, integration, true);
            }
        }
        // fix unit tests
        Command::Fix { options, tasks } => {
            let (project_dir, work_dirs) = get_dirs(options);
            info!(
                "Fixing tests for project at {}, with work directory(s) {:?}",
                project_dir.display(),
                work_dirs
            );
            for work_dir in work_dirs.iter() {
                llm_fix(project_dir.clone(), work_dir.clone()).await;
            }
            //gen_test_rate_aggregated(&project_dir, false);
            for work_dir in work_dirs.iter() {
                gen_test_rate(&project_dir, &work_dir, false, false);
            }
        }
    }
}

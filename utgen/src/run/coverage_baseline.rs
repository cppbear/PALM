use super::{
    coverage::{project_metadata, with_test_exclusions},
    coverage_json::CoverageJson,
};
use crate::{FunctionSelection, types::BrData, utils::TemporaryFile};
use cargo_metadata::{Message, Metadata, Target, TargetKind};
use std::{
    collections::{BTreeSet, HashMap},
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

const BASELINE_TARGET: &str = "palm_coverage_baseline";

/// Reject unsupported target ownership before model requests or source edits.
pub(crate) fn validate_targets(
    project_dir: &Path,
    work_dir: &Path,
    functions: &FunctionSelection,
    integration: bool,
) -> io::Result<()> {
    let metadata = project_metadata(work_dir)?;
    if integration
        && !metadata
            .root_package()
            .unwrap()
            .targets
            .iter()
            .any(|target| target.is_kind(TargetKind::Lib))
    {
        return Err(io::Error::other(
            "Function-level integration tests require a library target",
        ));
    }
    for entry in fs::read_dir(work_dir.join("brinfo/brdata"))? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let data: BrData = serde_json::from_slice(&fs::read(path)?)?;
        if functions.contains(&data.name) && (!integration || data.visible) {
            select_target(
                &metadata,
                &data.name,
                &project_dir.join(data.loc.get_file()),
                integration,
            )?;
        }
    }
    Ok(())
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct FunctionMapping {
    pub lines: Vec<i32>,
    pub branches: Vec<(i32, i32, i32, i32)>,
}

struct Baseline {
    lines: HashMap<PathBuf, BTreeSet<i32>>,
    branches: CoverageJson,
}

impl Baseline {
    fn function(&self, file: &Path, begin: i32, end: i32) -> FunctionMapping {
        let lines = self
            .lines
            .get(file)
            .into_iter()
            .flatten()
            .filter(|&&line| begin <= line && line <= end)
            .copied()
            .collect();
        let mut branches = BTreeSet::new();
        for data in &self.branches.data {
            for entry in &data.files {
                if Path::new(&entry.file_name) != file {
                    continue;
                }
                for branch in &entry.coverage_branches {
                    if begin <= branch.start_line as i32 && branch.end_line as i32 <= end {
                        branches.insert((
                            branch.start_line as i32,
                            branch.start_column as i32,
                            branch.end_line as i32,
                            branch.end_column as i32,
                        ));
                    }
                }
            }
        }
        FunctionMapping {
            lines,
            branches: branches.into_iter().collect(),
        }
    }
}

/// Parsed maps live only for this statistics invocation, separately for each target.
pub(crate) struct BaselineCache {
    metadata: Metadata,
    integration: bool,
    reports: HashMap<String, Baseline>,
}

impl BaselineCache {
    pub fn new(work_dir: &Path, integration: bool) -> io::Result<Self> {
        Ok(Self {
            metadata: project_metadata(work_dir)?,
            integration,
            reports: HashMap::new(),
        })
    }

    pub fn mapping(
        &mut self,
        work_dir: &Path,
        name: &str,
        file: &Path,
        begin: i32,
        end: i32,
    ) -> io::Result<FunctionMapping> {
        let target = select_target(&self.metadata, name, file, self.integration)?.clone();
        let key = format!("{:?}:{}", target.kind, target.name);
        if !self.reports.contains_key(&key) {
            log::info!(
                "Collecting {} coverage baseline for {}",
                if self.integration {
                    "integration"
                } else {
                    "unit"
                },
                target.name
            );
            let report = collect(work_dir, &self.metadata, &target, self.integration)?;
            self.reports.insert(key.clone(), report);
        }
        Ok(self.reports[&key].function(file, begin, end))
    }
}

fn select_target<'a>(
    metadata: &'a Metadata,
    name: &str,
    file: &Path,
    integration: bool,
) -> io::Result<&'a Target> {
    let crate_name = name.split("::").next().unwrap();
    let candidates: Vec<_> = metadata
        .root_package()
        .unwrap()
        .targets
        .iter()
        .filter(|t| t.is_kind(TargetKind::Lib) || t.is_kind(TargetKind::Bin))
        .filter(|t| t.name.replace('-', "_") == crate_name)
        .collect();
    let target = if candidates.len() == 1 {
        Some(candidates[0])
    } else {
        let roots: Vec<_> = candidates
            .into_iter()
            .filter(|t| t.src_path.as_std_path() == file)
            .collect();
        if roots.len() == 1 {
            Some(roots[0])
        } else {
            None
        }
    };
    let target = target.ok_or_else(|| io::Error::other(format!(
        "Cannot uniquely select a Cargo target for {name}; use a standalone library or binary working copy"
    )))?;
    if integration && !target.is_kind(TargetKind::Lib) {
        return Err(io::Error::other(
            "Function-level integration tests require a library target",
        ));
    }
    Ok(target)
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "palm-coverage-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            log::warn!(
                "Cannot remove coverage scratch directory {}: {error}",
                self.0.display()
            );
        }
    }
}

fn checked(command: &mut Command) -> io::Result<Output> {
    check_output(command.output()?)
}

fn check_output(output: Output) -> io::Result<Output> {
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "Coverage baseline command failed ({}):\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(output)
}

fn llvm_tools(work_dir: &Path) -> io::Result<PathBuf> {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let sysroot = checked(
        Command::new(&rustc)
            .args(["--print", "sysroot"])
            .current_dir(work_dir),
    )?;
    let version = checked(Command::new(&rustc).arg("-vV").current_dir(work_dir))?;
    let version = String::from_utf8_lossy(&version.stdout);
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or_else(|| io::Error::other("rustc did not report its host triple"))?;
    Ok(
        PathBuf::from(String::from_utf8_lossy(&sysroot.stdout).trim())
            .join("lib/rustlib")
            .join(host)
            .join("bin"),
    )
}

fn collect(
    work_dir: &Path,
    metadata: &Metadata,
    target: &Target,
    integration: bool,
) -> io::Result<Baseline> {
    let scratch = Scratch::new()?;
    let tools = llvm_tools(work_dir)?;
    let target_dir = std::env::var_os("CARGO_LLVM_COV_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            metadata
                .target_directory
                .as_std_path()
                .join("llvm-cov-target")
        });
    let output = checked(
        Command::new("cargo")
            .args(["llvm-cov", "show-env", "--branch"])
            .env("CARGO_LLVM_COV_TARGET_DIR", &target_dir)
            .current_dir(work_dir),
    )?;
    // Decode assignments as data, never execute show-env output in a shell.
    let assignments = shlex::split(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| io::Error::other("Cannot parse cargo llvm-cov show-env output"))?;
    let mut command = Command::new("cargo");
    for assignment in &assignments {
        let (key, value) = assignment
            .split_once('=')
            .ok_or_else(|| io::Error::other("Invalid coverage environment assignment"))?;
        command.env(key, value);
    }
    command
        .env("CARGO_TARGET_DIR", &target_dir)
        .env(
            "LLVM_PROFILE_FILE",
            scratch.0.join("baseline-%p-%m.profraw"),
        )
        .current_dir(work_dir)
        .arg("test");
    let temporary = if integration {
        let path = work_dir.join(format!("tests/{BASELINE_TARGET}.rs"));
        fs::create_dir_all(path.parent().unwrap())?;
        let guard = TemporaryFile::new(&path)?;
        fs::write(
            &path,
            format!(
                "#![feature(coverage_attribute)]\nextern crate {};\n#[test]\n#[coverage(off)]\nfn baseline() {{}}\n",
                target.name.replace('-', "_")
            ),
        )?;
        command.args(["--test", BASELINE_TARGET]);
        Some(guard)
    } else {
        if target.is_kind(TargetKind::Lib) {
            command.arg("--lib");
        } else {
            command.args(["--bin", &target.name]);
        }
        None
    };
    command.args(["--message-format=json", "--", "--skip", ""]);
    let output = with_test_exclusions(work_dir, metadata, || checked(&mut command))?;
    if let Some(temporary) = temporary {
        temporary.finish()?;
    }
    let package_id = &metadata.root_package().unwrap().id;
    let mut objects = Vec::new();
    for message in Message::parse_stream(output.stdout.as_slice()) {
        let Message::CompilerArtifact(artifact) = message.map_err(io::Error::other)? else {
            continue;
        };
        if &artifact.package_id != package_id
            || artifact.target.name != target.name
            || artifact.target.kind != target.kind
        {
            continue;
        }
        if integration && !artifact.profile.test {
            for filename in artifact
                .filenames
                .iter()
                .filter(|file| file.extension() == Some("rlib"))
            {
                let directory = scratch.0.join("objects");
                fs::create_dir(&directory)?;
                checked(
                    Command::new(tools.join("llvm-ar"))
                        .arg("xo")
                        .arg(filename)
                        .current_dir(&directory),
                )?;
                for entry in fs::read_dir(directory)? {
                    let path = entry?.path();
                    if path.extension().is_some_and(|ext| ext == "o") {
                        objects.push(path);
                    }
                }
            }
        } else if !integration && artifact.profile.test {
            if let Some(executable) = artifact.executable {
                objects.push(executable.into_std_path_buf());
            }
        }
    }
    if objects.is_empty() {
        return Err(io::Error::other(
            "Cargo did not produce the selected coverage artifact",
        ));
    }
    let profile = scratch.0.join("baseline.profdata");
    let mut merge = Command::new(tools.join("llvm-profdata"));
    merge.args(["merge", "-sparse"]).arg("-o").arg(&profile);
    for entry in fs::read_dir(&scratch.0)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "profraw") {
            merge.arg(path);
        }
    }
    checked(&mut merge)?;
    let export = |format: &str| -> io::Result<String> {
        let mut command = Command::new(tools.join("llvm-cov"));
        command
            .arg("export")
            .arg(format!("--format={format}"))
            .arg("--instr-profile")
            .arg(&profile);
        for object in &objects {
            command.arg("--object").arg(object);
        }
        let output = command.output()?;
        let errors = String::from_utf8_lossy(&output.stderr);
        // LLVM reports an error when a successfully built target has no maps
        // at all (for example, every function is cfg(not(test))). Recognize
        // only that diagnostic; profile, artifact and tool failures still fail.
        if !output.status.success()
            && errors
                .lines()
                .any(|line| line.ends_with(": no coverage data found"))
            && errors.lines().all(|line| {
                (line.starts_with("error: failed to load coverage: ")
                    && line.ends_with(": no coverage data found"))
                    || line == "error: could not load coverage information"
            })
        {
            return Ok(if format == "lcov" {
                ""
            } else {
                "{\"data\":[]}"
            }
            .to_owned());
        }
        let output = check_output(output)?;
        String::from_utf8(output.stdout).map_err(io::Error::other)
    };
    parse_report(&export("lcov")?, &export("text")?)
}

fn parse_report(lcov: &str, json: &str) -> io::Result<Baseline> {
    let mut lines: HashMap<PathBuf, BTreeSet<i32>> = HashMap::new();
    let mut current_file = None;
    for line in lcov.lines() {
        if let Some(file) = line.strip_prefix("SF:") {
            current_file = Some(PathBuf::from(file));
        } else if let Some(record) = line.strip_prefix("DA:") {
            let number = record
                .split(',')
                .next()
                .unwrap()
                .parse()
                .map_err(io::Error::other)?;
            if let Some(file) = &current_file {
                lines.entry(file.clone()).or_default().insert(number);
            }
        } else if line == "end_of_record" {
            current_file = None;
        }
    }
    let mut branches: CoverageJson = serde_json::from_str(json).map_err(io::Error::other)?;
    branches.parse_coverage_branches();
    Ok(Baseline { lines, branches })
}

#[cfg(test)]
#[path = "coverage_baseline_tests.rs"]
mod tests;

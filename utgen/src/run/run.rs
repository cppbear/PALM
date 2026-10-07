use super::{
    coverage_json::CoverageJson,
    integration::{self, gen_integration},
    prepare::get_test_gen_infos,
};
use crate::{
    run::TIMEOUT_DERIVE,
    types::TestGenInfo,
    utils::{backup_file, delete_backup, insert_test, restore_file, target_clean},
    run::run_all::{prepare_all_unit_tests_for_one_shot_run},
};
use log::{error, info};
use quick_xml::{events::Event, Reader};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap, env, fmt::format, fs::{self, File, exists}, io::{BufReader, Write}, path::{Path, PathBuf}, process::{Command, exit}, time::{SystemTime, UNIX_EPOCH}
};
use std::{fs::create_dir_all, vec};
use std::{fs::remove_file, io::Read};

pub enum TestType {
    CoverageRate,
    Error,
}

pub fn run_test(
    project_dir: &Path,
    work_path: &Path,
    test_type: TestType,
    is_json: bool,
    is_integration: bool,
) -> Vec<String> {
    let mut return_value: Vec<String> = Vec::new();

    if let TestType::Error = test_type {
        // println!("Running tests...");
        let output_file = File::create(work_path.join("error_output.json")).unwrap();
        Command::new("cargo")
            .args(["test", "--tests", "--message-format", "json"])
            .stdout(output_file)
            .current_dir(work_path)
            .output()
            .expect("Failed to run tests");
    } else {
        let coverage_output = Command::new("cargo")
            .args([
                "llvm-cov",
                "--tests",
                "--ignore-run-fail",
                "--branch",
                "--cobertura",
                "--output-path",
                "coverage.xml",
            ]) // RUST_BACKTRACE=full cargo llvm-cov --tests --ignore-run-fail --branch --cobertura
            .current_dir(work_path)
            .output()
            .expect("Failed to generate coverage report");
        if !is_integration {
            return_value = String::from_utf8_lossy(&coverage_output.stdout)
                .to_string()
                .lines()
                .into_iter()
                .filter(|line| line.contains("test result:") || line.contains("running "))
                .map(|line| line.to_string())
                .collect();
        } else {
            return_value = String::from_utf8_lossy(&coverage_output.stdout)
                .to_string()
                .lines()
                .map(|line| line.to_string())
                .collect::<Vec<String>>();
        }
        if is_json {
            let json_output = Command::new("cargo")
                .args([
                    "llvm-cov",
                    "--tests",
                    "--ignore-run-fail",
                    "--branch",
                    "--json",
                    "--output-path",
                    "coverage.json",
                ])
                .current_dir(work_path)
                .output()
                .expect("Failed to generate coverage report");
        }
    }
    return return_value;
}

// fn cargo_clean(work_path: &Path) {
//     // println!("Cleaning the project...");
//     let clean_output = Command::new("cargo")
//         .arg("clean")
//         .current_dir(work_path)
//         .output()
//         .expect("Failed to clean the project");

//     if !clean_output.status.success() {
//         eprintln!("Clean failed.");
//         return;
//     }
// }

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BranchCoverageInfo {
    start_line: i32,
    start_column: i32,
    end_line: i32,
    end_column: i32,
    positive: bool,
    negative: bool,
}

impl PartialEq for BranchCoverageInfo {
    fn eq(&self, other: &Self) -> bool {
        self.start_line == other.start_line
            && self.start_column == other.start_column
            && self.end_line == other.end_line
            && self.end_column == other.end_column
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRateInfo {
    pub function_name: String,
    pub file_path: String,
    pub work_dir: String,
    pub tests: i32,
    pub tests_lines: Vec<i32>,
    pub oracles: i32,
    pub oracles_compiled: i32,
    pub oracles_compiled_rate: f64,
    pub tests_compiled: i32,
    pub tests_compiled_rate: f64,
    pub oracles_run: i32,
    pub oracles_passed: i32,
    pub oracles_passed_rate: f64,
    pub tests_run: i32,
    pub tests_passed: i32,
    pub tests_passed_rate: f64,
    pub lines: i32,
    pub lines_covered: i32,
    pub lines_coveraged_rate: f64,
    pub branches: i32,
    pub branches_covered: i32,
    pub branches_coverage_rate: f64,
    pub codes_lines: Vec<i32>,
    pub codes_lines_covered: Vec<(Vec<String>, Vec<i32>)>,
    pub codes_branches: Vec<BranchCoverageInfo>,
    pub codes_branches_covered: Vec<(Vec<String>, Vec<BranchCoverageInfo>)>,
}

impl TestRateInfo {
    fn parse_codes_lines_and_branches(&mut self) {
        let mut lines = 0;
        let mut lines_covered = 0;
        let mut lines_coverage_rate = 0.0;
        let mut branches = 0;
        let mut branches_covered = 0;
        let mut branches_coverage_rate = 0.0;
        let mut codes_lines_covered: Vec<i32> = Vec::new();
        let mut codes_branches_covered: Vec<BranchCoverageInfo> = Vec::new();

        lines = self.codes_lines.len() as i32;
        for (_, codes_lines) in self.codes_lines_covered.iter() {
            for codes_line in codes_lines.iter() {
                if !codes_lines_covered.contains(codes_line) {
                    codes_lines_covered.push(*codes_line);
                }
            }
        }
        lines_covered = codes_lines_covered.len() as i32;
        lines_coverage_rate = if lines > 0 {
            lines_covered as f64 / lines as f64 * 100.0
        } else {
            0.0
        };
        for (_, codes_branches) in self.codes_branches_covered.iter() {
            for codes_branch in codes_branches.iter() {
                let mut has_codes_branch = false;
                for codes_branch_covered in codes_branches_covered.iter_mut() {
                    if *codes_branch_covered == *codes_branch {
                        has_codes_branch = true;
                        if codes_branch.positive {
                            codes_branch_covered.positive = true;
                        }
                        if codes_branch.negative {
                            codes_branch_covered.negative = true;
                        }
                        break;
                    }
                }
                if !has_codes_branch {
                    codes_branches_covered.push(codes_branch.clone());
                }
            }
        }
        branches = if codes_branches_covered.len() > 0 {
            2 * codes_branches_covered.len() as i32
        } else {
            1
        };
        branches_covered = if codes_branches_covered.len() > 0 {
            let mut temp_branches_covered = 0;
            for codes_branch_covered in codes_branches_covered.iter() {
                if codes_branch_covered.positive {
                    temp_branches_covered += 1;
                }
                if codes_branch_covered.negative {
                    temp_branches_covered += 1;
                }
            }
            temp_branches_covered
        } else {
            if lines_covered > 0 {
                1
            } else {
                0
            }
        };
        branches_coverage_rate = if branches > 0 {
            branches_covered as f64 / branches as f64 * 100.0
        } else {
            0.0
        };
        self.lines = lines;
        self.lines_covered = lines_covered;
        self.lines_coveraged_rate = lines_coverage_rate;
        self.branches = branches;
        self.branches_covered = branches_covered;
        self.branches_coverage_rate = branches_coverage_rate;
        self.codes_branches = codes_branches_covered;
    }
}

fn dump_result_for_test_rate_infos(
    test_rate_infos: &Vec<TestRateInfo>,
    project_dir: &Path,
    work_dir: &Path,
    is_pre: bool,
    is_origin: bool,
    test_time: f64,
) {
    let mut result_directory = project_dir.join("utgen");
    if is_origin {
        let output_file_path = work_dir.join("origin_result.json");
    } else {
        if is_pre {
            let output_file_path = work_dir.join("result.json");
            result_directory = result_directory.join("result");
        } else {
            let output_file_path = work_dir.join("fixed_result.json");
            result_directory = result_directory.join("fixed_result");
        }
    }
    create_dir_all(&result_directory).unwrap();
    let map_path = work_dir.join("brinfo/name_map.json");
    let nmap: HashMap<String, String> =
        serde_json::from_str(&fs::read_to_string(&map_path).unwrap()).unwrap();
    for test_rate_info in test_rate_infos.iter() {
        let file_path = PathBuf::from(&result_directory).join(format!(
            "{}.json",
            nmap.get(&test_rate_info.function_name).unwrap()
        ));
        let mut file = File::create(&file_path).unwrap();
        file.write_all(serde_json::to_string(&test_rate_info).unwrap().as_bytes())
            .unwrap();
    }
}

fn dump_result_for_original_test_rate_infos(
    original_test_rate_infos: &Vec<OriginalTestRateInfo>,
    project_dir: &Path,
    work_dir: &Path,
) {
    let result_directory = project_dir.join("utgen");
    let result_file_path = result_directory.join("original_result.json");
    create_dir_all(&result_directory).unwrap();
    let mut file = File::create(&result_file_path).unwrap();
    file.write_all(
        serde_json::to_string(&original_test_rate_infos)
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
}

pub fn gen_test_rate_aggregated(project_dir: &Path, is_pre: bool) {
    let test_gen_infos = get_test_gen_infos(project_dir, is_pre);

    let manifest_path = project_dir.join("utgen/injected_tests_manifest.json");

    let touched_files = prepare_all_unit_tests_for_one_shot_run(
        project_dir,
        &test_gen_infos,
        Some(&manifest_path),
    );

    println!("All tests injected.");
    println!("Now run coverage manually, for example:");
    println!(
        "  cd {} && cargo llvm-cov --tests --ignore-run-fail --branch --cobertura --output-path coverage.xml",
        project_dir.display()
    );
    println!(
        "  cd {} && cargo llvm-cov --tests --ignore-run-fail --branch --json --output-path coverage.json",
        project_dir.display()
    );
    println!("After parsing coverage, call restore_all_injected_files(...).");

    // keep `touched_files` somewhere, serialize if needed
}

pub fn gen_test_rate(project_dir: &Path, work_dir: &Path, integration: bool, is_pre: bool) {
    // add_ntest_dependency(work_dir);
    let map_path = work_dir.join("brinfo/name_map.json");
    let nmap: HashMap<String, String> =
        serde_json::from_str(&fs::read_to_string(&map_path).unwrap()).unwrap();
    if !integration {
        let mut test_gen_infos = get_test_gen_infos(project_dir, is_pre);
        let mut test_rate_infos: Vec<TestRateInfo> = Vec::new();
        gen_coverage_and_pass_rate(project_dir, work_dir, &test_gen_infos, &mut test_rate_infos);
        dump_result_for_test_rate_infos(
            &test_rate_infos,
            project_dir,
            work_dir,
            is_pre,
            false,
            0.0,
        );
        if is_pre {
            gen_coverage_rate_for_original_tests(project_dir, work_dir, &test_gen_infos);
        }
    } else {
        let mut test_gen_infos = get_test_gen_infos(project_dir, is_pre);
        let integration_infos = gen_integration(&test_gen_infos, project_dir, work_dir);
        let mut test_rate_infos: Vec<TestRateInfo> = Vec::new();
        let test_type = TestType::CoverageRate;
        let start_time = SystemTime::now();
        let _ = target_clean(work_dir);
        let test_output = run_test(project_dir, work_dir, test_type, true, true);
        let end_time = SystemTime::now();
        let test_time = end_time.duration_since(start_time).unwrap().as_secs_f64();
        for integration_info in integration_infos.iter() {
            for test_gen_info in test_gen_infos.iter() {
                let function_name = test_gen_info.get_name().to_string();
                if function_name == integration_info.function_name {
                    let file_rela = test_gen_info.get_loc().get_file();
                    let file_path = project_dir.join(file_rela);
                    let mut test_rate_info = TestRateInfo {
                        function_name: function_name.clone(),
                        file_path: file_path.to_string_lossy().to_string(),
                        work_dir: work_dir.to_string_lossy().to_string(),
                        tests: integration_info.tests,
                        tests_lines: integration_info.tests_lines.clone(),
                        oracles: integration_info.oracles,
                        oracles_compiled: integration_info.oracles_compiled,
                        oracles_compiled_rate: integration_info.oracles_compiled_rate,
                        tests_compiled: integration_info.tests_compiled,
                        tests_compiled_rate: integration_info.tests_compiled_rate,
                        oracles_run: 0,
                        oracles_passed: 0,
                        oracles_passed_rate: 0.0,
                        tests_run: 0,
                        tests_passed: 0,
                        tests_passed_rate: 0.0,
                        lines: 0,
                        lines_covered: 0,
                        lines_coveraged_rate: 0.0,
                        branches: 0,
                        branches_covered: 0,
                        branches_coverage_rate: 0.0,
                        codes_lines: Vec::new(),
                        codes_lines_covered: Vec::new(),
                        codes_branches: Vec::new(),
                        codes_branches_covered: Vec::new(),
                    };
                    info!("Generate coverage for {}", function_name);
                    let loc = test_gen_info.get_loc();
                    let file_rela = test_gen_info.get_file();
                    let file_path = project_dir.join(&file_rela);
                    let begin = loc.get_startline() as i32;
                    let end = loc.get_endline() as i32;
                    gen_one_coverage_rate_lines(
                        project_dir,
                        work_dir,
                        &function_name,
                        &file_path,
                        begin,
                        end,
                        &mut test_rate_info,
                    );
                    gen_one_coverage_rate_branches(
                        project_dir,
                        work_dir,
                        &function_name,
                        &file_path,
                        begin,
                        end,
                        &mut test_rate_info,
                    );
                    test_rate_infos.push(test_rate_info);
                    break;
                }
            }
        }
        for integration_info in integration_infos.iter() {
            let mut tests = integration_info.test_function_names.len() as i32;
            let mut passed_tests = 0;
            for test_function_name in integration_info.test_function_names.iter() {
                for line in test_output.iter() {
                    if line.contains(test_function_name) {
                        info!("Generate pass rate for {}", test_function_name);
                        let line_spilit = line
                            .split(" ... ")
                            .map(|line| line.to_string())
                            .collect::<Vec<String>>();
                        if line_spilit[1] == "ok" {
                            passed_tests = passed_tests + 1;
                        }
                        break;
                    }
                }
            }
            for already_test_rate_info in test_rate_infos.iter_mut() {
                if already_test_rate_info.function_name == integration_info.function_name {
                    already_test_rate_info.tests_run = tests;
                    already_test_rate_info.tests_passed = passed_tests;
                    already_test_rate_info.tests_passed_rate =
                        passed_tests as f64 / tests as f64 * 100.0;
                    break;
                }
            }
        }
        let _ = target_clean(work_dir);
        let test_path = work_dir.join("tests");
        let bak_test_path2 = work_dir.join("tests.bak2");
        fs::rename(&test_path, &bak_test_path2).unwrap();
        gen_codes_lines_and_branches_covered(
            project_dir,
            work_dir,
            &test_gen_infos,
            &mut test_rate_infos,
        );
        fs::rename(&bak_test_path2, &test_path).unwrap();
        dump_result_for_test_rate_infos(
            &test_rate_infos,
            project_dir,
            work_dir,
            is_pre,
            false,
            test_time,
        );
        if is_pre {
            gen_coverage_rate_for_original_tests(project_dir, work_dir, &test_gen_infos);
        }
    }
}

fn gen_one_coverage_rate_lines(
    project_dir: &Path,
    work_dir: &Path,
    function_name: &String,
    file_path: &Path,
    begin: i32,
    end: i32,
    test_rate_info: &mut TestRateInfo,
) {
    let relative_path = file_path.strip_prefix(project_dir).unwrap();
    let coverage_file_path = work_dir.join("coverage.xml");

    if exists(&coverage_file_path).unwrap() {
        let file = File::open(&coverage_file_path).unwrap();
        let reader = BufReader::new(file);

        let mut xml_reader = Reader::from_reader(reader);
        xml_reader.config_mut().trim_text(true);

        let mut buf = Vec::new();
        let mut in_target_file = false;
        let mut in_lines = false;
        let mut lines = 0;
        let mut lines_covered = 0;
        let mut lines_coverage_rate = 0.0;
        let mut codes_lines: Vec<i32> = Vec::new();

        while let Ok(event) = xml_reader.read_event_into(&mut buf) {
            match event {
                Event::Start(ref e) => match e.name().as_ref() {
                    b"class" => {
                        // println!("{:#?}", event);
                        if let Some(attr) = e
                            .attributes()
                            .filter_map(|a| a.ok())
                            .find(|a| a.key.as_ref() == b"filename")
                        {
                            let current_filename =
                                String::from_utf8_lossy(&attr.value).into_owned();
                            in_target_file = current_filename == relative_path.to_string_lossy();
                        }
                    }
                    _ => {
                        // println!("{:#?}", event);
                    }
                },
                Event::Empty(ref e) => match e.name().as_ref() {
                    b"line" if in_target_file && in_lines => {
                        // println!("{:#?}", event);
                        let mut number = 0;
                        let mut hits = 0;
                        let mut branch = String::new();
                        let mut condition_coverage = String::new();

                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            match attr.key.as_ref() {
                                b"number" => {
                                    number = String::from_utf8_lossy(&attr.value)
                                        .parse::<i32>()
                                        .unwrap_or(0);
                                }
                                b"hits" => {
                                    hits = String::from_utf8_lossy(&attr.value)
                                        .parse::<i32>()
                                        .unwrap_or(0);
                                }
                                b"branch" => {
                                    branch = String::from_utf8_lossy(&attr.value).to_string();
                                }
                                b"condition-coverage" => {
                                    condition_coverage =
                                        String::from_utf8_lossy(&attr.value).to_string();
                                }
                                _ => {}
                            }
                        }
                        if number >= begin && number <= end {
                            lines += 1;
                            codes_lines.push(number);
                            if hits > 0 {
                                lines_covered += 1;
                            }
                            // if branch == "true" {
                            //     let re = Regex::new(r"\((\d+)/(\d+)\)").unwrap();
                            //     if let Some(caps) = re.captures(&condition_coverage) {
                            //         let num1: i32 = caps[1].parse().unwrap();
                            //         let num2: i32 = caps[2].parse().unwrap();
                            //         branches += num2;
                            //         branches_covered += num1;
                            //         codes_branches.push((number, num2));
                            //     }
                            // }
                        }
                    }
                    _ => {}
                },
                Event::End(ref e) => {
                    // println!("{:#?}", event);
                    if e.name().as_ref() == b"methods" && in_target_file {
                        in_lines = true;
                    } else if e.name().as_ref() == b"lines" && in_target_file && in_lines {
                        break;
                    }
                }
                Event::Eof => break,
                _ => {
                    // println!("{:#?}", event);
                }
            }
            buf.clear();
        }

        // if lines_covered > 0 {
        //     branches_covered += 1;
        // }

        // 计算覆盖率
        lines_coverage_rate = if lines > 0 {
            lines_covered as f64 / lines as f64 * 100.0
        } else {
            0.0
        };

        // branches_coverage_rate = if branches > 0 {
        //     branches_covered as f64 / branches as f64 * 100.0
        // } else {
        //     0.0
        // };

        test_rate_info.lines = lines;
        test_rate_info.lines_covered = lines_covered;
        test_rate_info.lines_coveraged_rate = lines_coverage_rate;
        // test_rate_info.branches = branches;
        // test_rate_info.branches_covered = branches_covered;
        // test_rate_info.branches_coverage_rate = branches_coverage_rate;
        test_rate_info.codes_lines = codes_lines;
        // test_rate_info.codes_branches = codes_branches;
        remove_file(&coverage_file_path).unwrap();
    }
}

fn gen_one_coverage_rate_branches(
    project_dir: &Path,
    work_dir: &Path,
    function_name: &String,
    file_path: &Path,
    begin: i32,
    end: i32,
    test_rate_info: &mut TestRateInfo,
) {
    let coverage_file_path = work_dir.join("coverage.json");

    let mut file = File::open(&coverage_file_path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let mut coverage_json: CoverageJson = serde_json::from_str(&contents).unwrap();
    coverage_json.parse_coverage_branches();

    let mut codes_branches: Vec<BranchCoverageInfo> = Vec::new();

    for coverage_data in coverage_json.data.iter() {
        for coverage_file in coverage_data.files.iter() {
            if coverage_file.file_name == file_path.to_string_lossy() {
                for branch in coverage_file.coverage_branches.iter() {
                    if branch.start_line >= begin as usize && branch.end_line <= end as usize {
                        let branch_coverage_info = BranchCoverageInfo {
                            start_line: branch.start_line as i32,
                            start_column: branch.start_column as i32,
                            end_line: branch.end_line as i32,
                            end_column: branch.end_column as i32,
                            positive: branch.positive > 0,
                            negative: branch.negative > 0,
                        };
                        let mut has_codes_branch = false;
                        for codes_branch in codes_branches.iter_mut() {
                            if *codes_branch == branch_coverage_info {
                                has_codes_branch = true;
                                if branch_coverage_info.positive {
                                    codes_branch.positive = true;
                                }
                                if branch_coverage_info.negative {
                                    codes_branch.negative = true;
                                }
                                break;
                            }
                        }
                        if !has_codes_branch {
                            codes_branches.push(branch_coverage_info);
                        }
                    }
                }
                break;
            }
        }
    }

    let mut branches = 0;
    let mut branches_covered = 0;
    let mut branches_coverage_rate = 0.0;

    if codes_branches.len() > 0 {
        branches = 2 * codes_branches.len() as i32;
        for codes_branch in codes_branches.iter() {
            if codes_branch.positive {
                branches_covered += 1;
            }
            if codes_branch.negative {
                branches_covered += 1;
            }
        }
        branches_coverage_rate = if branches > 0 {
            branches_covered as f64 / branches as f64 * 100.0
        } else {
            0.0
        };
    } else {
        branches = 1;
        if test_rate_info.lines_covered > 0 {
            branches_covered = 1;
        }
        branches_coverage_rate = if branches > 0 {
            branches_covered as f64 / branches as f64 * 100.0
        } else {
            0.0
        };
    }

    test_rate_info.branches = branches;
    test_rate_info.branches_covered = branches_covered;
    test_rate_info.branches_coverage_rate = branches_coverage_rate;
    test_rate_info.codes_branches = codes_branches;

    fs::remove_file(&coverage_file_path).unwrap();
}

fn gen_one_pass_rate(test_output: &String) -> (i32, i32, f64) {
    // 初始化统计数据
    let mut total_passed = 0;
    let mut total_run = 0;
    let mut in_target = false;
    // 按行解析输出
    for line in test_output.lines() {
        if line.starts_with("running") && (line.ends_with("tests") || line.ends_with("test")) {
            let parts: Vec<&str> = line.split(" ").collect();
            let test_run_num: i32 = parts[1].parse().unwrap();
            if test_run_num > 0 {
                in_target = true;
                total_run = test_run_num;
                continue;
            }
        }
        if in_target {
            if line.starts_with("test result:") {
                let new_line = line.split_once(". ").unwrap().1;
                let parts: Vec<&str> = new_line.split(";").collect();
                let pass_parts: Vec<&str> = parts[0].split_whitespace().collect();
                total_passed = pass_parts[0].parse().unwrap();
                break;
            }
        }
    }

    // 计算总测试数和通过率
    let pass_rate = if total_run > 0 {
        (total_passed as f64 / total_run as f64) * 100.0
    } else {
        0.0
    };
    (total_run, total_passed, pass_rate)
}

fn gen_coverage_and_pass_rate(
    project_dir: &Path,
    work_dir: &Path,
    test_gen_infos: &Vec<TestGenInfo>,
    test_rate_infos: &mut Vec<TestRateInfo>,
) {
    let mut progress = 0;
    for test_gen_info in test_gen_infos.iter() {
        let file_rela = test_gen_info.get_file();
        let file_path = project_dir.join(&file_rela);
        if (project_dir == work_dir && !file_rela.starts_with("src"))
            || (project_dir != work_dir && !file_path.starts_with(work_dir))
        {
            continue;
        }
        progress += 1;
        let name = test_gen_info.get_name().to_string();
        let fn_name = name.split("::").last().unwrap();
        info!(
            "Generate coverage and pass rate for {}, progress {}/{}",
            fn_name,
            progress,
            test_gen_infos.len()
        );
        let insert_kind = test_gen_info.get_insert_kind();
        let template = include_str!("../../res/code_template.json");
        let code_template: Vec<String> = serde_json::from_str(template).unwrap();
        backup_file(Path::new(&file_path));

        let mut test_rate_info = TestRateInfo {
            function_name: name.clone(),
            file_path: file_path.to_string_lossy().to_string(),
            work_dir: work_dir.to_string_lossy().to_string(),
            tests: 0,
            tests_lines: Vec::new(),
            oracles: 0,
            oracles_compiled: 0,
            oracles_compiled_rate: 0.0,
            tests_compiled: 0,
            tests_compiled_rate: 0.0,
            oracles_run: 0,
            oracles_passed: 0,
            oracles_passed_rate: 0.0,
            tests_run: 0,
            tests_passed: 0,
            tests_passed_rate: 0.0,
            lines: 0,
            lines_covered: 0,
            lines_coveraged_rate: 0.0,
            branches: 0,
            branches_covered: 0,
            branches_coverage_rate: 0.0,
            codes_lines: Vec::new(),
            codes_lines_covered: Vec::new(),
            codes_branches: Vec::new(),
            codes_branches_covered: Vec::new(),
        };

        let mut tests = 0;
        let mut tests_lines: Vec<i32> = Vec::new();
        let mut oracles = 0;
        let mut oracles_compiled = 0;
        let mut oracles_compiled_rate = 0.0;
        let mut tests_compiled = 0;
        let mut tests_compiled_rate = 0.0;

        let mut oracles_run = 0;
        let mut oracles_passed = 0;
        let mut oracles_passed_rate = 0.0;
        let mut tests_run = 0;
        let mut tests_passed = 0;
        let mut tests_passed_rate = 0.0;

        for chain_test in test_gen_info.get_tests().iter() {
            for answer in chain_test.get_answers().iter() {
                for test in answer.get_tests().iter() {
                    oracles += 1;
                    let mut has_oracle_compiled = false;
                    let mut has_oracles_run = false;
                    let mut has_oracles_passed = false;
                    for (num, test_code) in test.codes.iter().enumerate() {
                        tests += 1;
                        tests_lines.push(test_code.len() as i32);
                        if test.can_compile[num].is_ok() {
                            tests_compiled += 1;
                            has_oracle_compiled = true;
                            let mut mod_code = code_template.clone();
                            let mut common = answer.get_common().clone();
                            common.push("".to_string());
                            let pos = mod_code.len() - 1;
                            mod_code.splice(pos..pos, common);
                            let sig = format!("fn test_{}_{:02}()", fn_name, num);
                            info!("Running test fn test_{}_{:02}()", fn_name, tests);
                            let mut fn_code =
                                vec!["#[test]".to_string()];
                            fn_code.extend(test.attrs.clone().iter().map(|attr| {
                                if attr.contains("#[should_panic(") {
                                    return "#[should_panic]".to_string();
                                } else {
                                    attr.clone()
                                }
                            }));
                            fn_code.push(sig);
                            fn_code.extend(test_code.clone());
                            let pos = mod_code.len() - 1;
                            mod_code.splice(pos..pos, fn_code);
                            insert_test(insert_kind, Path::new(&file_path), &mod_code);
                            // let _ = target_clean(work_dir);
                            let test_type = TestType::CoverageRate;
                            let test_name = format!("test_{}", fn_name);
                            let run_test_output =
                                run_test(project_dir, work_dir, test_type, true, false);
                            let loc = test_gen_info.get_loc();
                            let begin = loc.get_startline() as i32;
                            let end = loc.get_endline() as i32;
                            gen_one_codes_lines(
                                project_dir,
                                work_dir,
                                &name,
                                &file_path,
                                begin,
                                end,
                                &test_code,
                                &mut test_rate_info,
                            );
                            gen_one_codes_branchs(
                                project_dir,
                                work_dir,
                                &name,
                                &file_path,
                                begin,
                                end,
                                &test_code,
                                &mut test_rate_info,
                            );
                            // println!("{:#?}", run_test_output);
                            let reg = Regex::new(r"running (\d+) test").unwrap();
                            let rep = Regex::new(r"(\d+) passed;").unwrap();

                            let mut this_tests_run = 0;
                            let mut this_tests_passed = 0;
                            for output in run_test_output.iter() {
                                if let Some(caps) = reg.captures(&output) {
                                    this_tests_run +=
                                        caps.get(1).unwrap().as_str().parse::<i32>().unwrap();
                                }
                                if let Some(caps) = rep.captures(&output) {
                                    this_tests_passed +=
                                        caps.get(1).unwrap().as_str().parse::<i32>().unwrap();
                                }
                            }
                            if this_tests_run > 1 {
                                panic!("There are more than one running tests");
                                println!("{:#?}", run_test_output);
                                has_oracles_run = false;
                                this_tests_passed = 0;
                            }
                            if this_tests_run == 1 {
                                has_oracles_run = true;
                            }
                            if this_tests_passed == 1 {
                                has_oracles_passed = true;
                            }
                            tests_run += this_tests_run;
                            tests_passed += this_tests_passed;

                            restore_file(Path::new(&file_path));
                        }
                    }
                    if has_oracle_compiled {
                        oracles_compiled += 1;
                        has_oracle_compiled = false;
                    }
                    if has_oracles_run {
                        oracles_run += 1;
                        has_oracles_run = false;
                    }
                    if has_oracles_passed {
                        oracles_passed += 1;
                        has_oracles_passed = false;
                    }
                }
            }
        }
        if tests_run == 0 {
            let mut mod_code = code_template.clone();
            let sig = format!("fn test_{}()", fn_name,);
            info!("Running test fn test_{}_{:02}()", fn_name, tests);
            let mut fn_code = vec!["#[test]".to_string()];
            fn_code.push(sig);
            let empty_codes = vec!["{".to_string(), "}".to_string()];
            fn_code.extend(empty_codes.clone());
            let pos = mod_code.len() - 1;
            mod_code.splice(pos..pos, fn_code);
            insert_test(insert_kind, Path::new(&file_path), &mod_code);
            // let _ = target_clean(work_dir);
            let test_type = TestType::CoverageRate;
            let test_name = format!("test_{}", fn_name);
            let run_test_output = run_test(project_dir, work_dir, test_type, true, false);
            let loc = test_gen_info.get_loc();
            let begin = loc.get_startline() as i32;
            let end = loc.get_endline() as i32;
            gen_one_codes_lines(
                project_dir,
                work_dir,
                &name,
                &file_path,
                begin,
                end,
                &empty_codes,
                &mut test_rate_info,
            );
            gen_one_codes_branchs(
                project_dir,
                work_dir,
                &name,
                &file_path,
                begin,
                end,
                &empty_codes,
                &mut test_rate_info,
            );
            restore_file(&file_path);
        }
        delete_backup(&file_path);
        tests_compiled_rate = if tests > 0 {
            tests_compiled as f64 / tests as f64 * 100.0
        } else {
            0.0
        };
        oracles_compiled_rate = if oracles > 0 {
            oracles_compiled as f64 / oracles as f64 * 100.0
        } else {
            0.0
        };
        tests_passed_rate = if tests_run > 0 {
            tests_passed as f64 / tests_run as f64 * 100.0
        } else {
            0.0
        };
        oracles_passed_rate = if oracles_run > 0 {
            oracles_passed as f64 / oracles_run as f64 * 100.0
        } else {
            0.0
        };

        test_rate_info.tests = tests;
        test_rate_info.tests_lines = tests_lines;
        test_rate_info.oracles = oracles;
        test_rate_info.oracles_compiled = oracles_compiled;
        test_rate_info.oracles_compiled_rate = oracles_compiled_rate;
        test_rate_info.tests_compiled = tests_compiled;
        test_rate_info.tests_compiled_rate = tests_compiled_rate;
        test_rate_info.tests_run = tests_run;
        test_rate_info.tests_passed = tests_passed;
        test_rate_info.tests_passed_rate = tests_passed_rate;
        test_rate_info.oracles_run = oracles_run;
        test_rate_info.oracles_passed = oracles_passed;
        test_rate_info.oracles_passed_rate = oracles_passed_rate;
        test_rate_info.parse_codes_lines_and_branches();
        test_rate_infos.push(test_rate_info);
    }
}

fn gen_one_codes_lines(
    project_dir: &Path,
    work_dir: &Path,
    function_name: &String,
    file_path: &Path,
    begin: i32,
    end: i32,
    codes: &Vec<String>,
    test_rate_info: &mut TestRateInfo,
) {
    let relative_path = file_path.strip_prefix(project_dir).unwrap();
    let coverage_file_path = work_dir.join("coverage.xml");
    let mut codes_lines: Vec<i32> = Vec::new();

    if exists(&coverage_file_path).unwrap() {
        let file = File::open(&coverage_file_path).unwrap();
        let reader = BufReader::new(file);

        let mut xml_reader = Reader::from_reader(reader);
        xml_reader.config_mut().trim_text(true);

        let mut buf = Vec::new();
        let mut in_target_file = false;
        let mut in_lines = false;
        let mut codes_lines_covered: Vec<i32> = Vec::new();
        while let Ok(event) = xml_reader.read_event_into(&mut buf) {
            match event {
                Event::Start(ref e) => match e.name().as_ref() {
                    b"class" => {
                        // println!("{:#?}", event);
                        if let Some(attr) = e
                            .attributes()
                            .filter_map(|a| a.ok())
                            .find(|a| a.key.as_ref() == b"filename")
                        {
                            let current_filename =
                                String::from_utf8_lossy(&attr.value).into_owned();
                            in_target_file = current_filename == relative_path.to_string_lossy();
                        }
                    }
                    _ => {
                        // println!("{:#?}", event);
                    }
                },
                Event::Empty(ref e) => match e.name().as_ref() {
                    b"line" if in_target_file && in_lines => {
                        // println!("{:#?}", event);
                        let mut number = 0;
                        let mut hits = 0;
                        let mut branch = String::new();
                        let mut condition_coverage = String::new();

                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            match attr.key.as_ref() {
                                b"number" => {
                                    number = String::from_utf8_lossy(&attr.value)
                                        .parse::<i32>()
                                        .unwrap_or(0);
                                }
                                b"hits" => {
                                    hits = String::from_utf8_lossy(&attr.value)
                                        .parse::<i32>()
                                        .unwrap_or(0);
                                }
                                b"branch" => {
                                    branch = String::from_utf8_lossy(&attr.value).to_string();
                                }
                                b"condition-coverage" => {
                                    condition_coverage =
                                        String::from_utf8_lossy(&attr.value).to_string();
                                }
                                _ => {}
                            }
                        }
                        if number >= begin && number <= end {
                            if test_rate_info.codes_lines.is_empty() {
                                codes_lines.push(number);
                            }
                            if hits > 0 {
                                codes_lines_covered.push(number);
                            }
                        }
                    }
                    _ => {}
                },
                Event::End(ref e) => {
                    // println!("{:#?}", event);
                    if e.name().as_ref() == b"methods" && in_target_file {
                        in_lines = true;
                    } else if e.name().as_ref() == b"lines" && in_target_file && in_lines {
                        break;
                    }
                }
                Event::Eof => break,
                _ => {
                    // println!("{:#?}", event);
                }
            }
            buf.clear();
        }
        if test_rate_info.codes_lines.is_empty() {
            test_rate_info.codes_lines = codes_lines;
        }
        test_rate_info
            .codes_lines_covered
            .push((codes.clone(), codes_lines_covered));
        remove_file(&coverage_file_path).unwrap();
    }
}

fn gen_one_codes_branchs(
    project_dir: &Path,
    work_dir: &Path,
    function_name: &String,
    file_path: &Path,
    begin: i32,
    end: i32,
    codes: &Vec<String>,
    test_rate_info: &mut TestRateInfo,
) {
    let coverage_file_path = work_dir.join("coverage.json");

    let mut file = File::open(&coverage_file_path).expect(format!("{}()", function_name).as_str());
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let mut coverage_json: CoverageJson = serde_json::from_str(&contents).unwrap();
    coverage_json.parse_coverage_branches();

    let mut codes_branches: Vec<BranchCoverageInfo> = Vec::new();

    for coverage_data in coverage_json.data.iter() {
        for coverage_file in coverage_data.files.iter() {
            if coverage_file.file_name == file_path.to_string_lossy() {
                for branch in coverage_file.coverage_branches.iter() {
                    if branch.start_line >= begin as usize && branch.end_line <= end as usize {
                        let branch_coverage_info = BranchCoverageInfo {
                            start_line: branch.start_line as i32,
                            start_column: branch.start_column as i32,
                            end_line: branch.end_line as i32,
                            end_column: branch.end_column as i32,
                            positive: branch.positive > 0,
                            negative: branch.negative > 0,
                        };
                        let mut has_codes_branch = false;
                        for codes_branch in codes_branches.iter_mut() {
                            if *codes_branch == branch_coverage_info {
                                has_codes_branch = true;
                                if branch_coverage_info.positive {
                                    codes_branch.positive = true;
                                }
                                if branch_coverage_info.negative {
                                    codes_branch.negative = true;
                                }
                                break;
                            }
                        }
                        if !has_codes_branch {
                            codes_branches.push(branch_coverage_info);
                        }
                    }
                }
                break;
            }
        }
    }

    test_rate_info
        .codes_branches_covered
        .push((codes.clone(), codes_branches));

    fs::remove_file(&coverage_file_path).unwrap();
}

fn gen_codes_lines_and_branches_covered(
    project_dir: &Path,
    work_dir: &Path,
    test_gen_infos: &Vec<TestGenInfo>,
    test_rate_infos: &mut Vec<TestRateInfo>,
) {
    for test_rate_info in test_rate_infos.iter_mut() {
        let function_name = test_rate_info.function_name.clone();
        for test_gen_info in test_gen_infos.iter() {
            if function_name != test_gen_info.get_name().to_string() {
                continue;
            }
            let file_rela = test_gen_info.get_file();
            let file_path = project_dir.join(&file_rela);
            let name = test_gen_info.get_name().to_string();
            let fn_name = name.split("::").last().unwrap();
            info!("Generate codes lines and branches for {}", fn_name);
            let insert_kind = test_gen_info.get_insert_kind();
            let template = include_str!("../../res/code_template.json");
            let code_template: Vec<String> = serde_json::from_str(template).unwrap();
            backup_file(Path::new(&file_path));
            for chain_test in test_gen_info.get_tests().iter() {
                for answer in chain_test.get_answers().iter() {
                    for test in answer.get_tests().iter() {
                        for (num, test_code) in test.codes.iter().enumerate() {
                            if test.can_compile[num].is_ok() {
                                let mut mod_code = code_template.clone();
                                let mut common = answer.get_common().clone();
                                common.push("".to_string());
                                let pos = mod_code.len() - 1;
                                mod_code.splice(pos..pos, common);
                                let sig = format!("fn test_{}_{:02}()", fn_name, num);
                                info!("Running test fn test_{}_{:02}()", fn_name, num);
                                let mut fn_code =
                                    vec!["#[test]".to_string()];
                                fn_code.extend(test.attrs.clone().iter().map(|attr| {
                                    if attr.contains("#[should_panic(") {
                                        return "#[should_panic]".to_string();
                                    } else {
                                        attr.clone()
                                    }
                                }));
                                fn_code.push(sig);
                                fn_code.extend(test_code.clone());
                                let pos = mod_code.len() - 1;
                                mod_code.splice(pos..pos, fn_code);
                                insert_test(insert_kind, Path::new(&file_path), &mod_code);
                                // let _ = target_clean(work_dir);
                                let test_type = TestType::CoverageRate;
                                let test_name = format!("test_{}", fn_name);
                                run_test(project_dir, work_dir, test_type, true, false);
                                let loc = test_gen_info.get_loc();
                                let begin = loc.get_startline() as i32;
                                let end = loc.get_endline() as i32;
                                gen_one_codes_lines(
                                    project_dir,
                                    work_dir,
                                    &name,
                                    &file_path,
                                    begin,
                                    end,
                                    &test_code,
                                    test_rate_info,
                                );
                                gen_one_codes_branchs(
                                    project_dir,
                                    work_dir,
                                    &function_name,
                                    &file_path,
                                    begin,
                                    end,
                                    &test_code,
                                    test_rate_info,
                                );
                                restore_file(Path::new(&file_path));
                            }
                        }
                    }
                }
            }
            delete_backup(&file_path);
            break;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OriginalTestRateInfo {
    function_name: String,
    lines: Vec<i32>,
    lines_covered: Vec<i32>,
    branches_with_covered: Vec<BranchCoverageInfo>,
}

impl OriginalTestRateInfo {
    fn new() -> Self {
        OriginalTestRateInfo {
            function_name: String::new(),
            lines: Vec::new(),
            lines_covered: Vec::new(),
            branches_with_covered: Vec::new(),
        }
    }
}

fn gen_one_lines_for_original_tests(
    project_dir: &Path,
    work_dir: &Path,
    function_name: &String,
    file_path: &Path,
    begin: i32,
    end: i32,
    original_test_rate_info: &mut OriginalTestRateInfo,
) {
    let relative_path = file_path.strip_prefix(project_dir).unwrap();
    let coverage_file_path = work_dir.join("coverage.xml");

    if exists(&coverage_file_path).unwrap() {
        let file = File::open(&coverage_file_path).unwrap();
        let reader = BufReader::new(file);

        let mut xml_reader = Reader::from_reader(reader);
        xml_reader.config_mut().trim_text(true);

        let mut buf = Vec::new();

        let mut in_target_file = false;
        let mut in_lines = false;
        let mut lines: Vec<i32> = Vec::new();
        let mut lines_covered: Vec<i32> = Vec::new();

        while let Ok(event) = xml_reader.read_event_into(&mut buf) {
            match event {
                Event::Start(ref e) => match e.name().as_ref() {
                    b"class" => {
                        // println!("{:#?}", event);
                        if let Some(attr) = e
                            .attributes()
                            .filter_map(|a| a.ok())
                            .find(|a| a.key.as_ref() == b"filename")
                        {
                            let current_filename =
                                String::from_utf8_lossy(&attr.value).into_owned();
                            in_target_file = current_filename == relative_path.to_string_lossy();
                        }
                    }
                    _ => {
                        // println!("{:#?}", event);
                    }
                },
                Event::Empty(ref e) => match e.name().as_ref() {
                    b"line" if in_target_file && in_lines => {
                        // println!("{:#?}", event);
                        let mut number = 0;
                        let mut hits = 0;
                        let mut branch = String::new();
                        let mut condition_coverage = String::new();

                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            match attr.key.as_ref() {
                                b"number" => {
                                    number = String::from_utf8_lossy(&attr.value)
                                        .parse::<i32>()
                                        .unwrap_or(0);
                                }
                                b"hits" => {
                                    hits = String::from_utf8_lossy(&attr.value)
                                        .parse::<i32>()
                                        .unwrap_or(0);
                                }
                                b"branch" => {
                                    branch = String::from_utf8_lossy(&attr.value).to_string();
                                }
                                b"condition-coverage" => {
                                    condition_coverage =
                                        String::from_utf8_lossy(&attr.value).to_string();
                                }
                                _ => {}
                            }
                        }
                        if number >= begin && number <= end {
                            lines.push(number);
                            if hits > 0 {
                                lines_covered.push(number);
                            }
                            // if branch == "true" {
                            //     let re = Regex::new(r"\((\d+)/(\d+)\)").unwrap();
                            //     if let Some(caps) = re.captures(&condition_coverage) {
                            //         let num1: i32 = caps[1].parse().unwrap();
                            //         let num2: i32 = caps[2].parse().unwrap();
                            //         branches.push((number, num2));
                            //     }
                            // }
                        }
                    }
                    _ => {}
                },
                Event::End(ref e) => {
                    // println!("{:#?}", event);
                    if e.name().as_ref() == b"methods" && in_target_file {
                        in_lines = true;
                    } else if e.name().as_ref() == b"lines" && in_target_file && in_lines {
                        break;
                    }
                }
                Event::Eof => break,
                _ => {
                    // println!("{:#?}", event);
                }
            }
            buf.clear();
        }
        original_test_rate_info.lines = lines;
        original_test_rate_info.lines_covered = lines_covered;
        // remove_file(&coverage_file_path).unwrap();
    }
}

fn gen_one_branches_for_original_tests(
    project_dir: &Path,
    work_dir: &Path,
    function_name: &String,
    file_path: &Path,
    begin: i32,
    end: i32,
    original_test_rate_info: &mut OriginalTestRateInfo,
) {
    let coverage_file_path = work_dir.join("coverage.json");

    let mut file = File::open(&coverage_file_path).unwrap();
    let mut contents = String::new();
    file.read_to_string(&mut contents).unwrap();
    let mut coverage_json: CoverageJson = serde_json::from_str(&contents).unwrap();
    coverage_json.parse_coverage_branches();

    let mut branches_with_covered: Vec<BranchCoverageInfo> = Vec::new();

    for coverage_data in coverage_json.data.iter() {
        for coverage_file in coverage_data.files.iter() {
            if coverage_file.file_name == file_path.to_string_lossy() {
                for branch in coverage_file.coverage_branches.iter() {
                    if branch.start_line >= begin as usize && branch.end_line <= end as usize {
                        let branch_coverage_info = BranchCoverageInfo {
                            start_line: branch.start_line as i32,
                            start_column: branch.start_column as i32,
                            end_line: branch.end_line as i32,
                            end_column: branch.end_column as i32,
                            positive: branch.positive > 0,
                            negative: branch.negative > 0,
                        };
                        let mut has_codes_branch = false;
                        for codes_branch in branches_with_covered.iter_mut() {
                            if *codes_branch == branch_coverage_info {
                                has_codes_branch = true;
                                if branch_coverage_info.positive {
                                    codes_branch.positive = true;
                                }
                                if branch_coverage_info.negative {
                                    codes_branch.negative = true;
                                }
                                break;
                            }
                        }
                        if !has_codes_branch {
                            branches_with_covered.push(branch_coverage_info);
                        }
                    }
                }
                break;
            }
        }
    }
    original_test_rate_info.branches_with_covered = branches_with_covered;
}

fn gen_coverage_rate_for_original_tests(
    project_dir: &Path,
    work_dir: &Path,
    test_gen_infos: &Vec<TestGenInfo>,
) {
    let mut original_test_rate_infos: Vec<OriginalTestRateInfo> = Vec::new();
    let test_dir = work_dir.join("tests");
    let bak_test_dir = work_dir.join("tests.bak");
    if exists(&bak_test_dir).unwrap() {
        info!("Generate coverage rate for original tests");
        fs::rename(&bak_test_dir, &test_dir).unwrap();
        let _ = target_clean(work_dir);
        let test_type = TestType::CoverageRate;
        run_test(project_dir, work_dir, test_type, true, false);
        for test_gen_info in test_gen_infos.iter() {
            let function_name = test_gen_info.get_name().to_string();
            let file_rela = test_gen_info.get_file();
            let file_path = project_dir.join(&file_rela);
            let loc = test_gen_info.get_loc();
            let begin = loc.get_startline() as i32;
            let end = loc.get_endline() as i32;
            let mut original_test_rate_info = OriginalTestRateInfo::new();
            original_test_rate_info.function_name = function_name.clone();
            gen_one_lines_for_original_tests(
                project_dir,
                work_dir,
                &function_name,
                &file_path,
                begin,
                end,
                &mut original_test_rate_info,
            );
            gen_one_branches_for_original_tests(
                project_dir,
                work_dir,
                &function_name,
                &file_path,
                begin,
                end,
                &mut original_test_rate_info,
            );
            original_test_rate_infos.push(original_test_rate_info);
        }
        fs::rename(&test_dir, &bak_test_dir).unwrap();
        dump_result_for_original_test_rate_infos(&original_test_rate_infos, project_dir, work_dir);
        let coverage_file_path = work_dir.join("coverage.xml");
        let lcov_file_path = work_dir.join("coverage.json");
        fs::remove_file(&coverage_file_path).unwrap();
        fs::remove_file(&lcov_file_path).unwrap();
    }
}

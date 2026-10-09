use super::super::coverage::collect_coverage_for_tests;
use super::*;

const SOURCE: &str = r#"pub fn ordinary(value: i32) -> i32 {
    if value > 0 { 1 } else { 0 }
}
pub fn conditional(value: i32) -> i32 {
    #[cfg(test)]
    { value + 1 }
    #[cfg(not(test))]
    { if value > 0 { 1 } else { 0 } }
}
#[cfg(not(test))]
pub fn production_only(value: i32) -> i32 {
    if value > 0 { 1 } else { 0 }
}
pub fn generic<T: Ord>(a: T, b: T) -> T {
    if a > b { a } else { b }
}
pub struct Container<T>(pub T);
impl<T> Container<T> {
    pub fn get(&self) -> &T { &self.0 }
}
#[test]
fn must_not_run() { panic!("baseline executed an existing test"); }
"#;

fn fixture(directory: &Path, source: &str, binary: bool) -> PathBuf {
    fs::create_dir_all(directory.join("src")).unwrap();
    fs::write(directory.join("Cargo.toml"), "[package]\nname = \"baseline_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[workspace]\n").unwrap();
    fs::write(
        directory.join("rust-toolchain.toml"),
        include_str!("../../../rust-toolchain.toml"),
    )
    .unwrap();
    let file = directory.join(if binary { "src/main.rs" } else { "src/lib.rs" });
    fs::write(&file, source).unwrap();
    file
}

fn range(marker: &str) -> (i32, i32) {
    let start = SOURCE.find(marker).unwrap();
    let end = start + SOURCE[start..].find("\n}").unwrap();
    (
        SOURCE[..start].lines().count() as i32 + 1,
        SOURCE[..end].lines().count() as i32 + 1,
    )
}

#[test]
#[ignore = "requires the pinned cargo-llvm-cov and llvm-tools-preview"]
fn coverage_baselines_match_test_build_modes() {
    let scratch = Scratch::new().unwrap();
    let work = scratch.0.canonicalize().unwrap();
    let file = fixture(&work, SOURCE, false);
    fs::create_dir(work.join("tests")).unwrap();
    let helper = work.join(format!("tests/{BASELINE_TARGET}.rs"));
    fs::write(&helper, "// existing file must survive\n").unwrap();
    let markers = [
        "pub fn ordinary",
        "pub fn conditional",
        "pub fn production_only",
        "pub fn generic",
        "impl<T> Container",
    ];
    let mut unit = BaselineCache::new(&work, false).unwrap();
    let mut integration = BaselineCache::new(&work, true).unwrap();
    let mut unit_maps = Vec::new();
    let mut integration_maps = Vec::new();
    for marker in markers {
        let (begin, end) = range(marker);
        unit_maps.push(
            unit.mapping(&work, "baseline_fixture::focal", &file, begin, end)
                .unwrap(),
        );
        integration_maps.push(
            integration
                .mapping(&work, "baseline_fixture::focal", &file, begin, end)
                .unwrap(),
        );
    }
    assert_eq!(fs::read_to_string(&file).unwrap(), SOURCE);
    assert_eq!(
        fs::read_to_string(&helper).unwrap(),
        "// existing file must survive\n"
    );
    assert_eq!(unit.reports.len(), 1);
    assert_eq!(integration.reports.len(), 1);
    for index in [0, 3, 4] {
        assert!(!unit_maps[index].lines.is_empty());
        assert_eq!(unit_maps[index], integration_maps[index]);
    }
    assert_eq!(unit_maps[1].branches.len(), 0);
    assert_eq!(integration_maps[1].branches.len(), 1);
    assert_eq!(unit_maps[2], FunctionMapping::default());
    assert!(!integration_maps[2].lines.is_empty());
    // Outside the focal range is absent even when the file has coverage.
    assert_eq!(
        unit.mapping(&work, "baseline_fixture::absent", &file, 100, 110)
            .unwrap(),
        FunctionMapping::default()
    );

    // Compare the ordinary-library object maps with a real integration run,
    // including generic instantiations and cfg(not(test)) code.
    let calls = work.join("tests/calls.rs");
    fs::write(&calls, "use baseline_fixture::*;\n#[test]\nfn calls() { assert_eq!(ordinary(1), 1); assert_eq!(conditional(1), 1); assert_eq!(production_only(1), 1); assert_eq!(generic(1, 2), 2); assert_eq!(*Container(3).get(), 3); }\n").unwrap();
    collect_coverage_for_tests(&work, true, &["calls".into()]).unwrap();
    let real_lcov = checked(
        Command::new("cargo")
            .args(["llvm-cov", "report", "--lcov"])
            .current_dir(&work),
    )
    .unwrap();
    let real = parse_report(
        &String::from_utf8(real_lcov.stdout).unwrap(),
        &fs::read_to_string(work.join("coverage.json")).unwrap(),
    )
    .unwrap();
    for (marker, expected) in markers.into_iter().zip(&integration_maps) {
        let (begin, end) = range(marker);
        assert_eq!(&real.function(&file, begin, end), expected, "{marker}");
    }
    // A subsequent baseline must not add profiles to the real run.
    let before = fs::read_to_string(work.join("coverage.json")).unwrap();
    let mut fresh = BaselineCache::new(&work, true).unwrap();
    fresh
        .mapping(&work, "baseline_fixture::ordinary", &file, 1, 3)
        .unwrap();
    checked(
        Command::new("cargo")
            .args([
                "llvm-cov",
                "report",
                "--json",
                "--output-path",
                "after.json",
            ])
            .current_dir(&work),
    )
    .unwrap();
    assert_eq!(before, fs::read_to_string(work.join("after.json")).unwrap());

    // Build errors restore temporary source annotations and the helper file.
    let broken = format!("{SOURCE}\ncompile_error!(\"baseline build failure\");\n");
    fs::write(&file, &broken).unwrap();
    let mut failing = BaselineCache::new(&work, true).unwrap();
    let error = failing
        .mapping(&work, "baseline_fixture::ordinary", &file, 1, 3)
        .unwrap_err();
    assert!(error.to_string().contains("baseline build failure"));
    assert_eq!(fs::read_to_string(&file).unwrap(), broken);
    assert_eq!(
        fs::read_to_string(&helper).unwrap(),
        "// existing file must survive\n"
    );
    // Existing cache entries do not rebuild on each focal function.
    assert_eq!(
        unit.mapping(&work, "baseline_fixture::ordinary", &file, 1, 3)
            .unwrap(),
        unit_maps[0]
    );

    // A standalone binary uses its test harness, never executes main, and
    // rejects function-level integration mode before needing a baseline.
    let bin = work.join("binary");
    let bin_source = format!("{SOURCE}\nfn main() {{ panic!(\"baseline executed main\"); }}\n");
    let bin_file = fixture(&bin, &bin_source, true);
    let mut binary = BaselineCache::new(&bin, false).unwrap();
    assert_eq!(
        binary
            .mapping(&bin, "baseline_fixture::ordinary", &bin_file, 1, 3)
            .unwrap(),
        unit_maps[0]
    );
    assert_eq!(fs::read_to_string(&bin_file).unwrap(), bin_source);
    assert!(
        select_target(
            &binary.metadata,
            "baseline_fixture::ordinary",
            &bin_file,
            true
        )
        .unwrap_err()
        .to_string()
        .contains("require a library")
    );

    // Same-name library and binary modules cannot be selected by crate name.
    fs::write(work.join("src/main.rs"), "fn main() {}\n").unwrap();
    let mixed = project_metadata(&work).unwrap();
    assert!(
        select_target(
            &mixed,
            "baseline_fixture::shared::f",
            &work.join("src/shared.rs"),
            false
        )
        .is_err()
    );
    assert!(
        select_target(&mixed, "baseline_fixture::ordinary", &file, false)
            .unwrap()
            .is_kind(TargetKind::Lib)
    );

    let cfg_only = work.join("cfg-only");
    let cfg_file = fixture(
        &cfg_only,
        "#[cfg(not(test))]\npub fn production_only() -> i32 { 1 }\n",
        false,
    );
    let mut absent = BaselineCache::new(&cfg_only, false).unwrap();
    assert_eq!(
        absent
            .mapping(
                &cfg_only,
                "baseline_fixture::production_only",
                &cfg_file,
                1,
                2
            )
            .unwrap(),
        FunctionMapping::default()
    );
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageBranch {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub positive: usize,
    pub negative: usize,
    pub unused1: usize,
    pub unused2: usize,
    pub unused3: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageFile {
    #[serde(rename = "filename")]
    pub file_name: String,
    branches: Vec<Vec<usize>>,
    #[serde(skip_deserializing)]
    pub coverage_branches: Vec<CoverageBranch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageData {
    pub files: Vec<CoverageFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageJson {
    pub data: Vec<CoverageData>,
}

impl CoverageJson {
    pub fn parse_coverage_branches(&mut self) {
        for coverage_data in self.data.iter_mut() {
            for coverage_file in coverage_data.files.iter_mut() {
                for branch in coverage_file.branches.iter() {
                    let coverage_branch = CoverageBranch {
                        start_line: branch[0],
                        start_column: branch[1],
                        end_line: branch[2],
                        end_column: branch[3],
                        positive: branch[4],
                        negative: branch[5],
                        unused1: branch[6],
                        unused2: branch[7],
                        unused3: branch[8],
                    };
                    coverage_file.coverage_branches.push(coverage_branch);
                }
            }
        }
    }
}

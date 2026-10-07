use rustc_span::source_map::SourceMap;
use rustc_span::FileName;
use std::fmt::Debug;
use std::fs::File;
use std::io::{BufRead, BufReader};

#[derive(Clone, Hash, PartialEq, PartialOrd, Eq, Ord)]
pub struct SourceInfo {
    file_path: String,
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
}

impl SourceInfo {
    pub fn from_span(span: rustc_span::Span, source_map: &SourceMap) -> Self {
        let start = source_map.lookup_char_pos(span.lo());
        let end = source_map.lookup_char_pos(span.hi());

        let file_path = match &start.file.name {
            FileName::Real(realname) => {
                format!("{}", realname.local_path_if_available().display())
            }
            _ => {
                error!("{:?} is NOT a real path.", start.file.name);
                String::new()
            }
        };

        SourceInfo {
            file_path,
            start_line: start.line,
            start_column: start.col.0 + 1,
            end_line: end.line,
            end_column: end.col.0 + 1,
        }
    }

    pub fn get_file(&self) -> String {
        self.file_path.clone()
    }

    pub fn get_startline(&self) -> usize {
        self.start_line
    }

    pub fn get_startcolumn(&self) -> usize {
        self.start_column
    }

    pub fn get_endline(&self) -> usize {
        self.end_line
    }

    pub fn get_endcolumn(&self) -> usize {
        self.end_column
    }

    pub fn get_string(&self) -> String {
        if self.file_path.is_empty() {
            return String::new();
        }

        let file = File::open(&self.file_path).unwrap();
        let reader = BufReader::new(file);

        let mut result = String::new();
        for (line_number, line) in reader.lines().enumerate() {
            let line_number = line_number + 1; // Convert to 1-based index
            let line = line.unwrap();
            if line_number < self.start_line {
                continue;
            }
            if line_number > self.end_line {
                break;
            }
            let start_col = if line_number == self.start_line {
                self.start_column - 1
            } else {
                0
            };
            let end_col = if line_number == self.end_line {
                self.end_column - 1
            } else {
                line.len()
            };
            if start_col <= end_col && end_col <= line.len() {
                let snippet = line
                    .chars()
                    .skip(start_col)
                    .take(end_col - start_col)
                    .collect::<String>();
                result.push_str(&snippet);
            }
            if line_number < self.end_line {
                result.push('\n');
            }
        }
        result
    }

    pub fn substring_source_info(&self, start: usize, length: usize) -> SourceInfo {
        let original = self.get_string();
        let substring = &original[start..start + length];
        let mut current_line = self.start_line;
        let mut current_column = self.start_column;

        // Calculate the starting line and column for the substring
        for (_, c) in original[..start].chars().enumerate() {
            if c == '\n' {
                current_line += 1;
                current_column = 1;
            } else {
                current_column += 1;
            }
        }
        let start_line = current_line;
        let start_column = current_column;
        // Calculate the ending line and column for the substring
        for c in substring.chars() {
            if c == '\n' {
                current_line += 1;
                current_column = 1;
            } else {
                current_column += 1;
            }
        }
        let end_line = current_line;
        let end_column = current_column;
        SourceInfo {
            file_path: self.file_path.clone(),
            start_line,
            start_column,
            end_line,
            end_column,
        }
    }

    pub fn contains(&self, other: &SourceInfo) -> bool {
        // Ensure the file paths are the same
        if self.file_path != other.file_path {
            return false;
        }

        // Check if the start position of `other` is after or at the start of `self`
        let start_within = (other.start_line > self.start_line)
            || (other.start_line == self.start_line && other.start_column >= self.start_column);

        // Check if the end position of `other` is before or at the end of `self`
        let end_within = (other.end_line < self.end_line)
            || (other.end_line == self.end_line && other.end_column <= self.end_column);

        start_within && end_within
    }

    pub fn expand(&self, other: &SourceInfo) -> Option<SourceInfo> {
        // Ensure the file paths are the same
        if self.file_path != other.file_path {
            return None;
        }

        // Determine the earliest start position
        let (start_line, start_column) = if (self.start_line < other.start_line)
            || (self.start_line == other.start_line && self.start_column <= other.start_column)
        {
            (self.start_line, self.start_column)
        } else {
            (other.start_line, other.start_column)
        };

        // Determine the latest end position
        let (end_line, end_column) = if (self.end_line > other.end_line)
            || (self.end_line == other.end_line && self.end_column >= other.end_column)
        {
            (self.end_line, self.end_column)
        } else {
            (other.end_line, other.end_column)
        };

        Some(SourceInfo {
            file_path: self.file_path.clone(),
            start_line,
            start_column,
            end_line,
            end_column,
        })
    }
}

impl Debug for SourceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SourceInfo({}:{}:{}-{}:{})",
            self.file_path, self.start_line, self.start_column, self.end_line, self.end_column
        )
    }
}

impl serde::Serialize for SourceInfo {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let s = format!(
            "{}:{}:{}:{}:{}",
            self.file_path, self.start_line, self.start_column, self.end_line, self.end_column
        );
        serializer.serialize_str(&s)
    }
}

impl<'de> serde::Deserialize<'de> for SourceInfo {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 5 {
            return Err(serde::de::Error::custom("Invalid SourceInfo format"));
        }
        let file_path = parts[0].to_string();
        let start_line = parts[1].parse().unwrap();
        let start_column = parts[2].parse().unwrap();
        let end_line = parts[3].parse().unwrap();
        let end_column = parts[4].parse().unwrap();
        Ok(SourceInfo {
            file_path,
            start_line,
            start_column,
            end_line,
            end_column,
        })
    }
}

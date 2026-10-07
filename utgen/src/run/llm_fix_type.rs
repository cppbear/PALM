use std::{
    cmp::{max, min, Ordering},
    fs::{exists, read_to_string},
    path::{Path, PathBuf},
};

use regex::Regex;
use serde::Deserialize;

static LINESPAN: i32 = 50;

#[derive(Debug, Clone, Deserialize)]
struct Span {
    file_name: String,
    line_start: i32,
    line_end: i32,
}

#[derive(Debug, Clone, Deserialize)]
struct Children {
    children: Vec<Children>,
    spans: Vec<Span>,
}

#[derive(Debug, Clone, Deserialize)]
struct Message {
    spans: Vec<Span>,
    children: Vec<Children>,
    rendered: String,
}

impl Message {
    fn has_spans(&self) -> bool {
        return !self.spans.is_empty();
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompilerMessage {
    message: Message,
}

impl CompilerMessage {
    pub fn has_spans(&self) -> bool {
        return self.message.has_spans();
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Location {
    start: i32,
    end: i32,
}

impl Location {
    fn new(start: i32, end: i32, line_num: i32) -> Self {
        let mut location = Location { start, end };
        location.start = max(1, start - LINESPAN);
        location.end = min(line_num, end + LINESPAN);
        location
    }

    fn has_intersection(&self, location: &Location) -> bool {
        return !(self.end < location.start || self.start > location.end);
    }

    fn merge(&mut self, location: &Location) {
        self.start = min(self.start, location.start);
        self.end = max(self.end, location.end);
    }
}

#[derive(Debug, Clone, Deserialize)]
struct FileLocation {
    file_name: String,
    locations: Vec<Location>,
    line_num: i32,
}

impl FileLocation {
    fn new(file_name: &String, line_num: i32) -> Self {
        FileLocation {
            file_name: file_name.clone(),
            locations: Vec::new(),
            line_num: line_num,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ErrorMessage {
    compiler_message: CompilerMessage,
    file_locations: Vec<FileLocation>,
    pub code_snippets: Vec<String>,
}

fn parse_children(work_path: &Path, children: &Children, file_locations: &mut Vec<FileLocation>) {
    parse_spans(work_path, &children.spans, file_locations);
    for child in children.children.iter() {
        parse_children(work_path, child, file_locations);
    }
}

impl ErrorMessage {
    pub fn new(work_path: &Path, compiler_message: &CompilerMessage) -> Self {
        let mut file_locations: Vec<FileLocation> = Vec::new();
        parse_spans(
            work_path,
            &compiler_message.message.spans,
            &mut file_locations,
        );
        for children in compiler_message.message.children.iter() {
            parse_children(work_path, children, &mut file_locations);
        }
        let mut error_message = ErrorMessage {
            compiler_message: compiler_message.clone(),
            file_locations: file_locations,
            code_snippets: Vec::new(),
        };
        error_message.code_snippets = error_message.compile_error_message_construct(work_path);
        error_message
    }

    fn compile_error_message_construct(&self, work_path: &Path) -> Vec<String> {
        let mut code_snippets: Vec<String> = Vec::new();
        code_snippets.push(self.compiler_message.message.rendered.clone() + "\n");
        code_snippets.push("---\n".to_string());
        for file_location in self.file_locations.iter() {
            let mut code_snippet = String::new();
            code_snippet += format!("file: {}\n", file_location.file_name).as_str();
            let file_path = PathBuf::from(&file_location.file_name);
            if file_path.is_absolute() {
                let content = read_to_string(&file_path).unwrap();
                let lines: Vec<&str> = content.lines().collect();
                for location in file_location.locations.iter() {
                    for i in location.start..=location.end {
                        let line = format!("[{}]", i) + lines[(i - 1) as usize] + "\n";
                        code_snippet += &line;
                    }
                }
            } else {
                let file_path = work_path.join(&file_location.file_name);
                let content = read_to_string(&file_path).unwrap();
                let lines: Vec<&str> = content.lines().collect();
                for location in file_location.locations.iter() {
                    for i in location.start..=location.end {
                        if i as usize == lines.len() {
                            break;
                        }
                        let line = format!("[{}]", i) + lines[(i - 1) as usize] + "\n";
                        code_snippet += &line;
                    }
                }
            }
            code_snippets.push(code_snippet);
        }
        code_snippets
    }
}

fn parse_spans(work_path: &Path, spans: &Vec<Span>, file_locations: &mut Vec<FileLocation>) {
    for span in spans.iter() {
        let mut has_file_location = false;
        for file_location in file_locations.iter_mut() {
            if file_location.file_name == span.file_name {
                has_file_location = true;
                let location =
                    Location::new(span.line_start, span.line_end, file_location.line_num);
                let mut has_merge = false;
                let mut has_merge_num = 0;
                for exist_location in file_location.locations.iter_mut() {
                    if exist_location.has_intersection(&location) {
                        exist_location.merge(&location);
                        has_merge = true;
                        break;
                    }
                    has_merge_num += 1;
                }
                if has_merge {
                    let mut inner_has_merge = false;
                    loop {
                        let location = file_location.locations.remove(has_merge_num);
                        has_merge_num = 0;
                        inner_has_merge = false;
                        for exist_location in file_location.locations.iter_mut() {
                            if exist_location.has_intersection(&location) {
                                exist_location.merge(&location);
                                inner_has_merge = true;
                                break;
                            }
                            has_merge_num += 1;
                        }
                        if !inner_has_merge {
                            file_location.locations.push(location);
                            break;
                        }
                    }
                } else {
                    file_location.locations.push(location);
                }
                break;
            }
        }
        if !has_file_location {
            let file_path = work_path.join(span.file_name.clone());
            if exists(&file_path).unwrap() {
                let content = read_to_string(file_path).unwrap();
                let lines: Vec<&str> = content.lines().collect();
                let line_num = lines.len() as i32;
                let location = Location::new(span.line_start, span.line_end, line_num);
                let mut file_location = FileLocation::new(&span.file_name, line_num);
                file_location.locations.push(location);
                file_locations.push(file_location);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct TestCode {
    pub file_path: PathBuf,
    pub index: Vec<i32>,
    pub codes: Vec<String>,
    pub start: i32,
    pub end: i32,
}

impl TestCode {
    pub fn new(file_path: &Path, code: &Vec<String>) -> Self {
        let content = read_to_string(file_path).unwrap();
        let file_lines: Vec<&str> = content.lines().collect();
        let code_len = code.len();
        let file_len = file_lines.len();
        let mut start = 0;
        let mut end = 0;
        let mut index: Vec<i32> = Vec::new();
        for i in 0..file_len {
            if i + code_len > file_len {
                break;
            }

            let mut matched = true;
            for j in 0..code_len {
                if file_lines[i + j] != code[j] {
                    matched = false;
                    break;
                }
            }

            if matched {
                start = i + 1;
                end = i + code_len;
                break;
            }
        }
        for j in start..=end {
            index.push(j as i32);
        }
        TestCode {
            file_path: PathBuf::from(file_path),
            index: index,
            codes: code.clone(),
            start: start as i32,
            end: end as i32,
        }
    }

    pub fn change_codes(&mut self, changelogs: &Vec<ChangeLog>) -> bool {
        for changelog in changelogs.iter() {
            for change_data in changelog.change_datas.iter() {
                let change_start = change_data.origin[0].0;
                let change_end = change_data.origin[change_data.origin.len() - 1].0;
                let mut i = 0;
                let mut j = 0;
                while i < self.index.len() {
                    if self.index[i] == change_start as i32 {
                        break;
                    }
                    i += 1;
                }
                while j < self.index.len() {
                    if self.index[j] == change_end as i32 {
                        break;
                    }
                    j += 1;
                }
                if i == self.index.len() || j == self.index.len() {
                    return false;
                }
                let mut z = 0;
                for k in i..=j {
                    if self.codes[k].trim() != change_data.origin[z].1.trim() {
                        return false;
                    }
                    z += 1;
                }
            }
        }

        let mut insert_changelogs: Vec<ChangeData> = Vec::new();
        let mut not_insert_changelogs: Vec<ChangeData> = Vec::new();
        for changelog in changelogs.iter() {
            for change_data in changelog.change_datas.iter() {
                if change_data.fixed.len() > change_data.origin.len() {
                    insert_changelogs.push(change_data.clone());
                } else {
                    not_insert_changelogs.push(change_data.clone());
                }
            }
        }
        insert_changelogs.sort_by(|a, b| {
            if a.origin[0].0 >= b.origin[0].0 {
                return Ordering::Less;
            } else {
                return Ordering::Greater;
            }
        });
        for changelog in not_insert_changelogs.iter() {
            let mut start_index = 0;
            for i in self.index.iter() {
                if changelog.origin[0].0 == *i as usize {
                    break;
                }
                start_index += 1;
            }
            let mut j = 0;
            while j < changelog.fixed.len() {
                self.codes[start_index as usize + j] = changelog.fixed[j].1.clone();
                j += 1;
            }
            while j < changelog.origin.len() {
                self.codes[start_index as usize + j] = "".to_string();
                j += 1;
            }
        }
        for changelog in insert_changelogs.iter() {
            let mut start_index = 0;
            for i in self.index.iter() {
                if changelog.origin[0].0 == *i as usize {
                    break;
                }
                start_index += 1;
            }
            let mut j = 0;
            while j < changelog.origin.len() {
                self.codes[start_index as usize + j] = changelog.fixed[j].1.clone();
                j += 1;
            }
            while j < changelog.fixed.len() {
                self.codes
                    .insert(start_index as usize + j, changelog.fixed[j].1.clone());
                j += 1;
            }
        }
        let mut small: i32 = 0;
        let mut middle: i32 = 0;
        let mut big: i32 = 0;
        for code in self.codes.iter() {
            for c in code.chars() {
                match c {
                    '(' => {
                        small += 1;
                    }
                    ')' => {
                        small -= 1;
                    }
                    '[' => {
                        middle += 1;
                    }
                    ']' => {
                        middle -= 1;
                    }
                    '{' => {
                        big += 1;
                    }
                    '}' => {
                        big -= 1;
                    }
                    _ => {}
                }
            }
        }
        if small != 0 || middle != 0 || big != 0 {
            return false;
        }
        return true;
    }
}

// #[derive(Debug, Clone)]

// pub enum ChangeOperation {
//     Insert,
//     Substitute,
//     Delete,
// }

#[derive(Debug, Clone)]

pub enum ChangeLogError {
    LineNumberMismatch,
    InvalidFormat,
}

#[derive(Debug, Clone)]

pub struct ChangeData {
    // pub change_operation: ChangeOperation,
    pub origin: Vec<(usize, String)>,
    pub fixed: Vec<(usize, String)>,
}

#[derive(Debug, Clone)]
pub struct ChangeLog {
    pub file_path: PathBuf,
    pub change_datas: Vec<ChangeData>,
}

impl ChangeLog {
    pub fn reduce_the_scope(&mut self, start_line: usize, end_line: usize) -> bool {
        let mut new_change_datas: Vec<ChangeData> = Vec::new();
        for change_data in self.change_datas.iter_mut() {
            let mut new_change_data = ChangeData {
                origin: Vec::new(),
                fixed: Vec::new(),
            };
            let mut new_origin: Vec<(usize, String)> = Vec::new();
            for origin_line in change_data.origin.iter() {
                if origin_line.0 >= start_line && origin_line.0 <= end_line {
                    new_origin.push(origin_line.clone());
                }
            }
            let mut new_fixed: Vec<(usize, String)> = Vec::new();
            for fixed_line in change_data.fixed.iter() {
                if fixed_line.0 >= start_line && fixed_line.0 <= end_line {
                    new_fixed.push(fixed_line.clone());
                }
            }
            new_change_data.origin = new_origin;
            new_change_data.fixed = new_fixed;
            if new_change_data.origin.len() > 0 {
                new_change_datas.push(new_change_data);
            }
        }
        if new_change_datas.len() > 0 {
            self.change_datas = new_change_datas;
            return true;
        } else {
            return false;
        }
    }
}

fn parse_continue_line(
    current_index: &mut usize,
    changelog: &[&str],
    start_line: usize,
    end_line: usize,
) -> Result<Vec<(usize, String)>, ChangeLogError> {
    let line_regex = Regex::new(r"\[(\d+)\] ?(.*)").unwrap();
    let mut result: Vec<(usize, String)> = Vec::new();
    for no in start_line..=end_line {
        *current_index += 1;
        let line = changelog
            .get(*current_index)
            .ok_or(ChangeLogError::InvalidFormat)?;
        let captures = line_regex
            .captures(line)
            .ok_or(ChangeLogError::InvalidFormat)?;
        let line_number = captures[1]
            .parse::<usize>()
            .map_err(|_| ChangeLogError::InvalidFormat)?;
        if no != line_number {
            return Err(ChangeLogError::LineNumberMismatch);
        } else {
            result.push((line_number, captures[2].to_string()));
        }
    }
    *current_index += 1;
    Ok(result)
}

impl ChangeLog {
    pub fn new(work_path: &Path, changelog: &Vec<&str>) -> Result<Self, ChangeLogError> {
        let title_regex = Regex::new(r"ChangeLog:\d+@ *([^ ]+)").unwrap();
        let title_captures_result = title_regex
            .captures(changelog[0])
            .ok_or(ChangeLogError::InvalidFormat);
        let title_captures;
        match title_captures_result {
            Ok(t) => {
                title_captures = t;
            }
            Err(e) => {
                return Err(e);
            }
        }
        let file = title_captures[1].to_string();
        let mut file_path = PathBuf::from(file);
        if !file_path.is_absolute() {
            file_path = work_path.join(file_path);
        }
        let mut i = 2;
        let mut origin: Vec<(usize, String)> = Vec::new();
        let mut fixed: Vec<(usize, String)> = Vec::new();
        let mut change_datas: Vec<ChangeData> = Vec::new();
        let mut has_origin = false;
        let mut has_fixed = false;

        while i < changelog.len() {
            if changelog[i].starts_with("OriginalCode@") {
                let origin_regex = Regex::new(r"OriginalCode@(\d+)-(\d+):").unwrap();
                let origin_captures_result = origin_regex
                    .captures(changelog[i])
                    .ok_or(ChangeLogError::InvalidFormat);
                let origin_captures;
                match origin_captures_result {
                    Ok(o) => {
                        origin_captures = o;
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
                let start_line = origin_captures[1]
                    .parse::<usize>()
                    .map_err(|_| ChangeLogError::InvalidFormat)?;
                let end_line = origin_captures[2]
                    .parse::<usize>()
                    .map_err(|_| ChangeLogError::InvalidFormat)?;
                let origin_result = parse_continue_line(&mut i, changelog, start_line, end_line);
                match origin_result {
                    Ok(o) => {
                        origin = o;
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
                has_origin = true;
            } else if changelog[i].starts_with("FixedCode@") {
                let fixed_regex = Regex::new(r"FixedCode@(\d+)-(\d+):").unwrap();
                let fixed_captures_result = fixed_regex
                    .captures(changelog[i])
                    .ok_or(ChangeLogError::InvalidFormat);
                let fixed_captures;
                match fixed_captures_result {
                    Ok(f) => {
                        fixed_captures = f;
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
                let start_line = fixed_captures[1]
                    .parse::<usize>()
                    .map_err(|_| ChangeLogError::InvalidFormat)?;
                let end_line = fixed_captures[2]
                    .parse::<usize>()
                    .map_err(|_| ChangeLogError::InvalidFormat)?;
                let fixed_result = parse_continue_line(&mut i, changelog, start_line, end_line);
                match fixed_result {
                    Ok(f) => {
                        fixed = f;
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
                has_fixed = true;
                // for j in 0..origin.len().min(fixed.len()) {
                //     if origin[j].0 != fixed[j].0 {
                //         return Err(ChangeLogError::LineNumberMismatch);
                //     }
                //     changelog_operation_list.insert(
                //         (origin[j].0, ChangeOperation::Substitute),
                //         (origin[j].1.clone(), fixed[j].1.clone()),
                //     );
                // }
                // if origin.len() > fixed.len() {
                //     for j in fixed.len()..origin.len() {
                //         changelog_operation_list.insert(
                //             (origin[j].0, ChangeOperation::Substitute),
                //             (origin[j].1.clone(), String::new()),
                //         );
                //     }
                // } else if fixed.len() > origin.len() {
                //     for j in origin.len()..fixed.len() {
                //         changelog_operation_list.insert(
                //             (fixed[j].0, ChangeOperation::Insert),
                //             (String::new(), fixed[j].1.clone()),
                //         );
                //     }
                // }
            } else {
                i += 1;
            }
            if has_origin && has_fixed && origin.len() == 0 {
                has_origin = false;
                has_fixed = false;
                continue;
            }
            if has_origin && has_fixed {
                let start_line = origin[0].0;
                let mut new_fixed: Vec<(usize, String)> = Vec::new();
                for fix in fixed.iter() {
                    if fix.0 >= start_line {
                        new_fixed.push(fix.clone());
                    }
                }
                let change_data = ChangeData {
                    origin: origin.clone(),
                    fixed: new_fixed.clone(),
                };
                change_datas.push(change_data);
                has_origin = false;
                has_fixed = false;
            }
        }
        // if origin.len() < fixed.len() {
        //     change_data = ChangeData {
        //         change_operation: ChangeOperation::Insert,
        //         origin: origin.clone(),
        //         fixed: fixed.clone(),
        //     };
        // } else if origin.len() == fixed.len() {
        //     change_data = ChangeData {
        //         change_operation: ChangeOperation::Substitute,
        //         origin: origin.clone(),
        //         fixed: fixed.clone(),
        //     };
        // } else {
        //     change_data = ChangeData {
        //         change_operation: ChangeOperation::Delete,
        //         origin: origin.clone(),
        //         fixed: fixed.clone(),
        //     };
        // }
        Ok(ChangeLog {
            file_path: PathBuf::from(file_path),
            change_datas: change_datas,
        })
    }
}

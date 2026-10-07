use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use syn::spanned::Spanned;
use syn::visit::Visit;
// use syn::Attribute;
use walkdir::WalkDir;

pub fn rename_tests_to_bak(dir: &Path) -> std::io::Result<()> {
    for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() && path.file_name().unwrap() == "tests" {
            let new_name = path.with_file_name("tests.bak");
            fs::rename(path, new_name)?;
        }
    }
    Ok(())
}

pub fn comment_out_tests(dir: &Path) -> io::Result<()> {
    for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.is_file()
            && path.extension().and_then(|s| s.to_str()) == Some("rs")
            && path
                .strip_prefix(dir)
                .unwrap()
                .components()
                .next()
                .map(|c| c.as_os_str())
                == Some("src".as_ref())
        {
            comment_out_tests_in_file(path)?;
        }
    }
    Ok(())
}

fn is_test_module(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .meta
                .require_list()
                .is_ok_and(|list| list.tokens.to_string().contains("test"))
    })
}

struct TestCommenter {
    mods_pos: Vec<(usize, usize, usize, usize)>,
    functions_pos: Vec<(usize, usize, usize, usize)>,
}

impl TestCommenter {
    fn new() -> Self {
        Self {
            mods_pos: Vec::new(),
            functions_pos: Vec::new(),
        }
    }
}
fn is_test_like_attr(attr: &syn::Attribute) -> bool {
    let path = attr.path();

    path.is_ident("test")
        || path.segments.last().is_some_and(|seg| seg.ident == "test")
}
impl<'ast> Visit<'ast> for TestCommenter {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        if is_test_module(&i.attrs) {
            let span = i.span();
            self.mods_pos.push((
                span.start().line,
                span.start().column,
                span.end().line,
                span.end().column,
            ));
        }
        syn::visit::visit_item_mod(self, i);
    }

    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        if i.attrs.iter().any(is_test_like_attr) {
            let span = i.span();
            self.functions_pos.push((
                span.start().line,
                span.start().column,
                span.end().line,
                span.end().column,
            ));
        }
        syn::visit::visit_item_fn(self, i);
    }
}

fn comment_out_tests_in_file(file_path: &Path) -> io::Result<()> {
    let mut file = File::open(file_path)?;
    let mut src = String::new();
    file.read_to_string(&mut src)?;

    let syntax = syn::parse_file(&src).expect("Failed to parse file");
    let mut commenter = TestCommenter::new();
    commenter.visit_file(&syntax);

    let mut lines: Vec<String> = src.lines().map(|s| s.to_string()).collect();
    for (start_line, start_col, end_line, end_col) in commenter.functions_pos.into_iter().rev().chain(commenter.mods_pos.into_iter()) {
        if start_line == end_line {
            let line = &mut lines[start_line - 1];
            let (before, target, after) = (
                &line[..start_col],
                &line[start_col..end_col],
                &line[end_col..],
            );
            if after.trim_start().is_empty() {
                *line = format!("{}// {}", before, target);
            } else {
                let indent = before
                    .chars()
                    .take_while(|&c| c.is_whitespace())
                    .collect::<String>();
                let new_line = format!("{}// {}", before, target);
                let moved_line = format!("{}{}", indent, after.trim_start());
                *line = new_line;
                lines.insert(start_line, moved_line);
            }
        } else {
            let end_line_content = &mut lines[end_line - 1];
            let (target, after) = end_line_content.split_at_mut(end_col);
            if !after.trim_start().is_empty() {
                let indent = " ".repeat(end_col - 1);
                let moved_line = format!("{}{}", indent, after.trim_start());
                *end_line_content = format!("// {}", target);
                lines.insert(end_line, moved_line);
            } else {
                *end_line_content = format!("// {}", target);
            }

            for line_index in start_line..end_line - 1 {
                let line_content = &mut lines[line_index];
                if !line_content.trim().is_empty() {
                    *line_content = format!("// {}", line_content);
                }
            }

            let start_line_content = &mut lines[start_line - 1];
            let (before, target) = start_line_content.split_at_mut(start_col);
            if !before.trim_end().is_empty() {
                let indent = before
                    .chars()
                    .rev()
                    .take_while(|&c| c.is_whitespace())
                    .collect::<String>();
                let moved_line = format!("// {}{}", indent, target.trim_start());
                *start_line_content = format!("{}", before.trim_end());
                lines.insert(start_line, moved_line);
            } else {
                *start_line_content = format!("{}// {}", before, target);
            }
        }
    }

    let modified_code = lines.join("\n");
    let mut file = File::create(file_path)?;
    file.write_all(modified_code.as_bytes())?;

    Ok(())
}

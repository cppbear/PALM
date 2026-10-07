use crate::types::TestExtract;
use syn::spanned::Spanned;
use syn::visit::Visit;
use syn::{Attribute, ItemFn, UseTree};

fn trim_min_leading_spaces(lines: Vec<String>) -> Vec<String> {
    if lines.len() <= 1 {
        return lines;
    }
    let leading_spaces: Vec<usize> = lines
        .iter()
        .skip(1)
        .map(|line| line.chars().take_while(|c| c.is_whitespace()).count())
        .collect();
    let min_leading_spaces = *leading_spaces.iter().min().unwrap_or(&0);
    let mut trimmed_lines = Vec::with_capacity(lines.len());
    trimmed_lines.push(lines[0].clone());

    for line in lines.iter().skip(1) {
        let trimmed_line = if line.len() > min_leading_spaces {
            line[min_leading_spaces..].to_string()
        } else {
            line.clone()
        };
        trimmed_lines.push(trimmed_line);
    }
    trimmed_lines
}

fn expand_use_tree(use_tree: &UseTree, prefix: &str) -> Vec<String> {
    match use_tree {
        UseTree::Path(path) => {
            let new_prefix = if prefix.is_empty() {
                path.ident.to_string()
            } else {
                format!("{}::{}", prefix, path.ident)
            };
            expand_use_tree(&path.tree, &new_prefix)
        }
        UseTree::Name(name) => {
            if name.ident == "self" {
                vec![prefix.to_string()]
            } else {
                vec![format!("{}::{}", prefix, name.ident)]
            }
        }
        UseTree::Group(group) => group
            .items
            .iter()
            .flat_map(|tree| expand_use_tree(tree, prefix))
            .collect(),
        UseTree::Glob(_) => vec![format!("{}::*", prefix)], // Handles wildcard imports
        UseTree::Rename(rename) => {
            vec![format!("{}::{} as {}", prefix, rename.ident, rename.rename)]
        }
    }
}

fn remove_lines_and_ranges(lines: &mut Vec<String>, specs: Vec<(usize, Option<usize>)>) {
    // Flatten the specs into individual indices
    let mut indices: Vec<usize> = specs
        .iter()
        .flat_map(|&(start, end)| match end {
            Some(end) => start..=end,
            None => start..=start,
        })
        .collect();

    // Sort indices in descending order and remove duplicates
    indices.sort_unstable_by(|a, b| b.cmp(a));
    indices.dedup();

    // Remove lines
    for index in indices {
        if index - 1 < lines.len() {
            lines.remove(index - 1);
        }
    }
}

fn trim_empty_lines(lines: &mut Vec<String>) {
    // Remove leading empty lines
    while let Some(first) = lines.first() {
        if first.trim().is_empty() {
            lines.remove(0);
        } else {
            break;
        }
    }

    // Remove trailing empty lines
    while let Some(last) = lines.last() {
        if last.trim().is_empty() {
            lines.pop();
        } else {
            break;
        }
    }
}

struct TestFnVisitor {
    uses: Vec<String>,
    has_test_mod: bool,
    test_fns: Vec<(Vec<String>, Vec<String>)>, // (attrs, lines)
    specs: Vec<(usize, Option<usize>)>,
}

impl TestFnVisitor {
    fn add_spec(&mut self, span: &proc_macro2::Span) {
        if span.start().line == span.end().line {
            self.specs.push((span.start().line, None));
        } else {
            self.specs.push((span.start().line, Some(span.end().line)));
        }
    }
}

impl<'ast> Visit<'ast> for TestFnVisitor {
    fn visit_item_fn(&mut self, i: &'ast ItemFn) {
        let is_test = i.attrs.iter().any(is_test_fn_attr);
        if is_test {
            self.add_spec(&i.span());
            let mut attrs = Vec::new();
            for attr in &i.attrs {
                if !is_test_fn_attr(attr) {
                    if let Some(attr_str) = attr.span().source_text() {
                        attrs.push(attr_str);
                    }
                }
            }
            if let Some(code) = i.block.span().source_text() {
                let lines: Vec<String> = code.lines().map(|s| s.to_string()).collect();
                self.test_fns.push((attrs, trim_min_leading_spaces(lines)));
            }
        }
        syn::visit::visit_item_fn(self, i);
    }

    fn visit_item_use(&mut self, i: &'ast syn::ItemUse) {
        self.add_spec(&i.span());
        let use_items = expand_use_tree(&i.tree, "");
        self.uses
            .extend(use_items.into_iter().map(|s| format!("use {};", s)));
    }

    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        self.has_test_mod = i.attrs.iter().any(is_test_mod_attr);
        if self.has_test_mod {
            let mod_span = i.span();
            if let Some(content) = &i.content {
                let delim_span = content.0.span.join();
                self.specs
                    .push((mod_span.start().line, Some(delim_span.start().line)));
                self.specs
                    .push((delim_span.end().line, Some(mod_span.end().line)));
            } else {
                self.add_spec(&mod_span);
            }
        }
        syn::visit::visit_item_mod(self, i);
    }
}

fn is_test_fn_attr(attr: &Attribute) -> bool {
    attr.path().is_ident("test")
}

fn is_test_mod_attr(attr: &Attribute) -> bool {
    let mut flag = false;
    if attr.path().is_ident("cfg") {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("test") {
                flag = true;
                return Ok(());
            }
            Ok(())
        })
        .unwrap();
    }
    flag
}

pub fn try_parse(source: &str) -> Result<(), ()> {
    match syn::parse_str::<syn::File>(source) {
        Ok(_) => Ok(()),
        Err(_) => Err(()),
    }
}

pub fn extract_test_functions(source: &str) -> TestExtract {
    let syntax_tree: syn::File = syn::parse_str(source).expect("Failed to parse source");
    let mut visitor = TestFnVisitor {
        uses: Vec::new(),
        has_test_mod: false,
        test_fns: Vec::new(),
        specs: Vec::new(),
    };
    visitor.visit_file(&syntax_tree);
    let mut lines = source.lines().map(|s| s.to_string()).collect::<Vec<_>>();
    remove_lines_and_ranges(&mut lines, visitor.specs);

    trim_empty_lines(&mut lines);
    TestExtract {
        uses: visitor.uses,
        has_test_mod: visitor.has_test_mod,
        common: lines,
        test_fns: visitor.test_fns,
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_extract_test_functions() {
        let source = r#"
            use std::io;
            use std::fs::{*, self};
            use std::path::{self, Path, PathBuf};
            use std::collections::{HashMap as HMap, HashSet as HSet};

            fn test_1() {
                assert_eq!(1, 1);
            }

            #[test]
            #[should_panic(expected = "2 != 3")]
            fn test_2() {
                assert_eq!(2, 3);
            }

            #[test]
            #[should_panic]
            fn test_3() {
                assert_eq!(2, 4);
            }
        "#;
        let extract = extract_test_functions(source);
        assert_eq!(extract.has_test_mod, false);
        println!("uses: {:?}", extract.uses);
        println!("common: {:?}", extract.common);
        assert_eq!(extract.test_fns.len(), 2);
        assert_eq!(extract.test_fns[0].1.len(), 3);
        println!("attrs: {:?}", extract.test_fns[0].0);
        println!("attrs: {:?}", extract.test_fns[1].0);
        // assert_eq!(uses.len(), 4);
    }

    #[test]
    fn test_extract_test_functions_with_mod() {
        let source = r#"
            use std::io;
            use std::fs::{*, self};
            use std::path::{self, Path, PathBuf};
            use std::collections::{HashMap as HMap, HashSet as HSet};

            #[cfg(test)]
            mod tests {
                use super::*;

                fn test_1() {
                    assert_eq!(1, 1);
                }

                #[test]
                #[should_panic(expected = "2 != 3")]
                fn test_2() {
                    assert_eq!(2, 3);
                }

                #[test]
                #[should_panic]
                fn test_3() {
                    assert_eq!(2, 4);
                }
            }
        "#;
        let extract = extract_test_functions(source);
        assert_eq!(extract.has_test_mod, true);
        println!("uses: {:?}", extract.uses);
        println!("common: {:?}", extract.common);
        assert_eq!(extract.test_fns.len(), 2);
        assert_eq!(extract.test_fns[0].1.len(), 3);
        println!("attrs: {:?}", extract.test_fns[0].0);
        println!("attrs: {:?}", extract.test_fns[1].0);
        // assert_eq!(uses.len(), 4);
    }
}

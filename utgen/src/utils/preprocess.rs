use cargo_metadata::MetadataCommand;
use std::{collections::BTreeSet, fs, io, ops::Range, path::Path};
use syn::{Meta, Token, punctuated::Punctuated, spanned::Spanned, visit::Visit};
use walkdir::WalkDir;

/// Only the selected crate's integration-test directory is renamed.
pub fn rename_tests_to_bak(dir: &Path) -> io::Result<()> {
    let tests = dir.join("tests");
    let backup = dir.join("tests.bak");
    if tests.exists() {
        if backup.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "{} already exists; use a fresh working copy",
                    backup.display()
                ),
            ));
        }
        fs::rename(tests, backup)?;
    }
    Ok(())
}

pub fn comment_out_tests(dir: &Path) -> io::Result<()> {
    let src = dir.join("src");
    if !src.is_dir() {
        return Ok(());
    }
    let src = src.canonicalize()?;
    // A Cargo entry must be a complete file. Non-entry expression fragments
    // used by include! can be left alone. Directory-only preprocessing remains
    // usable without requiring a manifest.
    let roots: BTreeSet<_> = if dir.join("Cargo.toml").is_file() {
        MetadataCommand::new()
            .manifest_path(dir.join("Cargo.toml").canonicalize()?)
            .current_dir(dir)
            .no_deps()
            .exec()
            .map_err(io::Error::other)?
            .packages
            .into_iter()
            .flat_map(|p| {
                p.targets
                    .into_iter()
                    .map(|t| t.src_path.into_std_path_buf())
            })
            .collect()
    } else {
        BTreeSet::new()
    };
    // Parse every file before writing any of them.
    let mut changes = Vec::new();
    for entry in WalkDir::new(src) {
        let entry = entry.map_err(io::Error::other)?;
        let path = entry.path();
        if entry.file_type().is_file() && path.extension().is_some_and(|e| e == "rs") {
            let original = fs::read_to_string(path)?;
            let prepared = prepare_source(&original, roots.contains(path)).map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: {err}", path.display()),
                )
            })?;
            if prepared != original {
                changes.push((path.to_owned(), prepared));
            }
        }
    }
    for (path, prepared) in changes {
        fs::write(path, prepared)?;
    }
    Ok(())
}

// Evaluate only what is known with cfg(test) disabled. Unknown feature/target
// predicates stay unknown, so a production module is never removed on a guess.
fn without_tests(meta: &Meta) -> Option<bool> {
    match meta {
        Meta::Path(path) if path.is_ident("test") => Some(false),
        Meta::List(list) => {
            let args = list
                .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .ok()?;
            let values: Vec<_> = args.iter().map(without_tests).collect();
            if list.path.is_ident("not") && values.len() == 1 {
                values[0].map(|v| !v)
            } else if list.path.is_ident("all") {
                if values.contains(&Some(false)) {
                    Some(false)
                } else if values.iter().all(|v| *v == Some(true)) {
                    Some(true)
                } else {
                    None
                }
            } else if list.path.is_ident("any") {
                if values.contains(&Some(true)) {
                    Some(true)
                } else if values.iter().all(|v| *v == Some(false)) {
                    Some(false)
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(crate) fn is_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<Meta>()
                .ok()
                .is_some_and(|meta| without_tests(&meta) == Some(false))
    })
}

pub(crate) fn is_test_function(attrs: &[syn::Attribute]) -> bool {
    is_test_only(attrs)
        || attrs
            .iter()
            .any(|a| a.path().segments.last().is_some_and(|s| s.ident == "test"))
}

/// Expression fragments are deliberately not rewritten or macro-expanded.
pub(crate) fn parse_source(source: &str, crate_root: bool) -> syn::Result<Option<syn::File>> {
    match syn::parse_file(source) {
        Ok(file) => Ok(Some(file)),
        Err(_) if !crate_root && syn::parse_str::<syn::Expr>(source).is_ok() => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn source_span_offset(source: &str, syntax: &syn::File) -> usize {
    // syn strips BOM/shebang prefixes before tokenizing.
    (if source.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    }) + syntax.shebang.as_ref().map_or(0, |s| s.len())
}

#[derive(Default)]
struct TestRanges {
    ranges: Vec<Range<usize>>,
}

impl<'ast> Visit<'ast> for TestRanges {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if is_test_only(&item.attrs) {
            self.ranges.push(item.span().byte_range());
        } else {
            syn::visit::visit_item_mod(self, item);
        }
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if is_test_function(&item.attrs) {
            self.ranges.push(item.span().byte_range());
        } else {
            syn::visit::visit_item_fn(self, item);
        }
    }
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if is_test_only(&item.attrs) {
            self.ranges.push(item.span().byte_range());
        } else {
            syn::visit::visit_item_impl(self, item);
        }
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if is_test_function(&item.attrs) {
            self.ranges.push(item.span().byte_range());
        } else {
            syn::visit::visit_impl_item_fn(self, item);
        }
    }
}

fn prepare_source(source: &str, crate_root: bool) -> syn::Result<String> {
    let Some(syntax) = parse_source(source, crate_root)? else {
        return Ok(source.to_owned());
    };
    let offset = source_span_offset(source, &syntax);
    let mut visitor = TestRanges::default();
    let ranges = if is_test_only(&syntax.attrs) {
        vec![offset..source.len()]
    } else {
        visitor.visit_file(&syntax);
        visitor
            .ranges
            .into_iter()
            .map(|r| r.start + offset..r.end + offset)
            .collect()
    };
    let mut bytes = source.as_bytes().to_vec();
    for range in ranges {
        // Retain newlines and byte offsets, including CRLF and same-line items.
        // Byte ranges include full UTF-8 characters; spaces keep valid UTF-8.
        for byte in &mut bytes[range] {
            if *byte != b'\n' && *byte != b'\r' {
                *byte = b' ';
            }
        }
    }
    Ok(String::from_utf8(bytes).expect("test ranges end on UTF-8 boundaries"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_line_tests_do_not_move_production_code() {
        let source = "#[test] fn old() { let s = \"中文\"; } pub fn keep() -> u8 { 1 }\r\n";
        let prepared = prepare_source(source, false).unwrap();
        assert_eq!(prepared.len(), source.len());
        let position = source.find("pub fn keep").unwrap();
        assert_eq!(&prepared[position..], &source[position..]);
        assert!(prepared[..position].trim().is_empty());
        assert!(syn::parse_file(&prepared).is_ok());
        assert_eq!(prepare_source(&prepared, false).unwrap(), prepared);
    }

    #[test]
    fn nested_ranges_leave_following_items_intact() {
        let source = "#[cfg(test)] mod tests { #[test] fn a() {} mod nested { #[test] fn b() {} } } fn keep() {}\n#[tokio::test] async fn c() {}\nfn last() {}\n";
        let prepared = prepare_source(source, false).unwrap();
        assert_eq!(prepared.len(), source.len());
        let syntax = syn::parse_file(&prepared).unwrap();
        assert_eq!(syntax.items.len(), 2);
        assert!(prepared.contains("fn keep() {}"));
        assert!(prepared.contains("fn last() {}"));
    }

    #[test]
    fn cfg_predicates_do_not_erase_production_modules() {
        let source = "#[cfg(not(test))] mod normal { fn a() {} }\n#[cfg(feature = \"contest\")] mod feature { fn b() {} }\n#[cfg(any(test, feature = \"enabled\"))] mod shared { fn c() {} }\n#[cfg(all(test, feature = \"enabled\"))] mod tests { fn d() {} }\n";
        let prepared = prepare_source(source, false).unwrap();
        assert!(prepared.contains("mod normal"));
        assert!(prepared.contains("mod feature"));
        assert!(prepared.contains("mod shared"));
        assert!(!prepared.contains("mod tests"));
        assert_eq!(source.lines().count(), prepared.lines().count());
    }

    #[test]
    fn files_without_tests_are_byte_identical() {
        let source = "// 中文\r\npub fn keep() {}\r\n";
        assert_eq!(prepare_source(source, false).unwrap(), source);
    }

    #[test]
    fn test_methods_and_impls_are_removed_without_moving_production() {
        let source = "struct S;\nimpl S { #[cfg(test)] fn helper() {} fn keep() {} }\n#[cfg(test)] impl S { fn other() {} }\nfn last() {}\n";
        let prepared = prepare_source(source, false).unwrap();
        assert!(!prepared.contains("helper"));
        assert!(!prepared.contains("other"));
        assert_eq!(prepared.len(), source.len());
        for name in ["fn keep", "fn last"] {
            assert_eq!(prepared.find(name), source.find(name));
        }
        syn::parse_file(&prepared).unwrap();
    }

    #[test]
    fn file_level_tests_are_blanked_and_expression_fragments_preserved() {
        let source = "#![cfg(test)]\r\nfn helper() {}\r\n#[test] fn checks() {}\r\n";
        let prepared = prepare_source(source, false).unwrap();
        assert!(prepared.trim().is_empty());
        assert_eq!(prepared.len(), source.len());
        assert_eq!(prepared.matches("\r\n").count(), 3);
        assert_eq!(prepare_source("42\n", false).unwrap(), "42\n");
        assert!(prepare_source("42\n", true).is_err());
        assert!(prepare_source("fn broken( {", false).is_err());
    }

    #[test]
    fn bom_and_shebang_do_not_shift_ranges() {
        for prefix in [
            "\u{feff}",
            "#!/usr/bin/env rust-script\n",
            "\u{feff}#!/usr/bin/env rust-script\n",
        ] {
            let source = format!("{prefix}#[test] fn old() {{}} fn keep() {{}}\n");
            let prepared = prepare_source(&source, false).unwrap();
            assert!(prepared.starts_with(prefix));
            let position = source.find("fn keep").unwrap();
            assert_eq!(&source[position..], &prepared[position..]);
            assert_eq!(syn::parse_file(&prepared).unwrap().items.len(), 1);
        }
    }
}

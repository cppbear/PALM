static TIMEOUT_DERIVE: &str = "#[timeout(5000)]";
mod coverage;
mod coverage_baseline;
mod coverage_json;
pub(crate) mod integration;
mod llm_fix;
mod llm_fix_type;
mod prepare;
mod run;
mod run_all;

pub use llm_fix::llm_fix;
pub use prepare::add_ntest_dependency;
// pub use run::cargo_clean;
pub use run::gen_test_rate;
pub use run::gen_test_rate_aggregated;

pub use coverage::collect_coverage;
pub(crate) use coverage_baseline::validate_targets;

/// Assemble a statistics test with the shared execution limit.
fn timed_test(signature: &str, attrs: &[String], body: &[String]) -> Vec<String> {
    use quote::ToTokens;
    use syn::parse::Parser;

    let attrs = syn::Attribute::parse_outer
        .parse_str(&attrs.join("\n"))
        .expect("generated test attributes must be valid Rust");
    let should_panic = attrs.iter().any(|attr| attr.path().is_ident("should_panic"));
    let mut code = vec!["#[test]".to_string(), TIMEOUT_DERIVE.to_string()];
    code.extend(
        attrs.iter()
            .filter(|attr| !attr.path().is_ident("should_panic"))
            .map(|attr| attr.to_token_stream().to_string()),
    );
    code.push(signature.to_string());
    if should_panic {
        // Keep the existing any-panic expectation inside the timeout boundary,
        // so the timeout's own panic cannot make a hanging test pass.
        code.push("{".to_string());
        code.push("assert!(std::panic::catch_unwind(||".to_string());
        code.extend_from_slice(body);
        code.push(").is_err(), \"test did not panic as expected\");".to_string());
        code.push("}".to_string());
    } else {
        code.extend_from_slice(body);
    }
    code
}

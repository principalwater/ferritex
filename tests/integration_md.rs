#[path = "common/base.rs"]
mod common;

use ferritex::build::{BuildConfig, OutputFormat, PdfBiberMode, ToolInstallPolicy, run_build};

#[test]
fn markdown_build_path_is_wired_to_md_backend() {
    let input = common::fixture_path("simple.tex");
    let output_dir = std::env::temp_dir().join(format!("ferritex_md_test_{}", std::process::id()));
    std::fs::create_dir_all(&output_dir).expect("failed to create temp output dir");

    let config = BuildConfig::from_build_args(
        &input,
        Some(&output_dir),
        OutputFormat::Md,
        None,
        PdfBiberMode::Auto,
        ToolInstallPolicy::Ask,
    );
    let err = run_build(&config).expect_err("Markdown backend should be stubbed for now");
    let err_text = err.to_string();
    let chain_text = err
        .chain()
        .map(|cause| cause.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        err_text.contains("failed to write"),
        "unexpected top-level error: {err_text}"
    );
    assert!(
        chain_text.contains("Markdown output backend is not implemented yet"),
        "unexpected error chain: {chain_text}"
    );

    let _ = std::fs::remove_file(config.md_path());
    let _ = std::fs::remove_dir_all(output_dir);
}

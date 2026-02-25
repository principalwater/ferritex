#[path = "common/base.rs"]
mod common;

use ferritex::build::{BuildConfig, OutputFormat, run_build};

#[test]
fn pdf_build_path_is_wired_to_pdf_backend() {
    let input = common::fixture_path("simple.tex");
    let output_dir = std::env::temp_dir().join(format!("ferritex_pdf_test_{}", std::process::id()));
    std::fs::create_dir_all(&output_dir).expect("failed to create temp output dir");

    let config = BuildConfig::from_build_args(&input, Some(&output_dir), OutputFormat::Pdf);
    let result = run_build(&config).expect("PDF backend should compile via tectonic::latex_to_pdf");
    let pdf_path = result
        .pdf
        .expect("expected PDF artifact path in build result");
    let pdf_bytes = std::fs::read(&pdf_path).expect("failed to read generated PDF artifact");
    assert!(
        !pdf_bytes.is_empty(),
        "generated PDF artifact must not be empty"
    );
    assert!(
        pdf_bytes.starts_with(b"%PDF-"),
        "generated artifact is not a PDF file"
    );

    let _ = std::fs::remove_file(pdf_path);
    let _ = std::fs::remove_dir_all(output_dir);
}

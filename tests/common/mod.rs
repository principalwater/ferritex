use ferritex::model::Document;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TMP_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) struct TempDocx {
    path: PathBuf,
}

impl TempDocx {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDocx {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

pub(crate) fn expected_fixture(name: &str) -> String {
    let path = fixture_path(&format!("expected/{name}"));
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read expected fixture {path:?}: {e}"))
}

pub(crate) fn expected_fixture_lines(name: &str) -> Vec<String> {
    expected_fixture(name)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(crate) fn parse_fixture(name: &str) -> Document {
    let path = fixture_path(name);
    let source =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read fixture {path:?}: {e}"));
    ferritex::parser::latex::parse_latex(&source)
}

pub(crate) fn render_document_to_temp_docx(document: &Document, stem: &str) -> TempDocx {
    let unique_id = NEXT_TMP_ID.fetch_add(1, Ordering::Relaxed);
    let output_path = std::env::temp_dir().join(format!(
        "ferritex_test_{}_{}_{}.docx",
        stem,
        std::process::id(),
        unique_id
    ));

    ferritex::renderer::docx::render_docx(document, &output_path)
        .unwrap_or_else(|e| panic!("render_docx failed for {stem}: {e}"));

    TempDocx { path: output_path }
}

pub(crate) fn read_docx_entry(docx_path: &Path, entry_name: &str) -> String {
    use std::io::Read;

    let file = fs::File::open(docx_path)
        .unwrap_or_else(|e| panic!("cannot open output DOCX {docx_path:?}: {e}"));
    let mut zip =
        zip::ZipArchive::new(file).unwrap_or_else(|e| panic!("output is not a valid ZIP: {e}"));

    let mut entry = zip
        .by_name(entry_name)
        .unwrap_or_else(|e| panic!("{entry_name} missing from DOCX: {e}"));
    let mut content = String::new();
    entry
        .read_to_string(&mut content)
        .unwrap_or_else(|e| panic!("cannot read {entry_name}: {e}"));
    content
}

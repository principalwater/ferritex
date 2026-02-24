# Integration Tests

Integration tests are split by output format and use format-specific helper
modules:
- `tests/common/mod.rs` for DOCX-heavy parsing/render/XML helpers,
- `tests/common/base.rs` for shared minimal helpers (`fixture_path`) used by
  `pdf`/`md` backend wiring tests.

Rules:
- use `tests/fixtures/*.tex` as test input documents,
- keep expected values in `tests/fixtures/expected/*` when practical,
- avoid duplicating ZIP/XML helper code in each test file.

Current entrypoints:
- `tests/integration_docx.rs` -> DOCX behavior and XML assertions
- `tests/integration_pdf.rs` -> PDF backend wiring (stub error contract)
- `tests/integration_md.rs` -> Markdown backend wiring (stub error contract)

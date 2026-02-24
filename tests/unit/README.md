# Unit Test Layout

`ferritex` now keeps unit tests alongside backend/core crates and includes them
from source modules via `include!` wrappers:

- `crates/ferritex-core/src/parser/latex.rs` ->
  `crates/ferritex-core/tests/unit/parser_latex_tests.rs`
- `crates/ferritex-renderer-docx/src/lib.rs` ->
  `crates/ferritex-renderer-docx/tests/unit/renderer_docx_tests.rs`

Why:
- keeps production files shorter and easier to scan,
- keeps private-unit-test access (`super::*`) without widening API visibility,
- keeps test ownership aligned with crate boundaries (`core`, `docx`, and future
  `pdf`/`md` backends).

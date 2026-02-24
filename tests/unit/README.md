# Unit Test Layout

`ferritex` keeps unit tests physically under `tests/unit/` and includes them
from source modules via `include!` wrappers:

- `src/parser/latex.rs` -> `tests/unit/parser_latex_tests.rs`
- `src/renderer/docx.rs` -> `tests/unit/renderer_docx_tests.rs`

Why:
- keeps production files shorter and easier to scan,
- keeps private-unit-test access (`super::*`) without widening API visibility,
- follows the same high-level idea used in `rust-lang/rust` where tests are
  organized in dedicated test directories/crates instead of being packed into
  implementation files.

# Architecture

`ferritex` follows a strict pipeline orchestrated by the **build core**:

1. **Build core** (`src/build/`): Unified orchestrator that resolves paths,
   selects output format(s), and drives the pipeline.
2. **Parse** (`src/parser/`): LaTeX source is converted into an internal AST.
3. **Model** (`src/model/`): AST is the only shared contract between stages.
4. **Render** (`src/renderer/`): AST is rendered to the target format (DOCX; PDF planned).

## Build core

The build core provides a single entry point (`build::run_build`) that both
the `build`, `convert`, and `tui` CLI modes delegate to. This ensures path
resolution, artifact naming, and pipeline sequencing are defined in one place.

Output formats are selected via `OutputFormat` (docx / pdf / both). The PDF
backend is planned but not yet implemented.

## Design goals

- Deterministic, reproducible conversion.
- Parser and renderer are decoupled by AST types.
- No subprocess wrappers around external converters.
- Single build core with format-specific backends (DOCX now, PDF next).

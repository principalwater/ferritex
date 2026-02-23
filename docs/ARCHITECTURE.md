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
- LaTeX-first parameter propagation: backend layout/styling decisions must be driven by source LaTeX metadata whenever available.

## LaTeX-first propagation

The parser is responsible for extracting style-relevant intent from LaTeX sources
(counters, labels, bibliography/citation behavior, include graph, layout hints).
The renderer must consume this intent through AST/metadata rather than introducing
document-specific hardcoded formatting rules.

This requirement is strict for all backends: the effective parameters of the
source LaTeX build configuration must be propagated to ferritex outputs. In
practice this includes page geometry, paragraph/heading formatting, page number
placement, and footnote/citation presentation.

## Mapping rule (mandatory)

For every visual/formatting issue discovered during manual QA:
1. Identify the corresponding LaTeX-origin parameter or semantic signal.
2. Add/extend parser extraction logic.
3. Normalize into generic AST/metadata fields.
4. Apply in renderer through deterministic mapping and keep backend defaults as fallback.

Do not patch renderer output with project-tailored constants when the same
effect can be achieved by propagating LaTeX semantics.

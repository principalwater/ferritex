# Architecture

`ferritex` uses a strict pipeline orchestrated by the build core.
The parser produces two output-neutral contracts:
- structural AST (`Document`, `Block`, `Inline`)
- style metadata (`DocumentLayout`, treated as **StyleMap**)

```text
LaTeX source
    ↓ parser
AST + StyleMap (extracted from preamble)
    ↓           ↓          ↓
DOCX        PDF        Markdown ...
renderer  renderer    renderer
```

## Workspace crates

| Crate | Purpose |
|-------|---------|
| `ferritex-core` | LaTeX parser, AST model (`Block`, `Inline`), `DocumentLayout` (StyleMap) |
| `ferritex-renderer-docx` | DOCX renderer + `RenderProfile::from_layout()` — **fully implemented** |
| `ferritex-renderer-pdf` | PDF renderer — **stub** |
| `ferritex-renderer-md` | Markdown renderer — **stub** |

`RenderProfile::from_layout()` is the single bridge where `DocumentLayout` option fields
are resolved to concrete values with fallback defaults. All renderer functions consume
`RenderProfile`, never raw `DocumentLayout`.

## Stage responsibilities

1. **Build core** (`src/build/`): resolves paths, selects output format(s), orchestrates pipeline execution.
2. **Parse** (`crates/ferritex-core/src/parser/`): reads LaTeX (including recursive includes), extracts document semantics and style parameters.
3. **Model** (`crates/ferritex-core/src/model/`): defines AST + StyleMap as the only parser↔renderer contract.
4. **Render** (`crates/ferritex-renderer-*/src/`): maps AST + StyleMap to backend primitives (`docx`, `pdf`, `md` backends are crate-isolated).

## Build core

The build core provides a single entry point (`build::run_build`) used by
`build`, `convert`, and `tui`, ensuring one implementation for path resolution,
artifact naming, and sequencing.

Output formats are selected via `OutputFormat`
(`docx` / `pdf` / `md` / `both` / `all`).

## Design goals

- Deterministic, reproducible conversion.
- Parser and renderers are decoupled by AST + StyleMap types.
- No subprocess wrappers around external converters.
- Single build core with format-specific renderers.
- LaTeX-first parameter propagation: backend layout/styling decisions should be driven by source LaTeX metadata whenever available.

## LaTeX-first propagation

The parser is responsible for extracting style-relevant intent from LaTeX
sources (geometry, heading/TOC formatting, counters, labels, bibliography and
citation behavior, include graph, block-local directives).

Renderers must consume this intent through AST + StyleMap rather than adding
document-specific hardcoded formatting rules.

This rule is strict for all backends: effective LaTeX build parameters must be
propagated to ferritex outputs. Backend constants are fallback-only.

## Renderer isolation rule

- Renderers must not parse LaTeX syntax directly.
- Any new style signal is added parser-first (extract → model field), then mapped in renderers.

## Output extensibility rule

- Adding a new output format should be done by adding a new renderer over the
  same AST + StyleMap contract.
- Parser/model changes are required only for new shared semantics, not for
  backend-specific visual hacks.

## Mapping rule (mandatory)

For every visual/formatting issue discovered during QA:
1. Identify the corresponding LaTeX-origin parameter or semantic signal.
2. Add/extend parser extraction logic.
3. Normalize into generic AST/StyleMap fields.
4. Apply in renderer through deterministic mapping and keep backend defaults as fallback.

Do not patch renderer output with project-tailored constants when the same
effect can be achieved by propagating LaTeX semantics.

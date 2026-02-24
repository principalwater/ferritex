# Session Summary (LaTeX-driven DOCX pipeline)

## Project goal

`ferritex` is a generic LaTeX-driven converter (currently with DOCX output) for arbitrary LaTeX projects:
- academic papers
- journal submissions
- dissertations
- technical reports

The core requirement is strict: formatting and layout behavior must be derived from LaTeX/project configuration, not from hardcoded renderer constants.

## Current status

The pipeline already enforces parser/model/renderer separation:
- parser extracts structure and style intent from LaTeX
- model carries style metadata (`DocumentLayout`, treated as StyleMap)
- renderer consumes AST + StyleMap only

Recent completed work includes:
- title page and TOC generation with LaTeX-driven parameters
- TOC chapter prefix support via `\cftchaptername` (model -> parser -> renderer)
- inline style declarations (`\bfseries`, `\itshape`) working without mandatory `{...}`
- control-space handling (`\ `) to avoid merged words
- TOC dot leaders and right tab stop driven by LaTeX (`\setrmarg`, `\cftdotfill`)
- TOC indent/numwidth extraction from:
  - `\cftsetindents{...}{...}{...}`
  - `\setlength{\cft...indent}{...}`
  - `\setlength{\cft...numwidth}{...}`
- DOCX TOC hanging indent support when LaTeX provides `numwidth`

Quality gate state after these changes:
- `cargo fmt --all`: pass
- `cargo clippy -- -D warnings`: pass
- `cargo test`: pass

## Observed issues and constraints

- Visual parity with manually edited DOCX is not always exact because Word-level overrides can exist in manual files that are not represented in LaTeX sources.
- For the current dissertation LaTeX style file, some TOC layout controls are not explicitly defined (`cft*indent/numwidth`), so renderer fallback behavior is expected.
- Remaining style gaps are tracked as missing LaTeX parameter mappings, not patched via project-specific constants.

## Non-negotiable decisions

- Keep the project fully LaTeX-driven.
- Do not add one-off hardcoded values to match a single corpus.
- Any style fix must follow:
  1. Extract parameter from LaTeX (or explicit project config).
  2. Store it in model metadata with generic naming.
  3. Apply it in renderer with deterministic fallback only.

## Next development steps

1. Extend TOC mapping for currently uncovered commands (`\setpnumwidth`, `\cftbefore...skip`, additional `\cft...aftersnum` controls where relevant).
2. Continue reducing DOCX style drift by adding parser-first extraction for missing layout/typography signals.
3. Keep `docs/SUPPORTED_ELEMENTS.md` aligned with exact mapped vs unmapped LaTeX controls.
4. Keep tests paired with every new `DocumentLayout` field (extraction present/absent + renderer fallback behavior).

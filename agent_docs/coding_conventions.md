# Coding Conventions

## Scope

These rules apply to all ferritex Rust code (`crates/`, `src/`, tests, and internal tooling code in this repository).

## 1) No Magic Numbers

- Do not embed unexplained numeric literals in logic.
- Use named `const` values for all non-trivial numbers.
- Every non-obvious constant must have a Rust doc comment that explains:
  - what the unit is (`twips`, `half-points`, etc.),
  - where the value comes from (LaTeX convention, DOCX requirement, fallback policy).

## 2) LaTeX-Driven Formatting Only

- Renderer formatting must come from `DocumentLayout` fields.
- Do not add hardcoded renderer formatting that bypasses parser extraction.
- Any new formatting parameter must be implemented end-to-end:
  - parser extraction in `crates/ferritex-core/src/parser/latex.rs`,
  - model field in `crates/ferritex-core/src/model/mod.rs`,
  - renderer consumption in `crates/ferritex-renderer-docx/src/lib.rs`.

## 3) Fallback Defaults Location

- Fallback defaults are allowed only in `RenderProfile::from_layout()`.
- Do not duplicate fallback logic in other renderer functions.
- Parser should return `None` / empty when LaTeX does not express a setting.

## 4) Comments Explain Why

- Use comments for intent, constraints, and tradeoffs.
- Do not add comments that restate obvious code mechanics.
- Prefer short comments near complex parsing/rendering branches.

## 5) Error Handling

- Library code: use `anyhow::Result` or typed errors.
- No `unwrap()` in library/runtime code.
- `unwrap()` is allowed only in tests where panic is intentional.

## 6) Types and Units

- Prefer domain-specific fields and clear naming over ambiguous integers.
- Preserve unit clarity in field/constant names (`_twips`, `_hp`, etc.).
- Convert units once at extraction/mapping boundaries, not repeatedly in deep rendering code.

## 7) Generic Product Mindset

ferritex is a generic LaTeX-driven converter for many document classes (articles, journals, dissertations, books, theses). Code must not assume any single corpus-specific formatting profile.

## 8) Three-File Rule

Every new LaTeX element requires changes in all three layers:
1. Parser extraction: `crates/ferritex-core/src/parser/latex.rs`
2. Model field: `crates/ferritex-core/src/model/mod.rs`
3. Renderer consumption: `crates/ferritex-renderer-docx/src/lib.rs`

## 9) Test Conventions

- Every `DocumentLayout` field must have paired tests:
  1. Positive: extraction from a representative LaTeX snippet.
  2. Negative: `None`/default when the command is absent.
  3. Renderer fallback: minimal document renders without panics using documented fallback.

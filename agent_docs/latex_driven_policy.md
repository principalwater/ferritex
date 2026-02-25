# LaTeX-Driven Policy

## Core Principle

ferritex is a generic LaTeX-driven converter (DOCX implemented; PDF/Markdown scaffolded).
All formatting parameters must be derived from the LaTeX project, not hardcoded in the renderer.

## Required Data Flow

Every formatting rule must follow this pipeline:

1. LaTeX source (main file + included style/config files)
2. Parser extraction (`crates/ferritex-core/src/parser/latex.rs`)
3. `DocumentLayout` field (`crates/ferritex-core/src/model/mod.rs`)
4. Renderer mapping (`RenderProfile::from_layout()` in `crates/ferritex-renderer-docx/src/lib.rs`)
5. Backend emission logic consumes mapped values

## Product Coverage

The implementation must work for arbitrary LaTeX projects, including:
- articles,
- journal papers,
- dissertations/theses,
- books,
- technical reports.

No behavior may be tied to one private template or one project-specific style package.

## Renderer Hardcoding Policy

- Renderer constants are fallback defaults only.
- Fallbacks apply only when LaTeX does not provide a value.
- Renderer must not introduce one-off formatting overrides for a specific corpus.

## Bug-Fix Rule

When users report formatting mismatches:

- First fix parser extraction and layout propagation.
- Do not patch with renderer-local hardcoded values as the primary fix.

## Change Checklist for New Formatting Parameters

For each new LaTeX formatting feature, update all three layers:

1. Parser extraction (`crates/ferritex-core/src/parser/latex.rs`)
2. Model field (`crates/ferritex-core/src/model/mod.rs`)
3. Renderer consumption + fallback mapping (`crates/ferritex-renderer-docx/src/lib.rs`)

Plus tests for extraction behavior and fallback behavior.

This checklist applies to **all backends** (DOCX, PDF, MD), not just the DOCX renderer.

## Known Exception

`apply_known_counter_fallbacks` in `parser/latex.rs` contains dissertation-specific
Russian counter placeholder templates. This is gated on `ParseMetadata.document_class`
containing `"disser"`: only documents whose `\documentclass` matches that pattern
trigger the replacement. All other document classes get an early no-op return.
See `AGENTS.md` for the full exception note.

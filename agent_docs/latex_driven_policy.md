# LaTeX-Driven Policy

## Core Principle

ferritex is a generic LaTeX→DOCX converter.  
All formatting parameters must be derived from the LaTeX project, not hardcoded in the renderer.

## Required Data Flow

Every formatting rule must follow this pipeline:

1. LaTeX source (main file + included style/config files)
2. Parser extraction (`src/parser/latex.rs`)
3. `DocumentLayout` field (`src/model/mod.rs`)
4. Renderer mapping (`RenderProfile::from_layout()` in `src/renderer/docx.rs`)
5. DOCX emission logic consumes mapped values

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

1. Parser extraction (`src/parser/latex.rs`)
2. Model field (`src/model/mod.rs`)
3. Renderer consumption + fallback mapping (`src/renderer/docx.rs`)

Plus tests for extraction behavior and fallback behavior.

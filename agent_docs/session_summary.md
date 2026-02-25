# Session Summary (updated 2026-02-25)

## Project Goal

`ferritex` (FerriTeX) is a generic LaTeX-driven converter with:
- a single shared semantic core (`ferritex-core`),
- multiple backend renderers (`docx` implemented, `pdf`/`md` scaffolded),
- zero project-specific formatting hardcodes.

All layout/formatting behavior comes from LaTeX sources and/or explicit project config:

```text
LaTeX source -> parser extraction -> AST + DocumentLayout -> backend renderer
```

## Repository State

- **Current version**: v0.9.1
- **Branch**: `master` (clean, all PRs merged through #31)
- **Active branch**: `feat/docs-audit-and-update` (docs audit + hardcode inventory)
- **Build health**: quality gate passes (`cargo fmt`, `cargo clippy`, `cargo test` — 257+ tests)

## What ferritex-core can do

- Parse LaTeX documents with recursive `\input{}`/`\include{}` expansion
- Extract 30+ layout parameters from preamble: page geometry, fonts, line spacing,
  paragraph indent, heading format, list geometry, caption settings, TOC formatting,
  source-line spacing, float alignment, language tag
- Build a typed AST: sections, paragraphs, inline styles, tables, figures, lists,
  math (inline + display), footnotes, TOC marker, bibliography heading
- Resolve references, labels, bookmarks for cross-linking
- Extract title-page blocks with per-paragraph style overrides
- Language-aware defaults: chapter name, bibliography title derived from `document_language`
- Dissertation-specific counter fallbacks gated on `document_class` (known exception)

## What ferritex-renderer-docx can do

- Full DOCX generation from AST + DocumentLayout via `RenderProfile::from_layout()`
- Page setup: margins, paper size, different-first-page mode
- Document styles: BodyText, Heading1-3, Caption, TableGrid, FootnoteText
- TOC generation with indent/numwidth, dot leaders, chapter/page bold settings,
  aftersnum separators, anchor hyperlinks
- Tables with cell content and alignment
- Figures with captions (text only, no image embed yet)
- Lists with LaTeX-driven geometry (labelsep, labelwidth, bullet marker)
- Native DOCX footnotes with reference markers
- Cross-reference bookmarks and anchor hyperlinks
- Source attribution lines (italic, single-spaced, with vspace)
- Post-processing: footnote marker injection, language tag injection

## Known Issues and Remaining Hardcodes

### Renderer — HIGH (fix in v0.9.2)
1. Hyperlink text color hardcoded to `"000000"` — needs `\hypersetup{linkcolor=…}` extraction
2. Hyperlink underline hardcoded to `"none"` — needs `\hypersetup{colorlinks=…}` extraction
3. TOC chapter entries unconditionally uppercased — should respect `heading_uppercase`

### Renderer — MEDIUM (fix in v0.9.2)
4. Page gutter hardcoded to `0` — needs `DocumentLayout` field or documented fallback
5. Page number alignment hardcoded to `Center` — needs extraction
6. Body text alignment hardcoded to `Both` (justified) — needs extraction
7. Caption label style hardcoded to bold — needs `captionsetup{font=…}` extraction
8. TOC depth hardcoded to 2 — needs `\setcounter{tocdepth}{N}` extraction
9. TOC tab stop minimum `1_000` twips — safety guard, needs comment

### Parser — MEDIUM (fix in v0.9.2)
10. Em-to-twips conversion assumes 14pt base (`280.0`) — should compute from body font size
11. Plain-text chapter heading detection (`"ГЛАВА"`) is Russian-only without language gating
12. DOCX line-spacing unit `240.0` in parser — should be in renderer constants

### Not yet implemented
- OMML / Word equation rendering (currently plain-text approximation)
- Image embedding in DOCX
- Bibliography entry list rendering
- Resolved `\ref`/`\cite` hyperlinks in body text
- PDF output backend
- Markdown output backend

## Key Agreements (must stay stable)

1. No project-specific renderer hacks for one corpus.
2. Every formatting fix follows parser -> `DocumentLayout` -> renderer mapping.
3. Renderer constants are fallback-only when LaTeX/config does not specify a parameter.
4. Decisions and stable context must be written into repository Markdown files;
   chat/global memory is not authoritative.
5. For manual QA build requests, the user provides the input path at runtime.
   Default output directory: `/tmp/` (e.g. `/tmp/output.docx`).

## Completed in Recent Sessions

### Session 2026-02-24
- PR #28: WIP stabilization (parser/renderer/model/tests/docs)
- PR #29: LaTeX-driven audit — 4 policy violations fixed:
  1. `Block::TableOfContents` replaces Russian string-matching TOC detection
  2. List labelsep fallback scaled with body font size
  3. Bibliography default title language-aware
  4. Dissertation counter fallbacks gated on document class
- PR #30: CLAUDE.md added
- PR #31: README.md rewritten with FerriTeX branding

### Session 2026-02-25
- Full repository audit (code + docs):
  - 3 HIGH + 6 MEDIUM renderer hardcodes identified
  - 3 MEDIUM parser hardcodes identified
  - Privacy violations removed from 4 files (previous session WIP)
  - ROADMAP.md updated through v0.9.2
  - All agent_docs and docs files reviewed for accuracy
  - session_summary.md, plans, memory_policy.md, future_session_prompt.md updated
  - New plan: `v0.9.2-remaining-hardcodes.md`

## Immediate Next Steps

1. Commit docs audit + hardcode inventory to `feat/docs-audit-and-update` branch, open PR
2. Implement v0.9.2 fixes (12 items above — hyperlinks, TOC uppercase, tocdepth, body alignment, em-to-twips, etc.)
3. Continue DOCX parity workstreams from `v0.9.1-docx-manual-qa-followup.md`
4. Validate updated DOCX on synthetic fixtures plus large multi-file corpus
5. Only after DOCX parity stabilization, resume v1.0 backend expansion (PDF/MD)

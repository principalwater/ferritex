# Session Summary (2026-02-24)

## Project Goal

`ferritex` is a generic LaTeX-driven converter with:
- a single shared semantic core (`ferritex-core`),
- multiple backend renderers (`docx` implemented, `pdf`/`md` scaffolded),
- zero project-specific formatting hardcodes.

All layout/formatting behavior must come from LaTeX sources and/or explicit project config, then propagate through:

```text
LaTeX source -> parser extraction -> AST + DocumentLayout -> backend renderer
```

## Repository State Snapshot

- Current branch: `master`.
- Uncommitted WIP exists in:
  - `crates/ferritex-core/src/parser/latex.rs`
  - `crates/ferritex-core/src/model/mod.rs`
  - `crates/ferritex-renderer-docx/src/lib.rs`
- Build health restored for current WIP:
  - parser lifetime bug (`E0515` in `extract_setlist_param_raw`) fixed,
  - quality gate currently passes: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`.

This means the current tree is an in-progress parity batch, not a merge-ready state.

## Stable Baseline (already merged before current WIP)

1. Workspace split into backend-oriented crates:
   - `ferritex-core`
   - `ferritex-renderer-docx`
   - `ferritex-renderer-pdf` (stub)
   - `ferritex-renderer-md` (stub)
2. Unified build flow (`build::run_build`) and `OutputFormat::{Docx,Pdf,Md,Both,All}`.
3. Existing DOCX support includes:
   - sections/paragraphs/lists/tables/figures,
   - captions/source lines,
   - footnotes,
   - TOC generation,
   - layout mapping from many LaTeX preamble/style parameters.

## Current WIP (manual QA parity follow-up, not finalized)

### `ferritex-core` (parser/model side)

1. Parse-mode split:
   - `parse_latex(...)` keeps legacy resolved-text behavior,
   - `parse_latex_file(...)` enables preserving dynamic reference nodes for renderer-side hyperlinking.
2. Standalone label handling:
   - detects chunks like `\label{...}` and attaches them to the previous relevant block (section/table/figure/equation).
3. Reference resolution extensions:
   - optional preservation of `Inline::Reference` for anchor-aware DOCX rendering.
4. Footnote citation repeat logic (early implementation):
   - consecutive repeat -> `Там же.`,
   - repeated non-consecutive -> `... Указ. соч.`,
   - first mention -> formatted bibliographic entry.
5. Bibliography injection (early implementation):
   - after `Block::BibliographyHeading`, parser injects numbered bibliography paragraphs based on parsed `.bib`.
6. List settings extraction expanded:
   - support for `itemindent` and `leftmargin` from `\setlist{...}`,
   - partial `\dimexpr` evaluation for enumitem expressions.
7. Model extension:
   - `DocumentLayout.list_item_indent_twips: Option<i32>`.
8. Title-page text-flow extraction upgraded:
   - `titlepage` now treated the same as `titlingpage` for paragraph-style overrides,
   - leading `\vspace{...}` in mixed title-page chunks is propagated into `ParagraphStyle.space_before_twips`,
   - spacing directives now recognize `\setSpacing{...}`, `\setstretch{...}`, and `\linespread{...}`,
   - `\fontsize{<size>}{<baseline>}` now propagates both font size and per-paragraph line-spacing override.
9. Global line-spacing extraction hardened:
   - document-level spacing (`DocumentLayout.body_line_spacing_twips`) is now extracted from preamble only,
   - body-local `\linespread{...}` no longer overrides global layout spacing.
10. Title-page block-layout extraction further refined:
   - manual `\\` line breaks now drop synthetic leading/trailing spaces around break markers,
   - `flushright + tabular{l}` title-page blocks are now mapped to a generic paragraph left-indent estimate (model extension: `ParagraphStyle.left_indent_twips`),
   - memoir `\sethangfrom{\noindent #1}` is now treated as a heading-justify signal (`heading_alignment = "both"` unless explicit center/right is set).
11. Spacing-command semantics refined for memoir/setspace compatibility (updated 2026-02-24):
   - `\OnehalfSpacing` / `\DoubleSpacing` / `\SingleSpacing` are now parsed as size-aware factors (instead of fixed 1.5/2.0/1.0 assumptions),
   - memoir uppercase commands use memoir class factors by `\documentclass` size (`10/11/12/14/17/9pt` mappings),
   - 14pt memoir one-half/double mappings are calibrated for DOCX visual parity (`312`/`416` twips equivalents),
   - setspace lowercase commands use setspace factors (`10/11/12pt` mappings + fallback),
   - title-page chunk-level spacing extraction now uses the same logic as document-level spacing extraction.
12. TOC chapter-spacing extraction extended (updated 2026-02-24):
   - `DocumentLayout` now carries `toc_chapter_space_before_twips`,
   - parser extracts `\cftbeforechapterskip` (and memoir default when absent),
   - DOCX renderer applies this spacing before numbered TOC chapter entries (`ГЛАВА ...` lines).
13. Title-page tabular block spacing and TOC chapter pre-gap tuned (updated 2026-02-24):
   - title-page `flushright + tabular{l}` blocks now receive generic vertical box-padding in parser (`ParagraphStyle.space_before/space_after`) so DOCX preserves LaTeX tabular block extents without corpus-specific hacks,
   - memoir fallback for `\cftbeforechapterskip` is slightly expanded in deterministic mapping to reduce under-spacing before chapter entries in DOCX TOC.

### `ferritex-renderer-docx` (renderer side)

1. Reference index/bookmark infrastructure introduced:
   - label -> displayed value
   - label -> bookmark
   - section -> bookmark
   - TOC entry -> anchor mapping
2. Internal hyperlinks in DOCX (WIP):
   - `Inline::Reference` can render as `HyperlinkType::Anchor` when target bookmark exists.
3. TOC rendering updates (WIP):
   - entry text can be wrapped in anchor hyperlinks,
   - per-level `aftersnum`, chapter/page bold settings respected.
4. Indentation changes (WIP):
   - section/list handling moved toward first-line indentation behavior,
   - list item first-line indent uses new layout field.
5. Source/caption/table updates (WIP):
   - source lines rendered with single spacing + footnote-size italic style,
   - table paragraphs use `TableGrid` style and inline reference-aware rendering.
6. Bookmark attachment for section/figure/table/equation nodes (WIP).
7. Stabilization pass completed for current WIP:
   - parser + renderer unit tests synchronized with new helper signatures (`ReferenceRenderIndex` arguments and expanded list-settings tuple),
   - clippy compliance restored without `allow` downgrades (`collapsible_if`, function argument count).
8. Additional parser tests added for title-page spacing/line-spacing behavior:
   - mixed `titlepage` chunk with `\centering` + leading `\vspace`,
   - title-page `\setstretch` line-spacing override,
   - global `\linespread` extraction into `DocumentLayout.body_line_spacing_twips`.
9. TOC and heading paragraph layout mapping refined:
   - TOC paragraph styles/entries now use document body line spacing (LaTeX-driven `DocumentLayout.body_line_spacing_twips`) instead of hardcoded single spacing,
   - styled paragraphs now consume `ParagraphStyle.left_indent_twips`,
   - heading styles/paragraphs now honor parser-provided justify alignment from memoir `\sethangfrom`.

## Open Issues / Limitations (from latest manual QA and current state)

1. Title-page spacing/line spacing extraction and supervisor-block offset mapping were improved, but final visual parity still requires manual DOCX validation on full corpus.
2. TOC active links are in place; visual parity still needs manual verification after latest line-spacing update.
3. Indent behavior still has regressions (full-block shift vs first-line-only expectations) in lists/body paragraphs.
4. Math is still rendered as plain text approximation; Word equation/OMML rendering is pending.
5. Citation behavior needs full LaTeX-style parity (`Там же.`, `Указ. соч.`, related bibliography semantics).
6. Figure/table source-line formatting still needs final spacing/indent tuning.
7. Table content readability/formatting needs additional mapping parity.
8. Cross-reference hyperlinks in body text (figures/tables/sections/appendices) need full validation.
9. Bibliography section rendering is still incomplete at semantic quality level.
10. Appendix rendering remains a deferred follow-up area.

## Key Agreements (must stay stable)

1. No project-specific renderer hacks for one corpus.
2. Every formatting fix follows parser -> `DocumentLayout` -> renderer mapping.
3. Renderer constants are fallback-only when LaTeX/config does not specify a parameter.
4. Decisions and stable context must be written into repository Markdown files; chat/global memory is not authoritative.
5. For manual QA build requests from the user, use:
   - input: `/Users/principalwater/Documents/git/phd-eaeu-electricity-market/thesis/dissertation.tex`,
   - outputs by format: `/tmp/dissertation.docx`, `/tmp/dissertation.pdf`, `/tmp/dissertation.md`.

## Immediate Next Steps

1. Continue DOCX parity workstreams now that build/test baseline is green (links/TOC/indent/citations/bibliography/title-page spacing).
2. Validate updated DOCX behavior on synthetic fixtures plus a large external multi-file LaTeX corpus.
3. Record resolved vs deferred manual-QA items in `v0.9.1` plan and `SUPPORTED_ELEMENTS`.
4. Only after DOCX parity stabilization, resume v1.0 backend expansion (`pdf`/`md`).

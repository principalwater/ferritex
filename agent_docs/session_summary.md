# Session Summary (LaTeX-driven DOCX pipeline)

## Project goal

`ferritex` is a generic LaTeX-driven converter (currently with DOCX output) for arbitrary LaTeX projects:
- academic papers
- journal submissions
- dissertations
- technical reports

The core requirement is strict: formatting and layout behavior must be derived from LaTeX/project configuration, not from hardcoded renderer constants.

## Pipeline architecture

```
LaTeX source → Parser (extract_layout_settings) → DocumentLayout
                                                        ↓
                 AST (Block/Inline)            RenderProfile::from_layout()
                       ↓                               ↓
                 renderer/docx.rs ← ← ← ← ← ← ← ← ← ←
```

- Parser extracts structure and style intent from LaTeX.
- `DocumentLayout` carries style metadata (all `Option<T>` fields; `None` = "LaTeX did not express this, use fallback").
- `RenderProfile::from_layout()` resolves every field to a concrete value with fallback defaults.
- Renderer consumes `RenderProfile` only — never `DocumentLayout` directly.

## Non-negotiable decisions

- Keep the project fully LaTeX-driven.
- Do not add one-off hardcoded values to match a single corpus.
- Any style fix must follow:
  1. Extract parameter from LaTeX (or explicit project config).
  2. Store it in `DocumentLayout` with generic naming.
  3. Resolve in `RenderProfile::from_layout()` with fallback default.
  4. Consume in renderer via `RenderProfile`.
- Constants in `renderer/docx.rs` are **fallback defaults only**, not project-specific tweaks.

## Observed constraints

- Visual parity with manually edited DOCX is not always exact; Word-level overrides in manual files may not correspond to any LaTeX source expression.
- Remaining style gaps are tracked as missing LaTeX parameter mappings, not patched via project-specific constants.
- Tests must be paired with every new `DocumentLayout` field: present → correct value, absent → `None`, renderer fallback.

## Merged PRs (master, as of 2026-02-24)

| PR | Branch | Content |
|----|--------|---------|
| #11 | build-core | Unified build orchestrator (`ferritex build`) |
| #12 | parser-resilience | Parser error resilience improvements |
| #13 | preamble+chapter | Preamble extraction, chapter rendering |
| #14 | gost-styling | GOST-compliant heading/page styles |
| #15 | latex-driven-pipeline | Full LaTeX-driven pipeline (DocumentLayout) |
| #16 | font-size-extraction | Font size extraction from documentclass |
| #17–#20 | various | captionsetup, figure/table label extraction |
| #21 | project-memory-docs | agent_docs/ directory setup |
| #22 | ci-fix | Bot merge CI fix |
| #23 | v0.8-toc-parity | TOC cft* mapping, LineBreak fix, paired tests |

## v0.8 (PR #23, merged): TOC parity + paragraph drift fix

Added 7 new `DocumentLayout` fields for `cft*` TOC formatting:
- `toc_chapter_entry_bold` / `toc_chapter_page_bold` — driven by `\cftchapterfont` / `\cftchapterpagefont`
- `toc_aftersnum_chapter/section/subsection/subsubsection` — driven by `\cft...aftersnum`
- `toc_appendix_name` — driven by `\cftappendixname` (extracted; appendix TOC rendering deferred)

Fixed:
- Paragraphs consisting solely of `Inline::LineBreak` no longer produce empty `Block::Paragraph` entries.
- `extract_renewcommand_value()` now handles both `\renewcommand{\cmd}{...}` and `\renewcommand\cmd{...}` forms.

Renderer:
- `toc_level_aftersnum()` helper on `RenderProfile` for per-level separator.
- `build_toc_entry_paragraph()` applies `disable_bold()` and aftersnum separator correctly.

Tests: 11 parser unit tests + 3 renderer unit tests. Total: 198 lib tests + 6 integration tests.

## v0.9 (feat/v0.9-visual-parity, IN PROGRESS): Visual parity batch

### Branch state (as of 2026-02-24)

- Branch: `feat/v0.9-visual-parity` (from master after PR #23)
- Modified: `src/model/mod.rs`, `src/parser/latex.rs`, `src/renderer/docx.rs`, `AGENTS.md`, `docs/SUPPORTED_ELEMENTS.md`, `agent_docs/*.md`
- New untracked: `agent_docs/plans/` directory
- Full quality gate: **PASSING** (`cargo fmt --all && cargo clippy -- -D warnings && cargo test`)
- Test status: 211 lib tests + 220 bin tests + 6 integration tests passing

### What was implemented in v0.9

**Task 1: List indent geometry fix**

New `DocumentLayout` fields:
- `list_label_sep_twips` — from `\setlist{labelsep=...}`
- `list_label_width_twips` — from `\setlist{labelwidth=...}` (`None` when `!`/auto)
- `list_bullet_char` — from `\renewcommand{\labelitemi}{...}` (e.g. `"–"` for en-dash)

New renderer constants: `DEFAULT_LIST_LEFT_TWIPS=709`, `DEFAULT_LIST_HANGING_TWIPS=284`, `DEFAULT_LIST_BULLET="•"`.

`from_layout()` now computes:
- `list_left_indent_twips` = `body_first_line_indent_twips` (= parindent) when not explicitly set
- `list_hanging_indent_twips` = `list_label_sep_twips + list_label_width_twips` (auto width = labelsep)
- `list_bullet_char` from layout or fallback `"•"`

`register_numbering()` now uses `profile.list_bullet_char` instead of hardcoded `"•"`.

Parser: `extract_list_settings()` + `extract_setlist_param_twips()` + `extract_setlist_labelwidth_twips()` + `extract_labelitemi_char()` in `src/parser/latex.rs`.

**Task 2: Source line rendering fix**

New `DocumentLayout` fields:
- `source_vspace_table_twips` — vertical space above `\tablesource` line (from `\vspace{4pt}`)
- `source_vspace_figure_twips` — vertical space above `\figuresource` line (from `\vspace{2pt}`)

Defaults: `DEFAULT_SOURCE_VSPACE_TABLE_TWIPS=80` (4pt), `DEFAULT_SOURCE_VSPACE_FIGURE_TWIPS=40` (2pt).

`source_paragraph()` changed:
- Alignment: `AlignmentType::Both` → `AlignmentType::Left` (matches `\raggedright`)
- Italic: third arg to `inline_runs_with_footnote_size` is now `true`
- `vspace_twips` added as `space_before` via `LineSpacing::before()`
- Indent: explicit `FirstLine(0)` to suppress inherited first-line indent

Parser: `extract_source_vspace_twips()` finds `\vspace{<dim>}` inside `\newcommand`/`\renewcommand` definition body.

**Task 3: Caption indent verification**

- Verified parser behavior: figure caption indent remains `None` when not defined, table caption indent extracted from `\captionsetup`.
- Verified renderer fallback: `caption_indent_twips_figure=0` and `caption_indent_twips_table=0` when LaTeX does not define indent.
- Added renderer test: `caption_indent_defaults_to_zero_for_figure_and_table`.

**Task 4: Bibliography placeholder rendering**

New `Block::BibliographyHeading { title: String }` variant in model.

Parser: `try_parse_bibliography_command()` detects:
- `\printbibliography` → heading from optional `[title=...]` arg, or default `"СПИСОК ЛИТЕРАТУРЫ"`
- `\printbibliography[heading=nobibheading,...]` → skipped (no block emitted)
- `\insertbibliofullsorted`, `\insertbiblioauthor`, `\insertbibliofull` → default title

Also: `extract_printbibliography_title()` helper.

`BibliographyHeading` handled in all match statements in parser and renderer (no exhaustiveness gaps).

Renderer: `build_bibliography_heading()` renders as unnumbered `Heading1` paragraph, applying `heading_uppercase` and `heading_alignment` from `RenderProfile`.

**Task 5: Title page page number suppression**

New `DocumentLayout` field:
- `title_page_suppress_number` — `Some(true)` when `\thispagestyle{empty}` found in `\begin{titlingpage}` context or early in document.

`create_styled_docx()` now calls `docx.title_pg()` when `profile.title_page_suppress_number == true`, enabling DOCX different-first-page mode. First-page header is empty (no page number); subsequent pages use the standard centered page-number header.

Parser: `extract_title_page_suppress_number()` checks `\begin{titlingpage}` and `\begin{titlepage}` bodies, plus early-document fallback.

**Task 6: Unit tests**

Parser tests added (~12):
- `test_extract_list_settings_labelsep_em`
- `test_extract_list_settings_absent`
- `test_extract_labelitemi_char_endash`
- `test_extract_labelitemi_char_absent`
- `test_extract_source_vspace_tablesource`
- `test_extract_source_vspace_figuresource`
- `test_extract_source_vspace_absent`
- `test_extract_title_page_suppress_inside_titlingpage`
- `test_extract_title_page_suppress_absent`
- `test_try_parse_bibliography_printbibliography_no_title`
- `test_try_parse_bibliography_printbibliography_with_title`
- `test_try_parse_bibliography_nobibheading_skipped`
- `test_try_parse_bibliography_insertbibliofullsorted`
- `test_try_parse_bibliography_not_a_bib_command`

Renderer tests added (~6):
- `list_indent_defaults_use_body_first_line_indent`
- `list_bullet_char_fallback_is_bullet`
- `list_bullet_char_from_layout`
- `source_vspace_defaults`
- `source_vspace_from_layout`
- `caption_indent_defaults_to_zero_for_figure_and_table`
- `title_page_suppress_defaults_false`
- `title_page_suppress_from_layout`

### What is NOT done yet in v0.9 (to do next session)

1. **CI/merge wait**: PR #24 is open with `bot-merge`; wait for green CI and repository gates.
2. **Optional manual visual validation** on large external LaTeX corpus: check list wrapping, source lines, bibliography heading, first-page numbering.

### Deferred (not in v0.9 scope)

- Math OOXML `<w:oMath>` (requires docx-rs raw XML injection)
- Citation "Там же." / "Указ. соч." deduplication (requires runtime state)
- Internal hyperlinks / TOC active hyperlinks (requires DOCX bookmark infrastructure)
- Appendix letter numbering (`\Asbuk`)
- `.bib` file parsing for actual bibliography entries
- `\cftbefore...skip` TOC entry spacing

## Plans location

Active plan: `agent_docs/plans/v0.9-visual-parity.md`

## Key files changed in v0.9 / follow-up

| File | Changes |
|------|---------|
| `src/model/mod.rs` | +8 new `DocumentLayout` fields; `Block::BibliographyHeading` variant |
| `src/parser/latex.rs` | +6 extraction fns; `try_parse_bibliography_command`; call sites; +14 tests |
| `src/renderer/docx.rs` | +6 new constants; updated `RenderProfile` + `from_layout()`; `source_paragraph()` fixed; `build_bibliography_heading()`; `create_styled_docx()` title_pg; +8 tests |
| `docs/SUPPORTED_ELEMENTS.md` | v0.9 support rows added (list geometry, source vspace, bibliography heading commands, title-page number suppression) |
| `AGENTS.md` | LaTeX-driven policy table extended with v0.9 parameter categories |
| `tests/unit/parser_latex_tests.rs` | parser unit tests moved out of `src/parser/latex.rs` |
| `tests/unit/renderer_docx_tests.rs` | renderer unit tests moved out of `src/renderer/docx.rs` |
| `tests/unit/README.md` | documents new test layout and rationale |

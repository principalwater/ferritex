# Session Summary (updated 2026-02-25)

## Project Goal

`ferritex` is a generic LaTeX-driven conversion engine with:
- a shared semantic core (`ferritex-core`),
- backend renderers (`ferritex-renderer-docx` implemented, `pdf`/`md` scaffolded),
- no document-specific formatting hardcodes.

Mandatory pipeline:

```text
LaTeX source -> parser extraction -> AST + DocumentLayout -> RenderProfile::from_layout() -> renderer
```

All renderer constants are fallback defaults only.

## Repository State

- Active branch: `feat/docs-audit-and-update`
- Working tree: dirty (v0.9.2+ parity fixes are local, not committed in this branch yet)
- Open PR with conflicts: `#33` (`feat/docs-audit-and-update` -> `master`), `mergeStateStatus=DIRTY`
  - root cause: squash-merge trap (branch still contains commit equivalent to already merged `#32`)
- Local quality gate on current tree: green
  - `cargo fmt --all` ✅
  - `cargo clippy --workspace --all-targets -- -D warnings` ✅
  - `cargo test --workspace` ✅

## Incremental Update (2026-02-25, current working tree)

- Active implementation branch for current fixes: `fix/v0.9.3-list-parity`
- Completed in code (not yet committed/PR'ed):
  1. **Lists parity (wave-2 item #1)**:
     - DOCX list paragraphs switched from `FirstLine(...)` to proper hanging-indent mapping.
     - List text indent now resolves as `leftmargin + itemindent` with deterministic fallback behavior.
     - Parser `\setlist{...}` parameter extraction now prefers the **last** declaration (LaTeX override order), not the first one.
     - Added parser + renderer unit tests covering override order, hanging-indent XML, and leftmargin/itemindent compensation.
  2. **Superscript citation spacing (wave-2 item #9)**:
     - `append_inlines_to_paragraph` now trims trailing spaces before `Inline::Footnote` markers in the main render path.
     - Added renderer unit test asserting no trailing space before superscript footnote markers.
  3. **Parser coverage expansion for list geometry extraction**:
     - `\setlist{...}` parsing now handles:
       - key/value whitespace forms (`key = value`),
       - starred key aliases (e.g. `labelsep*`),
       - top-level comma splitting with nested `{...}` / `[...]` / `(...)` safety,
       - last-match override order for repeated declarations.
     - `itemindent` extraction now falls back to `listparindent` when `itemindent` is absent.
     - `\dimexpr` evaluation for list parameters now accepts length terms (e.g. `-1em+1pt`) in addition to plain integer twips substitutions.
     - LaTeX length parser now supports additional TeX units and additive glue forms:
       - units: `pc`, `bp`, `dd`, `cc`, `sp`, `ex` (plus existing `mm`, `cm`, `in`, `em`, `pt`),
       - forms: `<len> plus <len>`, `<len> minus <len>` (deterministic additive mapping).
     - Added parser unit tests covering all of the above extraction and conversion paths.
- Focused validation passed on this tree:
  - `cargo fmt --all` ✅
  - `cargo clippy -p ferritex-core -p ferritex-renderer-docx --all-targets -- -D warnings` ✅
  - `cargo test -p ferritex-renderer-docx -- --nocapture` ✅
  - `cargo test -p ferritex-core list_ -- --nocapture` ✅
  - `cargo test --test integration_docx -- --nocapture` ✅
  - `cargo clippy -p ferritex-core --all-targets -- -D warnings` ✅
  - `cargo test -p ferritex-core parse_latex_length_supports -- --nocapture` ✅

## Capability Snapshot

### `ferritex-core` (current)

- Recursive include expansion (`\input`, `\include`)
- AST coverage for sections/paragraphs/lists/tables/figures/display math/footnotes/TOC marker/bibliography heading
- `DocumentLayout` as style contract:
  - geometry/page size/gutter/header/footer
  - body spacing/font families/font sizes/paragraph indents/alignment
  - heading alignment/uppercase/indents/spacing/number delimiters (including level-specific delimiters)
  - TOC depth/indents/numwidth/leaders/chapter prefix/aftersnum/appendix prefix/chapter entry spacing
  - captions (labels/separators/position/skip/indent/singlelinecheck/labelfont bold)
  - list geometry and bullet marker
  - hyperlink styling (`\hypersetup`)
  - page number alignment
  - source-line spacing (`\tablesource`, `\figuresource`)
- Reference and label normalization (internal anchors/placeholders)

### `ferritex-renderer-docx` (current)

- DOCX style system (`BodyText`, `Heading*`, `TOC*`, `Caption`, `FootnoteText`, list/table paragraph styles)
- TOC generation from AST / `.toc`, with internal hyperlinks
- Table and figure rendering (caption/source handling, alignment mapping)
- Native DOCX footnotes and reference markers
- Parser-driven paragraph/heading/list/table mapping via `RenderProfile::from_layout()`
- Hyperlink color/underline, body/page-number alignment, heading spacing, TOC depth all driven from `DocumentLayout`

### Not implemented yet (major)

- Native OMML equation generation (math is still text approximation)
- Image embedding for DOCX
- Full bibliography entry rendering
- Production PDF/MD renderers

## Completed in This Session

Primary goal in this session: continue parity against a large dissertation-style LaTeX project without introducing corpus-specific hardcodes.

Implemented and validated:

1. Fixed heading-number delimiter semantics for memoir/disstyles patterns:
   - added `heading_number_delimiter_{section,subsection,subsubsection}` to `DocumentLayout`
   - parser extraction now splits chapter delimiter vs section-level delimiter via:
     - `\setcounter{headingdelim}{...}`
     - `\setsecnumformat{...}` fallback
   - DOCX renderer now applies delimiter by heading level instead of one global value

2. Fixed TOC false positives caused by inactive conditional branches in style files:
   - `toc_aftersnum_*` is now normalized from `headingdelim` when that counter is present
   - prevents incorrect TOC numbering like `1.1.` when effective style requires `1.1`

3. Added chapter-prefix override for TOC based on `chapstyle`:
   - avoids picking `\cftchaptername` from inactive branches when `chapstyle=0`

4. Added/updated parser+renderer unit tests for all changes above.

5. Rebuilt manual QA artifact:
   - `/tmp/dissertation.docx` (latest in this session)

## Manual QA Gaps Reported by User (Next Priority)

The following defects are confirmed as next work batch and must be solved parser-first:

1. Numbered and unordered lists:
   - wrong left indent and width-justification behavior

2. Title page fidelity:
   - header area and central title block differ from LaTeX PDF/manual DOCX

3. TOC pagination density:
   - content that moves to the next page in PDF/manual DOCX still fits on previous page in ferritex DOCX

4. Math support:
   - formulas still not rendered as proper Word equations

5. Appendices page layout:
   - mixed portrait/landscape pages and explicit LaTeX page breaks not respected

6. Table fonts:
   - font sizing/weight behavior remains partially incorrect

7. “Объем и структура работы” counter text mismatch:
   - rendered text matches canonical PDF text, but resulting DOCX page/figure/table counts differ

8. “Публикации по теме исследования” section rendering:
   - content rendered incorrectly (unsupported LaTeX constructs/macros in this block)

9. Superscript citation spacing:
   - footnote superscripts appear visually detached from surrounding text

## Architecture/Policy Decisions Reaffirmed

1. No renderer-side one-off style hacks for this dissertation template.
2. Every visual fix must map to generic LaTeX semantics and flow through:
   parser -> `DocumentLayout` -> `RenderProfile` -> renderer.
3. `ferritex` must remain reusable across articles/journals/dissertations/reports/books without code rewrites.
4. Repository Markdown files are the persistent multi-agent memory; chat/global memory is not authoritative.

## Immediate Next Steps

1. Execute the new active plan:
   - `agent_docs/plans/v0.9.3-docx-parity-disstyles-wave2.md`
2. Start cross-cutting test modernization stream:
   - `agent_docs/plans/v0.9.4-test-organization-and-property-based.md`
   - run comprehensive test organization audit (Rust Book model) and property-based adoption plan

3. Resolve PR conflict hygiene before merge:
   - rebuild branch from `origin/master`
   - cherry-pick only unique commits (exclude duplicate equivalent of merged `c33803e`)

4. Keep `v1.0` backend work (PDF/MD) blocked until this DOCX parity wave stabilizes on quality gate + manual QA.

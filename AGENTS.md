# AGENTS.md — Contributor & AI Agent Instructions (ferritex)

## Non-negotiable constraints
- This project is 100% Rust. No Python, no shell scripts, no Pandoc
  subprocess calls, no FFI to non-Rust libraries unless absolutely
  unavoidable (document why in this file if so).
- ferritex is a generic LaTeX→DOCX tool for arbitrary LaTeX projects
  (articles, journal papers, dissertations, books, theses). Do not tune
  implementation to a single corpus.
- All crate versions must be stable releases from crates.io. Do not
  use git dependencies or pre-release versions.
- CLI is the primary interface. Use clap with derive macros.
- All public types must have doc comments.
- No unwrap() in library code — use anyhow::Result or thiserror types.

## Architecture rules
- The pipeline is strictly: LaTeX source → AST (model/) → DOCX output
- The parser and renderer are completely decoupled via the AST.
  Never let docx-rs types leak into the parser, never let nom/regex
  types leak into the renderer.
- Treat `DocumentLayout` as the project-wide **StyleMap**:
  extracted style/layout semantics passed from parser to renderers.
- New LaTeX elements go in: src/parser/latex.rs + src/model/mod.rs
  + src/renderer/docx.rs — always all three.
- Rendering parameters must be propagated from LaTeX sources (including
  included style/config files) so ferritex output matches the effective
  LaTeX build settings for layout, numbering, spacing, page headers, and
  footnote/citation behavior.
- Style parameters must be extracted from LaTeX preamble/config sources;
  renderer hardcodes are fallback-only and never project-specific.
- Renderers consume only AST + StyleMap fields; renderers must not parse
  LaTeX syntax directly.
- Adding a new output format means adding a new renderer that consumes the
  same AST + StyleMap contract. Parser/model changes are required only when
  a new semantic parameter is needed for all backends.
- StyleMap must define reasonable deterministic defaults for every field so
  minimal LaTeX inputs still render without panics.
- Never implement project-specific formatting fixes by hardcoding constants
  in the renderer when the source LaTeX project already expresses the setting.
  Instead:
  1. Parse the parameter from LaTeX sources.
  2. Store it in AST/metadata with generic naming.
  3. Consume it in renderer via mapping with fallback defaults.
- Manual validation feedback must be translated into parser/model extraction
  improvements first, not one-off renderer hacks bound to a single corpus.

### LaTeX-driven parameter policy (comprehensive)
Every formatting decision in the renderer must trace back to parser
extraction:
- global document-level settings via `DocumentLayout`
- block-local directives (inside specific float/list blocks) via block fields
Renderer constants serve only as **fallback defaults** for when the
LaTeX project does not express a preference. The following parameter
categories must all follow this pattern — no exceptions:

| Category | LaTeX source | Model field(s) | Renderer fallback | Status |
|----------|-------------|----------------|-------------------|--------|
| Page margins | `\geometry{top=…, bottom=…, left=…, right=…}` | `page_margin_top/bottom/left/right/header/footer_twips` | GOST: 20/20/25/10 mm | ✅ parsed + rendered |
| Page size | `\geometry` `paperwidth`/`paperheight`, `\documentclass[a4paper]` | `page_width_twips`, `page_height_twips` | A4 (11906 × 16838 tw) | ✅ parsed + rendered |
| Line spacing | `\OnehalfSpacing`, `\setSpacing{1.5}`, `\linespread` | `body_line_spacing_twips` | 360 (1.5) | ✅ parsed + rendered |
| Float counter scoping | `\setcounter{contnumfig/contnumtab/contnumeq}{0\|1}` | `figure/table/equation_counter_within_chapter` | true (per-chapter) | ✅ parsed + rendered |
| Font family | `\setmainfont` (incl. `\ifnumequal` conditional), `\setmonofont` | `font_family_body`, `font_family_mono` | "Times New Roman" | ✅ parsed + rendered |
| Font sizes | `\documentclass[14pt]`, `\SetTblrInner{font=…}`, `\captionsetup{font=…}` | `font_size_body_hp`, `font_size_table_hp`, `font_size_footnote_hp`, `font_size_caption_hp` | 28, 24, 20, 28 (half-points) | ✅ parsed + rendered |
| Paragraph indent | `\setlength{\parindent}{…}` | `body_first_line_indent_twips` | 709 (1.25 cm) | ✅ parsed + rendered |
| Heading format | `\chaptername`, `\MakeUppercase`, numbering delimiter, alignment | `chapter_name`, `heading_uppercase`, `heading_alignment`, `heading_number_delimiter` | "", false, Left, "." | ✅ parsed + rendered |
| Heading indents | `\setsecindent`, `\setsubsecindent`, `\setsubsubsecindent` | `heading_indent_section/subsection/subsubsection_twips` | 0 twips | ✅ parsed + rendered |
| TOC formatting | `\setrmarg`, `\cft...leader`, `\cftchaptername`, `\cftsetindents`, `\setlength{\cft...indent}`, `\setlength{\cft...numwidth}`, `\cftchapterfont`, `\cftchapterpagefont`, `\cft...aftersnum`, `\cftappendixname` | `toc_right_margin_twips`, `toc_use_dot_leader`, `toc_chapter_name_prefix`, `toc_indent_*_twips`, `toc_numwidth_*_twips`, `toc_chapter_entry_bold`, `toc_chapter_page_bold`, `toc_aftersnum_{chapter,section,subsection,subsubsection}`, `toc_appendix_name` | level indents from body indent, no hanging (`numwidth=0`), entry/page bold=true, aftersnum="" | ✅ parsed + rendered |
| Caption labels | `\renewcommand{\figurename}{…}`, `\renewcommand{\tablename}{…}`, `\captionsetup{labelsep=…}`, `\DeclareCaptionLabelSeparator` | `caption_label_figure`, `caption_label_table`, `caption_label_separator_figure`, `caption_label_separator_table` | "Figure", "Table", ". " | ✅ parsed + rendered |
| Caption layout | `\captionsetup{skip=…, position=…, singlelinecheck=…, indent=…}` (global and `[figure]/[table]`) | `caption_skip_twips_figure/table`, `caption_position_figure/table`, `caption_singlelinecheck_figure/table`, `caption_indent_twips_figure/table` | skip=0 twips, figure position=bottom, table position=top, singlelinecheck=true, indent=0 | ✅ parsed + rendered |
| Float block alignment | `\centering`, `\raggedright`, `\raggedleft`, `\flushleft`, `\flushright` inside `table/figure` blocks | `Table.alignment`, `Figure.alignment` | table: left, figure: center | ✅ parsed + rendered |
| Language | babel/polyglossia main language | `document_language` | None (no language tag) | ✅ parsed + rendered |

**Test requirement**: every `DocumentLayout` field must have a
unit test in `parser/latex.rs` that:
1. Verifies extraction from a representative LaTeX snippet.
2. Verifies `None`/default when the command is absent.
3. Verifies the renderer produces valid DOCX with only fallback defaults
   (i.e. a minimal document without the LaTeX command must not panic and
   must use the documented fallback value).

## DOCX structure notes
- DOCX is a ZIP containing XML files (word/document.xml, etc.)
- Paragraphs are <w:p>, runs are <w:r>, properties are <w:pPr>/<w:rPr>
- Alignment: <w:jc w:val="both"/> for justify
- Section breaks that must NOT be inserted mid-document: <w:sectPr>
  inside <w:pPr> — these break text flow. Only one <w:sectPr> is
  allowed as the last child of <w:body>.
- Use docx-rs as the primary builder; fall back to quick-xml + zip
  only for features docx-rs doesn't expose.

## Supported LaTeX elements (expand as implemented)
Track implementation status in docs/SUPPORTED_ELEMENTS.md.

## Before every commit
- cargo fmt --all
- cargo clippy -- -D warnings
- cargo test

## PR automation context
- CI workflow name is `CI`; bot workflows react to successful CI runs.
- `bot-merge` label enables optional bot-driven squash merge with branch deletion after repository gates pass.
- If a PR branch is behind base, workflow requests an update before approval/merge steps.

## When adding a new crate
Update this file with: crate name, version pinned, reason for adding.

## Current crates in use
(agent must update this table when Cargo.toml changes)
| Crate | Version | Purpose |
|-------|---------|---------|
| clap | latest stable | CLI argument parsing |
| anyhow | latest stable | Error propagation |
| thiserror | latest stable | Error type definitions |
| docx-rs | latest stable | DOCX generation |
| quick-xml | latest stable | Low-level XML manipulation |
| serde/serde_json | latest stable | Serialization |
| zip | latest stable | ZIP/DOCX container |
| log + env_logger | latest stable | Logging |
| crossterm | latest stable | Terminal events/raw mode for TUI |
| ratatui | latest stable | TUI rendering/layout |

## Project extensions

### Pinned crate versions
| Crate | Version | Purpose |
|---|---|---|
| clap | 4.5.60 | CLI argument parsing |
| anyhow | 1.0.102 | Error propagation |
| thiserror | 2.0.18 | Error type definitions |
| docx-rs | 0.4.19 | DOCX generation |
| quick-xml | 0.39.2 | Low-level XML manipulation |
| serde | 1.0.228 | Serialization traits |
| serde_json | 1.0.149 | JSON support |
| walkdir | 2.5.0 | Recursive filesystem traversal |
| log | 0.4.29 | Logging facade |
| env_logger | 0.11.9 | Env-driven logging implementation |
| zip | 8.1.0 | ZIP/DOCX container handling |
| crossterm | 0.29.0 | Terminal events/raw mode for TUI |
| ratatui | 0.29.0 | TUI rendering/layout |

### Contributor checklist
- Keep `src/main.rs` as orchestration only.
- Put new parsing logic in `src/parser/latex.rs` and output-neutral AST changes in `src/model/mod.rs`.
- Put DOCX emission logic in `src/renderer/docx.rs`.

---

## Privacy & Open-Source Safety Rules

ferritex is a general-purpose open-source tool. It must contain
ZERO references to any specific person, institution, research domain,
dissertation, or private project.

### What must never appear in this repository
- Real names or usernames in source code, comments, fixtures, or docs
  (exception: `Cargo.toml authors` field — that is standard OSS practice)
- University or organization names
- Dissertation or thesis titles, chapter names, or subject matter
- Absolute local file paths (use relative paths or CLI args)
- Relative paths that point to external/private projects or corpora
  (e.g., `thesis/dissertation.tex` from another repo/workspace)
- Private GitHub repository names or URLs
- API tokens, SSH keys, credentials of any kind
- Email addresses (unless explicitly public and consented)

### Commit / PR metadata policy
- The same privacy rules apply to commit messages, PR titles, PR bodies,
  release notes, CI logs, and validation notes.
- Do not mention external/private project names, file paths, or identifiers
  in PR/commit text. Use generic wording:
  - good: `validated on a large multi-file LaTeX corpus`
  - bad: `validated on thesis/dissertation.tex`
- Validation examples in this repository must use synthetic paths such as
  `sample/main.tex` unless the file is inside this public repo.

### Test fixtures policy
- Test .tex fixtures must be synthetic and generic
  (e.g. "Sample Document", "Introduction", "Lorem ipsum...")
- Corner cases from real usage MAY be extracted as fixtures,
  but only after full anonymization — no real content, no real structure
  that could identify the source document
- Good fixture naming: simple.tex, with_footnotes.tex, with_table.tex
- Bad fixture naming: dissertation_chapter1.tex, author_thesis.tex

### When in doubt
If you are unsure whether a piece of content is sensitive,
remove it or replace it with a generic placeholder.

# Roadmap

## Engineering workflow ✅
- Required CI quality gate: fmt + clippy + test
- Optional bot-assisted merge path with `bot-merge` label (squash + delete branch after repository gates)

## Cross-version requirement
- Preserve a single LaTeX-driven semantic core for all output formats.
- New formatting features should be implemented as extraction/model semantics first (`LayoutProbe` + parser), then rendered in DOCX/PDF backends.
- Avoid backend-only visual hacks when source LaTeX intent can be propagated.
- Keep ferritex output aligned with the effective parameters of the canonical LaTeX build configuration (layout, numbering, headings, page numbers, footnotes/citations).
- Treat manual QA deltas as extraction/mapping gaps, not as invitations for corpus-specific hardcoded tweaks.
- Every formatting milestone must explicitly list which LaTeX parameters are now parsed and how they map to renderer settings.
- Fallback defaults are last-resort only: use them only when both probe and parser extraction are absent.

## v0.1 ✅
- Sections and subsections
- Paragraph text blocks
- Basic inline styles (bold/italic)

## v0.2 ✅
- Tables (`\begin{tabular}`, `\begin{tblr}`, `\begin{longtblr}`)
- Figures and captions (`\begin{figure}`, `\includegraphics`)
- `\tablesource` / `\figuresource` attribution lines
- GitHub Actions CI (fmt + clippy + test)

## v0.3 ✅
- Lists: `\begin{itemize}`, `\begin{enumerate}`

## v0.4 ✅
- Math: inline `$…$` and display `\begin{equation}…\end{equation}` / `\begin{equation*}…\end{equation*}` — rendered as plain-text approximation (italic; display math centered)

## v0.5 (partial)
- Footnotes: `\footnote{}` ✅
- Display math `\[…\]` ✅
- `\input{}` file inclusion ✅
- `\include{}` file inclusion ✅
- `\autocite{}` style-aware placeholder ✅
- Resolved cross-references (`\ref`, `\cite`) — still as `[key]` placeholders

## v0.6 ✅
- TUI mode on `ratatui` for interactive conversion
- Explicit CLI subcommands (`convert`, `tui`) with compatibility for `--input/--output`

## v0.7 ✅
- Unified build-core orchestrator (`src/build/`)
- `ferritex build --format docx|pdf|both|all` CLI entrypoint
- `convert` and `tui` routed through shared build core
- Path resolution and artifact naming centralized

## v0.8 ✅
- Workspace refactor: mono-repo with `ferritex-core`, `ferritex-renderer-docx`, `ferritex-renderer-pdf` (initially stub; baseline now implemented), `ferritex-renderer-md` (stub)
- TOC `cft*` formatting: indent/numwidth, dot leaders, chapter font, page font, aftersnum separators, appendix prefix
- Caption layout: `captionsetup` extraction (skip, position, singlelinecheck, indent, labelsep)
- Float alignment: `\centering`, `\raggedright`, etc. inside figure/table blocks
- Heading format: uppercase, alignment, indents, number delimiter extraction
- Language extraction: babel/polyglossia main language -> BCP-47 tag
- Font family/size extraction: `\setmainfont`, `\documentclass[Xpt]`, `\captionsetup{font=...}`

## v0.9 ✅
- Visual parity batch: list geometry from `\setlist{...}`, bullet marker from `\labelitemi`
- Source attribution lines: `\tablesource{...}`, `\figuresource{...}` with `\vspace` extraction
- Title page: `titlepage`/`titlingpage` paragraph style overrides, `\thispagestyle{empty}` page-number suppression
- Bibliography heading: `\printbibliography`, `\insertbibliofullsorted`, etc. with `title=` extraction
- Line spacing: memoir/setspace size-aware factors (`\OnehalfSpacing`, `\setstretch`, `\linespread`)
- Title page block layout: `flushright+tabular`, `\fontsize`, `\vspace` propagation

## v0.9.1 ✅
- WIP stabilization: all uncommitted parser/renderer/model changes committed (PR #28)
- LaTeX-driven audit — 4 policy violations fixed (PR #29):
  1. `Block::TableOfContents` AST node replaces Russian string-matching TOC detection
  2. List labelsep fallback scaled with actual body font size (em-based)
  3. Bibliography default title derived from document language (not hardcoded Russian)
  4. Dissertation counter fallbacks gated on document class containing "disser"
- CLAUDE.md added for Claude Code session rules (PR #30)
- README.md rewritten with FerriTeX branding (PR #31)
- Full documentation audit and hardcode inventory

## v0.9.2 ✅
- Hyperlink style extraction from `\hypersetup`:
  `linkcolor`/`allcolors` -> text color, `colorlinks`/`hidelinks` -> underline behavior
- TOC uppercase and chapter prefix casing now gated by `heading_uppercase`
- TOC depth extraction from `\setcounter{tocdepth}{N}`
- Body text alignment extraction from body-context alignment directives
- Page-number alignment extraction from footer/page-style commands
- Heading spacing extraction from memoir/disstyles skip commands (`before/afterchapskip`, `setbefore/after...skip`)
- Heading delimiter extraction by level from memoir/disstyles (`headingdelim`, `setsecnumformat`) and TOC `aftersnum` normalization for conditional style branches
- Caption label bold extraction from `\captionsetup{labelfont=…}`
- Plain-text Russian heading detection gated by document language
- Em-to-twips conversion made dynamic from actual body font size
- Page gutter extraction from `\geometry{bindingoffset=...}`
- TOC tab-stop minimum guard documented as DOCX safety constraint

## v0.9.3 (in progress)
- Explicit page breaks (`\newpage`, `\clearpage`, `\cleardoublepage`) are parsed and rendered as DOCX page-break paragraphs.
- Landscape markers (`\begin{landscape}`, `\end{landscape}`, `\landscape`, `\endlandscape`) now map to `Block::PageOrientationSwitch`.
- DOCX renderer now emits ordered section breaks with `w:type="nextPage"` and correct portrait/landscape section geometry.
- Parser consumes leading structural commands inside mixed text chunks, preserving orientation/page-break semantics before headings/paragraphs.

## v0.9.5 (foundation in progress)
- `LayoutProbeOutput` contract added to `ferritex-core`.
- Feature-gated embedded backend added: `ferritex-core` feature `layout-probe-tectonic` with `tectonic` (`0.15.0`, MIT).
- Root CLI crate now enables `layout-probe-tectonic` by default (`ferritex` feature wiring),
  so standard build/convert/tui flows are probe-enabled unless `--no-default-features` is used.
- Parser entrypoint (`parse_latex_file`) now applies deterministic merge:
  `LayoutProbe + parser -> DocumentLayout` with precedence `probe > parser`.
- Build start logs active probe mode (`tectonic` vs parser-only), and probe failures
  are reported before deterministic parser fallback is applied.
- Degraded probe mode now applies a field-level confidence model for
  typography-sensitive fields (`font_size_body_hp`, `body_line_spacing_twips`)
  when TeX error traces are detected in probe logs, avoiding incorrect DOCX-wide
  line-spacing regressions while retaining geometry/list probe signals.
- Probe runtime now uses direct low-level `TexEngine` pass orchestration:
  - explicit engine profile (`halt_on_error`, `shell_escape`, `build_date`) remains aligned with runtime settings,
  - primary probe pass is `PassSetting::Tex` (single-pass extraction),
  - conditional recovery pass uses `PassSetting::Default` with fixed `reruns=1` when primary signal is empty or errored.
- Build-core external tool install policy is now explicit:
  - `--tool-install-policy ask|auto|never` (default `ask`),
  - currently consumed by PDF runtime for policy-aware biber compatibility bootstrap.
- `TotPages` layering no longer depends on `.aux` only:
  - sidecar resolution now supports `.aux` and `.log`,
  - fallback heuristic now considers `\\newpage` + `\\clearpage` + `\\cleardoublepage` + `\\pagebreak`,
  - runtime fallback can infer page count from canonical tectonic PDF bytes when sidecars are absent.
- First probe-covered fields:
  page geometry, body font size/family, paragraph indent, line spacing, list geometry.
- Optional helper crates for targeted tasks:
  - `codebook-tree-sitter-latex` (`0.6.1`, MIT) for syntax indexing only,
  - `biblatex` (`0.11.0`, MIT/Apache-2.0) for bibliography semantics.
- License guardrail: avoid GPL-only TeX engine crates (`tex_engine`, `rustex_lib`) in MIT ferritex core.
- Regression coverage added for merge precedence, absent-probe fallback behavior,
  multi-file integration, and property-style determinism/conversion invariants.

## v1.0 (in progress)
- OMML / Word equation rendering (upgrade from plain-text math approximation)
- Resolved `\ref` / `\cite` cross-reference hyperlinks in body text
- Bibliography entry list rendering (heading is placed, entries are pending)
- Image embedding in DOCX
- PDF output backend (`ferritex-renderer-pdf`) baseline:
  - canonical runtime path via `tectonic::latex_to_pdf` is implemented,
  - integration test validates `build --format pdf` artifact generation.
- Markdown output backend (`ferritex-renderer-md`)

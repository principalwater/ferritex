# Roadmap

## Engineering workflow ✅
- Required CI quality gate: fmt + clippy + test
- Optional bot-assisted merge path with `bot-merge` label (squash + delete branch after repository gates)

## Cross-version requirement
- Preserve a single LaTeX-driven semantic core for all output formats.
- New formatting features should be implemented as parser/model semantics first, then rendered in DOCX/PDF backends.
- Avoid backend-only visual hacks when source LaTeX intent can be propagated.
- Keep ferritex output aligned with the effective parameters of the canonical LaTeX build configuration (layout, numbering, headings, page numbers, footnotes/citations).
- Treat manual QA deltas as extraction/mapping gaps, not as invitations for corpus-specific hardcoded tweaks.
- Every formatting milestone must explicitly list which LaTeX parameters are now parsed and how they map to renderer settings.

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
- Workspace refactor: mono-repo with `ferritex-core`, `ferritex-renderer-docx`, `ferritex-renderer-pdf` (stub), `ferritex-renderer-md` (stub)
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

## v0.9.2 (next)
- Hyperlink color extraction from `\hypersetup{linkcolor=…}` (currently hardcoded black)
- Hyperlink underline extraction from `\hypersetup{colorlinks=…}` (currently hardcoded none)
- TOC uppercase gated on `heading_uppercase` (currently unconditional)
- TOC depth extraction from `\setcounter{tocdepth}{N}`
- Body text alignment extraction (currently hardcoded `Both`/justified)
- Page number alignment extraction (currently hardcoded `Center`)
- Caption bold extraction from `\captionsetup{labelfont=…}`
- Plain-text chapter heading detection language-gated (currently Russian-only)
- Em-to-twips conversion dynamic based on body font size (currently assumes 14pt)
- Page gutter field or documented fallback (currently hardcoded `0`)

## v1.0
- OMML / Word equation rendering (upgrade from plain-text math approximation)
- Resolved `\ref` / `\cite` cross-reference hyperlinks in body text
- Bibliography entry list rendering (heading is placed, entries are pending)
- Image embedding in DOCX
- PDF output backend (`ferritex-renderer-pdf`)
- Markdown output backend (`ferritex-renderer-md`)

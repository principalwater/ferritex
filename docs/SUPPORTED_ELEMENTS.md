# Supported LaTeX Elements

Rendering policy:
- Prefer LaTeX-originated semantics over backend-local defaults.
- DOCX/PDF backends should produce equivalent structure from the same AST.
- Effective LaTeX build parameters are the source of truth for layout and style decisions; backend constants are fallback-only.
- New visual fixes must be implemented as generic parameter mapping, not hardcoded values for a specific project.
- Target extraction architecture: `LayoutProbe` (engine-evaluated values, feature-gated) + parser static extraction, merged before rendering.

| Element | Status | Notes |
|---|---|---|
| `\chapter{}`, `\section{}`, `\subsection{}`, `\subsubsection{}` | ✅ v0.1 | Including `*` (unnumbered) variants; chapter→H1, section→H2, subsection/subsubsection→H3 |
| Paragraph text | ✅ v0.1 | Blank-line separated, whitespace normalized |
| `\textbf{}`, `{\bf …}`, `{\bfseries …}` | ✅ v0.1 | Recursive nesting supported |
| `\textit{}`, `\emph{}`, `{\it …}`, `{\itshape …}` | ✅ v0.1 | Recursive nesting supported |
| `\label{}`, `\ref{}`, `\cite{}` | ✅ v0.1 | Emitted as `[key]` placeholder text |
| `\autocite{}` | ✅ v0.5 | Style-aware placeholder: footnote or inline (`[key]`) based on LaTeX style config |
| Preamble (`\documentclass`, `\usepackage`, etc.) | ✅ v0.1 | Everything before `\begin{document}` is discarded; preamble-only layout commands are skipped |
| `\tableofcontents` | ✅ v0.9.1 | Emits `Block::TableOfContents`; renderer generates TOC paragraphs from following section blocks or `.toc` file entries. No language text stored in AST — heading text must come from a preceding `\chapter*{...}` in the document. |
| TOC depth (`\setcounter{tocdepth}{N}`) | ✅ v0.9.2 | Parsed into `DocumentLayout.toc_depth`; generated DOCX TOC includes only levels allowed by LaTeX counter depth. |
| `%` comments | ✅ v0.1 | Stripped before parsing |
| TOC leaders and right margin (`\setrmarg`, `\cft...leader`, `\cftdotfill`) | ✅ | Dot leaders and page tab stop are LaTeX-driven when defined |
| TOC chapter prefix (`\cftchaptername`) | ✅ | Parsed and rendered for numbered chapter entries (for example `Chapter 1.` / `ГЛАВА 1.`) |
| TOC indent/number width (`\cftsetindents{...}{...}{...}`, `\setlength{\cft...indent}{...}`, `\setlength{\cft...numwidth}{...}`) | ✅ | Parsed for chapter/section/subsection/subsubsection; rendered as level indent and hanging indent when `numwidth` is provided |
| Hyperlink style (`\hypersetup{linkcolor=...}`, `\hypersetup{allcolors=...}`, `\hypersetup{colorlinks=...}`, `\hypersetup{hidelinks}`) | ✅ v0.9.2 | Parsed into `hyperlink_text_color` + `hyperlink_underline`; applied to TOC links and inline internal reference hyperlinks. |
| Heading spacing (`\setlength{\beforechapskip}{...}`, `\setlength{\afterchapskip}{...}`, `\setbeforesecskip`, `\setaftersecskip`, `\setbeforesubsecskip`, `\setaftersubsecskip`, `\setbeforesubsubsecskip`, `\setaftersubsubsecskip`) | ✅ v0.9.2 | Parsed into heading spacing twips and applied to DOCX heading paragraph spacing before/after. |
| Heading number delimiters (`\setcounter{headingdelim}{...}`, `\setsecnumformat{...}`, `\renewcommand{\thechapter}{...}`) | ✅ v0.9.2 | Parsed into per-level delimiters for chapter/section/subsection/subsubsection; `headingdelim=1` now keeps section numbers as `1.1` (no trailing dot) while chapter keeps `1.`. |
| `\begin{tabular}…\end{tabular}`, `\begin{tblr}…\end{tblr}`, `\begin{longtblr}…\end{longtblr}` | ✅ v0.2 | Basic table layout |
| `\tablesource{…}`, `\figuresource{…}` | ✅ v0.9 | Rendered as left-aligned small italic source line; `\vspace{...}` before line is extracted from macro definition |
| `\begin{figure}…\end{figure}` + `\caption{}` | ✅ v0.2 | Caption text only (no image embed yet) |
| Caption label emphasis (`\captionsetup{labelfont=...}`) | ✅ v0.9.2 | Parsed per float type (`[figure]` / `[table]`) with global fallback; DOCX caption label bold is no longer hardcoded. |
| `\begin{itemize}`, `\begin{enumerate}` | ✅ v0.9 | List geometry is LaTeX-driven via `\setlist{labelsep=...,labelwidth=...}` and bullet marker via `\renewcommand{\labelitemi}{...}` |
| Body paragraph alignment (`\raggedright`, `\centering`, `\raggedleft` in body context) | ✅ v0.9.2 | Parsed into `body_text_alignment`; mapped to BodyText/ListParagraph defaults and paragraph rendering fallback alignment. |
| Page-number alignment (`\lfoot`, `\cfoot`, `\rfoot`, `\fancyfoot[...]`, `\makeoddfoot`, `\makeevenfoot`, `\makeoddhead`, `\makeevenhead`) | ✅ v0.9.2 | Parsed into `page_number_alignment`; used by generated DOCX page-number header paragraph. |
| Page gutter (`\geometry{bindingoffset=...}`) | ✅ v0.9.2 | Parsed into `page_gutter_twips`; mapped to DOCX section gutter with fallback `0`. |
| Explicit page breaks (`\newpage`, `\clearpage`, `\cleardoublepage`) | ✅ v0.9.3 | Parsed into `Block::PageBreak`; renderer emits DOCX page-break paragraph (`w:br w:type=\"page\"`). |
| Landscape switch markers (`\begin{landscape}`, `\end{landscape}`, `\landscape`, `\endlandscape`) | 🟨 v0.9.3 | Parsed as structural forced page-break markers (`Block::PageBreak`) to preserve flow boundaries. True DOCX orientation section switching is not implemented yet. |
| Math (`$…$`) | ✅ v0.4 | Rendered as italic plain-text approximation |
| Math (`\begin{equation}`, `\begin{equation*}`) | ✅ v0.4 | Rendered as centered italic plain-text approximation |
| Math (`\[…\]`) | ✅ v0.5 | Rendered as centered italic plain-text approximation |
| Footnotes (`\footnote{}`) | ✅ v0.5 | Native DOCX footnote references + footnotes.xml |
| Bibliography heading (`\printbibliography`, `\insertbibliofullsorted`, `\insertbiblioauthor`, `\insertbibliofull`) | ✅ v0.9.1 | Emits chapter-level heading; `title=...` respected; default title derived from `document_language` (e.g. `"REFERENCES"` for English, `"СПИСОК ЛИТЕРАТУРЫ"` for Russian); bibliography entries are not parsed yet |
| Bibliography entries (`\bibliography`, `\bibitem`, `.bib`) | ⬜ | Entry list rendering is not implemented yet |
| Title page first-page number suppression (`\thispagestyle{empty}` in `titlepage`/`titlingpage`) | ✅ v0.9 | Maps to DOCX different-first-page mode (`titlePg`) so page 1 number is hidden |
| Cross-references (`\ref`, resolved) | ⬜ v0.5 | Currently emitted as placeholder |
| File inclusion (`\input{}`, `\include{}`) | ✅ v0.5 | Recursive expansion from entry `.tex` file |
| LayoutProbe merge contract (`probe > parser > fallback`) | ✅ v0.9.5 foundation | `LayoutProbeOutput` + deterministic merge helper implemented in `ferritex-core`; `parse_latex_file` applies merge before renderer mapping. |
| LayoutProbe embedded backend (`layout-probe-tectonic`) | 🟨 v0.9.5 foundation | Feature-gated `tectonic` backend extracts first high-impact fields: page geometry, body font size/family, paragraph indent, line spacing, list geometry. |
| PDF output | ⬜ v1.0 | |

## TOC `cft*` coverage

Mapped into `DocumentLayout` and rendered:
- `\setrmarg{...}`
- `\renewcommand{\cft...leader}{...}` (dot vs non-dot leader detection)
- `\renewcommand*{\cftchaptername}{...}`
- `\cftsetindents{chapter|section|subsection|subsubsection}{indent}{numwidth}`
- `\setlength{\cftchapterindent}{...}`, `\setlength{\cftchapternumwidth}{...}`
- `\setlength{\cftsectionindent}{...}`, `\setlength{\cftsectionnumwidth}{...}`
- `\setlength{\cftsubsectionindent}{...}`, `\setlength{\cftsubsectionnumwidth}{...}`
- `\setlength{\cftsubsubsectionindent}{...}`, `\setlength{\cftsubsubsectionnumwidth}{...}`
- `\setlength{\cftbeforechapterskip}{...}`, `\setlength{\cftbeforesectionskip}{...}`, `\setlength{\cftbeforesubsectionskip}{...}`, `\setlength{\cftbeforesubsubsectionskip}{...}`

Newly mapped in v0.8:
- `\renewcommand{\cftchapterfont}{\normalfont}` → `toc_chapter_entry_bold` (bold/non-bold chapter title in TOC)
- `\renewcommand{\cftchapterpagefont}{\normalfont}` → `toc_chapter_page_bold` (bold/non-bold page number in TOC)
- `\renewcommand\cftchapteraftersnum{...}` → `toc_aftersnum_chapter` (separator after chapter number in TOC)
- `\renewcommand\cftsectionaftersnum{...}` → `toc_aftersnum_section`
- `\renewcommand\cftsubsectionaftersnum{...}` → `toc_aftersnum_subsection`
- `\renewcommand\cftsubsubsectionaftersnum{...}` → `toc_aftersnum_subsubsection`
- `\setcounter{headingdelim}{...}` → conditional override for `toc_aftersnum_*` (avoids taking inactive `\ifnumgreater` branches from style sources)
- `\renewcommand{\cftappendixname}{...}` → `toc_appendix_name` (extracted; appendix TOC rendering reserved for future)

Not mapped yet (fallback behavior applies):
- `\setpnumwidth{...}` (page number width box)
- `\settoctitlefont{...}` and other title-level TOC font macros

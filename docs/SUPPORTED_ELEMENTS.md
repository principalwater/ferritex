# Supported LaTeX Elements

Rendering policy:
- Prefer LaTeX-originated semantics over backend-local defaults.
- DOCX/PDF backends should produce equivalent structure from the same AST.
- Effective LaTeX build parameters are the source of truth for layout and style decisions; backend constants are fallback-only.
- New visual fixes must be implemented as generic parameter mapping, not hardcoded values for a specific project.

| Element | Status | Notes |
|---|---|---|
| `\chapter{}`, `\section{}`, `\subsection{}`, `\subsubsection{}` | ✅ v0.1 | Including `*` (unnumbered) variants; chapter→H1, section→H2, subsection/subsubsection→H3 |
| Paragraph text | ✅ v0.1 | Blank-line separated, whitespace normalized |
| `\textbf{}`, `{\bf …}`, `{\bfseries …}` | ✅ v0.1 | Recursive nesting supported |
| `\textit{}`, `\emph{}`, `{\it …}`, `{\itshape …}` | ✅ v0.1 | Recursive nesting supported |
| `\label{}`, `\ref{}`, `\cite{}` | ✅ v0.1 | Emitted as `[key]` placeholder text |
| `\autocite{}` | ✅ v0.5 | Style-aware placeholder: footnote or inline (`[key]`) based on LaTeX style config |
| Preamble (`\documentclass`, `\usepackage`, etc.) | ✅ v0.1 | Everything before `\begin{document}` is discarded; layout commands (`\vspace`, `\newpage`, `\tableofcontents`, etc.) skipped |
| `%` comments | ✅ v0.1 | Stripped before parsing |
| `\begin{tabular}…\end{tabular}`, `\begin{tblr}…\end{tblr}`, `\begin{longtblr}…\end{longtblr}` | ✅ v0.2 | Basic table layout |
| `\tablesource{…}`, `\figuresource{…}` | ✅ v0.2 | Rendered as source line below table/figure |
| `\begin{figure}…\end{figure}` + `\caption{}` | ✅ v0.2 | Caption text only (no image embed yet) |
| `\begin{itemize}`, `\begin{enumerate}` | ✅ v0.3 | Bullet and numbered lists |
| Math (`$…$`) | ✅ v0.4 | Rendered as italic plain-text approximation |
| Math (`\begin{equation}`, `\begin{equation*}`) | ✅ v0.4 | Rendered as centered italic plain-text approximation |
| Math (`\[…\]`) | ✅ v0.5 | Rendered as centered italic plain-text approximation |
| Footnotes (`\footnote{}`) | ✅ v0.5 | Native DOCX footnote references + footnotes.xml |
| Bibliography (`\bibliography`, `\bibitem`) | ⬜ v0.5 | |
| Cross-references (`\ref`, resolved) | ⬜ v0.5 | Currently emitted as placeholder |
| File inclusion (`\input{}`, `\include{}`) | ✅ v0.5 | Recursive expansion from entry `.tex` file |
| PDF output | ⬜ v1.0 | |

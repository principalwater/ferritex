# Supported LaTeX Elements

| Element | Status | Notes |
|---|---|---|
| `\section{}`, `\subsection{}`, `\subsubsection{}` | ✅ v0.1 | Including `*` (unnumbered) variants |
| Paragraph text | ✅ v0.1 | Blank-line separated, whitespace normalized |
| `\textbf{}`, `{\bf …}`, `{\bfseries …}` | ✅ v0.1 | Recursive nesting supported |
| `\textit{}`, `\emph{}`, `{\it …}`, `{\itshape …}` | ✅ v0.1 | Recursive nesting supported |
| `\label{}`, `\ref{}`, `\cite{}`, `\autocite{}` | ✅ v0.1 | Emitted as `[key]` placeholder text |
| `\documentclass`, `\usepackage`, preamble | ✅ v0.1 | Silently stripped |
| `%` comments | ✅ v0.1 | Stripped before parsing |
| `\begin{tabular}…\end{tabular}`, `\begin{tblr}…\end{tblr}`, `\begin{longtblr}…\end{longtblr}` | ✅ v0.2 | Basic table layout |
| `\tablesource{…}`, `\figuresource{…}` | ✅ v0.2 | Rendered as source line below table/figure |
| `\begin{figure}…\end{figure}` + `\caption{}` | ✅ v0.2 | Caption text only (no image embed yet) |
| `\begin{itemize}`, `\begin{enumerate}` | ✅ v0.3 | Bullet and numbered lists |
| Math (`$…$`) | ✅ v0.4 | Rendered as italic plain-text approximation |
| Math (`\begin{equation}`, `\begin{equation*}`) | ✅ v0.4 | Rendered as centered italic plain-text approximation |
| Math (`\[…\]`) | ✅ v0.5 | Rendered as centered italic plain-text approximation |
| Footnotes (`\footnote{}`) | ⬜ v0.5 | DOCX footnote elements |
| Bibliography (`\bibliography`, `\bibitem`) | ⬜ v0.5 | |
| Cross-references (`\ref`, resolved) | ⬜ v0.5 | Currently emitted as placeholder |
| PDF output | ⬜ v1.0 | |

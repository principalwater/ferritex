# Roadmap

## Engineering workflow ✅
- Required CI quality gate: fmt + clippy + test
- Optional bot-assisted merge path with `bot-merge` label (squash + delete branch after repository gates)

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

## v0.5
- Footnotes: `\footnote{}` ✅
- Resolved cross-references (`\ref`, `\cite`) instead of placeholders
- Display math `\[…\]` ✅
- `\input{}` file inclusion ✅
- `\include{}` file inclusion ✅

## v1.0
- PDF output via `printpdf` or similar
- Image embedding in DOCX

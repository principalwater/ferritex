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

## v0.5
- Footnotes: `\footnote{}` ✅
- Resolved cross-references (`\ref`, `\cite`) instead of placeholders
- Display math `\[…\]` ✅
- `\input{}` file inclusion ✅
- `\include{}` file inclusion ✅

## v0.6 ✅
- TUI mode on `ratatui` for interactive conversion
- Explicit CLI subcommands (`convert`, `tui`) with compatibility for `--input/--output`

## v0.7 (in progress)
- Unified build-core orchestrator (`src/build/`)
- `ferritex build --format docx|pdf|both` CLI entrypoint
- `convert` and `tui` routed through shared build core
- Path resolution and artifact naming centralized

## v1.0
- PDF output via `printpdf` or similar
- Image embedding in DOCX

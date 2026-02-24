# FerriTeX

**FerriTeX** is a native Rust CLI/TUI tool that converts LaTeX (`.tex`) projects
into DOCX (`.docx`) and other output formats — without invoking LaTeX or any external
toolchain. *Ferri* for Rust's iron-clad reliability, *TeX* for the source format.

---

## How it works

FerriTeX parses your LaTeX source, extracts layout and style parameters from the
preamble (geometry, fonts, heading formatting, list settings, spacing, captions, and
more), and feeds them into a backend renderer. The rendering contract is strictly
**LaTeX-first**: every visual decision is derived from what the LaTeX project itself
declares. Backend constants are used only as fallback defaults when the source does
not express a preference.

```
LaTeX source → parser → AST + DocumentLayout → renderer → DOCX / PDF / Markdown
```

All output formats consume the same intermediate representation, so adding a new
backend does not require touching the parser.

---

## Status

Current release: **v0.9.1**

### Implemented

| Feature | Notes |
|---|---|
| Sections / subsections / paragraphs | `\chapter`, `\section`, `\subsection`, `\subsubsection`; `*` variants |
| Inline styles | Bold, italic, nested combinations |
| File inclusion | `\input{}`, `\include{}` — recursive expansion |
| Tables | `tabular`, `tblr`, `longtblr` |
| Figures | Caption text; source attribution via `\figuresource{...}` |
| Lists | `itemize`, `enumerate`; geometry from `\setlist{...}` |
| Math | Inline `$...$`, display `equation` / `equation*` / `\[...\]` (plain-text approximation) |
| Footnotes | Native DOCX footnote references (`\footnote{}`) |
| Citations | `\autocite{}` — footnote or inline placeholder depending on project style |
| Table of contents | `\tableofcontents` — generates TOC from document section structure |
| Bibliography heading | `\printbibliography`, `\insertbibliofullsorted`, etc.; title from `title=` or derived from document language |
| Title page | `titlepage` / `titlingpage`; page number suppression via `\thispagestyle{empty}` |
| Layout extraction | Page margins, paper size, line spacing, paragraph indent, font family and size, heading format, list geometry, caption labels, TOC indent/numwidth/leader settings, source-line spacing |
| Cross-references | `\label{}`, `\ref{}`, `\cite{}` — currently as `[key]` placeholders |
| TUI mode | Interactive conversion via `ratatui` |

### Not yet implemented

- OMML / Word equation rendering (currently plain-text approximation)
- Image embedding in DOCX
- Bibliography entry list rendering (heading is placed, entries are pending)
- PDF output backend
- Resolved `\ref` / `\cite` hyperlinks in body text

---

## Conversion contract

FerriTeX follows a strict **LaTeX-driven rendering policy**:

- Layout, numbering, spacing, and typography are extracted from the LaTeX source and
  propagate through `DocumentLayout` into the renderer.
- Renderer constants serve only as documented fallback defaults.
- Formatting bugs are fixed by improving parser extraction, never by hardcoding
  project-specific values in the renderer.
- Validated on one project counts as QA coverage; implementation must remain reusable
  for arbitrary LaTeX projects (articles, theses, books, journals).

---

## Installation

```bash
cargo install --path .
```

## Usage

```bash
# Build to DOCX
ferritex build --input main.tex --format docx

# Build to multiple formats
ferritex build --input main.tex --format all --output-dir out/

# Legacy shorthand (compatible with older versions)
ferritex --input main.tex --output main.docx
ferritex convert --input main.tex --output main.docx

# Interactive TUI
ferritex tui
ferritex tui --input main.tex --output main.docx
```

### TUI keys

| Key | Action |
|-----|--------|
| `Tab` / `Up` / `Down` | Switch field |
| `Enter` | Run conversion |
| `Ctrl+U` | Clear focused field |
| `q` / `Esc` | Quit |

---

## Documentation

- [Supported elements](docs/SUPPORTED_ELEMENTS.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Roadmap](docs/ROADMAP.md)
- [Agent / contributor instructions](AGENTS.md)

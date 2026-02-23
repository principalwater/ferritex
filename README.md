# ferritex

`ferritex` is a production-oriented native Rust TUI/CLI utility for building LaTeX (`.tex`) projects into DOCX (`.docx`) and PDF output formats.

## Status

`ferritex` v0.7 is in progress — unified build core with format selection.

Implemented so far:
- Sections/subsections, paragraphs, inline styles.
- Placeholder references (`\label`, `\ref`, `\cite`) as `[key]`.
- `\autocite{...}` style-aware placeholder:
  - footnote placeholder when project style sets footnote autocites
  - inline placeholder otherwise
- Tables (`tabular`, `tblr`, `longtblr`) and source lines.
- Figures (caption text + source lines).
- Lists (`itemize`, `enumerate`).
- Math: inline `$...$`, display `equation` / `equation*` / `\[...\]` (plain-text approximation).
- Footnotes: `\footnote{...}` rendered as native DOCX footnotes.
- File inclusion: `\input{...}` and `\include{...}` are recursively expanded.

## Installation

```bash
cargo install --path .
```

## Usage

```bash
# Unified build (recommended)
ferritex build --input main.tex --format docx
ferritex build --input main.tex --format both --output-dir out/

# Legacy non-interactive (compatible with previous versions)
ferritex --input main.tex --output main.docx
ferritex convert --input main.tex --output main.docx

# Interactive TUI mode
ferritex tui
ferritex tui --input main.tex --output main.docx
```

### TUI Keys

- `Tab` / `Up` / `Down`: switch input/output field
- `Enter`: run conversion
- `Ctrl+U`: clear focused field
- `q` / `Esc`: quit

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Supported elements](docs/SUPPORTED_ELEMENTS.md)
- [Roadmap](docs/ROADMAP.md)

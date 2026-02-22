# ferritex

`ferritex` is a production-oriented native Rust CLI for converting LaTeX (`.tex`) files to Microsoft Word DOCX (`.docx`).

## Status

`ferritex` v0.5 is in progress on top of `master`.

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
ferritex --input thesis.tex --output thesis.docx
ferritex --input thesis.tex --output thesis.docx --verbose
```

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Supported elements](docs/SUPPORTED_ELEMENTS.md)
- [Roadmap](docs/ROADMAP.md)

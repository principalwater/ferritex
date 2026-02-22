# ferritex

`ferritex` is a production-oriented native Rust CLI for converting LaTeX (`.tex`) files to Microsoft Word DOCX (`.docx`).

## Status

`ferritex` v0.4 is merged to `master`.

Implemented so far:
- Sections/subsections, paragraphs, inline styles.
- Placeholder references (`\label`, `\ref`, `\cite`, `\autocite`) as `[key]`.
- Tables (`tabular`, `tblr`, `longtblr`) and source lines.
- Figures (caption text + source lines).
- Lists (`itemize`, `enumerate`).
- Math: inline `$...$`, display `equation` / `equation*` / `\[...\]` (plain-text approximation).

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

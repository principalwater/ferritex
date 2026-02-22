# ferritex

`ferritex` is a production-oriented native Rust CLI for converting LaTeX (`.tex`) files to Microsoft Word DOCX (`.docx`).

## Status

Scaffold phase: CLI and module architecture are in place, conversion logic is not implemented yet.

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

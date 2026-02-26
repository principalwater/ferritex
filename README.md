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
LaTeX source
  → LayoutProbe + parser extraction
  → merge (parser > probe > fallback)
  → AST + DocumentLayout
  → renderer
  → DOCX / PDF / Markdown
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

## Reproducible environment

- Rust toolchain is pinned in `rust-toolchain.toml` (`1.93.1`).
- CI uses the same pinned Rust version and runs with `--locked`.
- Dependency versions are locked by `Cargo.lock`; update them intentionally via
  `cargo update` only when planned.

Recommended local workflow:

```bash
rustup show active-toolchain
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

### `layout-probe-tectonic` native prerequisites

The embedded `tectonic` probe requires native libraries.

`ferritex` enables `layout-probe-tectonic` by default in the root CLI crate,
so standard `cargo run -- ...` / `cargo test` paths use probe-enabled builds
unless explicitly disabled with `--no-default-features`.

macOS (Homebrew):

```bash
brew install pkg-config icu4c harfbuzz graphite2
```

Ubuntu/Debian:

```bash
sudo apt-get update
sudo apt-get install -y pkg-config libicu-dev libharfbuzz-dev libgraphite2-dev
```

Feature check:

```bash
cargo check -p ferritex-core --features layout-probe-tectonic --locked

# Optional parser-only build (debugging fallback path)
cargo check --workspace --no-default-features --locked
```

## Usage

```bash
# Build to DOCX
ferritex build --input main.tex --format docx

# Build to multiple formats
ferritex build --input main.tex --format all --output-dir out/

# PDF build with explicit compatible biber binary location
ferritex build --input main.tex --format pdf --pdf-biber-bin-dir /opt/biber/bin

# Non-interactive CI-style run: allow automatic tool bootstrap
ferritex build --input main.tex --format pdf --tool-install-policy auto

# Legacy shorthand (compatible with older versions)
ferritex --input main.tex --output main.docx
ferritex convert --input main.tex --output main.docx

# Interactive TUI
ferritex tui
ferritex tui --input main.tex --output main.docx
```

### PDF bibliography compatibility (`biblatex`/`biber`)

PDF build runs through tectonic runtime and fails fast when bibliography tooling
is incompatible (for example, BCF version mismatch).

By default, ferritex uses:

- `--pdf-biber-mode auto` (retry alternative `biber` candidates),
- `--tool-install-policy ask` (prompt before downloading/installing compatible tooling).

If the first `biber` candidate from `PATH` is incompatible, ferritex retries
alternative local candidates available in `PATH`.

If no compatible local candidate is found, auto mode can bootstrap a compatible
`biber` binary into the ferritex cache (supported platforms only), depending on
`--tool-install-policy`:

- `ask` (default): prompt user for confirmation in interactive terminal sessions;
- `auto`: install automatically (good for CI/non-interactive runs);
- `never`: fail with explicit manual-install guidance.

To force-disable bootstrap regardless of CLI policy:

```bash
export FERRITEX_PDF_BIBER_AUTO_INSTALL=0
```

If your system has multiple `biber` installations, select a compatible one:

```bash
# CLI override (preferred for one-off runs)
ferritex build --input main.tex --format pdf --pdf-biber-bin-dir /path/to/biber/bin --pdf-biber-mode strict

# Always auto-install compatible tools when needed
ferritex build --input main.tex --format pdf --tool-install-policy auto

# Never auto-install tools (strictly manual environment)
ferritex build --input main.tex --format pdf --tool-install-policy never

# Environment override (for repeated runs)
export FERRITEX_PDF_BIBER_BIN_DIR=/path/to/biber/bin
ferritex build --input main.tex --format pdf

# Provide extra auto-mode candidates (PATH-like list)
export FERRITEX_PDF_BIBER_BIN_DIRS="/opt/biber-2.19/bin:/opt/biber-2.21/bin"
ferritex build --input main.tex --format pdf --pdf-biber-mode auto

# Optional cache override for auto-installed binaries
export FERRITEX_PDF_BIBER_CACHE_DIR=/tmp/ferritex-biber-cache
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

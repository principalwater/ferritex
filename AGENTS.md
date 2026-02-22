# AGENTS.md — Contributor & AI Agent Instructions (ferritex)

## Non-negotiable constraints
- This project is 100% Rust. No Python, no shell scripts, no Pandoc
  subprocess calls, no FFI to non-Rust libraries unless absolutely
  unavoidable (document why in this file if so).
- All crate versions must be stable releases from crates.io. Do not
  use git dependencies or pre-release versions.
- CLI is the primary interface. Use clap with derive macros.
- All public types must have doc comments.
- No unwrap() in library code — use anyhow::Result or thiserror types.

## Architecture rules
- The pipeline is strictly: LaTeX source → AST (model/) → DOCX output
- The parser and renderer are completely decoupled via the AST.
  Never let docx-rs types leak into the parser, never let nom/regex
  types leak into the renderer.
- New LaTeX elements go in: src/parser/latex.rs + src/model/mod.rs
  + src/renderer/docx.rs — always all three.

## DOCX structure notes
- DOCX is a ZIP containing XML files (word/document.xml, etc.)
- Paragraphs are <w:p>, runs are <w:r>, properties are <w:pPr>/<w:rPr>
- Alignment: <w:jc w:val="both"/> for justify
- Section breaks that must NOT be inserted mid-document: <w:sectPr>
  inside <w:pPr> — these break text flow. Only one <w:sectPr> is
  allowed as the last child of <w:body>.
- Use docx-rs as the primary builder; fall back to quick-xml + zip
  only for features docx-rs doesn't expose.

## Supported LaTeX elements (expand as implemented)
Track implementation status in docs/SUPPORTED_ELEMENTS.md.

## Before every commit
- cargo fmt --all
- cargo clippy -- -D warnings
- cargo test

## When adding a new crate
Update this file with: crate name, version pinned, reason for adding.

## Current crates in use
(agent must update this table when Cargo.toml changes)
| Crate | Version | Purpose |
|-------|---------|---------|
| clap | latest stable | CLI argument parsing |
| anyhow | latest stable | Error propagation |
| thiserror | latest stable | Error type definitions |
| docx-rs | latest stable | DOCX generation |
| quick-xml | latest stable | Low-level XML manipulation |
| serde/serde_json | latest stable | Serialization |
| zip | latest stable | ZIP/DOCX container |
| log + env_logger | latest stable | Logging |

## Project extensions

### Pinned crate versions
| Crate | Version | Purpose |
|---|---|---|
| clap | 4.5.60 | CLI argument parsing |
| anyhow | 1.0.102 | Error propagation |
| thiserror | 2.0.18 | Error type definitions |
| docx-rs | 0.4.19 | DOCX generation |
| quick-xml | 0.39.2 | Low-level XML manipulation |
| serde | 1.0.228 | Serialization traits |
| serde_json | 1.0.149 | JSON support |
| walkdir | 2.5.0 | Recursive filesystem traversal |
| log | 0.4.29 | Logging facade |
| env_logger | 0.11.9 | Env-driven logging implementation |
| zip | 8.1.0 | ZIP/DOCX container handling |

### Contributor checklist
- Keep `src/main.rs` as orchestration only.
- Put new parsing logic in `src/parser/latex.rs` and output-neutral AST changes in `src/model/mod.rs`.
- Put DOCX emission logic in `src/renderer/docx.rs`.

---

## Privacy & Open-Source Safety Rules

ferritex is a general-purpose open-source tool. It must contain
ZERO references to any specific person, institution, research domain,
dissertation, or private project.

### What must never appear in this repository
- Real names, initials, or usernames of contributors or users
- University or organization names
- Dissertation or thesis titles, chapter names, or subject matter
- Absolute local file paths (use relative paths or CLI args)
- Private GitHub repository names or URLs
- API tokens, SSH keys, credentials of any kind
- Email addresses (unless explicitly public and consented)

### Test fixtures policy
- Test .tex fixtures must be synthetic and generic
  (e.g. "Sample Document", "Introduction", "Lorem ipsum...")
- Corner cases from real usage MAY be extracted as fixtures,
  but only after full anonymization — no real content, no real structure
  that could identify the source document
- Good fixture naming: simple.tex, with_footnotes.tex, with_table.tex
- Bad fixture naming: dissertation_chapter1.tex, author_thesis.tex

### When in doubt
If you are unsure whether a piece of content is sensitive,
remove it or replace it with a generic placeholder.

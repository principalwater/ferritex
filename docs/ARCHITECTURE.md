# Architecture

`ferritex` follows a strict three-stage pipeline:

1. Parse: LaTeX source is converted into an internal AST.
2. Model: AST in `src/model` is the only shared contract between stages.
3. Render: AST is rendered to DOCX output.

## Design goals

- Deterministic, reproducible conversion.
- Parser and renderer are decoupled by AST types.
- No subprocess wrappers around external converters.

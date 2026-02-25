# Tectonic Integration Notes

## Goal

Use `tectonic` as the primary embedded TeX engine for:

1. high-fidelity layout/style probing for backend mapping (`docx`, future `md`),
2. PDF generation path for visual parity workflows (future `pdf` backend),
3. deterministic, reproducible execution without external LaTeX subprocess orchestration.

## APIs and Intended Roles

| API | Role in ferritex | Current state |
|---|---|---|
| `tectonic::driver::ProcessingSessionBuilder` | Probe session for effective style values with in-memory input and `.log` marker parsing | Implemented |
| `tectonic::TexEngine` | Low-level TeX pass control for advanced/custom probing scenarios | Not integrated yet |
| `tectonic::latex_to_pdf` | Direct PDF generation from LaTeX source bytes for parity-oriented PDF backend workflows | Planned |

## Probe Reliability Model

Probe output is split into two execution modes:

1. Clean mode:
   - no TeX error traces,
   - probe fields are consumed normally (probe > parser > fallback).
2. Degraded mode:
   - TeX run error and/or TeX error markers in `.log`,
   - field-level confidence model is applied:
     - `font_size_body_hp` is downgraded when font-metric risk is detected,
     - `body_line_spacing_twips` is downgraded when spacing/dimension risk is detected,
     - both are downgraded on general/fatal TeX failures or unknown-risk degraded logs,
   - parser extraction is used for these fields.

Rationale: raw TeX font/baseline metrics from degraded runs can produce incorrect DOCX line spacing despite successful extraction of geometric/list signals.

## Backend Strategy

### DOCX

- Keep current contract: `LayoutProbe + parser -> DocumentLayout -> RenderProfile`.
- Continue parser-first normalization for fields requiring DOCX-specific calibration (for example, line spacing mapping).

### PDF (target)

- Introduce a dedicated path that can call `tectonic::latex_to_pdf` for canonical PDF output when parity is the priority.
- Keep AST/StyleMap path for cross-backend feature consistency and metadata-aware workflows.

### Markdown (target)

- Reuse parser + probe semantic normalization; do not map raw TeX engine metrics directly to Markdown formatting decisions.

## Engineering Constraints

- No external TeX subprocess orchestration in runtime path.
- Preserve deterministic behavior and explicit degraded-mode logging.
- Keep fallback values as last resort only.

## Source References

- `https://docs.rs/tectonic/latest/tectonic/`
- `https://docs.rs/tectonic/latest/tectonic/struct.TexEngine.html`
- `https://docs.rs/tectonic/latest/tectonic/fn.latex_to_pdf.html`
- `https://docs.rs/tectonic/latest/tectonic/driver/struct.ProcessingSessionBuilder.html`
- `https://github.com/tectonic-typesetting/tectonic/tree/master`

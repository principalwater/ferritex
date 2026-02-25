# Tectonic Integration Notes

## Goal

Use `tectonic` as the primary embedded TeX engine for:

1. high-fidelity layout/style probing for backend mapping (`docx`, future `md`),
2. PDF generation path for visual parity workflows (canonical `pdf` backend baseline),
3. deterministic, reproducible execution without external LaTeX subprocess orchestration.

## APIs and Intended Roles

| API | Role in ferritex | Current state |
|---|---|---|
| `tectonic::driver::ProcessingSessionBuilder` | Probe session for effective style values with in-memory input and `.log` marker parsing | Implemented |
| `tectonic::TexEngine` | Runtime profile anchor for probe execution settings and low-level pass orchestration (`halt_on_error`, `shell_escape`, `build_date`, pass strategy) | Implemented with direct pass control |
| `tectonic::latex_to_pdf` | Direct PDF generation from LaTeX source bytes for parity-oriented PDF backend workflows | Implemented in `ferritex-renderer-pdf` |

## Probe Pass Orchestration

Probe execution now uses explicit low-level pass plans under the `TexEngine` runtime profile:

1. Primary pass:
   - `PassSetting::Tex` (single TeX pass),
   - used for fast marker extraction from probe macros.
2. Recovery pass (conditional):
   - `PassSetting::Default` with `reruns=1`,
   - executed only when primary pass returns run errors or produces zero extracted fields.
3. Selection policy:
   - prefer recovery output when it clears run-error status or yields richer probe signal,
   - otherwise keep primary output and continue with confidence filtering.

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

### PDF

- Dedicated canonical path now calls `tectonic::latex_to_pdf` in `ferritex-renderer-pdf`.
- Runtime compiles the input `.tex` source in the input-context directory so relative includes/assets resolve as in normal LaTeX project layout.
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

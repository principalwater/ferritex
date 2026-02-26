# Tectonic Integration Notes

## Goal

Use `tectonic` as the primary embedded TeX engine for:

1. high-fidelity layout/style probing for backend mapping (`docx`, future `md`),
2. PDF generation path for visual parity workflows (canonical `pdf` backend baseline),
3. deterministic, reproducible execution without external LaTeX subprocess orchestration.

## APIs and Intended Roles

| API | Role in ferritex | Current state |
|---|---|---|
| `tectonic::driver::ProcessingSessionBuilder` | Probe session and canonical PDF runtime session with explicit artifact/error handling | Implemented |
| `tectonic::TexEngine` | Runtime profile anchor for probe execution settings and low-level pass orchestration (`halt_on_error`, `shell_escape`, `build_date`, pass strategy) | Implemented with direct pass control |
| `tectonic::latex_to_pdf` | Reference one-call API for parity workflows; kept as compatibility baseline, not the active renderer path | Available |

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
   - probe fields are consumed normally (`parser > probe > fallback` contract still applies).
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

- Dedicated canonical path uses tectonic driver APIs in `ferritex-renderer-pdf` with explicit input-context root and artifact checks.
- Keep AST/StyleMap path for cross-backend feature consistency and metadata-aware workflows.
- Current known gap:
  - some `biblatex` corpora can fail on biber/BCF compatibility mismatch (for example `BCF 3.8` vs `biber expects 3.11`);
  - renderer now surfaces this cause explicitly in runtime errors and fails fast with guidance.
  - bibliography tool resolution mode is user-configurable:
    - CLI: `--pdf-biber-mode auto|strict` (default `auto`),
    - `auto` retries alternative `biber` candidates on compatibility mismatch,
    - `strict` fails immediately on the first selected candidate.
  - external tool bootstrap policy is user-configurable at build-core level:
    - CLI: `--tool-install-policy ask|auto|never` (default `ask`),
    - `ask`: interactive confirmation before installing compatible tools,
    - `auto`: install compatible tools automatically when possible,
    - `never`: fail-fast with manual-install + rerun guidance.
  - in `auto` mode, when no compatible local candidate exists, ferritex can
    bootstrap a compatible `biber` binary into its cache (supported platforms)
    if tool-install policy permits it.
  - users can select a local compatible biber installation for PDF runs via:
    - CLI: `--pdf-biber-bin-dir <DIR>`
    - env: `FERRITEX_PDF_BIBER_BIN_DIR=<DIR>`
  - extra auto-mode candidates can be supplied via:
    - env: `FERRITEX_PDF_BIBER_BIN_DIRS=<PATH-like list>`
  - auto-install controls:
    - disable: `FERRITEX_PDF_BIBER_AUTO_INSTALL=0`
    - cache root override: `FERRITEX_PDF_BIBER_CACHE_DIR=<DIR>`

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

# Review: Tectonic-first Audit (2026-02-25)

## Scope

- Probe-enabled DOCX build behavior on a large multi-file LaTeX corpus.
- Tectonic API usage strategy for `docx` / `pdf` / `md` backend goals.
- Repository hygiene around active plans and perceived "unused" root layout.

## Findings (ordered by severity)

1. **High: probe typography regression in degraded TeX runs**
   - Symptom: DOCX body spacing became `w:line="485"` (`2.02`) across document.
   - Root cause: probe captured raw TeX baseline/font metrics while TeX errors were present.
   - Fix implemented:
     - detect degraded probe runs via TeX error traces in probe `.log` (and run errors),
     - apply safety filter that drops probe `font_size_body_hp` and `body_line_spacing_twips`,
     - keep geometry/list probe signals intact.
   - Result: generated DOCX returned to calibrated parser typography (`w:line="332"` in styles for body text).

2. **Medium: active plan directory contained completed plans**
   - Action: moved completed plan files from `agent_docs/plans/` to `agent_docs/archive/plans/`.
   - Updated memory docs to reflect active-vs-archived plan flow.

3. **Info: root `src/` and root `tests/` are not dead code**
   - `src/` remains the orchestrator/facade layer for CLI/build pipeline.
   - `tests/` remains the workspace integration entrypoint (DOCX integration + stub-path checks for PDF/MD).

## Tectonic Integration Conclusions

1. Use `ProcessingSessionBuilder` for structured probe runs and marker extraction.
2. Keep parser-side normalization for typography-sensitive DOCX mapping.
3. Plan direct PDF-path evaluation via `tectonic::latex_to_pdf` in future PDF backend milestones.
4. Reserve low-level `TexEngine` integration for advanced probing cases that require explicit pass control.

## Follow-up Tasks

1. Extend probe confidence model from binary degraded/clean to field-level confidence where measurable.
2. Add backend-level regression fixtures asserting no global spacing inflation under degraded probe logs.
3. Continue orientation parity workstream on a separate coherent PR slice.

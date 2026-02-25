# Agent Docs

This directory contains the operational rules for AI/code agents working on ferritex.

## Files

- `coding_conventions.md` — source-level coding rules and quality expectations
- `latex_driven_policy.md` — core product policy for formatting extraction and rendering
- `git_workflow.md` — branch/PR/quality-gate workflow
- `session_summary.md` — compact current-state summary and next engineering steps
- `memory_policy.md` — repository-local memory policy for all AI clients
- `future_session_prompt.md` — copy-ready bootstrap prompt for new sessions
- `future_compaction_prompt.md` — copy-ready compaction/handoff prompt for session wrap-up
- `reviews/` — focused audit/review write-ups with dated findings

## Plans

- `plans/v0.9.1-docx-manual-qa-followup.md` — archival baseline for parity workstreams
- `plans/v0.9.3-docx-parity-disstyles-wave2.md` — partially completed (manual QA parity wave; orientation/page-break stream closed, remaining defects tracked)
- `plans/v0.9.4-test-organization-and-property-based.md` — in progress (property-based invariants started; full test-architecture audit pending)
- `plans/v0.9.5-layoutprobe-tectonic-foundation.md` — partially completed (`parser > probe > fallback` stabilized; `latex_to_pdf` path + first `TexEngine` runtime slice landed; remaining probe/TotPages hardening tracked)
- `plans/v1.0-multi-backend-foundation.md` — in progress (canonical PDF path via `latex_to_pdf` implemented baseline; DOCX/MD semantic parity path continues)

## Plan Archive

- `archive/plans/v0.9-visual-parity.md` — completed
- `archive/plans/v0.9.1-latex-driven-audit.md` — completed
- `archive/plans/v0.9.2-remaining-hardcodes.md` — completed (historical context retained)

## Usage

Use these documents together with `CLAUDE.md` and `AGENTS.md` in the repository root.

Notes:
- Local compaction scratch notes can be stored in `agent-docs/compaction/`.
- That path is gitignored and intentionally excluded from repository history.

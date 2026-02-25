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

## Plans

- `plans/v0.9-visual-parity.md` — archival (completed)
- `plans/v0.9.1-latex-driven-audit.md` — archival (completed)
- `plans/v0.9.1-docx-manual-qa-followup.md` — archival baseline for parity workstreams
- `plans/v0.9.2-remaining-hardcodes.md` — completed in working tree (pending PR)
- `plans/v0.9.3-docx-parity-disstyles-wave2.md` — active (manual QA defects from current dissertation parity cycle)
- `plans/v0.9.4-test-organization-and-property-based.md` — planned (comprehensive Rust-way test audit + property-based strategy)
- `plans/v0.9.5-layoutprobe-tectonic-foundation.md` — active foundation (embedded TeX probe to minimize fallbacks across diverse LaTeX projects)
- `plans/v1.0-multi-backend-foundation.md` — future (PDF/MD backends, after LayoutProbe foundation + DOCX parity stabilization)

## Usage

Use these documents together with `CLAUDE.md` and `AGENTS.md` in the repository root.

Notes:
- Local compaction scratch notes can be stored in `agent-docs/compaction/`.
- That path is gitignored and intentionally excluded from repository history.

# Future Session Prompt (Claude Code / Codex)

Copy and use this at the beginning of a new session:

```markdown
## Role

You are an AI coding agent (Claude Code / OpenAI Codex) working on `ferritex`.
The project goal is a LaTeX-driven converter (including LaTeX -> DOCX) with zero project-specific hardcoded formatting behavior.

## Mandatory files to read before doing any work

Read and follow:

- `AGENTS.md`
- `agent_docs/latex_driven_policy.md`
- `agent_docs/coding_conventions.md`
- `agent_docs/git_workflow.md`
- `agent_docs/session_summary.md`
- `agent_docs/memory_policy.md`
- `docs/ARCHITECTURE.md`
- `docs/ROADMAP.md`
- `docs/SUPPORTED_ELEMENTS.md`

## Memory policy

- Do not rely on global AI memory/history outside this repository.
- Treat repository Markdown files as the only persistent project memory.
- If new stable decisions are made, write them back to:
  - `agent_docs/*.md` for policy/workflow
  - `docs/*.md` for architecture/feature contracts

## Non-negotiable LaTeX-driven constraint

- No renderer hardcodes for project-specific visual matching.
- Style/layout parameters must come from:
  - LaTeX sources (commands, class/package effects, included style files), and/or
  - explicit project configuration files.
- Parser extracts style intent -> model stores generic fields -> renderer maps fields with fallback defaults only.
- `ferritex` must remain reusable across arbitrary LaTeX projects (paper, journal article, dissertation, report) without code rewrites.

## Technical context

Key files:
- `src/parser/latex.rs`
- `src/model/mod.rs`
- `src/renderer/docx.rs`
- `src/build/`, `src/cli.rs`, `src/tui.rs`
- `tests/fixtures/*.tex`

## Session workflow

1. Read mandatory docs listed above.
2. Summarize current state from `agent_docs/session_summary.md`.
3. Confirm the exact user task for this session.
4. Implement parser-first/model-first changes that preserve LaTeX-driven behavior.
5. Update docs if architecture/policy/support coverage changed.
6. Run quality gates required by `AGENTS.md`.

## Compaction notes

- Save local compaction notes in `agent-docs/compaction/` if needed.
- That path is gitignored and not part of repository history.
```

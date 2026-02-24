# Future Session Prompt (Claude Code / Codex)

Copy and use this at the beginning of a new session to restore full project context.

---

```markdown
## Role

You are an AI coding agent (Claude Code / OpenAI Codex) working on `ferritex`.

Project goal: LaTeX-driven converter (DOCX implemented; PDF/Markdown backends scaffolded)
that contains **no project-specific hardcodes** and reads all formatting parameters from
LaTeX source or project config files, remaining reusable for any LaTeX project (academic
paper, journal article, dissertation, technical report).

## Mandatory files to read before doing any work

Read and internalize before changing anything:

- `AGENTS.md` — agent rules, constraints, quality gates
- `agent_docs/README.md` — index of agent documentation
- `agent_docs/latex_driven_policy.md` — strict LaTeX-driven policy
- `agent_docs/coding_conventions.md` — code style rules
- `agent_docs/git_workflow.md` — branch, commit, PR workflow
- `agent_docs/memory_policy.md` — how project memory works
- `agent_docs/session_summary.md` — current state of the project
- `agent_docs/plans/` — active next-step plans (read all files here)
- `docs/ARCHITECTURE.md` — ferritex architecture
- `docs/ROADMAP.md` — development roadmap
- `docs/SUPPORTED_ELEMENTS.md` — LaTeX elements currently supported

If any of these files is missing, do not invent its content — ask the user or create
a minimal version following existing conventions.

## Memory policy

- Do not rely on global AI memory/history outside this repository (~/.codex/, ~/.claude/).
- The only authoritative persistent project memory is the Markdown files inside this repo.
- Hierarchy of truth:
  1. `AGENTS.md`
  2. `agent_docs/*.md` (including `plans/`)
  3. `docs/*.md`
- After any session that produces stable decisions, update the relevant files:
  - Policy/process → `agent_docs/*.md`
  - Architecture/format contracts → `docs/*.md`
  - Progress and next steps → `agent_docs/session_summary.md` and `agent_docs/plans/`
- Plans live in the project repo: `agent_docs/plans/`. Never store plans in `~/.claude/plans/`.
  After plan mode, copy the plan file into `agent_docs/plans/` immediately.

## Non-negotiable LaTeX-driven constraint

- Zero hardcodes in the renderer for project-specific visual matching.
- All formatting/layout parameters must come from:
  - LaTeX sources (commands, class/package effects, style files), **or**
  - explicit project configuration files (YAML/TOML).
- The pipeline is: LaTeX source → Parser extracts → `DocumentLayout` stores → `RenderProfile::from_layout()` resolves → renderer consumes.
- `DocumentLayout` fields are all `Option<T>`; `None` means "LaTeX did not express this preference, use fallback default."
- Constants in `crates/ferritex-renderer-docx/src/lib.rs` are fallback defaults only — not visual tweaks.
- `ferritex` must work for any LaTeX project without code changes.

When you think a "one-off hardcode" is needed:
1. Extend the parser to extract the parameter from LaTeX or config.
2. Add a field to `DocumentLayout`.
3. Resolve it in `RenderProfile::from_layout()` with a generic fallback.
4. Consume it in the renderer.
5. Add paired tests (present → value, absent → None, renderer fallback).

## Technical context

Key source files:
- `crates/ferritex-core/src/parser/latex.rs` — LaTeX parser
- `crates/ferritex-core/src/model/mod.rs` — `DocumentLayout`, `Block`, `Inline`, etc.
- `crates/ferritex-renderer-docx/src/lib.rs` — DOCX renderer + `RenderProfile`
- `src/build/`, `src/cli.rs`, `src/tui.rs`, `src/lib.rs`, `src/main.rs`
- `tests/fixtures/*.tex` — minimal test fixtures
- `tests/` — unit and integration tests

## Session workflow

1. Read all mandatory docs listed above.
2. Summarize current state from `agent_docs/session_summary.md` and `agent_docs/plans/`.
3. Confirm exact user task for this session (feature, bugfix, refactoring, tests, docs).
4. Implement changes: parser-first → model → renderer.
5. Add paired tests for every new `DocumentLayout` field.
6. Run quality gate: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
7. Commit to a feature branch and open a PR with `bot-merge` label.
8. Update `agent_docs/session_summary.md` and `agent_docs/plans/` with what was done and next steps.

## Git workflow summary

- Always work on feature branches: `git checkout -b feat/vX.Y-description`
- Never push directly to master.
- Always through PRs with quality gate green.
- Bot auto-merges with `bot-merge` label (squash + delete branch).
- SSH key must be added per-session: `export SSH_AUTH_SOCK=$(ls -t /tmp/com.apple.launchd.*/Listeners 2>/dev/null | head -1)`
```

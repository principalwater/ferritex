# Future Session Prompt (Claude Code / Codex)

Copy and use this at the beginning of a new session to restore full project context.

---

```markdown
## Role

You are an AI coding agent (Claude Code / OpenAI Codex) working on
the `ferritex` repository.

Project goal: a **LaTeX-driven, reusable conversion engine** with a core
crate (`ferritex-core`) and multiple renderer crates
(`ferritex-renderer-docx`, `ferritex-renderer-pdf`, `ferritex-renderer-md`, …),
with **no document-specific hardcoded formatting**.

---

## Mandatory files to read before any work

Before touching any code, read and internalize:

- `CLAUDE.md` — Claude Code session rules (if present)
- `AGENTS.md` — non-negotiable constraints, architecture, privacy, policy table
- `agent_docs/README.md` — index of agent documentation
- `agent_docs/latex_driven_policy.md` — strict LaTeX-driven policy
- `agent_docs/coding_conventions.md` — code style rules
- `agent_docs/git_workflow.md` — branch, commit, PR workflow
- `agent_docs/memory_policy.md` — how project memory works
- `agent_docs/session_summary.md` — current state of the project
- **all** `.md` files in `agent_docs/plans/` — active and archival plans
- `docs/ARCHITECTURE.md` — ferritex architecture
- `docs/ROADMAP.md` — development roadmap
- `docs/SUPPORTED_ELEMENTS.md` — LaTeX elements currently supported

If any of these files are missing or clearly outdated, do not invent
content — ask the user or create/update minimal versions following
existing conventions.

---

## Memory policy

- Do NOT rely on any global or external history (`~/.codex/`, `~/.claude/`).
- The _only_ long-term project memory lives in repository Markdown files.
- Project source-of-truth hierarchy (in order of authority):
  1. `AGENTS.md`
  2. `agent_docs/*.md` (including `plans/`)
  3. `docs/*.md`
- `CLAUDE.md` (if present) is Claude-specific runtime guidance; it is not a replacement
  for repository project memory listed above.
- When new stable decisions are made, update:
  - `agent_docs/*.md` for agent behavior, workflow, policies, plans;
  - `docs/*.md` for architecture, renderer contracts, supported elements;
  - `agent_docs/session_summary.md` for progress and next steps.
- Plans live in `agent_docs/plans/` only. Never store plans in `~/.claude/plans/`.
  After plan mode, copy the plan file into `agent_docs/plans/` immediately.

Scratch / temporary compaction notes go to `agent-docs/compaction/`
(gitignored, local-only).

---

## Key constraint: LaTeX-driven, multi-backend core

- No renderer or crate may hardcode layout or styling for a specific
  document.
- All formatting (fonts, sizes, margins, spacing, heading styles,
  list styles, footnote formatting, captions, table layout, math, TOC,
  numbering, etc.) must come from:
  - LaTeX sources (classes, packages, style files, commands), and/or
  - explicit project configuration files, and/or
  - generic rules described in `agent_docs/latex_driven_policy.md`.
- The pipeline is:
  `LaTeX source → Parser → AST + DocumentLayout → RenderProfile::from_layout() → renderer`
- `DocumentLayout` fields are all `Option<T>`; `None` means "LaTeX did not
  express this preference, use fallback default."
- Constants in renderer crates are fallback defaults only — not visual tweaks.
- `ferritex` must work for any LaTeX project (articles, journals,
  dissertations, books, reports) without code changes.

If you think a one-off hack is needed:

1. Extend the LaTeX parser in `ferritex-core` to extract the parameter.
2. Add a field to `DocumentLayout`.
3. Resolve it in `RenderProfile::from_layout()` with a generic fallback.
4. Consume it in the renderer.
5. Add paired tests (present → value, absent → None, renderer fallback).
6. Document the change in `docs/` and/or `agent_docs/`.

---

## Technical layout

Core:

- `crates/ferritex-core/src/parser/latex.rs` — LaTeX parser
- `crates/ferritex-core/src/model/mod.rs` — AST, `DocumentLayout`, `Block`, `Inline`
- `crates/ferritex-core/tests/` — core tests

Renderers:

- `crates/ferritex-renderer-docx/src/lib.rs` — DOCX renderer + `RenderProfile`
- `crates/ferritex-renderer-pdf/src/` — PDF renderer (stub)
- `crates/ferritex-renderer-md/src/` — Markdown renderer (stub)

CLI / glue:

- `src/build/mod.rs` — build orchestration
- `src/renderer/mod.rs` — glue layer
- `src/cli.rs`, `src/tui.rs`, `src/lib.rs`, `src/main.rs`

Tests:

- `tests/fixtures/*.tex` — minimal test fixtures
- `tests/integration_docx.rs`, `tests/integration_pdf.rs`, `tests/integration_md.rs`
- `tests/integration/*.rs`
- `tests/common/*`
- `tests/unit/` — unit test suites

When implementing changes: parser-first → model → renderer → tests.

---

## Manual QA artifact convention

- When the user asks to build DOCX/PDF/MD for parity checks:
  - ask the user for the input `.tex` path (never assume or hardcode
    a private project path),
  - default output directory: `/tmp/` (e.g. `/tmp/output.docx`).

---

## Git workflow summary

- Always work on feature branches: `git checkout -b feat/vX.Y-description`
- Never push directly to master.
- Always through PRs with quality gate green:
  `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
- Bot auto-merges with `bot-merge` label (squash + delete branch).
- SSH key must be added per-session:
  `export SSH_AUTH_SOCK=$(ls -t /tmp/com.apple.launchd.*/Listeners 2>/dev/null | head -1)`

---

## Session workflow

1. Read all mandatory docs and all plan files.
2. From `agent_docs/session_summary.md` and `agent_docs/plans/`, rebuild
   your understanding of:
   - current state of `ferritex-core` and renderers,
   - current focus areas,
   - agreed constraints and recent decisions,
   - next steps.
3. Ask the user what **specific task** should be done in this session
   (feature, bugfix, refactor, tests, docs).
4. Design and implement the task while strictly following:
   - `latex_driven_policy.md`,
   - `ARCHITECTURE.md`,
   - `ROADMAP.md`,
   - `coding_conventions.md`,
   - `git_workflow.md`.
5. If you change architecture, supported elements, or policies:
   - update `docs/*.md` and/or `agent_docs/*.md`;
   - update `agent_docs/session_summary.md` with:
     - what was done,
     - important decisions,
     - issues discovered,
     - suggested next steps.
```

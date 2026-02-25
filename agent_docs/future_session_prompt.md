# Future Session Prompt (Claude Code / Codex)

Copy and use this at the beginning of a new session to restore project context without wasting tokens on unnecessary full-repo scans.

---

```markdown
## Role

You are an AI coding agent (Claude Code / OpenAI Codex) working on the `ferritex` repository.

Project goal: a **LaTeX-driven, reusable conversion engine** with a shared core
(`ferritex-core`) and multiple renderer crates
(`ferritex-renderer-docx`, `ferritex-renderer-pdf`, `ferritex-renderer-md`, …),
with **no document-specific hardcoded formatting**.

---

## Operating mode (context-efficient)

Work in **context-efficient mode** by default:

1. Rebuild context from repository Markdown memory first.
2. Do **not** do a full codebase scan unless the task explicitly requires repo-wide audit/refactor.
3. Read code lazily and only for files relevant to the current task.
4. Use targeted `rg`/file reads and focused test runs during iteration.
5. Run full workspace quality gate only before final commit/PR (or when changes are cross-cutting).

---

## Mandatory docs to read before coding

Read and internalize these files before touching code:

- `CLAUDE.md` — client-specific runtime/session rules (if present)
- `AGENTS.md` — non-negotiable constraints, architecture, privacy, policy table
- `agent_docs/README.md` — index of agent docs and plans
- `agent_docs/latex_driven_policy.md`
- `agent_docs/coding_conventions.md`
- `agent_docs/git_workflow.md`
- `agent_docs/memory_policy.md`
- `agent_docs/session_summary.md`
- all `.md` files in `agent_docs/plans/`
- `docs/ARCHITECTURE.md`
- `docs/ROADMAP.md`
- `docs/SUPPORTED_ELEMENTS.md`

If any file is missing/outdated, do not invent hidden context. Ask the user or update docs minimally and explicitly.

---

## Memory policy (authoritative context)

- Do **not** rely on global/external histories (`~/.codex/`, `~/.claude/`).
- The only authoritative project memory is repository Markdown.
- Source-of-truth hierarchy:
  1. `AGENTS.md`
  2. `agent_docs/*.md` (including `plans/`)
  3. `docs/*.md`
- `CLAUDE.md` is runtime guidance for Claude clients, not a replacement for project memory.
- Persist stable decisions back into repository Markdown:
  - process/policy -> `agent_docs/*.md`
  - architecture/contracts/support matrix -> `docs/*.md`
  - progress/next actions -> `agent_docs/session_summary.md`
- Plans must live in `agent_docs/plans/` only.

Scratch notes: `agent-docs/compaction/` (gitignored).

---

## Core constraint: LaTeX-driven, multi-backend architecture

No renderer/crate may hardcode layout/styling for a specific document.

All formatting decisions (fonts, margins, spacing, headings, lists, captions, footnotes, TOC, numbering, math, etc.) must come from:
- LaTeX sources (including included style/config files), and/or
- explicit project config, and/or
- generic documented fallback rules.

Pipeline (mandatory):
`LaTeX source -> (LayoutProbe + Parser) -> AST + DocumentLayout -> RenderProfile::from_layout() -> renderer`

`DocumentLayout` fields are `Option<T>`:
- `Some(...)` => explicit LaTeX preference (probe or parser extracted)
- `None` => renderer fallback default

Extraction precedence is mandatory:
1. LayoutProbe (engine-evaluated effective values)
2. Parser static extraction
3. Renderer fallback defaults only if both layers are absent

If a one-off fix seems needed, do this instead:
1. probe/parser extraction (`ferritex-core`)
2. `DocumentLayout` field
3. merge precedence check (`probe > parser > fallback`)
4. `RenderProfile::from_layout()` mapping + generic fallback
5. renderer consumption
6. paired tests (present/absent/fallback)
7. docs update

---

## Test strategy expectations

Follow Rust-way test organization and keep test signal high:
- Rust Book reference:
  - https://doc.rust-lang.org/book/ch11-03-test-organization.html

Adopt property-based testing where it gives high leverage:
- prioritize invariants over many hand-written examples
- start from core/renderer critical invariants
- keep deterministic CI behavior and reproducible failures
- align with:
  - `agent_docs/plans/v0.9.4-test-organization-and-property-based.md`

---

## Technical map (quick reference)

Core:
- `crates/ferritex-core/src/parser/latex.rs`
- `crates/ferritex-core/src/model/mod.rs`
- `crates/ferritex-core/tests/`

DOCX renderer:
- `crates/ferritex-renderer-docx/src/lib.rs`
- `crates/ferritex-renderer-docx/tests/`

Other renderers:
- `crates/ferritex-renderer-pdf/src/` (stub/in progress)
- `crates/ferritex-renderer-md/src/` (stub/in progress)

Orchestration/CLI:
- `src/build/mod.rs`
- `src/renderer/mod.rs`
- `src/cli.rs`, `src/tui.rs`, `src/lib.rs`, `src/main.rs`

Workspace/integration tests:
- `tests/fixtures/*.tex`
- `tests/integration*.rs`
- `tests/integration/*.rs`
- `tests/common/*`
- `tests/unit/*`

Implementation order: parser -> model -> renderer -> tests -> docs.

---

## Session flow

1. Read mandatory docs and rebuild context from `session_summary` + plans.
2. Determine active focus from docs (do not assume stale priorities).
3. If user already gave a concrete task, execute it directly.
   If not, ask for one specific session task.
4. Implement with LaTeX-driven policy and minimal required code exploration.
5. Validate changes.
6. Update docs memory before session end.

---

## Validation policy

During iteration:
- run focused tests/checks for touched areas.

Before commit/PR:
- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`

---

## Manual QA artifact convention

When user asks to build DOCX/PDF/MD for parity checks:
- ask for input `.tex` path (never assume private path),
- default output to `/tmp/` (e.g. `/tmp/output.docx`).

---

## Git/PR workflow

- Always work on feature branches (`feat/...`, `fix/...`, `chore/...`).
- Never push directly to `master`.
- Merge only through PR with green quality gate.
- Bot merge path allowed via `bot-merge` label.
- Be careful with squash-merge rebase trap; prefer clean branch + cherry-pick of unique commits when needed.
```

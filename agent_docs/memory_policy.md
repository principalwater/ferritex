# Memory Policy (Project-local source of truth)

## Why this exists

Different AI clients may have incomplete or unsynchronized global histories (`~/.codex/`,
`~/.claude/`, etc.). To avoid context loss and drift, ferritex keeps persistent working
memory inside this repository.

## Source of truth hierarchy

These repository Markdown files are the **single source of truth** for project memory.
Agents must treat them as authoritative, in order:

1. `AGENTS.md` — non-negotiable constraints, architecture rules, privacy rules
2. `CLAUDE.md` — Claude Code session rules (if present)
3. `agent_docs/*.md` (including `plans/`) — agent workflow docs, plans, session state
4. `docs/*.md` — architecture, roadmap, supported elements

Global AI memory outside the repository (`~/.codex/`, `~/.claude/`) is advisory at best and
must not override local project docs. External logs/history may be used only for recovery, and
any stable recovered decision must be written back into repository docs immediately.

## Plans storage

**Plans live in the project repository only.**

- Active plans: `agent_docs/plans/` (all `.md` files here)
- Never store plans in `~/.claude/plans/`, `~/.codex/plans/`, or any global path.
- After plan mode ends, immediately copy the plan file to `agent_docs/plans/<descriptive-name>.md`.
- Completed plans should be marked (e.g. add `## Status: DONE` header) but not deleted, so history is preserved.

## Session start requirements (for any agent)

Before making changes, read and follow:
- `CLAUDE.md` (Claude Code agents) or equivalent agent config
- `AGENTS.md`
- `agent_docs/README.md`
- `agent_docs/latex_driven_policy.md`
- `agent_docs/coding_conventions.md`
- `agent_docs/git_workflow.md`
- `agent_docs/session_summary.md`
- `agent_docs/memory_policy.md` (this file)
- `agent_docs/future_session_prompt.md`
- all files in `agent_docs/plans/`
- `docs/ARCHITECTURE.md`
- `docs/ROADMAP.md`
- `docs/SUPPORTED_ELEMENTS.md`

## Update policy

When a session produces stable decisions, update repository docs **before** the session ends:
- Policy/process decisions → `agent_docs/*.md`
- Architecture/format contracts → `docs/*.md`
- Progress and near-term plan → `agent_docs/session_summary.md` and `agent_docs/plans/`

Do not leave critical context only in chat transcripts.
Do not leave stable context only in global AI memory.

## What belongs in session_summary.md

- Current merged PR state (what is on master)
- Active branch and its uncommitted state
- What was implemented and what is not yet done
- Deferred items
- Key files changed

## What belongs in agent_docs/plans/

- Detailed task breakdown for the next PR / feature
- Rationale for implementation approach
- Critical files to modify
- Verification steps
- Git workflow for this specific feature

## Compaction

When a session approaches context limits (90-95% usage):
1. Write/update `agent_docs/session_summary.md` with full current state.
2. Write/update the active plan in `agent_docs/plans/` with remaining tasks.
3. Notify the user that a new session can be started using `agent_docs/future_session_prompt.md`.

# Memory Policy (Project-local source of truth)

## Why this exists

Different AI clients may have incomplete or unsynchronized global histories (`~/.codex/`,
`~/.claude/`, etc.). To avoid context loss and drift, ferritex keeps persistent working
memory inside this repository.

## Source of truth hierarchy

The **only authoritative project memory** is repository Markdown:

1. `AGENTS.md` — non-negotiable constraints, architecture rules, privacy rules
2. `agent_docs/*.md` (including `plans/`) — workflow docs, plans, session state
3. `docs/*.md` — architecture, roadmap, supported elements

`CLAUDE.md` (when present) is a Claude-specific runtime instruction profile.
It must be read by Claude clients, but it is not a substitute for project memory in the files above.

Global AI memory outside the repository (`~/.codex/`, `~/.claude/`) is not reliable and must not
override repository docs. If external history is used for recovery, stable decisions must be
written back into repository Markdown immediately.

## Plans storage

**Plans live in the project repository only.**

- Active plans: `agent_docs/plans/`
- Completed plans archive: `agent_docs/archive/plans/`
- Never store plans in `~/.claude/plans/`, `~/.codex/plans/`, or any global path.
- After plan mode ends, immediately copy the plan file to `agent_docs/plans/<descriptive-name>.md`.
- Completed plans should be moved from `agent_docs/plans/` to `agent_docs/archive/plans/`
  so active planning context stays small while preserving history.

## Session start requirements (for any agent)

Before making changes, read and follow:
- `AGENTS.md`
- `agent_docs/README.md`
- `agent_docs/latex_driven_policy.md`
- `agent_docs/coding_conventions.md`
- `agent_docs/git_workflow.md`
- `agent_docs/session_summary.md`
- `agent_docs/memory_policy.md` (this file)
- `agent_docs/future_session_prompt.md`
- all active files in `agent_docs/plans/`
- `docs/ARCHITECTURE.md`
- `docs/ROADMAP.md`
- `docs/SUPPORTED_ELEMENTS.md`

Additional client-specific rule:
- Claude Code agents must also read `CLAUDE.md` when present.

## Update policy

When a session produces stable decisions, update repository docs **before** the session ends:
- Policy/process decisions → `agent_docs/*.md`
- Architecture/format contracts → `docs/*.md`
- Progress and near-term plan → `agent_docs/session_summary.md` and `agent_docs/plans/`

Do not leave critical context only in chat transcripts.
Do not leave stable context only in global AI memory.
Every new stable decision must be persisted in repository Markdown before session end.

## What belongs in session_summary.md

- Current merged PR state (what is on master)
- Active branch and its uncommitted state
- What was implemented and what is not yet done
- Deferred items
- Key files changed
- For LayoutProbe-related tasks: explicit run mode (`probe-enabled` vs `parser-only`)
  used for validation conclusions.

## What belongs in agent_docs/plans/

- Detailed task breakdown for the next PR / feature
- Rationale for implementation approach
- Critical files to modify
- Verification steps
- Git workflow for this specific feature

## What belongs in agent_docs/archive/plans/

- Completed plans retained for traceability
- Historical implementation rationale that no longer requires active execution

## Compaction

When a session approaches context limits (90-95% usage):
1. Write/update `agent_docs/session_summary.md` with full current state.
2. Write/update the active plan in `agent_docs/plans/` with remaining tasks.
3. Notify the user that a new session can be started using `agent_docs/future_session_prompt.md`.

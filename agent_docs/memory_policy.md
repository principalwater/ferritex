# Memory Policy (Project-local source of truth)

## Why this exists

Different AI clients may have incomplete or unsynchronized global histories (`~/.codex/`, `~/.claude/`, etc.).  
To avoid context loss and drift, ferritex keeps persistent working memory inside this repository.

## Source of truth hierarchy

Agents must treat these files as authoritative project memory:

1. `AGENTS.md`
2. `agent_docs/*.md`
3. `docs/*.md`

Global AI memory outside the repository is advisory at best and must not override local project docs.

## Session start requirements (for any agent)

Before making changes, read and follow:
- `AGENTS.md`
- `agent_docs/latex_driven_policy.md`
- `agent_docs/coding_conventions.md`
- `agent_docs/git_workflow.md`
- `agent_docs/session_summary.md` (or the latest current plan file)
- `agent_docs/memory_policy.md`
- `docs/ARCHITECTURE.md`
- `docs/ROADMAP.md`
- `docs/SUPPORTED_ELEMENTS.md`

## Update policy

When a session produces stable decisions, update repository docs immediately:
- policy/process decisions -> `agent_docs/*.md`
- architecture/format contracts -> `docs/*.md`
- progress and near-term execution plan -> `agent_docs/session_summary.md` (or current plan file)

Do not leave critical context only in chat transcripts.

## Compaction storage

Local compaction artifacts should be stored at:
- `agent-docs/compaction/`

This path is intentionally ignored by git and is not part of repository history.

# Future Compaction Prompt (Claude Code / Codex)

Copy and use this at the end of a session to create a reliable handoff for the next session.

---

```markdown
## Role

You are an AI coding agent (Claude Code / OpenAI Codex) working on the `ferritex` repository.

Goal: perform clean **session compaction** so the next session can start from repository memory, not chat history.

---

## Compaction Principles

1. Repository Markdown files are the only reliable long-term project memory.
2. Do not rescan the entire codebase unless needed; use current git state, existing docs, and targeted checks.
3. Record only stable decisions, actual repository state, and explicit next steps.
4. Preserve LaTeX-driven architecture constraints; no document-specific hardcodes.

---

## Required Files to Update

1. `agent_docs/session_summary.md`
2. Relevant active/archival files in `agent_docs/plans/` (status updates or new plan when needed)
3. `agent_docs/memory_policy.md` (if memory/process rules changed)
4. `agent_docs/future_session_prompt.md` (if startup workflow changed)
5. `docs/*.md` and/or `AGENTS.md` only when architecture/support/policy actually changed

---

## Required Content for `agent_docs/session_summary.md`

1. Repository state snapshot:
   - active branch,
   - working tree status,
   - PR status (if any),
   - quality-gate status (`fmt`/`clippy`/`test`).
2. Work completed in the session:
   - parser/model/renderer/tests/docs changes,
   - key technical decisions and rationale.
3. Not completed / constraints:
   - known gaps,
   - deferred items.
4. Next steps:
   - priority plan file in `agent_docs/plans/`,
   - concrete execution order for the next session.

---

## Plan Requirements (`agent_docs/plans/`)

- Each new work wave must have an explicit plan file (new or updated) with status:
  - `PLANNED`, `IN PROGRESS`, `PARTIALLY COMPLETED`, `COMPLETED`.
- Each plan should include:
  - goal,
  - constraints,
  - workstreams,
  - verification,
  - exit criteria.
- New cross-cutting tracks (for example test architecture or property-based adoption) should be captured in dedicated plan files and linked from active roadmap plans.

---

## Compaction Checklist (in order)

1. Check `git status`, open PRs, and active branch.
2. Update `agent_docs/session_summary.md`.
3. Update statuses and links in relevant `agent_docs/plans/*.md`.
4. Update `agent_docs/README.md` if plan files were added or renamed.
5. Update `agent_docs/future_session_prompt.md` if startup workflow changed.
6. Verify `agent_docs/memory_policy.md` is still accurate.
7. Provide short handoff to the user:
   - files updated,
   - active plan path,
   - first priority task for the next session.

---

## Important

- Do not rely on `~/.codex/` or `~/.claude/` as source of truth.
- Do not leave critical decisions only in chat.
- If time is limited, prefer minimal but accurate updates to `session_summary.md` + active plans over a long but incomplete report.
```

# CLAUDE.md — Claude Code Instructions (ferritex)

This file is loaded automatically by Claude Code at session start.
It supplements `AGENTS.md` (which covers architecture and policy rules).
Here we capture Claude-specific workflow habits, persistent mistakes to avoid,
and session-to-session continuity rules.

> **Priority**: CLAUDE.md overrides any Claude Code default behavior.
> Rules here are non-negotiable regardless of what the model infers from context.

---

## Memory location

- **Project memory lives in this repo only.**
  - Primary running notes: `agent_docs/session_summary.md`
  - Plans: `agent_docs/plans/`
  - Do NOT write memory or plans to `~/.claude/plans/` or any path outside this repo.
- Global `~/.claude/projects/.../MEMORY.md` may be updated for brief cross-project
  state (e.g. current open PR numbers), but the authoritative context is always
  `agent_docs/session_summary.md`.

---

## Session startup checklist

At the start of every session, read these files before touching any code:

1. `AGENTS.md` — architecture rules, LaTeX-driven policy, parameter table
2. `agent_docs/session_summary.md` — last known state, open issues, next steps
3. Active plan file(s) in `agent_docs/plans/` (check session_summary for which is active)

Do NOT start writing code before completing this checklist.

---

## Plan mode rules

- When entering plan mode, write the plan file directly to `agent_docs/plans/<name>.md`.
- Never use the auto-generated `~/.claude/plans/<random-slug>.md` as the final location.
- After plan mode exits, verify the plan file exists under `agent_docs/plans/`.

---

## Git workflow

- All work goes through feature branches + PRs. Never push directly to master.
- Branch naming: `feat/vX.Y-short-description`
- Quality gate before every commit (all three must pass):
  ```
  cargo fmt --all
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```
- After committing, add the `bot-merge` label to trigger automated squash + merge.
- SSH agent may not carry over from the user's terminal:
  ```
  export SSH_AUTH_SOCK=$(ls -t /tmp/com.apple.launchd.*/Listeners 2>/dev/null | head -1)
  ```
  Run this before any `git push` or `gh` command if SSH fails.

### Rebase trap: squash-merged PRs

When a PR is merged via squash, its individual commits disappear from master history.
If a follow-up branch was based on those commits, a naive `git rebase origin/master`
will replay them as duplicates and cause conflicts.

**Correct procedure:**
1. Identify commits unique to the follow-up branch:
   `git log --oneline <master-tip>..<branch-tip>`
2. Create a new branch from master:
   `git checkout -b <new-branch> origin/master`
3. Cherry-pick only the unique commits:
   `git cherry-pick <sha1> <sha2> ...`
4. Delete the old branch; push and PR the new one.

---

## CI / clippy notes

- CI runs on a newer Rust toolchain than the local environment.
  Lints that pass locally may fail on CI (e.g. `clippy::replace_box` added in Rust 1.93+).
- Always run `cargo clippy --workspace --all-targets -- -D warnings` locally before push,
  and treat any new CI-only lint as a blocker — fix immediately, do not `allow`.
- Known past CI lint failure:
  `*run = Box::new((**run).clone().foo())` → must be `**run = (**run).clone().foo()`

---

## LaTeX-driven policy (summary — full table in AGENTS.md)

- Zero hardcoded formatting values in the renderer.
- Every formatting decision traces: LaTeX source → parser → `DocumentLayout` → `RenderProfile::from_layout()` → renderer.
- Renderer constants are fallback-only defaults for when LaTeX does not express a setting.
- When a formatting issue is reported: fix the parser extraction first, never patch the renderer.
- Exception: `apply_known_counter_fallbacks` is dissertation-class-gated (see AGENTS.md known exception note).

---

## Privacy rules (summary — full rules in AGENTS.md)

- No real names, institution names, dissertation titles, or private paths anywhere in code,
  comments, fixtures, tests, commit messages, or PR bodies.
- Build personal corpus outputs only to external paths (e.g. `/tmp/dissertation.docx`).
- Commit/PR text must use generic wording: `validated on a large multi-file LaTeX corpus`,
  not `validated on thesis/dissertation.tex`.

---

## Communication

- Respond to the user in **Russian**.
- All code, comments, doc strings, test names, commit messages, and PR text — in **English**.

---

## After completing significant work

Update `agent_docs/session_summary.md` with:
- What was done (PR numbers, commit hashes, decisions made)
- Open issues and known limitations
- Immediate next steps

This is the handoff document for the next session.

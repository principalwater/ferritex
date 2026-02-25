# Git Workflow

## Branching

- Never push feature work directly to `master`.
- Start from latest `master` and create a focused branch:
  - `feat/...` for features,
  - `fix/...` for bug fixes,
  - `chore/...` for maintenance.
- Keep one logical change-set per branch.

## Pull Requests

- All changes must go through a PR.
- PR title/body must describe user-visible intent and technical scope.
- Keep PR scope coherent: parser/model/renderer updates for the same feature belong together.

## Quality Gate (Mandatory Before PR Merge)

Run and pass:

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## Review Expectations

- No project-specific hardcoded formatting in renderer.
- New formatting behavior must be LaTeX-driven and propagated through `DocumentLayout`.
- Tests must cover positive extraction and absence/fallback behavior.

## Bot Merge

- After CI is green and repository gates pass, add `bot-merge` label to enable automated merge flow.
- Keep branch up to date with base if CI requests branch update.

## Commit Hygiene

- Use clear commit messages (`feat: ...`, `fix: ...`, `chore: ...`).
- Do not mix unrelated changes in one commit.
- Keep repository content privacy-safe and generic.

## Branch Protection

- Master branch requires 1 approval + CI green before merge.

## SSH Agent (Claude Code / remote agents)

The shell may not inherit the user's SSH agent. Before any `git push` or `gh` command:
```bash
export SSH_AUTH_SOCK=$(ls -t /tmp/com.apple.launchd.*/Listeners 2>/dev/null | head -1)
```

## Squash-Merge Rebase Trap

When a PR is squash-merged, its individual commits disappear from master history.
If a follow-up branch was based on those commits, `git rebase origin/master` will
replay them as duplicates and cause conflicts.

Correct procedure:
1. Identify unique commits: `git log --oneline <master-tip>..<branch-tip>`
2. Create a new branch from master: `git checkout -b <new-branch> origin/master`
3. Cherry-pick only unique commits: `git cherry-pick <sha1> <sha2> ...`
4. Delete old branch; push and PR the new one.

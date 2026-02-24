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
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
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

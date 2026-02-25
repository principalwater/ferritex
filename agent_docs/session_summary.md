# Session Summary (updated 2026-02-26, post-pass-orchestration)

## Project Goal

`ferritex` stays LaTeX-driven with one semantic contract for all backends:

```text
LaTeX source -> LayoutProbe + parser extraction
             -> merge (probe > parser > fallback)
             -> DocumentLayout
             -> renderer mapping
```

Canonical runtime direction remains:

```text
PDF:  tectonic::latex_to_pdf
DOCX/MD: parser-first semantics + LayoutProbe fallback/validation
```

## Repository State

- `origin/master` includes merged PRs:
  - `#40` probe confidence + docs,
  - `#41` DOCX orientation section switching,
  - `#42` canonical PDF path + TotPages hardening.
- Current working branch: `feat/v0.9.6-texengine-pass-orchestration`.
- Working tree: dirty (new low-level pass orchestration slice + docs/memory sync).
- Stash note still present: `wip-unrelated-doc-delta`.

## Quality Gate Status

- Historical full gate for merged slices is green on `master`:
  - `cargo fmt --all` ✅
  - `cargo clippy --workspace --all-targets --locked -- -D warnings` ✅
  - `cargo test --workspace --locked` ✅
- Current branch focused validation is green:
  - `cargo fmt --all` ✅
  - `cargo clippy -p ferritex-core --all-targets --features layout-probe-tectonic --locked -- -D warnings` ✅
  - `cargo test -p ferritex-core --features layout-probe-tectonic --locked layout_probe::tectonic::tests` ✅

## Completed in This Session

1. Closed merge pipeline for split PR stream:
   - force-updated and merged `#42` after `#40/#41` landed,
   - verified green CI and branch deletion.
2. Implemented direct low-level probe pass orchestration under `TexEngine` runtime profile:
   - primary probe pass: `PassSetting::Tex` (single pass),
   - conditional recovery pass: `PassSetting::Default` with `reruns=1`,
   - recovery triggers only on primary run error or empty extracted signal,
   - selection policy keeps recovery only when it improves signal/health.
3. Added unit coverage for pass-orchestration policy decisions in `layout_probe::tectonic` tests.
4. Synced architecture/plan docs to the new `TexEngine` pass-control state.

## Not Done / Known Gaps

1. Current pass-orchestration slice is not committed/opened as PR yet.
2. Full workspace quality gate has not yet been re-run on this branch after doc updates.
3. TotPages runtime fallback on larger multi-file corpora still needs broader regression corpus coverage.

## Active Plans

1. Primary: `agent_docs/plans/v0.9.5-layoutprobe-tectonic-foundation.md`
2. Parallel parity stream: `agent_docs/plans/v0.9.3-docx-parity-disstyles-wave2.md`
3. Cross-cutting testing stream: `agent_docs/plans/v0.9.4-test-organization-and-property-based.md`
4. Backend stream: `agent_docs/plans/v1.0-multi-backend-foundation.md`

## Next Session Steps (ordered)

1. Run full workspace quality gate on `feat/v0.9.6-texengine-pass-orchestration`.
2. Commit and open PR for low-level pass orchestration + doc sync.
3. Validate probe pass-selection behavior on a larger multi-file corpus and add regressions if thresholds need tuning.

# Integration Tests

Integration tests are split by feature area and share common helpers from
`tests/common/mod.rs`.

Rules:
- use `tests/fixtures/*.tex` as test input documents,
- keep expected values in `tests/fixtures/expected/*` when practical,
- avoid duplicating ZIP/XML helper code in each test file.

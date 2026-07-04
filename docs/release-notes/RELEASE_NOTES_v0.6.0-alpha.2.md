# LocalAgent v0.6.0-alpha.2 Release Notes

Release date: 2026-07-04

## Highlights

- Split inferred Rust validation into ordered validator outcomes:
  - `cargo fmt --check`
  - `cargo test`
  - `cargo clippy -- -D warnings`
- Validator steps now record pass, fail, or skipped outcomes.
- Rust validation stops after the first failed validator and marks later validators as skipped.
- Preserved the existing one-repair budget, shell/gate/audit path, and bounded continuation behavior.

## Release Process

- Updated crate metadata to `0.6.0-alpha.2`.
- Release readiness now supports validating that a release tag matches `Cargo.toml` package metadata.

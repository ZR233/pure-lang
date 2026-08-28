# Fixture agent rules

- Update `design/task-workflows.md` before implementation.
- Do not add dependencies or change `Cargo.toml`.
- Normalization work owns only `src/normalize.rs` and `tests/normalize.rs`.
- Its executor blueprint must use exactly `scopeHints: ["src/normalize.rs", "tests/normalize.rs"]`; do not put prose in `scopeHints`.
- Normalization discards leading/trailing ASCII whitespace/`_`/`-` runs and collapses only internal runs to one `-`; this boundary is not open to reinterpretation.
- Validation work owns only `src/validate.rs` and `tests/validate.rs`.
- Its executor blueprint must use exactly `scopeHints: ["src/validate.rs", "tests/validate.rs"]`; do not put prose in `scopeHints`.
- Run the exact commands required by the user prompt and leave the Git worktree clean.

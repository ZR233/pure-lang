# Fixture agent rules

- Update `design/task-workflows.md` before implementation.
- Do not add dependencies or change `Cargo.toml`.
- Normalization work owns only `src/normalize.rs` and `tests/normalize.rs`.
- Normalization discards leading/trailing ASCII whitespace/`_`/`-` runs and collapses only internal runs to one `-`; this boundary is not open to reinterpretation.
- Validation work owns only `src/validate.rs` and `tests/validate.rs`.
- Run the exact commands required by the user prompt.
- This fixture intentionally has no Git repository. Do not initialize one or create worktree state.

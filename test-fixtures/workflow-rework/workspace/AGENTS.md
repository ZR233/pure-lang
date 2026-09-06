# Rework fixture rules

- Root updates `design/task-workflows.md` before implementation.
- One implementation assignment owns `src/normalize.rs` and `tests/normalize.rs` together;
  delegate this pair to an executor and keep its implementation and regression tests together.
- Other existing files are read-only. Do not add dependencies or change the API.
- Run `cargo test` as acceptance evidence.
- This fixture intentionally has no Git repository. Do not initialize one or create worktree state.

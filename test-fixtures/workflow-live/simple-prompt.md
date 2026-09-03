Directly complete the following Rust library change in this temporary dependency-free project.

Use the ordinary workspace, file, and process tools as needed. This Thread uses
`mode.simple`, so it has no registered workflow graph or `workflow_*` tools. Do
not create a plan or wait for confirmation, and do not ask clarifying questions.
Inspect the existing contract, modify the allowed files, run the checks, and
finish the request with the `complete` tool.

Implement two independent APIs:

1. `normalize_key` in `src/normalize.rs` with tests in `tests/normalize.rs`.
   Lowercase ASCII letters, preserve digits, trim leading/trailing runs of ASCII
   whitespace, `_`, or `-`, and collapse each internal separator run to one `-`.
   Reject invalid bytes at their original index, then empty and overlong results.

2. `validate_key` in `src/validate.rs` with tests in `tests/validate.rs`.
   Accept canonical keys of 1..=48 bytes beginning with a lowercase ASCII letter,
   followed by lowercase letters, digits, and isolated internal hyphens. Preserve
   the documented error variants for every invalid boundary.

Before implementation, update `design/task-workflows.md` with the API contract,
error boundaries, examples, and verification commands. Do not add dependencies
or modify existing public files outside the two implementation/test pairs and
the design document. The project is intentionally not a Git repository.

After implementation run:

```text
cargo test
cargo run --quiet --bin fixture_verify
```

The verifier must print `PURE_WORKFLOW_GUI_VERIFY_OK` on its own line. The final
`complete` summary must describe both APIs and the real verification results.

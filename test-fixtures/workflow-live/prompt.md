Complete the following Rust library change in this temporary dependency-free project.

First inspect the existing contract. Before producing a plan, ask exactly one material user
question with `request_user_input`: whether an invalid non-ASCII input position is reported as its
original UTF-8 byte index or Unicode scalar index. Incorporate the answer into a concrete
implementation plan. While the Task workflow remains in `planning`, call `plan_current`,
`plan_next`, and `plan_history`, then use `plan_submit` with the canonical Plan revision and wait
for confirmation. Do not modify any project file before the revised Plan reaches `approved`; if I
request additions, read the resulting `revisionRequested` state and submit the complete updated
Plan. Only then transition the Task workflow from `planning` to `editing_documents`.

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
response must summarize both APIs and the real verification results.

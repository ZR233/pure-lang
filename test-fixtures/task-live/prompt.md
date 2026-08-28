Complete the following Rust library change in this temporary Git repository and drive the Task state machine to a successful terminal state.

The repository is a dependency-free crate named `task-live-fixture`. Before implementation, update and commit `design/task-workflows.md` so it specifies both APIs, their error boundaries, their acceptance examples, and the required verification commands. The implementation must then be split into exactly two independent executor workstreams with non-overlapping write scopes:

1. Normalization workstream
   - May change only `src/normalize.rs` and `tests/normalize.rs`.
   - Implement `normalize_key` as documented by the existing public API: lowercase ASCII letters, preserve ASCII digits and letters, discard leading and trailing runs made from ASCII whitespace, `_`, or `-`, and turn each remaining internal run of those separator bytes into one `-`. Thus `"  -Release__Candidate 42-_  "` must become `"release-candidate-42"`; leading/trailing separators must never remain in the normalized key. Reject any other byte at its original input byte index before checking the normalized empty/length boundaries, then reject an empty normalized value, and finally reject normalized values longer than 48 bytes.
   - Preserve precise `NormalizeError` variants and cover success plus every error boundary.
   - Run `cargo test --test normalize` and `cargo test` in the executor worktree.

2. Validation workstream
   - May change only `src/validate.rs` and `tests/validate.rs`.
   - Implement `validate_key` as documented by the existing public API: accept 1..=48 byte canonical keys whose first byte is an ASCII lowercase letter and whose remaining bytes are ASCII lowercase letters, digits, or isolated internal `-` separators. Reject uppercase input, a leading/trailing hyphen, consecutive hyphens, invalid characters, empty input, and overlong input with the precise `ValidationError` variant.
   - Preserve precise `ValidationError` variants and cover success plus every error boundary.
   - Run `cargo test --test validate` and `cargo test` in the executor worktree.

Do not add dependencies or modify `Cargo.toml`, `Cargo.lock`, `src/lib.rs`, `src/bin/fixture_verify.rs`, `README.md`, `AGENTS.md`, `.gitignore`, or files outside the explicitly assigned workstream and the planner-owned design document. Each executor must receive a self-contained structured blueprint with at least two ordered implementation steps, explicit acceptance criteria, references to `design/task-workflows.md`, and the exact verification commands. The normalization blueprint's `scope.scopeHints` JSON array must be exactly `["src/normalize.rs", "tests/normalize.rs"]`; the validation blueprint's must be exactly `["src/validate.rs", "tests/validate.rs"]`. These entries are repository-relative path prefixes, never prose or evidence notes. Keep each blueprint concise and spawn only one executor per provider response; after the first spawn succeeds, immediately spawn the second in the next continuation instead of composing both blueprints in one response.

Required Task process:

- Start with `task_status`. In Planning, submit a concrete plan using the exact current revision and generation, then stop for plan confirmation.
- After confirmation, update and commit only `design/task-workflows.md`, finish the document-editing phase using a fresh status revision/generation, then spawn both executors. They may run concurrently and must work in separate worktrees.
- For each executor Completion, request and obtain an independent Delivery Review. Do not approve, close, or merge a Completion without a passing current-head delivery review.
- The normalization boundary above is final and intentionally guarantees that every successful output has no leading/trailing separator; do not request user clarification or revise the design to preserve boundary separators.
- Close each approved executor, integrate both commits into the planner worktree using ordinary Git, and record every integration with `task_record_merge` using the exact Completion revision and actual before/after HEAD values.
- After both merges, run `cargo test` and `cargo run --quiet --bin fixture_verify`. The latter must print `PURE_TASK_FIXTURE_VERIFY_OK` on its own line.
- Begin and pass an Integrated Review of the combined current HEAD. The final Task must not use the single-executor-equivalent exemption.
- Complete with outcome `succeeded` only when the current completion gate has no blocker, all interactions and Turns are terminal, both work units are merged, the verification commands pass, and the root Git worktree is clean.

The final answer must summarize the two delivered APIs and cite the actual test and verifier results, but deterministic repository checks—not the prose answer—are the acceptance authority.

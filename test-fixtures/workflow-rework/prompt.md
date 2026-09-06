Complete normalize_key in this temporary dependency-free Rust project according to
AGENTS.md and docs/product-contract.md. The invalid-byte index is already decided:
use the original UTF-8 byte offset. Preserve the public signature and error variants.

Update design/task-workflows.md with the contract, examples, boundaries and validation
commands, then delegate src/normalize.rs and tests/normalize.rs together as one owned
implementation task. Extend the existing tests to cover the documented boundaries.
Do not modify other existing files or add dependencies.
The project is intentionally not a Git repository.

After integration run cargo test. Report actual verification evidence.

This acceptance has one external review-snapshot checkpoint. After the first
implementation delivery has finished and been read and integrated, and before
spawning any reviewer, call request_user_input once with question id
fixture_review_checkpoint. Ask whether the external review snapshot is ready and
await the answer. Do not close implementation agents before this checkpoint.
Do not repeat it on subsequent review waves. The checkpoint does not replace review.

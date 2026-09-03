Produce and obtain approval for a complete implementation plan for the following Rust library
change in this temporary dependency-free project. This acceptance task ends after the Plan is
approved: do not modify project files, run implementation commands, transition the workflow out of
`planning`, or call `complete`.

All product decisions are already specified. `normalize_key` in `src/normalize.rs` must lowercase
ASCII letters, preserve digits, trim leading and trailing runs of ASCII whitespace, `_`, or `-`,
collapse each internal separator run to one `-`, reject the first invalid non-ASCII byte at its
original UTF-8 byte index, then reject empty and overlong results. `validate_key` in
`src/validate.rs` must accept canonical keys of 1..=48 bytes beginning with a lowercase ASCII
letter, followed by lowercase letters, digits, and isolated internal hyphens, while preserving the
documented error variants for every invalid boundary.

The plan must update `design/task-workflows.md` before implementation, limit production and test
changes to `src/normalize.rs`, `src/validate.rs`, `tests/normalize.rs`, and `tests/validate.rs`, add
no dependencies, and validate with `cargo test` followed by
`cargo run --quiet --bin fixture_verify`. The verifier must print `PURE_WORKFLOW_GUI_VERIFY_OK` on
its own line. Include exact ownership, ordering, regression coverage, failure handling, and
verification evidence in the Plan.

Use the canonical Plan lifecycle to submit the complete Plan for review. If the user requests a
revision, incorporate every requested change and submit the complete replacement. After approval,
read the canonical Plan state to confirm it is `approved`, report that planning is complete, and
stop without beginning implementation.

use pl_core::StaticThreadModeRegistration;

pub const PROMPT: &str = r#"# Simple Thread Mode

You own one canonical root task and may use the available workspace, command, Git, collaboration,
interaction, and completion tools under their ordinary contracts. Directly understand the user's
goal, choose an appropriate exploration, implementation, and verification sequence, and deliver the
result. This Mode has no workflow graph and no workflow tools; do not manufacture stages or wait for
plan approval unless the user explicitly asks for it.

Use child agents only when they improve isolation or parallelism. Before ending a successful root
Turn, call `complete` once with a concise summary and concrete evidence. Do not substitute ordinary
assistant text for the completion tool."#;

pub const REGISTRATION: StaticThreadModeRegistration = StaticThreadModeRegistration {
    id: "mode.simple",
    display_name: "简洁",
    description: "直接完成普通请求，并按实际风险选择探索、修改和验证步骤",
    order: 10,
    prompt: PROMPT,
    workflow: None,
};

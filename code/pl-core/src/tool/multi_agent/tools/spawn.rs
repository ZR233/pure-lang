use pl_protocol::{AgentStatus, PureError};

use crate::agent::AgentSpawnInput;

use super::super::schema::AgentControlToolKind;
use super::super::types::{SpawnAgentArgs, SpawnAgentResult, SpawnAgentTool};
use super::super::{
    BoxFuture, ForkTurns, Tool, ToolContext, ToolInput, ToolOutput, child_agent_options,
    current_agent_path, fork_session, invalid_spawn_input, json_output, role_key,
};

impl Tool for SpawnAgentTool {
    fn name(&self) -> &str {
        "spawn_agent"
    }

    fn description(&self) -> &str {
        "Spawn a managed sub-agent for an independent task. Use this when \
         the user asks for subagents, per-crate exploration, parallel work, \
         or isolated context. The spawned agent runs asynchronously; use \
         wait_agent to observe completion."
    }

    fn input_schema(&self) -> serde_json::Value {
        AgentControlToolKind::SpawnAgent.input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let args: SpawnAgentArgs =
                serde_json::from_value(input.arguments).map_err(invalid_spawn_input)?;
            let role = role_key(args.agent_type.as_deref())?;
            let fork_turns = ForkTurns::parse(args.fork_turns.as_deref())?;
            let parent_path = current_agent_path(&context);
            let prompt = args.message.clone();
            let child_session = fork_session(&context.parent_session, fork_turns);
            let options = child_agent_options(&context.options);
            let run_spec = self.runtime.run_config(
                &context,
                options,
                input.tool_id.clone(),
                prompt.clone(),
                child_session,
            );
            let handle = context
                .agent_supervisor
                .spawn_agent(
                    AgentSpawnInput {
                        task_name: args.task_name.clone(),
                        message: prompt.clone(),
                        role: role.clone(),
                        parent_path: Some(parent_path.clone()),
                    },
                    run_spec,
                )
                .await?;

            json_output(SpawnAgentResult {
                agent_id: handle.id,
                task_name: args.task_name,
                path: handle.path,
                status: AgentStatus::Queued,
                turn_id: None,
                worktree: handle.worktree.clone(),
            })
        })
    }
}

use std::collections::BTreeSet;

use pl_core::{
    AgentAccessPolicy, AgentExecutionPolicy, AgentRoleId, AgentSnapshot, AgentTargetSelector,
    ToolEffect, ToolEffectSet, ToolVisibilitySet, TurnFinalizationPolicy,
};

use crate::StudioMode;
use crate::studio::task_coordinator::TaskRunStateKind;

const COLLABORATION_CONTROL_TOOLS: [&str; 7] = [
    "spawn_agent",
    "send_message",
    "interrupt_agent",
    "list_agents",
    "wait_agents",
    "read_agent_session",
    "close_agent",
];
const PLAN_EXIT_TOOL: [&str; 1] = ["plan_exit"];

#[derive(Debug, Clone, Copy)]
pub(super) struct StudioPolicyContext {
    pub(super) mode: StudioMode,
    pub(super) task_phase: Option<TaskRunStateKind>,
}

pub(super) fn studio_execution_policy(
    snapshot: &AgentSnapshot,
    context: StudioPolicyContext,
    mut visible_tools: ToolVisibilitySet,
) -> AgentExecutionPolicy {
    let is_root = snapshot.identity.parent_id.is_none();
    let collaboration = if is_root {
        AgentAccessPolicy {
            spawn_roles: spawn_roles(context.mode),
            list_targets: AgentTargetSelector::Tree,
            message_targets: AgentTargetSelector::Tree,
            close_targets: AgentTargetSelector::Tree,
        }
    } else {
        AgentAccessPolicy::default()
    };
    if is_root {
        visible_tools.extend_tool_names(COLLABORATION_CONTROL_TOOLS);
    } else {
        visible_tools = without_tools(visible_tools, &COLLABORATION_CONTROL_TOOLS);
        visible_tools.extend_tool_names(["report_progress"]);
    }
    if context.mode == StudioMode::Task {
        visible_tools = without_tools(visible_tools, &PLAN_EXIT_TOOL);
    }

    AgentExecutionPolicy {
        visible_tools,
        allowed_effects: ToolEffectSet::from_effects(allowed_effects(snapshot)),
        collaboration,
        finalization: finalization(snapshot, context),
    }
}

fn spawn_roles(mode: StudioMode) -> BTreeSet<AgentRoleId> {
    let roles: &[&str] = match mode {
        StudioMode::Simple | StudioMode::Task => &["explorer"],
    };
    roles
        .iter()
        .map(|role| AgentRoleId::new(*role).expect("Studio built-in role must be valid"))
        .collect()
}

fn without_tools(visible_tools: ToolVisibilitySet, denied: &[&str]) -> ToolVisibilitySet {
    ToolVisibilitySet::from_tool_names(
        visible_tools
            .into_names()
            .into_iter()
            .filter(|name| !denied.contains(&name.as_str())),
    )
}

fn allowed_effects(snapshot: &AgentSnapshot) -> Vec<ToolEffect> {
    if snapshot.identity.parent_id.is_none() {
        return vec![
            ToolEffect::Read,
            ToolEffect::WorkspaceWrite,
            ToolEffect::Process,
            ToolEffect::AgentControl,
            ToolEffect::BranchControl,
        ];
    }
    match snapshot.identity.role.as_str() {
        "executor" => vec![
            ToolEffect::Read,
            ToolEffect::WorkspaceWrite,
            ToolEffect::Process,
            ToolEffect::AgentControl,
            ToolEffect::BranchControl,
        ],
        "explorer" | "reviewer" | "planner" => vec![
            ToolEffect::Read,
            ToolEffect::WorkspaceWrite,
            ToolEffect::Process,
            ToolEffect::AgentControl,
        ],
        _ => vec![ToolEffect::Read],
    }
}

fn finalization(snapshot: &AgentSnapshot, context: StudioPolicyContext) -> TurnFinalizationPolicy {
    if context.mode == StudioMode::Simple {
        return TurnFinalizationPolicy::Direct;
    }
    if snapshot.identity.parent_id.is_some() {
        return match snapshot.identity.role.as_str() {
            "executor" => required_tool("report_completion"),
            "reviewer" => required_tool("review_exit"),
            _ => TurnFinalizationPolicy::Direct,
        };
    }
    match context.task_phase {
        Some(TaskRunStateKind::Planning | TaskRunStateKind::EditingDocuments) => {
            required_tool("task_transition")
        }
        None
        | Some(
            TaskRunStateKind::PendingConfirmation
            | TaskRunStateKind::Working
            | TaskRunStateKind::Reviewing
            | TaskRunStateKind::Completed,
        ) => TurnFinalizationPolicy::Direct,
    }
}

fn required_tool(name: &str) -> TurnFinalizationPolicy {
    TurnFinalizationPolicy::RequiredTool {
        name: name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use pl_core::{AgentIdentity, AgentState, ThreadId};

    use super::*;

    #[test]
    fn task_state_never_removes_ordinary_workspace_tools() {
        let root = snapshot("planner", true);
        for state in [
            TaskRunStateKind::Planning,
            TaskRunStateKind::PendingConfirmation,
            TaskRunStateKind::EditingDocuments,
            TaskRunStateKind::Working,
            TaskRunStateKind::Reviewing,
            TaskRunStateKind::Completed,
        ] {
            let policy = studio_execution_policy(
                &root,
                StudioPolicyContext {
                    mode: StudioMode::Task,
                    task_phase: Some(state),
                },
                ToolVisibilitySet::from_tool_names([
                    "exec",
                    "apply_patch",
                    "git_commit",
                    "plan_exit",
                ]),
            );
            assert!(
                policy.allows_tool("exec", Some(ToolEffect::Process)),
                "{state:?}"
            );
            assert!(
                policy.allows_tool("apply_patch", Some(ToolEffect::WorkspaceWrite)),
                "{state:?}"
            );
            assert!(
                policy.allows_tool("git_commit", Some(ToolEffect::BranchControl)),
                "{state:?}"
            );
            assert!(!policy.visible_tools.contains("plan_exit"), "{state:?}");
        }
    }

    #[test]
    fn only_planning_checkpoints_require_task_transition() {
        let root = snapshot("planner", true);
        for state in [
            TaskRunStateKind::Planning,
            TaskRunStateKind::EditingDocuments,
        ] {
            let policy = studio_execution_policy(
                &root,
                StudioPolicyContext {
                    mode: StudioMode::Task,
                    task_phase: Some(state),
                },
                ToolVisibilitySet::default(),
            );
            assert_eq!(policy.finalization, required_tool("task_transition"));
        }
    }

    fn snapshot(role: &str, root: bool) -> AgentSnapshot {
        let id = ThreadId::new("agent-test").unwrap();
        AgentSnapshot {
            identity: AgentIdentity {
                id,
                parent_id: (!root).then(|| ThreadId::new("agent-root").unwrap()),
                role: AgentRoleId::new(role).unwrap(),
                depth: u32::from(!root),
            },
            state: AgentState::idle(),
            pending_inputs: 0,
            progress: None,
            last_turn: None,
            revision: 0,
            event_sequence: 0,
            updated_at: 0,
        }
    }
}

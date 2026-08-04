use std::collections::BTreeSet;

use pl_core::{
    AgentAccessPolicy, AgentExecutionPolicy, AgentRoleId, AgentSnapshot, AgentTargetSelector,
    ToolEffect, ToolEffectSet, ToolVisibilitySet, TurnFinalizationPolicy,
};

use crate::StudioMode;
use crate::studio::task_coordinator::TaskRunPhase;

const COLLABORATION_CONTROL_TOOLS: [&str; 7] = [
    "spawn_agent",
    "send_message",
    "interrupt_agent",
    "list_agents",
    "wait_agents",
    "read_agent_session",
    "close_agent",
];
const CONFLICT_TOOLS: [&str; 6] = [
    "merge_status",
    "merge_conflict_files",
    "merge_resolve",
    "merge_verify",
    "merge_continue",
    "merge_abort",
];

#[derive(Debug, Clone, Copy)]
pub(super) struct StudioPolicyContext {
    pub(super) mode: StudioMode,
    pub(super) task_phase: Option<TaskRunPhase>,
}

pub(super) fn studio_execution_policy(
    snapshot: &AgentSnapshot,
    context: StudioPolicyContext,
    mut visible_tools: ToolVisibilitySet,
) -> AgentExecutionPolicy {
    let is_root = snapshot.identity.parent_id.is_none();
    let spawn_roles = spawn_roles(is_root, context.mode);
    let collaboration = if is_root {
        AgentAccessPolicy {
            spawn_roles,
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
    if context.task_phase != Some(TaskRunPhase::ResolvingConflict) {
        visible_tools = without_tools(visible_tools, &CONFLICT_TOOLS);
    }
    AgentExecutionPolicy {
        visible_tools,
        allowed_effects: ToolEffectSet::from_effects(allowed_effects(snapshot, context)),
        collaboration,
        finalization: finalization(snapshot, context),
    }
}

fn spawn_roles(is_root: bool, mode: StudioMode) -> BTreeSet<AgentRoleId> {
    if !is_root {
        return BTreeSet::new();
    }
    let roles: &[&str] = match mode {
        StudioMode::Simple => &["explorer"],
        StudioMode::Task => &["explorer"],
    };
    roles
        .iter()
        .map(|role| AgentRoleId::new(*role).expect("Studio 内置角色必须有效"))
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

fn allowed_effects(snapshot: &AgentSnapshot, context: StudioPolicyContext) -> Vec<ToolEffect> {
    if snapshot.identity.parent_id.is_none() {
        return match context.mode {
            StudioMode::Simple => vec![
                ToolEffect::Read,
                ToolEffect::WorkspaceWrite,
                ToolEffect::Process,
                ToolEffect::AgentControl,
                ToolEffect::BranchControl,
            ],
            StudioMode::Task => {
                let mut effects = vec![
                    ToolEffect::Read,
                    ToolEffect::AgentControl,
                    ToolEffect::BranchControl,
                ];
                if context.task_phase == Some(TaskRunPhase::ResolvingConflict) {
                    effects.push(ToolEffect::ConflictWrite);
                }
                effects
            }
        };
    }
    match snapshot.identity.role.as_str() {
        "explorer" | "reviewer" => vec![ToolEffect::Read, ToolEffect::AgentControl],
        "executor" => vec![
            ToolEffect::Read,
            ToolEffect::WorkspaceWrite,
            ToolEffect::Process,
            ToolEffect::AgentControl,
            ToolEffect::BranchControl,
        ],
        "planner" => vec![ToolEffect::Read],
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
            "explorer" | "planner" => TurnFinalizationPolicy::Direct,
            _ => TurnFinalizationPolicy::Direct,
        };
    }
    match context.task_phase {
        None | Some(TaskRunPhase::Planning | TaskRunPhase::PendingConfirmation) => {
            required_tool("plan_exit")
        }
        Some(TaskRunPhase::Reviewing) => required_tool("task_complete"),
        Some(
            TaskRunPhase::DesignUpdating
            | TaskRunPhase::Implementing
            | TaskRunPhase::Merging
            | TaskRunPhase::ResolvingConflict
            | TaskRunPhase::Reworking
            | TaskRunPhase::Stopping
            | TaskRunPhase::Completed
            | TaskRunPhase::Blocked
            | TaskRunPhase::Failed
            | TaskRunPhase::Cancelled,
        ) => TurnFinalizationPolicy::Direct,
    }
}

fn required_tool(name: &str) -> TurnFinalizationPolicy {
    TurnFinalizationPolicy::RequiredTool {
        name: name.to_string(),
    }
}

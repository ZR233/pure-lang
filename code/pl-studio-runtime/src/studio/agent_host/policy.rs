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
const PLANNER_GIT_MUTATION_TOOLS: [&str; 5] = [
    "git_fetch",
    "git_push",
    "git_sync_default_branch",
    "git_branch",
    "git_commit",
];
const PLAN_EXIT_TOOL: [&str; 1] = ["plan_exit"];

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
    if !may_submit_plan(snapshot, context) {
        visible_tools = without_tools(visible_tools, &PLAN_EXIT_TOOL);
    }
    if is_root
        && context.mode == StudioMode::Task
        && context.task_phase != Some(TaskRunPhase::Merging)
    {
        visible_tools = without_tools(visible_tools, &PLANNER_GIT_MUTATION_TOOLS);
    }
    AgentExecutionPolicy {
        visible_tools,
        allowed_effects: ToolEffectSet::from_effects(allowed_effects(snapshot, context)),
        collaboration,
        finalization: finalization(snapshot, context),
    }
}

fn may_submit_plan(snapshot: &AgentSnapshot, context: StudioPolicyContext) -> bool {
    snapshot.identity.parent_id.is_none()
        && snapshot.identity.role.as_str() == "planner"
        && context.mode == StudioMode::Task
        && matches!(
            context.task_phase,
            None | Some(TaskRunPhase::Planning | TaskRunPhase::PendingConfirmation)
        )
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
                    ToolEffect::Process,
                    ToolEffect::AgentControl,
                    ToolEffect::BranchControl,
                ];
                if context.task_phase == Some(TaskRunPhase::Merging) {
                    effects.push(ToolEffect::WorkspaceWrite);
                }
                effects
            }
        };
    }
    match snapshot.identity.role.as_str() {
        "explorer" | "reviewer" => vec![
            ToolEffect::Read,
            ToolEffect::Process,
            ToolEffect::AgentControl,
        ],
        "executor" => vec![
            ToolEffect::Read,
            ToolEffect::WorkspaceWrite,
            ToolEffect::Process,
            ToolEffect::AgentControl,
            ToolEffect::BranchControl,
        ],
        "planner" => vec![ToolEffect::Read, ToolEffect::Process],
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
        None | Some(TaskRunPhase::Planning | TaskRunPhase::PendingConfirmation)
            if may_submit_plan(snapshot, context) =>
        {
            required_tool("plan_exit")
        }
        None | Some(TaskRunPhase::Planning | TaskRunPhase::PendingConfirmation) => {
            TurnFinalizationPolicy::Direct
        }
        Some(TaskRunPhase::Reviewing) => required_tool("task_complete"),
        Some(
            TaskRunPhase::DesignUpdating
            | TaskRunPhase::Implementing
            | TaskRunPhase::Merging
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

#[cfg(test)]
mod tests {
    use pl_core::{
        AgentActivityState, AgentId, AgentIdentity, AgentLifecycleState, AgentRoleId, AgentSnapshot,
    };

    use super::*;

    #[test]
    fn process_effect_is_available_to_every_studio_role_and_task_phase() {
        let visible = ToolVisibilitySet::from_tool_names(["exec", "write_stdin"]);
        let root = snapshot("planner", true);
        let task_phases = [
            None,
            Some(TaskRunPhase::Planning),
            Some(TaskRunPhase::PendingConfirmation),
            Some(TaskRunPhase::DesignUpdating),
            Some(TaskRunPhase::Implementing),
            Some(TaskRunPhase::Merging),
            Some(TaskRunPhase::Reviewing),
            Some(TaskRunPhase::Reworking),
        ];
        for task_phase in task_phases {
            let policy = studio_execution_policy(
                &root,
                StudioPolicyContext {
                    mode: StudioMode::Task,
                    task_phase,
                },
                visible.clone(),
            );
            assert!(policy.allowed_effects.contains(ToolEffect::Process));
            assert!(policy.visible_tools.contains("exec"));
            assert!(policy.visible_tools.contains("write_stdin"));
        }

        for role in ["planner", "explorer", "reviewer", "executor"] {
            let policy = studio_execution_policy(
                &snapshot(role, false),
                StudioPolicyContext {
                    mode: StudioMode::Task,
                    task_phase: Some(TaskRunPhase::Implementing),
                },
                visible.clone(),
            );
            assert!(
                policy.allowed_effects.contains(ToolEffect::Process),
                "{role} should allow Process"
            );
            assert!(policy.visible_tools.contains("exec"));
            assert!(policy.visible_tools.contains("write_stdin"));
        }
    }

    #[test]
    fn plan_exit_is_only_visible_to_task_root_planner_during_planning() {
        let visible = ToolVisibilitySet::from_tool_names(["plan_exit", "exec"]);
        for role in ["planner", "explorer", "reviewer", "executor"] {
            let policy = studio_execution_policy(
                &snapshot(role, false),
                StudioPolicyContext {
                    mode: StudioMode::Task,
                    task_phase: Some(TaskRunPhase::Planning),
                },
                visible.clone(),
            );
            assert!(!policy.visible_tools.contains("plan_exit"), "{role}");
            assert!(
                !policy.allows_tool("plan_exit", Some(ToolEffect::Read)),
                "{role}"
            );
        }

        for task_phase in [
            None,
            Some(TaskRunPhase::Planning),
            Some(TaskRunPhase::PendingConfirmation),
        ] {
            let planning = studio_execution_policy(
                &snapshot("planner", true),
                StudioPolicyContext {
                    mode: StudioMode::Task,
                    task_phase,
                },
                visible.clone(),
            );
            assert!(planning.visible_tools.contains("plan_exit"));
            assert!(planning.allows_tool("plan_exit", Some(ToolEffect::Read)));
            assert_eq!(planning.finalization, required_tool("plan_exit"));
        }

        let reviewing = studio_execution_policy(
            &snapshot("planner", true),
            StudioPolicyContext {
                mode: StudioMode::Task,
                task_phase: Some(TaskRunPhase::Reviewing),
            },
            visible.clone(),
        );
        assert!(!reviewing.visible_tools.contains("plan_exit"));

        let simple = studio_execution_policy(
            &snapshot("executor", true),
            StudioPolicyContext {
                mode: StudioMode::Simple,
                task_phase: None,
            },
            visible,
        );
        assert!(!simple.visible_tools.contains("plan_exit"));
    }

    fn snapshot(role: &str, root: bool) -> AgentSnapshot {
        AgentSnapshot {
            identity: AgentIdentity {
                id: AgentId::new(format!("agent-{role}")).unwrap(),
                parent_id: (!root).then(|| AgentId::new("agent-root").unwrap()),
                role: AgentRoleId::new(role).unwrap(),
                depth: u32::from(!root),
            },
            lifecycle: AgentLifecycleState::Active,
            activity: AgentActivityState::Idle,
            active_turn_id: None,
            pending_inputs: 0,
            progress: None,
            last_turn: None,
            revision: 0,
            event_sequence: 0,
            updated_at: 0,
        }
    }
}

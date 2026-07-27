use std::collections::BTreeSet;

use pl_core::{
    AgentAccessPolicy, AgentExecutionPolicy, AgentRoleId, AgentSnapshot, AgentTargetSelector,
    ToolEffect, ToolEffectSet, ToolVisibilitySet, TurnFinalizationPolicy,
};

use crate::StudioMode;
use crate::studio::task_coordinator::TaskRunPhase;

const COLLABORATION_TOOLS: [&str; 4] = ["spawn_agent", "send_input", "list_agents", "close_agent"];
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
        visible_tools.extend_tool_names(COLLABORATION_TOOLS);
    } else {
        visible_tools = without_tools(visible_tools, &COLLABORATION_TOOLS);
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
        "explorer" | "reviewer" => vec![ToolEffect::Read],
        "executor" => vec![
            ToolEffect::Read,
            ToolEffect::WorkspaceWrite,
            ToolEffect::Process,
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
            "executor" => required_tool("submit_delivery"),
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

#[cfg(test)]
mod tests {
    use pl_core::{AgentActivityState, AgentId, AgentIdentity, AgentLifecycleState, AgentRoleId};

    use super::*;

    #[test]
    fn simple_root_only_spawns_explorer() {
        let policy = policy(
            root("executor"),
            StudioMode::Simple,
            None,
            ["apply_patch", "spawn_agent", "merge_resolve"],
        );

        assert_eq!(
            policy.collaboration.spawn_roles,
            BTreeSet::from([AgentRoleId::new("explorer").unwrap()])
        );
        assert!(policy.visible_tools.contains("spawn_agent"));
        assert!(!policy.visible_tools.contains("merge_resolve"));
        assert!(policy.allowed_effects.contains(ToolEffect::WorkspaceWrite));
        assert_eq!(policy.finalization, TurnFinalizationPolicy::Direct);
    }

    #[test]
    fn task_children_have_role_finalizers_without_agent_control() {
        for (role, tool) in [("executor", "submit_delivery"), ("reviewer", "review_exit")] {
            let policy = policy(
                child(role),
                StudioMode::Task,
                Some(TaskRunPhase::Implementing),
                [tool, "spawn_agent"],
            );

            assert!(policy.collaboration.spawn_roles.is_empty());
            assert!(!policy.visible_tools.contains("spawn_agent"));
            assert!(!policy.allowed_effects.contains(ToolEffect::AgentControl));
            assert_eq!(policy.finalization, required_tool(tool));
        }
    }

    #[test]
    fn task_root_enables_conflict_tools_only_in_conflict_phase() {
        let ordinary = policy(
            root("planner"),
            StudioMode::Task,
            Some(TaskRunPhase::Implementing),
            ["merge_resolve"],
        );
        let conflict = policy(
            root("planner"),
            StudioMode::Task,
            Some(TaskRunPhase::ResolvingConflict),
            ["merge_resolve"],
        );

        assert!(!ordinary.visible_tools.contains("merge_resolve"));
        assert!(!ordinary.allowed_effects.contains(ToolEffect::ConflictWrite));
        assert!(conflict.visible_tools.contains("merge_resolve"));
        assert!(conflict.allowed_effects.contains(ToolEffect::ConflictWrite));
    }

    #[test]
    fn task_root_generic_spawn_only_allows_explorer() {
        let policy = policy(
            root("planner"),
            StudioMode::Task,
            Some(TaskRunPhase::Implementing),
            ["spawn_agent", "task_spawn_executor"],
        );

        assert_eq!(
            policy.collaboration.spawn_roles,
            BTreeSet::from([AgentRoleId::new("explorer").unwrap()])
        );
        assert!(policy.visible_tools.contains("spawn_agent"));
        assert!(policy.visible_tools.contains("task_spawn_executor"));
    }

    #[test]
    fn task_root_finalizer_follows_product_phase() {
        assert_eq!(
            policy(root("planner"), StudioMode::Task, None, ["plan_exit"]).finalization,
            required_tool("plan_exit")
        );
        assert_eq!(
            policy(
                root("planner"),
                StudioMode::Task,
                Some(TaskRunPhase::Reviewing),
                ["task_complete"],
            )
            .finalization,
            required_tool("task_complete")
        );
    }

    fn policy<const N: usize>(
        snapshot: AgentSnapshot,
        mode: StudioMode,
        task_phase: Option<TaskRunPhase>,
        tools: [&str; N],
    ) -> AgentExecutionPolicy {
        studio_execution_policy(
            &snapshot,
            StudioPolicyContext { mode, task_phase },
            ToolVisibilitySet::from_tool_names(tools),
        )
    }

    fn root(role: &str) -> AgentSnapshot {
        snapshot(role, None, 0)
    }

    fn child(role: &str) -> AgentSnapshot {
        snapshot(role, Some(AgentId::new("root").unwrap()), 1)
    }

    fn snapshot(role: &str, parent_id: Option<AgentId>, depth: u32) -> AgentSnapshot {
        AgentSnapshot {
            identity: AgentIdentity {
                id: AgentId::new(format!("{role}-{depth}")).unwrap(),
                parent_id,
                role: AgentRoleId::new(role).unwrap(),
                depth,
            },
            wake_policy: pl_core::AgentWakePolicy::RuntimeTerminal,
            lifecycle: AgentLifecycleState::Active,
            activity: AgentActivityState::Idle,
            active_turn_id: None,
            active_session_id: None,
            pending_inputs: 0,
            last_turn: None,
            revision: 1,
            event_sequence: 1,
            updated_at: 1,
        }
    }
}

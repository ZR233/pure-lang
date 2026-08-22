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
    pub(super) task_phase: Option<TaskRunStateKind>,
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
        && context.task_phase != Some(TaskRunStateKind::Merging)
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

/// plan_exit 只属于 Task root；root 角色由 mode 派生，无需比对 identity.role。
fn may_submit_plan(snapshot: &AgentSnapshot, context: StudioPolicyContext) -> bool {
    snapshot.identity.parent_id.is_none()
        && context.mode == StudioMode::Task
        && context.task_phase.is_none()
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
                if context
                    .task_phase
                    .is_some_and(TaskRunStateKind::allows_planner_workspace_mutation)
                {
                    effects.push(ToolEffect::Process);
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
        None if may_submit_plan(snapshot, context) => required_tool("plan_exit"),
        None => TurnFinalizationPolicy::Direct,
        Some(TaskRunStateKind::DesignUpdating) => required_tool("task_finalize_design"),
        Some(TaskRunStateKind::Reviewing) => required_tool("task_complete"),
        Some(
            TaskRunStateKind::Implementing
            | TaskRunStateKind::Merging
            | TaskRunStateKind::Reworking
            | TaskRunStateKind::Stopping
            | TaskRunStateKind::Completed
            | TaskRunStateKind::Blocked
            | TaskRunStateKind::Failed
            | TaskRunStateKind::Cancelled,
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
    use pl_core::{AgentIdentity, AgentRoleId, AgentSnapshot, AgentState, ThreadId};

    use super::*;

    #[test]
    fn task_root_process_effect_is_available_during_design_and_merge() {
        let visible = ToolVisibilitySet::from_tool_names(["exec", "write_stdin"]);
        let root = snapshot("planner", true);
        let task_phases = [
            (None, false),
            (Some(TaskRunStateKind::DesignUpdating), true),
            (Some(TaskRunStateKind::Implementing), false),
            (Some(TaskRunStateKind::Merging), true),
            (Some(TaskRunStateKind::Reviewing), false),
            (Some(TaskRunStateKind::Reworking), false),
            (Some(TaskRunStateKind::Stopping), false),
            (Some(TaskRunStateKind::Blocked), false),
            (Some(TaskRunStateKind::Completed), false),
            (Some(TaskRunStateKind::Failed), false),
            (Some(TaskRunStateKind::Cancelled), false),
        ];
        for (task_phase, process_allowed) in task_phases {
            let policy = studio_execution_policy(
                &root,
                StudioPolicyContext {
                    mode: StudioMode::Task,
                    task_phase,
                },
                visible.clone(),
            );
            assert_eq!(
                policy.allowed_effects.contains(ToolEffect::Process),
                process_allowed,
                "unexpected Process policy for {task_phase:?}"
            );
            assert_eq!(
                policy.allows_tool("exec", Some(ToolEffect::Process)),
                process_allowed,
                "unexpected exec policy for {task_phase:?}"
            );
            assert_eq!(
                policy.allows_tool("write_stdin", Some(ToolEffect::Process)),
                process_allowed,
                "unexpected write_stdin policy for {task_phase:?}"
            );
        }
    }

    #[test]
    fn process_effect_remains_available_to_child_roles() {
        let visible = ToolVisibilitySet::from_tool_names(["exec", "write_stdin"]);
        for role in ["planner", "explorer", "reviewer", "executor"] {
            let policy = studio_execution_policy(
                &snapshot(role, false),
                StudioPolicyContext {
                    mode: StudioMode::Task,
                    task_phase: Some(TaskRunStateKind::Implementing),
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
    fn lifecycle_finalizers_are_visible_only_in_their_own_stage() {
        let visible = ToolVisibilitySet::from_tool_names(["plan_exit", "exec"]);
        for role in ["planner", "explorer", "reviewer", "executor"] {
            let policy = studio_execution_policy(
                &snapshot(role, false),
                StudioPolicyContext {
                    mode: StudioMode::Task,
                    task_phase: None,
                },
                visible.clone(),
            );
            assert!(!policy.visible_tools.contains("plan_exit"), "{role}");
            assert!(
                !policy.allows_tool("plan_exit", Some(ToolEffect::Read)),
                "{role}"
            );
        }

        let planning = studio_execution_policy(
            &snapshot("planner", true),
            StudioPolicyContext {
                mode: StudioMode::Task,
                task_phase: None,
            },
            visible.clone(),
        );
        assert!(planning.visible_tools.contains("plan_exit"));
        assert!(planning.allows_tool("plan_exit", Some(ToolEffect::Read)));
        assert_eq!(planning.finalization, required_tool("plan_exit"));

        let reviewing = studio_execution_policy(
            &snapshot("planner", true),
            StudioPolicyContext {
                mode: StudioMode::Task,
                task_phase: Some(TaskRunStateKind::Reviewing),
            },
            visible.clone(),
        );
        assert!(!reviewing.visible_tools.contains("plan_exit"));

        let design_updating = studio_execution_policy(
            &snapshot("planner", true),
            StudioPolicyContext {
                mode: StudioMode::Task,
                task_phase: Some(TaskRunStateKind::DesignUpdating),
            },
            ToolVisibilitySet::from_tool_names(["task_finalize_design"]),
        );
        assert_eq!(
            design_updating.finalization,
            required_tool("task_finalize_design")
        );

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
                id: ThreadId::new(format!("agent-{role}")).unwrap(),
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

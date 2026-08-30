use pl_core::{
    AgentAccessPolicy, AgentExecutionPolicy, AgentRoleId, AgentSnapshot, AgentTargetSelector,
    ToolEffect, ToolEffectSet, TurnFinalizationPolicy,
};

/// Studio 不再按 Simple/Task 或工作流阶段裁剪普通工具能力。
///
/// Mode Skill 和 `workflow_state` 只约束模型接下来应该完成的阶段；文件、命令、Git、
/// Agent 与最终回复能力始终由统一的根会话策略提供。
pub(super) fn studio_execution_policy(
    snapshot: &AgentSnapshot,
    profiles: &[pl_protocol::AgentProfileSnapshot],
) -> AgentExecutionPolicy {
    let collaboration = if snapshot.identity.parent_id.is_none() {
        AgentAccessPolicy {
            spawn_roles: profiles
                .iter()
                .filter_map(|profile| AgentRoleId::new(profile.profile_id.clone()).ok())
                .collect(),
            list_targets: AgentTargetSelector::Tree,
            message_targets: AgentTargetSelector::Tree,
            close_targets: AgentTargetSelector::Tree,
        }
    } else {
        AgentAccessPolicy::default()
    };
    AgentExecutionPolicy {
        allowed_effects: ToolEffectSet::from_effects([
            ToolEffect::Read,
            ToolEffect::WorkspaceWrite,
            ToolEffect::Process,
            ToolEffect::AgentControl,
            ToolEffect::BranchControl,
        ]),
        collaboration,
        finalization: TurnFinalizationPolicy::Direct,
    }
}

#[cfg(test)]
mod tests {
    use pl_core::{AgentIdentity, AgentState, ThreadId};

    use super::*;

    #[test]
    fn every_stage_keeps_all_ordinary_effects_and_direct_finalization() {
        let profiles = ["explorer", "planner", "executor", "reviewer"]
            .into_iter()
            .map(profile)
            .collect::<Vec<_>>();
        let policy = studio_execution_policy(&snapshot("planner", true), &profiles);
        for effect in [
            ToolEffect::Read,
            ToolEffect::WorkspaceWrite,
            ToolEffect::Process,
            ToolEffect::AgentControl,
            ToolEffect::BranchControl,
        ] {
            assert!(policy.allows_effect(Some(effect)), "{effect:?}");
        }
        assert_eq!(policy.finalization, TurnFinalizationPolicy::Direct);
        assert_eq!(
            policy
                .collaboration
                .spawn_roles
                .iter()
                .map(AgentRoleId::as_str)
                .collect::<Vec<_>>(),
            ["executor", "explorer", "planner", "reviewer"]
        );
    }

    fn profile(profile_id: &str) -> pl_protocol::AgentProfileSnapshot {
        pl_protocol::AgentProfileSnapshot {
            profile_id: profile_id.to_string(),
            display_name: profile_id.to_string(),
            description: String::new(),
            when_to_use: String::new(),
            system_instructions: String::new(),
            provider_id: "provider".to_string(),
            model: "model".to_string(),
            effort: None,
            source: "test".to_string(),
            revision: "1".to_string(),
            content_hash: "hash".to_string(),
            system: true,
            enabled: true,
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

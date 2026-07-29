use std::collections::BTreeSet;

use crate::{AgentRoleId, ToolEffect, ToolVisibilitySet};

use super::{AgentActivityState, AgentId, AgentLifecycleState, AgentWakeContext, AgentWakeReason};

const LIST_AGENTS_TOOL: &str = "list_agents";
const SEND_INPUT_TOOL: &str = "send_input";
const CLOSE_AGENT_TOOL: &str = "close_agent";

/// 协作操作可访问的 agent 集合。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AgentTargetSelector {
    /// 不允许访问任何 agent。
    #[default]
    None,
    /// 允许访问调用方所属的父子树。
    Tree,
    /// 只允许访问显式列出的 agent。
    Explicit(BTreeSet<AgentId>),
    /// 允许访问 runtime 内全部 agent。
    All,
}

/// 动态角色与协作目标授权。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentAccessPolicy {
    pub spawn_roles: BTreeSet<AgentRoleId>,
    pub list_targets: AgentTargetSelector,
    pub message_targets: AgentTargetSelector,
    pub close_targets: AgentTargetSelector,
}

/// 允许执行的工具 effect 集合。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolEffectSet {
    effects: Vec<ToolEffect>,
}

impl ToolEffectSet {
    /// 从明确的 effect 列表创建集合。
    pub fn from_effects(effects: impl IntoIterator<Item = ToolEffect>) -> Self {
        let mut unique = Vec::new();
        for effect in effects {
            if !unique.contains(&effect) {
                unique.push(effect);
            }
        }
        Self { effects: unique }
    }

    /// 判断指定 effect 是否允许。
    pub fn contains(&self, effect: ToolEffect) -> bool {
        self.effects.contains(&effect)
    }
}

impl AgentExecutionPolicy {
    /// 判断工具名称和声明 effect 是否同时被策略允许。
    pub fn allows_tool(&self, name: &str, effect: Option<ToolEffect>) -> bool {
        self.visible_tools.contains(name)
            && effect.is_none_or(|effect| self.allowed_effects.contains(effect))
    }

    /// 根据 typed agent wake context 收窄本轮权限。
    ///
    /// inactivity wake 只能诊断 canonical 状态。仍有 active/pending turn 的健康目标会同时
    /// 从协作 schema 与 dispatch 授权中移除；纯诊断轮不会获得其他任务控制能力。
    pub fn constrain_for_agent_wake(&mut self, context: &AgentWakeContext) {
        let AgentWakeReason::InactivityDiagnostic {
            timed_out_agent_ids,
        } = &context.wake_reason
        else {
            return;
        };
        let timed_out_agent_ids = timed_out_agent_ids.iter().collect::<BTreeSet<_>>();
        let protected = context
            .current_agent_states
            .iter()
            .filter(|snapshot| {
                timed_out_agent_ids.contains(&snapshot.identity.id)
                    && snapshot.lifecycle == AgentLifecycleState::Active
                    && matches!(
                        snapshot.activity,
                        AgentActivityState::Queued
                            | AgentActivityState::Running
                            | AgentActivityState::WaitingTool
                            | AgentActivityState::WaitingInteraction
                    )
                    && ((snapshot.active_turn_id.is_some() && snapshot.active_session_id.is_some())
                        || snapshot.pending_trigger_inputs > 0)
            })
            .map(|snapshot| snapshot.identity.id.clone())
            .collect::<BTreeSet<_>>();
        let unprotected = context
            .current_agent_states
            .iter()
            .map(|snapshot| snapshot.identity.id.clone())
            .filter(|agent_id| !protected.contains(agent_id))
            .collect::<BTreeSet<_>>();

        self.collaboration.message_targets =
            constrained_targets(&self.collaboration.message_targets, &unprotected);
        self.collaboration.close_targets =
            constrained_targets(&self.collaboration.close_targets, &unprotected);

        if context.diagnostic_only {
            self.collaboration.spawn_roles.clear();
            let message_allowed = selector_has_targets(&self.collaboration.message_targets);
            let close_allowed = selector_has_targets(&self.collaboration.close_targets);
            self.visible_tools = ToolVisibilitySet::from_tool_names(
                self.visible_tools
                    .iter()
                    .filter(|name| {
                        name.as_str() == LIST_AGENTS_TOOL
                            || (message_allowed && name.as_str() == SEND_INPUT_TOOL)
                            || (close_allowed && name.as_str() == CLOSE_AGENT_TOOL)
                    })
                    .cloned(),
            );
            self.allowed_effects =
                ToolEffectSet::from_effects([ToolEffect::AgentControl, ToolEffect::Read]);
            self.finalization = TurnFinalizationPolicy::Direct;
        }
    }
}

fn constrained_targets(
    selector: &AgentTargetSelector,
    unprotected: &BTreeSet<AgentId>,
) -> AgentTargetSelector {
    match selector {
        AgentTargetSelector::None => AgentTargetSelector::None,
        AgentTargetSelector::Explicit(agent_ids) => AgentTargetSelector::Explicit(
            agent_ids
                .intersection(unprotected)
                .cloned()
                .collect::<BTreeSet<_>>(),
        ),
        AgentTargetSelector::Tree | AgentTargetSelector::All => {
            AgentTargetSelector::Explicit(unprotected.clone())
        }
    }
}

fn selector_has_targets(selector: &AgentTargetSelector) -> bool {
    match selector {
        AgentTargetSelector::None => false,
        AgentTargetSelector::Explicit(agent_ids) => !agent_ids.is_empty(),
        AgentTargetSelector::Tree | AgentTargetSelector::All => true,
    }
}

/// turn 如何完成的产品无关策略。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum TurnFinalizationPolicy {
    /// 模型最终文本可直接完成 turn。
    #[default]
    Direct,
    /// 模型必须调用指定工具完成 turn。
    RequiredTool { name: String },
}

/// 宿主为一次 agent turn 编译出的完整执行策略。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentExecutionPolicy {
    pub visible_tools: ToolVisibilitySet,
    pub allowed_effects: ToolEffectSet,
    pub collaboration: AgentAccessPolicy,
    pub finalization: TurnFinalizationPolicy,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        AgentIdentity, AgentRoleId, AgentSnapshot, MailboxDeliveryPhase, SessionId, TurnId,
    };

    #[test]
    fn diagnostic_wake_protects_only_healthy_timed_out_targets() {
        let healthy = snapshot(
            "healthy",
            AgentActivityState::Running,
            Some("healthy-turn"),
            0,
        );
        let abnormal = snapshot("abnormal", AgentActivityState::Idle, None, 0);
        let mut policy = policy(["list_agents", "send_input", "close_agent", "task_stop"]);
        let context = diagnostic_context(
            vec![healthy.clone(), abnormal.clone()],
            vec![healthy.identity.id.clone(), abnormal.identity.id.clone()],
            true,
        );

        policy.constrain_for_agent_wake(&context);

        assert_eq!(
            policy.collaboration.message_targets,
            AgentTargetSelector::Explicit(BTreeSet::from([abnormal.identity.id.clone()]))
        );
        assert_eq!(
            policy.collaboration.close_targets,
            AgentTargetSelector::Explicit(BTreeSet::from([abnormal.identity.id]))
        );
        assert!(policy.visible_tools.contains("list_agents"));
        assert!(policy.visible_tools.contains("send_input"));
        assert!(policy.visible_tools.contains("close_agent"));
        assert!(!policy.visible_tools.contains("task_stop"));
        assert!(policy.collaboration.spawn_roles.is_empty());
        assert_eq!(policy.finalization, TurnFinalizationPolicy::Direct);
    }

    #[test]
    fn mixed_inactivity_batch_keeps_actions_but_not_healthy_target_control() {
        let healthy = snapshot(
            "healthy",
            AgentActivityState::WaitingTool,
            Some("healthy-turn"),
            0,
        );
        let actionable = snapshot("actionable", AgentActivityState::Idle, None, 0);
        let mut policy = policy(["list_agents", "send_input", "close_agent", "task_stop"]);
        let context = diagnostic_context(
            vec![healthy.clone(), actionable.clone()],
            vec![healthy.identity.id.clone(), actionable.identity.id.clone()],
            false,
        );

        policy.constrain_for_agent_wake(&context);

        assert!(policy.visible_tools.contains("task_stop"));
        assert_eq!(
            policy.collaboration.message_targets,
            AgentTargetSelector::Explicit(BTreeSet::from([actionable.identity.id]))
        );
    }

    #[test]
    fn queued_target_requires_a_pending_trigger_turn_to_be_protected() {
        let queued = snapshot("queued", AgentActivityState::Queued, None, 1);
        let stale = snapshot("stale", AgentActivityState::Queued, None, 0);
        let mut policy = policy(["list_agents", "send_input"]);
        let context = diagnostic_context(
            vec![queued.clone(), stale.clone()],
            vec![queued.identity.id.clone(), stale.identity.id.clone()],
            true,
        );

        policy.constrain_for_agent_wake(&context);

        assert_eq!(
            policy.collaboration.message_targets,
            AgentTargetSelector::Explicit(BTreeSet::from([stale.identity.id]))
        );
    }

    fn policy<const N: usize>(tools: [&str; N]) -> AgentExecutionPolicy {
        AgentExecutionPolicy {
            visible_tools: ToolVisibilitySet::from_tool_names(tools),
            allowed_effects: ToolEffectSet::from_effects([
                ToolEffect::Read,
                ToolEffect::AgentControl,
                ToolEffect::BranchControl,
            ]),
            collaboration: AgentAccessPolicy {
                spawn_roles: BTreeSet::from([AgentRoleId::new("explorer").unwrap()]),
                list_targets: AgentTargetSelector::Tree,
                message_targets: AgentTargetSelector::Tree,
                close_targets: AgentTargetSelector::Tree,
            },
            finalization: TurnFinalizationPolicy::RequiredTool {
                name: "task_complete".to_string(),
            },
        }
    }

    fn diagnostic_context(
        current_agent_states: Vec<AgentSnapshot>,
        timed_out_agent_ids: Vec<AgentId>,
        diagnostic_only: bool,
    ) -> AgentWakeContext {
        AgentWakeContext {
            current_agent_states,
            wake_reason: AgentWakeReason::InactivityDiagnostic {
                timed_out_agent_ids,
            },
            last_activity_at: BTreeMap::new(),
            recent_progress: Vec::new(),
            latest_commentary: None,
            terminal_facts: Vec::new(),
            user_stop_requested: false,
            signal_revision: 1,
            lag_reconciled: false,
            diagnostic_only,
        }
    }

    fn snapshot(
        id: &str,
        activity: AgentActivityState,
        turn_id: Option<&str>,
        pending_trigger_inputs: usize,
    ) -> AgentSnapshot {
        AgentSnapshot {
            identity: AgentIdentity {
                id: AgentId::new(id).unwrap(),
                parent_id: Some(AgentId::new("root").unwrap()),
                role: AgentRoleId::new("executor").unwrap(),
                depth: 1,
            },
            wake_policy: super::super::AgentWakePolicy::ProductGated,
            lifecycle: AgentLifecycleState::Active,
            activity,
            active_turn_id: turn_id.map(|turn_id| TurnId::new(turn_id).unwrap()),
            active_session_id: turn_id.map(|_| SessionId::new(format!("{id}-session")).unwrap()),
            pending_inputs: pending_trigger_inputs,
            pending_trigger_inputs,
            mailbox_delivery_phase: MailboxDeliveryPhase::CurrentTurn,
            dispatch_generation: 3,
            last_turn: None,
            revision: 5,
            event_sequence: 7,
            updated_at: 11,
        }
    }
}

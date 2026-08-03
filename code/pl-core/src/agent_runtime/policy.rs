use std::collections::BTreeSet;

use crate::{AgentRoleId, ToolEffect, ToolVisibilitySet};

use super::AgentId;

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

impl AgentExecutionPolicy {
    /// 判断工具名称和声明 effect 是否同时被策略允许。
    pub fn allows_tool(&self, name: &str, effect: Option<ToolEffect>) -> bool {
        self.visible_tools.contains(name)
            && effect.is_none_or(|effect| self.allowed_effects.contains(effect))
    }
}

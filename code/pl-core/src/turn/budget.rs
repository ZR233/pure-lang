use std::time::Instant;

use pl_protocol::{BudgetLimitKind, BudgetUsage};
use serde::{Deserialize, Serialize};

/// Agent 树结构限制常量。
pub const AGENT_MAX_COUNT: usize = 16;
pub const AGENT_MAX_DEPTH: u32 = 3;
/// 默认 wall-clock 安全上限（30 分钟），参考 Codex 的 agent_job_max_runtime_seconds。
pub const DEFAULT_WALL_CLOCK_MS: u64 = 1_800_000;

/// 单轮 wall-clock 安全预算。
///
/// 参考 Codex 的设计：不限制 model step / tool call 迭代次数，
/// 让模型自己决定何时完成（通过返回无 tool call 的 content-only 响应）。
/// 仅保留 wall-clock 作为防止无限运行的安全兜底。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnBudget {
    /// Wall-clock 安全上限（毫秒）。超时后 turn 将被终止。
    pub wall_clock_ms: u64,
}

impl TurnBudget {
    pub fn new(wall_clock_ms: u64) -> Self {
        Self { wall_clock_ms }
    }
}

impl Default for TurnBudget {
    fn default() -> Self {
        Self {
            wall_clock_ms: DEFAULT_WALL_CLOCK_MS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BudgetLimit {
    pub kind: BudgetLimitKind,
    pub usage: BudgetUsage,
}

/// 运行时用量追踪器。
///
/// 仅追踪 wall-clock 安全上限；model step / tool call 仅做可观测性计数，不强制。
#[derive(Debug, Clone)]
pub(crate) struct BudgetTracker {
    wall_clock_ms: u64,
    usage: BudgetUsage,
    started_at: Instant,
}

impl BudgetTracker {
    pub fn new(budget: TurnBudget) -> Self {
        Self {
            wall_clock_ms: budget.wall_clock_ms,
            usage: BudgetUsage::default(),
            started_at: Instant::now(),
        }
    }

    pub fn usage(&self) -> BudgetUsage {
        let mut usage = self.usage;
        usage.elapsed_ms = self.started_at.elapsed().as_millis() as u64;
        usage
    }

    /// 记录一次模型推理（仅追踪，不限制）。
    pub fn record_model_step(&mut self) {
        self.usage.model_steps += 1;
    }

    /// 记录一次工具调用（仅追踪，不限制）。
    pub fn record_tool_call(&mut self, tool_name: &str) {
        if tool_name == "wait_agent" {
            self.usage.wait_calls += 1;
        } else {
            self.usage.tool_calls += 1;
        }
    }

    /// 检查 wall-clock 安全上限。
    pub fn check_wall_clock(&self) -> std::result::Result<(), BudgetLimit> {
        let usage = self.usage();
        if usage.elapsed_ms > self.wall_clock_ms {
            return Err(BudgetLimit {
                kind: BudgetLimitKind::WallClock,
                usage,
            });
        }
        Ok(())
    }
}

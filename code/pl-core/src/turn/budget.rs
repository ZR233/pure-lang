use std::time::{Duration, Instant};

use pl_protocol::{BudgetLimitKind, BudgetUsage};
use serde::{Deserialize, Serialize};

/// Agent 树结构限制常量。
pub const AGENT_MAX_COUNT: usize = 16;
pub const AGENT_MAX_DEPTH: u32 = 1;
/// 默认 wall-clock 安全上限（30 分钟），参考 Codex 的 agent_job_max_runtime_seconds。
pub const DEFAULT_TURN_WALL_CLOCK: Duration = Duration::from_secs(30 * 60);

/// 单轮 wall-clock 安全预算。
///
/// 参考 Codex 的设计：不限制 model step / tool call 迭代次数，
/// 让模型自己决定何时完成（通过返回无 tool call 的 content-only 响应）。
/// 仅保留 wall-clock 作为防止无限运行的安全兜底。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TurnBudget {
    /// Wall-clock 安全上限（毫秒）。超时后 turn 将被终止。
    wall_clock_ms: u64,
}

impl TurnBudget {
    /// 创建只强制 wall-clock 的单轮预算。
    ///
    /// 超出协议毫秒字段可表达范围的时长会饱和为 [`u64::MAX`]，避免宿主侧的
    /// 平台相关整数转换改变预算语义。
    pub fn new(wall_clock_limit: Duration) -> Self {
        Self {
            wall_clock_ms: wall_clock_limit.as_millis().try_into().unwrap_or(u64::MAX),
        }
    }

    /// 返回本轮强制执行的 wall-clock 上限。
    pub const fn wall_clock_limit(self) -> Duration {
        Duration::from_millis(self.wall_clock_ms)
    }
}

impl Default for TurnBudget {
    fn default() -> Self {
        Self::new(DEFAULT_TURN_WALL_CLOCK)
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
    excluded_wall_clock: Duration,
}

impl BudgetTracker {
    pub fn new(budget: TurnBudget) -> Self {
        Self {
            wall_clock_ms: budget.wall_clock_limit().as_millis() as u64,
            usage: BudgetUsage::default(),
            started_at: Instant::now(),
            excluded_wall_clock: Duration::ZERO,
        }
    }

    pub fn usage(&self) -> BudgetUsage {
        let mut usage = self.usage;
        usage.elapsed_ms = self
            .started_at
            .elapsed()
            .saturating_sub(self.excluded_wall_clock)
            .as_millis() as u64;
        usage
    }

    /// 以 runtime 接受 parent 消息的时刻开始新的预算 tranche。
    pub(crate) fn refresh_at(&mut self, accepted_at: Instant) {
        self.usage = BudgetUsage::default();
        self.started_at = accepted_at;
        self.excluded_wall_clock = Duration::ZERO;
    }

    /// 记录一次模型推理（仅追踪，不限制）。
    pub fn record_model_step(&mut self) {
        self.usage.model_steps += 1;
    }

    /// 记录一次工具调用（仅追踪，不限制）。
    pub fn record_tool_call(&mut self, tool_name: &str) {
        self.usage.tool_calls += 1;
        if tool_name == "wait_agents" {
            self.usage.wait_calls += 1;
        }
    }

    /// 从活跃 wall-clock 中扣除单独 `wait_agents` 的阻塞区间。
    pub fn exclude_wall_clock(&mut self, duration: Duration) {
        self.excluded_wall_clock = self.excluded_wall_clock.saturating_add(duration);
    }

    /// 检查 wall-clock 安全上限。
    pub fn check_wall_clock(&self) -> std::result::Result<(), BudgetLimit> {
        let usage = self.usage();
        if usage.elapsed_ms >= self.wall_clock_ms {
            return Err(BudgetLimit {
                kind: BudgetLimitKind::WallClock,
                usage,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use pretty_assertions::assert_eq;

    use super::super::options::TurnOptions;
    use super::*;

    #[test]
    fn turn_budget_uses_typed_duration_and_preserves_wire_milliseconds() {
        let one_hour = Duration::from_secs(60 * 60);
        let budget = TurnBudget::new(one_hour);

        assert_eq!(budget.wall_clock_limit(), one_hour);
        assert_eq!(
            serde_json::to_value(budget).unwrap(),
            serde_json::json!({ "wallClockMs": 3_600_000 })
        );
        assert_eq!(
            serde_json::from_value::<TurnBudget>(serde_json::json!({
                "wallClockMs": 3_600_000
            }))
            .unwrap(),
            budget
        );
        assert_eq!(
            TurnBudget::default().wall_clock_limit(),
            DEFAULT_TURN_WALL_CLOCK
        );
    }

    #[test]
    fn budget_tracker_records_observability() {
        let mut tracker =
            BudgetTracker::new(TurnBudget::new(std::time::Duration::from_millis(60_000)));

        tracker.record_model_step();
        tracker.record_tool_call("exec");
        tracker.record_tool_call("wait_agents");

        let usage = tracker.usage();
        assert_eq!(usage.model_steps, 1);
        assert_eq!(usage.tool_calls, 2);
        assert_eq!(usage.wait_calls, 1);
    }

    #[test]
    fn budget_tracker_only_enforces_wall_clock() {
        let mut tracker =
            BudgetTracker::new(TurnBudget::new(std::time::Duration::from_millis(60_000)));

        for _ in 0..200 {
            tracker.record_model_step();
            tracker.record_tool_call("exec");
        }

        assert!(tracker.check_wall_clock().is_ok());

        let usage = tracker.usage();
        assert_eq!(usage.model_steps, 200);
        assert_eq!(usage.tool_calls, 200);
    }

    #[test]
    fn budget_tracker_stops_when_active_wall_clock_reaches_limit() {
        let tracker = BudgetTracker::new(TurnBudget::new(std::time::Duration::from_millis(0)));

        assert!(tracker.check_wall_clock().is_err());
    }

    #[test]
    fn budget_refresh_resets_time_exclusions_and_tranche_counts() {
        let mut tracker =
            BudgetTracker::new(TurnBudget::new(std::time::Duration::from_millis(60_000)));
        tracker.record_model_step();
        tracker.record_tool_call("exec");
        tracker.record_tool_call("wait_agents");
        tracker.exclude_wall_clock(Duration::from_secs(30));

        tracker.refresh_at(Instant::now() - Duration::from_millis(5));

        let usage = tracker.usage();
        assert_eq!(usage.model_steps, 0);
        assert_eq!(usage.tool_calls, 0);
        assert_eq!(usage.wait_calls, 0);
        assert!(usage.elapsed_ms >= 5);
        assert!(usage.elapsed_ms < 1_000);
    }

    #[test]
    fn turn_options_consumes_only_the_latest_budget_refresh_once() {
        let (refresh, receiver) = crate::agent_runtime::turn_budget_refresh_channel();
        let mut options = TurnOptions::default();
        options.budget_refresh = Some(receiver);
        let mut tracker =
            BudgetTracker::new(TurnBudget::new(std::time::Duration::from_millis(60_000)));
        tracker.record_model_step();

        refresh.refresh();
        refresh.refresh();
        options.apply_budget_refresh(&mut tracker);
        assert_eq!(tracker.usage().model_steps, 0);

        tracker.record_model_step();
        options.apply_budget_refresh(&mut tracker);
        assert_eq!(tracker.usage().model_steps, 1);
    }
}

//! 轻量调用事件与版本化 JSONL 记录的编解码。
//!
//! 队列只承载这里提取出的轻量记录：绝不 clone/encode 整个 [`pl_core::thread::ThreadEffectBatch`]，
//! 记录里不包含 context、tools 或其它重型正文。每条记录序列化成一行版本化 JSON，写入滚动日志。

use super::*;

/// 单条 JSONL 记录的 schema 版本；写入每行的 `version` 字段，构成“版本化 JSONL”。
pub(super) const CALL_LOG_RECORD_VERSION: u32 = 1;
/// 单条记录的最大字节数；超过即显式 `truncated`，绝不静默截断正文。
pub(super) const CALL_LOG_RECORD_MAX_BYTES: usize = 1024 * 1024;

/// 迁移自旧 `model_calls` 的调用记录类别。
pub(super) const CALL_LOG_KIND_ATTEMPT: &str = "attempt";
pub(super) const CALL_LOG_KIND_BILLING: &str = "billing";
pub(super) const CALL_LOG_KIND_MIGRATED: &str = "migrated";

/// 六个 token 计数；`None` 表示未测得，绝不以零冒充。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct CallUsageRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) cache_read_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) cache_write_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) reasoning_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) total_tokens: Option<u64>,
}

/// 一次成功调用的耗时事实；缺失的计时不会被折叠成零样本。
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct CallTimingRecord {
    pub(super) ttft_millis: u64,
    pub(super) decode_millis: u64,
    pub(super) response_millis: u64,
}

/// 一行版本化 JSONL 日志记录；也是队列事件的载荷。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CallLogRecord {
    #[serde(default = "record_version")]
    pub(super) version: u32,
    pub(super) kind: String,
    pub(super) thread_id: String,
    pub(super) call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) root_thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) retry_of: Option<String>,
    pub(super) status: String,
    pub(super) terminal: bool,
    pub(super) revision: i64,
    pub(super) recorded_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) retention: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) purpose: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) provider_instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) provider_display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) configured_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) sent_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) reported_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) usage: Option<CallUsageRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) cost: Option<RuntimeCostAmount>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(super) has_unpriced_usage: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) timing: Option<CallTimingRecord>,
    /// 正文超过单条上限，字段被显式丢弃；读取端据此区分截断与完整记录。
    #[serde(default, skip_serializing_if = "is_false")]
    pub(super) truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) input_revision: Option<u64>,
    /// Bounded JSON text of this result/error only, never a request or context snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) diagnostic: Option<String>,
}

impl CallLogRecord {
    /// 构造一条最小身份的显式截断记录。
    pub(super) fn truncated_fallback(&self) -> Self {
        Self {
            version: self.version,
            kind: self.kind.clone(),
            thread_id: self.thread_id.clone(),
            call_id: self.call_id.clone(),
            root_thread_id: self.root_thread_id.clone(),
            turn_id: None,
            retry_of: None,
            status: self.status.clone(),
            terminal: self.terminal,
            revision: self.revision,
            recorded_at: self.recorded_at,
            retention: self.retention.clone(),
            purpose: None,
            provider_instance_id: None,
            provider_display_name: None,
            configured_model: None,
            sent_model: None,
            reported_model: None,
            reasoning_effort: None,
            usage: None,
            cost: None,
            has_unpriced_usage: self.has_unpriced_usage,
            timing: None,
            truncated: true,
            input_revision: self.input_revision,
            diagnostic: None,
        }
    }
}

fn record_version() -> u32 {
    CALL_LOG_RECORD_VERSION
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// 从一次 effect 的 attempt 更新提取轻量记录；`tools`、context 与 request 元数据一律不进入记录。
pub(super) fn attempt_record(
    thread_id: &str,
    revision: i64,
    committed_at: i64,
    attempt: &AttemptUpdate,
) -> CallLogRecord {
    let status = attempt_status(&attempt.outcome);
    let (diagnostic, truncated) = diagnostic(&attempt.outcome);
    CallLogRecord {
        version: CALL_LOG_RECORD_VERSION,
        kind: CALL_LOG_KIND_ATTEMPT.to_owned(),
        thread_id: thread_id.to_owned(),
        call_id: attempt.attempt_id.clone(),
        root_thread_id: None,
        turn_id: Some(attempt.turn_id.clone()),
        retry_of: attempt.retry_of.clone(),
        status: status.as_str().to_owned(),
        terminal: status.is_terminal(),
        revision,
        recorded_at: committed_at,
        retention: None,
        purpose: None,
        provider_instance_id: None,
        provider_display_name: None,
        configured_model: None,
        sent_model: None,
        reported_model: None,
        reasoning_effort: None,
        usage: outcome_usage(&attempt.outcome).map(usage_record_from_model),
        cost: None,
        has_unpriced_usage: false,
        timing: None,
        truncated,
        input_revision: Some(attempt.input_revision),
        diagnostic: Some(diagnostic),
    }
}

/// 从一次计费观察提取轻量记录；只保留展示与统计所需的身份、用量、价格与时延。
pub(super) fn billing_record(
    root_thread_id: &str,
    thread_id: &str,
    retention: CallRetention,
    billing: &InferenceBillingRecord,
    status: CallStatus,
) -> CallLogRecord {
    let usage = billing.accounting.usage.totals();
    let (configured_model, sent_model, reported_model) = match &billing.model_observation {
        Some(observation) => (
            Some(observation.configured_model.clone()),
            Some(observation.sent_model.clone()),
            observation.reported_model.clone(),
        ),
        None => (None, Some(billing.model.clone()), None),
    };
    let cost = billing.accounting.estimated_costs().into_iter().next();
    let timing = billing.timing.map(|timing| CallTimingRecord {
        ttft_millis: timing.ttft_millis,
        decode_millis: timing.decode_millis,
        response_millis: timing.total_millis,
    });
    CallLogRecord {
        version: CALL_LOG_RECORD_VERSION,
        kind: CALL_LOG_KIND_BILLING.to_owned(),
        thread_id: thread_id.to_owned(),
        call_id: billing.inference_id.clone(),
        root_thread_id: Some(root_thread_id.to_owned()),
        turn_id: None,
        retry_of: None,
        status: status.as_str().to_owned(),
        terminal: true,
        revision: 0,
        recorded_at: billing.recorded_at,
        retention: Some(retention.as_str().to_owned()),
        purpose: billing.purpose.clone(),
        provider_instance_id: Some(billing.provider_instance_id.clone()),
        provider_display_name: Some(billing.provider.clone()),
        configured_model,
        sent_model,
        reported_model,
        reasoning_effort: billing.reasoning_effort.clone(),
        usage: Some(CallUsageRecord {
            input_tokens: Some(usage.prompt_tokens),
            output_tokens: Some(usage.completion_tokens),
            cache_read_tokens: Some(usage.cached_prompt_tokens),
            cache_write_tokens: Some(usage.cache_write_tokens),
            reasoning_tokens: Some(usage.reasoning_tokens),
            total_tokens: Some(usage.total_tokens),
        }),
        cost,
        has_unpriced_usage: billing.accounting.has_unpriced_usage(),
        timing,
        truncated: false,
        input_revision: None,
        diagnostic: None,
    }
}

fn usage_record_from_model(usage: &ModelUsage) -> CallUsageRecord {
    let total = usage
        .input_tokens
        .zip(usage.output_tokens)
        .and_then(|(input, output)| input.checked_add(output))
        .or(usage.total_tokens);
    CallUsageRecord {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_write_tokens: usage.cache_write_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        total_tokens: total,
    }
}

/// 记录所属的日志日（UTC 自然日）；跨日时轮转到新段。
pub(super) fn record_day(recorded_at: i64) -> i64 {
    recorded_at.div_euclid(SECONDS_PER_DAY)
}

/// 队列压力预算用的编码字节估计；只序列化轻量记录，绝不编码整个 effect。
pub(super) fn estimate_bytes(record: &CallLogRecord) -> usize {
    serde_json::to_vec(record).map_or(256, |bytes| bytes.len())
}

/// 编码一行记录；返回 `(line, truncated)`，`line` 不含换行。
///
/// 正文超过 [`CALL_LOG_RECORD_MAX_BYTES`] 时逐步丢弃可选字段并显式标记 `truncated`，绝不写出
/// 超长或非法的单行，也绝不静默截断。
pub(super) fn encode_record(record: &CallLogRecord) -> Result<(String, bool)> {
    let line = serde_json::to_string(record)?;
    if line.len() < CALL_LOG_RECORD_MAX_BYTES {
        return Ok((line, record.truncated));
    }
    let fallback = record.truncated_fallback();
    let line = serde_json::to_string(&fallback)?;
    ensure!(
        line.len() < CALL_LOG_RECORD_MAX_BYTES,
        "call identity exceeds log record budget"
    );
    Ok((line, true))
}

/// 解析一行版本化 JSONL 记录；损坏的行返回错误而不猜测内容。
pub(super) fn decode_record(line: &str) -> Result<CallLogRecord> {
    ensure!(
        line.len() <= CALL_LOG_RECORD_MAX_BYTES,
        "oversized call log record"
    );
    let record: CallLogRecord = serde_json::from_str(line)?;
    ensure!(
        record.version == CALL_LOG_RECORD_VERSION,
        "unsupported call log record version"
    );
    Ok(record)
}

/// Stops encoding before allocating an unbounded diagnostic buffer.
pub(super) fn diagnostic(value: &impl serde::Serialize) -> (String, bool) {
    struct Bounded(Vec<u8>);
    impl std::io::Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let keep = bytes
                .len()
                .min((128 * 1024_usize).saturating_sub(self.0.len()));
            self.0.extend_from_slice(&bytes[..keep]);
            if keep < bytes.len() {
                return Err(std::io::Error::other("diagnostic truncated"));
            }
            Ok(keep)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Bounded(Vec::new());
    let truncated = serde_json::to_writer(&mut writer, value).is_err();
    (String::from_utf8_lossy(&writer.0).into_owned(), truncated)
}

pub(super) fn tool_record(
    thread_id: &str,
    turn_id: Option<String>,
    revision: i64,
    recorded_at: i64,
    delivery: &pl_core::thread::ToolDelivery,
) -> CallLogRecord {
    let (diagnostic, truncated) = diagnostic(delivery);
    let status = match delivery.outcome {
        pl_core::thread::ToolOutcome::Succeeded => "committed",
        pl_core::thread::ToolOutcome::Failed(_) => "failed",
        pl_core::thread::ToolOutcome::Cancelled => "cancelled",
        pl_core::thread::ToolOutcome::Interrupted => "interrupted",
    };
    CallLogRecord {
        version: CALL_LOG_RECORD_VERSION,
        kind: "tool".into(),
        thread_id: thread_id.into(),
        call_id: delivery.call_id.clone(),
        turn_id,
        revision,
        recorded_at,
        status: status.into(),
        terminal: true,
        diagnostic: Some(diagnostic),
        truncated,
        ..Default::default()
    }
}

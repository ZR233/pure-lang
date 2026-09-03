//! Canonical 流式补全：收集编排与子状态机入口。
//!
//! [`decode_raw_event_stream`] 把 provider 原始流解码为 canonical 事件流，
//! [`collect_completion_event_stream`] 驱动 [`StreamCompletionAccumulator`]
//! 累积出 `CompletionResponse`。生命周期合法性（`lifecycle`）、工具调用增量
//! （`tool_stream`）、标签式可见输出（`tagged_output`）与 trace 投影
//! （`trace_projection`）是独立子状态机。

mod accumulator;
pub(crate) mod decode;
pub(crate) mod event;
mod lifecycle;
mod state;
mod tagged_output;
mod tool_stream;
mod trace_projection;

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;

use pl_protocol::{PureError, Result};
use pl_trace::{AgentEventSender, TraceEventSink};

use crate::completion::CompletionTraceContext;

pub(crate) use accumulator::StreamCompletionAccumulator;
pub(crate) use decode::{CompletionEventStream, OpenAiRawEventStream, decode_raw_event_stream};

#[cfg(test)]
pub(crate) use decode::VisibleOutputDecoder;

const COMPLETION_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(180);

/// 流收集期间的宿主上下文：事件输出、trace 目标与取消信号。
pub(crate) struct StreamCollectContext<'a> {
    pub(crate) event_tx: &'a AgentEventSender,
    pub(crate) trace: Option<CompletionTraceContext>,
    pub(crate) trace_sink: Option<Arc<dyn TraceEventSink>>,
    pub(crate) cancellation: Option<tokio_util::sync::CancellationToken>,
}

pub(crate) async fn collect_completion_event_stream(
    stream: CompletionEventStream,
    context: StreamCollectContext<'_>,
) -> Result<crate::completion::CompletionResponse> {
    collect_completion_event_stream_with_idle_timeout(
        stream,
        context,
        COMPLETION_STREAM_IDLE_TIMEOUT,
    )
    .await
}

pub(crate) async fn collect_completion_event_stream_with_idle_timeout(
    mut stream: CompletionEventStream,
    context: StreamCollectContext<'_>,
    idle_timeout: Duration,
) -> Result<crate::completion::CompletionResponse> {
    let StreamCollectContext {
        event_tx,
        trace,
        trace_sink,
        cancellation,
    } = context;
    let mut accumulator = StreamCompletionAccumulator::with_trace_sink(trace, trace_sink);

    loop {
        let next_event = tokio::time::timeout(idle_timeout, stream.next());
        let next_event = match cancellation.as_ref() {
            Some(token) => {
                tokio::select! {
                    event = next_event => event,
                    _ = token.cancelled() => {
                        accumulator.cancel_attempt("model invocation cancelled", event_tx);
                        return Err(PureError::LlmError("model invocation cancelled".to_string()));
                    }
                }
            }
            None => next_event.await,
        };
        let event = match next_event {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(_) => {
                let error = PureError::transient_model_transport(
                    "stream error: idle timeout waiting for provider event",
                );
                accumulator.fail_attempt(&error, event_tx);
                return Err(error);
            }
        };
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                accumulator.fail_attempt(&error, event_tx);
                return Err(error);
            }
        };
        if let Err(error) = accumulator.apply(event, event_tx) {
            accumulator.fail_attempt(&error, event_tx);
            return Err(error);
        }
    }

    accumulator.finish(event_tx)
}

#[cfg(test)]
mod unit_tests;

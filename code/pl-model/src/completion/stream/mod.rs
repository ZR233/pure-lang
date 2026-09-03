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
mod tests {
    use futures::StreamExt;
    use pl_protocol::{ProviderFailureKind, Result};

    use super::event::ModelStreamEvent;
    use super::{
        CompletionEventStream, StreamCollectContext,
        collect_completion_event_stream_with_idle_timeout,
    };

    #[tokio::test]
    async fn collect_completion_event_stream_returns_idle_timeout_when_stream_stalls() {
        let stream: CompletionEventStream =
            futures::stream::pending::<Result<ModelStreamEvent>>().boxed();
        let (event_tx, _) = tokio::sync::broadcast::channel(1);

        let error = collect_completion_event_stream_with_idle_timeout(
            stream,
            StreamCollectContext {
                event_tx: &event_tx,
                trace: None,
                trace_sink: None,
                cancellation: None,
            },
            std::time::Duration::from_millis(10),
        )
        .await
        .unwrap_err();

        let failure = error
            .provider_failure_ref()
            .expect("typed provider failure");
        assert_eq!(failure.kind, ProviderFailureKind::Transport);
        assert_eq!(
            failure.message,
            "stream error: idle timeout waiting for provider event"
        );
        assert!(error.is_transient_model_transport());
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use pl_protocol::Result;
    use pl_trace::{AgentEventSender, TraceDelta, TracePart};

    use crate::completion::CompletionResponse;

    use super::StreamCompletionAccumulator;
    use super::decode::VisibleOutputDecoder;
    use super::event::ModelStreamEvent;

    pub(crate) fn apply_completed(
        accumulator: &mut StreamCompletionAccumulator,
        event_tx: &AgentEventSender,
    ) {
        accumulator
            .apply(ModelStreamEvent::Completed { response_id: None }, event_tx)
            .unwrap();
    }

    pub(crate) fn apply_tagged(
        decoder: &mut VisibleOutputDecoder,
        accumulator: &mut StreamCompletionAccumulator,
        event: ModelStreamEvent,
        event_tx: &AgentEventSender,
    ) {
        for event in decoder.decode(event) {
            accumulator.apply(event, event_tx).unwrap();
        }
    }

    pub(crate) fn tagged_decoder() -> VisibleOutputDecoder {
        VisibleOutputDecoder::new(crate::runtime::openai::VisibleOutputProtocol::TaggedText)
    }

    pub(crate) fn final_delta(id: &str, delta: &str) -> ModelStreamEvent {
        ModelStreamEvent::text_delta(
            id.to_string(),
            pl_trace::TraceTextChannel::Final,
            delta.to_string(),
        )
    }

    pub(crate) fn final_started(id: &str) -> ModelStreamEvent {
        ModelStreamEvent::text_started(id.to_string(), pl_trace::TraceTextChannel::Final)
    }

    pub(crate) fn commentary_started(id: &str) -> ModelStreamEvent {
        ModelStreamEvent::text_started(id.to_string(), pl_trace::TraceTextChannel::Commentary)
    }

    pub(crate) fn completed_text(
        id: &str,
        channel: pl_trace::TraceTextChannel,
        authoritative_text: Option<&str>,
    ) -> ModelStreamEvent {
        ModelStreamEvent::text_completed(
            id.to_string(),
            channel,
            authoritative_text.map(ToOwned::to_owned),
        )
    }

    pub(crate) fn summary_delta(id: &str, section_index: u32, delta: &str) -> ModelStreamEvent {
        ModelStreamEvent::reasoning_summary_delta(id.to_string(), section_index, delta.to_string())
    }

    pub(crate) fn summary_started(id: &str) -> ModelStreamEvent {
        ModelStreamEvent::reasoning_summary_started(id.to_string(), None)
    }

    pub(crate) fn trace_part_text(item: &TracePart) -> String {
        match item.state() {
            pl_trace::TracePartState::Text(text) => text.content().to_string(),
            pl_trace::TracePartState::Turn(_) => String::new(),
            pl_trace::TracePartState::Thinking(thinking) => {
                let summary = thinking
                    .summary()
                    .iter()
                    .map(|chunk| chunk.content.as_str())
                    .collect::<Vec<_>>()
                    .join("");
                if summary.is_empty() {
                    thinking
                        .content()
                        .iter()
                        .map(|chunk| chunk.content.as_str())
                        .collect::<Vec<_>>()
                        .join("")
                } else {
                    summary
                }
            }
            pl_trace::TracePartState::Tool(tool) => tool.invocation().arguments().to_string(),
            pl_trace::TracePartState::Agent(_) | pl_trace::TracePartState::Inference(_) => {
                String::new()
            }
        }
    }

    pub(crate) fn trace_delta_text(delta: &TraceDelta) -> String {
        match delta {
            pl_trace::TraceDelta::Text { delta, .. }
            | pl_trace::TraceDelta::Thinking { delta, .. }
            | pl_trace::TraceDelta::ReasoningContent { delta, .. }
            | pl_trace::TraceDelta::ToolArguments { delta }
            | pl_trace::TraceDelta::ToolResult { delta } => delta.clone(),
        }
    }

    pub(crate) fn trace_text_channel(item: &TracePart) -> Option<pl_trace::TraceTextChannel> {
        item.text().map(pl_trace::TraceTextPart::channel)
    }

    pub(crate) struct TestCompletionResponse {
        pub(crate) response: CompletionResponse,
        pub(crate) trace_events: Vec<pl_trace::TraceEvent>,
    }

    impl std::ops::Deref for TestCompletionResponse {
        type Target = CompletionResponse;

        fn deref(&self) -> &Self::Target {
            &self.response
        }
    }

    pub(crate) fn finish_with_trace(
        accumulator: StreamCompletionAccumulator,
        event_tx: &AgentEventSender,
    ) -> Result<TestCompletionResponse> {
        let trace_events = Default::default();
        let response = accumulator.finish_with_trace_events(event_tx, &trace_events)?;
        let trace_events = trace_events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default();
        Ok(TestCompletionResponse {
            response,
            trace_events,
        })
    }
}

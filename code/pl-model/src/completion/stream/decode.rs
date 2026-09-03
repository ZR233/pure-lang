//! Provider 原始 SSE 流到 canonical 事件流的解码链。
//!
//! `OpenAiStreamDecoder` 先把 wire 事件归一为 `ModelStreamEvent`，
//! `VisibleOutputDecoder` 再按模型的可见输出协议（原生 phases 或标签文本）
//! 适配为最终对外的事件。

use futures::StreamExt;
use futures::stream::BoxStream;
use std::collections::VecDeque;

use pl_protocol::Result;

use crate::runtime::openai::sse;
use crate::runtime::openai::{OpenAiProtocol, VisibleOutputProtocol};

use super::event::ModelStreamEvent;
use super::tagged_output::{TaggedOutputDiagnostics, TaggedVisibleOutputAdapter};

pub(crate) type CompletionEventStream = BoxStream<'static, Result<ModelStreamEvent>>;
pub(crate) type OpenAiRawEventStream = BoxStream<'static, Result<sse::SseStreamEvent>>;

pub(crate) fn decode_raw_event_stream(
    stream: OpenAiRawEventStream,
    protocol: OpenAiProtocol,
) -> CompletionEventStream {
    let state = ProviderStreamDecodeState {
        stream,
        decoder: protocol.new_stream_decoder(),
        visible_output: VisibleOutputDecoder::new(protocol.visible_output_protocol()),
        pending: VecDeque::new(),
    };

    futures::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(event) = state.pending.pop_front() {
                return Some((Ok(event), state));
            }

            let sse_event = match state.stream.next().await {
                Some(Ok(event)) => event,
                Some(Err(error)) => return Some((Err(error), state)),
                None => {
                    state.visible_output.record_diagnostics();
                    return None;
                }
            };

            for stream_event in state.decoder.decode(&sse_event) {
                state
                    .pending
                    .extend(state.visible_output.decode(stream_event));
            }
        }
    })
    .boxed()
}

struct ProviderStreamDecodeState {
    stream: OpenAiRawEventStream,
    decoder: sse::OpenAiStreamDecoder,
    visible_output: VisibleOutputDecoder,
    pending: VecDeque<ModelStreamEvent>,
}

pub(crate) enum VisibleOutputDecoder {
    NativePhases,
    TaggedText(TaggedVisibleOutputAdapter),
}

impl VisibleOutputDecoder {
    pub(crate) fn new(protocol: VisibleOutputProtocol) -> Self {
        match protocol {
            VisibleOutputProtocol::NativePhases => Self::NativePhases,
            VisibleOutputProtocol::TaggedText => {
                Self::TaggedText(TaggedVisibleOutputAdapter::new())
            }
        }
    }

    pub(crate) fn decode(&mut self, event: ModelStreamEvent) -> Vec<ModelStreamEvent> {
        match self {
            Self::NativePhases => vec![event],
            Self::TaggedText(decoder) => decoder.adapt(event),
        }
    }

    pub(crate) fn diagnostics(&self) -> TaggedOutputDiagnostics {
        match self {
            Self::NativePhases => TaggedOutputDiagnostics::default(),
            Self::TaggedText(decoder) => decoder.diagnostics(),
        }
    }

    fn record_diagnostics(&self) {
        let diagnostics = self.diagnostics();
        if diagnostics.untagged_visible_text_segments == 0 {
            return;
        }
        tracing::warn!(
            segments = diagnostics.untagged_visible_text_segments,
            chars = diagnostics.untagged_visible_text_chars,
            "tagged visible output contained untagged visible text; using fallback final text"
        );
    }
}

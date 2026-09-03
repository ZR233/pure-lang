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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion::stream::event::{ModelBlockField, ModelBlockKind};
    use pl_trace::TraceTextChannel;

    #[test]
    fn native_phase_decoder_does_not_parse_visible_tags() {
        let mut decoder = VisibleOutputDecoder::new(VisibleOutputProtocol::NativePhases);
        let events = decoder.decode(ModelStreamEvent::text_delta(
            "native-final".to_string(),
            TraceTextChannel::Final,
            "<final>literal</final>".to_string(),
        ));

        assert!(matches!(
            events.as_slice(),
            [ModelStreamEvent::BlockDelta {
                id,
                kind: ModelBlockKind::Text {
                    channel: TraceTextChannel::Final,
                },
                field: ModelBlockField::Text,
                delta,
                ..
            }]
                if id == "native-final" && delta == "<final>literal</final>"
        ));
    }

    #[test]
    fn tagged_text_decoder_extracts_visible_tags() {
        let mut decoder = VisibleOutputDecoder::new(VisibleOutputProtocol::TaggedText);
        let events = decoder.decode(ModelStreamEvent::text_delta(
            "chat-final".to_string(),
            TraceTextChannel::Final,
            "<commentary>working</commentary><final>done</final>".to_string(),
        ));

        assert!(matches!(
            events.as_slice(),
            [
                ModelStreamEvent::BlockOpened {
                    id: commentary_started_id,
                    kind: ModelBlockKind::Text {
                        channel: TraceTextChannel::Commentary,
                    },
                    ..
                },
                ModelStreamEvent::BlockDelta {
                    id: commentary_id,
                    kind: ModelBlockKind::Text {
                        channel: TraceTextChannel::Commentary,
                    },
                    field: ModelBlockField::Text,
                    delta: commentary,
                    ..
                },
                ModelStreamEvent::BlockClosed {
                    id: commentary_completed_id,
                    kind: ModelBlockKind::Text {
                        channel: TraceTextChannel::Commentary,
                    },
                    authoritative_content: None,
                    ..
                },
                ModelStreamEvent::BlockOpened {
                    id: final_started_id,
                    kind: ModelBlockKind::Text {
                        channel: TraceTextChannel::Final,
                    },
                    ..
                },
                ModelStreamEvent::BlockDelta {
                    id: final_id,
                    kind: ModelBlockKind::Text {
                        channel: TraceTextChannel::Final,
                    },
                    field: ModelBlockField::Text,
                    delta: final_text,
                    ..
                },
                ModelStreamEvent::BlockClosed {
                    id: final_completed_id,
                    kind: ModelBlockKind::Text {
                        channel: TraceTextChannel::Final,
                    },
                    authoritative_content: None,
                    ..
                },
            ] if commentary_started_id == "tagged-commentary-1"
                && commentary_id == commentary_started_id
                && commentary_completed_id == commentary_started_id
                && commentary == "working"
                && final_started_id == "tagged-final-2"
                && final_id == final_started_id
                && final_completed_id == final_started_id
                && final_text == "done"
        ));
    }

    #[test]
    fn tagged_text_decoder_records_untagged_visible_text_diagnostic() {
        let mut decoder = VisibleOutputDecoder::new(VisibleOutputProtocol::TaggedText);
        let events = decoder.decode(ModelStreamEvent::text_delta(
            "chat-final".to_string(),
            TraceTextChannel::Final,
            "plain fallback".to_string(),
        ));

        assert!(matches!(
            events.as_slice(),
            [
                ModelStreamEvent::BlockOpened {
                    id,
                    kind: ModelBlockKind::Text {
                        channel: TraceTextChannel::Final,
                    },
                    ..
                },
                ModelStreamEvent::BlockDelta {
                    id: delta_id,
                    kind: ModelBlockKind::Text {
                        channel: TraceTextChannel::Final,
                    },
                    field: ModelBlockField::Text,
                    delta,
                    ..
                },
            ] if id == "tagged-final-1"
                && delta_id == id
                && delta == "plain fallback"
        ));
        let diagnostics = decoder.diagnostics();
        assert_eq!(diagnostics.untagged_visible_text_segments, 1);
        assert_eq!(
            diagnostics.untagged_visible_text_chars,
            "plain fallback".len()
        );
    }

    #[test]
    fn tagged_text_decoder_gives_repeated_tags_distinct_blocks() {
        let mut decoder = VisibleOutputDecoder::new(VisibleOutputProtocol::TaggedText);
        let events = decoder.decode(ModelStreamEvent::text_delta(
            "chat-final".to_string(),
            TraceTextChannel::Final,
            "<commentary>A</commentary><commentary>B</commentary>".to_string(),
        ));

        let completed_ids = events
            .iter()
            .filter_map(|event| match event {
                ModelStreamEvent::BlockClosed {
                    id,
                    kind:
                        ModelBlockKind::Text {
                            channel: TraceTextChannel::Commentary,
                        },
                    authoritative_content: None,
                    ..
                } => Some(id.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            completed_ids,
            vec!["tagged-commentary-1", "tagged-commentary-2"]
        );
    }

    #[test]
    fn tagged_text_decoder_keeps_raw_reasoning_tags_hidden() {
        let mut decoder = VisibleOutputDecoder::new(VisibleOutputProtocol::TaggedText);
        let events = decoder.decode(ModelStreamEvent::ReasoningRawDelta {
            id: "thinking".to_string(),
            content_index: 0,
            delta: "<commentary>hidden</commentary><final>hidden</final>".to_string(),
        });

        assert!(matches!(
            events.as_slice(),
            [ModelStreamEvent::ReasoningRawDelta { delta, .. }]
                if delta == "<commentary>hidden</commentary><final>hidden</final>"
        ));
    }
}

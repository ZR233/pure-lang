use super::*;
use futures::StreamExt;

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
            ModelStreamEvent::ResponseStarted { .. }
            | ModelStreamEvent::BlockOpened { .. }
            | ModelStreamEvent::BlockDelta { .. }
            | ModelStreamEvent::BlockClosed { .. }
            | ModelStreamEvent::ReasoningRawDelta { .. }
            | ModelStreamEvent::ToolInputStarted { .. }
            | ModelStreamEvent::ToolInputDelta { .. }
            | ModelStreamEvent::ToolInputCompleted { .. }
            | ModelStreamEvent::ToolCallReady { .. }
            | ModelStreamEvent::ToolCallCaller { .. }
            | ModelStreamEvent::ResponsesContextItem { .. }
            | ModelStreamEvent::WebSearchStarted { .. }
            | ModelStreamEvent::WebSearchCompleted { .. }
            | ModelStreamEvent::Usage(_)
            | ModelStreamEvent::Completed { .. }
            | ModelStreamEvent::Failed { .. } => None,
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

#[tokio::test]
async fn collect_completion_event_stream_returns_idle_timeout_when_stream_stalls() {
    let stream: CompletionEventStream =
        futures::stream::pending::<Result<CompletionStreamEvent>>().boxed();
    let (event_tx, _) = tokio::sync::broadcast::channel(1);

    let error = collect_completion_event_stream_with_idle_timeout(
        stream,
        &event_tx,
        None,
        Default::default(),
        std::time::Duration::from_millis(10),
    )
    .await
    .unwrap_err();

    let failure = error
        .provider_failure_ref()
        .expect("typed provider failure");
    assert_eq!(failure.kind, pl_protocol::ProviderFailureKind::Transport);
    assert_eq!(
        failure.message,
        "stream error: idle timeout waiting for provider event"
    );
    assert!(error.is_transient_model_transport());
}

use crate::runtime::openai::sse::SseStreamEvent;

#[derive(Debug)]
pub(in crate::runtime::responses_websocket) struct CompletedResponsesStream {
    event: SseStreamEvent,
}

impl CompletedResponsesStream {
    pub(in crate::runtime::responses_websocket) fn new(event: SseStreamEvent) -> Self {
        Self { event }
    }

    pub(in crate::runtime::responses_websocket) fn event(&self) -> &SseStreamEvent {
        &self.event
    }
}

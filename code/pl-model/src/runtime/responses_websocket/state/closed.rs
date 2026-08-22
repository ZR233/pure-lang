#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime::responses_websocket) struct ClosedResponsesStream;

impl ClosedResponsesStream {
    pub(in crate::runtime::responses_websocket) const fn new() -> Self {
        Self
    }
}

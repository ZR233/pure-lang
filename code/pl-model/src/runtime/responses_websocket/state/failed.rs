#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::runtime::responses_websocket) struct FailedResponsesStream {
    detail: String,
}

impl FailedResponsesStream {
    pub(in crate::runtime::responses_websocket) fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }

    pub(in crate::runtime::responses_websocket) fn detail(&self) -> &str {
        &self.detail
    }
}

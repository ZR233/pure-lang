#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::completion::stream) struct FailedStream {
    message: String,
}

impl FailedStream {
    pub(in crate::completion::stream) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub(in crate::completion::stream) fn message(&self) -> &str {
        &self.message
    }
}

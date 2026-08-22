#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::completion::stream) struct CompletedStream;

impl CompletedStream {
    pub(in crate::completion::stream) const fn new() -> Self {
        Self
    }
}

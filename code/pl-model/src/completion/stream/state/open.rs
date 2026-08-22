#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::completion::stream) struct OpenStream;

impl OpenStream {
    pub(in crate::completion::stream) const fn new() -> Self {
        Self
    }
}

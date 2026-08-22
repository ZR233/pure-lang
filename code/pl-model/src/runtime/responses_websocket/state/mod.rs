//! 单个 Responses WebSocket 响应流的终止状态。

mod closed;
mod completed;
mod failed;
mod open;

pub(super) use closed::ClosedResponsesStream;
pub(super) use completed::CompletedResponsesStream;
pub(super) use failed::FailedResponsesStream;
pub(super) use open::OpenResponsesStream;

#[derive(Debug)]
pub(super) enum ResponsesStreamState {
    Open(OpenResponsesStream),
    Completed(Box<CompletedResponsesStream>),
    Failed(FailedResponsesStream),
    Closed(ClosedResponsesStream),
}

impl ResponsesStreamState {
    pub(super) const fn open() -> Self {
        Self::Open(OpenResponsesStream::new())
    }
}

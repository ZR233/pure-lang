//! 模型流累积器的终止状态。

mod completed;
mod failed;
mod open;

pub(super) use completed::CompletedStream;
pub(super) use failed::FailedStream;
pub(super) use open::OpenStream;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StreamAccumulatorState {
    Open(OpenStream),
    Completed(CompletedStream),
    Failed(FailedStream),
}

impl StreamAccumulatorState {
    pub(super) const fn open() -> Self {
        Self::Open(OpenStream::new())
    }
}

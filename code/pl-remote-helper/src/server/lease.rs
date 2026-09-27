use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, ReadBuf};
use tokio::sync::watch;
use tokio::time::Instant;

pub(super) const TIMEOUT: Duration = Duration::from_secs(30);

pub(super) async fn wait_expired(mut activity: watch::Receiver<Instant>) {
    loop {
        // Mark the observed value as seen; borrow() would leave changed() ready
        // forever after the first byte and starve the expiry timer.
        let deadline = *activity.borrow_and_update() + TIMEOUT;
        tokio::select! {
            changed = activity.changed() => {
                if changed.is_err() {
                    std::future::pending::<()>().await;
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                if Instant::now().duration_since(*activity.borrow()) >= TIMEOUT {
                    return;
                }
            }
        }
    }
}

/// Incoming byte progress renews the lease even while a large frame is incomplete.
pub(super) struct LeaseReader<R> {
    reader: R,
    activity: watch::Sender<Instant>,
}

impl<R> LeaseReader<R> {
    pub(super) fn new(reader: R) -> (Self, watch::Receiver<Instant>) {
        let (activity, observed) = watch::channel(Instant::now());
        (Self { reader, activity }, observed)
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for LeaseReader<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        let before = buffer.filled().len();
        let result = Pin::new(&mut this.reader).poll_read(context, buffer);
        if matches!(result, Poll::Ready(Ok(()))) && buffer.filled().len() > before {
            this.activity.send_replace(Instant::now());
        }
        result
    }
}

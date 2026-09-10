//! Bounded command previews, drained by the same future that owns command completion.
use crate::command::process_manager::{CommandOutputObserver, CommandOutputStream};
use pl_core::{context::ContextContent, thread::TaskAccess};
use std::{future::Future, sync::Arc};
use tokio::sync::watch;

const MAX_PREVIEW_BYTES: usize = 64 * 1024;
const OMITTED: &str =
    "\n[Earlier command output omitted from this live preview; complete output is archived.]\n";

pub(super) struct CommandPreview {
    preview: watch::Sender<String>,
}

impl CommandPreview {
    pub(super) fn channel() -> (Arc<Self>, watch::Receiver<String>) {
        let (preview, receiver) = watch::channel(String::new());
        (Arc::new(Self { preview }), receiver)
    }
}

impl CommandOutputObserver for CommandPreview {
    fn output_chunk(&self, stream: CommandOutputStream, chunk: &[u8], _: u64) {
        if chunk.is_empty() {
            return;
        }
        self.preview.send_modify(|preview| {
            if stream == CommandOutputStream::Stderr {
                preview.push_str("[stderr] ");
            }
            preview.push_str(&String::from_utf8_lossy(chunk));
            if preview.len() > MAX_PREVIEW_BYTES {
                let mut start = preview.len() - (MAX_PREVIEW_BYTES - OMITTED.len());
                while !preview.is_char_boundary(start) {
                    start += 1;
                }
                preview.drain(..start);
                preview.insert_str(0, OMITTED);
            }
        });
    }
}

pub(super) async fn drive<F: Future>(
    future: F,
    mut previews: watch::Receiver<String>,
    access: Option<&TaskAccess>,
) -> F::Output {
    tokio::pin!(future);
    let mut access = access;
    loop {
        tokio::select! {
            result = &mut future => {
                if let Some(access) = access {
                    let preview = previews.borrow_and_update().clone();
                    report(access, preview).await;
                }
                return result;
            }
            changed = previews.changed(), if access.is_some() => {
                if changed.is_err() { access = None; continue; }
                let preview = previews.borrow_and_update().clone();
                if let Some(current) = access && !report(current, preview).await { access = None; }
            }
        }
    }
}

async fn report(access: &TaskAccess, preview: String) -> bool {
    if preview.is_empty() {
        return true;
    }
    match access
        .report_progress(vec![ContextContent::Text {
            text: preview.into(),
        }])
        .await
    {
        Ok(()) => true,
        Err(error) => {
            // Preview availability never changes the command's canonical result or prevents drain.
            tracing::debug!(%error, "command preview observer stopped");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn command_preview_retains_stream_labels_and_bounds_unicode_without_touching_full_output() {
        let (observer, receiver) = CommandPreview::channel();
        observer.output_chunk(CommandOutputStream::Stdout, b"stdout\n", 1);
        observer.output_chunk(CommandOutputStream::Stderr, b"failure\n", 2);
        assert_eq!(&*receiver.borrow(), "stdout\n[stderr] failure\n");
        let oversized = "汉".repeat(MAX_PREVIEW_BYTES);
        observer.output_chunk(CommandOutputStream::Stdout, oversized.as_bytes(), 3);
        let preview = receiver.borrow();
        assert!(preview.len() <= MAX_PREVIEW_BYTES);
        assert!(preview.starts_with(OMITTED));
        assert!(preview.ends_with("汉"));
        assert_eq!(oversized.len(), MAX_PREVIEW_BYTES * 3);
    }
}

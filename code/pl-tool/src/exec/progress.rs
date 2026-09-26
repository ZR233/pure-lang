//! Bounded command previews, drained by the same future that owns command completion.
//!
//! The observer keeps the live window as a shared [`ContentBlock`] chain and publishes it as one
//! `Arc` per change; the pump then reports only the bytes the Thread has not accepted yet. Appending
//! a chunk therefore copies no accumulated text at all — neither here nor in the Thread — and the
//! canonical result still comes from the archived capture, not from this preview.
use crate::command::process_manager::{
    CommandCaptureFailure, CommandOutputObserver, CommandOutputStream,
};
use pl_core::model::{ContentBlock, ToolProgressUpdate};
use pl_core::thread::TaskAccess;
use std::{future::Future, sync::Arc};
use tokio::sync::{mpsc, watch};

/// Producer-chosen identity of the live preview.
///
/// stdout and stderr share the existing single merged window, so the preview is one opaque part:
/// the key is not parsed by core, and nothing infers a tool's structure from the text.
const PREVIEW_PART: &str = "command-output";

/// Ceiling of the live preview window.
///
/// Appends stay at or below it, and a chunk that would cross it rolls the window over instead. The
/// canonical result is unaffected: the complete capture is archived and reported separately.
const MAX_PREVIEW_BYTES: usize = 64 * 1024;
/// Bytes kept when the window rolls over.
///
/// Half the ceiling leaves room for the next appends, so a rollover materializes the tail once per
/// half-ceiling of output instead of once per chunk.
const KEEP_AFTER_ROLLOVER_BYTES: usize = MAX_PREVIEW_BYTES / 2;
const OMITTED: &str =
    "\n[Earlier command output omitted from this live preview; complete output is archived.]\n";

pub(super) struct CommandPreview {
    window: watch::Sender<Arc<ContentBlock>>,
}

/// The exec tool's own observer: it keeps the live preview and relays a hard capture failure to its
/// owner the moment the reader observes it.
///
/// `output_chunk` stays on the preview's shared chain (no per-chunk copy of the accumulated text),
/// while `output_failed` hands the typed failure to the reporter task that latches it through the
/// running call's channel — before the process tree finishes draining, not when `execute` returns.
pub(super) struct ExecOutputObserver {
    preview: Arc<CommandPreview>,
    failures: mpsc::UnboundedSender<CommandCaptureFailure>,
}

impl ExecOutputObserver {
    pub(super) fn channel() -> (
        Arc<Self>,
        watch::Receiver<Arc<ContentBlock>>,
        mpsc::UnboundedReceiver<CommandCaptureFailure>,
    ) {
        let (preview, windows) = CommandPreview::channel();
        let (failures, failure_rx) = mpsc::unbounded_channel();
        (Arc::new(Self { preview, failures }), windows, failure_rx)
    }
}

impl CommandOutputObserver for ExecOutputObserver {
    fn output_chunk(&self, stream: CommandOutputStream, chunk: &[u8], revision: u64) {
        self.preview.output_chunk(stream, chunk, revision);
    }
    fn output_failed(&self, failure: &CommandCaptureFailure) {
        // A send that finds no receiver only means the call already returned and reports the same
        // typed fault on its return path, so it is never the only report.
        let _ = self.failures.send(failure.clone());
    }
}

impl CommandPreview {
    pub(super) fn channel() -> (Arc<Self>, watch::Receiver<Arc<ContentBlock>>) {
        let (window, receiver) = watch::channel(ContentBlock::empty());
        (Arc::new(Self { window }), receiver)
    }
}

impl CommandOutputObserver for CommandPreview {
    fn output_chunk(&self, stream: CommandOutputStream, chunk: &[u8], _: u64) {
        if chunk.is_empty() {
            return;
        }
        let mut text = String::new();
        if stream == CommandOutputStream::Stderr {
            text.push_str("[stderr] ");
        }
        text.push_str(&String::from_utf8_lossy(chunk));
        self.window.send_modify(|window| {
            // The append path shares every byte already observed and copies nothing but the chunk.
            if window.len().saturating_add(text.len()) <= MAX_PREVIEW_BYTES {
                let appended = ContentBlock::append(window, &text);
                *window = appended;
                return;
            }
            // Only a real rollover rebuilds a bounded window. It is published as one shared block,
            // and the Thread takes it as an explicit replacement, so a dropped head is never
            // mistaken for an append onto a body that changed identity.
            let grown = ContentBlock::append(window, &text);
            let mut tail = String::with_capacity(KEEP_AFTER_ROLLOVER_BYTES + OMITTED.len());
            tail.push_str(OMITTED);
            tail.push_str(&grown.suffix(KEEP_AFTER_ROLLOVER_BYTES));
            *window = ContentBlock::from_shared(Arc::from(tail.as_str()));
        });
    }
}

/// Reports a command's live output while the future that owns the command completes.
pub(super) async fn drive<F: Future>(
    future: F,
    mut windows: watch::Receiver<Arc<ContentBlock>>,
    access: Option<&TaskAccess>,
) -> F::Output {
    tokio::pin!(future);
    let mut access = access;
    // The newest window the Thread accepted. It is the only state this pump keeps: the increment is
    // derived from the shared chain, so nothing here holds the accumulated output either.
    let mut delivered: Option<Arc<ContentBlock>> = None;
    loop {
        tokio::select! {
            result = &mut future => {
                if let Some(current) = access {
                    let window = windows.borrow().clone();
                    report(current, &window, &mut delivered).await;
                }
                return result;
            }
            changed = windows.changed(), if access.is_some() => {
                if changed.is_err() { access = None; continue; }
                let window = windows.borrow_and_update().clone();
                if let Some(current) = access
                    && !report(current, &window, &mut delivered).await
                {
                    access = None;
                }
            }
        }
    }
}

/// Delivers the bytes this Thread has not accepted yet from the newest published window.
async fn report(
    access: &TaskAccess,
    window: &Arc<ContentBlock>,
    delivered: &mut Option<Arc<ContentBlock>>,
) -> bool {
    let update = match delivered.as_ref() {
        // Nothing was delivered yet, and a bounded window may already have dropped its head once, so
        // the whole current window is the only sound statement about this part.
        None => {
            if window.is_empty() {
                return true;
            }
            ToolProgressUpdate::replace(PREVIEW_PART, Arc::from(window.text()))
        }
        Some(previous) => match ContentBlock::appended_since(window, previous) {
            Some(appended) if appended.is_empty() => return true,
            Some(appended) => ToolProgressUpdate::append(PREVIEW_PART, Arc::from(appended)),
            // The window rolled over, so the previous baseline is no longer a prefix of this body.
            None => ToolProgressUpdate::replace(PREVIEW_PART, Arc::from(window.text())),
        },
    };
    match access.report_output(update).await {
        Ok(()) => {
            *delivered = Some(window.clone());
            true
        }
        Err(error) => {
            // Preview availability never changes the command's canonical result or prevents drain.
            tracing::debug!(%error, "command preview observer stopped");
            false
        }
    }
}

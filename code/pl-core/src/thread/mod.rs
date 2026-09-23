//! Serial Thread ownership over protocol-independent model context and committed private material.

use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, watch};
mod background;
mod cancellation;
mod checkpoint;
pub mod cold;
pub mod context_preparation;
mod continuation;
pub mod extensions;
mod facts;
pub mod inbox;
pub mod input;
pub mod interactions;
pub mod journal;
mod owner;
pub mod permissions;
mod recovery;
mod subscription;
pub mod task;
mod task_access;
pub use task_access::{TaskAccess, TaskWaitSnapshot};
mod turn;
pub use checkpoint::{
    CHECKPOINT_BODY_THRESHOLD_BYTES, CheckpointBodyError, CheckpointBodyKind,
    CheckpointBodyReference, CheckpointBodySlot, CheckpointExternalBody, ExtractedCheckpointBody,
    ThreadCheckpoint,
};
pub use journal::ThreadEffectBatch;
use owner::{Owner, PendingCall};
pub use subscription::ThreadSubscription;
use tokio_util::sync::CancellationToken;

use crate::context::{
    ContextContent, ContextRecord, ContextSnapshot, ContextSource, OpaquePayload,
};
use crate::model::{
    DynModelSession, ModelError, ModelRequest, ModelStepOutput, ModelToolDeclaration,
};

mod handle;
mod reconfiguration;
pub use reconfiguration::IdleReconfiguration;
mod mailbox;
mod model_step;
mod model_update;
pub use model_update::{DeferredModelUpdate, DeferredModelUpdatePrecondition};
mod replacement;
mod tool_execution;
mod types;
use handle::Command;
pub use handle::ThreadHandle;
pub use types::*;

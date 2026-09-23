//! Optional host-owned context transformation before an ordinary model request is admitted.
use super::*;
use extensions::{ExtensionMutation, ExtensionRecord};
use std::{collections::BTreeMap, fmt, future::Future};

/// Frozen committed context and application versions. New input has not been consumed yet.
#[derive(Debug, Clone)]
pub struct ContextPreparationRequest {
    pub model: ModelRequest,
    pub extensions: Arc<BTreeMap<String, ExtensionRecord>>,
    pub extension_sequence: u64,
    pub previous_usage: Option<crate::model::ModelUsage>,
}

/// Prepared replacement and producer facts. Successful replacement and receipts commit together.
#[derive(Debug)]
pub enum ContextPreparation {
    Unchanged,
    Replaced {
        replacement: ReplaceContext,
        mutations: Vec<ExtensionMutation>,
    },
    Failed {
        error: ModelError,
        mutations: Vec<ExtensionMutation>,
    },
}

/// An optional context policy; implementation owns model protocols and its reduction algorithm.
pub trait ContextPreparationHook: Send + Sync + fmt::Debug + 'static {
    /// Produces a candidate without mutating its caller. Must finish cleanup on cancellation.
    fn before_step(
        &self,
        request: ContextPreparationRequest,
    ) -> impl Future<Output = ContextPreparation> + Send;
}
trait ErasedHook: Send + Sync + fmt::Debug {
    fn before_step(
        &self,
        request: ContextPreparationRequest,
    ) -> futures::future::BoxFuture<'_, ContextPreparation>;
}
impl<T: ContextPreparationHook> ErasedHook for T {
    fn before_step(
        &self,
        request: ContextPreparationRequest,
    ) -> futures::future::BoxFuture<'_, ContextPreparation> {
        Box::pin(ContextPreparationHook::before_step(self, request))
    }
}

/// Shareable immutable policy. Invocation state belongs to the Thread-owned awaited future.
#[derive(Debug, Clone)]
pub struct ContextPreparer(Arc<dyn ErasedHook>);
impl ContextPreparer {
    /// Erases a policy without invoking it or opening model resources.
    pub fn new(hook: impl ContextPreparationHook) -> Self {
        Self(Arc::new(hook))
    }
}

impl Owner {
    pub(super) async fn prepare_context(
        &mut self,
        input: &StepInput,
        tools: Arc<[ModelToolDeclaration]>,
    ) -> Result<(), ThreadError> {
        let Some(hook) = self.context_preparation.clone() else {
            return Ok(());
        };
        let request = ContextPreparationRequest {
            model: ModelRequest {
                tool_call_mode: self.tools.freeze().call_mode(),
                solo_tool_ids: self.tools.freeze().solo_tool_ids(),
                thread_id: self.id.clone(),
                turn_id: input.turn_id.clone(),
                attempt_id: input.attempt_id.clone(),
                context: self.state.context.clone(),
                tools,
                committed_private_context: self.state.private_context.clone(),
                resources: self.resources.clone(),
                cancellation: input.cancellation.clone(),
                progress: None,
            },
            extensions: Arc::new(self.state.extensions.clone()),
            extension_sequence: self.state.extension_sequence,
            previous_usage: self
                .state
                .attempts
                .last()
                .and_then(RequestAttempt::usage)
                .or(self.state.last_attempt_usage.as_ref())
                .cloned(),
        };
        let result = self
            .await_with_mailbox(crate::error_record::catch_boundary(
                "context preparation",
                hook.0.before_step(request),
            ))
            .await
            .map_err(|source| {
                ThreadError::Model(Arc::new(ModelError {
                    kind: crate::model::ModelFailureKind::ImplementationPanicked,
                    details: None,
                    usage: Default::default(),
                    source: Some(Box::new(source)),
                }))
            })?;
        let (replacement, mutations, failure) = match result {
            ContextPreparation::Unchanged => return Ok(()),
            ContextPreparation::Replaced {
                replacement,
                mutations,
            } => (Some(replacement), mutations, None),
            ContextPreparation::Failed { error, mutations } => (None, mutations, Some(error)),
        };
        let mut candidate = self.state.clone();
        extensions::stage_extensions(&mut candidate, mutations)?;
        let cancelled = input.cancellation.is_cancelled() || self.interrupt.is_closing();
        let replacement_error = if !cancelled {
            replacement.and_then(|replacement| {
                super::replacement::stage_replacement(&mut candidate, replacement).err()
            })
        } else {
            None
        };
        // A rejected or cancelled replacement must still retain actual producer accounting.
        self.state = candidate;
        self.publish();
        if cancelled {
            return Err(ThreadError::Cancelled);
        }
        if let Some(error) = replacement_error {
            return Err(error);
        }
        if let Some(error) = failure {
            return Err(ThreadError::Model(Arc::new(error)));
        }
        Ok(())
    }
}

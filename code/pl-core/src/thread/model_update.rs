//! Deferred model configuration is consumed only at the next Turn boundary.
use super::*;

/// A host-frozen configuration; identity is compared without interpreting provider parameters.
#[derive(Debug, Clone)]
pub struct DeferredModelUpdate {
    identity: String,
    factory: crate::model::ModelFactory,
    preparation: Option<context_preparation::ContextPreparer>,
}

/// Host-owned facts that must still match before a deferred model binding is queued.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeferredModelUpdatePrecondition {
    commit_sequence: Option<u64>,
    extensions: Vec<(String, Option<u64>)>,
}

impl DeferredModelUpdatePrecondition {
    /// Requires the journal watermark to match before queueing the binding.
    pub fn commit_sequence(commit_sequence: u64) -> Self {
        Self {
            commit_sequence: Some(commit_sequence),
            extensions: Vec::new(),
        }
    }

    /// Requires one application extension to retain the observed revision or absence.
    pub fn extension(id: impl Into<String>, revision: Option<u64>) -> Self {
        Self {
            commit_sequence: None,
            extensions: vec![(id.into(), revision)],
        }
    }
}

impl DeferredModelUpdate {
    /// Freezes one host-selected model binding for application at the next Turn boundary.
    pub fn new(
        identity: String,
        factory: crate::model::ModelFactory,
        preparation: Option<context_preparation::ContextPreparer>,
    ) -> Self {
        Self {
            identity,
            factory,
            preparation,
        }
    }
}
impl Owner {
    pub(super) fn pending_model_update(
        &self,
        update: DeferredModelUpdate,
    ) -> Result<Option<DeferredModelUpdate>, ThreadError> {
        if update.identity.is_empty() {
            return Err(ThreadError::InvalidIdentity);
        }
        if self.state.lifecycle != ThreadLifecycle::Open || self.interrupt.is_closing() {
            return Err(ThreadError::Closed);
        }
        if self.model_identity.as_deref() == Some(&update.identity) {
            Ok(None)
        } else {
            Ok(Some(update))
        }
    }
    pub(super) fn queue_model_update(
        &mut self,
        update: DeferredModelUpdate,
        precondition: DeferredModelUpdatePrecondition,
        mutations: Vec<extensions::ExtensionMutation>,
    ) -> Result<ThreadSnapshot, ThreadError> {
        let pending = self.pending_model_update(update)?;
        if let Some(expected) = precondition.commit_sequence
            && self.state.commit_sequence != expected
        {
            return Err(ThreadError::ContextConflict {
                expected,
                actual: self.state.commit_sequence,
            });
        }
        for (id, expected) in precondition.extensions {
            if id.is_empty() {
                return Err(ThreadError::InvalidIdentity);
            }
            let actual = self.state.extensions.get(&id).map(|record| record.revision);
            if actual != expected {
                return Err(ThreadError::ExtensionConflict {
                    id,
                    expected,
                    actual,
                });
            }
        }
        let mut candidate = self.state.clone();
        extensions::stage_extensions(&mut candidate, mutations)?;
        self.state = candidate;
        self.pending_model_update = pending;
        self.publish();
        Ok(self.state.clone())
    }
    pub(super) async fn apply_model_update(&mut self) -> Result<(), ThreadError> {
        let Some(update) = self.pending_model_update.take() else {
            return Ok(());
        };
        if self.model_identity.as_deref() == Some(&update.identity) {
            return Ok(());
        }
        // While cleanup/open is in flight, reverting to the previous identity is a real update.
        self.model_identity = None;
        match self.replace_model(update.factory.clone()).await {
            Ok(()) => {
                self.model_identity = Some(update.identity);
                self.context_preparation = update.preparation;
                Ok(())
            }
            Err(error) => {
                // A newer configuration can supersede a failed update while cleanup is awaited.
                if self.pending_model_update.is_none() {
                    self.pending_model_update = Some(update);
                }
                Err(error)
            }
        }
    }
}

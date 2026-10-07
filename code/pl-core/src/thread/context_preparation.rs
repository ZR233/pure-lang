//! Optional host-owned context transformation before an ordinary model request is admitted.
use super::*;
use extensions::{ExtensionMutation, ExtensionRecord};
use std::{collections::BTreeMap, fmt, future::Future};

/// Frozen full request preview and committed history. New input is previewed, never consumed.
#[derive(Debug, Clone)]
pub struct ContextPreparationRequest {
    pub model: ModelRequest,
    /// One capacity-recovery pass before a main request has been admitted.
    pub force_compaction: bool,
    pub committed_context: ContextSnapshot,
    pub current_facts: Arc<[RuntimeFact]>,
    pub extensions: Arc<BTreeMap<String, ExtensionRecord>>,
    pub extension_sequence: u64,
    pub previous_usage: Option<PreviousContextUsage>,
}

/// Usage may assist capacity checks only for the exact retained context and bound route.
#[derive(Debug, Clone)]
pub struct PreviousContextUsage {
    pub usage: crate::model::ModelUsage,
    pub origin: ModelUsageOrigin,
}

/// Prepared replacement and producer facts. Successful replacement and receipts commit together.
#[derive(Debug)]
pub enum ContextPreparation {
    Unchanged,
    Prepared {
        expected_extension_sequence: u64,
        /// Producer accounting only; committed if the candidate is cancelled or rejected.
        rejected_mutations: Vec<ExtensionMutation>,
        replacement: Option<ReplaceContext>,
        facts: Vec<RuntimeFact>,
        mutations: Vec<ExtensionMutation>,
    },
    Failed {
        error: ModelError,
        /// Observed accounting only, independently committed against the current ledger revision.
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
        plan: &crate::tool::opaque::ToolPlan,
        frozen_input: &[ContextRecord],
        force_compaction: bool,
    ) -> Result<(), ThreadError> {
        let Some(hook) = self.context_preparation.clone() else {
            return Ok(());
        };
        let mut preview_records = self.state.context.records.to_vec();
        preview_records.extend_from_slice(frozen_input);
        let preview = ContextSnapshot {
            revision: if frozen_input.is_empty() {
                self.state.context.revision
            } else {
                self.state
                    .context
                    .revision
                    .checked_add(1)
                    .ok_or(ThreadError::RevisionExhausted)?
            },
            records: preview_records.into(),
        };
        preview.validate_complete()?;
        let expected_context_revision = self.state.context.revision;
        let expected_extension_sequence = self.state.extension_sequence;
        let request = ContextPreparationRequest {
            force_compaction,
            model: ModelRequest {
                tool_call_mode: plan.call_mode(),
                solo_tool_ids: plan.solo_tool_ids(),
                thread_id: self.id.clone(),
                turn_id: input.turn_id.clone(),
                attempt_id: input.attempt_id.clone(),
                context: preview,
                tools: plan.declarations(),
                committed_private_context: self.state.private_context.clone(),
                resources: self.resources.clone(),
                cancellation: input.cancellation.clone(),
                progress: None,
            },
            committed_context: self.state.context.clone(),
            current_facts: self.state.runtime_facts.clone(),
            extensions: Arc::new(self.state.extensions.clone()),
            extension_sequence: expected_extension_sequence,
            // Unbound historical usage must never stand in for the current frozen input.
            previous_usage: self
                .state
                .last_attempt_usage_origin
                .as_ref()
                .filter(|origin| {
                    origin.binding.route_identity.is_some() && origin.input_hash.is_some()
                })
                .filter(|origin| {
                    !self
                        .state
                        .context
                        .records
                        .get(origin.input_record_count..)
                        .is_none_or(|tail| {
                            tail.iter().any(|record| {
                                matches!(
                                    record.source,
                                    ContextSource::Instruction
                                        | ContextSource::InstructionSnapshot { .. }
                                )
                            })
                        })
                })
                .filter(|origin| {
                    self.state
                        .context
                        .records
                        .get(..origin.input_record_count)
                        .is_some_and(|records| {
                            input_hash(
                                records,
                                &plan.declarations(),
                                plan.call_mode(),
                                &plan.solo_tool_ids(),
                            )
                            .ok()
                            .as_ref()
                                == origin.input_hash.as_ref()
                        })
                })
                .and_then(|origin| {
                    self.state
                        .last_attempt_usage
                        .clone()
                        .map(|usage| PreviousContextUsage {
                            usage,
                            origin: origin.clone(),
                        })
                }),
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
        if input.cancellation.is_cancelled() || self.interrupt.is_closing() {
            let mutations = match result {
                ContextPreparation::Prepared {
                    rejected_mutations, ..
                } => rejected_mutations,
                ContextPreparation::Failed { mutations, .. } => mutations,
                ContextPreparation::Unchanged => Vec::new(),
            };
            self.commit_preparation_accounting(mutations)?;
            return Err(ThreadError::Cancelled);
        }
        match result {
            ContextPreparation::Unchanged => Ok(()),
            ContextPreparation::Failed { error, mutations } => {
                self.commit_preparation_accounting(mutations)?;
                Err(ThreadError::Model(Arc::new(error)))
            }
            ContextPreparation::Prepared {
                expected_extension_sequence: producer_sequence,
                rejected_mutations,
                replacement,
                facts,
                mutations,
            } => {
                if self.state.context.revision != expected_context_revision {
                    self.commit_preparation_accounting(rejected_mutations)?;
                    return Err(ThreadError::ContextConflict {
                        expected: expected_context_revision,
                        actual: self.state.context.revision,
                    });
                }
                if producer_sequence != expected_extension_sequence
                    || self.state.extension_sequence != expected_extension_sequence
                {
                    self.commit_preparation_accounting(rejected_mutations)?;
                    return Err(ThreadError::ExtensionConflict {
                        id: "context preparation".into(),
                        expected: Some(expected_extension_sequence),
                        actual: Some(self.state.extension_sequence),
                    });
                }
                let mut candidate = self.state.clone();
                let staged = (|| {
                    if let Some(replacement) = replacement {
                        // Validate/normalize current facts before restoring the new window, while
                        // retaining the actually committed history as the replacement's predecessor.
                        let committed = candidate.context.clone();
                        super::facts::stage_facts(&mut candidate, facts)?;
                        candidate.context = committed;
                        super::replacement::stage_replacement(&mut candidate, replacement)?;
                    } else {
                        super::facts::stage_facts(&mut candidate, facts)?;
                    }
                    extensions::stage_extensions(&mut candidate, mutations)?;
                    Ok::<_, ThreadError>(())
                })();
                if let Err(error) = staged {
                    self.commit_preparation_accounting(rejected_mutations)?;
                    return Err(error);
                }
                if candidate.context != self.state.context
                    || candidate.runtime_facts != self.state.runtime_facts
                    || candidate.extensions != self.state.extensions
                {
                    self.state = candidate;
                    self.publish();
                }
                Ok(())
            }
        }
    }
    fn commit_preparation_accounting(
        &mut self,
        mut mutations: Vec<ExtensionMutation>,
    ) -> Result<(), ThreadError> {
        if !mutations.is_empty() {
            // The producer has already incurred this accounting. A rejected candidate cannot
            // use an obsolete CAS to erase it; execution mutations never enter this path.
            for mutation in &mut mutations {
                match mutation {
                    ExtensionMutation::Put {
                        id,
                        expected_revision,
                        ..
                    } => {
                        *expected_revision =
                            self.state.extensions.get(id).map(|record| record.revision);
                    }
                    ExtensionMutation::Delete {
                        id,
                        expected_revision,
                    } => {
                        if let Some(record) = self.state.extensions.get(id) {
                            *expected_revision = record.revision;
                        }
                    }
                }
            }
            let mut candidate = self.state.clone();
            extensions::stage_extensions(&mut candidate, mutations)?;
            self.state = candidate;
            self.publish();
        }
        Ok(())
    }
}

/// Immutable input prefix and tool plan identity; no producer contents enter diagnostics.
pub(super) fn input_hash(
    records: &[ContextRecord],
    tools: &[ModelToolDeclaration],
    mode: crate::model::ToolCallMode,
    solo: &[String],
) -> Result<String, ThreadError> {
    let mode = match mode {
        crate::model::ToolCallMode::Sequential => "sequential",
        crate::model::ToolCallMode::Parallel => "parallel",
    };
    let bytes = serde_json::to_vec(&(records, tools, mode, solo))
        .map_err(|_| ThreadError::InvalidContext)?;
    Ok(crate::context::content_hash(&bytes))
}

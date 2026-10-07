//! Studio wording for model-owned context compaction.
pub(crate) const LATEST_RECEIPT: &str = "studio.compaction.latest";

use pl_model::{
    completion::OpenAiCompactionMode,
    runtime::{ThreadCompactionOptions, ThreadCompactionStrategy},
};

/// Selects the product summary policy without changing provider capabilities.
pub(crate) fn policy(mode: OpenAiCompactionMode) -> ThreadCompactionOptions {
    ThreadCompactionOptions {
        strategy: match mode {
            OpenAiCompactionMode::Local => ThreadCompactionStrategy::TextSummary,
            OpenAiCompactionMode::RemoteV2 => ThreadCompactionStrategy::PreferNative,
        },
        instructions: include_str!("prompts/compact.md").into(),
        requirement: "请根据以上完整上下文生成压缩摘要。".into(),
        summary_prefix: "以下是此前对话的压缩摘要。".into(),
        max_output_tokens: None,
    }
}

use pl_core::thread::{
    context_preparation::{
        ContextPreparation, ContextPreparationHook, ContextPreparationRequest, ContextPreparer,
    },
    extensions::ExtensionMutation,
};
use pl_model::{
    config::ResolvedModelRoute,
    runtime::{ModelRuntime, ThreadModel},
};

/// Binds the policy to the same frozen route as the main model session.
pub(crate) fn preparer(
    route: &ResolvedModelRoute,
    mode: OpenAiCompactionMode,
    modes: crate::mode::ThreadModeManager,
) -> Result<Option<ContextPreparer>, pl_model::PureError> {
    let model = ThreadModel::new(ModelRuntime::from_route(route)?, route.reasoning_config());
    let instruction_binding = InstructionBinding {
        route_identity: model.route_identity(),
        append_snapshots: route.endpoint.instruction_update_strategy(&route.model)
            == pl_model::provider::InstructionUpdateStrategy::AppendFullSnapshot,
    };
    Ok(Some(ContextPreparer::new(StudioCompaction {
        model,
        limit: route.auto_compact_limit,
        modes,
        reasoning_effort: route
            .effort
            .as_ref()
            .map(|effort| effort.as_str().to_owned()),
        context_window: route.model.resolved_context_window(),
        options: policy(mode),
        base_instructions: route.model.base_instructions.clone(),
        instruction_binding,
    })))
}

#[derive(Debug)]
struct StudioCompaction {
    model: ThreadModel,
    limit: Option<u64>,
    modes: crate::mode::ThreadModeManager,
    reasoning_effort: Option<String>,
    context_window: Option<u64>,
    options: ThreadCompactionOptions,
    base_instructions: String,
    instruction_binding: InstructionBinding,
}
impl ContextPreparationHook for StudioCompaction {
    async fn before_step(&self, mut request: ContextPreparationRequest) -> ContextPreparation {
        let instruction_repair = match restore_current_instructions(
            &mut request,
            &self.base_instructions,
            &self.instruction_binding,
        ) {
            Ok(repair) => repair,
            Err(error) => {
                return ContextPreparation::Failed {
                    error,
                    mutations: vec![],
                };
            }
        };
        let facts = match current_facts(&request, &self.modes) {
            Ok(facts) => facts,
            Err(error) => {
                return ContextPreparation::Failed {
                    error,
                    mutations: vec![],
                };
            }
        };
        let unchanged = || ContextPreparation::Prepared {
            expected_extension_sequence: request.extension_sequence,
            rejected_mutations: Vec::new(),
            replacement: instruction_repair.replacement.as_ref().map(|repair| {
                pl_core::thread::ReplaceContext {
                    expected_revision: repair.expected_revision,
                    reason: repair.reason,
                    records: repair.records.clone(),
                }
            }),
            facts: facts.clone(),
            mutations: instruction_repair.mutations.clone(),
        };
        let limit = match self.limit {
            Some(limit) => limit,
            None if request.force_compaction => u64::MAX,
            None => return unchanged(),
        };
        let mut preview = request.model.clone();
        let mut records = preview.context.records.to_vec();
        // Capacity includes freshly projected state, as well as this step's frozen input.
        for fact in &facts {
            let source = pl_core::context::ContextSource::RuntimeFact {
                source_id: fact.source_id.clone(),
            };
            if !records
                .iter()
                .rev()
                .find(|record| record.source == source)
                .is_some_and(|record| record.content == fact.content)
            {
                records.push(pl_core::context::ContextRecord {
                    id: format!("preview:{}:{}", request.extension_sequence, fact.source_id),
                    turn_id: None,
                    source,
                    content: fact.content.clone(),
                    tool_calls: Vec::new(),
                });
            }
        }
        preview.context.records = records.into();
        let estimate = match self
            .model
            .estimate_input(&preview, request.previous_usage.as_ref())
            .await
        {
            Ok(estimate) => estimate.map(|estimate| estimate.tokens),
            Err(error) => {
                return ContextPreparation::Failed {
                    error,
                    mutations: vec![],
                };
            }
        };
        if request
            .committed_context
            .records
            .iter()
            .all(|record| record.source == pl_core::context::ContextSource::Instruction)
            || (!request.force_compaction && !estimate.is_some_and(|tokens| tokens >= limit))
        {
            return unchanged();
        }
        let turn_id = request.model.turn_id.clone();
        let id = format!(
            "studio.compaction:{}:{}",
            request.model.attempt_id, request.extension_sequence
        );
        let previous = request
            .extensions
            .get(LATEST_RECEIPT)
            .map(|record| record.revision);
        let mut model_request = request.model;
        model_request.context = request.committed_context;
        model_request.attempt_id = id.clone();
        match self
            .model
            .compact(model_request, self.options.clone())
            .await
        {
            Ok(result) => {
                let receipt = CompactionReceipt {
                    inference_id: id.clone(),
                    binding: result.binding,
                    reasoning_effort: self.reasoning_effort.clone(),
                    context_window: self.context_window,
                    turn_id: turn_id.clone(),
                    accounting: result.accounting,
                    model_observation: result.model_observation,
                    implementation: Some(
                        match result.implementation {
                            ThreadCompactionStrategy::TextSummary => "textSummary",
                            ThreadCompactionStrategy::PreferNative => "native",
                        }
                        .into(),
                    ),
                    error: None,
                };
                let mut rejected = receipt.clone();
                rejected.error =
                    Some("context preparation candidate was cancelled or rejected".into());
                let rejected_mutations = match receipt_mutation(previous, rejected) {
                    Ok(mutation) => vec![mutation],
                    Err(error) => {
                        return ContextPreparation::Failed {
                            error,
                            mutations: vec![],
                        };
                    }
                };
                match receipt_mutation(previous, receipt) {
                    Ok(mutation) => ContextPreparation::Prepared {
                        expected_extension_sequence: request.extension_sequence,
                        rejected_mutations,
                        replacement: Some(result.replacement),
                        facts,
                        mutations: instruction_repair
                            .mutations
                            .clone()
                            .into_iter()
                            .chain(std::iter::once(mutation))
                            .collect(),
                    },
                    Err(error) => ContextPreparation::Failed {
                        error,
                        mutations: vec![],
                    },
                }
            }
            Err(error) => {
                let receipt = pl_model::runtime::model_failure_receipt(&error)
                    .ok()
                    .flatten()
                    .map(|receipt| CompactionReceipt {
                        inference_id: id.clone(),
                        binding: receipt.binding,
                        reasoning_effort: self.reasoning_effort.clone(),
                        context_window: self.context_window,
                        turn_id: turn_id.clone(),
                        accounting: receipt.accounting,
                        model_observation: receipt.model_observation,
                        implementation: None,
                        error: Some(receipt.message),
                    });
                let mutations = receipt
                    .and_then(|receipt| receipt_mutation(previous, receipt).ok())
                    .into_iter()
                    .collect();
                ContextPreparation::Failed { error, mutations }
            }
        }
    }
}

/// Rewind and old checkpoints may carry obsolete instructions. The current extension is the
/// host authority; normalize obsolete text or route bindings without rescanning files.
#[derive(Default)]
struct InstructionRepair {
    replacement: Option<pl_core::thread::ReplaceContext>,
    mutations: Vec<ExtensionMutation>,
}
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstructionBinding {
    route_identity: String,
    append_snapshots: bool,
}
fn restore_current_instructions(
    request: &mut ContextPreparationRequest,
    base_instructions: &str,
    binding: &InstructionBinding,
) -> Result<InstructionRepair, pl_core::model::ModelError> {
    use pl_core::context::{ContextContent, ContextRecord, ContextSource};
    let Some(mut record) = request.extensions.get("studio.instructions").cloned() else {
        return Ok(InstructionRepair::default());
    };
    if record.payload.format() != "pl.studio.instructions"
        || record.payload.version() != 1
        || record.revision == 0
    {
        return Err(receipt_error(std::io::Error::other(
            "invalid current instruction snapshot authority",
        )));
    }
    let mut snapshot: crate::instruction::InstructionSnapshot =
        serde_json::from_str(record.payload.content()).map_err(receipt_error)?;
    let mut mutations = Vec::new();
    let previous_binding = request.extensions.get("studio.instructions.binding");
    let binding_changed = match previous_binding {
        Some(record) => {
            if record.payload.format() != "pl.studio.instruction-binding"
                || record.payload.version() != 1
                || record.revision == 0
            {
                return Err(receipt_error(std::io::Error::other(
                    "invalid instruction route binding",
                )));
            }
            let previous: InstructionBinding =
                serde_json::from_str(record.payload.content()).map_err(receipt_error)?;
            if previous.route_identity.is_empty() {
                return Err(receipt_error(std::io::Error::other(
                    "empty instruction route binding",
                )));
            }
            previous != *binding
        }
        None => true,
    };
    if binding_changed {
        mutations.push(ExtensionMutation::Put {
            id: "studio.instructions.binding".into(),
            expected_revision: previous_binding.map(|record| record.revision),
            payload: pl_core::context::OpaquePayload::new(
                "pl.studio.instruction-binding",
                1,
                serde_json::to_string(binding).map_err(receipt_error)?,
            )
            .map_err(receipt_error)?,
        });
    }
    if snapshot.rebind_model_base(base_instructions) {
        let expected_revision = record.revision;
        record.revision = request
            .extension_sequence
            .checked_add(u64::try_from(mutations.len()).map_err(receipt_error)?)
            .and_then(|sequence| sequence.checked_add(1))
            .ok_or_else(|| {
                receipt_error(std::io::Error::other("instruction revision exhausted"))
            })?;
        record.payload = pl_core::context::OpaquePayload::new(
            "pl.studio.instructions",
            1,
            serde_json::to_string(&snapshot).map_err(receipt_error)?,
        )
        .map_err(receipt_error)?;
        mutations.push(ExtensionMutation::Put {
            id: "studio.instructions".into(),
            expected_revision: Some(expected_revision),
            payload: record.payload.clone(),
        });
        std::sync::Arc::make_mut(&mut request.extensions)
            .insert("studio.instructions".into(), record.clone());
    }
    let rendered = snapshot.context_records(&request.model.thread_id);
    let complete = rendered
        .iter()
        .filter(|record| record.source == ContextSource::Instruction)
        .flat_map(|record| &record.content)
        .filter_map(|content| match content {
            ContextContent::Text { text } => Some(text.as_ref()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let hash = pl_core::context::content_hash(complete.as_bytes());
    let latest = request
        .committed_context
        .records
        .iter()
        .rev()
        .find(|record| matches!(record.source, ContextSource::InstructionSnapshot { .. }));
    let current = if let Some(record) = latest {
        record
            .content
            .iter()
            .filter_map(|content| match content {
                ContextContent::Text { text } => Some(text.as_ref()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        request
            .committed_context
            .records
            .iter()
            .filter(|record| record.source == ContextSource::Instruction)
            .flat_map(|record| &record.content)
            .filter_map(|content| match content {
                ContextContent::Text { text } => Some(text.as_ref()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    if current == complete && (!binding_changed || latest.is_none()) {
        return Ok(InstructionRepair {
            replacement: None,
            mutations,
        });
    }
    let replaces = latest.and_then(|record| match &record.source {
        ContextSource::InstructionSnapshot {
            content_hash,
            replaces,
            ..
        } => {
            if current == complete {
                replaces.clone()
            } else {
                Some(content_hash.clone())
            }
        }
        _ => None,
    });
    let canonical = ContextRecord {
        id: format!(
            "instruction:{}:restore:{}",
            request.model.thread_id, record.revision
        ),
        turn_id: None,
        source: ContextSource::InstructionSnapshot {
            revision: record.revision,
            content_hash: hash,
            replaces,
        },
        content: vec![ContextContent::Text {
            text: complete.into(),
        }],
        tool_calls: Vec::new(),
    };
    let normalize = |records: &[ContextRecord]| {
        std::iter::once(canonical.clone())
            .chain(
                records
                    .iter()
                    .filter(|record| {
                        !matches!(
                            record.source,
                            ContextSource::Instruction | ContextSource::InstructionSnapshot { .. }
                        )
                    })
                    .cloned(),
            )
            .collect::<Vec<_>>()
    };
    request.model.context.records = normalize(&request.model.context.records).into();
    // Core validated usage against the frozen pre-repair prefix. Rebuilding it invalidates
    // that proof even if the record count happens to remain equal.
    request.previous_usage = None;
    let records = normalize(&request.committed_context.records);
    request.committed_context.records = records.clone().into();
    Ok(InstructionRepair {
        replacement: Some(pl_core::thread::ReplaceContext {
            expected_revision: request.committed_context.revision,
            reason: pl_core::thread::ContextReplacementReason::Rebuild,
            records,
        }),
        mutations,
    })
}

/// Each projection consumes only this Thread's frozen extensions, never a parent's state.
fn current_facts(
    request: &ContextPreparationRequest,
    modes: &crate::mode::ThreadModeManager,
) -> Result<Vec<pl_core::thread::RuntimeFact>, pl_core::model::ModelError> {
    use pl_core::{context::ContextContent, thread::RuntimeFact};
    let mut facts = request
        .current_facts
        .iter()
        .filter(|fact| {
            !matches!(
                fact.source_id.as_str(),
                "studio.plan" | "studio.plan.document" | "studio.workflow" | "studio.workspace"
            ) && !fact.source_id.starts_with("studio.instructions:")
        })
        .cloned()
        .map(|fact| (fact.source_id.clone(), fact))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut put = |source_id: &str, content: String| {
        facts.insert(
            source_id.into(),
            RuntimeFact {
                source_id: source_id.into(),
                content: vec![ContextContent::Text {
                    text: content.into(),
                }],
            },
        );
    };
    if let Some(record) = request.extensions.get(crate::plan_tool::PLAN_EXTENSION) {
        let state = crate::plan_tool::decode_plan_state(&record.payload).map_err(receipt_error)?;
        let section =
            crate::plan_tool::plan_model_context_section(&state).map_err(receipt_error)?;
        let mut metadata: serde_json::Value =
            serde_json::from_str(&section.content).map_err(receipt_error)?;
        if let Some(document) = &state.document {
            put("studio.plan.document", serde_json::to_string(&serde_json::json!({"version":document.version,"contentHash":document.content_hash,"markdown":document.markdown})).map_err(receipt_error)?);
            if let Some(document) = metadata
                .get_mut("document")
                .and_then(serde_json::Value::as_object_mut)
            {
                document.remove("markdown");
            }
        } else {
            put(
                "studio.plan.document",
                "Previous plan document is no longer current.".into(),
            );
        }
        put(
            "studio.plan",
            serde_json::to_string(&metadata).map_err(receipt_error)?,
        );
    }
    if let Some(record) = request
        .extensions
        .get(crate::workflow_tool::WORKFLOW_EXTENSION)
    {
        let state =
            crate::workflow_tool::decode_workflow_state(&record.payload).map_err(receipt_error)?;
        if let Some(run) = &state.current_run {
            let mode = modes.snapshot().mode(&run.mode_id).ok_or_else(|| {
                receipt_error(std::io::Error::other(
                    "current workflow mode is unavailable",
                ))
            })?;
            let section =
                crate::mode::workflow_model_context_section(&state, &mode).ok_or_else(|| {
                    receipt_error(std::io::Error::other(
                        "current workflow state does not match its registered graph",
                    ))
                })?;
            put("studio.workflow", section.content);
        }
    }
    if let Some(record) = request.extensions.get("studio.workspace") {
        put(
            "studio.workspace",
            format!("Assigned workspace:\n{}", record.payload.content()),
        );
    }
    if let Some(record) = request.extensions.get("studio.instructions") {
        let snapshot: crate::instruction::InstructionSnapshot =
            serde_json::from_str(record.payload.content()).map_err(receipt_error)?;
        for record in snapshot.context_records(&request.model.thread_id) {
            if let pl_core::context::ContextSource::RuntimeFact { source_id } = record.source {
                facts.insert(
                    source_id.clone(),
                    RuntimeFact {
                        source_id,
                        content: record.content,
                    },
                );
            }
        }
    }
    Ok(facts.into_values().collect())
}

/// Auxiliary model accounting is retained separately from normal turn-response receipts.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompactionReceipt {
    pub inference_id: String,
    pub binding: pl_model::runtime::ModelCallBinding,
    pub reasoning_effort: Option<String>,
    pub context_window: Option<u64>,
    pub turn_id: String,
    pub accounting: pl_model::completion::InferenceAccounting,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_observation: Option<pl_protocol::InferenceModelObservation>,
    pub implementation: Option<String>,
    pub error: Option<String>,
}
fn receipt_mutation(
    expected_revision: Option<u64>,
    receipt: CompactionReceipt,
) -> Result<ExtensionMutation, pl_core::model::ModelError> {
    let payload = serde_json::to_string(&receipt).map_err(receipt_error)?;
    let payload = pl_core::context::OpaquePayload::new("pl.studio.compaction", 1, payload)
        .map_err(receipt_error)?;
    Ok(ExtensionMutation::Put {
        id: LATEST_RECEIPT.into(),
        expected_revision,
        payload,
    })
}
fn receipt_error(
    source: impl std::error::Error + Send + Sync + 'static,
) -> pl_core::model::ModelError {
    pl_core::model::ModelError {
        kind: pl_core::model::ModelFailureKind::InvalidResponse,
        details: None,
        usage: Default::default(),
        source: Some(Box::new(source)),
    }
}

/// Producer payload conversion is confined to the persisted checkpoint migration boundary.
pub(crate) fn migrate_checkpoint_sources(
    checkpoint: &mut pl_core::thread::ThreadCheckpoint,
) -> anyhow::Result<()> {
    use pl_core::context::ContextSource;
    for record in std::sync::Arc::make_mut(&mut checkpoint.state.context.records) {
        if let ContextSource::Runtime { source_id } = &record.source
            && ((source_id == "studio.workspace"
                && record.id == format!("workspace:{}", checkpoint.thread_id))
                || (source_id == "studio.workflow"
                    && record.id == format!("workflow:{}:initial", checkpoint.thread_id)))
        {
            record.source = ContextSource::RuntimeFact {
                source_id: source_id.clone(),
            };
        }
    }
    let Some(record) = checkpoint.state.extensions.get(LATEST_RECEIPT) else {
        return Ok(());
    };
    if record.payload.format() != "pl.studio.compaction" || record.payload.version() != 1 {
        return Ok(());
    }
    let receipt: CompactionReceipt = serde_json::from_str(record.payload.content())?;
    if receipt.error.is_some() || receipt.implementation.as_deref() != Some("native") {
        return Ok(());
    }
    for record in std::sync::Arc::make_mut(&mut checkpoint.state.context.records) {
        pl_model::runtime::migrate_legacy_compaction(
            record,
            &receipt.inference_id,
            receipt.binding.clone(),
        )?;
    }
    Ok(())
}

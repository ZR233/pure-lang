//! Frozen model-owned assistant frames, with source-role validation on every replay.
use super::{AdapterError, failure, usage};
use crate::completion::{
    CompletionRequest, CompletionResponse, Message, MessageContent, MessageRole, ModelContextItem,
    ToolCall, ToolSpec,
};
use pl_core::{
    context::{ContextContent, ContextSource, OpaquePayload},
    model::{
        ModelError, ModelFailureKind, ModelRequest, ModelStepOutput, ModelToolCall,
        ModelToolDeclaration,
    },
};
use std::collections::{BTreeMap, BTreeSet};

const FRAME_FORMAT: &str = "pl.model.assistant";
const FRAME_VERSION: u32 = 2;

#[derive(Clone, Copy)]
enum ArgumentKind {
    Json,
    Text,
}

pub(super) struct ToolBinding {
    id: String,
    arguments: ArgumentKind,
}

impl ToolBinding {
    pub(super) fn new(id: String, spec: &ToolSpec) -> Self {
        Self {
            id,
            arguments: match spec {
                ToolSpec::Custom { .. } => ArgumentKind::Text,
                ToolSpec::Function { .. }
                | ToolSpec::WebSearch { .. }
                | ToolSpec::ProgrammaticToolCalling => ArgumentKind::Json,
            },
        }
    }
}

fn arguments(call: &ToolCall, kind: ArgumentKind) -> Result<OpaquePayload, ModelError> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct CustomEnvelope {
        input: String,
    }
    let (format, content) = match (kind, call.kind()) {
        (ArgumentKind::Json, pl_protocol::ToolCallKind::Function) => {
            ("application/json", call.payload_text())
        }
        (ArgumentKind::Text, pl_protocol::ToolCallKind::Custom) => {
            ("text/plain", call.payload_text())
        }
        (ArgumentKind::Text, pl_protocol::ToolCallKind::Function) => {
            let envelope: CustomEnvelope = serde_json::from_str(&call.payload_text())
                .map_err(|error| failure(ModelFailureKind::InvalidResponse, error))?;
            ("text/plain", envelope.input)
        }
        (ArgumentKind::Json, pl_protocol::ToolCallKind::Custom) => {
            return Err(invalid("function tool returned custom wire arguments"));
        }
    };
    OpaquePayload::new(format, 1, content)
        .map_err(|error| failure(ModelFailureKind::InvalidResponse, error))
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssistantFrame {
    receipt: super::receipt::ModelResponseReceipt,
    bindings: Vec<ModelToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    compatibility_family: Option<String>,
}

pub(super) fn receipt(
    output: &ModelStepOutput,
) -> Result<Option<super::receipt::ModelResponseReceipt>, ModelError> {
    let mut frames = output.content.iter().filter_map(|content| match content {
        ContextContent::Opaque { payload } if payload.format() == FRAME_FORMAT => Some(payload),
        ContextContent::Text { .. }
        | ContextContent::Resource { .. }
        | ContextContent::Opaque { .. } => None,
    });
    let Some(payload) = frames.next() else {
        return Ok(None);
    };
    if frames.next().is_some() || payload.version() != FRAME_VERSION {
        return Err(invalid("unsupported or repeated model response receipt"));
    }
    let frame: AssistantFrame = serde_json::from_str(payload.content())
        .map_err(|source| failure(ModelFailureKind::UnsupportedContent, source))?;
    let text = output
        .content
        .iter()
        .filter_map(|content| match content {
            ContextContent::Text { text } => Some(text.as_ref()),
            ContextContent::Opaque { .. } | ContextContent::Resource { .. } => None,
        })
        .collect::<Vec<_>>();
    if text
        != frame
            .receipt
            .response
            .content
            .as_deref()
            .into_iter()
            .collect::<Vec<_>>()
    {
        return Err(invalid("response receipt differs from its visible text"));
    }
    if frame.bindings != output.tool_calls {
        return Err(invalid("response receipt differs from its tool bindings"));
    }
    Ok(Some(frame.receipt))
}

fn invalid(message: &'static str) -> ModelError {
    failure(
        ModelFailureKind::UnsupportedContent,
        AdapterError::Content(message),
    )
}

pub(super) fn declarations(
    tools: &[ModelToolDeclaration],
) -> Result<Vec<(String, ToolSpec)>, ModelError> {
    let mut names = BTreeSet::new();
    let mut ids = BTreeSet::new();
    tools
        .iter()
        .map(|tool| {
            if tool.declaration.format() != "pl.model.tool-spec" || tool.declaration.version() != 1
            {
                return Err(invalid("unknown tool declaration format"));
            }
            let spec: ToolSpec = serde_json::from_str(tool.declaration.content())
                .map_err(|error| failure(ModelFailureKind::UnsupportedContent, error))?;
            if tool.tool_id.is_empty()
                || !ids.insert(tool.tool_id.clone())
                || !names.insert(spec.name().to_owned())
            {
                return Err(invalid("duplicate or empty tool identity"));
            }
            Ok((tool.tool_id.clone(), spec))
        })
        .collect()
}

pub(super) fn request(
    request: &ModelRequest,
    runtime: &super::ModelRuntime,
) -> Result<CompletionRequest, ModelError> {
    request
        .context
        .validate_complete()
        .map_err(|error| failure(ModelFailureKind::IncompatibleContext, error))?;
    let mut instructions = Vec::new();
    if request.tool_call_mode == pl_core::model::ToolCallMode::Sequential {
        instructions.push("Call at most one tool in each model response. Wait for its result before making another tool call.".to_owned());
    }
    if !request.solo_tool_ids.is_empty()
        && request.tool_call_mode == pl_core::model::ToolCallMode::Parallel
    {
        let declarations = declarations(&request.tools)?;
        let names = request
            .solo_tool_ids
            .iter()
            .map(|id| {
                declarations
                    .iter()
                    .find(|(tool_id, _)| tool_id == id)
                    .map(|(_, declaration)| declaration.name())
                    .ok_or_else(|| invalid("solo tool identity is absent from the frozen catalog"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        instructions.push(format!("These tools must each be called alone in a model response: {}. Never combine one of these with any other tool call. Other independent tools may be called together in parallel.", names.join(", ")));
    }
    let mut input = Vec::new();
    let mut replay_spans = Vec::new();
    let mut calls = BTreeMap::new();
    let mut pending_calls = BTreeSet::new();
    let mut call_order = Vec::new();
    let mut result_messages = BTreeMap::new();
    let mut tool_media = Vec::new();
    let mut previous_snapshot: Option<(u64, &str)> = None;
    for record in request.context.records.iter() {
        if let ContextSource::InstructionSnapshot {
            revision,
            content_hash,
            replaces,
        } = &record.source
        {
            if let Some((previous_revision, previous_hash)) = previous_snapshot
                && (*revision <= previous_revision || replaces.as_deref() != Some(previous_hash))
            {
                return Err(invalid(
                    "instruction snapshot revision or predecessor is inconsistent",
                ));
            }
            previous_snapshot = Some((*revision, content_hash));
        }
    }
    let append = runtime
        .endpoint()
        .instruction_update_strategy(runtime.model())
        == crate::provider::InstructionUpdateStrategy::AppendFullSnapshot;
    let latest_snapshot = request
        .context
        .records
        .iter()
        .rev()
        .find(|record| matches!(record.source, ContextSource::InstructionSnapshot { .. }));
    let records = if !append && latest_snapshot.is_some() {
        latest_snapshot
            .into_iter()
            .chain(request.context.records.iter().filter(|record| {
                !matches!(
                    record.source,
                    ContextSource::Instruction | ContextSource::InstructionSnapshot { .. }
                )
            }))
            .collect::<Vec<_>>()
    } else {
        request.context.records.iter().collect::<Vec<_>>()
    };
    for record in records {
        let snapshot = matches!(record.source, ContextSource::InstructionSnapshot { .. });
        if record.source == ContextSource::Instruction || snapshot {
            let text = record
                .content
                .iter()
                .map(|content| match content {
                    ContextContent::Text { text } => Ok(text.as_ref()),
                    _ => Err(invalid("instructions require text")),
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("\n");
            if let ContextSource::InstructionSnapshot {
                revision,
                content_hash,
                ..
            } = &record.source
                && (*revision == 0
                    || pl_core::context::content_hash(text.as_bytes()) != *content_hash)
            {
                return Err(invalid(
                    "invalid instruction snapshot revision or content hash",
                ));
            }
            if snapshot && append && !input.is_empty() {
                input.push(ModelContextItem::from(Message {
                    presentation: pl_protocol::MessagePresentation::Hidden,
                    role: MessageRole::System,
                    content: MessageContent::text(format!("Current complete host instruction snapshot. This supersedes every earlier host instruction snapshot.\n{text}")),
                    reasoning_content: None, tool_calls: None, tool_result: None, metadata: Default::default(),
                }));
                continue;
            }
            if !input.is_empty() {
                return Err(invalid(
                    "tail instructions require an explicitly supported update strategy",
                ));
            }
            for content in &record.content {
                let ContextContent::Text { text } = content else {
                    return Err(invalid("instructions require text"));
                };
                instructions.push(text.to_string());
            }
            continue;
        }
        if let Some(checkpoint) = super::compaction::decode(record, runtime)? {
            if !pending_calls.is_empty() {
                return Err(invalid("compaction splits pending tool calls"));
            }
            input.push(checkpoint);
            continue;
        }
        if record.source == ContextSource::Assistant {
            let frames = record
                .content
                .iter()
                .filter_map(|content| match content {
                    ContextContent::Opaque { payload } if payload.format() == FRAME_FORMAT => {
                        Some(payload)
                    }
                    ContextContent::Text { .. }
                    | ContextContent::Resource { .. }
                    | ContextContent::Opaque { .. } => None,
                })
                .collect::<Vec<_>>();
            if let [payload] = frames.as_slice() {
                if payload.version() != FRAME_VERSION {
                    return Err(invalid("unknown assistant frame version"));
                }
                let mut frame: AssistantFrame = serde_json::from_str(payload.content())
                    .map_err(|error| failure(ModelFailureKind::IncompatibleContext, error))?;
                let text = record
                    .content
                    .iter()
                    .filter_map(|content| match content {
                        ContextContent::Text { text } => Some(text.as_ref()),
                        ContextContent::Resource { .. } | ContextContent::Opaque { .. } => None,
                    })
                    .collect::<Vec<_>>();
                if frame.bindings != record.tool_calls
                    || frame.receipt.response.tool_calls.len() != frame.bindings.len()
                    || text
                        != frame
                            .receipt
                            .response
                            .content
                            .as_deref()
                            .into_iter()
                            .collect::<Vec<_>>()
                    || record.content.len() != frames.len() + text.len()
                {
                    return Err(invalid("assistant frame differs from its context envelope"));
                }
                for (call, binding) in frame
                    .receipt
                    .response
                    .tool_calls
                    .iter()
                    .zip(&frame.bindings)
                {
                    let kind = match binding.arguments.format() {
                        "application/json" => ArgumentKind::Json,
                        "text/plain" => ArgumentKind::Text,
                        _ => {
                            return Err(invalid("unknown tool argument format in assistant frame"));
                        }
                    };
                    if call.call_id != binding.call_id
                        || arguments(call, kind)? != binding.arguments
                        || calls
                            .insert(call.call_id.clone(), call.history_record())
                            .is_some()
                    {
                        return Err(invalid("assistant call identity mismatch"));
                    }
                    pending_calls.insert(call.call_id.clone());
                    call_order.push(call.call_id.clone());
                }
                let mut encoded = CompletionRequest::builder().input(input).build();
                encoded.replay_spans = replay_spans;
                let target = super::receipt::ModelCallBinding::capture(runtime, "replay");
                if !frame.receipt.binding.native_context_compatible(
                    &target,
                    frame.compatibility_family.as_deref(),
                    runtime
                        .model()
                        .capabilities
                        .native_context_family
                        .as_deref(),
                ) {
                    // 只投影当前请求；持久化回执、可见文本和工具身份仍保留原始事实。
                    if let Some(crate::completion::AssistantReplay::Responses { output }) =
                        &mut frame.receipt.response.replay
                    {
                        output.retain(|item| item["type"] != "reasoning");
                    }
                    frame
                        .receipt
                        .response
                        .responses_context_items
                        .retain(|item| item.value["type"] != "reasoning");
                }
                encoded
                    .append_response_from(
                        &frame.receipt.response,
                        Some(frame.receipt.binding.isolation.as_str()),
                    )
                    .map_err(|error| failure(ModelFailureKind::IncompatibleContext, error))?;
                input = encoded.input;
                replay_spans = encoded.replay_spans;
                continue;
            }
            if !frames.is_empty() || !record.tool_calls.is_empty() {
                return Err(invalid(
                    "assistant calls require their original model frame",
                ));
            }
        }
        let mut parts = Vec::new();
        if let ContextSource::RuntimeFact { source_id } = &record.source {
            parts.push(pl_protocol::ContentPart::Text {
                text: format!("[当前宿主事实：{source_id}]\n最新快照优先于历史摘要中的旧状态。\n"),
            });
        }
        for content in &record.content {
            match content {
                ContextContent::Text { text } => parts.push(pl_protocol::ContentPart::Text {
                    text: text.to_string(),
                }),
                ContextContent::Opaque { payload } => {
                    let attachment = super::media::decode(payload)?;
                    let reference = attachment.reference;
                    match &record.source {
                        ContextSource::ToolResult { call_id, .. } => {
                            tool_media.push(pl_protocol::ToolMediaContext {
                                call_id: call_id.clone(),
                                label: reference.id().to_owned(),
                                attachment: pl_protocol::ThreadAttachment {
                                    id: reference.id().to_owned(),
                                    modality: attachment.modality,
                                    media_type: reference.media_type().to_owned(),
                                    filename: None,
                                    width: None,
                                    height: None,
                                    byte_size: reference.byte_len(),
                                },
                            })
                        }
                        ContextSource::User
                        | ContextSource::Runtime { .. }
                        | ContextSource::RuntimeFact { .. }
                        | ContextSource::AgentMessage { .. } => {
                            parts.push(pl_protocol::ContentPart::Attachment {
                                attachment_id: reference.id().to_owned(),
                                modality: attachment.modality,
                                media_type: reference.media_type().to_owned(),
                                filename: None,
                            })
                        }
                        ContextSource::Instruction
                        | ContextSource::InstructionSnapshot { .. }
                        | ContextSource::Assistant => {
                            return Err(invalid(
                                "attachment projection has an unsupported source role",
                            ));
                        }
                    }
                }
                ContextContent::Resource { reference } => {
                    reference
                        .validate()
                        .map_err(|error| failure(ModelFailureKind::UnsupportedContent, error))?;
                    let descriptor = serde_json::to_string(reference)
                        .map_err(|error| failure(ModelFailureKind::UnsupportedContent, error))?;
                    parts.push(pl_protocol::ContentPart::Text {
                        text: format!("Resource reference: {descriptor}"),
                    });
                }
            }
        }
        let role = match &record.source {
            ContextSource::User
            | ContextSource::Runtime { .. }
            | ContextSource::RuntimeFact { .. }
            | ContextSource::AgentMessage { .. } => MessageRole::User,
            ContextSource::Assistant => MessageRole::Assistant,
            ContextSource::ToolResult { .. } => MessageRole::Tool,
            ContextSource::Instruction | ContextSource::InstructionSnapshot { .. } => {
                return Err(invalid("instruction order changed"));
            }
        };
        let mut message = message(role, MessageContent::new(parts));
        if let ContextSource::ToolResult { call_id, .. } = &record.source {
            let call = calls
                .get(call_id)
                .ok_or_else(|| invalid("missing original tool call frame"))?;
            message.tool_result = Some(pl_protocol::ToolResultRecord {
                item_id: call.item_id.clone(),
                call_id: call.call_id.clone(),
                name: call.name.clone(),
                kind: call.kind,
            });
        }
        if let ContextSource::ToolResult { call_id, .. } = &record.source {
            if !pending_calls.remove(call_id)
                || result_messages.insert(call_id.clone(), message).is_some()
            {
                return Err(invalid("tool result has no pending call"));
            }
            if pending_calls.is_empty() {
                for call_id in &call_order {
                    let message = result_messages
                        .remove(call_id)
                        .ok_or_else(|| invalid("tool result batch is incomplete"))?;
                    input.push(ModelContextItem::from(message));
                }
                let positions = call_order
                    .iter()
                    .enumerate()
                    .map(|(index, id)| (id.as_str(), index))
                    .collect::<BTreeMap<_, _>>();
                tool_media.sort_by_key(|media| positions.get(media.call_id.as_str()).copied());
                if !tool_media.is_empty() {
                    input.push(ModelContextItem::ToolMedia {
                        items: std::mem::take(&mut tool_media),
                    });
                }
                call_order.clear();
            }
        } else {
            input.push(ModelContextItem::from(message));
        }
    }
    if !pending_calls.is_empty() {
        return Err(invalid("incomplete tool result batch"));
    }
    let mut encoded = CompletionRequest::builder()
        .instructions(instructions.join("\n\n"))
        .input(input)
        .build();
    encoded.replay_spans = replay_spans;
    Ok(encoded)
}

pub(super) struct ResponseContext<'a> {
    pub request: &'a ModelRequest,
    pub names: &'a BTreeMap<String, ToolBinding>,
    pub marker: OpaquePayload,
    pub binding: super::receipt::ModelCallBinding,
    pub compatibility_family: Option<String>,
}

pub(super) fn response(
    context: ResponseContext<'_>,
    response: CompletionResponse,
) -> Result<ModelStepOutput, ModelError> {
    let failure_binding = context.binding.clone();
    let failure_accounting = response.accounting.clone();
    let failure_model_observation = response.model_observation.clone();
    let failure_progress = context
        .request
        .progress
        .as_ref()
        .map(|progress| progress.latest());
    response_inner(context, response).map_err(|error| {
        super::receipt::postprocess_failure_error(
            failure_binding,
            failure_accounting,
            failure_model_observation,
            failure_progress,
            error,
        )
    })
}

fn response_inner(
    context: ResponseContext<'_>,
    response: CompletionResponse,
) -> Result<ModelStepOutput, ModelError> {
    let ResponseContext {
        request,
        names,
        marker,
        binding,
        compatibility_family,
    } = context;
    let replay = response
        .replay
        .as_ref()
        .ok_or_else(|| invalid("model response has no replay material"))?;
    replay
        .semantic_input(&response)
        .map_err(|error| failure(ModelFailureKind::InvalidResponse, error))?;
    let observed_usage = usage(&response.accounting.usage);
    let bindings = response
        .tool_calls
        .iter()
        .map(|call| {
            let mapped = names.get(&call.name).and_then(|binding| {
                arguments(call, binding.arguments)
                    .ok()
                    .map(|arguments| (binding.id.clone(), arguments))
            });
            let (tool_id, arguments) = match mapped {
                Some(mapped) => mapped,
                None => (
                    String::new(),
                    OpaquePayload::new("pl.model.invalid-call", 1, call.payload_text())
                        .map_err(|error| failure(ModelFailureKind::InvalidResponse, error))?,
                ),
            };
            Ok(ModelToolCall {
                call_id: call.call_id.clone(),
                tool_id,
                arguments,
            })
        })
        .collect::<Result<Vec<_>, ModelError>>()?;
    let mut content = response
        .content
        .as_ref()
        .map(|text| ContextContent::Text {
            text: std::sync::Arc::from(text.as_str()),
        })
        .into_iter()
        .collect::<Vec<_>>();
    let frame = AssistantFrame {
        receipt: super::receipt::ModelResponseReceipt { binding, response },
        bindings: bindings.clone(),
        compatibility_family,
    };
    let encoded = serde_json::to_string(&frame).map_err(|error| ModelError {
        details: None,
        kind: ModelFailureKind::InvalidResponse,
        usage: Box::new(observed_usage.clone()),
        source: Some(Box::new(error)),
    })?;
    let payload = OpaquePayload::new(FRAME_FORMAT, FRAME_VERSION, encoded)
        .map_err(|error| failure(ModelFailureKind::InvalidResponse, error))?;
    content.push(ContextContent::Opaque { payload });
    Ok(ModelStepOutput {
        attempt_id: request.attempt_id.clone(),
        base_context_revision: request.context.revision,
        content,
        tool_calls: bindings,
        private_context: Some(marker),
        usage: observed_usage,
    })
}

fn message(role: MessageRole, content: MessageContent) -> Message {
    Message {
        role,
        content,
        presentation: Default::default(),
        reasoning_content: None,
        tool_calls: None,
        tool_result: None,
        metadata: Default::default(),
    }
}

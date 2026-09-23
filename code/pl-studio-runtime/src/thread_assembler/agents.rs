//! Product-owned relationship checks and descendant shutdown over generic Thread handles.
use super::observation::DeliveryDisposition;
use super::{Registry, State, StudioThreadAssembler, ThreadAssemblyError};
use pl_core::{
    context::{ContextContent, OpaquePayload},
    thread::{ThreadLifecycle, input::InputState, task::TaskStatus},
    tool::{
        ToolOutput,
        opaque::{Registration, ToolError},
    },
};
use pl_tool::collaboration::thread::{
    AgentControlHost, AgentControlKind, AgentMessage, AgentSpawn, ThreadAgentControl,
};
use std::sync::{Arc, Weak};

#[derive(Debug)]
struct AgentHost(Weak<Registry>);

#[derive(Debug, thiserror::Error)]
enum ControlError {
    #[error("agent coordinator is closed")]
    Closed,
    #[error("agent is missing or not published: {0}")]
    Missing(String),
    #[error("target {target} is not a descendant of caller {caller}")]
    Relationship { caller: String, target: String },
    #[error("invalid agent ancestry")]
    Ancestry,
}

impl AgentHost {
    fn owner(&self) -> Result<StudioThreadAssembler, ToolError> {
        self.0
            .upgrade()
            .map(StudioThreadAssembler)
            .ok_or_else(|| ToolError::new(ControlError::Closed))
    }
}

fn ancestry(state: &State, id: &str) -> Result<Vec<String>, ToolError> {
    let mut path = Vec::new();
    let mut cursor = Some(id);
    while let Some(id) = cursor {
        if path.iter().any(|previous| previous == id) {
            return Err(ToolError::new(ControlError::Ancestry));
        }
        let entry = state
            .entries
            .get(id)
            .ok_or_else(|| ToolError::new(ControlError::Missing(id.into())))?;
        path.push(id.to_owned());
        cursor = entry.parent_id.as_deref();
    }
    Ok(path)
}

fn descendant(state: &State, caller: &str, target: &str) -> Result<(), ToolError> {
    if !state.entries.get(caller).is_some_and(|entry| {
        entry.ready && entry.thread.snapshot().lifecycle == ThreadLifecycle::Open
    }) {
        return Err(ToolError::new(ControlError::Missing(caller.into())));
    }
    ancestry(state, caller)?;
    if caller == target
        || !ancestry(state, target)?
            .iter()
            .skip(1)
            .any(|id| id == caller)
    {
        return Err(ToolError::new(ControlError::Relationship {
            caller: caller.into(),
            target: target.into(),
        }));
    }
    Ok(())
}

fn output(value: impl serde::Serialize) -> Result<ToolOutput, ToolError> {
    let encoded = serde_json::to_string(&value).map_err(ToolError::new)?;
    Ok(ToolOutput::new(
        OpaquePayload::new("pl.studio.agent-control", 1, encoded.clone())
            .map_err(ToolError::new)?,
        vec![ContextContent::Text {
            text: Arc::from(encoded),
        }],
    ))
}

pub(super) fn agent_row(
    id: &str,
    parent: Option<&str>,
    snapshot: &pl_core::thread::ThreadSnapshot,
) -> serde_json::Value {
    serde_json::json!({ "id":id, "parentId":parent, "lifecycle":snapshot.lifecycle,
        "lastTurn":snapshot.turns.last(), "pendingInputs":snapshot.inputs.iter().filter(|input| input.state == InputState::Pending).count(),
        "runningTasks":snapshot.tasks.values().filter(|task| task.status == TaskStatus::Running).count() })
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SpawnReceipt {
    agent_id: String,
    profile_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace: Option<pl_protocol::AgentWorkspaceAssignmentSnapshot>,
    message_accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_sequence: Option<u64>,
}

fn assigned_workspace(
    snapshot: &pl_core::thread::ThreadSnapshot,
) -> Result<Option<pl_protocol::AgentWorkspaceAssignmentSnapshot>, ThreadAssemblyError> {
    let Some(record) = snapshot.extensions.get("studio.workspace") else {
        return Ok(None);
    };
    let payload = &record.payload;
    let decoded = if payload.format() == "pl.studio.workspace" && payload.version() == 1 {
        serde_json::from_str(payload.content()).map_err(anyhow::Error::new)
    } else {
        Err(anyhow::anyhow!(
            "unsupported child workspace codec {} version {}",
            payload.format(),
            payload.version()
        ))
    };
    decoded
        .map(Some)
        .map_err(|source| ThreadAssemblyError::Resource {
            operation: "decode assigned child workspace",
            source: source.into_boxed_dyn_error(),
        })
}

#[derive(Debug, thiserror::Error)]
#[error("child {id} preparation failed ({operation}); resource cleanup failed ({cleanup})")]
struct SpawnCleanupError {
    id: String,
    #[source]
    operation: ThreadAssemblyError,
    cleanup: ThreadAssemblyError,
}

async fn cleanup_spawn_failure(
    owner: &StudioThreadAssembler,
    id: &str,
    operation: ThreadAssemblyError,
) -> ToolError {
    match owner.discard_child_resources(id).await {
        Ok(()) => ToolError::new(operation),
        Err(cleanup) => ToolError::new(SpawnCleanupError {
            id: id.into(),
            operation,
            cleanup,
        }),
    }
}

impl AgentControlHost for AgentHost {
    async fn spawn(
        &self,
        context: pl_core::tool::opaque::CallContext,
        request: AgentSpawn,
    ) -> Result<ToolOutput, ToolError> {
        let owner = self.owner()?;
        let (factory, reservation) = owner.reserve_child(&context).map_err(ToolError::new)?;
        let spec = factory
            .prepare(super::ChildThreadRequest {
                id: reservation.id.clone(),
                cancellation: reservation.cancellation.clone(),
                caller: context.thread_id.clone(),
                call_id: context.call_id.clone(),
                profile_id: request.profile_id.clone(),
                task_summary: request.task_summary,
                writable_paths: request.writable_paths,
                metadata: OpaquePayload::new(
                    "pl.studio.agent-metadata",
                    1,
                    serde_json::to_string(&request.metadata).map_err(ToolError::new)?,
                )
                .map_err(ToolError::new)?,
            })
            .await;
        let spec = match spec {
            Ok(spec) => spec,
            Err(error) => {
                return Err(cleanup_spawn_failure(&owner, &reservation.id, error).await);
            }
        };
        if reservation.cancellation.is_cancelled() {
            return Err(cleanup_spawn_failure(
                &owner,
                &reservation.id,
                pl_core::thread::ThreadError::Cancelled.into(),
            )
            .await);
        }
        if spec.id != reservation.id {
            return Err(cleanup_spawn_failure(
                &owner,
                &reservation.id,
                ThreadAssemblyError::Identity(spec.id),
            )
            .await);
        }
        let id = spec.id.clone();
        let options = spec.execution;
        let history = request.fork_turns.inheritance();
        let child = owner
            .assemble_child(
                spec,
                &context,
                pl_core::context::ContextInheritance {
                    history,
                    instructions: pl_core::context::InstructionInheritance::Exclude,
                },
            )
            .await;
        let child = match child {
            Ok(child) => child,
            Err(error) => {
                return Err(cleanup_spawn_failure(&owner, &reservation.id, error).await);
            }
        };
        if reservation.cancellation.is_cancelled() {
            return Err(cleanup_spawn_failure(
                &owner,
                &id,
                pl_core::thread::ThreadError::Cancelled.into(),
            )
            .await);
        }
        let workspace = match assigned_workspace(&child.snapshot()) {
            Ok(workspace) => workspace,
            Err(error) => return Err(cleanup_spawn_failure(&owner, &id, error).await),
        };
        if let Err(error) = owner.publish_child_resources(&id) {
            return Err(cleanup_spawn_failure(&owner, &id, error).await);
        }
        let message = pl_core::thread::inbox::ThreadMessage {
            id: format!("initial:{}:{}", context.thread_id.len(), context.call_id),
            source_id: format!("agent:{}", context.thread_id),
            payload: OpaquePayload::text(request.message.clone()),
            context: vec![ContextContent::Text {
                text: Arc::from(request.message),
            }],
        };
        // The child's initial delivery is adjudicated against the child's durable identity index
        // first: a repeated spawn command must answer with the original receipt instead of queueing
        // the same initial message as new work, and an unverifiable or conflicting identity is
        // rejected rather than delivered again.
        let delivery = match owner.adjudicate_message(&id, &message).await {
            Ok(DeliveryDisposition::Receipt(sequence)) => Ok(sequence),
            Ok(DeliveryDisposition::New) => child
                .send_message_and_continue(message, options)
                .await
                .map_err(ThreadAssemblyError::from),
            Ok(DeliveryDisposition::Conflict) => {
                Err(ThreadAssemblyError::MessageConflict(message.id.clone()))
            }
            Ok(DeliveryDisposition::Unverifiable) => {
                Err(ThreadAssemblyError::MessageUnverifiable(message.id.clone()))
            }
            Err(error) => Err(error),
        };
        match delivery {
            Ok(sequence) => output(SpawnReceipt {
                agent_id: id,
                profile_id: request.profile_id,
                workspace,
                message_accepted: true,
                message_sequence: Some(sequence),
            }),
            Err(source) => Err(ToolError::new(source).with_output(output(SpawnReceipt {
                agent_id: id,
                profile_id: request.profile_id,
                workspace,
                message_accepted: false,
                message_sequence: None,
            })?)),
        }
    }

    async fn send(&self, caller: &str, message: AgentMessage) -> Result<ToolOutput, ToolError> {
        let owner = self.owner()?;
        if owner.thread(&message.target).is_none() {
            let services = owner.0.state().agent_services.clone();
            if let Some(services) = services {
                services
                    .activate_child(&owner, caller, &message.target)
                    .await
                    .map_err(|error| ToolError::new(super::agent_services::ServiceError(error)))?;
            }
        }
        let (target, options) = {
            let state = owner.0.state();
            if state.closing {
                return Err(ToolError::new(ControlError::Closed));
            }
            descendant(&state, caller, &message.target)?;
            let entry = state
                .entries
                .get(&message.target)
                .filter(|entry| entry.ready && entry.parent_id.as_deref() == Some(caller))
                .ok_or_else(|| {
                    ToolError::new(ControlError::Relationship {
                        caller: caller.into(),
                        target: message.target.clone(),
                    })
                })?;
            (entry.thread.clone(), entry.execution)
        };
        let delivery = pl_core::thread::inbox::ThreadMessage {
            id: message.id.clone(),
            source_id: format!("agent:{caller}"),
            payload: OpaquePayload::text(message.message.clone()),
            context: vec![ContextContent::Text {
                text: Arc::from(message.message),
            }],
        };
        // The target's durable identity index adjudicates the delivery before it can become new work:
        // an identical repeat answers with its original receipt instead of a second delivery, while a
        // conflicting or unverifiable identity is rejected and a failed lookup never admits a
        // possible duplicate.
        let sequence = match owner
            .adjudicate_message(&message.target, &delivery)
            .await
            .map_err(ToolError::new)?
        {
            DeliveryDisposition::Receipt(sequence) => sequence,
            DeliveryDisposition::New => target
                .send_message_and_continue(delivery, options)
                .await
                .map_err(ToolError::new)?,
            DeliveryDisposition::Conflict => {
                return Err(ToolError::new(ThreadAssemblyError::MessageConflict(
                    message.id.clone(),
                )));
            }
            DeliveryDisposition::Unverifiable => {
                return Err(ToolError::new(ThreadAssemblyError::MessageUnverifiable(
                    message.id.clone(),
                )));
            }
        };
        output(
            serde_json::json!({"target":message.target, "messageId":message.id, "sequence":sequence}),
        )
    }

    async fn list(&self, caller: &str) -> Result<ToolOutput, ToolError> {
        let owner = self.owner()?;
        let mut rows = {
            let state = owner.0.state();
            if state.closing {
                return Err(ToolError::new(ControlError::Closed));
            }
            if !state.entries.get(caller).is_some_and(|entry| entry.ready) {
                return Err(ToolError::new(ControlError::Missing(caller.into())));
            }
            let path = ancestry(&state, caller)?;
            let root = path
                .last()
                .ok_or_else(|| ToolError::new(ControlError::Ancestry))?;
            let mut rows = Vec::new();
            for (id, entry) in &state.entries {
                let snapshot = entry.thread.snapshot();
                if (!entry.ready && snapshot.lifecycle == ThreadLifecycle::Open)
                    || ancestry(&state, id)?.last() != Some(root)
                {
                    continue;
                }
                rows.push(agent_row(id, entry.parent_id.as_deref(), &snapshot));
            }
            rows
        };
        let services = owner.0.state().agent_services.clone();
        if let Some(services) = services {
            let loaded = rows
                .iter()
                .filter_map(|row| row["id"].as_str().map(str::to_owned))
                .collect();
            rows.extend(
                services
                    .saved_agents(caller, &loaded)
                    .await
                    .map_err(|error| ToolError::new(super::agent_services::ServiceError(error)))?,
            );
        }
        output(rows)
    }

    async fn interrupt(&self, caller: &str, target: &str) -> Result<ToolOutput, ToolError> {
        let owner = self.owner()?;
        let thread = {
            let state = owner.0.state();
            if state.closing {
                return Err(ToolError::new(ControlError::Closed));
            }
            descendant(&state, caller, target)?;
            state
                .entries
                .get(target)
                .ok_or_else(|| ToolError::new(ControlError::Missing(target.into())))?
                .thread
                .clone()
        };
        let interrupted = thread.interrupt_turn(None).await.map_err(ToolError::new)?;
        output(serde_json::json!({"target":target,"interrupted":interrupted}))
    }

    async fn close(
        &self,
        caller: &str,
        target: &str,
        disposition: pl_tool::collaboration::thread::AgentWorkspaceDisposition,
    ) -> Result<ToolOutput, ToolError> {
        let owner = self.owner()?;
        let mut targets = {
            let state = owner.0.state();
            if state.closing {
                return Err(ToolError::new(ControlError::Closed));
            }
            descendant(&state, caller, target)?;
            let mut targets = Vec::new();
            for id in state.entries.keys() {
                let path = ancestry(&state, id)?;
                if path.iter().any(|ancestor| ancestor == target) {
                    targets.push((path.len(), id.clone()));
                }
            }
            targets
        };
        targets.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        owner
            .seal_agent_close(&targets, disposition)
            .map_err(ToolError::new)?;
        for (_, id) in targets {
            owner.close(&id).await.map_err(ToolError::new)?;
        }
        output(
            serde_json::json!({"target":target,"lifecycle":ThreadLifecycle::Closed,"workspaceDisposition":disposition}),
        )
    }
}

impl StudioThreadAssembler {
    /// Creates control frontends with a weak coordinator reference; tools cannot keep their registry alive.
    ///
    /// # Errors
    /// Returns model declaration encoding or registration errors.
    pub fn agent_control_tools(&self) -> Result<Vec<Registration>, ThreadAssemblyError> {
        let host = Arc::new(AgentHost(Arc::downgrade(&self.0)));
        let mut kinds = vec![
            AgentControlKind::Send,
            AgentControlKind::List,
            AgentControlKind::Interrupt,
            AgentControlKind::Close,
        ];
        if self.0.state().child_factory.is_some() {
            kinds.push(AgentControlKind::Spawn);
        }
        let mut tools = kinds
            .into_iter()
            .map(|kind| {
                let declaration = pl_model::runtime::thread_tool_declaration(&kind.declaration())?;
                Ok(ThreadAgentControl::new(host.clone(), kind).registration(declaration)?)
            })
            .collect::<Result<Vec<_>, ThreadAssemblyError>>()?;
        tools.extend(self.agent_query_tools()?);
        Ok(tools)
    }
}

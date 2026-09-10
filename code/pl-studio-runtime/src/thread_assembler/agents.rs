//! Product-owned relationship checks and descendant shutdown over generic Thread handles.
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
        let delivery = child
            .send_message_and_resume(
                pl_core::thread::inbox::ThreadMessage {
                    id: format!("initial:{}:{}", context.thread_id.len(), context.call_id),
                    source_id: format!("agent:{}", context.thread_id),
                    payload: OpaquePayload::text(request.message.clone()),
                    context: vec![ContextContent::Text {
                        text: Arc::from(request.message),
                    }],
                },
                options,
            )
            .await;
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
        let sequence = target
            .send_message_and_resume(
                pl_core::thread::inbox::ThreadMessage {
                    id: message.id.clone(),
                    source_id: format!("agent:{caller}"),
                    payload: OpaquePayload::text(message.message.clone()),
                    context: vec![ContextContent::Text {
                        text: Arc::from(message.message),
                    }],
                },
                options,
            )
            .await
            .map_err(ToolError::new)?;
        output(
            serde_json::json!({"target":message.target, "messageId":message.id, "sequence":sequence}),
        )
    }

    async fn list(&self, caller: &str) -> Result<ToolOutput, ToolError> {
        let owner = self.owner()?;
        let rows = {
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
                rows.push(serde_json::json!({ "id":id, "parentId":entry.parent_id, "lifecycle":snapshot.lifecycle,
                    "lastTurn":snapshot.turns.last(), "pendingInputs":snapshot.inputs.iter().filter(|input| input.state == InputState::Pending).count(),
                    "runningTasks":snapshot.tasks.values().filter(|task| task.status == TaskStatus::Running).count() }));
            }
            rows
        };
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
        thread.pause_inputs().await.map_err(ToolError::new)?;
        let interrupted = thread.interrupt();
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

#[cfg(test)]
mod tests {
    use super::super::tests::spec;
    use super::*;
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn control_ports_restrict_targets_to_descendants_and_close_children_first() {
        let directory = tempfile::tempdir().unwrap();
        let owner = StudioThreadAssembler::default();
        for (id, parent) in [
            ("root", None),
            ("child", Some("root")),
            ("grandchild", Some("child")),
            ("other", None),
        ] {
            owner
                .assemble(spec(id, parent, directory.path()))
                .await
                .unwrap();
        }
        let host = AgentHost(Arc::downgrade(&owner.0));
        let listed = host.list("child").await.unwrap();
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(listed.payload().content()).unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| row["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["child", "grandchild", "root"]
        );
        assert!(host.interrupt("child", "root").await.is_err());
        assert!(
            host.send(
                "child",
                AgentMessage {
                    id: "forbidden".into(),
                    target: "root".into(),
                    message: "cannot steer parent".into()
                }
            )
            .await
            .is_err()
        );
        assert!(
            host.send(
                "root",
                AgentMessage {
                    id: "not-direct".into(),
                    target: "grandchild".into(),
                    message: "direct children only".into()
                }
            )
            .await
            .is_err()
        );
        assert!(
            host.close("root", "other", Default::default())
                .await
                .is_err()
        );
        assert!(
            host.close("root", "root", Default::default())
                .await
                .is_err()
        );
        host.close("root", "child", Default::default())
            .await
            .unwrap();
        assert!(owner.thread("child").is_none());
        assert!(owner.thread("grandchild").is_none());
        assert!(owner.thread("root").is_some());
        assert!(owner.thread("other").is_some());
        assert!(
            owner
                .assemble(spec("orphan", Some("child"), directory.path()))
                .await
                .is_err()
        );
        assert!(owner.close_all().await.is_empty());
    }
    #[test]
    fn spawn_receipt_retains_resolved_profile_for_parent_coordination_and_evidence() {
        for sequence in [None, Some(3)] {
            let output = output(SpawnReceipt {
                agent_id: "child".into(),
                profile_id: "reviewer".into(),
                workspace: None,
                message_accepted: sequence.is_some(),
                message_sequence: sequence,
            })
            .unwrap();
            let payload: serde_json::Value =
                serde_json::from_str(output.payload().content()).unwrap();
            assert_eq!(payload["agentId"], "child");
            assert_eq!(payload["profileId"], "reviewer");
            assert_eq!(payload["messageAccepted"], sequence.is_some());
            assert_eq!(
                payload
                    .get("messageSequence")
                    .and_then(serde_json::Value::as_u64),
                sequence
            );
        }
    }

    #[tokio::test]
    async fn receipt_uses_frozen_assignment_including_worktree_identity_on_delivery_failure() {
        let directory = tempfile::tempdir().unwrap();
        let owner = StudioThreadAssembler::default();
        let assignment = pl_protocol::AgentWorkspaceAssignmentSnapshot {
            mode: pl_protocol::AgentWorkspaceMode::Worktree,
            project_root: "/project/subdirectory".into(),
            root: "/allocated/worktrees/child".into(),
            writable_paths: None,
            worktree: Some(pl_protocol::AgentWorktreeSnapshot {
                repository_root: "/project".into(),
                path: "/allocated/worktrees/child".into(),
                branch: "agent/child".into(),
                base_commit: "frozen-commit".into(),
            }),
        };
        let mut specification = spec("allocated", None, directory.path());
        specification.initial_extensions.insert(
            "studio.workspace".into(),
            OpaquePayload::new(
                "pl.studio.workspace",
                1,
                serde_json::to_string(&assignment).unwrap(),
            )
            .unwrap(),
        );
        let child = owner.assemble(specification).await.unwrap();
        for sequence in [None, Some(4)] {
            let receipt = output(SpawnReceipt {
                agent_id: "allocated".into(),
                profile_id: "worker".into(),
                workspace: assigned_workspace(&child.snapshot()).unwrap(),
                message_accepted: sequence.is_some(),
                message_sequence: sequence,
            })
            .unwrap();
            let payload: serde_json::Value =
                serde_json::from_str(receipt.payload().content()).unwrap();
            assert_eq!(
                serde_json::from_value::<pl_protocol::AgentWorkspaceAssignmentSnapshot>(
                    payload["workspace"].clone()
                )
                .unwrap(),
                assignment
            );
        }
        child
            .mutate_extensions(vec![pl_core::thread::extensions::ExtensionMutation::Put {
                id: "studio.workspace".into(),
                expected_revision: Some(child.snapshot().extensions["studio.workspace"].revision),
                payload: OpaquePayload::new("future.workspace", 99, "unknown").unwrap(),
            }])
            .await
            .unwrap();
        assert!(assigned_workspace(&child.snapshot()).is_err());
        assert!(owner.close_all().await.is_empty());
    }

    #[derive(Debug)]
    struct DeniedFactory;
    impl super::super::StudioChildFactory for DeniedFactory {
        async fn discard_unpublished(&self, _id: &str) -> Result<(), ThreadAssemblyError> {
            Ok(())
        }
        async fn prepare(
            &self,
            request: super::super::ChildThreadRequest,
        ) -> Result<super::super::StudioThreadSpec, ThreadAssemblyError> {
            Err(ThreadAssemblyError::Identity(format!(
                "profile denied for {}: {}",
                request.caller, request.profile_id
            )))
        }
    }

    #[tokio::test]
    async fn child_factory_rejection_does_not_publish_a_child() {
        let directory = tempfile::tempdir().unwrap();
        let owner = StudioThreadAssembler::default();
        owner.set_child_factory(DeniedFactory).unwrap();
        owner
            .assemble(spec("root", None, directory.path()))
            .await
            .unwrap();
        let host = AgentHost(Arc::downgrade(&owner.0));
        let context = pl_core::tool::opaque::CallContext {
            grant: Default::default(),
            context: Default::default(),
            model_projection: None,
            tasks: None,
            thread_id: "root".into(),
            turn_id: "turn".into(),
            call_id: "call".into(),
            cancellation: tokio_util::sync::CancellationToken::new(),
            extensions: Arc::new(Default::default()),
            catalog: Vec::new().into(),
            extension_sequence: 0,
        };
        assert!(
            host.spawn(
                context,
                AgentSpawn {
                    profile_id: "forbidden".into(),
                    message: "task".into(),
                    fork_turns: pl_tool::collaboration::thread::AgentHistory::None,
                    writable_paths: None,
                    metadata: serde_json::Value::Null,
                }
            )
            .await
            .is_err()
        );
        assert_eq!(owner.0.state().entries.len(), 1);
        assert!(owner.close_all().await.is_empty());
    }
}

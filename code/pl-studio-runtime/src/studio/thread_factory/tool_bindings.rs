//! Frozen physical targets and weak command leases for one active Thread incarnation.
use super::{
    StudioThreadFactory,
    errors::resource_error,
    thread_tools::{CatalogPreparation, ThreadToolAssembly},
};
use crate::{
    studio::ProjectRecord,
    thread_assembler::{StudioThreadTools, ThreadAssemblyError, WeakCommandProcesses},
};
use pl_core::thread::ThreadSnapshot;
use pl_tool::workspace::ToolWorkspace;

#[derive(Clone)]
pub(super) struct ToolBinding {
    pub incarnation: std::sync::Arc<()>,
    pub target_invalidated: bool,
    pub catalog: Option<std::sync::Arc<pl_tool::skill::FrozenSkillCatalog>>,
    pub initial_skill_prompt: String,
    pub policy_key: String,
    pub policies:
        std::collections::BTreeMap<String, pl_core::tool::execution_policy::ExecutionPolicyHandle>,
    pub project: ProjectRecord,
    pub workspace: ToolWorkspace,
    pub commands: Option<WeakCommandProcesses>,
    pub remote: Option<pl_tool::remote::RemoteWorkspaceHost>,
    pub ssh_profile: Option<pl_tool::remote::SshServerProfile>,
}

/// Candidate metadata is published only after the Thread accepts its registrations.
pub(in crate::studio) struct RefreshedToolBinding {
    previous: std::sync::Arc<()>,
    binding: ToolBinding,
}

impl StudioThreadFactory {
    #[cfg(test)]
    pub(in crate::studio) fn binding_incarnation_for_test(
        &self,
        id: &str,
    ) -> Option<std::sync::Arc<()>> {
        self.bindings
            .lock()
            .unwrap()
            .get(id)
            .map(|binding| binding.incarnation.clone())
    }

    #[cfg(test)]
    pub(in crate::studio) fn remote_binding_for_test(
        &self,
        id: &str,
    ) -> Option<pl_tool::remote::RemoteWorkspaceHost> {
        self.bindings
            .lock()
            .unwrap()
            .get(id)
            .and_then(|binding| binding.remote.clone())
    }

    /// 测试探针：activation 为某个 Thread 解析出的会话工作区根。
    #[cfg(test)]
    pub(in crate::studio) fn session_workspace_root_for_test(
        &self,
        id: &str,
    ) -> Option<std::path::PathBuf> {
        self.session_roots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
            .cloned()
    }

    #[cfg(test)]
    pub(in crate::studio) fn record_session_workspace_root_for_test(
        &self,
        id: &str,
        root: std::path::PathBuf,
    ) {
        self.session_roots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.to_string(), root);
    }

    pub(in crate::studio) fn ssh_alias(&self, id: &str) -> Option<String> {
        self.bindings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
            .and_then(|binding| binding.project.ssh_alias.clone())
    }

    pub(in crate::studio) fn commit_refreshed_binding(
        &self,
        id: &str,
        candidate: RefreshedToolBinding,
    ) -> Result<(), ThreadAssemblyError> {
        let mut bindings = self
            .bindings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = bindings
            .get_mut(id)
            .ok_or(pl_core::thread::ThreadError::Closed)?;
        if !std::sync::Arc::ptr_eq(&current.incarnation, &candidate.previous) {
            return Err(pl_core::thread::ThreadError::Closed.into());
        }
        if current.target_invalidated {
            return Err(ThreadAssemblyError::Identity(
                "SSH target or credentials changed during tool refresh; reactivate this Thread"
                    .into(),
            ));
        }
        *current = candidate.binding;
        Ok(())
    }
    pub(in crate::studio) fn skill_catalog_fact(
        &self,
        id: &str,
        snapshot: &ThreadSnapshot,
    ) -> Option<pl_core::thread::RuntimeFact> {
        let binding = self
            .bindings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)?
            .clone();
        let prompt = binding
            .catalog
            .as_ref()
            .map(|catalog| pl_tool::skill::build_skills_prompt_from_catalog(catalog.snapshot()))
            .unwrap_or_default();
        catalog_fact(prompt, &binding.initial_skill_prompt, snapshot)
    }

    pub(in crate::studio) fn skill_suggestions(
        &self,
        id: &str,
        query: &str,
        snapshot: &ThreadSnapshot,
    ) -> Option<String> {
        let catalog = self
            .bindings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)?
            .catalog
            .clone()?;
        let loaded = snapshot
            .extensions
            .values()
            .filter_map(|record| pl_tool::skill::saved_skill_name(&record.payload).ok())
            .collect::<Vec<_>>();
        pl_tool::skill::build_skill_suggestions_from_catalog(catalog.snapshot(), query, &loaded)
    }

    pub(in crate::studio) fn invalidate_ssh_bindings(&self, server_id: &str) {
        for binding in self
            .bindings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values_mut()
        {
            if binding.project.ssh_alias.as_deref() == Some(server_id) {
                binding.target_invalidated = true;
            }
        }
    }
    pub(in crate::studio) fn forget_tool_binding(&self, id: &str) {
        self.bindings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(id);
    }

    pub(in crate::studio) async fn refresh_thread_tools(
        &self,
        id: &str,
        snapshot: &ThreadSnapshot,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<
        (
            StudioThreadTools,
            crate::thread_assembler::AgentControlExposure,
            RefreshedToolBinding,
        ),
        ThreadAssemblyError,
    > {
        let binding = self
            .bindings
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
            .cloned()
            .ok_or_else(|| {
                ThreadAssemblyError::Identity(
                    "Thread tool binding is unavailable; reactivate this Thread".into(),
                )
            })?;
        if binding.target_invalidated {
            return Err(ThreadAssemblyError::Identity(
                "SSH target or credentials changed; reactivate this Thread".into(),
            ));
        }
        let thread = self
            .services
            .product_events
            .thread_snapshot(id)
            .map(crate::studio::ThreadRecord::from_directory_thread)
            .ok_or_else(|| ThreadAssemblyError::Identity(id.into()))?;
        let project = self
            .services
            .product_events
            .project_snapshot()
            .await
            .into_iter()
            .find(|project| project.id == thread.project_id)
            .ok_or_else(|| {
                ThreadAssemblyError::Identity(
                    "project target disappeared; reactivate this Thread".into(),
                )
            })?;
        ensure_same_target(&binding.project, &project)?;
        if let Some(previous) = &binding.ssh_profile {
            let current = self
                .services
                .ssh_manager
                .list_servers()
                .await
                .into_iter()
                .find(|entry| entry.profile.alias == previous.alias)
                .map(|entry| entry.profile);
            if current.as_ref() != Some(previous) {
                return Err(ThreadAssemblyError::Identity(
                    "SSH target configuration changed; reactivate this Thread".into(),
                ));
            }
        }
        let config_runtime = self.services.config_runtime.clone();
        let role = thread.role.clone();
        let child = thread.parent_thread_id.is_some();
        let assignment = snapshot
            .extensions
            .get("studio.workspace")
            .map(|saved| {
                serde_json::from_str::<pl_protocol::AgentWorkspaceAssignmentSnapshot>(
                    saved.payload.content(),
                )
            })
            .transpose()
            .map_err(|error| resource_error("decode workspace binding", error))?;
        let (config, route) =
            tokio::task::spawn_blocking(move || -> Result<_, ThreadAssemblyError> {
                if child {
                    let profile = config_runtime.resolve_agent_profile(&role)?;
                    if assignment
                        .as_ref()
                        .is_some_and(|assignment| assignment.mode != profile.profile.workspace_mode)
                    {
                        return Err(ThreadAssemblyError::Identity(
                            "child Profile workspace policy changed; reactivate this Thread".into(),
                        ));
                    }
                    Ok((profile.config, profile.route))
                } else {
                    let config = config_runtime.read()?.config;
                    let route = config
                        .models
                        .resolve(&crate::config::StudioRole::Planner.id())?;
                    Ok((config, route))
                }
            })
            .await
            .map_err(|error| resource_error("resolve refreshed tool configuration", error))??;
        let resources = crate::resource_store::FileResourceStore::new(
            self.services
                .store
                .attachments_dir()
                .join("thread-resources"),
        );
        let remote = match &project.ssh_alias {
            Some(server) => {
                // 身份比较在同一 POSIX 表示上进行：`canonical_path` 由远端 helper 返回
                // 恒为 POSIX，会话工作区根也必须归一化后比较，否则 Windows 宿主形态会被
                // 误判为「目标已改变」。
                let requested = crate::agent::worktree::remote_path_text(binding.workspace.root());
                let host = self
                    .services
                    .ssh_manager
                    .open_workspace_host(
                        server,
                        binding.workspace.root().to_string_lossy().into_owned(),
                    )
                    .await
                    .map_err(|error| resource_error("reopen Thread remote workspace", error))?;
                if host.files.canonical_path() != requested.as_str() {
                    return Err(ThreadAssemblyError::Identity(
                        "remote workspace target changed; reactivate this Thread".into(),
                    ));
                }
                Some(host)
            }
            None => None,
        };
        let same_connection = match (&binding.remote, &remote) {
            (Some(previous), Some(current)) => previous.files.is_same_binding(&current.files),
            (None, None) => true,
            _ => false,
        };
        let previous_commands = if same_connection {
            binding.commands.clone()
        } else {
            None
        };
        let mut prepared = self
            .prepare_thread_tools_with(
                ThreadToolAssembly {
                    thread_id: id,
                    cancellation: &cancellation,
                    config: &config,
                    route: &route,
                    project: &project,
                    root_thread_id: &thread.root_thread_id,
                    workspace: binding.workspace.clone(),
                    store: resources,
                },
                CatalogPreparation::Refresh {
                    policy_key: binding.policy_key.clone(),
                    policies: binding.policies.clone(),
                    remote: remote.map(Box::new),
                    commands: previous_commands
                        .as_ref()
                        .and_then(WeakCommandProcesses::upgrade),
                },
            )
            .await?;
        let exposure =
            if child || prepared.visibility == crate::search::ToolVisibilityConstraint::Exclusive {
                crate::thread_assembler::AgentControlExposure::Disabled
            } else {
                let mode_id = crate::studio::thread_projection::saved_mode(snapshot)
                    .map_err(|error| resource_error("read current mode for tool refresh", error))?
                    .unwrap_or(thread.mode);
                let mode = self
                    .services
                    .thread_modes
                    .snapshot()
                    .mode(&mode_id)
                    .ok_or_else(|| {
                        ThreadAssemblyError::Identity(
                            "current Mode is unavailable; reactivate this Thread".into(),
                        )
                    })?;
                prepared.tools = prepared
                    .tools
                    .with_tools(crate::workflow_tool::workflow_registrations(mode)?);
                crate::thread_assembler::AgentControlExposure::Enabled
            };
        if cancellation.is_cancelled() {
            return Err(pl_core::thread::ThreadError::Cancelled.into());
        }
        let commands = prepared
            .tools
            .command_processes
            .as_ref()
            .map(|commands| commands.downgrade())
            .or(previous_commands);
        let candidate = RefreshedToolBinding {
            previous: binding.incarnation.clone(),
            binding: ToolBinding {
                incarnation: std::sync::Arc::new(()),
                commands,
                remote: prepared.remote.clone(),
                catalog: prepared.catalog.clone(),
                policy_key: super::thread_tools::approval_policy_key(&config, &route),
                policies: prepared.tools.approval_policies.clone(),
                ..binding
            },
        };
        Ok((prepared.tools, exposure, candidate))
    }
}

fn catalog_fact(
    prompt: String,
    initial: &str,
    snapshot: &ThreadSnapshot,
) -> Option<pl_core::thread::RuntimeFact> {
    let previous = snapshot
        .runtime_facts
        .iter()
        .find(|fact| fact.source_id == "studio.skills");
    if previous.is_none() && prompt == initial {
        return None;
    }
    let fact = pl_core::thread::RuntimeFact {
        source_id: "studio.skills".into(),
        content: if prompt.is_empty() {
            Vec::new()
        } else {
            vec![pl_core::context::ContextContent::Text {
                text: prompt.into(),
            }]
        },
    };
    (previous != Some(&fact)).then_some(fact)
}

fn ensure_same_target(
    previous: &ProjectRecord,
    current: &ProjectRecord,
) -> Result<(), ThreadAssemblyError> {
    if previous.id != current.id
        || previous.path != current.path
        || previous.ssh_alias != current.ssh_alias
    {
        return Err(ThreadAssemblyError::Identity("physical workspace target changed; reactivate this Thread before accessing the new target".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn changed_skill_catalog_appends_once_and_never_rewrites_the_initial_prefix() {
        use pl_core::{context::*, model::*, thread::*};
        use pretty_assertions::assert_eq;
        struct NoCalls;
        impl ModelSession for NoCalls {
            async fn prepare(&mut self, _: ModelRequest) -> Result<PreparedModelCall, ModelError> {
                panic!("catalog updates must not invoke a model")
            }
            async fn close(&mut self) -> Result<(), ModelError> {
                Ok(())
            }
        }
        let thread = ThreadHandle::start("catalog".into(), DynModelSession::new(NoCalls)).unwrap();
        let initial = "# Skills\noriginal";
        assert!(catalog_fact(initial.into(), initial, &thread.snapshot()).is_none());
        thread
            .patch_runtime_facts(vec![RuntimeFact {
                source_id: "studio.workflow".into(),
                content: vec![ContextContent::Text {
                    text: "workflow".into(),
                }],
            }])
            .await
            .unwrap();
        let before = thread.snapshot();
        let changed = catalog_fact("# Skills\nnew tool".into(), initial, &before).unwrap();
        thread.patch_runtime_facts(vec![changed]).await.unwrap();
        let after = thread.snapshot();
        assert_eq!(
            &after.context.records[..before.context.records.len()],
            before.context.records.as_ref()
        );
        assert_eq!(
            after.context.records.len(),
            before.context.records.len() + 1
        );
        assert!(catalog_fact("# Skills\nnew tool".into(), initial, &after).is_none());
        let cleared = catalog_fact(String::new(), initial, &after).unwrap();
        thread.patch_runtime_facts(vec![cleared]).await.unwrap();
        let disabled = thread.snapshot();
        assert_eq!(
            disabled.context.records.len(),
            after.context.records.len() + 1
        );
        assert!(catalog_fact(String::new(), initial, &disabled).is_none());
        assert!(
            disabled
                .runtime_facts
                .iter()
                .any(|fact| fact.source_id == "studio.workflow" && !fact.content.is_empty())
        );
        thread.close().await.unwrap();
    }

    #[test]
    fn target_identity_requires_reactivation_only_for_physical_changes() {
        let original = ProjectRecord {
            id: "p".into(),
            name: "workspace".into(),
            path: "/frozen".into(),
            ssh_alias: None,
            updated_at: 1,
        };
        let mut renamed = original.clone();
        renamed.name = "renamed".into();
        renamed.updated_at = 2;
        assert!(ensure_same_target(&original, &renamed).is_ok());
        let mut moved = original.clone();
        moved.path = "/other".into();
        assert!(ensure_same_target(&original, &moved).is_err());
        let mut remote = original.clone();
        remote.ssh_alias = Some("server".into());
        assert!(ensure_same_target(&original, &remote).is_err());
    }
}

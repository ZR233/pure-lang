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
        let config = self.services.config_runtime.read()?.config;
        let (_, selector) = crate::studio::model_route::route_record(snapshot, child)
            .map_err(|error| resource_error("decode saved model route", error))?
            .ok_or_else(|| {
                ThreadAssemblyError::Identity(
                    "Thread has no saved model route; reactivate this Thread".into(),
                )
            })?;
        if child {
            let (profile, _) = crate::studio::model_route::saved_profile(snapshot)
                .map_err(|error| resource_error("decode saved child Agent Profile", error))?
                .ok_or_else(|| {
                    ThreadAssemblyError::Identity(
                        "child has no frozen Agent Profile; reactivate this Thread".into(),
                    )
                })?;
            if assignment
                .as_ref()
                .is_some_and(|assignment| assignment.mode != profile.workspace_mode)
            {
                return Err(ThreadAssemblyError::Identity(
                    "saved child Profile conflicts with its workspace assignment".into(),
                ));
            }
        }
        let role = if child {
            pl_protocol::AgentRoleId::new(thread.role.clone())?
        } else {
            crate::config::StudioRole::Planner.id()
        };
        let route = config.models.resolve_route(role, &selector)?;
        let resources = crate::resource_store::FileResourceStore::new(
            self.services.store.session_resources_dir(&thread.id),
        );
        let remote = match &project.ssh_alias {
            Some(server) => {
                // 身份比较在同一 POSIX 表示上进行：`canonical_path` 由远端 helper 返回
                // 恒为 POSIX，会话工作区根也必须归一化后比较，否则 Windows 宿主形态会被
                // 误判为「目标已改变」。
                let requested = pl_tool::remote::normalize_remote_absolute_path(
                    &binding.workspace.root().to_string_lossy(),
                )
                .map_err(|error| resource_error("normalize Thread remote workspace", error))?;
                let host = self
                    .services
                    .ssh_manager
                    .open_workspace_host(server, requested.clone())
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

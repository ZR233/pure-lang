use crate::api::studio::bridge_runtime::BridgeRuntime;
use crate::api::studio::convert::records::{project_dto, thread_from_record};
use crate::api::studio::convert::runtime::{bridge_recovery_issue, bridge_task_runtime};
use crate::api::studio::convert::settings::studio_settings_dto;
use crate::api::studio::convert::thread_stream::bridge_thread;
use crate::api::studio::types::{BridgeGeneralSettingsDto, BridgeStudioSnapshotResponse};
use anyhow::{Context, Result};
use std::collections::HashSet;
// ── Inner async helpers ──

pub(super) async fn bootstrap_studio_inner(
    bridge: &'static BridgeRuntime,
) -> Result<BridgeStudioSnapshotResponse> {
    let projects = bridge.studio.list_projects().await?;
    let selected_project_id = projects.first().map(|project| project.id.clone());
    studio_snapshot_from_projects_inner(bridge, projects, selected_project_id, None).await
}

pub(super) async fn studio_snapshot_inner(
    bridge: &'static BridgeRuntime,
    requested_project_id: Option<String>,
    requested_thread_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let projects = bridge.studio.list_projects().await?;
    studio_snapshot_from_projects_inner(bridge, projects, requested_project_id, requested_thread_id)
        .await
}

pub(super) async fn studio_snapshot_from_projects_inner(
    bridge: &'static BridgeRuntime,
    projects: Vec<pl_studio_runtime::ProjectRecord>,
    requested_project_id: Option<String>,
    requested_thread_id: Option<String>,
) -> Result<BridgeStudioSnapshotResponse> {
    let recovery_issues = bridge.studio.recovery_issues();
    let blocked_project_ids = recovery_issues
        .iter()
        .filter(|issue| issue.scope == pl_studio_runtime::StudioRecoveryIssueScope::Project)
        .filter_map(|issue| issue.project_id.as_deref())
        .map(ToOwned::to_owned)
        .collect::<HashSet<_>>();
    let blocked_thread_ids = recovery_issues
        .iter()
        .filter(|issue| issue.scope == pl_studio_runtime::StudioRecoveryIssueScope::Thread)
        .filter_map(|issue| issue.thread_id.as_deref())
        .collect::<std::collections::HashSet<_>>();
    let selected_project = select_available_project(
        &projects,
        requested_project_id.as_deref(),
        &blocked_project_ids,
    );
    let selected_project_id = selected_project.as_ref().map(|project| project.id.clone());
    let mut threads = Vec::new();
    let mut selected_thread_id = None;

    if let Some(project) = selected_project {
        bridge
            .studio
            .reconcile_lsp_runtime_for_project(&project.id)
            .await?;
        let roots = bridge.studio.ensure_project_threads(&project.id).await?;
        threads = bridge.studio.store().list_threads(&project.id).await?;
        selected_thread_id = requested_thread_id
            .filter(|thread_id| {
                !blocked_thread_ids.contains(thread_id.as_str())
                    && threads.iter().any(|thread| thread.id == *thread_id)
            })
            .or_else(|| {
                roots
                    .iter()
                    .find(|thread| !blocked_thread_ids.contains(thread.id.as_str()))
                    .map(|thread| thread.id.clone())
            });
    }
    let selected_thread_task = match selected_thread_id.as_deref() {
        Some(thread_id) => bridge
            .studio
            .thread_task_view(thread_id)
            .await?
            .map(bridge_task_runtime),
        None => None,
    };
    let config = bridge.studio.config_store().load_or_default()?;
    let web_search_role = selected_thread_id
        .as_deref()
        .and_then(|thread_id| threads.iter().find(|thread| thread.id == thread_id))
        .map(|thread| pl_studio_runtime::StudioMode::from_label(&thread.mode))
        .map_or(pl_studio_runtime::StudioRole::Executor, |mode| match mode {
            pl_studio_runtime::StudioMode::Simple => pl_studio_runtime::StudioRole::Executor,
            pl_studio_runtime::StudioMode::Task => pl_studio_runtime::StudioRole::Planner,
        });
    let general_settings = bridge
        .studio
        .store()
        .load_setting("flutterSettings:general")
        .await?
        .map(|value| {
            serde_json::from_str::<BridgeGeneralSettingsDto>(&value)
                .context("invalid stored Flutter general settings")
        })
        .transpose()?
        .unwrap_or_default();
    let settings = studio_settings_dto(&config, general_settings, web_search_role)?;

    Ok(BridgeStudioSnapshotResponse {
        projects: projects.into_iter().map(project_dto).collect(),
        selected_project_id,
        threads: threads
            .into_iter()
            .map(thread_from_record)
            .map(bridge_thread)
            .collect(),
        selected_thread_id,
        selected_thread_task,
        recovery_issues: recovery_issues
            .into_iter()
            .map(bridge_recovery_issue)
            .collect(),
        settings,
    })
}

fn select_available_project(
    projects: &[pl_studio_runtime::ProjectRecord],
    requested_project_id: Option<&str>,
    blocked_project_ids: &HashSet<String>,
) -> Option<pl_studio_runtime::ProjectRecord> {
    requested_project_id
        .and_then(|project_id| {
            projects
                .iter()
                .find(|project| {
                    project.id == project_id && !blocked_project_ids.contains(&project.id)
                })
                .cloned()
        })
        .or_else(|| {
            projects
                .iter()
                .find(|project| !blocked_project_ids.contains(&project.id))
                .cloned()
        })
}

pub(super) fn ensure_project_recovery_available(
    bridge: &'static BridgeRuntime,
    project_id: &str,
) -> Result<()> {
    if bridge.studio.recovery_issues().iter().any(|issue| {
        issue.scope == pl_studio_runtime::StudioRecoveryIssueScope::Project
            && issue.project_id.as_deref() == Some(project_id)
    }) {
        anyhow::bail!("selected project is blocked by a recovery issue");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::select_available_project;
    use pl_studio_runtime::ProjectRecord;
    use std::collections::HashSet;

    #[test]
    fn blocked_requested_project_falls_back_to_healthy_project() {
        let projects = vec![project("broken"), project("healthy")];
        let blocked = HashSet::from(["broken".to_string()]);

        let selected = select_available_project(&projects, Some("broken"), &blocked).unwrap();

        assert_eq!(selected.id, "healthy");
    }

    #[test]
    fn all_blocked_projects_leave_selection_empty() {
        let projects = vec![project("broken")];
        let blocked = HashSet::from(["broken".to_string()]);

        let selected = select_available_project(&projects, Some("broken"), &blocked);

        assert_eq!(selected, None);
    }

    #[test]
    fn requested_healthy_project_takes_precedence() {
        let projects = vec![project("first"), project("requested")];

        let selected =
            select_available_project(&projects, Some("requested"), &HashSet::new()).unwrap();

        assert_eq!(selected.id, "requested");
    }

    fn project(id: &str) -> ProjectRecord {
        ProjectRecord {
            id: id.to_string(),
            name: id.to_string(),
            path: format!("C:/work/{id}"),
            updated_at: 0,
        }
    }
}

//! prompt/turn 提交入口：内容校验、附件接入、运行时就绪检查与 root 角色对齐。

use anyhow::{Context, Result, bail};

use crate::config::StudioRole;
use crate::studio::ThreadRecord;

use super::super::{
    StudioRuntime, StudioSubmitPromptOptions, StudioSubmitPromptRequest, StudioSubmitPromptResponse,
};

impl StudioRuntime {
    /// Starts a new active Turn for a Thread.
    pub async fn start_turn(
        &self,
        thread_id: String,
        request: pl_protocol::studio::StartTurnRequest,
    ) -> Result<StudioSubmitPromptResponse> {
        self.submit_prompt(StudioSubmitPromptRequest {
            thread_id,
            input: request.input,
            options: StudioSubmitPromptOptions {
                turn_policy: pl_core::AgentTurnSubmitPolicy::StartOnly,
                ..StudioSubmitPromptOptions::default()
            },
        })
        .await
    }

    /// Steers the currently active Turn for a Thread.
    pub async fn steer_turn(
        &self,
        thread_id: String,
        request: pl_protocol::studio::SteerTurnRequest,
    ) -> Result<StudioSubmitPromptResponse> {
        self.submit_prompt(StudioSubmitPromptRequest {
            thread_id,
            input: request.input,
            options: StudioSubmitPromptOptions {
                turn_policy: pl_core::AgentTurnSubmitPolicy::SteerOnly,
                ..StudioSubmitPromptOptions::default()
            },
        })
        .await
    }

    pub async fn submit_prompt(
        &self,
        request: StudioSubmitPromptRequest,
    ) -> Result<StudioSubmitPromptResponse> {
        validate_prompt_content(&request.input)?;
        // Serialize turn registration with the updater's final idle check.
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        self.submit_prompt_with_lifecycle_lock(request).await
    }

    pub(in crate::studio::runtime) async fn submit_prompt_with_lifecycle_lock(
        &self,
        request: StudioSubmitPromptRequest,
    ) -> Result<StudioSubmitPromptResponse> {
        let thread_record = self.read_owned_thread(&request.thread_id).await?;
        self.submit_prompt_for_owned_thread_with_lifecycle_lock(request, thread_record)
            .await
    }

    pub(in crate::studio::runtime) async fn submit_prompt_for_owned_thread_with_lifecycle_lock(
        &self,
        request: StudioSubmitPromptRequest,
        thread_record: ThreadRecord,
    ) -> Result<StudioSubmitPromptResponse> {
        let StudioSubmitPromptRequest {
            thread_id,
            input,
            options,
        } = request;
        validate_prompt_content(&input)?;
        let pl_protocol::studio::StudioPromptInput {
            text: prompt,
            attachment_draft_ids,
        } = input;
        anyhow::ensure!(
            thread_record.id == thread_id,
            "prompt Thread does not match its canonical owner"
        );
        let drafts = self
            .attachment_drafts
            .resolve(&attachment_draft_ids)
            .await?;
        let role = if thread_record.parent_thread_id.is_none() {
            StudioRole::Planner
        } else {
            StudioRole::from_key(&thread_record.role).context("Thread has an invalid model role")?
        };
        let config = self.config_runtime.read()?;
        let route = config.config.models.resolve(&role.id())?;
        self.attachment_drafts
            .validate_for_model(&route.model, &drafts)?;
        let attachments = self
            .store
            .promote_attachment_drafts(&thread_id, &drafts)
            .await?;
        self.agent_facility
            .resources
            .insert_thread_attachments(&thread_id, attachments.clone())
            .await;
        let attachment_ids = attachments
            .iter()
            .map(|attachment| attachment.id.clone())
            .collect::<Vec<_>>();
        let thread_attachments = attachments
            .iter()
            .map(crate::studio::store::attachment::thread_attachment)
            .collect::<Vec<_>>();
        self.agent_facility
            .resources
            .insert_initial_remote_urls(attachments.iter().zip(&drafts).filter_map(
                |(attachment, draft)| {
                    draft
                        .initial_remote_url
                        .clone()
                        .map(|url| (attachment.id.clone(), url))
                },
            ))
            .await;
        let mut accepted = false;
        let result = async {
            self.ensure_prompt_runtime_ready().await?;
            let (handle, agent_id) = self
                .ensure_thread_agent_for_record(thread_record.clone())
                .await?;
            let mut snapshot = handle
                .snapshot(agent_id.clone())
                .await
                .map_err(|error| anyhow::anyhow!(error))?;
            if matches!(
                &snapshot.state,
                pl_core::AgentState::Faulted(faulted)
                    if faulted.classification().is_recoverable()
            ) {
                snapshot = handle
                    .recover_faulted(agent_id.clone())
                    .await
                    .map_err(|error| anyhow::anyhow!(error))?;
            }
            self.reconcile_root_role(&handle, &agent_id, &thread_record, &snapshot)
                .await?;
            let thread = pl_core::ThreadId::new(thread_id.clone())?;
            let metadata = submit_metadata();
            let presentation = options.presentation;
            let turn_id = handle
                .submit(
                    agent_id.clone(),
                    pl_core::AgentSubmitRequest::start(thread.clone(), prompt.clone())
                        .with_presentation(presentation)
                        .with_attachments(thread_attachments)
                        .with_metadata(metadata)
                        .with_turn_policy(options.turn_policy),
                )
                .await
                .map_err(|error| anyhow::anyhow!(error))?;
            accepted = true;
            let cursor = handle
                .thread_snapshot(&thread)
                .map_err(|error| anyhow::anyhow!(error))?
                .revision;
            Ok::<_, anyhow::Error>(StudioSubmitPromptResponse {
                thread_id,
                turn_id: turn_id.into_string(),
                cursor,
            })
        }
        .await;
        if accepted {
            self.agent_facility
                .product_events
                .record_attachments(attachments);
            self.attachment_drafts.commit(&attachment_draft_ids).await;
            return result;
        }
        self.agent_facility
            .resources
            .remove_initial_remote_urls(&attachment_ids)
            .await;
        self.agent_facility
            .resources
            .remove_thread_attachment_ids(&thread_record.id, &attachment_ids)
            .await;
        result
    }

    pub(in crate::studio::runtime) async fn ensure_prompt_runtime_ready(&self) -> Result<()> {
        if !self.runtime_snapshot().await?.state.is_ready() {
            bail!("Studio runtime is not ready");
        }
        Ok(())
    }

    /// 把 root actor 的 `identity.role` 对齐到统一 planner 模型路由。
    /// 非 root、已一致或 actor 非 idle 时是 no-op。
    async fn reconcile_root_role(
        &self,
        handle: &pl_core::AgentRuntimeHandle,
        agent_id: &pl_core::ThreadId,
        thread: &ThreadRecord,
        snapshot: &pl_core::AgentSnapshot,
    ) -> Result<()> {
        if thread.parent_thread_id.is_some() {
            return Ok(());
        }
        let desired = StudioRole::Planner.id();
        if snapshot.identity.role == desired {
            return Ok(());
        }
        if snapshot.active_turn_id().is_some()
            || snapshot.pending_inputs > 0
            || !snapshot.state.is_idle()
        {
            return Ok(());
        }
        handle
            .reconfigure_idle_role(agent_id.clone(), desired)
            .await
            .map_err(|error| anyhow::anyhow!(error))?;
        Ok(())
    }
}

pub(in crate::studio::runtime) fn validate_prompt_content(
    input: &pl_protocol::studio::StudioPromptInput,
) -> Result<()> {
    if input.text.trim().is_empty() && input.attachment_draft_ids.is_empty() {
        bail!("prompt is empty");
    }
    Ok(())
}

fn submit_metadata() -> serde_json::Value {
    serde_json::json!({
        "historyPolicy": "persist",
    })
}

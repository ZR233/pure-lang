//! 交互读取、收束与重启后的 pending 交互恢复。

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use futures::FutureExt;

use crate::studio::{InteractionEmitter, resolution_matches_kind};
use crate::{InteractionKind, InteractionRequest, InteractionResolution, InteractionStatus};

use super::super::{StudioResolveInteractionResponse, StudioRuntime};

impl StudioRuntime {
    pub async fn resolve_interaction(
        &self,
        interaction_id: String,
        resolution: InteractionResolution,
    ) -> Result<StudioResolveInteractionResponse> {
        // 这是已经开始的 Turn 的收束入口。持久化降级只暂停新的生命周期，
        // 不能阻止用户回答、审批或确认当前交互。
        let current = self
            .read_interaction_for_resolve(&interaction_id)
            .await?
            .context("interaction not found")?;
        let thread_id = current.scope.thread_id.clone();
        if !resolution_matches_kind(current.kind(), &resolution) {
            bail!("interaction resolution kind does not match interaction");
        }
        let emitter = self.interaction_emitter(thread_id.clone());

        if current.status() != InteractionStatus::Pending {
            return Ok(StudioResolveInteractionResponse {
                thread_id,
                interaction: current,
            });
        }
        let resolved = if current.kind() == InteractionKind::UserInput {
            let mail_id =
                pl_core::AgentInteractionContinuationRequest::stable_mail_id(&interaction_id);
            let continuation = current
                .continuation_input(&resolution)?
                .context("UserInput interaction has no continuation preset")?;
            self.submit_durable_interaction_continuation(
                &current,
                resolution,
                continuation,
                serde_json::json!({
                    "interactionResolutionId": interaction_id,
                    "mailId": mail_id,
                }),
            )
            .await?
        } else {
            self.agent_facility
                .interactions
                .resolve_loaded(current, resolution, emitter)
                .await?
        };
        Ok(StudioResolveInteractionResponse {
            thread_id,
            interaction: resolved,
        })
    }

    /// 内存优先读取交互：pending 交互必须来自驻留 actor 的权威快照；
    /// 已离开快照的历史交互（非 pending）回 SQLite 冷源。
    pub(in crate::studio) async fn read_interaction_for_resolve(
        &self,
        interaction_id: &str,
    ) -> Result<Option<InteractionRequest>> {
        if let Some(framework) = self.agent_facility.framework.lock().await.clone() {
            let handle = framework.handle();
            for agent in handle.directory_snapshot().agents {
                let Ok(snapshot) = handle.thread_snapshot(&agent.identity.id) else {
                    continue;
                };
                if let Some(found) = snapshot
                    .interactions
                    .iter()
                    .find(|candidate| candidate.interaction_id == interaction_id)
                {
                    return Ok(Some(found.clone()));
                }
            }
        }
        self.store.read_interaction(interaction_id).await
    }

    /// 读取驻留线程的 pending 交互；未驻留线程没有 pending 交互
    /// （钉住集合恢复 + LRU 空闲淘汰不变量）。
    pub(in crate::studio) async fn pending_thread_interactions(
        &self,
        thread_id: &str,
    ) -> Result<Vec<InteractionRequest>> {
        let Some((handle, agent_id)) = self.try_get_thread_handle(thread_id).await? else {
            return Ok(Vec::new());
        };
        match handle.thread_snapshot(&agent_id) {
            Ok(snapshot) => Ok(snapshot.interactions),
            Err(pl_core::AgentRuntimeError::NotFound(_)) => Ok(Vec::new()),
            Err(error) => Err(anyhow::anyhow!(error)),
        }
    }

    pub(in crate::studio::runtime) async fn record_thread_facts(
        &self,
        thread_id: &str,
        facts: Vec<pl_core::ThreadNotificationFact>,
    ) -> Result<()> {
        let runtime = self.agent_framework().await?.handle();
        let agent_path = self.thread_agent_path(thread_id).await?;
        runtime
            .record_thread_facts(
                agent_path,
                pl_core::ThreadId::new(thread_id.to_string())?,
                facts,
            )
            .await
            .map_err(Into::into)
    }

    pub(in crate::studio::runtime) fn interaction_emitter(
        &self,
        thread_id: String,
    ) -> InteractionEmitter {
        let runtime = self.clone();
        Arc::new(move |interaction| {
            let runtime = runtime.clone();
            let thread_id = thread_id.clone();
            async move {
                runtime
                    .record_thread_facts(
                        &thread_id,
                        vec![pl_core::ThreadNotificationFact::durable(
                            interaction.updated_at,
                            pl_protocol::ThreadNotification::InteractionChanged {
                                interaction: Box::new(interaction),
                            },
                        )],
                    )
                    .await?;
                Ok(())
            }
            .boxed()
        })
    }

    pub(in crate::studio::runtime) async fn recover_interactions_after_restart(
        &self,
    ) -> Result<()> {
        for interaction in self.store.list_restart_recoverable_user_inputs().await? {
            let thread_id = interaction.scope.thread_id.clone();
            let _ = self.ensure_thread_agent(&thread_id).await?;
            let emitter = self.interaction_emitter(thread_id);
            let recovered = self
                .agent_facility
                .interactions
                .recover_user_input(interaction, emitter)
                .await?;
            self.store
                .mark_restart_user_input_recovered(&recovered)
                .await?;
        }

        let mut thread_ids = self
            .store
            .list_threads_with_transient_pending_interactions()
            .await?;
        thread_ids.sort();
        thread_ids.dedup();
        for thread_id in thread_ids {
            // pending interaction 可能先于 framework registration 持久化。
            // 先恢复 canonical owner，事件仍由 PL actor 分配序列、持久化并广播。
            let (handle, _) = self.ensure_thread_agent(&thread_id).await?;
            let canonical = handle
                .thread_snapshot(&pl_core::ThreadId::new(thread_id.clone())?)
                .map_err(|error| anyhow::anyhow!(error))?;
            let emitter = self.interaction_emitter(thread_id.clone());
            for interaction in self.store.list_pending_interactions(&thread_id).await? {
                if interaction.kind() == InteractionKind::ToolApproval
                    || canonical
                        .interactions
                        .iter()
                        .any(|candidate| candidate.interaction_id == interaction.interaction_id)
                {
                    continue;
                }
                emitter(interaction).await?;
            }
            let canonical = handle
                .thread_snapshot(&pl_core::ThreadId::new(thread_id.clone())?)
                .map_err(|error| anyhow::anyhow!(error))?;
            self.agent_facility
                .interactions
                .cancel_recovered_tool_approvals(
                    canonical.interactions,
                    "application restarted before approval completed",
                    emitter,
                )
                .await?;
        }
        Ok(())
    }
}

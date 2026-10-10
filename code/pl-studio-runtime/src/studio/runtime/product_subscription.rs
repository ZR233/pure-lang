//! typed 产品 topic 订阅：登记接收者、读取领域基线与帧交付。

use anyhow::Result;
use tokio::sync::broadcast;

use crate::{
    StudioProductBaseline, StudioProductBaselineState, StudioProductFrame, StudioProductTopic,
};

use super::StudioRuntime;

const BASELINE_THREAD_PAGE_LIMIT: usize = 50;

/// 一个 typed 产品 topic 订阅句柄。
///
/// 订阅在构造时先登记 topic 接收者，再读取该领域 canonical 基线；`recv` 首帧返回
/// 基线，之后只交付该领域事件。全局 `sequence` 允许不连续，只有
/// [`StudioProductFrame::Lagged`] 表示无法证明连续，需要仅重读该 topic 的基线。查询不
/// activation、不扫描、不 mutation；订阅本身不创建新的 owner。
pub struct StudioProductTopicSubscription {
    topic: StudioProductTopic,
    receiver: broadcast::Receiver<crate::StudioProductEventEnvelope>,
    baseline: Option<StudioProductBaseline>,
}

impl StudioProductTopicSubscription {
    /// 订阅的 topic。
    pub fn topic(&self) -> &StudioProductTopic {
        &self.topic
    }

    /// 交付下一帧：首帧为基线，其后为该领域事件；`None` 表示通道已关闭。
    pub async fn recv(&mut self) -> Result<Option<StudioProductFrame>> {
        if let Some(baseline) = self.baseline.take() {
            return Ok(Some(StudioProductFrame::Baseline(Box::new(baseline))));
        }
        match self.receiver.recv().await {
            Ok(event) => Ok(Some(StudioProductFrame::Event(Box::new(event)))),
            Err(broadcast::error::RecvError::Lagged(dropped)) => {
                Ok(Some(StudioProductFrame::Lagged {
                    topic: self.topic.clone(),
                    dropped,
                }))
            }
            Err(broadcast::error::RecvError::Closed) => Ok(None),
        }
    }
}

impl StudioRuntime {
    /// 订阅一个 typed 产品 topic。
    ///
    /// 先登记接收者再读取基线：登记与基线之间发布的事件仍留在通道缓冲里，消费端按
    /// payload 的领域 revision 合并，旧基线不会覆盖较新事件。带作用域 topic 的身份
    /// 为空时返回类型化错误。
    pub async fn subscribe_product_topic(
        &self,
        topic: StudioProductTopic,
    ) -> Result<StudioProductTopicSubscription> {
        topic.validate()?;
        let receiver = self.agent_facility.product_events.subscribe_topic(&topic);
        let baseline = self.read_product_baseline(&topic).await?;
        Ok(StudioProductTopicSubscription {
            topic,
            receiver,
            baseline: Some(baseline),
        })
    }

    /// 读取一个 topic 的当前 canonical 基线；纯查询，不产生副作用。
    pub async fn read_product_baseline(
        &self,
        topic: &StudioProductTopic,
    ) -> Result<StudioProductBaseline> {
        topic.validate()?;
        let state = match topic {
            StudioProductTopic::ProjectDirectory => {
                StudioProductBaselineState::ProjectDirectory(Box::new(
                    self.agent_facility
                        .product_events
                        .read_project_directory()
                        .await?,
                ))
            }
            StudioProductTopic::ThreadDirectory => {
                StudioProductBaselineState::ThreadDirectory(Box::new(
                    self.agent_facility
                        .product_events
                        .read_thread_directory_page(None, BASELINE_THREAD_PAGE_LIMIT)
                        .await?,
                ))
            }
            StudioProductTopic::AgentDirectory => {
                StudioProductBaselineState::AgentDirectory(Box::new(
                    self.agent_facility
                        .product_events
                        .read_agent_directory()
                        .await,
                ))
            }
            StudioProductTopic::SettingsConfig => {
                let settings =
                    super::settings_api::settings_config_snapshot(&self.config_runtime.read()?)?;
                StudioProductBaselineState::SettingsConfig(Box::new(
                    crate::StudioSettingsConfigStateSnapshot {
                        state: pl_protocol::ObservedResource::ready(
                            settings.revision,
                            settings.updated_at,
                            settings,
                        ),
                    },
                ))
            }
            StudioProductTopic::ModelCatalog => {
                let settings = self.config_runtime.read()?;
                let catalog_state = self.config_runtime.read_catalog()?;
                let catalog =
                    super::settings_api::model_catalog_snapshot(&settings, &catalog_state)?;
                StudioProductBaselineState::ModelCatalog(Box::new(
                    crate::StudioModelCatalogStateSnapshot {
                        state: pl_protocol::ObservedResource::ready(
                            catalog.revision,
                            catalog.updated_at,
                            catalog,
                        ),
                    },
                ))
            }
            StudioProductTopic::Recovery => {
                StudioProductBaselineState::Recovery(Box::new(crate::StudioRecoveryStateSnapshot {
                    state: self.recovery.state(),
                }))
            }
            StudioProductTopic::Mcp => {
                StudioProductBaselineState::Mcp(Box::new(self.read_mcp_state().await?))
            }
            StudioProductTopic::Lsp => {
                StudioProductBaselineState::Lsp(Box::new(self.read_lsp_state().await))
            }
            StudioProductTopic::Skills { project_id } => StudioProductBaselineState::Skills(
                Box::new(self.read_skills_state(project_id).await),
            ),
            StudioProductTopic::ThreadModeCatalog => StudioProductBaselineState::ThreadModeCatalog(
                Box::new(self.read_thread_mode_catalog()),
            ),
            StudioProductTopic::ProviderUsage => StudioProductBaselineState::ProviderUsage(
                Box::new(self.read_provider_usage_state().await),
            ),
            StudioProductTopic::ModelPerformance => StudioProductBaselineState::ModelPerformance(
                Box::new(self.model_performance.snapshot().await),
            ),
            StudioProductTopic::SessionCosts { root_thread_id } => {
                StudioProductBaselineState::SessionCosts(Box::new(
                    self.model_performance
                        .session_costs_snapshot(root_thread_id)
                        .await,
                ))
            }
            StudioProductTopic::Updater => {
                StudioProductBaselineState::Updater(Box::new(self.read_update_state().await))
            }
            StudioProductTopic::Persistence => StudioProductBaselineState::Persistence(Box::new(
                self.agent_facility.product_events.persistence_state(),
            )),
            StudioProductTopic::PersistenceQueue => StudioProductBaselineState::PersistenceQueue(
                Box::new(self.agent_facility.product_events.persistence_queue_state()),
            ),
            StudioProductTopic::AgentProfiles => StudioProductBaselineState::AgentProfiles(
                Box::new(self.read_agent_profiles_state()?),
            ),
        };
        let revision = state.revision();
        Ok(StudioProductBaseline {
            topic: topic.clone(),
            revision,
            state,
        })
    }
}

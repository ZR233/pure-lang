use anyhow::{Context, Result};
use pl_core::{
    AgentCommand, AgentRecoveryTarget, AgentState, MailboxCommand, MailboxDeliveryState, TurnId,
    canonical_content_hash,
};
use pl_protocol::{Turn, TurnCancellationCause, TurnCommand, TurnState};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder, TransactionTrait,
};

use crate::studio::StudioStore;
use crate::studio::entity::{
    thread, thread_context_segment, thread_input, thread_session_state, turn,
};
use crate::studio::ids::unix_seconds;

pub(in crate::studio) struct ThreadRuntimeSeed {
    pub thread_revision: u64,
    pub runtime_revision: u64,
    pub event_sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::studio) enum UnregisteredThreadFault {
    Faulted,
    RuntimeOwned,
}

impl StudioStore {
    /// 在 actor 首次注册前把已创建的 child Thread 收敛为 canonical Faulted 状态。
    /// 已由 runtime 持有 revision 的 Thread 留给 core spawn compensation 处理。
    pub(in crate::studio) async fn fault_unregistered_child_thread(
        &self,
        thread_id: &str,
        error: &str,
    ) -> Result<UnregisteredThreadFault> {
        let row = thread::Entity::find_by_id(thread_id)
            .one(&self.db)
            .await?
            .with_context(|| format!("spawn compensation Thread not found: {thread_id}"))?;
        if row.runtime_revision.is_some() {
            return Ok(UnregisteredThreadFault::RuntimeOwned);
        }
        let state: AgentState = serde_json::from_str(&row.state_json)?;
        let state = state
            .decide(AgentCommand::Fault {
                error: pl_protocol::StateError {
                    code: "agentRegistrationFailed".to_string(),
                    message: error.to_string(),
                    retryable: false,
                },
                turn_id: None,
                classification: pl_core::AgentFaultClassification::RecoverableRuntime,
            })?
            .next_state;
        let mut active = row.into_active_model();
        active.state_json = Set(serde_json::to_string(&state)?);
        active.updated_at = Set(unix_seconds());
        active.update(&self.db).await?;
        Ok(UnregisteredThreadFault::Faulted)
    }

    pub(in crate::studio) async fn reset_agent_sessions_for_root(
        &self,
        root_thread_id: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let threads = thread::Entity::find()
                .filter(thread::Column::RootThreadId.eq(root_thread_id))
                .order_by_asc(thread::Column::CreatedAt)
                .order_by_asc(thread::Column::Id)
                .all(&tx)
                .await?;
            anyhow::ensure!(!threads.is_empty(), "Thread reset target not found");
            let now = unix_seconds();
            let state = pl_protocol::AgentWorkingState::default();
            let state_json = serde_json::to_string(&state)?;
            let state_hash = canonical_content_hash(state_json.as_bytes());
            for thread_row in threads {
                thread_context_segment::Entity::delete_many()
                    .filter(thread_context_segment::Column::ThreadId.eq(&thread_row.id))
                    .exec(&tx)
                    .await?;
                thread_session_state::Entity::delete_by_id(thread_row.id.clone())
                    .exec(&tx)
                    .await?;
                thread_session_state::ActiveModel {
                    thread_id: Set(thread_row.id.clone()),
                    revision: Set(0),
                    state_json: Set(state_json.clone()),
                    state_hash: Set(state_hash.clone()),
                    updated_at: Set(now),
                }
                .insert(&tx)
                .await?;

                let inputs = thread_input::Entity::find()
                    .filter(thread_input::Column::ThreadId.eq(&thread_row.id))
                    .filter(thread_input::Column::StateKind.ne("consumed"))
                    .all(&tx)
                    .await?;
                for input in inputs {
                    let mut state: MailboxDeliveryState = serde_json::from_str(&input.state_json)?;
                    if state.is_pending() {
                        state = state
                            .decide(MailboxCommand::Claim {
                                turn_id: TurnId::new(input.turn_id.clone())?,
                            })?
                            .next_state;
                    }
                    let turn_id = state
                        .turn_id()
                        .cloned()
                        .context("claimed mailbox is missing its Turn identity")?;
                    let checkpoint_seq = state.checkpoint_seq().unwrap_or_default();
                    let state = state
                        .decide(MailboxCommand::Consume {
                            turn_id,
                            checkpoint_seq,
                        })?
                        .next_state;
                    let mut active = input.into_active_model();
                    active.state_json = Set(serde_json::to_string(&state)?);
                    active.update(&tx).await?;
                }

                let active_turns = turn::Entity::find()
                    .filter(turn::Column::ThreadId.eq(&thread_row.id))
                    .filter(turn::Column::StateKind.is_in(["queued", "running"]))
                    .all(&tx)
                    .await?;
                for turn in active_turns {
                    let state: TurnState = serde_json::from_str(&turn.state_json)?;
                    let aggregate = Turn {
                        id: turn.id.clone(),
                        thread_id: turn.thread_id.clone(),
                        revision: u64::try_from(turn.revision)?,
                        state,
                        updated_at: turn.updated_at,
                    };
                    let mut aggregate = aggregate;
                    let decision = aggregate.decide(TurnCommand::Cancel {
                        turn_id: aggregate.id.clone(),
                        expected_revision: aggregate.revision,
                        cause: TurnCancellationCause::Recovery,
                        completed_at: now,
                    })?;
                    aggregate.apply(decision, now);
                    let mut active = turn.into_active_model();
                    active.revision = Set(i64::try_from(aggregate.revision)?);
                    active.state_json = Set(serde_json::to_string(&aggregate.state)?);
                    active.updated_at = Set(aggregate.updated_at);
                    active.update(&tx).await?;
                }

                let is_root = thread_row.id == root_thread_id;
                let state: AgentState = serde_json::from_str(&thread_row.state_json)?;
                let state = state
                    .decide(AgentCommand::Recover {
                        target: if is_root {
                            AgentRecoveryTarget::Idle
                        } else {
                            AgentRecoveryTarget::Closed
                        },
                    })?
                    .next_state;
                let mut active = thread_row.into_active_model();
                active.runtime_revision = Set(None);
                active.state_json = Set(serde_json::to_string(&state)?);
                active.last_context_tokens = Set(None);
                active.updated_at = Set(now);
                active.update(&tx).await?;
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        match result {
            Ok(()) => {
                tx.commit().await?;
                Ok(())
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub(in crate::studio) async fn thread_runtime_seed(
        &self,
        thread_id: &str,
    ) -> Result<ThreadRuntimeSeed> {
        let row = thread::Entity::find_by_id(thread_id)
            .one(&self.db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Thread runtime seed not found"))?;
        let event_sequence = u64::try_from(row.event_sequence)?;
        Ok(ThreadRuntimeSeed {
            thread_revision: u64::try_from(row.revision)?,
            runtime_revision: event_sequence.saturating_add(1).max(1),
            event_sequence: event_sequence.saturating_add(1).max(1),
        })
    }

    pub(crate) async fn read_thread_todo(
        &self,
        thread_id: &str,
    ) -> Result<Option<pl_protocol::TodoListSnapshot>> {
        let Some(row) = thread_session_state::Entity::find_by_id(thread_id.to_string())
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        let state: pl_protocol::AgentWorkingState =
            serde_json::from_str(&row.state_json).context("thread working state is invalid")?;
        state
            .sections
            .iter()
            .find(|section| section.id.as_str() == pl_core::CURRENT_TODO_SECTION_ID)
            .map(|section| {
                serde_json::from_str(&section.content).context("thread todo section is invalid")
            })
            .transpose()
    }
}

//! 在调用方事务内应用 Thread commit，并生成幂等判定的 payload hash。

use pl_core::ThreadCommit;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, IntoActiveModel};

use crate::PureError;
use crate::studio::entity::thread;
use crate::studio::store::object::{ThreadCommitReceipt, load_object, put_object};

use super::billing::persist_inference_billing;
use super::context::{persist_session_snapshot, serialize_thread_metadata};
use super::inputs::persist_inputs;
use super::projection::{persist_state_turns, persist_thread_notifications};
use super::submissions::persist_submission;
use super::{i64_from_u64, store_error, u64_from_i64};

/// 在调用方事务内应用一次 Thread commit；不负责 begin/commit/rollback。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::studio::agent_host) enum ApplyCommitOutcome {
    Applied,
    AlreadyApplied,
    RevisionConflict { actual_revision: Option<u64> },
}

pub(in crate::studio::agent_host) async fn apply_state_commit(
    tx: &sea_orm::DatabaseTransaction,
    commit: &ThreadCommit,
) -> Result<ApplyCommitOutcome, PureError> {
    let thread_id = commit.agent_id.to_string();
    let payload_hash = thread_commit_payload_hash(commit)?;
    let Some(existing) = thread::Entity::find_by_id(thread_id.clone())
        .one(tx)
        .await
        .map_err(store_error)?
    else {
        return Err(store_error(format!(
            "Thread {thread_id} must exist before runtime registration"
        )));
    };
    let actual_revision = existing.runtime_revision.map(u64_from_i64).transpose()?;
    if actual_revision != commit.expected_revision {
        if actual_revision == Some(commit.next_state.snapshot.revision)
            && load_object::<ThreadCommitReceipt>(tx, &thread_id)
                .await
                .map_err(store_error)?
                .is_some_and(|receipt| {
                    receipt.revision == commit.next_state.snapshot.revision
                        && receipt.payload_hash == payload_hash
                })
        {
            return Ok(ApplyCommitOutcome::AlreadyApplied);
        }
        return Ok(ApplyCommitOutcome::RevisionConflict { actual_revision });
    }

    let mut active = existing.into_active_model();
    active.parent_thread_id = Set(commit
        .next_state
        .snapshot
        .identity
        .parent_id
        .as_ref()
        .map(ToString::to_string));
    active.role = Set(commit.next_state.snapshot.identity.role.to_string());
    active.state_json = Set(serde_json::to_string(&commit.next_state.snapshot.state)?);
    active.revision = Set(i64_from_u64(commit.next_state.session.thread_revision)?);
    active.runtime_revision = Set(Some(i64_from_u64(commit.next_state.snapshot.revision)?));
    active.event_sequence = Set(i64_from_u64(commit.next_state.snapshot.event_sequence)?);
    active.metadata_json = Set(serialize_thread_metadata(
        &commit.next_state.session.metadata,
        &commit.next_state.session.session,
    )?);
    active.usage_json = Set(serde_json::to_string(&commit.next_state.session.usage)?);
    active.last_context_tokens = Set(commit
        .next_state
        .session
        .last_context_tokens
        .map(i64_from_u64)
        .transpose()?);
    active.trace_sequence = Set(i64_from_u64(commit.next_state.session.trace_sequence)?);
    active.updated_at = Set(commit.next_state.snapshot.updated_at);
    active.update(tx).await.map_err(store_error)?;

    persist_inputs(tx, &commit.next_state).await?;
    persist_state_turns(tx, &commit.next_state).await?;
    persist_thread_notifications(tx, commit).await?;
    persist_session_snapshot(tx, commit).await?;
    persist_inference_billing(tx, commit).await?;
    persist_submission(tx, commit).await?;
    put_object(
        tx,
        &thread_id,
        &ThreadCommitReceipt {
            revision: commit.next_state.snapshot.revision,
            payload_hash,
        },
        commit.next_state.snapshot.updated_at,
    )
    .await
    .map_err(store_error)?;
    Ok(ApplyCommitOutcome::Applied)
}

/// 在 persistence worker 内把 typed commit 转成稳定的 receipt payload 并计算 hash。
fn thread_commit_payload_hash(commit: &ThreadCommit) -> Result<String, PureError> {
    let persistence = match commit.persistence {
        pl_core::PersistenceClass::Coalescible => "coalescible",
        pl_core::PersistenceClass::Standard => "standard",
        pl_core::PersistenceClass::Settlement => "settlement",
    };
    let mutation = match &commit.mutation {
        pl_core::ThreadMutation::SnapshotAndQueue => serde_json::json!({
            "kind": "snapshotAndQueue"
        }),
        pl_core::ThreadMutation::ReplaceThread { thread_id } => serde_json::json!({
            "kind": "replaceThread",
            "threadId": thread_id.as_str(),
        }),
        pl_core::ThreadMutation::AppendTrace => serde_json::json!({
            "kind": "appendTrace"
        }),
        pl_core::ThreadMutation::AppendThreadNotifications { thread_id } => {
            serde_json::json!({
                "kind": "appendThreadNotifications",
                "threadId": thread_id.as_str(),
            })
        }
    };
    let context = commit.facts.context.as_ref().map(|context| match context {
        pl_core::ThreadContextMutation::Append { items } => serde_json::json!({
            "kind": "append",
            "items": items,
        }),
        pl_core::ThreadContextMutation::Replace { items } => serde_json::json!({
            "kind": "replace",
            "items": items,
        }),
    });
    let inference = commit.facts.inference.as_ref().map(|inference| {
        serde_json::json!({
            "billing": &inference.billing,
            "runtimeDelta": &inference.runtime_delta,
        })
    });
    let submission = commit.facts.submission.as_ref().map(|submission| {
        serde_json::json!({
            "report": &submission.report,
            "detail": &submission.detail,
            "createdAt": submission.created_at,
        })
    });
    let payload = serde_json::json!({
        "agentId": commit.agent_id.as_str(),
        "persistence": persistence,
        "expectedRevision": commit.expected_revision,
        "nextState": {
            "snapshot": &commit.next_state.snapshot,
            "session": {
                "metadata": &commit.next_state.session.metadata,
                "canonical": commit.next_state.session.session.snapshot(),
                "usage": &commit.next_state.session.usage,
                "billingByTurn": &commit.next_state.session.billing_by_turn,
                "lastContextTokens": commit.next_state.session.last_context_tokens,
                "traceSequence": commit.next_state.session.trace_sequence,
                "threadRevision": commit.next_state.session.thread_revision,
            },
            "pendingInputs": &commit.next_state.pending_inputs,
            "activeInput": &commit.next_state.active_input,
        },
        "facts": {
            "threadId": commit.facts.thread_id.as_str(),
            "turnId": commit.facts.turn_id.as_ref().map(|turn_id| turn_id.as_str()),
            "throughRevision": commit.facts.through_revision,
            "revision": commit.facts.revision,
            "notifications": &commit.facts.notifications,
            "turnTransition": &commit.facts.turn_transition,
            "context": context,
            "projectionSnapshot": &commit.facts.projection_snapshot,
            "runtimeEvents": &commit.facts.runtime_events,
            "traceEvents": &commit.facts.trace_events,
            "inference": inference,
            "submission": submission,
        },
        "mutation": mutation,
    });
    let encoded = serde_json::to_vec(&payload)?;
    Ok(pl_core::canonical_content_hash(&encoded))
}

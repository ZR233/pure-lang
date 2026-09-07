use std::collections::BTreeSet;
use std::sync::Arc;

use sea_orm::{ConnectionTrait, TransactionTrait};

use super::{SessionStoreError, SqliteSessionStore, sqlite};
use crate::{
    AgentSessionTimelineQuery, AgentSessionTimelineRepositoryPage, RestoredAgentRuntime, ThreadId,
    ThreadRepository,
};

impl SqliteSessionStore {
    /// Reads durable lifecycle metadata without loading transcript or activating an actor.
    ///
    /// # Errors
    /// Returns database or payload validation errors.
    pub async fn read_agent_snapshot(
        &self,
        session_id: &str,
    ) -> Result<Option<crate::AgentSnapshot>, SessionStoreError> {
        Ok(
            sqlite::read::<crate::ThreadActorState>(&self.owner.shared.db, session_id, "pl.actor")
                .await?
                .map(|state| state.snapshot),
        )
    }
    /// Lists durable session identities without activating any actors.
    ///
    /// # Errors
    /// Returns database or identity decoding errors.
    pub async fn session_ids(&self) -> Result<Vec<String>, SessionStoreError> {
        let rows = self
            .owner
            .shared
            .db
            .query_all_raw(sqlite::statement(
                "SELECT DISTINCT session_id FROM session_entries ORDER BY session_id",
                vec![],
            ))
            .await?;
        rows.into_iter()
            .map(|row| Ok(row.try_get("", "session_id")?))
            .collect()
    }

    /// Reads opaque records including unknown extension types, in stable creation order.
    ///
    /// # Errors
    /// Returns database or malformed envelope errors. This never rewrites payloads.
    pub async fn read_entries(
        &self,
        session_id: &str,
        type_id: Option<&str>,
    ) -> Result<Vec<crate::session::entry::SessionEntry>, SessionStoreError> {
        sqlite::entries(&self.owner.shared.db, session_id, type_id).await
    }

    /// Restores a coherent cold snapshot without activating or executing its session.
    ///
    /// # Errors
    /// Returns database or payload validation failures.
    pub async fn read_session(
        &self,
        session_id: &str,
    ) -> Result<Option<RestoredAgentRuntime>, SessionStoreError> {
        let tx = self.owner.shared.db.begin().await?;
        let restored = sqlite::restore(&tx, session_id).await?;
        tx.commit().await?;
        Ok(restored)
    }

    /// Reads a durable interaction without activating its owner.
    ///
    /// # Errors
    /// Returns database or payload validation failures.
    pub async fn read_interaction(
        &self,
        interaction_id: &str,
    ) -> Result<Option<crate::InteractionRequest>, SessionStoreError> {
        let row = self
            .owner
            .shared
            .db
            .query_one_raw(sqlite::statement(
                "SELECT * FROM session_entries WHERE id=? AND type_id='pl.interaction'",
                vec![format!("pl.interaction.{interaction_id}").into()],
            ))
            .await?;
        row.map(|row| {
            let entry = sqlite::decode_row(row)?;
            Ok(serde_json::from_value(entry.payload)?)
        })
        .transpose()
    }

    /// Reads older Turn pages using a session-bound exclusive cursor.
    ///
    /// # Errors
    /// Returns an invalid cursor, database or payload error.
    pub async fn list_turns(
        &self,
        session_id: &str,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<pl_protocol::ThreadTurnPage, SessionStoreError> {
        let tx = self.owner.shared.db.begin().await?;
        let before = if let Some(cursor) = cursor {
            Some(tx.query_one_raw(sqlite::statement("SELECT ordinal FROM session_entries WHERE session_id=? AND type_id='pl.turn' AND id=?",vec![session_id.into(),format!("pl.turn.{cursor}").into()])).await?
                .ok_or_else(|| SessionStoreError::Invalid("unknown session turn cursor".into()))?
                .try_get::<i64>("","ordinal")?)
        } else {
            None
        };
        let limit = limit.clamp(1, 200);
        let rows = tx.query_all_raw(sqlite::statement(
            "SELECT * FROM session_entries WHERE session_id=? AND type_id='pl.turn' AND (? IS NULL OR ordinal < ?) ORDER BY ordinal DESC LIMIT ?",
            vec![session_id.into(),before.into(),before.into(),((limit+1) as i64).into()])).await?;
        let mut turns: Vec<(u64, pl_protocol::Turn)> = rows
            .into_iter()
            .map(|row| {
                let entry = sqlite::decode_row(row)?;
                Ok((entry.ordinal, serde_json::from_value(entry.payload)?))
            })
            .collect::<Result<_, SessionStoreError>>()?;
        let has_more = turns.len() > limit;
        turns.truncate(limit);
        let next_cursor = has_more
            .then(|| turns.last().map(|(_, turn)| turn.id.clone()))
            .flatten();
        let state = sqlite::read::<crate::ThreadActorState>(&tx, session_id, "pl.actor").await?;
        let rolled_back: BTreeSet<_> = state
            .as_ref()
            .into_iter()
            .flat_map(|state| {
                state
                    .session
                    .session
                    .conversation_recovery()
                    .rolled_back_turn_ranges
                    .iter()
            })
            .flat_map(|range| range.turn_ids.iter().cloned())
            .collect();
        let mut history = Vec::with_capacity(turns.len());
        for (_, turn) in turns {
            let rows = tx.query_all_raw(sqlite::statement(
                "SELECT * FROM session_entries WHERE session_id=? AND type_id='pl.item' AND turn_id=? ORDER BY ordinal",
                vec![session_id.into(),turn.id.clone().into()])).await?;
            let items = rows
                .into_iter()
                .map(|row| {
                    let entry = sqlite::decode_row(row)?;
                    Ok(serde_json::from_value(entry.payload)?)
                })
                .collect::<Result<Vec<crate::ThreadItem>, SessionStoreError>>()?;
            let context_disposition = if rolled_back.contains(&turn.id) {
                pl_protocol::ThreadContextDisposition::RolledBack
            } else {
                pl_protocol::ThreadContextDisposition::Active
            };
            history.push(pl_protocol::ThreadTurnHistory {
                turn,
                items,
                context_disposition,
            });
        }
        tx.commit().await?;
        Ok(pl_protocol::ThreadTurnPage {
            turns: history,
            next_cursor,
        })
    }
}

impl ThreadRepository for SqliteSessionStore {
    type Error = Arc<SessionStoreError>;

    async fn restore_runtime(&self) -> Result<Vec<RestoredAgentRuntime>, Self::Error> {
        if self.persistence().stopped {
            return Err(Arc::new(SessionStoreError::Stopped));
        }
        let mut agents = std::collections::BTreeMap::new();
        let mut pinned = BTreeSet::new();
        for id in self.session_ids().await.map_err(Arc::new)? {
            if let Some(agent) = self.read_session(&id).await.map_err(Arc::new)? {
                let state = &agent.state.snapshot.state;
                let closed = matches!(state, crate::AgentState::Closed(_));
                if !closed
                    && (!agent.state.pending_inputs.is_empty()
                        || agent.state.active_input.is_some()
                        || !state.is_idle()
                        || state.is_budget_paused()
                        || agent.state.snapshot.identity.parent_id.is_some()
                        || agent
                            .thread_snapshot
                            .as_ref()
                            .is_some_and(|snapshot| !snapshot.snapshot.interactions.is_empty()))
                {
                    pinned.insert(id.clone());
                }
                agents.insert(id, agent);
            }
        }
        for id in pinned.clone() {
            let mut current = agents
                .get(&id)
                .and_then(|agent| agent.state.snapshot.identity.parent_id.clone());
            let mut seen = BTreeSet::new();
            while let Some(parent) = current {
                if !seen.insert(parent.clone()) {
                    return Err(Arc::new(SessionStoreError::Invalid(
                        "cyclic session ancestry".into(),
                    )));
                }
                pinned.insert(parent.to_string());
                current = agents
                    .get(parent.as_str())
                    .and_then(|agent| agent.state.snapshot.identity.parent_id.clone());
            }
        }
        Ok(agents
            .into_iter()
            .filter_map(|(id, agent)| pinned.contains(&id).then_some(agent))
            .collect())
    }

    async fn restore_thread(
        &self,
        thread_id: &ThreadId,
    ) -> Result<Option<RestoredAgentRuntime>, Self::Error> {
        self.read_session(thread_id.as_str())
            .await
            .map_err(Arc::new)
    }

    fn record_committed(&self, commit: crate::ThreadCommit) {
        self.record(commit);
    }
    fn is_durable(&self, thread_id: &ThreadId, revision: u64) -> bool {
        self.is_durable(thread_id.as_str(), revision)
    }
    async fn await_durable(&self, thread_id: &ThreadId, revision: u64) -> Result<(), Self::Error> {
        self.await_durable(thread_id.as_str(), revision).await
    }
    fn pending_commit_count(&self) -> usize {
        self.persistence().pending_commits
    }
    async fn flush(&self) -> Result<(), Self::Error> {
        self.flush().await
    }
    async fn shutdown(&self) -> Result<(), Self::Error> {
        self.shutdown().await
    }

    async fn list_submissions(
        &self,
        thread_id: &ThreadId,
        offset: usize,
        limit: usize,
    ) -> Result<crate::AgentSubmissionPage, Self::Error> {
        let agent = self
            .read_session(thread_id.as_str())
            .await
            .map_err(Arc::new)?
            .ok_or_else(|| {
                Arc::new(SessionStoreError::Invalid(format!(
                    "unknown session {thread_id}"
                )))
            })?;
        let history = agent.state.session.submissions;
        let limit = limit.max(1);
        let items: Vec<_> = history.iter().skip(offset).take(limit).cloned().collect();
        Ok(crate::AgentSubmissionPage {
            has_more: offset.saturating_add(items.len()) < history.len(),
            items,
            offset,
            limit,
            total: history.len(),
        })
    }

    async fn list_agent_session(
        &self,
        query: AgentSessionTimelineQuery,
    ) -> Result<AgentSessionTimelineRepositoryPage, Self::Error> {
        self.timeline(query).await.map_err(Arc::new)
    }
}

impl SqliteSessionStore {
    async fn timeline(
        &self,
        query: AgentSessionTimelineQuery,
    ) -> Result<AgentSessionTimelineRepositoryPage, SessionStoreError> {
        if !(1..=50).contains(&query.limit)
            || (query.anchor.is_some()
                && (query.watermark.is_none() || query.through_sequence.is_none()))
        {
            return Err(SessionStoreError::Invalid(
                "invalid session page request".into(),
            ));
        }
        let tx = self.owner.shared.db.begin().await?;
        let agent = sqlite::restore(&tx, query.target.as_str())
            .await?
            .ok_or_else(|| SessionStoreError::Invalid("unknown session".into()))?;
        let identity = agent.state.snapshot.identity;
        let current_sequence = agent.state.session.thread_revision;
        let through_sequence = query.through_sequence.unwrap_or(current_sequence);
        if through_sequence > current_sequence {
            return Err(SessionStoreError::Invalid(
                "cursor is ahead of session".into(),
            ));
        }
        let mut path = vec![identity.id.clone()];
        let mut parent = identity.parent_id.clone();
        while let Some(id) = parent {
            if path.contains(&id) {
                return Err(SessionStoreError::Invalid("cyclic session ancestry".into()));
            }
            let parent_state =
                sqlite::read::<crate::ThreadActorState>(&tx, id.as_str(), "pl.actor")
                    .await?
                    .ok_or_else(|| SessionStoreError::Invalid("missing session parent".into()))?;
            parent = parent_state.snapshot.identity.parent_id;
            path.push(id);
        }
        path.reverse();
        let mut items = agent
            .thread_snapshot
            .map(|snapshot| snapshot.snapshot.items)
            .unwrap_or_default();
        if query.detail == crate::AgentSessionReadDetail::Text {
            items.retain(|item| item.text().is_some());
        }
        let key = |item: &crate::ThreadItem| crate::AgentSessionTimelineKey {
            ordinal: item.ordinal,
            item_id: item.id.clone(),
        };
        for cursor in [query.anchor.as_ref(), query.watermark.as_ref()]
            .into_iter()
            .flatten()
        {
            if !items.iter().any(|item| key(item) == *cursor) {
                return Err(SessionStoreError::Invalid(
                    "cursor does not belong to session query".into(),
                ));
            }
        }
        let watermark = query.watermark.or_else(|| items.iter().map(key).max());
        items.retain(|item| {
            watermark
                .as_ref()
                .is_some_and(|watermark| key(item) <= *watermark)
                && query
                    .anchor
                    .as_ref()
                    .is_none_or(|anchor| match query.order {
                        crate::AgentSessionReadOrder::Ascending => key(item) > *anchor,
                        crate::AgentSessionReadOrder::Descending => key(item) < *anchor,
                    })
        });
        items.sort_by_key(key);
        if query.order == crate::AgentSessionReadOrder::Descending {
            items.reverse();
        }
        let has_more = items.len() > query.limit;
        items.truncate(query.limit);
        let next_anchor = has_more.then(|| items.last().map(key)).flatten();
        tx.commit().await?;
        Ok(AgentSessionTimelineRepositoryPage {
            identity,
            path,
            through_sequence,
            watermark,
            items,
            has_more,
            next_anchor,
        })
    }
}

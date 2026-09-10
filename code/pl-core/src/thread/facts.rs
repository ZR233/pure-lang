//! Append-only runtime facts, projected by hosts and ordered by stable source identity.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

impl Owner {
    pub(super) fn patch_runtime_facts(
        &mut self,
        facts: Vec<RuntimeFact>,
    ) -> Result<ContextSnapshot, ThreadError> {
        let mut sources = BTreeSet::new();
        let mut merged = self
            .state
            .runtime_facts
            .iter()
            .map(|fact| (fact.source_id.clone(), fact.clone()))
            .collect::<BTreeMap<_, _>>();
        for fact in facts {
            if fact.source_id.is_empty() || !sources.insert(fact.source_id.clone()) {
                return Err(ThreadError::InvalidContext);
            }
            merged.insert(fact.source_id.clone(), fact);
        }
        self.update_facts(merged.into_values().collect())
    }

    pub(super) fn update_facts(
        &mut self,
        facts: Vec<RuntimeFact>,
    ) -> Result<ContextSnapshot, ThreadError> {
        if !self.pending.is_empty() {
            return Err(ThreadError::PendingTools);
        }
        let context = stage_facts(&mut self.state, facts)?;
        self.publish();
        Ok(context)
    }
}

pub(super) fn stage_facts(
    state: &mut ThreadSnapshot,
    facts: Vec<RuntimeFact>,
) -> Result<ContextSnapshot, ThreadError> {
    if state.lifecycle != ThreadLifecycle::Open {
        return Err(ThreadError::Closed);
    }
    let mut next = BTreeMap::new();
    for fact in facts {
        if fact.source_id.is_empty() || next.insert(fact.source_id.clone(), fact).is_some() {
            return Err(ThreadError::InvalidContext);
        }
    }
    // An omitted known source is a cleared fact, kept as a tombstone for rewind/compaction.
    for old in state.runtime_facts.iter() {
        next.entry(old.source_id.clone())
            .or_insert_with(|| RuntimeFact {
                source_id: old.source_id.clone(),
                content: Vec::new(),
            });
    }
    let mut records = state.context.records.to_vec();
    let mut identities = records
        .iter()
        .map(|record| record.id.clone())
        .collect::<BTreeSet<_>>();
    let revision = state
        .context
        .revision
        .checked_add(1)
        .ok_or(ThreadError::RevisionExhausted)?;
    for fact in next.values() {
        append_if_missing(&mut records, &mut identities, fact, revision);
    }
    let facts = next.into_values().collect::<Vec<_>>();
    if records.as_slice() != state.context.records.as_ref() {
        state.context = ContextSnapshot {
            revision,
            records: records.into(),
        };
    }
    state.runtime_facts = facts.into();
    Ok(state.context.clone())
}

pub(super) fn restore_facts(
    records: &mut Vec<ContextRecord>,
    facts: &[RuntimeFact],
    revision: u64,
) {
    let mut identities = records
        .iter()
        .map(|record| record.id.clone())
        .collect::<BTreeSet<_>>();
    for fact in facts {
        append_if_missing(records, &mut identities, fact, revision);
    }
}

fn append_if_missing(
    records: &mut Vec<ContextRecord>,
    identities: &mut BTreeSet<String>,
    fact: &RuntimeFact,
    revision: u64,
) {
    let content = if fact.content.is_empty() {
        vec![ContextContent::Text {
            text: Arc::from(
                "This source has no active facts. Previously supplied facts from this source are no longer current.",
            ),
        }]
    } else {
        fact.content.clone()
    };
    let source = ContextSource::Runtime {
        source_id: fact.source_id.clone(),
    };
    if records
        .iter()
        .rev()
        .find(|record| record.source == source)
        .is_some_and(|record| record.content == content)
    {
        return;
    }
    let mut suffix = 0_u64;
    let id = loop {
        let id = format!("runtime:{revision}:{}:{suffix}", fact.source_id);
        if identities.insert(id.clone()) {
            break id;
        }
        suffix += 1;
    };
    records.push(ContextRecord {
        tool_calls: Vec::new(),
        id,
        turn_id: None,
        source,
        content,
    });
}

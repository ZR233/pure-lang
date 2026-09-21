//! Per-session conversion of one legacy journal into the current layout.
//!
//! * current state → `<staging>/<storage-key>/state.toml`
//! * history → `<staging>/<storage-key>/history.sqlite`
//! * model/tool calls → `<staging>/calls.sqlite`
//! * attachments → `<staging>/<storage-key>/attachments.toml` plus content-addressed blobs
//!
//! Nothing here mutates the legacy databases. Conversion is deterministic and idempotent, so a
//! resumed run recreates the same staging bytes and the same manifest before publication.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use pl_core::model::ModelUsage;
use pl_core::thread::input::{InputChange, InputState};
use pl_core::thread::journal::{AttemptUpdate, legacy_migration};
use pl_core::thread::{
    AttemptOutcome, ThreadCheckpoint, ThreadEffectBatch, ThreadSnapshot, ToolDelivery, UsageSummary,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, Value};
use sha2::{Digest, Sha256};

use crate::studio::catalog::CatalogEntry;
use crate::studio::records::AttachmentRecord;
use crate::studio::storage::calls::{CallStatus, CallsStore, legacy_call_store_blobs_dir};
use crate::studio::storage::history::{
    EffectCommit, FactReceiptWrite, HistoryStore, InputIdentityWrite, MessageIdentityWrite,
};
use crate::studio::storage::state::{InputIdentityEntry, StateStore};
use crate::studio::store::attachment::write_attachment_catalog;

use super::report::{SessionMigrationRecord, SessionMigrationStatus};

/// 每会话目录摘要在 staging 中的固定文件名；发布阶段按稳定身份合并为 `catalog.toml`。
pub(super) const STAGED_CATALOG_ENTRY_FILE_NAME: &str = "catalog-entry.toml";

/// The immutable result of converting one legacy session; used as the durable verification record.
#[derive(Debug, Clone)]
pub(super) struct SessionConversion {
    pub(super) record: SessionMigrationRecord,
}

/// Locates the staged directory that holds one session's converted facts.
pub(super) fn staged_session_dir(staging: &Path, storage_key: &str) -> PathBuf {
    staging.join(storage_key)
}

/// Reads one legacy journal and appends its migration-only settlement with a reproducible timestamp.
///
/// `ThreadEffectBatch::between` stamps the synthesized settlement with wall-clock time, which would
/// make an otherwise deterministic conversion time-dependent: a resumed export or a phase-4
/// re-projection of the same legacy journal would produce different item/call timestamps. The
/// settlement carries no source time of its own, so it keeps the last legacy commit's time — it is
/// derived from that journal, so this is the closest source fact and it is stable across runs.
fn legacy_journal_with_settlement(
    mut effects: Vec<Arc<ThreadEffectBatch>>,
    thread_id: &str,
) -> Result<Vec<Arc<ThreadEffectBatch>>> {
    if effects.is_empty() {
        anyhow::bail!("legacy session {thread_id} has no Thread commits");
    }
    if let Some(mut settlement) = legacy_migration::recovery_commit(&effects)? {
        let recorded_at = effects
            .last()
            .map_or(settlement.committed_at, |commit| commit.committed_at);
        settlement.committed_at = recorded_at;
        // `recovery::settle` also stamps every permission it cancels with wall-clock time (through
        // `permissions::cancel_pending`), which would leak into the staged checkpoint, the durable
        // permission receipt and their digests. Those synthesized facts carry no source time, so they
        // keep the same source-derived time as the settlement itself.
        if !settlement.permissions.is_empty() {
            settlement.permissions = settlement
                .permissions
                .iter()
                .map(|record| {
                    let mut record = record.clone();
                    record.updated_at = recorded_at;
                    record
                })
                .collect::<Vec<_>>()
                .into();
        }
        effects.push(Arc::new(settlement));
    }
    Ok(effects)
}

/// Converts one legacy session into staged current-layout facts.
///
/// The caller supplies `product` so the projector can preserve saved ancestry; a session without a
/// directory row still exports its journal with a placeholder identity.
pub(super) fn export_session(
    legacy: &pl_core::persistence::SqliteSessionStore,
    calls: &CallsStore,
    product: &sea_orm::DatabaseConnection,
    staging: &Path,
    thread_id: &str,
    updated_at: i64,
) -> impl std::future::Future<Output = Result<SessionConversion>> + Send + 'static {
    // Own every input before the future exists: a spawned caller cannot prove `Send` for a future
    // that captures borrowed parameters (`Send is not general enough`), so the returned future must
    // capture only owned values and be `Send + 'static`.
    let legacy = legacy.clone();
    let calls = calls.clone();
    let product = product.clone();
    let staging = staging.to_path_buf();
    let thread_id = thread_id.to_owned();
    async move {
        let legacy = &legacy;
        let calls = &calls;
        let product = &product;
        let staging = staging.as_path();
        let thread_id = thread_id.as_str();
        let storage_key = crate::studio::paths::thread_storage_key(thread_id);
        let directory = staged_session_dir(staging, &storage_key);
        tokio::fs::create_dir_all(&directory)
            .await
            .with_context(|| format!("failed to create staging directory for {thread_id}"))?;

        let effects = legacy
            .read_legacy_thread_journal(thread_id)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let effects = legacy_journal_with_settlement(effects, thread_id)?;
        let journal_head = effects.last().map(|commit| commit.sequence).unwrap_or(0);
        let mut state = legacy_migration::replay(&effects)?;
        state.usage_summary = fold_usage(&effects)?;
        ensure!(
            state.usage_summary.applied_sequence == journal_head,
            "replayed usage summary did not fold through the legacy journal head for {thread_id}"
        );

        let thread = thread_identity(product, thread_id, updated_at).await?;
        let mut thread = thread;
        if let Some(mode) = crate::studio::thread_projection::saved_mode(&state)? {
            thread.mode = mode;
        }
        thread.status = crate::studio::thread_projection::status(&state);
        let mut items =
            crate::studio::thread_projection::project_history_items(&thread, &state, &effects)?;
        let turns = crate::studio::runtime::timeline::timeline_turns(thread_id, &state, &items);

        let history = HistoryStore::open(&directory.join("history.sqlite"), thread_id).await?;
        // 持久 identity/receipt/消息身份索引按 effect 顺序回填：每条输入身份、每条已受理消息身份与每条
        // 终态 interaction/permission receipt 都落在产生它的 effect 写序列事务里，与实时 writer 的
        // `commit_effect` 边界一致。重复写入幂等，内容摘要冲突让整个导出失败，因此索引永远不会脱离它
        // 描述的事实先落库。
        for effect in &effects {
            history
                .commit_effect(
                    effect.sequence,
                    EffectCommit {
                        items: &[],
                        rolled_back_turns: &BTreeSet::new(),
                        identities: &effect_input_identities(&state, effect),
                        messages: &effect_message_identities(effect),
                        receipts: &effect_fact_receipts(effect)?,
                    },
                )
                .await?;
        }
        // staged 条目必须在与实时 writer 同一张 `history_ordinals` 表里留下同序号的预留：durable
        // writer 与该表共同决定 ordinal，缺行会让普通启动在首次预览一条已物化条目时重新编号，与
        // durable 条目错位。旧 journal 的准入 ordinal 会为"预留但未投影"的位置（例如未物化的投递/
        // 收件箱槽）留下空档，而 staged 库按插入顺序分配连续 ordinal，因此这里按投影顺序为每个身份
        // 预留并复用同一 ordinal。缺失一条预留会让导出失败并保留源字节，写入后的校验器再逐行核对
        // 预留与条目一致。
        // 用显式循环收集身份，而不是 `.map(|item| item.id.clone())` 闭包：当迭代器被传进
        // `reserve_ordinals(impl IntoIterator<Item = String>)` 这类泛型 async 参数、并跨 await
        // 持有在整个导出 future 里时，闭包只会被推断成某个具体生命周期的 `fn(&ThreadItem)`，
        // 无法满足 future 类型要求的 `for<'a>` 泛化（`FnOnce is not general enough`）。显式收集成
        // owned `Vec<String>` 后迭代器类型具体且无闭包，身份、顺序与数量都不变。
        let mut item_ids = Vec::with_capacity(items.len());
        for item in &items {
            item_ids.push(item.id.clone());
        }
        let reserved = history.reserve_ordinals(item_ids).await?;
        for item in &mut items {
            item.ordinal = *reserved.get(&item.id).ok_or_else(|| {
                anyhow::anyhow!(
                    "staged history reserved no ordinal for item {} of {thread_id}; existing data \
                 preserved",
                    item.id
                )
            })?;
        }
        // timeline 一次写入整段 journal 的投影：迁移只有这一条有界的一次性投影路径，不需要按 effect
        // 逐步物化条目；身份索引已在上面的逐 effect 事务中落库。
        history
            .commit(state.commit_sequence, &items, &turns)
            .await?;
        let history_watermark = history.watermark().await?;
        ensure!(
            history_watermark == state.commit_sequence,
            "staged history watermark is behind the replayed state for {thread_id}"
        );

        for effect in &effects {
            calls.commit(effect).await?;
        }

        let (attachment_count, attachment_hashes) =
            export_attachments(legacy, &directory, thread_id).await?;

        // checkpoint 最后发布：它引用的 history fence、模型/工具调用事实与内容寻址附件 blob 都必须先
        // durable，`state.toml` 的原子替换才发生在"全部引用都可读"之后（design/15 §128-131 的屏障顺序）。
        let state_store = StateStore::new(directory.clone(), thread_id);
        let checkpoint =
            ThreadCheckpoint::capture(thread_id.to_owned(), state.commit_sequence, state);
        state_store.publish(&checkpoint).await?;
        let persisted = state_store
            .load()
            .await?
            .context("staged checkpoint was not published")?;
        ensure!(
            persisted.history_fence == persisted.state_revision,
            "staged checkpoint fence and revision disagree for {thread_id}"
        );
        ensure!(
            persisted.state.usage_summary.applied_sequence == persisted.state_revision,
            "staged checkpoint usage summary did not fold through its revision for {thread_id}"
        );

        // Directory summary is staged next to the session facts so publication can rebuild
        // `catalog.toml` from stable per-session identities instead of re-reading any session.
        let catalog = CatalogEntry::from_thread(&thread);
        let catalog_bytes = toml::to_string_pretty(&catalog)?.into_bytes();
        let catalog_path = directory.join(STAGED_CATALOG_ENTRY_FILE_NAME);
        tokio::task::spawn_blocking(move || {
            pl_tool::workspace::write_file_atomically(&catalog_path, &catalog_bytes)
        })
        .await??;

        let record = SessionMigrationRecord {
            thread_id: thread_id.to_owned(),
            storage_key,
            status: SessionMigrationStatus::Staged,
            journal_head,
            history_watermark,
            checkpoint_saved_at: persisted.saved_at,
            item_count: items.len() as u64,
            turn_count: turns.len() as u64,
            attachment_count,
            attachment_hashes,
        };
        let manifest = serde_json::to_vec_pretty(&record)?;
        let manifest_path = directory.join("migration.json");
        tokio::task::spawn_blocking(move || {
            pl_tool::workspace::write_file_atomically(&manifest_path, &manifest)
        })
        .await??;
        Ok(SessionConversion { record })
    }
}

/// 一个 effect 让哪些输入进入终态，并给出它们的最小持久身份。
///
/// 与实时 writer 的 `terminal_input_identities` 同一语义：只有本 effect 自己的输入变化让某个输入
/// 从 pending 变为终态时才写入，因此每条 identity 都随产生它的 effect 写序列落库，而不会被提前写
/// 进更早的事务。终态在重放中不可再变，所以身份内容直接取最终事实中的同一条记录。
fn effect_input_identities(
    state: &ThreadSnapshot,
    effect: &ThreadEffectBatch,
) -> Vec<InputIdentityWrite> {
    let mut identities = Vec::new();
    let mut seen = BTreeSet::new();
    for change in effect.inputs.iter() {
        let (id, reached_terminal) = match change {
            InputChange::Accepted(record) => (
                record.input.id.as_str(),
                record.state != InputState::Pending,
            ),
            InputChange::Transition { id, state, .. } => {
                (id.as_str(), *state != InputState::Pending)
            }
        };
        if !reached_terminal || !seen.insert(id) {
            continue;
        }
        let Some(record) = state.inputs.iter().find(|record| record.input.id == id) else {
            continue;
        };
        identities.push(InputIdentityWrite {
            entry: InputIdentityEntry::new(
                pl_core::thread::input::input_identity(record),
                record.accepted_sequence,
            ),
            request_digest: crate::studio::thread_projection::saved_prompt_request_digest(
                &record.input.payload,
            ),
        });
    }
    identities
}

/// 一个 effect 提交的终态 interaction/permission receipt，身份与载荷编码与实时 writer 一致。
fn effect_fact_receipts(effect: &ThreadEffectBatch) -> Result<Vec<FactReceiptWrite>> {
    let mut receipts = Vec::new();
    for record in effect.interactions.iter() {
        receipts.push(FactReceiptWrite {
            item_id: crate::studio::thread_projection::order::receipt_id(
                "interaction",
                &record.request.id,
            ),
            revision: record.revision,
            kind: "interaction",
            payload: serde_json::to_string(record)?,
        });
    }
    for record in effect.permissions.iter() {
        receipts.push(FactReceiptWrite {
            item_id: crate::studio::thread_projection::order::receipt_id("permission", &record.id),
            revision: record.revision,
            kind: "permission",
            payload: serde_json::to_string(record)?,
        });
    }
    Ok(receipts)
}

/// 一个 effect 受理的消息的持久身份，与实时 writer 的 `admitted_message_identities` 同构造。
///
/// `digest` 是 core 对原始消息身份与正文（含冻结 context）计算的内容摘要，旧 journal 保留了原始
/// 正文，因此迁移场景可以精确回填。迁移**绝不**写入空摘要：正文无法证明时整段导出失败，而不是让
/// 运行期把已受理消息当成新消息重投。
fn effect_message_identities(effect: &ThreadEffectBatch) -> Vec<MessageIdentityWrite> {
    effect
        .inbox
        .iter()
        .map(|record| MessageIdentityWrite {
            message_id: record.message.id.clone(),
            item_id: crate::studio::thread_projection::order::message_id(&record.message.id),
            sequence: record.sequence,
            digest: Some(record.message.digest()),
        })
        .collect()
}

/// 旧 journal 重放后每条已受理消息的持久身份期望（消息身份、受理序号与正文摘要）。
fn expected_message_identities(effects: &[Arc<ThreadEffectBatch>]) -> Vec<(String, u64, String)> {
    let mut expected = Vec::new();
    let mut seen = BTreeSet::new();
    for effect in effects {
        for record in effect.inbox.iter() {
            if seen.insert(record.message.id.clone()) {
                expected.push((
                    record.message.id.clone(),
                    record.sequence,
                    record.message.digest(),
                ));
            }
        }
    }
    expected
}

/// 旧 journal 重放后每个终态 interaction/permission 身份上最新的 receipt。
///
/// host 读取身份索引时只取该身份的最新 revision（`latest_fact_receipt`），因此校验也以最新一条
/// 为期望；同身份多 revision 的旧 receipt 仍然允许存在，绝不因为出现新 revision 就判定冲突。
fn expected_fact_receipts(
    effects: &[Arc<ThreadEffectBatch>],
) -> Result<BTreeMap<String, (u64, String, String)>> {
    let mut newest = BTreeMap::new();
    for effect in effects {
        for receipt in effect_fact_receipts(effect)? {
            let replace = newest
                .get(&receipt.item_id)
                .is_none_or(|(revision, _, _)| *revision <= receipt.revision);
            if replace {
                newest.insert(
                    receipt.item_id,
                    (receipt.revision, receipt.kind.to_owned(), receipt.payload),
                );
            }
        }
    }
    Ok(newest)
}

/// Re-reads staged facts and re-derives every check recorded in the manifest.
pub(super) fn verify_session(
    legacy: &pl_core::persistence::SqliteSessionStore,
    product: &DatabaseConnection,
    staging: &Path,
    calls_destination: &Path,
    merged_retired_calls: bool,
    record: &SessionMigrationRecord,
) -> impl std::future::Future<Output = Result<()>> + Send + 'static {
    let legacy = legacy.clone();
    let product = product.clone();
    let staging = staging.to_path_buf();
    let calls_destination = calls_destination.to_path_buf();
    let record = record.clone();
    async move {
        let legacy = &legacy;
        let product = &product;
        let staging = staging.as_path();
        let calls_destination = calls_destination.as_path();
        let record = &record;
        let directory = staged_session_dir(staging, &record.storage_key);
        let state_store = StateStore::new(directory.clone(), &record.thread_id);
        let checkpoint = state_store
            .load()
            .await?
            .with_context(|| format!("staged checkpoint missing for {}", record.thread_id))?;
        ensure!(
            checkpoint.history_fence == checkpoint.state_revision
                && checkpoint.state_revision == record.journal_head,
            "staged checkpoint does not match the legacy journal head for {}",
            record.thread_id
        );
        let effects = legacy
            .read_legacy_thread_journal(&record.thread_id)
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let effects = legacy_journal_with_settlement(effects, &record.thread_id)?;
        let expected_usage = fold_usage(&effects)?;
        ensure!(
            expected_usage.applied_sequence == record.journal_head,
            "replayed usage summary does not cover the legacy journal head for {}",
            record.thread_id
        );
        ensure!(
            checkpoint.state.usage_summary == expected_usage,
            "staged checkpoint usage summary disagrees with the legacy journal for {}",
            record.thread_id
        );
        let mut expected_state = legacy_migration::replay(&effects)?;
        expected_state.usage_summary = expected_usage;
        // staged 目标必须等于旧 journal 的完整投影，而不只是条目数量相同：这里重跑与导出完全一致的
        // 身份/ordinal/revision/正文投影，稍后与 `history.sqlite` 逐行比对。
        let updated_at = super::thread_updated_at(product, &record.thread_id).await?;
        let mut expected_thread = thread_identity(product, &record.thread_id, updated_at).await?;
        if let Some(mode) = crate::studio::thread_projection::saved_mode(&expected_state)? {
            expected_thread.mode = mode;
        }
        expected_thread.status = crate::studio::thread_projection::status(&expected_state);
        let expected_items = crate::studio::thread_projection::project_history_items(
            &expected_thread,
            &expected_state,
            &effects,
        )?;
        let expected_turns = crate::studio::runtime::timeline::timeline_turns(
            &record.thread_id,
            &expected_state,
            &expected_items,
        );
        // checkpoint 引用的 Turn 必须在同一份投影里：只有 `state.toml` 的引用与其 durable 历史一致，
        // 发布后的普通启动才能按 fence 读到它引用的 Turn。
        let projected_turns = expected_turns
            .iter()
            .map(|turn| turn.turn.id.as_str())
            .collect::<BTreeSet<_>>();
        for turn in expected_state.turns.iter() {
            ensure!(
                projected_turns.contains(turn.turn_id.as_str()),
                "staged checkpoint Turn {} has no projected history Turn for {}",
                turn.turn_id,
                record.thread_id
            );
        }
        let expected_messages = expected_message_identities(&effects);
        // 持久 identity/receipt 索引必须能从旧 journal 重放出来，并且真的落进了 staged history：
        // 普通启动用它们回答重复提交，缺失或与源事实冲突都说明索引回填不完整。
        let expected_identities = effects
            .iter()
            .flat_map(|effect| effect_input_identities(&expected_state, effect))
            .collect::<Vec<_>>();
        let expected_receipts = expected_fact_receipts(&effects)?;
        let mut expected_checkpoint = ThreadCheckpoint::capture(
            record.thread_id.clone(),
            expected_state.commit_sequence,
            expected_state,
        );
        expected_checkpoint.saved_at = checkpoint.saved_at;
        ensure!(
            toml::to_string(&checkpoint)? == toml::to_string(&expected_checkpoint)?,
            "staged checkpoint contents disagree with the replayed legacy journal for {}",
            record.thread_id
        );

        let history_path = directory.join("history.sqlite");
        let history = HistoryStore::open(&history_path, &record.thread_id).await?;
        let watermark = history.watermark().await?;
        ensure!(
            watermark == record.history_watermark
                && watermark == record.journal_head
                && watermark == checkpoint.state_revision,
            "staged history does not cover the checkpoint fence for {}",
            record.thread_id
        );
        // 只比对数量与水位无法证明 staged 目标没有丢条目、错序或换了正文。这里逐行核对身份、ordinal、
        // revision、时间戳与完整 payload（含 raw 未知正文），再核对 Turn 行与 ordinal 引用表；任何缺失、
        // 额外行或内容不符都 fail closed。
        verify_staged_history(
            &history_path,
            &record.thread_id,
            &expected_items,
            &expected_turns,
        )
        .await?;
        // 每条已受理消息都必须有可证明正文的持久身份：序号是原始受理回执，摘要是正文证明。缺失、无摘要
        // 或与源事实不符都说明回填不完整，必须 fail closed 而不是让运行期把已受理消息重投。
        for (message_id, sequence, digest) in &expected_messages {
            let stored = history
                .message_identity(message_id)
                .await?
                .with_context(|| {
                    format!(
                        "staged history has no durable message identity for {message_id} of {}",
                        record.thread_id
                    )
                })?;
            ensure!(
                stored.sequence == *sequence && stored.digest.as_deref() == Some(digest.as_str()),
                "staged message identity {message_id} of {} disagrees with the legacy journal",
                record.thread_id
            );
        }
        for identity in &expected_identities {
            let stored = history
                .input_identity(&identity.entry.id)
                .await?
                .with_context(|| {
                    format!(
                        "staged history has no durable input identity for {} of {}",
                        identity.entry.id, record.thread_id
                    )
                })?;
            ensure!(
                stored.entry.digest == identity.entry.digest
                    && stored.entry.delivery == identity.entry.delivery
                    && stored.entry.ordinal == identity.entry.ordinal
                    && stored.entry.revision == identity.entry.revision
                    && stored.entry.state == identity.entry.state
                    && stored.entry.accepted_sequence == identity.entry.accepted_sequence
                    && stored.request_digest == identity.request_digest,
                "staged input identity {} disagrees with the replayed legacy journal for {}",
                identity.entry.id,
                record.thread_id
            );
        }
        for (item_id, (revision, kind, payload)) in &expected_receipts {
            let stored = history
                .latest_fact_receipt(item_id)
                .await?
                .with_context(|| {
                    format!(
                        "staged history has no durable fact receipt for {item_id} of {}",
                        record.thread_id
                    )
                })?;
            ensure!(
                stored.kind == *kind && stored.payload == *payload,
                "staged fact receipt {item_id} (revision {revision}) disagrees with the replayed legacy \
             journal for {}",
                record.thread_id
            );
        }
        // 调用事实（模型 attempt、工具受理/投递、水位与正文）同样必须能从旧 journal 逐项核对，
        // 只确认 staged `calls.sqlite` 存在不足以证明它没有丢事实。
        verify_staged_calls(
            calls_destination,
            &effects,
            &record.thread_id,
            record.journal_head,
            merged_retired_calls,
        )
        .await?;

        let manifest_path = directory.join("migration.json");
        let bytes = tokio::fs::read(&manifest_path)
            .await
            .with_context(|| format!("staged manifest missing for {}", record.thread_id))?;
        let manifest: SessionMigrationRecord = serde_json::from_slice(&bytes)?;
        ensure!(
            manifest.thread_id == record.thread_id
                && manifest.storage_key == record.storage_key
                && manifest.journal_head == record.journal_head
                && manifest.history_watermark == record.history_watermark
                && manifest.checkpoint_saved_at == record.checkpoint_saved_at
                && manifest.item_count == record.item_count
                && manifest.turn_count == record.turn_count
                && manifest.attachment_count == record.attachment_count
                && manifest.attachment_hashes == record.attachment_hashes,
            "staged manifest disagrees with the durable migration record for {}",
            record.thread_id
        );

        // The regenerated directory summary must describe the same session and come from the same
        // checkpoint the runtime will publish; a stale or foreign catalog entry fails closed.
        let catalog = load_staged_catalog(&directory, &record.thread_id)?;
        // Same identity invariant the directory-row validator (`legacy_thread_entries`) enforces:
        // the entry owns its canonical agent path, names a Project, keeps self-consistent timestamps,
        // and a root Thread is its own root. A child Thread belongs to its parent, so its
        // `root_thread_id` is the ancestor root, not itself; requiring `root_thread_id == self`
        // unconditionally would wrongly reject every legal child session.
        ensure!(
            catalog.id == record.thread_id
                && catalog.agent_path == record.thread_id
                && !catalog.project_id.is_empty()
                && catalog.created_at <= catalog.updated_at
                && (catalog.parent_thread_id.is_some()
                    || catalog.root_thread_id == record.thread_id),
            "staged catalog entry identity is inconsistent for {}",
            record.thread_id
        );
        ensure!(
            catalog.workspace_mode != pl_protocol::ThreadWorkspaceMode::Worktree
                || !catalog.workspace_path.is_empty(),
            "staged worktree catalog entry has no workspace address for {}",
            record.thread_id
        );

        let legacy_attachments = legacy
            .resources(&record.thread_id, "studio.attachment")
            .iter()
            .map(AttachmentRecord::from_session_entry)
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            legacy_attachments.len() as u64 == record.attachment_count
                && legacy_attachments
                    .iter()
                    .map(|attachment| attachment.content_sha256.clone())
                    .collect::<Vec<_>>()
                    == record.attachment_hashes,
            "staged attachment catalog disagrees with the legacy repository for {}",
            record.thread_id
        );
        let catalog_records = load_attachment_catalog(&directory, &record.thread_id)
            .await?
            .with_context(|| {
                format!("staged attachment catalog missing for {}", record.thread_id)
            })?;
        ensure!(
            catalog_records.len() == legacy_attachments.len()
                && catalog_records
                    .iter()
                    .zip(&legacy_attachments)
                    .all(|(staged, legacy)| staged.id == legacy.id
                        && staged.content_sha256 == legacy.content_sha256
                        && staged.byte_size == legacy.byte_size),
            "staged attachment catalog identity mismatch for {}",
            record.thread_id
        );
        let blobs_root = directory.join("blobs");
        let catalog_ids = catalog_records
            .iter()
            .map(|attachment| attachment.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for attachment_id in
            crate::studio::thread_projection::referenced_attachment_ids(&checkpoint.state)
        {
            ensure!(
                catalog_ids.contains(attachment_id.as_str()),
                "staged checkpoint references attachment {} without a migrated catalog record",
                attachment_id
            );
        }
        for attachment in &catalog_records {
            let recorded = PathBuf::from(&attachment.storage_path);
            let resolved = if recorded.is_absolute() {
                recorded
            } else {
                directory.join(recorded)
            };
            ensure!(
                resolved.starts_with(&blobs_root),
                "staged attachment {} is outside the per-session blob root",
                attachment.id
            );
            verify_blob(&resolved, attachment).await?;
        }
        Ok(())
    }
}

/// 旧 journal 要求 staged 调用库里出现的模型 attempt 及其最新状态。
struct ExpectedAttempt<'a> {
    attempt: &'a AttemptUpdate,
    revision: u64,
    terminal: bool,
}

/// 旧 journal 要求 staged 调用库里出现的工具调用及其受理/投递/任务事实。
struct ExpectedToolCall<'a> {
    turn_id: Option<String>,
    tool_id: Option<String>,
    delivery: Option<&'a ToolDelivery>,
    revision: u64,
    cancel_requested: Option<i64>,
}

/// Models the writer's monotonic attempt row: a later revision applies, and a non-terminal update
/// never clears an already terminal call.
fn expected_attempts(effects: &[Arc<ThreadEffectBatch>]) -> BTreeMap<String, ExpectedAttempt<'_>> {
    let mut expected: BTreeMap<String, ExpectedAttempt<'_>> = BTreeMap::new();
    for effect in effects {
        let Some(attempt) = effect.attempt.as_ref() else {
            continue;
        };
        let terminal = attempt_status_is_terminal(&attempt.outcome);
        let entry = expected
            .entry(attempt.attempt_id.clone())
            .or_insert(ExpectedAttempt {
                attempt,
                revision: 0,
                terminal: false,
            });
        if effect.sequence >= entry.revision && (terminal || !entry.terminal) {
            entry.attempt = attempt;
            entry.revision = effect.sequence;
            entry.terminal = terminal;
        }
    }
    expected
}

/// Models the writer's tool-call row: admission supplies identity, delivery terminalizes it and a
/// task record only folds into an identity the journal already admitted.
fn expected_tool_calls(
    effects: &[Arc<ThreadEffectBatch>],
) -> BTreeMap<String, ExpectedToolCall<'_>> {
    let mut expected: BTreeMap<String, ExpectedToolCall<'_>> = BTreeMap::new();
    for effect in effects {
        if let Some(attempt) = effect.attempt.as_ref()
            && let AttemptOutcome::Committed(output) = &attempt.outcome
        {
            for call in output.tool_calls.iter() {
                let entry = expected
                    .entry(call.call_id.clone())
                    .or_insert(ExpectedToolCall {
                        turn_id: None,
                        tool_id: None,
                        delivery: None,
                        revision: 0,
                        cancel_requested: None,
                    });
                // Admission is insert-only in the writer: a repeated identity keeps the first
                // admitted turn/tool even though a later revision may still raise the row.
                if entry.turn_id.is_none() {
                    entry.turn_id = Some(attempt.turn_id.clone());
                    entry.tool_id = Some(call.tool_id.clone());
                }
                entry.revision = entry.revision.max(effect.sequence);
            }
        }
        for delivery in effect.deliveries.iter() {
            let entry = expected
                .entry(delivery.call_id.clone())
                .or_insert(ExpectedToolCall {
                    turn_id: None,
                    tool_id: None,
                    delivery: None,
                    revision: 0,
                    cancel_requested: None,
                });
            entry.delivery = entry.delivery.or(Some(delivery));
            entry.revision = entry.revision.max(effect.sequence);
        }
    }
    for effect in effects {
        for record in effect.tasks.iter() {
            let Some(entry) = expected.get_mut(&record.call_id) else {
                // The writer's task-lifecycle update is a no-op for an identity it never admitted,
                // so such a record is not an expectation about the staged row.
                continue;
            };
            entry.cancel_requested = Some(
                if record.cancel_requested
                    || record.status == pl_core::thread::task::TaskStatus::Cancelled
                {
                    1
                } else {
                    0
                },
            );
            entry.revision = entry.revision.max(effect.sequence);
        }
    }
    expected
}

/// 调用库判定模型 attempt 是否终态：只有仍在运行不是终态（被打断也是终态）。
fn attempt_status_is_terminal(outcome: &AttemptOutcome) -> bool {
    !matches!(outcome, AttemptOutcome::Running)
}

/// 与调用库写 attempt 行相同的用量来源；未知计数保持 `None`，不塌缩成 0。
fn attempt_usage(outcome: &AttemptOutcome) -> Option<&ModelUsage> {
    match outcome {
        AttemptOutcome::Running | AttemptOutcome::Interrupted => None,
        AttemptOutcome::Committed(output) | AttemptOutcome::Rejected { output, .. } => {
            Some(&output.usage)
        }
        AttemptOutcome::Failed(error) => Some(&error.usage),
        AttemptOutcome::Cancelled { result } => Some(match result {
            Ok(output) => &output.usage,
            Err(error) => &error.usage,
        }),
    }
}

/// 与调用库写库时相同的列投影：`total` 只在 input/output 都可测量时给出。
#[allow(clippy::type_complexity)]
fn usage_columns(
    usage: Option<&ModelUsage>,
) -> (
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
) {
    let Some(usage) = usage else {
        return (None, None, None, None, None, None);
    };
    let total = usage
        .input_tokens
        .zip(usage.output_tokens)
        .and_then(|(input, output)| input.checked_add(output));
    (
        usage
            .input_tokens
            .and_then(|value| i64::try_from(value).ok()),
        usage
            .output_tokens
            .and_then(|value| i64::try_from(value).ok()),
        usage
            .cache_read_tokens
            .and_then(|value| i64::try_from(value).ok()),
        usage
            .cache_write_tokens
            .and_then(|value| i64::try_from(value).ok()),
        usage
            .reasoning_tokens
            .and_then(|value| i64::try_from(value).ok()),
        total.and_then(|value| i64::try_from(value).ok()),
    )
}

/// 校验 staged 调用库真的持有旧 journal 重放出的模型/工具事实。
///
/// 没有退役调用库快照的安装无法从别处再推导调用事实，只确认 staged `calls.sqlite` 存在并不足以
/// 证明身份、正文或水位没有丢失。这里按身份、关联、终态、统计与内容寻址正文逐项核对，并把线程写
/// 水位绑到 journal head；任一缺失或冲突都 fail closed，保留全部源字节。export 阶段已在发布前停机
/// 调用库 writer，所以这里的只读连接看到的就是发布前的最终内容。
async fn verify_staged_calls(
    destination: &Path,
    effects: &[Arc<ThreadEffectBatch>],
    thread_id: &str,
    journal_head: u64,
    merged_retired_calls: bool,
) -> Result<()> {
    let db = super::connect(destination, super::DatabaseAccess::ReadOnly).await?;
    let blobs = legacy_call_store_blobs_dir(destination);
    let result = verify_staged_calls_with(
        &db,
        &blobs,
        effects,
        thread_id,
        journal_head,
        merged_retired_calls,
    )
    .await;
    super::finish_connection(db, result).await
}

async fn verify_staged_calls_with(
    db: &DatabaseConnection,
    blobs: &Path,
    effects: &[Arc<ThreadEffectBatch>],
    thread_id: &str,
    journal_head: u64,
    merged_retired_calls: bool,
) -> Result<()> {
    let head = i64::try_from(journal_head).context("journal head exceeds the SQLite range")?;
    let watermark = db
        .query_one_raw(query_statement(
            "SELECT admitted_write_seq,durable_write_seq FROM call_watermarks WHERE thread_id=?",
            vec![thread_id.into()],
        ))
        .await?
        .with_context(|| {
            format!("staged call store has no watermark for {thread_id}; existing data preserved")
        })?;
    let admitted: i64 = watermark.try_get("", "admitted_write_seq")?;
    let durable: i64 = watermark.try_get("", "durable_write_seq")?;
    ensure!(
        durable >= head && admitted >= durable,
        "staged call store watermark for {thread_id} does not cover the legacy journal head \
         (durable {durable}, admitted {admitted}, head {head}); existing data preserved"
    );

    for (call_id, expected) in &expected_attempts(effects) {
        verify_expected_attempt(
            db,
            blobs,
            thread_id,
            call_id,
            expected,
            merged_retired_calls,
        )
        .await?;
    }
    for (call_id, expected) in &expected_tool_calls(effects) {
        verify_expected_tool_call(db, blobs, thread_id, call_id, expected).await?;
    }
    // 关联/归属：这一 Thread 引用的每个正文（含计费观察正文）都必须在 `call_bodies` 登记，且引用本身
    // 是内容寻址。登记缺失说明发布出去的行会指向运行期无法解析的正文。
    let unregistered = db
        .query_one_raw(query_statement(
            "SELECT COUNT(*) AS unregistered FROM (\
                SELECT body_ref AS reference FROM model_calls \
                 WHERE thread_id=? AND body_ref IS NOT NULL \
                UNION SELECT billing_ref FROM model_calls \
                 WHERE thread_id=? AND billing_ref IS NOT NULL \
                UNION SELECT body_ref FROM tool_calls \
                 WHERE thread_id=? AND body_ref IS NOT NULL\
             ) refs LEFT JOIN call_bodies bodies ON bodies.body_ref=refs.reference \
             WHERE bodies.body_ref IS NULL",
            vec![thread_id.into(), thread_id.into(), thread_id.into()],
        ))
        .await?
        .context("staged call store body registration query returned no row")?
        .try_get::<i64>("", "unregistered")?;
    ensure!(
        unregistered == 0,
        "staged call store leaves {unregistered} call body reference(s) of {thread_id} \
         unregistered; existing data preserved"
    );
    Ok(())
}

async fn verify_expected_attempt(
    db: &DatabaseConnection,
    blobs: &Path,
    thread_id: &str,
    call_id: &str,
    expected: &ExpectedAttempt<'_>,
    merged_retired_calls: bool,
) -> Result<()> {
    let row = db
        .query_one_raw(query_statement(
            "SELECT turn_id,attempt_id,revision,terminal,status,billing_ref,body_ref,input_tokens,\
             output_tokens,cache_read_tokens,cache_write_tokens,reasoning_tokens,total_tokens \
             FROM model_calls WHERE thread_id=? AND call_id=?",
            vec![thread_id.into(), call_id.into()],
        ))
        .await?
        .with_context(|| {
            format!(
                "staged call store lost model call {call_id} of {thread_id}; existing data preserved"
            )
        })?;
    let turn_id: String = row.try_get("", "turn_id")?;
    let attempt_id: String = row.try_get("", "attempt_id")?;
    let revision: i64 = row.try_get("", "revision")?;
    let terminal: i64 = row.try_get("", "terminal")?;
    ensure!(
        turn_id == expected.attempt.turn_id
            && attempt_id == call_id
            && u64::try_from(revision).is_ok_and(|value| value >= expected.revision)
            && (!expected.terminal || terminal != 0),
        "staged model call {call_id} of {thread_id} disagrees with the legacy journal identity, \
         revision regressed or its terminal fact was lost; existing data preserved"
    );
    let status: Option<String> = row.try_get("", "status")?;
    let status = CallStatus::parse(status.as_deref().unwrap_or_default()).with_context(|| {
        format!(
            "staged model call {call_id} of {thread_id} has an unrecognized persisted status; \
            existing data preserved"
        )
    })?;
    ensure!(
        status.is_terminal() == (terminal != 0) && (terminal != 0 || status == CallStatus::Running),
        "staged model call {call_id} of {thread_id} persisted status disagrees with its terminal \
         flag; existing data preserved"
    );
    // 计费观察或退役调用库合并都可能按同一调用身份补齐统计列；那部分由退役调用库审计负责对账。
    // 没有这些来源覆盖的安装必须逐列等于旧 journal 事实。
    let billing_ref: Option<String> = row.try_get("", "billing_ref")?;
    if !merged_retired_calls && billing_ref.as_deref().is_none_or(str::is_empty) {
        let expected_columns = usage_columns(attempt_usage(&expected.attempt.outcome));
        let actual_columns = (
            row.try_get::<Option<i64>>("", "input_tokens")?,
            row.try_get::<Option<i64>>("", "output_tokens")?,
            row.try_get::<Option<i64>>("", "cache_read_tokens")?,
            row.try_get::<Option<i64>>("", "cache_write_tokens")?,
            row.try_get::<Option<i64>>("", "reasoning_tokens")?,
            row.try_get::<Option<i64>>("", "total_tokens")?,
        );
        ensure!(
            actual_columns == expected_columns,
            "staged model call {call_id} of {thread_id} usage columns disagree with the legacy \
             journal; existing data preserved"
        );
    }
    let body_ref: Option<String> = row.try_get("", "body_ref")?;
    let body =
        read_staged_call_body(blobs, body_ref.as_deref(), &format!("model call {call_id}")).await?;
    let parsed: AttemptUpdate = serde_json::from_str(&body).with_context(|| {
        format!("staged model call {call_id} of {thread_id} body is not a model attempt")
    })?;
    ensure!(
        serde_json::to_value(&parsed)? == serde_json::to_value(expected.attempt)?,
        "staged model call {call_id} of {thread_id} body disagrees with the legacy journal; \
         existing data preserved"
    );
    Ok(())
}

async fn verify_expected_tool_call(
    db: &DatabaseConnection,
    blobs: &Path,
    thread_id: &str,
    call_id: &str,
    expected: &ExpectedToolCall<'_>,
) -> Result<()> {
    let row = db
        .query_one_raw(query_statement(
            "SELECT turn_id,tool_id,revision,terminal,status,body_ref,cancel_requested \
             FROM tool_calls WHERE thread_id=? AND call_id=?",
            vec![thread_id.into(), call_id.into()],
        ))
        .await?
        .with_context(|| {
            format!(
                "staged call store lost tool call {call_id} of {thread_id}; existing data preserved"
            )
        })?;
    let turn_id: String = row.try_get("", "turn_id")?;
    let tool_id: String = row.try_get("", "tool_id")?;
    let revision: i64 = row.try_get("", "revision")?;
    let terminal: i64 = row.try_get("", "terminal")?;
    let delivered = expected.delivery.is_some();
    ensure!(
        expected
            .turn_id
            .as_deref()
            .is_none_or(|value| value == turn_id)
            && expected
                .tool_id
                .as_deref()
                .is_none_or(|value| value == tool_id)
            && u64::try_from(revision).is_ok_and(|value| value >= expected.revision)
            && (!delivered || terminal != 0),
        "staged tool call {call_id} of {thread_id} disagrees with the legacy journal identity, \
         revision regressed or its delivered fact was lost; existing data preserved"
    );
    let status: Option<String> = row.try_get("", "status")?;
    let status = CallStatus::parse(status.as_deref().unwrap_or_default()).with_context(|| {
        format!(
            "staged tool call {call_id} of {thread_id} has an unrecognized persisted status; \
             existing data preserved"
        )
    })?;
    ensure!(
        status.is_terminal() == (terminal != 0) && (terminal != 0 || status == CallStatus::Running),
        "staged tool call {call_id} of {thread_id} persisted status disagrees with its terminal \
         flag; existing data preserved"
    );
    if let Some(cancel_requested) = expected.cancel_requested {
        let stored: i64 = row.try_get("", "cancel_requested")?;
        ensure!(
            stored == cancel_requested,
            "staged tool call {call_id} of {thread_id} lost its task cancel fact; existing data \
             preserved"
        );
    }
    let body_ref: Option<String> = row.try_get("", "body_ref")?;
    let body =
        read_staged_call_body(blobs, body_ref.as_deref(), &format!("tool call {call_id}")).await?;
    match expected.delivery {
        Some(delivery) => {
            let parsed: ToolDelivery = serde_json::from_str(&body).with_context(|| {
                format!("staged tool call {call_id} of {thread_id} body is not a tool delivery")
            })?;
            ensure!(
                serde_json::to_value(&parsed)? == serde_json::to_value(delivery)?,
                "staged tool delivery {call_id} of {thread_id} disagrees with the legacy journal; \
                 existing data preserved"
            );
        }
        None => {
            // 未投递的已受理调用只保留调用参数正文；它必须是内容寻址的合法 JSON。
            serde_json::from_str::<serde_json::Value>(&body).with_context(|| {
                format!(
                    "staged tool call {call_id} of {thread_id} admitted body is not JSON; existing \
                     data preserved"
                )
            })?;
        }
    }
    Ok(())
}

/// 读取 staged 调用库引用的内容寻址正文并校验摘要；缺失、非摘要引用或内容不符都 fail closed。
async fn read_staged_call_body(blobs: &Path, body_ref: Option<&str>, what: &str) -> Result<String> {
    let body_ref = body_ref.with_context(|| {
        format!("staged call fact {what} has no content-addressed body; existing data preserved")
    })?;
    let name = body_ref
        .strip_prefix("sha256:")
        .filter(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .with_context(|| {
            format!(
                "staged call fact {what} has a body reference that is not content-addressed; \
                 existing data preserved"
            )
        })?;
    let path = blobs.join(name);
    let bytes = tokio::fs::read(&path).await.with_context(|| {
        format!(
            "staged call body is missing for {what}: {}; existing data preserved",
            path.display()
        )
    })?;
    ensure!(
        pl_core::context::content_hash(&bytes) == body_ref,
        "staged call body does not match its content-addressed reference for {what}; existing data \
         preserved"
    );
    String::from_utf8(bytes).with_context(|| {
        format!("staged call body is not UTF-8 for {what}; existing data preserved")
    })
}

/// 参数化 SQLite 查询语句；表名与列名都是编译期常量，不做动态拼接。
fn query_statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

/// 逐行证明 staged `history.sqlite` 就是旧 journal 的完整投影。
///
/// 迁移写出的每一步都可由旧 journal 重放，所以这里可以在只读连接上重新核对：metadata 归属、条目
/// 集合与顺序（ordinal）、身份字段、revision、生命周期、时间戳与完整 payload（含 raw 未知正文），
/// Turn 行的首尾 ordinal/revision/payload，以及 ordinal 引用表。任何缺失、额外行、错序或正文不符都
/// fail closed，原始字节保持不变。
async fn verify_staged_history(
    history_path: &Path,
    thread_id: &str,
    expected_items: &[pl_protocol::ThreadItem],
    expected_turns: &[pl_protocol::TimelineTurn],
) -> Result<()> {
    let db = super::connect(history_path, super::DatabaseAccess::ReadOnly).await?;
    let result = verify_staged_history_with(&db, thread_id, expected_items, expected_turns).await;
    super::finish_connection(db, result).await
}

async fn verify_staged_history_with(
    db: &DatabaseConnection,
    thread_id: &str,
    expected_items: &[pl_protocol::ThreadItem],
    expected_turns: &[pl_protocol::TimelineTurn],
) -> Result<()> {
    let meta = db
        .query_one_raw(query_statement(
            "SELECT thread_id,database_id FROM history_meta WHERE id=1",
            vec![],
        ))
        .await?
        .context("staged history has no metadata; existing data preserved")?;
    let owner: String = meta.try_get("", "thread_id")?;
    let database_id: String = meta.try_get("", "database_id")?;
    ensure!(
        owner == thread_id && !database_id.is_empty(),
        "staged history belongs to another Thread or lost its database identity; existing data \
         preserved"
    );

    let rows = db
        .query_all_raw(query_statement(
            "SELECT ordinal,item_id,turn_id,kind,revision,lifecycle,created_at,updated_at,payload \
             FROM history_items ORDER BY ordinal",
            vec![],
        ))
        .await?;
    ensure!(
        rows.len() == expected_items.len(),
        "staged history holds {} timeline item(s) but the legacy journal projects {}; existing data \
         preserved",
        rows.len(),
        expected_items.len()
    );
    // 投影允许"预留了 ordinal 位置但没有条目"（例如根 Thread 的收件箱消息只预留位置），而 staged 库按
    // 插入顺序分配连续 ordinal。因此这里核对的是**同一集合、同一顺序**：按 ordinal 升序逐项与投影顺序
    // 对齐，staged ordinal 必须连续，条目内容（除 ordinal 本身）必须与投影逐字段相等。
    let mut staged_ordinals = BTreeMap::new();
    for (index, (row, expected)) in rows.iter().zip(expected_items.iter()).enumerate() {
        let item_id: String = row.try_get("", "item_id")?;
        let ordinal: i64 = row.try_get("", "ordinal")?;
        let turn_id: String = row.try_get("", "turn_id")?;
        let kind: String = row.try_get("", "kind")?;
        let revision: i64 = row.try_get("", "revision")?;
        let lifecycle: String = row.try_get("", "lifecycle")?;
        let created_at: i64 = row.try_get("", "created_at")?;
        let updated_at: i64 = row.try_get("", "updated_at")?;
        let payload: String = row.try_get("", "payload")?;
        let position = u64::try_from(index).context("staged history has too many items")? + 1;
        let staged_ordinal = u64::try_from(ordinal).ok();
        let duplicate = staged_ordinals.insert(item_id.clone(), position).is_some();
        ensure!(
            staged_ordinal == Some(position) && item_id == expected.id && !duplicate,
            "staged history item order {item_id} disagrees with the legacy journal projection for \
             {thread_id}; existing data preserved"
        );
        let parsed: pl_protocol::ThreadItem = serde_json::from_str(&payload)
            .with_context(|| format!("staged history item {item_id} is not a timeline item"))?;
        // The store allocates the staged ordinal itself and serializes it into the payload, so the
        // projection is compared with that ordinal substituted in; every other field must be equal.
        let mut expected = expected.clone();
        expected.ordinal = position;
        ensure!(
            u64::try_from(revision).ok() == Some(expected.revision)
                && turn_id == expected.turn_id
                && created_at == expected.created_at
                && updated_at == expected.updated_at
                && kind == item_kind_label(expected.kind())
                && lifecycle
                    == if expected.is_terminal() {
                        "terminal"
                    } else {
                        "open"
                    }
                && parsed == expected,
            "staged history item {item_id} disagrees with the legacy journal projection for \
             {thread_id}; existing data preserved"
        );
    }

    let turn_rows = db
        .query_all_raw(query_statement(
            "SELECT turn_id,first_ordinal,last_ordinal,revision,payload FROM history_turns \
             ORDER BY turn_id",
            vec![],
        ))
        .await?;
    ensure!(
        turn_rows.len() == expected_turns.len(),
        "staged history holds {} Turn(s) but the legacy journal projects {}; existing data preserved",
        turn_rows.len(),
        expected_turns.len()
    );
    for row in &turn_rows {
        let turn_id: String = row.try_get("", "turn_id")?;
        let first_ordinal: i64 = row.try_get("", "first_ordinal")?;
        let last_ordinal: i64 = row.try_get("", "last_ordinal")?;
        let revision: i64 = row.try_get("", "revision")?;
        let payload: String = row.try_get("", "payload")?;
        let expected = expected_turns
            .iter()
            .find(|turn| turn.turn.id == turn_id)
            .with_context(|| {
                format!(
                    "staged history holds a Turn the legacy journal does not project: {turn_id}; \
                     existing data preserved"
                )
            })?;
        let parsed: pl_protocol::TimelineTurn = serde_json::from_str(&payload)
            .with_context(|| format!("staged history Turn {turn_id} is not a timeline Turn"))?;
        let expected_first = expected_items
            .iter()
            .filter(|item| item.turn_id == turn_id)
            .filter_map(|item| staged_ordinals.get(item.id.as_str()).copied())
            .min();
        let expected_last = staged_ordinals.get(expected.last_item_id.as_str()).copied();
        ensure!(
            parsed == *expected
                && u64::try_from(revision).ok() == Some(expected.turn.revision)
                && u64::try_from(first_ordinal).ok() == expected_first
                && u64::try_from(last_ordinal).ok() == expected_last,
            "staged history Turn {turn_id} disagrees with the legacy journal projection for \
             {thread_id}; existing data preserved"
        );
    }

    let ordinal_rows = db
        .query_all_raw(query_statement(
            "SELECT item_id,ordinal FROM history_ordinals",
            vec![],
        ))
        .await?;
    ensure!(
        ordinal_rows.len() == expected_items.len(),
        "staged history holds {} ordinal reservation(s) for {} timeline item(s); existing data \
         preserved",
        ordinal_rows.len(),
        expected_items.len()
    );
    for row in &ordinal_rows {
        let item_id: String = row.try_get("", "item_id")?;
        let ordinal: i64 = row.try_get("", "ordinal")?;
        let staged = staged_ordinals.get(item_id.as_str()).with_context(|| {
            format!(
                "staged history reserves an ordinal for an item the legacy journal does not \
                 project: {item_id}; existing data preserved"
            )
        })?;
        ensure!(
            u64::try_from(ordinal).ok() == Some(*staged),
            "staged history ordinal reservation for {item_id} disagrees with the legacy journal \
             projection; existing data preserved"
        );
    }
    Ok(())
}

/// Staged `history_items.kind` 标签；与 history store 的 `kind_label` 同一映射，用于核对列与
/// payload 一致（payload 相同但列被改写同样是 corruption）。
fn item_kind_label(kind: pl_protocol::ThreadItemKind) -> &'static str {
    use pl_protocol::ThreadItemKind;
    match kind {
        ThreadItemKind::Raw => "raw",
        ThreadItemKind::Text => "text",
        ThreadItemKind::Thinking => "thinking",
        ThreadItemKind::Tool => "tool",
        ThreadItemKind::Agent => "agent",
        ThreadItemKind::Turn => "turn",
        ThreadItemKind::Inference => "inference",
        ThreadItemKind::Skill => "skill",
        ThreadItemKind::File => "file",
        ThreadItemKind::ContextCompaction => "contextCompaction",
    }
}

fn fold_usage(
    effects: &[std::sync::Arc<pl_core::thread::ThreadEffectBatch>],
) -> Result<UsageSummary> {
    let mut summary = UsageSummary::default();
    for effect in effects {
        crate::studio::thread_projection::fold_effect_accounting(&mut summary, effect)?;
    }
    Ok(summary)
}

/// Imports the pre-`attachments.toml` catalog for one Thread and copies its blobs.
async fn export_attachments(
    legacy: &pl_core::persistence::SqliteSessionStore,
    directory: &Path,
    thread_id: &str,
) -> Result<(u64, Vec<String>)> {
    let legacy_records = legacy
        .resources(thread_id, "studio.attachment")
        .iter()
        .map(AttachmentRecord::from_session_entry)
        .collect::<Result<Vec<_>>>()?;
    if legacy_records.is_empty() {
        return Ok((0, Vec::new()));
    }
    let mut migrated = Vec::with_capacity(legacy_records.len());
    for mut record in legacy_records {
        let source = PathBuf::from(&record.storage_path);
        ensure!(
            !record.content_sha256.is_empty() && record.content_sha256.len() >= 2,
            "legacy attachment {} has no content hash",
            record.id
        );
        let destination = content_addressed_path(directory, &record.content_sha256);
        if let Some(parent) = destination.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let bytes = tokio::fs::read(&source)
            .await
            .with_context(|| format!("legacy attachment blob is missing: {}", source.display()))?;
        ensure!(
            hex_digest(&bytes) == record.content_sha256,
            "legacy attachment {} content hash mismatch",
            record.id
        );
        if !tokio::fs::try_exists(&destination).await? {
            let path = destination.clone();
            tokio::task::spawn_blocking(move || {
                pl_tool::workspace::write_file_atomically(&path, &bytes)
            })
            .await??;
        }
        record.storage_path = destination
            .strip_prefix(directory)
            .context("attachment destination escaped the staged session directory")?
            .to_string_lossy()
            .into_owned();
        migrated.push(record);
    }
    write_attachment_catalog(&directory.join("attachments.toml"), thread_id, &migrated).await?;
    let hashes = migrated
        .iter()
        .map(|record| record.content_sha256.clone())
        .collect();
    Ok((migrated.len() as u64, hashes))
}

fn content_addressed_path(directory: &Path, content_sha256: &str) -> PathBuf {
    directory
        .join("blobs")
        .join(&content_sha256[..2])
        .join(content_sha256)
}

/// Loads one staged per-session directory summary (`catalog-entry.toml`).
pub(super) fn load_staged_catalog(directory: &Path, thread_id: &str) -> Result<CatalogEntry> {
    let path = directory.join(STAGED_CATALOG_ENTRY_FILE_NAME);
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("staged catalog entry missing: {}", path.display()))?;
    let entry: CatalogEntry = toml::from_str(&content)
        .with_context(|| format!("invalid staged catalog entry {}", path.display()))?;
    ensure!(
        entry.id == thread_id,
        "staged catalog entry belongs to another Thread"
    );
    Ok(entry)
}

async fn load_attachment_catalog(
    directory: &Path,
    thread_id: &str,
) -> Result<Option<Vec<AttachmentRecord>>> {
    let path = directory.join("attachments.toml");
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => {
            let catalog: crate::studio::store::attachment::AttachmentCatalog =
                toml::from_str(&content).with_context(|| {
                    format!("invalid staged attachment catalog {}", path.display())
                })?;
            ensure!(
                catalog.thread_id == thread_id,
                "staged attachment catalog identity mismatch"
            );
            Ok(Some(catalog.records))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

async fn verify_blob(path: &Path, record: &AttachmentRecord) -> Result<()> {
    let bytes = tokio::fs::read(path)
        .await
        .with_context(|| format!("staged attachment blob is missing: {}", path.display()))?;
    ensure!(
        bytes.len() as u64 == record.byte_size,
        "staged attachment {} byte size mismatch",
        record.id
    );
    ensure!(
        hex_digest(&bytes) == record.content_sha256,
        "staged attachment {} content hash mismatch",
        record.id
    );
    Ok(())
}

/// Builds the projection identity for one legacy session, preserving saved ancestry.
///
/// The projector only consumes the identity and ancestry of this value; the authoritative
/// execution `mode` and `status` come from the replayed state that becomes `state.toml`. The
/// static directory fields (title, role, workspace mode/address, project) and the directory identity
/// facts (root/parent ancestry, canonical agent path, created/updated timestamps, archived) come
/// from the legacy `threads` row because they are product facts the journal never stores, so the
/// regenerated `catalog.toml` summary matches the directory UI exactly. A missing/empty ancestry or
/// agent-path cell falls back to this session's own id, the same canonical identity the directory
/// mapper (`mappers::thread_record`) derives, so a legitimate root row is never rejected.
async fn thread_identity(
    product: &sea_orm::DatabaseConnection,
    thread_id: &str,
    updated_at: i64,
) -> Result<pl_protocol::Thread> {
    let row = product
        .query_one_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            "SELECT project_id, title, mode, workspace_mode, workspace_path, parent_thread_id, \
             root_thread_id, agent_path, role, created_at, updated_at, archived FROM threads \
             WHERE id=?",
            [Value::String(Some(thread_id.to_owned()))],
        ))
        .await?;
    let mut thread = pl_protocol::Thread::placeholder(thread_id);
    thread.updated_at = updated_at;
    if let Some(row) = row {
        thread.project_id = row.try_get::<String>("", "project_id")?;
        let title = row.try_get::<String>("", "title")?;
        if !title.is_empty() {
            thread.title = title;
        }
        let mode = row.try_get::<String>("", "mode")?;
        if !mode.is_empty() {
            thread.mode = pl_protocol::ThreadModeId::new(mode)?;
        }
        let workspace_mode = row.try_get::<String>("", "workspace_mode")?;
        if !workspace_mode.is_empty() {
            thread.workspace_mode = pl_protocol::ThreadWorkspaceMode::from_label(&workspace_mode)?;
        }
        thread.workspace_path = row.try_get::<String>("", "workspace_path")?;
        // Directory identity: preserve the legacy root ancestry and canonical agent path verbatim.
        // Both columns are NOT NULL in the retired schema, but an empty cell (older/oddly written
        // rows) falls back to this session's own id, matching the canonical directory mapper and
        // keeping a legal root Thread the identity the verifier expects.
        let root_thread_id = row.try_get::<String>("", "root_thread_id")?;
        thread.root_thread_id = if root_thread_id.is_empty() {
            thread_id.to_owned()
        } else {
            root_thread_id
        };
        let agent_path = row.try_get::<String>("", "agent_path")?;
        thread.agent_path = if agent_path.is_empty() {
            thread_id.to_owned()
        } else {
            agent_path
        };
        let role = row.try_get::<String>("", "role")?;
        if !role.is_empty() {
            thread.role = role;
        }
        thread.parent_thread_id = row.try_get::<Option<String>>("", "parent_thread_id")?;
        thread.created_at = row.try_get::<i64>("", "created_at")?;
        thread.updated_at = row.try_get::<i64>("", "updated_at")?;
        thread.archived = row.try_get::<i32>("", "archived")? != 0;
    }
    Ok(thread)
}

fn hex_digest(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_attempt_is_terminal_but_running_is_not() {
        assert!(!attempt_status_is_terminal(&AttemptOutcome::Running));
        assert!(attempt_status_is_terminal(&AttemptOutcome::Interrupted));
    }

    #[test]
    fn usage_columns_keep_unmeasured_tokens_unknown() {
        let measured = ModelUsage {
            input_tokens: Some(10),
            cache_read_tokens: None,
            cache_write_tokens: None,
            output_tokens: Some(4),
            reasoning_tokens: Some(2),
        };
        assert_eq!(
            usage_columns(Some(&measured)),
            (Some(10), Some(4), None, None, Some(2), Some(14))
        );
        // 任一方向不可测量时 `total` 保持未知，不塌缩成 0。
        let partial = ModelUsage {
            input_tokens: None,
            output_tokens: Some(4),
            ..ModelUsage::default()
        };
        assert_eq!(
            usage_columns(Some(&partial)),
            (None, Some(4), None, None, None, None)
        );
        assert_eq!(usage_columns(None), (None, None, None, None, None, None));
    }

    #[test]
    fn a_session_without_commits_is_rejected_before_settlement() {
        assert!(legacy_journal_with_settlement(Vec::new(), "thread").is_err());
    }

    #[test]
    fn staged_item_kind_labels_match_the_history_store() {
        use pl_protocol::ThreadItemKind;
        assert_eq!(item_kind_label(ThreadItemKind::Raw), "raw");
        assert_eq!(item_kind_label(ThreadItemKind::Turn), "turn");
        assert_eq!(
            item_kind_label(ThreadItemKind::ContextCompaction),
            "contextCompaction"
        );
    }
}

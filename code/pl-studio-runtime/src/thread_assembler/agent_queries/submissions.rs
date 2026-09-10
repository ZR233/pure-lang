//! Stable UTF-8 continuation when a complete progress record exceeds a model page.
use super::*;
use pl_core::thread::{TurnOutcome, TurnRecord, TurnState};

const PAGE_BYTES: usize = 56 * 1024;
const FRAGMENT_BYTES: usize = 24 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SubmissionCursor {
    version: u32,
    target: String,
    through: u64,
    offset: usize,
    byte_offset: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Fragment {
    encoding: &'static str,
    byte_offset: usize,
    total_bytes: usize,
    content: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum Completion {
    NotStarted,
    Running,
    Completed,
    ToolCompleted,
    WaitingInteraction,
    StepLimit,
    Cancelled,
    Interrupted,
    Failed,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TargetState {
    agent_id: String,
    through_sequence: u64,
    turn: Option<TurnRecord>,
    completion: Completion,
    guidance: &'static str,
}
impl TargetState {
    fn from_history(agent_id: &str, history: &[Arc<ThreadCommit>]) -> Self {
        // The page cursor freezes both progress records and lifecycle facts. A later live
        // snapshot must never lend an earlier submission page a newer completion state.
        let turn = history.iter().rev().find_map(|commit| commit.turn.clone());
        let (completion, guidance) = match turn.as_ref().map(|turn| &turn.state) {
            None => (
                Completion::NotStarted,
                "No Turn has started at this watermark; submissions are not completion evidence.",
            ),
            Some(TurnState::Running) => (
                Completion::Running,
                "This page contains published progress only, not a terminal delivery, even if stage is readyForCompletion. Continue wait until you consume the matching child and Turn's successful completion notification before advancing dependent work.",
            ),
            Some(TurnState::Finished(TurnOutcome::Completed)) => (
                Completion::Completed,
                "This Turn completed. Before advancing dependent work, consume its matching child and Turn completion notification; this page alone does not prove parent consumption or bind old submissions to a new dispatch.",
            ),
            Some(TurnState::Finished(TurnOutcome::ToolCompleted)) => (
                Completion::ToolCompleted,
                "This Turn completed through a completing tool. Before advancing dependent work, consume its matching child and Turn completion notification; this page alone does not prove parent consumption or bind old submissions to a new dispatch.",
            ),
            Some(TurnState::Finished(TurnOutcome::WaitingInteraction)) => (
                Completion::WaitingInteraction,
                "This Turn is waiting for interaction, not successfully delivered. Resolve the interaction and await the corresponding successful completion.",
            ),
            Some(TurnState::Finished(TurnOutcome::StepLimit)) => (
                Completion::StepLimit,
                "This Turn reached its step limit, not successful completion. Diagnose progress and arrange continuation if needed.",
            ),
            Some(TurnState::Cancelled) => (
                Completion::Cancelled,
                "This Turn was cancelled; its progress is not a successful delivery.",
            ),
            Some(TurnState::Interrupted) => (
                Completion::Interrupted,
                "This Turn was interrupted; its progress is not a successful delivery.",
            ),
            Some(TurnState::Failed { .. }) => (
                Completion::Failed,
                "This Turn failed; inspect its failure before reusing published progress.",
            ),
        };
        Self {
            agent_id: agent_id.into(),
            through_sequence: history.last().map_or(0, |commit| commit.sequence),
            turn,
            completion,
            guidance,
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    #[serde(flatten)]
    page: AgentSubmissionPage,
    target_state: TargetState,
    next_cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fragment: Option<Fragment>,
}

pub(super) fn page(
    input: SubmissionsInput,
    mut history: Vec<Arc<ThreadCommit>>,
) -> Result<ToolOutput> {
    if !(1..=50).contains(&input.limit) {
        bail!("submission limit must be between 1 and 50");
    }
    let mut cursor = match input.cursor {
        Some(encoded) => {
            let cursor: SubmissionCursor = serde_json::from_slice(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded)?,
            )?;
            if cursor.version != 1
                || cursor.target != input.target
                || cursor.through > history.len() as u64
            {
                bail!("submission cursor does not belong to this target or history");
            }
            cursor
        }
        None => SubmissionCursor {
            version: 1,
            target: input.target,
            through: history.len() as u64,
            offset: input.offset,
            byte_offset: 0,
        },
    };
    history.truncate(usize::try_from(cursor.through)?);
    let target_state = TargetState::from_history(&cursor.target, &history);
    let submissions = history
        .iter()
        .flat_map(|commit| commit.extensions.iter())
        .filter_map(|change| match change {
            pl_core::thread::extensions::ExtensionChange::Put { id, record }
                if id == super::super::progress::EXTENSION =>
            {
                Some(super::super::progress::decode(&record.payload))
            }
            pl_core::thread::extensions::ExtensionChange::Put { .. }
            | pl_core::thread::extensions::ExtensionChange::Delete { .. } => None,
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let total = submissions.len();
    if cursor.byte_offset != 0 && cursor.offset >= total {
        bail!("submission fragment cursor is beyond the saved history");
    }
    let offset = cursor.offset;
    let mut items = Vec::new();
    let mut bytes = 0;
    let mut fragment = None;
    for record in submissions.into_iter().skip(offset).take(input.limit) {
        let encoded = serde_json::to_string(&record)?;
        if cursor.byte_offset != 0 || encoded.len() > PAGE_BYTES {
            if !items.is_empty() {
                break;
            }
            if cursor.byte_offset >= encoded.len() || !encoded.is_char_boundary(cursor.byte_offset)
            {
                bail!("invalid submission fragment boundary");
            }
            let start = cursor.byte_offset;
            let mut end = start.saturating_add(FRAGMENT_BYTES).min(encoded.len());
            while !encoded.is_char_boundary(end) {
                end -= 1;
            }
            fragment = Some(Fragment {
                encoding: "utf8-json",
                byte_offset: start,
                total_bytes: encoded.len(),
                content: encoded[start..end].into(),
            });
            if end == encoded.len() {
                cursor.offset += 1;
                cursor.byte_offset = 0;
            } else {
                cursor.byte_offset = end;
            }
            break;
        }
        if bytes + encoded.len() > PAGE_BYTES {
            break;
        }
        bytes += encoded.len();
        items.push(record);
        cursor.offset += 1;
    }
    let has_more = cursor.offset < total;
    let next_cursor = has_more
        .then(|| {
            serde_json::to_vec(&cursor)
                .map(|bytes| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
        })
        .transpose()?;
    output(&Page {
        page: AgentSubmissionPage {
            items,
            offset,
            limit: input.limit,
            total,
            has_more,
        },
        target_state,
        next_cursor,
        fragment,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pl_core::{
        context::OpaquePayload,
        model::{DynModelSession, ModelError, ModelRequest, ModelSession, PreparedModelCall},
    };
    use pretty_assertions::assert_eq;

    struct NeverRun;
    impl ModelSession for NeverRun {
        async fn prepare(&mut self, _: ModelRequest) -> Result<PreparedModelCall, ModelError> {
            panic!("history queries must never invoke a model")
        }
        async fn close(&mut self) -> Result<(), ModelError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn ready_progress_preserves_payload_without_implying_successful_completion() {
        let thread = ThreadHandle::start("agent".into(), DynModelSession::new(NeverRun)).unwrap();
        let record = pl_protocol::AgentSubmissionRecord {
            report: pl_protocol::AgentProgressReport {
                stage: pl_protocol::AgentProgressStage::ReadyForCompletion,
                summary: "ready".into(),
                next_step: "deliver".into(),
                revision: 1,
            },
            detail: Some("unchanged complete detail".into()),
            created_at: 4,
        };
        thread
            .mutate_extensions(vec![pl_core::thread::extensions::ExtensionMutation::Put {
                id: super::super::super::progress::EXTENSION.into(),
                expected_revision: None,
                payload: OpaquePayload::new(
                    super::super::super::progress::FORMAT,
                    1,
                    serde_json::to_string(&record).unwrap(),
                )
                .unwrap(),
            }])
            .await
            .unwrap();
        let history = thread
            .journal_page(0, std::num::NonZeroUsize::new(128).unwrap())
            .await
            .unwrap();
        // Query consumes immutable committed facts. Vary only the recorded Turn disposition,
        // keeping the producer's readyForCompletion payload identical in every page.
        for (state, completion) in [
            (None, "notStarted"),
            (Some(TurnState::Running), "running"),
            (
                Some(TurnState::Finished(TurnOutcome::Completed)),
                "completed",
            ),
            (
                Some(TurnState::Finished(TurnOutcome::ToolCompleted)),
                "toolCompleted",
            ),
            (
                Some(TurnState::Finished(TurnOutcome::WaitingInteraction)),
                "waitingInteraction",
            ),
            (
                Some(TurnState::Finished(TurnOutcome::StepLimit)),
                "stepLimit",
            ),
            (Some(TurnState::Cancelled), "cancelled"),
            (Some(TurnState::Interrupted), "interrupted"),
            (
                Some(TurnState::Failed {
                    description: "failure".into(),
                }),
                "failed",
            ),
        ] {
            let mut commit = (*history[0]).clone();
            commit.turn = state.map(|state| TurnRecord {
                elapsed_ms: None,
                input_id: Some("dispatch".into()),
                turn_id: "turn".into(),
                state,
                model_steps: 1,
            });
            let expected_turn = serde_json::to_value(&commit.turn).unwrap();
            let result = page(
                SubmissionsInput {
                    target: "agent".into(),
                    cursor: None,
                    offset: 0,
                    limit: 50,
                },
                vec![Arc::new(commit)],
            )
            .unwrap();
            let result: serde_json::Value =
                serde_json::from_str(result.payload().content()).unwrap();
            assert_eq!(result["items"], serde_json::json!([record]));
            assert_eq!(result["targetState"]["completion"], completion);
            assert_eq!(result["targetState"]["turn"], expected_turn);
            assert_eq!(result["targetState"]["agentId"], "agent");
            assert_eq!(result["targetState"]["throughSequence"], 1);
            if completion == "running" {
                assert!(
                    result["targetState"]["guidance"]
                        .as_str()
                        .unwrap()
                        .contains("Continue wait")
                );
            }
        }
        thread.close().await.unwrap();
    }

    #[tokio::test]
    async fn oversized_progress_is_losslessly_paged_without_reading_later_commits() {
        let thread = ThreadHandle::start("agent".into(), DynModelSession::new(NeverRun)).unwrap();
        let record = pl_protocol::AgentSubmissionRecord {
            report: pl_protocol::AgentProgressReport {
                stage: pl_protocol::AgentProgressStage::Implementing,
                summary: "summary".into(),
                next_step: "verify".into(),
                revision: 1,
            },
            detail: Some("🦀\n\"".repeat(15_000)),
            created_at: 4,
        };
        let original = serde_json::to_string(&record).unwrap();
        thread
            .mutate_extensions(vec![pl_core::thread::extensions::ExtensionMutation::Put {
                id: super::super::super::progress::EXTENSION.into(),
                expected_revision: None,
                payload: OpaquePayload::new(
                    super::super::super::progress::FORMAT,
                    1,
                    original.clone(),
                )
                .unwrap(),
            }])
            .await
            .unwrap();
        let mut history = thread
            .journal_page(0, std::num::NonZeroUsize::new(128).unwrap())
            .await
            .unwrap();
        Arc::make_mut(&mut history[0]).turn = Some(TurnRecord {
            elapsed_ms: None,
            input_id: None,
            turn_id: "turn".into(),
            state: TurnState::Running,
            model_steps: 1,
        });
        let frozen = history[0].clone();
        let mut cursor = None;
        let mut reconstructed = String::new();
        loop {
            let result = page(
                SubmissionsInput {
                    target: "agent".into(),
                    cursor,
                    offset: 0,
                    limit: 50,
                },
                history.clone(),
            )
            .unwrap();
            assert!(
                result.payload().content().len() < 64 * 1024,
                "model page must stay bounded"
            );
            let page: serde_json::Value = serde_json::from_str(result.payload().content()).unwrap();
            let fragment = &page["fragment"];
            assert_eq!(
                fragment["byteOffset"].as_u64().unwrap(),
                reconstructed.len() as u64
            );
            reconstructed.push_str(fragment["content"].as_str().unwrap());
            cursor = page["nextCursor"].as_str().map(str::to_owned);
            if history.len() == 1 {
                let mut later = record.clone();
                later.report.revision = 2;
                later.detail = Some("later progress must not enter the frozen page".into());
                thread
                    .mutate_extensions(vec![pl_core::thread::extensions::ExtensionMutation::Put {
                        id: super::super::super::progress::EXTENSION.into(),
                        expected_revision: Some(1),
                        payload: OpaquePayload::new(
                            super::super::super::progress::FORMAT,
                            1,
                            serde_json::to_string(&later).unwrap(),
                        )
                        .unwrap(),
                    }])
                    .await
                    .unwrap();
                history = thread
                    .journal_page(0, std::num::NonZeroUsize::new(128).unwrap())
                    .await
                    .unwrap();
            }
            if history.len() > 1 {
                history[0] = frozen.clone();
                Arc::make_mut(&mut history[1]).turn = Some(TurnRecord {
                    elapsed_ms: Some(1),
                    input_id: None,
                    turn_id: "turn".into(),
                    state: TurnState::Finished(TurnOutcome::Completed),
                    model_steps: 1,
                });
            }
            assert_eq!(page["targetState"]["throughSequence"], 1);
            assert_eq!(page["targetState"]["completion"], "running");
            assert_eq!(page["total"].as_u64(), Some(1));
            if cursor.is_none() {
                break;
            }
        }
        let fresh = page(
            SubmissionsInput {
                target: "agent".into(),
                cursor: None,
                offset: 0,
                limit: 50,
            },
            history,
        )
        .unwrap();
        let fresh: serde_json::Value = serde_json::from_str(fresh.payload().content()).unwrap();
        assert_eq!(fresh["targetState"]["completion"], "completed");
        assert_eq!(fresh["targetState"]["throughSequence"], 2);
        assert_eq!(reconstructed, original);
        assert_eq!(
            serde_json::from_str::<pl_protocol::AgentSubmissionRecord>(&reconstructed).unwrap(),
            record
        );
        thread.close().await.unwrap();
    }
}

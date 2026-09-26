use pl_protocol::studio::HistoryFault;
use pl_protocol::{
    ThreadActivity, ThreadActivityArguments, ThreadActivityDetail, ThreadActivityKind,
    ThreadActivityToolState, ThreadStorageExecution, ThreadStorageState,
};

use crate::api::studio::types::{
    BridgeHistoryFault, BridgeThreadActivity, BridgeThreadActivityArguments,
    BridgeThreadActivityContentPart, BridgeThreadActivityDetail, BridgeThreadActivityKind,
    BridgeThreadActivityToolDetail, BridgeThreadActivityToolEntry, BridgeThreadActivityToolState,
    BridgeThreadActivityTools, BridgeThreadStorageExecution, BridgeThreadStorageState,
};

pub(crate) fn activity(value: ThreadActivity) -> BridgeThreadActivity {
    BridgeThreadActivity {
        thread_id: value.thread_id,
        identity: value.identity,
        revision: value.revision,
        turn_id: value.turn_id,
        input_id: value.input_id,
        attempt_id: value.attempt_id,
        kind: activity_kind(value.kind),
        summary: value.summary,
        summary_truncated: value.summary_truncated,
        tools: BridgeThreadActivityTools {
            count: value.tools.count,
            background: value.tools.background,
            active: value.tools.active.into_iter().map(activity_tool).collect(),
            latest_started: value.tools.latest_started.map(activity_tool),
        },
    }
}

fn activity_kind(value: ThreadActivityKind) -> BridgeThreadActivityKind {
    match value {
        ThreadActivityKind::Preparing => BridgeThreadActivityKind::Preparing,
        ThreadActivityKind::WaitingApi => BridgeThreadActivityKind::WaitingApi,
        ThreadActivityKind::Thinking => BridgeThreadActivityKind::Thinking,
        ThreadActivityKind::Responding => BridgeThreadActivityKind::Responding,
        ThreadActivityKind::Planning => BridgeThreadActivityKind::Planning,
        ThreadActivityKind::RunningTool => BridgeThreadActivityKind::RunningTool,
        ThreadActivityKind::AwaitingApproval => BridgeThreadActivityKind::AwaitingApproval,
        ThreadActivityKind::AwaitingInput => BridgeThreadActivityKind::AwaitingInput,
        ThreadActivityKind::Stopping => BridgeThreadActivityKind::Stopping,
    }
}

fn activity_arguments(value: ThreadActivityArguments) -> BridgeThreadActivityArguments {
    match value {
        ThreadActivityArguments::CommandLine => BridgeThreadActivityArguments::CommandLine,
        ThreadActivityArguments::Opaque => BridgeThreadActivityArguments::Opaque,
        ThreadActivityArguments::Streaming => BridgeThreadActivityArguments::Streaming,
        ThreadActivityArguments::Unavailable => BridgeThreadActivityArguments::Unavailable,
    }
}

fn tool_state(value: ThreadActivityToolState) -> BridgeThreadActivityToolState {
    match value {
        ThreadActivityToolState::Running => BridgeThreadActivityToolState::Running,
        ThreadActivityToolState::AwaitingApproval => {
            BridgeThreadActivityToolState::AwaitingApproval
        }
        ThreadActivityToolState::Cancelling => BridgeThreadActivityToolState::Cancelling,
        ThreadActivityToolState::Finished => BridgeThreadActivityToolState::Finished,
    }
}

fn activity_tool(value: pl_protocol::ThreadActivityToolEntry) -> BridgeThreadActivityToolEntry {
    BridgeThreadActivityToolEntry {
        call_id: value.call_id,
        task_id: value.task_id,
        name: value.name,
        summary: value.summary,
        arguments: activity_arguments(value.arguments),
        state: tool_state(value.state),
        ordinal: value.ordinal,
        started_at: value.started_at,
    }
}

pub(crate) fn activity_detail(value: ThreadActivityDetail) -> BridgeThreadActivityDetail {
    match value {
        ThreadActivityDetail::Current {
            activity: current,
            reasoning,
            response,
            tools,
        } => BridgeThreadActivityDetail::Current {
            activity: activity(current),
            reasoning: reasoning.into_iter().map(content_part).collect(),
            response: response.into_iter().map(content_part).collect(),
            tools: tools.into_iter().map(tool_detail).collect(),
        },
        ThreadActivityDetail::Superseded {
            activity: current,
            requested_activity_id,
        } => BridgeThreadActivityDetail::Superseded {
            activity: activity(current),
            requested_activity_id,
        },
        ThreadActivityDetail::Ended {
            thread_id,
            activity_id,
        } => BridgeThreadActivityDetail::Ended {
            thread_id,
            activity_id,
        },
    }
}

fn content_part(value: pl_protocol::ThreadActivityContentPart) -> BridgeThreadActivityContentPart {
    BridgeThreadActivityContentPart {
        item_id: value.item_id,
        revision: value.revision,
        complete: value.complete,
        text: value.text,
    }
}

fn tool_detail(value: pl_protocol::ThreadActivityToolDetail) -> BridgeThreadActivityToolDetail {
    BridgeThreadActivityToolDetail {
        call_id: value.call_id,
        task_id: value.task_id,
        name: value.name,
        state: tool_state(value.state),
        arguments: value.arguments,
        output: value.output,
        ordinal: value.ordinal,
        started_at: value.started_at,
    }
}

pub(crate) fn storage(value: ThreadStorageState) -> BridgeThreadStorageState {
    BridgeThreadStorageState {
        fault: value.fault.map(history_fault),
        fault_generation: value.fault_generation,
        accepted_sequence: value.accepted_sequence,
        durable_sequence: value.durable_sequence,
        execution: match value.execution {
            ThreadStorageExecution::Running => BridgeThreadStorageExecution::Running,
            ThreadStorageExecution::Pausing => BridgeThreadStorageExecution::Pausing,
            ThreadStorageExecution::Paused => BridgeThreadStorageExecution::Paused,
        },
        pressure_paused: value.pressure_paused,
        resume_required: value.resume_required,
        can_resume: value.can_resume,
        last_error: value.last_error,
    }
}

fn history_fault(value: HistoryFault) -> BridgeHistoryFault {
    match value {
        HistoryFault::QueueFull => BridgeHistoryFault::QueueFull,
        HistoryFault::WriteFailed => BridgeHistoryFault::WriteFailed,
        HistoryFault::WriterUnavailable => BridgeHistoryFault::WriterUnavailable,
        HistoryFault::NoProgress => BridgeHistoryFault::NoProgress,
        HistoryFault::CheckpointFailed => BridgeHistoryFault::CheckpointFailed,
        HistoryFault::BlobFailed => BridgeHistoryFault::BlobFailed,
    }
}

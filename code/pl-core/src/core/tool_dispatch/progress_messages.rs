use pl_trace::TracePartStatus;

use crate::core::progress::ProgressEmitter;

use super::ToolExecutionRecord;

pub(super) fn emit_tool_progress(
    progress: &mut ProgressEmitter,
    recorder: &mut crate::TraceRecorder,
    record: &ToolExecutionRecord,
) {
    progress.tool_detail(recorder, tool_terminal_progress_message(record));
}

pub(super) fn tool_start_progress_message(name: &str) -> String {
    match name {
        "plan_exit" => "正在提交计划。".to_string(),
        "request_user_input" => "正在等待用户输入。".to_string(),
        "update_todo_list" => "正在更新 todo list。".to_string(),
        "spawn_agent" => "正在创建子代理。".to_string(),
        "report_progress" => "正在记录执行进度。".to_string(),
        "list_agents" => "正在检查子代理状态。".to_string(),
        "wait_agents" => "正在等待子代理进度。".to_string(),
        "read_agent_session" => "正在读取子代理会话摘要。".to_string(),
        "send_message" => "正在给子代理发送消息。".to_string(),
        "interrupt_agent" => "正在中断子代理当前轮次。".to_string(),
        "close_agent" => "正在关闭子代理。".to_string(),
        _ => format!("正在执行工具 `{name}`。"),
    }
}

pub(super) fn tool_terminal_progress_message(record: &ToolExecutionRecord) -> String {
    let name = &record.name;
    match record.status {
        TracePartStatus::Completed => match name.as_str() {
            "plan_exit" => "计划已生成，等待确认。".to_string(),
            "request_user_input" => "用户输入已收到。".to_string(),
            "update_todo_list" => "Todo list 已更新。".to_string(),
            "spawn_agent" => "子代理已创建。".to_string(),
            "report_progress" => "执行进度已记录。".to_string(),
            "list_agents" => "子代理状态已更新。".to_string(),
            "wait_agents" => "子代理有了新进度。".to_string(),
            "read_agent_session" => "子代理会话摘要已读取。".to_string(),
            "send_message" => "子代理消息已发送。".to_string(),
            "interrupt_agent" => "子代理当前轮次已中断。".to_string(),
            "close_agent" => "子代理已关闭。".to_string(),
            _ => format!("工具 `{name}` 已完成。"),
        },
        TracePartStatus::Denied => format!("工具 `{name}` 已拒绝。"),
        TracePartStatus::Failed => match name.as_str() {
            "plan_exit" => "计划提交失败。".to_string(),
            "request_user_input" => "用户输入请求失败。".to_string(),
            "update_todo_list" => "Todo list 更新失败。".to_string(),
            "spawn_agent" | "report_progress" | "list_agents" | "wait_agents"
            | "read_agent_session" | "send_message" | "interrupt_agent" | "close_agent" => {
                format!("子代理工具 `{name}` 执行失败。")
            }
            _ => format!("工具 `{name}` 执行失败。"),
        },
        TracePartStatus::Interrupted => format!("工具 `{name}` 已中断。"),
        TracePartStatus::BudgetLimited => format!("工具 `{name}` 因预算限制停止。"),
        TracePartStatus::Started
        | TracePartStatus::Streaming
        | TracePartStatus::AwaitingApproval
        | TracePartStatus::Approved
        | TracePartStatus::Running => format!("工具 `{name}` 已结束。"),
    }
}

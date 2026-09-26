//! Canonical `ThreadItem` ↔ typed `ChatItem` 适配。
//!
//! core 的窗口条目是「静态结构 + 若干共享 `ContentBlock` 内容域」。宿主把 canonical 条目的**静态部分**
//! （身份、kind、时间戳、附件、工具身份……）编码一次进 `meta`，把**动态正文**按 [`ChatField`] 放进
//! `fields`：流式热路径只克隆 `Arc`/共享链，不 JSON、不逐 token 复制全文。
//!
//! 反向只发生在**交付/落盘边界**（窗口出帧、writer 持久化）：用 [`canonical_item`] 把 `meta` 的结构与
//! `fields` 的正文重新组装成一条 canonical [`ThreadItem`]，结构化字段（attachments、tool 参数/结果、
//! reasoning summary/content）都在这里如实还原，不丢域、不做字符串拼接。

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use pl_core::chat::{ChatField, ChatItem, ChatLifecycle, PresentationPart};
use pl_core::model::ContentBlock;
use pl_protocol::{
    ThreadItem, ThreadItemState, ThreadTextItem, ThreadThinkingItem, ThreadToolInvocation,
    ThreadToolItem, ThreadToolState,
};

/// 工具参数与工具结果各自的宿主字段域 key（稳定身份，不是内容推断）。
pub(crate) const TOOL_ARGUMENTS_FIELD: &str = "tool.arguments";
pub(crate) const TOOL_RESULT_FIELD: &str = "tool.result";

/// 把一条 canonical 条目拆成 typed 窗口条目：静态结构进 `meta`，动态正文进 `fields`。
///
/// `saved` 是 writer 已确认的保存水位，独立于执行终态 `lifecycle`；`omitted_bytes` 由调用方按有界
/// 预览策略给出（0 表示完整）。
pub(crate) fn typed_item(item: ThreadItem, saved: bool, omitted_bytes: u64) -> Result<ChatItem> {
    let lifecycle = if item.state().is_terminal() {
        ChatLifecycle::Terminal
    } else {
        ChatLifecycle::Streaming
    };
    let fields = content_fields(item.state());
    let meta = static_meta(&item)?;
    Ok(ChatItem {
        item_id: item.id,
        turn_id: item.turn_id,
        order: item.ordinal,
        revision: item.revision,
        fields,
        meta,
        omitted_bytes,
        saved,
        lifecycle,
    })
}

/// 只含静态结构的 `meta`：身份、kind、时间戳、附件与工具身份，动态正文清空、revision 归零。
///
/// 静态部分与动态正文分开编码，热路径因此可以按身份缓存它一次、逐 token 只交付共享正文块；窗口
/// revision 由调用方自己的内容版本给出，不从 `meta` 里回读，避免把提交序号冒充成内容版本。
pub(crate) fn static_meta(item: &ThreadItem) -> Result<Arc<[u8]>> {
    let blank = ThreadItem::new(
        item.id.clone(),
        item.thread_id.clone(),
        item.turn_id.clone(),
        item.ordinal,
        0,
        item.created_at,
        item.updated_at,
        static_state(item.state()),
    );
    Ok(Arc::from(serde_json::to_vec(&blank)?))
}

/// 动态正文域：只由条目自己的动态事实派生，正文块与生产者共享。
pub(crate) fn content_fields(state: &ThreadItemState) -> BTreeMap<ChatField, Arc<ContentBlock>> {
    let mut fields = BTreeMap::new();
    match state {
        ThreadItemState::Text(text) => {
            fields.insert(ChatField::Body, ContentBlock::from_text(text.text()));
        }
        ThreadItemState::Thinking(thinking) => {
            for (index, chunk) in thinking.summary().iter().enumerate() {
                fields.insert(
                    ChatField::Part(PresentationPart::SummaryText(index as u32)),
                    ContentBlock::from_text(chunk),
                );
            }
            for (index, chunk) in thinking.content().iter().enumerate() {
                fields.insert(
                    ChatField::Part(PresentationPart::ReasoningText(index as u32)),
                    ContentBlock::from_text(chunk),
                );
            }
        }
        ThreadItemState::Tool(tool) => {
            fields.insert(
                ChatField::host(TOOL_ARGUMENTS_FIELD),
                ContentBlock::from_text(tool.invocation().arguments()),
            );
            if let Some(output) = streamed_output(tool.state()) {
                fields.insert(
                    ChatField::host(TOOL_RESULT_FIELD),
                    ContentBlock::from_text(output),
                );
            }
        }
        _ => {}
    }
    fields
}

/// 同一条目的**结构 / 终态形状**：动态正文被清空，kind、附件、生命周期与工具状态如实保留。
///
/// 有两个用途，都是为了让「可持久化载荷」与「易变元数据」分开：
/// * [`static_meta`] 用它编码一次静态结构，热路径按身份复用；
/// * 内容版本比较（宿主投影）用它判断持久化载荷的**结构 / 终态状态**是否变化——工具状态转移
///   （Running→Succeeded 等）与执行终态（streaming→completed/failed/cancelled）都是载荷变化，
///   必须递增内容版本，否则旧的保存回执会张冠李戴地确认新载荷。
///
/// 它**不含** `ThreadItem` 上会随每次投影变化的易变时间戳（`created_at`/`updated_at`），因此同一条目
/// 重复投影不会无意义地抬高内容版本；但生命周期自带的 `completed_at`/`failed_at` 属于真实转移，保留。
pub(crate) fn static_state(state: &ThreadItemState) -> ThreadItemState {
    match state {
        ThreadItemState::Text(text) => ThreadItemState::Text(ThreadTextItem::new(
            text.channel(),
            String::new(),
            text.attachments().to_vec(),
            text.lifecycle().clone(),
        )),
        ThreadItemState::Thinking(thinking) => ThreadItemState::Thinking(ThreadThinkingItem::new(
            Vec::new(),
            Vec::new(),
            thinking.lifecycle().clone(),
        )),
        ThreadItemState::Tool(tool) => ThreadItemState::Tool(ThreadToolItem::new(
            clear_arguments(tool.invocation()),
            clear_streamed_output(tool.state()),
        )),
        other => other.clone(),
    }
}

/// 把 typed 窗口条目重新组装成 canonical 条目（只在交付/落盘边界调用）。
pub(crate) fn canonical_item(item: &ChatItem) -> Result<ThreadItem> {
    let canonical: ThreadItem = serde_json::from_slice(&item.meta)?;
    let state = match canonical.state() {
        ThreadItemState::Text(text) => ThreadItemState::Text(ThreadTextItem::new(
            text.channel(),
            text_of(item, &ChatField::Body),
            text.attachments().to_vec(),
            text.lifecycle().clone(),
        )),
        ThreadItemState::Thinking(thinking) => {
            let summary = chunk_vec(item, ChunkKind::Summary);
            let content = chunk_vec(item, ChunkKind::Reasoning);
            ThreadItemState::Thinking(ThreadThinkingItem::new(
                summary,
                content,
                thinking.lifecycle().clone(),
            ))
        }
        ThreadItemState::Tool(tool) => {
            let arguments = text_of(item, &ChatField::host(TOOL_ARGUMENTS_FIELD));
            let output = text_of(item, &ChatField::host(TOOL_RESULT_FIELD));
            ThreadItemState::Tool(ThreadToolItem::new(
                fill_arguments(tool.invocation(), arguments),
                fill_streamed_output(tool.state(), &output),
            ))
        }
        other => other.clone(),
    };
    // `meta` 只携带静态结构（revision 归零），窗口自己的内容版本才是权威，绝不回读成提交序号。
    Ok(ThreadItem::new(
        item.item_id.clone(),
        canonical.thread_id.clone(),
        item.turn_id.clone(),
        item.order,
        item.revision,
        canonical.created_at,
        canonical.updated_at,
        state,
    ))
}

fn text_of(item: &ChatItem, field: &ChatField) -> String {
    item.field(field)
        .map_or_else(String::new, |block| block.text())
}

/// thinking 的 chunk 类别：summary 与 reasoning 各自独立成字段域。
#[derive(Clone, Copy)]
enum ChunkKind {
    Summary,
    Reasoning,
}

/// 重建 thinking 某类 chunk 向量：以已交付的 Part 字段索引为准，缺位补空串。
fn chunk_vec(item: &ChatItem, kind: ChunkKind) -> Vec<String> {
    let mut indexed: Vec<(u32, String)> = Vec::new();
    for (field, block) in &item.fields {
        let ChatField::Part(part) = field else {
            continue;
        };
        let index = match (kind, part) {
            (ChunkKind::Summary, PresentationPart::SummaryText(index)) => Some(*index),
            (ChunkKind::Reasoning, PresentationPart::ReasoningText(index)) => Some(*index),
            _ => None,
        };
        if let Some(index) = index {
            indexed.push((index, block.text()));
        }
    }
    indexed.sort_by_key(|(index, _)| *index);
    let len = indexed.last().map_or(0, |(index, _)| *index as usize + 1);
    let mut chunks = vec![String::new(); len];
    for (index, text) in indexed {
        chunks[index as usize] = text;
    }
    chunks
}

fn streamed_output(state: &ThreadToolState) -> Option<&str> {
    match state {
        ThreadToolState::Running(running) => Some(running.streamed_output()),
        ThreadToolState::Cancelling(cancelling) => Some(cancelling.streamed_output()),
        _ => None,
    }
}

fn clear_arguments(invocation: &ThreadToolInvocation) -> ThreadToolInvocation {
    rebuild_invocation(invocation, String::new())
}

fn clear_streamed_output(state: &ThreadToolState) -> ThreadToolState {
    match state {
        ThreadToolState::Running(_) => {
            ThreadToolState::Running(pl_protocol::RunningThreadTool::new(String::new()))
        }
        ThreadToolState::Cancelling(_) => {
            ThreadToolState::Cancelling(pl_protocol::CancellingThreadTool::new(String::new()))
        }
        other => other.clone(),
    }
}

fn fill_arguments(invocation: &ThreadToolInvocation, arguments: String) -> ThreadToolInvocation {
    rebuild_invocation(invocation, arguments)
}

/// 用给定正文重建工具调用身份，保留 provider 身份、工作目录与任务关联。
///
/// 只有 `arguments` 是正文域，其余字段是静态身份：静态 `meta` 清空正文、落盘边界再填回时都不能
/// 丢掉它们，否则窗口条目与 canonical 条目会在 task/call 关联上分叉。
fn rebuild_invocation(
    invocation: &ThreadToolInvocation,
    arguments: String,
) -> ThreadToolInvocation {
    let rebuilt = ThreadToolInvocation::new(
        invocation.tool_call_id().to_owned(),
        invocation.name().to_owned(),
        arguments,
    )
    .with_provider_identity(
        invocation.call_id().map(str::to_owned),
        invocation.provider_item_id().map(str::to_owned),
    )
    .with_working_directory(invocation.working_directory().map(str::to_owned));
    match invocation.task_id() {
        Some(task_id) => rebuilt.with_task_id(task_id.to_owned()),
        None => rebuilt,
    }
}

fn fill_streamed_output(state: &ThreadToolState, output: &str) -> ThreadToolState {
    match state {
        ThreadToolState::Running(_) => {
            ThreadToolState::Running(pl_protocol::RunningThreadTool::new(output.to_owned()))
        }
        ThreadToolState::Cancelling(_) => {
            ThreadToolState::Cancelling(pl_protocol::CancellingThreadTool::new(output.to_owned()))
        }
        other => other.clone(),
    }
}

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use futures::future::BoxFuture;
use pl_protocol::{PureError, Result, ThreadAttachment};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedAttachment {
    pub attachment_id: String,
    pub modality: pl_protocol::AttachmentModality,
    pub media_type: String,
    pub filename: Option<String>,
    pub data: String,
    pub byte_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub initial_remote_url: Option<String>,
}

/// 经过图片格式与尺寸校验、可安全写入线程附件存储的快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolImageAttachmentInput {
    pub filename: String,
    pub media_type: String,
    pub data: Vec<u8>,
    pub content_sha256: String,
    pub width: u32,
    pub height: u32,
}

type ToolImageAttachmentWriter = dyn Fn(Vec<ToolImageAttachmentInput>) -> BoxFuture<'static, Result<Vec<ThreadAttachment>>>
    + Send
    + Sync;
type AttachmentLoader =
    dyn Fn(Vec<String>) -> BoxFuture<'static, Result<Vec<MaterializedAttachment>>> + Send + Sync;

/// 宿主提供的线程附件持久化与读取边界。
///
/// `pl-core` 不持有数据库、blob 路径或线程所有权规则；宿主通过该运行时把工具
/// 产生的图片原子写入当前线程，并按附件 ID 读取经过授权的持久快照。
#[derive(Clone)]
pub struct AttachmentRuntime {
    writer: Arc<ToolImageAttachmentWriter>,
    loader: Arc<AttachmentLoader>,
    atomic_batch: bool,
}

impl AttachmentRuntime {
    /// 使用只承诺单图写入的宿主边界构造运行时。
    ///
    /// 该兼容入口不声明批次原子性，因此不能用于接收 MCP 多图结果。
    pub fn new<W, WFut, L, LFut>(writer: W, loader: L) -> Self
    where
        W: Fn(ToolImageAttachmentInput) -> WFut + Send + Sync + 'static,
        WFut: Future<Output = Result<ThreadAttachment>> + Send + 'static,
        L: Fn(Vec<String>) -> LFut + Send + Sync + 'static,
        LFut: Future<Output = Result<Vec<MaterializedAttachment>>> + Send + 'static,
    {
        let writer = Arc::new(writer);
        Self {
            writer: Arc::new(move |inputs| {
                let writer = writer.clone();
                Box::pin(async move {
                    let mut attachments = Vec::with_capacity(inputs.len());
                    for input in inputs {
                        attachments.push(writer(input).await?);
                    }
                    Ok(attachments)
                })
            }),
            loader: Arc::new(move |attachment_ids| Box::pin(loader(attachment_ids))),
            atomic_batch: false,
        }
    }

    /// 使用宿主提供的原子批量 writer 构造附件运行时。
    pub fn new_batch<W, WFut, L, LFut>(writer: W, loader: L) -> Self
    where
        W: Fn(Vec<ToolImageAttachmentInput>) -> WFut + Send + Sync + 'static,
        WFut: Future<Output = Result<Vec<ThreadAttachment>>> + Send + 'static,
        L: Fn(Vec<String>) -> LFut + Send + Sync + 'static,
        LFut: Future<Output = Result<Vec<MaterializedAttachment>>> + Send + 'static,
    {
        Self {
            writer: Arc::new(move |inputs| Box::pin(writer(inputs))),
            loader: Arc::new(move |attachment_ids| Box::pin(loader(attachment_ids))),
            atomic_batch: true,
        }
    }

    /// 返回宿主是否承诺图片批次在单一原子边界内提交。
    pub(crate) fn supports_atomic_image_batch(&self) -> bool {
        self.atomic_batch
    }

    /// 持久化一张已经验证和规范化的工具图片。
    ///
    /// # Errors
    ///
    /// 当宿主拒绝当前线程写入，或 blob/metadata 提交失败时返回错误。
    pub async fn write_image(&self, input: ToolImageAttachmentInput) -> Result<ThreadAttachment> {
        let mut attachments = self.write_images(vec![input]).await?;
        attachments
            .pop()
            .ok_or_else(|| attachment_writer_error(1, 0))
    }

    /// 持久化一个工具结果产生的有序图片批次。
    ///
    /// 通过 [`Self::new_batch`] 构造时由宿主保证原子性；[`Self::new`] 只用于
    /// `view_image` 等单图便利入口。
    ///
    /// # Errors
    ///
    /// 当宿主拒绝批次、持久化失败，或返回的附件数量与输入不一致时返回错误。
    pub async fn write_images(
        &self,
        inputs: Vec<ToolImageAttachmentInput>,
    ) -> Result<Vec<ThreadAttachment>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let expected = inputs.len();
        let attachments = (self.writer)(inputs).await?;
        if attachments.len() != expected {
            return Err(attachment_writer_error(expected, attachments.len()));
        }
        Ok(attachments)
    }

    /// 批量读取当前线程有权访问的附件快照。
    ///
    /// # Errors
    ///
    /// 当附件缺失、所有权不匹配或持久层读取失败时返回错误。
    pub async fn load(&self, attachment_ids: Vec<String>) -> Result<Vec<MaterializedAttachment>> {
        (self.loader)(attachment_ids).await
    }
}

fn attachment_writer_error(expected: usize, actual: usize) -> PureError {
    PureError::ToolExecutionFailed {
        tool: "attachment".to_string(),
        error: format!(
            "attachment writer returned {actual} items for a batch of {expected} images"
        ),
    }
}

impl fmt::Debug for AttachmentRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AttachmentRuntime")
            .field("atomic_batch", &self.atomic_batch)
            .finish_non_exhaustive()
    }
}

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use futures::future::BoxFuture;
use pl_protocol::{Result, ThreadAttachment};

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

type ToolImageAttachmentWriter =
    dyn Fn(ToolImageAttachmentInput) -> BoxFuture<'static, Result<ThreadAttachment>> + Send + Sync;
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
}

impl AttachmentRuntime {
    pub fn new<W, WFut, L, LFut>(writer: W, loader: L) -> Self
    where
        W: Fn(ToolImageAttachmentInput) -> WFut + Send + Sync + 'static,
        WFut: Future<Output = Result<ThreadAttachment>> + Send + 'static,
        L: Fn(Vec<String>) -> LFut + Send + Sync + 'static,
        LFut: Future<Output = Result<Vec<MaterializedAttachment>>> + Send + 'static,
    {
        Self {
            writer: Arc::new(move |input| Box::pin(writer(input))),
            loader: Arc::new(move |attachment_ids| Box::pin(loader(attachment_ids))),
        }
    }

    /// 持久化一张已经验证和规范化的工具图片。
    ///
    /// # Errors
    ///
    /// 当宿主拒绝当前线程写入，或 blob/metadata 提交失败时返回错误。
    pub async fn write_image(&self, input: ToolImageAttachmentInput) -> Result<ThreadAttachment> {
        (self.writer)(input).await
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

impl fmt::Debug for AttachmentRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AttachmentRuntime")
            .finish_non_exhaustive()
    }
}

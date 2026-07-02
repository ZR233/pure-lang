#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedAttachment {
    pub attachment_id: String,
    pub media_type: String,
    pub filename: Option<String>,
    pub data: String,
    pub byte_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

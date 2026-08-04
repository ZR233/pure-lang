/// Studio 数据库打开失败时需要跨越 runtime/Bridge 边界保留的稳定错误。
#[derive(Debug, thiserror::Error)]
pub enum StudioDatabaseError {
    /// 数据库 schema 来自更高版本，当前应用必须原样保留并拒绝打开。
    #[error("Studio 数据库版本 {found} 高于当前支持版本 {supported}，已保留原数据库")]
    UnsupportedSchema {
        /// 数据库实际 schema 版本。
        found: i64,
        /// 当前应用支持的 schema 版本。
        supported: i64,
    },
    /// SQLite 完整性检查失败；当前应用必须保留现场并拒绝打开。
    #[error("Studio 数据库完整性检查失败：{reason}，已保留原数据库")]
    CorruptDatabase { reason: String },
}

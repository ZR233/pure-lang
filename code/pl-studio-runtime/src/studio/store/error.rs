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
    /// 状态库和历史库必须作为同一 generation 成对存在。
    #[error(
        "Studio 数据库不完整：state_exists={state_exists}, history_exists={history_exists}，已保留现场"
    )]
    IncompleteDatabasePair {
        /// 状态库是否存在。
        state_exists: bool,
        /// 历史库是否存在。
        history_exists: bool,
    },
    /// 数据库缺失配对身份元数据。
    #[error("Studio 数据库缺少 storage_metadata，已保留原数据库")]
    MissingStorageMetadata,
    /// 文件类型或 metadata version 与预期不一致。
    #[error(
        "Studio 数据库身份不匹配：预期 {expected_kind} v{expected_version}，实际 {found_kind} v{found_version}"
    )]
    StorageMetadataMismatch {
        /// 预期数据库类型。
        expected_kind: String,
        /// 实际数据库类型。
        found_kind: String,
        /// 预期 schema version。
        expected_version: i64,
        /// 实际 metadata schema version。
        found_version: i64,
    },
    /// 两个数据库来自不同创建代际。
    #[error(
        "Studio 数据库 generation 不匹配：state={state_generation}, history={history_generation}"
    )]
    GenerationMismatch {
        /// 状态库 generation。
        state_generation: String,
        /// 历史库 generation。
        history_generation: String,
    },
}

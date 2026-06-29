use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Pure-Lang 内置 rust-analyzer 服务器标识
pub(crate) const RUST_ANALYZER_ID: &str = "rust-analyzer";

/// LSP 服务器定义，用于描述如何启动一个 LSP 服务器以及它支持的语言。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LspServerDefinition {
    pub id: String,
    pub display_name: String,
    pub command: String,
    pub args: Vec<String>,
    pub extensions: Vec<String>,
    pub language_ids: Vec<String>,
    pub workspace_root: PathBuf,
}

impl LspServerDefinition {
    /// 根据文件扩展名返回对应的 language ID。
    pub fn language_for_path(&self, path: &std::path::Path) -> Option<&str> {
        let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
        let extension = format!(".{extension}");
        self.extensions
            .iter()
            .position(|candidate| candidate == &extension)
            .and_then(|index| self.language_ids.get(index))
            .map(String::as_str)
    }
}

//! 绑定到单个 workspace 的已解析 server 定义。

use std::path::{Path, PathBuf};

use crate::driver::LspResolvedCommand;
use crate::types::LspQueryOperation;

/// catalog 定义经 driver 解析并绑定 workspace root 后的运行时形态。
#[derive(Debug, Clone)]
pub(crate) struct ResolvedLspServer {
    pub id: String,
    pub display_name: String,
    pub program: String,
    pub args: Vec<String>,
    pub extensions: Vec<String>,
    pub language_ids: Vec<String>,
    pub operations: Vec<LspQueryOperation>,
    pub workspace_root: PathBuf,
}

impl ResolvedLspServer {
    pub(crate) fn command(&self) -> LspResolvedCommand {
        LspResolvedCommand {
            program: self.program.clone(),
            args: self.args.clone(),
        }
    }

    /// 路由校验：该 server 是否声明支持此操作。
    pub(crate) fn supports(&self, operation: LspQueryOperation) -> bool {
        self.operations.contains(&operation)
    }

    /// 与运行态无关的定义指纹；探测/成员合并用它丢弃过期结果。
    pub(crate) fn fingerprint(&self) -> String {
        let operations = self
            .operations
            .iter()
            .map(|operation| operation.as_str())
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.id,
            self.display_name,
            self.program,
            self.args.join(","),
            self.language_ids.join(","),
            operations,
        )
    }

    /// 根据文件扩展名返回对应的 language ID。
    pub(crate) fn language_for_path(&self, path: &Path) -> Option<&str> {
        let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
        let extension = format!(".{extension}");
        self.extensions
            .iter()
            .position(|candidate| candidate == &extension)
            .and_then(|index| self.language_ids.get(index))
            .map(String::as_str)
    }
}

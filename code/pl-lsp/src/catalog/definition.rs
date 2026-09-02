use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::collection::LspCatalogServer;
use super::matching::workspace_matches;
use crate::driver::LspResolvedCommand;
use crate::driver::command::CommandDriver;
use crate::query::LspQueryOperation;

const WORKSPACE_ROOT_PLACEHOLDER: &str = "{workspaceRoot}";

/// catalog 中一条 server 的纯数据定义。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspServerDefinition {
    pub id: String,
    pub display_name: String,
    pub language_ids: Vec<String>,
    pub extensions: Vec<String>,
    pub detection: Vec<String>,
    pub command: LspCommandSpec,
    pub operations: Vec<LspQueryOperation>,
}

impl LspServerDefinition {
    /// 静态检测当前 workspace 是否适用该 server；只做文件系统检查。
    pub fn matches_workspace(&self, workspace_root: &Path) -> bool {
        workspace_matches(&self.detection, workspace_root)
    }

    /// 与 workspace 无关的定义指纹；用于探测/合并结果的新鲜度判断。
    pub fn fingerprint(&self) -> String {
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
            self.language_ids.join(","),
            self.extensions.join(","),
            self.detection.join(","),
            operations,
        )
    }
}

/// command 解析：程序与参数模板，`{workspaceRoot}` 会被渲染为 workspace root。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspCommandSpec {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl LspCommandSpec {
    pub fn render(&self, workspace_root: &Path) -> LspResolvedCommand {
        let root = workspace_root.display().to_string();
        LspResolvedCommand {
            program: self.program.replace(WORKSPACE_ROOT_PLACEHOLDER, &root),
            args: self
                .args
                .iter()
                .map(|arg| arg.replace(WORKSPACE_ROOT_PLACEHOLDER, &root))
                .collect(),
        }
    }
}

/// 用户在 `[lsp.servers.<id>]` 中声明的自定义 server 配置。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspUserServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub language_ids: Vec<String>,
    #[serde(default)]
    pub detection: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub operations: Vec<LspQueryOperation>,
}

impl LspUserServerConfig {
    /// 转换为 catalog 条目：自定义 server 绑定通用命令 driver。
    pub fn to_catalog_server(&self, id: &str) -> LspCatalogServer {
        LspCatalogServer {
            definition: LspServerDefinition {
                id: id.to_string(),
                display_name: self.display_name.clone().unwrap_or_else(|| id.to_string()),
                language_ids: self.language_ids.clone(),
                extensions: self.extensions.clone(),
                detection: self.detection.clone(),
                command: LspCommandSpec {
                    program: self.command.clone(),
                    args: self.args.clone(),
                },
                operations: if self.operations.is_empty() {
                    LspQueryOperation::all().to_vec()
                } else {
                    self.operations.clone()
                },
            },
            driver: Arc::new(CommandDriver::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn command_rendering_substitutes_the_workspace_boundary() {
        let rendered = LspCommandSpec {
            program: "purelang-lsp".to_string(),
            args: vec!["--root".to_string(), "{workspaceRoot}".to_string()],
        }
        .render(Path::new("/tmp/demo"));

        assert_eq!(
            rendered,
            LspResolvedCommand {
                program: "purelang-lsp".to_string(),
                args: vec!["--root".to_string(), "/tmp/demo".to_string()],
            }
        );
    }
}

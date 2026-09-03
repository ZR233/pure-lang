//! 模型目录、provider endpoint、canonical completion 与单模型运行时。
//!
//! 公开 API 按四个稳定域模块组织（见 design/07-model.md 7.2 节）：
//!
//! - [`completion`]：canonical 请求/响应、工具调用、用量与流语义。
//! - [`model`]：模型元数据、能力、可调参数与内置目录。
//! - [`provider`]：endpoint、wire 协议与服务能力声明。
//! - [`runtime`]：单模型运行时入口与会话状态。
//!
//! 消费方通过 `pl_model::<domain>::` 前缀访问类型；crate 根不重导出本 crate 的
//! 自有类型（同一接口只有域级一条 canonical 路径）。各域精确重导出其公共签名中
//! 出现的 `pl-protocol` 类型，错误基础类型（`PureError`/`Result`）在根重导出，
//! 消费方无需为命名完整签名而额外依赖 `pl-protocol`。

pub mod completion;
pub mod model;
pub mod provider;
pub mod runtime;

pub use pl_protocol::{PureError, Result};

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    fn rust_sources(root: &Path) -> Vec<PathBuf> {
        let mut pending = vec![root.to_path_buf()];
        let mut sources = Vec::new();
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    sources.push(path);
                }
            }
        }
        sources
    }

    #[test]
    fn crate_keeps_one_runtime_path_without_obsolete_model_layers() {
        let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut top_level = std::fs::read_dir(&source_root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        top_level.sort();
        assert_eq!(
            top_level,
            ["completion", "lib.rs", "model", "provider", "runtime"]
        );

        let production = ["completion", "model", "provider", "runtime"]
            .into_iter()
            .flat_map(|module| rust_sources(&source_root.join(module)))
            .map(|path| std::fs::read_to_string(path).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            "trait ModelProvider",
            "SharedModelProvider",
            "OpenAiProvider",
            "ModelsManager",
            "create_provider",
            "decode_provider_stream",
            "#[path =",
        ] {
            assert!(
                !production.contains(forbidden),
                "obsolete model architecture symbol reappeared: {forbidden}"
            );
        }
    }

    #[test]
    fn crate_root_exports_only_domain_namespaces_and_error_aliases() {
        let lib_rs =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
                .unwrap();
        let outside_tests = lib_rs.split("#[cfg(test)]").next().unwrap_or_default();
        let allowed = "pub use pl_protocol::{PureError, Result};";
        let stripped = outside_tests.replace(allowed, "");
        assert!(
            !stripped.contains("pub use"),
            "crate root must not re-export own types; use pl_model::<domain>:: paths instead"
        );
    }

    /// 编译期契约：公共签名中出现的 pl-protocol 类型必须能从本 crate 导入，
    /// 消费方只依赖 pl-model 即可命名完整签名；删除任一重导出会使本测试编译失败。
    #[test]
    fn dependency_types_in_public_signatures_stay_importable() {
        fn assert_nameable<T>() {}
        assert_nameable::<crate::completion::AttachmentModality>();
        assert_nameable::<crate::completion::ContentPart>();
        assert_nameable::<crate::completion::InferenceOrchestrationMetrics>();
        assert_nameable::<crate::completion::InferenceTiming>();
        assert_nameable::<crate::completion::Message>();
        assert_nameable::<crate::completion::MessageContent>();
        assert_nameable::<crate::completion::MessageRole>();
        assert_nameable::<crate::completion::ModelContextItem>();
        assert_nameable::<crate::completion::PureError>();
        assert_nameable::<crate::completion::ResponsesContextItem>();
        assert_nameable::<crate::completion::Result<()>>();
        assert_nameable::<crate::completion::TokenUsage>();
        assert_nameable::<crate::completion::ToolCallCaller>();
        assert_nameable::<crate::completion::ToolCallKind>();
        assert_nameable::<crate::completion::ToolSpec>();
        assert_nameable::<crate::completion::WebSearchContextSize>();
        assert_nameable::<crate::completion::WebSearchFilters>();
        assert_nameable::<crate::completion::WebSearchUserLocation>();
        assert_nameable::<crate::provider::HostedWebSearchDialect>();
        assert_nameable::<crate::PureError>();
        assert_nameable::<crate::Result<()>>();
    }
}

//! 模型目录、provider endpoint、canonical completion 与单模型运行时。
//!
//! 公开 API 按四个稳定域模块组织（见 design/07-model.md 7.2 节）：
//!
//! - [`completion`]：canonical 请求/响应、工具调用、用量与流语义。
//! - [`model`]：模型元数据、能力、可调参数与内置目录。
//! - [`provider`]：endpoint、wire 协议与服务能力声明。
//! - [`runtime`]：单模型运行时入口与会话状态。
//!
//! 消费方通过 `pl_model::<domain>::` 前缀访问类型；crate 根不重导出类型，
//! 也不转发 `pl-protocol` 的类型，跨 crate 消费方直接依赖 `pl-protocol`。

pub mod completion;
pub mod model;
pub mod provider;
pub mod runtime;

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
    fn crate_root_exports_only_domain_namespaces() {
        let lib_rs =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
                .unwrap();
        let outside_tests = lib_rs.split("#[cfg(test)]").next().unwrap_or_default();
        assert!(
            !outside_tests.contains("pub use"),
            "crate root must not re-export types; use pl_model::<domain>:: paths instead"
        );
        assert!(
            !outside_tests.contains("pl_protocol"),
            "crate root must not forward pl-protocol types; depend on pl-protocol directly"
        );
    }
}

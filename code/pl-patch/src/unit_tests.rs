//! pl-patch 端到端行为测试：解析、应用与失败摘要。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pretty_assertions::assert_eq;

use crate::apply::apply_patch;
use crate::backend::{PatchBackend, PatchPathDisplay};
use crate::error::{PatchError, PatchResult};
use crate::parse::parse_patch;

#[derive(Debug, Default, Clone)]
struct MemoryBackend {
    files: Arc<Mutex<HashMap<PathBuf, String>>>,
}

impl MemoryBackend {
    fn with_file(path: impl Into<PathBuf>, content: impl Into<String>) -> Self {
        let backend = Self::default();
        backend
            .files
            .lock()
            .unwrap()
            .insert(path.into(), content.into());
        backend
    }

    fn read(&self, path: impl AsRef<Path>) -> Option<String> {
        self.files.lock().unwrap().get(path.as_ref()).cloned()
    }
}

impl PatchPathDisplay for MemoryBackend {
    fn display_path(&self, path: &Path) -> String {
        path.display().to_string()
    }
}

impl PatchBackend for MemoryBackend {
    async fn resolve_existing(&self, path: &str) -> PatchResult<PathBuf> {
        let path = PathBuf::from(path);
        if self.files.lock().unwrap().contains_key(&path) {
            Ok(path)
        } else {
            Err(PatchError::new(format!(
                "failed to resolve path '{}': not found",
                path.display()
            )))
        }
    }

    async fn resolve_for_write(&self, path: &str) -> PatchResult<PathBuf> {
        Ok(PathBuf::from(path))
    }

    async fn reject_symlink_write(&self, _path: &Path) -> PatchResult<()> {
        Ok(())
    }

    async fn ensure_file(&self, path: &Path) -> PatchResult<()> {
        if self.files.lock().unwrap().contains_key(path) {
            Ok(())
        } else {
            Err(PatchError::new(format!(
                "cannot delete '{}': path is not a file",
                path.display()
            )))
        }
    }

    async fn read_to_string(&self, path: &Path) -> PatchResult<String> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| {
                PatchError::new(format!("failed to read '{}': not found", path.display()))
            })
    }

    async fn read_optional_text(&self, path: &Path) -> PatchResult<Option<String>> {
        Ok(self.files.lock().unwrap().get(path).cloned())
    }

    async fn create_parent_dirs(&self, _path: &Path) -> PatchResult<()> {
        Ok(())
    }

    async fn write_text(&self, path: &Path, content: &str) -> PatchResult<()> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), content.to_string());
        Ok(())
    }

    async fn remove_file(&self, path: &Path) -> PatchResult<()> {
        self.files.lock().unwrap().remove(path);
        Ok(())
    }
}

#[test]
fn invalid_header_reports_recovery_guidance() {
    let error = parse_patch("*** Begin Patch\n--- a/file.txt\n*** End Patch").unwrap_err();

    assert!(error.message().contains("unified diff"));
    assert!(error.message().contains("*** Update File:"));
    assert!(
        error
            .message()
            .contains("Recovery: read the target file again")
    );
}

#[tokio::test]
async fn applies_add_then_update_in_order() {
    let backend = MemoryBackend::default();
    let patch = "*** Begin Patch\n*** Add File: notes.txt\n+new\n*** Update File: notes.txt\n@@\n-new\n+newer\n*** End Patch";

    let outcome = apply_patch(patch, &backend).await.unwrap();

    assert_eq!(backend.read("notes.txt"), Some("newer\n".to_string()));
    assert_eq!(
        outcome.summary(&backend),
        "Success. Updated the following files:\nA notes.txt\nM notes.txt\n"
    );
}

#[tokio::test]
async fn unicode_context_uses_normalized_matching() {
    let backend = MemoryBackend::with_file(
        "unicode.txt",
        "import asyncio  # local import \u{2013} avoids top\u{2011}level dep\n",
    );
    let patch = "*** Begin Patch\n*** Update File: unicode.txt\n@@\n-import asyncio  # local import - avoids top-level dep\n+done\n*** End Patch";

    apply_patch(patch, &backend).await.unwrap();

    assert_eq!(backend.read("unicode.txt"), Some("done\n".to_string()));
}

#[tokio::test]
async fn preserved_arb_keys_match_without_overwriting_current_values() {
    let backend = MemoryBackend::with_file(
        "app_zh.arb",
        "{\n  \"settingsModelField\": \"Model\",\n  \"settingsMcpTitle\": \"MCP\"\n}\n",
    );
    let patch = "*** Begin Patch\n*** Update File: app_zh.arb\n@@\n   \"settingsModelField\": \"模型\",\n+  \"settingsReasoningEffortField\": \"推理强度\",\n   \"settingsMcpTitle\": \"MCP\"\n*** End Patch";

    apply_patch(patch, &backend).await.unwrap();

    assert_eq!(
        backend.read("app_zh.arb"),
        Some(
            "{\n  \"settingsModelField\": \"Model\",\n  \"settingsReasoningEffortField\": \"推理强度\",\n  \"settingsMcpTitle\": \"MCP\"\n}\n"
                .to_string()
        )
    );
}

#[tokio::test]
async fn arb_value_replacement_still_requires_the_expected_value() {
    let backend =
        MemoryBackend::with_file("app_zh.arb", "{\n  \"settingsModelField\": \"Model\"\n}\n");
    let patch = "*** Begin Patch\n*** Update File: app_zh.arb\n@@\n-  \"settingsModelField\": \"模型\"\n+  \"settingsModelField\": \"模型名称\"\n*** End Patch";

    let error = apply_patch(patch, &backend).await.unwrap_err();

    assert!(error.message().contains("failed to find expected lines"));
    assert_eq!(
        backend.read("app_zh.arb"),
        Some("{\n  \"settingsModelField\": \"Model\"\n}\n".to_string())
    );
}

#[tokio::test]
async fn json_shaped_text_files_do_not_use_key_matching() {
    let backend = MemoryBackend::with_file(
        "notes.txt",
        "\"settingsModelField\": \"Model\",\n\"settingsMcpTitle\": \"MCP\"\n",
    );
    let patch = "*** Begin Patch\n*** Update File: notes.txt\n@@\n \"settingsModelField\": \"模型\",\n+\"settingsReasoningEffortField\": \"推理强度\",\n \"settingsMcpTitle\": \"MCP\"\n*** End Patch";

    let error = apply_patch(patch, &backend).await.unwrap_err();

    assert!(error.message().contains("failed to find expected lines"));
    assert_eq!(
        backend.read("notes.txt"),
        Some("\"settingsModelField\": \"Model\",\n\"settingsMcpTitle\": \"MCP\"\n".to_string())
    );
}

#[tokio::test]
async fn failure_reports_applied_changes() {
    let backend = MemoryBackend::default();
    let patch = "*** Begin Patch\n*** Add File: created.txt\n+hello\n*** Update File: missing.txt\n@@\n-old\n+new\n*** End Patch";

    let error = apply_patch(patch, &backend).await.unwrap_err();

    assert!(
        error
            .message()
            .contains("failed to resolve path 'missing.txt'")
    );
    assert!(error.message().contains("Changes applied before failure"));
    assert!(!error.message().contains("Committed changes"));
    assert!(error.message().contains("A created.txt"));
    assert_eq!(backend.read("created.txt"), Some("hello\n".to_string()));
}

//! apply_patch 的文件系统抽象。

use std::future::Future;
use std::path::{Path, PathBuf};

use crate::error::PatchResult;

/// 为 patch 结果提供面向用户的路径展示。
///
/// 调用方可用它把真实路径、容器路径或 workspace 相对路径转换为产品需要的显示格式。
pub trait PatchPathDisplay {
    fn display_path(&self, path: &Path) -> String;
}

/// apply_patch 的文件系统后端。
///
/// 该 trait 只负责路径解析、读写和删除，patch 语法、上下文匹配和失败摘要由
/// `pl-patch` 统一处理。实现方应在解析阶段完成产品自己的安全策略，例如 workspace
/// 边界、符号链接拒绝、Docker 容器路径映射等。
pub trait PatchBackend: PatchPathDisplay {
    fn resolve_existing<'a>(
        &'a self,
        path: &'a str,
    ) -> impl Future<Output = PatchResult<PathBuf>> + Send + 'a;

    fn resolve_for_write<'a>(
        &'a self,
        path: &'a str,
    ) -> impl Future<Output = PatchResult<PathBuf>> + Send + 'a;

    fn reject_symlink_write<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Future<Output = PatchResult<()>> + Send + 'a;

    fn ensure_file<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Future<Output = PatchResult<()>> + Send + 'a;

    fn read_to_string<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Future<Output = PatchResult<String>> + Send + 'a;

    fn read_optional_text<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Future<Output = PatchResult<Option<String>>> + Send + 'a;

    fn create_parent_dirs<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Future<Output = PatchResult<()>> + Send + 'a;

    fn write_text<'a>(
        &'a self,
        path: &'a Path,
        content: &'a str,
    ) -> impl Future<Output = PatchResult<()>> + Send + 'a;

    fn remove_file<'a>(
        &'a self,
        path: &'a Path,
    ) -> impl Future<Output = PatchResult<()>> + Send + 'a;
}

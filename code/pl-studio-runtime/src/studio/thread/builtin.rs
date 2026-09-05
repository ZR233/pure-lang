//! 随 Studio 二进制发布的内置 Thread Mode 注册入口。

use super::{simple, task};
use std::sync::Arc;

use anyhow::{Context, Result};
use pl_core::{
    ThreadModeManager, ThreadModeRegistrySnapshot, ThreadModeSource, ThreadModeSourceId,
    ThreadModeSourceKind,
};

const BUILTIN_SOURCE_ID: &str = "studio.builtin";

pub(crate) fn register_builtins(
    manager: &ThreadModeManager,
) -> Result<Arc<ThreadModeRegistrySnapshot>> {
    let registrations = [simple::REGISTRATION, task::REGISTRATION]
        .into_iter()
        .map(|registration| registration.to_registration())
        .collect::<Result<Vec<_>, _>>()?;
    manager
        .replace_source(
            ThreadModeSource {
                id: ThreadModeSourceId::new(BUILTIN_SOURCE_ID)?,
                kind: ThreadModeSourceKind::Builtin,
            },
            registrations,
        )
        .context("failed to register built-in Thread Modes")
}

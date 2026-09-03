//! Thread Mode 的稳定协议类型。

use std::fmt;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Thread 所选择的 Mode ID。
///
/// ID 的 wire 形式固定为 `mode.<name>`。内置与未来外部注册的 Mode 使用同一类型。
#[derive(Clone)]
pub struct ThreadModeId(String);

impl ThreadModeId {
    pub const SIMPLE: &'static str = "mode.simple";
    pub const TASK: &'static str = "mode.task";

    pub fn new(value: impl Into<String>) -> Result<Self, crate::UnknownLabelError> {
        let canonical = value.into();
        let name = canonical.strip_prefix("mode.").unwrap_or_default();
        if name.is_empty()
            || canonical.len() > 64
            || !name.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
            })
        {
            return Err(crate::UnknownLabelError::new("ThreadModeId", canonical));
        }
        Ok(Self(canonical))
    }

    pub fn simple() -> Self {
        Self(Self::SIMPLE.to_string())
    }

    pub fn task() -> Self {
        Self(Self::TASK.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn label(&self) -> &str {
        self.as_str()
    }

    pub fn from_label(label: &str) -> Result<Self, crate::UnknownLabelError> {
        Self::new(label)
    }
}

impl Default for ThreadModeId {
    fn default() -> Self {
        Self::simple()
    }
}

impl fmt::Debug for ThreadModeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ThreadModeId")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for ThreadModeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialEq for ThreadModeId {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for ThreadModeId {}

impl PartialOrd for ThreadModeId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ThreadModeId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for ThreadModeId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl Serialize for ThreadModeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ThreadModeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// GUI 与其他只读消费者可见的一项 Mode 元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadModeDescriptor {
    pub id: ThreadModeId,
    pub display_name: String,
    pub description: String,
    pub order: u32,
    pub has_workflow: bool,
}

/// 某一时刻已注册 Mode 的不可变目录投影。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadModeCatalogSnapshot {
    pub revision: u64,
    #[serde(default)]
    pub modes: Vec<ThreadModeDescriptor>,
}

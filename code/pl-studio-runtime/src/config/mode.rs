use std::borrow::Cow;
use std::fmt;
use std::hash::{Hash, Hasher};

use pl_protocol::{ModeId, ThreadMode, UnknownLabelError};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Studio 动态模式 ID；完整 `mode.*` Skill 名称就是持久化和 wire 身份。
#[derive(Clone)]
pub struct StudioModeId(Cow<'static, str>);

/// Studio API 使用的动态模式 ID 名称。
pub type StudioMode = StudioModeId;

impl StudioModeId {
    pub const fn simple() -> Self {
        Self(Cow::Borrowed(ModeId::SIMPLE))
    }

    pub const fn task() -> Self {
        Self(Cow::Borrowed(ModeId::TASK))
    }

    pub fn new(value: impl Into<String>) -> Result<Self, UnknownLabelError> {
        let mode = ModeId::new(value)?;
        Ok(Self(match mode.as_str() {
            ModeId::SIMPLE => Cow::Borrowed(ModeId::SIMPLE),
            ModeId::TASK => Cow::Borrowed(ModeId::TASK),
            custom => Cow::Owned(custom.to_string()),
        }))
    }

    pub fn label(&self) -> &str {
        self.0.as_ref()
    }

    pub fn from_label(label: &str) -> Result<Self, UnknownLabelError> {
        Self::new(label)
    }

    /// 根 Agent 使用统一会话提示，模式差异由预加载 Mode Skill 提供。
    pub const fn root_instructions() -> &'static str {
        include_str!("../prompts/unified_root.md")
    }
}

impl Default for StudioModeId {
    fn default() -> Self {
        Self::simple()
    }
}

impl fmt::Debug for StudioModeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("StudioModeId")
            .field(&self.label())
            .finish()
    }
}

impl fmt::Display for StudioModeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

impl PartialEq for StudioModeId {
    fn eq(&self, other: &Self) -> bool {
        self.label() == other.label()
    }
}

impl Eq for StudioModeId {}

impl Hash for StudioModeId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.label().hash(state);
    }
}

impl Serialize for StudioModeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.label())
    }
}

impl<'de> Deserialize<'de> for StudioModeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl From<StudioModeId> for ThreadMode {
    fn from(value: StudioModeId) -> Self {
        ModeId::new(value.label()).expect("validated Studio mode must remain valid")
    }
}

impl From<ThreadMode> for StudioModeId {
    fn from(value: ThreadMode) -> Self {
        Self::new(value.as_str()).expect("validated Thread mode must remain valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_and_custom_modes_round_trip_with_full_skill_names() {
        for mode in [
            StudioMode::simple(),
            StudioMode::task(),
            StudioMode::new("mode.release").unwrap(),
        ] {
            assert_eq!(StudioMode::from_label(mode.label()).unwrap(), mode);
            assert!(mode.label().starts_with("mode."));
        }
        assert!(StudioMode::from_label("legacy").is_err());
    }

    #[test]
    fn root_prompt_delegates_behavior_to_the_mode_skill() {
        let instructions = StudioMode::root_instructions();
        assert!(instructions.contains("Mode Skill"));
        assert!(!instructions.contains("mode.simple"));
        assert!(!instructions.contains("mode.task"));
    }
}

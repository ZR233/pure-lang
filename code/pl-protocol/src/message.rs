use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

pub const TOOL_CALLS_METADATA_KEY: &str = "tool_calls";
pub const TOOL_CALL_ID_METADATA_KEY: &str = "tool_call_id";
pub const TOOL_CALL_CALL_ID_METADATA_KEY: &str = "tool_call_call_id";
pub const TOOL_NAME_METADATA_KEY: &str = "tool_name";
pub const TOOL_CALL_KIND_METADATA_KEY: &str = "tool_call_kind";
pub const TOOL_CALL_ARGUMENTS_METADATA_KEY: &str = "tool_call_arguments";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub role: MessageRole,
    pub content: MessageContent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageContent {
    Text(String),
    MultiPart(Vec<ContentPart>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ContentPart {
    Text {
        text: String,
    },
    Image {
        source: ImageSource,
        media_type: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ImageSource {
    Attachment { attachment_id: String },
    InlineBase64 { data: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ToolCallKind {
    Function,
    Custom,
}

impl ToolCallKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Custom => "custom",
        }
    }

    fn from_metadata_value(value: &str) -> std::result::Result<Self, String> {
        match value {
            "function" => Ok(Self::Function),
            "custom" => Ok(Self::Custom),
            other => Err(format!("unknown tool_call_kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolMetadataCompatibility {
    Strict,
    LegacyMissingKindAsFunction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCallHistoryMetadata {
    pub tool_calls_json: String,
}

impl ToolCallHistoryMetadata {
    pub fn new(tool_calls_json: String) -> Self {
        Self { tool_calls_json }
    }

    pub fn insert_into(self, metadata: &mut HashMap<String, String>) {
        metadata.insert(TOOL_CALLS_METADATA_KEY.to_string(), self.tool_calls_json);
    }

    pub fn from_metadata(metadata: &HashMap<String, String>) -> Option<Self> {
        metadata
            .get(TOOL_CALLS_METADATA_KEY)
            .cloned()
            .map(Self::new)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResultMetadata {
    pub tool_call_id: String,
    pub tool_call_call_id: Option<String>,
    pub tool_name: String,
    pub tool_call_kind: ToolCallKind,
    pub tool_call_arguments: Option<String>,
}

impl ToolResultMetadata {
    pub fn new(
        tool_call_id: String,
        tool_call_call_id: Option<String>,
        tool_name: String,
        tool_call_kind: ToolCallKind,
        tool_call_arguments: String,
    ) -> Self {
        Self {
            tool_call_id,
            tool_call_call_id,
            tool_name,
            tool_call_kind,
            tool_call_arguments: Some(tool_call_arguments),
        }
    }

    pub fn insert_into(self, metadata: &mut HashMap<String, String>) {
        if let Some(call_id) = self.tool_call_call_id {
            metadata.insert(TOOL_CALL_CALL_ID_METADATA_KEY.to_string(), call_id);
        }
        metadata.insert(TOOL_CALL_ID_METADATA_KEY.to_string(), self.tool_call_id);
        metadata.insert(TOOL_NAME_METADATA_KEY.to_string(), self.tool_name);
        metadata.insert(
            TOOL_CALL_KIND_METADATA_KEY.to_string(),
            self.tool_call_kind.as_str().to_string(),
        );
        if let Some(arguments) = self.tool_call_arguments {
            metadata.insert(TOOL_CALL_ARGUMENTS_METADATA_KEY.to_string(), arguments);
        }
    }

    pub fn from_metadata(
        metadata: &HashMap<String, String>,
        compatibility: ToolMetadataCompatibility,
    ) -> std::result::Result<Self, String> {
        let tool_call_id = metadata
            .get(TOOL_CALL_ID_METADATA_KEY)
            .filter(|value| !value.is_empty())
            .cloned()
            .ok_or_else(|| "tool result metadata missing tool_call_id".to_string())?;
        let tool_call_kind = match metadata
            .get(TOOL_CALL_KIND_METADATA_KEY)
            .map(String::as_str)
        {
            Some(value) => ToolCallKind::from_metadata_value(value)?,
            None if compatibility == ToolMetadataCompatibility::LegacyMissingKindAsFunction => {
                ToolCallKind::Function
            }
            None => return Err("tool result metadata missing tool_call_kind".to_string()),
        };

        Ok(Self {
            tool_call_id,
            tool_call_call_id: metadata
                .get(TOOL_CALL_CALL_ID_METADATA_KEY)
                .filter(|value| !value.is_empty())
                .cloned(),
            tool_name: metadata
                .get(TOOL_NAME_METADATA_KEY)
                .cloned()
                .unwrap_or_default(),
            tool_call_kind,
            tool_call_arguments: metadata.get(TOOL_CALL_ARGUMENTS_METADATA_KEY).cloned(),
        })
    }
}

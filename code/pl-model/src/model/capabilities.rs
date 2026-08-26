use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    #[serde(default)]
    pub streaming: bool,
    #[serde(default)]
    pub temperature: bool,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub web_search: bool,
    #[serde(default)]
    pub input: Vec<ModelModality>,
    #[serde(default)]
    pub output: Vec<ModelModality>,
    #[serde(default)]
    pub tools: ToolCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interleaved: Option<ReasoningInterleaved>,
    #[serde(default)]
    pub prompt_cache: PromptCacheModelCapabilities,
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self::text_only()
    }
}

impl ModelCapabilities {
    pub fn text_only() -> Self {
        Self {
            streaming: true,
            temperature: true,
            reasoning: false,
            web_search: false,
            input: vec![ModelModality::Text],
            output: vec![ModelModality::Text],
            tools: ToolCapabilities {
                function_calling: true,
                parallel_tool_calls: false,
                custom_tools: false,
                freeform_tools: false,
                programmatic_tool_calling: false,
            },
            interleaved: None,
            prompt_cache: PromptCacheModelCapabilities::default(),
        }
    }

    pub fn supports_streaming(&self) -> bool {
        self.streaming
    }

    pub fn supports_temperature(&self) -> bool {
        self.temperature
    }

    pub fn supports_reasoning(&self) -> bool {
        self.reasoning
    }

    pub fn supports_web_search(&self) -> bool {
        self.web_search
    }

    pub fn supports_function_calling(&self) -> bool {
        self.tools.function_calling
    }

    pub fn supports_parallel_tool_calls(&self) -> bool {
        self.tools.parallel_tool_calls
    }

    pub fn supports_custom_tools(&self) -> bool {
        self.tools.custom_tools
    }

    pub fn supports_freeform_tools(&self) -> bool {
        self.tools.freeform_tools
    }

    pub fn supports_programmatic_tool_calling(&self) -> bool {
        self.tools.programmatic_tool_calling
    }

    pub fn supports_input_modality(&self, modality: ModelModality) -> bool {
        self.input.contains(&modality)
    }

    pub fn supports_output_modality(&self, modality: ModelModality) -> bool {
        self.output.contains(&modality)
    }

    pub fn with_native_custom_tools(mut self, native_custom_tools: bool) -> Self {
        if !native_custom_tools {
            self.tools.custom_tools = false;
            self.tools.freeform_tools = false;
        }
        self
    }
}

/// 模型可报告的提示词缓存 usage 能力。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptCacheModelCapabilities {
    #[serde(default)]
    pub cache_write_tokens: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelModality {
    Text,
    Image,
    Audio,
    Video,
    Pdf,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolCapabilities {
    #[serde(default)]
    pub function_calling: bool,
    #[serde(default)]
    pub parallel_tool_calls: bool,
    #[serde(default)]
    pub custom_tools: bool,
    #[serde(default)]
    pub freeform_tools: bool,
    #[serde(default)]
    pub programmatic_tool_calling: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningInterleaved {
    pub field: ReasoningInterleavedField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningInterleavedField {
    Reasoning,
    ReasoningContent,
    ReasoningDetails,
}

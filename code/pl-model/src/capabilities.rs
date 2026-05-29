use serde::Deserialize;
use serde::Serialize;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ModelCapabilities: u32 {
        const STREAMING             = 0b00000001;
        const FUNCTION_CALLING      = 0b00000010;
        const VISION                = 0b00000100;
        const PARALLEL_TOOL_CALLS   = 0b00001000;
        const REASONING             = 0b00010000;
        const WEB_SEARCH            = 0b00100000;
        const CUSTOM_TOOLS          = 0b01000000;
        const FREEFORM_TOOLS        = 0b10000000;
    }
}

impl Serialize for ModelCapabilities {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let flags: Vec<&str> = [
            (Self::STREAMING, "STREAMING"),
            (Self::FUNCTION_CALLING, "FUNCTION_CALLING"),
            (Self::VISION, "VISION"),
            (Self::PARALLEL_TOOL_CALLS, "PARALLEL_TOOL_CALLS"),
            (Self::REASONING, "REASONING"),
            (Self::WEB_SEARCH, "WEB_SEARCH"),
            (Self::CUSTOM_TOOLS, "CUSTOM_TOOLS"),
            (Self::FREEFORM_TOOLS, "FREEFORM_TOOLS"),
        ]
        .iter()
        .filter(|(flag, _)| self.contains(*flag))
        .map(|(_, name)| *name)
        .collect();
        flags.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ModelCapabilities {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let flags: Vec<String> = Vec::deserialize(deserializer)?;
        let mut caps = Self::empty();
        for flag in &flags {
            match flag.as_str() {
                "STREAMING" => caps |= Self::STREAMING,
                "FUNCTION_CALLING" => caps |= Self::FUNCTION_CALLING,
                "VISION" => caps |= Self::VISION,
                "PARALLEL_TOOL_CALLS" => caps |= Self::PARALLEL_TOOL_CALLS,
                "REASONING" => caps |= Self::REASONING,
                "WEB_SEARCH" => caps |= Self::WEB_SEARCH,
                "CUSTOM_TOOLS" => caps |= Self::CUSTOM_TOOLS,
                "FREEFORM_TOOLS" => caps |= Self::FREEFORM_TOOLS,
                _ => {}
            }
        }
        Ok(caps)
    }
}

impl ModelCapabilities {
    pub fn supports_streaming(self) -> bool {
        self.contains(Self::STREAMING)
    }

    pub fn supports_function_calling(self) -> bool {
        self.contains(Self::FUNCTION_CALLING)
    }

    pub fn supports_parallel_tool_calls(self) -> bool {
        self.contains(Self::PARALLEL_TOOL_CALLS)
    }

    pub fn supports_custom_tools(self) -> bool {
        self.contains(Self::CUSTOM_TOOLS)
    }

    pub fn supports_freeform_tools(self) -> bool {
        self.contains(Self::FREEFORM_TOOLS)
    }
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ProviderCapabilities: u32 {
        const STREAMING             = 0b00000001;
        const FUNCTION_CALLING      = 0b00000010;
        const VISION                = 0b00000100;
        const PARALLEL_TOOL_CALLS   = 0b00001000;
    }
}

impl ProviderCapabilities {
    pub fn supports_parallel_tool_calls(self) -> bool {
        self.contains(Self::PARALLEL_TOOL_CALLS)
    }
}

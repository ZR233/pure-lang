//! Declarative projection from streamed function arguments to timeline parts.

/// A frozen, provider-independent projection attached to one tool definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolInputTraceProjection {
    /// Decode one JSON string field as Markdown plan content.
    PlanMarkdown {
        content_field: String,
        discriminator: Option<ToolInputTraceDiscriminator>,
    },
}

impl ToolInputTraceProjection {
    pub fn plan_markdown(content_field: impl Into<String>) -> Self {
        Self::PlanMarkdown {
            content_field: content_field.into(),
            discriminator: None,
        }
    }

    pub fn conditional_plan_markdown(
        content_field: impl Into<String>,
        discriminator_field: impl Into<String>,
        discriminator_value: impl Into<String>,
    ) -> Self {
        Self::PlanMarkdown {
            content_field: content_field.into(),
            discriminator: Some(ToolInputTraceDiscriminator {
                field: discriminator_field.into(),
                value: discriminator_value.into(),
            }),
        }
    }
}

/// Exact top-level JSON string match required before a projection becomes visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInputTraceDiscriminator {
    pub field: String,
    pub value: String,
}

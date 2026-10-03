//! Versioned bundled definitions. Model records retain the existing ModelInfo serde contract.
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::ModelInfo;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundledModelDefinition {
    pub schema_version: u32,
    pub catalog: String,
    pub suggested_model: String,
    /// Initial route recommendation, independent of the model's weakest default effort.
    pub suggested_effort: Option<String>,
    pub models: Vec<ModelInfo>,
}

/// A binary/packaging defect, never a damaged user configuration.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelDefinitionError {
    #[error("bundled model definition could not be decoded")]
    Decode,
    #[error("bundled model definition has an unsupported schema or identity")]
    Identity,
    #[error("bundled model definition contains invalid model declarations")]
    Declaration,
    #[error("bundled model definition has an invalid recommendation")]
    Recommendation,
    #[error("no bundled JSON definition for this catalog")]
    Unsupported,
}

impl From<ModelDefinitionError> for pl_protocol::PureError {
    fn from(error: ModelDefinitionError) -> Self {
        // Provider/internal failure must not enter Studio's PersistentData reset path.
        Self::provider_failure(pl_protocol::ProviderFailure {
            context: Default::default(),
            kind: pl_protocol::ProviderFailureKind::Unknown,
            code: None,
            http_status: None,
            message: error.to_string(),
            retry: pl_protocol::RetryDisposition::Permanent,
        })
    }
}

impl BundledModelDefinition {
    /// Decode a versioned definition and validate every record and recommendation.
    /// # Errors
    /// Returns a packaging error, not a user configuration error.
    pub fn parse(json: &str, catalog: &str) -> Result<Self, ModelDefinitionError> {
        let definition: Self =
            serde_json::from_str(json).map_err(|_| ModelDefinitionError::Decode)?;
        if definition.schema_version != 1 || definition.catalog != catalog {
            return Err(ModelDefinitionError::Identity);
        }
        validate_inventory(&definition.models).map_err(|_| ModelDefinitionError::Declaration)?;
        let suggested = definition
            .models
            .iter()
            .find(|m| m.slug == definition.suggested_model)
            .ok_or(ModelDefinitionError::Recommendation)?;
        let supported_efforts = suggested.supported_efforts();
        let valid_recommendation = match &definition.suggested_effort {
            Some(effort) => supported_efforts.contains(effort),
            None => supported_efforts.is_empty(),
        };
        if !valid_recommendation {
            return Err(ModelDefinitionError::Recommendation);
        }
        Ok(definition)
    }
}

/// Loads the embedded vendor definition; no filesystem or user configuration IO.
pub fn bundled_model_definition(
    catalog: &str,
) -> Result<BundledModelDefinition, ModelDefinitionError> {
    let json = match catalog {
        "openai" => include_str!("../../assets/models/openai/model.json"),
        "deepseek" => include_str!("../../assets/models/deepseek/model.json"),
        _ => return Err(ModelDefinitionError::Unsupported),
    };
    BundledModelDefinition::parse(json, catalog)
}

pub(crate) fn validate_inventory(models: &[ModelInfo]) -> Result<(), ()> {
    let mut slugs = std::collections::BTreeSet::new();
    for model in models {
        if model.slug.trim().is_empty() || !slugs.insert(&model.slug) {
            return Err(());
        }
        model.validate().map_err(|_| ())?;
    }
    Ok(())
}

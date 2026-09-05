//! Concrete clients retain native capabilities while sharing one invocation runner.
use super::{ProviderAdapterKind, ProviderEndpoint};
use crate::model::ModelInfo;
use crate::runtime::InvocationRunner;
use pl_protocol::Result;

/// Runtime route with statically named native clients. Matching exposes their concrete APIs.
#[derive(Debug, Clone)]
pub enum ProviderClient {
    OpenAi(super::openai::OpenAiClient),
    DeepSeek(super::deepseek::DeepSeekClient),
    Zhipu(super::zhipu::ZhipuClient),
    MiMo(super::mimo::MiMoClient),
    OpenAiCompatible(super::compatible::CompatibleClient),
}

impl ProviderClient {
    pub(crate) fn new(id: String, endpoint: ProviderEndpoint, model: ModelInfo) -> Result<Self> {
        let kind = endpoint.adapter;
        let runner = InvocationRunner::new_with_provider_id(id, endpoint, model)?;
        Ok(match kind {
            ProviderAdapterKind::OpenAi => Self::OpenAi(super::openai::OpenAiClient { runner }),
            ProviderAdapterKind::DeepSeek => {
                Self::DeepSeek(super::deepseek::DeepSeekClient { runner })
            }
            ProviderAdapterKind::Zhipu => Self::Zhipu(super::zhipu::ZhipuClient { runner }),
            ProviderAdapterKind::MiMo => Self::MiMo(super::mimo::MiMoClient { runner }),
            ProviderAdapterKind::OpenAiCompatible => {
                Self::OpenAiCompatible(super::compatible::CompatibleClient { runner })
            }
        })
    }

    pub(crate) fn runner(&self) -> &InvocationRunner {
        match self {
            Self::OpenAi(client) => &client.runner,
            Self::DeepSeek(client) => &client.runner,
            Self::Zhipu(client) => &client.runner,
            Self::MiMo(client) => &client.runner,
            Self::OpenAiCompatible(client) => &client.runner,
        }
    }

    pub(crate) fn runner_mut(&mut self) -> &mut InvocationRunner {
        match self {
            Self::OpenAi(client) => &mut client.runner,
            Self::DeepSeek(client) => &mut client.runner,
            Self::Zhipu(client) => &mut client.runner,
            Self::MiMo(client) => &mut client.runner,
            Self::OpenAiCompatible(client) => &mut client.runner,
        }
    }
}

pub(super) fn native_body(
    options: impl serde::Serialize,
) -> Result<serde_json::Map<String, serde_json::Value>> {
    let value = serde_json::to_value(options)?;
    match value {
        serde_json::Value::Object(body) => Ok(body),
        _ => Err(pl_protocol::PureError::Protocol(
            "native request options must be an object".into(),
        )),
    }
}

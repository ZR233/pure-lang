//! Concrete clients retain native capabilities while sharing one invocation runner.
use super::{ProviderAdapterKind, ProviderEndpoint};
use crate::runtime::InvocationRunner;
use pl_protocol::Result;

/// Runtime route with statically named native clients. Matching exposes their concrete APIs.
#[derive(Debug, Clone)]
pub enum ProviderClient<'a> {
    OpenAi(super::openai::OpenAiClient<'a>),
    DeepSeek(super::deepseek::DeepSeekClient<'a>),
    Zhipu(super::zhipu::ZhipuClient<'a>),
    MiMo(super::mimo::MiMoClient<'a>),
    OpenAiCompatible(super::compatible::CompatibleClient<'a>),
}

impl<'a> ProviderClient<'a> {
    pub(crate) fn new(endpoint: &ProviderEndpoint, runner: &'a InvocationRunner) -> Self {
        let kind = endpoint.adapter;
        match kind {
            ProviderAdapterKind::OpenAi => Self::OpenAi(super::openai::OpenAiClient { runner }),
            ProviderAdapterKind::DeepSeek => {
                Self::DeepSeek(super::deepseek::DeepSeekClient { runner })
            }
            ProviderAdapterKind::Zhipu => Self::Zhipu(super::zhipu::ZhipuClient { runner }),
            ProviderAdapterKind::MiMo => Self::MiMo(super::mimo::MiMoClient { runner }),
            ProviderAdapterKind::OpenAiCompatible => {
                Self::OpenAiCompatible(super::compatible::CompatibleClient { runner })
            }
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

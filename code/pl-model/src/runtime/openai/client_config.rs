//! `async-openai` 客户端的 endpoint 配置适配。
//!
//! 把 provider endpoint、模型 headers 与凭证投影为 `async_openai::config::Config`，
//! 第三方类型只停留在本边界文件内。

use std::collections::HashMap;

use async_openai::config::Config;
use pl_protocol::{PureError, Result};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use secrecy::SecretString;

#[derive(Debug, Clone)]
pub(crate) struct PureOpenAiConfig {
    api_base: String,
    api_key: SecretString,
    bearer_token: Option<String>,
    custom_headers: HeaderMap,
}

impl PureOpenAiConfig {
    pub(crate) fn new(
        api_base: String,
        bearer_token: Option<String>,
        http_headers: Option<&HashMap<String, String>>,
        model_headers: &HashMap<String, String>,
    ) -> Result<Self> {
        let mut custom_headers = HeaderMap::new();
        if let Some(headers) = http_headers {
            for (key, value) in headers {
                insert_header(&mut custom_headers, key, value)?;
            }
        }
        for (key, value) in model_headers {
            insert_header(&mut custom_headers, key, value)?;
        }

        Ok(Self {
            api_base,
            api_key: bearer_token.clone().unwrap_or_default().into(),
            bearer_token,
            custom_headers,
        })
    }
}

fn insert_header(headers: &mut HeaderMap, key: &str, value: &str) -> Result<()> {
    let name = HeaderName::from_bytes(key.as_bytes())
        .map_err(|error| PureError::HttpError(error.to_string()))?;
    let value =
        HeaderValue::from_str(value).map_err(|error| PureError::HttpError(error.to_string()))?;
    headers.insert(name, value);
    Ok(())
}

impl Config for PureOpenAiConfig {
    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(token) = &self.bearer_token
            && let Ok(value) = HeaderValue::from_str(&format!("Bearer {token}"))
        {
            headers.insert(AUTHORIZATION, value);
        }

        for (key, value) in &self.custom_headers {
            headers.insert(key, value.clone());
        }

        headers
    }

    fn url(&self, path: &str) -> String {
        let base = &self.api_base;
        format!("{base}{path}")
    }

    fn query(&self) -> Vec<(&str, &str)> {
        Vec::new()
    }

    fn api_base(&self) -> &str {
        &self.api_base
    }

    fn api_key(&self) -> &SecretString {
        &self.api_key
    }
}

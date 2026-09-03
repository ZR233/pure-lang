//! 默认模型目录，按 provider 家族文件组织，用 `ModelFamily` 预设复用共享元数据。
//!
//! 参见 design/07-model.md 7.8 / 7.9 节。同 provider 的模型共享 capabilities、
//! truncation_policy、effort 参数声明（`ModelParameter`）和 base body，具体模型
//! 仅以 [`ModelInstanceSpec`] 差异字段从 family 派生。各 provider 的家族预设、
//! 实例数据与能力矩阵放在对应的 `deepseek` / `openai` / `mimo` / `zhipu` 子文件，
//! 本模块只保留目录编排与跨家族共享的 media/wire helper。

mod deepseek;
mod mimo;
mod openai;
mod zhipu;

use serde_json::Value;

use crate::model::capabilities::ModelModality;
use crate::model::info::{
    MediaRepresentation, MediaWireFormat, ModelInfo, ModelMediaInputProfile, ModelRequestProfile,
};
use crate::model::parameter::{ParameterWire, WireAssignment};

pub use deepseek::deepseek_default_model_slugs;
pub use mimo::mimo_default_model_slugs;
pub use openai::openai_default_model_slugs;
pub use zhipu::zhipu_default_model_slugs;

/// 内建默认模型目录；顺序固定为 DeepSeek → OpenAI → MiMo → Zhipu。
pub fn default_models() -> Vec<ModelInfo> {
    let mut models = deepseek::models();
    models.extend(openai::models());
    models.extend(mimo::models());
    models.extend(zhipu::models());
    models
}

impl ModelRequestProfile {
    /// 覆盖 profile 的图片媒体表示顺序。
    fn with_image_media(mut self, wire: MediaWireFormat, remote_url_first: bool) -> Self {
        self.media = image_media_profiles(wire, remote_url_first);
        self
    }
}

/// 构造图片模态的媒体表示 profile。
///
/// `remote_url_first` 控制首次发送是否优先 RemoteUrl（重放固定使用 DataUrl）。
fn image_media_profiles(
    wire: MediaWireFormat,
    remote_url_first: bool,
) -> Vec<ModelMediaInputProfile> {
    let first_send = if remote_url_first {
        vec![MediaRepresentation::RemoteUrl, MediaRepresentation::DataUrl]
    } else {
        vec![MediaRepresentation::DataUrl]
    };
    vec![ModelMediaInputProfile {
        modality: ModelModality::Image,
        wire,
        first_send,
        replay: vec![MediaRepresentation::DataUrl],
    }]
}

/// 构造「把单个字段设为字符串值」的 wire。
fn wire_set_one(path: &str, value: &str) -> ParameterWire {
    ParameterWire {
        set: vec![WireAssignment {
            path: path.to_string(),
            value: Value::String(value.to_string()),
        }],
        remove: Vec::new(),
    }
}

#[cfg(test)]
mod unit_tests;

//! 模型目录、provider endpoint、canonical completion 与单模型运行时。
//!
//! 公开 API 按稳定域模块组织（见 design/06-model.md 6.2 节）：
//!
//! - [`completion`]：canonical 请求/响应、工具调用、用量与流语义。
//! - [`config`]：产品无关的 provider 配置与路由值对象。
//! - [`model`]：模型元数据、能力、可调参数与内置目录。
//! - [`provider`]：endpoint、wire 协议与服务能力声明。
//! - [`runtime`]：单模型运行时入口与会话状态。
//!
//! 消费方通过 `pl_model::<domain>::` 前缀访问类型；crate 根不重导出本 crate 的
//! 自有类型（同一接口只有域级一条 canonical 路径）。各域精确重导出其公共签名中
//! 出现的 `pl-protocol` 类型，错误基础类型（`PureError`/`Result`）在根重导出，
//! 消费方无需为命名完整签名而额外依赖 `pl-protocol`。

pub mod completion;
pub mod config;
pub mod model;
pub mod provider;
pub mod runtime;

pub use pl_protocol::{PureError, Result};

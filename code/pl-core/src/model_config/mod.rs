//! 产品无关的模型、Provider 与动态角色路由配置。

mod catalog;
mod id;
mod provider;
mod route;

#[cfg(test)]
mod tests;

pub use catalog::*;
pub use id::*;
pub use provider::*;
pub use route::*;

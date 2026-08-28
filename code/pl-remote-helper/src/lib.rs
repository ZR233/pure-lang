//! Pure SSH remote helper 的最小 stdio 服务。

mod codec;
mod path;
mod server;

pub use server::run_stdio;

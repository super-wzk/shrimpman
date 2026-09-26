//! 服务进程共用的配置、日志、退出信号与 TCP 连接生命周期。
//!
//! 本 crate 只处理进程基础设施，不依赖领域模型、服务发现或业务处理器。

#![warn(unreachable_pub)]

mod config;
mod server;
mod signal;

pub use config::{init_tracing, load_config};
pub use server::serve_tcp;
pub use signal::shutdown_signal;

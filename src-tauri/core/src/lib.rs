//! FilePilot 核心库：规则引擎、文件监控、dry-run 预览、操作日志与撤销。
//! 不依赖 Tauri，可独立单元测试。

pub mod engine;
pub mod oplog;
pub mod rules;
pub mod store;
pub mod watcher;

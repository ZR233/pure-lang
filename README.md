# Pure-Lang

自然语言编译器 — 将用户的自然语言需求整理为可执行导向的编译计划、代码生成意图和后续动作建议。

## 架构

```text
pure-studio            Tauri 2 桌面应用：React UI、命令桥接、事件推送
    │
pl-core                核心逻辑层：turn、session、配置、工具审批、编译流程编排
    │
pl-model               LLM provider 层：模型元数据、API 适配、SSE 流式响应
    │
pl-protocol            公共协议层：消息、事件、错误、权限等跨 crate 共享类型
```

## 快速开始

### 前置条件

- [Rust](https://rustup.rs/) (edition 2024)
- [Node.js](https://nodejs.org/) LTS
- Windows / macOS / Linux

### 启动开发环境

```powershell
# Windows
./run-pure-studio.ps1

# 或手动启动
cd code/pure-studio
npm install
npm run tauri:dev
```

### 配置

首次启动后，在 Pure Studio 设置页面配置 LLM provider。配置保存在：

```text
~/.pure/config.toml                 # provider/model/role 配置
~/.pure/studio/studio_1.sqlite      # 会话、消息、工具审批记录
```

## 项目结构

```text
pure-lang/
├── code/
│   ├── pl-protocol/     # 跨 crate 公共协议类型
│   ├── pl-model/        # LLM provider 适配
│   ├── pl-core/         # 核心编译逻辑
│   └── pure-studio/     # Tauri 2 桌面应用
│       ├── src-tauri/   # Rust 后端（Tauri 命令桥接）
│       └── src/         # React + TypeScript 前端
└── design/              # 架构设计文档
```

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Tauri 2 |
| 后端 | Rust, tokio, SeaORM (SQLite) |
| 前端 | React 18, TypeScript, Vite |
| LLM 集成 | OpenAI 兼容 API, SSE 流式 |
| 序列化 | serde, TOML |

## 开发

```bash
# 格式化
cargo fmt

# Lint
cargo clippy -- -D warnings

# 运行各 crate 测试
cargo test -p pl-protocol
cargo test -p pl-model
cargo test -p pl-core

# 前端类型检查
npm --prefix code/pure-studio run typecheck

# 前端构建
npm --prefix code/pure-studio run build
```

## License

Apache-2.0

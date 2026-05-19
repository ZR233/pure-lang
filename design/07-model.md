# 07 - Model 层设计

> 参考 OpenAI Codex CLI (`codex-rs/`) 的 model 层架构，设计 pure-lang 的 LLM 集成层。

## 7.1 Codex Model 层架构分析

Codex 将 model 层拆分为 4 个职责清晰的 crate：

| Crate | 职责 | 关键抽象 |
|-------|------|---------|
| `codex-protocol` | 共享类型定义 | `ModelInfo`, `ModelPreset`, `ModelsResponse` |
| `codex-model-provider-info` | Provider 静态配置 | `ModelProviderInfo`（序列化配置结构） |
| `codex-model-provider` | Provider 运行时抽象 | `ModelProvider` trait（auth、capabilities、api 调用） |
| `codex-models-manager` | 模型发现与缓存 | `ModelsManager` trait（bundled + remote + cache） |

### 核心设计模式

**1. 配置与运行时分离**

`ModelProviderInfo` 是纯数据结构（可序列化到 TOML/JSON），描述 provider 的连接配置。
`ModelProvider` trait 是运行时抽象，封装认证、API 调用、能力查询。

```
ModelProviderInfo (静态配置) ──构建──> ModelProvider (运行时实例)
```

**2. 模型元数据的分层管理**

```
bundled models.json (编译时嵌入)
        │
        ├── remote /models API (运行时拉取)
        │
        ├── models_cache.json (磁盘缓存，TTL 5 分钟)
        │
        └── 合并策略：remote 覆盖 bundled 中相同 slug 的条目
```

**3. Provider 工厂模式**

```rust
// codex-rs/model-provider/src/provider.rs
pub fn create_model_provider(
    provider_info: ModelProviderInfo,
    auth_manager: Option<Arc<AuthManager>>,
) -> SharedModelProvider {
    if provider_info.is_amazon_bedrock() {
        Arc::new(AmazonBedrockModelProvider::new(provider_info))
    } else {
        Arc::new(ConfiguredModelProvider::new(provider_info, auth_manager))
    }
}
```

**4. 丰富的模型元数据**

Codex 的 `ModelInfo` 包含远超"模型名称"的信息：

- `context_window` / `max_context_window` — 上下文窗口
- `auto_compact_token_limit` — 自动压缩阈值
- `truncation_policy` — 输出截断策略（bytes/tokens）
- `supports_parallel_tool_calls` — 是否支持并行工具调用
- `reasoning_effort` — 推理强度级别
- `base_instructions` / `model_messages` — 模型专属系统提示
- `shell_type` — Shell 执行方式
- `input_modalities` — 支持的输入模态（text/image）

---

## 7.2 Pure-Lang Model 层设计

### 设计目标

1. 支持多 LLM Provider（OpenAI、Anthropic、Ollama、自定义）
2. 模型元数据可配置，支持运行时动态发现
3. 认证方式可扩展（API Key、OAuth、命令行获取）
4. 兼容 OpenAI Responses API 和 Chat Completions API
5. 模型切换无需重启
6. **原生全流式**：所有 LLM 调用通过 `AgentEventSender` 推送增量

### 独立 Crate：pl-model

参考 09-conventions.md 的 R7 规范（不向 pl-core 无节制添加代码），将 model 层独立为 `pl-model` crate：

```
pl-model/
├── Cargo.toml
└── src/
    ├── lib.rs              # pub use 导出
    ├── provider.rs         # ModelProvider trait + 工厂函数
    ├── provider_info.rs    # ProviderInfo 静态配置
    ├── model_info.rs       # ModelInfo 元数据
    ├── manager.rs          # ModelsManager trait + DefaultModelsManager
    ├── capabilities.rs     # ModelCapabilities / ProviderCapabilities bitflags
    ├── auth.rs             # 认证抽象
    ├── wire_api.rs         # WireAdapter trait
    ├── openai.rs           # OpenAI 兼容实现
    ├── anthropic.rs        # Anthropic 实现
    └── sse.rs              # SSE 解析工具
```

依赖关系：

```
pl-core (无内部依赖)
    ↑
pl-model (依赖 pl-core)
    ↑
pl-tool, pl-memory, pl-runtime (各依赖 pl-core)
```

---

## 7.3 核心类型定义

### ModelCapabilities — 位标志集

替代多个 `supports_*: bool` 字段：

```rust
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct ModelCapabilities: u32 {
        const STREAMING             = 0b00000001;
        const FUNCTION_CALLING      = 0b00000010;
        const VISION                = 0b00000100;
        const PARALLEL_TOOL_CALLS   = 0b00001000;
        const REASONING             = 0b00010000;
        const WEB_SEARCH            = 0b00100000;
    }
}
```

### ModelInfo — 模型元数据

参考 Codex 的 `ModelInfo`，定义纯 Rust 结构：

```rust
/// 模型元数据，描述一个 LLM 模型的能力和配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// 模型标识符（如 "gpt-4", "claude-sonnet-4-6"）
    pub slug: String,

    /// 显示名称
    pub display_name: String,

    /// 模型描述
    pub description: Option<String>,

    // ── 上下文窗口 ──

    /// 上下文窗口大小（token 数）
    pub context_window: Option<u64>,

    /// 最大上下文窗口（用于配置覆盖上限）
    pub max_context_window: Option<u64>,

    /// 自动压缩阈值（token 数），默认为 context_window 的 90%
    pub auto_compact_token_limit: Option<u64>,

    // ── 生成参数 ──

    /// 默认生成温度
    pub default_temperature: Option<f32>,

    /// 最大输出 token 数
    pub max_output_tokens: Option<u64>,

    // ── 能力标记（位标志集） ──

    /// 模型能力位标志
    pub capabilities: ModelCapabilities,

    /// 支持的输入模态
    pub input_modalities: Vec<InputModality>,

    // ── 截断策略 ──

    /// 工具输出截断策略
    pub truncation_policy: TruncationPolicy,

    // ── 提示词 ──

    /// 模型基础系统提示
    pub base_instructions: String,

    /// 是否为回退元数据（未知模型时使用）
    #[serde(skip)]
    pub used_fallback: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum InputModality {
    Text,
    Image,
    Audio,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TruncationPolicy {
    pub mode: TruncationMode,
    pub limit: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TruncationMode {
    Bytes,
    Tokens,
}

impl ModelInfo {
    /// 解析后的上下文窗口大小
    pub fn resolved_context_window(&self) -> Option<u64> {
        self.context_window.or(self.max_context_window)
    }

    /// 解析后的自动压缩阈值
    pub fn resolved_auto_compact_limit(&self) -> Option<u64> {
        let context = self.resolved_context_window()?;
        let default_limit = (context * 90) / 100;
        Some(self.auto_compact_token_limit
            .map_or(default_limit, |limit| limit.min(default_limit)))
    }

    /// 便捷方法：检查是否支持流式
    pub fn supports_streaming(&self) -> bool {
        self.capabilities.contains(ModelCapabilities::STREAMING)
    }

    /// 便捷方法：检查是否支持并行工具调用
    pub fn supports_parallel_tool_calls(&self) -> bool {
        self.capabilities.contains(ModelCapabilities::PARALLEL_TOOL_CALLS)
    }

    /// 为未知模型创建回退元数据
    pub fn fallback(slug: &str) -> Self {
        Self {
            slug: slug.to_string(),
            display_name: slug.to_string(),
            description: None,
            context_window: Some(128_000),
            max_context_window: Some(128_000),
            auto_compact_token_limit: None,
            default_temperature: Some(0.3),
            max_output_tokens: Some(4096),
            capabilities: ModelCapabilities::STREAMING | ModelCapabilities::FUNCTION_CALLING,
            input_modalities: vec![InputModality::Text],
            truncation_policy: TruncationPolicy { mode: TruncationMode::Bytes, limit: 10_000 },
            base_instructions: String::new(),
            used_fallback: true,
        }
    }
}
```

### ProviderInfo — Provider 静态配置

参考 Codex 的 `ModelProviderInfo`：

```rust
/// Provider 的静态配置，可从 pure.toml 反序列化。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    /// 显示名称（如 "OpenAI", "Anthropic", "Ollama"）
    pub name: String,

    /// API Base URL
    pub base_url: Option<String>,

    /// API Key 环境变量名（如 "OPENAI_API_KEY"）
    pub env_key: Option<String>,

    /// 环境变量设置指引
    pub env_key_instructions: Option<String>,

    /// 静态 Bearer Token（不推荐，优先用 env_key）
    pub bearer_token: Option<String>,

    /// 外部认证命令（输出 token 到 stdout）
    pub auth_command: Option<AuthCommand>,

    /// API 协议类型
    #[serde(default)]
    pub wire_api: WireApi,

    /// 额外 HTTP 请求头
    pub http_headers: Option<HashMap<String, String>>,

    /// 从环境变量读取的请求头
    pub env_http_headers: Option<HashMap<String, String>>,

    /// 请求最大重试次数
    pub request_max_retries: Option<u32>,

    /// 流式连接最大重试次数
    pub stream_max_retries: Option<u32>,

    /// 流式空闲超时（毫秒）
    pub stream_idle_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthCommand {
    pub command: String,
    pub args: Vec<String>,
    pub timeout_ms: u64,
}

/// API 协议类型
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireApi {
    /// OpenAI Responses API（/v1/responses）
    #[default]
    Responses,
    /// OpenAI Chat Completions API（/v1/chat/completions）
    Chat,
}
```

### ProviderInfo 内置定义

```rust
impl ProviderInfo {
    pub fn openai(base_url: Option<String>) -> Self {
        Self {
            name: "OpenAI".into(),
            base_url: base_url.or_else(|| Some("https://api.openai.com/v1".into())),
            env_key: Some("OPENAI_API_KEY".into()),
            wire_api: WireApi::Responses,
            ..Default::default()
        }
    }

    pub fn anthropic(base_url: Option<String>) -> Self {
        Self {
            name: "Anthropic".into(),
            base_url: base_url.or_else(|| Some("https://api.anthropic.com".into())),
            env_key: Some("ANTHROPIC_API_KEY".into()),
            wire_api: WireApi::Chat,  // Anthropic 使用 Messages API
            http_headers: Some(HashMap::from([
                ("anthropic-version".into(), "2023-06-01".into()),
            ])),
            ..Default::default()
        }
    }

    pub fn ollama() -> Self {
        Self {
            name: "Ollama".into(),
            base_url: Some("http://localhost:11434/v1".into()),
            wire_api: WireApi::Chat,
            ..Default::default()
        }
    }
}
```

### ProviderCapabilities — 位标志集

```rust
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct ProviderCapabilities: u32 {
        const STREAMING             = 0b00000001;
        const FUNCTION_CALLING      = 0b00000010;
        const VISION                = 0b00000100;
        const PARALLEL_TOOL_CALLS   = 0b00001000;
    }
}
```

### ModelProvider — 运行时 Provider Trait

```rust
/// LLM Provider 运行时抽象。
///
/// 封装认证、API 调用、能力查询等 provider 特定逻辑。
/// 通过工厂函数 `create_provider()` 创建。
///
/// 实现者契约：
/// - 通过 event_tx 推送 LLM 输出增量（TextDelta/ThinkingDelta/ToolCallDelta）
/// - capabilities() 如实报告支持的功能
/// - auth_token() 返回当前有效的认证凭据
pub trait ModelProvider: Debug + Send + Sync {
    /// Provider 配置信息
    fn info(&self) -> &ProviderInfo;

    /// Provider 能力
    fn capabilities(&self) -> ProviderCapabilities;

    /// 流式补全请求（唯一调用入口）
    ///
    /// 通过 event_tx 推送增量事件，最终返回完整响应。
    fn stream_complete(
        &self,
        request: CompletionRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompletionResponse>> + Send;

    /// 获取认证 token
    fn auth_token(
        &self,
    ) -> impl std::future::Future<Output = Result<Option<String>>> + Send;

    /// 获取模型元数据
    fn model_info(&self, model: &str) -> ModelInfo;

    /// 获取默认模型名称
    fn default_model(&self) -> &str;
}

/// 共享 Provider 句柄
pub type SharedModelProvider = Arc<dyn ModelProvider>;
```

### CompletionRequest / CompletionResponse

```rust
/// 补全请求
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    /// 模型标识
    pub model: String,

    /// 消息列表
    pub messages: Vec<Message>,

    /// 可调用的工具 schema 列表
    pub tools: Vec<ToolSchema>,

    /// 生成温度
    pub temperature: Option<f32>,

    /// 最大输出 token
    pub max_tokens: Option<u64>,
}

/// 补全响应
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    /// 文本内容
    pub content: Option<String>,

    /// 工具调用列表
    pub tool_calls: Vec<ToolCall>,

    /// Token 用量
    pub usage: TokenUsage,

    /// 完成原因
    pub finish_reason: FinishReason,

    /// 使用的模型
    pub model: String,
}

#[derive(Debug, Clone)]
pub enum FinishReason {
    Stop,
    ToolCalls,
    MaxTokens,
    ContentFilter,
}

#[derive(Debug, Clone)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// 工具 Schema（JSON Schema 格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}
```

---

## 7.4 Provider 实现

### OpenAI Compatible Provider

支持所有兼容 OpenAI API 的 provider（OpenAI、Azure、自定义代理）：

```rust
pub struct OpenAiCompatibleProvider {
    info: ProviderInfo,
    http_client: reqwest::Client,
    wire_adapter: Box<dyn WireAdapter>,
    models: Vec<ModelInfo>,
}

impl OpenAiCompatibleProvider {
    pub fn new(info: ProviderInfo) -> Result<Self> {
        let wire_adapter: Box<dyn WireAdapter> = match info.wire_api {
            WireApi::Responses => Box::new(ResponsesApiAdapter),
            WireApi::Chat => Box::new(ChatCompletionsAdapter),
        };
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?;
        Ok(Self {
            info,
            http_client,
            wire_adapter,
            models: Self::default_models(),
        })
    }

    fn default_models() -> Vec<ModelInfo> {
        serde_json::from_str(include_str!("../models/openai.json"))
            .unwrap_or_default()
    }
}

impl ModelProvider for OpenAiCompatibleProvider {
    fn info(&self) -> &ProviderInfo { &self.info }
    fn default_model(&self) -> &str { "gpt-4" }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::STREAMING
            | ProviderCapabilities::FUNCTION_CALLING
            | ProviderCapabilities::VISION
            | ProviderCapabilities::PARALLEL_TOOL_CALLS
    }

    fn auth_token(
        &self,
    ) -> impl std::future::Future<Output = Result<Option<String>>> + Send {
        async move {
            // 1. 优先 bearer_token
            if let Some(token) = &self.info.bearer_token {
                return Ok(Some(token.clone()));
            }
            // 2. 其次 auth_command
            if let Some(cmd) = &self.info.auth_command {
                let output = tokio::process::Command::new(&cmd.command)
                    .args(&cmd.args)
                    .output()
                    .await?;
                return Ok(Some(String::from_utf8_lossy(&output.stdout).trim().into()));
            }
            // 3. 最后 env_key
            if let Some(env_key) = &self.info.env_key {
                return Ok(std::env::var(env_key).ok());
            }
            Ok(None)
        }
    }

    fn stream_complete(
        &self,
        request: CompletionRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompletionResponse>> + Send {
        async move {
            let url = format!("{}/responses", self.info.base_url.as_deref().unwrap_or(""));
            let token = self.auth_token().await?;
            let body = self.wire_adapter.build_request_body(&request);

            let response = self.http_client
                .post(&url)
                .bearer_auth(token.as_deref().unwrap_or(""))
                .json(&body)
                .send()
                .await?;

            // SSE 流式解析
            self.parse_sse_stream(response, &event_tx).await
        }
    }

    fn model_info(&self, model: &str) -> ModelInfo {
        self.models.iter()
            .find(|m| m.slug == model)
            .cloned()
            .unwrap_or_else(|| ModelInfo::fallback(model))
    }
}
```

### Anthropic Provider

```rust
pub struct AnthropicProvider {
    info: ProviderInfo,
    http_client: reqwest::Client,
    wire_adapter: AnthropicMessagesAdapter,
}

impl ModelProvider for AnthropicProvider {
    fn info(&self) -> &ProviderInfo { &self.info }
    fn default_model(&self) -> &str { "claude-sonnet-4-6" }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::STREAMING
            | ProviderCapabilities::FUNCTION_CALLING
            | ProviderCapabilities::VISION
            | ProviderCapabilities::PARALLEL_TOOL_CALLS
    }

    fn stream_complete(
        &self,
        request: CompletionRequest,
        event_tx: AgentEventSender,
    ) -> impl std::future::Future<Output = Result<CompletionResponse>> + Send {
        async move {
            let body = self.wire_adapter.build_request_body(&request);
            let token = self.auth_token().await?;

            let response = self.http_client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", token.as_deref().unwrap_or(""))
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()
                .await?;

            self.parse_sse_stream(response, &event_tx).await
        }
    }

    // auth_token, model_info 等实现...
}
```

### Provider 工厂

```rust
/// 根据 ProviderInfo 创建对应的 ModelProvider 实例
pub fn create_provider(info: ProviderInfo) -> Result<SharedModelProvider> {
    let provider: SharedModelProvider = match info.name.as_str() {
        "Anthropic" => Arc::new(AnthropicProvider::new(info)?),
        _ => Arc::new(OpenAiCompatibleProvider::new(info)?),
        // OpenAI、Ollama、LMStudio、自定义代理都走 OpenAI 兼容
    };
    Ok(provider)
}
```

---

## 7.5 ModelsManager — 模型发现与缓存

参考 Codex 的 `ModelsManager`，管理模型元数据的发现、缓存和合并：

```rust
/// 模型刷新策略
#[derive(Debug, Clone, Copy)]
pub enum RefreshStrategy {
    /// 总是从网络拉取
    Online,
    /// 只用缓存
    Offline,
    /// 缓存未命中时拉取
    OnlineIfUncached,
}

/// 模型管理器 trait
///
/// 实现者契约：
/// - Offline 策略必须不发起网络请求
/// - Online 策略失败时应回退到 Offline
/// - get_model_info() 对未知模型返回 fallback 元数据
pub trait ModelsManager: Debug + Send + Sync {
    fn list_models(
        &self,
        strategy: RefreshStrategy,
    ) -> impl std::future::Future<Output = Vec<ModelInfo>> + Send;

    fn get_model_info(
        &self,
        model: &str,
    ) -> impl std::future::Future<Output = ModelInfo> + Send;

    fn default_model(&self) -> &str;
}

/// 基于 bundled + remote + cache 的模型管理器
pub struct DefaultModelsManager {
    /// Provider
    provider: SharedModelProvider,
    /// 编译时嵌入的模型列表
    bundled: Vec<ModelInfo>,
    /// 运行时缓存的远程模型
    remote: RwLock<Vec<ModelInfo>>,
    /// 磁盘缓存路径
    cache_path: PathBuf,
    /// 缓存 TTL
    cache_ttl: Duration,
}

impl DefaultModelsManager {
    pub fn new(provider: SharedModelProvider, cache_dir: &Path) -> Self {
        let bundled: Vec<ModelInfo> = serde_json::from_str(
            include_str!("../models/default.json")
        ).unwrap_or_default();

        Self {
            provider,
            bundled,
            remote: RwLock::new(Vec::new()),
            cache_path: cache_dir.join("models_cache.json"),
            cache_ttl: Duration::from_secs(300),
        }
    }

    /// 合并 bundled + remote 模型列表（remote 覆盖同 slug 的 bundled 条目）
    fn merge_models(bundled: &[ModelInfo], remote: &[ModelInfo]) -> Vec<ModelInfo> {
        let mut result = bundled.to_vec();
        for model in remote {
            if let Some(idx) = result.iter().position(|m| m.slug == model.slug) {
                result[idx] = model.clone();
            } else {
                result.push(model.clone());
            }
        }
        result
    }
}

impl ModelsManager for DefaultModelsManager {
    fn list_models(
        &self,
        strategy: RefreshStrategy,
    ) -> impl std::future::Future<Output = Vec<ModelInfo>> + Send {
        async move {
            match strategy {
                RefreshStrategy::Offline => {
                    let remote = self.remote.read().await.clone();
                    Self::merge_models(&self.bundled, &remote)
                }
                RefreshStrategy::Online => {
                    // 尝试远程拉取，失败回退 Offline
                    let online_result = async {
                        // self.provider.list_models().await
                        todo!("调用 provider 的模型列表 API")
                    }.await;

                    match online_result {
                        Ok(models) => {
                            *self.remote.write().await = models.clone();
                            // self.persist_cache(&models);
                            Self::merge_models(&self.bundled, &models)
                        }
                        Err(_) => {
                            self.list_models(RefreshStrategy::Offline).await
                        }
                    }
                }
                RefreshStrategy::OnlineIfUncached => {
                    if !self.remote.read().await.is_empty() {
                        self.list_models(RefreshStrategy::Offline).await
                    } else {
                        self.list_models(RefreshStrategy::Online).await
                    }
                }
            }
        }
    }

    fn get_model_info(
        &self,
        model: &str,
    ) -> impl std::future::Future<Output = ModelInfo> + Send {
        async move {
            let models = self.list_models(RefreshStrategy::OnlineIfUncached).await;
            models.into_iter()
                .find(|m| m.slug == model || model.starts_with(&m.slug))
                .unwrap_or_else(|| ModelInfo::fallback(model))
        }
    }

    fn default_model(&self) -> &str {
        self.provider.default_model()
    }
}
```

---

## 7.6 模型数据文件

### models/default.json（编译时嵌入）

```json
[
  {
    "slug": "gpt-4",
    "display_name": "GPT-4",
    "context_window": 128000,
    "max_context_window": 128000,
    "capabilities": ["STREAMING", "FUNCTION_CALLING", "VISION", "PARALLEL_TOOL_CALLS"],
    "input_modalities": ["text", "image"],
    "truncation_policy": { "mode": "bytes", "limit": 10000 },
    "base_instructions": ""
  },
  {
    "slug": "claude-sonnet-4-6",
    "display_name": "Claude Sonnet 4.6",
    "context_window": 200000,
    "max_context_window": 200000,
    "capabilities": ["STREAMING", "FUNCTION_CALLING", "VISION", "PARALLEL_TOOL_CALLS"],
    "input_modalities": ["text", "image"],
    "truncation_policy": { "mode": "bytes", "limit": 10000 },
    "base_instructions": ""
  }
]
```

---

## 7.7 Wire API 适配层

不同 provider 使用不同的 API 格式。Wire API 适配层负责统一转换：

```
                ┌─────────────────────────────┐
                │     CompletionRequest        │
                │     (内部统一格式)             │
                └──────────┬──────────────────┘
                           │
              ┌────────────┼────────────────┐
              │            │                │
    ┌─────────▼──┐  ┌──────▼──────┐  ┌─────▼──────┐
    │ Responses   │  │   Chat      │  │  Anthropic │
    │ API Format  │  │   Completions│  │  Messages  │
    │             │  │   Format    │  │  Format    │
    └─────────┬──┘  └──────┬──────┘  └─────┬──────┘
              │            │                │
              ▼            ▼                ▼
         OpenAI API   Ollama/LMStudio   Anthropic API
```

```rust
/// API 协议适配器。
///
/// 将内部统一的 CompletionRequest 转换为不同 provider 的 wire 格式，
/// 并将 provider 返回的响应解析回 CompletionResponse。
///
/// 实现者契约：
/// - build_request_body() 产生的 JSON 必须符合目标 API 规范
/// - parse_stream_line() 处理单行 SSE 数据，返回 None 表示跳过
pub trait WireAdapter: Send + Sync {
    fn build_request_body(&self, request: &CompletionRequest) -> serde_json::Value;
    fn parse_response(&self, body: serde_json::Value) -> Result<CompletionResponse>;
    fn parse_stream_line(&self, line: &str) -> Result<Option<Vec<AgentEvent>>>;
}

struct ResponsesApiAdapter;       // /v1/responses
struct ChatCompletionsAdapter;    // /v1/chat/completions
struct AnthropicMessagesAdapter;  // /v1/messages
```

这样每个 provider 只需选择对应的 adapter，不需要自己处理格式转换。

---

## 7.8 配置集成

### pure.toml 中的模型配置

```toml
[model]
provider = "openai"             # 当前使用的 provider ID
model = "gpt-4"                 # 默认模型
temperature = 0.3
max_tokens = 4096

[model.providers.openai]
name = "OpenAI"
base_url = "https://api.openai.com/v1"
env_key = "OPENAI_API_KEY"
wire_api = "responses"

[model.providers.anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com"
env_key = "ANTHROPIC_API_KEY"
wire_api = "chat"

[model.providers.ollama]
name = "Ollama"
base_url = "http://localhost:11434/v1"
wire_api = "chat"

[model.providers.custom-proxy]
name = "My Proxy"
base_url = "https://my-proxy.example.com/v1"
env_key = "MY_API_KEY"
wire_api = "responses"
http_headers = { "X-Custom-Header" = "value" }

# 可选：覆盖特定模型的上下文窗口
[model.overrides.gpt-4]
context_window = 100000
auto_compact_token_limit = 90000
```

### 配置加载流程

```
启动
  │
  ├── 读取 pure.toml
  │
  ├── 解析 [model] 段 → 确定当前 provider ID
  │
  ├── 解析 [model.providers.<id>] → ProviderInfo
  │
  ├── create_provider(info) → SharedModelProvider
  │
  ├── DefaultModelsManager::new(provider, cache_dir)
  │
  └── 注入到 Agent 和 Compiler
```

---

## 7.9 与 Codex 架构的对比

| 维度 | Codex | Pure-Lang | 设计理由 |
|------|-------|-----------|---------|
| 模块划分 | 4 个独立 crate | 独立 `pl-model` crate | 独立成 crate 避免膨胀 pl-core |
| Provider 实现 | OpenAI + Bedrock | OpenAI兼容 + Anthropic | 优先支持主流 provider |
| 认证方式 | OAuth + API Key + 命令行 | API Key + 命令行 + Bearer Token | 首版简化 OAuth |
| 模型元数据 | 远端 /models API 拉取 | bundled JSON + 远端可选 | 首版以本地为主，降低网络依赖 |
| 协议支持 | Responses API only | Responses + Chat + Anthropic | 兼容更多 provider |
| 缓存策略 | JSON 文件 + TTL | JSON 文件 + TTL | 相同 |
| 上下文管理 | 服务端远程压缩 | 本地压缩 | 简化首版 |
| 流式模式 | 服务端流式 | AgentEvent 统一流式 | 全系统统一事件流 |

---

## 7.10 首版实现范围

| 优先级 | 内容 | 说明 |
|-------|------|------|
| P0 | `ModelCapabilities` bitflags | 替代多个 bool 字段 |
| P0 | `ModelInfo`, `ProviderInfo`, `CompletionRequest/Response` | 核心类型 |
| P0 | `OpenAiCompatibleProvider` + `stream_complete()` | 流式调用，OpenAI 兼容 |
| P0 | `ProviderInfo::openai()`, `::ollama()` | 内置 provider 定义 |
| P0 | `create_provider()` 工厂函数 | provider 创建入口 |
| P0 | bundled models JSON | 编译时嵌入默认模型列表 |
| P0 | `WireAdapter` trait + SSE 解析 | 流式响应解析 |
| P1 | `AnthropicProvider` | Anthropic Messages API |
| P1 | `DefaultModelsManager` + 缓存 | 模型发现与缓存 |
| P1 | `AuthCommand` 支持 | 外部命令获取 token |
| P2 | 远端 /models 拉取 | 运行时模型发现 |
| P2 | 模型配置覆盖 | pure.toml 中的 model overrides |
| P2 | Fallback 策略 | 多 provider 自动切换 |

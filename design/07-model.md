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

### Crate 映射

Codex 的 4 crate 模式在 pure-lang 中映射为 `pl-core` 内的子模块：

```
pl-core/
├── src/
│   ├── model/
│   │   ├── mod.rs              # 模块导出
│   │   ├── provider_info.rs    # Provider 静态配置（对应 codex-model-provider-info）
│   │   ├── provider.rs         # Provider 运行时 trait（对应 codex-model-provider）
│   │   ├── model_info.rs       # 模型元数据类型（对应 codex-protocol/openai_models）
│   │   ├── manager.rs          # 模型发现与缓存（对应 codex-models-manager）
│   │   ├── auth.rs             # 认证抽象
│   │   └── wire_api.rs         # API 协议抽象（Responses / Chat）
│   ├── error.rs
│   ├── message.rs
│   ├── permission.rs
│   └── lib.rs
```

> **设计决策**：首版将 model 层放在 `pl-core` 中作为子模块，而非独立 crate。
> 原因：pure-lang 的 model 层是核心基础设施，几乎所有其他 crate 都依赖它，
> 独立成 crate 会增加维护成本但收益有限。当模型足够复杂时再拆分。

---

## 7.3 核心类型定义

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

    // ── 能力标记 ──

    /// 是否支持并行工具调用
    pub supports_parallel_tool_calls: bool,

    /// 是否支持流式输出
    pub supports_streaming: bool,

    /// 是否支持视觉/图片输入
    pub supports_vision: bool,

    /// 是否支持 function calling
    pub supports_function_calling: bool,

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
            supports_parallel_tool_calls: false,
            supports_streaming: true,
            supports_vision: false,
            supports_function_calling: true,
            input_modalities: vec![InputModality::Text],
            truncation_policy: TruncationPolicy::bytes(10_000),
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

### ModelProvider — 运行时 Provider Trait

参考 Codex 的 `ModelProvider` trait：

```rust
/// Provider 运行时能力
#[derive(Debug, Clone, Copy)]
pub struct ProviderCapabilities {
    pub supports_streaming: bool,
    pub supports_function_calling: bool,
    pub supports_vision: bool,
    pub supports_web_search: bool,
}

impl Default for ProviderCapabilities {
    fn default() -> Self {
        Self {
            supports_streaming: true,
            supports_function_calling: true,
            supports_vision: true,
            supports_web_search: false,
        }
    }
}

/// LLM Provider 运行时抽象。
///
/// 封装了认证、API 调用、能力查询等 provider 特定逻辑。
/// 每个 provider 实现此 trait，通过工厂函数创建。
#[async_trait]
pub trait ModelProvider: Debug + Send + Sync {
    /// Provider 配置信息
    fn info(&self) -> &ProviderInfo;

    /// Provider 能力
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::default()
    }

    /// 获取认证 token
    async fn auth_token(&self) -> Result<Option<String>>;

    /// 发送补全请求（非流式）
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;

    /// 发送补全请求（流式）
    async fn stream(&self, request: CompletionRequest)
        -> Result<Box<dyn CompletionStream>>;

    /// 获取模型元数据
    fn model_info(&self, model: &str) -> ModelInfo;

    /// 列出可用模型
    async fn list_models(&self) -> Result<Vec<ModelInfo>>;

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

    /// 是否流式
    pub stream: bool,
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

/// 流式响应 trait
#[async_trait]
pub trait CompletionStream: Send {
    async fn next_chunk(&mut self) -> Result<Option<StreamChunk>>;
}

#[derive(Debug, Clone)]
pub enum StreamChunk {
    Delta { content: String },
    ToolCallDelta { id: String, name: String, arguments_delta: String },
    Done(CompletionResponse),
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
    models: Vec<ModelInfo>,
}

impl OpenAiCompatibleProvider {
    pub fn new(info: ProviderInfo) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?;
        Ok(Self {
            info,
            http_client,
            models: Self::default_models(),
        })
    }

    fn default_models() -> Vec<ModelInfo> {
        // 编译时嵌入的默认模型列表
        serde_json::from_str(include_str!("../models/openai.json"))
            .unwrap_or_default()
    }

    fn resolve_base_url(&self) -> String {
        self.info.base_url.clone()
            .unwrap_or_else(|| "https://api.openai.com/v1".into())
    }
}

#[async_trait]
impl ModelProvider for OpenAiCompatibleProvider {
    fn info(&self) -> &ProviderInfo { &self.info }
    fn default_model(&self) -> &str { "gpt-4" }

    async fn auth_token(&self) -> Result<Option<String>> {
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

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        let url = format!("{}/responses", self.resolve_base_url());
        let token = self.auth_token().await?;

        let body = self.build_request_body(&request);
        let response = self.http_client
            .post(&url)
            .bearer_auth(token.as_deref().unwrap_or(""))
            .json(&body)
            .send()
            .await?;

        self.parse_response(response).await
    }

    // ... stream, model_info, list_models 实现
}
```

### Anthropic Provider

```rust
pub struct AnthropicProvider {
    info: ProviderInfo,
    http_client: reqwest::Client,
}

#[async_trait]
impl ModelProvider for AnthropicProvider {
    fn info(&self) -> &ProviderInfo { &self.info }
    fn default_model(&self) -> &str { "claude-sonnet-4-6" }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse> {
        // 将内部 CompletionRequest 转换为 Anthropic Messages API 格式
        let body = self.convert_to_anthropic_format(&request);
        let token = self.auth_token().await?;

        let response = self.http_client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", token.as_deref().unwrap_or(""))
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?;

        self.parse_anthropic_response(response).await
    }

    // ...
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
#[async_trait]
pub trait ModelsManager: Debug + Send + Sync {
    /// 列出可用模型
    async fn list_models(&self, strategy: RefreshStrategy) -> Vec<ModelInfo>;

    /// 获取指定模型的元数据
    async fn get_model_info(&self, model: &str) -> ModelInfo;

    /// 获取默认模型
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

#[async_trait]
impl ModelsManager for DefaultModelsManager {
    async fn list_models(&self, strategy: RefreshStrategy) -> Vec<ModelInfo> {
        match strategy {
            RefreshStrategy::Offline => {
                let remote = self.remote.read().await.clone();
                Self::merge_models(&self.bundled, &remote)
            }
            RefreshStrategy::Online => {
                match self.provider.list_models().await {
                    Ok(models) => {
                        *self.remote.write().await = models.clone();
                        self.persist_cache(&models);
                        Self::merge_models(&self.bundled, &models)
                    }
                    Err(_) => self.list_models(RefreshStrategy::Offline).await,
                }
            }
            RefreshStrategy::OnlineIfUncached => {
                if let Some(cached) = self.load_cache() {
                    *self.remote.write().await = cached.clone();
                    Self::merge_models(&self.bundled, &cached)
                } else {
                    self.list_models(RefreshStrategy::Online).await
                }
            }
        }
    }

    async fn get_model_info(&self, model: &str) -> ModelInfo {
        let models = self.list_models(RefreshStrategy::OnlineIfUncached).await;
        models.into_iter()
            .find(|m| m.slug == model || model.starts_with(&m.slug))
            .unwrap_or_else(|| ModelInfo::fallback(model))
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
    "supports_parallel_tool_calls": true,
    "supports_streaming": true,
    "supports_vision": true,
    "supports_function_calling": true,
    "input_modalities": ["text", "image"],
    "truncation_policy": { "mode": "bytes", "limit": 10000 },
    "base_instructions": ""
  },
  {
    "slug": "claude-sonnet-4-6",
    "display_name": "Claude Sonnet 4.6",
    "context_window": 200000,
    "max_context_window": 200000,
    "supports_parallel_tool_calls": true,
    "supports_streaming": true,
    "supports_vision": true,
    "supports_function_calling": true,
    "input_modalities": ["text", "image"],
    "truncation_policy": { "mode": "bytes", "limit": 10000 },
    "base_instructions": ""
  }
]
```

---

## 7.7 配置集成

### pure.toml 中的模型配置

```toml
[llm]
provider = "openai"             # 当前使用的 provider ID
model = "gpt-4"                 # 默认模型
temperature = 0.3
max_tokens = 4096

[llm.providers.openai]
name = "OpenAI"
base_url = "https://api.openai.com/v1"
env_key = "OPENAI_API_KEY"
wire_api = "responses"

[llm.providers.anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com"
env_key = "ANTHROPIC_API_KEY"
wire_api = "chat"

[llm.providers.ollama]
name = "Ollama"
base_url = "http://localhost:11434/v1"
wire_api = "chat"

[llm.providers.custom-proxy]
name = "My Proxy"
base_url = "https://my-proxy.example.com/v1"
env_key = "MY_API_KEY"
wire_api = "responses"
http_headers = { "X-Custom-Header" = "value" }

# 可选：覆盖特定模型的上下文窗口
[llm.model_overrides.gpt-4]
context_window = 100000
auto_compact_token_limit = 90000
```

### 配置加载流程

```
启动
  │
  ├── 读取 pure.toml
  │
  ├── 解析 [llm] 段 → 确定当前 provider ID
  │
  ├── 解析 [llm.providers.<id>] → ProviderInfo
  │
  ├── create_provider(info) → SharedModelProvider
  │
  ├── DefaultModelsManager::new(provider, cache_dir)
  │
  └── 注入到 Agent 和 Compiler
```

---

## 7.8 与 Codex 架构的对比

| 维度 | Codex | Pure-Lang | 设计理由 |
|------|-------|-----------|---------|
| 模块划分 | 4 个独立 crate | pl-core 内子模块 | pure-lang 规模较小，首版不需要独立 crate |
| Provider 实现 | OpenAI + Bedrock | OpenAI兼容 + Anthropic | 优先支持主流 provider |
| 认证方式 | OAuth + API Key + 命令行 | API Key + 命令行 + Bearer Token | 首版简化 OAuth |
| 模型元数据 | 远端 /models API 拉取 | bundled JSON + 远端可选 | 首版以本地为主，降低网络依赖 |
| 协议支持 | Responses API only | Responses + Chat | 兼容更多 provider |
| 缓存策略 | JSON 文件 + TTL | JSON 文件 + TTL | 相同 |
| 上下文管理 | 服务端远程压缩 | 本地压缩 | 简化首版 |

---

## 7.9 Wire API 适配层

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
/// 请求格式转换 trait
trait WireAdapter: Send + Sync {
    fn build_request_body(&self, request: &CompletionRequest) -> serde_json::Value;
    fn parse_response(&self, body: serde_json::Value) -> Result<CompletionResponse>;
    fn parse_stream_chunk(&self, chunk: serde_json::Value) -> Result<Option<StreamChunk>>;
}

struct ResponsesApiAdapter;   // /v1/responses
struct ChatCompletionsAdapter; // /v1/chat/completions
struct AnthropicMessagesAdapter; // /v1/messages
```

这样每个 provider 只需选择对应的 adapter，不需要自己处理格式转换。

---

## 7.10 首版实现范围

| 优先级 | 内容 | 说明 |
|-------|------|------|
| P0 | `ModelInfo`, `ProviderInfo`, `CompletionRequest/Response` | 核心类型 |
| P0 | `OpenAiCompatibleProvider` | 支持所有 OpenAI 兼容 API |
| P0 | `ProviderInfo::openai()`, `::ollama()` | 内置 provider 定义 |
| P0 | `create_provider()` 工厂函数 | provider 创建入口 |
| P0 | bundled models JSON | 编译时嵌入默认模型列表 |
| P1 | `AnthropicProvider` | Anthropic Messages API |
| P1 | `DefaultModelsManager` + 缓存 | 模型发现与缓存 |
| P1 | `AuthCommand` 支持 | 外部命令获取 token |
| P1 | 流式输出 | `CompletionStream` |
| P2 | 远端 /models 拉取 | 运行时模型发现 |
| P2 | 模型配置覆盖 | pure.toml 中的 model_overrides |
| P2 | Fallback 策略 | 多 provider 自动切换 |

import type {
  ConfigPayload,
  AgentDto,
  LspServerRecord,
  McpServerRecord,
  ProjectRecord,
  RoleRecord,
  SessionRecord,
  SessionRuntime,
  SkillRecord,
  ConversationPartView,
  StudioAgentTimelineEvent,
} from "../types";
import { makeProvider } from "./provider-mapper";
import { previewTemplates } from "./templates";

export { previewTemplates };

export const previewProjects: ProjectRecord[] = [
  {
    id: "pure-lang",
    name: "pure-lang",
    path: "D:\\Users\\zrufo\\Documents\\opensource\\pure-lang\\code\\pure-studio\\src-tauri",
    updatedAt: 1779688800,
  },
];

export const previewSessions: SessionRecord[] = [
  {
    id: "preview-session",
    projectId: "pure-lang",
    title: "分析项目架构",
    mode: "auto",
    updatedAt: 1779688800,
    visibility: "active",
  },
];

export const previewSessionRuntime: SessionRuntime = {
  sessionId: "preview-session",
  usage: {
    model: "deepseek-v4-flash",
    contextWindow: 1_000_000,
    latestContextTokens: 42_600,
    promptTokens: 84_200,
    completionTokens: 3_100,
    cachedPromptTokens: 51_800,
    totalTokens: 87_300,
    cacheHitRate: 0.62,
    estimatedCosts: [{ currency: "CNY", amount: 0.04 }],
    hasUnpricedUsage: false,
    updatedAt: 1779688800,
  },
  activeSkills: ["rust", "git", "doc"],
  activeMcpServers: ["github", "filesystem"],
  activeLspServers: ["rust-analyzer"],
  updatedAt: 1779688800,
};

export const previewSkills: SkillRecord[] = [
  {
    name: "skill-creator",
    description: "创建和更新 Pure 项目 skills 的结构化工作流。",
    category: "skills",
    platforms: [],
    scope: "system",
    path: "C:\\Users\\preview\\.pure\\skills\\.system\\skill-creator",
  },
  {
    name: "subagent-workflow",
    description: "规划子代理分工、等待结果和汇总输出的内置指导。",
    category: "agents",
    platforms: [],
    scope: "system",
    path: "C:\\Users\\preview\\.pure\\skills\\.system\\subagent-workflow",
  },
  {
    name: "pure-studio-local",
    description: "当前项目的 Studio 调试和验证流程。",
    category: "project",
    platforms: ["windows"],
    scope: "project",
    path: "D:\\Users\\zrufo\\Documents\\opensource\\pure-lang\\skills\\pure-studio-local",
  },
];

export const previewMcpServers: McpServerRecord[] = [
  {
    id: "filesystem",
    enabled: true,
    transport: "stdio",
    command: "npx",
    args: ["-y", "@modelcontextprotocol/server-filesystem", "D:/workspace"],
    env: [],
    cwd: null,
    url: null,
    bearerTokenEnvVar: null,
    headers: [],
    endpoint: "npx",
    sourceKind: "user",
    sourceLabel: "User",
    sourceDetail: null,
    statusKind: "enabled",
    statusMessage: null,
    mutationPolicy: "userEditable",
    availabilityKind: "available",
    availabilityMessage: "Available with 8 tools",
    lastCheckedAt: 1779688800,
    toolCount: 8,
  },
  {
    id: "github",
    enabled: true,
    transport: "streamableHttp",
    command: null,
    args: [],
    env: [],
    cwd: null,
    url: "https://example.com/mcp",
    bearerTokenEnvVar: "GITHUB_MCP_TOKEN",
    headers: [],
    endpoint: "https://example.com/mcp",
    sourceKind: "user",
    sourceLabel: "User",
    sourceDetail: null,
    statusKind: "enabled",
    statusMessage: null,
    mutationPolicy: "userEditable",
    availabilityKind: "available",
    availabilityMessage: "Available with 5 tools",
    lastCheckedAt: 1779688800,
    toolCount: 5,
  },
  {
    id: "zhipu_search",
    enabled: false,
    transport: "streamableHttp",
    command: null,
    args: [],
    env: [],
    cwd: null,
    url: "https://open.bigmodel.cn/api/mcp/web_search_prime/mcp",
    bearerTokenEnvVar: null,
    headers: [],
    endpoint: "https://open.bigmodel.cn/api/mcp/web_search_prime/mcp",
    sourceKind: "builtIn",
    sourceLabel: "Built-in",
    sourceDetail: "Zhipu Coding Plan",
    statusKind: "missingCredential",
    statusMessage: "Configure a Zhipu Coding Plan or Zhipu provider token to enable this server",
    mutationPolicy: "lockedIdentity",
    availabilityKind: "missingCredential",
    availabilityMessage: "Configure a Zhipu Coding Plan or Zhipu provider token to enable this server",
    lastCheckedAt: null,
    toolCount: null,
  },
  {
    id: "zhipu_reader",
    enabled: false,
    transport: "streamableHttp",
    command: null,
    args: [],
    env: [],
    cwd: null,
    url: "https://open.bigmodel.cn/api/mcp/web_reader/mcp",
    bearerTokenEnvVar: null,
    headers: [],
    endpoint: "https://open.bigmodel.cn/api/mcp/web_reader/mcp",
    sourceKind: "builtIn",
    sourceLabel: "Built-in",
    sourceDetail: "Zhipu Coding Plan",
    statusKind: "missingCredential",
    statusMessage: "Configure a Zhipu Coding Plan or Zhipu provider token to enable this server",
    mutationPolicy: "lockedIdentity",
    availabilityKind: "missingCredential",
    availabilityMessage: "Configure a Zhipu Coding Plan or Zhipu provider token to enable this server",
    lastCheckedAt: null,
    toolCount: null,
  },
  {
    id: "zhipu_zread",
    enabled: false,
    transport: "streamableHttp",
    command: null,
    args: [],
    env: [],
    cwd: null,
    url: "https://open.bigmodel.cn/api/mcp/zread/mcp",
    bearerTokenEnvVar: null,
    headers: [],
    endpoint: "https://open.bigmodel.cn/api/mcp/zread/mcp",
    sourceKind: "builtIn",
    sourceLabel: "Built-in",
    sourceDetail: "Zhipu Coding Plan",
    statusKind: "missingCredential",
    statusMessage: "Configure a Zhipu Coding Plan or Zhipu provider token to enable this server",
    mutationPolicy: "lockedIdentity",
    availabilityKind: "missingCredential",
    availabilityMessage: "Configure a Zhipu Coding Plan or Zhipu provider token to enable this server",
    lastCheckedAt: null,
    toolCount: null,
  },
  {
    id: "zhipu_vision",
    enabled: false,
    transport: "stdio",
    command: "npx",
    args: ["-y", "@z_ai/mcp-server"],
    env: [
      { key: "Z_AI_MODE", value: "ZHIPU" },
    ],
    cwd: null,
    url: null,
    bearerTokenEnvVar: null,
    headers: [],
    endpoint: "npx",
    sourceKind: "builtIn",
    sourceLabel: "Built-in",
    sourceDetail: "Zhipu Coding Plan",
    statusKind: "missingCredential",
    statusMessage: "Configure a Zhipu Coding Plan or Zhipu provider token to enable this server",
    mutationPolicy: "lockedIdentity",
    availabilityKind: "missingCredential",
    availabilityMessage: "Configure a Zhipu Coding Plan or Zhipu provider token to enable this server",
    lastCheckedAt: null,
    toolCount: null,
  },
];

export const previewLspServers: LspServerRecord[] = [
  {
    id: "rust-analyzer",
    displayName: "rust-analyzer",
    extensions: [".rs"],
    languageIds: ["rust"],
    availabilityKind: "available",
    availabilityMessage: "Available",
    lastCheckedAt: 1779688800,
    diagnosticCount: 0,
    activityKind: "idle",
    activityTitle: null,
    activityMessage: null,
    activityPercentage: null,
    lastError: null,
    lastErrorAt: null,
  },
];

const previewAssistantMarkdown = `✅ **Markdown 渲染检查完成**

| 检查项 | 状态 |
|--------|------|
| TypeScript 类型检查 | ✅ 通过 |
| Vite 生产构建 | ✅ 通过 |
| CSS 输出 | ✅ 24.32 kB |
| JS 输出 | ✅ 605.40 kB |

### 变更摘要

- 新增 \`MarkdownContent.tsx\`，统一渲染回答、计划和子代理摘要。
- 表格会在窄窗口中横向滚动，不会撑破 timeline。
- 链接只允许 [安全协议](https://example.com/pure-studio-markdown)。

\`\`\`tsx
<MarkdownContent content={message.content} />
\`\`\``;

const previewPlanMarkdown = `## Markdown 支持计划

1. 使用 \`marked.lexer\` 解析 GFM token。
2. 将 table / code / link 渲染为 Studio timeline markdown DOM。
3. 给预览和测试补齐覆盖。`;

const previewAgentTaskMarkdown = `审查 Markdown 渲染链路：

- 消息正文
- Plan 卡片
- Subagent summary`;

const previewAgentSummaryMarkdown = `**检查结论：** 表格和代码块可渲染。

| 路径 | 结果 |
|------|------|
| assistant | ✅ |
| plan | ✅ |
| subagent | ✅ |`;

export const previewAgents: AgentDto[] = [
  {
    id: "agent-preview-executor",
    sessionId: "preview-session",
    path: "/root/preview_executor",
    parentPath: null,
    role: "executor",
    task: previewAgentTaskMarkdown,
    status: "completed",
    summary: previewAgentSummaryMarkdown,
    depth: 1,
    error: null,
    runtimeUsage: {
      model: "deepseek-v4-flash",
      contextWindow: 1_000_000,
      latestContextTokens: 12_000,
      promptTokens: 24_000,
      completionTokens: 900,
      cachedPromptTokens: 8_000,
      totalTokens: 24_900,
      cacheHitRate: 0.33,
      estimatedCosts: [{ currency: "CNY", amount: 0.02 }],
      hasUnpricedUsage: false,
      updatedAt: 1779688800,
    },
    updatedAt: 1779688800,
  },
  {
    id: "agent-preview-reviewer",
    sessionId: "preview-session",
    path: "/root/preview_executor/reviewer",
    parentPath: "/root/preview_executor",
    role: "reviewer",
    task: "最终确认前审查嵌套工具输出。",
    status: "waiting",
    summary: "**等待中：** 正在等待嵌套 `bash` 审批。",
    depth: 2,
    error: null,
    runtimeUsage: null,
    updatedAt: 1779688860,
  },
];

export const previewAgentEvents: StudioAgentTimelineEvent[] = [
  {
    eventId: "preview-agent-event-1",
    sessionId: "preview-session",
    sequence: 1,
    createdAt: 1779688798,
    kind: {
      type: "spawnBegin",
      callId: "preview-spawn-1",
      senderPath: "/root",
      taskName: "preview_executor",
      prompt: "检查当前工作区变更并报告可能的下一步。",
      role: "executor",
      model: "deepseek-v4-flash",
      reasoningEffort: "high",
    },
  },
  {
    eventId: "preview-agent-event-2",
    sessionId: "preview-session",
    sequence: 2,
    createdAt: 1779688800,
    kind: {
      type: "spawnEnd",
      callId: "preview-spawn-1",
      senderPath: "/root",
      agentId: "agent-preview-executor",
      path: "/root/preview_executor",
      role: "executor",
      status: "running",
      prompt: "检查当前工作区变更并报告可能的下一步。",
      error: null,
    },
  },
  {
    eventId: "preview-agent-event-4",
    sessionId: "preview-session",
    sequence: 4,
    createdAt: 1779688858,
    kind: {
      type: "waitingBegin",
      callId: "preview-wait-1",
      senderPath: "/root/preview_executor",
    },
  },
];

export const previewConversationPartViews: ConversationPartView[] = [
  {
    turnId: "preview-turn-1",
    itemId: "preview-user-1",
    startedSequence: 0,
    kind: "text",
    status: "completed",
    createdAt: 1779688798,
    updatedAt: 1779688798,
    textChannel: "user",
    content: "修复窗口缩放后状态栏挤压和 timeline 过宽的问题",
    thinkingChunks: [],
  },
  {
    turnId: "preview-turn-1",
    itemId: "preview-thinking-1",
    startedSequence: 1,
    kind: "thinking",
    status: "completed",
    createdAt: 1779688799,
    updatedAt: 1779688799,
    content: "",
    thinkingChunks: [{ chunkIndex: 0, content: "Thought for 6s\n需要先找到 SessionStatusBar 和 ConversationPanel 的渲染边界。" }],
  },
  {
    turnId: "preview-turn-1",
    itemId: "preview-assistant-1",
    startedSequence: 2,
    kind: "text",
    status: "completed",
    createdAt: 1779688800,
    updatedAt: 1779688800,
    textChannel: "final",
    content: previewAssistantMarkdown,
    thinkingChunks: [],
  },
  {
    turnId: "preview-turn-1",
    itemId: "preview-plan-1",
    startedSequence: 3,
    kind: "plan",
    status: "completed",
    createdAt: 1779688801,
    updatedAt: 1779688801,
    content: previewPlanMarkdown,
    thinkingChunks: [],
  },
  {
    turnId: "preview-turn-1",
    itemId: "preview-tool-1",
    startedSequence: 4,
    kind: "tool",
    status: "completed",
    createdAt: 1779688802,
    updatedAt: 1779688803,
    content: "",
    thinkingChunks: [],
    tool: {
      toolCallId: "preview-tool-1",
      callId: "preview-tool-1",
      providerItemId: "preview-tool-1",
      name: "grep",
      arguments: "{\"query\":\"status|StatusBar|sessionRuntime|skills|mcp\",\"path\":\"src\"}",
      result: "Found 17 lines of output",
      exitCode: 0,
      timedOut: false,
    },
  },
  {
    turnId: "preview-turn-1",
    itemId: "preview-agent-1",
    startedSequence: 5,
    kind: "agent",
    status: "completed",
    createdAt: 1779688804,
    updatedAt: 1779688804,
    content: "",
    thinkingChunks: [],
    agent: {
      id: "agent-preview-executor",
      path: "/root/preview_executor",
      parentPath: null,
      role: "executor",
      task: previewAgentTaskMarkdown,
      status: "completed",
      summary: previewAgentSummaryMarkdown,
      depth: 1,
      error: null,
      reason: null,
    },
  },
  {
    turnId: "preview-turn-1",
    itemId: "preview-turn-1-turn",
    startedSequence: 6,
    kind: "turn",
    status: "completed",
    createdAt: 1779688860,
    updatedAt: 1779688860,
    content: "",
    thinkingChunks: [],
    usage: {
      promptTokens: 42000,
      completionTokens: 1200,
      cachedPromptTokens: 18000,
      totalTokens: 43200,
    },
  },
];

export const previewRoles: RoleRecord[] = [
  { key: "explorer", displayName: "Explorer", provider: "deepseek", model: "deepseek-v4-flash", effort: "high" },
  { key: "planner", displayName: "Planner", provider: "deepseek", model: "deepseek-v4-flash", effort: "high" },
  { key: "executor", displayName: "Executor", provider: "deepseek", model: "deepseek-v4-flash", effort: "high" },
  { key: "reviewer", displayName: "Reviewer", provider: "deepseek", model: "deepseek-v4-flash", effort: "high" },
];

export function createPreviewConfig(): ConfigPayload {
  const roles = previewRoles;
  return {
    toml: `schema_version = 4

[runtime]
permission_mode = "request-approval"
active_skills = ["rust", "git", "doc"]
active_mcp_servers = ["github", "filesystem"]

[mcp_servers.filesystem]
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "D:/workspace"]

[mcp_servers.github]
transport = "streamableHttp"
url = "https://example.com/mcp"
bearer_token_env_var = "GITHUB_MCP_TOKEN"

[roles.explorer]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[roles.planner]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[roles.executor]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[roles.reviewer]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[providers.deepseek]
provider_kind = "deep_seek"
name = "DeepSeek"
base_url = "https://api.deepseek.com"
default_model = "deepseek-v4-flash"

[[providers.deepseek.models]]
slug = "deepseek-v4-flash"
display_name = "DeepSeek V4 Flash"
context_window = 1000000
max_context_window = 1000000
max_output_tokens = 384000
currency = "CNY"
input_price_per_mtok = 1.0
output_price_per_mtok = 2.0
cache_read_price_per_mtok = 0.02
capabilities = { streaming = true, temperature = false, reasoning = true, web_search = false, input = ["text"], output = ["text"], tools = { function_calling = true, parallel_tool_calls = true, custom_tools = false, freeform_tools = false }, interleaved = { field = "reasoning_content" } }
truncation_policy = { mode = "tokens", limit = 10000 }

[providers.openai]
provider_kind = "open_ai"
name = "OpenAI"
base_url = "https://api.openai.com/v1"
default_model = "gpt-5.5"
`,
    permissionMode: "request-approval",
    instructions: {
      baseOverride: "",
      developer: "",
      user: "",
      projectDocMaxBytes: 65536,
      projectDocFallbackFilenames: [],
    },
    providers: [
      {
        ...makeProvider({
          id: "deepseek",
          templateKind: "deepseek",
          name: "DeepSeek",
          baseUrl: "https://api.deepseek.com",
          bearerToken: "",
          defaultModel: "deepseek-v4-flash",
          providerKind: "deep_seek",
          customModels: [],
        }),
        hasBearerToken: true,
        status: "Healthy",
      },
      makeProvider({
        id: "openai-work",
        templateKind: "openai",
        name: "OpenAI Work",
        baseUrl: "https://api.openai.com/v1",
        bearerToken: "",
        defaultModel: "gpt-5.5",
        providerKind: "open_ai",
        customModels: [],
      }),
      {
        ...makeProvider({
          id: "zhipu-coding-plan",
          templateKind: "zhipu-coding-plan",
          name: "Zhipu Coding Plan",
          baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4",
          bearerToken: "",
          defaultModel: "glm-5.2",
          providerKind: "zhipu",
          customModels: [],
        }),
        hasBearerToken: true,
        status: "Healthy",
      },
      makeProvider({
        id: "zhipu-plan-missing",
        templateKind: "zhipu-coding-plan",
        name: "Zhipu Plan Missing Key",
        baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4",
        bearerToken: "",
        defaultModel: "glm-5.2",
        providerKind: "zhipu",
        customModels: [],
      }),
    ],
    roles,
    templates: previewTemplates,
    mcpServers: previewMcpServers,
    configExists: false,
  };
}

import type {
  ChatMessage,
  ConfigPayload,
  ProjectRecord,
  RoleRecord,
  SessionRecord,
  SubagentActivity,
  SessionRuntime,
  TimelineItem,
} from "../types";
import { makeProvider, makeRole } from "./provider-mapper";
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
    mode: "manual",
    updatedAt: 1779688800,
  },
];

export const previewMessages: ChatMessage[] = [
  {
    role: "user",
    content: "修复窗口缩放后状态栏挤压和 timeline 过宽的问题",
    reasoningContent: null,
  },
  {
    role: "assistant",
    content: "我先定位状态栏和对话面板的布局代码，然后把状态栏元素收拢到右侧。",
    reasoningContent: "Thought for 6s\n需要先找到 SessionStatusBar 和 ConversationPanel 的渲染边界。",
  },
  {
    role: "tool",
    content: "Found 17 lines of output",
    reasoningContent: null,
    metadata: {
      tool_call_id: "preview-tool-1",
      tool_name: "grep",
      tool_call_arguments:
        "\"status|StatusBar|sessionRuntime|skills|mcp\" in src, glob: *.{ts,tsx,css}",
    },
  },
  {
    role: "user",
    content: "状态栏增加子代理数量，点击可以展开子代理列表",
    reasoningContent: null,
  },
  {
    role: "assistant",
    content: "已把子代理数量放到状态栏右侧，并增加向上展开的列表，运行中和等待中会排在前面。",
    reasoningContent: "Thought for 5s\n子代理入口应该和 skills/MCP 分开，否则数量语义不清楚。",
  },
];

export const previewSessionRuntime: SessionRuntime = {
  sessionId: "preview-session",
  model: "deepseek-v4-flash",
  contextWindow: 1_000_000,
  latestContextTokens: 42_600,
  promptTokens: 84_200,
  completionTokens: 3_100,
  cachedPromptTokens: 51_800,
  totalTokens: 87_300,
  cacheHitRate: 0.62,
  currency: "CNY",
  inputPricePerMTok: 8,
  outputPricePerMTok: 32,
  cacheReadPricePerMTok: 2,
  estimatedCost: 0.38,
  activeSkills: ["rust", "git", "doc"],
  activeMcpServers: ["github", "filesystem"],
  updatedAt: 1779688800,
};

export const previewSubagentEvents: SubagentActivity[] = [
  {
    eventId: "preview-subagent-1",
    id: "subagent-preview-executor",
    parentId: null,
    role: "executor",
    task: "检查当前工作区变更并报告可能的下一步。",
    status: "succeeded",
    summary: "工作区检查已完成；设置和路由代码可以验证。",
    depth: 1,
    error: null,
    updatedAt: 1779688800,
  },
  {
    eventId: "preview-subagent-2",
    id: "subagent-preview-reviewer",
    parentId: "subagent-preview-executor",
    role: "reviewer",
    task: "最终确认前审查嵌套工具输出。",
    status: "awaitingToolApproval",
    summary: "正在等待嵌套 bash 审批。",
    depth: 2,
    error: null,
    updatedAt: 1779688860,
  },
];

export const previewTimelineItems: TimelineItem[] = [
  {
    kind: "turn",
    sequence: 0,
    timestamp: 1779688800,
    turnId: "preview-turn-1",
    turnStatus: "started",
  },
  {
    kind: "inference",
    sequence: 1,
    timestamp: 1779688801,
    inferenceModel: "deepseek-v4-flash",
  },
  {
    kind: "tool_call",
    sequence: 2,
    timestamp: 1779688802,
    toolCallId: "preview-tool-1",
    toolName: "grep",
    toolArguments:
      "\"status|StatusBar|sessionRuntime|skills|mcp\" in src, glob: *.{ts,tsx,css}",
    toolStatus: "completed",
    toolResult: "Found 17 lines of output",
  },
  {
    kind: "turn",
    sequence: 3,
    timestamp: 1779688860,
    turnId: "preview-turn-1",
    turnStatus: "completed",
    turnModel: "deepseek-v4-flash",
    turnUsage: {
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
    toml: `schema_version = 1

[runtime]
active_skills = ["rust", "git", "doc"]
active_mcp_servers = ["github", "filesystem"]

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
name = "DeepSeek"
base_url = "https://api.deepseek.com"
default_model = "deepseek-v4-flash"
wire_api = "chat"

[[providers.deepseek.models]]
slug = "deepseek-v4-flash"
display_name = "DeepSeek V4 Flash"
context_window = 1000000
max_context_window = 1000000
max_output_tokens = 384000
currency = "CNY"
input_price_per_mtok = 8.0
output_price_per_mtok = 32.0
cache_read_price_per_mtok = 2.0
reasoning_efforts = ["high", "max"]
capabilities = ["streaming", "function_calling", "parallel_tool_calls", "reasoning"]
input_modalities = ["text"]
truncation_policy = { mode = "tokens", limit = 10000 }

[providers.openai]
name = "OpenAI"
base_url = "https://api.openai.com/v1"
default_model = "gpt-4.1"
wire_api = "responses"
`,
    providers: [
      makeProvider({
        id: "deepseek",
        templateKind: "deepseek",
        name: "DeepSeek",
        baseUrl: "https://api.deepseek.com",
        bearerToken: "",
        defaultModel: "deepseek-v4-flash",
        wireApi: "chat",
        customModels: [],
      }),
      makeProvider({
        id: "openai-work",
        templateKind: "openai",
        name: "OpenAI Work",
        baseUrl: "https://api.openai.com/v1",
        bearerToken: "",
        defaultModel: "gpt-4.1",
        wireApi: "responses",
        customModels: [],
      }),
    ],
    roles,
    templates: previewTemplates,
    configExists: false,
  };
}

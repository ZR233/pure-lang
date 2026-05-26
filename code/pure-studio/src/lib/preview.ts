import type {
  ChatMessage,
  ConfigPayload,
  ProjectRecord,
  RoleRecord,
  SessionRecord,
  SubagentActivity,
  SessionRuntime,
} from "../types";
import { makeProvider, makeRole } from "./provider-mapper";
import { previewTemplates } from "./templates";

export { previewTemplates };

export const previewProjects: ProjectRecord[] = [
  {
    id: "pure-lang",
    name: "pure-lang",
    path: "C:\\Users\\zhoudongsheng\\Documents\\opensource\\pure-lang",
    updatedAt: 1779688800,
  },
];

export const previewSessions: SessionRecord[] = [
  {
    id: "preview-session",
    projectId: "pure-lang",
    title: "介绍项目",
    mode: "manual",
    updatedAt: 1779688800,
  },
];

export const previewMessages: ChatMessage[] = [
  {
    role: "user",
    content: "修复 Unknown tool: \"\" 的问题",
    reasoningContent: null,
  },
  {
    role: "assistant",
    content:
      "我将修复工具调用解析中工具名为空的问题。首先分析 SSE 流中 tool_call 的 delta 结构。\n\n已读取 3 个文件：sse.rs、openai.rs、wire_api.rs\n\n问题原因：Chat Completions 流中，后续参数片段只带 index 不带 name，导致累积的工具名丢失。\n\n修复完成，工具调用将正确识别工具名。",
    reasoningContent: null,
  },
  {
    role: "user",
    content: "帮我再跑一次 clippy",
    reasoningContent: null,
  },
  {
    role: "assistant",
    content: "好的，正在运行 clippy 检查代码质量。\n\ncargo clippy --all-targets --all-features -- -D warnings\n\n检查通过，没有发现新的 warning。",
    reasoningContent: null,
  },
];

export const previewSessionRuntime: SessionRuntime = {
  sessionId: "preview-session",
  model: "deepseek-v4-flash",
  contextWindow: 1_000_000,
  latestContextTokens: 128_000,
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

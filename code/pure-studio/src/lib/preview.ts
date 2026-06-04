import type {
  ConfigPayload,
  AgentDto,
  AgentTimelineEvent,
  ProjectRecord,
  RoleRecord,
  SessionRecord,
  SessionRuntime,
  SkillRecord,
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
    mode: "auto",
    updatedAt: 1779688800,
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

export const previewAgents: AgentDto[] = [
  {
    id: "agent-preview-executor",
    sessionId: "preview-session",
    path: "/root/preview_executor",
    parentPath: null,
    role: "executor",
    task: "检查当前工作区变更并报告可能的下一步。",
    status: "completed",
    summary: "工作区检查已完成；设置和路由代码可以验证。",
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
    summary: "正在等待嵌套 bash 审批。",
    depth: 2,
    error: null,
    runtimeUsage: null,
    updatedAt: 1779688860,
  },
];

export const previewAgentEvents: AgentTimelineEvent[] = [
  {
    eventId: "preview-agent-event-1",
    sessionId: "preview-session",
    sequence: 1,
    kind: "spawnBegin",
    agentId: null,
    path: "/root/preview_executor",
    parentPath: null,
    payload: {
      collabAgentSpawnBegin: {
        callId: "preview-spawn-1",
        startedAt: 1779688798,
        senderPath: "/root",
        taskName: "preview_executor",
        prompt: "检查当前工作区变更并报告可能的下一步。",
        role: "executor",
        model: "deepseek-v4-flash",
        reasoningEffort: "high",
      },
    },
    createdAt: 1779688798,
  },
  {
    eventId: "preview-agent-event-2",
    sessionId: "preview-session",
    sequence: 2,
    kind: "spawnEnd",
    agentId: "agent-preview-executor",
    path: "/root/preview_executor",
    parentPath: null,
    payload: {
      collabAgentSpawnEnd: {
        callId: "preview-spawn-1",
        completedAt: 1779688800,
        senderPath: "/root",
        agentId: "agent-preview-executor",
        path: "/root/preview_executor",
        role: "executor",
        status: "running",
        prompt: "检查当前工作区变更并报告可能的下一步。",
        error: null,
      },
    },
    createdAt: 1779688800,
  },
  {
    eventId: "preview-agent-event-3",
    sessionId: "preview-session",
    sequence: 3,
    kind: "agentStatus",
    agentId: "agent-preview-executor",
    path: "/root/preview_executor",
    parentPath: null,
    payload: {
      agentStateChanged: {
        id: "agent-preview-executor",
        path: "/root/preview_executor",
        parentPath: null,
        role: "executor",
        task: "检查当前工作区变更并报告可能的下一步。",
        status: "completed",
        summary: "工作区检查已完成；设置和路由代码可以验证。",
        depth: 1,
        error: null,
        reason: null,
        budgetLimitKind: null,
        budgetUsage: null,
        updatedAt: 1779688800,
      },
    },
    createdAt: 1779688800,
  },
  {
    eventId: "preview-agent-event-4",
    sessionId: "preview-session",
    sequence: 4,
    kind: "waitingBegin",
    agentId: "agent-preview-reviewer",
    path: "/root/preview_executor/reviewer",
    parentPath: "/root/preview_executor",
    payload: {
      collabWaitingBegin: {
        callId: "preview-wait-1",
        startedAt: 1779688858,
        senderPath: "/root/preview_executor",
      },
    },
    createdAt: 1779688858,
  },
];

export const previewTimelineItems: TimelineItem[] = [
  {
    turnId: "preview-turn-1",
    itemId: "preview-user-1",
    sequence: 0,
    kind: "text",
    status: "completed",
    createdAt: 1779688798,
    updatedAt: 1779688798,
    role: "user",
    content: "修复窗口缩放后状态栏挤压和 timeline 过宽的问题",
    thinkingChunks: [],
  },
  {
    turnId: "preview-turn-1",
    itemId: "preview-thinking-1",
    sequence: 1,
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
    sequence: 2,
    kind: "text",
    status: "completed",
    createdAt: 1779688800,
    updatedAt: 1779688800,
    role: "assistant",
    content: "我先定位状态栏和对话面板的布局代码，然后把状态栏元素收拢到右侧。",
    thinkingChunks: [],
  },
  {
    turnId: "preview-turn-1",
    itemId: "preview-tool-1",
    sequence: 3,
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
    itemId: "preview-turn-1-turn",
    sequence: 4,
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
    toml: `schema_version = 1

[runtime]
permission_mode = "workspace-write"
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
input_price_per_mtok = 1.0
output_price_per_mtok = 2.0
cache_read_price_per_mtok = 0.02
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
    permissionMode: "workspace-write",
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

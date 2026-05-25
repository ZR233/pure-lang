import type {
  ChatMessage,
  ConfigPayload,
  ProjectRecord,
  RoleRecord,
  SessionRecord,
  SubagentActivity,
} from "../types";
import { makeProvider, makeRole } from "./provider-mapper";
import { previewTemplates } from "./templates";

export { previewTemplates };

export const previewProjects: ProjectRecord[] = [
  {
    id: "pure-lang",
    name: "pure-lang",
    path: "D:\\Users\\zrufo\\Documents\\opensource\\pure-lang",
    updatedAt: 1779688800,
  },
];

export const previewSessions: SessionRecord[] = [
  {
    id: "preview-session",
    projectId: "pure-lang",
    title: "模型服务设置预览",
    mode: "manual",
    updatedAt: 1779688800,
  },
];

export const previewMessages: ChatMessage[] = [
  {
    role: "assistant",
    content: "Pure Studio 预览状态已加载，可用于浏览器布局检查。",
    reasoningContent: null,
  },
];

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

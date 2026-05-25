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
    title: "Provider settings preview",
    mode: "manual",
    updatedAt: 1779688800,
  },
];

export const previewMessages: ChatMessage[] = [
  {
    role: "assistant",
    content: "Pure Studio preview state is loaded for browser layout checks.",
    reasoningContent: null,
  },
];

export const previewSubagentEvents: SubagentActivity[] = [
  {
    eventId: "preview-subagent-1",
    id: "subagent-preview-executor",
    parentId: null,
    role: "executor",
    task: "Inspect the current workspace changes and report the likely next step.",
    status: "succeeded",
    summary: "Workspace inspection finished; settings and routing code are ready to verify.",
    depth: 1,
    error: null,
    updatedAt: 1779688800,
  },
  {
    eventId: "preview-subagent-2",
    id: "subagent-preview-reviewer",
    parentId: "subagent-preview-executor",
    role: "reviewer",
    task: "Review nested tool output before finalizing.",
    status: "awaitingToolApproval",
    summary: "Waiting on a nested bash approval.",
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

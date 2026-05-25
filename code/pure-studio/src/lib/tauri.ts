import { invoke } from "@tauri-apps/api/core";
import {
  BootstrapPayload,
  ConfigPayload,
  ModelRecord,
  ProjectRecord,
  ProjectSelectionPayload,
  ProviderInput,
  ProviderRecord,
  ProviderSettingsInput,
  ProviderTemplateRecord,
  RoleInput,
  RoleRecord,
  RunPromptResponse,
  SessionRecord,
  SessionSelectionPayload,
  SubagentActivity,
} from "../types";

type TauriWindow = Window & {
  __TAURI_INTERNALS__?: unknown;
};

const previewTemplates: ProviderTemplateRecord[] = [
  {
    id: "deepseek",
    name: "DeepSeek",
    baseUrl: "https://api.deepseek.com",
    defaultModel: "deepseek-v4-flash",
    wireApi: "chat",
    defaultModels: [
      {
        slug: "deepseek-v4-flash",
        displayName: "DeepSeek V4 Flash",
        description: "DeepSeek fast reasoning model with thinking mode.",
        contextWindow: 1_000_000,
        maxContextWindow: 1_000_000,
        maxOutputTokens: 384_000,
        reasoningEfforts: ["high", "max"],
        capabilities: ["streaming", "function_calling", "parallel_tool_calls", "reasoning"],
        inputModalities: ["text"],
        truncationMode: "tokens",
        truncationLimit: 10_000,
      },
      {
        slug: "deepseek-v4-pro",
        displayName: "DeepSeek V4 Pro",
        description: "DeepSeek flagship reasoning model with thinking mode.",
        contextWindow: 1_000_000,
        maxContextWindow: 1_000_000,
        maxOutputTokens: 384_000,
        reasoningEfforts: ["high", "max"],
        capabilities: ["streaming", "function_calling", "parallel_tool_calls", "reasoning"],
        inputModalities: ["text"],
        truncationMode: "tokens",
        truncationLimit: 10_000,
      },
    ],
  },
  {
    id: "openai",
    name: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    defaultModel: "gpt-4.1",
    wireApi: "responses",
    defaultModels: [
      {
        slug: "gpt-4.1",
        displayName: "GPT-4.1",
        contextWindow: 1_050_000,
        maxContextWindow: 1_050_000,
        maxOutputTokens: 128_000,
        reasoningEfforts: ["medium", "high"],
        capabilities: [
          "streaming",
          "function_calling",
          "vision",
          "parallel_tool_calls",
          "reasoning",
          "web_search",
        ],
        inputModalities: ["text", "image"],
        truncationMode: "tokens",
        truncationLimit: 10_000,
      },
      {
        slug: "o4-mini",
        displayName: "o4 mini",
        contextWindow: 400_000,
        maxContextWindow: 400_000,
        maxOutputTokens: 128_000,
        reasoningEfforts: ["medium"],
        capabilities: [
          "streaming",
          "function_calling",
          "vision",
          "parallel_tool_calls",
          "reasoning",
          "web_search",
        ],
        inputModalities: ["text", "image"],
        truncationMode: "tokens",
        truncationLimit: 10_000,
      },
    ],
  },
];

const previewProjects: ProjectRecord[] = [
  {
    id: "pure-lang",
    name: "pure-lang",
    path: "D:\\Users\\zrufo\\Documents\\opensource\\pure-lang",
    updatedAt: 1779688800,
  },
];

const previewSessions: SessionRecord[] = [
  {
    id: "preview-session",
    projectId: "pure-lang",
    title: "Provider settings preview",
    mode: "manual",
    updatedAt: 1779688800,
  },
];

const previewMessages = [
  {
    role: "assistant" as const,
    content: "Pure Studio preview state is loaded for browser layout checks.",
    reasoningContent: null,
  },
];

const previewSubagentEvents: SubagentActivity[] = [
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

const previewRoles: RoleRecord[] = [
  {
    key: "explorer",
    displayName: "探索者",
    provider: "deepseek",
    model: "deepseek-v4-flash",
    effort: "high",
  },
  {
    key: "planner",
    displayName: "计划者",
    provider: "deepseek",
    model: "deepseek-v4-flash",
    effort: "high",
  },
  {
    key: "executor",
    displayName: "执行者",
    provider: "deepseek",
    model: "deepseek-v4-flash",
    effort: "high",
  },
  {
    key: "reviewer",
    displayName: "审查者",
    provider: "deepseek",
    model: "deepseek-v4-flash",
    effort: "high",
  },
];

let previewConfig: ConfigPayload = {
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
  roles: previewRoles,
  templates: previewTemplates,
  configExists: false,
};

export function isTauriRuntime() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in (window as TauriWindow);
}

export function bootstrapStudio() {
  if (!isTauriRuntime()) {
    return Promise.resolve(
      clone({
        projects: previewProjects,
        selectedProjectId: previewProjects[0]?.id ?? null,
        sessions: previewSessions,
        selectedSessionId: previewSessions[0]?.id ?? null,
        messages: previewMessages,
        subagentEvents: previewSubagentEvents,
        config: previewConfig,
      }),
    );
  }
  return invoke<BootstrapPayload>("bootstrap_studio");
}

export function openProject(path: string) {
  if (!isTauriRuntime()) {
    const pathParts = path.split(/[\\/]/).filter(Boolean);
    const project = {
      id: "preview-project",
      name: pathParts[pathParts.length - 1] ?? "Preview",
      path,
      updatedAt: Math.floor(Date.now() / 1000),
    };
    return Promise.resolve(
      clone({
        projectId: project.id,
        projects: [project, ...previewProjects],
        sessions: previewSessions,
        selectedSessionId: previewSessions[0]?.id ?? null,
        messages: previewMessages,
        subagentEvents: previewSubagentEvents,
      }),
    );
  }
  return invoke<ProjectSelectionPayload>("open_project", { path });
}

export function selectProject(projectId: string) {
  if (!isTauriRuntime()) {
    return Promise.resolve(
      clone({
        projectId,
        projects: previewProjects,
        sessions: previewSessions,
        selectedSessionId: previewSessions[0]?.id ?? null,
        messages: previewMessages,
        subagentEvents: previewSubagentEvents,
      }),
    );
  }
  return invoke<ProjectSelectionPayload>("select_project", { projectId });
}

export function createSession(projectId: string, title?: string) {
  if (!isTauriRuntime()) {
    const session = {
      id: "preview-new-session",
      projectId,
      title: title ?? "New session",
      mode: "manual",
      updatedAt: Math.floor(Date.now() / 1000),
    };
    return Promise.resolve(
      clone({
        sessionId: session.id,
        sessions: [session, ...previewSessions],
        messages: [],
        subagentEvents: [],
      }),
    );
  }
  return invoke<SessionSelectionPayload>("create_session", {
    projectId,
    title: title ?? null,
  });
}

export function selectSession(sessionId: string) {
  if (!isTauriRuntime()) {
    return Promise.resolve(
      clone({
        sessionId,
        sessions: previewSessions,
        messages: previewMessages,
        subagentEvents: previewSubagentEvents,
      }),
    );
  }
  return invoke<SessionSelectionPayload>("select_session", { sessionId });
}

export function runPrompt(sessionId: string, prompt: string) {
  if (!isTauriRuntime()) {
    return Promise.resolve(
      clone({
        sessionId,
        sessions: previewSessions,
        messages: [
          ...previewMessages,
          { role: "user" as const, content: prompt, reasoningContent: null },
        ],
        subagentEvents: [
          ...previewSubagentEvents,
          {
            eventId: `preview-subagent-${Date.now()}`,
            id: "subagent-preview-latest",
            parentId: null,
            role: "executor",
            task: prompt,
            status: "succeeded" as const,
            summary: "Preview run completed.",
            depth: 1,
            error: null,
            updatedAt: Math.floor(Date.now() / 1000),
          },
        ],
      }),
    );
  }
  return invoke<RunPromptResponse>("run_prompt", { sessionId, prompt });
}

export function approveTool(approvalId: string) {
  if (!isTauriRuntime()) {
    return Promise.resolve(void approvalId);
  }
  return invoke<void>("approve_tool", { approvalId });
}

export function denyTool(approvalId: string, reason?: string) {
  if (!isTauriRuntime()) {
    return Promise.resolve(void approvalId);
  }
  return invoke<void>("deny_tool", { approvalId, reason: reason ?? null });
}

export function loadConfig() {
  if (!isTauriRuntime()) {
    return Promise.resolve(clone(previewConfig));
  }
  return invoke<ConfigPayload>("load_config");
}

export function saveConfig(toml: string) {
  if (!isTauriRuntime()) {
    previewConfig = { ...previewConfig, toml, configExists: true };
    return Promise.resolve(clone(previewConfig));
  }
  return invoke<ConfigPayload>("save_config", { toml });
}

export function saveProviderSettings(input: ProviderSettingsInput) {
  if (!isTauriRuntime()) {
    previewConfig = {
      ...previewConfig,
      toml: renderPreviewToml(input),
      providers: input.providers.map(makeProvider),
      roles: input.roles.map(makeRole),
      configExists: true,
    };
    return Promise.resolve(clone(previewConfig));
  }
  return invoke<ConfigPayload>("save_provider_settings", { input });
}

function makeProvider(input: ProviderInput): ProviderRecord {
  const template = previewTemplates.find((item) => item.id === input.templateKind);
  const defaultModels = template?.defaultModels.map(cloneModel) ?? [];
  const customModels = input.customModels.map(cloneModel);
  const models = [...defaultModels, ...customModels];
  const defaultModel = models.some((model) => model.slug === input.defaultModel)
    ? input.defaultModel
    : (models[0]?.slug ?? "");
  return {
    id: input.id,
    templateKind: input.templateKind,
    name: input.name,
    subtitle: `${input.name || input.id} Platform`,
    status: input.bearerToken.trim() ? "Healthy" : "Needs setup",
    baseUrl: input.baseUrl,
    bearerToken: input.bearerToken,
    defaultModel,
    modelCount: models.length.toString(),
    updatedAt: "Preview",
    wireApi: input.wireApi,
    models,
    defaultModels,
    customModels,
  };
}

function makeRole(input: RoleInput): RoleRecord {
  const displayName =
    previewRoles.find((role) => role.key === input.key)?.displayName ?? input.key;
  return {
    key: input.key,
    displayName,
    provider: input.provider,
    model: input.model,
    effort: input.effort,
  };
}

function cloneModel(model: ModelRecord): ModelRecord {
  return {
    slug: model.slug,
    displayName: model.displayName,
    description: model.description ?? null,
    contextWindow: model.contextWindow ?? null,
    maxContextWindow: model.maxContextWindow ?? null,
    autoCompactTokenLimit: model.autoCompactTokenLimit ?? null,
    defaultTemperature: model.defaultTemperature ?? null,
    maxOutputTokens: model.maxOutputTokens ?? null,
    reasoningEfforts: [...model.reasoningEfforts],
    capabilities: [...(model.capabilities ?? [])],
    inputModalities: [...(model.inputModalities ?? [])],
    truncationMode: model.truncationMode,
    truncationLimit: model.truncationLimit,
  };
}

function renderPreviewToml(input: ProviderSettingsInput) {
  return [
    "schema_version = 1",
    "",
    ...input.roles.flatMap((role) => [
      `[roles.${role.key}]`,
      `provider = "${role.provider}"`,
      `model = "${role.model}"`,
      `effort = "${role.effort}"`,
      "",
    ]),
    ...input.providers.flatMap((provider) => [
      `[providers.${provider.id}]`,
      `name = "${provider.name}"`,
      `base_url = "${provider.baseUrl}"`,
      `bearer_token = "${provider.bearerToken}"`,
      `default_model = "${provider.defaultModel}"`,
      `wire_api = "${provider.wireApi}"`,
      "",
    ]),
  ].join("\n");
}

function clone<T>(value: T): T {
  if (typeof structuredClone === "function") {
    return structuredClone(value);
  }
  return JSON.parse(JSON.stringify(value)) as T;
}

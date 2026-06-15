import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  BootstrapPayload,
  AttachmentRecord,
  CompileMode,
  ConfigPayload,
  DiscoveredSkillsPayload,
  InstructionsInput,
  McpSettingsInput,
  McpServerInput,
  PermissionMode,
  InteractionResolution,
  PlanLifecycleResponse,
  ProjectSelectionPayload,
  ProviderSettingsInput,
  ProviderUsagesPayload,
  ResolveInteractionResponse,
  RunPromptResponse,
  SessionStatePayload,
  SessionSelectionPayload,
  SessionTimeline,
  StudioEventsPayload,
  SubmitPromptResponse,
} from "../types";
import {
  createPreviewConfig,
  previewProjects,
  previewSessions,
  previewSessionRuntime,
  previewAgents,
  previewAgentEvents,
  previewTimelineEvents,
  previewTimelineItems,
  previewTimelineEventRecords,
  nextTimelineSequenceForItems,
  previewSkills,
  previewLspServers,
} from "./preview";
import { makeProvider, makeRole } from "./provider-mapper";
import { renderPreviewToml } from "./toml-renderer";

type TauriGlobal = typeof globalThis & {
  __TAURI_INTERNALS__?: unknown;
  __TAURI__?: unknown;
  isTauri?: boolean;
};

let previewConfig = createPreviewConfig();
const previewInterruptedSessions = new Set<string>();

function clone<T>(value: T): T {
  if (typeof structuredClone === "function") {
    return structuredClone(value);
  }
  return JSON.parse(JSON.stringify(value)) as T;
}

export function isTauriRuntime() {
  if (typeof globalThis === "undefined") {
    return false;
  }
  if (isTauri()) {
    return true;
  }
  const tauriGlobal = globalThis as TauriGlobal;
  return Boolean(tauriGlobal.__TAURI_INTERNALS__ || tauriGlobal.__TAURI__ || tauriGlobal.isTauri);
}

export function bootstrapStudio() {
  if (!isTauriRuntime()) {
    return Promise.resolve(
      clone({
        projects: previewProjects,
        selectedProjectId: previewProjects[0]?.id ?? null,
        sessions: previewSessions,
        selectedSessionId: previewSessions[0]?.id ?? null,
        agentEvents: previewAgentEvents,
        agents: previewAgents,
        sessionRuntime: previewSessionRuntime,
        interactions: [],
        lspHealth: {
          lspServers: previewLspServers,
          activeLspServers: previewSessionRuntime.activeLspServers,
        },
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
        selectedProjectId: project.id,
        projects: [project, ...previewProjects],
        sessions: previewSessions,
        selectedSessionId: previewSessions[0]?.id ?? null,
        agentEvents: previewAgentEvents,
        agents: previewAgents,
        sessionRuntime: previewSessionRuntime,
        interactions: [],
        lspHealth: {
          lspServers: previewLspServers,
          activeLspServers: previewSessionRuntime.activeLspServers,
        },
      }),
    );
  }
  return invoke<ProjectSelectionPayload>("open_project", { path });
}

export function selectProject(projectId: string) {
  if (!isTauriRuntime()) {
    return Promise.resolve(
      clone({
        selectedProjectId: projectId,
        projects: previewProjects,
        sessions: previewSessions,
        selectedSessionId: previewSessions[0]?.id ?? null,
        agentEvents: previewAgentEvents,
        agents: previewAgents,
        sessionRuntime: previewSessionRuntime,
        lspHealth: {
          lspServers: previewLspServers,
          activeLspServers: previewSessionRuntime.activeLspServers,
        },
      }),
    );
  }
  return invoke<ProjectSelectionPayload>("select_project", { projectId });
}

export function archiveProject(projectId: string, selectedProjectId?: string | null) {
  if (!isTauriRuntime()) {
    const projects = previewProjects.filter((project) => project.id !== projectId);
    const nextSelectedProjectId =
      selectedProjectId && selectedProjectId !== projectId && projects.some((project) => project.id === selectedProjectId)
        ? selectedProjectId
        : projects[0]?.id ?? null;
    return Promise.resolve(
      clone({
        selectedProjectId: nextSelectedProjectId,
        projects,
        sessions: nextSelectedProjectId ? previewSessions : [],
        selectedSessionId: nextSelectedProjectId ? previewSessions[0]?.id ?? null : null,
        agentEvents: nextSelectedProjectId ? previewAgentEvents : [],
        agents: nextSelectedProjectId ? previewAgents : [],
        sessionRuntime: nextSelectedProjectId ? previewSessionRuntime : null,
        lspHealth: {
          lspServers: previewLspServers,
          activeLspServers: nextSelectedProjectId ? previewSessionRuntime.activeLspServers : [],
        },
      }),
    );
  }
  return invoke<ProjectSelectionPayload>("archive_project", { projectId, selectedProjectId: selectedProjectId ?? null });
}

export function createSession(projectId: string, title?: string) {
  if (!isTauriRuntime()) {
    const session = {
      id: "preview-new-session",
      projectId,
      title: title ?? "New session",
      mode: "auto",
      updatedAt: Math.floor(Date.now() / 1000),
      visibility: "active" as const,
    };
    return Promise.resolve(
      clone({
        sessionId: session.id,
        sessions: [session, ...previewSessions],
        agentEvents: [],
        agents: [],
        sessionRuntime: {
          ...previewSessionRuntime,
          sessionId: session.id,
          usage: {
            ...previewSessionRuntime.usage,
            latestContextTokens: 0,
            promptTokens: 0,
            completionTokens: 0,
            cachedPromptTokens: 0,
            totalTokens: 0,
            cacheHitRate: null,
            estimatedCosts: [],
            hasUnpricedUsage: false,
          },
        },
      }),
    );
  }
  return invoke<SessionSelectionPayload>("create_session", {
    projectId,
    title: title ?? null,
  });
}

export function deleteSession(sessionId: string, selectedSessionId?: string | null) {
  if (!isTauriRuntime()) {
    const sessions = previewSessions.filter((session) => session.id !== sessionId);
    const nextSelectedSessionId =
      selectedSessionId && selectedSessionId !== sessionId && sessions.some((session) => session.id === selectedSessionId)
        ? selectedSessionId
        : sessions[0]?.id ?? null;
    return Promise.resolve(
      clone({
        projectId: previewProjects[0]?.id ?? "preview-project",
        projects: previewProjects,
        sessions,
        selectedSessionId: nextSelectedSessionId,
        agentEvents: nextSelectedSessionId ? previewAgentEvents : [],
        agents: nextSelectedSessionId ? previewAgents : [],
        sessionRuntime: nextSelectedSessionId ? previewSessionRuntime : null,
        interactions: [],
        lspHealth: {
          lspServers: previewLspServers,
          activeLspServers: previewSessionRuntime.activeLspServers,
        },
      }),
    );
  }
  return invoke<ProjectSelectionPayload>("delete_session", {
    sessionId,
    selectedSessionId: selectedSessionId ?? null,
  });
}

export function selectSession(sessionId: string) {
  if (!isTauriRuntime()) {
    return Promise.resolve(
      clone({
        sessionId,
        sessions: previewSessions,
        agentEvents: previewAgentEvents,
        agents: previewAgents,
        sessionRuntime: previewSessionRuntime,
        interactions: [],
      }),
    );
  }
  return invoke<SessionSelectionPayload>("select_session", { sessionId });
}

export function setSessionMode(sessionId: string, mode: CompileMode) {
  if (!isTauriRuntime()) {
    previewSessions.forEach((session) => {
      if (session.id === sessionId) {
        session.mode = mode;
        session.updatedAt = Math.floor(Date.now() / 1000);
      }
    });
    return Promise.resolve(
      clone({
        sessionId,
        sessions: previewSessions,
        agentEvents: previewAgentEvents,
        agents: previewAgents,
        sessionRuntime: previewSessionRuntime,
        interactions: [],
      }),
    );
  }
  return invoke<SessionSelectionPayload>("set_session_mode", { sessionId, mode });
}

export function runPrompt(sessionId: string, prompt: string) {
  if (isTauriRuntime()) {
    return Promise.reject(new Error("run_prompt has been replaced by submit_prompt"));
  }
  previewInterruptedSessions.delete(sessionId);
  return new Promise<RunPromptResponse>((resolve) => {
    window.setTimeout(() => {
      const interrupted = previewInterruptedSessions.delete(sessionId);
      const now = Math.floor(Date.now() / 1000);
      const latestAgent = {
        id: "agent-preview-latest",
        sessionId,
        path: "/root/preview_latest",
        parentPath: null,
        role: "executor",
        task: prompt,
        status: interrupted ? "interrupted" as const : "completed" as const,
        summary: interrupted ? "预览运行已停止。" : "预览运行已完成。",
        depth: 1,
        error: null,
        reason: interrupted ? "interrupted" : null,
        budgetLimitKind: null,
        budgetUsage: null,
        updatedAt: now,
      };
      resolve(clone({
        sessionId,
        sessions: previewSessions,
        agentEvents: [
          ...previewAgentEvents,
          {
            eventId: `preview-agent-event-${Date.now()}`,
            sessionId,
            sequence: previewAgentEvents.length + 1,
            kind: "agentStatus",
            agentId: latestAgent.id,
            path: latestAgent.path,
            parentPath: null,
            payload: {
              agentStateChanged: {
                id: latestAgent.id,
                path: latestAgent.path,
                parentPath: latestAgent.parentPath,
                role: latestAgent.role,
                task: latestAgent.task,
                status: latestAgent.status,
                summary: latestAgent.summary,
                depth: latestAgent.depth,
                error: latestAgent.error,
                reason: latestAgent.reason,
                budgetLimitKind: latestAgent.budgetLimitKind,
                budgetUsage: latestAgent.budgetUsage,
                updatedAt: latestAgent.updatedAt,
              },
            },
            createdAt: now,
          },
        ],
        agents: [...previewAgents, latestAgent],
        sessionRuntime: {
          ...previewSessionRuntime,
          usage: {
            ...previewSessionRuntime.usage,
            promptTokens: previewSessionRuntime.usage.promptTokens + 1200,
            completionTokens: previewSessionRuntime.usage.completionTokens + 260,
            totalTokens: previewSessionRuntime.usage.totalTokens + 1460,
          },
        },
        timelineEvents: previewTimelineEventRecords(sessionId, [
          ...previewTimelineItems,
          {
            turnId: "preview-turn-latest",
            itemId: `preview-user-${Date.now()}`,
            startedSequence: previewTimelineItems.length + 8,
            kind: "text" as const,
            status: "completed" as const,
            createdAt: now,
            updatedAt: now,
            textChannel: "user" as const,
            content: prompt,
            thinkingChunks: [],
          },
          {
            turnId: "preview-turn-latest",
            itemId: `preview-turn-latest-${Date.now()}`,
            startedSequence: previewTimelineItems.length + 10,
            kind: "turn" as const,
            status: interrupted ? "interrupted" as const : "completed" as const,
            createdAt: Math.floor(Date.now() / 1000),
            updatedAt: Math.floor(Date.now() / 1000),
            content: "",
            thinkingChunks: [],
            usage: {
              promptTokens: 1200,
              completionTokens: 260,
              cachedPromptTokens: 0,
              totalTokens: 1460,
            },
          },
        ]),
        planStates: [],
        interactions: [],
        timelineNextSequence: previewTimelineItems.length + 11,
        turnStatus: interrupted ? "aborted" as const : "completed" as const,
        turnAbortReason: interrupted ? "interrupted" : null,
        turnError: null,
      }));
    }, 900);
  });
}

export function createPromptAttachment(sessionId: string, dataUrl: string, filename?: string | null) {
  if (!isTauriRuntime()) {
    return Promise.resolve<AttachmentRecord>({
      id: `preview-attachment-${Date.now()}`,
      sessionId,
      mediaType: dataUrl.slice(5, dataUrl.indexOf(";base64")) || "image/png",
      filename: filename ?? null,
      byteSize: dataUrl.length,
      width: null,
      height: null,
      createdAt: Math.floor(Date.now() / 1000),
      dataUrl,
    });
  }
  return invoke<AttachmentRecord>("create_prompt_attachment", {
    sessionId,
    dataUrl,
    filename: filename ?? null,
  });
}

export function submitPromptCommand(sessionId: string, prompt: string, attachmentIds: string[] = []) {
  if (!isTauriRuntime()) {
    previewInterruptedSessions.delete(sessionId);
    return Promise.resolve<SubmitPromptResponse>(
      clone({
        sessionId,
        turnId: `preview-turn-${Date.now()}`,
        cursor: previewTimelineItems.length,
      }),
    );
  }
  return invoke<SubmitPromptResponse>("submit_prompt", { sessionId, prompt, attachmentIds });
}

export function resolveInteraction(interactionId: string, resolution: InteractionResolution) {
  if (!isTauriRuntime()) {
    return Promise.resolve<ResolveInteractionResponse>(
      clone({
        sessionId: "preview-session",
        interaction: {
          interactionId,
          kind: resolution.type === "planConfirmation" ? "planConfirmation" : resolution.type,
          status: "resolved",
          scope: { sessionId: "preview-session", turnId: "preview-turn" },
          payload: resolution.type === "planConfirmation"
            ? { type: "planConfirmation", planId: interactionId, content: resolution.content ?? "" }
            : resolution.type === "toolApproval"
              ? { type: "toolApproval", name: "tool", arguments: {} }
              : { type: "userInput", questions: [] },
          createdAt: Math.floor(Date.now() / 1000),
          updatedAt: Math.floor(Date.now() / 1000),
          resolvedAt: Math.floor(Date.now() / 1000),
          resolution,
        },
        planLifecycle: null,
      }),
    );
  }
  return invoke<ResolveInteractionResponse>("resolve_interaction", { interactionId, resolution });
}

export function stopPrompt(sessionId: string) {
  if (!isTauriRuntime()) {
    previewInterruptedSessions.add(sessionId);
    return Promise.resolve(
      clone({
        sessionId,
        stopped: true,
      }),
    );
  }
  return invoke<{ sessionId: string; stopped: boolean }>("stop_prompt", { sessionId });
}

export function loadConfig() {
  if (!isTauriRuntime()) {
    return Promise.resolve(clone(previewConfig));
  }
  return invoke<ConfigPayload>("load_config");
}

export function loadProviderUsages() {
  if (!isTauriRuntime()) {
    const now = Math.floor(Date.now() / 1000);
    return Promise.resolve<ProviderUsagesPayload>({
      usages: previewConfig.providers.map((provider) => {
        if (provider.templateKind !== "deepseek" && provider.templateKind !== "zhipu-coding-plan") {
          return {
            providerId: provider.id,
            updatedAt: now,
            status: "unsupported",
            usageKind: "unsupported",
            message: null,
            balance: null,
            codingPlan: null,
          };
        }
        if (!provider.hasBearerToken && !provider.bearerToken.trim()) {
          return {
            providerId: provider.id,
            updatedAt: now,
            status: "missingCredential",
            usageKind: "unknown",
            message: "provider API key is not configured",
            balance: null,
            codingPlan: null,
          };
        }
        if (provider.templateKind === "deepseek") {
          return {
            providerId: provider.id,
            updatedAt: now,
            status: "ready",
            usageKind: "deepseekBalance",
            message: null,
            balance: {
              isAvailable: true,
              balances: [
                {
                  currency: "CNY",
                  totalBalance: "88.00",
                  grantedBalance: "8.00",
                  toppedUpBalance: "80.00",
                },
              ],
            },
            codingPlan: null,
          };
        }
        if (provider.templateKind === "zhipu-coding-plan") {
          return {
            providerId: provider.id,
            updatedAt: now,
            status: "ready",
            usageKind: "zhipuCodingPlan",
            message: null,
            balance: null,
            codingPlan: {
              level: "pro",
              limits: [
                {
                  window: "fiveHour",
                  label: "5h",
                  percentage: 38,
                  currentValue: 38,
                  total: 100,
                  remaining: 62,
                  nextResetAt: now + 7200,
                  usageDetails: [],
                },
                {
                  window: "weekly",
                  label: "7d",
                  percentage: 52,
                  currentValue: 520,
                  total: 1000,
                  remaining: 480,
                  nextResetAt: now + 3600 * 24 * 3,
                  usageDetails: [],
                },
                {
                  window: "mcpMonthly",
                  label: "MCP",
                  percentage: 12,
                  currentValue: 120,
                  total: 1000,
                  remaining: 880,
                  nextResetAt: null,
                  usageDetails: [
                    { name: "Web Search", currentValue: 72, total: 1000, percentage: 7.2 },
                    { name: "Web Reader", currentValue: 40, total: 1000, percentage: 4 },
                    { name: "ZRead", currentValue: 8, total: 1000, percentage: 0.8 },
                  ],
                },
              ],
            },
          };
        }
        return {
          providerId: provider.id,
          updatedAt: now,
          status: "unsupported",
          usageKind: "unsupported",
          message: null,
          balance: null,
          codingPlan: null,
        };
      }),
    });
  }
  return invoke<ProviderUsagesPayload>("load_provider_usages");
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
    const nextProviders = input.providers.map(makeProvider);
    const nextMcpServers = refreshPreviewBuiltinMcpServers(previewConfig.mcpServers, nextProviders);
    previewConfig = {
      ...previewConfig,
      toml: renderPreviewToml(
        input,
        previewConfig.mcpServers.map(mcpServerInput),
        previewConfig.instructions,
      ),
      providers: nextProviders,
      roles: input.roles.map((r) => makeRole(r, previewConfig.roles)),
      mcpServers: nextMcpServers,
      configExists: true,
    };
    previewSessionRuntime.activeMcpServers = nextMcpServers
      .filter((server) => server.availabilityKind === "available")
      .map((server) => server.id);
    return Promise.resolve(clone(previewConfig));
  }
  return invoke<ConfigPayload>("save_provider_settings", { input });
}

export function saveInstructionsSettings(input: InstructionsInput) {
  if (!isTauriRuntime()) {
    const providerInput = {
      defaultProviderId: previewConfig.providers[0]?.id ?? null,
      providers: previewConfig.providers.map((provider) => ({
        id: provider.id,
        templateKind: provider.templateKind,
        name: provider.name,
        baseUrl: provider.baseUrl,
        bearerToken: provider.bearerToken,
        defaultModel: provider.defaultModel,
        providerKind: provider.providerKind,
        customModels: provider.customModels,
      })),
      roles: previewConfig.roles.map((role) => ({
        key: role.key,
        provider: role.provider,
        model: role.model,
        effort: role.effort,
      })),
    };
    previewConfig = {
      ...previewConfig,
      instructions: input,
      toml: renderPreviewToml(
        providerInput,
        previewConfig.mcpServers.map(mcpServerInput),
        input,
      ),
      configExists: true,
    };
    return Promise.resolve(clone(previewConfig));
  }
  return invoke<ConfigPayload>("save_instructions_settings", { input });
}

export function saveMcpSettings(input: McpSettingsInput) {
  if (!isTauriRuntime()) {
    const providerInput = {
      defaultProviderId: previewConfig.providers[0]?.id ?? null,
      providers: previewConfig.providers.map((provider) => ({
        id: provider.id,
        templateKind: provider.templateKind,
        name: provider.name,
        baseUrl: provider.baseUrl,
        bearerToken: provider.bearerToken,
        defaultModel: provider.defaultModel,
        providerKind: provider.providerKind,
        customModels: provider.customModels,
      })),
      roles: previewConfig.roles.map((role) => ({
        key: role.key,
        provider: role.provider,
        model: role.model,
        effort: role.effort,
      })),
    };
    const nextMcpServers = refreshPreviewBuiltinMcpServers(
      input.servers.map((server) => ({
        ...server,
        endpoint: server.transport === "stdio" ? server.command ?? "" : server.url ?? "",
        sourceKind: server.sourceKind ?? "user",
        sourceLabel: server.sourceLabel ?? "User",
        sourceDetail: server.sourceDetail ?? null,
        statusKind: server.statusKind ?? (server.enabled ? "enabled" : "disabled"),
        statusMessage: server.statusMessage ?? null,
        mutationPolicy: server.mutationPolicy ?? "userEditable",
        availabilityKind: server.enabled ? "available" : "disabled",
        availabilityMessage: server.enabled ? "Preview server is available" : "MCP server is disabled",
        lastCheckedAt: server.enabled ? Math.floor(Date.now() / 1000) : null,
        toolCount: null,
      })),
      previewConfig.providers,
    );
    previewConfig = {
      ...previewConfig,
      toml: renderPreviewToml(providerInput, input.servers, previewConfig.instructions),
      mcpServers: nextMcpServers,
      configExists: true,
    };
    previewSessionRuntime.activeMcpServers = nextMcpServers
      .filter((server) => server.availabilityKind === "available")
      .map((server) => server.id);
    return Promise.resolve(clone(previewConfig));
  }
  return invoke<ConfigPayload>("save_mcp_settings", { input });
}

function refreshPreviewBuiltinMcpServers(
  servers: ConfigPayload["mcpServers"],
  providers: ConfigPayload["providers"],
): ConfigPayload["mcpServers"] {
  const hasZhipuCodingPlanToken = providers.some(
    (provider) => provider.templateKind === "zhipu-coding-plan" && provider.bearerToken.trim(),
  );
  const hasZhipuToken =
    hasZhipuCodingPlanToken ||
    providers.some(
    (provider) => provider.providerKind === "zhipu" && provider.bearerToken.trim(),
    );
  return servers.map((server) => {
    if (server.sourceKind !== "builtIn") return server;
    return {
      ...server,
      enabled: hasZhipuToken,
      statusKind: hasZhipuToken ? "enabled" : "missingCredential",
      statusMessage: hasZhipuToken
        ? hasZhipuCodingPlanToken
          ? "Using the configured Zhipu Coding Plan provider token"
          : "Using the configured Zhipu provider token"
        : "Configure a Zhipu Coding Plan or Zhipu provider token to enable this server",
      availabilityKind: hasZhipuToken ? "available" : "missingCredential",
      availabilityMessage: hasZhipuToken
        ? "Preview server is available"
        : "Configure a Zhipu Coding Plan or Zhipu provider token to enable this server",
      lastCheckedAt: hasZhipuToken ? Math.floor(Date.now() / 1000) : null,
      toolCount: hasZhipuToken ? server.toolCount ?? null : null,
    };
  });
}

export function savePermissionMode(mode: PermissionMode) {
  if (!isTauriRuntime()) {
    previewConfig = {
      ...previewConfig,
      permissionMode: mode,
      configExists: true,
    };
    return Promise.resolve(clone(previewConfig));
  }
  return invoke<ConfigPayload>("save_permission_mode", { mode });
}

function mcpServerInput(server: ConfigPayload["mcpServers"][number]): McpServerInput {
  return {
    id: server.id,
    enabled: server.enabled,
    transport: server.transport,
    command: server.command ?? null,
    args: [...server.args],
    env: server.env.map((entry) => ({ ...entry })),
    cwd: server.cwd ?? null,
    url: server.url ?? null,
    bearerTokenEnvVar: server.bearerTokenEnvVar ?? null,
    headers: server.headers.map((entry) => ({ ...entry })),
    sourceKind: server.sourceKind,
    sourceLabel: server.sourceLabel,
    sourceDetail: server.sourceDetail,
    statusKind: server.statusKind,
    statusMessage: server.statusMessage,
    mutationPolicy: server.mutationPolicy,
  };
}

export function listDiscoveredSkills(projectId: string) {
  if (!isTauriRuntime()) {
    return Promise.resolve(
      clone({
        projectDir: `${previewProjects.find((project) => project.id === projectId)?.path ?? ""}\\skills`,
        skills: previewSkills,
        warnings: [],
      }),
    );
  }
  return invoke<DiscoveredSkillsPayload>("list_discovered_skills", { projectId });
}

export function loadSessionTimeline(
  sessionId: string,
  afterSequence?: number,
  limit?: number,
) {
  if (!isTauriRuntime()) {
    return Promise.resolve(
      clone({
        sessionId,
        events: previewTimelineEvents,
        planStates: [],
        interactions: [],
        nextSequence: nextTimelineSequenceForItems(previewTimelineItems),
      }),
    );
  }
  return invoke<SessionTimeline>("load_session_timeline", {
    sessionId,
    afterSequence: afterSequence ?? null,
    limit: limit ?? null,
  });
}

export function loadStudioEvents(
  sessionId: string,
  afterSequence?: number,
  limit?: number,
) {
  if (!isTauriRuntime()) {
    return Promise.resolve<StudioEventsPayload>(
      clone({
        sessionId,
        events: [],
        nextSequence: afterSequence ?? 0,
      }),
    );
  }
  return invoke<StudioEventsPayload>("load_studio_events", {
    sessionId,
    afterSequence: afterSequence ?? null,
    limit: limit ?? null,
  });
}

export function loadSessionState(sessionId: string) {
  if (!isTauriRuntime()) {
    return Promise.resolve<SessionStatePayload>(
      clone({
        sessionId,
        session: previewSessions.find((session) => session.id === sessionId) ?? previewSessions[0],
        sessions: previewSessions,
        agentEvents: previewAgentEvents,
        agents: previewAgents,
        sessionRuntime: previewSessionRuntime,
        interactions: [],
        timeline: {
          sessionId,
          events: previewTimelineEvents,
          planStates: [],
          interactions: [],
          nextSequence: nextTimelineSequenceForItems(previewTimelineItems),
        },
        events: [],
        eventNextSequence: 0,
      }),
    );
  }
  return invoke<SessionStatePayload>("load_session_state", { sessionId });
}

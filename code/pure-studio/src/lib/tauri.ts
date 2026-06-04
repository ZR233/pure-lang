import { invoke } from "@tauri-apps/api/core";
import type {
  BootstrapPayload,
  CompileMode,
  ConfigPayload,
  DiscoveredSkillsPayload,
  ProjectSelectionPayload,
  ProviderSettingsInput,
  RunPromptResponse,
  SessionSelectionPayload,
  SessionTimeline,
} from "../types";
import {
  createPreviewConfig,
  previewProjects,
  previewSessions,
  previewSessionRuntime,
  previewAgents,
  previewAgentEvents,
  previewTimelineItems,
  previewSkills,
} from "./preview";
import { makeProvider, makeRole } from "./provider-mapper";
import { renderPreviewToml } from "./toml-renderer";

type TauriWindow = Window & {
  __TAURI_INTERNALS__?: unknown;
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
        agentEvents: previewAgentEvents,
        agents: previewAgents,
        sessionRuntime: previewSessionRuntime,
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
        agentEvents: previewAgentEvents,
        agents: previewAgents,
        sessionRuntime: previewSessionRuntime,
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
        agentEvents: previewAgentEvents,
        agents: previewAgents,
        sessionRuntime: previewSessionRuntime,
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
      mode: "auto",
      updatedAt: Math.floor(Date.now() / 1000),
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

export function selectSession(sessionId: string) {
  if (!isTauriRuntime()) {
    return Promise.resolve(
      clone({
        sessionId,
        sessions: previewSessions,
        agentEvents: previewAgentEvents,
        agents: previewAgents,
        sessionRuntime: previewSessionRuntime,
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
      }),
    );
  }
  return invoke<SessionSelectionPayload>("set_session_mode", { sessionId, mode });
}

export function runPrompt(sessionId: string, prompt: string) {
  if (!isTauriRuntime()) {
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
        timelineItems: [
          ...previewTimelineItems,
          {
            turnId: "preview-turn-latest",
            itemId: `preview-user-${Date.now()}`,
            sequence: previewTimelineItems.length + 8,
            kind: "text" as const,
            status: "completed" as const,
            createdAt: now,
            updatedAt: now,
            role: "user" as const,
            content: prompt,
            thinkingChunks: [],
          },
          {
            turnId: "preview-turn-latest",
            itemId: `preview-turn-latest-${Date.now()}`,
            sequence: previewTimelineItems.length + 10,
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
        ],
        timelineNextSequence: previewTimelineItems.length + 11,
        turnStatus: interrupted ? "aborted" as const : "completed" as const,
        turnAbortReason: interrupted ? "interrupted" : null,
        }));
      }, 900);
    });
  }
  return invoke<RunPromptResponse>("run_prompt", { sessionId, prompt });
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
      roles: input.roles.map((r) => makeRole(r, previewConfig.roles)),
      configExists: true,
    };
    return Promise.resolve(clone(previewConfig));
  }
  return invoke<ConfigPayload>("save_provider_settings", { input });
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
        items: previewTimelineItems,
        nextSequence: previewTimelineItems.length,
      }),
    );
  }
  return invoke<SessionTimeline>("load_session_timeline", {
    sessionId,
    afterSequence: afterSequence ?? null,
    limit: limit ?? null,
  });
}

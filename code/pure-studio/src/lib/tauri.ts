import { invoke } from "@tauri-apps/api/core";
import type {
  BootstrapPayload,
  ConfigPayload,
  ProjectSelectionPayload,
  ProviderSettingsInput,
  RunPromptResponse,
  SessionSelectionPayload,
  SessionTimeline,
} from "../types";
import {
  createPreviewConfig,
  previewMessages,
  previewProjects,
  previewSessions,
  previewSessionRuntime,
  previewSubagentEvents,
} from "./preview";
import { makeProvider, makeRole } from "./provider-mapper";
import { renderPreviewToml } from "./toml-renderer";

type TauriWindow = Window & {
  __TAURI_INTERNALS__?: unknown;
};

let previewConfig = createPreviewConfig();

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
        messages: previewMessages,
        subagentEvents: previewSubagentEvents,
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
        messages: previewMessages,
        subagentEvents: previewSubagentEvents,
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
        messages: previewMessages,
        subagentEvents: previewSubagentEvents,
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
      mode: "manual",
      updatedAt: Math.floor(Date.now() / 1000),
    };
    return Promise.resolve(
      clone({
        sessionId: session.id,
        sessions: [session, ...previewSessions],
        messages: [],
        subagentEvents: [],
        sessionRuntime: {
          ...previewSessionRuntime,
          sessionId: session.id,
          latestContextTokens: 0,
          promptTokens: 0,
          completionTokens: 0,
          cachedPromptTokens: 0,
          totalTokens: 0,
          cacheHitRate: null,
          estimatedCost: null,
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
        messages: previewMessages,
        subagentEvents: previewSubagentEvents,
        sessionRuntime: previewSessionRuntime,
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
            summary: "预览运行已完成。",
            depth: 1,
            error: null,
            updatedAt: Math.floor(Date.now() / 1000),
          },
        ],
        sessionRuntime: {
          ...previewSessionRuntime,
          promptTokens: previewSessionRuntime.promptTokens + 1200,
          completionTokens: previewSessionRuntime.completionTokens + 260,
          totalTokens: previewSessionRuntime.totalTokens + 1460,
        },
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
      roles: input.roles.map((r) => makeRole(r, previewConfig.roles)),
      configExists: true,
    };
    return Promise.resolve(clone(previewConfig));
  }
  return invoke<ConfigPayload>("save_provider_settings", { input });
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
        items: [],
        nextSequence: 0,
      }),
    );
  }
  return invoke<SessionTimeline>("load_session_timeline", {
    sessionId,
    afterSequence: afterSequence ?? null,
    limit: limit ?? null,
  });
}

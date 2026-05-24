import { invoke } from "@tauri-apps/api/core";
import {
  BootstrapPayload,
  ConfigPayload,
  ProviderSettingsInput,
  ProjectSelectionPayload,
  RunPromptResponse,
  SessionSelectionPayload,
} from "../types";

export function bootstrapStudio() {
  return invoke<BootstrapPayload>("bootstrap_studio");
}

export function openProject(path: string) {
  return invoke<ProjectSelectionPayload>("open_project", { path });
}

export function selectProject(projectId: string) {
  return invoke<ProjectSelectionPayload>("select_project", { projectId });
}

export function createSession(projectId: string, title?: string) {
  return invoke<SessionSelectionPayload>("create_session", {
    projectId,
    title: title ?? null,
  });
}

export function selectSession(sessionId: string) {
  return invoke<SessionSelectionPayload>("select_session", { sessionId });
}

export function runPrompt(sessionId: string, prompt: string) {
  return invoke<RunPromptResponse>("run_prompt", { sessionId, prompt });
}

export function approveTool(approvalId: string) {
  return invoke<void>("approve_tool", { approvalId });
}

export function denyTool(approvalId: string, reason?: string) {
  return invoke<void>("deny_tool", { approvalId, reason: reason ?? null });
}

export function loadConfig() {
  return invoke<ConfigPayload>("load_config");
}

export function saveConfig(toml: string) {
  return invoke<ConfigPayload>("save_config", { toml });
}

export function saveProviderSettings(input: ProviderSettingsInput) {
  return invoke<ConfigPayload>("save_provider_settings", { input });
}

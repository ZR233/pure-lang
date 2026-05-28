import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { Dispatch, SetStateAction } from "react";
import { useEffect, useMemo, useReducer } from "react";
import { useTranslation } from "react-i18next";
import { normalizeRolesForProviders } from "../components/RoleSettings";
import {
  approveTool,
  bootstrapStudio,
  createSession,
  denyTool,
  loadConfig,
  loadSessionTimeline,
  openProject,
  runPrompt,
  saveConfig,
  saveProviderSettings,
  selectProject,
  selectSession,
  isTauriRuntime,
  stopPrompt,
} from "../lib/tauri";
import { errorText } from "../lib/utils";
import {
  selectChatItems,
  selectSelectedProject,
  selectSelectedSession,
} from "../state/selectors";
import { initialStudioState, studioReducer } from "../state/studio-state";
import type {
  AgentEvent,
  AgentEventPayload,
  ChatItem,
  PromptFailed,
  RoleRecord,
  RunPromptResponse,
  ToolApprovalRequest,
  ToolApprovalResolved,
  SubagentStatus,
} from "../types";

const subagentStatusKeys: Record<SubagentStatus, string> = {
  queued: "subagent.queued",
  awaitingApproval: "subagent.awaitingApproval",
  running: "subagent.running",
  awaitingToolApproval: "subagent.awaitingTool",
  succeeded: "subagent.succeeded",
  failed: "subagent.failed",
  denied: "subagent.denied",
};

function statusTextForEvent(event: AgentEvent, t: (key: string, args?: Record<string, unknown>) => string) {
  if (event === "turnStarted") return t("status.running");
  if (event === "done") return t("status.done");
  if ("turnInterrupted" in event) return t("status.interrupted");
  if ("textDelta" in event) return t("status.running");
  if ("thinkingDelta" in event) return t("status.thinking");
  if ("toolCallDelta" in event) return t("status.toolInput", { name: event.toolCallDelta.name });
  if ("toolApprovalGranted" in event) return t("status.approved", { name: event.toolApprovalGranted.name });
  if ("toolApprovalDenied" in event) return t("status.denied", { name: event.toolApprovalDenied.name });
  if ("subagentStateChanged" in event) {
    return t("status.subagentStatus", {
      status: t(subagentStatusKeys[event.subagentStateChanged.status]).toLowerCase(),
    });
  }
  if ("agentStateChanged" in event) {
    return t("status.subagentStatus", {
      status: event.agentStateChanged.status,
    });
  }
  if ("error" in event) return t("status.error", { message: event.error.message });
  return t("status.running");
}

export function useStudioApp() {
  const { t } = useTranslation();
  const [state, dispatch] = useReducer(studioReducer, initialStudioState(t("status.starting")));

  const selectedProject = selectSelectedProject(state);
  const selectedSession = selectSelectedSession(state);

  const chatItems = useMemo((): ChatItem[] => {
    return selectChatItems(state, t("status.thinking"));
  }, [state.messages, state.toolCalls, state.streamingText, state.thinkingText, t]);

  useEffect(() => {
    bootstrapStudio()
      .then((payload) => {
        dispatch({ type: "bootstrapLoaded", payload, status: t("status.ready") });
      })
      .catch((error) => {
        dispatch({
          type: "bootstrapFailed",
          status: t("status.bootstrapFailed", { error: errorText(error) }),
        });
      });
  }, [t]);

  useEffect(() => {
    if (!state.selectedSessionId) {
      dispatch({ type: "timelineLoaded", items: [] });
      return;
    }
    loadSessionTimeline(state.selectedSessionId)
      .then((payload) => dispatch({ type: "timelineLoaded", items: payload.items }))
      .catch((error) => {
        dispatch({
          type: "timelineLoadFailed",
          status: t("status.timelineLoadFailed", { error: errorText(error) }),
        });
      });
  }, [state.selectedSessionId, t]);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    const unlisteners = [
      listen<AgentEventPayload>("studio-agent-event", ({ payload }) => {
        dispatch({
          type: "agentEvent",
          event: payload.event,
          statusText: statusTextForEvent(payload.event, t),
        });
      }),
      listen<ToolApprovalRequest>("studio-tool-approval-requested", ({ payload }) => {
        dispatch({
          type: "enqueueApproval",
          payload,
          status: t("status.approvalRequired", { name: payload.name }),
        });
      }),
      listen<ToolApprovalResolved>("studio-tool-approval-resolved", ({ payload }) => {
        dispatch({
          type: "resolveApproval",
          payload,
          status: payload.decision === "approved" ? t("status.toolApproved") : t("status.toolDenied"),
        });
      }),
      listen<RunPromptResponse>("studio-prompt-finished", ({ payload }) => {
        dispatch({
          type: "runPromptLoaded",
          payload,
          status: payload.turnStatus === "interrupted" ? t("status.interrupted") : t("status.done"),
        });
      }),
      listen<PromptFailed>("studio-prompt-failed", ({ payload }) => {
        dispatch({ type: "runPromptFailed", status: payload.message });
      }),
    ];

    return () => {
      void Promise.all(unlisteners).then((items) => {
        for (const unlisten of items) {
          unlisten();
        }
      });
    };
  }, [t]);

  const setRolesState: Dispatch<SetStateAction<RoleRecord[]>> = (value) => {
    const next = typeof value === "function" ? value(state.roles) : value;
    dispatch({ type: "setRoles", roles: next });
  };

  const setProvidersState: Dispatch<SetStateAction<typeof state.providers>> = (value) => {
    const next = typeof value === "function" ? value(state.providers) : value;
    dispatch({ type: "setProviders", providers: next });
  };

  const setSelectedProviderIdState: Dispatch<SetStateAction<string | null>> = (value) => {
    const next = typeof value === "function" ? value(state.selectedProviderId) : value;
    dispatch({ type: "setSelectedProviderId", providerId: next });
  };

  const setProviderSearchState: Dispatch<SetStateAction<string>> = (value) => {
    const next = typeof value === "function" ? value(state.providerSearch) : value;
    dispatch({ type: "setProviderSearch", search: next });
  };

  const setConfigTomlState: Dispatch<SetStateAction<string>> = (value) => {
    const next = typeof value === "function" ? value(state.configToml) : value;
    dispatch({ type: "setConfigToml", toml: next });
  };

  async function addProject(path: string) {
    try {
      const payload = await openProject(path);
      dispatch({ type: "projectSelectionLoaded", payload, status: t("status.projectLoaded") });
      dispatch({ type: "setManualPath", path: "" });
    } catch (error) {
      dispatch({
        type: "bootstrapFailed",
        status: t("status.addProjectFailed", { error: errorText(error) }),
      });
    }
  }

  async function chooseFolder() {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string") {
      await addProject(picked);
    }
  }

  async function onSelectProject(projectId: string) {
    try {
      const payload = await selectProject(projectId);
      dispatch({ type: "projectSelectionLoaded", payload, status: t("status.projectLoaded") });
    } catch (error) {
      dispatch({
        type: "bootstrapFailed",
        status: t("status.selectProjectFailed", { error: errorText(error) }),
      });
    }
  }

  async function onNewSession() {
    if (!state.selectedProjectId) {
      return;
    }
    try {
      const payload = await createSession(state.selectedProjectId, t("common.newSessionTitle"));
      dispatch({ type: "sessionSelectionLoaded", payload, status: t("status.sessionLoaded") });
    } catch (error) {
      dispatch({
        type: "bootstrapFailed",
        status: t("status.newSessionFailed", { error: errorText(error) }),
      });
    }
  }

  async function onSelectSession(sessionId: string) {
    try {
      const payload = await selectSession(sessionId);
      dispatch({ type: "sessionSelectionLoaded", payload, status: t("status.sessionLoaded") });
    } catch (error) {
      dispatch({
        type: "bootstrapFailed",
        status: t("status.selectSessionFailed", { error: errorText(error) }),
      });
    }
  }

  async function onSendPrompt() {
    const content = state.prompt.trim();
    if (!content || !state.selectedSessionId || state.isBusy) {
      return;
    }
    dispatch({
      type: "appendUserPrompt",
      content,
      status: t("status.running"),
      startedAt: Date.now(),
    });
    try {
      const payload = await runPrompt(state.selectedSessionId, content);
      dispatch({
        type: "runPromptLoaded",
        payload,
        status: payload.turnStatus === "interrupted" ? t("status.interrupted") : t("status.done"),
      });
    } catch (error) {
      dispatch({
        type: "runPromptFailed",
        status: t("status.runFailed", { error: errorText(error) }),
      });
    }
  }

  async function onStopPrompt() {
    if (!state.selectedSessionId || !state.isBusy || state.turnPhase === "stopping") {
      return;
    }
    dispatch({ type: "stopRequested", status: t("status.stopping") });
    try {
      const result = await stopPrompt(state.selectedSessionId);
      if (!result.stopped || !isTauriRuntime()) {
        dispatch({ type: "stopFallback", status: t("status.interrupted") });
      }
    } catch (error) {
      dispatch({
        type: "runPromptFailed",
        status: t("status.stopFailed", { error: errorText(error) }),
      });
    }
  }

  async function openSettings() {
    dispatch({ type: "setSettingsOpen", value: true, tab: "providers" });
    try {
      dispatch({ type: "configLoaded", payload: await loadConfig() });
    } catch (error) {
      dispatch({
        type: "bootstrapFailed",
        status: t("status.configLoadFailed", { error: errorText(error) }),
      });
    }
  }

  async function onSaveConfig() {
    try {
      dispatch({
        type: "configLoaded",
        payload: await saveConfig(state.configToml),
        status: t("status.configSaved"),
      });
    } catch (error) {
      dispatch({
        type: "bootstrapFailed",
        status: t("status.configInvalid", { error: errorText(error) }),
      });
    }
  }

  async function onSaveProviderSettings(explicitRoles?: RoleRecord[]) {
    try {
      const rolesToSave = explicitRoles ?? state.roles;
      const normalizedRoles = normalizeRolesForProviders(rolesToSave, state.providers);
      dispatch({
        type: "configLoaded",
        payload: await saveProviderSettings({
          defaultProviderId: state.selectedProviderId,
          providers: state.providers.map((provider) => ({
            id: provider.id,
            templateKind: provider.templateKind,
            name: provider.name,
            baseUrl: provider.baseUrl,
            bearerToken: provider.bearerToken,
            defaultModel: provider.defaultModel,
            wireApi: provider.wireApi,
            customModels: provider.customModels.map((model) => ({
              slug: model.slug,
              displayName: model.displayName,
              reasoningEfforts: [...model.reasoningEfforts],
            })),
          })),
          roles: normalizedRoles.map((role) => ({
            key: role.key,
            provider: role.provider,
            model: role.model,
            effort: role.effort,
          })),
        }),
        status: t("status.providerSettingsSaved"),
      });
    } catch (error) {
      dispatch({
        type: "bootstrapFailed",
        status: t("status.providerSettingsInvalid", { error: errorText(error) }),
      });
    }
  }

  async function onReloadConfig() {
    try {
      dispatch({
        type: "configLoaded",
        payload: await loadConfig(),
        status: t("status.configReloaded"),
      });
    } catch (error) {
      dispatch({
        type: "bootstrapFailed",
        status: t("status.reloadFailed", { error: errorText(error) }),
      });
    }
  }

  async function onApprove(approvalId: string) {
    await approveTool(approvalId);
  }

  async function onDeny(approvalId: string) {
    await denyTool(approvalId, "denied by user");
  }

  return {
    state,
    chatItems,
    selectedProject,
    selectedSession,
    setRolesState,
    setProvidersState,
    setSelectedProviderIdState,
    setProviderSearchState,
    setConfigTomlState,
    dispatch,
    addProject,
    chooseFolder,
    onSelectProject,
    onNewSession,
    onSelectSession,
    onSendPrompt,
    onStopPrompt,
    openSettings,
    onSaveConfig,
    onSaveProviderSettings,
    onReloadConfig,
    onApprove,
    onDeny,
  };
}

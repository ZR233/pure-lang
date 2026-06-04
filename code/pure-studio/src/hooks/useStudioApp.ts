import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { Dispatch, SetStateAction } from "react";
import { useEffect, useMemo, useReducer, useRef } from "react";
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
  setSessionMode,
  isTauriRuntime,
  stopPrompt,
} from "../lib/tauri";
import { errorText } from "../lib/utils";
import {
  selectSelectedProject,
  selectSelectedSession,
  selectTimelineEntries,
} from "../state/selectors";
import { initialStudioState, studioReducer } from "../state/studio-state";
import type {
  AgentEvent,
  AgentEventPayload,
  CompileMode,
  PromptFailed,
  ProviderRecord,
  ProviderSettingsSaveSnapshot,
  RoleRecord,
  ToolApprovalRequest,
  ToolApprovalResolved,
} from "../types";

function providerInput(provider: ProviderRecord) {
  return {
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
  };
}

function statusTextForEvent(
  event: AgentEvent | null | undefined,
  t: (key: string, args?: Record<string, unknown>) => string,
) {
  if (!event) return t("turnPhase.subagent");
  if (event === "done") return t("status.done");
  if ("turnInterrupted" in event) return t("status.interrupted");
  if ("turnBudgetLimited" in event) return t("turnPhase.budgetLimited");
  if ("timelineItemStarted" in event) return t("status.running");
  if ("timelineItemDelta" in event) {
    const itemEvent = event.timelineItemDelta.event;
    if (itemEvent.kind === "thinking") return t("status.thinking");
    if (itemEvent.kind === "tool") return t("status.toolInput", { name: "tool" });
    return t("status.running");
  }
  if ("timelineItemCompleted" in event) {
    const item = event.timelineItemCompleted.item;
    if (item.kind === "tool") return t("status.toolCompleted", { name: item.tool?.name ?? "tool" });
    return t("status.running");
  }
  if ("timelineItemFailed" in event) return t("status.error", { message: event.timelineItemFailed.error });
  if ("toolApprovalGranted" in event) return t("status.approved", { name: event.toolApprovalGranted.name });
  if ("toolApprovalDenied" in event) return t("status.denied", { name: event.toolApprovalDenied.name });
  if ("agentRuntimeUpdated" in event) return t("status.running");
  if ("error" in event) return t("status.error", { message: event.error.message });
  return t("status.running");
}

export function useStudioApp() {
  const { t } = useTranslation();
  const [state, dispatch] = useReducer(studioReducer, initialStudioState(t("status.starting")));
  const providerSaveQueueRef = useRef<Promise<void>>(Promise.resolve());
  const providerSaveVersionRef = useRef(0);

  const selectedProject = selectSelectedProject(state);
  const selectedSession = selectSelectedSession(state);

  const timelineEntries = useMemo(() => {
    return selectTimelineEntries(state);
  }, [state.timelineItems, state.timelineOrder]);

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
      dispatch({ type: "timelineLoaded", sessionId: null, items: [], nextSequence: 0 });
      return;
    }
    const sessionId = state.selectedSessionId;
    loadSessionTimeline(sessionId)
      .then((payload) =>
        dispatch({
          type: "timelineLoaded",
          sessionId: payload.sessionId,
          items: payload.items,
          nextSequence: payload.nextSequence,
        }),
      )
      .catch((error) => {
        dispatch({
          type: "timelineLoadFailed",
          sessionId,
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
          sessionId: payload.sessionId,
          event: payload.event,
          timelineEvent: payload.timelineEvent,
          agent: payload.agent,
          sessionRuntime: payload.sessionRuntime,
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
      listen<PromptFailed>("studio-prompt-failed", ({ payload }) => {
        dispatch({
          type: "runPromptFailed",
          sessionId: payload.sessionId,
          status: payload.message,
        });
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

  async function onSetSessionMode(mode: CompileMode) {
    const sessionId = state.selectedSessionId;
    if (!sessionId || state.isBusy) {
      return;
    }
    try {
      const payload = await setSessionMode(sessionId, mode);
      dispatch({ type: "sessionModeUpdated", payload, status: t("status.modeUpdated") });
    } catch (error) {
      dispatch({
        type: "bootstrapFailed",
        status: t("status.modeUpdateFailed", { error: errorText(error) }),
      });
    }
  }

  async function onSendPrompt() {
    const content = state.prompt.trim();
    const sessionId = state.selectedSessionId;
    if (!content || !sessionId || state.isBusy) {
      return;
    }
    await submitPrompt(sessionId, content);
  }

  async function onImplementPlan(plan: string) {
    const content = plan.trim();
    const sessionId = state.selectedSessionId;
    if (!content || !sessionId || state.isBusy) {
      return;
    }
    try {
      const modePayload = await setSessionMode(sessionId, "auto");
      dispatch({ type: "sessionModeUpdated", payload: modePayload, status: t("status.modeUpdated") });
      await submitPrompt(sessionId, `PLEASE IMPLEMENT THIS PLAN:\n\n${content}`);
    } catch (error) {
      dispatch({
        type: "runPromptFailed",
        sessionId,
        status: t("status.runFailed", { error: errorText(error) }),
      });
    }
  }

  async function submitPrompt(sessionId: string, content: string) {
    dispatch({
      type: "promptSubmitted",
      status: t("status.running"),
      startedAt: Date.now(),
    });
    try {
      const payload = await runPrompt(sessionId, content);
      dispatch({
        type: "runPromptLoaded",
        payload,
        status: payload.turnAbortReason === "interrupted" ? t("status.interrupted") : t("status.done"),
      });
    } catch (error) {
      dispatch({
        type: "runPromptFailed",
        sessionId,
        status: t("status.runFailed", { error: errorText(error) }),
      });
    }
  }

  async function onStopPrompt() {
    const sessionId = state.selectedSessionId;
    if (!sessionId || !state.isBusy || state.turnPhase === "stopping") {
      return;
    }
    dispatch({ type: "stopRequested", status: t("status.stopping") });
    try {
      const result = await stopPrompt(sessionId);
      if (!result.stopped || !isTauriRuntime()) {
        dispatch({ type: "stopFallback", status: t("status.interrupted") });
      }
    } catch (error) {
      dispatch({
        type: "runPromptFailed",
        sessionId,
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

  async function onSaveProviderSettings(
    snapshotOrRoles?: ProviderSettingsSaveSnapshot | RoleRecord[],
  ): Promise<boolean> {
    const snapshot = Array.isArray(snapshotOrRoles)
      ? { roles: snapshotOrRoles }
      : (snapshotOrRoles ?? {});
    const providersToSave = snapshot.providers ?? state.providers;
    const rolesToSave = snapshot.roles ?? state.roles;
    const selectedProviderId =
      snapshot.selectedProviderId !== undefined
        ? snapshot.selectedProviderId
        : state.selectedProviderId;
    const normalizedRoles = normalizeRolesForProviders(rolesToSave, providersToSave);
    const input = {
      defaultProviderId: selectedProviderId,
      providers: providersToSave.map(providerInput),
      roles: normalizedRoles.map((role) => ({
        key: role.key,
        provider: role.provider,
        model: role.model,
        effort: role.effort,
      })),
    };
    const requestVersion = providerSaveVersionRef.current + 1;
    providerSaveVersionRef.current = requestVersion;

    const saveTask = providerSaveQueueRef.current
      .catch(() => undefined)
      .then(async () => {
        try {
          const payload = await saveProviderSettings(input);
          if (requestVersion === providerSaveVersionRef.current) {
            dispatch({
              type: "configLoaded",
              payload,
              status: t("status.providerSettingsSaved"),
            });
            dispatch({ type: "setSelectedProviderId", providerId: selectedProviderId });
          }
          return true;
        } catch (error) {
          if (requestVersion === providerSaveVersionRef.current) {
            dispatch({
              type: "bootstrapFailed",
              status: t("status.providerSettingsInvalid", { error: errorText(error) }),
            });
          }
          return false;
        }
      });
    providerSaveQueueRef.current = saveTask.then(() => undefined, () => undefined);
    return saveTask;
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
    timelineEntries,
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
    onSetSessionMode,
    onSendPrompt,
    onImplementPlan,
    onStopPrompt,
    openSettings,
    onSaveConfig,
    onSaveProviderSettings,
    onReloadConfig,
    onApprove,
    onDeny,
  };
}

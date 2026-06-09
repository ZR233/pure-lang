import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { Dispatch, SetStateAction } from "react";
import { useEffect, useMemo, useReducer, useRef } from "react";
import { useTranslation } from "react-i18next";
import { normalizeRolesForProviders } from "../components/RoleSettings";
import {
  approveTool,
  answerUserInput,
  bootstrapStudio,
  createSession,
  denyTool,
  dismissPlan,
  implementPlan,
  loadConfig,
  loadSessionTimeline,
  openProject,
  runPrompt,
  saveConfig,
  savePermissionMode,
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
  selectPlanAction,
  selectTimelineEntries,
} from "../state/selectors";
import { initialStudioState, studioReducer } from "../state/studio-state";
import type {
  AgentEvent,
  AgentEventPayload,
  CompileMode,
  PermissionMode,
  PromptFailed,
  ProviderRecord,
  ProviderSettingsSaveSnapshot,
  RoleRecord,
  TimelineItem,
  ToolApprovalRequest,
  ToolApprovalResolved,
  UserInputRequest,
  UserInputResolved,
  UserInputResponse,
} from "../types";

function providerInput(provider: ProviderRecord) {
  return {
    id: provider.id,
    templateKind: provider.templateKind,
    name: provider.name,
    baseUrl: provider.baseUrl,
    bearerToken: provider.bearerToken,
    defaultModel: provider.defaultModel,
    providerKind: provider.providerKind,
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
    if (item.kind === "tool") return statusTextForToolItem(item, t);
    return t("status.running");
  }
  if ("timelineItemFailed" in event) return t("status.error", { message: event.timelineItemFailed.error });
  if ("toolApprovalGranted" in event) return t("status.approved", { name: event.toolApprovalGranted.name });
  if ("toolApprovalDenied" in event) return t("status.denied", { name: event.toolApprovalDenied.name });
  if ("userInputRequested" in event) return t("status.userInputRequired");
  if ("userInputAnswered" in event) return t("status.userInputAnswered");
  if ("agentRuntimeUpdated" in event) return t("status.running");
  if ("error" in event) return t("status.error", { message: event.error.message });
  return t("status.running");
}

function statusTextForToolItem(
  item: TimelineItem,
  t: (key: string, args?: Record<string, unknown>) => string,
) {
  const name = item.tool?.name || "tool";
  switch (item.status) {
    case "approved":
      return t("status.approved", { name });
    case "denied":
      return t("status.denied", { name });
    case "failed":
      return t("status.error", { message: item.tool?.result ?? name });
    case "completed":
      return t("status.toolCompleted", { name });
    case "awaitingApproval":
      return t("status.approvalRequired", { name });
    case "interrupted":
      return t("status.interrupted");
    case "budgetLimited":
      return t("turnPhase.budgetLimited");
    case "started":
    case "streaming":
    case "running":
      return t("status.toolInput", { name });
  }
}

export function useStudioApp() {
  const { t } = useTranslation();
  const [state, dispatch] = useReducer(studioReducer, initialStudioState(t("status.starting")));
  const providerSaveQueueRef = useRef<Promise<void>>(Promise.resolve());
  const providerSaveVersionRef = useRef(0);

  const selectedProject = selectSelectedProject(state);
  const selectedSession = selectSelectedSession(state);
  const planAction = selectPlanAction(state);

  const timelineEntries = useMemo(() => {
    return selectTimelineEntries(state);
  }, [state.timelineItems, state.timelineOrder, state.planStates]);

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
      dispatch({ type: "timelineLoaded", sessionId: null, items: [], planStates: [], nextSequence: 0 });
      return;
    }
    const sessionId = state.selectedSessionId;
    reloadTimeline(sessionId);
  }, [state.selectedSessionId, t]);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    const unlisteners = [
      listen<AgentEventPayload>("studio-agent-event", ({ payload }) => {
        if (payload.timelineStale) {
          void reloadTimeline(payload.sessionId);
          if (!payload.event && !payload.timelineEvent && !payload.agent && !payload.sessionRuntime) {
            return;
          }
        }
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
      listen<UserInputRequest>("studio-user-input-requested", ({ payload }) => {
        dispatch({
          type: "userInputRequested",
          payload,
          status: t("status.userInputRequired"),
        });
      }),
      listen<UserInputResolved>("studio-user-input-resolved", ({ payload }) => {
        dispatch({
          type: "userInputResolved",
          payload,
          status: t("status.userInputAnswered"),
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

  function reloadTimeline(sessionId: string) {
    return loadSessionTimeline(sessionId)
      .then((payload) =>
        dispatch({
          type: "timelineLoaded",
          sessionId: payload.sessionId,
          items: payload.items,
          planStates: payload.planStates ?? [],
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
  }

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

  async function onSendPromptContent(content: string) {
    const trimmed = content.trim();
    const sessionId = state.selectedSessionId;
    if (!trimmed || !sessionId || state.isBusy) {
      return;
    }
    await submitPrompt(sessionId, trimmed);
  }

  async function onImplementPlan(planId: string, plan: string) {
    const content = plan.trim();
    const sessionId = state.selectedSessionId;
    if (!planId || !content || !sessionId || state.isBusy) {
      return;
    }
    const prompt = `PLEASE IMPLEMENT THIS PLAN:\n\n${content}`;
    dispatch({
      type: "promptSubmitted",
      status: t("status.running"),
      startedAt: Date.now(),
      prompt,
    });
    try {
      const payload = await implementPlan(sessionId, planId, content);
      const turnError = payload.turnError ?? payload.turnAbortReason ?? t("subagent.providerError");
      const status =
        payload.turnStatus === "errored"
          ? t("status.runFailed", { error: turnError })
          : payload.turnAbortReason === "interrupted"
            ? t("status.interrupted")
            : t("status.done");
      dispatch({ type: "runPromptLoaded", payload, status });
    } catch (error) {
      dispatch({
        type: "runPromptFailed",
        sessionId,
        status: t("status.runFailed", { error: errorText(error) }),
      });
    }
  }

  async function dismissCurrentPlan(planId: string, reason: string): Promise<boolean> {
    const sessionId = state.selectedSessionId;
    if (!planId || !sessionId || state.isBusy) {
      return false;
    }
    try {
      const payload = await dismissPlan(sessionId, planId, reason);
      dispatch({
        type: "planLifecycleLoaded",
        sessionId: payload.sessionId,
        planStates: payload.planStates,
        timelineNextSequence: payload.timelineNextSequence,
        status: t("status.ready"),
      });
      dispatch({ type: "dismissPlanAction", planId });
      return true;
    } catch (error) {
      dispatch({
        type: "runPromptFailed",
        sessionId,
        status: t("status.runFailed", { error: errorText(error) }),
      });
      return false;
    }
  }

  async function onDiscussPlan(planId: string, content: string) {
    const trimmed = content.trim();
    const sessionId = state.selectedSessionId;
    if (!trimmed || !sessionId || state.isBusy) {
      return;
    }
    const dismissed = await dismissCurrentPlan(planId, "discuss");
    if (dismissed) {
      await submitPrompt(sessionId, trimmed);
    }
  }

  async function submitPrompt(sessionId: string, content: string) {
    dispatch({
      type: "promptSubmitted",
      status: t("status.running"),
      startedAt: Date.now(),
      prompt: content,
    });
    try {
      const payload = await runPrompt(sessionId, content);
      const turnError = payload.turnError ?? payload.turnAbortReason ?? t("subagent.providerError");
      const status =
        payload.turnStatus === "errored"
          ? t("status.runFailed", { error: turnError })
          : payload.turnAbortReason === "interrupted"
            ? t("status.interrupted")
            : t("status.done");
      dispatch({
        type: "runPromptLoaded",
        payload,
        status,
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

  async function onSavePermissionMode(mode: PermissionMode) {
    try {
      dispatch({
        type: "configLoaded",
        payload: await savePermissionMode(mode),
        status: t("status.permissionModeSaved"),
      });
    } catch (error) {
      dispatch({
        type: "bootstrapFailed",
        status: t("status.permissionModeSaveFailed", { error: errorText(error) }),
      });
    }
  }

  async function onApprove(approvalId: string) {
    await approveTool(approvalId);
  }

  async function onDeny(approvalId: string) {
    await denyTool(approvalId, "denied by user");
  }

  async function onAnswerUserInput(requestId: string, response: UserInputResponse) {
    try {
      await answerUserInput(requestId, response);
      if (!isTauriRuntime()) {
        dispatch({
          type: "userInputResolved",
          payload: { requestId },
          status: t("status.userInputAnswered"),
        });
      }
    } catch (error) {
      dispatch({
        type: "runPromptFailed",
        sessionId: state.selectedSessionId,
        status: t("status.runFailed", { error: errorText(error) }),
      });
    }
  }

  return {
    state,
    timelineEntries,
    selectedProject,
    selectedSession,
    planAction,
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
    onSendPromptContent,
    onImplementPlan,
    onSetPlanActionMode: (mode: "choice" | "discuss") => dispatch({ type: "setPlanActionMode", mode }),
    onDiscussPlan,
    onDismissPlanAction: (planId: string) => void dismissCurrentPlan(planId, "dismissed"),
    onStopPrompt,
    openSettings,
    onSaveConfig,
    onSaveProviderSettings,
    onSavePermissionMode,
    onReloadConfig,
    onApprove,
    onDeny,
    onAnswerUserInput,
  };
}

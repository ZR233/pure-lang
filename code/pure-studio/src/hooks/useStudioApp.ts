import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { Dispatch, SetStateAction } from "react";
import { useEffect, useMemo, useReducer, useRef } from "react";
import { useTranslation } from "react-i18next";
import { normalizeRolesForProviders } from "../components/RoleSettings";
import {
  archiveProject,
  bootstrapStudio,
  createSession,
  deleteSession,
  loadConfig,
  loadProviderUsages,
  loadStudioEvents,
  loadSessionTimeline,
  openProject,
  saveMcpSettings,
  saveConfig,
  saveInstructionsSettings,
  savePermissionMode,
  saveProviderSettings,
  selectProject,
  selectSession,
  setSessionMode,
  isTauriRuntime,
  resolveInteraction,
  stopPrompt,
  submitPromptCommand,
  runPrompt,
  createPromptAttachment,
} from "../lib/tauri";
import { errorText } from "../lib/utils";
import {
  selectSelectedProject,
  selectSelectedSession,
  selectActiveInteraction,
  selectTimelineEntries,
} from "../state/selectors";
import { initialStudioState, studioReducer } from "../state/studio-state";
import type {
  AgentEvent,
  AttachmentRecord,
  CompileMode,
  McpServerInput,
  InstructionsInput,
  InteractionChangedPayload,
  InteractionResolution,
  PermissionMode,
  ProviderRecord,
  ProviderSettingsSaveSnapshot,
  RoleRecord,
  StudioEventEnvelope,
  StudioTimelineChange,
  StudioTurnStatus,
  TimelineAttachment,
  TimelineItem,
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
        baseInstructions: model.baseInstructions ?? "",
      })),
  };
}

function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result ?? ""));
    reader.onerror = () => reject(reader.error ?? new Error("failed to read file"));
    reader.readAsDataURL(file);
  });
}

function timelineAttachmentFromRecord(record: AttachmentRecord): TimelineAttachment {
  return {
    id: record.id,
    mediaType: record.mediaType,
    filename: record.filename,
    width: record.width,
    height: record.height,
    byteSize: record.byteSize,
    dataUrl: record.dataUrl,
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
  if ("interactionChanged" in event) {
    return statusTextForInteraction(
      {
        sessionId: "",
        event: event.interactionChanged.event,
      },
      t,
    );
  }
  if ("agentRuntimeUpdated" in event) return t("status.running");
  if ("skillActivated" in event) return t("status.running");
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

function statusTextForInteraction(
  payload: InteractionChangedPayload,
  t: (key: string, args?: Record<string, unknown>) => string,
) {
  const interaction = payload.event.interaction;
  if (interaction.status !== "pending") {
    return t("status.ready");
  }
  switch (interaction.kind) {
    case "toolApproval":
      return interaction.payload.type === "toolApproval"
        ? t("status.approvalRequired", { name: interaction.payload.name })
        : t("status.approvalRequired", { name: "tool" });
    case "userInput":
      return t("status.userInputRequired");
    case "planConfirmation":
      return t("planConfirm.promptTitle");
  }
}

function statusTextForStudioEvent(
  envelope: StudioEventEnvelope,
  t: (key: string, args?: Record<string, unknown>) => string,
) {
  const kind = envelope.kind;
  switch (kind.type) {
    case "turnChanged":
      return statusTextForStudioTurn(kind.turn.status, kind.turn.reason, t);
    case "timelineChanged":
      return statusTextForEvent(agentEventFromStudioTimelineChange(kind.change), t);
    case "interactionChanged":
      return statusTextForInteraction(
        {
          sessionId: envelope.sessionId ?? kind.event.interaction.scope.sessionId,
          event: kind.event,
        },
        t,
      );
    case "agentChanged":
    case "agentTimelineChanged":
    case "sessionRuntimeChanged":
    case "skillActivated":
    case "planLifecycleChanged":
    case "sessionHandoffChanged":
      return t("status.running");
    case "sessionListChanged":
    case "mcpHealthChanged":
    case "lspHealthChanged":
      return t("status.ready");
    case "stale":
      return t("status.running");
  }
}

function statusTextForStudioTurn(
  status: StudioTurnStatus,
  reason: string | null | undefined,
  t: (key: string, args?: Record<string, unknown>) => string,
) {
  if (reason && (status === "failed" || status === "cancelled")) {
    return status === "failed" ? t("status.runFailed", { error: reason }) : t("status.interrupted");
  }
  switch (status) {
    case "queued":
    case "contextLoading":
    case "waitingForModel":
      return t("status.waitingForModel");
    case "streaming":
      return t("status.running");
    case "waitingForInteraction":
      return t("status.userInputRequired");
    case "runningTool":
      return t("turnPhase.tool");
    case "persisting":
      return t("status.running");
    case "completed":
      return t("status.done");
    case "failed":
      return t("status.runFailed", { error: reason ?? "unknown" });
    case "cancelled":
      return t("status.interrupted");
  }
}

function agentEventFromStudioTimelineChange(change: StudioTimelineChange): AgentEvent {
  switch (change.type) {
    case "started":
      return { timelineItemStarted: { item: change.item } };
    case "delta":
      return { timelineItemDelta: { event: change.event } };
    case "completed":
      return { timelineItemCompleted: { sequence: change.sequence, item: change.item } };
    case "failed":
      return {
        timelineItemFailed: {
          sequence: change.sequence,
          item: change.item,
          error: change.error,
        },
      };
  }
}

export function useStudioApp() {
  const { t } = useTranslation();
  const [state, dispatch] = useReducer(studioReducer, initialStudioState(t("status.starting")));
  const providerSaveQueueRef = useRef<Promise<void>>(Promise.resolve());
  const providerSaveVersionRef = useRef(0);

  const selectedProject = selectSelectedProject(state);
  const selectedSession = selectSelectedSession(state);
  const activeInteraction = selectActiveInteraction(state);

  const timelineEntries = useMemo(() => {
    return selectTimelineEntries(state);
  }, [state.timelineItems, state.timelineOrder, state.planStates]);

  const providerUsageAutoRefreshKey = useMemo(() => {
    if (!state.settingsOpen || state.activeSettingsTab !== "providers") {
      return "";
    }
    return state.providers
      .map(
        (provider) =>
          `${provider.id}:${provider.templateKind}:${provider.hasBearerToken ?? provider.bearerToken.length > 0}`,
      )
      .join("|");
  }, [state.settingsOpen, state.activeSettingsTab, state.providers]);

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
      dispatch({ type: "timelineLoaded", sessionId: null, events: [], planStates: [], nextSequence: 0 });
      return;
    }
    const sessionId = state.selectedSessionId;
    reloadTimeline(sessionId);
  }, [state.selectedSessionId, t]);

  useEffect(() => {
    if (!providerUsageAutoRefreshKey) {
      return;
    }
    void refreshProviderUsages();
  }, [providerUsageAutoRefreshKey]);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    const unlisteners = [
      listen<StudioEventEnvelope>("studio-runtime-event", ({ payload }) => {
        if (payload.kind.type === "stale" && payload.sessionId) {
          const afterSequence = Math.max(0, payload.sequence - payload.kind.laggedEvents - 1);
          void reloadStudioEvents(payload.sessionId, afterSequence);
          return;
        }
        dispatch({
          type: "studioEvent",
          envelope: payload,
          status: statusTextForStudioEvent(payload, t),
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
          events: payload.events,
          planStates: payload.planStates ?? [],
          interactions: payload.interactions ?? [],
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

  function reloadStudioEvents(sessionId: string, afterSequence?: number) {
    return loadStudioEvents(sessionId, afterSequence)
      .then((payload) => {
        for (const envelope of payload.events) {
          dispatch({
            type: "studioEvent",
            envelope,
            status: statusTextForStudioEvent(envelope, t),
          });
        }
      })
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

  async function onArchiveProject(projectId: string) {
    if (state.isBusy) {
      return;
    }
    try {
      const payload = await archiveProject(projectId, state.selectedProjectId);
      dispatch({ type: "projectSelectionLoaded", payload, status: t("status.projectArchived") });
    } catch (error) {
      dispatch({
        type: "bootstrapFailed",
        status: t("status.archiveProjectFailed", { error: errorText(error) }),
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
    if (sessionId === state.selectedSessionId) {
      return;
    }
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

  async function onDeleteSession(sessionId: string) {
    if (state.isBusy) {
      return;
    }
    const session = state.sessions.find((candidate) => candidate.id === sessionId);
    const title = session?.title ?? t("common.sessionFallbackTitle");
    if (!window.confirm(t("sessions.confirmDelete", { title }))) {
      return;
    }
    try {
      const payload = await deleteSession(sessionId, state.selectedSessionId);
      dispatch({ type: "projectSelectionLoaded", payload, status: t("status.sessionDeleted") });
    } catch (error) {
      dispatch({
        type: "bootstrapFailed",
        status: t("status.deleteSessionFailed", { error: errorText(error) }),
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

  async function onCreatePromptAttachment(file: File): Promise<AttachmentRecord | null> {
    const sessionId = state.selectedSessionId;
    if (!sessionId) {
      return null;
    }
    const dataUrl = await fileToDataUrl(file);
    return createPromptAttachment(sessionId, dataUrl, file.name);
  }

  async function onSendPrompt(attachments: AttachmentRecord[] = []) {
    const content = state.prompt.trim();
    const sessionId = state.selectedSessionId;
    if ((!content && attachments.length === 0) || !sessionId || state.isBusy) {
      return;
    }
    await submitPrompt(sessionId, content, attachments);
  }

  async function onSendPromptContent(content: string) {
    const trimmed = content.trim();
    const sessionId = state.selectedSessionId;
    if (!trimmed || !sessionId || state.isBusy) {
      return;
    }
    await submitPrompt(sessionId, trimmed);
  }

  async function onResolveInteraction(
    interactionId: string,
    resolution: InteractionResolution,
  ): Promise<boolean> {
    const interaction = state.interactions.get(interactionId);
    const sessionId = state.selectedSessionId;
    if (!interaction || !sessionId) {
      return false;
    }
    if (resolution.type === "planConfirmation" && resolution.decision === "implementFreshContext") {
      dispatch({
        type: "planImplementationSubmitted",
        status: t("status.running"),
        startedAt: Date.now(),
      });
    }
    try {
      const payload = await resolveInteraction(interactionId, resolution);
      dispatch({
        type: "interactionChanged",
        payload: {
          sessionId: payload.sessionId,
          event: { interaction: payload.interaction },
        },
        status: t("status.ready"),
      });
      if (payload.planLifecycle) {
        dispatch({
          type: "planLifecycleLoaded",
          sessionId: payload.planLifecycle.sessionId,
          planStates: payload.planLifecycle.planStates,
          timelineNextSequence: payload.planLifecycle.timelineNextSequence,
          status: t("status.ready"),
        });
      }
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

  async function onImplementPlanFresh(interactionId: string): Promise<boolean> {
    const sessionId = state.selectedSessionId;
    if (!interactionId || !sessionId || state.isBusy) {
      return false;
    }
    return onResolveInteraction(interactionId, {
      type: "planConfirmation",
      decision: "implementFreshContext",
    });
  }

  async function onDiscussPlan(interactionId: string, content: string): Promise<boolean> {
    const trimmed = content.trim();
    if (!trimmed || state.isBusy) {
      return false;
    }
    const resolved = await onResolveInteraction(interactionId, {
      type: "planConfirmation",
      decision: "continuePlanning",
      content: trimmed,
      reason: "continue planning",
    });
    if (!resolved) {
      return false;
    }
    const sessionId = state.selectedSessionId;
    if (sessionId) {
      await submitPrompt(sessionId, trimmed, []);
      return true;
    }
    return false;
  }

  async function submitPrompt(sessionId: string, content: string, attachments: AttachmentRecord[] = []) {
    const timelineAttachments = attachments.map(timelineAttachmentFromRecord);
    dispatch({
      type: "promptSubmitted",
      status: t("status.running"),
      startedAt: Date.now(),
      prompt: content,
      attachments: timelineAttachments,
    });
    try {
      if (isTauriRuntime()) {
        await submitPromptCommand(sessionId, content, attachments.map((attachment) => attachment.id));
      } else {
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
      }
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

  async function onSaveMcpSettings(servers: McpServerInput[]): Promise<boolean> {
    try {
      dispatch({
        type: "configLoaded",
        payload: await saveMcpSettings({ servers }),
        status: t("status.mcpSettingsSaved"),
      });
      return true;
    } catch (error) {
      dispatch({
        type: "bootstrapFailed",
        status: t("status.mcpSettingsInvalid", { error: errorText(error) }),
      });
      return false;
    }
  }

  async function onSaveInstructionsSettings(input: InstructionsInput): Promise<boolean> {
    try {
      dispatch({
        type: "configLoaded",
        payload: await saveInstructionsSettings(input),
        status: t("status.instructionsSettingsSaved"),
      });
      return true;
    } catch (error) {
      dispatch({
        type: "bootstrapFailed",
        status: t("status.instructionsSettingsInvalid", { error: errorText(error) }),
      });
      return false;
    }
  }

  async function refreshProviderUsages() {
    dispatch({ type: "providerUsagesLoading" });
    try {
      const payload = await loadProviderUsages();
      dispatch({ type: "providerUsagesLoaded", usages: payload.usages });
    } catch (error) {
      dispatch({ type: "providerUsagesFailed", error: errorText(error) });
    }
  }

  return {
    state,
    timelineEntries,
    selectedProject,
    selectedSession,
    activeInteraction,
    setRolesState,
    setProvidersState,
    setSelectedProviderIdState,
    setProviderSearchState,
    setConfigTomlState,
    dispatch,
    addProject,
    chooseFolder,
    onSelectProject,
    onArchiveProject,
    onNewSession,
    onSelectSession,
    onDeleteSession,
    onSetSessionMode,
    onCreatePromptAttachment,
    onSendPrompt,
    onSendPromptContent,
    onResolveInteraction,
    onImplementPlanFresh,
    onDiscussPlan,
    onDismissPlanAction: (interactionId: string) =>
      void onResolveInteraction(interactionId, {
        type: "planConfirmation",
        decision: "dismiss",
        reason: "dismissed",
      }),
    onStopPrompt,
    openSettings,
    onSaveConfig,
    onSaveProviderSettings,
    onSaveInstructionsSettings,
    onSavePermissionMode,
    onSaveMcpSettings,
    refreshProviderUsages,
    onReloadConfig,
  };
}

import { listen } from "@tauri-apps/api/event";
import { createStore, produce } from "solid-js/store";
import { onCleanup } from "solid-js";
import type {
  AgentDto,
  BootstrapPayload,
  CompileMode,
  ConfigPayload,
  InstructionsInput,
  InstructionsRecord,
  InteractionRequest,
  LspHealthUpdatedPayload,
  LspServerRecord,
  McpSettingsInput,
  McpHealthUpdatedPayload,
  McpServerRecord,
  PermissionMode,
  PlanState,
  ProjectRecord,
  ProviderRecord,
  ProviderSettingsInput,
  ProviderSettingsSaveSnapshot,
  ProviderTemplateRecord,
  ProviderUsageRecord,
  RoleRecord,
  SessionRecord,
  SessionRuntime,
  SkillActivation,
  StudioAgentTimelineEvent,
  StudioEventEnvelope,
  StudioMessage,
  StudioMessageProjection,
  StudioPart,
  StudioPartDelta,
  StudioPartProjection,
} from "../types";
import {
  bootstrapStudio,
  archiveProject as archiveProjectCommand,
  chooseProjectDirectory as chooseProjectDirectoryCommand,
  createSession,
  deleteSession as deleteSessionCommand,
  isTauriRuntime,
  loadProviderUsages,
  loadSessionState,
  loadStudioEvents,
  openProject,
  resolveInteraction,
  saveInstructionsSettings,
  saveMcpSettings,
  savePermissionMode,
  saveProviderSettings,
  selectProject,
  selectSession,
  setSessionMode,
  stopPrompt,
  submitPromptCommand,
} from "../lib/tauri";
import i18n from "../i18n";
import { selectedSessionBusy } from "./studio-selectors";

export type SettingsTab =
  | "providers"
  | "instructions"
  | "skills"
  | "roles"
  | "mcp"
  | "security"
  | "general";

export type MessageStore = {
  projects: ProjectRecord[];
  sessions: SessionRecord[];
  selectedProjectId: string | null;
  selectedSessionId: string | null;
  providers: ProviderRecord[];
  providerTemplates: ProviderTemplateRecord[];
  providerUsages: ProviderUsageRecord[];
  providerUsagesLoading: boolean;
  providerUsageError: string | null;
  providerUsageErrors: Record<string, string | undefined>;
  providerUsageRefreshing: Record<string, boolean | undefined>;
  providerUsagesLoadedAt: number | null;
  roles: RoleRecord[];
  instructions: InstructionsRecord;
  configToml: string;
  configExists: boolean;
  selectedProviderId: string | null;
  providerSearch: string;
  activeSettingsTab: SettingsTab;
  permissionMode: PermissionMode;
  prompt: string;
  status: string;
  busy: boolean;
  sessionBusy: Record<string, boolean | undefined>;
  settingsOpen: boolean;
  mcpServers: McpServerRecord[];
  activeMcpServers: string[];
  lspServers: LspServerRecord[];
  activeLspServers: string[];
  turnPhase: Record<string, string | undefined>;
  turnStartedAt: Record<string, number | null | undefined>;
  messages: Record<string, StudioMessage[] | undefined>;
  parts: Record<string, StudioPart[] | undefined>;
  partTextAccumDelta: Record<string, string | undefined>;
  partDeltaChunks: Record<string, Record<number, string> | undefined>;
  messageSequence: Record<string, number | undefined>;
  partSequence: Record<string, number | undefined>;
  partDeltaSequence: Record<string, number | undefined>;
  eventNextSequence: Record<string, number | undefined>;
  sessionRuntime: Record<string, SessionRuntime | null | undefined>;
  agents: Record<string, AgentDto[] | undefined>;
  agentEvents: Record<string, StudioAgentTimelineEvent[] | undefined>;
  interactions: Record<string, InteractionRequest | undefined>;
  activeInteractionId: string | null;
  activeInteractionPhase: "toolApproval" | "userInput" | "planConfirmation" | null;
  planStates: Record<string, PlanState | undefined>;
};

export function createStudioStore() {
  const [store, setStore] = createStore<MessageStore>({
    projects: [],
    sessions: [],
    selectedProjectId: null,
    selectedSessionId: null,
    providers: [],
    providerTemplates: [],
    providerUsages: [],
    providerUsagesLoading: false,
    providerUsageError: null,
    providerUsageErrors: {},
    providerUsageRefreshing: {},
    providerUsagesLoadedAt: null,
    roles: [],
    instructions: {
      baseOverride: "",
      developer: "",
      user: "",
      projectDocMaxBytes: 65536,
      projectDocFallbackFilenames: [],
    },
    configToml: "",
    configExists: false,
    selectedProviderId: null,
    providerSearch: "",
    activeSettingsTab: "providers",
    permissionMode: "request-approval",
    prompt: "",
    status: i18n.t("status.starting"),
    busy: false,
    sessionBusy: {},
    settingsOpen: false,
    mcpServers: [],
    activeMcpServers: [],
    lspServers: [],
    activeLspServers: [],
    turnPhase: {},
    turnStartedAt: {},
    messages: {},
    parts: {},
    partTextAccumDelta: {},
    partDeltaChunks: {},
    messageSequence: {},
    partSequence: {},
    partDeltaSequence: {},
    eventNextSequence: {},
    sessionRuntime: {},
    agents: {},
    agentEvents: {},
    interactions: {},
    activeInteractionId: null,
    activeInteractionPhase: null,
    planStates: {},
  });

  const queue: StudioEventEnvelope[] = [];
  let timer: number | undefined;
  const coalesced = new Map<string, number>();
  const staleDeltas = new Set<string>();

  function init() {
    void bootstrapStudio()
      .then((payload) => {
        applyBootstrap(payload);
        const sessionId = payload.selectedSessionId ?? payload.sessions[0]?.id;
        if (sessionId) void reloadSession(sessionId);
        setupEvents();
      })
      .catch((error) => setStore("status", errorText(error)));
  }

  function setupEvents() {
    if (!isTauriRuntime()) return;
    let disposed = false;
    const unlisten = listen<StudioEventEnvelope>("studio-runtime-event", ({ payload }) => {
      if (disposed) return;
      enqueueEvent(payload);
      if (payload.kind.type === "sessionHandoffChanged") {
        void reloadSession(payload.kind.handoff.targetSessionId);
      }
      if (payload.kind.type === "stale" && payload.sessionId) {
        const afterSequence = Math.max(0, (store.eventNextSequence[payload.sessionId] ?? 0) - 1);
        void loadStudioEvents(payload.sessionId, afterSequence).then((response) => {
          for (const event of response.events) enqueueEvent(event);
        });
      }
    });
    onCleanup(() => {
      disposed = true;
      void unlisten.then((fn) => fn());
    });
  }

  function enqueueEvent(event: StudioEventEnvelope) {
    const key = coalesceKey(event);
    if (key) {
      const index = coalesced.get(key);
      if (index !== undefined) {
        if (event.kind.type === "messagePartUpdated") {
          staleDeltas.add(deltaKey(event.sessionId ?? "", event.kind.part.messageId, event.kind.part.partId));
        }
        queue[index] = event;
        scheduleFlush();
        return;
      }
      coalesced.set(key, queue.length);
    }
    queue.push(event);
    scheduleFlush();
  }

  function scheduleFlush() {
    if (timer !== undefined) return;
    timer = window.setTimeout(flushEvents, 16);
  }

  function flushEvents() {
    if (timer !== undefined) window.clearTimeout(timer);
    timer = undefined;
    const events = queue.splice(0);
    const staleDeltaKeys = new Set(staleDeltas);
    staleDeltas.clear();
    coalesced.clear();
    setStore(produce((draft) => applyStudioEventBatch(draft, events, draft.selectedSessionId, staleDeltaKeys)));
  }

  function applyBootstrap(payload: BootstrapPayload) {
    const config = payload.config;
    setStore(
      produce((draft) => {
        draft.projects = payload.projects;
        draft.sessions = mergeSessions(draft.sessions, payload.sessions);
        draft.selectedProjectId = payload.selectedProjectId ?? payload.projects[0]?.id ?? null;
        draft.selectedSessionId = payload.selectedSessionId ?? payload.sessions[0]?.id ?? null;
        applyConfig(draft, config);
        const sessionId = draft.selectedSessionId;
        draft.busy = selectedSessionBusy(draft, sessionId);
        if (sessionId) {
          draft.agents[sessionId] = payload.agents;
          draft.agentEvents[sessionId] = payload.agentEvents;
          draft.sessionRuntime[sessionId] = payload.sessionRuntime ?? null;
          applyMcpHealth(draft, payload.mcpHealth ?? null);
          applyLspHealth(draft, payload.lspHealth ?? null);
          applyInteractions(draft, payload.interactions ?? [], sessionId);
        }
        draft.status = i18n.t("status.ready");
      }),
    );
  }

  function applyConfig(draft: MessageStore, config: ConfigPayload) {
    draft.providers = config.providers;
    draft.providerTemplates = config.templates;
    draft.roles = config.roles;
    draft.instructions = config.instructions;
    draft.configToml = config.toml;
    draft.configExists = config.configExists;
    draft.permissionMode = config.permissionMode;
    draft.mcpServers = config.mcpServers;
    draft.selectedProviderId = normalizeSelectedProviderId(draft.selectedProviderId, config.providers);
    pruneProviderUsageState(draft, config.providers);
    draft.activeMcpServers = config.mcpServers
      .filter((server) => server.availabilityKind === "available")
      .map((server) => server.id);
  }

  function applyProjection(sessionId: string, messages: StudioMessageProjection[], parts: StudioPartProjection[], nextSequence: number) {
    setStore(
      produce((draft) => applyStudioProjection(draft, sessionId, messages, parts, nextSequence)),
    );
  }

  function applyEvent(event: StudioEventEnvelope) {
    setStore(produce((draft) => applyStudioEvent(draft, event, draft.selectedSessionId)));
  }

  async function reloadSession(sessionId: string) {
    const payload = await loadSessionState(sessionId);
    setStore(
      produce((draft) => {
        draft.sessions = mergeSessions(draft.sessions, [payload.session, ...payload.sessions]);
        draft.selectedSessionId = payload.sessionId;
        draft.busy = selectedSessionBusy(draft, payload.sessionId);
        draft.agents[sessionId] = payload.agents;
        draft.agentEvents[sessionId] = payload.agentEvents;
        draft.sessionRuntime[sessionId] = payload.sessionRuntime;
        applySessionRuntimeHealth(draft, payload.sessionRuntime ?? null);
        applyInteractions(draft, payload.interactions, sessionId);
      }),
    );
    applyProjection(sessionId, payload.messages, payload.parts, payload.eventNextSequence);
    setStore(produce((draft) => applyStudioEventBatch(draft, payload.events, draft.selectedSessionId)));
  }

  async function applyProjectSelectionPayload(payload: {
    selectedProjectId?: string | null;
    projects: ProjectRecord[];
    sessions: SessionRecord[];
    selectedSessionId?: string | null;
    agentEvents: StudioAgentTimelineEvent[];
    agents: AgentDto[];
    sessionRuntime?: SessionRuntime | null;
    interactions?: InteractionRequest[];
    mcpHealth?: McpHealthUpdatedPayload | null;
    lspHealth?: LspHealthUpdatedPayload | null;
  }, status: string) {
    const selectedSessionId = payload.selectedSessionId ?? null;
    setStore(
      produce((draft) => {
        draft.projects = payload.projects;
        draft.sessions = replaceProjectSessions(draft.sessions, payload.selectedProjectId ?? null, payload.sessions);
        draft.selectedProjectId = payload.selectedProjectId ?? null;
        draft.selectedSessionId = selectedSessionId;
        draft.busy = selectedSessionBusy(draft, selectedSessionId);
        if (selectedSessionId) {
          draft.agents[selectedSessionId] = payload.agents;
          draft.agentEvents[selectedSessionId] = payload.agentEvents;
          draft.sessionRuntime[selectedSessionId] = payload.sessionRuntime ?? null;
          applyInteractions(draft, payload.interactions ?? [], selectedSessionId);
        } else {
          draft.activeInteractionId = null;
          draft.activeInteractionPhase = null;
        }
        applyMcpHealth(draft, payload.mcpHealth ?? null);
        applyLspHealth(draft, payload.lspHealth ?? null);
        draft.status = status;
      }),
    );
    if (selectedSessionId) {
      await reloadSession(selectedSessionId);
    }
  }

  async function submitPrompt() {
    const sessionId = store.selectedSessionId;
    const prompt = store.prompt.trim();
    if (!sessionId || !prompt || selectedSessionBusy(store, sessionId)) return;
    const now = Math.floor(Date.now() / 1000);
    const stamp = Date.now();
    const turnId = `optimistic-turn-${stamp}`;
    const messageId = `optimistic-message-${stamp}`;
    const partId = `optimistic-user-${stamp}`;
    setStore(
      produce((draft) => {
        draft.prompt = "";
        draft.busy = true;
        draft.sessionBusy[sessionId] = true;
        draft.status = i18n.t("status.running");
        upsertMessage(draft, {
          messageId,
          sessionId,
          turnId,
          role: "user",
          status: "streaming",
          createdAt: now,
          updatedAt: now,
        }, undefined);
        upsertPart(draft, {
          partId,
          messageId,
          sessionId,
          turnId,
          partType: "text",
          order: draft.eventNextSequence[sessionId] ?? 0,
          status: "streaming",
          createdAt: now,
          updatedAt: now,
          textChannel: "user",
          text: prompt,
        }, undefined);
      }),
    );
    try {
      await submitPromptCommand(sessionId, prompt, []);
    } catch (error) {
      setStore("status", errorText(error));
      setStore("busy", false);
      setStore("sessionBusy", sessionId, false);
    }
  }

  async function refreshProviderUsages(providerId?: string) {
    const targetProviderId = providerId?.trim() || null;
    if (targetProviderId) {
      if (store.providerUsageRefreshing[targetProviderId]) return;
    } else if (store.providerUsagesLoading) {
      return;
    }
    setStore(produce((draft) => {
      if (targetProviderId) {
        draft.providerUsageRefreshing[targetProviderId] = true;
        delete draft.providerUsageErrors[targetProviderId];
      } else {
        draft.providerUsagesLoading = true;
        draft.providerUsageError = null;
        for (const provider of draft.providers) {
          delete draft.providerUsageErrors[provider.id];
        }
      }
    }));
    try {
      const payload = await loadProviderUsages();
      const loadedAt = Math.floor(Date.now() / 1000);
      setStore(produce((draft) => {
        draft.providerUsages = mergeProviderUsages(draft.providerUsages, payload.usages);
        draft.providerUsagesLoading = false;
        draft.providerUsageError = null;
        draft.providerUsagesLoadedAt = loadedAt;
        if (targetProviderId) {
          draft.providerUsageRefreshing[targetProviderId] = false;
          delete draft.providerUsageErrors[targetProviderId];
        }
      }));
    } catch (error) {
      const message = errorText(error);
      setStore(produce((draft) => {
        if (targetProviderId) {
          draft.providerUsageRefreshing[targetProviderId] = false;
          draft.providerUsageErrors[targetProviderId] = message;
        } else {
          draft.providerUsagesLoading = false;
          draft.providerUsageError = message;
        }
        draft.status = i18n.t("status.configLoadFailed", { error: message });
      }));
    }
  }

  return {
    store,
    setStore,
    init,
    reloadSession,
    actions: {
      setPrompt(value: string) {
        setStore("prompt", value);
      },
      setSettingsOpen(value: boolean) {
        setStore("settingsOpen", value);
      },
      openSettings(tab?: SettingsTab) {
        setStore(produce((draft) => {
          draft.settingsOpen = true;
          if (tab) draft.activeSettingsTab = tab;
        }));
        if ((tab ?? store.activeSettingsTab) === "providers" && shouldRefreshProviderUsages(store)) {
          void refreshProviderUsages();
        }
      },
      setSettingsTab(tab: SettingsTab) {
        setStore("activeSettingsTab", tab);
        if (tab === "providers" && shouldRefreshProviderUsages(store)) {
          void refreshProviderUsages();
        }
      },
      setProviderSearch(value: string) {
        setStore("providerSearch", value);
      },
      setSelectedProviderId(providerId: string | null) {
        setStore("selectedProviderId", normalizeSelectedProviderId(providerId, store.providers));
      },
      refreshProviderUsages,
      async addProject(path: string) {
        try {
          const payload = await openProject(path);
          await applyProjectSelectionPayload(payload, i18n.t("status.projectLoaded"));
        } catch (error) {
          setStore("status", i18n.t("status.addProjectFailed", { error: errorText(error) }));
        }
      },
      async chooseProjectDirectory() {
        try {
          const path = await chooseProjectDirectoryCommand();
          if (!path) return;
          const payload = await openProject(path);
          await applyProjectSelectionPayload(payload, i18n.t("status.projectLoaded"));
        } catch (error) {
          setStore("status", i18n.t("status.addProjectFailed", { error: errorText(error) }));
        }
      },
      async selectProject(id: string) {
        try {
          const payload = await selectProject(id);
          await applyProjectSelectionPayload(payload, i18n.t("status.projectLoaded"));
        } catch (error) {
          setStore("status", i18n.t("status.selectProjectFailed", { error: errorText(error) }));
        }
      },
      async archiveProject(projectId: string) {
        if (store.projects.length === 0) return;
        try {
          const payload = await archiveProjectCommand(projectId, store.selectedProjectId);
          await applyProjectSelectionPayload(payload, i18n.t("status.projectArchived"));
        } catch (error) {
          setStore("status", i18n.t("status.archiveProjectFailed", { error: errorText(error) }));
        }
      },
      async createSession() {
        if (!store.selectedProjectId) return;
        try {
          const payload = await createSession(store.selectedProjectId, i18n.t("common.newSessionTitle"));
          setStore("sessions", (sessions) => mergeSessions(sessions, payload.sessions));
          setStore("selectedSessionId", payload.sessionId);
          setStore("busy", selectedSessionBusy(store, payload.sessionId));
          await reloadSession(payload.sessionId);
        } catch (error) {
          setStore("status", i18n.t("status.newSessionFailed", { error: errorText(error) }));
        }
      },
      async selectSession(id: string) {
        if (id === store.selectedSessionId) return;
        try {
          const payload = await selectSession(id);
          setStore(produce((draft) => {
            draft.sessions = mergeSessions(draft.sessions, payload.sessions);
            draft.selectedSessionId = payload.sessionId;
            draft.busy = selectedSessionBusy(draft, payload.sessionId);
            applyMcpHealth(draft, payload.mcpHealth ?? null);
            applyLspHealth(draft, payload.lspHealth ?? null);
          }));
          await reloadSession(payload.sessionId);
        } catch (error) {
          setStore("status", i18n.t("status.selectSessionFailed", { error: errorText(error) }));
        }
      },
      async deleteSession(sessionId: string) {
        try {
          const payload = await deleteSessionCommand(sessionId, store.selectedSessionId);
          await applyProjectSelectionPayload(payload, i18n.t("status.sessionDeleted"));
        } catch (error) {
          setStore("status", i18n.t("status.deleteSessionFailed", { error: errorText(error) }));
        }
      },
      async setSessionMode(mode: CompileMode) {
        const sessionId = store.selectedSessionId;
        if (!sessionId) return;
        const payload = await setSessionMode(sessionId, mode);
        setStore("sessions", (sessions) => {
          const existing = sessions.find((session) => session.id === sessionId);
          const selected = existing ? [{ ...existing, mode, updatedAt: Math.floor(Date.now() / 1000) }] : [];
          return mergeSessions(sessions, [...selected, ...payload.sessions]);
        });
      },
      async saveProviderSettings(snapshot?: ProviderSettingsSaveSnapshot) {
        try {
          const config = await saveProviderSettings(providerSettingsInput(store, snapshot));
          setStore(produce((draft) => {
            applyConfig(draft, config);
            draft.status = i18n.t("status.providerSettingsSaved");
          }));
          void refreshProviderUsages();
          return true;
        } catch (error) {
          setStore("status", i18n.t("status.providerSettingsInvalid", { error: errorText(error) }));
          return false;
        }
      },
      async saveInstructionsSettings(input: InstructionsInput) {
        try {
          const config = await saveInstructionsSettings(input);
          setStore(produce((draft) => {
            applyConfig(draft, config);
            draft.status = i18n.t("status.instructionsSettingsSaved");
          }));
          return true;
        } catch (error) {
          setStore("status", i18n.t("status.instructionsSettingsInvalid", { error: errorText(error) }));
          return false;
        }
      },
      async saveMcpSettings(input: McpSettingsInput) {
        try {
          const config = await saveMcpSettings(input);
          setStore(produce((draft) => {
            applyConfig(draft, config);
            draft.status = i18n.t("status.mcpSettingsSaved");
          }));
          return true;
        } catch (error) {
          setStore("status", i18n.t("status.mcpSettingsInvalid", { error: errorText(error) }));
          return false;
        }
      },
      async savePermissionMode(mode: PermissionMode) {
        try {
          const config = await savePermissionMode(mode);
          setStore(produce((draft) => {
            applyConfig(draft, config);
            draft.status = i18n.t("status.permissionModeSaved");
          }));
        } catch (error) {
          setStore("status", i18n.t("status.permissionModeSaveFailed", { error: errorText(error) }));
        }
      },
      submitPrompt,
      async stop() {
        const sessionId = store.selectedSessionId;
        if (!sessionId) return;
        if (!selectedSessionBusy(store, sessionId)) return;
        setStore(produce((draft) => {
          draft.status = i18n.t("status.stopping");
          draft.busy = true;
          draft.sessionBusy[sessionId] = true;
          draft.turnPhase[sessionId] = "stopping";
        }));
        try {
          const response = await stopPrompt(sessionId);
          if (!response.stopped) {
            setStore(produce((draft) => {
              draft.sessionBusy[sessionId] = false;
              if (draft.selectedSessionId === sessionId) {
                draft.busy = false;
                draft.status = i18n.t("status.ready");
                draft.turnPhase[sessionId] = "idle";
                draft.turnStartedAt[sessionId] = null;
              }
            }));
          }
        } catch (error) {
          setStore("status", errorText(error));
        }
      },
      async resolveInteraction(interactionId: string, resolution: Parameters<typeof resolveInteraction>[1]) {
        await resolveInteraction(interactionId, resolution);
      },
    },
  };
}

function upsertMessage(draft: MessageStore, message: StudioMessage, sequence: number | undefined) {
  const existingSequence = draft.messageSequence[message.messageId];
  if (sequence !== undefined && existingSequence !== undefined && existingSequence > sequence) return;
  const list = draft.messages[message.sessionId] ?? [];
  const index = list.findIndex((item) => item.messageId === message.messageId);
  draft.messages[message.sessionId] = (index >= 0
    ? [...list.slice(0, index), message, ...list.slice(index + 1)]
    : [...list, message]).sort(compareMessages);
  if (sequence !== undefined) draft.messageSequence[message.messageId] = sequence;
}

export function applyStudioProjection(
  draft: MessageStore,
  sessionId: string,
  messages: StudioMessageProjection[],
  parts: StudioPartProjection[],
  nextSequence: number,
) {
  const projectionMessageIds = new Set(messages.map((record) => record.message.messageId));
  for (const message of draft.messages[sessionId] ?? []) {
    if (!projectionMessageIds.has(message.messageId)) removeMessage(draft, sessionId, message.messageId);
  }
  draft.messages[sessionId] = messages
    .map((record) => record.message)
    .sort(compareMessages);
  for (const record of messages) draft.messageSequence[record.message.messageId] = record.sequence;
  const byMessage: Record<string, StudioPart[]> = {};
  for (const record of parts) {
    const list = byMessage[record.part.messageId] ?? [];
    list.push(record.part);
    byMessage[record.part.messageId] = list;
    draft.partSequence[record.part.partId] = record.sequence;
    delete draft.partTextAccumDelta[record.part.partId];
    delete draft.partDeltaChunks[record.part.partId];
  }
  for (const [messageId, list] of Object.entries(draft.parts)) {
    if (!projectionMessageIds.has(messageId) && (list ?? []).some((part) => part.sessionId === sessionId)) {
      delete draft.parts[messageId];
    }
  }
  for (const [messageId, list] of Object.entries(byMessage)) {
    draft.parts[messageId] = list.sort(compareParts);
  }
  draft.eventNextSequence[sessionId] = Math.max(draft.eventNextSequence[sessionId] ?? 0, nextSequence);
}

export function applyStudioEventBatch(
  draft: MessageStore,
  events: StudioEventEnvelope[],
  selectedSessionId: string | null,
  staleDeltaKeys = new Set<string>(),
) {
  const latestSnapshotIndex = new Map<string, number>();
  events.forEach((event, index) => {
    if (event.kind.type === "messagePartUpdated") {
      const part = event.kind.part;
      latestSnapshotIndex.set(deltaKey(event.sessionId ?? "", part.messageId, part.partId), index);
    }
  });
  for (const [index, event] of events.entries()) {
    if (event.kind.type === "messagePartDelta") {
      const delta = event.kind.delta;
      const key = deltaKey(event.sessionId ?? "", delta.messageId, delta.partId);
      const snapshotIndex = latestSnapshotIndex.get(key);
      if (staleDeltaKeys.has(key) || (snapshotIndex !== undefined && snapshotIndex > index)) continue;
    }
    applyStudioEvent(draft, event, selectedSessionId);
  }
}

export function applyStudioEvent(
  draft: MessageStore,
  event: StudioEventEnvelope,
  selectedSessionId: string | null,
) {
  const sessionId = event.sessionId;
  const kind = event.kind;
  if (kind.type !== "messagePartDelta" && kind.type !== "stale" && sessionId) {
    draft.eventNextSequence[sessionId] = Math.max(draft.eventNextSequence[sessionId] ?? 0, event.sequence + 1);
  }
  switch (kind.type) {
    case "messageUpdated":
      upsertMessage(draft, kind.message, event.sequence);
      break;
    case "messageRemoved":
      removeMessage(draft, sessionId, kind.messageId);
      break;
    case "messagePartUpdated":
      if (isRealUserTextPart(kind.part)) removeOptimisticUserMessage(draft, kind.part.sessionId);
      upsertPart(draft, kind.part, event.sequence);
      break;
    case "messagePartRemoved":
      removePart(draft, kind.messageId, kind.partId);
      break;
    case "messagePartDelta":
      applyDelta(draft, kind.delta, event.sequence);
      break;
    case "turnChanged":
      draft.sessionBusy[kind.turn.sessionId] = isBusyTurnStatus(kind.turn.status);
      if (kind.turn.sessionId === selectedSessionId) {
        draft.busy = draft.sessionBusy[kind.turn.sessionId] ?? false;
        draft.status = statusForTurn(kind.turn.status);
      }
      draft.turnPhase[kind.turn.sessionId] = turnPhaseForStatus(kind.turn.status);
      if (["queued", "contextLoading", "waitingForModel", "streaming", "runningTool", "waitingForInteraction", "persisting"].includes(kind.turn.status)) {
        draft.turnStartedAt[kind.turn.sessionId] = draft.turnStartedAt[kind.turn.sessionId] ?? kind.turn.updatedAt;
      } else {
        draft.turnStartedAt[kind.turn.sessionId] = null;
      }
      break;
    case "interactionChanged":
      draft.interactions[kind.event.interaction.interactionId] = kind.event.interaction;
      refreshActiveInteraction(draft, selectedSessionId);
      break;
    case "agentChanged": {
      const list = draft.agents[kind.agent.sessionId] ?? [];
      const index = list.findIndex((item) => item.id === kind.agent.id);
      const agent = index >= 0 ? mergeAgentSnapshot(list[index], kind.agent) : kind.agent;
      const next = index >= 0 ? [...list.slice(0, index), agent, ...list.slice(index + 1)] : [...list, agent];
      draft.agents[kind.agent.sessionId] = next;
      break;
    }
    case "agentTimelineChanged": {
      const list = draft.agentEvents[kind.event.sessionId] ?? [];
      draft.agentEvents[kind.event.sessionId] = [...list.filter((item) => item.eventId !== kind.event.eventId), kind.event]
        .sort((a, b) => a.sequence - b.sequence);
      break;
    }
    case "sessionRuntimeChanged":
      draft.sessionRuntime[kind.runtime.sessionId] = kind.runtime;
      break;
    case "planLifecycleChanged":
      draft.planStates[kind.event.planId] = {
        planId: kind.event.planId,
        state: kind.event.state,
        turnId: kind.event.turnId ?? null,
        reason: kind.event.reason ?? null,
        updatedAt: kind.event.updatedAt,
      };
      break;
    case "sessionListChanged":
      draft.sessions = mergeSessions(draft.sessions, kind.sessions);
      break;
    case "sessionHandoffChanged":
      if (kind.handoff.targetSession) {
        draft.sessions = mergeSessions(draft.sessions, [kind.handoff.targetSession]);
      }
      break;
    case "skillActivated":
      applySkillActivation(draft, sessionId, kind.activation);
      break;
    case "mcpHealthChanged":
      applyMcpHealth(draft, kind.health);
      break;
    case "lspHealthChanged":
      applyLspHealth(draft, kind.health);
      break;
    case "stale":
      break;
  }
}

function removeMessage(draft: MessageStore, sessionId: string | undefined | null, messageId: string) {
  if (sessionId) draft.messages[sessionId] = (draft.messages[sessionId] ?? []).filter((message) => message.messageId !== messageId);
  for (const part of draft.parts[messageId] ?? []) {
    delete draft.partTextAccumDelta[part.partId];
    delete draft.partDeltaChunks[part.partId];
    delete draft.partSequence[part.partId];
    delete draft.partDeltaSequence[part.partId];
  }
  delete draft.parts[messageId];
  delete draft.messageSequence[messageId];
}

function upsertPart(draft: MessageStore, part: StudioPart, sequence: number | undefined) {
  const existingSequence = draft.partSequence[part.partId];
  if (sequence !== undefined && existingSequence !== undefined && existingSequence > sequence) return;
  const list = draft.parts[part.messageId] ?? [];
  const index = list.findIndex((item) => item.partId === part.partId);
  draft.parts[part.messageId] = (index >= 0
    ? [...list.slice(0, index), part, ...list.slice(index + 1)]
    : [...list, part]).sort(compareParts);
  delete draft.partTextAccumDelta[part.partId];
  delete draft.partDeltaChunks[part.partId];
  delete draft.partDeltaSequence[part.partId];
  if (sequence !== undefined) draft.partSequence[part.partId] = sequence;
}

function removePart(draft: MessageStore, messageId: string, partId: string) {
  draft.parts[messageId] = (draft.parts[messageId] ?? []).filter((part) => part.partId !== partId);
  delete draft.partTextAccumDelta[partId];
  delete draft.partDeltaChunks[partId];
  delete draft.partSequence[partId];
  delete draft.partDeltaSequence[partId];
}

function applyDelta(draft: MessageStore, delta: StudioPartDelta, sequence: number) {
  const part = (draft.parts[delta.messageId] ?? []).find((candidate) => candidate.partId === delta.partId);
  if (!part) return;
  const snapshotSequence = draft.partSequence[delta.partId];
  if (isTerminalPart(part) && snapshotSequence !== undefined && sequence <= snapshotSequence) return;
  const lastDeltaSequence = draft.partDeltaSequence[delta.partId];
  if (lastDeltaSequence !== undefined && sequence < lastDeltaSequence) return;
  const chunkIndex = delta.chunkIndex ?? undefined;
  const text = delta.delta;
  switch (delta.field) {
    case "text": {
      const next = `${draft.partTextAccumDelta[delta.partId] ?? part.text ?? ""}${text}`;
      draft.partTextAccumDelta[delta.partId] = next;
      break;
    }
    case "reasoningText": {
      if (chunkIndex !== undefined) {
        const chunks = { ...(draft.partDeltaChunks[delta.partId] ?? {}) };
        chunks[chunkIndex] = `${chunks[chunkIndex] ?? ""}${text}`;
        draft.partDeltaChunks[delta.partId] = chunks;
        const next = Object.entries(chunks)
          .sort(([left], [right]) => Number(left) - Number(right))
          .map(([, value]) => value)
          .join("");
        draft.partTextAccumDelta[delta.partId] = next;
        break;
      }
      const next = `${draft.partTextAccumDelta[delta.partId] ?? part.text ?? ""}${text}`;
      draft.partTextAccumDelta[delta.partId] = next;
      break;
    }
    case "planContent": {
      const next = `${draft.partTextAccumDelta[delta.partId] ?? part.plan?.content ?? part.text ?? ""}${text}`;
      draft.partTextAccumDelta[delta.partId] = next;
      break;
    }
    case "tool.arguments":
      if (part.tool) {
        draft.partTextAccumDelta[delta.partId] = `${draft.partTextAccumDelta[delta.partId] ?? part.tool.arguments ?? ""}${text}`;
      }
      break;
    case "tool.result":
      if (part.tool) {
        draft.partTextAccumDelta[delta.partId] = `${draft.partTextAccumDelta[delta.partId] ?? part.tool.result ?? ""}${text}`;
      }
      break;
  }
  draft.partDeltaSequence[delta.partId] = sequence;
}

function isRealUserTextPart(part: StudioPart) {
  return part.partType === "text"
    && part.textChannel === "user"
    && !part.partId.startsWith("optimistic-")
    && !part.messageId.startsWith("optimistic-");
}

function removeOptimisticUserMessage(draft: MessageStore, sessionId: string) {
  const removeIds = new Set<string>();
  for (const message of draft.messages[sessionId] ?? []) {
    if (message.role === "user" && message.messageId.startsWith("optimistic-message-")) {
      removeIds.add(message.messageId);
    }
  }
  for (const [messageId, parts] of Object.entries(draft.parts)) {
    if ((parts ?? []).some((part) => part.sessionId === sessionId && part.partId.startsWith("optimistic-user-"))) {
      removeIds.add(messageId);
    }
  }
  for (const messageId of removeIds) removeMessage(draft, sessionId, messageId);
}

function isTerminalPart(part: StudioPart) {
  return part.status === "completed"
    || part.status === "failed"
    || part.status === "interrupted"
    || part.status === "budgetLimited"
    || part.status === "denied";
}

function applyMcpHealth(draft: MessageStore, health: McpHealthUpdatedPayload | null | undefined) {
  if (!health) return;
  draft.mcpServers = health.mcpServers;
  draft.activeMcpServers = health.activeMcpServers;
}

function applyLspHealth(draft: MessageStore, health: LspHealthUpdatedPayload | null | undefined) {
  if (!health) return;
  draft.lspServers = health.lspServers;
  draft.activeLspServers = health.activeLspServers;
}

function providerSettingsInput(
  store: Pick<MessageStore, "providers" | "roles" | "selectedProviderId">,
  snapshot?: ProviderSettingsSaveSnapshot,
): ProviderSettingsInput {
  const providers = snapshot?.providers ?? store.providers;
  const roles = snapshot?.roles ?? store.roles;
  return {
    defaultProviderId: snapshot?.selectedProviderId ?? store.selectedProviderId ?? providers[0]?.id ?? null,
    providers: providers.map((provider) => ({
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
        baseInstructions: model.baseInstructions,
      })),
    })),
    roles: roles.map((role) => ({
      key: role.key,
      provider: role.provider,
      model: role.model,
      effort: role.effort,
    })),
  };
}

function normalizeSelectedProviderId(providerId: string | null | undefined, providers: ProviderRecord[]) {
  if (providerId && providers.some((provider) => provider.id === providerId)) return providerId;
  return providers[0]?.id ?? null;
}

function pruneProviderUsageState(draft: MessageStore, providers: ProviderRecord[]) {
  const providerIds = new Set(providers.map((provider) => provider.id));
  draft.providerUsages = draft.providerUsages.filter((usage) => providerIds.has(usage.providerId));
  for (const providerId of Object.keys(draft.providerUsageErrors)) {
    if (!providerIds.has(providerId)) delete draft.providerUsageErrors[providerId];
  }
  for (const providerId of Object.keys(draft.providerUsageRefreshing)) {
    if (!providerIds.has(providerId)) delete draft.providerUsageRefreshing[providerId];
  }
}

function shouldRefreshProviderUsages(store: MessageStore) {
  if (store.providerUsagesLoading) return false;
  if (!store.providers.some(providerSupportsUsageQuery)) return false;
  const loadedProviderIds = new Set(store.providerUsages.map((usage) => usage.providerId));
  if (store.providers.some((provider) => providerSupportsUsageQuery(provider) && !loadedProviderIds.has(provider.id))) {
    return true;
  }
  const now = Math.floor(Date.now() / 1000);
  const loadedAt = store.providerUsagesLoadedAt ?? 0;
  return now - loadedAt > 300;
}

function providerSupportsUsageQuery(provider: ProviderRecord) {
  return provider.templateKind === "deepseek" || provider.templateKind === "zhipu-coding-plan";
}

function mergeProviderUsages(existing: ProviderUsageRecord[], incoming: ProviderUsageRecord[]) {
  const byId = new Map(existing.map((usage) => [usage.providerId, usage]));
  for (const usage of incoming) byId.set(usage.providerId, usage);
  return [...byId.values()].sort((left, right) => left.providerId.localeCompare(right.providerId));
}

function applySessionRuntimeHealth(draft: MessageStore, runtime: SessionRuntime | null | undefined) {
  if (!runtime) return;
  draft.activeMcpServers = runtime.activeMcpServers;
  draft.activeLspServers = runtime.activeLspServers;
}

function mergeAgentSnapshot(
  existing: AgentDto | undefined,
  next: AgentDto,
): AgentDto {
  if (!existing) return next;
  return {
    ...existing,
    ...next,
    runtimeUsage: next.runtimeUsage ?? existing.runtimeUsage ?? null,
  };
}

function mergeSessions(existing: SessionRecord[], incoming: SessionRecord[]) {
  const replaceRootProjectIds = new Set(
    incoming
      .filter((session) => !session.parentSessionId)
      .map((session) => session.projectId),
  );
  const byId = new Map<string, SessionRecord>();
  for (const session of existing) {
    if (replaceRootProjectIds.has(session.projectId) && !session.parentSessionId) continue;
    byId.set(session.id, session);
  }
  for (const session of incoming) {
    const previous = byId.get(session.id);
    byId.set(session.id, previous && previous.updatedAt > session.updatedAt ? previous : session);
  }
  return [...byId.values()].sort(compareSessions);
}

function replaceProjectSessions(existing: SessionRecord[], projectId: string | null, incoming: SessionRecord[]) {
  if (!projectId) {
    return [];
  }
  const byId = new Map<string, SessionRecord>();
  for (const session of existing) {
    if (session.projectId !== projectId) byId.set(session.id, session);
  }
  for (const session of incoming) {
    if (session.projectId === projectId) byId.set(session.id, session);
  }
  return [...byId.values()].sort(compareSessions);
}

function applySkillActivation(draft: MessageStore, sessionId: string | null | undefined, activation: SkillActivation) {
  if (!sessionId) return;
  const runtime = draft.sessionRuntime[sessionId];
  if (!runtime || runtime.activeSkills.includes(activation.name)) return;
  draft.sessionRuntime[sessionId] = {
    ...runtime,
    activeSkills: [...runtime.activeSkills, activation.name],
    updatedAt: activation.activatedAt,
  };
}

function applyInteractions(draft: MessageStore, interactions: InteractionRequest[], sessionId: string | null) {
  for (const interaction of interactions) {
    draft.interactions[interaction.interactionId] = interaction;
  }
  refreshActiveInteraction(draft, sessionId);
}

function refreshActiveInteraction(draft: MessageStore, sessionId: string | null) {
  const id = selectActiveInteractionId(draft, sessionId);
  draft.activeInteractionId = id;
  draft.activeInteractionPhase = id ? draft.interactions[id]?.kind ?? null : null;
}

function selectActiveInteractionId(draft: MessageStore, sessionId: string | null) {
  const pending = Object.values(draft.interactions).filter(
    (interaction): interaction is InteractionRequest =>
      Boolean(interaction && interaction.status === "pending" && interaction.scope.sessionId === sessionId),
  );
  const priority = { toolApproval: 0, userInput: 1, planConfirmation: 2 };
  pending.sort((a, b) => priority[a.kind] - priority[b.kind] || a.createdAt - b.createdAt);
  return pending[0]?.interactionId ?? null;
}

function coalesceKey(event: StudioEventEnvelope) {
  const session = event.sessionId ?? "";
  switch (event.kind.type) {
    case "messageUpdated":
      return `message:${session}:${event.kind.message.messageId}`;
    case "messagePartUpdated":
      return `part:${session}:${event.kind.part.messageId}:${event.kind.part.partId}`;
    case "turnChanged":
      return `turn:${session}:${event.kind.turn.turnId}`;
    case "sessionRuntimeChanged":
      return `runtime:${event.kind.runtime.sessionId}`;
    default:
      return undefined;
  }
}

function deltaKey(sessionId: string, messageId: string, partId: string) {
  return `${sessionId}:${messageId}:${partId}`;
}

function compareMessages(left: StudioMessage, right: StudioMessage) {
  if (left.createdAt !== right.createdAt) return left.createdAt - right.createdAt;
  return left.messageId.localeCompare(right.messageId);
}

function compareParts(left: StudioPart, right: StudioPart) {
  if (left.order !== right.order) return left.order - right.order;
  return left.partId.localeCompare(right.partId);
}

function compareSessions(left: SessionRecord, right: SessionRecord) {
  if (left.updatedAt !== right.updatedAt) return right.updatedAt - left.updatedAt;
  return left.id.localeCompare(right.id);
}

function statusForTurn(status: string) {
  switch (status) {
    case "queued":
    case "contextLoading":
    case "waitingForModel":
      return i18n.t("status.waitingForModel");
    case "streaming":
    case "runningTool":
    case "persisting":
      return i18n.t("status.running");
    case "waitingForInteraction":
      return i18n.t("status.userInputRequired");
    case "completed":
      return i18n.t("status.done");
    case "failed":
      return i18n.t("turnPhase.failed");
    case "cancelled":
      return i18n.t("status.interrupted");
    default:
      return status;
  }
}

function isBusyTurnStatus(status: string) {
  return !["completed", "failed", "cancelled"].includes(status);
}

function turnPhaseForStatus(status: string) {
  switch (status) {
    case "queued":
    case "contextLoading":
    case "waitingForModel":
      return "thinking";
    case "streaming":
    case "persisting":
      return "running";
    case "runningTool":
      return "tool";
    case "waitingForInteraction":
      return "approval";
    case "completed":
      return "completed";
    case "failed":
      return "failed";
    case "cancelled":
      return "interrupted";
    default:
      return status;
  }
}

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

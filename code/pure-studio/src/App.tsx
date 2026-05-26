import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ApprovalOverlay } from "./components/ApprovalOverlay";
import { ContextPanel } from "./components/ContextPanel";
import { ConversationPanel } from "./components/ConversationPanel";
import { ProjectRail } from "./components/ProjectRail";
import { SettingsPage } from "./components/SettingsPage";
import { normalizeRolesForProviders } from "./components/RoleSettings";
import {
  approveTool,
  bootstrapStudio,
  createSession,
  denyTool,
  loadConfig,
  openProject,
  runPrompt,
  saveConfig,
  saveProviderSettings,
  selectProject,
  selectSession,
  isTauriRuntime,
} from "./lib/tauri";
import { errorText } from "./lib/utils";
import type {
  AgentEventPayload,
  ChatItem,
  ChatMessage,
  ConfigPayload,
  ProjectSelectionPayload,
  ProjectRecord,
  PromptFailed,
  ProviderRecord,
  ProviderTemplateRecord,
  RoleRecord,
  RunPromptResponse,
  SessionRecord,
  SessionSelectionPayload,
  SubagentActivity,
  SubagentEventPayload,
  SubagentStatus,
  ToolApprovalRequest,
  ToolApprovalResolved,
  TrackedToolCall,
} from "./types";

type SettingsTab = "providers" | "models" | "roles" | "security" | "general";

const subagentStatusKeys: Record<SubagentStatus, string> = {
  queued: "subagent.queued",
  awaitingApproval: "subagent.awaitingApproval",
  running: "subagent.running",
  awaitingToolApproval: "subagent.awaitingTool",
  succeeded: "subagent.succeeded",
  failed: "subagent.failed",
  denied: "subagent.denied",
};

const roleI18nKeys: Record<string, string> = {
  explorer: "roles.explorer",
  planner: "roles.planner",
  executor: "roles.executor",
  reviewer: "roles.reviewer",
};

function normalizeSubagentActivity(event: SubagentEventPayload): SubagentActivity {
  return {
    eventId:
      event.eventId ??
      `${event.id}-${event.updatedAt}-${event.status}-${Math.random().toString(16).slice(2)}`,
    id: event.id,
    parentId: event.parentId ?? null,
    role: event.role,
    task: event.task,
    status: event.status,
    summary: event.summary ?? null,
    depth: event.depth,
    error: event.error ?? null,
    updatedAt: event.updatedAt,
  };
}

function mergeSubagentActivities(
  current: SubagentActivity[],
  events: SubagentEventPayload[],
): SubagentActivity[] {
  const byId = new Map(current.map((activity) => [activity.id, activity]));
  for (const event of events) {
    byId.set(event.id, normalizeSubagentActivity(event));
  }
  return [...byId.values()].sort((left, right) => {
    if (right.updatedAt !== left.updatedAt) {
      return right.updatedAt - left.updatedAt;
    }
    return left.depth - right.depth;
  });
}

export function App() {
  const { t } = useTranslation();
  const [projects, setProjects] = useState<ProjectRecord[]>([]);
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [providers, setProviders] = useState<ProviderRecord[]>([]);
  const [roles, setRoles] = useState<RoleRecord[]>([]);
  const [providerTemplates, setProviderTemplates] = useState<ProviderTemplateRecord[]>([]);
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(null);
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);
  const [manualPath, setManualPath] = useState("");
  const [prompt, setPrompt] = useState("");
  const [status, setStatus] = useState(t("status.starting"));
  const [isBusy, setIsBusy] = useState(false);
  const [streamingText, setStreamingText] = useState("");
  const [thinkingText, setThinkingText] = useState("");
  const [toolStatuses, setToolStatuses] = useState<string[]>([]);
  const [toolCalls, setToolCalls] = useState<Map<string, TrackedToolCall>>(new Map());
  const [subagentActivities, setSubagentActivities] = useState<SubagentActivity[]>([]);
  const [approvals, setApprovals] = useState<ToolApprovalRequest[]>([]);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [activeSettingsTab, setActiveSettingsTab] = useState<SettingsTab>("providers");
  const [providerSearch, setProviderSearch] = useState("");
  const [selectedProviderId, setSelectedProviderId] = useState<string | null>(null);
  const [configToml, setConfigToml] = useState("");
  const [configExists, setConfigExists] = useState(false);

  const selectedProject = projects.find((project) => project.id === selectedProjectId) ?? null;
  const selectedSession = sessions.find((session) => session.id === selectedSessionId) ?? null;

  const chatItems = useMemo((): ChatItem[] => {
    const items: ChatItem[] = [];
    let index = 0;

    for (const msg of messages) {
      if (msg.role === "tool" && msg.metadata?.tool_call_id) {
        items.push({
          kind: "tool_call",
          toolCall: {
            id: msg.metadata.tool_call_id,
            name: msg.metadata.tool_name ?? "tool",
            arguments: msg.metadata.tool_call_arguments ?? "",
            status: "result_ready",
            result: msg.content,
            startedAt: 0,
          },
          key: `tc-${msg.metadata.tool_call_id}`,
        });
      } else {
        items.push({ kind: "message", message: msg, key: `msg-${index}` });
      }
      index++;
    }

    if (thinkingText || streamingText) {
      items.push({
        kind: "message",
        message: {
          role: "assistant",
          content: streamingText || t("status.thinking"),
          reasoningContent: thinkingText || null,
        },
        key: "streaming",
      });
    }

    for (const tc of toolCalls.values()) {
      if (!items.some((item) => item.kind === "tool_call" && item.key === `tc-${tc.id}`)) {
        items.push({ kind: "tool_call", toolCall: tc, key: `tc-${tc.id}` });
      }
    }

    return items;
  }, [messages, toolCalls, streamingText, thinkingText, t]);

  const recentActivities = useMemo(
    () => [
      ...subagentActivities.slice(0, 4).map((activity) => ({
        id: `subagent-${activity.id}`,
        title: `${t(roleI18nKeys[activity.role] ?? `roles.${activity.role}`)} · ${t(subagentStatusKeys[activity.status])}`,
        detail: activity.task,
      })),
      ...toolStatuses.slice(0, 4).map((item) => ({
        id: `tool-${item}`,
        title: item,
        detail: t("subagent.toolCall"),
      })),
    ].slice(0, 5),
    [subagentActivities, toolStatuses, t],
  );

  useEffect(() => {
    bootstrapStudio()
      .then((payload) => {
        setProjects(payload.projects);
        setSelectedProjectId(payload.selectedProjectId ?? null);
        setSessions(payload.sessions);
        setSelectedSessionId(payload.selectedSessionId ?? null);
        setMessages(payload.messages);
        setSubagentActivities(mergeSubagentActivities([], payload.subagentEvents));
        applyConfig(payload.config);
        setStatus(t("status.ready"));
      })
      .catch((error) => setStatus(t("status.bootstrapFailed", { error: errorText(error) })));
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    const unlisteners = [
      listen<AgentEventPayload>("studio-agent-event", ({ payload }) => {
        const event = payload.event;
        if (event === "turnStarted") {
          setStatus(t("status.running"));
          return;
        }
        if (event === "done") {
          setStatus(t("status.done"));
          return;
        }
        if ("textDelta" in event) {
          setStreamingText((current) => current + event.textDelta.content);
          return;
        }
        if ("thinkingDelta" in event) {
          setThinkingText((current) => current + event.thinkingDelta.content);
          return;
        }
        if ("toolCallDelta" in event) {
          setStatus(t("status.toolInput", { name: event.toolCallDelta.name }));
          setToolCalls((current) => {
            const next = new Map(current);
            const existing = next.get(event.toolCallDelta.id);
            if (existing) {
              next.set(event.toolCallDelta.id, {
                ...existing,
                arguments: existing.arguments + event.toolCallDelta.argumentsDelta,
              });
            } else {
              next.set(event.toolCallDelta.id, {
                id: event.toolCallDelta.id,
                name: event.toolCallDelta.name,
                arguments: event.toolCallDelta.argumentsDelta,
                status: "streaming",
                startedAt: Date.now(),
              });
            }
            return next;
          });
          return;
        }
        if ("toolCallComplete" in event) {
          setToolStatuses((current) => [
            t("status.toolCompleted", { name: event.toolCallComplete.name }),
            ...current.slice(0, 4),
          ]);
          setToolCalls((current) => {
            const next = new Map(current);
            const existing = next.get(event.toolCallComplete.id);
            if (existing) {
              next.set(event.toolCallComplete.id, {
                ...existing,
                name: event.toolCallComplete.name || existing.name,
                status: "completed",
                arguments: event.toolCallComplete.arguments,
              });
            } else {
              next.set(event.toolCallComplete.id, {
                id: event.toolCallComplete.id,
                name: event.toolCallComplete.name,
                arguments: event.toolCallComplete.arguments,
                status: "completed",
                startedAt: Date.now(),
              });
            }
            return next;
          });
          return;
        }
        if ("toolApprovalGranted" in event) {
          setStatus(t("status.approved", { name: event.toolApprovalGranted.name }));
          setToolCalls((current) => {
            const next = new Map(current);
            const existing = next.get(event.toolApprovalGranted.id);
            if (existing) {
              next.set(event.toolApprovalGranted.id, { ...existing, status: "approved" });
            }
            return next;
          });
          return;
        }
        if ("toolApprovalDenied" in event) {
          setStatus(t("status.denied", { name: event.toolApprovalDenied.name }));
          setToolCalls((current) => {
            const next = new Map(current);
            const existing = next.get(event.toolApprovalDenied.id);
            if (existing) {
              next.set(event.toolApprovalDenied.id, { ...existing, status: "denied" });
            }
            return next;
          });
          return;
        }
        if ("subagentStateChanged" in event) {
          setSubagentActivities((current) =>
            mergeSubagentActivities(current, [event.subagentStateChanged]),
          );
          setStatus(
            t("status.subagentStatus", { status: t(subagentStatusKeys[event.subagentStateChanged.status]).toLowerCase() }),
          );
          return;
        }
        if ("error" in event) {
          setStatus(t("status.error", { message: event.error.message }));
        }
      }),
      listen<ToolApprovalRequest>("studio-tool-approval-requested", ({ payload }) => {
        setApprovals((current) => [payload, ...current]);
        setStatus(t("status.approvalRequired", { name: payload.name }));
      }),
      listen<ToolApprovalResolved>("studio-tool-approval-resolved", ({ payload }) => {
        setApprovals((current) =>
          current.filter((approval) => approval.approvalId !== payload.approvalId),
        );
        setStatus(payload.decision === "approved" ? t("status.toolApproved") : t("status.toolDenied"));
      }),
      listen<RunPromptResponse>("studio-prompt-finished", ({ payload }) => {
        applyRunPrompt(payload);
        setIsBusy(false);
      }),
      listen<PromptFailed>("studio-prompt-failed", ({ payload }) => {
        setStatus(payload.message);
        setIsBusy(false);
      }),
    ];

    return () => {
      void Promise.all(unlisteners).then((items) => {
        for (const unlisten of items) {
          unlisten();
        }
      });
    };
  }, []);

  function applyConfig(payload: ConfigPayload) {
    setProviders(payload.providers);
    setRoles(payload.roles);
    setProviderTemplates(payload.templates);
    setConfigToml(payload.toml);
    setConfigExists(payload.configExists);
    setSelectedProviderId((current) => {
      if (current && payload.providers.some((provider) => provider.id === current)) {
        return current;
      }
      return payload.providers[0]?.id ?? null;
    });
  }

  function applyProjectSelection(payload: ProjectSelectionPayload) {
    setProjects(payload.projects);
    setSelectedProjectId(payload.projectId);
    setSessions(payload.sessions);
    setSelectedSessionId(payload.selectedSessionId ?? null);
    setMessages(payload.messages);
    setSubagentActivities(mergeSubagentActivities([], payload.subagentEvents));
    setStreamingText("");
    setThinkingText("");
    setToolCalls(new Map());
    setStatus(t("status.projectLoaded"));
  }

  function applySessionSelection(payload: SessionSelectionPayload) {
    if (payload.sessions.length > 0) {
      setSessions(payload.sessions);
    }
    setSelectedSessionId(payload.sessionId);
    setMessages(payload.messages);
    setSubagentActivities(mergeSubagentActivities([], payload.subagentEvents));
    setStreamingText("");
    setThinkingText("");
    setToolCalls(new Map());
    setStatus(t("status.sessionLoaded"));
  }

  function applyRunPrompt(payload: RunPromptResponse) {
    setSelectedSessionId(payload.sessionId);
    setSessions(payload.sessions);
    setMessages(payload.messages);
    setSubagentActivities(mergeSubagentActivities([], payload.subagentEvents));
    setStreamingText("");
    setThinkingText("");
    setStatus(t("status.done"));
  }

  async function addProject(path: string) {
    try {
      const payload = await openProject(path);
      applyProjectSelection(payload);
      setManualPath("");
    } catch (error) {
      setStatus(t("status.addProjectFailed", { error: errorText(error) }));
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
      applyProjectSelection(await selectProject(projectId));
    } catch (error) {
      setStatus(t("status.selectProjectFailed", { error: errorText(error) }));
    }
  }

  async function onNewSession() {
    if (!selectedProjectId) {
      return;
    }
    try {
      applySessionSelection(await createSession(selectedProjectId, t("common.newSessionTitle")));
    } catch (error) {
      setStatus(t("status.newSessionFailed", { error: errorText(error) }));
    }
  }

  async function onSelectSession(sessionId: string) {
    try {
      applySessionSelection(await selectSession(sessionId));
    } catch (error) {
      setStatus(t("status.selectSessionFailed", { error: errorText(error) }));
    }
  }

  async function onSendPrompt() {
    const content = prompt.trim();
    if (!content || !selectedSessionId || isBusy) {
      return;
    }
    setPrompt("");
    setIsBusy(true);
    setStreamingText("");
    setThinkingText("");
    setMessages((current) => [...current, { role: "user", content }]);
    setStatus(t("status.running"));
    try {
      applyRunPrompt(await runPrompt(selectedSessionId, content));
    } catch (error) {
      setStatus(t("status.runFailed", { error: errorText(error) }));
      setIsBusy(false);
    }
  }

  async function openSettings() {
    setSettingsOpen(true);
    setActiveSettingsTab("providers");
    try {
      applyConfig(await loadConfig());
    } catch (error) {
      setStatus(t("status.configLoadFailed", { error: errorText(error) }));
    }
  }

  async function onSaveConfig() {
    try {
      applyConfig(await saveConfig(configToml));
      setStatus(t("status.configSaved"));
    } catch (error) {
      setStatus(t("status.configInvalid", { error: errorText(error) }));
    }
  }

  async function onSaveProviderSettings() {
    try {
      const normalizedRoles = normalizeRolesForProviders(roles, providers);
      applyConfig(
        await saveProviderSettings({
          defaultProviderId: selectedProviderId,
          providers: providers.map((provider) => ({
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
      );
      setStatus(t("status.providerSettingsSaved"));
    } catch (error) {
      setStatus(t("status.providerSettingsInvalid", { error: errorText(error) }));
    }
  }

  async function onReloadConfig() {
    try {
      applyConfig(await loadConfig());
      setStatus(t("status.configReloaded"));
    } catch (error) {
      setStatus(t("status.reloadFailed", { error: errorText(error) }));
    }
  }

  async function onApprove(approvalId: string) {
    await approveTool(approvalId);
  }

  async function onDeny(approvalId: string) {
    await denyTool(approvalId, "denied by user");
  }

  return (
    <main className="app-shell">
      <ProjectRail
        projects={projects}
        sessions={sessions}
        selectedProjectId={selectedProjectId}
        selectedSessionId={selectedSessionId}
        manualPath={manualPath}
        onSetManualPath={setManualPath}
        onAddProject={(path) => void addProject(path)}
        onSelectProject={(id) => void onSelectProject(id)}
        onNewSession={() => void onNewSession()}
        onSelectSession={(id) => void onSelectSession(id)}
        onOpenSettings={() => void openSettings()}
        chooseFolder={() => void chooseFolder()}
      />

      <ConversationPanel
        selectedSession={selectedSession}
        selectedProject={selectedProject}
        status={status}
        isBusy={isBusy}
        chatItems={chatItems}
        subagentActivities={subagentActivities}
        prompt={prompt}
        onSetPrompt={setPrompt}
        onSendPrompt={() => void onSendPrompt()}
      />

      <ContextPanel
        selectedProject={selectedProject}
        sessions={sessions}
        messages={messages}
        providers={providers}
        recentActivities={recentActivities}
      />

      <ApprovalOverlay
        approvals={approvals}
        onApprove={(id) => void onApprove(id)}
        onDeny={(id) => void onDeny(id)}
      />

      {settingsOpen ? (
        <SettingsPage
          activeSettingsTab={activeSettingsTab}
          providers={providers}
          providerTemplates={providerTemplates}
          roles={roles}
          selectedProviderId={selectedProviderId}
          providerSearch={providerSearch}
          configExists={configExists}
          configToml={configToml}
          setProviders={setProviders}
          setRoles={setRoles}
          setSelectedProviderId={setSelectedProviderId}
          setProviderSearch={setProviderSearch}
          setConfigToml={setConfigToml}
          onClose={() => setSettingsOpen(false)}
          onSetActiveTab={setActiveSettingsTab}
          onSaveProviderSettings={() => void onSaveProviderSettings()}
          onSaveConfig={() => void onSaveConfig()}
          onReloadConfig={() => void onReloadConfig()}
        />
      ) : null}
    </main>
  );
}

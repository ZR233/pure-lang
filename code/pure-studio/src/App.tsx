import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  Activity,
  ArrowLeft,
  Check,
  FolderOpen,
  MessageSquare,
  Plus,
  RefreshCw,
  Save,
  Send,
  Settings,
  ShieldAlert,
  Terminal,
  X,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { ProviderSettings } from "./components/ProviderSettings";
import { RoleSettings, normalizeRolesForProviders } from "./components/RoleSettings";
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
import {
  AgentEventPayload,
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
} from "./types";

type SettingsTab = "providers" | "models" | "roles" | "security" | "general";

const roleLabels: Record<string, string> = {
  explorer: "探索者",
  planner: "计划者",
  executor: "执行者",
  reviewer: "审查者",
};

const statusLabels: Record<SubagentStatus, string> = {
  queued: "Queued",
  awaitingApproval: "Awaiting approval",
  running: "Running",
  awaitingToolApproval: "Awaiting tool",
  succeeded: "Succeeded",
  failed: "Failed",
  denied: "Denied",
};

const statusClassNames: Record<SubagentStatus, string> = {
  queued: "queued",
  awaitingApproval: "awaiting-approval",
  running: "running",
  awaitingToolApproval: "awaiting-tool-approval",
  succeeded: "succeeded",
  failed: "failed",
  denied: "denied",
};

function errorText(error: unknown) {
  if (typeof error === "object" && error !== null && "message" in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}

function initials(name: string) {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
}

function formatTime(value: number) {
  if (!value) {
    return "";
  }
  return new Date(value * 1000).toLocaleString();
}

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

function subagentSummary(activity: SubagentActivity) {
  if (activity.error) {
    return activity.error;
  }
  if (activity.summary) {
    return activity.summary;
  }
  return activity.status === "queued" ? "Waiting to start." : "No summary yet.";
}

export function App() {
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
  const [status, setStatus] = useState("Starting");
  const [isBusy, setIsBusy] = useState(false);
  const [streamingText, setStreamingText] = useState("");
  const [thinkingText, setThinkingText] = useState("");
  const [toolStatuses, setToolStatuses] = useState<string[]>([]);
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

  const liveMessages = useMemo(() => {
    const rows = [...messages];
    if (thinkingText || streamingText) {
      rows.push({
        role: "assistant",
        content: streamingText || "Thinking...",
        reasoningContent: thinkingText || null,
      });
    }
    return rows;
  }, [messages, streamingText, thinkingText]);

  const recentActivities = useMemo(
    () => [
      ...subagentActivities.slice(0, 4).map((activity) => ({
        id: `subagent-${activity.id}`,
        title: `${roleLabels[activity.role] ?? activity.role} · ${statusLabels[activity.status]}`,
        detail: activity.task,
      })),
      ...toolStatuses.slice(0, 4).map((item) => ({
        id: `tool-${item}`,
        title: item,
        detail: "Tool call",
      })),
    ].slice(0, 5),
    [subagentActivities, toolStatuses],
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
        setStatus("Ready");
      })
      .catch((error) => setStatus(`Bootstrap failed: ${errorText(error)}`));
  }, []);

  useEffect(() => {
    if (!isTauriRuntime()) {
      return;
    }

    const unlisteners = [
      listen<AgentEventPayload>("studio-agent-event", ({ payload }) => {
        const event = payload.event;
        if (event === "turnStarted") {
          setStatus("Running");
          return;
        }
        if (event === "done") {
          setStatus("Done");
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
          setStatus(`Tool input: ${event.toolCallDelta.name}`);
          return;
        }
        if ("toolCallComplete" in event) {
          setToolStatuses((current) => [
            `${event.toolCallComplete.name} completed`,
            ...current.slice(0, 4),
          ]);
          return;
        }
        if ("toolApprovalGranted" in event) {
          setStatus(`Approved: ${event.toolApprovalGranted.name}`);
          return;
        }
        if ("toolApprovalDenied" in event) {
          setStatus(`Denied: ${event.toolApprovalDenied.name}`);
          return;
        }
        if ("subagentStateChanged" in event) {
          setSubagentActivities((current) =>
            mergeSubagentActivities(current, [event.subagentStateChanged]),
          );
          setStatus(
            `Subagent ${statusLabels[event.subagentStateChanged.status].toLowerCase()}`,
          );
          return;
        }
        if ("error" in event) {
          setStatus(`Error: ${event.error.message}`);
        }
      }),
      listen<ToolApprovalRequest>("studio-tool-approval-requested", ({ payload }) => {
        setApprovals((current) => [payload, ...current]);
        setStatus(`Approval required: ${payload.name}`);
      }),
      listen<ToolApprovalResolved>("studio-tool-approval-resolved", ({ payload }) => {
        setApprovals((current) =>
          current.filter((approval) => approval.approvalId !== payload.approvalId),
        );
        setStatus(payload.decision === "approved" ? "Tool approved" : "Tool denied");
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
    setStatus("Project loaded");
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
    setStatus("Session loaded");
  }

  function applyRunPrompt(payload: RunPromptResponse) {
    setSelectedSessionId(payload.sessionId);
    setSessions(payload.sessions);
    setMessages(payload.messages);
    setSubagentActivities(mergeSubagentActivities([], payload.subagentEvents));
    setStreamingText("");
    setThinkingText("");
    setStatus("Done");
  }

  async function addProject(path: string) {
    try {
      const payload = await openProject(path);
      applyProjectSelection(payload);
      setManualPath("");
    } catch (error) {
      setStatus(`Add project failed: ${errorText(error)}`);
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
      setStatus(`Select project failed: ${errorText(error)}`);
    }
  }

  async function onNewSession() {
    if (!selectedProjectId) {
      return;
    }
    try {
      applySessionSelection(await createSession(selectedProjectId, "New session"));
    } catch (error) {
      setStatus(`New session failed: ${errorText(error)}`);
    }
  }

  async function onSelectSession(sessionId: string) {
    try {
      applySessionSelection(await selectSession(sessionId));
    } catch (error) {
      setStatus(`Select session failed: ${errorText(error)}`);
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
    setStatus("Running");
    try {
      applyRunPrompt(await runPrompt(selectedSessionId, content));
    } catch (error) {
      setStatus(`Run failed: ${errorText(error)}`);
      setIsBusy(false);
    }
  }

  async function openSettings() {
    setSettingsOpen(true);
    setActiveSettingsTab("providers");
    try {
      applyConfig(await loadConfig());
    } catch (error) {
      setStatus(`Config load failed: ${errorText(error)}`);
    }
  }

  async function onSaveConfig() {
    try {
      applyConfig(await saveConfig(configToml));
      setStatus("Config saved");
    } catch (error) {
      setStatus(`Config invalid: ${errorText(error)}`);
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
      setStatus("Provider settings saved");
    } catch (error) {
      setStatus(`Provider settings invalid: ${errorText(error)}`);
    }
  }

  async function onReloadConfig() {
    try {
      applyConfig(await loadConfig());
      setStatus("Config reloaded");
    } catch (error) {
      setStatus(`Reload failed: ${errorText(error)}`);
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
      <aside className="project-rail">
        <div className="brand">
          <div className="brand-mark">P</div>
          <div>
            <div className="brand-title">Pure Studio</div>
            <div className="brand-subtitle">Natural language compiler</div>
          </div>
        </div>

        <button className="settings-entry" onClick={() => void openSettings()}>
          <Settings size={17} />
          <span>Settings</span>
        </button>

        <section className="rail-section">
          <div className="section-heading">
            <span>Projects</span>
            <button className="icon-button" onClick={chooseFolder} title="Choose folder">
              <FolderOpen size={16} />
            </button>
          </div>
          <div className="path-add">
            <input
              value={manualPath}
              onChange={(event) => setManualPath(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  void addProject(manualPath);
                }
              }}
              placeholder="Project path"
            />
            <button className="icon-button" onClick={() => void addProject(manualPath)}>
              <Plus size={16} />
            </button>
          </div>
          <div className="project-list">
            {projects.map((project) => (
              <button
                key={project.id}
                className={`project-row ${project.id === selectedProjectId ? "active" : ""}`}
                onClick={() => void onSelectProject(project.id)}
              >
                <span className="project-avatar">{initials(project.name) || "P"}</span>
                <span>
                  <strong>{project.name}</strong>
                  <small>{project.path}</small>
                </span>
              </button>
            ))}
          </div>
        </section>

        <section className="rail-section sessions-section">
          <div className="section-heading">
            <span>Sessions</span>
            <button
              className="icon-button"
              disabled={!selectedProjectId}
              onClick={() => void onNewSession()}
              title="New session"
            >
              <Plus size={16} />
            </button>
          </div>
          <div className="session-list">
            {sessions.map((session) => (
              <button
                key={session.id}
                className={`session-row ${session.id === selectedSessionId ? "active" : ""}`}
                onClick={() => void onSelectSession(session.id)}
              >
                <MessageSquare size={16} />
                <span>
                  <strong>{session.title}</strong>
                  <small>{formatTime(session.updatedAt)}</small>
                </span>
              </button>
            ))}
          </div>
        </section>

      </aside>

      <section className="conversation">
        <header className="conversation-header">
          <div>
            <h1>{selectedSession?.title ?? "Conversation"}</h1>
            <p>{selectedProject?.path ?? "Add or select a project to begin"}</p>
          </div>
          <div className={`status-pill ${isBusy ? "running" : ""}`}>{status}</div>
        </header>

        <div className="message-stream">
          {liveMessages.length === 0 && subagentActivities.length === 0 ? (
            <div className="empty-state">
              <Terminal size={34} />
              <h2>Ready when you are</h2>
              <p>Select a project and ask Pure Studio to explore, plan, or execute.</p>
            </div>
          ) : (
            <>
              {liveMessages.map((message, index) => (
                <article key={`${message.role}-${index}`} className={`message ${message.role}`}>
                  <div className="message-role">{message.role}</div>
                  {message.reasoningContent ? (
                    <pre className="thinking-block">{message.reasoningContent}</pre>
                  ) : null}
                  <div className="message-content">{message.content}</div>
                </article>
              ))}
              {subagentActivities.length > 0 ? (
                <section className="subagent-timeline" aria-label="Subagent activity">
                  <div className="subagent-timeline-head">
                    <Activity size={16} />
                    <span>Subagents</span>
                  </div>
                  {subagentActivities.map((activity) => (
                    <article
                      key={activity.id}
                      className={`subagent-card status-${statusClassNames[activity.status]}`}
                      style={{ marginLeft: `${Math.max(0, activity.depth - 1) * 14}px` }}
                    >
                      <div className="subagent-card-head">
                        <span className="subagent-role">
                          {roleLabels[activity.role] ?? activity.role}
                        </span>
                        <span className="subagent-status">
                          {statusLabels[activity.status]}
                        </span>
                      </div>
                      <p className="subagent-task">{activity.task}</p>
                      <p className="subagent-result">{subagentSummary(activity)}</p>
                      <div className="subagent-meta">
                        <span>Depth {activity.depth}</span>
                        {activity.parentId ? <span>Parent {activity.parentId}</span> : null}
                        <span>{formatTime(activity.updatedAt)}</span>
                      </div>
                    </article>
                  ))}
                </section>
              ) : null}
            </>
          )}
        </div>

        <footer className="composer">
          <textarea
            value={prompt}
            disabled={!selectedSessionId || isBusy}
            onChange={(event) => setPrompt(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                void onSendPrompt();
              }
            }}
            placeholder={selectedSessionId ? "Ask Pure Studio..." : "Create or select a session"}
          />
          <button
            className="send-button"
            disabled={!prompt.trim() || !selectedSessionId || isBusy}
            onClick={() => void onSendPrompt()}
          >
            <Send size={18} />
            <span>{isBusy ? "Running" : "Send"}</span>
          </button>
        </footer>
      </section>

      <aside className="context-panel">
        <section className="context-card">
          <h2>Project</h2>
          <p className="context-title">{selectedProject?.name ?? "No project"}</p>
          <p className="muted">{selectedProject?.path ?? "Choose a folder to load context."}</p>
        </section>
        <section className="context-card">
          <h2>Runtime</h2>
          <div className="metric-row">
            <span>Sessions</span>
            <strong>{sessions.length}</strong>
          </div>
          <div className="metric-row">
            <span>Messages</span>
            <strong>{messages.length}</strong>
          </div>
          <div className="metric-row">
            <span>Providers</span>
            <strong>{providers.length}</strong>
          </div>
        </section>
        <section className="context-card">
          <h2>Tools</h2>
          {recentActivities.length === 0 ? (
            <p className="muted">Subagent and tool activity appears here after a run.</p>
          ) : (
            recentActivities.map((item) => (
              <div className="activity-row" key={item.id}>
                <strong>{item.title}</strong>
                <span>{item.detail}</span>
              </div>
            ))
          )}
        </section>
      </aside>

      {approvals.length > 0 ? (
        <div className="approval-stack">
          {approvals.map((approval) => (
            <section className="approval-card" key={approval.approvalId}>
              <div className="approval-heading">
                <ShieldAlert size={19} />
                <div>
                  <strong>{approval.name}</strong>
                  <span>{approval.workingDirectory ?? "(default working directory)"}</span>
                  {approval.parentSubagentId ? (
                    <span>Subagent {approval.parentSubagentId}</span>
                  ) : null}
                </div>
              </div>
              <pre>{JSON.stringify(approval.arguments, null, 2)}</pre>
              <div className="approval-actions">
                <button onClick={() => void onDeny(approval.approvalId)}>
                  <X size={16} />
                  Deny
                </button>
                <button className="primary" onClick={() => void onApprove(approval.approvalId)}>
                  <Check size={16} />
                  Approve
                </button>
              </div>
            </section>
          ))}
        </div>
      ) : null}

      {settingsOpen ? (
        <section className="settings-page">
          <header className="settings-header">
            <button className="back-button" onClick={() => setSettingsOpen(false)}>
              <ArrowLeft size={18} />
            </button>
            <div>
              <h1>Settings</h1>
              <p>{configExists ? "~/.pure/config.toml" : "Default config draft"}</p>
            </div>
            <div className="settings-actions">
              <button onClick={() => void onReloadConfig()}>
                <RefreshCw size={16} />
                Reload
              </button>
              <button
                className="primary"
                onClick={() =>
                  activeSettingsTab === "providers" || activeSettingsTab === "roles"
                    ? void onSaveProviderSettings()
                    : void onSaveConfig()
                }
              >
                <Save size={16} />
                Save
              </button>
            </div>
          </header>

          <nav className="settings-tabs">
            {(["providers", "models", "roles", "security", "general"] as SettingsTab[]).map(
              (tab) => (
                <button
                  key={tab}
                  className={tab === activeSettingsTab ? "active" : ""}
                  onClick={() => setActiveSettingsTab(tab)}
                >
                  {tab[0].toUpperCase() + tab.slice(1)}
                </button>
              ),
            )}
          </nav>

          {activeSettingsTab === "providers" ? (
            <ProviderSettings
              providers={providers}
              templates={providerTemplates}
              selectedProviderId={selectedProviderId}
              providerSearch={providerSearch}
              setProviders={setProviders}
              setSelectedProviderId={setSelectedProviderId}
              setProviderSearch={setProviderSearch}
            />
          ) : activeSettingsTab === "roles" ? (
            <RoleSettings providers={providers} roles={roles} setRoles={setRoles} />
          ) : (
            <div className="settings-placeholder">
              <h2>Coming soon</h2>
              <p>This settings tab will be wired in a later iteration.</p>
            </div>
          )}
        </section>
      ) : null}
    </main>
  );
}

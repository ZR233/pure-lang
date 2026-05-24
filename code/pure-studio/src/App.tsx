import { listen } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  ArrowLeft,
  Check,
  FolderOpen,
  MessageSquare,
  Plus,
  RefreshCw,
  Save,
  Search,
  Send,
  Settings,
  ShieldAlert,
  Terminal,
  X,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import {
  approveTool,
  bootstrapStudio,
  createSession,
  denyTool,
  loadConfig,
  openProject,
  runPrompt,
  saveConfig,
  selectProject,
  selectSession,
} from "./lib/tauri";
import {
  AgentEventPayload,
  ChatMessage,
  ConfigPayload,
  ProjectSelectionPayload,
  ProjectRecord,
  PromptFailed,
  ProviderRecord,
  RunPromptResponse,
  SessionRecord,
  SessionSelectionPayload,
  ToolApprovalRequest,
  ToolApprovalResolved,
} from "./types";

type SettingsTab = "providers" | "models" | "roles" | "security" | "general";

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

export function App() {
  const [projects, setProjects] = useState<ProjectRecord[]>([]);
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [providers, setProviders] = useState<ProviderRecord[]>([]);
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(null);
  const [selectedSessionId, setSelectedSessionId] = useState<string | null>(null);
  const [manualPath, setManualPath] = useState("");
  const [prompt, setPrompt] = useState("");
  const [status, setStatus] = useState("Starting");
  const [isBusy, setIsBusy] = useState(false);
  const [streamingText, setStreamingText] = useState("");
  const [thinkingText, setThinkingText] = useState("");
  const [toolStatuses, setToolStatuses] = useState<string[]>([]);
  const [approvals, setApprovals] = useState<ToolApprovalRequest[]>([]);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [activeSettingsTab, setActiveSettingsTab] = useState<SettingsTab>("providers");
  const [providerSearch, setProviderSearch] = useState("");
  const [selectedProviderId, setSelectedProviderId] = useState<string | null>(null);
  const [configToml, setConfigToml] = useState("");
  const [configExists, setConfigExists] = useState(false);

  const selectedProject = projects.find((project) => project.id === selectedProjectId) ?? null;
  const selectedSession = sessions.find((session) => session.id === selectedSessionId) ?? null;
  const filteredProviders = providers.filter((provider) => {
    const query = providerSearch.trim().toLowerCase();
    if (!query) {
      return true;
    }
    return (
      provider.name.toLowerCase().includes(query) ||
      provider.id.toLowerCase().includes(query) ||
      provider.baseUrl.toLowerCase().includes(query)
    );
  });
  const selectedProvider =
    providers.find((provider) => provider.id === selectedProviderId) ?? providers[0] ?? null;

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

  useEffect(() => {
    bootstrapStudio()
      .then((payload) => {
        setProjects(payload.projects);
        setSelectedProjectId(payload.selectedProjectId ?? null);
        setSessions(payload.sessions);
        setSelectedSessionId(payload.selectedSessionId ?? null);
        setMessages(payload.messages);
        applyConfig(payload.config);
        setStatus("Ready");
      })
      .catch((error) => setStatus(`Bootstrap failed: ${errorText(error)}`));
  }, []);

  useEffect(() => {
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
    setStreamingText("");
    setThinkingText("");
    setStatus("Session loaded");
  }

  function applyRunPrompt(payload: RunPromptResponse) {
    setSelectedSessionId(payload.sessionId);
    setSessions(payload.sessions);
    setMessages(payload.messages);
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
          {liveMessages.length === 0 ? (
            <div className="empty-state">
              <Terminal size={34} />
              <h2>Ready when you are</h2>
              <p>Select a project and ask Pure Studio to explore, plan, or execute.</p>
            </div>
          ) : (
            liveMessages.map((message, index) => (
              <article key={`${message.role}-${index}`} className={`message ${message.role}`}>
                <div className="message-role">{message.role}</div>
                {message.reasoningContent ? (
                  <pre className="thinking-block">{message.reasoningContent}</pre>
                ) : null}
                <div className="message-content">{message.content}</div>
              </article>
            ))
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
          {toolStatuses.length === 0 ? (
            <p className="muted">Tool activity appears here after approval.</p>
          ) : (
            toolStatuses.map((item) => <p key={item}>{item}</p>)
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
              <button className="primary" onClick={() => void onSaveConfig()}>
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
            <div className="provider-settings">
              <section className="provider-list-card">
                <div className="provider-toolbar">
                  <div className="search-box">
                    <Search size={16} />
                    <input
                      value={providerSearch}
                      onChange={(event) => setProviderSearch(event.target.value)}
                      placeholder="Search providers..."
                    />
                  </div>
                  <button onClick={() => setStatus("Add Provider is a placeholder")}>
                    <Plus size={16} />
                    Add Provider
                  </button>
                </div>
                <div className="provider-table">
                  <div className="provider-row header">
                    <span className="provider-name-header">Provider</span>
                    <span>Status</span>
                    <span>Base URL</span>
                    <span>Models</span>
                    <span>Updated</span>
                  </div>
                  {filteredProviders.map((provider) => (
                    <button
                      key={provider.id}
                      className={`provider-row ${provider.id === selectedProvider?.id ? "active" : ""}`}
                      onClick={() => setSelectedProviderId(provider.id)}
                    >
                      <span className="provider-name-cell">
                        <span className="provider-badge">{initials(provider.name)}</span>
                        <span>
                          <strong>{provider.name}</strong>
                          <small>{provider.subtitle}</small>
                        </span>
                      </span>
                      <span className="health">{provider.status}</span>
                      <span>{provider.baseUrl}</span>
                      <span>{provider.modelCount}</span>
                      <span>{provider.updatedAt}</span>
                    </button>
                  ))}
                </div>
              </section>

              <section className="provider-detail-card">
                {selectedProvider ? (
                  <>
                    <div className="provider-detail-head">
                      <span className="provider-badge large">{initials(selectedProvider.name)}</span>
                      <div>
                        <h2>{selectedProvider.name}</h2>
                        <p>{selectedProvider.subtitle}</p>
                      </div>
                      <span className="health">{selectedProvider.status}</span>
                    </div>
                    <dl>
                      <dt>Provider ID</dt>
                      <dd>{selectedProvider.id}</dd>
                      <dt>Protocol</dt>
                      <dd>{selectedProvider.wireApi}</dd>
                      <dt>Base URL</dt>
                      <dd>{selectedProvider.baseUrl}</dd>
                      <dt>Models</dt>
                      <dd>{selectedProvider.modelCount}</dd>
                    </dl>
                  </>
                ) : (
                  <p className="muted">No provider configured.</p>
                )}
              </section>

              <section className="config-editor-card">
                <div>
                  <h2>Config TOML</h2>
                  <p>Validate and save writes to ~/.pure/config.toml.</p>
                </div>
                <textarea value={configToml} onChange={(event) => setConfigToml(event.target.value)} />
              </section>
            </div>
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

import { Archive, FolderOpen, FolderPlus, Pause, Play, Plus, Settings, Sparkles, X } from "lucide-solid";
import { For, Show, createEffect, createMemo, createSignal, onMount } from "solid-js";
import type { InteractionRequest, InteractionResolution, SessionRecord, StudioMessage, StudioPart } from "./types";
import { createStudioStore } from "./solid/studio-store";
import { selectedSessionView, visibleProjectSessions } from "./solid/studio-selectors";
import { MessageTimeline } from "./solid/timeline/message-timeline";
import { SessionStatusBar } from "./solid/status/session-status-bar";
import { InteractionComposer } from "./solid/interaction/interaction-composer";
import type { InteractionComposerState } from "./solid/interaction/interaction-resolution";
import { SettingsPanel } from "./solid/settings/settings-panel";
import i18n from "./i18n";

export function App() {
  const studio = createStudioStore();
  const state = studio.store;
  const [resolvingInteractionId, setResolvingInteractionId] = createSignal<string | null>(null);
  const [interactionError, setInteractionError] = createSignal<string | null>(null);

  onMount(studio.init);

  const view = createMemo(() => selectedSessionView(state));
  const sessions = createMemo(() => visibleProjectSessions(state));
  const activeInteraction = createMemo(() => view().activeInteraction);

  createEffect(() => {
    activeInteraction()?.interactionId;
    setInteractionError(null);
    setResolvingInteractionId(null);
  });

  async function resolveActiveInteraction(interactionId: string, resolution: InteractionResolution) {
    setInteractionError(null);
    setResolvingInteractionId(interactionId);
    try {
      await studio.actions.resolveInteraction(interactionId, resolution);
    } catch (error) {
      setInteractionError(error instanceof Error ? error.message : String(error));
    } finally {
      setResolvingInteractionId((current) => current === interactionId ? null : current);
    }
  }

  function getMessageParts(messageId: string) {
    return state.parts[messageId] ?? [];
  }

  function getPart(messageId: string, partId: string) {
    return (state.parts[messageId] ?? []).find((part) => part.partId === partId);
  }

  return (
    <main class="studio-shell">
      <ProjectSidebar
        projects={state.projects}
        sessions={sessions()}
        selectedProjectId={state.selectedProjectId}
        selectedSessionId={state.selectedSessionId}
        busy={view().busy}
        onAddProject={(path) => void studio.actions.addProject(path)}
        onChooseProjectDirectory={() => void studio.actions.chooseProjectDirectory()}
        onSelectProject={(id) => void studio.actions.selectProject(id)}
        onArchiveProject={(id) => void studio.actions.archiveProject(id)}
        onNewSession={() => void studio.actions.createSession()}
        onSelectSession={(id) => void studio.actions.selectSession(id)}
        onDeleteSession={(id) => void studio.actions.deleteSession(id)}
        onOpenSettings={() => studio.actions.openSettings()}
      />
      <section class="conversation-shell">
        <header class="conversation-header">
          <div>
            <h1>{view().session?.title ?? "Pure Studio"}</h1>
            <p>{state.status}</p>
          </div>
          <div class="header-actions">
            <button type="button" class="icon-button" onClick={() => studio.actions.openSettings()} aria-label="Settings">
              <Settings size={16} />
            </button>
          </div>
        </header>
        <MessageTimeline
          sessionId={view().sessionId}
          messages={view().messages}
          getMessageParts={getMessageParts}
          getPart={getPart}
          getPartDelta={(partId) => state.partTextAccumDelta[partId]}
          busy={view().busy}
          empty="Start a conversation"
        />
        <SessionStatusBar
          runtime={view().runtime}
          providers={state.providers}
          roles={state.roles}
          permissionMode={state.permissionMode}
          currentMode={(view().session?.mode === "plan" ? "plan" : "auto")}
          selectedSession={view().session}
          busy={view().busy}
          turnPhase={view().turnPhase}
          activeInteraction={activeInteraction()}
          turnStartedAt={view().turnStartedAt}
          mcpServers={state.mcpServers}
          activeMcpServers={view().activeMcpServers}
          lspServers={state.lspServers}
          activeLspServers={view().activeLspServers}
          agents={view().agents}
          onSetSessionMode={(mode) => void studio.actions.setSessionMode(mode)}
          onSavePermissionMode={(mode) => void studio.actions.savePermissionMode(mode)}
          onSaveRoles={(roles) => {
            void studio.actions.saveProviderSettings({
              roles,
            });
          }}
        />
        <Footer
          prompt={state.prompt}
          busy={view().busy}
          activeInteraction={activeInteraction()}
          resolvingInteractionId={resolvingInteractionId()}
          interactionError={interactionError()}
          onSetPrompt={studio.actions.setPrompt}
          onSubmit={() => void studio.actions.submitPrompt()}
          onStop={() => void studio.actions.stop()}
          onResolve={(interaction, resolution) => void resolveActiveInteraction(interaction.interactionId, resolution)}
        />
      </section>
      <Show when={state.settingsOpen}>
        <SettingsPanel
          activeTab={state.activeSettingsTab}
          providers={state.providers}
          templates={state.providerTemplates}
          providerUsages={state.providerUsages}
          providerUsagesLoading={state.providerUsagesLoading}
          providerUsageError={state.providerUsageError}
          providerUsageErrors={state.providerUsageErrors}
          providerUsageRefreshing={state.providerUsageRefreshing}
          providerUsagesLoadedAt={state.providerUsagesLoadedAt}
          providerSearch={state.providerSearch}
          selectedProviderId={state.selectedProviderId}
          roles={state.roles}
          instructions={state.instructions}
          mcpServers={state.mcpServers}
          selectedProjectId={state.selectedProjectId}
          configExists={state.configExists}
          configToml={state.configToml}
          permissionMode={state.permissionMode}
          onClose={() => studio.actions.setSettingsOpen(false)}
          onSetTab={studio.actions.setSettingsTab}
          onSetProviderSearch={studio.actions.setProviderSearch}
          onSetSelectedProviderId={studio.actions.setSelectedProviderId}
          onRefreshProviderUsages={(providerId) => void studio.actions.refreshProviderUsages(providerId)}
          onSaveProviderSettings={studio.actions.saveProviderSettings}
          onSaveInstructionsSettings={studio.actions.saveInstructionsSettings}
          onSaveMcpSettings={studio.actions.saveMcpSettings}
          onSavePermissionMode={studio.actions.savePermissionMode}
        />
      </Show>
    </main>
  );
}

function ProjectSidebar(props: {
  projects: { id: string; name: string; path: string }[];
  sessions: SessionRecord[];
  selectedProjectId: string | null;
  selectedSessionId: string | null;
  busy: boolean;
  onAddProject: (path: string) => void;
  onChooseProjectDirectory: () => void;
  onSelectProject: (id: string) => void;
  onArchiveProject: (id: string) => void;
  onNewSession: () => void;
  onSelectSession: (id: string) => void;
  onDeleteSession: (id: string) => void;
  onOpenSettings: () => void;
}) {
  let pathInput: HTMLInputElement | undefined;
  const openTypedProject = () => {
    const value = pathInput?.value.trim();
    if (!value) return;
    props.onAddProject(value);
    if (pathInput) pathInput.value = "";
  };
  const confirmArchiveProject = (project: { id: string; name: string }) => {
    if (!window.confirm(i18n.t("projects.confirmArchive", { name: project.name }))) return;
    props.onArchiveProject(project.id);
  };
  const confirmDeleteSession = (session: SessionRecord) => {
    if (!window.confirm(i18n.t("sessions.confirmDelete", { title: session.title }))) return;
    props.onDeleteSession(session.id);
  };
  return (
    <aside class="studio-sidebar">
      <div class="brand">
        <Sparkles size={16} />
        <span>Pure</span>
      </div>
      <div class="project-add">
        <input
          ref={pathInput}
          placeholder={i18n.t("projects.addPlaceholder")}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              openTypedProject();
            }
          }}
        />
        <button type="button" onClick={props.onChooseProjectDirectory} title={i18n.t("common.chooseFolder")}>
          <FolderOpen size={14} />
        </button>
        <button type="button" onClick={openTypedProject} title={i18n.t("projects.openTypedPath")}>
          <FolderPlus size={14} />
        </button>
      </div>
      <div class="sidebar-section">
        <div class="section-title">{i18n.t("nav.projects")}</div>
        <Show when={props.projects.length > 0} fallback={<div class="sidebar-empty">{i18n.t("projects.empty")}</div>}>
          <For each={props.projects}>
            {(project) => (
              <div class="sidebar-row" data-active={project.id === props.selectedProjectId || undefined}>
                <button
                  type="button"
                  class="sidebar-item row-main"
                  onClick={() => props.onSelectProject(project.id)}
                  title={project.path}
                >
                  <span>{project.name}</span>
                </button>
                <button
                  type="button"
                  class="sidebar-row-action"
                  disabled={props.busy && project.id === props.selectedProjectId}
                  onClick={() => confirmArchiveProject(project)}
                  title={i18n.t("projects.archive")}
                >
                  <Archive size={13} />
                </button>
              </div>
            )}
          </For>
        </Show>
      </div>
      <div class="sidebar-section grow">
        <div class="section-title with-action">
          <span>{i18n.t("nav.sessions")}</span>
          <button type="button" onClick={props.onNewSession} disabled={props.busy || !props.selectedProjectId} title={i18n.t("common.newSession")}><Plus size={14} /></button>
        </div>
        <div class="session-list">
          <Show when={props.sessions.length > 0} fallback={<div class="sidebar-empty">{props.selectedProjectId ? i18n.t("sessions.empty") : i18n.t("projects.openHint")}</div>}>
            <For each={props.sessions}>
              {(session) => (
                <div class="sidebar-row session" data-active={session.id === props.selectedSessionId || undefined}>
                  <button
                    type="button"
                    class="sidebar-item session row-main"
                    onClick={() => props.onSelectSession(session.id)}
                  >
                    <span>{session.title}</span>
                    <small>{session.mode}</small>
                  </button>
                  <button
                    type="button"
                    class="sidebar-row-action"
                    disabled={props.busy && session.id === props.selectedSessionId}
                    onClick={() => confirmDeleteSession(session)}
                    title={i18n.t("sessions.close")}
                  >
                    <X size={13} />
                  </button>
                </div>
              )}
            </For>
          </Show>
        </div>
      </div>
      <button type="button" class="sidebar-settings" onClick={props.onOpenSettings}>
        <Settings size={14} />
        <span>{i18n.t("nav.settings")}</span>
      </button>
    </aside>
  );
}

function Footer(props: {
  prompt: string;
  busy: boolean;
  activeInteraction: InteractionRequest | null;
  resolvingInteractionId: string | null;
  interactionError: string | null;
  onSetPrompt: (value: string) => void;
  onSubmit: () => void;
  onStop: () => void;
  onResolve: (interaction: InteractionRequest, resolution: InteractionResolution) => void;
}) {
  let textArea: HTMLTextAreaElement | undefined;
  const interactionState = (interaction: InteractionRequest): InteractionComposerState =>
    props.resolvingInteractionId === interaction.interactionId
      ? "responding"
      : props.interactionError
        ? "error"
        : "pending";

  return (
    <footer class="conversation-footer">
      <Show when={props.activeInteraction} fallback={
        <div class="composer">
          <textarea
            ref={textArea}
            value={props.prompt}
            placeholder="Message Pure"
            onInput={(event) => props.onSetPrompt(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                props.onSubmit();
              }
            }}
          />
          <button type="button" class="send-button" onClick={props.busy ? props.onStop : props.onSubmit}>
            <Show when={props.busy} fallback={<Play size={15} />}>
              <Pause size={15} />
            </Show>
          </button>
        </div>
      }>
        {(interaction) => (
          <div class="interaction-footer-shell">
            <InteractionComposer
              interaction={interaction()}
              state={interactionState(interaction())}
              error={props.interactionError}
              onResolve={props.onResolve}
            />
            <Show when={props.busy}>
              <button type="button" class="send-button interaction-stop-button" onClick={props.onStop} aria-label="Stop">
                <Pause size={15} />
              </button>
            </Show>
          </div>
        )}
      </Show>
    </footer>
  );
}

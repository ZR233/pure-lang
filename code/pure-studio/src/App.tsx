import { Show, createEffect, createMemo, createSignal, onMount } from "solid-js";
import type { InteractionResolution } from "./types";
import { createStudioStore } from "./solid/studio-store";
import { selectedSessionView, visibleProjectSessions } from "./solid/studio-selectors";
import { MessageTimeline } from "./solid/timeline/message-timeline";
import { SessionStatusBar } from "./solid/status/session-status-bar";
import { SettingsPanel } from "./solid/settings/settings-panel";
import i18n from "./i18n";
import { ProjectSidebar } from "./solid/shell/project-sidebar";
import { ConversationHeader } from "./solid/shell/conversation-header";
import { ConversationFooter } from "./solid/shell/conversation-footer";

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
      <Show
        when={state.settingsOpen}
        fallback={
          <>
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
              <ConversationHeader
                title={view().session?.title ?? "Pure Studio"}
                status={state.status}
                onOpenSettings={() => studio.actions.openSettings()}
              />
              <div class="conversation-workspace">
                <MessageTimeline
                  sessionId={view().sessionId}
                  messages={view().messages}
                  getMessageParts={getMessageParts}
                  getPart={getPart}
                  getPartDelta={(partId) => state.partTextAccumDelta[partId]}
                  busy={view().busy}
                  empty={i18n.t("conversation.emptyTitle")}
                />
                <div class="conversation-dock">
                  <div class="conversation-dock-inner">
                    <SessionStatusBar
                      runtime={view().runtime}
                      providers={state.providers}
                      roles={state.roles}
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
                      onSaveRoles={(roles) => {
                        void studio.actions.saveProviderSettings({
                          roles,
                        });
                      }}
                    />
                    <ConversationFooter
                      prompt={state.prompt}
                      busy={view().busy}
                      activeInteraction={activeInteraction()}
                      resolvingInteractionId={resolvingInteractionId()}
                      interactionError={interactionError()}
                      permissionMode={state.permissionMode}
                      onSetPrompt={studio.actions.setPrompt}
                      onSavePermissionMode={(mode) => void studio.actions.savePermissionMode(mode)}
                      onSubmit={() => void studio.actions.submitPrompt()}
                      onStop={() => void studio.actions.stop()}
                      onResolve={(interaction, resolution) => void resolveActiveInteraction(interaction.interactionId, resolution)}
                    />
                  </div>
                </div>
              </div>
            </section>
          </>
        }
      >
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

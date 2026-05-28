import { ApprovalOverlay } from "./components/ApprovalOverlay";
import { ConversationPanel } from "./components/ConversationPanel";
import { ProjectRail } from "./components/ProjectRail";
import { SettingsPage } from "./components/SettingsPage";
import { useStudioApp } from "./hooks/useStudioApp";

export function App() {
  const studio = useStudioApp();
  const { state } = studio;

  return (
    <main className="app-shell">
      <ProjectRail
        projects={state.projects}
        sessions={state.sessions}
        selectedProjectId={state.selectedProjectId}
        selectedSessionId={state.selectedSessionId}
        manualPath={state.manualPath}
        onSetManualPath={(path) => studio.dispatch({ type: "setManualPath", path })}
        onAddProject={(path) => void studio.addProject(path)}
        onSelectProject={(id) => void studio.onSelectProject(id)}
        onNewSession={() => void studio.onNewSession()}
        onSelectSession={(id) => void studio.onSelectSession(id)}
        onOpenSettings={() => void studio.openSettings()}
        chooseFolder={() => void studio.chooseFolder()}
      />

      <ConversationPanel
        selectedSession={studio.selectedSession}
        selectedProject={studio.selectedProject}
        isBusy={state.isBusy}
        chatItems={studio.chatItems}
        subagentActivities={state.subagentActivities}
        timelineItems={state.timelineItems}
        sessionRuntime={state.sessionRuntime}
        prompt={state.prompt}
        status={state.status}
        turnPhase={state.turnPhase}
        turnStartedAt={state.turnStartedAt}
        providers={state.providers}
        roles={state.roles}
        setRoles={studio.setRolesState}
        onSaveProviderSettings={(explicitRoles) => void studio.onSaveProviderSettings(explicitRoles)}
        onSetPrompt={(value) => studio.dispatch({ type: "setPrompt", prompt: value })}
        onSendPrompt={() => void studio.onSendPrompt()}
        onStopPrompt={() => void studio.onStopPrompt()}
      />

      <ApprovalOverlay
        approvals={state.approvals}
        onApprove={(id) => void studio.onApprove(id)}
        onDeny={(id) => void studio.onDeny(id)}
      />

      {state.settingsOpen ? (
        <SettingsPage
          activeSettingsTab={state.activeSettingsTab}
          providers={state.providers}
          providerTemplates={state.providerTemplates}
          roles={state.roles}
          selectedProviderId={state.selectedProviderId}
          providerSearch={state.providerSearch}
          configExists={state.configExists}
          configToml={state.configToml}
          setProviders={studio.setProvidersState}
          setRoles={studio.setRolesState}
          setSelectedProviderId={studio.setSelectedProviderIdState}
          setProviderSearch={studio.setProviderSearchState}
          setConfigToml={studio.setConfigTomlState}
          onClose={() => studio.dispatch({ type: "setSettingsOpen", value: false })}
          onSetActiveTab={(tab) => studio.dispatch({ type: "setSettingsOpen", value: true, tab })}
          onSaveProviderSettings={() => void studio.onSaveProviderSettings()}
          onSaveConfig={() => void studio.onSaveConfig()}
          onReloadConfig={() => void studio.onReloadConfig()}
        />
      ) : null}
    </main>
  );
}

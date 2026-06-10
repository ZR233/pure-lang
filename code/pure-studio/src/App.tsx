import { ApprovalOverlay } from "./components/ApprovalOverlay";
import { ConversationPanel } from "./components/ConversationPanel";
import { ProjectRail } from "./components/ProjectRail";
import { SettingsPage } from "./components/SettingsPage";
import { useStudioApp } from "./hooks/useStudioApp";

export function App() {
  const studio = useStudioApp();
  const { state } = studio;

  return (
    <main className="flex h-screen overflow-hidden bg-background">
      <div className="flex-shrink-0">
        <ProjectRail
          projects={state.projects}
          sessions={state.sessions}
          selectedProjectId={state.selectedProjectId}
          selectedSessionId={state.selectedSessionId}
          manualPath={state.manualPath}
          onSetManualPath={(path) =>
            studio.dispatch({ type: "setManualPath", path })
          }
          onAddProject={(path) => void studio.addProject(path)}
          onSelectProject={(id) => void studio.onSelectProject(id)}
          onNewSession={() => void studio.onNewSession()}
          onSelectSession={(id) => void studio.onSelectSession(id)}
          onDeleteSession={(id) => void studio.onDeleteSession(id)}
          onOpenSettings={() => void studio.openSettings()}
          chooseFolder={() => void studio.chooseFolder()}
          sessionActionsDisabled={state.isBusy}
        />
      </div>

      <div className="flex flex-1 min-w-0">
        <ConversationPanel
          selectedSession={studio.selectedSession}
          isBusy={state.isBusy}
          entries={studio.timelineEntries}
          agents={state.agents}
          sessionRuntime={state.sessionRuntime}
          pendingUserInput={state.pendingUserInput}
          planAction={studio.planAction}
          prompt={state.prompt}
          status={state.status}
          turnPhase={state.turnPhase}
          turnStartedAt={state.turnStartedAt}
          permissionMode={state.permissionMode}
          providers={state.providers}
          roles={state.roles}
          setRoles={studio.setRolesState}
          onSaveProviderSettings={(explicitRoles) =>
            void studio.onSaveProviderSettings(explicitRoles)
          }
          onSavePermissionMode={(mode) =>
            void studio.onSavePermissionMode(mode)
          }
          onSetPrompt={(value) =>
            studio.dispatch({ type: "setPrompt", prompt: value })
          }
          onSetSessionMode={(mode) => void studio.onSetSessionMode(mode)}
          onImplementPlan={(planId, plan) =>
            void studio.onImplementPlan(planId, plan)
          }
          onDiscussPlan={(planId, content) =>
            void studio.onDiscussPlan(planId, content)
          }
          onSendPrompt={() => void studio.onSendPrompt()}
          onSetPlanActionMode={(mode) => void studio.onSetPlanActionMode(mode)}
          onDismissPlanAction={(planId) =>
            void studio.onDismissPlanAction(planId)
          }
          onStopPrompt={() => void studio.onStopPrompt()}
          onAnswerUserInput={(requestId, response) =>
            void studio.onAnswerUserInput(requestId, response)
          }
        />
      </div>

      <ApprovalOverlay
        approvals={state.approvals}
        onApprove={(id) => void studio.onApprove(id)}
        onDeny={(id) => void studio.onDeny(id)}
      />

      {state.settingsOpen ? (
        <SettingsPage
          activeSettingsTab={state.activeSettingsTab}
          providers={state.providers}
          mcpServers={state.mcpServers}
          providerTemplates={state.providerTemplates}
          providerUsages={state.providerUsages}
          providerUsagesLoading={state.providerUsagesLoading}
          providerUsageError={state.providerUsageError}
          roles={state.roles}
          selectedProjectId={state.selectedProjectId}
          selectedProviderId={state.selectedProviderId}
          providerSearch={state.providerSearch}
          configExists={state.configExists}
          permissionMode={state.permissionMode}
          setRoles={studio.setRolesState}
          setProviderSearch={studio.setProviderSearchState}
          onClose={() =>
            studio.dispatch({ type: "setSettingsOpen", value: false })
          }
          onSetActiveTab={(tab) =>
            studio.dispatch({ type: "setSettingsOpen", value: true, tab })
          }
          onSaveProviderSettings={(snapshot) =>
            studio.onSaveProviderSettings(snapshot)
          }
          onSavePermissionMode={(mode) => studio.onSavePermissionMode(mode)}
          onSaveMcpSettings={(servers) => studio.onSaveMcpSettings(servers)}
          onRefreshProviderUsages={() => void studio.refreshProviderUsages()}
        />
      ) : null}
    </main>
  );
}

part of 'studio_shell.dart';

@Preview(
  name: 'Root workspace',
  group: 'Agent Workspace',
  size: Size(1280, 800),
  brightness: Brightness.dark,
)
Widget agentWorkspaceRootPreview() {
  return _agentWorkspacePreview(_agentWorkspacePreviewState());
}

@Preview(
  name: 'Child workspace',
  group: 'Agent Workspace',
  size: Size(1280, 800),
  brightness: Brightness.dark,
)
Widget agentWorkspaceChildPreview() {
  return _agentWorkspacePreview(_agentWorkspacePreviewState(selectChild: true));
}

@Preview(
  name: 'Child workspace loading',
  group: 'Agent Workspace',
  size: Size(1280, 800),
  brightness: Brightness.dark,
)
Widget agentWorkspaceLoadingPreview() {
  return _agentWorkspacePreview(
    _agentWorkspacePreviewState(selectChild: true, childLoading: true),
  );
}

Widget _agentWorkspacePreview(StudioState state) {
  return ProviderScope(
    key: ValueKey(
      'agent-workspace-preview-${state.selectedSessionId}-'
      '${state.selectedAgentWorkspace?.syncState.name}',
    ),
    overrides: [
      studioControllerProvider.overrideWith(
        () => _AgentWorkspacePreviewController(state),
      ),
    ],
    child: MaterialApp(
      debugShowCheckedModeBanner: false,
      theme: pureStudioTheme(Brightness.light),
      darkTheme: pureStudioTheme(Brightness.dark),
      themeMode: ThemeMode.dark,
      localizationsDelegates: AppLocalizations.localizationsDelegates,
      supportedLocales: AppLocalizations.supportedLocales,
      home: const Scaffold(body: AgentWorkspacePane()),
    ),
  );
}

class _AgentWorkspacePreviewController extends StudioController {
  _AgentWorkspacePreviewController(this.previewState);

  final StudioState previewState;

  @override
  Future<StudioState> build() async => previewState;
}

StudioState _agentWorkspacePreviewState({
  bool selectChild = false,
  bool childLoading = false,
}) {
  const project = StudioProject(
    id: 'preview-project',
    name: 'Agent Workspace Preview',
    path: '.',
  );
  final timestamp = DateTime.fromMillisecondsSinceEpoch(1000);
  final root = StudioSession(
    id: 'preview-root',
    projectId: project.id,
    title: 'Planner workspace',
    mode: StudioMode.task,
    createdAt: timestamp,
    updatedAt: timestamp,
    rootSessionId: 'preview-root',
    ownerRole: 'planner',
  );
  final child = StudioSession(
    id: 'preview-child',
    projectId: project.id,
    title: 'Reviewer workspace',
    mode: StudioMode.task,
    createdAt: timestamp.add(const Duration(seconds: 1)),
    updatedAt: timestamp.add(const Duration(seconds: 1)),
    parentSessionId: root.id,
    rootSessionId: root.id,
    sessionKind: StudioSessionKind.agent,
    ownerAgentId: 'preview-reviewer',
    ownerRole: 'reviewer',
    agentStatus: 'running',
  );
  final selected = selectChild ? child : root;
  final message = TimelineMessage(
    id: '${selected.id}-message',
    sessionId: selected.id,
    role: 'assistant',
    createdAt: timestamp,
  );
  final part = TimelinePartSnapshot(
    id: '${selected.id}-part',
    messageId: message.id,
    sessionId: selected.id,
    turnId: '${selected.id}-turn',
    type: TimelinePartType.text,
    order: 0,
    revision: 0,
    text: selectChild
        ? 'Reviewer is checking the workspace boundary.'
        : 'Planner owns this root workspace and its editable composer.',
    status: 'completed',
    createdAt: timestamp,
    updatedAt: timestamp,
  );
  final runtime = SessionRuntimeView(
    model: selectChild ? 'reviewer/model' : 'planner/model',
    contextTokens: selectChild ? 4800 : 12800,
    contextWindow: 128000,
    totalTokens: selectChild ? 7200 : 19200,
    costLabel: selectChild ? 'CNY 0.05' : 'CNY 0.16',
    activeSkills: selectChild ? const ['review-skill'] : const ['planning'],
    activeMcpServers: const ['dart'],
    activeLspServers: const ['rust-analyzer'],
    agentCount: 1,
  );
  return StudioState(
    projects: const [project],
    sessions: [root, child],
    messagesBySession: childLoading
        ? const {}
        : {
            selected.id: [message],
          },
    partSnapshotsBySession: childLoading
        ? const {}
        : {
            selected.id: {part.id: part},
          },
    agentTimelineEventsBySession: childLoading
        ? const {}
        : {
            selected.id: {
              '${selected.id}-todo': TimelineAgentEvent(
                eventId: '${selected.id}-todo',
                sessionId: selected.id,
                sequence: 1,
                createdAt: timestamp,
                payload: const TimelineTodoListUpdate(
                  callId: 'preview-todo',
                  explanation: 'Workspace checklist',
                  items: [
                    TimelineTodoItem(
                      step: 'Keep every panel on one session',
                      status: 'inProgress',
                    ),
                  ],
                ),
              ),
            },
          },
    providers: const [
      ProviderSettingsView(
        id: 'preview-provider',
        name: 'Preview Provider',
        baseUrl: '',
        defaultModel: 'planner/model',
        models: [
          ProviderModelView(
            slug: 'planner/model',
            displayName: 'Planner Model',
            reasoningEfforts: ['high'],
          ),
        ],
        status: 'ready',
        usageLabel: '',
        wireProtocol: 'responses',
      ),
    ],
    roles: const [
      RoleSettingsView(
        key: 'planner',
        providerId: 'preview-provider',
        model: 'planner/model',
        effort: 'high',
      ),
      RoleSettingsView(
        key: 'executor',
        providerId: 'preview-provider',
        model: 'planner/model',
        effort: 'high',
      ),
    ],
    mcpServers: const [],
    selectedProjectId: project.id,
    selectedSessionId: selected.id,
    selectedRootSessionId: root.id,
    permissionMode: PermissionMode.requestApproval,
    turnPhasesBySession: childLoading
        ? const {}
        : {selected.id: selectChild ? TurnPhase.streaming : TurnPhase.idle},
    runtimesBySession: childLoading ? const {} : {selected.id: runtime},
    workspaceSyncBySession: {
      selected.id: childLoading
          ? AgentWorkspaceSyncState.loading
          : AgentWorkspaceSyncState.ready,
    },
    pendingInteractions: const [],
    composerTextsBySession: selectChild
        ? const {}
        : {root.id: 'Refine the implementation plan'},
  );
}

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
      'agent-workspace-preview-${state.selectedThreadId}-'
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
  final root = StudioThread(
    id: 'preview-root',
    projectId: project.id,
    title: 'Planner workspace',
    mode: StudioMode.task,
    createdAt: timestamp,
    updatedAt: timestamp,
    rootThreadId: 'preview-root',
    role: 'planner',
  );
  final child = StudioThread(
    id: 'preview-child',
    projectId: project.id,
    title: 'Reviewer workspace',
    mode: StudioMode.task,
    createdAt: timestamp.add(const Duration(seconds: 1)),
    updatedAt: timestamp.add(const Duration(seconds: 1)),
    parentThreadId: root.id,
    rootThreadId: root.id,
    agentPath: 'preview-reviewer',
    role: 'reviewer',
    status: 'running',
  );
  final selected = selectChild ? child : root;
  final item = ThreadItemView(
    id: '${selected.id}-item',
    threadId: selected.id,
    turnId: '${selected.id}-turn',
    ordinal: 0,
    revision: 0,
    status: 'completed',
    createdAt: timestamp,
    updatedAt: timestamp,
    completedAt: timestamp,
    kind: ThreadItemKind.agentMessage,
    channel: AgentMessageChannel.finalAnswer,
    text: selectChild
        ? 'Reviewer is checking the workspace boundary.'
        : 'Planner owns this root workspace and its editable composer.',
  );
  final runtime = ThreadRuntimeView(
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
    threads: [root, child],
    workspacesByThread: childLoading
        ? const {}
        : {
            selected.id: ThreadWorkspace(
              thread: selected,
              revision: 1,
              items: [item],
              interactions: const [],
              runtime: runtime,
              activeTurn: !selectChild
                  ? null
                  : StudioTurnView(
                      turnId: '${selected.id}-turn',
                      threadId: selected.id,
                      state: const StudioTurnState.inProgress(
                        StudioTurnActivity.responding,
                      ),
                      updatedAt: timestamp,
                    ),
              todo: const TimelineTodoListUpdate(
                callId: 'preview-todo',
                explanation: 'Workspace checklist',
                items: [
                  TimelineTodoItem(
                    step: 'Keep every panel on one Thread',
                    status: 'inProgress',
                  ),
                ],
              ),
            ),
          },
    workspaceUiByThread: {
      selected.id: WorkspaceUiState(
        syncState: childLoading
            ? AgentWorkspaceSyncState.loading
            : AgentWorkspaceSyncState.ready,
        composer: selectChild
            ? const ComposerThreadState.idle()
            : const ComposerThreadState.idle(
                draft: 'Refine the implementation plan',
              ),
      ),
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
    selectedThreadId: selected.id,
    permissionMode: PermissionMode.requestApproval,
  );
}

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
  return _AgentWorkspacePreviewScope(
    key: ValueKey(
      'agent-workspace-preview-${state.selectedThreadId}-'
      '${state.selectedAgentWorkspace?.syncState.name}',
    ),
    previewState: state,
  );
}

class _AgentWorkspacePreviewScope extends StatefulWidget {
  const _AgentWorkspacePreviewScope({super.key, required this.previewState});

  final StudioState previewState;

  @override
  State<_AgentWorkspacePreviewScope> createState() =>
      _AgentWorkspacePreviewScopeState();
}

class _AgentWorkspacePreviewScopeState
    extends State<_AgentWorkspacePreviewScope> {
  late final ProviderContainer _container;

  @override
  void initState() {
    super.initState();
    _container = ProviderContainer(
      overrides: [
        studioControllerProvider.overrideWith(
          () => _AgentWorkspacePreviewController(widget.previewState),
        ),
      ],
    );
  }

  @override
  void dispose() {
    _container.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return UncontrolledProviderScope(
      container: _container,
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
    mode: ThreadModeId.task,
    createdAt: timestamp,
    updatedAt: timestamp,
    rootThreadId: 'preview-root',
    role: 'planner',
  );
  final child = StudioThread(
    id: 'preview-child',
    projectId: project.id,
    title: 'Reviewer workspace',
    mode: ThreadModeId.task,
    createdAt: timestamp.add(const Duration(seconds: 1)),
    updatedAt: timestamp.add(const Duration(seconds: 1)),
    parentThreadId: root.id,
    rootThreadId: root.id,
    agentPath: 'preview-reviewer',
    role: 'reviewer',
    status: ThreadStatusView.running,
  );
  final selected = selectChild ? child : root;
  final item = ThreadItemView(
    id: '${selected.id}-item',
    threadId: selected.id,
    turnId: '${selected.id}-turn',
    ordinal: 0,
    revision: 0,
    createdAt: timestamp,
    updatedAt: timestamp,
    state: ThreadTextItemStateView(
      channel: ThreadTextChannel.finalAnswer,
      text: selectChild
          ? 'Reviewer is checking the workspace boundary.'
          : 'Planner owns this root workspace and its editable composer.',
      attachments: const [],
      lifecycle: CompletedThreadContentView(timestamp),
    ),
  );
  final runtime = ThreadRuntimeView(
    model: selectChild ? 'reviewer/model' : 'planner/model',
    contextTokens: selectChild ? 4800 : 12800,
    contextWindow: 128000,
    totalTokens: selectChild ? 7200 : 19200,
    costLabel: selectChild ? '￥0.05' : '￥0.16',
    activeSkills: selectChild ? const ['review-skill'] : const ['planning'],
    activeMcpServers: const ['dart'],
    activeLspServers: const ['rust-analyzer'],
    agentCount: 1,
  );
  return StudioState(
    projectDirectory: ProjectDirectoryState.fromState(
      state: ReadyObservedResource(
        revision: 1,
        updatedAt: timestamp.millisecondsSinceEpoch ~/ 1000,
        lastCheckedAt: null,
        value: const [project],
      ),
    ),
    threadDirectory: ThreadDirectoryWindow(threads: [root, child]),
    agentDirectory: AgentDirectoryState.fromState(
      state: UninitializedObservedResource(
        updatedAt: timestamp.millisecondsSinceEpoch ~/ 1000,
      ),
    ),
    settingsState: SettingsStateSnapshot.fromState(
      state: ReadyObservedResource(
        revision: 1,
        updatedAt: timestamp.millisecondsSinceEpoch ~/ 1000,
        lastCheckedAt: null,
        value: const SettingsStateData(
          providers: [
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
                  wireProtocol: 'responses',
                  supportedConnectionModes: ['web_socket', 'http'],
                  defaultConnectionMode: 'web_socket',
                  connectionMode: 'web_socket',
                ),
              ],
              status: 'ready',
              usageLabel: '',
            ),
          ],
          roles: [
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
          permissionMode: PermissionMode.requestApproval,
        ),
      ),
    ),
    recoveryState: RecoveryStateSnapshot.fromState(
      state: UninitializedObservedResource(
        updatedAt: timestamp.millisecondsSinceEpoch ~/ 1000,
      ),
    ),
    mcpState: McpStateSnapshot.fromState(
      state: UninitializedObservedResource(
        updatedAt: timestamp.millisecondsSinceEpoch ~/ 1000,
      ),
    ),
    lspState: LspStateSnapshot.fromState(
      state: UninitializedObservedResource(
        updatedAt: timestamp.millisecondsSinceEpoch ~/ 1000,
      ),
    ),
    skillsByProject: const {},
    providerUsageState: ProviderUsageStateSnapshot.fromState(
      state: UninitializedObservedResource(
        updatedAt: timestamp.millisecondsSinceEpoch ~/ 1000,
      ),
    ),
    updaterState: UpdaterStateSnapshot.idle(
      revision: 0,
      updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
    ),
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
                      revision: 1,
                      state: RunningStudioTurnState(
                        startedAt: timestamp.millisecondsSinceEpoch ~/ 1000,
                        activity: StudioTurnActivity.responding,
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
    selectedProjectId: project.id,
    selectedThreadId: selected.id,
  );
}

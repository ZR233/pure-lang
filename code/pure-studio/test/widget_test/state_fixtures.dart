part of '../widget_test.dart';

const _testProviderCatalog = ProviderCatalogView(
  schemaVersion: 6,
  revision: 'widget-test-catalog-v6',
  presets: [
    ProviderPresetView(
      id: 'deepseek',
      displayName: 'DeepSeek',
      description: 'DeepSeek API',
      baseUrl: 'https://api.deepseek.com',
      credentialLabel: 'API Key',
      credentialEnv: 'DEEPSEEK_API_KEY',
      modelCatalogId: 'deepseek',
      suggestedModel: 'deepseek-v4-flash',
      promptCacheDialect: 'implicit_prefix',
    ),
    ProviderPresetView(
      id: 'openai',
      displayName: 'OpenAI',
      description: 'OpenAI API',
      baseUrl: 'https://api.openai.com/v1',
      credentialLabel: 'API Key',
      credentialEnv: 'OPENAI_API_KEY',
      modelCatalogId: 'openai',
      suggestedModel: 'gpt-5.6-sol',
      hostedWebSearch: true,
      standaloneWebSearch: 'open_ai_search_api',
      promptCacheDialect: 'open_ai_prompt_cache_key',
      responsesToolSearch: true,
      responsesProgrammaticToolCalling: true,
    ),
    ProviderPresetView(
      id: 'zhipu-coding-plan',
      displayName: 'Zhipu Coding Plan',
      description: 'Zhipu Coding Plan API',
      baseUrl: 'https://open.bigmodel.cn/api/coding/paas/v4',
      credentialLabel: 'API Key',
      credentialEnv: 'ZHIPU_API_KEY',
      modelCatalogId: 'zhipu',
      suggestedModel: 'glm-4.7',
    ),
    ProviderPresetView(
      id: 'future-provider',
      displayName: 'Future Provider',
      description: 'Unknown provider fixture',
      baseUrl: 'https://future.example/v1',
      credentialLabel: 'Access Key',
      credentialEnv: 'FUTURE_PROVIDER_KEY',
      modelCatalogId: 'future-catalog',
      suggestedModel: 'future-model',
      hostedWebSearch: true,
      standaloneWebSearch: 'future_search_dialect',
      promptCacheDialect: 'implicit_prefix',
    ),
  ],
  modelCatalogs: {
    'deepseek': [
      ProviderModelView(
        slug: 'deepseek-v4-flash',
        displayName: 'DeepSeek V4 Flash',
        reasoningEfforts: ['high', 'max'],
        defaultReasoningEffort: 'high',
        wireProtocol: 'responses',
        supportedConnectionModes: ['http'],
      ),
      ProviderModelView(
        slug: 'deepseek-reasoner',
        displayName: 'DeepSeek Reasoner',
        reasoningEfforts: ['high', 'max'],
        defaultReasoningEffort: 'high',
        wireProtocol: 'chat_completions',
      ),
    ],
    'openai': [
      ProviderModelView(
        slug: 'gpt-5.6-sol',
        displayName: 'GPT-5.6-Sol',
        reasoningEfforts: ['low', 'medium', 'high', 'max'],
        defaultReasoningEffort: 'high',
        wireProtocol: 'responses',
        supportedConnectionModes: ['web_socket', 'http'],
        defaultConnectionMode: 'web_socket',
        connectionMode: 'web_socket',
      ),
      ProviderModelView(
        slug: 'gpt-5.6-terra',
        displayName: 'GPT-5.6-Terra',
        reasoningEfforts: ['low', 'medium', 'high', 'max'],
        defaultReasoningEffort: 'high',
        wireProtocol: 'responses',
        supportedConnectionModes: ['web_socket', 'http'],
        defaultConnectionMode: 'web_socket',
        connectionMode: 'web_socket',
      ),
      ProviderModelView(
        slug: 'gpt-5.6-luna',
        displayName: 'GPT-5.6-Luna',
        reasoningEfforts: ['low', 'medium', 'high', 'max'],
        defaultReasoningEffort: 'high',
        wireProtocol: 'responses',
        supportedConnectionModes: ['web_socket', 'http'],
        defaultConnectionMode: 'web_socket',
        connectionMode: 'web_socket',
      ),
    ],
    'zhipu': [
      ProviderModelView(
        slug: 'glm-4.7',
        displayName: 'GLM-4.7',
        reasoningEfforts: ['enabled', 'disabled'],
        defaultReasoningEffort: 'enabled',
      ),
    ],
    'future-catalog': [
      ProviderModelView(
        slug: 'future-model',
        displayName: 'Future Model',
        reasoningEfforts: ['eco', 'balanced', 'max'],
        defaultReasoningEffort: 'balanced',
      ),
    ],
  },
);

StudioState _emptyState() {
  const project = StudioProject(id: 'project-1', name: 'project', path: '.');
  final session = StudioThread(
    id: 'session-1',
    projectId: project.id,
    title: 'Session',
    mode: StudioMode.simple,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
  );
  return _studioStateFixture(
    projects: const [project],
    threads: [session],
    workspacesByThread: {
      session.id: ThreadWorkspace(
        thread: session,
        revision: 0,
        items: const [],
        interactions: const [],
        runtime: _testRuntime(),
      ),
    },
    workspaceUiByThread: {
      session.id: const WorkspaceUiState(
        syncState: AgentWorkspaceSyncState.ready,
      ),
    },
    selectedProjectId: project.id,
    selectedThreadId: session.id,
  );
}

StudioState _noProjectState() {
  return _studioStateFixture(selectedProjectId: null, selectedThreadId: null);
}

StudioState _twoProjectState({
  required String selectedProjectId,
  List<StudioProject> projects = const [
    StudioProject(id: 'project-a', name: 'Project A', path: 'a'),
    StudioProject(id: 'project-b', name: 'Project B', path: 'b'),
  ],
  StudioTurnState? turnState,
}) {
  final threads = [
    if (projects.any((project) => project.id == 'project-a'))
      StudioThread(
        id: 'session-a',
        projectId: 'project-a',
        title: 'Session A',
        mode: StudioMode.simple,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
      ),
    if (projects.any((project) => project.id == 'project-b'))
      StudioThread(
        id: 'session-b',
        projectId: 'project-b',
        title: 'Session B',
        mode: StudioMode.task,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
      ),
  ];
  final selectedThreadId = selectedProjectId == 'project-b'
      ? 'session-b'
      : 'session-a';
  return _emptyState().copyWith(
    projectDirectory: ProjectDirectoryState(values: projects),
    threadDirectory: ThreadDirectoryWindow(threads: threads),
    workspacesByThread: {
      for (final session in threads)
        session.id: ThreadWorkspace(
          thread: session,
          revision: 0,
          items: const [],
          interactions: const [],
          runtime: _testRuntime(),
          activeTurn: session.id == selectedThreadId && turnState != null
              ? _testTurn(threadId: session.id, state: turnState)
              : null,
        ),
    },
    workspaceUiByThread: {
      for (final session in threads)
        session.id: const WorkspaceUiState(
          syncState: AgentWorkspaceSyncState.ready,
        ),
    },
    selectedProjectId: selectedProjectId,
    selectedThreadId: selectedThreadId,
  );
}

StudioTurnView _testTurn({
  required String threadId,
  required StudioTurnState state,
  String turnId = 'turn-1',
  int updatedAt = 1,
  StudioTurnFailureView? failure,
}) {
  return StudioTurnView(
    turnId: turnId,
    threadId: threadId,
    state: state,
    failure: failure,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(updatedAt),
  );
}

StudioState _stateWithPlannerModels() {
  final state = _emptyState();
  return state.copyWith(
    settingsState: SettingsStateSnapshot(
      providers: const [
        ProviderSettingsView(
          id: 'deepseek',
          name: 'DeepSeek',
          baseUrl: 'https://api.deepseek.com',
          defaultModel: 'deepseek-v4-flash',
          models: [
            ProviderModelView(
              slug: 'deepseek-v4-flash',
              displayName: 'DeepSeek V4 Flash',
              reasoningEfforts: ['high', 'max'],
              wireProtocol: 'responses',
              supportedConnectionModes: ['http'],
              defaultConnectionMode: 'http',
              connectionMode: 'http',
            ),
            ProviderModelView(
              slug: 'deepseek-reasoner',
              displayName: 'DeepSeek Reasoner',
              reasoningEfforts: ['high', 'max'],
            ),
          ],
          status: 'ready',
          usageLabel: '2 models',
          promptCacheDialect: 'implicit_prefix',
        ),
      ],
      roles: const [
        RoleSettingsView(
          key: 'executor',
          providerId: 'deepseek',
          model: 'deepseek-v4-flash',
          effort: 'high',
        ),
        RoleSettingsView(
          key: 'planner',
          providerId: 'deepseek',
          model: 'deepseek-v4-flash',
          effort: 'high',
        ),
      ],
    ),
    workspacesByThread: {
      state.selectedThreadId!: state.selectedWorkspace!.copyWith(
        runtime: state.runtime.copyWith(model: 'deepseek-v4-flash'),
      ),
    },
  );
}

StudioState _studioStateFixture({
  List<StudioProject> projects = const [],
  List<StudioThread> threads = const [],
  Map<String, TaskRuntimeView> tasksByRootThread = const {},
  List<StudioAgentView> agents = const [],
  List<StudioRecoveryIssue> recoveryIssues = const [],
  List<ProviderSettingsView> providers = const [],
  String? defaultProviderId,
  List<RoleSettingsView> roles = const [],
  List<McpServerSettingsView> mcpServers = const [],
  InstructionsSettingsView instructions = const InstructionsSettingsView(),
  SkillsSettingsView skills = const SkillsSettingsView(),
  GeneralSettingsView general = const GeneralSettingsView(),
  WebSearchSettingsView webSearch = const WebSearchSettingsView(),
  PermissionMode permissionMode = PermissionMode.requestApproval,
  List<ProviderUsageView> providerUsages = const [],
  Map<String, SkillsStateSnapshot> skillsByProject = const {},
  Map<String, ThreadWorkspace> workspacesByThread = const {},
  Map<String, WorkspaceUiState> workspaceUiByThread = const {},
  ProviderCatalogView providerCatalog = const ProviderCatalogView.empty(),
  String? selectedProjectId,
  String? selectedThreadId,
}) {
  return StudioState(
    projectDirectory: ProjectDirectoryState(values: projects),
    threadDirectory: ThreadDirectoryWindow(threads: threads),
    taskDirectory: TaskDirectoryState(
      values: [
        for (final entry in tasksByRootThread.entries)
          TaskDirectoryEntryView(rootThreadId: entry.key, task: entry.value),
      ],
    ),
    agentDirectory: AgentDirectoryState(values: agents),
    settingsState: SettingsStateSnapshot(
      providers: providers,
      defaultProviderId: defaultProviderId,
      roles: roles,
      mcpServers: mcpServers,
      instructions: instructions,
      skills: skills,
      general: general,
      webSearch: webSearch,
      permissionMode: permissionMode,
    ),
    recoveryState: RecoveryStateSnapshot(values: recoveryIssues),
    mcpState: const McpStateSnapshot(),
    lspState: const LspStateSnapshot(),
    skillsByProject: skillsByProject,
    providerUsageState: ProviderUsageStateSnapshot(usages: providerUsages),
    updaterState: const UpdaterStateSnapshot(),
    workspacesByThread: workspacesByThread,
    workspaceUiByThread: workspaceUiByThread,
    providerCatalog: providerCatalog,
    selectedProjectId: selectedProjectId,
    selectedThreadId: selectedThreadId,
  );
}

ObservedStateMeta _testObservedMeta(int revision) {
  return ObservedStateMeta(
    revision: revision,
    phase: ObservedStatePhase.ready,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
    stale: false,
  );
}

StudioState _withSettingsFixture(
  StudioState state, {
  List<ProviderSettingsView>? providers,
  Object? defaultProviderId = _fixtureUnset,
  List<RoleSettingsView>? roles,
  List<McpServerSettingsView>? mcpServers,
  SkillsSettingsView? skills,
  WebSearchSettingsView? webSearch,
  PermissionMode? permissionMode,
}) {
  final current = state.settingsState;
  return state.copyWith(
    settingsState: SettingsStateSnapshot(
      meta: current.meta,
      providers: providers ?? current.providers,
      defaultProviderId: identical(defaultProviderId, _fixtureUnset)
          ? current.defaultProviderId
          : defaultProviderId as String?,
      roles: roles ?? current.roles,
      mcpServers: mcpServers ?? current.mcpServers,
      instructions: current.instructions,
      skills: skills ?? current.skills,
      general: current.general,
      webSearch: webSearch ?? current.webSearch,
      permissionMode: permissionMode ?? current.permissionMode,
    ),
  );
}

const _fixtureUnset = Object();

ThreadRuntimeView _testRuntime() => const ThreadRuntimeView(
  model: '',
  contextTokens: 0,
  contextWindow: 0,
  totalTokens: 0,
  costLabel: '',
  activeSkills: [],
  activeMcpServers: [],
  activeLspServers: [],
  agentCount: 0,
);

ThreadItemView _threadItemFixture({
  required String id,
  required String threadId,
  required String turnId,
  required int ordinal,
  String text = '',
  ThreadItemKind kind = ThreadItemKind.agentMessage,
  AgentMessageChannel? channel = AgentMessageChannel.finalAnswer,
  int revision = 0,
  String status = 'completed',
  int createdAt = 1,
  TimelineToolPart? tool,
  List<String> reasoningSummary = const [],
  List<String> reasoningContent = const [],
}) {
  final timestamp = _fixtureDate(createdAt);
  return ThreadItemView(
    id: id,
    threadId: threadId,
    turnId: turnId,
    ordinal: ordinal,
    revision: revision,
    status: status,
    createdAt: timestamp,
    updatedAt: timestamp,
    completedAt: status == 'completed' ? timestamp : null,
    kind: kind,
    channel: channel,
    text: text,
    tool: tool,
    reasoningSummary: reasoningSummary,
    reasoningContent: reasoningContent,
  );
}

StudioState _withSelectedRuntime(StudioState state, ThreadRuntimeView runtime) {
  final threadId = state.selectedThreadId!;
  return state.copyWith(
    workspacesByThread: {
      ...state.workspacesByThread,
      threadId: state.workspacesByThread[threadId]!.copyWith(runtime: runtime),
    },
  );
}

StudioState _withSelectedTurn(StudioState state, StudioTurnView? turn) {
  final threadId = state.selectedThreadId!;
  return state.copyWith(
    workspacesByThread: {
      ...state.workspacesByThread,
      threadId: state.workspacesByThread[threadId]!.copyWith(activeTurn: turn),
    },
  );
}

StudioState _withSelectedInteractions(
  StudioState state,
  List<PendingInteraction> interactions,
) {
  final threadId = state.selectedThreadId!;
  return state.copyWith(
    workspacesByThread: {
      ...state.workspacesByThread,
      threadId: state.workspacesByThread[threadId]!.copyWith(
        interactions: interactions,
      ),
    },
  );
}

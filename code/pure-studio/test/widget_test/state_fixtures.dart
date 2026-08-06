part of '../widget_test.dart';

const _testProviderCatalog = ProviderCatalogView(
  schemaVersion: 4,
  revision: 'widget-test-catalog-v4',
  presets: [
    ProviderPresetView(
      id: 'deepseek',
      displayName: 'DeepSeek',
      description: 'DeepSeek API',
      wireProtocol: 'chat_completions',
      connectionModes: [
        ProviderConnectionModeView(id: 'http', displayName: 'HTTP'),
      ],
      defaultConnectionMode: 'http',
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
      wireProtocol: 'responses',
      connectionModes: [
        ProviderConnectionModeView(id: 'web_socket', displayName: 'WebSocket'),
        ProviderConnectionModeView(id: 'http', displayName: 'HTTP'),
      ],
      defaultConnectionMode: 'web_socket',
      baseUrl: 'https://api.openai.com/v1',
      credentialLabel: 'API Key',
      credentialEnv: 'OPENAI_API_KEY',
      modelCatalogId: 'openai',
      suggestedModel: 'gpt-5.6-sol',
      hostedWebSearch: true,
      standaloneWebSearch: 'open_ai_search_api',
      promptCacheDialect: 'open_ai_prompt_cache_key',
    ),
    ProviderPresetView(
      id: 'zhipu-coding-plan',
      displayName: 'Zhipu Coding Plan',
      description: 'Zhipu Coding Plan API',
      wireProtocol: 'chat_completions',
      connectionModes: [
        ProviderConnectionModeView(id: 'http', displayName: 'HTTP'),
      ],
      defaultConnectionMode: 'http',
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
      wireProtocol: 'chat_completions',
      connectionModes: [
        ProviderConnectionModeView(id: 'http', displayName: 'HTTP'),
      ],
      defaultConnectionMode: 'http',
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
      ),
      ProviderModelView(
        slug: 'deepseek-reasoner',
        displayName: 'DeepSeek Reasoner',
        reasoningEfforts: ['high', 'max'],
        defaultReasoningEffort: 'high',
      ),
    ],
    'openai': [
      ProviderModelView(
        slug: 'gpt-5.6-sol',
        displayName: 'GPT-5.6-Sol',
        reasoningEfforts: ['low', 'medium', 'high', 'max'],
        defaultReasoningEffort: 'high',
      ),
      ProviderModelView(
        slug: 'gpt-5.6-terra',
        displayName: 'GPT-5.6-Terra',
        reasoningEfforts: ['low', 'medium', 'high', 'max'],
        defaultReasoningEffort: 'high',
      ),
      ProviderModelView(
        slug: 'gpt-5.6-luna',
        displayName: 'GPT-5.6-Luna',
        reasoningEfforts: ['low', 'medium', 'high', 'max'],
        defaultReasoningEffort: 'high',
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
  return StudioState(
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
    providers: const [],
    roles: const [],
    mcpServers: const [],
    selectedProjectId: project.id,
    selectedThreadId: session.id,
    permissionMode: PermissionMode.requestApproval,
  );
}

StudioState _noProjectState() {
  return StudioState(
    projects: [],
    threads: [],
    providers: [],
    roles: [],
    mcpServers: [],
    selectedProjectId: null,
    selectedThreadId: null,
    permissionMode: PermissionMode.requestApproval,
  );
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
    projects: projects,
    threads: threads,
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
}) {
  return StudioTurnView(
    turnId: turnId,
    threadId: threadId,
    state: state,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(updatedAt),
  );
}

StudioState _stateWithPlannerModels() {
  final state = _emptyState();
  return state.copyWith(
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
    workspacesByThread: {
      state.selectedThreadId!: state.selectedWorkspace!.copyWith(
        runtime: state.runtime.copyWith(model: 'deepseek-v4-flash'),
      ),
    },
  );
}

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

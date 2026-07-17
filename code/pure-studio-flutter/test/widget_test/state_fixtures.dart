part of '../widget_test.dart';

const _testProviderCatalog = ProviderCatalogView(
  schemaVersion: 3,
  revision: 'widget-test-catalog-v3',
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
  final session = StudioSession(
    id: 'session-1',
    projectId: project.id,
    title: 'Session',
    mode: StudioMode.simple,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
  );
  return StudioState(
    projects: const [project],
    sessions: [session],
    messagesBySession: const {'session-1': []},
    providers: const [],
    roles: const [],
    mcpServers: const [],
    selectedProjectId: project.id,
    selectedSessionId: session.id,
    permissionMode: PermissionMode.requestApproval,
    turnPhase: TurnPhase.idle,
    runtime: const SessionRuntimeView(
      model: '',
      contextTokens: 0,
      contextWindow: 0,
      totalTokens: 0,
      costLabel: '',
      activeSkills: [],
      activeMcpServers: [],
      activeLspServers: [],
      agentCount: 0,
    ),
    pendingInteractions: const [],
  );
}

StudioState _noProjectState() {
  return const StudioState(
    projects: [],
    sessions: [],
    messagesBySession: {},
    providers: [],
    roles: [],
    mcpServers: [],
    selectedProjectId: null,
    selectedSessionId: null,
    permissionMode: PermissionMode.requestApproval,
    turnPhase: TurnPhase.idle,
    runtime: SessionRuntimeView(
      model: '',
      contextTokens: 0,
      contextWindow: 0,
      totalTokens: 0,
      costLabel: '',
      activeSkills: [],
      activeMcpServers: [],
      activeLspServers: [],
      agentCount: 0,
    ),
    pendingInteractions: [],
  );
}

StudioState _twoProjectState({
  required String selectedProjectId,
  List<StudioProject> projects = const [
    StudioProject(id: 'project-a', name: 'Project A', path: 'a'),
    StudioProject(id: 'project-b', name: 'Project B', path: 'b'),
  ],
  TurnPhase turnPhase = TurnPhase.idle,
}) {
  final sessions = [
    if (projects.any((project) => project.id == 'project-a'))
      StudioSession(
        id: 'session-a',
        projectId: 'project-a',
        title: 'Session A',
        mode: StudioMode.simple,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
      ),
    if (projects.any((project) => project.id == 'project-b'))
      StudioSession(
        id: 'session-b',
        projectId: 'project-b',
        title: 'Session B',
        mode: StudioMode.task,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
      ),
  ];
  final selectedSessionId = selectedProjectId == 'project-b'
      ? 'session-b'
      : 'session-a';
  return _emptyState().copyWith(
    projects: projects,
    sessions: sessions,
    messagesBySession: {for (final session in sessions) session.id: const []},
    selectedProjectId: selectedProjectId,
    selectedSessionId: selectedSessionId,
    turnPhase: turnPhase,
  );
}

StudioState _sessionHistoryState({
  required String projectId,
  required String sessionId,
  required String text,
  int eventCursor = 42,
  int messageSequence = 0,
  int partSequence = 0,
}) {
  final session = StudioSession(
    id: sessionId,
    projectId: projectId,
    title: 'Loaded $sessionId',
    mode: StudioMode.simple,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(1),
  );
  return _emptyState().copyWith(
    projects: [StudioProject(id: projectId, name: projectId, path: projectId)],
    sessions: [session],
    selectedProjectId: projectId,
    selectedSessionId: sessionId,
    messagesBySession: {
      sessionId: [
        TimelineMessage(
          id: '$sessionId-message-history',
          sessionId: sessionId,
          role: 'assistant',
          createdAt: DateTime.fromMillisecondsSinceEpoch(1),
          sequence: messageSequence,
        ),
      ],
    },
    partSnapshotsBySession: {
      sessionId: {
        '$sessionId-part-history': TimelinePartSnapshot(
          id: '$sessionId-part-history',
          messageId: '$sessionId-message-history',
          sessionId: sessionId,
          turnId: '$sessionId-turn-history',
          type: TimelinePartType.text,
          order: 0,
          revision: 0,
          sequence: partSequence,
          text: text,
          status: 'completed',
          createdAt: DateTime.fromMillisecondsSinceEpoch(1),
          updatedAt: DateTime.fromMillisecondsSinceEpoch(1),
        ),
      },
    },
    eventCursorsBySession: {sessionId: eventCursor},
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
    runtime: state.runtime.copyWith(model: 'deepseek-v4-flash'),
  );
}

frb.BridgeStudioPartDto _bridgePartDto({
  required String partId,
  required String messageId,
  required String partType,
  String sessionId = 'session-1',
  String turnId = 'turn-1',
  String status = 'completed',
  String text = '',
  String? textChannel,
  String? activityGroupId,
  bool synthetic = false,
}) {
  return frb.BridgeStudioPartDto(
    partId: partId,
    messageId: messageId,
    sessionId: sessionId,
    turnId: turnId,
    partType: partType,
    order: BigInt.zero,
    revision: BigInt.zero,
    status: status,
    createdAt: 1,
    updatedAt: 1,
    textChannel: textChannel,
    activityGroupId: activityGroupId,
    text: text,
    synthetic: synthetic,
    ignored: false,
  );
}

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
    runtimesBySession: {
      session.id: const SessionRuntimeView(
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
    },
    pendingInteractions: const [],
  );
}

StudioState _pausedTaskState() {
  final state = _emptyState();
  final session = state.sessions.single.copyWith(
    mode: StudioMode.task,
    agentStatus: 'interrupted',
  );
  final runtime = state.runtimesBySession[session.id]!;
  return state.copyWith(
    sessions: [session],
    runtimesBySession: {
      session.id: runtime.copyWith(
        task: const TaskRuntimeView(
          runId: 'task-run-1',
          phase: 'implementing',
          branch: 'codex/task-1',
          expectedHead: '0123456789abcdef',
          statusMessage: null,
          stopRequestedOrigin: null,
          stopRequestedReason: null,
          taskGeneration: 1,
          workUnits: [],
          agents: [],
          completions: [],
          merges: [],
          reviews: [],
        ),
      ),
    },
  );
}

StudioState _noProjectState() {
  return StudioState(
    projects: [],
    sessions: [],
    messagesBySession: {},
    providers: [],
    roles: [],
    mcpServers: [],
    selectedProjectId: null,
    selectedSessionId: null,
    permissionMode: PermissionMode.requestApproval,
    pendingInteractions: [],
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
    turnsBySession: turnState == null
        ? const {}
        : {
            selectedSessionId: _testTurn(
              sessionId: selectedSessionId,
              state: turnState,
            ),
          },
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

SessionSnapshotFrame _sessionSnapshotFrame(StudioState state) {
  final sessionId = state.selectedSessionId!;
  final runtime = state.runtime;
  return SessionSnapshotFrame(
    snapshot: StudioSessionSnapshot(
      sessionId: sessionId,
      throughSequence: state.eventCursorsBySession[sessionId] ?? 0,
      messages: state.messagesBySession[sessionId] ?? const [],
      parts: state.partSnapshotsBySession[sessionId] ?? const {},
      interactions: const [],
      agents: const {},
      timelineEvents: const {},
      runtime: runtime,
      turn: null,
    ),
  );
}

StudioTurnView _testTurn({
  required String sessionId,
  required StudioTurnState state,
  String turnId = 'turn-1',
  int updatedAt = 1,
}) {
  return StudioTurnView(
    turnId: turnId,
    sessionId: sessionId,
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
    runtimesBySession: {
      state.selectedAgentSessionId!: state.runtime.copyWith(
        model: 'deepseek-v4-flash',
      ),
    },
  );
}

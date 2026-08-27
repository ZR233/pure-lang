part of '../widget_test.dart';

const _testProviderCatalog = ProviderCatalogView(
  schemaVersion: 8,
  revision: 'widget-test-catalog-v8',
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
        inputCapabilities: [
          ModelInputCapabilityView(
            modality: ModelModalityView.text,
            sources: [],
          ),
        ],
        reasoningEfforts: ['high', 'max'],
        defaultReasoningEffort: 'high',
        wireProtocol: 'responses',
        supportedConnectionModes: ['http'],
      ),
      ProviderModelView(
        slug: 'deepseek-v4-flash-vision-exp',
        displayName: 'DeepSeek V4 Flash Vision Exp',
        inputCapabilities: [
          ModelInputCapabilityView(
            modality: ModelModalityView.text,
            sources: [],
          ),
          ModelInputCapabilityView(
            modality: ModelModalityView.image,
            sources: [
              ModelInputSourceView.local,
              ModelInputSourceView.remoteUrl,
            ],
            maxCount: 600,
            maxBytes: 32 * 1024 * 1024,
            maxTotalBytes: 32 * 1024 * 1024,
            maxWidth: 4096,
            maxHeight: 4096,
            mediaTypes: ['image/jpeg', 'image/png', 'image/gif', 'image/webp'],
          ),
        ],
        reasoningEfforts: ['high', 'max'],
        defaultReasoningEffort: 'high',
        wireProtocol: 'responses',
        supportedConnectionModes: ['http'],
      ),
      ProviderModelView(
        slug: 'deepseek-reasoner',
        displayName: 'DeepSeek Reasoner',
        inputCapabilities: [
          ModelInputCapabilityView(
            modality: ModelModalityView.text,
            sources: [],
          ),
        ],
        reasoningEfforts: ['high', 'max'],
        defaultReasoningEffort: 'high',
        wireProtocol: 'chat_completions',
      ),
    ],
    'openai': [
      ProviderModelView(
        slug: 'gpt-5.6-sol',
        displayName: 'GPT-5.6-Sol',
        inputCapabilities: [
          ModelInputCapabilityView(
            modality: ModelModalityView.text,
            sources: [],
          ),
        ],
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
        inputCapabilities: [
          ModelInputCapabilityView(
            modality: ModelModalityView.text,
            sources: [],
          ),
        ],
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
        inputCapabilities: [
          ModelInputCapabilityView(
            modality: ModelModalityView.text,
            sources: [],
          ),
        ],
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
        inputCapabilities: [
          ModelInputCapabilityView(
            modality: ModelModalityView.text,
            sources: [],
          ),
        ],
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
    projectDirectory: ProjectDirectoryState.fromState(
      state: _testReady(projects),
    ),
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
}) {
  return StudioTurnView(
    turnId: turnId,
    threadId: threadId,
    revision: 0,
    state: state,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(updatedAt),
  );
}

StudioState _stateWithPlannerModels() {
  final state = _emptyState();
  return state.copyWith(
    settingsState: SettingsStateSnapshot.fromState(
      state: _testReady(
        const SettingsStateData(
          providers: [
            ProviderSettingsView(
              id: 'deepseek',
              templateKind: 'deepseek',
              name: 'DeepSeek',
              baseUrl: 'https://api.deepseek.com',
              defaultModel: 'deepseek-v4-flash',
              models: [],
              status: 'ready',
              usageLabel: '2 models',
              promptCacheDialect: 'implicit_prefix',
            ),
          ],
          roles: [
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
      ),
    ),
    workspacesByThread: {
      state.selectedThreadId!: state.selectedWorkspace!.copyWith(
        runtime: state.runtime.copyWith(model: 'deepseek-v4-flash'),
      ),
    },
  );
}

StudioState _stateWithAttachmentModels() {
  final state = _emptyState();
  const imageModel = ProviderModelView(
    slug: 'glm-5.3-flash',
    displayName: 'GLM-5.3-Flash',
    reasoningEfforts: ['high'],
    defaultReasoningEffort: 'high',
    inputCapabilities: [
      ModelInputCapabilityView(modality: ModelModalityView.text, sources: []),
      ModelInputCapabilityView(
        modality: ModelModalityView.image,
        sources: [ModelInputSourceView.local, ModelInputSourceView.remoteUrl],
      ),
    ],
    outputModalities: [ModelModalityView.text],
  );
  const textModel = ProviderModelView(
    slug: 'glm-5.3',
    displayName: 'GLM-5.3',
    reasoningEfforts: ['high'],
    defaultReasoningEffort: 'high',
    inputCapabilities: [
      ModelInputCapabilityView(modality: ModelModalityView.text, sources: []),
    ],
    outputModalities: [ModelModalityView.text],
  );
  return state.copyWith(
    settingsState: SettingsStateSnapshot.fromState(
      state: _testReady(
        const SettingsStateData(
          providers: [
            ProviderSettingsView(
              id: 'zhipu',
              templateKind: 'zhipu-coding-plan',
              name: 'Zhipu',
              baseUrl: 'https://open.bigmodel.cn/api/paas/v4',
              defaultModel: 'glm-5.3-flash',
              models: [],
              customModels: [imageModel, textModel],
              status: 'ready',
              usageLabel: '2 models',
            ),
          ],
          roles: [
            RoleSettingsView(
              key: 'executor',
              providerId: 'zhipu',
              model: 'glm-5.3-flash',
              effort: 'high',
            ),
            RoleSettingsView(
              key: 'planner',
              providerId: 'zhipu',
              model: 'glm-5.3-flash',
              effort: 'high',
            ),
          ],
        ),
      ),
    ),
    workspacesByThread: {
      state.selectedThreadId!: state.selectedWorkspace!.copyWith(
        runtime: state.runtime.copyWith(model: 'glm-5.3-flash'),
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
    projectDirectory: ProjectDirectoryState.fromState(
      state: _testReady(projects),
    ),
    threadDirectory: ThreadDirectoryWindow(threads: threads),
    taskDirectory: TaskDirectoryState.fromState(
      state: _testReady([
        for (final entry in tasksByRootThread.entries)
          TaskDirectoryEntryView(rootThreadId: entry.key, task: entry.value),
      ]),
    ),
    agentDirectory: AgentDirectoryState.fromState(state: _testReady(agents)),
    settingsState: SettingsStateSnapshot.fromState(
      state: _testReady(
        SettingsStateData(
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
      ),
    ),
    recoveryState: RecoveryStateSnapshot.fromState(
      state: _testReady(recoveryIssues),
    ),
    mcpState: McpStateSnapshot.fromState(
      state: _testReady(const McpStateData()),
    ),
    lspState: LspStateSnapshot.fromState(
      state: _testReady(
        const LspStateData(
          servers: [
            LspServerStateView(
              id: 'rust-analyzer',
              displayName: 'rust-analyzer',
              state: LspAvailableState(
                checkedAt: 0,
                diagnosticCount: 0,
                activity: LspIndexingActivity(
                  title: 'Roots Scanned',
                  message: '166/408',
                  percentage: 40,
                ),
              ),
            ),
          ],
        ),
      ),
    ),
    skillsByProject: skillsByProject,
    providerUsageState: ProviderUsageStateSnapshot.fromState(
      state: _testReady(ProviderUsageStateData(usages: providerUsages)),
    ),
    updaterState: UpdaterStateSnapshot.idle(
      revision: 0,
      updatedAt: DateTime.fromMillisecondsSinceEpoch(0),
    ),
    workspacesByThread: workspacesByThread,
    workspaceUiByThread: workspaceUiByThread,
    providerCatalog: providerCatalog,
    selectedProjectId: selectedProjectId,
    selectedThreadId: selectedThreadId,
  );
}

ObservedResource<T> _testReady<T>(T value, {int revision = 1}) {
  return ReadyObservedResource(
    revision: revision,
    updatedAt: 0,
    lastCheckedAt: null,
    value: value,
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
    settingsState: SettingsStateSnapshot.fromState(
      state: _testReady(
        SettingsStateData(
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
        revision: current.revision,
      ),
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
  Object createdAt = 1,
  DateTime? updatedAt,
  DateTime? completedAt,
  String? error,
  TimelineToolPart? tool,
  TimelineSkillActivation? skill,
  List<String> reasoningSummary = const [],
  List<String> reasoningContent = const [],
  String? filePath,
  String? mediaType,
  List<ThreadAttachmentView> attachments = const [],
  ThreadContextDisposition contextDisposition = ThreadContextDisposition.active,
}) {
  final timestamp = createdAt is DateTime
      ? createdAt
      : _fixtureDate(createdAt as int);
  final terminalAt = completedAt ?? timestamp;
  return ThreadItemView(
    id: id,
    threadId: threadId,
    turnId: turnId,
    ordinal: ordinal,
    revision: revision,
    createdAt: timestamp,
    updatedAt: updatedAt ?? timestamp,
    state: switch (kind) {
      ThreadItemKind.userMessage => ThreadTextItemStateView(
        channel: ThreadTextChannel.user,
        text: text,
        attachments: attachments,
        lifecycle: _contentLifecycleFixture(status, terminalAt, error: error),
      ),
      ThreadItemKind.agentMessage => ThreadTextItemStateView(
        channel: channel == AgentMessageChannel.commentary
            ? ThreadTextChannel.commentary
            : ThreadTextChannel.finalAnswer,
        text: text,
        attachments: attachments,
        lifecycle: _contentLifecycleFixture(status, terminalAt, error: error),
      ),
      ThreadItemKind.reasoning => ThreadThinkingItemStateView(
        summary: reasoningSummary,
        content: reasoningContent,
        lifecycle: _contentLifecycleFixture(status, terminalAt, error: error),
      ),
      ThreadItemKind.plan => ThreadPlanItemStateView(
        content: text,
        lifecycle: _contentLifecycleFixture(status, terminalAt, error: error),
      ),
      ThreadItemKind.toolCall => _toolItemFixture(
        tool ?? const TimelineToolPart(toolCallId: 'tool', name: 'tool'),
        status,
        terminalAt,
        error,
      ),
      ThreadItemKind.file => ThreadFileItemStateView(
        filePath ?? '',
        mediaType,
        terminalAt,
      ),
      ThreadItemKind.skill => ThreadSkillItemStateView(
        name: skill?.name ?? text,
        source: skill?.source ?? 'system',
        providerId: skill?.providerId ?? 'local-filesystem',
        resourceBase:
            skill?.resourceBase ??
            const SkillResourceBaseView(SkillResourceBaseKind.directory, ''),
        cause:
            skill?.cause ??
            const SkillActivationCauseView(
              SkillActivationCauseKind.tool,
              'skill-view',
            ),
        activatedAt: skill?.activatedAt ?? terminalAt,
      ),
      ThreadItemKind.agent ||
      ThreadItemKind.turn ||
      ThreadItemKind.inference ||
      ThreadItemKind.contextCompaction => throw ArgumentError.value(
        kind,
        'kind',
        'fixture requires an explicit canonical state for this item kind',
      ),
    },
    contextDisposition: contextDisposition,
  );
}

ThreadContentLifecycleView _contentLifecycleFixture(
  String status,
  DateTime terminalAt, {
  String? error,
}) {
  return switch (status) {
    'started' || 'streaming' || 'running' => const StreamingThreadContentView(),
    'completed' || 'succeeded' => CompletedThreadContentView(terminalAt),
    'failed' ||
    'budgetLimited' => FailedThreadContentView(terminalAt, error ?? status),
    'cancelled' ||
    'interrupted' ||
    'denied' => CancelledThreadContentView(terminalAt, error ?? status),
    _ => throw ArgumentError.value(status, 'status', 'unknown fixture state'),
  };
}

ThreadToolItemStateView _toolItemFixture(
  TimelineToolPart tool,
  String status,
  DateTime terminalAt,
  String? error,
) {
  final invocation = ThreadToolInvocationView(
    toolCallId: tool.toolCallId,
    callId: tool.callId,
    providerItemId: tool.providerItemId,
    name: tool.name,
    arguments: tool.arguments,
    workingDirectory: tool.workingDirectory,
  );
  final output = ThreadToolOutputView(
    result: tool.result ?? '',
    outputArtifacts: tool.outputArtifacts,
    exitCode: tool.exitCode,
  );
  final lifecycle = switch (status) {
    'started' => const StartedThreadToolView(),
    'streaming' => const StreamingThreadToolView(),
    'awaitingApproval' => const AwaitingApprovalThreadToolView(),
    'approved' => const ApprovedThreadToolView(),
    'running' => RunningThreadToolView(tool.result ?? ''),
    'succeeded' => SucceededThreadToolView(terminalAt, output),
    'failed' => FailedThreadToolView(
      terminalAt,
      ThreadToolFailureView(
        kind: tool.timedOut
            ? ThreadToolFailureKindView.timedOut
            : ThreadToolFailureKindView.execution,
        message: error ?? tool.result ?? status,
      ),
      output,
    ),
    'denied' => DeniedThreadToolView(
      terminalAt,
      tool.denialReason ?? error ?? 'denied',
    ),
    'cancelled' => CancelledThreadToolView(terminalAt, error ?? status),
    _ => throw ArgumentError.value(status, 'status', 'unknown tool state'),
  };
  return ThreadToolItemStateView(invocation: invocation, lifecycle: lifecycle);
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

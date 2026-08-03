part of '../widget_test.dart';

class _FakeStudioApi implements StudioApi {
  _FakeStudioApi(
    this.initialState, {
    List<ProviderUsageView>? providerUsages,
    this.providerCatalog = _testProviderCatalog,
  }) : providerUsages = providerUsages ?? _defaultProviderUsages;

  final StudioState initialState;
  final List<ProviderUsageView> providerUsages;
  final ProviderCatalogView providerCatalog;
  final _global = StreamController<Object>.broadcast();
  StreamController<SessionStreamFrame> _session =
      StreamController<SessionStreamFrame>.broadcast();
  final Map<String, StudioState> sessionStates = {};
  final Map<String, StudioState> selectProjectStates = {};
  final Map<String, StudioState> archiveProjectStates = {};
  final List<String> loadedSessionIds = [];
  final List<({String sessionId, int? afterSequence})> sessionSubscriptions =
      [];
  int createSessionCount = 0;
  int bootstrapCount = 0;
  Object? bootstrapError;
  String? openedProjectPath;
  String? selectedProjectRequest;
  String? archivedProjectId;
  String? archiveSelectedProjectId;
  String? archivedSessionId;
  StudioMode? sessionModeUpdate;
  _RoleUpdate? roleUpdate;
  Map<String, Object?>? savedProviderSettings;
  Map<String, Object?>? savedInstructionsSettings;
  Map<String, Object?>? savedSkillsSettings;
  Map<String, Object?>? savedMcpSettings;
  Map<String, Object?>? savedGeneralSettings;
  WebSearchSettingsCommand? savedWebSearchSettings;
  PermissionMode? savedPermissionMode;
  String? resolvedInteractionId;
  Map<String, Object?>? resolvedInteraction;
  Object? resolveInteractionError;
  int resolveInteractionCount = 0;
  String? discoverProjectId;
  List<String> discoveredSkills = const [];
  int loadProviderUsagesCount = 0;
  Completer<List<ProviderUsageView>>? blockedProviderUsageLoad;
  Completer<void>? blockedSessionCancellation;
  int submitPromptCount = 0;
  final List<({String sessionId, String prompt})> submittedPrompts = [];
  Completer<SubmitPromptReceipt>? blockedPromptSubmit;
  Exception? submitPromptError;
  int resumeTaskCount = 0;
  final List<String> resumedTaskSessionIds = [];
  Completer<SubmitPromptReceipt>? blockedTaskResume;
  Exception? resumeTaskError;
  String? submitReceiptSessionId;
  String submitTurnId = 'turn-1';
  final Map<String, RecoveryCleanupPreview> recoveryPreviews = {};
  final Map<String, RecoveryCleanupPreview> projectCleanupPreviews = {};
  int previewProjectCleanupCount = 0;
  Object? previewProjectCleanupError;
  StudioState? projectCleanupState;
  Object? projectCleanupError;
  String? cleanedProjectId;
  String? projectCleanupExpectedRevision;
  int previewRecoveryIssueCleanupCount = 0;
  Object? previewRecoveryIssueCleanupError;
  StudioState? recoveryCleanupState;
  Object? recoveryCleanupError;
  String? cleanedRecoveryIssueId;
  String? cleanupExpectedRevision;

  void emitGlobal(StudioBridgeEvent event) => _global.add(event);

  void emitSession(StudioBridgeEvent event) =>
      _session.add(SessionEventFrame(event: event));

  void emitSessionFrame(SessionStreamFrame frame) => _session.add(frame);

  Future<void> closeSessionStream() async {
    final closed = _session;
    _session = StreamController<SessionStreamFrame>.broadcast();
    await closed.close();
  }

  @override
  Future<ProviderCatalogView> loadProviderCatalog() async => providerCatalog;

  @override
  Future<StudioState> bootstrap() async {
    bootstrapCount += 1;
    if (bootstrapError case final error?) {
      throw error;
    }
    return initialState;
  }

  @override
  Future<StudioState> openProject(String path) async {
    openedProjectPath = path;
    return initialState;
  }

  @override
  Future<StudioState> selectProject(String projectId) async {
    selectedProjectRequest = projectId;
    return selectProjectStates[projectId] ?? initialState;
  }

  @override
  Future<StudioState> archiveProject(
    String projectId, {
    String? selectedProjectId,
  }) async {
    archivedProjectId = projectId;
    archiveSelectedProjectId = selectedProjectId;
    return archiveProjectStates[projectId] ?? initialState;
  }

  @override
  Future<RecoveryCleanupPreview> previewProjectCleanup(String projectId) async {
    previewProjectCleanupCount += 1;
    if (previewProjectCleanupError case final error?) {
      throw error;
    }
    return projectCleanupPreviews[projectId] ??
        RecoveryCleanupPreview(
          issueId: 'project-cleanup-$projectId',
          expectedRevision: 'revision-$projectId',
          scope: RecoveryIssueScope.project,
          projectId: projectId,
          detail: 'Project cleanup preview',
          resources: const [],
        );
  }

  @override
  Future<StudioState> cleanupProject(
    String projectId,
    String expectedRevision, {
    String? selectedProjectId,
  }) async {
    if (projectCleanupError case final error?) {
      throw error;
    }
    cleanedProjectId = projectId;
    projectCleanupExpectedRevision = expectedRevision;
    return projectCleanupState ?? initialState;
  }

  @override
  Future<StudioState> createSession(String projectId, {String? title}) async {
    createSessionCount += 1;
    return initialState;
  }

  @override
  Future<StudioState> archiveSession(
    String sessionId, {
    String? selectedSessionId,
  }) async {
    archivedSessionId = sessionId;
    return initialState;
  }

  @override
  Future<RecoveryCleanupPreview> previewRecoveryIssueCleanup(
    String issueId,
  ) async {
    previewRecoveryIssueCleanupCount += 1;
    if (previewRecoveryIssueCleanupError case final error?) {
      throw error;
    }
    return recoveryPreviews[issueId] ??
        RecoveryCleanupPreview(
          issueId: issueId,
          expectedRevision: 'revision-$issueId',
          scope: RecoveryIssueScope.session,
          detail: 'Recovery cleanup preview',
          resources: const [],
        );
  }

  @override
  Future<StudioState> cleanupRecoveryIssue(
    String issueId,
    String expectedRevision, {
    String? selectedProjectId,
    String? selectedSessionId,
  }) async {
    if (recoveryCleanupError case final error?) {
      throw error;
    }
    cleanedRecoveryIssueId = issueId;
    cleanupExpectedRevision = expectedRevision;
    return recoveryCleanupState ?? initialState;
  }

  @override
  Future<StudioSession> setSessionMode(
    String sessionId,
    StudioMode mode,
  ) async {
    sessionModeUpdate = mode;
    return initialState.sessions
        .firstWhere((session) => session.id == sessionId)
        .copyWith(mode: mode);
  }

  @override
  Future<StudioState> setModelRole({
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
    String? selectedSessionId,
  }) async {
    roleUpdate = _RoleUpdate(
      roleKey: roleKey,
      providerId: providerId,
      model: model,
      effort: effort,
    );
    return initialState.copyWith(
      roles: [
        for (final role in initialState.roles)
          role.key == roleKey
              ? RoleSettingsView(
                  key: role.key,
                  providerId: providerId,
                  model: model,
                  effort: effort ?? role.effort,
                )
              : role,
      ],
    );
  }

  @override
  Future<InteractionResolutionResult> resolveInteraction(
    String interactionId,
    InteractionResolutionCommand resolution,
  ) async {
    resolveInteractionCount += 1;
    if (resolveInteractionError case final error?) {
      throw error;
    }
    resolvedInteractionId = interactionId;
    resolvedInteraction = _interactionResolutionJson(resolution);
    final sessions =
        resolution is PlanConfirmationResolutionCommand &&
            resolution.decision ==
                PlanConfirmationDecision.implementFreshContext
        ? [
            for (final session in initialState.sessions)
              session.id == initialState.selectedSessionId
                  ? session.copyWith(mode: StudioMode.task)
                  : session,
          ]
        : initialState.sessions;
    return InteractionResolutionResult(
      sessionId: initialState.selectedSessionId ?? '',
      interactionId: interactionId,
      status: 'resolved',
      sessions: sessions,
    );
  }

  @override
  Future<void> stopPrompt(String sessionId) async {}

  @override
  Stream<Object> subscribeProductEvents() => _global.stream;

  @override
  Stream<SessionStreamFrame> subscribeSessionEvents(
    String sessionId, {
    int? afterSequence,
  }) {
    sessionSubscriptions.add((
      sessionId: sessionId,
      afterSequence: afterSequence,
    ));
    final blockedCancellation = blockedSessionCancellation;
    if (blockedCancellation == null) {
      return _session.stream;
    }
    late final StreamController<SessionStreamFrame> controller;
    StreamSubscription<SessionStreamFrame>? forwarding;
    controller = StreamController<SessionStreamFrame>(
      onListen: () {
        forwarding = _session.stream.listen(
          controller.add,
          onError: controller.addError,
          onDone: controller.close,
        );
      },
      onCancel: () async {
        await forwarding?.cancel();
        await blockedCancellation.future;
      },
    );
    return controller.stream;
  }

  @override
  Future<SubmitPromptReceipt> submitPrompt(
    String sessionId,
    String prompt,
    List<String> attachmentIds,
  ) async {
    submitPromptCount += 1;
    submittedPrompts.add((sessionId: sessionId, prompt: prompt));
    if (submitPromptError case final error?) {
      throw error;
    }
    final blocked = blockedPromptSubmit;
    if (blocked != null) {
      return blocked.future;
    }
    return SubmitPromptReceipt(
      sessionId: submitReceiptSessionId ?? sessionId,
      turnId: submitTurnId,
      cursor: submitPromptCount,
    );
  }

  @override
  Future<SubmitPromptReceipt> resumeTask(String sessionId) async {
    resumeTaskCount += 1;
    resumedTaskSessionIds.add(sessionId);
    if (resumeTaskError case final error?) {
      throw error;
    }
    final blocked = blockedTaskResume;
    if (blocked != null) {
      return blocked.future;
    }
    return SubmitPromptReceipt(
      sessionId: submitReceiptSessionId ?? sessionId,
      turnId: submitTurnId,
      cursor: resumeTaskCount,
    );
  }

  @override
  Future<StudioState> saveRuntimePermissionMode(PermissionMode mode) async {
    savedPermissionMode = mode;
    return initialState.copyWith(permissionMode: mode);
  }

  @override
  Future<List<String>> listDiscoveredSkills(String projectId) async {
    discoverProjectId = projectId;
    return discoveredSkills;
  }

  @override
  Future<StudioState> saveProviderSettings(
    ProviderSettingsCommand command,
  ) async {
    final settings = _providerSettingsCommandJson(command);
    savedProviderSettings = settings;
    return initialState.copyWith(
      defaultProviderId: settings['defaultProviderId'] as String?,
      providers: [
        for (final value in settings['providers'] as List<Object?>)
          _providerFromSettings(value),
      ],
    );
  }

  @override
  Future<StudioState> saveInstructionsSettings(
    InstructionsSettingsCommand command,
  ) async {
    final settings = <String, Object?>{
      'baseOverride': command.baseOverride,
      'developer': command.developer,
      'user': command.user,
      'projectDocMaxBytes': command.projectDocMaxBytes,
      'projectDocFallbackFilenames': command.projectDocFallbackFilenames,
    };
    savedInstructionsSettings = settings;
    return initialState.copyWith(
      instructions: InstructionsSettingsView(
        baseOverride: settings['baseOverride'] as String? ?? '',
        developer: settings['developer'] as String? ?? '',
        user: settings['user'] as String? ?? '',
        projectDocMaxBytes: settings['projectDocMaxBytes'] as int? ?? 65536,
        projectDocFallbackFilenames: [
          for (final value
              in settings['projectDocFallbackFilenames'] as List<Object?>? ??
                  const <Object?>[])
            value.toString(),
        ],
      ),
    );
  }

  @override
  Future<StudioState> saveSkillsSettings(SkillsSettingsCommand command) async {
    final settings = <String, Object?>{
      'enabled': command.enabled,
      'autoLearn': command.autoLearn,
      'systemEnabled': command.systemEnabled,
      'projectDir': command.projectDir,
      'userDir': command.userDir,
      'externalDirs': command.externalDirs,
      'disabled': command.disabled,
      'autoLearnMinToolCalls': command.autoLearnMinToolCalls,
    };
    savedSkillsSettings = settings;
    return initialState.copyWith(
      skills: initialState.skills.copyWith(
        disabled: [
          for (final value
              in settings['disabled'] as List<Object?>? ?? const <Object?>[])
            value.toString(),
        ],
      ),
    );
  }

  @override
  Future<StudioState> saveMcpSettings(McpSettingsCommand command) async {
    final settings = <String, Object?>{
      'servers': [
        for (final server in command.servers)
          {
            'id': server.id,
            'enabled': server.enabled,
            'transport': server.transport,
            'endpoint': server.endpoint,
          },
      ],
    };
    savedMcpSettings = settings;
    return initialState;
  }

  @override
  Future<StudioState> saveGeneralSettings(
    GeneralSettingsCommand command,
  ) async {
    final settings = <String, Object?>{
      'followSystemTheme': command.followSystemTheme,
      'followActiveTurn': command.followActiveTurn,
      'compactTimeline': command.compactTimeline,
    };
    savedGeneralSettings = settings;
    return initialState.copyWith(
      general: GeneralSettingsView(
        followSystemTheme: settings['followSystemTheme'] as bool? ?? true,
        followActiveTurn: settings['followActiveTurn'] as bool? ?? true,
        compactTimeline: settings['compactTimeline'] as bool? ?? false,
      ),
    );
  }

  @override
  Future<StudioState> saveWebSearchSettings(
    WebSearchSettingsCommand command,
  ) async {
    savedWebSearchSettings = command;
    return initialState.copyWith(
      webSearch: initialState.webSearch.withConfiguredValues(
        configuredMode: command.mode,
        contextSize: command.contextSize,
        allowedDomains: command.allowedDomains,
        country: command.country,
        region: command.region,
        city: command.city,
        timezone: command.timezone,
      ),
    );
  }

  @override
  Future<List<ProviderUsageView>> loadProviderUsages() async {
    loadProviderUsagesCount += 1;
    final blocked = blockedProviderUsageLoad;
    return blocked == null ? providerUsages : blocked.future;
  }
}

Map<String, Object?> _providerSettingsCommandJson(
  ProviderSettingsCommand command,
) {
  return {
    'defaultProviderId': command.defaultProviderId,
    'providers': [
      for (final provider in command.providers)
        {
          'id': provider.id,
          'originalId': provider.originalId,
          'templateKind': provider.templateKind,
          'wireProtocol': provider.wireProtocol,
          'connectionMode': provider.connectionMode,
          'name': provider.name,
          'baseUrl': provider.baseUrl,
          'bearerToken': provider.secret.value ?? '',
          'capabilitySource': provider.capabilitySource,
          'hostedWebSearch': provider.hostedWebSearch,
          'standaloneWebSearch': provider.standaloneWebSearch,
          'defaultModel': provider.defaultModel,
          'customModels': [
            for (final model in provider.customModels)
              {
                'slug': model.slug,
                'displayName': model.displayName,
                'reasoningEfforts': model.reasoningEfforts,
                'baseInstructions': model.baseInstructions,
              },
          ],
        },
    ],
    'roles': [
      for (final role in command.roles)
        {
          'key': role.key,
          'provider': role.providerId,
          'model': role.model,
          'effort': role.effort,
        },
    ],
  };
}

Map<String, Object?> _interactionResolutionJson(
  InteractionResolutionCommand resolution,
) {
  return switch (resolution) {
    UserInputResolutionCommand(:final answers) => {
      'type': 'userInput',
      'answers': {
        for (final answer in answers)
          answer.questionId: {'answers': answer.answers},
      },
    },
    ToolApprovalResolutionCommand(:final decision, :final reason) => {
      'type': 'toolApproval',
      'decision': decision.name,
      'reason': ?reason,
    },
    PlanConfirmationResolutionCommand(
      :final decision,
      :final content,
      :final reason,
    ) =>
      {
        'type': 'planConfirmation',
        'decision': decision.name,
        'content': ?content,
        'reason': ?reason,
      },
  };
}

const _defaultProviderUsages = [
  ProviderUsageView(
    providerId: 'deepseek',
    updatedAt: 1,
    status: 'ready',
    usageKind: 'deepseekBalance',
    balance: DeepSeekBalanceUsageView(
      isAvailable: true,
      balances: [
        DeepSeekBalanceInfoView(
          currency: 'CNY',
          totalBalance: '88.00',
          grantedBalance: '8.00',
          toppedUpBalance: '80.00',
        ),
      ],
    ),
  ),
];

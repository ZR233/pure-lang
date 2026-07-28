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
  WebSearchSettingsView? savedWebSearchSettings;
  PermissionMode? savedPermissionMode;
  String? resolvedInteractionId;
  Map<String, Object?>? resolvedInteraction;
  Object? resolveInteractionError;
  int resolveInteractionCount = 0;
  String? discoverProjectId;
  List<String> discoveredSkills = const [];
  int loadProviderUsagesCount = 0;
  Completer<List<ProviderUsageView>>? blockedProviderUsageLoad;
  final Map<String, RecoveryCleanupPreview> recoveryPreviews = {};
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
  Future<StudioState> openProject(String path) async => initialState;

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
  Future<void> resolveInteraction(
    String interactionId,
    Map<String, Object?> resolution,
  ) async {
    jsonEncode(resolution);
    resolveInteractionCount += 1;
    if (resolveInteractionError case final error?) {
      throw error;
    }
    resolvedInteractionId = interactionId;
    resolvedInteraction = resolution;
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
    return _session.stream;
  }

  @override
  Future<void> submitPrompt(
    String sessionId,
    String prompt,
    List<String> attachmentIds,
  ) async {}

  @override
  Future<void> saveRuntimePermissionMode(PermissionMode mode) async {
    savedPermissionMode = mode;
  }

  @override
  Future<List<String>> listDiscoveredSkills(String projectId) async {
    discoverProjectId = projectId;
    return discoveredSkills;
  }

  @override
  Future<StudioState> saveProviderSettings(
    Map<String, Object?> settings,
  ) async {
    jsonEncode(settings);
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
    Map<String, Object?> settings,
  ) async {
    jsonEncode(settings);
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
  Future<StudioState> saveSkillsSettings(Map<String, Object?> settings) async {
    jsonEncode(settings);
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
  Future<StudioState> saveMcpSettings(Map<String, Object?> settings) async {
    jsonEncode(settings);
    savedMcpSettings = settings;
    return initialState;
  }

  @override
  Future<StudioState> saveGeneralSettings(Map<String, Object?> settings) async {
    jsonEncode(settings);
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
    WebSearchSettingsView settings,
  ) async {
    savedWebSearchSettings = settings;
    return initialState.copyWith(webSearch: settings);
  }

  @override
  Future<List<ProviderUsageView>> loadProviderUsages() async {
    loadProviderUsagesCount += 1;
    final blocked = blockedProviderUsageLoad;
    return blocked == null ? providerUsages : blocked.future;
  }

  @override
  Future<void> saveStudioSettingsDraft(
    String section,
    Map<String, Object?> draft,
  ) async {}
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

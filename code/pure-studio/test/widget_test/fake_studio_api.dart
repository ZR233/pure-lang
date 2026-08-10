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
  StreamController<ThreadStreamFrame> _thread =
      StreamController<ThreadStreamFrame>.broadcast();
  final Map<String, StudioState> sessionStates = {};
  final Map<String, StudioState> selectProjectStates = {};
  final Map<String, StudioState> archiveProjectStates = {};
  final List<String> loadedSessionIds = [];
  final List<String> threadSubscriptions = [];
  final List<({String threadId, String? cursor})> historyRequests = [];
  final Map<String, Map<String?, ThreadHistoryPage>> historyPagesByThread = {};
  int bootstrapCount = 0;
  Object? bootstrapError;
  String? openedProjectPath;
  String? selectedProjectRequest;
  String? createdThreadProjectId;
  String? archivedThreadId;
  String? archiveSelectedThreadId;
  StudioState? createThreadState;
  StudioState? archiveThreadState;
  String? archivedProjectId;
  String? archiveSelectedProjectId;
  ({String threadId, StudioMode mode})? modeUpdate;
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
  int discoverCallCount = 0;
  List<String> discoveredSkills = const [];
  int loadProviderUsagesCount = 0;
  Completer<List<ProviderUsageView>>? blockedProviderUsageLoad;
  Completer<void>? blockedThreadCancellation;
  int submitPromptCount = 0;
  final List<({String threadId, String prompt})> submittedPrompts = [];
  Completer<SubmitPromptReceipt>? blockedPromptSubmit;
  Exception? submitPromptError;
  int resumeTaskCount = 0;
  final List<String> resumedTaskSessionIds = [];
  Completer<SubmitPromptReceipt>? blockedTaskResume;
  Exception? resumeTaskError;
  String? submitReceiptSessionId;
  String submitTurnId = 'turn-1';
  ({String threadId, String turnId})? interruptedTurn;
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
  StudioState? recoveryRetryState;
  Object? recoveryRetryError;
  String? retriedRecoveryIssueId;
  String? retrySelectedProjectId;
  String? retrySelectedThreadId;
  TaskRecoveryPreview? taskRecoveryPreview;
  TaskRecoveryResult? taskRecoveryResult;
  TaskRecoveryRequest? taskRecoveryRequest;
  Object? taskRecoveryPreviewError;
  Object? taskRecoveryApplyError;
  Completer<TaskRecoveryResult>? blockedTaskRecoveryApply;

  void emitGlobal(StudioBridgeEvent event) => _global.add(event);

  void emitThreadFrame(ThreadStreamFrame frame) => _thread.add(frame);

  Future<void> closeThreadStream() async {
    final closed = _thread;
    _thread = StreamController<ThreadStreamFrame>.broadcast();
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
  Future<StudioState> createThread(String projectId, {String? title}) async {
    createdThreadProjectId = projectId;
    if (createThreadState case final next?) return next;
    final now = DateTime.fromMillisecondsSinceEpoch(1);
    final thread = StudioThread(
      id: 'session-created',
      projectId: projectId,
      title: title ?? 'New Session',
      mode: StudioMode.simple,
      role: 'executor',
      createdAt: now,
      updatedAt: now,
    );
    return initialState.copyWith(
      threads: [...initialState.threads, thread],
      selectedProjectId: projectId,
      selectedThreadId: thread.id,
    );
  }

  @override
  Future<StudioState> archiveThread(
    String threadId, {
    String? selectedThreadId,
  }) async {
    archivedThreadId = threadId;
    archiveSelectedThreadId = selectedThreadId;
    if (archiveThreadState case final next?) return next;
    final threads = initialState.threads
        .where((thread) => thread.effectiveRootThreadId != threadId)
        .toList();
    final remainingIds = threads.map((thread) => thread.id).toSet();
    final nextSelected = remainingIds.contains(selectedThreadId)
        ? selectedThreadId
        : threads.where((thread) => thread.isRoot).firstOrNull?.id;
    return initialState.copyWith(
      threads: threads,
      selectedThreadId: nextSelected,
    );
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
          scope: RecoveryIssueScope.thread,
          detail: 'Recovery cleanup preview',
          resources: const [],
        );
  }

  @override
  Future<StudioState> cleanupRecoveryIssue(
    String issueId,
    String expectedRevision, {
    String? selectedProjectId,
    String? selectedThreadId,
  }) async {
    if (recoveryCleanupError case final error?) {
      throw error;
    }
    cleanedRecoveryIssueId = issueId;
    cleanupExpectedRevision = expectedRevision;
    return recoveryCleanupState ?? initialState;
  }

  @override
  Future<StudioState> retryRecoveryIssue(
    String issueId, {
    String? selectedProjectId,
    String? selectedThreadId,
  }) async {
    if (recoveryRetryError case final error?) {
      throw error;
    }
    retriedRecoveryIssueId = issueId;
    retrySelectedProjectId = selectedProjectId;
    retrySelectedThreadId = selectedThreadId;
    return recoveryRetryState ?? initialState;
  }

  @override
  Future<TaskRecoveryPreview> previewTaskRecovery(String rootThreadId) async {
    if (taskRecoveryPreviewError case final error?) throw error;
    return taskRecoveryPreview ??
        (throw StateError('Missing Task recovery preview for $rootThreadId'));
  }

  @override
  Future<TaskRecoveryResult> applyTaskRecovery(
    TaskRecoveryRequest request,
  ) async {
    taskRecoveryRequest = request;
    if (taskRecoveryApplyError case final error?) throw error;
    final blocked = blockedTaskRecoveryApply;
    if (blocked != null) return blocked.future;
    return taskRecoveryResult ??
        (throw StateError('Missing Task recovery result'));
  }

  @override
  Future<StudioState> setModelRole({
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
    String? selectedThreadId,
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
  Future<StudioState> setThreadMode({
    required String threadId,
    required StudioMode mode,
  }) async {
    final thread = initialState.threads
        .where((candidate) => candidate.id == threadId)
        .firstOrNull;
    if (thread == null) {
      throw StateError('unknown fake thread $threadId');
    }
    if (!thread.isRoot) {
      throw StateError('only a root Thread can change mode');
    }
    if (initialState.tasksByRootThread[threadId]?.isActive ?? false) {
      throw StateError('thread mode cannot change while a task is active');
    }
    modeUpdate = (threadId: threadId, mode: mode);
    final updated = thread.copyWith(
      mode: mode,
      role: mode == StudioMode.task ? 'planner' : 'executor',
    );
    final workspace = initialState.workspacesByThread[threadId];
    return initialState.copyWith(
      threads: [
        for (final candidate in initialState.threads)
          candidate.id == threadId ? updated : candidate,
      ],
      workspacesByThread: workspace == null
          ? initialState.workspacesByThread
          : {
              ...initialState.workspacesByThread,
              threadId: workspace.copyWith(thread: updated),
            },
    );
  }

  @override
  Future<PendingInteraction> respondInteraction(
    String interactionId,
    InteractionResolutionCommand resolution,
  ) async {
    resolveInteractionCount += 1;
    if (resolveInteractionError case final error?) {
      throw error;
    }
    resolvedInteractionId = interactionId;
    resolvedInteraction = _interactionResolutionJson(resolution);
    return PendingInteraction(
      id: interactionId,
      threadId: initialState.selectedThreadId ?? '',
      kind: InteractionKind.userInput,
      title: '',
      body: '',
    );
  }

  @override
  Stream<Object> subscribeProductEvents() => _global.stream;

  @override
  Stream<ThreadStreamFrame> subscribeThread(String threadId) {
    threadSubscriptions.add(threadId);
    final blockedCancellation = blockedThreadCancellation;
    if (blockedCancellation == null) {
      return _thread.stream;
    }
    late final StreamController<ThreadStreamFrame> controller;
    StreamSubscription<ThreadStreamFrame>? forwarding;
    controller = StreamController<ThreadStreamFrame>(
      onListen: () {
        forwarding = _thread.stream.listen(
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
  Future<ThreadHistoryPage> listThreadTurns(
    String threadId, {
    String? cursor,
    int limit = 50,
  }) async {
    historyRequests.add((threadId: threadId, cursor: cursor));
    return historyPagesByThread[threadId]?[cursor] ??
        const ThreadHistoryPage(items: [], nextCursor: null);
  }

  @override
  Future<SubmitPromptReceipt> startTurn(
    String threadId,
    String prompt,
    List<String> attachmentIds,
  ) => _submitTurn(threadId, prompt);

  @override
  Future<SubmitPromptReceipt> steerTurn(
    String threadId,
    String prompt,
    List<String> attachmentIds,
  ) => _submitTurn(threadId, prompt);

  Future<SubmitPromptReceipt> _submitTurn(
    String threadId,
    String prompt,
  ) async {
    submitPromptCount += 1;
    submittedPrompts.add((threadId: threadId, prompt: prompt));
    if (submitPromptError case final error?) {
      throw error;
    }
    final blocked = blockedPromptSubmit;
    if (blocked != null) {
      return blocked.future;
    }
    return SubmitPromptReceipt(
      threadId: submitReceiptSessionId ?? threadId,
      turnId: submitTurnId,
      cursor: submitPromptCount,
    );
  }

  @override
  Future<void> interruptTurn(String threadId, String turnId) async {
    interruptedTurn = (threadId: threadId, turnId: turnId);
  }

  @override
  Future<StudioState> saveRuntimePermissionMode(PermissionMode mode) async {
    savedPermissionMode = mode;
    return initialState.copyWith(permissionMode: mode);
  }

  @override
  Future<List<String>> listDiscoveredSkills(String projectId) async {
    discoverProjectId = projectId;
    discoverCallCount += 1;
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
          'name': provider.name,
          'baseUrl': provider.baseUrl,
          'bearerToken': provider.secret.value ?? '',
          'capabilitySource': provider.capabilitySource,
          'hostedWebSearch': provider.hostedWebSearch,
          'standaloneWebSearch': provider.standaloneWebSearch,
          'promptCacheDialect': provider.promptCacheDialect,
          'responsesToolSearch': provider.responsesToolSearch,
          'responsesProgrammaticToolCalling':
              provider.responsesProgrammaticToolCalling,
          'defaultModel': provider.defaultModel,
          'customModels': [
            for (final model in provider.customModels)
              {
                'slug': model.slug,
                'displayName': model.displayName,
                'reasoningEfforts': model.reasoningEfforts,
                'baseInstructions': model.baseInstructions,
                'wireProtocol': model.wireProtocol,
                'supportedConnectionModes': model.supportedConnectionModes,
                'defaultConnectionMode': model.defaultConnectionMode,
              },
          ],
          'modelConnectionModes': [
            for (final model in provider.modelConnectionModes)
              {'slug': model.slug, 'connectionMode': model.connectionMode},
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

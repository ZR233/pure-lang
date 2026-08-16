part of 'studio_api.dart';

abstract class StudioApi {
  Future<ProviderCatalogView> loadProviderCatalog();
  Future<StudioState> readStudioState();
  Future<ThreadDirectoryPage> listThreadsPage({String? cursor, int limit = 50});
  Future<void> activateProject(String projectId);
  Future<StudioProject> openProject(String path);
  Future<StudioThread> createThread(String projectId, {String? title});
  Future<StudioThread> archiveThread(String threadId);
  Future<void> archiveProject(String projectId);
  Future<RecoveryCleanupPreview> previewProjectCleanup(String projectId);
  Future<void> cleanupProject(String projectId, String expectedRevision);
  Future<RecoveryCleanupPreview> previewRecoveryIssueCleanup(String issueId);
  Future<void> cleanupRecoveryIssue(String issueId, String expectedRevision);
  Future<void> retryRecoveryIssue(String issueId);
  Future<TaskRecoveryPreview> previewTaskRecovery(String rootThreadId);
  Future<TaskRecoveryResult> applyTaskRecovery(TaskRecoveryRequest request);
  Future<SettingsStateSnapshot> setModelRole({
    required int expectedSettingsRevision,
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
  });
  Future<void> setThreadMode({
    required String threadId,
    required StudioMode mode,
  });
  Stream<Object> subscribeProductEvents();
  Stream<ThreadStreamFrame> subscribeThread(String threadId);
  Stream<StudioShutdownProgress> subscribeShutdownProgress();
  Future<void> shutdownRuntime();

  /// 读取线程快照；`historyCursor` 是快照窗口之外的回源锚点（Turn id）。
  Future<({ThreadWorkspace workspace, String? historyCursor})>
  readThreadSnapshot(String threadId);
  Future<ThreadHistoryPage> listThreadTurns(
    String threadId, {
    String? cursor,
    int limit = 50,
  });
  Future<SubmitPromptReceipt> startTurn(
    String threadId,
    String prompt,
    List<String> attachmentIds,
  );
  Future<SubmitPromptReceipt> steerTurn(
    String threadId,
    String prompt,
    List<String> attachmentIds,
  );
  Future<void> interruptTurn(String threadId, String turnId);
  Future<PendingInteraction> respondInteraction(
    String interactionId,
    InteractionResolutionCommand resolution,
  );
  Future<SettingsStateSnapshot> saveRuntimePermissionMode(
    int expectedSettingsRevision,
    PermissionMode mode,
  );
  Future<SettingsStateSnapshot> saveProviderSettings(
    int expectedSettingsRevision,
    ProviderSettingsCommand command,
  );
  Future<SettingsStateSnapshot> saveInstructionsSettings(
    int expectedSettingsRevision,
    InstructionsSettingsCommand command,
  );
  Future<SettingsStateSnapshot> saveSkillsSettings(
    int expectedSettingsRevision,
    SkillsSettingsCommand command,
  );
  Future<SettingsStateSnapshot> saveMcpSettings(
    int expectedSettingsRevision,
    McpSettingsCommand command,
  );
  Future<SettingsStateSnapshot> saveGeneralSettings(
    int expectedSettingsRevision,
    GeneralSettingsCommand command,
  );
  Future<SettingsStateSnapshot> saveWebSearchSettings(
    int expectedSettingsRevision,
    WebSearchSettingsCommand command,
  );
  Future<ProviderUsageStateSnapshot> checkProviderUsage();
  Future<SkillsStateSnapshot> readSkillsState(String projectId);
  Future<SkillsStateSnapshot> discoverSkills(String projectId);
  Future<McpStateSnapshot> readMcpState();
  Future<McpStateSnapshot> resetMcpServer(String serverId);
  Future<McpStateSnapshot> resetAllMcp();
  Future<LspStateSnapshot> readLspState();
  Future<LspStateSnapshot> probeLspServer(String projectId);
  Future<LspStateSnapshot> repairLspServer(String projectId, String serverId);
  Future<LspStateSnapshot> resetLspServer(String projectId, String serverId);
  Future<LspStateSnapshot> resetLspWorkspace(String projectId);
}

class FrbStudioApi implements StudioApi {
  static Future<void>? _initFuture;
  static Future<void>? _shutdownFuture;
  static Future<void> Function()? _initializationOverrideForTesting;
  static bool _rustInitialized = false;
  ProviderCatalogView? _providerCatalogCache;

  static Future<void> ensureReady() => _ensureReady();

  @visibleForTesting
  static void debugOverrideInitialization(
    Future<void> Function()? initialization,
  ) {
    _initFuture = null;
    _shutdownFuture = null;
    _initializationOverrideForTesting = initialization;
  }

  static Future<void> _ensureReady() {
    if (_shutdownFuture != null) {
      return Future<void>.error(
        _studioFailure(StateError('Studio runtime is shutting down')),
      );
    }
    final existing = _initFuture;
    if (existing != null) {
      return existing;
    }
    late final Future<void> attempt;
    attempt = () async {
      try {
        final initializationOverride = _initializationOverrideForTesting;
        if (initializationOverride != null) {
          await initializationOverride();
        } else {
          await RustLib.init();
          _rustInitialized = true;
          await frb.startStudioRuntime();
        }
      } catch (error, stackTrace) {
        if (identical(_initFuture, attempt)) {
          _initFuture = null;
        }
        Error.throwWithStackTrace(_studioFailure(error), stackTrace);
      }
    }();
    _initFuture = attempt;
    return attempt;
  }

  static Future<void> shutdownAndDispose() {
    return _shutdownFuture ??= _shutdownAndDispose();
  }

  static Future<void> _shutdownAndDispose() async {
    final initialization = _initFuture;
    if (initialization != null) {
      try {
        await initialization;
      } on Object {
        // A partial initialization may still own the native runtime.
      }
    }
    if (!_rustInitialized) return;
    try {
      await frb.shutdownRuntime();
    } on Object {
      // Process teardown is best effort; Rust diagnostics retain the cause.
    } finally {
      RustLib.dispose();
      _rustInitialized = false;
      _initFuture = null;
    }
  }

  @override
  Future<ProviderCatalogView> loadProviderCatalog() async {
    final cached = _providerCatalogCache;
    if (cached != null) return cached;
    await _ensureReady();
    final catalog = providerCatalogFromFrb(
      await _bridgeCall(frb.loadProviderCatalog),
    );
    _providerCatalogCache = catalog;
    return catalog;
  }

  @override
  Future<StudioState> readStudioState() async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(await _bridgeCall(frb.readStudioState));
  }

  @override
  Future<ThreadDirectoryPage> listThreadsPage({
    String? cursor,
    int limit = 50,
  }) async {
    await _ensureReady();
    final page = await _bridgeCall(
      () => frb.listThreadsPage(
        request: frb.BridgeListThreadsPageRequest(
          cursor: cursor,
          limit: limit.clamp(1, 100),
        ),
      ),
    );
    return ThreadDirectoryPage(
      threads: page.threads.map(_threadFromFrb).toList(),
      nextCursor: page.nextCursor,
    );
  }

  @override
  Future<StudioProject> openProject(String path) async {
    await _ensureReady();
    return _projectFromFrb(
      await _bridgeCall(() => frb.openProject(path: path)),
    );
  }

  @override
  Future<void> activateProject(String projectId) async {
    await _ensureReady();
    await _bridgeCall(() => frb.activateProject(projectId: projectId));
  }

  @override
  Future<StudioThread> createThread(String projectId, {String? title}) async {
    await _ensureReady();
    return _threadFromFrb(
      await _bridgeCall(
        () => frb.createThread(projectId: projectId, title: title),
      ),
    );
  }

  @override
  Future<StudioThread> archiveThread(String threadId) async {
    await _ensureReady();
    return _threadFromFrb(
      await _bridgeCall(() => frb.archiveThread(threadId: threadId)),
    );
  }

  @override
  Future<void> archiveProject(String projectId) async {
    await _ensureReady();
    await _bridgeCall(() => frb.archiveProject(projectId: projectId));
  }

  @override
  Future<RecoveryCleanupPreview> previewProjectCleanup(String projectId) async {
    await _ensureReady();
    return _recoveryCleanupPreviewFromFrb(
      await _bridgeCall(() => frb.previewProjectCleanup(projectId: projectId)),
    );
  }

  @override
  Future<void> cleanupProject(String projectId, String expectedRevision) async {
    await _ensureReady();
    await _bridgeCall(
      () => frb.cleanupProject(
        projectId: projectId,
        expectedRevision: expectedRevision,
      ),
    );
  }

  @override
  Future<RecoveryCleanupPreview> previewRecoveryIssueCleanup(
    String issueId,
  ) async {
    await _ensureReady();
    return _recoveryCleanupPreviewFromFrb(
      await _bridgeCall(
        () => frb.previewRecoveryIssueCleanup(issueId: issueId),
      ),
    );
  }

  @override
  Future<void> cleanupRecoveryIssue(
    String issueId,
    String expectedRevision,
  ) async {
    await _ensureReady();
    await _bridgeCall(
      () => frb.cleanupRecoveryIssue(
        issueId: issueId,
        expectedRevision: expectedRevision,
      ),
    );
  }

  @override
  Future<void> retryRecoveryIssue(String issueId) async {
    await _ensureReady();
    await _bridgeCall(() => frb.retryRecoveryIssue(issueId: issueId));
  }

  @override
  Future<TaskRecoveryPreview> previewTaskRecovery(String rootThreadId) async {
    await _ensureReady();
    return _taskRecoveryPreviewFromFrb(
      await _bridgeCall(
        () => frb.previewTaskRecovery(rootThreadId: rootThreadId),
      ),
    );
  }

  @override
  Future<TaskRecoveryResult> applyTaskRecovery(
    TaskRecoveryRequest request,
  ) async {
    await _ensureReady();
    return _taskRecoveryResultFromFrb(
      await _bridgeCall(
        () =>
            frb.applyTaskRecovery(request: _taskRecoveryRequestToFrb(request)),
      ),
    );
  }

  @override
  Future<SettingsStateSnapshot> setModelRole({
    required int expectedSettingsRevision,
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
  }) async {
    await _ensureReady();
    return _settingsStateFromFrb(
      await _bridgeCall(
        () => frb.setModelRole(
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          roleKey: roleKey,
          providerId: providerId,
          model: model,
          effort: effort,
        ),
      ),
    );
  }

  @override
  Future<void> setThreadMode({
    required String threadId,
    required StudioMode mode,
  }) async {
    await _ensureReady();
    await _bridgeCall(
      () => frb.setThreadMode(
        threadId: threadId,
        mode: switch (mode) {
          StudioMode.simple => frb.BridgeThreadMode.simple,
          StudioMode.task => frb.BridgeThreadMode.task,
        },
      ),
    );
  }

  @override
  Future<({ThreadWorkspace workspace, String? historyCursor})>
  readThreadSnapshot(String threadId) async {
    await _ensureReady();
    final snapshot = await _bridgeCall(
      () => frb.readThread(threadId: threadId),
    );
    return (
      workspace: _threadWorkspaceFromFrb(snapshot),
      historyCursor: snapshot.historyCursor,
    );
  }

  @override
  Stream<StudioShutdownProgress> subscribeShutdownProgress() {
    // 冷流：监听即建立；关机期间 bridge 不取消该订阅（Rust 侧独立生命周期）。
    return frb
        .subscribeShutdownProgress()
        .map(
          (event) => StudioShutdownProgress(
            phase: switch (event.phase) {
              frb.BridgeShutdownPhase.stoppingSubscriptions =>
                StudioShutdownPhase.stoppingSubscriptions,
              frb.BridgeShutdownPhase.cancellingTurns =>
                StudioShutdownPhase.cancellingTurns,
              frb.BridgeShutdownPhase.flushingPersistence =>
                StudioShutdownPhase.flushingPersistence,
              frb.BridgeShutdownPhase.suspendingTasks =>
                StudioShutdownPhase.suspendingTasks,
              frb.BridgeShutdownPhase.stoppingMcp =>
                StudioShutdownPhase.stoppingMcp,
              frb.BridgeShutdownPhase.stoppingLsp =>
                StudioShutdownPhase.stoppingLsp,
              frb.BridgeShutdownPhase.stopped => StudioShutdownPhase.stopped,
            },
            pendingCommits: event.pendingCommits.toInt(),
          ),
        )
        .handleError((Object error) => throw _studioFailure(error));
  }

  @override
  Future<void> shutdownRuntime() => shutdownAndDispose();

  @override
  Future<PendingInteraction> respondInteraction(
    String interactionId,
    InteractionResolutionCommand resolution,
  ) async {
    await _ensureReady();
    final response = await _bridgeCall(
      () => frb.respondInteraction(
        interactionId: interactionId,
        resolution: _interactionResolutionFromDomain(resolution),
      ),
    );
    return _interactionFromFrb(response);
  }

  static Future<T> _bridgeCall<T>(Future<T> Function() call) async {
    try {
      return await call();
    } catch (error, stackTrace) {
      Error.throwWithStackTrace(_studioFailure(error), stackTrace);
    }
  }

  @override
  Future<void> interruptTurn(String threadId, String turnId) async {
    await _ensureReady();
    await _bridgeCall(
      () => frb.interruptTurn(threadId: threadId, turnId: turnId),
    );
  }

  @override
  Stream<Object> subscribeProductEvents() {
    late final StreamController<Object> controller;
    frb.BridgeEventSubscription? handle;
    StreamSubscription<frb.BridgeProductStreamEnvelope>? subscription;
    var cancelled = false;

    Future<void> start() async {
      try {
        await _ensureReady();
        final created = await _bridgeCall(frb.createProductSubscription);
        if (cancelled) {
          await created.cancel();
          created.dispose();
          return;
        }
        handle = created;
        subscription = created.productStream().listen(
          (envelope) => envelope.when(
            data: (event) =>
                controller.add(StudioBridgeEvent.fromProduct(event)),
            failure: (error) => controller.addError(_studioFailure(error)),
            closed: controller.close,
          ),
          onError: (Object error, StackTrace stackTrace) =>
              controller.addError(_studioFailure(error), stackTrace),
          onDone: controller.close,
        );
      } catch (error, stackTrace) {
        controller.addError(_studioFailure(error), stackTrace);
        await controller.close();
      }
    }

    controller = StreamController<Object>(
      onListen: () => unawaited(start()),
      onCancel: () async {
        cancelled = true;
        await subscription?.cancel();
        final activeHandle = handle;
        if (activeHandle != null) {
          await activeHandle.cancel();
          activeHandle.dispose();
        }
      },
    );
    return controller.stream;
  }

  @override
  Stream<ThreadStreamFrame> subscribeThread(String threadId) {
    late final StreamController<ThreadStreamFrame> controller;
    frb.BridgeEventSubscription? handle;
    StreamSubscription<frb.BridgeThreadStreamEnvelope>? subscription;
    var cancelled = false;

    Future<void> start() async {
      try {
        await _ensureReady();
        final created = await _bridgeCall(
          () => frb.subscribeThread(threadId: threadId),
        );
        if (cancelled) {
          await created.cancel();
          created.dispose();
          return;
        }
        handle = created;
        subscription = created.threadStream().listen(
          (envelope) => envelope.when(
            data: (update) => controller.add(ThreadStreamFrame.fromFrb(update)),
            failure: (error) => controller.addError(_studioFailure(error)),
            closed: controller.close,
          ),
          onError: (Object error, StackTrace stackTrace) =>
              controller.addError(_studioFailure(error), stackTrace),
          onDone: controller.close,
        );
      } catch (error, stackTrace) {
        controller.addError(_studioFailure(error), stackTrace);
        await controller.close();
      }
    }

    controller = StreamController<ThreadStreamFrame>(
      onListen: () => unawaited(start()),
      onCancel: () async {
        cancelled = true;
        await subscription?.cancel();
        final activeHandle = handle;
        if (activeHandle != null) {
          await activeHandle.cancel();
          activeHandle.dispose();
        }
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
    await _ensureReady();
    final response = await _bridgeCall(
      () => frb.listThreadTurns(
        request: frb.ListThreadTurnsRequest(
          threadId: threadId,
          cursor: cursor,
          limit: limit.clamp(1, 200),
        ),
      ),
    );
    final items = [
      for (final turn in response.turns)
        for (final item in turn.items)
          _threadItemFromFrb(
            item,
            contextDisposition: switch (turn.contextDisposition) {
              frb.BridgeThreadContextDisposition.active =>
                ThreadContextDisposition.active,
              frb.BridgeThreadContextDisposition.rolledBack =>
                ThreadContextDisposition.rolledBack,
            },
          ),
    ]..sort(_compareThreadItems);
    return ThreadHistoryPage(items: items, nextCursor: response.nextCursor);
  }

  @override
  Future<SubmitPromptReceipt> startTurn(
    String threadId,
    String prompt,
    List<String> attachmentIds,
  ) async {
    await _ensureReady();
    final response = await _bridgeCall(
      () => frb.startTurn(
        threadId: threadId,
        prompt: prompt,
        attachmentIds: attachmentIds,
      ),
    );
    return SubmitPromptReceipt(
      threadId: response.threadId,
      turnId: response.turnId,
      cursor: response.revision.toInt(),
    );
  }

  @override
  Future<SubmitPromptReceipt> steerTurn(
    String threadId,
    String prompt,
    List<String> attachmentIds,
  ) async {
    await _ensureReady();
    final response = await _bridgeCall(
      () => frb.steerTurn(
        threadId: threadId,
        prompt: prompt,
        attachmentIds: attachmentIds,
      ),
    );
    return SubmitPromptReceipt(
      threadId: response.threadId,
      turnId: response.turnId,
      cursor: response.revision.toInt(),
    );
  }

  @override
  Future<SettingsStateSnapshot> saveRuntimePermissionMode(
    int expectedSettingsRevision,
    PermissionMode mode,
  ) async {
    await _ensureReady();
    return _settingsStateFromFrb(
      await _bridgeCall(
        () => frb.saveRuntimePermissionMode(
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          mode: _permissionModeLabel(mode),
        ),
      ),
    );
  }

  @override
  Future<SettingsStateSnapshot> saveProviderSettings(
    int expectedSettingsRevision,
    ProviderSettingsCommand command,
  ) async {
    await _ensureReady();
    return _settingsStateFromFrb(
      await _bridgeCall(
        () => frb.saveProviderSettings(
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          input: frb.ProviderSettingsInput(
            defaultProviderId: command.defaultProviderId,
            providers: [
              for (final provider in command.providers)
                frb.ProviderInput(
                  id: provider.id,
                  originalId: provider.originalId,
                  templateKind: provider.templateKind,
                  name: provider.name,
                  baseUrl: provider.baseUrl,
                  secret: switch (provider.secret.action) {
                    ProviderSecretAction.preserve =>
                      const frb.ProviderSecretInput.preserve(),
                    ProviderSecretAction.replace =>
                      frb.ProviderSecretInput.replace(
                        value: provider.secret.value!,
                      ),
                    ProviderSecretAction.clear =>
                      const frb.ProviderSecretInput.clear(),
                  },
                  capabilitySource: provider.capabilitySource,
                  hostedWebSearch: provider.hostedWebSearch,
                  standaloneWebSearch: provider.standaloneWebSearch,
                  promptCacheDialect: provider.promptCacheDialect,
                  responsesToolSearch: provider.responsesToolSearch,
                  responsesProgrammaticToolCalling:
                      provider.responsesProgrammaticToolCalling,
                  defaultModel: provider.defaultModel,
                  customModels: [
                    for (final model in provider.customModels)
                      frb.ProviderModelInput(
                        slug: model.slug,
                        displayName: model.displayName,
                        reasoningEfforts: model.reasoningEfforts,
                        baseInstructions: model.baseInstructions,
                        wireProtocol: model.wireProtocol,
                        supportedConnectionModes:
                            model.supportedConnectionModes,
                        defaultConnectionMode: model.defaultConnectionMode,
                      ),
                  ],
                  modelConnectionModes: [
                    for (final model in provider.modelConnectionModes)
                      frb.ProviderModelConnectionInput(
                        slug: model.slug,
                        connectionMode: model.connectionMode,
                      ),
                  ],
                ),
            ],
            roles: [
              for (final role in command.roles)
                frb.RoleInput(
                  key: role.key,
                  provider: role.providerId,
                  model: role.model,
                  effort: role.effort,
                ),
            ],
          ),
        ),
      ),
    );
  }

  @override
  Future<SettingsStateSnapshot> saveInstructionsSettings(
    int expectedSettingsRevision,
    InstructionsSettingsCommand command,
  ) async {
    await _ensureReady();
    return _settingsStateFromFrb(
      await _bridgeCall(
        () => frb.saveInstructionsSettings(
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          input: frb.InstructionsSettingsInput(
            baseOverride: command.baseOverride,
            developer: command.developer,
            user: command.user,
            projectDocMaxBytes: BigInt.from(command.projectDocMaxBytes),
            projectDocFallbackFilenames: command.projectDocFallbackFilenames,
          ),
        ),
      ),
    );
  }

  @override
  Future<SettingsStateSnapshot> saveSkillsSettings(
    int expectedSettingsRevision,
    SkillsSettingsCommand command,
  ) async {
    await _ensureReady();
    return _settingsStateFromFrb(
      await _bridgeCall(
        () => frb.saveSkillsSettings(
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          input: frb.SkillsSettingsInput(
            enabled: command.enabled,
            autoLearn: command.autoLearn,
            systemEnabled: command.systemEnabled,
            projectDir: command.projectDir,
            userDir: command.userDir,
            externalDirs: command.externalDirs,
            disabled: command.disabled,
            autoLearnMinToolCalls: command.autoLearnMinToolCalls,
          ),
        ),
      ),
    );
  }

  @override
  Future<SettingsStateSnapshot> saveMcpSettings(
    int expectedSettingsRevision,
    McpSettingsCommand command,
  ) async {
    await _ensureReady();
    return _settingsStateFromFrb(
      await _bridgeCall(
        () => frb.saveMcpSettings(
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          input: frb.McpSettingsInput(
            servers: [
              for (final server in command.servers)
                frb.McpServerInput(
                  id: server.id,
                  enabled: server.enabled,
                  transport: server.transport,
                  endpoint: server.endpoint,
                ),
            ],
          ),
        ),
      ),
    );
  }

  @override
  Future<SettingsStateSnapshot> saveGeneralSettings(
    int expectedSettingsRevision,
    GeneralSettingsCommand command,
  ) async {
    await _ensureReady();
    return _settingsStateFromFrb(
      await _bridgeCall(
        () => frb.saveGeneralSettings(
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          input: frb.GeneralSettingsInput(
            followSystemTheme: command.followSystemTheme,
            followActiveTurn: command.followActiveTurn,
            compactTimeline: command.compactTimeline,
          ),
        ),
      ),
    );
  }

  @override
  Future<SettingsStateSnapshot> saveWebSearchSettings(
    int expectedSettingsRevision,
    WebSearchSettingsCommand command,
  ) async {
    await _ensureReady();
    return _settingsStateFromFrb(
      await _bridgeCall(
        () => frb.saveWebSearchSettings(
          expectedSettingsRevision: BigInt.from(expectedSettingsRevision),
          input: frb.WebSearchSettingsInput(
            mode: command.mode,
            contextSize: command.contextSize,
            allowedDomains: command.allowedDomains,
            country: command.country,
            region: command.region,
            city: command.city,
            timezone: command.timezone,
          ),
        ),
      ),
    );
  }

  @override
  Future<ProviderUsageStateSnapshot> checkProviderUsage() async {
    await _ensureReady();
    return _providerUsageStateFromFrb(
      await _bridgeCall(frb.checkProviderUsage),
    );
  }

  @override
  Future<SkillsStateSnapshot> readSkillsState(String projectId) async {
    await _ensureReady();
    return _skillsStateFromFrb(
      await _bridgeCall(() => frb.readSkillsState(projectId: projectId)),
    );
  }

  @override
  Future<SkillsStateSnapshot> discoverSkills(String projectId) async {
    await _ensureReady();
    return _skillsStateFromFrb(
      await _bridgeCall(() => frb.discoverSkills(projectId: projectId)),
    );
  }

  @override
  Future<McpStateSnapshot> readMcpState() async {
    await _ensureReady();
    return _mcpStateFromFrb(await _bridgeCall(frb.readMcpState));
  }

  @override
  Future<McpStateSnapshot> resetMcpServer(String serverId) async {
    await _ensureReady();
    return _mcpStateFromFrb(
      await _bridgeCall(
        () => frb.resetMcp(input: frb.McpResetInput.server(serverId: serverId)),
      ),
    );
  }

  @override
  Future<McpStateSnapshot> resetAllMcp() async {
    await _ensureReady();
    return _mcpStateFromFrb(
      await _bridgeCall(
        () => frb.resetMcp(input: const frb.McpResetInput.all()),
      ),
    );
  }

  @override
  Future<LspStateSnapshot> readLspState() async {
    await _ensureReady();
    return _lspStateFromFrb(await _bridgeCall(frb.readLspState));
  }

  @override
  Future<LspStateSnapshot> probeLspServer(String projectId) async {
    await _ensureReady();
    return _lspStateFromFrb(
      await _bridgeCall(() => frb.probeLspServer(projectId: projectId)),
    );
  }

  @override
  Future<LspStateSnapshot> repairLspServer(
    String projectId,
    String serverId,
  ) async {
    await _ensureReady();
    return _lspStateFromFrb(
      await _bridgeCall(
        () => frb.repairLspServer(projectId: projectId, serverId: serverId),
      ),
    );
  }

  @override
  Future<LspStateSnapshot> resetLspServer(
    String projectId,
    String serverId,
  ) async {
    await _ensureReady();
    return _lspStateFromFrb(
      await _bridgeCall(
        () => frb.resetLsp(
          input: frb.LspScopeInput.server(
            projectId: projectId,
            serverId: serverId,
          ),
        ),
      ),
    );
  }

  @override
  Future<LspStateSnapshot> resetLspWorkspace(String projectId) async {
    await _ensureReady();
    return _lspStateFromFrb(
      await _bridgeCall(
        () => frb.resetLsp(
          input: frb.LspScopeInput.workspace(projectId: projectId),
        ),
      ),
    );
  }
}

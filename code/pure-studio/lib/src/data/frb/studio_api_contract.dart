part of 'studio_api.dart';

abstract class StudioApi {
  Future<ProviderCatalogView> loadProviderCatalog();
  Future<StudioState> bootstrap();
  Future<StudioState> openProject(String path);
  Future<StudioState> selectProject(String projectId);
  Future<StudioState> archiveProject(
    String projectId, {
    String? selectedProjectId,
  });
  Future<RecoveryCleanupPreview> previewProjectCleanup(String projectId);
  Future<StudioState> cleanupProject(
    String projectId,
    String expectedRevision, {
    String? selectedProjectId,
  });
  Future<StudioState> createSession(String projectId, {String? title});
  Future<StudioState> archiveSession(
    String sessionId, {
    String? selectedSessionId,
  });
  Future<RecoveryCleanupPreview> previewRecoveryIssueCleanup(String issueId);
  Future<StudioState> cleanupRecoveryIssue(
    String issueId,
    String expectedRevision, {
    String? selectedProjectId,
    String? selectedSessionId,
  });
  Future<StudioSession> setSessionMode(String sessionId, StudioMode mode);
  Future<StudioState> setModelRole({
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
    String? selectedSessionId,
  });
  Stream<Object> subscribeProductEvents();
  Stream<SessionStreamFrame> subscribeSessionEvents(
    String sessionId, {
    int? afterSequence,
  });
  Future<SubmitPromptReceipt> submitPrompt(
    String sessionId,
    String prompt,
    List<String> attachmentIds,
  );
  Future<void> stopPrompt(String sessionId);
  Future<InteractionResolutionResult> resolveInteraction(
    String interactionId,
    InteractionResolutionCommand resolution,
  );
  Future<StudioState> saveRuntimePermissionMode(PermissionMode mode);
  Future<StudioState> saveProviderSettings(ProviderSettingsCommand command);
  Future<StudioState> saveInstructionsSettings(
    InstructionsSettingsCommand command,
  );
  Future<StudioState> saveSkillsSettings(SkillsSettingsCommand command);
  Future<StudioState> saveMcpSettings(McpSettingsCommand command);
  Future<StudioState> saveGeneralSettings(GeneralSettingsCommand command);
  Future<StudioState> saveWebSearchSettings(WebSearchSettingsCommand command);
  Future<List<ProviderUsageView>> loadProviderUsages();
  Future<List<String>> listDiscoveredSkills(String projectId);
}

class FrbStudioApi implements StudioApi {
  static Future<void>? _initFuture;
  static Future<void> Function()? _initializationOverrideForTesting;
  static bool _rustInitialized = false;
  ProviderCatalogView? _providerCatalogCache;

  static Future<void> ensureReady() => _ensureReady();

  @visibleForTesting
  static void debugOverrideInitialization(
    Future<void> Function()? initialization,
  ) {
    _initFuture = null;
    _initializationOverrideForTesting = initialization;
  }

  static Future<void> _ensureReady() {
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
          await frb.initializeRuntime();
          await frb.startRuntime();
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

  static Future<void> shutdownAndDispose() async {
    if (!_rustInitialized) {
      return;
    }
    try {
      final initialization = _initFuture;
      if (initialization != null) {
        await initialization;
      }
      await frb.shutdownRuntime();
    } on Object {
      // Process teardown is best effort; Rust diagnostics retain the cause.
    } finally {
      RustLib.dispose();
      _rustInitialized = false;
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
  Future<StudioState> bootstrap() async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(await _bridgeCall(frb.bootstrapStudio));
  }

  @override
  Future<StudioState> openProject(String path) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(() => frb.openProject(path: path)),
    );
  }

  @override
  Future<StudioState> selectProject(String projectId) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(() => frb.selectProject(projectId: projectId)),
    );
  }

  @override
  Future<StudioState> archiveProject(
    String projectId, {
    String? selectedProjectId,
  }) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(
        () => frb.archiveProject(
          projectId: projectId,
          selectedProjectId: selectedProjectId,
        ),
      ),
    );
  }

  @override
  Future<RecoveryCleanupPreview> previewProjectCleanup(String projectId) async {
    await _ensureReady();
    return _recoveryCleanupPreviewFromFrb(
      await _bridgeCall(() => frb.previewProjectCleanup(projectId: projectId)),
    );
  }

  @override
  Future<StudioState> cleanupProject(
    String projectId,
    String expectedRevision, {
    String? selectedProjectId,
  }) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(
        () => frb.cleanupProject(
          projectId: projectId,
          expectedRevision: expectedRevision,
          selectedProjectId: selectedProjectId,
        ),
      ),
    );
  }

  @override
  Future<StudioState> createSession(String projectId, {String? title}) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(
        () => frb.createSession(projectId: projectId, title: title),
      ),
    );
  }

  @override
  Future<StudioState> archiveSession(
    String sessionId, {
    String? selectedSessionId,
  }) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(
        () => frb.archiveSession(
          sessionId: sessionId,
          selectedSessionId: selectedSessionId,
        ),
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
  Future<StudioState> cleanupRecoveryIssue(
    String issueId,
    String expectedRevision, {
    String? selectedProjectId,
    String? selectedSessionId,
  }) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(
        () => frb.cleanupRecoveryIssue(
          issueId: issueId,
          expectedRevision: expectedRevision,
          selectedProjectId: selectedProjectId,
          selectedSessionId: selectedSessionId,
        ),
      ),
    );
  }

  @override
  Future<StudioSession> setSessionMode(
    String sessionId,
    StudioMode mode,
  ) async {
    await _ensureReady();
    return _sessionFromFrb(
      await _bridgeCall(
        () => frb.setSessionMode(
          sessionId: sessionId,
          mode: _compileModeLabel(mode),
        ),
      ),
    );
  }

  @override
  Future<StudioState> setModelRole({
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
    String? selectedSessionId,
  }) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(
        () => frb.setModelRole(
          roleKey: roleKey,
          providerId: providerId,
          model: model,
          effort: effort,
          selectedSessionId: selectedSessionId,
        ),
      ),
    );
  }

  @override
  Future<InteractionResolutionResult> resolveInteraction(
    String interactionId,
    InteractionResolutionCommand resolution,
  ) async {
    await _ensureReady();
    final response = await _bridgeCall(
      () => frb.resolveInteraction(
        interactionId: interactionId,
        resolution: _interactionResolutionFromDomain(resolution),
      ),
    );
    return InteractionResolutionResult(
      sessionId: response.sessionId,
      interactionId: response.interaction.interactionId,
      status: response.interaction.status,
      sessions: response.sessions.map(_sessionFromFrb).toList(),
    );
  }

  static Future<T> _bridgeCall<T>(Future<T> Function() call) async {
    try {
      return await call();
    } catch (error, stackTrace) {
      Error.throwWithStackTrace(_studioFailure(error), stackTrace);
    }
  }

  @override
  Future<void> stopPrompt(String sessionId) async {
    await _ensureReady();
    await _bridgeCall(() => frb.stopPrompt(sessionId: sessionId));
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
  Stream<SessionStreamFrame> subscribeSessionEvents(
    String sessionId, {
    int? afterSequence,
  }) {
    late final StreamController<SessionStreamFrame> controller;
    frb.BridgeEventSubscription? handle;
    StreamSubscription<frb.BridgeSessionStreamEnvelope>? subscription;
    var cancelled = false;

    Future<void> start() async {
      try {
        await _ensureReady();
        final created = await _bridgeCall(
          () => frb.createSessionSubscription(
            sessionId: sessionId,
            afterSequence: afterSequence == null
                ? null
                : BigInt.from(afterSequence),
          ),
        );
        if (cancelled) {
          await created.cancel();
          created.dispose();
          return;
        }
        handle = created;
        subscription = created.sessionStream().listen(
          (envelope) => envelope.when(
            data: (frame) => controller.add(SessionStreamFrame.fromFrb(frame)),
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

    controller = StreamController<SessionStreamFrame>(
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
  Future<SubmitPromptReceipt> submitPrompt(
    String sessionId,
    String prompt,
    List<String> attachmentIds,
  ) async {
    await _ensureReady();
    final response = await _bridgeCall(
      () => frb.submitPrompt(
        sessionId: sessionId,
        prompt: prompt,
        attachmentIds: attachmentIds,
      ),
    );
    return SubmitPromptReceipt(
      sessionId: response.sessionId,
      turnId: response.turnId,
      cursor: response.cursor.toInt(),
    );
  }

  @override
  Future<StudioState> saveRuntimePermissionMode(PermissionMode mode) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(
        () => frb.saveRuntimePermissionMode(mode: _permissionModeLabel(mode)),
      ),
    );
  }

  @override
  Future<StudioState> saveProviderSettings(
    ProviderSettingsCommand command,
  ) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(
        () => frb.saveProviderSettings(
          input: frb.ProviderSettingsInput(
            defaultProviderId: command.defaultProviderId,
            providers: [
              for (final provider in command.providers)
                frb.ProviderInput(
                  id: provider.id,
                  originalId: provider.originalId,
                  templateKind: provider.templateKind,
                  wireProtocol: provider.wireProtocol,
                  connectionMode: provider.connectionMode,
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
                  defaultModel: provider.defaultModel,
                  customModels: [
                    for (final model in provider.customModels)
                      frb.ProviderModelInput(
                        slug: model.slug,
                        displayName: model.displayName,
                        reasoningEfforts: model.reasoningEfforts,
                        baseInstructions: model.baseInstructions,
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
  Future<StudioState> saveInstructionsSettings(
    InstructionsSettingsCommand command,
  ) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(
        () => frb.saveInstructionsSettings(
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
  Future<StudioState> saveSkillsSettings(SkillsSettingsCommand command) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(
        () => frb.saveSkillsSettings(
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
  Future<StudioState> saveMcpSettings(McpSettingsCommand command) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(
        () => frb.saveMcpSettings(
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
  Future<StudioState> saveGeneralSettings(
    GeneralSettingsCommand command,
  ) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(
        () => frb.saveGeneralSettings(
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
  Future<StudioState> saveWebSearchSettings(
    WebSearchSettingsCommand command,
  ) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await _bridgeCall(
        () => frb.saveWebSearchSettings(
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
  Future<List<ProviderUsageView>> loadProviderUsages() async {
    await _ensureReady();
    final response = await _bridgeCall(frb.loadProviderUsages);
    return response.usages.map(_providerUsageFromFrb).toList();
  }

  @override
  Future<List<String>> listDiscoveredSkills(String projectId) async {
    await _ensureReady();
    final response = await _bridgeCall(
      () => frb.listDiscoveredSkills(projectId: projectId),
    );
    return response.skills
        .map((skill) => skill.name)
        .where((name) => name.isNotEmpty)
        .toList()
      ..sort();
  }
}

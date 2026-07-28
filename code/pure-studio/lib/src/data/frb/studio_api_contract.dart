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
  Future<StudioState> createSession(String projectId, {String? title});
  Future<StudioState> archiveSession(
    String sessionId, {
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
  Future<void> submitPrompt(
    String sessionId,
    String prompt,
    List<String> attachmentIds,
  );
  Future<void> stopPrompt(String sessionId);
  Future<void> resolveInteraction(
    String interactionId,
    Map<String, Object?> resolution,
  );
  Future<void> saveRuntimePermissionMode(PermissionMode mode);
  Future<StudioState> saveProviderSettings(Map<String, Object?> settings);
  Future<StudioState> saveInstructionsSettings(Map<String, Object?> settings);
  Future<StudioState> saveSkillsSettings(Map<String, Object?> settings);
  Future<StudioState> saveMcpSettings(Map<String, Object?> settings);
  Future<StudioState> saveGeneralSettings(Map<String, Object?> settings);
  Future<StudioState> saveWebSearchSettings(WebSearchSettingsView settings);
  Future<List<ProviderUsageView>> loadProviderUsages();
  Future<List<String>> listDiscoveredSkills(String projectId);
  Future<void> saveStudioSettingsDraft(
    String section,
    Map<String, Object?> draft,
  );
}

class FrbStudioApi implements StudioApi {
  static Future<void>? _initFuture;
  ProviderCatalogView? _providerCatalogCache;

  static Future<void> ensureReady() => _ensureReady();

  static Future<void> _ensureReady() {
    return _initFuture ??= () async {
      await RustLib.init();
      await frb.initializeRuntime();
      await frb.startRuntime();
    }();
  }

  @override
  Future<ProviderCatalogView> loadProviderCatalog() async {
    final cached = _providerCatalogCache;
    if (cached != null) return cached;
    await _ensureReady();
    final catalog = providerCatalogFromFrb(await frb.loadProviderCatalog());
    _providerCatalogCache = catalog;
    return catalog;
  }

  @override
  Future<StudioState> bootstrap() async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(await frb.bootstrapStudio());
  }

  @override
  Future<StudioState> openProject(String path) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(await frb.openProject(path: path));
  }

  @override
  Future<StudioState> selectProject(String projectId) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.selectProject(projectId: projectId),
    );
  }

  @override
  Future<StudioState> archiveProject(
    String projectId, {
    String? selectedProjectId,
  }) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.archiveProject(
        projectId: projectId,
        selectedProjectId: selectedProjectId,
      ),
    );
  }

  @override
  Future<StudioState> createSession(String projectId, {String? title}) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.createSession(projectId: projectId, title: title),
    );
  }

  @override
  Future<StudioState> archiveSession(
    String sessionId, {
    String? selectedSessionId,
  }) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.archiveSession(
        sessionId: sessionId,
        selectedSessionId: selectedSessionId,
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
      await frb.setSessionMode(
        sessionId: sessionId,
        mode: _compileModeLabel(mode),
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
      await frb.setModelRole(
        roleKey: roleKey,
        providerId: providerId,
        model: model,
        effort: effort,
        selectedSessionId: selectedSessionId,
      ),
    );
  }

  @override
  Future<void> resolveInteraction(
    String interactionId,
    Map<String, Object?> resolution,
  ) async {
    await _ensureReady();
    await frb.resolveInteraction(
      interactionId: interactionId,
      resolutionJson: jsonEncode(resolution),
    );
  }

  @override
  Future<void> stopPrompt(String sessionId) async {
    await _ensureReady();
    await frb.stopPrompt(sessionId: sessionId);
  }

  @override
  Stream<Object> subscribeProductEvents() async* {
    await _ensureReady();
    yield* frb.subscribeProductEvents().map(StudioBridgeEvent.fromProduct);
  }

  @override
  Stream<SessionStreamFrame> subscribeSessionEvents(
    String sessionId, {
    int? afterSequence,
  }) async* {
    await _ensureReady();
    yield* frb
        .subscribeSessionEvents(
          sessionId: sessionId,
          afterSequence: afterSequence == null
              ? null
              : BigInt.from(afterSequence),
        )
        .map(SessionStreamFrame.fromFrb);
  }

  @override
  Future<void> submitPrompt(
    String sessionId,
    String prompt,
    List<String> attachmentIds,
  ) async {
    await _ensureReady();
    await frb.submitPrompt(
      sessionId: sessionId,
      prompt: prompt,
      attachmentIds: attachmentIds,
    );
  }

  @override
  Future<void> saveRuntimePermissionMode(PermissionMode mode) async {
    await _ensureReady();
    await frb.saveRuntimePermissionMode(mode: _permissionModeLabel(mode));
  }

  @override
  Future<StudioState> saveProviderSettings(
    Map<String, Object?> settings,
  ) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.saveProviderSettings(settingsJson: jsonEncode(settings)),
    );
  }

  @override
  Future<StudioState> saveInstructionsSettings(
    Map<String, Object?> settings,
  ) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.saveInstructionsSettings(settingsJson: jsonEncode(settings)),
    );
  }

  @override
  Future<StudioState> saveSkillsSettings(Map<String, Object?> settings) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.saveSkillsSettings(settingsJson: jsonEncode(settings)),
    );
  }

  @override
  Future<StudioState> saveMcpSettings(Map<String, Object?> settings) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.saveMcpSettings(settingsJson: jsonEncode(settings)),
    );
  }

  @override
  Future<StudioState> saveGeneralSettings(Map<String, Object?> settings) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.saveGeneralSettings(settingsJson: jsonEncode(settings)),
    );
  }

  @override
  Future<StudioState> saveWebSearchSettings(
    WebSearchSettingsView settings,
  ) async {
    await _ensureReady();
    return studioStateFromFrbSnapshot(
      await frb.saveWebSearchSettings(
        input: frb.WebSearchSettingsInput(
          mode: settings.configuredMode,
          contextSize: settings.contextSize,
          allowedDomains: settings.allowedDomains,
          country: settings.country,
          region: settings.region,
          city: settings.city,
          timezone: settings.timezone,
        ),
      ),
    );
  }

  @override
  Future<List<ProviderUsageView>> loadProviderUsages() async {
    await _ensureReady();
    final response = await frb.loadProviderUsages();
    return response.usages.map(_providerUsageFromFrb).toList();
  }

  @override
  Future<List<String>> listDiscoveredSkills(String projectId) async {
    await _ensureReady();
    final response = await frb.listDiscoveredSkills(projectId: projectId);
    return response.skills
        .map((skill) => skill.name)
        .where((name) => name.isNotEmpty)
        .toList()
      ..sort();
  }

  @override
  Future<void> saveStudioSettingsDraft(
    String section,
    Map<String, Object?> draft,
  ) async {
    return;
  }
}

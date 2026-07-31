import '../../domain/models/studio_models.dart';
import '../frb/studio_api.dart';

class StudioActionService {
  const StudioActionService(this._api);

  final StudioApi _api;

  Future<ProviderCatalogView> loadProviderCatalog() {
    return _api.loadProviderCatalog();
  }

  Future<StudioState> bootstrap() => _api.bootstrap();

  Future<StudioState> openProject(String path) => _api.openProject(path);

  Future<StudioState> selectProject(String projectId) {
    return _api.selectProject(projectId);
  }

  Future<StudioState> archiveProject(
    String projectId, {
    String? selectedProjectId,
  }) {
    return _api.archiveProject(projectId, selectedProjectId: selectedProjectId);
  }

  Future<RecoveryCleanupPreview> previewProjectCleanup(String projectId) {
    return _api.previewProjectCleanup(projectId);
  }

  Future<StudioState> cleanupProject(
    String projectId,
    String expectedRevision, {
    String? selectedProjectId,
  }) {
    return _api.cleanupProject(
      projectId,
      expectedRevision,
      selectedProjectId: selectedProjectId,
    );
  }

  Future<StudioState> createSession(String projectId) {
    return _api.createSession(projectId);
  }

  Future<StudioState> archiveSession(
    String sessionId, {
    String? selectedSessionId,
  }) {
    return _api.archiveSession(sessionId, selectedSessionId: selectedSessionId);
  }

  Future<SubmitPromptReceipt> submitPrompt(String sessionId, String prompt) {
    return _api.submitPrompt(sessionId, prompt, const []);
  }

  Future<void> stopPrompt(String sessionId) => _api.stopPrompt(sessionId);

  Future<StudioState> saveRuntimePermissionMode(PermissionMode mode) {
    return _api.saveRuntimePermissionMode(mode);
  }

  Future<StudioSession> setSessionMode(String sessionId, StudioMode mode) {
    return _api.setSessionMode(sessionId, mode);
  }

  Future<StudioState> setModelRole({
    required String roleKey,
    required String providerId,
    required String model,
    String? effort,
    String? selectedSessionId,
  }) {
    return _api.setModelRole(
      roleKey: roleKey,
      providerId: providerId,
      model: model,
      effort: effort,
      selectedSessionId: selectedSessionId,
    );
  }

  Future<StudioState> saveProviderSettings(ProviderSettingsCommand command) {
    return _api.saveProviderSettings(command);
  }

  Future<StudioState> saveInstructionsSettings(
    InstructionsSettingsCommand command,
  ) {
    return _api.saveInstructionsSettings(command);
  }

  Future<StudioState> saveSkillsSettings(SkillsSettingsCommand command) {
    return _api.saveSkillsSettings(command);
  }

  Future<StudioState> saveMcpSettings(McpSettingsCommand command) {
    return _api.saveMcpSettings(command);
  }

  Future<StudioState> saveGeneralSettings(GeneralSettingsCommand command) {
    return _api.saveGeneralSettings(command);
  }

  Future<StudioState> saveWebSearchSettings(WebSearchSettingsCommand command) {
    return _api.saveWebSearchSettings(command);
  }

  Future<List<ProviderUsageView>> loadProviderUsages() {
    return _api.loadProviderUsages();
  }

  Future<List<String>> listDiscoveredSkills(String projectId) {
    return _api.listDiscoveredSkills(projectId);
  }

  Future<RecoveryCleanupPreview> previewRecoveryIssueCleanup(String issueId) {
    return _api.previewRecoveryIssueCleanup(issueId);
  }

  Future<StudioState> cleanupRecoveryIssue(
    String issueId,
    String expectedRevision, {
    String? selectedProjectId,
    String? selectedSessionId,
  }) {
    return _api.cleanupRecoveryIssue(
      issueId,
      expectedRevision,
      selectedProjectId: selectedProjectId,
      selectedSessionId: selectedSessionId,
    );
  }

  Future<InteractionResolutionResult> resolveInteraction(
    String interactionId,
    InteractionResolutionCommand resolution,
  ) {
    return _api.resolveInteraction(interactionId, resolution);
  }
}

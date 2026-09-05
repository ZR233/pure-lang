import 'package:flutter/foundation.dart';

abstract final class StudioDriverKeys {
  static const shell = ValueKey<String>('studio-shell');
  static const startPage = ValueKey<String>('studio-start-page');
  static const sidebar = ValueKey<String>('studio-sidebar');
  static const timeline = ValueKey<String>('timeline-scrollable');
  static const workspaceFooterScroll = ValueKey<String>(
    'workspace-footer-scrollable',
  );
  static const newSession = ValueKey<String>('sidebar-new-session');
  static const openProject = ValueKey<String>('sidebar-open-project');
  static const projectPathDialog = ValueKey<String>('project-path-dialog');
  static const projectPathInput = ValueKey<String>('project-path-input');
  static const projectPathSubmit = ValueKey<String>('project-path-submit');
  static const settingsOpen = ValueKey<String>('settings-open');
  static const settingsPage = ValueKey<String>('settings-page');
  static const settingsBack = ValueKey<String>('settings-back');
  static const sessionMode = ValueKey<String>('session-mode-selector');
  static const startPageSelectors = ValueKey<String>('start-page-selectors');
  static const model = ValueKey<String>('model-selector');
  static const reasoningEffort = ValueKey<String>('reasoning-effort-selector');
  static const providerUsageCheck = ValueKey<String>('provider-usage-check');
  static const skillsDiscover = ValueKey<String>('skills-discover');
  static const mcpRefresh = ValueKey<String>('mcp-refresh');
  static const mcpResetAll = ValueKey<String>('mcp-reset-all');
  static const mcpResetAllConfirm = ValueKey<String>('mcp-reset-all-confirm');
  static const lspRefresh = ValueKey<String>('lsp-refresh');
  static const lspProbe = ValueKey<String>('lsp-probe');
  static const lspResetWorkspace = ValueKey<String>('lsp-reset-workspace');
  static const sshAddServer = ValueKey<String>('ssh-add-server');
  static const sshServerDialog = ValueKey<String>('ssh-server-dialog');
  static const sshServerNameInput = ValueKey<String>('ssh-server-name-input');
  static const sshServerHostInput = ValueKey<String>('ssh-server-host-input');
  static const sshServerUsernameInput = ValueKey<String>(
    'ssh-server-username-input',
  );
  static const sshServerPortInput = ValueKey<String>('ssh-server-port-input');
  static const sshServerAuthInput = ValueKey<String>('ssh-server-auth-input');
  static const sshServerIdentityInput = ValueKey<String>(
    'ssh-server-identity-input',
  );
  static const sshServerPasswordInput = ValueKey<String>(
    'ssh-server-password-input',
  );
  static const sshServerValidationError = ValueKey<String>(
    'ssh-server-validation-error',
  );
  static const sshServerSave = ValueKey<String>('ssh-server-save');
  static const sshDirectoryDialog = ValueKey<String>('ssh-directory-dialog');
  static const sshDirectoryList = ValueKey<String>('ssh-directory-list');
  static const sshOpenCurrentDirectory = ValueKey<String>(
    'ssh-open-current-directory',
  );
  static const composerInput = ValueKey<String>('composer-input');
  static const composerSubmit = ValueKey<String>('composer-submit');
  static const composerStop = ValueKey<String>('composer-stop');
  static const composerPending = ValueKey<String>('composer-pending');
  static const composerError = ValueKey<String>('composer-error');
  static const attachmentEntry = ValueKey<String>('composer-attachment-entry');
  static const attachmentLocal = ValueKey<String>('composer-attachment-local');
  static const attachmentUrl = ValueKey<String>('composer-attachment-url');
  static const attachmentUrlDialog = ValueKey<String>('attachment-url-dialog');
  static const attachmentUrlInput = ValueKey<String>('attachment-url-input');
  static const attachmentUrlSubmit = ValueKey<String>('attachment-url-submit');
  static const attachmentDraftRail = ValueKey<String>('attachment-draft-rail');
  static const agentSwitcher = ValueKey<String>('agent-switcher');
  static const sessionCost = ValueKey<String>('session-cost');
  static const threadThroughput = ValueKey<String>('thread-throughput');
  static const statisticsSummary = ValueKey<String>('statistics-summary');
  static const statisticsHistory = ValueKey<String>('statistics-history');
  static const statisticsFilter = ValueKey<String>('statistics-filter');

  static ValueKey<String> statisticsHistoryRow(
    String providerInstanceId,
    String model,
    int completedAtMillis,
  ) => ValueKey<String>(
    'statistics-history-row:$providerInstanceId:$model:$completedAtMillis',
  );
  static const providerEditor = ValueKey<String>('provider-editor');
  static const providerEdit = ValueKey<String>('provider-edit');
  static const providerSave = ValueKey<String>('provider-save');
  static const providerPricing = ValueKey<String>('provider-pricing');
  static const providerEditorScroll = ValueKey<String>(
    'provider-editor-scroll',
  );
  static const providerAdd = ValueKey<String>('provider-add');
  static const providerPreset = ValueKey<String>('provider-preset');
  static const providerBaseUrl = ValueKey<String>('provider-base-url');
  static const providerApiKey = ValueKey<String>('provider-api-key');
  static const providerModelAdd = ValueKey<String>('provider-model-add');
  static ValueKey<String> customModelId(int index) =>
      ValueKey<String>('provider-model-$index-id');
  static const providerCancel = ValueKey<String>('provider-cancel');
  static const toolApprove = ValueKey<String>('tool-approve');
  static const toolDeny = ValueKey<String>('tool-deny');
  static const userInputSubmit = ValueKey<String>('user-input-submit');
  static const planSummary = ValueKey<String>('plan-summary');
  static const planDetails = ValueKey<String>('plan-details');
  static const planDetailsScroll = ValueKey<String>('plan-details-scroll');
  static const planDetailsClose = ValueKey<String>('plan-details-close');
  static const planFeedbackInput = ValueKey<String>('plan-feedback-input');
  static const planSubmitRevision = ValueKey<String>('plan-submit-revision');
  static const planApprove = ValueKey<String>('plan-approve');
  static const userInputFirstOption = ValueKey<String>(
    'user-input-first-option',
  );
  static const userInputFirstText = ValueKey<String>('user-input-first-text');
  static const fallbackUserInput = ValueKey<String>('fallback-user-input');
  static const fallbackUserInputSubmit = ValueKey<String>(
    'fallback-user-input-submit',
  );

  static ValueKey<String> settingsTab(String id) =>
      ValueKey<String>('settings-tab-$id');

  static ValueKey<String> sshTest(String id) =>
      ValueKey<String>('ssh-test-$id');

  static ValueKey<String> sshReconnect(String id) =>
      ValueKey<String>('ssh-reconnect-$id');

  static ValueKey<String> sshOpen(String id) =>
      ValueKey<String>('ssh-open-$id');

  static ValueKey<String> sshDirectoryCurrent(String path) =>
      ValueKey<String>('ssh-directory-current-$path');

  static ValueKey<String> sshDirectoryEntry(String path) =>
      ValueKey<String>('ssh-directory-entry-$path');

  static ValueKey<String> projectRow(String id) =>
      ValueKey<String>('project-row-$id');

  static ValueKey<String> threadRow(String id) =>
      ValueKey<String>('thread-row-$id');

  static ValueKey<String> archiveThread(String id) =>
      ValueKey<String>('thread-archive-$id');

  static ValueKey<String> renameThread(String id) =>
      ValueKey<String>('thread-rename-$id');

  static ValueKey<String> renameThreadDialog(String id) =>
      ValueKey<String>('thread-rename-dialog-$id');

  static ValueKey<String> renameThreadInput(String id) =>
      ValueKey<String>('thread-rename-input-$id');

  static ValueKey<String> renameThreadSave(String id) =>
      ValueKey<String>('thread-rename-save-$id');

  static ValueKey<String> retryRecoveryIssue(String id) =>
      ValueKey<String>('recovery-retry-$id');

  static ValueKey<String> turnActivity(String id) =>
      ValueKey<String>('turn-activity-$id');

  static ValueKey<String> timelineRolledBack(String id) =>
      ValueKey<String>('timeline-rolled-back-$id');

  static ValueKey<String> timelineBlock(String id) =>
      ValueKey<String>('timeline-block-$id');

  static ValueKey<String> timelineSkillActivation(String id) =>
      ValueKey<String>('timeline-skill-activation-$id');

  static ValueKey<String> statusActiveSkill(String name) =>
      ValueKey<String>('status-active-skill-$name');

  static ValueKey<String> sessionModeOption(String mode) =>
      ValueKey<String>('session-mode-$mode');

  static ValueKey<String> settingsRoleModel(String role) =>
      ValueKey<String>('settings-role-$role-model');

  static ValueKey<String> settingsRoleModelOption(
    String role,
    String providerId,
    String model,
  ) => ValueKey<String>('settings-role-$role-model-$providerId-$model');

  static ValueKey<String> modelOption(String providerId, String model) =>
      ValueKey<String>('model-$providerId-$model');

  static ValueKey<String> modelCapabilityTags(
    String providerId,
    String model,
  ) => ValueKey<String>('model-$providerId-$model-capabilities');

  static ValueKey<String> attachmentDraft(String id) =>
      ValueKey<String>('attachment-draft-$id');

  static ValueKey<String> attachmentRemove(String id) =>
      ValueKey<String>('attachment-remove-$id');

  static ValueKey<String> attachmentModality(String id) =>
      ValueKey<String>('attachment-modality-$id');

  static ValueKey<String> historyAttachment(String id) =>
      ValueKey<String>('history-attachment-$id');

  static ValueKey<String> timelineToolGroupSummary(String id) =>
      ValueKey<String>('timeline-tool-group-summary-$id');

  static ValueKey<String> viewImageTool(String callId) =>
      ValueKey<String>('view-image-tool-$callId');

  static ValueKey<String> viewImageThumbnail(String id) =>
      ValueKey<String>('view-image-thumbnail-$id');

  static ValueKey<String> viewImageDialog(String id) =>
      ValueKey<String>('view-image-dialog-$id');

  static ValueKey<String> timelineImageDialog(String id) =>
      ValueKey<String>('timeline-image-dialog-$id');

  static ValueKey<String> timelineImageRetry(String id) =>
      ValueKey<String>('timeline-image-retry-$id');

  static const ValueKey<String> timelineImageClose = ValueKey<String>(
    'timeline-image-close',
  );

  static ValueKey<String> toolImageGallery(String groupId) =>
      ValueKey<String>('tool-image-gallery-$groupId');

  static ValueKey<String> markdownImageSource(String url) =>
      ValueKey<String>('markdown-image-source-$url');

  static ValueKey<String> markdownImageThumbnail(String url) =>
      ValueKey<String>('markdown-image-thumbnail-$url');

  static ValueKey<String> markdownImageDialog(String url) =>
      ValueKey<String>('markdown-image-dialog-$url');

  static ValueKey<String> reasoningEffortOption(String effort) =>
      ValueKey<String>('reasoning-effort-$effort');

  static ValueKey<String> mcpResetServer(String serverId) =>
      ValueKey<String>('mcp-reset-$serverId');

  static ValueKey<String> mcpServerRow(String serverId) =>
      ValueKey<String>('mcp-server-$serverId');

  static ValueKey<String> mcpServerError(String serverId) =>
      ValueKey<String>('mcp-server-error-$serverId');

  static ValueKey<String> lspRepairServer(String serverId) =>
      ValueKey<String>('lsp-repair-$serverId');

  static ValueKey<String> lspResetServer(String serverId) =>
      ValueKey<String>('lsp-reset-$serverId');

  static ValueKey<String> lspActivity() =>
      const ValueKey<String>('lsp-activity');

  static ValueKey<String> lspActivityDetail() =>
      const ValueKey<String>('lsp-activity-detail');

  static ValueKey<String> lspActivityOverflow() =>
      const ValueKey<String>('lsp-activity-overflow');

  static ValueKey<String> contextUsage() =>
      const ValueKey<String>('context-usage');

  static ValueKey<String> contextUsageDetail() =>
      const ValueKey<String>('context-usage-detail');

  static ValueKey<String> settingsRoleEffort(String role) =>
      ValueKey<String>('settings-role-$role-effort');

  static ValueKey<String> settingsRoleEffortOption(
    String role,
    String effort,
  ) => ValueKey<String>('settings-role-$role-effort-$effort');

  static ValueKey<String> userInputOption(String questionId, int optionIndex) =>
      ValueKey<String>('user-input-option-$questionId-$optionIndex');

  static ValueKey<String> userInputText(String questionId) =>
      ValueKey<String>('user-input-text-$questionId');

  static ValueKey<String> agentRow(String id) =>
      ValueKey<String>('agent-thread-$id');

  static ValueKey<String> providerRow(String id) =>
      ValueKey<String>('provider-row-$id');

  static ValueKey<String> providerConnectionMode(String providerId) =>
      ValueKey<String>('provider-$providerId-connection-mode');

  static ValueKey<String> providerConnectionModeOption(
    String providerId,
    String mode,
  ) => ValueKey<String>('provider-$providerId-connection-mode-$mode');

  static ValueKey<String> providerModelConnectionMode(
    String providerId,
    String model,
  ) => ValueKey<String>('provider-$providerId-model-$model-connection-mode');

  static ValueKey<String> providerModelConnectionModeOption(
    String providerId,
    String model,
    String mode,
  ) => ValueKey<String>(
    'provider-$providerId-model-$model-connection-mode-$mode',
  );
}

import 'package:flutter/foundation.dart';

abstract final class StudioDriverKeys {
  static const shell = ValueKey<String>('studio-shell');
  static const startPage = ValueKey<String>('studio-start-page');
  static const sidebar = ValueKey<String>('studio-sidebar');
  static const timeline = ValueKey<String>('timeline-scrollable');
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
  static const composerInput = ValueKey<String>('composer-input');
  static const composerSubmit = ValueKey<String>('composer-submit');
  static const composerStop = ValueKey<String>('composer-stop');
  static const composerPending = ValueKey<String>('composer-pending');
  static const composerError = ValueKey<String>('composer-error');
  static const taskPaused = ValueKey<String>('task-paused');
  static const taskResume = ValueKey<String>('task-resume');
  static const taskRecoveryDialog = ValueKey<String>('task-recovery-dialog');
  static const taskRecoveryTarget = ValueKey<String>('task-recovery-target');
  static const taskRecoveryTailCount = ValueKey<String>(
    'task-recovery-tail-count',
  );
  static const taskRecoveryMode = ValueKey<String>('task-recovery-mode');
  static const taskRecoveryConfirm = ValueKey<String>('task-recovery-confirm');
  static const taskRecoveryApply = ValueKey<String>('task-recovery-apply');
  static const taskRecoveryError = ValueKey<String>('task-recovery-error');
  static const agentSwitcher = ValueKey<String>('agent-switcher');
  static const providerEditor = ValueKey<String>('provider-editor');
  static const providerEdit = ValueKey<String>('provider-edit');
  static const providerSave = ValueKey<String>('provider-save');
  static const providerCancel = ValueKey<String>('provider-cancel');
  static const planImplement = ValueKey<String>('plan-implement');
  static const planDismiss = ValueKey<String>('plan-dismiss');
  static const planAdjustmentInput = ValueKey<String>('plan-adjustment-input');
  static const planContinue = ValueKey<String>('plan-continue');
  static const toolApprove = ValueKey<String>('tool-approve');
  static const toolDeny = ValueKey<String>('tool-deny');
  static const userInputSubmit = ValueKey<String>('user-input-submit');

  static ValueKey<String> settingsTab(String id) =>
      ValueKey<String>('settings-tab-$id');

  static ValueKey<String> projectRow(String id) =>
      ValueKey<String>('project-row-$id');

  static ValueKey<String> threadRow(String id) =>
      ValueKey<String>('thread-row-$id');

  static ValueKey<String> archiveThread(String id) =>
      ValueKey<String>('thread-archive-$id');

  static ValueKey<String> retryRecoveryIssue(String id) =>
      ValueKey<String>('recovery-retry-$id');

  static ValueKey<String> turnActivity(String id) =>
      ValueKey<String>('turn-activity-$id');

  static ValueKey<String> taskRecoveryTurn(String id) =>
      ValueKey<String>('task-recovery-turn-$id');

  static ValueKey<String> taskRecoveryModeOption(String mode) =>
      ValueKey<String>('task-recovery-mode-$mode');

  static ValueKey<String> timelineRolledBack(String id) =>
      ValueKey<String>('timeline-rolled-back-$id');

  static ValueKey<String> timelineBlock(String id) =>
      ValueKey<String>('timeline-block-$id');

  static ValueKey<String> timelineToolSearchCard(String id) =>
      ValueKey<String>('timeline-tool-search-card-$id');

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

  static ValueKey<String> taskAgent(String id) =>
      ValueKey<String>('task-agent-$id');

  static ValueKey<String> taskRuntime(String runId) =>
      ValueKey<String>('task-runtime-$runId');

  static ValueKey<String> taskPhase(String runId, String phase) =>
      ValueKey<String>('task-runtime-$runId-phase-$phase');

  static ValueKey<String> taskStatus(String runId, String status) =>
      ValueKey<String>('task-runtime-$runId-status-$status');

  static ValueKey<String> taskAgentStatus(String id) =>
      ValueKey<String>('task-agent-$id-status');

  static ValueKey<String> taskAgentSummaryAge(String id) =>
      ValueKey<String>('task-agent-summary-age-$id');

  static ValueKey<String> taskCompletion(String id) =>
      ValueKey<String>('task-completion-$id');

  static ValueKey<String> taskCompletionExecutor(String id) =>
      ValueKey<String>('task-completion-$id-executor');

  static ValueKey<String> taskCompletionStatus(String id) =>
      ValueKey<String>('task-completion-$id-status');

  static ValueKey<String> taskCompletionRevision(String id, int revision) =>
      ValueKey<String>('task-completion-$id-revision-$revision');

  static ValueKey<String> taskWorkUnit(String id) =>
      ValueKey<String>('task-work-unit-$id');

  static ValueKey<String> taskFailure(String id) =>
      ValueKey<String>('task-failure-$id');

  static ValueKey<String> taskWorkUnitExecution(String id) =>
      ValueKey<String>('task-work-unit-$id-execution');

  static ValueKey<String> taskWorkUnitBudgetSlice(String id) =>
      ValueKey<String>('task-work-unit-$id-budget-slice');

  static ValueKey<String> taskWorkUnitContinuation(String id) =>
      ValueKey<String>('task-work-unit-$id-continuation');

  static ValueKey<String> taskReview(String id) =>
      ValueKey<String>('task-review-$id');

  static ValueKey<String> taskReviewReviewer(String id) =>
      ValueKey<String>('task-review-$id-reviewer');

  static ValueKey<String> taskReviewVerdict(String id) =>
      ValueKey<String>('task-review-$id-verdict');

  static ValueKey<String> taskFinding(String reviewId, int index) =>
      ValueKey<String>('task-review-$reviewId-finding-$index');

  static ValueKey<String> taskFindingSeverity(String reviewId, int index) =>
      ValueKey<String>('task-review-$reviewId-finding-$index-severity');

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

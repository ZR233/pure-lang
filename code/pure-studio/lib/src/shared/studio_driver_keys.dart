import 'package:flutter/foundation.dart';

abstract final class StudioDriverKeys {
  static const shell = ValueKey<String>('studio-shell');
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
  static const reasoningEffort = ValueKey<String>('reasoning-effort-selector');
  static const composerInput = ValueKey<String>('composer-input');
  static const composerSubmit = ValueKey<String>('composer-submit');
  static const composerStop = ValueKey<String>('composer-stop');
  static const composerPending = ValueKey<String>('composer-pending');
  static const composerError = ValueKey<String>('composer-error');
  static const taskPaused = ValueKey<String>('task-paused');
  static const taskResume = ValueKey<String>('task-resume');
  static const agentSwitcher = ValueKey<String>('agent-switcher');
  static const providerEditor = ValueKey<String>('provider-editor');
  static const providerEdit = ValueKey<String>('provider-edit');
  static const providerSave = ValueKey<String>('provider-save');
  static const providerCancel = ValueKey<String>('provider-cancel');
  static const planImplement = ValueKey<String>('plan-implement');
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

  static ValueKey<String> turnActivity(String id) =>
      ValueKey<String>('turn-activity-$id');

  static ValueKey<String> timelineBlock(String id) =>
      ValueKey<String>('timeline-block-$id');

  static ValueKey<String> sessionModeOption(String mode) =>
      ValueKey<String>('session-mode-$mode');

  static ValueKey<String> settingsRoleModel(String role) =>
      ValueKey<String>('settings-role-$role-model');

  static ValueKey<String> settingsRoleModelOption(
    String role,
    String providerId,
    String model,
  ) => ValueKey<String>('settings-role-$role-model-$providerId-$model');

  static ValueKey<String> settingsRoleEffort(String role) =>
      ValueKey<String>('settings-role-$role-effort');

  static ValueKey<String> settingsRoleEffortOption(
    String role,
    String effort,
  ) => ValueKey<String>('settings-role-$role-effort-$effort');

  static ValueKey<String> userInputOption(String questionId, int optionIndex) =>
      ValueKey<String>('user-input-option-$questionId-$optionIndex');

  static ValueKey<String> agentRow(String id) =>
      ValueKey<String>('agent-thread-$id');

  static ValueKey<String> taskAgent(String id) =>
      ValueKey<String>('task-agent-$id');

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
}

import 'package:flutter/material.dart';

import '../domain/models/studio_enums.dart';
import '../domain/models/runtime_models.dart';
import '../domain/models/turn_models.dart';
import 'app_localizations.dart';

extension StudioLocalizationsX on BuildContext {
  AppLocalizations get l10n => AppLocalizations.of(this);

  String compileModeLabel(StudioMode mode) {
    return switch (mode) {
      StudioMode.simple => l10n.compileModeSimple,
      StudioMode.task => l10n.compileModeTask,
    };
  }

  String permissionModeLabel(PermissionMode mode) {
    return switch (mode) {
      PermissionMode.requestApproval => l10n.permissionModeRequestApproval,
      PermissionMode.autoReview => l10n.permissionModeAutoReview,
      PermissionMode.fullAccess => l10n.permissionModeFullAccess,
    };
  }

  String turnActivityLabel(StudioTurnActivity activity) {
    return activity.label(l10n);
  }

  String interactionKindLabel(InteractionKind kind) {
    return switch (kind) {
      InteractionKind.toolApproval => l10n.statusInteractionToolApproval,
      InteractionKind.userInput => l10n.statusInteractionUserInput,
      InteractionKind.planConfirmation =>
        l10n.statusInteractionPlanConfirmation,
    };
  }

  String taskPhaseLabel(TaskStateKind phase) {
    return switch (phase) {
      TaskStateKind.designUpdating => l10n.statusTaskPhaseDesignUpdating,
      TaskStateKind.implementing => l10n.statusTaskPhaseImplementing,
      TaskStateKind.merging => l10n.statusTaskPhaseMerging,
      TaskStateKind.reviewing => l10n.statusTaskPhaseReviewing,
      TaskStateKind.reworking => l10n.statusTaskPhaseReworking,
      TaskStateKind.stopping => l10n.statusTaskPhaseStopping,
      TaskStateKind.blocked => l10n.statusTaskPhaseBlocked,
      TaskStateKind.completed => l10n.statusTaskPhaseCompleted,
      TaskStateKind.failed => l10n.statusTaskPhaseFailed,
      TaskStateKind.cancelled => l10n.statusTaskPhaseCancelled,
    };
  }

  String taskStatusLabel(String status) {
    return switch (status) {
      'pending' || 'pendingDispatch' => l10n.statusTaskStatusPending,
      'dispatched' => l10n.statusTaskStatusQueued,
      'queued' => l10n.statusTaskStatusQueued,
      'running' => l10n.statusTaskStatusRunning,
      'awaitingCompletion' => l10n.statusTaskStatusAwaitingCompletion,
      'readyForCompletion' => l10n.statusTaskStatusAwaitingCompletion,
      'readyForReview' => l10n.statusTaskStatusReadyForReview,
      'reviewing' => l10n.statusTaskStatusReviewing,
      'changesRequested' => l10n.statusTaskStatusChangesRequested,
      'approved' => l10n.statusTaskStatusApproved,
      'merged' => l10n.statusTaskStatusMerged,
      'noDelivery' => l10n.statusTaskStatusNoDelivery,
      'completed' => l10n.statusTaskStatusCompleted,
      'budgetLimited' => l10n.statusTaskStatusBudgetLimited,
      'needsAttention' => l10n.statusTaskStatusNeedsAttention,
      'failed' => l10n.statusTaskStatusFailed,
      'cancelled' => l10n.statusTaskStatusCancelled,
      'passed' => l10n.statusTaskStatusPass,
      'changesRequired' => l10n.statusTaskStatusChangesRequired,
      'blocked' => l10n.statusTaskStatusBlocked,
      _ => status,
    };
  }

  String taskBudgetKindLabel(TaskBudgetLimitKindView kind) {
    return switch (kind) {
      TaskBudgetLimitKindView.modelStep => l10n.statusTaskBudgetModelStep,
      TaskBudgetLimitKindView.toolCall => l10n.statusTaskBudgetToolCall,
      TaskBudgetLimitKindView.wait => l10n.statusTaskBudgetWait,
      TaskBudgetLimitKindView.wallClock => l10n.statusTaskBudgetWallClock,
      TaskBudgetLimitKindView.agentCount => l10n.statusTaskBudgetAgentCount,
      TaskBudgetLimitKindView.agentDepth => l10n.statusTaskBudgetAgentDepth,
      TaskBudgetLimitKindView.finalization => l10n.statusTaskBudgetFinalization,
    };
  }

  String taskContinuationStateLabel(String state) {
    return switch (state) {
      'none' => l10n.statusTaskContinuationNone,
      'compacting' => l10n.statusTaskContinuationCompacting,
      'pendingStart' => l10n.statusTaskContinuationPendingStart,
      'needsAttention' => l10n.statusTaskContinuationNeedsAttention,
      _ => state,
    };
  }

  /// Maps a protocol role key to its localized display name.
  ///
  /// Fixed roles translate to the current locale; an empty role falls back to a
  /// generic label; unknown extension roles are returned unchanged so custom
  /// identities are never hidden or rewritten.
  String roleLabel(String role) {
    return switch (role.trim()) {
      'explorer' => l10n.roleExplorer,
      'planner' => l10n.rolePlanner,
      'executor' => l10n.roleExecutor,
      'reviewer' => l10n.roleReviewer,
      '' => l10n.roleEmpty,
      _ => role,
    };
  }
}

extension StudioTurnActivityX on StudioTurnActivity {
  IconData get icon => switch (this) {
    StudioTurnActivity.preparing => Icons.menu_book_outlined,
    StudioTurnActivity.thinking => Icons.psychology_alt_outlined,
    StudioTurnActivity.responding => Icons.edit_note_outlined,
    StudioTurnActivity.planning => Icons.route_outlined,
    StudioTurnActivity.runningTool => Icons.build_outlined,
    StudioTurnActivity.persisting => Icons.save_outlined,
  };

  bool get drivesToolGroup => this == StudioTurnActivity.runningTool;

  String label(AppLocalizations l10n) => switch (this) {
    StudioTurnActivity.preparing => l10n.statusTurnPreparing,
    StudioTurnActivity.thinking => l10n.timelineReasoningActive,
    StudioTurnActivity.responding => l10n.statusTurnResponding,
    StudioTurnActivity.planning => l10n.statusTurnPlanning,
    StudioTurnActivity.runningTool => l10n.statusTurnRunningTool,
    StudioTurnActivity.persisting => l10n.statusTurnPersisting,
  };
}

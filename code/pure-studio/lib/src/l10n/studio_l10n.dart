import 'package:flutter/widgets.dart';

import '../domain/models/studio_enums.dart';
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
    return switch (activity) {
      StudioTurnActivity.preparing => l10n.statusTurnPreparing,
      StudioTurnActivity.thinking => l10n.timelineReasoningActive,
      StudioTurnActivity.responding => l10n.statusTurnResponding,
      StudioTurnActivity.planning => l10n.statusTurnPlanning,
      StudioTurnActivity.runningTool => l10n.statusTurnRunningTool,
      StudioTurnActivity.waitingForApproval =>
        l10n.statusTurnWaitingForApproval,
      StudioTurnActivity.waitingForUserInput =>
        l10n.statusTurnWaitingForUserInput,
      StudioTurnActivity.waitingForPlanConfirmation =>
        l10n.statusTurnWaitingForPlanConfirmation,
      StudioTurnActivity.persisting => l10n.statusTurnPersisting,
    };
  }

  String interactionKindLabel(InteractionKind kind) {
    return switch (kind) {
      InteractionKind.toolApproval => l10n.statusInteractionToolApproval,
      InteractionKind.userInput => l10n.statusInteractionUserInput,
      InteractionKind.planConfirmation =>
        l10n.statusInteractionPlanConfirmation,
    };
  }

  String taskPhaseLabel(String phase) {
    return switch (phase) {
      'planning' => l10n.statusTaskPhasePlanning,
      'pendingConfirmation' => l10n.statusTaskPhasePendingConfirmation,
      'designUpdating' => l10n.statusTaskPhaseDesignUpdating,
      'implementing' => l10n.statusTaskPhaseImplementing,
      'merging' => l10n.statusTaskPhaseMerging,
      'reviewing' => l10n.statusTaskPhaseReviewing,
      'reworking' => l10n.statusTaskPhaseReworking,
      'stopping' => l10n.statusTaskPhaseStopping,
      'completed' => l10n.statusTaskPhaseCompleted,
      'blocked' => l10n.statusTaskPhaseBlocked,
      'failed' => l10n.statusTaskPhaseFailed,
      'cancelled' => l10n.statusTaskPhaseCancelled,
      _ => phase,
    };
  }

  String taskStatusLabel(String status) {
    return switch (status) {
      'pending' => l10n.statusTaskStatusPending,
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
      'failed' => l10n.statusTaskStatusFailed,
      'cancelled' => l10n.statusTaskStatusCancelled,
      'pass' => l10n.statusTaskStatusPass,
      'changesRequired' => l10n.statusTaskStatusChangesRequired,
      'blocked' => l10n.statusTaskStatusBlocked,
      _ => status,
    };
  }
}

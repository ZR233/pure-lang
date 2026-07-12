import 'package:flutter/widgets.dart';

import '../domain/models/studio_enums.dart';
import 'app_localizations.dart';

extension StudioLocalizationsX on BuildContext {
  AppLocalizations get l10n => AppLocalizations.of(this);

  String compileModeLabel(CompileMode mode) {
    return switch (mode) {
      CompileMode.simple => l10n.compileModeSimple,
      CompileMode.task => l10n.compileModeTask,
    };
  }

  String permissionModeLabel(PermissionMode mode) {
    return switch (mode) {
      PermissionMode.requestApproval => l10n.permissionModeRequestApproval,
      PermissionMode.autoReview => l10n.permissionModeAutoReview,
      PermissionMode.fullAccess => l10n.permissionModeFullAccess,
    };
  }

  String turnPhaseLabel(TurnPhase phase) {
    return switch (phase) {
      TurnPhase.idle => l10n.statusTurnIdle,
      TurnPhase.queued => l10n.statusTurnQueued,
      TurnPhase.contextLoading => l10n.statusTurnContextLoading,
      TurnPhase.waitingForModel => l10n.statusTurnWaitingForModel,
      TurnPhase.streaming => l10n.statusTurnStreaming,
      TurnPhase.waitingForInteraction => l10n.statusTurnWaitingForInteraction,
      TurnPhase.runningTool => l10n.statusTurnRunningTool,
      TurnPhase.completed => l10n.statusTurnCompleted,
      TurnPhase.failed => l10n.statusTurnFailed,
      TurnPhase.cancelled => l10n.statusTurnCancelled,
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
      'resolvingConflict' => l10n.statusTaskPhaseResolvingConflict,
      'reviewing' => l10n.statusTaskPhaseReviewing,
      'reworking' => l10n.statusTaskPhaseReworking,
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
      'waitingForDelivery' => l10n.statusTaskStatusWaitingForDelivery,
      'delivered' => l10n.statusTaskStatusDelivered,
      'merged' => l10n.statusTaskStatusMerged,
      'completed' => l10n.statusTaskStatusCompleted,
      'failed' => l10n.statusTaskStatusFailed,
      'cancelled' => l10n.statusTaskStatusCancelled,
      'conflicted' => l10n.statusTaskStatusConflicted,
      'verifying' => l10n.statusTaskStatusVerifying,
      'aborted' => l10n.statusTaskStatusAborted,
      'pass' => l10n.statusTaskStatusPass,
      'changesRequired' => l10n.statusTaskStatusChangesRequired,
      'blocked' => l10n.statusTaskStatusBlocked,
      _ => status,
    };
  }
}

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
}

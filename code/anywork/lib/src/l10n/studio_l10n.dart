import 'package:flutter/material.dart';

import '../domain/models/attachment_models.dart';
import '../domain/models/provider_models.dart';
import '../domain/models/studio_enums.dart';
import '../domain/models/turn_models.dart';
import 'app_localizations.dart';

extension StudioLocalizationsX on BuildContext {
  AppLocalizations get l10n => AppLocalizations.of(this);

  String compileModeLabel(ThreadModeId mode) {
    if (mode == ThreadModeId.simple) return l10n.compileModeSimple;
    if (mode == ThreadModeId.task) return l10n.compileModeTask;
    return mode.id;
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
    };
  }

  /// Maps a model modality to its localized display name.
  String modalityLabel(ModelModalityView modality) {
    return switch (modality) {
      ModelModalityView.text => l10n.modalityText,
      ModelModalityView.image => l10n.modalityImage,
      ModelModalityView.audio => l10n.modalityAudio,
      ModelModalityView.video => l10n.modalityVideo,
      ModelModalityView.file => l10n.modalityFile,
    };
  }

  /// Maps an attachment modality to its localized display name.
  String attachmentModalityLabel(AttachmentModalityView modality) {
    return switch (modality) {
      AttachmentModalityView.image => l10n.modalityImage,
      AttachmentModalityView.video => l10n.modalityVideo,
      AttachmentModalityView.file => l10n.modalityFile,
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
      'worktree_executor' => l10n.roleWorktreeExecutor,
      'reviewer' => l10n.roleReviewer,
      '' => l10n.roleEmpty,
      _ => role,
    };
  }

  /// Maps a fixed agent role key to its localized responsibility summary.
  ///
  /// System Agent cards and route rows share this single mapping so the same
  /// role always renders the same description in the active locale; unknown
  /// roles fall back to the generic summary instead of leaking runtime
  /// metadata, while user profiles never reach this mapping.
  String roleDescription(String role) {
    return switch (role.trim()) {
      'explorer' => l10n.settingsRoleExplorerDescription,
      'planner' => l10n.settingsRolePlannerDescription,
      'executor' => l10n.settingsRoleExecutorDescription,
      'worktree_executor' => l10n.settingsRoleWorktreeExecutorDescription,
      'reviewer' => l10n.settingsRoleReviewerDescription,
      _ => l10n.settingsRoleFallbackDescription,
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

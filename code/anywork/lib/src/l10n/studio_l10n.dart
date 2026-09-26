import 'package:flutter/material.dart';

import '../domain/models/attachment_models.dart';
import '../domain/models/conversation_activity_models.dart';
import '../domain/models/provider_models.dart';
import '../domain/models/runtime_models.dart';
import '../domain/models/studio_enums.dart';
import '../domain/models/thread_directory_models.dart';
import 'app_localizations.dart';

extension StudioLocalizationsX on BuildContext {
  AppLocalizations get l10n => AppLocalizations.of(this);

  String compileModeLabel(ThreadModeId mode) {
    if (mode == ThreadModeId.simple) return l10n.compileModeSimple;
    if (mode == ThreadModeId.task) return l10n.compileModeTask;
    return mode.id;
  }

  /// Maps a canonical workflow state id to its localized display label.
  ///
  /// Only the built-in Task workflow phases are localized; an unknown or
  /// future state id is returned unchanged so canonical identity is never
  /// hidden behind a wrong translation.
  String workflowStateLabel(String stateId) {
    return switch (stateId.trim()) {
      'planning' => l10n.workflowStatePlanning,
      'editing_documents' => l10n.workflowStateEditingDocuments,
      'working' => l10n.workflowStateWorking,
      'integrating' => l10n.workflowStateIntegrating,
      'reviewing' => l10n.workflowStateReviewing,
      'completed' => l10n.workflowStateCompleted,
      'stopped' => l10n.workflowStateStopped,
      _ => stateId,
    };
  }

  /// Maps a canonical built-in thread mode id to its localized display label.
  ///
  /// Custom or unknown modes are returned unchanged.
  String workflowModeLabel(String modeId) {
    return switch (modeId.trim()) {
      'mode.simple' => l10n.workflowModeSimple,
      'mode.task' => l10n.workflowModeTask,
      _ => modeId,
    };
  }

  /// 吞吐量标签：数值由领域层提供，单位按 locale 追加（如“词元/秒”）。
  String tokenThroughputLabel(double? tokensPerSecond) {
    final value = formatTokenThroughputValue(tokensPerSecond);
    return value == null
        ? l10n.statusThroughputUnavailable
        : l10n.statusThroughputValue(value);
  }

  /// Maps a canonical wire protocol to its localized display label.
  String modelProtocolLabel(String protocol) {
    return switch (protocol) {
      'responses' => l10n.modelProtocolResponses,
      'chat_completions' => l10n.modelProtocolChatCompletions,
      _ => protocol,
    };
  }

  /// Maps a canonical connection mode to its localized display label.
  String modelConnectionLabel(String mode) {
    return switch (mode) {
      'web_socket' => l10n.modelConnectionWebSocket,
      'http' => l10n.modelConnectionHttp,
      _ => mode,
    };
  }

  String permissionModeLabel(PermissionMode mode) {
    return switch (mode) {
      PermissionMode.requestApproval => l10n.permissionModeRequestApproval,
      PermissionMode.autoReview => l10n.permissionModeAutoReview,
      PermissionMode.fullAccess => l10n.permissionModeFullAccess,
    };
  }

  /// 固定活动条的准确阶段标签。
  ///
  /// 与 Timeline 的推理/工具组标签彼此独立：那里描述“历史上发生过什么”，这里只描述
  /// “此刻在做什么”。等待 API 与已开始 reasoning 都显示“思考中”；保存被明确阻塞时
  /// 显示准确原因，而不是回落到“思考中”。存储状态按 typed `canResume` 区分“已恢复
  /// 待继续”“等待保存恢复”与“具体故障”，绝不解析错误文本来判定。
  String conversationActivityLabel(ConversationActivityView view) {
    final storage = view.storage;
    return switch (view.kind) {
      ConversationActivityKind.preparing => l10n.conversationActivityPreparing,
      ConversationActivityKind.thinking => l10n.timelineReasoningActive,
      ConversationActivityKind.responding => l10n.statusTurnResponding,
      ConversationActivityKind.planning => l10n.statusTurnPlanning,
      ConversationActivityKind.runningTool =>
        l10n.conversationActivityRunningTool,
      ConversationActivityKind.awaitingApproval =>
        l10n.statusInteractionToolApproval,
      ConversationActivityKind.awaitingInput => l10n.statusInteractionUserInput,
      ConversationActivityKind.stopping => l10n.conversationActivityStopping,
      ConversationActivityKind.storageBlocked =>
        storage == null
            ? l10n.conversationActivitySaveFailed
            // 已核验可继续（`canResume`）表示保存已恢复、水位核验通过；core 的故障闩在
            // 显式继续前可能仍保留上一次的错误文本，此时不能显示“保存故障”，而应明确
            // “已保存，等待继续执行”。
            : storage.resumeRequired && storage.canResume
            ? l10n.conversationActivityResumeReady
            : storage.hasFault
            ? l10n.conversationActivitySaveFailed
            : storage.pressurePaused
            ? l10n.conversationActivityStoragePressure
            : storage.resumeRequired
            ? l10n.conversationActivityResumeRequired
            : l10n.conversationActivitySavePaused,
    };
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

  /// Maps a thread lifecycle status to its localized display label.
  ///
  /// The header agent switcher and the status bar share this single mapping so
  /// the same canonical status always renders the same localized text.
  String threadStatusLabel(ThreadStatusView status) {
    return switch (status) {
      ThreadStatusView.idle => l10n.threadStatusIdle,
      ThreadStatusView.queued => l10n.agentDetailStatusQueued,
      ThreadStatusView.running => l10n.agentDetailStatusRunning,
      ThreadStatusView.waitingTool => l10n.statusTurnWaitingForApproval,
      ThreadStatusView.waitingInteraction => l10n.statusTurnWaitingForUserInput,
      ThreadStatusView.cancelling => l10n.threadStatusCancelling,
      ThreadStatusView.closing => l10n.agentDetailStatusClosing,
      ThreadStatusView.closed => l10n.threadStatusClosed,
      ThreadStatusView.faulted => l10n.agentDetailStatusErrored,
    };
  }

  /// Maps a canonical tool status to its localized status-pill label.
  ///
  /// Unknown statuses are returned unchanged so extension or future values are
  /// never hidden behind a wrong translation.
  String toolStatusLabel(String status) {
    return switch (status.trim()) {
      'queued' => l10n.agentDetailStatusQueued,
      'running' ||
      'streaming' ||
      'approved' ||
      'started' => l10n.agentDetailStatusRunning,
      'awaitingApproval' => l10n.toolStatusAwaitingApproval,
      'succeeded' => l10n.agentDetailStatusCompleted,
      'failed' => l10n.toolStatusFailed,
      'denied' => l10n.toolStatusDenied,
      'cancelled' => l10n.toolStatusCancelled,
      'cancelling' => l10n.toolStatusCancelling,
      'interrupted' => l10n.agentDetailStatusInterrupted,
      _ => status,
    };
  }

  /// Maps a canonical todo item status to its localized status-pill label.
  String todoStatusLabel(String status) {
    return switch (status.trim()) {
      'completed' => l10n.timelineTodoCompleted,
      'inProgress' => l10n.timelineTodoInProgress,
      'pending' => l10n.timelineTodoPending,
      _ => status,
    };
  }

  /// Maps a canonical provider status to its localized display label.
  ///
  /// `ready` and `missingCredential` are the only canonical statuses today;
  /// any other value is returned unchanged so unknown or future statuses are
  /// never mislabeled as a configuration state.
  String providerStatusLabel(String status) {
    return switch (status.trim()) {
      'ready' => l10n.settingsReadyBadge,
      'missingCredential' => l10n.settingsProviderMissingCredential,
      _ => status,
    };
  }

  /// Maps a canonical SSH connection state to its localized display label.
  String sshConnectionStateLabel(String state) {
    return switch (state.trim()) {
      'ready' => l10n.settingsSshReady,
      'disconnected' => l10n.settingsSshStateDisconnected,
      'connecting' => l10n.settingsSshStateConnecting,
      'waitingForInput' => l10n.settingsSshStateWaitingForInput,
      'reconnecting' => l10n.settingsSshStateReconnecting,
      'failed' => l10n.settingsSshStateFailed,
      _ => state,
    };
  }

  /// Maps a canonical worktree lease state to its localized display label.
  String worktreeStateLabel(String state) {
    return switch (state.trim()) {
      'prepared' => l10n.settingsWorktreeStatePrepared,
      'active' => l10n.settingsWorktreeStateActive,
      'preserved' => l10n.settingsWorktreeStatePreserved,
      'cleanupRequested' => l10n.settingsWorktreeStateCleanupRequested,
      'cleaned' => l10n.settingsWorktreeStateCleaned,
      _ => state,
    };
  }
}

extension ConversationActivityKindX on ConversationActivityKind {
  IconData get icon => switch (this) {
    ConversationActivityKind.preparing => Icons.menu_book_outlined,
    ConversationActivityKind.thinking => Icons.psychology_alt_outlined,
    ConversationActivityKind.responding => Icons.edit_note_outlined,
    ConversationActivityKind.planning => Icons.route_outlined,
    ConversationActivityKind.runningTool => Icons.build_outlined,
    ConversationActivityKind.awaitingApproval => Icons.verified_outlined,
    ConversationActivityKind.awaitingInput => Icons.help_outline,
    ConversationActivityKind.stopping => Icons.stop_circle_outlined,
    ConversationActivityKind.storageBlocked => Icons.error_outline,
  };

  /// 保存故障使用错误色，其余状态跟随中性/强调色。
  bool get isIssue => this == ConversationActivityKind.storageBlocked;
}

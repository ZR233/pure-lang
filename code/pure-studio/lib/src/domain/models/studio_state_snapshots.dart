import 'agent_models.dart';
import 'provider_models.dart';
import 'recovery_models.dart';
import 'runtime_models.dart';
import 'settings_models.dart';
import 'studio_enums.dart';
import 'thread_directory_models.dart';

enum ObservedStatePhase { uninitialized, ready, running, failed, stopped }

class ObservedStateMeta {
  const ObservedStateMeta({
    required this.revision,
    required this.phase,
    required this.updatedAt,
    this.lastCheckedAt,
    required this.stale,
    this.operation,
    this.operationId,
    this.errorCode,
    this.errorMessage,
    this.retryable = false,
  });

  const ObservedStateMeta.initial()
    : revision = 0,
      phase = ObservedStatePhase.uninitialized,
      updatedAt = null,
      lastCheckedAt = null,
      stale = false,
      operation = null,
      operationId = null,
      errorCode = null,
      errorMessage = null,
      retryable = false;

  final int revision;
  final ObservedStatePhase phase;
  final DateTime? updatedAt;
  final DateTime? lastCheckedAt;
  final bool stale;
  final String? operation;
  final String? operationId;
  final String? errorCode;
  final String? errorMessage;
  final bool retryable;

  bool isNewerThan(ObservedStateMeta current) => revision > current.revision;
}

abstract interface class ObservedStateSnapshot {
  ObservedStateMeta get meta;
}

class SettingsStateSnapshot implements ObservedStateSnapshot {
  const SettingsStateSnapshot({
    this.meta = const ObservedStateMeta.initial(),
    this.providers = const [],
    this.defaultProviderId,
    this.roles = const [],
    this.mcpServers = const [],
    this.instructions = const InstructionsSettingsView(),
    this.skills = const SkillsSettingsView(),
    this.general = const GeneralSettingsView(),
    this.webSearch = const WebSearchSettingsView(),
    this.permissionMode = PermissionMode.requestApproval,
  });

  @override
  final ObservedStateMeta meta;
  final List<ProviderSettingsView> providers;
  final String? defaultProviderId;
  final List<RoleSettingsView> roles;
  final List<McpServerSettingsView> mcpServers;
  final InstructionsSettingsView instructions;
  final SkillsSettingsView skills;
  final GeneralSettingsView general;
  final WebSearchSettingsView webSearch;
  final PermissionMode permissionMode;
}

class LspServerStateView {
  const LspServerStateView({
    required this.id,
    required this.displayName,
    required this.availability,
    this.message,
    this.lastCheckedAt,
    this.lastError,
    this.diagnosticCount = 0,
    this.activityKind = 'idle',
    this.activityTitle,
    this.activityMessage,
    this.activityPercentage,
  });

  final String id;
  final String displayName;
  final String availability;
  final String? message;
  final DateTime? lastCheckedAt;
  final String? lastError;
  final int diagnosticCount;
  final String activityKind;
  final String? activityTitle;
  final String? activityMessage;
  final int? activityPercentage;
}

class McpStateSnapshot implements ObservedStateSnapshot {
  const McpStateSnapshot({
    this.meta = const ObservedStateMeta.initial(),
    this.desiredConfigFingerprint = '',
    this.appliedConfigFingerprint = '',
    this.activeServers = const [],
    this.servers = const [],
  });

  @override
  final ObservedStateMeta meta;
  final String desiredConfigFingerprint;
  final String appliedConfigFingerprint;
  final List<String> activeServers;
  final List<McpServerSettingsView> servers;
}

class LspStateSnapshot implements ObservedStateSnapshot {
  const LspStateSnapshot({
    this.meta = const ObservedStateMeta.initial(),
    this.activeServers = const [],
    this.servers = const [],
  });

  @override
  final ObservedStateMeta meta;
  final List<String> activeServers;
  final List<LspServerStateView> servers;
}

class SkillsStateSnapshot implements ObservedStateSnapshot {
  const SkillsStateSnapshot({
    required this.meta,
    required this.projectId,
    required this.configFingerprint,
    required this.catalogRevision,
    required this.skills,
    required this.warnings,
  });

  @override
  final ObservedStateMeta meta;
  final String projectId;
  final String configFingerprint;
  final int catalogRevision;
  final List<String> skills;
  final List<String> warnings;
}

class ProviderUsageStateSnapshot implements ObservedStateSnapshot {
  const ProviderUsageStateSnapshot({
    this.meta = const ObservedStateMeta.initial(),
    this.configFingerprint = '',
    this.usages = const [],
  });

  @override
  final ObservedStateMeta meta;
  final String configFingerprint;
  final List<ProviderUsageView> usages;
}

class UpdaterStateSnapshot implements ObservedStateSnapshot {
  const UpdaterStateSnapshot({
    this.meta = const ObservedStateMeta.initial(),
    this.version,
    this.publishedAt,
    this.notesUrl,
  });

  @override
  final ObservedStateMeta meta;
  final String? version;
  final DateTime? publishedAt;
  final String? notesUrl;
}

class DirectoryStateSnapshot<T> implements ObservedStateSnapshot {
  const DirectoryStateSnapshot({
    this.meta = const ObservedStateMeta.initial(),
    this.values = const [],
  });

  @override
  final ObservedStateMeta meta;
  final List<T> values;
}

class TaskDirectoryEntryView {
  const TaskDirectoryEntryView({
    required this.rootThreadId,
    required this.task,
  });

  final String rootThreadId;
  final TaskRuntimeView task;
}

typedef ProjectDirectoryState = DirectoryStateSnapshot<StudioProject>;
typedef ThreadDirectoryState = DirectoryStateSnapshot<StudioThread>;
typedef TaskDirectoryState = DirectoryStateSnapshot<TaskDirectoryEntryView>;
typedef AgentDirectoryState = DirectoryStateSnapshot<StudioAgentView>;
typedef RecoveryStateSnapshot = DirectoryStateSnapshot<StudioRecoveryIssue>;

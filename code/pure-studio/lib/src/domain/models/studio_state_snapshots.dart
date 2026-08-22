import 'agent_models.dart';
import 'provider_models.dart';
import 'recovery_models.dart';
import 'runtime_models.dart';
import 'settings_models.dart';
import 'studio_enums.dart';
import 'thread_directory_models.dart';

sealed class ObservedResource<T> {
  const ObservedResource();

  int get revision => switch (this) {
    UninitializedObservedResource<T>(:final revision) ||
    LoadingObservedResource<T>(:final revision) ||
    ReadyObservedResource<T>(:final revision) ||
    RefreshingObservedResource<T>(:final revision) ||
    StaleObservedResource<T>(:final revision) ||
    DegradedObservedResource<T>(:final revision) ||
    FailedObservedResource<T>(:final revision) ||
    StoppedObservedResource<T>(:final revision) => revision,
  };

  T? get value => switch (this) {
    ReadyObservedResource<T>(:final value) ||
    RefreshingObservedResource<T>(:final value) ||
    StaleObservedResource<T>(:final value) ||
    DegradedObservedResource<T>(:final value) => value,
    UninitializedObservedResource<T>() ||
    LoadingObservedResource<T>() ||
    FailedObservedResource<T>() ||
    StoppedObservedResource<T>() => null,
  };

  bool isNewerThan(ObservedResource<Object?> current) =>
      revision > current.revision;
}

final class UninitializedObservedResource<T> extends ObservedResource<T> {
  const UninitializedObservedResource({
    this.revision = 0,
    required this.updatedAt,
  });
  @override
  final int revision;
  final int updatedAt;
}

final class LoadingObservedResource<T> extends ObservedResource<T> {
  const LoadingObservedResource({
    required this.revision,
    required this.operation,
    required this.operationId,
    required this.startedAt,
  });
  @override
  final int revision;
  final String operation;
  final String operationId;
  final int startedAt;
}

final class ReadyObservedResource<T> extends ObservedResource<T> {
  const ReadyObservedResource({
    required this.revision,
    required this.updatedAt,
    required this.lastCheckedAt,
    required this.value,
  });
  @override
  final int revision;
  final int updatedAt;
  final int? lastCheckedAt;
  @override
  final T value;
}

final class RefreshingObservedResource<T> extends ObservedResource<T> {
  const RefreshingObservedResource({
    required this.revision,
    required this.operation,
    required this.operationId,
    required this.startedAt,
    required this.lastCheckedAt,
    required this.value,
  });
  @override
  final int revision;
  final String operation;
  final String operationId;
  final int startedAt;
  final int? lastCheckedAt;
  @override
  final T value;
}

final class StaleObservedResource<T> extends ObservedResource<T> {
  const StaleObservedResource({
    required this.revision,
    required this.staleAt,
    required this.lastCheckedAt,
    required this.value,
  });
  @override
  final int revision;
  final int staleAt;
  final int? lastCheckedAt;
  @override
  final T value;
}

class ObservedResourceError {
  const ObservedResourceError({
    required this.code,
    required this.message,
    required this.retryable,
  });
  final String code;
  final String message;
  final bool retryable;
}

final class DegradedObservedResource<T> extends ObservedResource<T> {
  const DegradedObservedResource({
    required this.revision,
    required this.failedAt,
    required this.lastCheckedAt,
    required this.operation,
    required this.error,
    required this.value,
  });
  @override
  final int revision;
  final int failedAt;
  final int? lastCheckedAt;
  final String operation;
  final ObservedResourceError error;
  @override
  final T value;
}

final class FailedObservedResource<T> extends ObservedResource<T> {
  const FailedObservedResource({
    required this.revision,
    required this.failedAt,
    required this.operation,
    required this.error,
  });
  @override
  final int revision;
  final int failedAt;
  final String operation;
  final ObservedResourceError error;
}

final class StoppedObservedResource<T> extends ObservedResource<T> {
  const StoppedObservedResource({
    required this.revision,
    required this.stoppedAt,
  });
  @override
  final int revision;
  final int stoppedAt;
}

abstract class ObservedStateSnapshot<T> {
  const ObservedStateSnapshot();
  ObservedResource<T> get state;
  int get revision => state.revision;
}

class SettingsStateData {
  const SettingsStateData({
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

class SettingsStateSnapshot extends ObservedStateSnapshot<SettingsStateData> {
  SettingsStateSnapshot({
    List<ProviderSettingsView> providers = const [],
    String? defaultProviderId,
    List<RoleSettingsView> roles = const [],
    List<McpServerSettingsView> mcpServers = const [],
    InstructionsSettingsView instructions = const InstructionsSettingsView(),
    SkillsSettingsView skills = const SkillsSettingsView(),
    GeneralSettingsView general = const GeneralSettingsView(),
    WebSearchSettingsView webSearch = const WebSearchSettingsView(),
    PermissionMode permissionMode = PermissionMode.requestApproval,
    int revision = 0,
  }) : state = ReadyObservedResource<SettingsStateData>(
         revision: revision,
         updatedAt: 0,
         lastCheckedAt: null,
         value: SettingsStateData(
           providers: providers,
           defaultProviderId: defaultProviderId,
           roles: roles,
           mcpServers: mcpServers,
           instructions: instructions,
           skills: skills,
           general: general,
           webSearch: webSearch,
           permissionMode: permissionMode,
         ),
       );

  const SettingsStateSnapshot.fromState({required this.state});

  @override
  final ObservedResource<SettingsStateData> state;
  SettingsStateData get _data => state.value ?? const SettingsStateData();
  List<ProviderSettingsView> get providers => _data.providers;
  String? get defaultProviderId => _data.defaultProviderId;
  List<RoleSettingsView> get roles => _data.roles;
  List<McpServerSettingsView> get mcpServers => _data.mcpServers;
  InstructionsSettingsView get instructions => _data.instructions;
  SkillsSettingsView get skills => _data.skills;
  GeneralSettingsView get general => _data.general;
  WebSearchSettingsView get webSearch => _data.webSearch;
  PermissionMode get permissionMode => _data.permissionMode;
}

class LspServerStateView {
  const LspServerStateView({
    required this.id,
    required this.displayName,
    required this.state,
  });

  final String id;
  final String displayName;
  final LspServerState state;
}

sealed class LspServerState {
  const LspServerState();
}

final class LspCheckingState extends LspServerState {
  const LspCheckingState({required this.message});
  final String message;
}

final class LspAvailableState extends LspServerState {
  const LspAvailableState({
    required this.checkedAt,
    required this.diagnosticCount,
    required this.activity,
  });
  final int checkedAt;
  final int diagnosticCount;
  final LspActivity activity;
}

final class LspUnavailableState extends LspServerState {
  const LspUnavailableState({
    required this.checkedAt,
    required this.code,
    required this.message,
    required this.retryable,
  });
  final int checkedAt;
  final String code;
  final String message;
  final bool retryable;
}

final class LspDisabledState extends LspServerState {
  const LspDisabledState({required this.message});
  final String message;
}

sealed class LspActivity {
  const LspActivity();
}

final class LspIdleActivity extends LspActivity {
  const LspIdleActivity();
}

final class LspBusyActivity extends LspActivity {
  const LspBusyActivity({this.title, this.message, this.percentage});
  final String? title;
  final String? message;
  final int? percentage;
}

final class LspIndexingActivity extends LspActivity {
  const LspIndexingActivity({this.title, this.message, this.percentage});
  final String? title;
  final String? message;
  final int? percentage;
}

class McpStateData {
  const McpStateData({
    this.desiredConfigFingerprint = '',
    this.appliedConfigFingerprint = '',
    this.activeServers = const [],
    this.servers = const [],
  });

  final String desiredConfigFingerprint;
  final String appliedConfigFingerprint;
  final List<String> activeServers;
  final List<McpServerSettingsView> servers;
}

class McpStateSnapshot extends ObservedStateSnapshot<McpStateData> {
  McpStateSnapshot({
    String desiredConfigFingerprint = '',
    String appliedConfigFingerprint = '',
    List<String> activeServers = const [],
    List<McpServerSettingsView> servers = const [],
    int revision = 0,
  }) : state = ReadyObservedResource<McpStateData>(
         revision: revision,
         updatedAt: 0,
         lastCheckedAt: null,
         value: McpStateData(
           desiredConfigFingerprint: desiredConfigFingerprint,
           appliedConfigFingerprint: appliedConfigFingerprint,
           activeServers: activeServers,
           servers: servers,
         ),
       );

  const McpStateSnapshot.fromState({required this.state});
  @override
  final ObservedResource<McpStateData> state;
  McpStateData get _data => state.value ?? const McpStateData();
  String get desiredConfigFingerprint => _data.desiredConfigFingerprint;
  String get appliedConfigFingerprint => _data.appliedConfigFingerprint;
  List<String> get activeServers => _data.activeServers;
  List<McpServerSettingsView> get servers => _data.servers;
}

class LspStateData {
  const LspStateData({this.activeServers = const [], this.servers = const []});
  final List<String> activeServers;
  final List<LspServerStateView> servers;
}

class LspStateSnapshot extends ObservedStateSnapshot<LspStateData> {
  LspStateSnapshot({
    List<String> activeServers = const [],
    List<LspServerStateView> servers = const [],
    int revision = 0,
  }) : state = ReadyObservedResource<LspStateData>(
         revision: revision,
         updatedAt: 0,
         lastCheckedAt: null,
         value: LspStateData(activeServers: activeServers, servers: servers),
       );

  const LspStateSnapshot.fromState({required this.state});
  @override
  final ObservedResource<LspStateData> state;
  LspStateData get _data => state.value ?? const LspStateData();
  List<String> get activeServers => _data.activeServers;
  List<LspServerStateView> get servers => _data.servers;
}

class SkillsStateData {
  const SkillsStateData({
    required this.configFingerprint,
    required this.catalogRevision,
    required this.skills,
    required this.warnings,
  });
  final String configFingerprint;
  final int catalogRevision;
  final List<String> skills;
  final List<String> warnings;
}

class SkillsStateSnapshot extends ObservedStateSnapshot<SkillsStateData> {
  SkillsStateSnapshot({
    required this.projectId,
    required String configFingerprint,
    required int catalogRevision,
    required List<String> skills,
    required List<String> warnings,
    int revision = 0,
  }) : state = ReadyObservedResource<SkillsStateData>(
         revision: revision,
         updatedAt: 0,
         lastCheckedAt: null,
         value: SkillsStateData(
           configFingerprint: configFingerprint,
           catalogRevision: catalogRevision,
           skills: skills,
           warnings: warnings,
         ),
       );

  const SkillsStateSnapshot.fromState({
    required this.projectId,
    required this.state,
  });

  @override
  final ObservedResource<SkillsStateData> state;
  final String projectId;
  SkillsStateData get _data =>
      state.value ??
      const SkillsStateData(
        configFingerprint: '',
        catalogRevision: 0,
        skills: [],
        warnings: [],
      );
  String get configFingerprint => _data.configFingerprint;
  int get catalogRevision => _data.catalogRevision;
  List<String> get skills => _data.skills;
  List<String> get warnings => _data.warnings;
}

class ProviderUsageStateData {
  const ProviderUsageStateData({
    this.configFingerprint = '',
    this.usages = const [],
  });
  final String configFingerprint;
  final List<ProviderUsageView> usages;
}

class ProviderUsageStateSnapshot
    extends ObservedStateSnapshot<ProviderUsageStateData> {
  ProviderUsageStateSnapshot({
    String configFingerprint = '',
    List<ProviderUsageView> usages = const [],
    int revision = 0,
  }) : state = ReadyObservedResource<ProviderUsageStateData>(
         revision: revision,
         updatedAt: 0,
         lastCheckedAt: null,
         value: ProviderUsageStateData(
           configFingerprint: configFingerprint,
           usages: usages,
         ),
       );

  const ProviderUsageStateSnapshot.fromState({required this.state});
  @override
  final ObservedResource<ProviderUsageStateData> state;
  ProviderUsageStateData get _data =>
      state.value ?? const ProviderUsageStateData();
  String get configFingerprint => _data.configFingerprint;
  List<ProviderUsageView> get usages => _data.usages;
}

sealed class UpdaterStateSnapshot {
  const UpdaterStateSnapshot();

  const factory UpdaterStateSnapshot.idle({
    required int revision,
    required DateTime updatedAt,
  }) = IdleUpdaterStateSnapshot;

  int get revision => switch (this) {
    DisabledUpdaterStateSnapshot(:final revision) ||
    IdleUpdaterStateSnapshot(:final revision) ||
    CheckingUpdaterStateSnapshot(:final revision) ||
    UpToDateUpdaterStateSnapshot(:final revision) ||
    AvailableUpdaterStateSnapshot(:final revision) ||
    DownloadingUpdaterStateSnapshot(:final revision) ||
    VerifyingUpdaterStateSnapshot(:final revision) ||
    InstallerLaunchedUpdaterStateSnapshot(:final revision) ||
    CheckFailedUpdaterStateSnapshot(:final revision) ||
    InstallFailedUpdaterStateSnapshot(:final revision) => revision,
  };

  DateTime get updatedAt => switch (this) {
    DisabledUpdaterStateSnapshot(:final updatedAt) ||
    IdleUpdaterStateSnapshot(:final updatedAt) ||
    DownloadingUpdaterStateSnapshot(:final updatedAt) ||
    VerifyingUpdaterStateSnapshot(:final updatedAt) => updatedAt,
    CheckingUpdaterStateSnapshot(:final startedAt) => startedAt,
    UpToDateUpdaterStateSnapshot(:final checkedAt) ||
    AvailableUpdaterStateSnapshot(:final checkedAt) => checkedAt,
    InstallerLaunchedUpdaterStateSnapshot(:final launchedAt) => launchedAt,
    CheckFailedUpdaterStateSnapshot(:final failedAt) ||
    InstallFailedUpdaterStateSnapshot(:final failedAt) => failedAt,
  };

  StudioUpdateInfoView? get update => switch (this) {
    AvailableUpdaterStateSnapshot(:final update) ||
    DownloadingUpdaterStateSnapshot(:final update) ||
    VerifyingUpdaterStateSnapshot(:final update) ||
    InstallerLaunchedUpdaterStateSnapshot(:final update) ||
    InstallFailedUpdaterStateSnapshot(:final update) => update,
    DisabledUpdaterStateSnapshot() ||
    IdleUpdaterStateSnapshot() ||
    CheckingUpdaterStateSnapshot() ||
    UpToDateUpdaterStateSnapshot() ||
    CheckFailedUpdaterStateSnapshot() => null,
  };

  bool get hasUpdate => switch (this) {
    AvailableUpdaterStateSnapshot() ||
    DownloadingUpdaterStateSnapshot() ||
    VerifyingUpdaterStateSnapshot() ||
    InstallFailedUpdaterStateSnapshot() => true,
    DisabledUpdaterStateSnapshot() ||
    IdleUpdaterStateSnapshot() ||
    CheckingUpdaterStateSnapshot() ||
    UpToDateUpdaterStateSnapshot() ||
    InstallerLaunchedUpdaterStateSnapshot() ||
    CheckFailedUpdaterStateSnapshot() => false,
  };
}

final class DisabledUpdaterStateSnapshot extends UpdaterStateSnapshot {
  const DisabledUpdaterStateSnapshot({
    required this.revision,
    required this.updatedAt,
  });

  @override
  final int revision;
  @override
  final DateTime updatedAt;
}

final class IdleUpdaterStateSnapshot extends UpdaterStateSnapshot {
  const IdleUpdaterStateSnapshot({
    required this.revision,
    required this.updatedAt,
  });

  @override
  final int revision;
  @override
  final DateTime updatedAt;
}

final class CheckingUpdaterStateSnapshot extends UpdaterStateSnapshot {
  const CheckingUpdaterStateSnapshot({
    required this.revision,
    required this.operationId,
    required this.startedAt,
  });

  @override
  final int revision;
  final String operationId;
  final DateTime startedAt;
}

final class UpToDateUpdaterStateSnapshot extends UpdaterStateSnapshot {
  const UpToDateUpdaterStateSnapshot({
    required this.revision,
    required this.checkedAt,
  });

  @override
  final int revision;
  final DateTime checkedAt;
}

final class AvailableUpdaterStateSnapshot extends UpdaterStateSnapshot {
  const AvailableUpdaterStateSnapshot({
    required this.revision,
    required this.checkedAt,
    required this.update,
  });

  @override
  final int revision;
  final DateTime checkedAt;
  @override
  final StudioUpdateInfoView update;
}

final class DownloadingUpdaterStateSnapshot extends UpdaterStateSnapshot {
  const DownloadingUpdaterStateSnapshot({
    required this.revision,
    required this.updatedAt,
    required this.update,
    required this.downloaded,
    required this.total,
  });

  @override
  final int revision;
  @override
  final DateTime updatedAt;
  @override
  final StudioUpdateInfoView update;
  final int downloaded;
  final int total;
}

final class VerifyingUpdaterStateSnapshot extends UpdaterStateSnapshot {
  const VerifyingUpdaterStateSnapshot({
    required this.revision,
    required this.updatedAt,
    required this.update,
    required this.downloaded,
    required this.total,
  });

  @override
  final int revision;
  @override
  final DateTime updatedAt;
  @override
  final StudioUpdateInfoView update;
  final int downloaded;
  final int total;
}

final class InstallerLaunchedUpdaterStateSnapshot extends UpdaterStateSnapshot {
  const InstallerLaunchedUpdaterStateSnapshot({
    required this.revision,
    required this.launchedAt,
    required this.update,
  });

  @override
  final int revision;
  final DateTime launchedAt;
  @override
  final StudioUpdateInfoView update;
}

final class CheckFailedUpdaterStateSnapshot extends UpdaterStateSnapshot {
  const CheckFailedUpdaterStateSnapshot({
    required this.revision,
    required this.failedAt,
    required this.error,
  });

  @override
  final int revision;
  final DateTime failedAt;
  final UpdaterErrorView error;
}

final class InstallFailedUpdaterStateSnapshot extends UpdaterStateSnapshot {
  const InstallFailedUpdaterStateSnapshot({
    required this.revision,
    required this.failedAt,
    required this.update,
    required this.error,
  });

  @override
  final int revision;
  final DateTime failedAt;
  @override
  final StudioUpdateInfoView update;
  final UpdaterErrorView error;
}

class StudioUpdateInfoView {
  const StudioUpdateInfoView({
    required this.version,
    required this.publishedAt,
    required this.notesUrl,
  });

  final String version;
  final DateTime publishedAt;
  final String notesUrl;
}

class UpdaterErrorView {
  const UpdaterErrorView({
    required this.code,
    required this.message,
    required this.retryable,
  });

  final String code;
  final String message;
  final bool retryable;
}

class DirectoryStateSnapshot<T> extends ObservedStateSnapshot<List<T>> {
  DirectoryStateSnapshot({List<T> values = const [], int revision = 0})
    : state = ReadyObservedResource<List<T>>(
        revision: revision,
        updatedAt: 0,
        lastCheckedAt: null,
        value: values,
      );

  const DirectoryStateSnapshot.fromState({required this.state});

  @override
  final ObservedResource<List<T>> state;
  List<T> get values => state.value ?? const [];
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

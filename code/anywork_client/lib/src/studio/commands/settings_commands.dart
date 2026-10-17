import 'dart:async';

import '../../../models.dart';
import '../../../studio_ports.dart';
import '../../../anywork_client.dart'
    show OperationHandle, OperationState, Observation;
import '../anywork_client.dart';
import '../stores/domain_sync.dart';
import 'command_lane.dart';

class SettingsCommands {
  SettingsCommands(this._client);
  final AnyworkClient _client;
  final CommandLane _lane = CommandLane();

  /// 供同 revision 的会话 route 写入复用这条 lane，避免与配置 CAS 交叉覆盖。
  Future<T> enqueueSettingsWrite<T>(Future<T> Function() action) =>
      _lane.enqueue(action);

  Future<SettingsStateSnapshot> applySettingsField(
    SettingsFieldCommand command, {
    String? mutationKey,
  }) {
    return _run(
      mutationKey ?? settingsFieldMutationKey(command),
      target: settingsFieldTarget(command),
      (revision, operationId) =>
          _api.applySettingsField(revision, command, operationId: operationId),
    );
  }

  Future<SettingsStateSnapshot> saveProvider(ProviderCommand command) {
    return _run(
      'provider:${command.id}',
      target: CommandTarget(providerId: command.id, label: command.name),
      (revision, operationId) =>
          _api.saveProvider(revision, command, operationId: operationId),
    );
  }

  /// 只持有目标模型意图；在串行 lane 内、每次 CAS 重试时都基于最新 canonical 重建。
  /// 其它模型覆盖、目录、endpoint 和凭据不会被发起输入时的旧快照覆盖。
  Future<SettingsStateSnapshot> setModelAutoCompactLimit(
    String providerId,
    String slug,
    int? limit,
  ) {
    if (limit != null && limit <= 0) {
      return Future.error(
        ArgumentError.value(limit, 'limit', 'Must be positive'),
      );
    }
    return _run(
      'model-auto-compact:${providerId.length}:$providerId:$slug',
      target: CommandTarget(providerId: providerId, model: slug),
      (revision, operationId) {
        final provider = _client.settings.state.canonical?.providers
            .where((provider) => provider.id == providerId)
            .firstOrNull;
        final model = provider?.models
            .where((model) => model.slug == slug)
            .firstOrNull;
        if (provider == null || model == null) {
          throw StudioFailure(
            code: StudioFailureCode.notFound,
            message: 'Provider model is unavailable: $providerId/$slug',
            retryable: false,
            correlationId: 'client-model-auto-compact',
          );
        }
        final updated = provider.withAutoCompactLimit(
          provider.autoCompactLimitFor(model).withOverride(limit),
        );
        final command = ProviderCommandBuilder.buildProvider(
          updated,
          originalId: provider.id,
          secret: const ProviderSecretCommand.preserve(),
        );
        return _api.saveProvider(revision, command, operationId: operationId);
      },
    );
  }

  Future<SettingsStateSnapshot> removeProviderModel(
    String providerId,
    String slug,
  ) {
    return _run(
      'provider-model:${providerId.length}:$providerId:$slug',
      target: CommandTarget(providerId: providerId, model: slug),
      (revision, _) => _api.removeProviderModel(revision, providerId, slug),
    );
  }

  Future<SettingsStateSnapshot> removeProvider(
    String providerId, {
    String? replacementProviderId,
  }) {
    return _run(
      'provider:$providerId',
      target: CommandTarget(
        providerId: providerId,
        label: replacementProviderId,
      ),
      (revision, operationId) => _api.removeProvider(
        revision,
        providerId,
        replacementProviderId: replacementProviderId,
        operationId: operationId,
      ),
    );
  }

  Future<void> setModeModelField({
    ThreadModeId? mode,
    required String providerId,
    required String model,
  }) async {
    if (mode == null) return;
    await applySettingsField(
      ModeModelCommand(modeId: mode.id, providerId: providerId, model: model),
      mutationKey: 'new-thread-route:model:${mode.id}',
    );
  }

  Future<void> setModeReasoningEffortField({
    ThreadModeId? mode,
    required String? effort,
  }) async {
    if (mode == null) return;
    await applySettingsField(
      ModeReasoningEffortCommand(modeId: mode.id, effort: effort),
      mutationKey: 'new-thread-route:effort:${mode.id}',
    );
  }

  Future<void> setRoleModelField({
    required String roleKey,
    required String providerId,
    required String model,
  }) {
    return applySettingsField(
      RoleModelCommand(role: roleKey, providerId: providerId, model: model),
      mutationKey: 'agent-role:model:$roleKey',
    );
  }

  Future<void> setRoleReasoningEffortField({
    required String roleKey,
    required String? effort,
  }) {
    return applySettingsField(
      RoleReasoningEffortCommand(role: roleKey, effort: effort),
      mutationKey: 'agent-role:effort:$roleKey',
    );
  }

  Future<void> setSystemAgentEnabled({
    required String profileId,
    required bool enabled,
  }) async {
    await _run(
      'system-agent:$profileId',
      target: CommandTarget(label: profileId, enabled: enabled),
      (revision, operationId) => _api.setSystemAgentEnabled(
        operationId: operationId,
        expectedSettingsRevision: revision,
        profileId: profileId,
        enabled: enabled,
      ),
    );
  }

  Future<void> setDefaultProvider(String providerId) async {
    await _run(
      'default-provider',
      target: CommandTarget(providerId: providerId),
      (revision, operationId) {
        final providers =
            _client.settings.state.canonical?.providers ?? const [];
        if (!providers.any((provider) => provider.id == providerId)) {
          throw StateError('Provider is unavailable: $providerId');
        }
        return _api.setDefaultProvider(
          revision,
          providerId,
          operationId: operationId,
        );
      },
    );
  }

  Future<void> saveUserAgentProfile(AgentProfileDraft draft) async {
    await _run(
      'user-agent-profile:${draft.id}',
      target: CommandTarget(label: draft.id),
      (revision, operationId) =>
          _api.saveUserAgentProfile(revision, draft, operationId: operationId),
    );
  }

  Future<void> setPermissionMode(PermissionMode mode) async {
    await _run(
      'permission-mode',
      target: CommandTarget(label: mode.name),
      (revision, operationId) => _api.saveRuntimePermissionMode(
        revision,
        mode,
        operationId: operationId,
      ),
    );
  }

  /// 只刷新事实源，不重放保存。失败时保留调用方原来的冲突错误。
  Future<SettingsStateSnapshot?> refreshCanonical() async {
    try {
      final snapshot = await _api.readSettingsState();
      if (!_client.isClosed) {
        _client.settings.merge(snapshot);
      }
      return snapshot;
    } catch (_) {
      return null;
    }
  }

  StudioTransport get _api => _client.transport;

  final Map<OperationId, Observation> _confirmations = {};

  Future<SettingsStateSnapshot> _run(
    String key,
    Future<SettingsCommandReply> Function(int revision, OperationId operationId)
    request, {
    CommandTarget target = const CommandTarget(),
  }) {
    if (_client.isClosed) return Future.error(StateError('Client is closed'));
    // Latest intent is visible before queueing. An older ACK may still update
    // canonical and its own operation record, but cannot erase this intent.
    final operation = _client.mutations.begin(target: 'settings:$key');
    final store = _client.settings;
    store.beginCommand(key, target);
    return _lane.enqueue(() async {
      try {
        final reply = await retryRevisionConflict(() async {
          final revision = await _expectedSettingsRevision();
          return request(revision, operation.id);
        }, refresh: () async => await refreshCanonical() != null);
        if (!_client.isClosed) store.merge(reply.snapshot);
        operation.accept(reply.receipt.toCommitReceipt());
        _confirm(key, operation);
        return reply.snapshot;
      } catch (error) {
        if (operation.state.value.receipt != null) {
          operation.syncFailed(error);
        } else if (_knownRejection(error)) {
          operation.reject(error);
        } else {
          operation.unknownOutcome(error);
        }
        if (!_client.isClosed && _isLatest(operation)) {
          store.failCommand(
            key,
            typedCommandFailure(error, correlationId: 'client-settings'),
            target: target,
          );
        }
        rethrow;
      }
    });
  }

  bool _isLatest(OperationHandle operation) =>
      _client.mutations.latest(operation.state.value.target)?.id ==
      operation.id;

  void _confirm(String key, OperationHandle operation) {
    if (_client.isClosed) return;
    void settle(OperationState result) {
      if (!result.observed && result.failurePhase == null) return;
      _confirmations.remove(operation.id)?.cancel();
      if (_client.isClosed || !_isLatest(operation)) return;
      if (result.observed) _client.settings.clearCommand(key);
    }

    if (operation.state.value.observed) {
      settle(operation.state.value);
      return;
    }
    final watched = operation.state.watchLatest(settle);
    _confirmations[operation.id] = watched.observation;
    settle(watched.value);
  }

  void closeObservations() {
    for (final observation in _confirmations.values) {
      observation.cancel();
    }
    _confirmations.clear();
  }

  Future<void> close() {
    _lane.seal();
    return _lane.join();
  }

  Future<int> _expectedSettingsRevision() async {
    final current = _client.settings.state;
    if (current.configLoaded) return current.settingsRevision;
    final refreshed = await refreshCanonical();
    final loaded = _client.settings.state;
    if (refreshed == null || !loaded.configLoaded) {
      throw _settingsNotReady;
    }
    return loaded.settingsRevision;
  }
}

const StudioFailure _settingsNotReady = StudioFailure(
  code: StudioFailureCode.notInitialized,
  message: 'Settings are not available yet',
  retryable: false,
  correlationId: 'client-settings-not-ready',
);

String settingsFieldMutationKey(SettingsFieldCommand command) =>
    switch (command) {
      InstructionBaseOverrideCommand() => 'instructions:base-override',
      InstructionDeveloperCommand() => 'instructions:developer',
      InstructionUserCommand() => 'instructions:user',
      ProjectDocMaxBytesCommand() => 'instructions:project-doc-max-bytes',
      ProjectDocFallbackFilenamesCommand() =>
        'instructions:project-doc-fallback-filenames',
      SkillsEnabledCommand() => 'skills:enabled',
      SkillsAutoLearnCommand() => 'skills:auto-learn',
      SkillsSystemEnabledCommand() => 'skills:system-enabled',
      SkillsProjectDirCommand() => 'skills:project-dir',
      SkillsUserDirCommand() => 'skills:user-dir',
      SkillsExternalDirsCommand() => 'skills:external-dirs',
      SkillsDisabledCommand() => 'skills:disabled',
      SkillsAutoLearnMinToolCallsCommand() =>
        'skills:auto-learn-min-tool-calls',
      McpServerEnabledCommand(:final id) => 'mcp:$id:enabled',
      McpServerTransportCommand(:final id) => 'mcp:$id:transport',
      McpServerEndpointCommand(:final id) => 'mcp:$id:endpoint',
      GeneralFollowActiveTurnCommand() => 'general:follow-active-turn',
      GeneralCompactTimelineCommand() => 'general:compact-timeline',
      GeneralSidebarWidthCommand() => 'general:sidebar-width',
      GeneralPinnedThreadIdsCommand() => 'general:pinned-thread-ids',
      GeneralPinnedProjectIdsCommand() => 'general:pinned-project-ids',
      WebSearchModeCommand() => 'web-search:mode',
      WebSearchContextSizeCommand() => 'web-search:context-size',
      WebSearchAllowedDomainsCommand() => 'web-search:allowed-domains',
      WebSearchCountryCommand() => 'web-search:country',
      WebSearchRegionCommand() => 'web-search:region',
      WebSearchCityCommand() => 'web-search:city',
      WebSearchTimezoneCommand() => 'web-search:timezone',
      DeepSeekWebSearchEnabledCommand() => 'deepseek:web-search-enabled',
      DeepSeekWebFetchEnabledCommand() => 'deepseek:web-fetch-enabled',
      ModeModelCommand(:final modeId) => 'mode-route:model:$modeId',
      ModeReasoningEffortCommand(:final modeId) => 'mode-route:effort:$modeId',
      RoleModelCommand(:final role) => 'agent-role:model:$role',
      RoleReasoningEffortCommand(:final role) => 'agent-role:effort:$role',
    };

CommandTarget settingsFieldTarget(
  SettingsFieldCommand command,
) => switch (command) {
  ModeModelCommand(:final providerId, :final model) => CommandTarget(
    providerId: providerId,
    model: model,
  ),
  RoleModelCommand(:final providerId, :final model) => CommandTarget(
    providerId: providerId,
    model: model,
  ),
  ModeReasoningEffortCommand(:final effort) => CommandTarget(effort: effort),
  RoleReasoningEffortCommand(:final effort) => CommandTarget(effort: effort),
  _ => const CommandTarget(),
};

bool _knownRejection(Object error) =>
    error is StudioFailure &&
    switch (error.code) {
      StudioFailureCode.invalidArgument ||
      StudioFailureCode.notFound ||
      StudioFailureCode.busy ||
      StudioFailureCode.conflict ||
      StudioFailureCode.staleRevision ||
      StudioFailureCode.permissionDenied ||
      StudioFailureCode.overloaded ||
      StudioFailureCode.notInitialized => true,
      _ => false,
    };

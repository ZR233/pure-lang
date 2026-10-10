import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart' show debugPrint;

import '../../domain/models/studio_models.dart';

/// The settings command lane is owned by the app-level repository, rather than
/// by a page. A route can be popped while a command is in flight and the lane
/// still reconciles its canonical response.
/// App-level coordinator for every settings mutation.  It is intentionally
/// independent from route/widget lifetime so a popped Settings page cannot
/// cancel or reorder a write that was already accepted by the UI.
final class SettingsMutationCoordinator {
  Future<void> _tail = Future<void>.value();

  Future<T> enqueue<T>(Future<T> Function() operation) {
    final task = _tail.then((_) => operation());
    // The tail is deliberately failure-free. A rejected command is returned to
    // its caller, while later commands remain executable.
    _tail = task.then<void>((_) {}, onError: (_) {});
    return task;
  }

  Future<bool> flush({Duration timeout = const Duration(seconds: 5)}) async {
    try {
      await _tail.timeout(timeout);
      return true;
    } on TimeoutException {
      return false;
    }
  }
}

/// The canonical configuration clock.  It contains only settings owned by
/// the configuration transaction lane; model discovery has its own clock.
final class SettingsConfigState {
  const SettingsConfigState({required this.revision, required this.data});

  final int revision;
  final SettingsStateData data;
}

/// The canonical provider/model directory clock.
final class ModelCatalogState {
  const ModelCatalogState({required this.revision, required this.providers});

  final int revision;
  final List<ProviderSettingsView> providers;
}

/// Structured merge telemetry exposed to the driver and debug logs.  It does
/// not contain credentials, prompts, or model content.
final class SettingsMergeDiagnostic {
  const SettingsMergeDiagnostic({
    required this.domain,
    required this.source,
    required this.currentRevision,
    required this.incomingRevision,
    required this.applied,
    required this.reason,
    this.operationId,
    this.pendingCount = 0,
    this.topicEpoch,
  });

  final String domain;
  final String source;
  final int currentRevision;
  final int incomingRevision;
  final bool applied;
  final String reason;
  final String? operationId;
  final int pendingCount;
  final int? topicEpoch;

  Map<String, Object?> toJson() => {
    'domain': domain,
    'source': source,
    'currentRevision': currentRevision,
    'incomingRevision': incomingRevision,
    'applied': applied,
    'reason': reason,
    if (operationId != null) 'operationId': operationId,
    'pendingCount': pendingCount,
    if (topicEpoch != null) 'topicEpoch': topicEpoch,
  };
}

/// App-level owner for the split settings resources.
///
/// The controller exposes immutable projections, while this repository keeps
/// the last canonical config/catalog clocks across route disposal.  Direct
/// responses and topic frames use the same merge path; a lower revision from
/// either domain is ignored without touching the other domain.
final class StudioSettingsRepository {
  SettingsStateSnapshot? _canonical;
  SettingsConfigState? _config;
  ModelCatalogState? _catalog;
  String? _lastSource;
  SettingsMergeDiagnostic? _lastMerge;
  int? _topicEpoch;

  SettingsStateSnapshot? get canonical => _canonical;
  SettingsConfigState? get configState => _config;
  ModelCatalogState? get modelCatalogState => _catalog;
  String? get lastSource => _lastSource;
  SettingsMergeDiagnostic? get lastMerge => _lastMerge;
  int? get topicEpoch => _topicEpoch;

  void seed(
    SettingsStateSnapshot snapshot, {
    String source = 'baseline',
    int? topicEpoch,
  }) {
    _canonical = snapshot;
    final data = snapshot.state.value;
    if (data != null) {
      _config = SettingsConfigState(revision: snapshot.revision, data: data);
      _catalog = ModelCatalogState(
        revision: snapshot.modelCatalogRevision,
        providers: List.unmodifiable(data.providers),
      );
    }
    _lastSource = source;
    _topicEpoch = topicEpoch ?? _topicEpoch;
  }

  SettingsStateSnapshot? merge(
    SettingsStateSnapshot incoming, {
    required String source,
    String? operationId,
    int pendingCount = 0,
    int? topicEpoch,
  }) {
    final previous = _canonical;
    if (previous == null) {
      seed(incoming, source: source, topicEpoch: topicEpoch);
      _recordMerge(
        SettingsMergeDiagnostic(
          domain: 'config/catalog',
          source: source,
          currentRevision: 0,
          incomingRevision: incoming.revision,
          applied: true,
          reason: 'baseline',
          operationId: operationId,
          pendingCount: pendingCount,
          topicEpoch: topicEpoch,
        ),
      );
      return incoming;
    }

    final previousConfigRevision = _config?.revision ?? previous.revision;
    final previousCatalogRevision =
        _catalog?.revision ?? previous.modelCatalogRevision;
    final configIsNewer = incoming.revision > previousConfigRevision;
    final catalogIsNewer =
        incoming.modelCatalogRevision > previousCatalogRevision;
    if (!configIsNewer && !catalogIsNewer) {
      _recordMerge(
        SettingsMergeDiagnostic(
          domain: 'config/catalog',
          source: source,
          currentRevision: previousConfigRevision,
          incomingRevision: incoming.revision,
          applied: false,
          reason: 'stale-or-idempotent',
          operationId: operationId,
          pendingCount: pendingCount,
          topicEpoch: topicEpoch,
        ),
      );
      return null;
    }

    final previousData = previous.state.value;
    final incomingData = incoming.state.value;
    if (previousData == null || incomingData == null) {
      _recordMerge(
        SettingsMergeDiagnostic(
          domain: 'config/catalog',
          source: source,
          currentRevision: previousConfigRevision,
          incomingRevision: incoming.revision,
          applied: false,
          reason: 'missing-state-data',
          operationId: operationId,
          pendingCount: pendingCount,
          topicEpoch: topicEpoch,
        ),
      );
      return null;
    }

    if (configIsNewer) {
      _config = SettingsConfigState(
        revision: incoming.revision,
        data: incomingData,
      );
    }
    if (catalogIsNewer) {
      _catalog = ModelCatalogState(
        revision: incoming.modelCatalogRevision,
        providers: List.unmodifiable(incomingData.providers),
      );
    }
    _canonical = _project(previous, config: _config, catalog: _catalog);
    _lastSource = source;
    _topicEpoch = topicEpoch ?? _topicEpoch;
    _recordMerge(
      SettingsMergeDiagnostic(
        domain: configIsNewer && catalogIsNewer
            ? 'config/catalog'
            : configIsNewer
            ? 'config'
            : 'catalog',
        source: source,
        currentRevision: configIsNewer
            ? previousConfigRevision
            : previousCatalogRevision,
        incomingRevision: configIsNewer
            ? incoming.revision
            : incoming.modelCatalogRevision,
        applied: true,
        reason: 'advanced',
        operationId: operationId,
        pendingCount: pendingCount,
        topicEpoch: topicEpoch,
      ),
    );
    return _canonical;
  }

  SettingsStateSnapshot? applyTo(
    SettingsStateSnapshot current,
    SettingsStateSnapshot incoming, {
    required String source,
    String? operationId,
    int pendingCount = 0,
    int? topicEpoch,
  }) {
    if (_canonical == null) seed(current, source: 'state');
    final merged = merge(
      incoming,
      source: source,
      operationId: operationId,
      pendingCount: pendingCount,
      topicEpoch: topicEpoch,
    );
    return merged ?? current;
  }

  void _recordMerge(SettingsMergeDiagnostic diagnostic) {
    _lastMerge = diagnostic;
    debugPrint('studio_settings_merge ${jsonEncode(diagnostic.toJson())}');
  }

  SettingsStateSnapshot _project(
    SettingsStateSnapshot previous, {
    required SettingsConfigState? config,
    required ModelCatalogState? catalog,
  }) {
    final configData = config?.data ?? previous.state.value!;
    final catalogProviders = catalog?.providers ?? configData.providers;
    final catalogByProvider = {
      for (final provider in catalogProviders) provider.id: provider,
    };
    final providers = [
      for (final provider in configData.providers)
        if (catalogByProvider[provider.id] case final catalogProvider?)
          provider.copyWith(
            models: catalogProvider.models,
            modelCatalog: catalogProvider.modelCatalog,
          )
        else
          provider,
    ];
    return SettingsStateSnapshot(
      providers: providers,
      defaultProviderId: configData.defaultProviderId,
      modeModelRoutes: configData.modeModelRoutes,
      roles: configData.roles,
      mcpServers: configData.mcpServers,
      instructions: configData.instructions,
      skills: configData.skills,
      general: configData.general,
      webSearch: configData.webSearch,
      deepSeekWebSearch: configData.deepSeekWebSearch,
      permissionMode: configData.permissionMode,
      revision: config?.revision ?? previous.revision,
      modelCatalogRevision: catalog?.revision ?? previous.modelCatalogRevision,
    );
  }
}

/// Merges the direct Bridge response into the two logical settings resources.
///
/// The UI keeps one immutable projection for convenience, but the config and
/// catalog clocks are merged independently. A response or topic frame can
/// advance either resource without replacing the other resource with an older
/// page snapshot.
SettingsStateSnapshot? mergeCanonicalSettingsSnapshots(
  SettingsStateSnapshot previous,
  SettingsStateSnapshot next, {
  bool mergeConfig = true,
  bool mergeCatalog = true,
}) {
  if (next.state.value == null || previous.state.value == null) return null;
  final nextConfigIsNewer = mergeConfig && next.revision > previous.revision;
  final nextCatalogIsNewer =
      mergeCatalog && next.modelCatalogRevision > previous.modelCatalogRevision;
  if (!nextConfigIsNewer && !nextCatalogIsNewer) {
    debugPrint(
      'settings_merge_dropped reason=stale-or-idempotent '
      'config=${next.revision}/${previous.revision} '
      'catalog=${next.modelCatalogRevision}/${previous.modelCatalogRevision}',
    );
    return null;
  }

  final previousData = previous.state.value!;
  final nextData = next.state.value!;
  final configData = nextConfigIsNewer ? nextData : previousData;
  final catalogData = nextCatalogIsNewer ? nextData : previousData;
  final catalogByProvider = {
    for (final provider in catalogData.providers) provider.id: provider,
  };
  final providers = [
    for (final provider in configData.providers)
      if (catalogByProvider[provider.id] case final catalogProvider?)
        provider.copyWith(
          models: catalogProvider.models,
          modelCatalog: catalogProvider.modelCatalog,
        )
      else
        provider,
  ];

  return SettingsStateSnapshot(
    providers: providers,
    defaultProviderId: configData.defaultProviderId,
    modeModelRoutes: configData.modeModelRoutes,
    roles: configData.roles,
    mcpServers: configData.mcpServers,
    instructions: configData.instructions,
    skills: configData.skills,
    general: configData.general,
    webSearch: configData.webSearch,
    deepSeekWebSearch: configData.deepSeekWebSearch,
    permissionMode: configData.permissionMode,
    revision: nextConfigIsNewer ? next.revision : previous.revision,
    modelCatalogRevision: nextCatalogIsNewer
        ? next.modelCatalogRevision
        : previous.modelCatalogRevision,
  );
}

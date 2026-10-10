import 'dart:async';

import 'package:flutter/foundation.dart' show debugPrint;

import '../../domain/models/studio_models.dart';

/// The settings command lane is owned by the app-level repository, rather than
/// by a page. A route can be popped while a command is in flight and the lane
/// still reconciles its canonical response.
final class StudioMutationCoordinator {
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

/// App-level owner for the split settings resources.
///
/// The controller exposes immutable projections, while this repository keeps
/// the last canonical config/catalog clocks across route disposal.  Direct
/// responses and topic frames use the same merge path; a lower revision from
/// either domain is ignored without touching the other domain.
final class StudioSettingsRepository {
  SettingsStateSnapshot? _canonical;
  String? _lastSource;

  SettingsStateSnapshot? get canonical => _canonical;
  String? get lastSource => _lastSource;

  void seed(SettingsStateSnapshot snapshot, {String source = 'baseline'}) {
    _canonical = snapshot;
    _lastSource = source;
  }

  SettingsStateSnapshot? merge(
    SettingsStateSnapshot incoming, {
    required String source,
  }) {
    final previous = _canonical;
    if (previous == null) {
      seed(incoming, source: source);
      return incoming;
    }
    final merged = mergeCanonicalSettingsSnapshots(previous, incoming);
    if (merged == null) return null;
    _canonical = merged;
    _lastSource = source;
    return merged;
  }

  SettingsStateSnapshot? applyTo(
    SettingsStateSnapshot current,
    SettingsStateSnapshot incoming, {
    required String source,
  }) {
    if (_canonical == null) seed(current, source: 'state');
    final merged = merge(incoming, source: source);
    return merged ?? current;
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
  SettingsStateSnapshot next,
) {
  if (next.state.value == null || previous.state.value == null) return null;
  final nextConfigIsNewer = next.revision > previous.revision;
  final nextCatalogIsNewer =
      next.modelCatalogRevision > previous.modelCatalogRevision;
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

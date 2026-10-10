import 'dart:async';

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

/// Merges the legacy Bridge envelope into the two logical settings resources.
///
/// The wire snapshot still contains both clocks for compatibility with the
/// current generated bindings, but configuration and model discovery are
/// merged independently. This is the same rule a split protocol uses and keeps
/// direct responses and topic baselines interchangeable.
SettingsStateSnapshot? mergeCanonicalSettingsSnapshots(
  SettingsStateSnapshot previous,
  SettingsStateSnapshot next,
) {
  if (next.state.value == null || previous.state.value == null) return null;
  final nextConfigIsNewer = next.revision > previous.revision;
  final nextCatalogIsNewer =
      next.modelCatalogRevision > previous.modelCatalogRevision;
  if (!nextConfigIsNewer && !nextCatalogIsNewer) return null;

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

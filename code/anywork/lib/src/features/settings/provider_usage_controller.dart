import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';

part 'provider_usage_controller.g.dart';

class ProviderUsageError {
  const ProviderUsageError({
    required this.message,
    required this.requestGeneration,
    required this.baselineRevisions,
  });

  final String message;
  final int requestGeneration;

  /// The canonical per-provider usage revisions observed when this refresh
  /// started. An error without a baseline must never hide a Ready projection.
  final Map<String, int> baselineRevisions;

  bool appliesTo(String providerId) =>
      baselineRevisions.containsKey(providerId);

  bool isObsoleteFor(String providerId, ProviderUsageView? usage) {
    if (usage == null || usage.state is! ReadyProviderUsageView) {
      return false;
    }
    final baseline = baselineRevisions[providerId];
    return baseline == null || usage.revision > baseline;
  }
}

class ProviderUsageState {
  const ProviderUsageState({
    this.loadingGenerationByProviderId = const {},
    this.latestRequestGenerationByProviderId = const {},
    this.errorsByProviderId = const {},
  });

  final Map<String, int> loadingGenerationByProviderId;
  final Map<String, int> latestRequestGenerationByProviderId;
  final Map<String, ProviderUsageError> errorsByProviderId;

  Set<String> get loadingProviderIds =>
      loadingGenerationByProviderId.keys.toSet();

  Map<String, String> visibleErrorsByProviderId({
    required Iterable<String> providerIds,
    required Iterable<ProviderUsageView> canonicalUsages,
  }) {
    final usageByProviderId = {
      for (final usage in canonicalUsages) usage.providerId: usage,
    };
    final visible = <String, String>{};
    for (final providerId in providerIds) {
      final latestRequest = latestRequestGenerationByProviderId[providerId];
      final candidate =
          [errorsByProviderId[providerId], errorsByProviderId['*']]
              .whereType<ProviderUsageError>()
              .where((error) {
                return error.appliesTo(providerId) &&
                    (latestRequest == null ||
                        error.requestGeneration >= latestRequest);
              })
              .fold<ProviderUsageError?>(null, (current, error) {
                if (current == null ||
                    error.requestGeneration > current.requestGeneration) {
                  return error;
                }
                return current;
              });
      if (candidate != null &&
          !candidate.isObsoleteFor(providerId, usageByProviderId[providerId])) {
        visible[providerId] = candidate.message;
      }
    }
    return visible;
  }
}

@riverpod
class ProviderUsageController extends _$ProviderUsageController {
  Future<void> _refreshTail = Future<void>.value();
  int _nextRequestGeneration = 0;

  @override
  ProviderUsageState build() => const ProviderUsageState();

  Future<void> refresh({String? providerId}) {
    final requestGeneration = ++_nextRequestGeneration;
    final providerIds = _targetProviderIds(providerId);
    _markRefreshStarted(
      providerId: providerId,
      providerIds: providerIds,
      requestGeneration: requestGeneration,
    );

    final operation = _refreshTail.then<void>(
      (_) => _runRefresh(
        providerId: providerId,
        providerIds: providerIds,
        requestGeneration: requestGeneration,
      ),
    );
    // _runRefresh converts bridge failures into local error state, so the
    // tail remains failure-free even when one refresh fails.
    _refreshTail = operation.catchError((Object _, StackTrace _) {});
    return operation;
  }

  Set<String> _targetProviderIds(String? providerId) {
    if (providerId != null) {
      return {providerId};
    }
    return ref
            .read(studioControllerProvider)
            .value
            ?.providers
            .map((provider) => provider.id)
            .toSet() ??
        const <String>{};
  }

  Map<String, int> _canonicalUsageRevisions(Set<String> providerIds) {
    final usages =
        ref.read(studioControllerProvider).value?.providerUsages ??
        const <ProviderUsageView>[];
    return {
      for (final providerId in providerIds)
        providerId:
            usages
                .where((usage) => usage.providerId == providerId)
                .firstOrNull
                ?.revision ??
            -1,
    };
  }

  void _markRefreshStarted({
    required String? providerId,
    required Set<String> providerIds,
    required int requestGeneration,
  }) {
    final current = state;
    final loading = {...current.loadingGenerationByProviderId};
    final latestRequests = {...current.latestRequestGenerationByProviderId};
    for (final id in providerIds) {
      loading[id] = requestGeneration;
      latestRequests[id] = requestGeneration;
    }
    final errors = {...current.errorsByProviderId};
    errors.remove(providerId ?? '*');
    state = ProviderUsageState(
      loadingGenerationByProviderId: loading,
      latestRequestGenerationByProviderId: latestRequests,
      errorsByProviderId: errors,
    );
  }

  Future<void> _runRefresh({
    required String? providerId,
    required Set<String> providerIds,
    required int requestGeneration,
  }) async {
    // The tail may have waited for an earlier refresh to update the
    // canonical topic, so capture this request's baseline immediately before
    // its bridge call rather than when it was queued.
    final baselineRevisions = _canonicalUsageRevisions(providerIds);
    try {
      await _refreshCanonicalUsages();
      _finishRefresh(
        providerId: providerId,
        providerIds: providerIds,
        requestGeneration: requestGeneration,
      );
    } catch (error) {
      _recordRefreshFailure(
        providerId: providerId,
        providerIds: providerIds,
        baselineRevisions: baselineRevisions,
        requestGeneration: requestGeneration,
        message: error.toString(),
      );
    }
  }

  void _finishRefresh({
    required String? providerId,
    required Set<String> providerIds,
    required int requestGeneration,
  }) {
    final current = state;
    final loading = {...current.loadingGenerationByProviderId};
    for (final id in providerIds) {
      if (loading[id] == requestGeneration) {
        loading.remove(id);
      }
    }
    final errors = {...current.errorsByProviderId};
    final errorKey = providerId ?? '*';
    if (errors[errorKey]?.requestGeneration == requestGeneration) {
      errors.remove(errorKey);
    }
    state = ProviderUsageState(
      loadingGenerationByProviderId: loading,
      latestRequestGenerationByProviderId:
          current.latestRequestGenerationByProviderId,
      errorsByProviderId: errors,
    );
  }

  void _recordRefreshFailure({
    required String? providerId,
    required Set<String> providerIds,
    required Map<String, int> baselineRevisions,
    required int requestGeneration,
    required String message,
  }) {
    final current = state;
    final reportableProviderIds = providerIds
        .where(
          (id) =>
              current.latestRequestGenerationByProviderId[id] ==
              requestGeneration,
        )
        .toSet();
    final loading = {...current.loadingGenerationByProviderId};
    for (final id in providerIds) {
      if (loading[id] == requestGeneration) {
        loading.remove(id);
      }
    }
    final errors = {...current.errorsByProviderId};
    final errorKey = providerId ?? '*';
    if (reportableProviderIds.isEmpty) {
      if (errors[errorKey]?.requestGeneration == requestGeneration) {
        errors.remove(errorKey);
      }
    } else {
      errors[errorKey] = ProviderUsageError(
        message: message,
        requestGeneration: requestGeneration,
        baselineRevisions: {
          for (final id in reportableProviderIds) id: baselineRevisions[id]!,
        },
      );
    }
    state = ProviderUsageState(
      loadingGenerationByProviderId: loading,
      latestRequestGenerationByProviderId:
          current.latestRequestGenerationByProviderId,
      errorsByProviderId: errors,
    );
  }

  Future<void> _refreshCanonicalUsages() async {
    await ref.read(studioControllerProvider.notifier).refreshProviderUsages();
  }
}

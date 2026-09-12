import 'package:riverpod_annotation/riverpod_annotation.dart';

import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';

part 'provider_usage_controller.g.dart';

class ProviderUsageState {
  const ProviderUsageState({
    this.usages = const [],
    this.loadingProviderIds = const {},
    this.errorsByProviderId = const {},
  });

  final List<ProviderUsageView> usages;
  final Set<String> loadingProviderIds;
  final Map<String, String> errorsByProviderId;

  String? errorFor(String providerId) {
    return errorsByProviderId[providerId] ?? errorsByProviderId['*'];
  }
}

@riverpod
class ProviderUsageController extends _$ProviderUsageController {
  @override
  Future<ProviderUsageState> build() async {
    final usages = await _loadCanonicalUsages();
    return ProviderUsageState(usages: usages);
  }

  Future<void> refresh({String? providerId}) async {
    final current = state.value ?? const ProviderUsageState();
    final errors = {...current.errorsByProviderId}..remove(providerId ?? '*');
    final providerIds = providerId == null
        ? ref
                  .read(studioControllerProvider)
                  .value
                  ?.providers
                  .map((provider) => provider.id)
                  .toSet() ??
              const <String>{}
        : {providerId};
    state = AsyncData(
      ProviderUsageState(
        usages: current.usages,
        loadingProviderIds: {...current.loadingProviderIds, ...providerIds},
        errorsByProviderId: errors,
      ),
    );
    try {
      final usages = await _loadCanonicalUsages();
      state = AsyncData(
        ProviderUsageState(
          usages: providerId == null
              ? usages
              : _mergeProviderUsage(current.usages, usages, providerId),
        ),
      );
    } catch (error) {
      state = AsyncData(
        ProviderUsageState(
          usages: current.usages,
          errorsByProviderId: {
            ...current.errorsByProviderId,
            providerId ?? '*': error.toString(),
          },
        ),
      );
    }
  }

  Future<List<ProviderUsageView>> _loadCanonicalUsages() async {
    await ref.read(studioControllerProvider.notifier).refreshProviderUsages();
    return ref.read(studioControllerProvider).value?.providerUsages ?? const [];
  }
}

List<ProviderUsageView> _mergeProviderUsage(
  List<ProviderUsageView> current,
  List<ProviderUsageView> incoming,
  String providerId,
) {
  final replacement = incoming
      .where((usage) => usage.providerId == providerId)
      .firstOrNull;
  return [
    for (final usage in current)
      if (usage.providerId != providerId) usage,
    ?replacement,
  ];
}

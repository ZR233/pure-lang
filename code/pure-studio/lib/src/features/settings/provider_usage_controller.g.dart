// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'provider_usage_controller.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(ProviderUsageController)
final providerUsageControllerProvider = ProviderUsageControllerProvider._();

final class ProviderUsageControllerProvider
    extends
        $AsyncNotifierProvider<ProviderUsageController, ProviderUsageState> {
  ProviderUsageControllerProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'providerUsageControllerProvider',
        isAutoDispose: true,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$providerUsageControllerHash();

  @$internal
  @override
  ProviderUsageController create() => ProviderUsageController();
}

String _$providerUsageControllerHash() =>
    r'1cbebece774d7206846a1493b177bec2a9c7b229';

abstract class _$ProviderUsageController
    extends $AsyncNotifier<ProviderUsageState> {
  FutureOr<ProviderUsageState> build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref =
        this.ref as $Ref<AsyncValue<ProviderUsageState>, ProviderUsageState>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<AsyncValue<ProviderUsageState>, ProviderUsageState>,
              AsyncValue<ProviderUsageState>,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}

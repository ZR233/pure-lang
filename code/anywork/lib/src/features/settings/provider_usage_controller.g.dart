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
    extends $NotifierProvider<ProviderUsageController, ProviderUsageState> {
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

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(ProviderUsageState value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<ProviderUsageState>(value),
    );
  }
}

String _$providerUsageControllerHash() =>
    r'31c4d5ec6dbcb69e1de44500fad7f0da6d488de6';

abstract class _$ProviderUsageController extends $Notifier<ProviderUsageState> {
  ProviderUsageState build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<ProviderUsageState, ProviderUsageState>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<ProviderUsageState, ProviderUsageState>,
              ProviderUsageState,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}

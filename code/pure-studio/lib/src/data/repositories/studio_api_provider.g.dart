// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'studio_api_provider.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(studioApi)
final studioApiProvider = StudioApiProvider._();

final class StudioApiProvider
    extends $FunctionalProvider<StudioApi, StudioApi, StudioApi>
    with $Provider<StudioApi> {
  StudioApiProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'studioApiProvider',
        isAutoDispose: false,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$studioApiHash();

  @$internal
  @override
  $ProviderElement<StudioApi> $createElement($ProviderPointer pointer) =>
      $ProviderElement(pointer);

  @override
  StudioApi create(Ref ref) {
    return studioApi(ref);
  }

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(StudioApi value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<StudioApi>(value),
    );
  }
}

String _$studioApiHash() => r'e66a97c48f00f14d1c2a4e34984c2154934b40d6';

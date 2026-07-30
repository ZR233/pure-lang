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

String _$studioApiHash() => r'43ebe5d51098809a5f0cefa9e43f5f68d031f058';

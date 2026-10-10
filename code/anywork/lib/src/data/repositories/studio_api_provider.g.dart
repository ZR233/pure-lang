// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'studio_api_provider.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(studioBridgeDataSource)
final studioBridgeDataSourceProvider = StudioBridgeDataSourceProvider._();

final class StudioBridgeDataSourceProvider
    extends
        $FunctionalProvider<
          StudioBridgeDataSource,
          StudioBridgeDataSource,
          StudioBridgeDataSource
        >
    with $Provider<StudioBridgeDataSource> {
  StudioBridgeDataSourceProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'studioBridgeDataSourceProvider',
        isAutoDispose: false,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$studioBridgeDataSourceHash();

  @$internal
  @override
  $ProviderElement<StudioBridgeDataSource> $createElement(
    $ProviderPointer pointer,
  ) => $ProviderElement(pointer);

  @override
  StudioBridgeDataSource create(Ref ref) {
    return studioBridgeDataSource(ref);
  }

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(StudioBridgeDataSource value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<StudioBridgeDataSource>(value),
    );
  }
}

String _$studioBridgeDataSourceHash() =>
    r'cc10337c4232ad3220374fd7dae4afe5c8f18432';

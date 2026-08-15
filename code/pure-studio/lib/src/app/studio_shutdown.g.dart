// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'studio_shutdown.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning
/// 当前关机进度；null 表示未在关机。

@ProviderFor(StudioShutdownProgressState)
final studioShutdownProgressStateProvider =
    StudioShutdownProgressStateProvider._();

/// 当前关机进度；null 表示未在关机。
final class StudioShutdownProgressStateProvider
    extends
        $NotifierProvider<
          StudioShutdownProgressState,
          StudioShutdownProgress?
        > {
  /// 当前关机进度；null 表示未在关机。
  StudioShutdownProgressStateProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'studioShutdownProgressStateProvider',
        isAutoDispose: false,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$studioShutdownProgressStateHash();

  @$internal
  @override
  StudioShutdownProgressState create() => StudioShutdownProgressState();

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(StudioShutdownProgress? value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<StudioShutdownProgress?>(value),
    );
  }
}

String _$studioShutdownProgressStateHash() =>
    r'bf08bb7e4a3e141656f19e71a0034c2c7947a52e';

/// 当前关机进度；null 表示未在关机。

abstract class _$StudioShutdownProgressState
    extends $Notifier<StudioShutdownProgress?> {
  StudioShutdownProgress? build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref =
        this.ref as $Ref<StudioShutdownProgress?, StudioShutdownProgress?>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<StudioShutdownProgress?, StudioShutdownProgress?>,
              StudioShutdownProgress?,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}

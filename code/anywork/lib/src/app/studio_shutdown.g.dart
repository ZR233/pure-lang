// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'studio_shutdown.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning
/// 当前关机进度或失败状态；两者均为空表示未在关机。

@ProviderFor(StudioShutdownProgressState)
final studioShutdownProgressStateProvider =
    StudioShutdownProgressStateProvider._();

/// 当前关机进度或失败状态；两者均为空表示未在关机。
final class StudioShutdownProgressStateProvider
    extends $NotifierProvider<StudioShutdownProgressState, StudioShutdownView> {
  /// 当前关机进度或失败状态；两者均为空表示未在关机。
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
  Override overrideWithValue(StudioShutdownView value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<StudioShutdownView>(value),
    );
  }
}

String _$studioShutdownProgressStateHash() =>
    r'c2a71b6a47697d372205acd894cccd231610bb76';

/// 当前关机进度或失败状态；两者均为空表示未在关机。

abstract class _$StudioShutdownProgressState
    extends $Notifier<StudioShutdownView> {
  StudioShutdownView build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<StudioShutdownView, StudioShutdownView>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<StudioShutdownView, StudioShutdownView>,
              StudioShutdownView,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}

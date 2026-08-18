// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'studio_controller.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(StudioController)
final studioControllerProvider = StudioControllerProvider._();

final class StudioControllerProvider
    extends $AsyncNotifierProvider<StudioController, StudioState> {
  StudioControllerProvider._()
    : super(
        from: null,
        argument: null,
        retry: _disableStudioRetry,
        name: r'studioControllerProvider',
        isAutoDispose: false,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$studioControllerHash();

  @$internal
  @override
  StudioController create() => StudioController();
}

String _$studioControllerHash() => r'552a51d3331b03bbb2a78092246f06bd0322b12e';

abstract class _$StudioController extends $AsyncNotifier<StudioState> {
  FutureOr<StudioState> build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<AsyncValue<StudioState>, StudioState>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<AsyncValue<StudioState>, StudioState>,
              AsyncValue<StudioState>,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}

/// 侧栏目录分页加载的最近一次错误文案；null 表示无未恢复错误。

@ProviderFor(DirectoryLoadError)
final directoryLoadErrorProvider = DirectoryLoadErrorProvider._();

/// 侧栏目录分页加载的最近一次错误文案；null 表示无未恢复错误。
final class DirectoryLoadErrorProvider
    extends $NotifierProvider<DirectoryLoadError, String?> {
  /// 侧栏目录分页加载的最近一次错误文案；null 表示无未恢复错误。
  DirectoryLoadErrorProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'directoryLoadErrorProvider',
        isAutoDispose: false,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$directoryLoadErrorHash();

  @$internal
  @override
  DirectoryLoadError create() => DirectoryLoadError();

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(String? value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<String?>(value),
    );
  }
}

String _$directoryLoadErrorHash() =>
    r'a39d2ee43498f55bf25fe1e9531e3194e62e5b1b';

/// 侧栏目录分页加载的最近一次错误文案；null 表示无未恢复错误。

abstract class _$DirectoryLoadError extends $Notifier<String?> {
  String? build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<String?, String?>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<String?, String?>,
              String?,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}

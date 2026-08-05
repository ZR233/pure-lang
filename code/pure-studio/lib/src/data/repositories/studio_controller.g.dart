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

String _$studioControllerHash() => r'e16408bd860753f8d21113bd76cb7a34b2d85366';

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

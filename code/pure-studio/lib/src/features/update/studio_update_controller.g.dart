// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'studio_update_controller.dart';

// **************************************************************************
// RiverpodGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// ignore_for_file: type=lint, type=warning

@ProviderFor(StudioUpdateController)
final studioUpdateControllerProvider = StudioUpdateControllerProvider._();

final class StudioUpdateControllerProvider
    extends $NotifierProvider<StudioUpdateController, StudioUpdateState> {
  StudioUpdateControllerProvider._()
    : super(
        from: null,
        argument: null,
        retry: null,
        name: r'studioUpdateControllerProvider',
        isAutoDispose: false,
        dependencies: null,
        $allTransitiveDependencies: null,
      );

  @override
  String debugGetCreateSourceHash() => _$studioUpdateControllerHash();

  @$internal
  @override
  StudioUpdateController create() => StudioUpdateController();

  /// {@macro riverpod.override_with_value}
  Override overrideWithValue(StudioUpdateState value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<StudioUpdateState>(value),
    );
  }
}

String _$studioUpdateControllerHash() =>
    r'178b86c6bfb699d8606e79b602f9e1940f4ed2bb';

abstract class _$StudioUpdateController extends $Notifier<StudioUpdateState> {
  StudioUpdateState build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<StudioUpdateState, StudioUpdateState>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<StudioUpdateState, StudioUpdateState>,
              StudioUpdateState,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}

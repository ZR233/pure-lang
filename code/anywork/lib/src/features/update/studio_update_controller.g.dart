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
    extends $NotifierProvider<StudioUpdateController, UpdaterStateSnapshot> {
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
  Override overrideWithValue(UpdaterStateSnapshot value) {
    return $ProviderOverride(
      origin: this,
      providerOverride: $SyncValueProvider<UpdaterStateSnapshot>(value),
    );
  }
}

String _$studioUpdateControllerHash() =>
    r'9d340da71568f5d71ec4a9ffe80e152e47d14c30';

abstract class _$StudioUpdateController
    extends $Notifier<UpdaterStateSnapshot> {
  UpdaterStateSnapshot build();
  @$mustCallSuper
  @override
  WhenComplete runBuild() {
    final ref = this.ref as $Ref<UpdaterStateSnapshot, UpdaterStateSnapshot>;
    final element =
        ref.element
            as $ClassProviderElement<
              AnyNotifier<UpdaterStateSnapshot, UpdaterStateSnapshot>,
              UpdaterStateSnapshot,
              Object?,
              Object?
            >;
    return element.handleCreate(ref, build);
  }
}

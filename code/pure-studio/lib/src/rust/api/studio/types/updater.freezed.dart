// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'updater.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeUpdaterStateSnapshot {

 Object get field0;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUpdaterStateSnapshot&&const DeepCollectionEquality().equals(other.field0, field0));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(field0));

@override
String toString() {
  return 'BridgeUpdaterStateSnapshot(field0: $field0)';
}


}

/// @nodoc
class $BridgeUpdaterStateSnapshotCopyWith<$Res>  {
$BridgeUpdaterStateSnapshotCopyWith(BridgeUpdaterStateSnapshot _, $Res Function(BridgeUpdaterStateSnapshot) __);
}


/// Adds pattern-matching-related methods to [BridgeUpdaterStateSnapshot].
extension BridgeUpdaterStateSnapshotPatterns on BridgeUpdaterStateSnapshot {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeUpdaterStateSnapshot_Disabled value)?  disabled,TResult Function( BridgeUpdaterStateSnapshot_Idle value)?  idle,TResult Function( BridgeUpdaterStateSnapshot_Checking value)?  checking,TResult Function( BridgeUpdaterStateSnapshot_UpToDate value)?  upToDate,TResult Function( BridgeUpdaterStateSnapshot_Available value)?  available,TResult Function( BridgeUpdaterStateSnapshot_Downloading value)?  downloading,TResult Function( BridgeUpdaterStateSnapshot_Verifying value)?  verifying,TResult Function( BridgeUpdaterStateSnapshot_InstallerLaunched value)?  installerLaunched,TResult Function( BridgeUpdaterStateSnapshot_CheckFailed value)?  checkFailed,TResult Function( BridgeUpdaterStateSnapshot_InstallFailed value)?  installFailed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeUpdaterStateSnapshot_Disabled() when disabled != null:
return disabled(_that);case BridgeUpdaterStateSnapshot_Idle() when idle != null:
return idle(_that);case BridgeUpdaterStateSnapshot_Checking() when checking != null:
return checking(_that);case BridgeUpdaterStateSnapshot_UpToDate() when upToDate != null:
return upToDate(_that);case BridgeUpdaterStateSnapshot_Available() when available != null:
return available(_that);case BridgeUpdaterStateSnapshot_Downloading() when downloading != null:
return downloading(_that);case BridgeUpdaterStateSnapshot_Verifying() when verifying != null:
return verifying(_that);case BridgeUpdaterStateSnapshot_InstallerLaunched() when installerLaunched != null:
return installerLaunched(_that);case BridgeUpdaterStateSnapshot_CheckFailed() when checkFailed != null:
return checkFailed(_that);case BridgeUpdaterStateSnapshot_InstallFailed() when installFailed != null:
return installFailed(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeUpdaterStateSnapshot_Disabled value)  disabled,required TResult Function( BridgeUpdaterStateSnapshot_Idle value)  idle,required TResult Function( BridgeUpdaterStateSnapshot_Checking value)  checking,required TResult Function( BridgeUpdaterStateSnapshot_UpToDate value)  upToDate,required TResult Function( BridgeUpdaterStateSnapshot_Available value)  available,required TResult Function( BridgeUpdaterStateSnapshot_Downloading value)  downloading,required TResult Function( BridgeUpdaterStateSnapshot_Verifying value)  verifying,required TResult Function( BridgeUpdaterStateSnapshot_InstallerLaunched value)  installerLaunched,required TResult Function( BridgeUpdaterStateSnapshot_CheckFailed value)  checkFailed,required TResult Function( BridgeUpdaterStateSnapshot_InstallFailed value)  installFailed,}){
final _that = this;
switch (_that) {
case BridgeUpdaterStateSnapshot_Disabled():
return disabled(_that);case BridgeUpdaterStateSnapshot_Idle():
return idle(_that);case BridgeUpdaterStateSnapshot_Checking():
return checking(_that);case BridgeUpdaterStateSnapshot_UpToDate():
return upToDate(_that);case BridgeUpdaterStateSnapshot_Available():
return available(_that);case BridgeUpdaterStateSnapshot_Downloading():
return downloading(_that);case BridgeUpdaterStateSnapshot_Verifying():
return verifying(_that);case BridgeUpdaterStateSnapshot_InstallerLaunched():
return installerLaunched(_that);case BridgeUpdaterStateSnapshot_CheckFailed():
return checkFailed(_that);case BridgeUpdaterStateSnapshot_InstallFailed():
return installFailed(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeUpdaterStateSnapshot_Disabled value)?  disabled,TResult? Function( BridgeUpdaterStateSnapshot_Idle value)?  idle,TResult? Function( BridgeUpdaterStateSnapshot_Checking value)?  checking,TResult? Function( BridgeUpdaterStateSnapshot_UpToDate value)?  upToDate,TResult? Function( BridgeUpdaterStateSnapshot_Available value)?  available,TResult? Function( BridgeUpdaterStateSnapshot_Downloading value)?  downloading,TResult? Function( BridgeUpdaterStateSnapshot_Verifying value)?  verifying,TResult? Function( BridgeUpdaterStateSnapshot_InstallerLaunched value)?  installerLaunched,TResult? Function( BridgeUpdaterStateSnapshot_CheckFailed value)?  checkFailed,TResult? Function( BridgeUpdaterStateSnapshot_InstallFailed value)?  installFailed,}){
final _that = this;
switch (_that) {
case BridgeUpdaterStateSnapshot_Disabled() when disabled != null:
return disabled(_that);case BridgeUpdaterStateSnapshot_Idle() when idle != null:
return idle(_that);case BridgeUpdaterStateSnapshot_Checking() when checking != null:
return checking(_that);case BridgeUpdaterStateSnapshot_UpToDate() when upToDate != null:
return upToDate(_that);case BridgeUpdaterStateSnapshot_Available() when available != null:
return available(_that);case BridgeUpdaterStateSnapshot_Downloading() when downloading != null:
return downloading(_that);case BridgeUpdaterStateSnapshot_Verifying() when verifying != null:
return verifying(_that);case BridgeUpdaterStateSnapshot_InstallerLaunched() when installerLaunched != null:
return installerLaunched(_that);case BridgeUpdaterStateSnapshot_CheckFailed() when checkFailed != null:
return checkFailed(_that);case BridgeUpdaterStateSnapshot_InstallFailed() when installFailed != null:
return installFailed(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeDisabledUpdaterState field0)?  disabled,TResult Function( BridgeIdleUpdaterState field0)?  idle,TResult Function( BridgeCheckingUpdaterState field0)?  checking,TResult Function( BridgeUpToDateUpdaterState field0)?  upToDate,TResult Function( BridgeAvailableUpdaterState field0)?  available,TResult Function( BridgeDownloadingUpdaterState field0)?  downloading,TResult Function( BridgeVerifyingUpdaterState field0)?  verifying,TResult Function( BridgeInstallerLaunchedUpdaterState field0)?  installerLaunched,TResult Function( BridgeCheckFailedUpdaterState field0)?  checkFailed,TResult Function( BridgeInstallFailedUpdaterState field0)?  installFailed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeUpdaterStateSnapshot_Disabled() when disabled != null:
return disabled(_that.field0);case BridgeUpdaterStateSnapshot_Idle() when idle != null:
return idle(_that.field0);case BridgeUpdaterStateSnapshot_Checking() when checking != null:
return checking(_that.field0);case BridgeUpdaterStateSnapshot_UpToDate() when upToDate != null:
return upToDate(_that.field0);case BridgeUpdaterStateSnapshot_Available() when available != null:
return available(_that.field0);case BridgeUpdaterStateSnapshot_Downloading() when downloading != null:
return downloading(_that.field0);case BridgeUpdaterStateSnapshot_Verifying() when verifying != null:
return verifying(_that.field0);case BridgeUpdaterStateSnapshot_InstallerLaunched() when installerLaunched != null:
return installerLaunched(_that.field0);case BridgeUpdaterStateSnapshot_CheckFailed() when checkFailed != null:
return checkFailed(_that.field0);case BridgeUpdaterStateSnapshot_InstallFailed() when installFailed != null:
return installFailed(_that.field0);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeDisabledUpdaterState field0)  disabled,required TResult Function( BridgeIdleUpdaterState field0)  idle,required TResult Function( BridgeCheckingUpdaterState field0)  checking,required TResult Function( BridgeUpToDateUpdaterState field0)  upToDate,required TResult Function( BridgeAvailableUpdaterState field0)  available,required TResult Function( BridgeDownloadingUpdaterState field0)  downloading,required TResult Function( BridgeVerifyingUpdaterState field0)  verifying,required TResult Function( BridgeInstallerLaunchedUpdaterState field0)  installerLaunched,required TResult Function( BridgeCheckFailedUpdaterState field0)  checkFailed,required TResult Function( BridgeInstallFailedUpdaterState field0)  installFailed,}) {final _that = this;
switch (_that) {
case BridgeUpdaterStateSnapshot_Disabled():
return disabled(_that.field0);case BridgeUpdaterStateSnapshot_Idle():
return idle(_that.field0);case BridgeUpdaterStateSnapshot_Checking():
return checking(_that.field0);case BridgeUpdaterStateSnapshot_UpToDate():
return upToDate(_that.field0);case BridgeUpdaterStateSnapshot_Available():
return available(_that.field0);case BridgeUpdaterStateSnapshot_Downloading():
return downloading(_that.field0);case BridgeUpdaterStateSnapshot_Verifying():
return verifying(_that.field0);case BridgeUpdaterStateSnapshot_InstallerLaunched():
return installerLaunched(_that.field0);case BridgeUpdaterStateSnapshot_CheckFailed():
return checkFailed(_that.field0);case BridgeUpdaterStateSnapshot_InstallFailed():
return installFailed(_that.field0);}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeDisabledUpdaterState field0)?  disabled,TResult? Function( BridgeIdleUpdaterState field0)?  idle,TResult? Function( BridgeCheckingUpdaterState field0)?  checking,TResult? Function( BridgeUpToDateUpdaterState field0)?  upToDate,TResult? Function( BridgeAvailableUpdaterState field0)?  available,TResult? Function( BridgeDownloadingUpdaterState field0)?  downloading,TResult? Function( BridgeVerifyingUpdaterState field0)?  verifying,TResult? Function( BridgeInstallerLaunchedUpdaterState field0)?  installerLaunched,TResult? Function( BridgeCheckFailedUpdaterState field0)?  checkFailed,TResult? Function( BridgeInstallFailedUpdaterState field0)?  installFailed,}) {final _that = this;
switch (_that) {
case BridgeUpdaterStateSnapshot_Disabled() when disabled != null:
return disabled(_that.field0);case BridgeUpdaterStateSnapshot_Idle() when idle != null:
return idle(_that.field0);case BridgeUpdaterStateSnapshot_Checking() when checking != null:
return checking(_that.field0);case BridgeUpdaterStateSnapshot_UpToDate() when upToDate != null:
return upToDate(_that.field0);case BridgeUpdaterStateSnapshot_Available() when available != null:
return available(_that.field0);case BridgeUpdaterStateSnapshot_Downloading() when downloading != null:
return downloading(_that.field0);case BridgeUpdaterStateSnapshot_Verifying() when verifying != null:
return verifying(_that.field0);case BridgeUpdaterStateSnapshot_InstallerLaunched() when installerLaunched != null:
return installerLaunched(_that.field0);case BridgeUpdaterStateSnapshot_CheckFailed() when checkFailed != null:
return checkFailed(_that.field0);case BridgeUpdaterStateSnapshot_InstallFailed() when installFailed != null:
return installFailed(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeUpdaterStateSnapshot_Disabled extends BridgeUpdaterStateSnapshot {
  const BridgeUpdaterStateSnapshot_Disabled(this.field0): super._();


@override final  BridgeDisabledUpdaterState field0;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUpdaterStateSnapshot_DisabledCopyWith<BridgeUpdaterStateSnapshot_Disabled> get copyWith => _$BridgeUpdaterStateSnapshot_DisabledCopyWithImpl<BridgeUpdaterStateSnapshot_Disabled>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUpdaterStateSnapshot_Disabled&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeUpdaterStateSnapshot.disabled(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeUpdaterStateSnapshot_DisabledCopyWith<$Res> implements $BridgeUpdaterStateSnapshotCopyWith<$Res> {
  factory $BridgeUpdaterStateSnapshot_DisabledCopyWith(BridgeUpdaterStateSnapshot_Disabled value, $Res Function(BridgeUpdaterStateSnapshot_Disabled) _then) = _$BridgeUpdaterStateSnapshot_DisabledCopyWithImpl;
@useResult
$Res call({
 BridgeDisabledUpdaterState field0
});




}
/// @nodoc
class _$BridgeUpdaterStateSnapshot_DisabledCopyWithImpl<$Res>
    implements $BridgeUpdaterStateSnapshot_DisabledCopyWith<$Res> {
  _$BridgeUpdaterStateSnapshot_DisabledCopyWithImpl(this._self, this._then);

  final BridgeUpdaterStateSnapshot_Disabled _self;
  final $Res Function(BridgeUpdaterStateSnapshot_Disabled) _then;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeUpdaterStateSnapshot_Disabled(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeDisabledUpdaterState,
  ));
}


}

/// @nodoc


class BridgeUpdaterStateSnapshot_Idle extends BridgeUpdaterStateSnapshot {
  const BridgeUpdaterStateSnapshot_Idle(this.field0): super._();


@override final  BridgeIdleUpdaterState field0;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUpdaterStateSnapshot_IdleCopyWith<BridgeUpdaterStateSnapshot_Idle> get copyWith => _$BridgeUpdaterStateSnapshot_IdleCopyWithImpl<BridgeUpdaterStateSnapshot_Idle>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUpdaterStateSnapshot_Idle&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeUpdaterStateSnapshot.idle(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeUpdaterStateSnapshot_IdleCopyWith<$Res> implements $BridgeUpdaterStateSnapshotCopyWith<$Res> {
  factory $BridgeUpdaterStateSnapshot_IdleCopyWith(BridgeUpdaterStateSnapshot_Idle value, $Res Function(BridgeUpdaterStateSnapshot_Idle) _then) = _$BridgeUpdaterStateSnapshot_IdleCopyWithImpl;
@useResult
$Res call({
 BridgeIdleUpdaterState field0
});




}
/// @nodoc
class _$BridgeUpdaterStateSnapshot_IdleCopyWithImpl<$Res>
    implements $BridgeUpdaterStateSnapshot_IdleCopyWith<$Res> {
  _$BridgeUpdaterStateSnapshot_IdleCopyWithImpl(this._self, this._then);

  final BridgeUpdaterStateSnapshot_Idle _self;
  final $Res Function(BridgeUpdaterStateSnapshot_Idle) _then;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeUpdaterStateSnapshot_Idle(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeIdleUpdaterState,
  ));
}


}

/// @nodoc


class BridgeUpdaterStateSnapshot_Checking extends BridgeUpdaterStateSnapshot {
  const BridgeUpdaterStateSnapshot_Checking(this.field0): super._();


@override final  BridgeCheckingUpdaterState field0;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUpdaterStateSnapshot_CheckingCopyWith<BridgeUpdaterStateSnapshot_Checking> get copyWith => _$BridgeUpdaterStateSnapshot_CheckingCopyWithImpl<BridgeUpdaterStateSnapshot_Checking>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUpdaterStateSnapshot_Checking&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeUpdaterStateSnapshot.checking(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeUpdaterStateSnapshot_CheckingCopyWith<$Res> implements $BridgeUpdaterStateSnapshotCopyWith<$Res> {
  factory $BridgeUpdaterStateSnapshot_CheckingCopyWith(BridgeUpdaterStateSnapshot_Checking value, $Res Function(BridgeUpdaterStateSnapshot_Checking) _then) = _$BridgeUpdaterStateSnapshot_CheckingCopyWithImpl;
@useResult
$Res call({
 BridgeCheckingUpdaterState field0
});




}
/// @nodoc
class _$BridgeUpdaterStateSnapshot_CheckingCopyWithImpl<$Res>
    implements $BridgeUpdaterStateSnapshot_CheckingCopyWith<$Res> {
  _$BridgeUpdaterStateSnapshot_CheckingCopyWithImpl(this._self, this._then);

  final BridgeUpdaterStateSnapshot_Checking _self;
  final $Res Function(BridgeUpdaterStateSnapshot_Checking) _then;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeUpdaterStateSnapshot_Checking(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeCheckingUpdaterState,
  ));
}


}

/// @nodoc


class BridgeUpdaterStateSnapshot_UpToDate extends BridgeUpdaterStateSnapshot {
  const BridgeUpdaterStateSnapshot_UpToDate(this.field0): super._();


@override final  BridgeUpToDateUpdaterState field0;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUpdaterStateSnapshot_UpToDateCopyWith<BridgeUpdaterStateSnapshot_UpToDate> get copyWith => _$BridgeUpdaterStateSnapshot_UpToDateCopyWithImpl<BridgeUpdaterStateSnapshot_UpToDate>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUpdaterStateSnapshot_UpToDate&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeUpdaterStateSnapshot.upToDate(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeUpdaterStateSnapshot_UpToDateCopyWith<$Res> implements $BridgeUpdaterStateSnapshotCopyWith<$Res> {
  factory $BridgeUpdaterStateSnapshot_UpToDateCopyWith(BridgeUpdaterStateSnapshot_UpToDate value, $Res Function(BridgeUpdaterStateSnapshot_UpToDate) _then) = _$BridgeUpdaterStateSnapshot_UpToDateCopyWithImpl;
@useResult
$Res call({
 BridgeUpToDateUpdaterState field0
});




}
/// @nodoc
class _$BridgeUpdaterStateSnapshot_UpToDateCopyWithImpl<$Res>
    implements $BridgeUpdaterStateSnapshot_UpToDateCopyWith<$Res> {
  _$BridgeUpdaterStateSnapshot_UpToDateCopyWithImpl(this._self, this._then);

  final BridgeUpdaterStateSnapshot_UpToDate _self;
  final $Res Function(BridgeUpdaterStateSnapshot_UpToDate) _then;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeUpdaterStateSnapshot_UpToDate(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeUpToDateUpdaterState,
  ));
}


}

/// @nodoc


class BridgeUpdaterStateSnapshot_Available extends BridgeUpdaterStateSnapshot {
  const BridgeUpdaterStateSnapshot_Available(this.field0): super._();


@override final  BridgeAvailableUpdaterState field0;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUpdaterStateSnapshot_AvailableCopyWith<BridgeUpdaterStateSnapshot_Available> get copyWith => _$BridgeUpdaterStateSnapshot_AvailableCopyWithImpl<BridgeUpdaterStateSnapshot_Available>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUpdaterStateSnapshot_Available&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeUpdaterStateSnapshot.available(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeUpdaterStateSnapshot_AvailableCopyWith<$Res> implements $BridgeUpdaterStateSnapshotCopyWith<$Res> {
  factory $BridgeUpdaterStateSnapshot_AvailableCopyWith(BridgeUpdaterStateSnapshot_Available value, $Res Function(BridgeUpdaterStateSnapshot_Available) _then) = _$BridgeUpdaterStateSnapshot_AvailableCopyWithImpl;
@useResult
$Res call({
 BridgeAvailableUpdaterState field0
});




}
/// @nodoc
class _$BridgeUpdaterStateSnapshot_AvailableCopyWithImpl<$Res>
    implements $BridgeUpdaterStateSnapshot_AvailableCopyWith<$Res> {
  _$BridgeUpdaterStateSnapshot_AvailableCopyWithImpl(this._self, this._then);

  final BridgeUpdaterStateSnapshot_Available _self;
  final $Res Function(BridgeUpdaterStateSnapshot_Available) _then;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeUpdaterStateSnapshot_Available(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeAvailableUpdaterState,
  ));
}


}

/// @nodoc


class BridgeUpdaterStateSnapshot_Downloading extends BridgeUpdaterStateSnapshot {
  const BridgeUpdaterStateSnapshot_Downloading(this.field0): super._();


@override final  BridgeDownloadingUpdaterState field0;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUpdaterStateSnapshot_DownloadingCopyWith<BridgeUpdaterStateSnapshot_Downloading> get copyWith => _$BridgeUpdaterStateSnapshot_DownloadingCopyWithImpl<BridgeUpdaterStateSnapshot_Downloading>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUpdaterStateSnapshot_Downloading&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeUpdaterStateSnapshot.downloading(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeUpdaterStateSnapshot_DownloadingCopyWith<$Res> implements $BridgeUpdaterStateSnapshotCopyWith<$Res> {
  factory $BridgeUpdaterStateSnapshot_DownloadingCopyWith(BridgeUpdaterStateSnapshot_Downloading value, $Res Function(BridgeUpdaterStateSnapshot_Downloading) _then) = _$BridgeUpdaterStateSnapshot_DownloadingCopyWithImpl;
@useResult
$Res call({
 BridgeDownloadingUpdaterState field0
});




}
/// @nodoc
class _$BridgeUpdaterStateSnapshot_DownloadingCopyWithImpl<$Res>
    implements $BridgeUpdaterStateSnapshot_DownloadingCopyWith<$Res> {
  _$BridgeUpdaterStateSnapshot_DownloadingCopyWithImpl(this._self, this._then);

  final BridgeUpdaterStateSnapshot_Downloading _self;
  final $Res Function(BridgeUpdaterStateSnapshot_Downloading) _then;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeUpdaterStateSnapshot_Downloading(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeDownloadingUpdaterState,
  ));
}


}

/// @nodoc


class BridgeUpdaterStateSnapshot_Verifying extends BridgeUpdaterStateSnapshot {
  const BridgeUpdaterStateSnapshot_Verifying(this.field0): super._();


@override final  BridgeVerifyingUpdaterState field0;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUpdaterStateSnapshot_VerifyingCopyWith<BridgeUpdaterStateSnapshot_Verifying> get copyWith => _$BridgeUpdaterStateSnapshot_VerifyingCopyWithImpl<BridgeUpdaterStateSnapshot_Verifying>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUpdaterStateSnapshot_Verifying&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeUpdaterStateSnapshot.verifying(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeUpdaterStateSnapshot_VerifyingCopyWith<$Res> implements $BridgeUpdaterStateSnapshotCopyWith<$Res> {
  factory $BridgeUpdaterStateSnapshot_VerifyingCopyWith(BridgeUpdaterStateSnapshot_Verifying value, $Res Function(BridgeUpdaterStateSnapshot_Verifying) _then) = _$BridgeUpdaterStateSnapshot_VerifyingCopyWithImpl;
@useResult
$Res call({
 BridgeVerifyingUpdaterState field0
});




}
/// @nodoc
class _$BridgeUpdaterStateSnapshot_VerifyingCopyWithImpl<$Res>
    implements $BridgeUpdaterStateSnapshot_VerifyingCopyWith<$Res> {
  _$BridgeUpdaterStateSnapshot_VerifyingCopyWithImpl(this._self, this._then);

  final BridgeUpdaterStateSnapshot_Verifying _self;
  final $Res Function(BridgeUpdaterStateSnapshot_Verifying) _then;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeUpdaterStateSnapshot_Verifying(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeVerifyingUpdaterState,
  ));
}


}

/// @nodoc


class BridgeUpdaterStateSnapshot_InstallerLaunched extends BridgeUpdaterStateSnapshot {
  const BridgeUpdaterStateSnapshot_InstallerLaunched(this.field0): super._();


@override final  BridgeInstallerLaunchedUpdaterState field0;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUpdaterStateSnapshot_InstallerLaunchedCopyWith<BridgeUpdaterStateSnapshot_InstallerLaunched> get copyWith => _$BridgeUpdaterStateSnapshot_InstallerLaunchedCopyWithImpl<BridgeUpdaterStateSnapshot_InstallerLaunched>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUpdaterStateSnapshot_InstallerLaunched&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeUpdaterStateSnapshot.installerLaunched(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeUpdaterStateSnapshot_InstallerLaunchedCopyWith<$Res> implements $BridgeUpdaterStateSnapshotCopyWith<$Res> {
  factory $BridgeUpdaterStateSnapshot_InstallerLaunchedCopyWith(BridgeUpdaterStateSnapshot_InstallerLaunched value, $Res Function(BridgeUpdaterStateSnapshot_InstallerLaunched) _then) = _$BridgeUpdaterStateSnapshot_InstallerLaunchedCopyWithImpl;
@useResult
$Res call({
 BridgeInstallerLaunchedUpdaterState field0
});




}
/// @nodoc
class _$BridgeUpdaterStateSnapshot_InstallerLaunchedCopyWithImpl<$Res>
    implements $BridgeUpdaterStateSnapshot_InstallerLaunchedCopyWith<$Res> {
  _$BridgeUpdaterStateSnapshot_InstallerLaunchedCopyWithImpl(this._self, this._then);

  final BridgeUpdaterStateSnapshot_InstallerLaunched _self;
  final $Res Function(BridgeUpdaterStateSnapshot_InstallerLaunched) _then;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeUpdaterStateSnapshot_InstallerLaunched(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeInstallerLaunchedUpdaterState,
  ));
}


}

/// @nodoc


class BridgeUpdaterStateSnapshot_CheckFailed extends BridgeUpdaterStateSnapshot {
  const BridgeUpdaterStateSnapshot_CheckFailed(this.field0): super._();


@override final  BridgeCheckFailedUpdaterState field0;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUpdaterStateSnapshot_CheckFailedCopyWith<BridgeUpdaterStateSnapshot_CheckFailed> get copyWith => _$BridgeUpdaterStateSnapshot_CheckFailedCopyWithImpl<BridgeUpdaterStateSnapshot_CheckFailed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUpdaterStateSnapshot_CheckFailed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeUpdaterStateSnapshot.checkFailed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeUpdaterStateSnapshot_CheckFailedCopyWith<$Res> implements $BridgeUpdaterStateSnapshotCopyWith<$Res> {
  factory $BridgeUpdaterStateSnapshot_CheckFailedCopyWith(BridgeUpdaterStateSnapshot_CheckFailed value, $Res Function(BridgeUpdaterStateSnapshot_CheckFailed) _then) = _$BridgeUpdaterStateSnapshot_CheckFailedCopyWithImpl;
@useResult
$Res call({
 BridgeCheckFailedUpdaterState field0
});




}
/// @nodoc
class _$BridgeUpdaterStateSnapshot_CheckFailedCopyWithImpl<$Res>
    implements $BridgeUpdaterStateSnapshot_CheckFailedCopyWith<$Res> {
  _$BridgeUpdaterStateSnapshot_CheckFailedCopyWithImpl(this._self, this._then);

  final BridgeUpdaterStateSnapshot_CheckFailed _self;
  final $Res Function(BridgeUpdaterStateSnapshot_CheckFailed) _then;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeUpdaterStateSnapshot_CheckFailed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeCheckFailedUpdaterState,
  ));
}


}

/// @nodoc


class BridgeUpdaterStateSnapshot_InstallFailed extends BridgeUpdaterStateSnapshot {
  const BridgeUpdaterStateSnapshot_InstallFailed(this.field0): super._();


@override final  BridgeInstallFailedUpdaterState field0;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUpdaterStateSnapshot_InstallFailedCopyWith<BridgeUpdaterStateSnapshot_InstallFailed> get copyWith => _$BridgeUpdaterStateSnapshot_InstallFailedCopyWithImpl<BridgeUpdaterStateSnapshot_InstallFailed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUpdaterStateSnapshot_InstallFailed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeUpdaterStateSnapshot.installFailed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeUpdaterStateSnapshot_InstallFailedCopyWith<$Res> implements $BridgeUpdaterStateSnapshotCopyWith<$Res> {
  factory $BridgeUpdaterStateSnapshot_InstallFailedCopyWith(BridgeUpdaterStateSnapshot_InstallFailed value, $Res Function(BridgeUpdaterStateSnapshot_InstallFailed) _then) = _$BridgeUpdaterStateSnapshot_InstallFailedCopyWithImpl;
@useResult
$Res call({
 BridgeInstallFailedUpdaterState field0
});




}
/// @nodoc
class _$BridgeUpdaterStateSnapshot_InstallFailedCopyWithImpl<$Res>
    implements $BridgeUpdaterStateSnapshot_InstallFailedCopyWith<$Res> {
  _$BridgeUpdaterStateSnapshot_InstallFailedCopyWithImpl(this._self, this._then);

  final BridgeUpdaterStateSnapshot_InstallFailed _self;
  final $Res Function(BridgeUpdaterStateSnapshot_InstallFailed) _then;

/// Create a copy of BridgeUpdaterStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeUpdaterStateSnapshot_InstallFailed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeInstallFailedUpdaterState,
  ));
}


}

// dart format on

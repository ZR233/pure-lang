// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'updater.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeStudioUpdateCheckDto {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeStudioUpdateCheckDto);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeStudioUpdateCheckDto()';
}


}

/// @nodoc
class $BridgeStudioUpdateCheckDtoCopyWith<$Res>  {
$BridgeStudioUpdateCheckDtoCopyWith(BridgeStudioUpdateCheckDto _, $Res Function(BridgeStudioUpdateCheckDto) __);
}


/// Adds pattern-matching-related methods to [BridgeStudioUpdateCheckDto].
extension BridgeStudioUpdateCheckDtoPatterns on BridgeStudioUpdateCheckDto {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeStudioUpdateCheckDto_UpToDate value)?  upToDate,TResult Function( BridgeStudioUpdateCheckDto_Available value)?  available,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeStudioUpdateCheckDto_UpToDate() when upToDate != null:
return upToDate(_that);case BridgeStudioUpdateCheckDto_Available() when available != null:
return available(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeStudioUpdateCheckDto_UpToDate value)  upToDate,required TResult Function( BridgeStudioUpdateCheckDto_Available value)  available,}){
final _that = this;
switch (_that) {
case BridgeStudioUpdateCheckDto_UpToDate():
return upToDate(_that);case BridgeStudioUpdateCheckDto_Available():
return available(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeStudioUpdateCheckDto_UpToDate value)?  upToDate,TResult? Function( BridgeStudioUpdateCheckDto_Available value)?  available,}){
final _that = this;
switch (_that) {
case BridgeStudioUpdateCheckDto_UpToDate() when upToDate != null:
return upToDate(_that);case BridgeStudioUpdateCheckDto_Available() when available != null:
return available(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  upToDate,TResult Function( BridgeStudioUpdateDto update)?  available,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeStudioUpdateCheckDto_UpToDate() when upToDate != null:
return upToDate();case BridgeStudioUpdateCheckDto_Available() when available != null:
return available(_that.update);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  upToDate,required TResult Function( BridgeStudioUpdateDto update)  available,}) {final _that = this;
switch (_that) {
case BridgeStudioUpdateCheckDto_UpToDate():
return upToDate();case BridgeStudioUpdateCheckDto_Available():
return available(_that.update);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  upToDate,TResult? Function( BridgeStudioUpdateDto update)?  available,}) {final _that = this;
switch (_that) {
case BridgeStudioUpdateCheckDto_UpToDate() when upToDate != null:
return upToDate();case BridgeStudioUpdateCheckDto_Available() when available != null:
return available(_that.update);case _:
  return null;

}
}

}

/// @nodoc


class BridgeStudioUpdateCheckDto_UpToDate extends BridgeStudioUpdateCheckDto {
  const BridgeStudioUpdateCheckDto_UpToDate(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeStudioUpdateCheckDto_UpToDate);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeStudioUpdateCheckDto.upToDate()';
}


}




/// @nodoc


class BridgeStudioUpdateCheckDto_Available extends BridgeStudioUpdateCheckDto {
  const BridgeStudioUpdateCheckDto_Available({required this.update}): super._();


 final  BridgeStudioUpdateDto update;

/// Create a copy of BridgeStudioUpdateCheckDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeStudioUpdateCheckDto_AvailableCopyWith<BridgeStudioUpdateCheckDto_Available> get copyWith => _$BridgeStudioUpdateCheckDto_AvailableCopyWithImpl<BridgeStudioUpdateCheckDto_Available>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeStudioUpdateCheckDto_Available&&(identical(other.update, update) || other.update == update));
}


@override
int get hashCode => Object.hash(runtimeType,update);

@override
String toString() {
  return 'BridgeStudioUpdateCheckDto.available(update: $update)';
}


}

/// @nodoc
abstract mixin class $BridgeStudioUpdateCheckDto_AvailableCopyWith<$Res> implements $BridgeStudioUpdateCheckDtoCopyWith<$Res> {
  factory $BridgeStudioUpdateCheckDto_AvailableCopyWith(BridgeStudioUpdateCheckDto_Available value, $Res Function(BridgeStudioUpdateCheckDto_Available) _then) = _$BridgeStudioUpdateCheckDto_AvailableCopyWithImpl;
@useResult
$Res call({
 BridgeStudioUpdateDto update
});




}
/// @nodoc
class _$BridgeStudioUpdateCheckDto_AvailableCopyWithImpl<$Res>
    implements $BridgeStudioUpdateCheckDto_AvailableCopyWith<$Res> {
  _$BridgeStudioUpdateCheckDto_AvailableCopyWithImpl(this._self, this._then);

  final BridgeStudioUpdateCheckDto_Available _self;
  final $Res Function(BridgeStudioUpdateCheckDto_Available) _then;

/// Create a copy of BridgeStudioUpdateCheckDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? update = null,}) {
  return _then(BridgeStudioUpdateCheckDto_Available(
update: null == update ? _self.update : update // ignore: cast_nullable_to_non_nullable
as BridgeStudioUpdateDto,
  ));
}


}

/// @nodoc
mixin _$BridgeStudioUpdateEventDto {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeStudioUpdateEventDto);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeStudioUpdateEventDto()';
}


}

/// @nodoc
class $BridgeStudioUpdateEventDtoCopyWith<$Res>  {
$BridgeStudioUpdateEventDtoCopyWith(BridgeStudioUpdateEventDto _, $Res Function(BridgeStudioUpdateEventDto) __);
}


/// Adds pattern-matching-related methods to [BridgeStudioUpdateEventDto].
extension BridgeStudioUpdateEventDtoPatterns on BridgeStudioUpdateEventDto {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeStudioUpdateEventDto_Started value)?  started,TResult Function( BridgeStudioUpdateEventDto_Progress value)?  progress,TResult Function( BridgeStudioUpdateEventDto_Verifying value)?  verifying,TResult Function( BridgeStudioUpdateEventDto_InstallerLaunched value)?  installerLaunched,TResult Function( BridgeStudioUpdateEventDto_Failed value)?  failed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeStudioUpdateEventDto_Started() when started != null:
return started(_that);case BridgeStudioUpdateEventDto_Progress() when progress != null:
return progress(_that);case BridgeStudioUpdateEventDto_Verifying() when verifying != null:
return verifying(_that);case BridgeStudioUpdateEventDto_InstallerLaunched() when installerLaunched != null:
return installerLaunched(_that);case BridgeStudioUpdateEventDto_Failed() when failed != null:
return failed(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeStudioUpdateEventDto_Started value)  started,required TResult Function( BridgeStudioUpdateEventDto_Progress value)  progress,required TResult Function( BridgeStudioUpdateEventDto_Verifying value)  verifying,required TResult Function( BridgeStudioUpdateEventDto_InstallerLaunched value)  installerLaunched,required TResult Function( BridgeStudioUpdateEventDto_Failed value)  failed,}){
final _that = this;
switch (_that) {
case BridgeStudioUpdateEventDto_Started():
return started(_that);case BridgeStudioUpdateEventDto_Progress():
return progress(_that);case BridgeStudioUpdateEventDto_Verifying():
return verifying(_that);case BridgeStudioUpdateEventDto_InstallerLaunched():
return installerLaunched(_that);case BridgeStudioUpdateEventDto_Failed():
return failed(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeStudioUpdateEventDto_Started value)?  started,TResult? Function( BridgeStudioUpdateEventDto_Progress value)?  progress,TResult? Function( BridgeStudioUpdateEventDto_Verifying value)?  verifying,TResult? Function( BridgeStudioUpdateEventDto_InstallerLaunched value)?  installerLaunched,TResult? Function( BridgeStudioUpdateEventDto_Failed value)?  failed,}){
final _that = this;
switch (_that) {
case BridgeStudioUpdateEventDto_Started() when started != null:
return started(_that);case BridgeStudioUpdateEventDto_Progress() when progress != null:
return progress(_that);case BridgeStudioUpdateEventDto_Verifying() when verifying != null:
return verifying(_that);case BridgeStudioUpdateEventDto_InstallerLaunched() when installerLaunched != null:
return installerLaunched(_that);case BridgeStudioUpdateEventDto_Failed() when failed != null:
return failed(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BigInt total)?  started,TResult Function( BigInt downloaded,  BigInt total)?  progress,TResult Function()?  verifying,TResult Function()?  installerLaunched,TResult Function( String code,  String message)?  failed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeStudioUpdateEventDto_Started() when started != null:
return started(_that.total);case BridgeStudioUpdateEventDto_Progress() when progress != null:
return progress(_that.downloaded,_that.total);case BridgeStudioUpdateEventDto_Verifying() when verifying != null:
return verifying();case BridgeStudioUpdateEventDto_InstallerLaunched() when installerLaunched != null:
return installerLaunched();case BridgeStudioUpdateEventDto_Failed() when failed != null:
return failed(_that.code,_that.message);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BigInt total)  started,required TResult Function( BigInt downloaded,  BigInt total)  progress,required TResult Function()  verifying,required TResult Function()  installerLaunched,required TResult Function( String code,  String message)  failed,}) {final _that = this;
switch (_that) {
case BridgeStudioUpdateEventDto_Started():
return started(_that.total);case BridgeStudioUpdateEventDto_Progress():
return progress(_that.downloaded,_that.total);case BridgeStudioUpdateEventDto_Verifying():
return verifying();case BridgeStudioUpdateEventDto_InstallerLaunched():
return installerLaunched();case BridgeStudioUpdateEventDto_Failed():
return failed(_that.code,_that.message);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BigInt total)?  started,TResult? Function( BigInt downloaded,  BigInt total)?  progress,TResult? Function()?  verifying,TResult? Function()?  installerLaunched,TResult? Function( String code,  String message)?  failed,}) {final _that = this;
switch (_that) {
case BridgeStudioUpdateEventDto_Started() when started != null:
return started(_that.total);case BridgeStudioUpdateEventDto_Progress() when progress != null:
return progress(_that.downloaded,_that.total);case BridgeStudioUpdateEventDto_Verifying() when verifying != null:
return verifying();case BridgeStudioUpdateEventDto_InstallerLaunched() when installerLaunched != null:
return installerLaunched();case BridgeStudioUpdateEventDto_Failed() when failed != null:
return failed(_that.code,_that.message);case _:
  return null;

}
}

}

/// @nodoc


class BridgeStudioUpdateEventDto_Started extends BridgeStudioUpdateEventDto {
  const BridgeStudioUpdateEventDto_Started({required this.total}): super._();


 final  BigInt total;

/// Create a copy of BridgeStudioUpdateEventDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeStudioUpdateEventDto_StartedCopyWith<BridgeStudioUpdateEventDto_Started> get copyWith => _$BridgeStudioUpdateEventDto_StartedCopyWithImpl<BridgeStudioUpdateEventDto_Started>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeStudioUpdateEventDto_Started&&(identical(other.total, total) || other.total == total));
}


@override
int get hashCode => Object.hash(runtimeType,total);

@override
String toString() {
  return 'BridgeStudioUpdateEventDto.started(total: $total)';
}


}

/// @nodoc
abstract mixin class $BridgeStudioUpdateEventDto_StartedCopyWith<$Res> implements $BridgeStudioUpdateEventDtoCopyWith<$Res> {
  factory $BridgeStudioUpdateEventDto_StartedCopyWith(BridgeStudioUpdateEventDto_Started value, $Res Function(BridgeStudioUpdateEventDto_Started) _then) = _$BridgeStudioUpdateEventDto_StartedCopyWithImpl;
@useResult
$Res call({
 BigInt total
});




}
/// @nodoc
class _$BridgeStudioUpdateEventDto_StartedCopyWithImpl<$Res>
    implements $BridgeStudioUpdateEventDto_StartedCopyWith<$Res> {
  _$BridgeStudioUpdateEventDto_StartedCopyWithImpl(this._self, this._then);

  final BridgeStudioUpdateEventDto_Started _self;
  final $Res Function(BridgeStudioUpdateEventDto_Started) _then;

/// Create a copy of BridgeStudioUpdateEventDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? total = null,}) {
  return _then(BridgeStudioUpdateEventDto_Started(
total: null == total ? _self.total : total // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

/// @nodoc


class BridgeStudioUpdateEventDto_Progress extends BridgeStudioUpdateEventDto {
  const BridgeStudioUpdateEventDto_Progress({required this.downloaded, required this.total}): super._();


 final  BigInt downloaded;
 final  BigInt total;

/// Create a copy of BridgeStudioUpdateEventDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeStudioUpdateEventDto_ProgressCopyWith<BridgeStudioUpdateEventDto_Progress> get copyWith => _$BridgeStudioUpdateEventDto_ProgressCopyWithImpl<BridgeStudioUpdateEventDto_Progress>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeStudioUpdateEventDto_Progress&&(identical(other.downloaded, downloaded) || other.downloaded == downloaded)&&(identical(other.total, total) || other.total == total));
}


@override
int get hashCode => Object.hash(runtimeType,downloaded,total);

@override
String toString() {
  return 'BridgeStudioUpdateEventDto.progress(downloaded: $downloaded, total: $total)';
}


}

/// @nodoc
abstract mixin class $BridgeStudioUpdateEventDto_ProgressCopyWith<$Res> implements $BridgeStudioUpdateEventDtoCopyWith<$Res> {
  factory $BridgeStudioUpdateEventDto_ProgressCopyWith(BridgeStudioUpdateEventDto_Progress value, $Res Function(BridgeStudioUpdateEventDto_Progress) _then) = _$BridgeStudioUpdateEventDto_ProgressCopyWithImpl;
@useResult
$Res call({
 BigInt downloaded, BigInt total
});




}
/// @nodoc
class _$BridgeStudioUpdateEventDto_ProgressCopyWithImpl<$Res>
    implements $BridgeStudioUpdateEventDto_ProgressCopyWith<$Res> {
  _$BridgeStudioUpdateEventDto_ProgressCopyWithImpl(this._self, this._then);

  final BridgeStudioUpdateEventDto_Progress _self;
  final $Res Function(BridgeStudioUpdateEventDto_Progress) _then;

/// Create a copy of BridgeStudioUpdateEventDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? downloaded = null,Object? total = null,}) {
  return _then(BridgeStudioUpdateEventDto_Progress(
downloaded: null == downloaded ? _self.downloaded : downloaded // ignore: cast_nullable_to_non_nullable
as BigInt,total: null == total ? _self.total : total // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

/// @nodoc


class BridgeStudioUpdateEventDto_Verifying extends BridgeStudioUpdateEventDto {
  const BridgeStudioUpdateEventDto_Verifying(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeStudioUpdateEventDto_Verifying);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeStudioUpdateEventDto.verifying()';
}


}




/// @nodoc


class BridgeStudioUpdateEventDto_InstallerLaunched extends BridgeStudioUpdateEventDto {
  const BridgeStudioUpdateEventDto_InstallerLaunched(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeStudioUpdateEventDto_InstallerLaunched);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeStudioUpdateEventDto.installerLaunched()';
}


}




/// @nodoc


class BridgeStudioUpdateEventDto_Failed extends BridgeStudioUpdateEventDto {
  const BridgeStudioUpdateEventDto_Failed({required this.code, required this.message}): super._();


 final  String code;
 final  String message;

/// Create a copy of BridgeStudioUpdateEventDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeStudioUpdateEventDto_FailedCopyWith<BridgeStudioUpdateEventDto_Failed> get copyWith => _$BridgeStudioUpdateEventDto_FailedCopyWithImpl<BridgeStudioUpdateEventDto_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeStudioUpdateEventDto_Failed&&(identical(other.code, code) || other.code == code)&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode => Object.hash(runtimeType,code,message);

@override
String toString() {
  return 'BridgeStudioUpdateEventDto.failed(code: $code, message: $message)';
}


}

/// @nodoc
abstract mixin class $BridgeStudioUpdateEventDto_FailedCopyWith<$Res> implements $BridgeStudioUpdateEventDtoCopyWith<$Res> {
  factory $BridgeStudioUpdateEventDto_FailedCopyWith(BridgeStudioUpdateEventDto_Failed value, $Res Function(BridgeStudioUpdateEventDto_Failed) _then) = _$BridgeStudioUpdateEventDto_FailedCopyWithImpl;
@useResult
$Res call({
 String code, String message
});




}
/// @nodoc
class _$BridgeStudioUpdateEventDto_FailedCopyWithImpl<$Res>
    implements $BridgeStudioUpdateEventDto_FailedCopyWith<$Res> {
  _$BridgeStudioUpdateEventDto_FailedCopyWithImpl(this._self, this._then);

  final BridgeStudioUpdateEventDto_Failed _self;
  final $Res Function(BridgeStudioUpdateEventDto_Failed) _then;

/// Create a copy of BridgeStudioUpdateEventDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? code = null,Object? message = null,}) {
  return _then(BridgeStudioUpdateEventDto_Failed(
code: null == code ? _self.code : code // ignore: cast_nullable_to_non_nullable
as String,message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

// dart format on

// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'agent.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeAgentTimelinePayloadDto {

 String get callId; String? get agentId; String? get path; String? get parentPath; String get kind; String? get status; String? get message; bool get timedOut; String? get error;
/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentTimelinePayloadDtoCopyWith<BridgeAgentTimelinePayloadDto> get copyWith => _$BridgeAgentTimelinePayloadDtoCopyWithImpl<BridgeAgentTimelinePayloadDto>(this as BridgeAgentTimelinePayloadDto, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentTimelinePayloadDto&&(identical(other.callId, callId) || other.callId == callId)&&(identical(other.agentId, agentId) || other.agentId == agentId)&&(identical(other.path, path) || other.path == path)&&(identical(other.parentPath, parentPath) || other.parentPath == parentPath)&&(identical(other.kind, kind) || other.kind == kind)&&(identical(other.status, status) || other.status == status)&&(identical(other.message, message) || other.message == message)&&(identical(other.timedOut, timedOut) || other.timedOut == timedOut)&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,callId,agentId,path,parentPath,kind,status,message,timedOut,error);

@override
String toString() {
  return 'BridgeAgentTimelinePayloadDto(callId: $callId, agentId: $agentId, path: $path, parentPath: $parentPath, kind: $kind, status: $status, message: $message, timedOut: $timedOut, error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentTimelinePayloadDtoCopyWith<$Res>  {
  factory $BridgeAgentTimelinePayloadDtoCopyWith(BridgeAgentTimelinePayloadDto value, $Res Function(BridgeAgentTimelinePayloadDto) _then) = _$BridgeAgentTimelinePayloadDtoCopyWithImpl;
@useResult
$Res call({
 String callId, String? agentId, String? path, String? parentPath, String kind, String? status, String? message, bool timedOut, String? error
});




}
/// @nodoc
class _$BridgeAgentTimelinePayloadDtoCopyWithImpl<$Res>
    implements $BridgeAgentTimelinePayloadDtoCopyWith<$Res> {
  _$BridgeAgentTimelinePayloadDtoCopyWithImpl(this._self, this._then);

  final BridgeAgentTimelinePayloadDto _self;
  final $Res Function(BridgeAgentTimelinePayloadDto) _then;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? callId = null,Object? agentId = freezed,Object? path = freezed,Object? parentPath = freezed,Object? kind = null,Object? status = freezed,Object? message = freezed,Object? timedOut = null,Object? error = freezed,}) {
  return _then(_self.copyWith(
callId: null == callId ? _self.callId : callId // ignore: cast_nullable_to_non_nullable
as String,agentId: freezed == agentId ? _self.agentId : agentId // ignore: cast_nullable_to_non_nullable
as String?,path: freezed == path ? _self.path : path // ignore: cast_nullable_to_non_nullable
as String?,parentPath: freezed == parentPath ? _self.parentPath : parentPath // ignore: cast_nullable_to_non_nullable
as String?,kind: null == kind ? _self.kind : kind // ignore: cast_nullable_to_non_nullable
as String,status: freezed == status ? _self.status : status // ignore: cast_nullable_to_non_nullable
as String?,message: freezed == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String?,timedOut: null == timedOut ? _self.timedOut : timedOut // ignore: cast_nullable_to_non_nullable
as bool,error: freezed == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}

}


/// Adds pattern-matching-related methods to [BridgeAgentTimelinePayloadDto].
extension BridgeAgentTimelinePayloadDtoPatterns on BridgeAgentTimelinePayloadDto {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeAgentTimelinePayloadDto_SubAgentActivity value)?  subAgentActivity,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeAgentTimelinePayloadDto_SubAgentActivity() when subAgentActivity != null:
return subAgentActivity(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeAgentTimelinePayloadDto_SubAgentActivity value)  subAgentActivity,}){
final _that = this;
switch (_that) {
case BridgeAgentTimelinePayloadDto_SubAgentActivity():
return subAgentActivity(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeAgentTimelinePayloadDto_SubAgentActivity value)?  subAgentActivity,}){
final _that = this;
switch (_that) {
case BridgeAgentTimelinePayloadDto_SubAgentActivity() when subAgentActivity != null:
return subAgentActivity(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String callId,  String? agentId,  String? path,  String? parentPath,  String kind,  String? status,  String? message,  bool timedOut,  String? error)?  subAgentActivity,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeAgentTimelinePayloadDto_SubAgentActivity() when subAgentActivity != null:
return subAgentActivity(_that.callId,_that.agentId,_that.path,_that.parentPath,_that.kind,_that.status,_that.message,_that.timedOut,_that.error);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String callId,  String? agentId,  String? path,  String? parentPath,  String kind,  String? status,  String? message,  bool timedOut,  String? error)  subAgentActivity,}) {final _that = this;
switch (_that) {
case BridgeAgentTimelinePayloadDto_SubAgentActivity():
return subAgentActivity(_that.callId,_that.agentId,_that.path,_that.parentPath,_that.kind,_that.status,_that.message,_that.timedOut,_that.error);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String callId,  String? agentId,  String? path,  String? parentPath,  String kind,  String? status,  String? message,  bool timedOut,  String? error)?  subAgentActivity,}) {final _that = this;
switch (_that) {
case BridgeAgentTimelinePayloadDto_SubAgentActivity() when subAgentActivity != null:
return subAgentActivity(_that.callId,_that.agentId,_that.path,_that.parentPath,_that.kind,_that.status,_that.message,_that.timedOut,_that.error);case _:
  return null;

}
}

}

/// @nodoc


class BridgeAgentTimelinePayloadDto_SubAgentActivity extends BridgeAgentTimelinePayloadDto {
  const BridgeAgentTimelinePayloadDto_SubAgentActivity({required this.callId, this.agentId, this.path, this.parentPath, required this.kind, this.status, this.message, required this.timedOut, this.error}): super._();
  

@override final  String callId;
@override final  String? agentId;
@override final  String? path;
@override final  String? parentPath;
@override final  String kind;
@override final  String? status;
@override final  String? message;
@override final  bool timedOut;
@override final  String? error;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentTimelinePayloadDto_SubAgentActivityCopyWith<BridgeAgentTimelinePayloadDto_SubAgentActivity> get copyWith => _$BridgeAgentTimelinePayloadDto_SubAgentActivityCopyWithImpl<BridgeAgentTimelinePayloadDto_SubAgentActivity>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentTimelinePayloadDto_SubAgentActivity&&(identical(other.callId, callId) || other.callId == callId)&&(identical(other.agentId, agentId) || other.agentId == agentId)&&(identical(other.path, path) || other.path == path)&&(identical(other.parentPath, parentPath) || other.parentPath == parentPath)&&(identical(other.kind, kind) || other.kind == kind)&&(identical(other.status, status) || other.status == status)&&(identical(other.message, message) || other.message == message)&&(identical(other.timedOut, timedOut) || other.timedOut == timedOut)&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,callId,agentId,path,parentPath,kind,status,message,timedOut,error);

@override
String toString() {
  return 'BridgeAgentTimelinePayloadDto.subAgentActivity(callId: $callId, agentId: $agentId, path: $path, parentPath: $parentPath, kind: $kind, status: $status, message: $message, timedOut: $timedOut, error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentTimelinePayloadDto_SubAgentActivityCopyWith<$Res> implements $BridgeAgentTimelinePayloadDtoCopyWith<$Res> {
  factory $BridgeAgentTimelinePayloadDto_SubAgentActivityCopyWith(BridgeAgentTimelinePayloadDto_SubAgentActivity value, $Res Function(BridgeAgentTimelinePayloadDto_SubAgentActivity) _then) = _$BridgeAgentTimelinePayloadDto_SubAgentActivityCopyWithImpl;
@override @useResult
$Res call({
 String callId, String? agentId, String? path, String? parentPath, String kind, String? status, String? message, bool timedOut, String? error
});




}
/// @nodoc
class _$BridgeAgentTimelinePayloadDto_SubAgentActivityCopyWithImpl<$Res>
    implements $BridgeAgentTimelinePayloadDto_SubAgentActivityCopyWith<$Res> {
  _$BridgeAgentTimelinePayloadDto_SubAgentActivityCopyWithImpl(this._self, this._then);

  final BridgeAgentTimelinePayloadDto_SubAgentActivity _self;
  final $Res Function(BridgeAgentTimelinePayloadDto_SubAgentActivity) _then;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? callId = null,Object? agentId = freezed,Object? path = freezed,Object? parentPath = freezed,Object? kind = null,Object? status = freezed,Object? message = freezed,Object? timedOut = null,Object? error = freezed,}) {
  return _then(BridgeAgentTimelinePayloadDto_SubAgentActivity(
callId: null == callId ? _self.callId : callId // ignore: cast_nullable_to_non_nullable
as String,agentId: freezed == agentId ? _self.agentId : agentId // ignore: cast_nullable_to_non_nullable
as String?,path: freezed == path ? _self.path : path // ignore: cast_nullable_to_non_nullable
as String?,parentPath: freezed == parentPath ? _self.parentPath : parentPath // ignore: cast_nullable_to_non_nullable
as String?,kind: null == kind ? _self.kind : kind // ignore: cast_nullable_to_non_nullable
as String,status: freezed == status ? _self.status : status // ignore: cast_nullable_to_non_nullable
as String?,message: freezed == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String?,timedOut: null == timedOut ? _self.timedOut : timedOut // ignore: cast_nullable_to_non_nullable
as bool,error: freezed == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

// dart format on

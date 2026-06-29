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

 String get callId; String get senderPath;
/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentTimelinePayloadDtoCopyWith<BridgeAgentTimelinePayloadDto> get copyWith => _$BridgeAgentTimelinePayloadDtoCopyWithImpl<BridgeAgentTimelinePayloadDto>(this as BridgeAgentTimelinePayloadDto, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentTimelinePayloadDto&&(identical(other.callId, callId) || other.callId == callId)&&(identical(other.senderPath, senderPath) || other.senderPath == senderPath));
}


@override
int get hashCode => Object.hash(runtimeType,callId,senderPath);

@override
String toString() {
  return 'BridgeAgentTimelinePayloadDto(callId: $callId, senderPath: $senderPath)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentTimelinePayloadDtoCopyWith<$Res>  {
  factory $BridgeAgentTimelinePayloadDtoCopyWith(BridgeAgentTimelinePayloadDto value, $Res Function(BridgeAgentTimelinePayloadDto) _then) = _$BridgeAgentTimelinePayloadDtoCopyWithImpl;
@useResult
$Res call({
 String callId, String senderPath
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
@pragma('vm:prefer-inline') @override $Res call({Object? callId = null,Object? senderPath = null,}) {
  return _then(_self.copyWith(
callId: null == callId ? _self.callId : callId // ignore: cast_nullable_to_non_nullable
as String,senderPath: null == senderPath ? _self.senderPath : senderPath // ignore: cast_nullable_to_non_nullable
as String,
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeAgentTimelinePayloadDto_SpawnBegin value)?  spawnBegin,TResult Function( BridgeAgentTimelinePayloadDto_SpawnEnd value)?  spawnEnd,TResult Function( BridgeAgentTimelinePayloadDto_InteractionBegin value)?  interactionBegin,TResult Function( BridgeAgentTimelinePayloadDto_InteractionEnd value)?  interactionEnd,TResult Function( BridgeAgentTimelinePayloadDto_WaitingBegin value)?  waitingBegin,TResult Function( BridgeAgentTimelinePayloadDto_WaitingEnd value)?  waitingEnd,TResult Function( BridgeAgentTimelinePayloadDto_CloseBegin value)?  closeBegin,TResult Function( BridgeAgentTimelinePayloadDto_CloseEnd value)?  closeEnd,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeAgentTimelinePayloadDto_SpawnBegin() when spawnBegin != null:
return spawnBegin(_that);case BridgeAgentTimelinePayloadDto_SpawnEnd() when spawnEnd != null:
return spawnEnd(_that);case BridgeAgentTimelinePayloadDto_InteractionBegin() when interactionBegin != null:
return interactionBegin(_that);case BridgeAgentTimelinePayloadDto_InteractionEnd() when interactionEnd != null:
return interactionEnd(_that);case BridgeAgentTimelinePayloadDto_WaitingBegin() when waitingBegin != null:
return waitingBegin(_that);case BridgeAgentTimelinePayloadDto_WaitingEnd() when waitingEnd != null:
return waitingEnd(_that);case BridgeAgentTimelinePayloadDto_CloseBegin() when closeBegin != null:
return closeBegin(_that);case BridgeAgentTimelinePayloadDto_CloseEnd() when closeEnd != null:
return closeEnd(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeAgentTimelinePayloadDto_SpawnBegin value)  spawnBegin,required TResult Function( BridgeAgentTimelinePayloadDto_SpawnEnd value)  spawnEnd,required TResult Function( BridgeAgentTimelinePayloadDto_InteractionBegin value)  interactionBegin,required TResult Function( BridgeAgentTimelinePayloadDto_InteractionEnd value)  interactionEnd,required TResult Function( BridgeAgentTimelinePayloadDto_WaitingBegin value)  waitingBegin,required TResult Function( BridgeAgentTimelinePayloadDto_WaitingEnd value)  waitingEnd,required TResult Function( BridgeAgentTimelinePayloadDto_CloseBegin value)  closeBegin,required TResult Function( BridgeAgentTimelinePayloadDto_CloseEnd value)  closeEnd,}){
final _that = this;
switch (_that) {
case BridgeAgentTimelinePayloadDto_SpawnBegin():
return spawnBegin(_that);case BridgeAgentTimelinePayloadDto_SpawnEnd():
return spawnEnd(_that);case BridgeAgentTimelinePayloadDto_InteractionBegin():
return interactionBegin(_that);case BridgeAgentTimelinePayloadDto_InteractionEnd():
return interactionEnd(_that);case BridgeAgentTimelinePayloadDto_WaitingBegin():
return waitingBegin(_that);case BridgeAgentTimelinePayloadDto_WaitingEnd():
return waitingEnd(_that);case BridgeAgentTimelinePayloadDto_CloseBegin():
return closeBegin(_that);case BridgeAgentTimelinePayloadDto_CloseEnd():
return closeEnd(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeAgentTimelinePayloadDto_SpawnBegin value)?  spawnBegin,TResult? Function( BridgeAgentTimelinePayloadDto_SpawnEnd value)?  spawnEnd,TResult? Function( BridgeAgentTimelinePayloadDto_InteractionBegin value)?  interactionBegin,TResult? Function( BridgeAgentTimelinePayloadDto_InteractionEnd value)?  interactionEnd,TResult? Function( BridgeAgentTimelinePayloadDto_WaitingBegin value)?  waitingBegin,TResult? Function( BridgeAgentTimelinePayloadDto_WaitingEnd value)?  waitingEnd,TResult? Function( BridgeAgentTimelinePayloadDto_CloseBegin value)?  closeBegin,TResult? Function( BridgeAgentTimelinePayloadDto_CloseEnd value)?  closeEnd,}){
final _that = this;
switch (_that) {
case BridgeAgentTimelinePayloadDto_SpawnBegin() when spawnBegin != null:
return spawnBegin(_that);case BridgeAgentTimelinePayloadDto_SpawnEnd() when spawnEnd != null:
return spawnEnd(_that);case BridgeAgentTimelinePayloadDto_InteractionBegin() when interactionBegin != null:
return interactionBegin(_that);case BridgeAgentTimelinePayloadDto_InteractionEnd() when interactionEnd != null:
return interactionEnd(_that);case BridgeAgentTimelinePayloadDto_WaitingBegin() when waitingBegin != null:
return waitingBegin(_that);case BridgeAgentTimelinePayloadDto_WaitingEnd() when waitingEnd != null:
return waitingEnd(_that);case BridgeAgentTimelinePayloadDto_CloseBegin() when closeBegin != null:
return closeBegin(_that);case BridgeAgentTimelinePayloadDto_CloseEnd() when closeEnd != null:
return closeEnd(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String callId,  String senderPath,  String taskName,  String prompt,  String role,  String? model,  String? reasoningEffort)?  spawnBegin,TResult Function( String callId,  String senderPath,  String? agentId,  String? path,  String? role,  String status,  String prompt,  String? error)?  spawnEnd,TResult Function( String callId,  String senderPath,  String receiverPath,  String prompt)?  interactionBegin,TResult Function( String callId,  String senderPath,  String receiverPath,  String status,  String prompt,  String? error)?  interactionEnd,TResult Function( String callId,  String senderPath)?  waitingBegin,TResult Function( String callId,  String senderPath,  bool timedOut)?  waitingEnd,TResult Function( String callId,  String senderPath,  String receiverPath)?  closeBegin,TResult Function( String callId,  String senderPath,  String receiverPath,  String status,  String? error)?  closeEnd,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeAgentTimelinePayloadDto_SpawnBegin() when spawnBegin != null:
return spawnBegin(_that.callId,_that.senderPath,_that.taskName,_that.prompt,_that.role,_that.model,_that.reasoningEffort);case BridgeAgentTimelinePayloadDto_SpawnEnd() when spawnEnd != null:
return spawnEnd(_that.callId,_that.senderPath,_that.agentId,_that.path,_that.role,_that.status,_that.prompt,_that.error);case BridgeAgentTimelinePayloadDto_InteractionBegin() when interactionBegin != null:
return interactionBegin(_that.callId,_that.senderPath,_that.receiverPath,_that.prompt);case BridgeAgentTimelinePayloadDto_InteractionEnd() when interactionEnd != null:
return interactionEnd(_that.callId,_that.senderPath,_that.receiverPath,_that.status,_that.prompt,_that.error);case BridgeAgentTimelinePayloadDto_WaitingBegin() when waitingBegin != null:
return waitingBegin(_that.callId,_that.senderPath);case BridgeAgentTimelinePayloadDto_WaitingEnd() when waitingEnd != null:
return waitingEnd(_that.callId,_that.senderPath,_that.timedOut);case BridgeAgentTimelinePayloadDto_CloseBegin() when closeBegin != null:
return closeBegin(_that.callId,_that.senderPath,_that.receiverPath);case BridgeAgentTimelinePayloadDto_CloseEnd() when closeEnd != null:
return closeEnd(_that.callId,_that.senderPath,_that.receiverPath,_that.status,_that.error);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String callId,  String senderPath,  String taskName,  String prompt,  String role,  String? model,  String? reasoningEffort)  spawnBegin,required TResult Function( String callId,  String senderPath,  String? agentId,  String? path,  String? role,  String status,  String prompt,  String? error)  spawnEnd,required TResult Function( String callId,  String senderPath,  String receiverPath,  String prompt)  interactionBegin,required TResult Function( String callId,  String senderPath,  String receiverPath,  String status,  String prompt,  String? error)  interactionEnd,required TResult Function( String callId,  String senderPath)  waitingBegin,required TResult Function( String callId,  String senderPath,  bool timedOut)  waitingEnd,required TResult Function( String callId,  String senderPath,  String receiverPath)  closeBegin,required TResult Function( String callId,  String senderPath,  String receiverPath,  String status,  String? error)  closeEnd,}) {final _that = this;
switch (_that) {
case BridgeAgentTimelinePayloadDto_SpawnBegin():
return spawnBegin(_that.callId,_that.senderPath,_that.taskName,_that.prompt,_that.role,_that.model,_that.reasoningEffort);case BridgeAgentTimelinePayloadDto_SpawnEnd():
return spawnEnd(_that.callId,_that.senderPath,_that.agentId,_that.path,_that.role,_that.status,_that.prompt,_that.error);case BridgeAgentTimelinePayloadDto_InteractionBegin():
return interactionBegin(_that.callId,_that.senderPath,_that.receiverPath,_that.prompt);case BridgeAgentTimelinePayloadDto_InteractionEnd():
return interactionEnd(_that.callId,_that.senderPath,_that.receiverPath,_that.status,_that.prompt,_that.error);case BridgeAgentTimelinePayloadDto_WaitingBegin():
return waitingBegin(_that.callId,_that.senderPath);case BridgeAgentTimelinePayloadDto_WaitingEnd():
return waitingEnd(_that.callId,_that.senderPath,_that.timedOut);case BridgeAgentTimelinePayloadDto_CloseBegin():
return closeBegin(_that.callId,_that.senderPath,_that.receiverPath);case BridgeAgentTimelinePayloadDto_CloseEnd():
return closeEnd(_that.callId,_that.senderPath,_that.receiverPath,_that.status,_that.error);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String callId,  String senderPath,  String taskName,  String prompt,  String role,  String? model,  String? reasoningEffort)?  spawnBegin,TResult? Function( String callId,  String senderPath,  String? agentId,  String? path,  String? role,  String status,  String prompt,  String? error)?  spawnEnd,TResult? Function( String callId,  String senderPath,  String receiverPath,  String prompt)?  interactionBegin,TResult? Function( String callId,  String senderPath,  String receiverPath,  String status,  String prompt,  String? error)?  interactionEnd,TResult? Function( String callId,  String senderPath)?  waitingBegin,TResult? Function( String callId,  String senderPath,  bool timedOut)?  waitingEnd,TResult? Function( String callId,  String senderPath,  String receiverPath)?  closeBegin,TResult? Function( String callId,  String senderPath,  String receiverPath,  String status,  String? error)?  closeEnd,}) {final _that = this;
switch (_that) {
case BridgeAgentTimelinePayloadDto_SpawnBegin() when spawnBegin != null:
return spawnBegin(_that.callId,_that.senderPath,_that.taskName,_that.prompt,_that.role,_that.model,_that.reasoningEffort);case BridgeAgentTimelinePayloadDto_SpawnEnd() when spawnEnd != null:
return spawnEnd(_that.callId,_that.senderPath,_that.agentId,_that.path,_that.role,_that.status,_that.prompt,_that.error);case BridgeAgentTimelinePayloadDto_InteractionBegin() when interactionBegin != null:
return interactionBegin(_that.callId,_that.senderPath,_that.receiverPath,_that.prompt);case BridgeAgentTimelinePayloadDto_InteractionEnd() when interactionEnd != null:
return interactionEnd(_that.callId,_that.senderPath,_that.receiverPath,_that.status,_that.prompt,_that.error);case BridgeAgentTimelinePayloadDto_WaitingBegin() when waitingBegin != null:
return waitingBegin(_that.callId,_that.senderPath);case BridgeAgentTimelinePayloadDto_WaitingEnd() when waitingEnd != null:
return waitingEnd(_that.callId,_that.senderPath,_that.timedOut);case BridgeAgentTimelinePayloadDto_CloseBegin() when closeBegin != null:
return closeBegin(_that.callId,_that.senderPath,_that.receiverPath);case BridgeAgentTimelinePayloadDto_CloseEnd() when closeEnd != null:
return closeEnd(_that.callId,_that.senderPath,_that.receiverPath,_that.status,_that.error);case _:
  return null;

}
}

}

/// @nodoc


class BridgeAgentTimelinePayloadDto_SpawnBegin extends BridgeAgentTimelinePayloadDto {
  const BridgeAgentTimelinePayloadDto_SpawnBegin({required this.callId, required this.senderPath, required this.taskName, required this.prompt, required this.role, this.model, this.reasoningEffort}): super._();
  

@override final  String callId;
@override final  String senderPath;
 final  String taskName;
 final  String prompt;
 final  String role;
 final  String? model;
 final  String? reasoningEffort;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentTimelinePayloadDto_SpawnBeginCopyWith<BridgeAgentTimelinePayloadDto_SpawnBegin> get copyWith => _$BridgeAgentTimelinePayloadDto_SpawnBeginCopyWithImpl<BridgeAgentTimelinePayloadDto_SpawnBegin>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentTimelinePayloadDto_SpawnBegin&&(identical(other.callId, callId) || other.callId == callId)&&(identical(other.senderPath, senderPath) || other.senderPath == senderPath)&&(identical(other.taskName, taskName) || other.taskName == taskName)&&(identical(other.prompt, prompt) || other.prompt == prompt)&&(identical(other.role, role) || other.role == role)&&(identical(other.model, model) || other.model == model)&&(identical(other.reasoningEffort, reasoningEffort) || other.reasoningEffort == reasoningEffort));
}


@override
int get hashCode => Object.hash(runtimeType,callId,senderPath,taskName,prompt,role,model,reasoningEffort);

@override
String toString() {
  return 'BridgeAgentTimelinePayloadDto.spawnBegin(callId: $callId, senderPath: $senderPath, taskName: $taskName, prompt: $prompt, role: $role, model: $model, reasoningEffort: $reasoningEffort)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentTimelinePayloadDto_SpawnBeginCopyWith<$Res> implements $BridgeAgentTimelinePayloadDtoCopyWith<$Res> {
  factory $BridgeAgentTimelinePayloadDto_SpawnBeginCopyWith(BridgeAgentTimelinePayloadDto_SpawnBegin value, $Res Function(BridgeAgentTimelinePayloadDto_SpawnBegin) _then) = _$BridgeAgentTimelinePayloadDto_SpawnBeginCopyWithImpl;
@override @useResult
$Res call({
 String callId, String senderPath, String taskName, String prompt, String role, String? model, String? reasoningEffort
});




}
/// @nodoc
class _$BridgeAgentTimelinePayloadDto_SpawnBeginCopyWithImpl<$Res>
    implements $BridgeAgentTimelinePayloadDto_SpawnBeginCopyWith<$Res> {
  _$BridgeAgentTimelinePayloadDto_SpawnBeginCopyWithImpl(this._self, this._then);

  final BridgeAgentTimelinePayloadDto_SpawnBegin _self;
  final $Res Function(BridgeAgentTimelinePayloadDto_SpawnBegin) _then;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? callId = null,Object? senderPath = null,Object? taskName = null,Object? prompt = null,Object? role = null,Object? model = freezed,Object? reasoningEffort = freezed,}) {
  return _then(BridgeAgentTimelinePayloadDto_SpawnBegin(
callId: null == callId ? _self.callId : callId // ignore: cast_nullable_to_non_nullable
as String,senderPath: null == senderPath ? _self.senderPath : senderPath // ignore: cast_nullable_to_non_nullable
as String,taskName: null == taskName ? _self.taskName : taskName // ignore: cast_nullable_to_non_nullable
as String,prompt: null == prompt ? _self.prompt : prompt // ignore: cast_nullable_to_non_nullable
as String,role: null == role ? _self.role : role // ignore: cast_nullable_to_non_nullable
as String,model: freezed == model ? _self.model : model // ignore: cast_nullable_to_non_nullable
as String?,reasoningEffort: freezed == reasoningEffort ? _self.reasoningEffort : reasoningEffort // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc


class BridgeAgentTimelinePayloadDto_SpawnEnd extends BridgeAgentTimelinePayloadDto {
  const BridgeAgentTimelinePayloadDto_SpawnEnd({required this.callId, required this.senderPath, this.agentId, this.path, this.role, required this.status, required this.prompt, this.error}): super._();
  

@override final  String callId;
@override final  String senderPath;
 final  String? agentId;
 final  String? path;
 final  String? role;
 final  String status;
 final  String prompt;
 final  String? error;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentTimelinePayloadDto_SpawnEndCopyWith<BridgeAgentTimelinePayloadDto_SpawnEnd> get copyWith => _$BridgeAgentTimelinePayloadDto_SpawnEndCopyWithImpl<BridgeAgentTimelinePayloadDto_SpawnEnd>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentTimelinePayloadDto_SpawnEnd&&(identical(other.callId, callId) || other.callId == callId)&&(identical(other.senderPath, senderPath) || other.senderPath == senderPath)&&(identical(other.agentId, agentId) || other.agentId == agentId)&&(identical(other.path, path) || other.path == path)&&(identical(other.role, role) || other.role == role)&&(identical(other.status, status) || other.status == status)&&(identical(other.prompt, prompt) || other.prompt == prompt)&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,callId,senderPath,agentId,path,role,status,prompt,error);

@override
String toString() {
  return 'BridgeAgentTimelinePayloadDto.spawnEnd(callId: $callId, senderPath: $senderPath, agentId: $agentId, path: $path, role: $role, status: $status, prompt: $prompt, error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentTimelinePayloadDto_SpawnEndCopyWith<$Res> implements $BridgeAgentTimelinePayloadDtoCopyWith<$Res> {
  factory $BridgeAgentTimelinePayloadDto_SpawnEndCopyWith(BridgeAgentTimelinePayloadDto_SpawnEnd value, $Res Function(BridgeAgentTimelinePayloadDto_SpawnEnd) _then) = _$BridgeAgentTimelinePayloadDto_SpawnEndCopyWithImpl;
@override @useResult
$Res call({
 String callId, String senderPath, String? agentId, String? path, String? role, String status, String prompt, String? error
});




}
/// @nodoc
class _$BridgeAgentTimelinePayloadDto_SpawnEndCopyWithImpl<$Res>
    implements $BridgeAgentTimelinePayloadDto_SpawnEndCopyWith<$Res> {
  _$BridgeAgentTimelinePayloadDto_SpawnEndCopyWithImpl(this._self, this._then);

  final BridgeAgentTimelinePayloadDto_SpawnEnd _self;
  final $Res Function(BridgeAgentTimelinePayloadDto_SpawnEnd) _then;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? callId = null,Object? senderPath = null,Object? agentId = freezed,Object? path = freezed,Object? role = freezed,Object? status = null,Object? prompt = null,Object? error = freezed,}) {
  return _then(BridgeAgentTimelinePayloadDto_SpawnEnd(
callId: null == callId ? _self.callId : callId // ignore: cast_nullable_to_non_nullable
as String,senderPath: null == senderPath ? _self.senderPath : senderPath // ignore: cast_nullable_to_non_nullable
as String,agentId: freezed == agentId ? _self.agentId : agentId // ignore: cast_nullable_to_non_nullable
as String?,path: freezed == path ? _self.path : path // ignore: cast_nullable_to_non_nullable
as String?,role: freezed == role ? _self.role : role // ignore: cast_nullable_to_non_nullable
as String?,status: null == status ? _self.status : status // ignore: cast_nullable_to_non_nullable
as String,prompt: null == prompt ? _self.prompt : prompt // ignore: cast_nullable_to_non_nullable
as String,error: freezed == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc


class BridgeAgentTimelinePayloadDto_InteractionBegin extends BridgeAgentTimelinePayloadDto {
  const BridgeAgentTimelinePayloadDto_InteractionBegin({required this.callId, required this.senderPath, required this.receiverPath, required this.prompt}): super._();
  

@override final  String callId;
@override final  String senderPath;
 final  String receiverPath;
 final  String prompt;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentTimelinePayloadDto_InteractionBeginCopyWith<BridgeAgentTimelinePayloadDto_InteractionBegin> get copyWith => _$BridgeAgentTimelinePayloadDto_InteractionBeginCopyWithImpl<BridgeAgentTimelinePayloadDto_InteractionBegin>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentTimelinePayloadDto_InteractionBegin&&(identical(other.callId, callId) || other.callId == callId)&&(identical(other.senderPath, senderPath) || other.senderPath == senderPath)&&(identical(other.receiverPath, receiverPath) || other.receiverPath == receiverPath)&&(identical(other.prompt, prompt) || other.prompt == prompt));
}


@override
int get hashCode => Object.hash(runtimeType,callId,senderPath,receiverPath,prompt);

@override
String toString() {
  return 'BridgeAgentTimelinePayloadDto.interactionBegin(callId: $callId, senderPath: $senderPath, receiverPath: $receiverPath, prompt: $prompt)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentTimelinePayloadDto_InteractionBeginCopyWith<$Res> implements $BridgeAgentTimelinePayloadDtoCopyWith<$Res> {
  factory $BridgeAgentTimelinePayloadDto_InteractionBeginCopyWith(BridgeAgentTimelinePayloadDto_InteractionBegin value, $Res Function(BridgeAgentTimelinePayloadDto_InteractionBegin) _then) = _$BridgeAgentTimelinePayloadDto_InteractionBeginCopyWithImpl;
@override @useResult
$Res call({
 String callId, String senderPath, String receiverPath, String prompt
});




}
/// @nodoc
class _$BridgeAgentTimelinePayloadDto_InteractionBeginCopyWithImpl<$Res>
    implements $BridgeAgentTimelinePayloadDto_InteractionBeginCopyWith<$Res> {
  _$BridgeAgentTimelinePayloadDto_InteractionBeginCopyWithImpl(this._self, this._then);

  final BridgeAgentTimelinePayloadDto_InteractionBegin _self;
  final $Res Function(BridgeAgentTimelinePayloadDto_InteractionBegin) _then;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? callId = null,Object? senderPath = null,Object? receiverPath = null,Object? prompt = null,}) {
  return _then(BridgeAgentTimelinePayloadDto_InteractionBegin(
callId: null == callId ? _self.callId : callId // ignore: cast_nullable_to_non_nullable
as String,senderPath: null == senderPath ? _self.senderPath : senderPath // ignore: cast_nullable_to_non_nullable
as String,receiverPath: null == receiverPath ? _self.receiverPath : receiverPath // ignore: cast_nullable_to_non_nullable
as String,prompt: null == prompt ? _self.prompt : prompt // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeAgentTimelinePayloadDto_InteractionEnd extends BridgeAgentTimelinePayloadDto {
  const BridgeAgentTimelinePayloadDto_InteractionEnd({required this.callId, required this.senderPath, required this.receiverPath, required this.status, required this.prompt, this.error}): super._();
  

@override final  String callId;
@override final  String senderPath;
 final  String receiverPath;
 final  String status;
 final  String prompt;
 final  String? error;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentTimelinePayloadDto_InteractionEndCopyWith<BridgeAgentTimelinePayloadDto_InteractionEnd> get copyWith => _$BridgeAgentTimelinePayloadDto_InteractionEndCopyWithImpl<BridgeAgentTimelinePayloadDto_InteractionEnd>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentTimelinePayloadDto_InteractionEnd&&(identical(other.callId, callId) || other.callId == callId)&&(identical(other.senderPath, senderPath) || other.senderPath == senderPath)&&(identical(other.receiverPath, receiverPath) || other.receiverPath == receiverPath)&&(identical(other.status, status) || other.status == status)&&(identical(other.prompt, prompt) || other.prompt == prompt)&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,callId,senderPath,receiverPath,status,prompt,error);

@override
String toString() {
  return 'BridgeAgentTimelinePayloadDto.interactionEnd(callId: $callId, senderPath: $senderPath, receiverPath: $receiverPath, status: $status, prompt: $prompt, error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentTimelinePayloadDto_InteractionEndCopyWith<$Res> implements $BridgeAgentTimelinePayloadDtoCopyWith<$Res> {
  factory $BridgeAgentTimelinePayloadDto_InteractionEndCopyWith(BridgeAgentTimelinePayloadDto_InteractionEnd value, $Res Function(BridgeAgentTimelinePayloadDto_InteractionEnd) _then) = _$BridgeAgentTimelinePayloadDto_InteractionEndCopyWithImpl;
@override @useResult
$Res call({
 String callId, String senderPath, String receiverPath, String status, String prompt, String? error
});




}
/// @nodoc
class _$BridgeAgentTimelinePayloadDto_InteractionEndCopyWithImpl<$Res>
    implements $BridgeAgentTimelinePayloadDto_InteractionEndCopyWith<$Res> {
  _$BridgeAgentTimelinePayloadDto_InteractionEndCopyWithImpl(this._self, this._then);

  final BridgeAgentTimelinePayloadDto_InteractionEnd _self;
  final $Res Function(BridgeAgentTimelinePayloadDto_InteractionEnd) _then;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? callId = null,Object? senderPath = null,Object? receiverPath = null,Object? status = null,Object? prompt = null,Object? error = freezed,}) {
  return _then(BridgeAgentTimelinePayloadDto_InteractionEnd(
callId: null == callId ? _self.callId : callId // ignore: cast_nullable_to_non_nullable
as String,senderPath: null == senderPath ? _self.senderPath : senderPath // ignore: cast_nullable_to_non_nullable
as String,receiverPath: null == receiverPath ? _self.receiverPath : receiverPath // ignore: cast_nullable_to_non_nullable
as String,status: null == status ? _self.status : status // ignore: cast_nullable_to_non_nullable
as String,prompt: null == prompt ? _self.prompt : prompt // ignore: cast_nullable_to_non_nullable
as String,error: freezed == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc


class BridgeAgentTimelinePayloadDto_WaitingBegin extends BridgeAgentTimelinePayloadDto {
  const BridgeAgentTimelinePayloadDto_WaitingBegin({required this.callId, required this.senderPath}): super._();
  

@override final  String callId;
@override final  String senderPath;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentTimelinePayloadDto_WaitingBeginCopyWith<BridgeAgentTimelinePayloadDto_WaitingBegin> get copyWith => _$BridgeAgentTimelinePayloadDto_WaitingBeginCopyWithImpl<BridgeAgentTimelinePayloadDto_WaitingBegin>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentTimelinePayloadDto_WaitingBegin&&(identical(other.callId, callId) || other.callId == callId)&&(identical(other.senderPath, senderPath) || other.senderPath == senderPath));
}


@override
int get hashCode => Object.hash(runtimeType,callId,senderPath);

@override
String toString() {
  return 'BridgeAgentTimelinePayloadDto.waitingBegin(callId: $callId, senderPath: $senderPath)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentTimelinePayloadDto_WaitingBeginCopyWith<$Res> implements $BridgeAgentTimelinePayloadDtoCopyWith<$Res> {
  factory $BridgeAgentTimelinePayloadDto_WaitingBeginCopyWith(BridgeAgentTimelinePayloadDto_WaitingBegin value, $Res Function(BridgeAgentTimelinePayloadDto_WaitingBegin) _then) = _$BridgeAgentTimelinePayloadDto_WaitingBeginCopyWithImpl;
@override @useResult
$Res call({
 String callId, String senderPath
});




}
/// @nodoc
class _$BridgeAgentTimelinePayloadDto_WaitingBeginCopyWithImpl<$Res>
    implements $BridgeAgentTimelinePayloadDto_WaitingBeginCopyWith<$Res> {
  _$BridgeAgentTimelinePayloadDto_WaitingBeginCopyWithImpl(this._self, this._then);

  final BridgeAgentTimelinePayloadDto_WaitingBegin _self;
  final $Res Function(BridgeAgentTimelinePayloadDto_WaitingBegin) _then;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? callId = null,Object? senderPath = null,}) {
  return _then(BridgeAgentTimelinePayloadDto_WaitingBegin(
callId: null == callId ? _self.callId : callId // ignore: cast_nullable_to_non_nullable
as String,senderPath: null == senderPath ? _self.senderPath : senderPath // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeAgentTimelinePayloadDto_WaitingEnd extends BridgeAgentTimelinePayloadDto {
  const BridgeAgentTimelinePayloadDto_WaitingEnd({required this.callId, required this.senderPath, required this.timedOut}): super._();
  

@override final  String callId;
@override final  String senderPath;
 final  bool timedOut;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentTimelinePayloadDto_WaitingEndCopyWith<BridgeAgentTimelinePayloadDto_WaitingEnd> get copyWith => _$BridgeAgentTimelinePayloadDto_WaitingEndCopyWithImpl<BridgeAgentTimelinePayloadDto_WaitingEnd>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentTimelinePayloadDto_WaitingEnd&&(identical(other.callId, callId) || other.callId == callId)&&(identical(other.senderPath, senderPath) || other.senderPath == senderPath)&&(identical(other.timedOut, timedOut) || other.timedOut == timedOut));
}


@override
int get hashCode => Object.hash(runtimeType,callId,senderPath,timedOut);

@override
String toString() {
  return 'BridgeAgentTimelinePayloadDto.waitingEnd(callId: $callId, senderPath: $senderPath, timedOut: $timedOut)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentTimelinePayloadDto_WaitingEndCopyWith<$Res> implements $BridgeAgentTimelinePayloadDtoCopyWith<$Res> {
  factory $BridgeAgentTimelinePayloadDto_WaitingEndCopyWith(BridgeAgentTimelinePayloadDto_WaitingEnd value, $Res Function(BridgeAgentTimelinePayloadDto_WaitingEnd) _then) = _$BridgeAgentTimelinePayloadDto_WaitingEndCopyWithImpl;
@override @useResult
$Res call({
 String callId, String senderPath, bool timedOut
});




}
/// @nodoc
class _$BridgeAgentTimelinePayloadDto_WaitingEndCopyWithImpl<$Res>
    implements $BridgeAgentTimelinePayloadDto_WaitingEndCopyWith<$Res> {
  _$BridgeAgentTimelinePayloadDto_WaitingEndCopyWithImpl(this._self, this._then);

  final BridgeAgentTimelinePayloadDto_WaitingEnd _self;
  final $Res Function(BridgeAgentTimelinePayloadDto_WaitingEnd) _then;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? callId = null,Object? senderPath = null,Object? timedOut = null,}) {
  return _then(BridgeAgentTimelinePayloadDto_WaitingEnd(
callId: null == callId ? _self.callId : callId // ignore: cast_nullable_to_non_nullable
as String,senderPath: null == senderPath ? _self.senderPath : senderPath // ignore: cast_nullable_to_non_nullable
as String,timedOut: null == timedOut ? _self.timedOut : timedOut // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc


class BridgeAgentTimelinePayloadDto_CloseBegin extends BridgeAgentTimelinePayloadDto {
  const BridgeAgentTimelinePayloadDto_CloseBegin({required this.callId, required this.senderPath, required this.receiverPath}): super._();
  

@override final  String callId;
@override final  String senderPath;
 final  String receiverPath;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentTimelinePayloadDto_CloseBeginCopyWith<BridgeAgentTimelinePayloadDto_CloseBegin> get copyWith => _$BridgeAgentTimelinePayloadDto_CloseBeginCopyWithImpl<BridgeAgentTimelinePayloadDto_CloseBegin>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentTimelinePayloadDto_CloseBegin&&(identical(other.callId, callId) || other.callId == callId)&&(identical(other.senderPath, senderPath) || other.senderPath == senderPath)&&(identical(other.receiverPath, receiverPath) || other.receiverPath == receiverPath));
}


@override
int get hashCode => Object.hash(runtimeType,callId,senderPath,receiverPath);

@override
String toString() {
  return 'BridgeAgentTimelinePayloadDto.closeBegin(callId: $callId, senderPath: $senderPath, receiverPath: $receiverPath)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentTimelinePayloadDto_CloseBeginCopyWith<$Res> implements $BridgeAgentTimelinePayloadDtoCopyWith<$Res> {
  factory $BridgeAgentTimelinePayloadDto_CloseBeginCopyWith(BridgeAgentTimelinePayloadDto_CloseBegin value, $Res Function(BridgeAgentTimelinePayloadDto_CloseBegin) _then) = _$BridgeAgentTimelinePayloadDto_CloseBeginCopyWithImpl;
@override @useResult
$Res call({
 String callId, String senderPath, String receiverPath
});




}
/// @nodoc
class _$BridgeAgentTimelinePayloadDto_CloseBeginCopyWithImpl<$Res>
    implements $BridgeAgentTimelinePayloadDto_CloseBeginCopyWith<$Res> {
  _$BridgeAgentTimelinePayloadDto_CloseBeginCopyWithImpl(this._self, this._then);

  final BridgeAgentTimelinePayloadDto_CloseBegin _self;
  final $Res Function(BridgeAgentTimelinePayloadDto_CloseBegin) _then;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? callId = null,Object? senderPath = null,Object? receiverPath = null,}) {
  return _then(BridgeAgentTimelinePayloadDto_CloseBegin(
callId: null == callId ? _self.callId : callId // ignore: cast_nullable_to_non_nullable
as String,senderPath: null == senderPath ? _self.senderPath : senderPath // ignore: cast_nullable_to_non_nullable
as String,receiverPath: null == receiverPath ? _self.receiverPath : receiverPath // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeAgentTimelinePayloadDto_CloseEnd extends BridgeAgentTimelinePayloadDto {
  const BridgeAgentTimelinePayloadDto_CloseEnd({required this.callId, required this.senderPath, required this.receiverPath, required this.status, this.error}): super._();
  

@override final  String callId;
@override final  String senderPath;
 final  String receiverPath;
 final  String status;
 final  String? error;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentTimelinePayloadDto_CloseEndCopyWith<BridgeAgentTimelinePayloadDto_CloseEnd> get copyWith => _$BridgeAgentTimelinePayloadDto_CloseEndCopyWithImpl<BridgeAgentTimelinePayloadDto_CloseEnd>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentTimelinePayloadDto_CloseEnd&&(identical(other.callId, callId) || other.callId == callId)&&(identical(other.senderPath, senderPath) || other.senderPath == senderPath)&&(identical(other.receiverPath, receiverPath) || other.receiverPath == receiverPath)&&(identical(other.status, status) || other.status == status)&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,callId,senderPath,receiverPath,status,error);

@override
String toString() {
  return 'BridgeAgentTimelinePayloadDto.closeEnd(callId: $callId, senderPath: $senderPath, receiverPath: $receiverPath, status: $status, error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentTimelinePayloadDto_CloseEndCopyWith<$Res> implements $BridgeAgentTimelinePayloadDtoCopyWith<$Res> {
  factory $BridgeAgentTimelinePayloadDto_CloseEndCopyWith(BridgeAgentTimelinePayloadDto_CloseEnd value, $Res Function(BridgeAgentTimelinePayloadDto_CloseEnd) _then) = _$BridgeAgentTimelinePayloadDto_CloseEndCopyWithImpl;
@override @useResult
$Res call({
 String callId, String senderPath, String receiverPath, String status, String? error
});




}
/// @nodoc
class _$BridgeAgentTimelinePayloadDto_CloseEndCopyWithImpl<$Res>
    implements $BridgeAgentTimelinePayloadDto_CloseEndCopyWith<$Res> {
  _$BridgeAgentTimelinePayloadDto_CloseEndCopyWithImpl(this._self, this._then);

  final BridgeAgentTimelinePayloadDto_CloseEnd _self;
  final $Res Function(BridgeAgentTimelinePayloadDto_CloseEnd) _then;

/// Create a copy of BridgeAgentTimelinePayloadDto
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? callId = null,Object? senderPath = null,Object? receiverPath = null,Object? status = null,Object? error = freezed,}) {
  return _then(BridgeAgentTimelinePayloadDto_CloseEnd(
callId: null == callId ? _self.callId : callId // ignore: cast_nullable_to_non_nullable
as String,senderPath: null == senderPath ? _self.senderPath : senderPath // ignore: cast_nullable_to_non_nullable
as String,receiverPath: null == receiverPath ? _self.receiverPath : receiverPath // ignore: cast_nullable_to_non_nullable
as String,status: null == status ? _self.status : status // ignore: cast_nullable_to_non_nullable
as String,error: freezed == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

// dart format on

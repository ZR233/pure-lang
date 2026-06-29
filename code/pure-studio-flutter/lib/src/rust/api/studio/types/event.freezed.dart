// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'event.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeEventPayload {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeEventPayload()';
}


}

/// @nodoc
class $BridgeEventPayloadCopyWith<$Res>  {
$BridgeEventPayloadCopyWith(BridgeEventPayload _, $Res Function(BridgeEventPayload) __);
}


/// Adds pattern-matching-related methods to [BridgeEventPayload].
extension BridgeEventPayloadPatterns on BridgeEventPayload {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeEventPayload_TurnChanged value)?  turnChanged,TResult Function( BridgeEventPayload_MessageUpdated value)?  messageUpdated,TResult Function( BridgeEventPayload_MessageRemoved value)?  messageRemoved,TResult Function( BridgeEventPayload_MessagePartUpdated value)?  messagePartUpdated,TResult Function( BridgeEventPayload_MessagePartRemoved value)?  messagePartRemoved,TResult Function( BridgeEventPayload_MessagePartDelta value)?  messagePartDelta,TResult Function( BridgeEventPayload_InteractionChanged value)?  interactionChanged,TResult Function( BridgeEventPayload_AgentChanged value)?  agentChanged,TResult Function( BridgeEventPayload_AgentTimelineChanged value)?  agentTimelineChanged,TResult Function( BridgeEventPayload_SessionRuntimeChanged value)?  sessionRuntimeChanged,TResult Function( BridgeEventPayload_SkillActivated value)?  skillActivated,TResult Function( BridgeEventPayload_PlanLifecycleChanged value)?  planLifecycleChanged,TResult Function( BridgeEventPayload_SessionListChanged value)?  sessionListChanged,TResult Function( BridgeEventPayload_McpHealthChanged value)?  mcpHealthChanged,TResult Function( BridgeEventPayload_LspHealthChanged value)?  lspHealthChanged,TResult Function( BridgeEventPayload_Stale value)?  stale,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeEventPayload_TurnChanged() when turnChanged != null:
return turnChanged(_that);case BridgeEventPayload_MessageUpdated() when messageUpdated != null:
return messageUpdated(_that);case BridgeEventPayload_MessageRemoved() when messageRemoved != null:
return messageRemoved(_that);case BridgeEventPayload_MessagePartUpdated() when messagePartUpdated != null:
return messagePartUpdated(_that);case BridgeEventPayload_MessagePartRemoved() when messagePartRemoved != null:
return messagePartRemoved(_that);case BridgeEventPayload_MessagePartDelta() when messagePartDelta != null:
return messagePartDelta(_that);case BridgeEventPayload_InteractionChanged() when interactionChanged != null:
return interactionChanged(_that);case BridgeEventPayload_AgentChanged() when agentChanged != null:
return agentChanged(_that);case BridgeEventPayload_AgentTimelineChanged() when agentTimelineChanged != null:
return agentTimelineChanged(_that);case BridgeEventPayload_SessionRuntimeChanged() when sessionRuntimeChanged != null:
return sessionRuntimeChanged(_that);case BridgeEventPayload_SkillActivated() when skillActivated != null:
return skillActivated(_that);case BridgeEventPayload_PlanLifecycleChanged() when planLifecycleChanged != null:
return planLifecycleChanged(_that);case BridgeEventPayload_SessionListChanged() when sessionListChanged != null:
return sessionListChanged(_that);case BridgeEventPayload_McpHealthChanged() when mcpHealthChanged != null:
return mcpHealthChanged(_that);case BridgeEventPayload_LspHealthChanged() when lspHealthChanged != null:
return lspHealthChanged(_that);case BridgeEventPayload_Stale() when stale != null:
return stale(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeEventPayload_TurnChanged value)  turnChanged,required TResult Function( BridgeEventPayload_MessageUpdated value)  messageUpdated,required TResult Function( BridgeEventPayload_MessageRemoved value)  messageRemoved,required TResult Function( BridgeEventPayload_MessagePartUpdated value)  messagePartUpdated,required TResult Function( BridgeEventPayload_MessagePartRemoved value)  messagePartRemoved,required TResult Function( BridgeEventPayload_MessagePartDelta value)  messagePartDelta,required TResult Function( BridgeEventPayload_InteractionChanged value)  interactionChanged,required TResult Function( BridgeEventPayload_AgentChanged value)  agentChanged,required TResult Function( BridgeEventPayload_AgentTimelineChanged value)  agentTimelineChanged,required TResult Function( BridgeEventPayload_SessionRuntimeChanged value)  sessionRuntimeChanged,required TResult Function( BridgeEventPayload_SkillActivated value)  skillActivated,required TResult Function( BridgeEventPayload_PlanLifecycleChanged value)  planLifecycleChanged,required TResult Function( BridgeEventPayload_SessionListChanged value)  sessionListChanged,required TResult Function( BridgeEventPayload_McpHealthChanged value)  mcpHealthChanged,required TResult Function( BridgeEventPayload_LspHealthChanged value)  lspHealthChanged,required TResult Function( BridgeEventPayload_Stale value)  stale,}){
final _that = this;
switch (_that) {
case BridgeEventPayload_TurnChanged():
return turnChanged(_that);case BridgeEventPayload_MessageUpdated():
return messageUpdated(_that);case BridgeEventPayload_MessageRemoved():
return messageRemoved(_that);case BridgeEventPayload_MessagePartUpdated():
return messagePartUpdated(_that);case BridgeEventPayload_MessagePartRemoved():
return messagePartRemoved(_that);case BridgeEventPayload_MessagePartDelta():
return messagePartDelta(_that);case BridgeEventPayload_InteractionChanged():
return interactionChanged(_that);case BridgeEventPayload_AgentChanged():
return agentChanged(_that);case BridgeEventPayload_AgentTimelineChanged():
return agentTimelineChanged(_that);case BridgeEventPayload_SessionRuntimeChanged():
return sessionRuntimeChanged(_that);case BridgeEventPayload_SkillActivated():
return skillActivated(_that);case BridgeEventPayload_PlanLifecycleChanged():
return planLifecycleChanged(_that);case BridgeEventPayload_SessionListChanged():
return sessionListChanged(_that);case BridgeEventPayload_McpHealthChanged():
return mcpHealthChanged(_that);case BridgeEventPayload_LspHealthChanged():
return lspHealthChanged(_that);case BridgeEventPayload_Stale():
return stale(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeEventPayload_TurnChanged value)?  turnChanged,TResult? Function( BridgeEventPayload_MessageUpdated value)?  messageUpdated,TResult? Function( BridgeEventPayload_MessageRemoved value)?  messageRemoved,TResult? Function( BridgeEventPayload_MessagePartUpdated value)?  messagePartUpdated,TResult? Function( BridgeEventPayload_MessagePartRemoved value)?  messagePartRemoved,TResult? Function( BridgeEventPayload_MessagePartDelta value)?  messagePartDelta,TResult? Function( BridgeEventPayload_InteractionChanged value)?  interactionChanged,TResult? Function( BridgeEventPayload_AgentChanged value)?  agentChanged,TResult? Function( BridgeEventPayload_AgentTimelineChanged value)?  agentTimelineChanged,TResult? Function( BridgeEventPayload_SessionRuntimeChanged value)?  sessionRuntimeChanged,TResult? Function( BridgeEventPayload_SkillActivated value)?  skillActivated,TResult? Function( BridgeEventPayload_PlanLifecycleChanged value)?  planLifecycleChanged,TResult? Function( BridgeEventPayload_SessionListChanged value)?  sessionListChanged,TResult? Function( BridgeEventPayload_McpHealthChanged value)?  mcpHealthChanged,TResult? Function( BridgeEventPayload_LspHealthChanged value)?  lspHealthChanged,TResult? Function( BridgeEventPayload_Stale value)?  stale,}){
final _that = this;
switch (_that) {
case BridgeEventPayload_TurnChanged() when turnChanged != null:
return turnChanged(_that);case BridgeEventPayload_MessageUpdated() when messageUpdated != null:
return messageUpdated(_that);case BridgeEventPayload_MessageRemoved() when messageRemoved != null:
return messageRemoved(_that);case BridgeEventPayload_MessagePartUpdated() when messagePartUpdated != null:
return messagePartUpdated(_that);case BridgeEventPayload_MessagePartRemoved() when messagePartRemoved != null:
return messagePartRemoved(_that);case BridgeEventPayload_MessagePartDelta() when messagePartDelta != null:
return messagePartDelta(_that);case BridgeEventPayload_InteractionChanged() when interactionChanged != null:
return interactionChanged(_that);case BridgeEventPayload_AgentChanged() when agentChanged != null:
return agentChanged(_that);case BridgeEventPayload_AgentTimelineChanged() when agentTimelineChanged != null:
return agentTimelineChanged(_that);case BridgeEventPayload_SessionRuntimeChanged() when sessionRuntimeChanged != null:
return sessionRuntimeChanged(_that);case BridgeEventPayload_SkillActivated() when skillActivated != null:
return skillActivated(_that);case BridgeEventPayload_PlanLifecycleChanged() when planLifecycleChanged != null:
return planLifecycleChanged(_that);case BridgeEventPayload_SessionListChanged() when sessionListChanged != null:
return sessionListChanged(_that);case BridgeEventPayload_McpHealthChanged() when mcpHealthChanged != null:
return mcpHealthChanged(_that);case BridgeEventPayload_LspHealthChanged() when lspHealthChanged != null:
return lspHealthChanged(_that);case BridgeEventPayload_Stale() when stale != null:
return stale(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeStudioTurnDto turn)?  turnChanged,TResult Function( BridgeStudioMessageDto message)?  messageUpdated,TResult Function( String messageId)?  messageRemoved,TResult Function( BridgeStudioPartDto part_)?  messagePartUpdated,TResult Function( String messageId,  String partId)?  messagePartRemoved,TResult Function( BridgeStudioPartDeltaDto delta)?  messagePartDelta,TResult Function( BridgeInteractionChangedDto event)?  interactionChanged,TResult Function( BridgeAgentSnapshotDto agent)?  agentChanged,TResult Function( BridgeAgentTimelineEventDto event)?  agentTimelineChanged,TResult Function( BridgeSessionRuntimeDto runtime)?  sessionRuntimeChanged,TResult Function( BridgeSkillActivationDto activation)?  skillActivated,TResult Function( BridgePlanLifecycleDto event)?  planLifecycleChanged,TResult Function( String projectId,  List<SessionDto> sessions)?  sessionListChanged,TResult Function( BridgeMcpHealthDto health)?  mcpHealthChanged,TResult Function( BridgeLspHealthDto health)?  lspHealthChanged,TResult Function( BigInt laggedEvents)?  stale,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeEventPayload_TurnChanged() when turnChanged != null:
return turnChanged(_that.turn);case BridgeEventPayload_MessageUpdated() when messageUpdated != null:
return messageUpdated(_that.message);case BridgeEventPayload_MessageRemoved() when messageRemoved != null:
return messageRemoved(_that.messageId);case BridgeEventPayload_MessagePartUpdated() when messagePartUpdated != null:
return messagePartUpdated(_that.part_);case BridgeEventPayload_MessagePartRemoved() when messagePartRemoved != null:
return messagePartRemoved(_that.messageId,_that.partId);case BridgeEventPayload_MessagePartDelta() when messagePartDelta != null:
return messagePartDelta(_that.delta);case BridgeEventPayload_InteractionChanged() when interactionChanged != null:
return interactionChanged(_that.event);case BridgeEventPayload_AgentChanged() when agentChanged != null:
return agentChanged(_that.agent);case BridgeEventPayload_AgentTimelineChanged() when agentTimelineChanged != null:
return agentTimelineChanged(_that.event);case BridgeEventPayload_SessionRuntimeChanged() when sessionRuntimeChanged != null:
return sessionRuntimeChanged(_that.runtime);case BridgeEventPayload_SkillActivated() when skillActivated != null:
return skillActivated(_that.activation);case BridgeEventPayload_PlanLifecycleChanged() when planLifecycleChanged != null:
return planLifecycleChanged(_that.event);case BridgeEventPayload_SessionListChanged() when sessionListChanged != null:
return sessionListChanged(_that.projectId,_that.sessions);case BridgeEventPayload_McpHealthChanged() when mcpHealthChanged != null:
return mcpHealthChanged(_that.health);case BridgeEventPayload_LspHealthChanged() when lspHealthChanged != null:
return lspHealthChanged(_that.health);case BridgeEventPayload_Stale() when stale != null:
return stale(_that.laggedEvents);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeStudioTurnDto turn)  turnChanged,required TResult Function( BridgeStudioMessageDto message)  messageUpdated,required TResult Function( String messageId)  messageRemoved,required TResult Function( BridgeStudioPartDto part_)  messagePartUpdated,required TResult Function( String messageId,  String partId)  messagePartRemoved,required TResult Function( BridgeStudioPartDeltaDto delta)  messagePartDelta,required TResult Function( BridgeInteractionChangedDto event)  interactionChanged,required TResult Function( BridgeAgentSnapshotDto agent)  agentChanged,required TResult Function( BridgeAgentTimelineEventDto event)  agentTimelineChanged,required TResult Function( BridgeSessionRuntimeDto runtime)  sessionRuntimeChanged,required TResult Function( BridgeSkillActivationDto activation)  skillActivated,required TResult Function( BridgePlanLifecycleDto event)  planLifecycleChanged,required TResult Function( String projectId,  List<SessionDto> sessions)  sessionListChanged,required TResult Function( BridgeMcpHealthDto health)  mcpHealthChanged,required TResult Function( BridgeLspHealthDto health)  lspHealthChanged,required TResult Function( BigInt laggedEvents)  stale,}) {final _that = this;
switch (_that) {
case BridgeEventPayload_TurnChanged():
return turnChanged(_that.turn);case BridgeEventPayload_MessageUpdated():
return messageUpdated(_that.message);case BridgeEventPayload_MessageRemoved():
return messageRemoved(_that.messageId);case BridgeEventPayload_MessagePartUpdated():
return messagePartUpdated(_that.part_);case BridgeEventPayload_MessagePartRemoved():
return messagePartRemoved(_that.messageId,_that.partId);case BridgeEventPayload_MessagePartDelta():
return messagePartDelta(_that.delta);case BridgeEventPayload_InteractionChanged():
return interactionChanged(_that.event);case BridgeEventPayload_AgentChanged():
return agentChanged(_that.agent);case BridgeEventPayload_AgentTimelineChanged():
return agentTimelineChanged(_that.event);case BridgeEventPayload_SessionRuntimeChanged():
return sessionRuntimeChanged(_that.runtime);case BridgeEventPayload_SkillActivated():
return skillActivated(_that.activation);case BridgeEventPayload_PlanLifecycleChanged():
return planLifecycleChanged(_that.event);case BridgeEventPayload_SessionListChanged():
return sessionListChanged(_that.projectId,_that.sessions);case BridgeEventPayload_McpHealthChanged():
return mcpHealthChanged(_that.health);case BridgeEventPayload_LspHealthChanged():
return lspHealthChanged(_that.health);case BridgeEventPayload_Stale():
return stale(_that.laggedEvents);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeStudioTurnDto turn)?  turnChanged,TResult? Function( BridgeStudioMessageDto message)?  messageUpdated,TResult? Function( String messageId)?  messageRemoved,TResult? Function( BridgeStudioPartDto part_)?  messagePartUpdated,TResult? Function( String messageId,  String partId)?  messagePartRemoved,TResult? Function( BridgeStudioPartDeltaDto delta)?  messagePartDelta,TResult? Function( BridgeInteractionChangedDto event)?  interactionChanged,TResult? Function( BridgeAgentSnapshotDto agent)?  agentChanged,TResult? Function( BridgeAgentTimelineEventDto event)?  agentTimelineChanged,TResult? Function( BridgeSessionRuntimeDto runtime)?  sessionRuntimeChanged,TResult? Function( BridgeSkillActivationDto activation)?  skillActivated,TResult? Function( BridgePlanLifecycleDto event)?  planLifecycleChanged,TResult? Function( String projectId,  List<SessionDto> sessions)?  sessionListChanged,TResult? Function( BridgeMcpHealthDto health)?  mcpHealthChanged,TResult? Function( BridgeLspHealthDto health)?  lspHealthChanged,TResult? Function( BigInt laggedEvents)?  stale,}) {final _that = this;
switch (_that) {
case BridgeEventPayload_TurnChanged() when turnChanged != null:
return turnChanged(_that.turn);case BridgeEventPayload_MessageUpdated() when messageUpdated != null:
return messageUpdated(_that.message);case BridgeEventPayload_MessageRemoved() when messageRemoved != null:
return messageRemoved(_that.messageId);case BridgeEventPayload_MessagePartUpdated() when messagePartUpdated != null:
return messagePartUpdated(_that.part_);case BridgeEventPayload_MessagePartRemoved() when messagePartRemoved != null:
return messagePartRemoved(_that.messageId,_that.partId);case BridgeEventPayload_MessagePartDelta() when messagePartDelta != null:
return messagePartDelta(_that.delta);case BridgeEventPayload_InteractionChanged() when interactionChanged != null:
return interactionChanged(_that.event);case BridgeEventPayload_AgentChanged() when agentChanged != null:
return agentChanged(_that.agent);case BridgeEventPayload_AgentTimelineChanged() when agentTimelineChanged != null:
return agentTimelineChanged(_that.event);case BridgeEventPayload_SessionRuntimeChanged() when sessionRuntimeChanged != null:
return sessionRuntimeChanged(_that.runtime);case BridgeEventPayload_SkillActivated() when skillActivated != null:
return skillActivated(_that.activation);case BridgeEventPayload_PlanLifecycleChanged() when planLifecycleChanged != null:
return planLifecycleChanged(_that.event);case BridgeEventPayload_SessionListChanged() when sessionListChanged != null:
return sessionListChanged(_that.projectId,_that.sessions);case BridgeEventPayload_McpHealthChanged() when mcpHealthChanged != null:
return mcpHealthChanged(_that.health);case BridgeEventPayload_LspHealthChanged() when lspHealthChanged != null:
return lspHealthChanged(_that.health);case BridgeEventPayload_Stale() when stale != null:
return stale(_that.laggedEvents);case _:
  return null;

}
}

}

/// @nodoc


class BridgeEventPayload_TurnChanged extends BridgeEventPayload {
  const BridgeEventPayload_TurnChanged({required this.turn}): super._();
  

 final  BridgeStudioTurnDto turn;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_TurnChangedCopyWith<BridgeEventPayload_TurnChanged> get copyWith => _$BridgeEventPayload_TurnChangedCopyWithImpl<BridgeEventPayload_TurnChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_TurnChanged&&(identical(other.turn, turn) || other.turn == turn));
}


@override
int get hashCode => Object.hash(runtimeType,turn);

@override
String toString() {
  return 'BridgeEventPayload.turnChanged(turn: $turn)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_TurnChangedCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_TurnChangedCopyWith(BridgeEventPayload_TurnChanged value, $Res Function(BridgeEventPayload_TurnChanged) _then) = _$BridgeEventPayload_TurnChangedCopyWithImpl;
@useResult
$Res call({
 BridgeStudioTurnDto turn
});




}
/// @nodoc
class _$BridgeEventPayload_TurnChangedCopyWithImpl<$Res>
    implements $BridgeEventPayload_TurnChangedCopyWith<$Res> {
  _$BridgeEventPayload_TurnChangedCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_TurnChanged _self;
  final $Res Function(BridgeEventPayload_TurnChanged) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? turn = null,}) {
  return _then(BridgeEventPayload_TurnChanged(
turn: null == turn ? _self.turn : turn // ignore: cast_nullable_to_non_nullable
as BridgeStudioTurnDto,
  ));
}


}

/// @nodoc


class BridgeEventPayload_MessageUpdated extends BridgeEventPayload {
  const BridgeEventPayload_MessageUpdated({required this.message}): super._();
  

 final  BridgeStudioMessageDto message;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_MessageUpdatedCopyWith<BridgeEventPayload_MessageUpdated> get copyWith => _$BridgeEventPayload_MessageUpdatedCopyWithImpl<BridgeEventPayload_MessageUpdated>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_MessageUpdated&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode => Object.hash(runtimeType,message);

@override
String toString() {
  return 'BridgeEventPayload.messageUpdated(message: $message)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_MessageUpdatedCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_MessageUpdatedCopyWith(BridgeEventPayload_MessageUpdated value, $Res Function(BridgeEventPayload_MessageUpdated) _then) = _$BridgeEventPayload_MessageUpdatedCopyWithImpl;
@useResult
$Res call({
 BridgeStudioMessageDto message
});




}
/// @nodoc
class _$BridgeEventPayload_MessageUpdatedCopyWithImpl<$Res>
    implements $BridgeEventPayload_MessageUpdatedCopyWith<$Res> {
  _$BridgeEventPayload_MessageUpdatedCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_MessageUpdated _self;
  final $Res Function(BridgeEventPayload_MessageUpdated) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = null,}) {
  return _then(BridgeEventPayload_MessageUpdated(
message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as BridgeStudioMessageDto,
  ));
}


}

/// @nodoc


class BridgeEventPayload_MessageRemoved extends BridgeEventPayload {
  const BridgeEventPayload_MessageRemoved({required this.messageId}): super._();
  

 final  String messageId;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_MessageRemovedCopyWith<BridgeEventPayload_MessageRemoved> get copyWith => _$BridgeEventPayload_MessageRemovedCopyWithImpl<BridgeEventPayload_MessageRemoved>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_MessageRemoved&&(identical(other.messageId, messageId) || other.messageId == messageId));
}


@override
int get hashCode => Object.hash(runtimeType,messageId);

@override
String toString() {
  return 'BridgeEventPayload.messageRemoved(messageId: $messageId)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_MessageRemovedCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_MessageRemovedCopyWith(BridgeEventPayload_MessageRemoved value, $Res Function(BridgeEventPayload_MessageRemoved) _then) = _$BridgeEventPayload_MessageRemovedCopyWithImpl;
@useResult
$Res call({
 String messageId
});




}
/// @nodoc
class _$BridgeEventPayload_MessageRemovedCopyWithImpl<$Res>
    implements $BridgeEventPayload_MessageRemovedCopyWith<$Res> {
  _$BridgeEventPayload_MessageRemovedCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_MessageRemoved _self;
  final $Res Function(BridgeEventPayload_MessageRemoved) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? messageId = null,}) {
  return _then(BridgeEventPayload_MessageRemoved(
messageId: null == messageId ? _self.messageId : messageId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeEventPayload_MessagePartUpdated extends BridgeEventPayload {
  const BridgeEventPayload_MessagePartUpdated({required this.part_}): super._();
  

 final  BridgeStudioPartDto part_;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_MessagePartUpdatedCopyWith<BridgeEventPayload_MessagePartUpdated> get copyWith => _$BridgeEventPayload_MessagePartUpdatedCopyWithImpl<BridgeEventPayload_MessagePartUpdated>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_MessagePartUpdated&&(identical(other.part_, part_) || other.part_ == part_));
}


@override
int get hashCode => Object.hash(runtimeType,part_);

@override
String toString() {
  return 'BridgeEventPayload.messagePartUpdated(part_: $part_)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_MessagePartUpdatedCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_MessagePartUpdatedCopyWith(BridgeEventPayload_MessagePartUpdated value, $Res Function(BridgeEventPayload_MessagePartUpdated) _then) = _$BridgeEventPayload_MessagePartUpdatedCopyWithImpl;
@useResult
$Res call({
 BridgeStudioPartDto part_
});




}
/// @nodoc
class _$BridgeEventPayload_MessagePartUpdatedCopyWithImpl<$Res>
    implements $BridgeEventPayload_MessagePartUpdatedCopyWith<$Res> {
  _$BridgeEventPayload_MessagePartUpdatedCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_MessagePartUpdated _self;
  final $Res Function(BridgeEventPayload_MessagePartUpdated) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? part_ = null,}) {
  return _then(BridgeEventPayload_MessagePartUpdated(
part_: null == part_ ? _self.part_ : part_ // ignore: cast_nullable_to_non_nullable
as BridgeStudioPartDto,
  ));
}


}

/// @nodoc


class BridgeEventPayload_MessagePartRemoved extends BridgeEventPayload {
  const BridgeEventPayload_MessagePartRemoved({required this.messageId, required this.partId}): super._();
  

 final  String messageId;
 final  String partId;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_MessagePartRemovedCopyWith<BridgeEventPayload_MessagePartRemoved> get copyWith => _$BridgeEventPayload_MessagePartRemovedCopyWithImpl<BridgeEventPayload_MessagePartRemoved>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_MessagePartRemoved&&(identical(other.messageId, messageId) || other.messageId == messageId)&&(identical(other.partId, partId) || other.partId == partId));
}


@override
int get hashCode => Object.hash(runtimeType,messageId,partId);

@override
String toString() {
  return 'BridgeEventPayload.messagePartRemoved(messageId: $messageId, partId: $partId)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_MessagePartRemovedCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_MessagePartRemovedCopyWith(BridgeEventPayload_MessagePartRemoved value, $Res Function(BridgeEventPayload_MessagePartRemoved) _then) = _$BridgeEventPayload_MessagePartRemovedCopyWithImpl;
@useResult
$Res call({
 String messageId, String partId
});




}
/// @nodoc
class _$BridgeEventPayload_MessagePartRemovedCopyWithImpl<$Res>
    implements $BridgeEventPayload_MessagePartRemovedCopyWith<$Res> {
  _$BridgeEventPayload_MessagePartRemovedCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_MessagePartRemoved _self;
  final $Res Function(BridgeEventPayload_MessagePartRemoved) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? messageId = null,Object? partId = null,}) {
  return _then(BridgeEventPayload_MessagePartRemoved(
messageId: null == messageId ? _self.messageId : messageId // ignore: cast_nullable_to_non_nullable
as String,partId: null == partId ? _self.partId : partId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeEventPayload_MessagePartDelta extends BridgeEventPayload {
  const BridgeEventPayload_MessagePartDelta({required this.delta}): super._();
  

 final  BridgeStudioPartDeltaDto delta;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_MessagePartDeltaCopyWith<BridgeEventPayload_MessagePartDelta> get copyWith => _$BridgeEventPayload_MessagePartDeltaCopyWithImpl<BridgeEventPayload_MessagePartDelta>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_MessagePartDelta&&(identical(other.delta, delta) || other.delta == delta));
}


@override
int get hashCode => Object.hash(runtimeType,delta);

@override
String toString() {
  return 'BridgeEventPayload.messagePartDelta(delta: $delta)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_MessagePartDeltaCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_MessagePartDeltaCopyWith(BridgeEventPayload_MessagePartDelta value, $Res Function(BridgeEventPayload_MessagePartDelta) _then) = _$BridgeEventPayload_MessagePartDeltaCopyWithImpl;
@useResult
$Res call({
 BridgeStudioPartDeltaDto delta
});




}
/// @nodoc
class _$BridgeEventPayload_MessagePartDeltaCopyWithImpl<$Res>
    implements $BridgeEventPayload_MessagePartDeltaCopyWith<$Res> {
  _$BridgeEventPayload_MessagePartDeltaCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_MessagePartDelta _self;
  final $Res Function(BridgeEventPayload_MessagePartDelta) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? delta = null,}) {
  return _then(BridgeEventPayload_MessagePartDelta(
delta: null == delta ? _self.delta : delta // ignore: cast_nullable_to_non_nullable
as BridgeStudioPartDeltaDto,
  ));
}


}

/// @nodoc


class BridgeEventPayload_InteractionChanged extends BridgeEventPayload {
  const BridgeEventPayload_InteractionChanged({required this.event}): super._();
  

 final  BridgeInteractionChangedDto event;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_InteractionChangedCopyWith<BridgeEventPayload_InteractionChanged> get copyWith => _$BridgeEventPayload_InteractionChangedCopyWithImpl<BridgeEventPayload_InteractionChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_InteractionChanged&&(identical(other.event, event) || other.event == event));
}


@override
int get hashCode => Object.hash(runtimeType,event);

@override
String toString() {
  return 'BridgeEventPayload.interactionChanged(event: $event)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_InteractionChangedCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_InteractionChangedCopyWith(BridgeEventPayload_InteractionChanged value, $Res Function(BridgeEventPayload_InteractionChanged) _then) = _$BridgeEventPayload_InteractionChangedCopyWithImpl;
@useResult
$Res call({
 BridgeInteractionChangedDto event
});




}
/// @nodoc
class _$BridgeEventPayload_InteractionChangedCopyWithImpl<$Res>
    implements $BridgeEventPayload_InteractionChangedCopyWith<$Res> {
  _$BridgeEventPayload_InteractionChangedCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_InteractionChanged _self;
  final $Res Function(BridgeEventPayload_InteractionChanged) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? event = null,}) {
  return _then(BridgeEventPayload_InteractionChanged(
event: null == event ? _self.event : event // ignore: cast_nullable_to_non_nullable
as BridgeInteractionChangedDto,
  ));
}


}

/// @nodoc


class BridgeEventPayload_AgentChanged extends BridgeEventPayload {
  const BridgeEventPayload_AgentChanged({required this.agent}): super._();
  

 final  BridgeAgentSnapshotDto agent;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_AgentChangedCopyWith<BridgeEventPayload_AgentChanged> get copyWith => _$BridgeEventPayload_AgentChangedCopyWithImpl<BridgeEventPayload_AgentChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_AgentChanged&&(identical(other.agent, agent) || other.agent == agent));
}


@override
int get hashCode => Object.hash(runtimeType,agent);

@override
String toString() {
  return 'BridgeEventPayload.agentChanged(agent: $agent)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_AgentChangedCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_AgentChangedCopyWith(BridgeEventPayload_AgentChanged value, $Res Function(BridgeEventPayload_AgentChanged) _then) = _$BridgeEventPayload_AgentChangedCopyWithImpl;
@useResult
$Res call({
 BridgeAgentSnapshotDto agent
});




}
/// @nodoc
class _$BridgeEventPayload_AgentChangedCopyWithImpl<$Res>
    implements $BridgeEventPayload_AgentChangedCopyWith<$Res> {
  _$BridgeEventPayload_AgentChangedCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_AgentChanged _self;
  final $Res Function(BridgeEventPayload_AgentChanged) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? agent = null,}) {
  return _then(BridgeEventPayload_AgentChanged(
agent: null == agent ? _self.agent : agent // ignore: cast_nullable_to_non_nullable
as BridgeAgentSnapshotDto,
  ));
}


}

/// @nodoc


class BridgeEventPayload_AgentTimelineChanged extends BridgeEventPayload {
  const BridgeEventPayload_AgentTimelineChanged({required this.event}): super._();
  

 final  BridgeAgentTimelineEventDto event;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_AgentTimelineChangedCopyWith<BridgeEventPayload_AgentTimelineChanged> get copyWith => _$BridgeEventPayload_AgentTimelineChangedCopyWithImpl<BridgeEventPayload_AgentTimelineChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_AgentTimelineChanged&&(identical(other.event, event) || other.event == event));
}


@override
int get hashCode => Object.hash(runtimeType,event);

@override
String toString() {
  return 'BridgeEventPayload.agentTimelineChanged(event: $event)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_AgentTimelineChangedCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_AgentTimelineChangedCopyWith(BridgeEventPayload_AgentTimelineChanged value, $Res Function(BridgeEventPayload_AgentTimelineChanged) _then) = _$BridgeEventPayload_AgentTimelineChangedCopyWithImpl;
@useResult
$Res call({
 BridgeAgentTimelineEventDto event
});




}
/// @nodoc
class _$BridgeEventPayload_AgentTimelineChangedCopyWithImpl<$Res>
    implements $BridgeEventPayload_AgentTimelineChangedCopyWith<$Res> {
  _$BridgeEventPayload_AgentTimelineChangedCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_AgentTimelineChanged _self;
  final $Res Function(BridgeEventPayload_AgentTimelineChanged) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? event = null,}) {
  return _then(BridgeEventPayload_AgentTimelineChanged(
event: null == event ? _self.event : event // ignore: cast_nullable_to_non_nullable
as BridgeAgentTimelineEventDto,
  ));
}


}

/// @nodoc


class BridgeEventPayload_SessionRuntimeChanged extends BridgeEventPayload {
  const BridgeEventPayload_SessionRuntimeChanged({required this.runtime}): super._();
  

 final  BridgeSessionRuntimeDto runtime;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_SessionRuntimeChangedCopyWith<BridgeEventPayload_SessionRuntimeChanged> get copyWith => _$BridgeEventPayload_SessionRuntimeChangedCopyWithImpl<BridgeEventPayload_SessionRuntimeChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_SessionRuntimeChanged&&(identical(other.runtime, runtime) || other.runtime == runtime));
}


@override
int get hashCode => Object.hash(runtimeType,runtime);

@override
String toString() {
  return 'BridgeEventPayload.sessionRuntimeChanged(runtime: $runtime)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_SessionRuntimeChangedCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_SessionRuntimeChangedCopyWith(BridgeEventPayload_SessionRuntimeChanged value, $Res Function(BridgeEventPayload_SessionRuntimeChanged) _then) = _$BridgeEventPayload_SessionRuntimeChangedCopyWithImpl;
@useResult
$Res call({
 BridgeSessionRuntimeDto runtime
});




}
/// @nodoc
class _$BridgeEventPayload_SessionRuntimeChangedCopyWithImpl<$Res>
    implements $BridgeEventPayload_SessionRuntimeChangedCopyWith<$Res> {
  _$BridgeEventPayload_SessionRuntimeChangedCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_SessionRuntimeChanged _self;
  final $Res Function(BridgeEventPayload_SessionRuntimeChanged) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? runtime = null,}) {
  return _then(BridgeEventPayload_SessionRuntimeChanged(
runtime: null == runtime ? _self.runtime : runtime // ignore: cast_nullable_to_non_nullable
as BridgeSessionRuntimeDto,
  ));
}


}

/// @nodoc


class BridgeEventPayload_SkillActivated extends BridgeEventPayload {
  const BridgeEventPayload_SkillActivated({required this.activation}): super._();
  

 final  BridgeSkillActivationDto activation;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_SkillActivatedCopyWith<BridgeEventPayload_SkillActivated> get copyWith => _$BridgeEventPayload_SkillActivatedCopyWithImpl<BridgeEventPayload_SkillActivated>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_SkillActivated&&(identical(other.activation, activation) || other.activation == activation));
}


@override
int get hashCode => Object.hash(runtimeType,activation);

@override
String toString() {
  return 'BridgeEventPayload.skillActivated(activation: $activation)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_SkillActivatedCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_SkillActivatedCopyWith(BridgeEventPayload_SkillActivated value, $Res Function(BridgeEventPayload_SkillActivated) _then) = _$BridgeEventPayload_SkillActivatedCopyWithImpl;
@useResult
$Res call({
 BridgeSkillActivationDto activation
});




}
/// @nodoc
class _$BridgeEventPayload_SkillActivatedCopyWithImpl<$Res>
    implements $BridgeEventPayload_SkillActivatedCopyWith<$Res> {
  _$BridgeEventPayload_SkillActivatedCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_SkillActivated _self;
  final $Res Function(BridgeEventPayload_SkillActivated) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? activation = null,}) {
  return _then(BridgeEventPayload_SkillActivated(
activation: null == activation ? _self.activation : activation // ignore: cast_nullable_to_non_nullable
as BridgeSkillActivationDto,
  ));
}


}

/// @nodoc


class BridgeEventPayload_PlanLifecycleChanged extends BridgeEventPayload {
  const BridgeEventPayload_PlanLifecycleChanged({required this.event}): super._();
  

 final  BridgePlanLifecycleDto event;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_PlanLifecycleChangedCopyWith<BridgeEventPayload_PlanLifecycleChanged> get copyWith => _$BridgeEventPayload_PlanLifecycleChangedCopyWithImpl<BridgeEventPayload_PlanLifecycleChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_PlanLifecycleChanged&&(identical(other.event, event) || other.event == event));
}


@override
int get hashCode => Object.hash(runtimeType,event);

@override
String toString() {
  return 'BridgeEventPayload.planLifecycleChanged(event: $event)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_PlanLifecycleChangedCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_PlanLifecycleChangedCopyWith(BridgeEventPayload_PlanLifecycleChanged value, $Res Function(BridgeEventPayload_PlanLifecycleChanged) _then) = _$BridgeEventPayload_PlanLifecycleChangedCopyWithImpl;
@useResult
$Res call({
 BridgePlanLifecycleDto event
});




}
/// @nodoc
class _$BridgeEventPayload_PlanLifecycleChangedCopyWithImpl<$Res>
    implements $BridgeEventPayload_PlanLifecycleChangedCopyWith<$Res> {
  _$BridgeEventPayload_PlanLifecycleChangedCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_PlanLifecycleChanged _self;
  final $Res Function(BridgeEventPayload_PlanLifecycleChanged) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? event = null,}) {
  return _then(BridgeEventPayload_PlanLifecycleChanged(
event: null == event ? _self.event : event // ignore: cast_nullable_to_non_nullable
as BridgePlanLifecycleDto,
  ));
}


}

/// @nodoc


class BridgeEventPayload_SessionListChanged extends BridgeEventPayload {
  const BridgeEventPayload_SessionListChanged({required this.projectId, required final  List<SessionDto> sessions}): _sessions = sessions,super._();
  

 final  String projectId;
 final  List<SessionDto> _sessions;
 List<SessionDto> get sessions {
  if (_sessions is EqualUnmodifiableListView) return _sessions;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_sessions);
}


/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_SessionListChangedCopyWith<BridgeEventPayload_SessionListChanged> get copyWith => _$BridgeEventPayload_SessionListChangedCopyWithImpl<BridgeEventPayload_SessionListChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_SessionListChanged&&(identical(other.projectId, projectId) || other.projectId == projectId)&&const DeepCollectionEquality().equals(other._sessions, _sessions));
}


@override
int get hashCode => Object.hash(runtimeType,projectId,const DeepCollectionEquality().hash(_sessions));

@override
String toString() {
  return 'BridgeEventPayload.sessionListChanged(projectId: $projectId, sessions: $sessions)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_SessionListChangedCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_SessionListChangedCopyWith(BridgeEventPayload_SessionListChanged value, $Res Function(BridgeEventPayload_SessionListChanged) _then) = _$BridgeEventPayload_SessionListChangedCopyWithImpl;
@useResult
$Res call({
 String projectId, List<SessionDto> sessions
});




}
/// @nodoc
class _$BridgeEventPayload_SessionListChangedCopyWithImpl<$Res>
    implements $BridgeEventPayload_SessionListChangedCopyWith<$Res> {
  _$BridgeEventPayload_SessionListChangedCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_SessionListChanged _self;
  final $Res Function(BridgeEventPayload_SessionListChanged) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? projectId = null,Object? sessions = null,}) {
  return _then(BridgeEventPayload_SessionListChanged(
projectId: null == projectId ? _self.projectId : projectId // ignore: cast_nullable_to_non_nullable
as String,sessions: null == sessions ? _self._sessions : sessions // ignore: cast_nullable_to_non_nullable
as List<SessionDto>,
  ));
}


}

/// @nodoc


class BridgeEventPayload_McpHealthChanged extends BridgeEventPayload {
  const BridgeEventPayload_McpHealthChanged({required this.health}): super._();
  

 final  BridgeMcpHealthDto health;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_McpHealthChangedCopyWith<BridgeEventPayload_McpHealthChanged> get copyWith => _$BridgeEventPayload_McpHealthChangedCopyWithImpl<BridgeEventPayload_McpHealthChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_McpHealthChanged&&(identical(other.health, health) || other.health == health));
}


@override
int get hashCode => Object.hash(runtimeType,health);

@override
String toString() {
  return 'BridgeEventPayload.mcpHealthChanged(health: $health)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_McpHealthChangedCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_McpHealthChangedCopyWith(BridgeEventPayload_McpHealthChanged value, $Res Function(BridgeEventPayload_McpHealthChanged) _then) = _$BridgeEventPayload_McpHealthChangedCopyWithImpl;
@useResult
$Res call({
 BridgeMcpHealthDto health
});




}
/// @nodoc
class _$BridgeEventPayload_McpHealthChangedCopyWithImpl<$Res>
    implements $BridgeEventPayload_McpHealthChangedCopyWith<$Res> {
  _$BridgeEventPayload_McpHealthChangedCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_McpHealthChanged _self;
  final $Res Function(BridgeEventPayload_McpHealthChanged) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? health = null,}) {
  return _then(BridgeEventPayload_McpHealthChanged(
health: null == health ? _self.health : health // ignore: cast_nullable_to_non_nullable
as BridgeMcpHealthDto,
  ));
}


}

/// @nodoc


class BridgeEventPayload_LspHealthChanged extends BridgeEventPayload {
  const BridgeEventPayload_LspHealthChanged({required this.health}): super._();
  

 final  BridgeLspHealthDto health;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_LspHealthChangedCopyWith<BridgeEventPayload_LspHealthChanged> get copyWith => _$BridgeEventPayload_LspHealthChangedCopyWithImpl<BridgeEventPayload_LspHealthChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_LspHealthChanged&&(identical(other.health, health) || other.health == health));
}


@override
int get hashCode => Object.hash(runtimeType,health);

@override
String toString() {
  return 'BridgeEventPayload.lspHealthChanged(health: $health)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_LspHealthChangedCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_LspHealthChangedCopyWith(BridgeEventPayload_LspHealthChanged value, $Res Function(BridgeEventPayload_LspHealthChanged) _then) = _$BridgeEventPayload_LspHealthChangedCopyWithImpl;
@useResult
$Res call({
 BridgeLspHealthDto health
});




}
/// @nodoc
class _$BridgeEventPayload_LspHealthChangedCopyWithImpl<$Res>
    implements $BridgeEventPayload_LspHealthChangedCopyWith<$Res> {
  _$BridgeEventPayload_LspHealthChangedCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_LspHealthChanged _self;
  final $Res Function(BridgeEventPayload_LspHealthChanged) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? health = null,}) {
  return _then(BridgeEventPayload_LspHealthChanged(
health: null == health ? _self.health : health // ignore: cast_nullable_to_non_nullable
as BridgeLspHealthDto,
  ));
}


}

/// @nodoc


class BridgeEventPayload_Stale extends BridgeEventPayload {
  const BridgeEventPayload_Stale({required this.laggedEvents}): super._();
  

 final  BigInt laggedEvents;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeEventPayload_StaleCopyWith<BridgeEventPayload_Stale> get copyWith => _$BridgeEventPayload_StaleCopyWithImpl<BridgeEventPayload_Stale>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeEventPayload_Stale&&(identical(other.laggedEvents, laggedEvents) || other.laggedEvents == laggedEvents));
}


@override
int get hashCode => Object.hash(runtimeType,laggedEvents);

@override
String toString() {
  return 'BridgeEventPayload.stale(laggedEvents: $laggedEvents)';
}


}

/// @nodoc
abstract mixin class $BridgeEventPayload_StaleCopyWith<$Res> implements $BridgeEventPayloadCopyWith<$Res> {
  factory $BridgeEventPayload_StaleCopyWith(BridgeEventPayload_Stale value, $Res Function(BridgeEventPayload_Stale) _then) = _$BridgeEventPayload_StaleCopyWithImpl;
@useResult
$Res call({
 BigInt laggedEvents
});




}
/// @nodoc
class _$BridgeEventPayload_StaleCopyWithImpl<$Res>
    implements $BridgeEventPayload_StaleCopyWith<$Res> {
  _$BridgeEventPayload_StaleCopyWithImpl(this._self, this._then);

  final BridgeEventPayload_Stale _self;
  final $Res Function(BridgeEventPayload_Stale) _then;

/// Create a copy of BridgeEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? laggedEvents = null,}) {
  return _then(BridgeEventPayload_Stale(
laggedEvents: null == laggedEvents ? _self.laggedEvents : laggedEvents // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

// dart format on

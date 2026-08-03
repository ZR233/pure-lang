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
mixin _$BridgeProductEventPayload {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeProductEventPayload()';
}


}

/// @nodoc
class $BridgeProductEventPayloadCopyWith<$Res>  {
$BridgeProductEventPayloadCopyWith(BridgeProductEventPayload _, $Res Function(BridgeProductEventPayload) __);
}


/// Adds pattern-matching-related methods to [BridgeProductEventPayload].
extension BridgeProductEventPayloadPatterns on BridgeProductEventPayload {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeProductEventPayload_SessionListChanged value)?  sessionListChanged,TResult Function( BridgeProductEventPayload_McpHealthChanged value)?  mcpHealthChanged,TResult Function( BridgeProductEventPayload_LspHealthChanged value)?  lspHealthChanged,TResult Function( BridgeProductEventPayload_SessionTaskChanged value)?  sessionTaskChanged,TResult Function( BridgeProductEventPayload_AgentDirectoryChanged value)?  agentDirectoryChanged,TResult Function( BridgeProductEventPayload_Stale value)?  stale,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeProductEventPayload_SessionListChanged() when sessionListChanged != null:
return sessionListChanged(_that);case BridgeProductEventPayload_McpHealthChanged() when mcpHealthChanged != null:
return mcpHealthChanged(_that);case BridgeProductEventPayload_LspHealthChanged() when lspHealthChanged != null:
return lspHealthChanged(_that);case BridgeProductEventPayload_SessionTaskChanged() when sessionTaskChanged != null:
return sessionTaskChanged(_that);case BridgeProductEventPayload_AgentDirectoryChanged() when agentDirectoryChanged != null:
return agentDirectoryChanged(_that);case BridgeProductEventPayload_Stale() when stale != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeProductEventPayload_SessionListChanged value)  sessionListChanged,required TResult Function( BridgeProductEventPayload_McpHealthChanged value)  mcpHealthChanged,required TResult Function( BridgeProductEventPayload_LspHealthChanged value)  lspHealthChanged,required TResult Function( BridgeProductEventPayload_SessionTaskChanged value)  sessionTaskChanged,required TResult Function( BridgeProductEventPayload_AgentDirectoryChanged value)  agentDirectoryChanged,required TResult Function( BridgeProductEventPayload_Stale value)  stale,}){
final _that = this;
switch (_that) {
case BridgeProductEventPayload_SessionListChanged():
return sessionListChanged(_that);case BridgeProductEventPayload_McpHealthChanged():
return mcpHealthChanged(_that);case BridgeProductEventPayload_LspHealthChanged():
return lspHealthChanged(_that);case BridgeProductEventPayload_SessionTaskChanged():
return sessionTaskChanged(_that);case BridgeProductEventPayload_AgentDirectoryChanged():
return agentDirectoryChanged(_that);case BridgeProductEventPayload_Stale():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeProductEventPayload_SessionListChanged value)?  sessionListChanged,TResult? Function( BridgeProductEventPayload_McpHealthChanged value)?  mcpHealthChanged,TResult? Function( BridgeProductEventPayload_LspHealthChanged value)?  lspHealthChanged,TResult? Function( BridgeProductEventPayload_SessionTaskChanged value)?  sessionTaskChanged,TResult? Function( BridgeProductEventPayload_AgentDirectoryChanged value)?  agentDirectoryChanged,TResult? Function( BridgeProductEventPayload_Stale value)?  stale,}){
final _that = this;
switch (_that) {
case BridgeProductEventPayload_SessionListChanged() when sessionListChanged != null:
return sessionListChanged(_that);case BridgeProductEventPayload_McpHealthChanged() when mcpHealthChanged != null:
return mcpHealthChanged(_that);case BridgeProductEventPayload_LspHealthChanged() when lspHealthChanged != null:
return lspHealthChanged(_that);case BridgeProductEventPayload_SessionTaskChanged() when sessionTaskChanged != null:
return sessionTaskChanged(_that);case BridgeProductEventPayload_AgentDirectoryChanged() when agentDirectoryChanged != null:
return agentDirectoryChanged(_that);case BridgeProductEventPayload_Stale() when stale != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String projectId,  List<SessionDto> sessions)?  sessionListChanged,TResult Function( BridgeMcpHealthDto health)?  mcpHealthChanged,TResult Function( BridgeLspHealthDto health)?  lspHealthChanged,TResult Function( String sessionId,  BridgeTaskRuntimeDto? task)?  sessionTaskChanged,TResult Function( String rootSessionId,  BridgeAgentDirectoryEntryDto agent)?  agentDirectoryChanged,TResult Function( BigInt laggedEvents)?  stale,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeProductEventPayload_SessionListChanged() when sessionListChanged != null:
return sessionListChanged(_that.projectId,_that.sessions);case BridgeProductEventPayload_McpHealthChanged() when mcpHealthChanged != null:
return mcpHealthChanged(_that.health);case BridgeProductEventPayload_LspHealthChanged() when lspHealthChanged != null:
return lspHealthChanged(_that.health);case BridgeProductEventPayload_SessionTaskChanged() when sessionTaskChanged != null:
return sessionTaskChanged(_that.sessionId,_that.task);case BridgeProductEventPayload_AgentDirectoryChanged() when agentDirectoryChanged != null:
return agentDirectoryChanged(_that.rootSessionId,_that.agent);case BridgeProductEventPayload_Stale() when stale != null:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String projectId,  List<SessionDto> sessions)  sessionListChanged,required TResult Function( BridgeMcpHealthDto health)  mcpHealthChanged,required TResult Function( BridgeLspHealthDto health)  lspHealthChanged,required TResult Function( String sessionId,  BridgeTaskRuntimeDto? task)  sessionTaskChanged,required TResult Function( String rootSessionId,  BridgeAgentDirectoryEntryDto agent)  agentDirectoryChanged,required TResult Function( BigInt laggedEvents)  stale,}) {final _that = this;
switch (_that) {
case BridgeProductEventPayload_SessionListChanged():
return sessionListChanged(_that.projectId,_that.sessions);case BridgeProductEventPayload_McpHealthChanged():
return mcpHealthChanged(_that.health);case BridgeProductEventPayload_LspHealthChanged():
return lspHealthChanged(_that.health);case BridgeProductEventPayload_SessionTaskChanged():
return sessionTaskChanged(_that.sessionId,_that.task);case BridgeProductEventPayload_AgentDirectoryChanged():
return agentDirectoryChanged(_that.rootSessionId,_that.agent);case BridgeProductEventPayload_Stale():
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String projectId,  List<SessionDto> sessions)?  sessionListChanged,TResult? Function( BridgeMcpHealthDto health)?  mcpHealthChanged,TResult? Function( BridgeLspHealthDto health)?  lspHealthChanged,TResult? Function( String sessionId,  BridgeTaskRuntimeDto? task)?  sessionTaskChanged,TResult? Function( String rootSessionId,  BridgeAgentDirectoryEntryDto agent)?  agentDirectoryChanged,TResult? Function( BigInt laggedEvents)?  stale,}) {final _that = this;
switch (_that) {
case BridgeProductEventPayload_SessionListChanged() when sessionListChanged != null:
return sessionListChanged(_that.projectId,_that.sessions);case BridgeProductEventPayload_McpHealthChanged() when mcpHealthChanged != null:
return mcpHealthChanged(_that.health);case BridgeProductEventPayload_LspHealthChanged() when lspHealthChanged != null:
return lspHealthChanged(_that.health);case BridgeProductEventPayload_SessionTaskChanged() when sessionTaskChanged != null:
return sessionTaskChanged(_that.sessionId,_that.task);case BridgeProductEventPayload_AgentDirectoryChanged() when agentDirectoryChanged != null:
return agentDirectoryChanged(_that.rootSessionId,_that.agent);case BridgeProductEventPayload_Stale() when stale != null:
return stale(_that.laggedEvents);case _:
  return null;

}
}

}

/// @nodoc


class BridgeProductEventPayload_SessionListChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_SessionListChanged({required this.projectId, required final  List<SessionDto> sessions}): _sessions = sessions,super._();


 final  String projectId;
 final  List<SessionDto> _sessions;
 List<SessionDto> get sessions {
  if (_sessions is EqualUnmodifiableListView) return _sessions;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_sessions);
}


/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_SessionListChangedCopyWith<BridgeProductEventPayload_SessionListChanged> get copyWith => _$BridgeProductEventPayload_SessionListChangedCopyWithImpl<BridgeProductEventPayload_SessionListChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_SessionListChanged&&(identical(other.projectId, projectId) || other.projectId == projectId)&&const DeepCollectionEquality().equals(other._sessions, _sessions));
}


@override
int get hashCode => Object.hash(runtimeType,projectId,const DeepCollectionEquality().hash(_sessions));

@override
String toString() {
  return 'BridgeProductEventPayload.sessionListChanged(projectId: $projectId, sessions: $sessions)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_SessionListChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_SessionListChangedCopyWith(BridgeProductEventPayload_SessionListChanged value, $Res Function(BridgeProductEventPayload_SessionListChanged) _then) = _$BridgeProductEventPayload_SessionListChangedCopyWithImpl;
@useResult
$Res call({
 String projectId, List<SessionDto> sessions
});




}
/// @nodoc
class _$BridgeProductEventPayload_SessionListChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_SessionListChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_SessionListChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_SessionListChanged _self;
  final $Res Function(BridgeProductEventPayload_SessionListChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? projectId = null,Object? sessions = null,}) {
  return _then(BridgeProductEventPayload_SessionListChanged(
projectId: null == projectId ? _self.projectId : projectId // ignore: cast_nullable_to_non_nullable
as String,sessions: null == sessions ? _self._sessions : sessions // ignore: cast_nullable_to_non_nullable
as List<SessionDto>,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_McpHealthChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_McpHealthChanged({required this.health}): super._();


 final  BridgeMcpHealthDto health;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_McpHealthChangedCopyWith<BridgeProductEventPayload_McpHealthChanged> get copyWith => _$BridgeProductEventPayload_McpHealthChangedCopyWithImpl<BridgeProductEventPayload_McpHealthChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_McpHealthChanged&&(identical(other.health, health) || other.health == health));
}


@override
int get hashCode => Object.hash(runtimeType,health);

@override
String toString() {
  return 'BridgeProductEventPayload.mcpHealthChanged(health: $health)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_McpHealthChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_McpHealthChangedCopyWith(BridgeProductEventPayload_McpHealthChanged value, $Res Function(BridgeProductEventPayload_McpHealthChanged) _then) = _$BridgeProductEventPayload_McpHealthChangedCopyWithImpl;
@useResult
$Res call({
 BridgeMcpHealthDto health
});




}
/// @nodoc
class _$BridgeProductEventPayload_McpHealthChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_McpHealthChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_McpHealthChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_McpHealthChanged _self;
  final $Res Function(BridgeProductEventPayload_McpHealthChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? health = null,}) {
  return _then(BridgeProductEventPayload_McpHealthChanged(
health: null == health ? _self.health : health // ignore: cast_nullable_to_non_nullable
as BridgeMcpHealthDto,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_LspHealthChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_LspHealthChanged({required this.health}): super._();


 final  BridgeLspHealthDto health;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_LspHealthChangedCopyWith<BridgeProductEventPayload_LspHealthChanged> get copyWith => _$BridgeProductEventPayload_LspHealthChangedCopyWithImpl<BridgeProductEventPayload_LspHealthChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_LspHealthChanged&&(identical(other.health, health) || other.health == health));
}


@override
int get hashCode => Object.hash(runtimeType,health);

@override
String toString() {
  return 'BridgeProductEventPayload.lspHealthChanged(health: $health)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_LspHealthChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_LspHealthChangedCopyWith(BridgeProductEventPayload_LspHealthChanged value, $Res Function(BridgeProductEventPayload_LspHealthChanged) _then) = _$BridgeProductEventPayload_LspHealthChangedCopyWithImpl;
@useResult
$Res call({
 BridgeLspHealthDto health
});




}
/// @nodoc
class _$BridgeProductEventPayload_LspHealthChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_LspHealthChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_LspHealthChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_LspHealthChanged _self;
  final $Res Function(BridgeProductEventPayload_LspHealthChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? health = null,}) {
  return _then(BridgeProductEventPayload_LspHealthChanged(
health: null == health ? _self.health : health // ignore: cast_nullable_to_non_nullable
as BridgeLspHealthDto,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_SessionTaskChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_SessionTaskChanged({required this.sessionId, this.task}): super._();


 final  String sessionId;
 final  BridgeTaskRuntimeDto? task;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_SessionTaskChangedCopyWith<BridgeProductEventPayload_SessionTaskChanged> get copyWith => _$BridgeProductEventPayload_SessionTaskChangedCopyWithImpl<BridgeProductEventPayload_SessionTaskChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_SessionTaskChanged&&(identical(other.sessionId, sessionId) || other.sessionId == sessionId)&&(identical(other.task, task) || other.task == task));
}


@override
int get hashCode => Object.hash(runtimeType,sessionId,task);

@override
String toString() {
  return 'BridgeProductEventPayload.sessionTaskChanged(sessionId: $sessionId, task: $task)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_SessionTaskChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_SessionTaskChangedCopyWith(BridgeProductEventPayload_SessionTaskChanged value, $Res Function(BridgeProductEventPayload_SessionTaskChanged) _then) = _$BridgeProductEventPayload_SessionTaskChangedCopyWithImpl;
@useResult
$Res call({
 String sessionId, BridgeTaskRuntimeDto? task
});




}
/// @nodoc
class _$BridgeProductEventPayload_SessionTaskChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_SessionTaskChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_SessionTaskChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_SessionTaskChanged _self;
  final $Res Function(BridgeProductEventPayload_SessionTaskChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? sessionId = null,Object? task = freezed,}) {
  return _then(BridgeProductEventPayload_SessionTaskChanged(
sessionId: null == sessionId ? _self.sessionId : sessionId // ignore: cast_nullable_to_non_nullable
as String,task: freezed == task ? _self.task : task // ignore: cast_nullable_to_non_nullable
as BridgeTaskRuntimeDto?,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_AgentDirectoryChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_AgentDirectoryChanged({required this.rootSessionId, required this.agent}): super._();


 final  String rootSessionId;
 final  BridgeAgentDirectoryEntryDto agent;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_AgentDirectoryChangedCopyWith<BridgeProductEventPayload_AgentDirectoryChanged> get copyWith => _$BridgeProductEventPayload_AgentDirectoryChangedCopyWithImpl<BridgeProductEventPayload_AgentDirectoryChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_AgentDirectoryChanged&&(identical(other.rootSessionId, rootSessionId) || other.rootSessionId == rootSessionId)&&(identical(other.agent, agent) || other.agent == agent));
}


@override
int get hashCode => Object.hash(runtimeType,rootSessionId,agent);

@override
String toString() {
  return 'BridgeProductEventPayload.agentDirectoryChanged(rootSessionId: $rootSessionId, agent: $agent)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_AgentDirectoryChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_AgentDirectoryChangedCopyWith(BridgeProductEventPayload_AgentDirectoryChanged value, $Res Function(BridgeProductEventPayload_AgentDirectoryChanged) _then) = _$BridgeProductEventPayload_AgentDirectoryChangedCopyWithImpl;
@useResult
$Res call({
 String rootSessionId, BridgeAgentDirectoryEntryDto agent
});




}
/// @nodoc
class _$BridgeProductEventPayload_AgentDirectoryChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_AgentDirectoryChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_AgentDirectoryChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_AgentDirectoryChanged _self;
  final $Res Function(BridgeProductEventPayload_AgentDirectoryChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? rootSessionId = null,Object? agent = null,}) {
  return _then(BridgeProductEventPayload_AgentDirectoryChanged(
rootSessionId: null == rootSessionId ? _self.rootSessionId : rootSessionId // ignore: cast_nullable_to_non_nullable
as String,agent: null == agent ? _self.agent : agent // ignore: cast_nullable_to_non_nullable
as BridgeAgentDirectoryEntryDto,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_Stale extends BridgeProductEventPayload {
  const BridgeProductEventPayload_Stale({required this.laggedEvents}): super._();


 final  BigInt laggedEvents;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_StaleCopyWith<BridgeProductEventPayload_Stale> get copyWith => _$BridgeProductEventPayload_StaleCopyWithImpl<BridgeProductEventPayload_Stale>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_Stale&&(identical(other.laggedEvents, laggedEvents) || other.laggedEvents == laggedEvents));
}


@override
int get hashCode => Object.hash(runtimeType,laggedEvents);

@override
String toString() {
  return 'BridgeProductEventPayload.stale(laggedEvents: $laggedEvents)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_StaleCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_StaleCopyWith(BridgeProductEventPayload_Stale value, $Res Function(BridgeProductEventPayload_Stale) _then) = _$BridgeProductEventPayload_StaleCopyWithImpl;
@useResult
$Res call({
 BigInt laggedEvents
});




}
/// @nodoc
class _$BridgeProductEventPayload_StaleCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_StaleCopyWith<$Res> {
  _$BridgeProductEventPayload_StaleCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_Stale _self;
  final $Res Function(BridgeProductEventPayload_Stale) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? laggedEvents = null,}) {
  return _then(BridgeProductEventPayload_Stale(
laggedEvents: null == laggedEvents ? _self.laggedEvents : laggedEvents // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

// dart format on

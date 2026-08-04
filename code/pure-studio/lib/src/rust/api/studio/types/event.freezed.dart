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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeProductEventPayload_ThreadDirectoryChanged value)?  threadDirectoryChanged,TResult Function( BridgeProductEventPayload_McpHealthChanged value)?  mcpHealthChanged,TResult Function( BridgeProductEventPayload_LspHealthChanged value)?  lspHealthChanged,TResult Function( BridgeProductEventPayload_TaskChanged value)?  taskChanged,TResult Function( BridgeProductEventPayload_AgentDirectoryChanged value)?  agentDirectoryChanged,TResult Function( BridgeProductEventPayload_Stale value)?  stale,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeProductEventPayload_ThreadDirectoryChanged() when threadDirectoryChanged != null:
return threadDirectoryChanged(_that);case BridgeProductEventPayload_McpHealthChanged() when mcpHealthChanged != null:
return mcpHealthChanged(_that);case BridgeProductEventPayload_LspHealthChanged() when lspHealthChanged != null:
return lspHealthChanged(_that);case BridgeProductEventPayload_TaskChanged() when taskChanged != null:
return taskChanged(_that);case BridgeProductEventPayload_AgentDirectoryChanged() when agentDirectoryChanged != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeProductEventPayload_ThreadDirectoryChanged value)  threadDirectoryChanged,required TResult Function( BridgeProductEventPayload_McpHealthChanged value)  mcpHealthChanged,required TResult Function( BridgeProductEventPayload_LspHealthChanged value)  lspHealthChanged,required TResult Function( BridgeProductEventPayload_TaskChanged value)  taskChanged,required TResult Function( BridgeProductEventPayload_AgentDirectoryChanged value)  agentDirectoryChanged,required TResult Function( BridgeProductEventPayload_Stale value)  stale,}){
final _that = this;
switch (_that) {
case BridgeProductEventPayload_ThreadDirectoryChanged():
return threadDirectoryChanged(_that);case BridgeProductEventPayload_McpHealthChanged():
return mcpHealthChanged(_that);case BridgeProductEventPayload_LspHealthChanged():
return lspHealthChanged(_that);case BridgeProductEventPayload_TaskChanged():
return taskChanged(_that);case BridgeProductEventPayload_AgentDirectoryChanged():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeProductEventPayload_ThreadDirectoryChanged value)?  threadDirectoryChanged,TResult? Function( BridgeProductEventPayload_McpHealthChanged value)?  mcpHealthChanged,TResult? Function( BridgeProductEventPayload_LspHealthChanged value)?  lspHealthChanged,TResult? Function( BridgeProductEventPayload_TaskChanged value)?  taskChanged,TResult? Function( BridgeProductEventPayload_AgentDirectoryChanged value)?  agentDirectoryChanged,TResult? Function( BridgeProductEventPayload_Stale value)?  stale,}){
final _that = this;
switch (_that) {
case BridgeProductEventPayload_ThreadDirectoryChanged() when threadDirectoryChanged != null:
return threadDirectoryChanged(_that);case BridgeProductEventPayload_McpHealthChanged() when mcpHealthChanged != null:
return mcpHealthChanged(_that);case BridgeProductEventPayload_LspHealthChanged() when lspHealthChanged != null:
return lspHealthChanged(_that);case BridgeProductEventPayload_TaskChanged() when taskChanged != null:
return taskChanged(_that);case BridgeProductEventPayload_AgentDirectoryChanged() when agentDirectoryChanged != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String projectId,  List<BridgeThread> threads)?  threadDirectoryChanged,TResult Function( BridgeMcpHealthDto health)?  mcpHealthChanged,TResult Function( BridgeLspHealthDto health)?  lspHealthChanged,TResult Function( String rootThreadId,  BridgeTaskRuntimeDto? task)?  taskChanged,TResult Function( String rootThreadId,  BridgeAgentDirectoryEntryDto agent)?  agentDirectoryChanged,TResult Function( BigInt laggedEvents)?  stale,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeProductEventPayload_ThreadDirectoryChanged() when threadDirectoryChanged != null:
return threadDirectoryChanged(_that.projectId,_that.threads);case BridgeProductEventPayload_McpHealthChanged() when mcpHealthChanged != null:
return mcpHealthChanged(_that.health);case BridgeProductEventPayload_LspHealthChanged() when lspHealthChanged != null:
return lspHealthChanged(_that.health);case BridgeProductEventPayload_TaskChanged() when taskChanged != null:
return taskChanged(_that.rootThreadId,_that.task);case BridgeProductEventPayload_AgentDirectoryChanged() when agentDirectoryChanged != null:
return agentDirectoryChanged(_that.rootThreadId,_that.agent);case BridgeProductEventPayload_Stale() when stale != null:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String projectId,  List<BridgeThread> threads)  threadDirectoryChanged,required TResult Function( BridgeMcpHealthDto health)  mcpHealthChanged,required TResult Function( BridgeLspHealthDto health)  lspHealthChanged,required TResult Function( String rootThreadId,  BridgeTaskRuntimeDto? task)  taskChanged,required TResult Function( String rootThreadId,  BridgeAgentDirectoryEntryDto agent)  agentDirectoryChanged,required TResult Function( BigInt laggedEvents)  stale,}) {final _that = this;
switch (_that) {
case BridgeProductEventPayload_ThreadDirectoryChanged():
return threadDirectoryChanged(_that.projectId,_that.threads);case BridgeProductEventPayload_McpHealthChanged():
return mcpHealthChanged(_that.health);case BridgeProductEventPayload_LspHealthChanged():
return lspHealthChanged(_that.health);case BridgeProductEventPayload_TaskChanged():
return taskChanged(_that.rootThreadId,_that.task);case BridgeProductEventPayload_AgentDirectoryChanged():
return agentDirectoryChanged(_that.rootThreadId,_that.agent);case BridgeProductEventPayload_Stale():
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String projectId,  List<BridgeThread> threads)?  threadDirectoryChanged,TResult? Function( BridgeMcpHealthDto health)?  mcpHealthChanged,TResult? Function( BridgeLspHealthDto health)?  lspHealthChanged,TResult? Function( String rootThreadId,  BridgeTaskRuntimeDto? task)?  taskChanged,TResult? Function( String rootThreadId,  BridgeAgentDirectoryEntryDto agent)?  agentDirectoryChanged,TResult? Function( BigInt laggedEvents)?  stale,}) {final _that = this;
switch (_that) {
case BridgeProductEventPayload_ThreadDirectoryChanged() when threadDirectoryChanged != null:
return threadDirectoryChanged(_that.projectId,_that.threads);case BridgeProductEventPayload_McpHealthChanged() when mcpHealthChanged != null:
return mcpHealthChanged(_that.health);case BridgeProductEventPayload_LspHealthChanged() when lspHealthChanged != null:
return lspHealthChanged(_that.health);case BridgeProductEventPayload_TaskChanged() when taskChanged != null:
return taskChanged(_that.rootThreadId,_that.task);case BridgeProductEventPayload_AgentDirectoryChanged() when agentDirectoryChanged != null:
return agentDirectoryChanged(_that.rootThreadId,_that.agent);case BridgeProductEventPayload_Stale() when stale != null:
return stale(_that.laggedEvents);case _:
  return null;

}
}

}

/// @nodoc


class BridgeProductEventPayload_ThreadDirectoryChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_ThreadDirectoryChanged({required this.projectId, required final  List<BridgeThread> threads}): _threads = threads,super._();


 final  String projectId;
 final  List<BridgeThread> _threads;
 List<BridgeThread> get threads {
  if (_threads is EqualUnmodifiableListView) return _threads;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_threads);
}


/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_ThreadDirectoryChangedCopyWith<BridgeProductEventPayload_ThreadDirectoryChanged> get copyWith => _$BridgeProductEventPayload_ThreadDirectoryChangedCopyWithImpl<BridgeProductEventPayload_ThreadDirectoryChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_ThreadDirectoryChanged&&(identical(other.projectId, projectId) || other.projectId == projectId)&&const DeepCollectionEquality().equals(other._threads, _threads));
}


@override
int get hashCode => Object.hash(runtimeType,projectId,const DeepCollectionEquality().hash(_threads));

@override
String toString() {
  return 'BridgeProductEventPayload.threadDirectoryChanged(projectId: $projectId, threads: $threads)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_ThreadDirectoryChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_ThreadDirectoryChangedCopyWith(BridgeProductEventPayload_ThreadDirectoryChanged value, $Res Function(BridgeProductEventPayload_ThreadDirectoryChanged) _then) = _$BridgeProductEventPayload_ThreadDirectoryChangedCopyWithImpl;
@useResult
$Res call({
 String projectId, List<BridgeThread> threads
});




}
/// @nodoc
class _$BridgeProductEventPayload_ThreadDirectoryChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_ThreadDirectoryChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_ThreadDirectoryChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_ThreadDirectoryChanged _self;
  final $Res Function(BridgeProductEventPayload_ThreadDirectoryChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? projectId = null,Object? threads = null,}) {
  return _then(BridgeProductEventPayload_ThreadDirectoryChanged(
projectId: null == projectId ? _self.projectId : projectId // ignore: cast_nullable_to_non_nullable
as String,threads: null == threads ? _self._threads : threads // ignore: cast_nullable_to_non_nullable
as List<BridgeThread>,
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


class BridgeProductEventPayload_TaskChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_TaskChanged({required this.rootThreadId, this.task}): super._();


 final  String rootThreadId;
 final  BridgeTaskRuntimeDto? task;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_TaskChangedCopyWith<BridgeProductEventPayload_TaskChanged> get copyWith => _$BridgeProductEventPayload_TaskChangedCopyWithImpl<BridgeProductEventPayload_TaskChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_TaskChanged&&(identical(other.rootThreadId, rootThreadId) || other.rootThreadId == rootThreadId)&&(identical(other.task, task) || other.task == task));
}


@override
int get hashCode => Object.hash(runtimeType,rootThreadId,task);

@override
String toString() {
  return 'BridgeProductEventPayload.taskChanged(rootThreadId: $rootThreadId, task: $task)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_TaskChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_TaskChangedCopyWith(BridgeProductEventPayload_TaskChanged value, $Res Function(BridgeProductEventPayload_TaskChanged) _then) = _$BridgeProductEventPayload_TaskChangedCopyWithImpl;
@useResult
$Res call({
 String rootThreadId, BridgeTaskRuntimeDto? task
});




}
/// @nodoc
class _$BridgeProductEventPayload_TaskChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_TaskChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_TaskChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_TaskChanged _self;
  final $Res Function(BridgeProductEventPayload_TaskChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? rootThreadId = null,Object? task = freezed,}) {
  return _then(BridgeProductEventPayload_TaskChanged(
rootThreadId: null == rootThreadId ? _self.rootThreadId : rootThreadId // ignore: cast_nullable_to_non_nullable
as String,task: freezed == task ? _self.task : task // ignore: cast_nullable_to_non_nullable
as BridgeTaskRuntimeDto?,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_AgentDirectoryChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_AgentDirectoryChanged({required this.rootThreadId, required this.agent}): super._();


 final  String rootThreadId;
 final  BridgeAgentDirectoryEntryDto agent;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_AgentDirectoryChangedCopyWith<BridgeProductEventPayload_AgentDirectoryChanged> get copyWith => _$BridgeProductEventPayload_AgentDirectoryChangedCopyWithImpl<BridgeProductEventPayload_AgentDirectoryChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_AgentDirectoryChanged&&(identical(other.rootThreadId, rootThreadId) || other.rootThreadId == rootThreadId)&&(identical(other.agent, agent) || other.agent == agent));
}


@override
int get hashCode => Object.hash(runtimeType,rootThreadId,agent);

@override
String toString() {
  return 'BridgeProductEventPayload.agentDirectoryChanged(rootThreadId: $rootThreadId, agent: $agent)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_AgentDirectoryChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_AgentDirectoryChangedCopyWith(BridgeProductEventPayload_AgentDirectoryChanged value, $Res Function(BridgeProductEventPayload_AgentDirectoryChanged) _then) = _$BridgeProductEventPayload_AgentDirectoryChangedCopyWithImpl;
@useResult
$Res call({
 String rootThreadId, BridgeAgentDirectoryEntryDto agent
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
@pragma('vm:prefer-inline') $Res call({Object? rootThreadId = null,Object? agent = null,}) {
  return _then(BridgeProductEventPayload_AgentDirectoryChanged(
rootThreadId: null == rootThreadId ? _self.rootThreadId : rootThreadId // ignore: cast_nullable_to_non_nullable
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

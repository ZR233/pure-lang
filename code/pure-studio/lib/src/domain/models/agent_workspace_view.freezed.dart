// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'agent_workspace_view.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$AgentWorkspaceView {

 StudioThread get thread; StudioThread get rootThread; AgentWorkspaceSyncState get syncState; List<TimelineRow> get timelineRows; TimelineTodoListUpdate? get todo; ThreadRuntimeView get runtime; StudioTurnView? get turn; PendingInteraction? get activeInteraction; ComposerThreadState get composer; AgentComposerMode get composerMode; PermissionMode get permissionMode; List<ProviderSettingsView> get providers; List<RoleSettingsView> get roles; List<StudioAgentView> get agents;
/// Create a copy of AgentWorkspaceView
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$AgentWorkspaceViewCopyWith<AgentWorkspaceView> get copyWith => _$AgentWorkspaceViewCopyWithImpl<AgentWorkspaceView>(this as AgentWorkspaceView, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is AgentWorkspaceView&&(identical(other.thread, thread) || other.thread == thread)&&(identical(other.rootThread, rootThread) || other.rootThread == rootThread)&&(identical(other.syncState, syncState) || other.syncState == syncState)&&const DeepCollectionEquality().equals(other.timelineRows, timelineRows)&&(identical(other.todo, todo) || other.todo == todo)&&(identical(other.runtime, runtime) || other.runtime == runtime)&&(identical(other.turn, turn) || other.turn == turn)&&(identical(other.activeInteraction, activeInteraction) || other.activeInteraction == activeInteraction)&&(identical(other.composer, composer) || other.composer == composer)&&(identical(other.composerMode, composerMode) || other.composerMode == composerMode)&&(identical(other.permissionMode, permissionMode) || other.permissionMode == permissionMode)&&const DeepCollectionEquality().equals(other.providers, providers)&&const DeepCollectionEquality().equals(other.roles, roles)&&const DeepCollectionEquality().equals(other.agents, agents));
}


@override
int get hashCode => Object.hash(runtimeType,thread,rootThread,syncState,const DeepCollectionEquality().hash(timelineRows),todo,runtime,turn,activeInteraction,composer,composerMode,permissionMode,const DeepCollectionEquality().hash(providers),const DeepCollectionEquality().hash(roles),const DeepCollectionEquality().hash(agents));

@override
String toString() {
  return 'AgentWorkspaceView(thread: $thread, rootThread: $rootThread, syncState: $syncState, timelineRows: $timelineRows, todo: $todo, runtime: $runtime, turn: $turn, activeInteraction: $activeInteraction, composer: $composer, composerMode: $composerMode, permissionMode: $permissionMode, providers: $providers, roles: $roles, agents: $agents)';
}


}

/// @nodoc
abstract mixin class $AgentWorkspaceViewCopyWith<$Res>  {
  factory $AgentWorkspaceViewCopyWith(AgentWorkspaceView value, $Res Function(AgentWorkspaceView) _then) = _$AgentWorkspaceViewCopyWithImpl;
@useResult
$Res call({
 StudioThread thread, StudioThread rootThread, AgentWorkspaceSyncState syncState, List<TimelineRow> timelineRows, TimelineTodoListUpdate? todo, ThreadRuntimeView runtime, StudioTurnView? turn, PendingInteraction? activeInteraction, ComposerThreadState composer, AgentComposerMode composerMode, PermissionMode permissionMode, List<ProviderSettingsView> providers, List<RoleSettingsView> roles, List<StudioAgentView> agents
});




}
/// @nodoc
class _$AgentWorkspaceViewCopyWithImpl<$Res>
    implements $AgentWorkspaceViewCopyWith<$Res> {
  _$AgentWorkspaceViewCopyWithImpl(this._self, this._then);

  final AgentWorkspaceView _self;
  final $Res Function(AgentWorkspaceView) _then;

/// Create a copy of AgentWorkspaceView
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? thread = null,Object? rootThread = null,Object? syncState = null,Object? timelineRows = null,Object? todo = freezed,Object? runtime = null,Object? turn = freezed,Object? activeInteraction = freezed,Object? composer = null,Object? composerMode = null,Object? permissionMode = null,Object? providers = null,Object? roles = null,Object? agents = null,}) {
  return _then(AgentWorkspaceView(
thread: null == thread ? _self.thread : thread // ignore: cast_nullable_to_non_nullable
as StudioThread,rootThread: null == rootThread ? _self.rootThread : rootThread // ignore: cast_nullable_to_non_nullable
as StudioThread,syncState: null == syncState ? _self.syncState : syncState // ignore: cast_nullable_to_non_nullable
as AgentWorkspaceSyncState,timelineRows: null == timelineRows ? _self.timelineRows : timelineRows // ignore: cast_nullable_to_non_nullable
as List<TimelineRow>,todo: freezed == todo ? _self.todo : todo // ignore: cast_nullable_to_non_nullable
as TimelineTodoListUpdate?,runtime: null == runtime ? _self.runtime : runtime // ignore: cast_nullable_to_non_nullable
as ThreadRuntimeView,turn: freezed == turn ? _self.turn : turn // ignore: cast_nullable_to_non_nullable
as StudioTurnView?,activeInteraction: freezed == activeInteraction ? _self.activeInteraction : activeInteraction // ignore: cast_nullable_to_non_nullable
as PendingInteraction?,composer: null == composer ? _self.composer : composer // ignore: cast_nullable_to_non_nullable
as ComposerThreadState,composerMode: null == composerMode ? _self.composerMode : composerMode // ignore: cast_nullable_to_non_nullable
as AgentComposerMode,permissionMode: null == permissionMode ? _self.permissionMode : permissionMode // ignore: cast_nullable_to_non_nullable
as PermissionMode,providers: null == providers ? _self.providers : providers // ignore: cast_nullable_to_non_nullable
as List<ProviderSettingsView>,roles: null == roles ? _self.roles : roles // ignore: cast_nullable_to_non_nullable
as List<RoleSettingsView>,agents: null == agents ? _self.agents : agents // ignore: cast_nullable_to_non_nullable
as List<StudioAgentView>,
  ));
}

}


/// Adds pattern-matching-related methods to [AgentWorkspaceView].
extension AgentWorkspaceViewPatterns on AgentWorkspaceView {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>(TResult Function( _AgentWorkspaceView value)?  $default,{required TResult orElse(),}){
final _that = this;
switch (_that) {
case _AgentWorkspaceView() when $default != null:
return $default(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>(TResult Function( _AgentWorkspaceView value)  $default,){
final _that = this;
switch (_that) {
case _AgentWorkspaceView():
return $default(_that);case _:
  throw StateError('Unexpected subclass');

}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>(TResult? Function( _AgentWorkspaceView value)?  $default,){
final _that = this;
switch (_that) {
case _AgentWorkspaceView() when $default != null:
return $default(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>(TResult Function( StudioThread thread,  StudioThread rootThread,  AgentWorkspaceSyncState syncState,  List<TimelineRow> timelineRows,  TimelineTodoListUpdate? todo,  ThreadRuntimeView runtime,  StudioTurnView? turn,  PendingInteraction? activeInteraction,  ComposerThreadState composer,  AgentComposerMode composerMode,  PermissionMode permissionMode,  List<ProviderSettingsView> providers,  List<RoleSettingsView> roles,  List<StudioAgentView> agents)?  $default,{required TResult orElse(),}) {final _that = this;
switch (_that) {
case _AgentWorkspaceView() when $default != null:
return $default(_that.thread,_that.rootThread,_that.syncState,_that.timelineRows,_that.todo,_that.runtime,_that.turn,_that.activeInteraction,_that.composer,_that.composerMode,_that.permissionMode,_that.providers,_that.roles,_that.agents);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>(TResult Function( StudioThread thread,  StudioThread rootThread,  AgentWorkspaceSyncState syncState,  List<TimelineRow> timelineRows,  TimelineTodoListUpdate? todo,  ThreadRuntimeView runtime,  StudioTurnView? turn,  PendingInteraction? activeInteraction,  ComposerThreadState composer,  AgentComposerMode composerMode,  PermissionMode permissionMode,  List<ProviderSettingsView> providers,  List<RoleSettingsView> roles,  List<StudioAgentView> agents)  $default,) {final _that = this;
switch (_that) {
case _AgentWorkspaceView():
return $default(_that.thread,_that.rootThread,_that.syncState,_that.timelineRows,_that.todo,_that.runtime,_that.turn,_that.activeInteraction,_that.composer,_that.composerMode,_that.permissionMode,_that.providers,_that.roles,_that.agents);case _:
  throw StateError('Unexpected subclass');

}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>(TResult? Function( StudioThread thread,  StudioThread rootThread,  AgentWorkspaceSyncState syncState,  List<TimelineRow> timelineRows,  TimelineTodoListUpdate? todo,  ThreadRuntimeView runtime,  StudioTurnView? turn,  PendingInteraction? activeInteraction,  ComposerThreadState composer,  AgentComposerMode composerMode,  PermissionMode permissionMode,  List<ProviderSettingsView> providers,  List<RoleSettingsView> roles,  List<StudioAgentView> agents)?  $default,) {final _that = this;
switch (_that) {
case _AgentWorkspaceView() when $default != null:
return $default(_that.thread,_that.rootThread,_that.syncState,_that.timelineRows,_that.todo,_that.runtime,_that.turn,_that.activeInteraction,_that.composer,_that.composerMode,_that.permissionMode,_that.providers,_that.roles,_that.agents);case _:
  return null;

}
}

}

/// @nodoc


class _AgentWorkspaceView extends AgentWorkspaceView {
  const _AgentWorkspaceView({required this.thread, required this.rootThread, required this.syncState, required  List<TimelineRow> timelineRows, required this.todo, required this.runtime, required this.turn, required this.activeInteraction, required this.composer, required this.composerMode, required this.permissionMode, required  List<ProviderSettingsView> providers, required  List<RoleSettingsView> roles, required  List<StudioAgentView> agents}): _timelineRows = timelineRows,_providers = providers,_roles = roles,_agents = agents,super._();


@override final  StudioThread thread;
@override final  StudioThread rootThread;
@override final  AgentWorkspaceSyncState syncState;
 final  List<TimelineRow> _timelineRows;
@override List<TimelineRow> get timelineRows {
  if (_timelineRows is EqualUnmodifiableListView) return _timelineRows;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_timelineRows);
}

@override final  TimelineTodoListUpdate? todo;
@override final  ThreadRuntimeView runtime;
@override final  StudioTurnView? turn;
@override final  PendingInteraction? activeInteraction;
@override final  ComposerThreadState composer;
@override final  AgentComposerMode composerMode;
@override final  PermissionMode permissionMode;
 final  List<ProviderSettingsView> _providers;
@override List<ProviderSettingsView> get providers {
  if (_providers is EqualUnmodifiableListView) return _providers;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_providers);
}

 final  List<RoleSettingsView> _roles;
@override List<RoleSettingsView> get roles {
  if (_roles is EqualUnmodifiableListView) return _roles;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_roles);
}

 final  List<StudioAgentView> _agents;
@override List<StudioAgentView> get agents {
  if (_agents is EqualUnmodifiableListView) return _agents;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_agents);
}


/// Create a copy of AgentWorkspaceView
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
_$AgentWorkspaceViewCopyWith<_AgentWorkspaceView> get copyWith => __$AgentWorkspaceViewCopyWithImpl<_AgentWorkspaceView>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _AgentWorkspaceView&&(identical(other.thread, thread) || other.thread == thread)&&(identical(other.rootThread, rootThread) || other.rootThread == rootThread)&&(identical(other.syncState, syncState) || other.syncState == syncState)&&const DeepCollectionEquality().equals(other._timelineRows, _timelineRows)&&(identical(other.todo, todo) || other.todo == todo)&&(identical(other.runtime, runtime) || other.runtime == runtime)&&(identical(other.turn, turn) || other.turn == turn)&&(identical(other.activeInteraction, activeInteraction) || other.activeInteraction == activeInteraction)&&(identical(other.composer, composer) || other.composer == composer)&&(identical(other.composerMode, composerMode) || other.composerMode == composerMode)&&(identical(other.permissionMode, permissionMode) || other.permissionMode == permissionMode)&&const DeepCollectionEquality().equals(other._providers, _providers)&&const DeepCollectionEquality().equals(other._roles, _roles)&&const DeepCollectionEquality().equals(other._agents, _agents));
}


@override
int get hashCode => Object.hash(runtimeType,thread,rootThread,syncState,const DeepCollectionEquality().hash(_timelineRows),todo,runtime,turn,activeInteraction,composer,composerMode,permissionMode,const DeepCollectionEquality().hash(_providers),const DeepCollectionEquality().hash(_roles),const DeepCollectionEquality().hash(_agents));

@override
String toString() {
  return 'AgentWorkspaceView(thread: $thread, rootThread: $rootThread, syncState: $syncState, timelineRows: $timelineRows, todo: $todo, runtime: $runtime, turn: $turn, activeInteraction: $activeInteraction, composer: $composer, composerMode: $composerMode, permissionMode: $permissionMode, providers: $providers, roles: $roles, agents: $agents)';
}


}

/// @nodoc
abstract mixin class _$AgentWorkspaceViewCopyWith<$Res> implements $AgentWorkspaceViewCopyWith<$Res> {
  factory _$AgentWorkspaceViewCopyWith(_AgentWorkspaceView value, $Res Function(_AgentWorkspaceView) _then) = __$AgentWorkspaceViewCopyWithImpl;
@override @useResult
$Res call({
 StudioThread thread, StudioThread rootThread, AgentWorkspaceSyncState syncState, List<TimelineRow> timelineRows, TimelineTodoListUpdate? todo, ThreadRuntimeView runtime, StudioTurnView? turn, PendingInteraction? activeInteraction, ComposerThreadState composer, AgentComposerMode composerMode, PermissionMode permissionMode, List<ProviderSettingsView> providers, List<RoleSettingsView> roles, List<StudioAgentView> agents
});




}
/// @nodoc
class __$AgentWorkspaceViewCopyWithImpl<$Res>
    implements _$AgentWorkspaceViewCopyWith<$Res> {
  __$AgentWorkspaceViewCopyWithImpl(this._self, this._then);

  final _AgentWorkspaceView _self;
  final $Res Function(_AgentWorkspaceView) _then;

/// Create a copy of AgentWorkspaceView
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? thread = null,Object? rootThread = null,Object? syncState = null,Object? timelineRows = null,Object? todo = freezed,Object? runtime = null,Object? turn = freezed,Object? activeInteraction = freezed,Object? composer = null,Object? composerMode = null,Object? permissionMode = null,Object? providers = null,Object? roles = null,Object? agents = null,}) {
  return _then(_AgentWorkspaceView(
thread: null == thread ? _self.thread : thread // ignore: cast_nullable_to_non_nullable
as StudioThread,rootThread: null == rootThread ? _self.rootThread : rootThread // ignore: cast_nullable_to_non_nullable
as StudioThread,syncState: null == syncState ? _self.syncState : syncState // ignore: cast_nullable_to_non_nullable
as AgentWorkspaceSyncState,timelineRows: null == timelineRows ? _self._timelineRows : timelineRows // ignore: cast_nullable_to_non_nullable
as List<TimelineRow>,todo: freezed == todo ? _self.todo : todo // ignore: cast_nullable_to_non_nullable
as TimelineTodoListUpdate?,runtime: null == runtime ? _self.runtime : runtime // ignore: cast_nullable_to_non_nullable
as ThreadRuntimeView,turn: freezed == turn ? _self.turn : turn // ignore: cast_nullable_to_non_nullable
as StudioTurnView?,activeInteraction: freezed == activeInteraction ? _self.activeInteraction : activeInteraction // ignore: cast_nullable_to_non_nullable
as PendingInteraction?,composer: null == composer ? _self.composer : composer // ignore: cast_nullable_to_non_nullable
as ComposerThreadState,composerMode: null == composerMode ? _self.composerMode : composerMode // ignore: cast_nullable_to_non_nullable
as AgentComposerMode,permissionMode: null == permissionMode ? _self.permissionMode : permissionMode // ignore: cast_nullable_to_non_nullable
as PermissionMode,providers: null == providers ? _self._providers : providers // ignore: cast_nullable_to_non_nullable
as List<ProviderSettingsView>,roles: null == roles ? _self._roles : roles // ignore: cast_nullable_to_non_nullable
as List<RoleSettingsView>,agents: null == agents ? _self._agents : agents // ignore: cast_nullable_to_non_nullable
as List<StudioAgentView>,
  ));
}


}

// dart format on

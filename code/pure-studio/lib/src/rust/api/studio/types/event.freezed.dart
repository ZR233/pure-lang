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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeProductEventPayload_ProjectDirectoryChanged value)?  projectDirectoryChanged,TResult Function( BridgeProductEventPayload_ThreadDirectoryChanged value)?  threadDirectoryChanged,TResult Function( BridgeProductEventPayload_TaskDirectoryChanged value)?  taskDirectoryChanged,TResult Function( BridgeProductEventPayload_AgentDirectoryChanged value)?  agentDirectoryChanged,TResult Function( BridgeProductEventPayload_SettingsStateChanged value)?  settingsStateChanged,TResult Function( BridgeProductEventPayload_RecoveryStateChanged value)?  recoveryStateChanged,TResult Function( BridgeProductEventPayload_McpStateChanged value)?  mcpStateChanged,TResult Function( BridgeProductEventPayload_LspStateChanged value)?  lspStateChanged,TResult Function( BridgeProductEventPayload_SkillsStateChanged value)?  skillsStateChanged,TResult Function( BridgeProductEventPayload_ProviderUsageStateChanged value)?  providerUsageStateChanged,TResult Function( BridgeProductEventPayload_UpdaterStateChanged value)?  updaterStateChanged,TResult Function( BridgeProductEventPayload_Stale value)?  stale,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeProductEventPayload_ProjectDirectoryChanged() when projectDirectoryChanged != null:
return projectDirectoryChanged(_that);case BridgeProductEventPayload_ThreadDirectoryChanged() when threadDirectoryChanged != null:
return threadDirectoryChanged(_that);case BridgeProductEventPayload_TaskDirectoryChanged() when taskDirectoryChanged != null:
return taskDirectoryChanged(_that);case BridgeProductEventPayload_AgentDirectoryChanged() when agentDirectoryChanged != null:
return agentDirectoryChanged(_that);case BridgeProductEventPayload_SettingsStateChanged() when settingsStateChanged != null:
return settingsStateChanged(_that);case BridgeProductEventPayload_RecoveryStateChanged() when recoveryStateChanged != null:
return recoveryStateChanged(_that);case BridgeProductEventPayload_McpStateChanged() when mcpStateChanged != null:
return mcpStateChanged(_that);case BridgeProductEventPayload_LspStateChanged() when lspStateChanged != null:
return lspStateChanged(_that);case BridgeProductEventPayload_SkillsStateChanged() when skillsStateChanged != null:
return skillsStateChanged(_that);case BridgeProductEventPayload_ProviderUsageStateChanged() when providerUsageStateChanged != null:
return providerUsageStateChanged(_that);case BridgeProductEventPayload_UpdaterStateChanged() when updaterStateChanged != null:
return updaterStateChanged(_that);case BridgeProductEventPayload_Stale() when stale != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeProductEventPayload_ProjectDirectoryChanged value)  projectDirectoryChanged,required TResult Function( BridgeProductEventPayload_ThreadDirectoryChanged value)  threadDirectoryChanged,required TResult Function( BridgeProductEventPayload_TaskDirectoryChanged value)  taskDirectoryChanged,required TResult Function( BridgeProductEventPayload_AgentDirectoryChanged value)  agentDirectoryChanged,required TResult Function( BridgeProductEventPayload_SettingsStateChanged value)  settingsStateChanged,required TResult Function( BridgeProductEventPayload_RecoveryStateChanged value)  recoveryStateChanged,required TResult Function( BridgeProductEventPayload_McpStateChanged value)  mcpStateChanged,required TResult Function( BridgeProductEventPayload_LspStateChanged value)  lspStateChanged,required TResult Function( BridgeProductEventPayload_SkillsStateChanged value)  skillsStateChanged,required TResult Function( BridgeProductEventPayload_ProviderUsageStateChanged value)  providerUsageStateChanged,required TResult Function( BridgeProductEventPayload_UpdaterStateChanged value)  updaterStateChanged,required TResult Function( BridgeProductEventPayload_Stale value)  stale,}){
final _that = this;
switch (_that) {
case BridgeProductEventPayload_ProjectDirectoryChanged():
return projectDirectoryChanged(_that);case BridgeProductEventPayload_ThreadDirectoryChanged():
return threadDirectoryChanged(_that);case BridgeProductEventPayload_TaskDirectoryChanged():
return taskDirectoryChanged(_that);case BridgeProductEventPayload_AgentDirectoryChanged():
return agentDirectoryChanged(_that);case BridgeProductEventPayload_SettingsStateChanged():
return settingsStateChanged(_that);case BridgeProductEventPayload_RecoveryStateChanged():
return recoveryStateChanged(_that);case BridgeProductEventPayload_McpStateChanged():
return mcpStateChanged(_that);case BridgeProductEventPayload_LspStateChanged():
return lspStateChanged(_that);case BridgeProductEventPayload_SkillsStateChanged():
return skillsStateChanged(_that);case BridgeProductEventPayload_ProviderUsageStateChanged():
return providerUsageStateChanged(_that);case BridgeProductEventPayload_UpdaterStateChanged():
return updaterStateChanged(_that);case BridgeProductEventPayload_Stale():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeProductEventPayload_ProjectDirectoryChanged value)?  projectDirectoryChanged,TResult? Function( BridgeProductEventPayload_ThreadDirectoryChanged value)?  threadDirectoryChanged,TResult? Function( BridgeProductEventPayload_TaskDirectoryChanged value)?  taskDirectoryChanged,TResult? Function( BridgeProductEventPayload_AgentDirectoryChanged value)?  agentDirectoryChanged,TResult? Function( BridgeProductEventPayload_SettingsStateChanged value)?  settingsStateChanged,TResult? Function( BridgeProductEventPayload_RecoveryStateChanged value)?  recoveryStateChanged,TResult? Function( BridgeProductEventPayload_McpStateChanged value)?  mcpStateChanged,TResult? Function( BridgeProductEventPayload_LspStateChanged value)?  lspStateChanged,TResult? Function( BridgeProductEventPayload_SkillsStateChanged value)?  skillsStateChanged,TResult? Function( BridgeProductEventPayload_ProviderUsageStateChanged value)?  providerUsageStateChanged,TResult? Function( BridgeProductEventPayload_UpdaterStateChanged value)?  updaterStateChanged,TResult? Function( BridgeProductEventPayload_Stale value)?  stale,}){
final _that = this;
switch (_that) {
case BridgeProductEventPayload_ProjectDirectoryChanged() when projectDirectoryChanged != null:
return projectDirectoryChanged(_that);case BridgeProductEventPayload_ThreadDirectoryChanged() when threadDirectoryChanged != null:
return threadDirectoryChanged(_that);case BridgeProductEventPayload_TaskDirectoryChanged() when taskDirectoryChanged != null:
return taskDirectoryChanged(_that);case BridgeProductEventPayload_AgentDirectoryChanged() when agentDirectoryChanged != null:
return agentDirectoryChanged(_that);case BridgeProductEventPayload_SettingsStateChanged() when settingsStateChanged != null:
return settingsStateChanged(_that);case BridgeProductEventPayload_RecoveryStateChanged() when recoveryStateChanged != null:
return recoveryStateChanged(_that);case BridgeProductEventPayload_McpStateChanged() when mcpStateChanged != null:
return mcpStateChanged(_that);case BridgeProductEventPayload_LspStateChanged() when lspStateChanged != null:
return lspStateChanged(_that);case BridgeProductEventPayload_SkillsStateChanged() when skillsStateChanged != null:
return skillsStateChanged(_that);case BridgeProductEventPayload_ProviderUsageStateChanged() when providerUsageStateChanged != null:
return providerUsageStateChanged(_that);case BridgeProductEventPayload_UpdaterStateChanged() when updaterStateChanged != null:
return updaterStateChanged(_that);case BridgeProductEventPayload_Stale() when stale != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeProjectDirectoryState field0)?  projectDirectoryChanged,TResult Function( BridgeThreadDirectoryState field0)?  threadDirectoryChanged,TResult Function( BridgeTaskDirectoryState field0)?  taskDirectoryChanged,TResult Function( BridgeAgentDirectoryState field0)?  agentDirectoryChanged,TResult Function( BridgeSettingsStateSnapshot field0)?  settingsStateChanged,TResult Function( BridgeRecoveryStateSnapshot field0)?  recoveryStateChanged,TResult Function( BridgeMcpStateSnapshot field0)?  mcpStateChanged,TResult Function( BridgeLspStateSnapshot field0)?  lspStateChanged,TResult Function( BridgeSkillsStateSnapshot field0)?  skillsStateChanged,TResult Function( BridgeProviderUsageStateSnapshot field0)?  providerUsageStateChanged,TResult Function( BridgeUpdaterStateSnapshot field0)?  updaterStateChanged,TResult Function( BigInt laggedEvents)?  stale,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeProductEventPayload_ProjectDirectoryChanged() when projectDirectoryChanged != null:
return projectDirectoryChanged(_that.field0);case BridgeProductEventPayload_ThreadDirectoryChanged() when threadDirectoryChanged != null:
return threadDirectoryChanged(_that.field0);case BridgeProductEventPayload_TaskDirectoryChanged() when taskDirectoryChanged != null:
return taskDirectoryChanged(_that.field0);case BridgeProductEventPayload_AgentDirectoryChanged() when agentDirectoryChanged != null:
return agentDirectoryChanged(_that.field0);case BridgeProductEventPayload_SettingsStateChanged() when settingsStateChanged != null:
return settingsStateChanged(_that.field0);case BridgeProductEventPayload_RecoveryStateChanged() when recoveryStateChanged != null:
return recoveryStateChanged(_that.field0);case BridgeProductEventPayload_McpStateChanged() when mcpStateChanged != null:
return mcpStateChanged(_that.field0);case BridgeProductEventPayload_LspStateChanged() when lspStateChanged != null:
return lspStateChanged(_that.field0);case BridgeProductEventPayload_SkillsStateChanged() when skillsStateChanged != null:
return skillsStateChanged(_that.field0);case BridgeProductEventPayload_ProviderUsageStateChanged() when providerUsageStateChanged != null:
return providerUsageStateChanged(_that.field0);case BridgeProductEventPayload_UpdaterStateChanged() when updaterStateChanged != null:
return updaterStateChanged(_that.field0);case BridgeProductEventPayload_Stale() when stale != null:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeProjectDirectoryState field0)  projectDirectoryChanged,required TResult Function( BridgeThreadDirectoryState field0)  threadDirectoryChanged,required TResult Function( BridgeTaskDirectoryState field0)  taskDirectoryChanged,required TResult Function( BridgeAgentDirectoryState field0)  agentDirectoryChanged,required TResult Function( BridgeSettingsStateSnapshot field0)  settingsStateChanged,required TResult Function( BridgeRecoveryStateSnapshot field0)  recoveryStateChanged,required TResult Function( BridgeMcpStateSnapshot field0)  mcpStateChanged,required TResult Function( BridgeLspStateSnapshot field0)  lspStateChanged,required TResult Function( BridgeSkillsStateSnapshot field0)  skillsStateChanged,required TResult Function( BridgeProviderUsageStateSnapshot field0)  providerUsageStateChanged,required TResult Function( BridgeUpdaterStateSnapshot field0)  updaterStateChanged,required TResult Function( BigInt laggedEvents)  stale,}) {final _that = this;
switch (_that) {
case BridgeProductEventPayload_ProjectDirectoryChanged():
return projectDirectoryChanged(_that.field0);case BridgeProductEventPayload_ThreadDirectoryChanged():
return threadDirectoryChanged(_that.field0);case BridgeProductEventPayload_TaskDirectoryChanged():
return taskDirectoryChanged(_that.field0);case BridgeProductEventPayload_AgentDirectoryChanged():
return agentDirectoryChanged(_that.field0);case BridgeProductEventPayload_SettingsStateChanged():
return settingsStateChanged(_that.field0);case BridgeProductEventPayload_RecoveryStateChanged():
return recoveryStateChanged(_that.field0);case BridgeProductEventPayload_McpStateChanged():
return mcpStateChanged(_that.field0);case BridgeProductEventPayload_LspStateChanged():
return lspStateChanged(_that.field0);case BridgeProductEventPayload_SkillsStateChanged():
return skillsStateChanged(_that.field0);case BridgeProductEventPayload_ProviderUsageStateChanged():
return providerUsageStateChanged(_that.field0);case BridgeProductEventPayload_UpdaterStateChanged():
return updaterStateChanged(_that.field0);case BridgeProductEventPayload_Stale():
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeProjectDirectoryState field0)?  projectDirectoryChanged,TResult? Function( BridgeThreadDirectoryState field0)?  threadDirectoryChanged,TResult? Function( BridgeTaskDirectoryState field0)?  taskDirectoryChanged,TResult? Function( BridgeAgentDirectoryState field0)?  agentDirectoryChanged,TResult? Function( BridgeSettingsStateSnapshot field0)?  settingsStateChanged,TResult? Function( BridgeRecoveryStateSnapshot field0)?  recoveryStateChanged,TResult? Function( BridgeMcpStateSnapshot field0)?  mcpStateChanged,TResult? Function( BridgeLspStateSnapshot field0)?  lspStateChanged,TResult? Function( BridgeSkillsStateSnapshot field0)?  skillsStateChanged,TResult? Function( BridgeProviderUsageStateSnapshot field0)?  providerUsageStateChanged,TResult? Function( BridgeUpdaterStateSnapshot field0)?  updaterStateChanged,TResult? Function( BigInt laggedEvents)?  stale,}) {final _that = this;
switch (_that) {
case BridgeProductEventPayload_ProjectDirectoryChanged() when projectDirectoryChanged != null:
return projectDirectoryChanged(_that.field0);case BridgeProductEventPayload_ThreadDirectoryChanged() when threadDirectoryChanged != null:
return threadDirectoryChanged(_that.field0);case BridgeProductEventPayload_TaskDirectoryChanged() when taskDirectoryChanged != null:
return taskDirectoryChanged(_that.field0);case BridgeProductEventPayload_AgentDirectoryChanged() when agentDirectoryChanged != null:
return agentDirectoryChanged(_that.field0);case BridgeProductEventPayload_SettingsStateChanged() when settingsStateChanged != null:
return settingsStateChanged(_that.field0);case BridgeProductEventPayload_RecoveryStateChanged() when recoveryStateChanged != null:
return recoveryStateChanged(_that.field0);case BridgeProductEventPayload_McpStateChanged() when mcpStateChanged != null:
return mcpStateChanged(_that.field0);case BridgeProductEventPayload_LspStateChanged() when lspStateChanged != null:
return lspStateChanged(_that.field0);case BridgeProductEventPayload_SkillsStateChanged() when skillsStateChanged != null:
return skillsStateChanged(_that.field0);case BridgeProductEventPayload_ProviderUsageStateChanged() when providerUsageStateChanged != null:
return providerUsageStateChanged(_that.field0);case BridgeProductEventPayload_UpdaterStateChanged() when updaterStateChanged != null:
return updaterStateChanged(_that.field0);case BridgeProductEventPayload_Stale() when stale != null:
return stale(_that.laggedEvents);case _:
  return null;

}
}

}

/// @nodoc


class BridgeProductEventPayload_ProjectDirectoryChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_ProjectDirectoryChanged(this.field0): super._();


 final  BridgeProjectDirectoryState field0;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_ProjectDirectoryChangedCopyWith<BridgeProductEventPayload_ProjectDirectoryChanged> get copyWith => _$BridgeProductEventPayload_ProjectDirectoryChangedCopyWithImpl<BridgeProductEventPayload_ProjectDirectoryChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_ProjectDirectoryChanged&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeProductEventPayload.projectDirectoryChanged(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_ProjectDirectoryChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_ProjectDirectoryChangedCopyWith(BridgeProductEventPayload_ProjectDirectoryChanged value, $Res Function(BridgeProductEventPayload_ProjectDirectoryChanged) _then) = _$BridgeProductEventPayload_ProjectDirectoryChangedCopyWithImpl;
@useResult
$Res call({
 BridgeProjectDirectoryState field0
});




}
/// @nodoc
class _$BridgeProductEventPayload_ProjectDirectoryChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_ProjectDirectoryChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_ProjectDirectoryChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_ProjectDirectoryChanged _self;
  final $Res Function(BridgeProductEventPayload_ProjectDirectoryChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProductEventPayload_ProjectDirectoryChanged(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeProjectDirectoryState,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_ThreadDirectoryChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_ThreadDirectoryChanged(this.field0): super._();


 final  BridgeThreadDirectoryState field0;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_ThreadDirectoryChangedCopyWith<BridgeProductEventPayload_ThreadDirectoryChanged> get copyWith => _$BridgeProductEventPayload_ThreadDirectoryChangedCopyWithImpl<BridgeProductEventPayload_ThreadDirectoryChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_ThreadDirectoryChanged&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeProductEventPayload.threadDirectoryChanged(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_ThreadDirectoryChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_ThreadDirectoryChangedCopyWith(BridgeProductEventPayload_ThreadDirectoryChanged value, $Res Function(BridgeProductEventPayload_ThreadDirectoryChanged) _then) = _$BridgeProductEventPayload_ThreadDirectoryChangedCopyWithImpl;
@useResult
$Res call({
 BridgeThreadDirectoryState field0
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
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProductEventPayload_ThreadDirectoryChanged(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeThreadDirectoryState,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_TaskDirectoryChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_TaskDirectoryChanged(this.field0): super._();


 final  BridgeTaskDirectoryState field0;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_TaskDirectoryChangedCopyWith<BridgeProductEventPayload_TaskDirectoryChanged> get copyWith => _$BridgeProductEventPayload_TaskDirectoryChangedCopyWithImpl<BridgeProductEventPayload_TaskDirectoryChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_TaskDirectoryChanged&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeProductEventPayload.taskDirectoryChanged(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_TaskDirectoryChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_TaskDirectoryChangedCopyWith(BridgeProductEventPayload_TaskDirectoryChanged value, $Res Function(BridgeProductEventPayload_TaskDirectoryChanged) _then) = _$BridgeProductEventPayload_TaskDirectoryChangedCopyWithImpl;
@useResult
$Res call({
 BridgeTaskDirectoryState field0
});




}
/// @nodoc
class _$BridgeProductEventPayload_TaskDirectoryChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_TaskDirectoryChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_TaskDirectoryChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_TaskDirectoryChanged _self;
  final $Res Function(BridgeProductEventPayload_TaskDirectoryChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProductEventPayload_TaskDirectoryChanged(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskDirectoryState,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_AgentDirectoryChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_AgentDirectoryChanged(this.field0): super._();


 final  BridgeAgentDirectoryState field0;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_AgentDirectoryChangedCopyWith<BridgeProductEventPayload_AgentDirectoryChanged> get copyWith => _$BridgeProductEventPayload_AgentDirectoryChangedCopyWithImpl<BridgeProductEventPayload_AgentDirectoryChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_AgentDirectoryChanged&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeProductEventPayload.agentDirectoryChanged(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_AgentDirectoryChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_AgentDirectoryChangedCopyWith(BridgeProductEventPayload_AgentDirectoryChanged value, $Res Function(BridgeProductEventPayload_AgentDirectoryChanged) _then) = _$BridgeProductEventPayload_AgentDirectoryChangedCopyWithImpl;
@useResult
$Res call({
 BridgeAgentDirectoryState field0
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
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProductEventPayload_AgentDirectoryChanged(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeAgentDirectoryState,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_SettingsStateChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_SettingsStateChanged(this.field0): super._();


 final  BridgeSettingsStateSnapshot field0;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_SettingsStateChangedCopyWith<BridgeProductEventPayload_SettingsStateChanged> get copyWith => _$BridgeProductEventPayload_SettingsStateChangedCopyWithImpl<BridgeProductEventPayload_SettingsStateChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_SettingsStateChanged&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeProductEventPayload.settingsStateChanged(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_SettingsStateChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_SettingsStateChangedCopyWith(BridgeProductEventPayload_SettingsStateChanged value, $Res Function(BridgeProductEventPayload_SettingsStateChanged) _then) = _$BridgeProductEventPayload_SettingsStateChangedCopyWithImpl;
@useResult
$Res call({
 BridgeSettingsStateSnapshot field0
});




}
/// @nodoc
class _$BridgeProductEventPayload_SettingsStateChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_SettingsStateChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_SettingsStateChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_SettingsStateChanged _self;
  final $Res Function(BridgeProductEventPayload_SettingsStateChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProductEventPayload_SettingsStateChanged(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeSettingsStateSnapshot,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_RecoveryStateChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_RecoveryStateChanged(this.field0): super._();


 final  BridgeRecoveryStateSnapshot field0;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_RecoveryStateChangedCopyWith<BridgeProductEventPayload_RecoveryStateChanged> get copyWith => _$BridgeProductEventPayload_RecoveryStateChangedCopyWithImpl<BridgeProductEventPayload_RecoveryStateChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_RecoveryStateChanged&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeProductEventPayload.recoveryStateChanged(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_RecoveryStateChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_RecoveryStateChangedCopyWith(BridgeProductEventPayload_RecoveryStateChanged value, $Res Function(BridgeProductEventPayload_RecoveryStateChanged) _then) = _$BridgeProductEventPayload_RecoveryStateChangedCopyWithImpl;
@useResult
$Res call({
 BridgeRecoveryStateSnapshot field0
});




}
/// @nodoc
class _$BridgeProductEventPayload_RecoveryStateChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_RecoveryStateChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_RecoveryStateChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_RecoveryStateChanged _self;
  final $Res Function(BridgeProductEventPayload_RecoveryStateChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProductEventPayload_RecoveryStateChanged(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeRecoveryStateSnapshot,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_McpStateChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_McpStateChanged(this.field0): super._();


 final  BridgeMcpStateSnapshot field0;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_McpStateChangedCopyWith<BridgeProductEventPayload_McpStateChanged> get copyWith => _$BridgeProductEventPayload_McpStateChangedCopyWithImpl<BridgeProductEventPayload_McpStateChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_McpStateChanged&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeProductEventPayload.mcpStateChanged(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_McpStateChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_McpStateChangedCopyWith(BridgeProductEventPayload_McpStateChanged value, $Res Function(BridgeProductEventPayload_McpStateChanged) _then) = _$BridgeProductEventPayload_McpStateChangedCopyWithImpl;
@useResult
$Res call({
 BridgeMcpStateSnapshot field0
});




}
/// @nodoc
class _$BridgeProductEventPayload_McpStateChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_McpStateChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_McpStateChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_McpStateChanged _self;
  final $Res Function(BridgeProductEventPayload_McpStateChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProductEventPayload_McpStateChanged(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeMcpStateSnapshot,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_LspStateChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_LspStateChanged(this.field0): super._();


 final  BridgeLspStateSnapshot field0;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_LspStateChangedCopyWith<BridgeProductEventPayload_LspStateChanged> get copyWith => _$BridgeProductEventPayload_LspStateChangedCopyWithImpl<BridgeProductEventPayload_LspStateChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_LspStateChanged&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeProductEventPayload.lspStateChanged(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_LspStateChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_LspStateChangedCopyWith(BridgeProductEventPayload_LspStateChanged value, $Res Function(BridgeProductEventPayload_LspStateChanged) _then) = _$BridgeProductEventPayload_LspStateChangedCopyWithImpl;
@useResult
$Res call({
 BridgeLspStateSnapshot field0
});




}
/// @nodoc
class _$BridgeProductEventPayload_LspStateChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_LspStateChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_LspStateChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_LspStateChanged _self;
  final $Res Function(BridgeProductEventPayload_LspStateChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProductEventPayload_LspStateChanged(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeLspStateSnapshot,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_SkillsStateChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_SkillsStateChanged(this.field0): super._();


 final  BridgeSkillsStateSnapshot field0;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_SkillsStateChangedCopyWith<BridgeProductEventPayload_SkillsStateChanged> get copyWith => _$BridgeProductEventPayload_SkillsStateChangedCopyWithImpl<BridgeProductEventPayload_SkillsStateChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_SkillsStateChanged&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeProductEventPayload.skillsStateChanged(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_SkillsStateChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_SkillsStateChangedCopyWith(BridgeProductEventPayload_SkillsStateChanged value, $Res Function(BridgeProductEventPayload_SkillsStateChanged) _then) = _$BridgeProductEventPayload_SkillsStateChangedCopyWithImpl;
@useResult
$Res call({
 BridgeSkillsStateSnapshot field0
});




}
/// @nodoc
class _$BridgeProductEventPayload_SkillsStateChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_SkillsStateChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_SkillsStateChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_SkillsStateChanged _self;
  final $Res Function(BridgeProductEventPayload_SkillsStateChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProductEventPayload_SkillsStateChanged(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeSkillsStateSnapshot,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_ProviderUsageStateChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_ProviderUsageStateChanged(this.field0): super._();


 final  BridgeProviderUsageStateSnapshot field0;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_ProviderUsageStateChangedCopyWith<BridgeProductEventPayload_ProviderUsageStateChanged> get copyWith => _$BridgeProductEventPayload_ProviderUsageStateChangedCopyWithImpl<BridgeProductEventPayload_ProviderUsageStateChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_ProviderUsageStateChanged&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeProductEventPayload.providerUsageStateChanged(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_ProviderUsageStateChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_ProviderUsageStateChangedCopyWith(BridgeProductEventPayload_ProviderUsageStateChanged value, $Res Function(BridgeProductEventPayload_ProviderUsageStateChanged) _then) = _$BridgeProductEventPayload_ProviderUsageStateChangedCopyWithImpl;
@useResult
$Res call({
 BridgeProviderUsageStateSnapshot field0
});




}
/// @nodoc
class _$BridgeProductEventPayload_ProviderUsageStateChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_ProviderUsageStateChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_ProviderUsageStateChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_ProviderUsageStateChanged _self;
  final $Res Function(BridgeProductEventPayload_ProviderUsageStateChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProductEventPayload_ProviderUsageStateChanged(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeProviderUsageStateSnapshot,
  ));
}


}

/// @nodoc


class BridgeProductEventPayload_UpdaterStateChanged extends BridgeProductEventPayload {
  const BridgeProductEventPayload_UpdaterStateChanged(this.field0): super._();


 final  BridgeUpdaterStateSnapshot field0;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductEventPayload_UpdaterStateChangedCopyWith<BridgeProductEventPayload_UpdaterStateChanged> get copyWith => _$BridgeProductEventPayload_UpdaterStateChangedCopyWithImpl<BridgeProductEventPayload_UpdaterStateChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductEventPayload_UpdaterStateChanged&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeProductEventPayload.updaterStateChanged(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProductEventPayload_UpdaterStateChangedCopyWith<$Res> implements $BridgeProductEventPayloadCopyWith<$Res> {
  factory $BridgeProductEventPayload_UpdaterStateChangedCopyWith(BridgeProductEventPayload_UpdaterStateChanged value, $Res Function(BridgeProductEventPayload_UpdaterStateChanged) _then) = _$BridgeProductEventPayload_UpdaterStateChangedCopyWithImpl;
@useResult
$Res call({
 BridgeUpdaterStateSnapshot field0
});




}
/// @nodoc
class _$BridgeProductEventPayload_UpdaterStateChangedCopyWithImpl<$Res>
    implements $BridgeProductEventPayload_UpdaterStateChangedCopyWith<$Res> {
  _$BridgeProductEventPayload_UpdaterStateChangedCopyWithImpl(this._self, this._then);

  final BridgeProductEventPayload_UpdaterStateChanged _self;
  final $Res Function(BridgeProductEventPayload_UpdaterStateChanged) _then;

/// Create a copy of BridgeProductEventPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProductEventPayload_UpdaterStateChanged(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeUpdaterStateSnapshot,
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

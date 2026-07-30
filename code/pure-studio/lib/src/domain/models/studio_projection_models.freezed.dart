// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'studio_projection_models.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$ShellChromeView {

 List<StudioRecoveryIssue> get applicationRecoveryIssues;
/// Create a copy of ShellChromeView
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$ShellChromeViewCopyWith<ShellChromeView> get copyWith => _$ShellChromeViewCopyWithImpl<ShellChromeView>(this as ShellChromeView, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ShellChromeView&&const DeepCollectionEquality().equals(other.applicationRecoveryIssues, applicationRecoveryIssues));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(applicationRecoveryIssues));

@override
String toString() {
  return 'ShellChromeView(applicationRecoveryIssues: $applicationRecoveryIssues)';
}


}

/// @nodoc
abstract mixin class $ShellChromeViewCopyWith<$Res>  {
  factory $ShellChromeViewCopyWith(ShellChromeView value, $Res Function(ShellChromeView) _then) = _$ShellChromeViewCopyWithImpl;
@useResult
$Res call({
 List<StudioRecoveryIssue> applicationRecoveryIssues
});




}
/// @nodoc
class _$ShellChromeViewCopyWithImpl<$Res>
    implements $ShellChromeViewCopyWith<$Res> {
  _$ShellChromeViewCopyWithImpl(this._self, this._then);

  final ShellChromeView _self;
  final $Res Function(ShellChromeView) _then;

/// Create a copy of ShellChromeView
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? applicationRecoveryIssues = null,}) {
  return _then(_self.copyWith(
applicationRecoveryIssues: null == applicationRecoveryIssues ? _self.applicationRecoveryIssues : applicationRecoveryIssues // ignore: cast_nullable_to_non_nullable
as List<StudioRecoveryIssue>,
  ));
}

}


/// Adds pattern-matching-related methods to [ShellChromeView].
extension ShellChromeViewPatterns on ShellChromeView {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>(TResult Function( _ShellChromeView value)?  $default,{required TResult orElse(),}){
final _that = this;
switch (_that) {
case _ShellChromeView() when $default != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>(TResult Function( _ShellChromeView value)  $default,){
final _that = this;
switch (_that) {
case _ShellChromeView():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>(TResult? Function( _ShellChromeView value)?  $default,){
final _that = this;
switch (_that) {
case _ShellChromeView() when $default != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>(TResult Function( List<StudioRecoveryIssue> applicationRecoveryIssues)?  $default,{required TResult orElse(),}) {final _that = this;
switch (_that) {
case _ShellChromeView() when $default != null:
return $default(_that.applicationRecoveryIssues);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>(TResult Function( List<StudioRecoveryIssue> applicationRecoveryIssues)  $default,) {final _that = this;
switch (_that) {
case _ShellChromeView():
return $default(_that.applicationRecoveryIssues);case _:
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>(TResult? Function( List<StudioRecoveryIssue> applicationRecoveryIssues)?  $default,) {final _that = this;
switch (_that) {
case _ShellChromeView() when $default != null:
return $default(_that.applicationRecoveryIssues);case _:
  return null;

}
}

}

/// @nodoc


class _ShellChromeView implements ShellChromeView {
  const _ShellChromeView({required final  List<StudioRecoveryIssue> applicationRecoveryIssues}): _applicationRecoveryIssues = applicationRecoveryIssues;
  

 final  List<StudioRecoveryIssue> _applicationRecoveryIssues;
@override List<StudioRecoveryIssue> get applicationRecoveryIssues {
  if (_applicationRecoveryIssues is EqualUnmodifiableListView) return _applicationRecoveryIssues;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_applicationRecoveryIssues);
}


/// Create a copy of ShellChromeView
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
_$ShellChromeViewCopyWith<_ShellChromeView> get copyWith => __$ShellChromeViewCopyWithImpl<_ShellChromeView>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _ShellChromeView&&const DeepCollectionEquality().equals(other._applicationRecoveryIssues, _applicationRecoveryIssues));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_applicationRecoveryIssues));

@override
String toString() {
  return 'ShellChromeView(applicationRecoveryIssues: $applicationRecoveryIssues)';
}


}

/// @nodoc
abstract mixin class _$ShellChromeViewCopyWith<$Res> implements $ShellChromeViewCopyWith<$Res> {
  factory _$ShellChromeViewCopyWith(_ShellChromeView value, $Res Function(_ShellChromeView) _then) = __$ShellChromeViewCopyWithImpl;
@override @useResult
$Res call({
 List<StudioRecoveryIssue> applicationRecoveryIssues
});




}
/// @nodoc
class __$ShellChromeViewCopyWithImpl<$Res>
    implements _$ShellChromeViewCopyWith<$Res> {
  __$ShellChromeViewCopyWithImpl(this._self, this._then);

  final _ShellChromeView _self;
  final $Res Function(_ShellChromeView) _then;

/// Create a copy of ShellChromeView
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? applicationRecoveryIssues = null,}) {
  return _then(_ShellChromeView(
applicationRecoveryIssues: null == applicationRecoveryIssues ? _self._applicationRecoveryIssues : applicationRecoveryIssues // ignore: cast_nullable_to_non_nullable
as List<StudioRecoveryIssue>,
  ));
}


}

/// @nodoc
mixin _$SidebarView {

 List<StudioProject> get projects; List<StudioSession> get rootSessions; String? get selectedProjectId; String? get selectedRootSessionId; bool get isBusy; List<StudioRecoveryIssue> get recoveryIssues;
/// Create a copy of SidebarView
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SidebarViewCopyWith<SidebarView> get copyWith => _$SidebarViewCopyWithImpl<SidebarView>(this as SidebarView, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is SidebarView&&const DeepCollectionEquality().equals(other.projects, projects)&&const DeepCollectionEquality().equals(other.rootSessions, rootSessions)&&(identical(other.selectedProjectId, selectedProjectId) || other.selectedProjectId == selectedProjectId)&&(identical(other.selectedRootSessionId, selectedRootSessionId) || other.selectedRootSessionId == selectedRootSessionId)&&(identical(other.isBusy, isBusy) || other.isBusy == isBusy)&&const DeepCollectionEquality().equals(other.recoveryIssues, recoveryIssues));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(projects),const DeepCollectionEquality().hash(rootSessions),selectedProjectId,selectedRootSessionId,isBusy,const DeepCollectionEquality().hash(recoveryIssues));

@override
String toString() {
  return 'SidebarView(projects: $projects, rootSessions: $rootSessions, selectedProjectId: $selectedProjectId, selectedRootSessionId: $selectedRootSessionId, isBusy: $isBusy, recoveryIssues: $recoveryIssues)';
}


}

/// @nodoc
abstract mixin class $SidebarViewCopyWith<$Res>  {
  factory $SidebarViewCopyWith(SidebarView value, $Res Function(SidebarView) _then) = _$SidebarViewCopyWithImpl;
@useResult
$Res call({
 List<StudioProject> projects, List<StudioSession> rootSessions, String? selectedProjectId, String? selectedRootSessionId, bool isBusy, List<StudioRecoveryIssue> recoveryIssues
});




}
/// @nodoc
class _$SidebarViewCopyWithImpl<$Res>
    implements $SidebarViewCopyWith<$Res> {
  _$SidebarViewCopyWithImpl(this._self, this._then);

  final SidebarView _self;
  final $Res Function(SidebarView) _then;

/// Create a copy of SidebarView
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? projects = null,Object? rootSessions = null,Object? selectedProjectId = freezed,Object? selectedRootSessionId = freezed,Object? isBusy = null,Object? recoveryIssues = null,}) {
  return _then(_self.copyWith(
projects: null == projects ? _self.projects : projects // ignore: cast_nullable_to_non_nullable
as List<StudioProject>,rootSessions: null == rootSessions ? _self.rootSessions : rootSessions // ignore: cast_nullable_to_non_nullable
as List<StudioSession>,selectedProjectId: freezed == selectedProjectId ? _self.selectedProjectId : selectedProjectId // ignore: cast_nullable_to_non_nullable
as String?,selectedRootSessionId: freezed == selectedRootSessionId ? _self.selectedRootSessionId : selectedRootSessionId // ignore: cast_nullable_to_non_nullable
as String?,isBusy: null == isBusy ? _self.isBusy : isBusy // ignore: cast_nullable_to_non_nullable
as bool,recoveryIssues: null == recoveryIssues ? _self.recoveryIssues : recoveryIssues // ignore: cast_nullable_to_non_nullable
as List<StudioRecoveryIssue>,
  ));
}

}


/// Adds pattern-matching-related methods to [SidebarView].
extension SidebarViewPatterns on SidebarView {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>(TResult Function( _SidebarView value)?  $default,{required TResult orElse(),}){
final _that = this;
switch (_that) {
case _SidebarView() when $default != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>(TResult Function( _SidebarView value)  $default,){
final _that = this;
switch (_that) {
case _SidebarView():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>(TResult? Function( _SidebarView value)?  $default,){
final _that = this;
switch (_that) {
case _SidebarView() when $default != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>(TResult Function( List<StudioProject> projects,  List<StudioSession> rootSessions,  String? selectedProjectId,  String? selectedRootSessionId,  bool isBusy,  List<StudioRecoveryIssue> recoveryIssues)?  $default,{required TResult orElse(),}) {final _that = this;
switch (_that) {
case _SidebarView() when $default != null:
return $default(_that.projects,_that.rootSessions,_that.selectedProjectId,_that.selectedRootSessionId,_that.isBusy,_that.recoveryIssues);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>(TResult Function( List<StudioProject> projects,  List<StudioSession> rootSessions,  String? selectedProjectId,  String? selectedRootSessionId,  bool isBusy,  List<StudioRecoveryIssue> recoveryIssues)  $default,) {final _that = this;
switch (_that) {
case _SidebarView():
return $default(_that.projects,_that.rootSessions,_that.selectedProjectId,_that.selectedRootSessionId,_that.isBusy,_that.recoveryIssues);case _:
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>(TResult? Function( List<StudioProject> projects,  List<StudioSession> rootSessions,  String? selectedProjectId,  String? selectedRootSessionId,  bool isBusy,  List<StudioRecoveryIssue> recoveryIssues)?  $default,) {final _that = this;
switch (_that) {
case _SidebarView() when $default != null:
return $default(_that.projects,_that.rootSessions,_that.selectedProjectId,_that.selectedRootSessionId,_that.isBusy,_that.recoveryIssues);case _:
  return null;

}
}

}

/// @nodoc


class _SidebarView extends SidebarView {
  const _SidebarView({required final  List<StudioProject> projects, required final  List<StudioSession> rootSessions, required this.selectedProjectId, required this.selectedRootSessionId, required this.isBusy, required final  List<StudioRecoveryIssue> recoveryIssues}): _projects = projects,_rootSessions = rootSessions,_recoveryIssues = recoveryIssues,super._();
  

 final  List<StudioProject> _projects;
@override List<StudioProject> get projects {
  if (_projects is EqualUnmodifiableListView) return _projects;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_projects);
}

 final  List<StudioSession> _rootSessions;
@override List<StudioSession> get rootSessions {
  if (_rootSessions is EqualUnmodifiableListView) return _rootSessions;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_rootSessions);
}

@override final  String? selectedProjectId;
@override final  String? selectedRootSessionId;
@override final  bool isBusy;
 final  List<StudioRecoveryIssue> _recoveryIssues;
@override List<StudioRecoveryIssue> get recoveryIssues {
  if (_recoveryIssues is EqualUnmodifiableListView) return _recoveryIssues;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_recoveryIssues);
}


/// Create a copy of SidebarView
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
_$SidebarViewCopyWith<_SidebarView> get copyWith => __$SidebarViewCopyWithImpl<_SidebarView>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _SidebarView&&const DeepCollectionEquality().equals(other._projects, _projects)&&const DeepCollectionEquality().equals(other._rootSessions, _rootSessions)&&(identical(other.selectedProjectId, selectedProjectId) || other.selectedProjectId == selectedProjectId)&&(identical(other.selectedRootSessionId, selectedRootSessionId) || other.selectedRootSessionId == selectedRootSessionId)&&(identical(other.isBusy, isBusy) || other.isBusy == isBusy)&&const DeepCollectionEquality().equals(other._recoveryIssues, _recoveryIssues));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_projects),const DeepCollectionEquality().hash(_rootSessions),selectedProjectId,selectedRootSessionId,isBusy,const DeepCollectionEquality().hash(_recoveryIssues));

@override
String toString() {
  return 'SidebarView(projects: $projects, rootSessions: $rootSessions, selectedProjectId: $selectedProjectId, selectedRootSessionId: $selectedRootSessionId, isBusy: $isBusy, recoveryIssues: $recoveryIssues)';
}


}

/// @nodoc
abstract mixin class _$SidebarViewCopyWith<$Res> implements $SidebarViewCopyWith<$Res> {
  factory _$SidebarViewCopyWith(_SidebarView value, $Res Function(_SidebarView) _then) = __$SidebarViewCopyWithImpl;
@override @useResult
$Res call({
 List<StudioProject> projects, List<StudioSession> rootSessions, String? selectedProjectId, String? selectedRootSessionId, bool isBusy, List<StudioRecoveryIssue> recoveryIssues
});




}
/// @nodoc
class __$SidebarViewCopyWithImpl<$Res>
    implements _$SidebarViewCopyWith<$Res> {
  __$SidebarViewCopyWithImpl(this._self, this._then);

  final _SidebarView _self;
  final $Res Function(_SidebarView) _then;

/// Create a copy of SidebarView
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? projects = null,Object? rootSessions = null,Object? selectedProjectId = freezed,Object? selectedRootSessionId = freezed,Object? isBusy = null,Object? recoveryIssues = null,}) {
  return _then(_SidebarView(
projects: null == projects ? _self._projects : projects // ignore: cast_nullable_to_non_nullable
as List<StudioProject>,rootSessions: null == rootSessions ? _self._rootSessions : rootSessions // ignore: cast_nullable_to_non_nullable
as List<StudioSession>,selectedProjectId: freezed == selectedProjectId ? _self.selectedProjectId : selectedProjectId // ignore: cast_nullable_to_non_nullable
as String?,selectedRootSessionId: freezed == selectedRootSessionId ? _self.selectedRootSessionId : selectedRootSessionId // ignore: cast_nullable_to_non_nullable
as String?,isBusy: null == isBusy ? _self.isBusy : isBusy // ignore: cast_nullable_to_non_nullable
as bool,recoveryIssues: null == recoveryIssues ? _self._recoveryIssues : recoveryIssues // ignore: cast_nullable_to_non_nullable
as List<StudioRecoveryIssue>,
  ));
}


}

/// @nodoc
mixin _$HeaderView {

 StudioSession? get selectedRootSession; StudioProject? get selectedProject; String? get selectedProjectId; List<StudioSession> get agentSessions; String? get selectedAgentSessionId; SessionRuntimeView get runtime; List<PendingInteraction> get pendingInteractions;
/// Create a copy of HeaderView
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$HeaderViewCopyWith<HeaderView> get copyWith => _$HeaderViewCopyWithImpl<HeaderView>(this as HeaderView, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is HeaderView&&(identical(other.selectedRootSession, selectedRootSession) || other.selectedRootSession == selectedRootSession)&&(identical(other.selectedProject, selectedProject) || other.selectedProject == selectedProject)&&(identical(other.selectedProjectId, selectedProjectId) || other.selectedProjectId == selectedProjectId)&&const DeepCollectionEquality().equals(other.agentSessions, agentSessions)&&(identical(other.selectedAgentSessionId, selectedAgentSessionId) || other.selectedAgentSessionId == selectedAgentSessionId)&&(identical(other.runtime, runtime) || other.runtime == runtime)&&const DeepCollectionEquality().equals(other.pendingInteractions, pendingInteractions));
}


@override
int get hashCode => Object.hash(runtimeType,selectedRootSession,selectedProject,selectedProjectId,const DeepCollectionEquality().hash(agentSessions),selectedAgentSessionId,runtime,const DeepCollectionEquality().hash(pendingInteractions));

@override
String toString() {
  return 'HeaderView(selectedRootSession: $selectedRootSession, selectedProject: $selectedProject, selectedProjectId: $selectedProjectId, agentSessions: $agentSessions, selectedAgentSessionId: $selectedAgentSessionId, runtime: $runtime, pendingInteractions: $pendingInteractions)';
}


}

/// @nodoc
abstract mixin class $HeaderViewCopyWith<$Res>  {
  factory $HeaderViewCopyWith(HeaderView value, $Res Function(HeaderView) _then) = _$HeaderViewCopyWithImpl;
@useResult
$Res call({
 StudioSession? selectedRootSession, StudioProject? selectedProject, String? selectedProjectId, List<StudioSession> agentSessions, String? selectedAgentSessionId, SessionRuntimeView runtime, List<PendingInteraction> pendingInteractions
});




}
/// @nodoc
class _$HeaderViewCopyWithImpl<$Res>
    implements $HeaderViewCopyWith<$Res> {
  _$HeaderViewCopyWithImpl(this._self, this._then);

  final HeaderView _self;
  final $Res Function(HeaderView) _then;

/// Create a copy of HeaderView
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? selectedRootSession = freezed,Object? selectedProject = freezed,Object? selectedProjectId = freezed,Object? agentSessions = null,Object? selectedAgentSessionId = freezed,Object? runtime = null,Object? pendingInteractions = null,}) {
  return _then(_self.copyWith(
selectedRootSession: freezed == selectedRootSession ? _self.selectedRootSession : selectedRootSession // ignore: cast_nullable_to_non_nullable
as StudioSession?,selectedProject: freezed == selectedProject ? _self.selectedProject : selectedProject // ignore: cast_nullable_to_non_nullable
as StudioProject?,selectedProjectId: freezed == selectedProjectId ? _self.selectedProjectId : selectedProjectId // ignore: cast_nullable_to_non_nullable
as String?,agentSessions: null == agentSessions ? _self.agentSessions : agentSessions // ignore: cast_nullable_to_non_nullable
as List<StudioSession>,selectedAgentSessionId: freezed == selectedAgentSessionId ? _self.selectedAgentSessionId : selectedAgentSessionId // ignore: cast_nullable_to_non_nullable
as String?,runtime: null == runtime ? _self.runtime : runtime // ignore: cast_nullable_to_non_nullable
as SessionRuntimeView,pendingInteractions: null == pendingInteractions ? _self.pendingInteractions : pendingInteractions // ignore: cast_nullable_to_non_nullable
as List<PendingInteraction>,
  ));
}

}


/// Adds pattern-matching-related methods to [HeaderView].
extension HeaderViewPatterns on HeaderView {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>(TResult Function( _HeaderView value)?  $default,{required TResult orElse(),}){
final _that = this;
switch (_that) {
case _HeaderView() when $default != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>(TResult Function( _HeaderView value)  $default,){
final _that = this;
switch (_that) {
case _HeaderView():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>(TResult? Function( _HeaderView value)?  $default,){
final _that = this;
switch (_that) {
case _HeaderView() when $default != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>(TResult Function( StudioSession? selectedRootSession,  StudioProject? selectedProject,  String? selectedProjectId,  List<StudioSession> agentSessions,  String? selectedAgentSessionId,  SessionRuntimeView runtime,  List<PendingInteraction> pendingInteractions)?  $default,{required TResult orElse(),}) {final _that = this;
switch (_that) {
case _HeaderView() when $default != null:
return $default(_that.selectedRootSession,_that.selectedProject,_that.selectedProjectId,_that.agentSessions,_that.selectedAgentSessionId,_that.runtime,_that.pendingInteractions);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>(TResult Function( StudioSession? selectedRootSession,  StudioProject? selectedProject,  String? selectedProjectId,  List<StudioSession> agentSessions,  String? selectedAgentSessionId,  SessionRuntimeView runtime,  List<PendingInteraction> pendingInteractions)  $default,) {final _that = this;
switch (_that) {
case _HeaderView():
return $default(_that.selectedRootSession,_that.selectedProject,_that.selectedProjectId,_that.agentSessions,_that.selectedAgentSessionId,_that.runtime,_that.pendingInteractions);case _:
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>(TResult? Function( StudioSession? selectedRootSession,  StudioProject? selectedProject,  String? selectedProjectId,  List<StudioSession> agentSessions,  String? selectedAgentSessionId,  SessionRuntimeView runtime,  List<PendingInteraction> pendingInteractions)?  $default,) {final _that = this;
switch (_that) {
case _HeaderView() when $default != null:
return $default(_that.selectedRootSession,_that.selectedProject,_that.selectedProjectId,_that.agentSessions,_that.selectedAgentSessionId,_that.runtime,_that.pendingInteractions);case _:
  return null;

}
}

}

/// @nodoc


class _HeaderView implements HeaderView {
  const _HeaderView({required this.selectedRootSession, required this.selectedProject, required this.selectedProjectId, required final  List<StudioSession> agentSessions, required this.selectedAgentSessionId, required this.runtime, required final  List<PendingInteraction> pendingInteractions}): _agentSessions = agentSessions,_pendingInteractions = pendingInteractions;
  

@override final  StudioSession? selectedRootSession;
@override final  StudioProject? selectedProject;
@override final  String? selectedProjectId;
 final  List<StudioSession> _agentSessions;
@override List<StudioSession> get agentSessions {
  if (_agentSessions is EqualUnmodifiableListView) return _agentSessions;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_agentSessions);
}

@override final  String? selectedAgentSessionId;
@override final  SessionRuntimeView runtime;
 final  List<PendingInteraction> _pendingInteractions;
@override List<PendingInteraction> get pendingInteractions {
  if (_pendingInteractions is EqualUnmodifiableListView) return _pendingInteractions;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_pendingInteractions);
}


/// Create a copy of HeaderView
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
_$HeaderViewCopyWith<_HeaderView> get copyWith => __$HeaderViewCopyWithImpl<_HeaderView>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _HeaderView&&(identical(other.selectedRootSession, selectedRootSession) || other.selectedRootSession == selectedRootSession)&&(identical(other.selectedProject, selectedProject) || other.selectedProject == selectedProject)&&(identical(other.selectedProjectId, selectedProjectId) || other.selectedProjectId == selectedProjectId)&&const DeepCollectionEquality().equals(other._agentSessions, _agentSessions)&&(identical(other.selectedAgentSessionId, selectedAgentSessionId) || other.selectedAgentSessionId == selectedAgentSessionId)&&(identical(other.runtime, runtime) || other.runtime == runtime)&&const DeepCollectionEquality().equals(other._pendingInteractions, _pendingInteractions));
}


@override
int get hashCode => Object.hash(runtimeType,selectedRootSession,selectedProject,selectedProjectId,const DeepCollectionEquality().hash(_agentSessions),selectedAgentSessionId,runtime,const DeepCollectionEquality().hash(_pendingInteractions));

@override
String toString() {
  return 'HeaderView(selectedRootSession: $selectedRootSession, selectedProject: $selectedProject, selectedProjectId: $selectedProjectId, agentSessions: $agentSessions, selectedAgentSessionId: $selectedAgentSessionId, runtime: $runtime, pendingInteractions: $pendingInteractions)';
}


}

/// @nodoc
abstract mixin class _$HeaderViewCopyWith<$Res> implements $HeaderViewCopyWith<$Res> {
  factory _$HeaderViewCopyWith(_HeaderView value, $Res Function(_HeaderView) _then) = __$HeaderViewCopyWithImpl;
@override @useResult
$Res call({
 StudioSession? selectedRootSession, StudioProject? selectedProject, String? selectedProjectId, List<StudioSession> agentSessions, String? selectedAgentSessionId, SessionRuntimeView runtime, List<PendingInteraction> pendingInteractions
});




}
/// @nodoc
class __$HeaderViewCopyWithImpl<$Res>
    implements _$HeaderViewCopyWith<$Res> {
  __$HeaderViewCopyWithImpl(this._self, this._then);

  final _HeaderView _self;
  final $Res Function(_HeaderView) _then;

/// Create a copy of HeaderView
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? selectedRootSession = freezed,Object? selectedProject = freezed,Object? selectedProjectId = freezed,Object? agentSessions = null,Object? selectedAgentSessionId = freezed,Object? runtime = null,Object? pendingInteractions = null,}) {
  return _then(_HeaderView(
selectedRootSession: freezed == selectedRootSession ? _self.selectedRootSession : selectedRootSession // ignore: cast_nullable_to_non_nullable
as StudioSession?,selectedProject: freezed == selectedProject ? _self.selectedProject : selectedProject // ignore: cast_nullable_to_non_nullable
as StudioProject?,selectedProjectId: freezed == selectedProjectId ? _self.selectedProjectId : selectedProjectId // ignore: cast_nullable_to_non_nullable
as String?,agentSessions: null == agentSessions ? _self._agentSessions : agentSessions // ignore: cast_nullable_to_non_nullable
as List<StudioSession>,selectedAgentSessionId: freezed == selectedAgentSessionId ? _self.selectedAgentSessionId : selectedAgentSessionId // ignore: cast_nullable_to_non_nullable
as String?,runtime: null == runtime ? _self.runtime : runtime // ignore: cast_nullable_to_non_nullable
as SessionRuntimeView,pendingInteractions: null == pendingInteractions ? _self._pendingInteractions : pendingInteractions // ignore: cast_nullable_to_non_nullable
as List<PendingInteraction>,
  ));
}


}

/// @nodoc
mixin _$SettingsPageView {

 List<ProviderSettingsView> get providers; ProviderCatalogView get providerCatalog; String? get defaultProviderId; List<RoleSettingsView> get roles; InstructionsSettingsView get instructions; SkillsSettingsView get skills; List<String> get activeSkills; String? get selectedProjectId; List<McpServerSettingsView> get mcpServers; PermissionMode get permissionMode; GeneralSettingsView get general; WebSearchSettingsView get webSearch; bool get runtimeBusy;
/// Create a copy of SettingsPageView
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SettingsPageViewCopyWith<SettingsPageView> get copyWith => _$SettingsPageViewCopyWithImpl<SettingsPageView>(this as SettingsPageView, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is SettingsPageView&&const DeepCollectionEquality().equals(other.providers, providers)&&(identical(other.providerCatalog, providerCatalog) || other.providerCatalog == providerCatalog)&&(identical(other.defaultProviderId, defaultProviderId) || other.defaultProviderId == defaultProviderId)&&const DeepCollectionEquality().equals(other.roles, roles)&&(identical(other.instructions, instructions) || other.instructions == instructions)&&(identical(other.skills, skills) || other.skills == skills)&&const DeepCollectionEquality().equals(other.activeSkills, activeSkills)&&(identical(other.selectedProjectId, selectedProjectId) || other.selectedProjectId == selectedProjectId)&&const DeepCollectionEquality().equals(other.mcpServers, mcpServers)&&(identical(other.permissionMode, permissionMode) || other.permissionMode == permissionMode)&&(identical(other.general, general) || other.general == general)&&(identical(other.webSearch, webSearch) || other.webSearch == webSearch)&&(identical(other.runtimeBusy, runtimeBusy) || other.runtimeBusy == runtimeBusy));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(providers),providerCatalog,defaultProviderId,const DeepCollectionEquality().hash(roles),instructions,skills,const DeepCollectionEquality().hash(activeSkills),selectedProjectId,const DeepCollectionEquality().hash(mcpServers),permissionMode,general,webSearch,runtimeBusy);

@override
String toString() {
  return 'SettingsPageView(providers: $providers, providerCatalog: $providerCatalog, defaultProviderId: $defaultProviderId, roles: $roles, instructions: $instructions, skills: $skills, activeSkills: $activeSkills, selectedProjectId: $selectedProjectId, mcpServers: $mcpServers, permissionMode: $permissionMode, general: $general, webSearch: $webSearch, runtimeBusy: $runtimeBusy)';
}


}

/// @nodoc
abstract mixin class $SettingsPageViewCopyWith<$Res>  {
  factory $SettingsPageViewCopyWith(SettingsPageView value, $Res Function(SettingsPageView) _then) = _$SettingsPageViewCopyWithImpl;
@useResult
$Res call({
 List<ProviderSettingsView> providers, ProviderCatalogView providerCatalog, String? defaultProviderId, List<RoleSettingsView> roles, InstructionsSettingsView instructions, SkillsSettingsView skills, List<String> activeSkills, String? selectedProjectId, List<McpServerSettingsView> mcpServers, PermissionMode permissionMode, GeneralSettingsView general, WebSearchSettingsView webSearch, bool runtimeBusy
});




}
/// @nodoc
class _$SettingsPageViewCopyWithImpl<$Res>
    implements $SettingsPageViewCopyWith<$Res> {
  _$SettingsPageViewCopyWithImpl(this._self, this._then);

  final SettingsPageView _self;
  final $Res Function(SettingsPageView) _then;

/// Create a copy of SettingsPageView
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? providers = null,Object? providerCatalog = null,Object? defaultProviderId = freezed,Object? roles = null,Object? instructions = null,Object? skills = null,Object? activeSkills = null,Object? selectedProjectId = freezed,Object? mcpServers = null,Object? permissionMode = null,Object? general = null,Object? webSearch = null,Object? runtimeBusy = null,}) {
  return _then(_self.copyWith(
providers: null == providers ? _self.providers : providers // ignore: cast_nullable_to_non_nullable
as List<ProviderSettingsView>,providerCatalog: null == providerCatalog ? _self.providerCatalog : providerCatalog // ignore: cast_nullable_to_non_nullable
as ProviderCatalogView,defaultProviderId: freezed == defaultProviderId ? _self.defaultProviderId : defaultProviderId // ignore: cast_nullable_to_non_nullable
as String?,roles: null == roles ? _self.roles : roles // ignore: cast_nullable_to_non_nullable
as List<RoleSettingsView>,instructions: null == instructions ? _self.instructions : instructions // ignore: cast_nullable_to_non_nullable
as InstructionsSettingsView,skills: null == skills ? _self.skills : skills // ignore: cast_nullable_to_non_nullable
as SkillsSettingsView,activeSkills: null == activeSkills ? _self.activeSkills : activeSkills // ignore: cast_nullable_to_non_nullable
as List<String>,selectedProjectId: freezed == selectedProjectId ? _self.selectedProjectId : selectedProjectId // ignore: cast_nullable_to_non_nullable
as String?,mcpServers: null == mcpServers ? _self.mcpServers : mcpServers // ignore: cast_nullable_to_non_nullable
as List<McpServerSettingsView>,permissionMode: null == permissionMode ? _self.permissionMode : permissionMode // ignore: cast_nullable_to_non_nullable
as PermissionMode,general: null == general ? _self.general : general // ignore: cast_nullable_to_non_nullable
as GeneralSettingsView,webSearch: null == webSearch ? _self.webSearch : webSearch // ignore: cast_nullable_to_non_nullable
as WebSearchSettingsView,runtimeBusy: null == runtimeBusy ? _self.runtimeBusy : runtimeBusy // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}

}


/// Adds pattern-matching-related methods to [SettingsPageView].
extension SettingsPageViewPatterns on SettingsPageView {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>(TResult Function( _SettingsPageView value)?  $default,{required TResult orElse(),}){
final _that = this;
switch (_that) {
case _SettingsPageView() when $default != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>(TResult Function( _SettingsPageView value)  $default,){
final _that = this;
switch (_that) {
case _SettingsPageView():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>(TResult? Function( _SettingsPageView value)?  $default,){
final _that = this;
switch (_that) {
case _SettingsPageView() when $default != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>(TResult Function( List<ProviderSettingsView> providers,  ProviderCatalogView providerCatalog,  String? defaultProviderId,  List<RoleSettingsView> roles,  InstructionsSettingsView instructions,  SkillsSettingsView skills,  List<String> activeSkills,  String? selectedProjectId,  List<McpServerSettingsView> mcpServers,  PermissionMode permissionMode,  GeneralSettingsView general,  WebSearchSettingsView webSearch,  bool runtimeBusy)?  $default,{required TResult orElse(),}) {final _that = this;
switch (_that) {
case _SettingsPageView() when $default != null:
return $default(_that.providers,_that.providerCatalog,_that.defaultProviderId,_that.roles,_that.instructions,_that.skills,_that.activeSkills,_that.selectedProjectId,_that.mcpServers,_that.permissionMode,_that.general,_that.webSearch,_that.runtimeBusy);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>(TResult Function( List<ProviderSettingsView> providers,  ProviderCatalogView providerCatalog,  String? defaultProviderId,  List<RoleSettingsView> roles,  InstructionsSettingsView instructions,  SkillsSettingsView skills,  List<String> activeSkills,  String? selectedProjectId,  List<McpServerSettingsView> mcpServers,  PermissionMode permissionMode,  GeneralSettingsView general,  WebSearchSettingsView webSearch,  bool runtimeBusy)  $default,) {final _that = this;
switch (_that) {
case _SettingsPageView():
return $default(_that.providers,_that.providerCatalog,_that.defaultProviderId,_that.roles,_that.instructions,_that.skills,_that.activeSkills,_that.selectedProjectId,_that.mcpServers,_that.permissionMode,_that.general,_that.webSearch,_that.runtimeBusy);case _:
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>(TResult? Function( List<ProviderSettingsView> providers,  ProviderCatalogView providerCatalog,  String? defaultProviderId,  List<RoleSettingsView> roles,  InstructionsSettingsView instructions,  SkillsSettingsView skills,  List<String> activeSkills,  String? selectedProjectId,  List<McpServerSettingsView> mcpServers,  PermissionMode permissionMode,  GeneralSettingsView general,  WebSearchSettingsView webSearch,  bool runtimeBusy)?  $default,) {final _that = this;
switch (_that) {
case _SettingsPageView() when $default != null:
return $default(_that.providers,_that.providerCatalog,_that.defaultProviderId,_that.roles,_that.instructions,_that.skills,_that.activeSkills,_that.selectedProjectId,_that.mcpServers,_that.permissionMode,_that.general,_that.webSearch,_that.runtimeBusy);case _:
  return null;

}
}

}

/// @nodoc


class _SettingsPageView implements SettingsPageView {
  const _SettingsPageView({required final  List<ProviderSettingsView> providers, required this.providerCatalog, required this.defaultProviderId, required final  List<RoleSettingsView> roles, required this.instructions, required this.skills, required final  List<String> activeSkills, required this.selectedProjectId, required final  List<McpServerSettingsView> mcpServers, required this.permissionMode, required this.general, required this.webSearch, required this.runtimeBusy}): _providers = providers,_roles = roles,_activeSkills = activeSkills,_mcpServers = mcpServers;
  

 final  List<ProviderSettingsView> _providers;
@override List<ProviderSettingsView> get providers {
  if (_providers is EqualUnmodifiableListView) return _providers;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_providers);
}

@override final  ProviderCatalogView providerCatalog;
@override final  String? defaultProviderId;
 final  List<RoleSettingsView> _roles;
@override List<RoleSettingsView> get roles {
  if (_roles is EqualUnmodifiableListView) return _roles;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_roles);
}

@override final  InstructionsSettingsView instructions;
@override final  SkillsSettingsView skills;
 final  List<String> _activeSkills;
@override List<String> get activeSkills {
  if (_activeSkills is EqualUnmodifiableListView) return _activeSkills;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_activeSkills);
}

@override final  String? selectedProjectId;
 final  List<McpServerSettingsView> _mcpServers;
@override List<McpServerSettingsView> get mcpServers {
  if (_mcpServers is EqualUnmodifiableListView) return _mcpServers;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_mcpServers);
}

@override final  PermissionMode permissionMode;
@override final  GeneralSettingsView general;
@override final  WebSearchSettingsView webSearch;
@override final  bool runtimeBusy;

/// Create a copy of SettingsPageView
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
_$SettingsPageViewCopyWith<_SettingsPageView> get copyWith => __$SettingsPageViewCopyWithImpl<_SettingsPageView>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _SettingsPageView&&const DeepCollectionEquality().equals(other._providers, _providers)&&(identical(other.providerCatalog, providerCatalog) || other.providerCatalog == providerCatalog)&&(identical(other.defaultProviderId, defaultProviderId) || other.defaultProviderId == defaultProviderId)&&const DeepCollectionEquality().equals(other._roles, _roles)&&(identical(other.instructions, instructions) || other.instructions == instructions)&&(identical(other.skills, skills) || other.skills == skills)&&const DeepCollectionEquality().equals(other._activeSkills, _activeSkills)&&(identical(other.selectedProjectId, selectedProjectId) || other.selectedProjectId == selectedProjectId)&&const DeepCollectionEquality().equals(other._mcpServers, _mcpServers)&&(identical(other.permissionMode, permissionMode) || other.permissionMode == permissionMode)&&(identical(other.general, general) || other.general == general)&&(identical(other.webSearch, webSearch) || other.webSearch == webSearch)&&(identical(other.runtimeBusy, runtimeBusy) || other.runtimeBusy == runtimeBusy));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_providers),providerCatalog,defaultProviderId,const DeepCollectionEquality().hash(_roles),instructions,skills,const DeepCollectionEquality().hash(_activeSkills),selectedProjectId,const DeepCollectionEquality().hash(_mcpServers),permissionMode,general,webSearch,runtimeBusy);

@override
String toString() {
  return 'SettingsPageView(providers: $providers, providerCatalog: $providerCatalog, defaultProviderId: $defaultProviderId, roles: $roles, instructions: $instructions, skills: $skills, activeSkills: $activeSkills, selectedProjectId: $selectedProjectId, mcpServers: $mcpServers, permissionMode: $permissionMode, general: $general, webSearch: $webSearch, runtimeBusy: $runtimeBusy)';
}


}

/// @nodoc
abstract mixin class _$SettingsPageViewCopyWith<$Res> implements $SettingsPageViewCopyWith<$Res> {
  factory _$SettingsPageViewCopyWith(_SettingsPageView value, $Res Function(_SettingsPageView) _then) = __$SettingsPageViewCopyWithImpl;
@override @useResult
$Res call({
 List<ProviderSettingsView> providers, ProviderCatalogView providerCatalog, String? defaultProviderId, List<RoleSettingsView> roles, InstructionsSettingsView instructions, SkillsSettingsView skills, List<String> activeSkills, String? selectedProjectId, List<McpServerSettingsView> mcpServers, PermissionMode permissionMode, GeneralSettingsView general, WebSearchSettingsView webSearch, bool runtimeBusy
});




}
/// @nodoc
class __$SettingsPageViewCopyWithImpl<$Res>
    implements _$SettingsPageViewCopyWith<$Res> {
  __$SettingsPageViewCopyWithImpl(this._self, this._then);

  final _SettingsPageView _self;
  final $Res Function(_SettingsPageView) _then;

/// Create a copy of SettingsPageView
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? providers = null,Object? providerCatalog = null,Object? defaultProviderId = freezed,Object? roles = null,Object? instructions = null,Object? skills = null,Object? activeSkills = null,Object? selectedProjectId = freezed,Object? mcpServers = null,Object? permissionMode = null,Object? general = null,Object? webSearch = null,Object? runtimeBusy = null,}) {
  return _then(_SettingsPageView(
providers: null == providers ? _self._providers : providers // ignore: cast_nullable_to_non_nullable
as List<ProviderSettingsView>,providerCatalog: null == providerCatalog ? _self.providerCatalog : providerCatalog // ignore: cast_nullable_to_non_nullable
as ProviderCatalogView,defaultProviderId: freezed == defaultProviderId ? _self.defaultProviderId : defaultProviderId // ignore: cast_nullable_to_non_nullable
as String?,roles: null == roles ? _self._roles : roles // ignore: cast_nullable_to_non_nullable
as List<RoleSettingsView>,instructions: null == instructions ? _self.instructions : instructions // ignore: cast_nullable_to_non_nullable
as InstructionsSettingsView,skills: null == skills ? _self.skills : skills // ignore: cast_nullable_to_non_nullable
as SkillsSettingsView,activeSkills: null == activeSkills ? _self._activeSkills : activeSkills // ignore: cast_nullable_to_non_nullable
as List<String>,selectedProjectId: freezed == selectedProjectId ? _self.selectedProjectId : selectedProjectId // ignore: cast_nullable_to_non_nullable
as String?,mcpServers: null == mcpServers ? _self._mcpServers : mcpServers // ignore: cast_nullable_to_non_nullable
as List<McpServerSettingsView>,permissionMode: null == permissionMode ? _self.permissionMode : permissionMode // ignore: cast_nullable_to_non_nullable
as PermissionMode,general: null == general ? _self.general : general // ignore: cast_nullable_to_non_nullable
as GeneralSettingsView,webSearch: null == webSearch ? _self.webSearch : webSearch // ignore: cast_nullable_to_non_nullable
as WebSearchSettingsView,runtimeBusy: null == runtimeBusy ? _self.runtimeBusy : runtimeBusy // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc
mixin _$StatusBarView {

 StudioSession get session; SessionRuntimeView get runtime; PermissionMode get permissionMode; List<ProviderSettingsView> get providers; List<RoleSettingsView> get roles; bool get isBusy;
/// Create a copy of StatusBarView
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$StatusBarViewCopyWith<StatusBarView> get copyWith => _$StatusBarViewCopyWithImpl<StatusBarView>(this as StatusBarView, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is StatusBarView&&(identical(other.session, session) || other.session == session)&&(identical(other.runtime, runtime) || other.runtime == runtime)&&(identical(other.permissionMode, permissionMode) || other.permissionMode == permissionMode)&&const DeepCollectionEquality().equals(other.providers, providers)&&const DeepCollectionEquality().equals(other.roles, roles)&&(identical(other.isBusy, isBusy) || other.isBusy == isBusy));
}


@override
int get hashCode => Object.hash(runtimeType,session,runtime,permissionMode,const DeepCollectionEquality().hash(providers),const DeepCollectionEquality().hash(roles),isBusy);

@override
String toString() {
  return 'StatusBarView(session: $session, runtime: $runtime, permissionMode: $permissionMode, providers: $providers, roles: $roles, isBusy: $isBusy)';
}


}

/// @nodoc
abstract mixin class $StatusBarViewCopyWith<$Res>  {
  factory $StatusBarViewCopyWith(StatusBarView value, $Res Function(StatusBarView) _then) = _$StatusBarViewCopyWithImpl;
@useResult
$Res call({
 StudioSession session, SessionRuntimeView runtime, PermissionMode permissionMode, List<ProviderSettingsView> providers, List<RoleSettingsView> roles, bool isBusy
});




}
/// @nodoc
class _$StatusBarViewCopyWithImpl<$Res>
    implements $StatusBarViewCopyWith<$Res> {
  _$StatusBarViewCopyWithImpl(this._self, this._then);

  final StatusBarView _self;
  final $Res Function(StatusBarView) _then;

/// Create a copy of StatusBarView
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? session = null,Object? runtime = null,Object? permissionMode = null,Object? providers = null,Object? roles = null,Object? isBusy = null,}) {
  return _then(_self.copyWith(
session: null == session ? _self.session : session // ignore: cast_nullable_to_non_nullable
as StudioSession,runtime: null == runtime ? _self.runtime : runtime // ignore: cast_nullable_to_non_nullable
as SessionRuntimeView,permissionMode: null == permissionMode ? _self.permissionMode : permissionMode // ignore: cast_nullable_to_non_nullable
as PermissionMode,providers: null == providers ? _self.providers : providers // ignore: cast_nullable_to_non_nullable
as List<ProviderSettingsView>,roles: null == roles ? _self.roles : roles // ignore: cast_nullable_to_non_nullable
as List<RoleSettingsView>,isBusy: null == isBusy ? _self.isBusy : isBusy // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}

}


/// Adds pattern-matching-related methods to [StatusBarView].
extension StatusBarViewPatterns on StatusBarView {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>(TResult Function( _StatusBarView value)?  $default,{required TResult orElse(),}){
final _that = this;
switch (_that) {
case _StatusBarView() when $default != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>(TResult Function( _StatusBarView value)  $default,){
final _that = this;
switch (_that) {
case _StatusBarView():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>(TResult? Function( _StatusBarView value)?  $default,){
final _that = this;
switch (_that) {
case _StatusBarView() when $default != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>(TResult Function( StudioSession session,  SessionRuntimeView runtime,  PermissionMode permissionMode,  List<ProviderSettingsView> providers,  List<RoleSettingsView> roles,  bool isBusy)?  $default,{required TResult orElse(),}) {final _that = this;
switch (_that) {
case _StatusBarView() when $default != null:
return $default(_that.session,_that.runtime,_that.permissionMode,_that.providers,_that.roles,_that.isBusy);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>(TResult Function( StudioSession session,  SessionRuntimeView runtime,  PermissionMode permissionMode,  List<ProviderSettingsView> providers,  List<RoleSettingsView> roles,  bool isBusy)  $default,) {final _that = this;
switch (_that) {
case _StatusBarView():
return $default(_that.session,_that.runtime,_that.permissionMode,_that.providers,_that.roles,_that.isBusy);case _:
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>(TResult? Function( StudioSession session,  SessionRuntimeView runtime,  PermissionMode permissionMode,  List<ProviderSettingsView> providers,  List<RoleSettingsView> roles,  bool isBusy)?  $default,) {final _that = this;
switch (_that) {
case _StatusBarView() when $default != null:
return $default(_that.session,_that.runtime,_that.permissionMode,_that.providers,_that.roles,_that.isBusy);case _:
  return null;

}
}

}

/// @nodoc


class _StatusBarView extends StatusBarView {
  const _StatusBarView({required this.session, required this.runtime, required this.permissionMode, required final  List<ProviderSettingsView> providers, required final  List<RoleSettingsView> roles, required this.isBusy}): _providers = providers,_roles = roles,super._();
  

@override final  StudioSession session;
@override final  SessionRuntimeView runtime;
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

@override final  bool isBusy;

/// Create a copy of StatusBarView
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
_$StatusBarViewCopyWith<_StatusBarView> get copyWith => __$StatusBarViewCopyWithImpl<_StatusBarView>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _StatusBarView&&(identical(other.session, session) || other.session == session)&&(identical(other.runtime, runtime) || other.runtime == runtime)&&(identical(other.permissionMode, permissionMode) || other.permissionMode == permissionMode)&&const DeepCollectionEquality().equals(other._providers, _providers)&&const DeepCollectionEquality().equals(other._roles, _roles)&&(identical(other.isBusy, isBusy) || other.isBusy == isBusy));
}


@override
int get hashCode => Object.hash(runtimeType,session,runtime,permissionMode,const DeepCollectionEquality().hash(_providers),const DeepCollectionEquality().hash(_roles),isBusy);

@override
String toString() {
  return 'StatusBarView(session: $session, runtime: $runtime, permissionMode: $permissionMode, providers: $providers, roles: $roles, isBusy: $isBusy)';
}


}

/// @nodoc
abstract mixin class _$StatusBarViewCopyWith<$Res> implements $StatusBarViewCopyWith<$Res> {
  factory _$StatusBarViewCopyWith(_StatusBarView value, $Res Function(_StatusBarView) _then) = __$StatusBarViewCopyWithImpl;
@override @useResult
$Res call({
 StudioSession session, SessionRuntimeView runtime, PermissionMode permissionMode, List<ProviderSettingsView> providers, List<RoleSettingsView> roles, bool isBusy
});




}
/// @nodoc
class __$StatusBarViewCopyWithImpl<$Res>
    implements _$StatusBarViewCopyWith<$Res> {
  __$StatusBarViewCopyWithImpl(this._self, this._then);

  final _StatusBarView _self;
  final $Res Function(_StatusBarView) _then;

/// Create a copy of StatusBarView
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? session = null,Object? runtime = null,Object? permissionMode = null,Object? providers = null,Object? roles = null,Object? isBusy = null,}) {
  return _then(_StatusBarView(
session: null == session ? _self.session : session // ignore: cast_nullable_to_non_nullable
as StudioSession,runtime: null == runtime ? _self.runtime : runtime // ignore: cast_nullable_to_non_nullable
as SessionRuntimeView,permissionMode: null == permissionMode ? _self.permissionMode : permissionMode // ignore: cast_nullable_to_non_nullable
as PermissionMode,providers: null == providers ? _self._providers : providers // ignore: cast_nullable_to_non_nullable
as List<ProviderSettingsView>,roles: null == roles ? _self._roles : roles // ignore: cast_nullable_to_non_nullable
as List<RoleSettingsView>,isBusy: null == isBusy ? _self.isBusy : isBusy // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

// dart format on

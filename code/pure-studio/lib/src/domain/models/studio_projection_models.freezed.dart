// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'studio_projection_models.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$ShellChromeView {

 List<StudioRecoveryIssue> get applicationRecoveryIssues; ConfigRecoveryNotice? get configRecoveryNotice; PersistenceStateSnapshot get persistenceState;
/// Create a copy of ShellChromeView
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$ShellChromeViewCopyWith<ShellChromeView> get copyWith => _$ShellChromeViewCopyWithImpl<ShellChromeView>(this as ShellChromeView, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ShellChromeView&&const DeepCollectionEquality().equals(other.applicationRecoveryIssues, applicationRecoveryIssues)&&(identical(other.configRecoveryNotice, configRecoveryNotice) || other.configRecoveryNotice == configRecoveryNotice)&&(identical(other.persistenceState, persistenceState) || other.persistenceState == persistenceState));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(applicationRecoveryIssues),configRecoveryNotice,persistenceState);

@override
String toString() {
  return 'ShellChromeView(applicationRecoveryIssues: $applicationRecoveryIssues, configRecoveryNotice: $configRecoveryNotice, persistenceState: $persistenceState)';
}


}

/// @nodoc
abstract mixin class $ShellChromeViewCopyWith<$Res>  {
  factory $ShellChromeViewCopyWith(ShellChromeView value, $Res Function(ShellChromeView) _then) = _$ShellChromeViewCopyWithImpl;
@useResult
$Res call({
 List<StudioRecoveryIssue> applicationRecoveryIssues, ConfigRecoveryNotice? configRecoveryNotice, PersistenceStateSnapshot persistenceState
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
@pragma('vm:prefer-inline') @override $Res call({Object? applicationRecoveryIssues = null,Object? configRecoveryNotice = freezed,Object? persistenceState = null,}) {
  return _then(ShellChromeView(
applicationRecoveryIssues: null == applicationRecoveryIssues ? _self.applicationRecoveryIssues : applicationRecoveryIssues // ignore: cast_nullable_to_non_nullable
as List<StudioRecoveryIssue>,configRecoveryNotice: freezed == configRecoveryNotice ? _self.configRecoveryNotice : configRecoveryNotice // ignore: cast_nullable_to_non_nullable
as ConfigRecoveryNotice?,persistenceState: null == persistenceState ? _self.persistenceState : persistenceState // ignore: cast_nullable_to_non_nullable
as PersistenceStateSnapshot,
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>(TResult Function( List<StudioRecoveryIssue> applicationRecoveryIssues,  ConfigRecoveryNotice? configRecoveryNotice,  PersistenceStateSnapshot persistenceState)?  $default,{required TResult orElse(),}) {final _that = this;
switch (_that) {
case _ShellChromeView() when $default != null:
return $default(_that.applicationRecoveryIssues,_that.configRecoveryNotice,_that.persistenceState);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>(TResult Function( List<StudioRecoveryIssue> applicationRecoveryIssues,  ConfigRecoveryNotice? configRecoveryNotice,  PersistenceStateSnapshot persistenceState)  $default,) {final _that = this;
switch (_that) {
case _ShellChromeView():
return $default(_that.applicationRecoveryIssues,_that.configRecoveryNotice,_that.persistenceState);case _:
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>(TResult? Function( List<StudioRecoveryIssue> applicationRecoveryIssues,  ConfigRecoveryNotice? configRecoveryNotice,  PersistenceStateSnapshot persistenceState)?  $default,) {final _that = this;
switch (_that) {
case _ShellChromeView() when $default != null:
return $default(_that.applicationRecoveryIssues,_that.configRecoveryNotice,_that.persistenceState);case _:
  return null;

}
}

}

/// @nodoc


class _ShellChromeView implements ShellChromeView {
  const _ShellChromeView({required  List<StudioRecoveryIssue> applicationRecoveryIssues, required this.configRecoveryNotice, required this.persistenceState}): _applicationRecoveryIssues = applicationRecoveryIssues;


 final  List<StudioRecoveryIssue> _applicationRecoveryIssues;
@override List<StudioRecoveryIssue> get applicationRecoveryIssues {
  if (_applicationRecoveryIssues is EqualUnmodifiableListView) return _applicationRecoveryIssues;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_applicationRecoveryIssues);
}

@override final  ConfigRecoveryNotice? configRecoveryNotice;
@override final  PersistenceStateSnapshot persistenceState;

/// Create a copy of ShellChromeView
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
_$ShellChromeViewCopyWith<_ShellChromeView> get copyWith => __$ShellChromeViewCopyWithImpl<_ShellChromeView>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _ShellChromeView&&const DeepCollectionEquality().equals(other._applicationRecoveryIssues, _applicationRecoveryIssues)&&(identical(other.configRecoveryNotice, configRecoveryNotice) || other.configRecoveryNotice == configRecoveryNotice)&&(identical(other.persistenceState, persistenceState) || other.persistenceState == persistenceState));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_applicationRecoveryIssues),configRecoveryNotice,persistenceState);

@override
String toString() {
  return 'ShellChromeView(applicationRecoveryIssues: $applicationRecoveryIssues, configRecoveryNotice: $configRecoveryNotice, persistenceState: $persistenceState)';
}


}

/// @nodoc
abstract mixin class _$ShellChromeViewCopyWith<$Res> implements $ShellChromeViewCopyWith<$Res> {
  factory _$ShellChromeViewCopyWith(_ShellChromeView value, $Res Function(_ShellChromeView) _then) = __$ShellChromeViewCopyWithImpl;
@override @useResult
$Res call({
 List<StudioRecoveryIssue> applicationRecoveryIssues, ConfigRecoveryNotice? configRecoveryNotice, PersistenceStateSnapshot persistenceState
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
@override @pragma('vm:prefer-inline') $Res call({Object? applicationRecoveryIssues = null,Object? configRecoveryNotice = freezed,Object? persistenceState = null,}) {
  return _then(_ShellChromeView(
applicationRecoveryIssues: null == applicationRecoveryIssues ? _self._applicationRecoveryIssues : applicationRecoveryIssues // ignore: cast_nullable_to_non_nullable
as List<StudioRecoveryIssue>,configRecoveryNotice: freezed == configRecoveryNotice ? _self.configRecoveryNotice : configRecoveryNotice // ignore: cast_nullable_to_non_nullable
as ConfigRecoveryNotice?,persistenceState: null == persistenceState ? _self.persistenceState : persistenceState // ignore: cast_nullable_to_non_nullable
as PersistenceStateSnapshot,
  ));
}


}

/// @nodoc
mixin _$SidebarView {

 List<StudioProject> get projects; List<StudioThread> get rootThreads; String? get selectedProjectId; String? get selectedRootThreadId; bool get isBusy; Map<String, StudioRecoveryIssue> get projectRecoveryIssues; Map<String, StudioRecoveryIssue> get threadRecoveryIssues; Map<String, String> get modeDisplayNames; bool get directoryHasMore; bool get directoryIsLoading;
/// Create a copy of SidebarView
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SidebarViewCopyWith<SidebarView> get copyWith => _$SidebarViewCopyWithImpl<SidebarView>(this as SidebarView, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is SidebarView&&const DeepCollectionEquality().equals(other.projects, projects)&&const DeepCollectionEquality().equals(other.rootThreads, rootThreads)&&(identical(other.selectedProjectId, selectedProjectId) || other.selectedProjectId == selectedProjectId)&&(identical(other.selectedRootThreadId, selectedRootThreadId) || other.selectedRootThreadId == selectedRootThreadId)&&(identical(other.isBusy, isBusy) || other.isBusy == isBusy)&&const DeepCollectionEquality().equals(other.projectRecoveryIssues, projectRecoveryIssues)&&const DeepCollectionEquality().equals(other.threadRecoveryIssues, threadRecoveryIssues)&&const DeepCollectionEquality().equals(other.modeDisplayNames, modeDisplayNames)&&(identical(other.directoryHasMore, directoryHasMore) || other.directoryHasMore == directoryHasMore)&&(identical(other.directoryIsLoading, directoryIsLoading) || other.directoryIsLoading == directoryIsLoading));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(projects),const DeepCollectionEquality().hash(rootThreads),selectedProjectId,selectedRootThreadId,isBusy,const DeepCollectionEquality().hash(projectRecoveryIssues),const DeepCollectionEquality().hash(threadRecoveryIssues),const DeepCollectionEquality().hash(modeDisplayNames),directoryHasMore,directoryIsLoading);

@override
String toString() {
  return 'SidebarView(projects: $projects, rootThreads: $rootThreads, selectedProjectId: $selectedProjectId, selectedRootThreadId: $selectedRootThreadId, isBusy: $isBusy, projectRecoveryIssues: $projectRecoveryIssues, threadRecoveryIssues: $threadRecoveryIssues, modeDisplayNames: $modeDisplayNames, directoryHasMore: $directoryHasMore, directoryIsLoading: $directoryIsLoading)';
}


}

/// @nodoc
abstract mixin class $SidebarViewCopyWith<$Res>  {
  factory $SidebarViewCopyWith(SidebarView value, $Res Function(SidebarView) _then) = _$SidebarViewCopyWithImpl;
@useResult
$Res call({
 List<StudioProject> projects, List<StudioThread> rootThreads, String? selectedProjectId, String? selectedRootThreadId, bool isBusy, Map<String, StudioRecoveryIssue> projectRecoveryIssues, Map<String, StudioRecoveryIssue> threadRecoveryIssues, Map<String, String> modeDisplayNames, bool directoryHasMore, bool directoryIsLoading
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
@pragma('vm:prefer-inline') @override $Res call({Object? projects = null,Object? rootThreads = null,Object? selectedProjectId = freezed,Object? selectedRootThreadId = freezed,Object? isBusy = null,Object? projectRecoveryIssues = null,Object? threadRecoveryIssues = null,Object? modeDisplayNames = null,Object? directoryHasMore = null,Object? directoryIsLoading = null,}) {
  return _then(SidebarView(
projects: null == projects ? _self.projects : projects // ignore: cast_nullable_to_non_nullable
as List<StudioProject>,rootThreads: null == rootThreads ? _self.rootThreads : rootThreads // ignore: cast_nullable_to_non_nullable
as List<StudioThread>,selectedProjectId: freezed == selectedProjectId ? _self.selectedProjectId : selectedProjectId // ignore: cast_nullable_to_non_nullable
as String?,selectedRootThreadId: freezed == selectedRootThreadId ? _self.selectedRootThreadId : selectedRootThreadId // ignore: cast_nullable_to_non_nullable
as String?,isBusy: null == isBusy ? _self.isBusy : isBusy // ignore: cast_nullable_to_non_nullable
as bool,projectRecoveryIssues: null == projectRecoveryIssues ? _self.projectRecoveryIssues : projectRecoveryIssues // ignore: cast_nullable_to_non_nullable
as Map<String, StudioRecoveryIssue>,threadRecoveryIssues: null == threadRecoveryIssues ? _self.threadRecoveryIssues : threadRecoveryIssues // ignore: cast_nullable_to_non_nullable
as Map<String, StudioRecoveryIssue>,modeDisplayNames: null == modeDisplayNames ? _self.modeDisplayNames : modeDisplayNames // ignore: cast_nullable_to_non_nullable
as Map<String, String>,directoryHasMore: null == directoryHasMore ? _self.directoryHasMore : directoryHasMore // ignore: cast_nullable_to_non_nullable
as bool,directoryIsLoading: null == directoryIsLoading ? _self.directoryIsLoading : directoryIsLoading // ignore: cast_nullable_to_non_nullable
as bool,
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>(TResult Function( List<StudioProject> projects,  List<StudioThread> rootThreads,  String? selectedProjectId,  String? selectedRootThreadId,  bool isBusy,  Map<String, StudioRecoveryIssue> projectRecoveryIssues,  Map<String, StudioRecoveryIssue> threadRecoveryIssues,  Map<String, String> modeDisplayNames,  bool directoryHasMore,  bool directoryIsLoading)?  $default,{required TResult orElse(),}) {final _that = this;
switch (_that) {
case _SidebarView() when $default != null:
return $default(_that.projects,_that.rootThreads,_that.selectedProjectId,_that.selectedRootThreadId,_that.isBusy,_that.projectRecoveryIssues,_that.threadRecoveryIssues,_that.modeDisplayNames,_that.directoryHasMore,_that.directoryIsLoading);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>(TResult Function( List<StudioProject> projects,  List<StudioThread> rootThreads,  String? selectedProjectId,  String? selectedRootThreadId,  bool isBusy,  Map<String, StudioRecoveryIssue> projectRecoveryIssues,  Map<String, StudioRecoveryIssue> threadRecoveryIssues,  Map<String, String> modeDisplayNames,  bool directoryHasMore,  bool directoryIsLoading)  $default,) {final _that = this;
switch (_that) {
case _SidebarView():
return $default(_that.projects,_that.rootThreads,_that.selectedProjectId,_that.selectedRootThreadId,_that.isBusy,_that.projectRecoveryIssues,_that.threadRecoveryIssues,_that.modeDisplayNames,_that.directoryHasMore,_that.directoryIsLoading);case _:
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>(TResult? Function( List<StudioProject> projects,  List<StudioThread> rootThreads,  String? selectedProjectId,  String? selectedRootThreadId,  bool isBusy,  Map<String, StudioRecoveryIssue> projectRecoveryIssues,  Map<String, StudioRecoveryIssue> threadRecoveryIssues,  Map<String, String> modeDisplayNames,  bool directoryHasMore,  bool directoryIsLoading)?  $default,) {final _that = this;
switch (_that) {
case _SidebarView() when $default != null:
return $default(_that.projects,_that.rootThreads,_that.selectedProjectId,_that.selectedRootThreadId,_that.isBusy,_that.projectRecoveryIssues,_that.threadRecoveryIssues,_that.modeDisplayNames,_that.directoryHasMore,_that.directoryIsLoading);case _:
  return null;

}
}

}

/// @nodoc


class _SidebarView extends SidebarView {
  const _SidebarView({required  List<StudioProject> projects, required  List<StudioThread> rootThreads, required this.selectedProjectId, required this.selectedRootThreadId, required this.isBusy, required  Map<String, StudioRecoveryIssue> projectRecoveryIssues, required  Map<String, StudioRecoveryIssue> threadRecoveryIssues, required  Map<String, String> modeDisplayNames, this.directoryHasMore = false, this.directoryIsLoading = false}): _projects = projects,_rootThreads = rootThreads,_projectRecoveryIssues = projectRecoveryIssues,_threadRecoveryIssues = threadRecoveryIssues,_modeDisplayNames = modeDisplayNames,super._();


 final  List<StudioProject> _projects;
@override List<StudioProject> get projects {
  if (_projects is EqualUnmodifiableListView) return _projects;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_projects);
}

 final  List<StudioThread> _rootThreads;
@override List<StudioThread> get rootThreads {
  if (_rootThreads is EqualUnmodifiableListView) return _rootThreads;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_rootThreads);
}

@override final  String? selectedProjectId;
@override final  String? selectedRootThreadId;
@override final  bool isBusy;
 final  Map<String, StudioRecoveryIssue> _projectRecoveryIssues;
@override Map<String, StudioRecoveryIssue> get projectRecoveryIssues {
  if (_projectRecoveryIssues is EqualUnmodifiableMapView) return _projectRecoveryIssues;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableMapView(_projectRecoveryIssues);
}

 final  Map<String, StudioRecoveryIssue> _threadRecoveryIssues;
@override Map<String, StudioRecoveryIssue> get threadRecoveryIssues {
  if (_threadRecoveryIssues is EqualUnmodifiableMapView) return _threadRecoveryIssues;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableMapView(_threadRecoveryIssues);
}

 final  Map<String, String> _modeDisplayNames;
@override Map<String, String> get modeDisplayNames {
  if (_modeDisplayNames is EqualUnmodifiableMapView) return _modeDisplayNames;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableMapView(_modeDisplayNames);
}

@override@JsonKey() final  bool directoryHasMore;
@override@JsonKey() final  bool directoryIsLoading;

/// Create a copy of SidebarView
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
_$SidebarViewCopyWith<_SidebarView> get copyWith => __$SidebarViewCopyWithImpl<_SidebarView>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _SidebarView&&const DeepCollectionEquality().equals(other._projects, _projects)&&const DeepCollectionEquality().equals(other._rootThreads, _rootThreads)&&(identical(other.selectedProjectId, selectedProjectId) || other.selectedProjectId == selectedProjectId)&&(identical(other.selectedRootThreadId, selectedRootThreadId) || other.selectedRootThreadId == selectedRootThreadId)&&(identical(other.isBusy, isBusy) || other.isBusy == isBusy)&&const DeepCollectionEquality().equals(other._projectRecoveryIssues, _projectRecoveryIssues)&&const DeepCollectionEquality().equals(other._threadRecoveryIssues, _threadRecoveryIssues)&&const DeepCollectionEquality().equals(other._modeDisplayNames, _modeDisplayNames)&&(identical(other.directoryHasMore, directoryHasMore) || other.directoryHasMore == directoryHasMore)&&(identical(other.directoryIsLoading, directoryIsLoading) || other.directoryIsLoading == directoryIsLoading));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_projects),const DeepCollectionEquality().hash(_rootThreads),selectedProjectId,selectedRootThreadId,isBusy,const DeepCollectionEquality().hash(_projectRecoveryIssues),const DeepCollectionEquality().hash(_threadRecoveryIssues),const DeepCollectionEquality().hash(_modeDisplayNames),directoryHasMore,directoryIsLoading);

@override
String toString() {
  return 'SidebarView(projects: $projects, rootThreads: $rootThreads, selectedProjectId: $selectedProjectId, selectedRootThreadId: $selectedRootThreadId, isBusy: $isBusy, projectRecoveryIssues: $projectRecoveryIssues, threadRecoveryIssues: $threadRecoveryIssues, modeDisplayNames: $modeDisplayNames, directoryHasMore: $directoryHasMore, directoryIsLoading: $directoryIsLoading)';
}


}

/// @nodoc
abstract mixin class _$SidebarViewCopyWith<$Res> implements $SidebarViewCopyWith<$Res> {
  factory _$SidebarViewCopyWith(_SidebarView value, $Res Function(_SidebarView) _then) = __$SidebarViewCopyWithImpl;
@override @useResult
$Res call({
 List<StudioProject> projects, List<StudioThread> rootThreads, String? selectedProjectId, String? selectedRootThreadId, bool isBusy, Map<String, StudioRecoveryIssue> projectRecoveryIssues, Map<String, StudioRecoveryIssue> threadRecoveryIssues, Map<String, String> modeDisplayNames, bool directoryHasMore, bool directoryIsLoading
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
@override @pragma('vm:prefer-inline') $Res call({Object? projects = null,Object? rootThreads = null,Object? selectedProjectId = freezed,Object? selectedRootThreadId = freezed,Object? isBusy = null,Object? projectRecoveryIssues = null,Object? threadRecoveryIssues = null,Object? modeDisplayNames = null,Object? directoryHasMore = null,Object? directoryIsLoading = null,}) {
  return _then(_SidebarView(
projects: null == projects ? _self._projects : projects // ignore: cast_nullable_to_non_nullable
as List<StudioProject>,rootThreads: null == rootThreads ? _self._rootThreads : rootThreads // ignore: cast_nullable_to_non_nullable
as List<StudioThread>,selectedProjectId: freezed == selectedProjectId ? _self.selectedProjectId : selectedProjectId // ignore: cast_nullable_to_non_nullable
as String?,selectedRootThreadId: freezed == selectedRootThreadId ? _self.selectedRootThreadId : selectedRootThreadId // ignore: cast_nullable_to_non_nullable
as String?,isBusy: null == isBusy ? _self.isBusy : isBusy // ignore: cast_nullable_to_non_nullable
as bool,projectRecoveryIssues: null == projectRecoveryIssues ? _self._projectRecoveryIssues : projectRecoveryIssues // ignore: cast_nullable_to_non_nullable
as Map<String, StudioRecoveryIssue>,threadRecoveryIssues: null == threadRecoveryIssues ? _self._threadRecoveryIssues : threadRecoveryIssues // ignore: cast_nullable_to_non_nullable
as Map<String, StudioRecoveryIssue>,modeDisplayNames: null == modeDisplayNames ? _self._modeDisplayNames : modeDisplayNames // ignore: cast_nullable_to_non_nullable
as Map<String, String>,directoryHasMore: null == directoryHasMore ? _self.directoryHasMore : directoryHasMore // ignore: cast_nullable_to_non_nullable
as bool,directoryIsLoading: null == directoryIsLoading ? _self.directoryIsLoading : directoryIsLoading // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc
mixin _$HeaderView {

 StudioThread? get selectedRootThread; StudioProject? get selectedProject; String? get selectedProjectId; List<StudioThread> get workspaceThreads; List<StudioAgentView> get agents; String? get selectedThreadId; ThreadRuntimeView get runtime; SessionCostView? get sessionCost; List<PendingInteraction> get pendingInteractions;
/// Create a copy of HeaderView
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$HeaderViewCopyWith<HeaderView> get copyWith => _$HeaderViewCopyWithImpl<HeaderView>(this as HeaderView, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is HeaderView&&(identical(other.selectedRootThread, selectedRootThread) || other.selectedRootThread == selectedRootThread)&&(identical(other.selectedProject, selectedProject) || other.selectedProject == selectedProject)&&(identical(other.selectedProjectId, selectedProjectId) || other.selectedProjectId == selectedProjectId)&&const DeepCollectionEquality().equals(other.workspaceThreads, workspaceThreads)&&const DeepCollectionEquality().equals(other.agents, agents)&&(identical(other.selectedThreadId, selectedThreadId) || other.selectedThreadId == selectedThreadId)&&(identical(other.runtime, runtime) || other.runtime == runtime)&&(identical(other.sessionCost, sessionCost) || other.sessionCost == sessionCost)&&const DeepCollectionEquality().equals(other.pendingInteractions, pendingInteractions));
}


@override
int get hashCode => Object.hash(runtimeType,selectedRootThread,selectedProject,selectedProjectId,const DeepCollectionEquality().hash(workspaceThreads),const DeepCollectionEquality().hash(agents),selectedThreadId,runtime,sessionCost,const DeepCollectionEquality().hash(pendingInteractions));

@override
String toString() {
  return 'HeaderView(selectedRootThread: $selectedRootThread, selectedProject: $selectedProject, selectedProjectId: $selectedProjectId, workspaceThreads: $workspaceThreads, agents: $agents, selectedThreadId: $selectedThreadId, runtime: $runtime, sessionCost: $sessionCost, pendingInteractions: $pendingInteractions)';
}


}

/// @nodoc
abstract mixin class $HeaderViewCopyWith<$Res>  {
  factory $HeaderViewCopyWith(HeaderView value, $Res Function(HeaderView) _then) = _$HeaderViewCopyWithImpl;
@useResult
$Res call({
 StudioThread? selectedRootThread, StudioProject? selectedProject, String? selectedProjectId, List<StudioThread> workspaceThreads, List<StudioAgentView> agents, String? selectedThreadId, ThreadRuntimeView runtime, SessionCostView? sessionCost, List<PendingInteraction> pendingInteractions
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
@pragma('vm:prefer-inline') @override $Res call({Object? selectedRootThread = freezed,Object? selectedProject = freezed,Object? selectedProjectId = freezed,Object? workspaceThreads = null,Object? agents = null,Object? selectedThreadId = freezed,Object? runtime = null,Object? sessionCost = freezed,Object? pendingInteractions = null,}) {
  return _then(HeaderView(
selectedRootThread: freezed == selectedRootThread ? _self.selectedRootThread : selectedRootThread // ignore: cast_nullable_to_non_nullable
as StudioThread?,selectedProject: freezed == selectedProject ? _self.selectedProject : selectedProject // ignore: cast_nullable_to_non_nullable
as StudioProject?,selectedProjectId: freezed == selectedProjectId ? _self.selectedProjectId : selectedProjectId // ignore: cast_nullable_to_non_nullable
as String?,workspaceThreads: null == workspaceThreads ? _self.workspaceThreads : workspaceThreads // ignore: cast_nullable_to_non_nullable
as List<StudioThread>,agents: null == agents ? _self.agents : agents // ignore: cast_nullable_to_non_nullable
as List<StudioAgentView>,selectedThreadId: freezed == selectedThreadId ? _self.selectedThreadId : selectedThreadId // ignore: cast_nullable_to_non_nullable
as String?,runtime: null == runtime ? _self.runtime : runtime // ignore: cast_nullable_to_non_nullable
as ThreadRuntimeView,sessionCost: freezed == sessionCost ? _self.sessionCost : sessionCost // ignore: cast_nullable_to_non_nullable
as SessionCostView?,pendingInteractions: null == pendingInteractions ? _self.pendingInteractions : pendingInteractions // ignore: cast_nullable_to_non_nullable
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>(TResult Function( StudioThread? selectedRootThread,  StudioProject? selectedProject,  String? selectedProjectId,  List<StudioThread> workspaceThreads,  List<StudioAgentView> agents,  String? selectedThreadId,  ThreadRuntimeView runtime,  SessionCostView? sessionCost,  List<PendingInteraction> pendingInteractions)?  $default,{required TResult orElse(),}) {final _that = this;
switch (_that) {
case _HeaderView() when $default != null:
return $default(_that.selectedRootThread,_that.selectedProject,_that.selectedProjectId,_that.workspaceThreads,_that.agents,_that.selectedThreadId,_that.runtime,_that.sessionCost,_that.pendingInteractions);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>(TResult Function( StudioThread? selectedRootThread,  StudioProject? selectedProject,  String? selectedProjectId,  List<StudioThread> workspaceThreads,  List<StudioAgentView> agents,  String? selectedThreadId,  ThreadRuntimeView runtime,  SessionCostView? sessionCost,  List<PendingInteraction> pendingInteractions)  $default,) {final _that = this;
switch (_that) {
case _HeaderView():
return $default(_that.selectedRootThread,_that.selectedProject,_that.selectedProjectId,_that.workspaceThreads,_that.agents,_that.selectedThreadId,_that.runtime,_that.sessionCost,_that.pendingInteractions);case _:
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>(TResult? Function( StudioThread? selectedRootThread,  StudioProject? selectedProject,  String? selectedProjectId,  List<StudioThread> workspaceThreads,  List<StudioAgentView> agents,  String? selectedThreadId,  ThreadRuntimeView runtime,  SessionCostView? sessionCost,  List<PendingInteraction> pendingInteractions)?  $default,) {final _that = this;
switch (_that) {
case _HeaderView() when $default != null:
return $default(_that.selectedRootThread,_that.selectedProject,_that.selectedProjectId,_that.workspaceThreads,_that.agents,_that.selectedThreadId,_that.runtime,_that.sessionCost,_that.pendingInteractions);case _:
  return null;

}
}

}

/// @nodoc


class _HeaderView implements HeaderView {
  const _HeaderView({required this.selectedRootThread, required this.selectedProject, required this.selectedProjectId, required  List<StudioThread> workspaceThreads, required  List<StudioAgentView> agents, required this.selectedThreadId, required this.runtime, required this.sessionCost, required  List<PendingInteraction> pendingInteractions}): _workspaceThreads = workspaceThreads,_agents = agents,_pendingInteractions = pendingInteractions;


@override final  StudioThread? selectedRootThread;
@override final  StudioProject? selectedProject;
@override final  String? selectedProjectId;
 final  List<StudioThread> _workspaceThreads;
@override List<StudioThread> get workspaceThreads {
  if (_workspaceThreads is EqualUnmodifiableListView) return _workspaceThreads;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_workspaceThreads);
}

 final  List<StudioAgentView> _agents;
@override List<StudioAgentView> get agents {
  if (_agents is EqualUnmodifiableListView) return _agents;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_agents);
}

@override final  String? selectedThreadId;
@override final  ThreadRuntimeView runtime;
@override final  SessionCostView? sessionCost;
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
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _HeaderView&&(identical(other.selectedRootThread, selectedRootThread) || other.selectedRootThread == selectedRootThread)&&(identical(other.selectedProject, selectedProject) || other.selectedProject == selectedProject)&&(identical(other.selectedProjectId, selectedProjectId) || other.selectedProjectId == selectedProjectId)&&const DeepCollectionEquality().equals(other._workspaceThreads, _workspaceThreads)&&const DeepCollectionEquality().equals(other._agents, _agents)&&(identical(other.selectedThreadId, selectedThreadId) || other.selectedThreadId == selectedThreadId)&&(identical(other.runtime, runtime) || other.runtime == runtime)&&(identical(other.sessionCost, sessionCost) || other.sessionCost == sessionCost)&&const DeepCollectionEquality().equals(other._pendingInteractions, _pendingInteractions));
}


@override
int get hashCode => Object.hash(runtimeType,selectedRootThread,selectedProject,selectedProjectId,const DeepCollectionEquality().hash(_workspaceThreads),const DeepCollectionEquality().hash(_agents),selectedThreadId,runtime,sessionCost,const DeepCollectionEquality().hash(_pendingInteractions));

@override
String toString() {
  return 'HeaderView(selectedRootThread: $selectedRootThread, selectedProject: $selectedProject, selectedProjectId: $selectedProjectId, workspaceThreads: $workspaceThreads, agents: $agents, selectedThreadId: $selectedThreadId, runtime: $runtime, sessionCost: $sessionCost, pendingInteractions: $pendingInteractions)';
}


}

/// @nodoc
abstract mixin class _$HeaderViewCopyWith<$Res> implements $HeaderViewCopyWith<$Res> {
  factory _$HeaderViewCopyWith(_HeaderView value, $Res Function(_HeaderView) _then) = __$HeaderViewCopyWithImpl;
@override @useResult
$Res call({
 StudioThread? selectedRootThread, StudioProject? selectedProject, String? selectedProjectId, List<StudioThread> workspaceThreads, List<StudioAgentView> agents, String? selectedThreadId, ThreadRuntimeView runtime, SessionCostView? sessionCost, List<PendingInteraction> pendingInteractions
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
@override @pragma('vm:prefer-inline') $Res call({Object? selectedRootThread = freezed,Object? selectedProject = freezed,Object? selectedProjectId = freezed,Object? workspaceThreads = null,Object? agents = null,Object? selectedThreadId = freezed,Object? runtime = null,Object? sessionCost = freezed,Object? pendingInteractions = null,}) {
  return _then(_HeaderView(
selectedRootThread: freezed == selectedRootThread ? _self.selectedRootThread : selectedRootThread // ignore: cast_nullable_to_non_nullable
as StudioThread?,selectedProject: freezed == selectedProject ? _self.selectedProject : selectedProject // ignore: cast_nullable_to_non_nullable
as StudioProject?,selectedProjectId: freezed == selectedProjectId ? _self.selectedProjectId : selectedProjectId // ignore: cast_nullable_to_non_nullable
as String?,workspaceThreads: null == workspaceThreads ? _self._workspaceThreads : workspaceThreads // ignore: cast_nullable_to_non_nullable
as List<StudioThread>,agents: null == agents ? _self._agents : agents // ignore: cast_nullable_to_non_nullable
as List<StudioAgentView>,selectedThreadId: freezed == selectedThreadId ? _self.selectedThreadId : selectedThreadId // ignore: cast_nullable_to_non_nullable
as String?,runtime: null == runtime ? _self.runtime : runtime // ignore: cast_nullable_to_non_nullable
as ThreadRuntimeView,sessionCost: freezed == sessionCost ? _self.sessionCost : sessionCost // ignore: cast_nullable_to_non_nullable
as SessionCostView?,pendingInteractions: null == pendingInteractions ? _self._pendingInteractions : pendingInteractions // ignore: cast_nullable_to_non_nullable
as List<PendingInteraction>,
  ));
}


}

/// @nodoc
mixin _$SettingsPageView {

 List<ProviderSettingsView> get providers; ProviderCatalogView get providerCatalog; String? get defaultProviderId; List<RoleSettingsView> get roles; InstructionsSettingsView get instructions; SkillsSettingsView get skills; List<String> get activeSkills; List<String> get catalogSkills; List<SkillSummaryView> get catalogSkillSummaries; int get catalogRevision; String? get selectedProjectId; List<McpServerSettingsView> get mcpServers; McpStateSnapshot get mcpState; LspStateSnapshot get lspState; PermissionMode get permissionMode; GeneralSettingsView get general; WebSearchSettingsView get webSearch; ModelPerformanceSnapshotView get modelPerformance; bool get runtimeBusy;
/// Create a copy of SettingsPageView
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SettingsPageViewCopyWith<SettingsPageView> get copyWith => _$SettingsPageViewCopyWithImpl<SettingsPageView>(this as SettingsPageView, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is SettingsPageView&&const DeepCollectionEquality().equals(other.providers, providers)&&(identical(other.providerCatalog, providerCatalog) || other.providerCatalog == providerCatalog)&&(identical(other.defaultProviderId, defaultProviderId) || other.defaultProviderId == defaultProviderId)&&const DeepCollectionEquality().equals(other.roles, roles)&&(identical(other.instructions, instructions) || other.instructions == instructions)&&(identical(other.skills, skills) || other.skills == skills)&&const DeepCollectionEquality().equals(other.activeSkills, activeSkills)&&const DeepCollectionEquality().equals(other.catalogSkills, catalogSkills)&&const DeepCollectionEquality().equals(other.catalogSkillSummaries, catalogSkillSummaries)&&(identical(other.catalogRevision, catalogRevision) || other.catalogRevision == catalogRevision)&&(identical(other.selectedProjectId, selectedProjectId) || other.selectedProjectId == selectedProjectId)&&const DeepCollectionEquality().equals(other.mcpServers, mcpServers)&&(identical(other.mcpState, mcpState) || other.mcpState == mcpState)&&(identical(other.lspState, lspState) || other.lspState == lspState)&&(identical(other.permissionMode, permissionMode) || other.permissionMode == permissionMode)&&(identical(other.general, general) || other.general == general)&&(identical(other.webSearch, webSearch) || other.webSearch == webSearch)&&(identical(other.modelPerformance, modelPerformance) || other.modelPerformance == modelPerformance)&&(identical(other.runtimeBusy, runtimeBusy) || other.runtimeBusy == runtimeBusy));
}


@override
int get hashCode => Object.hashAll([runtimeType,const DeepCollectionEquality().hash(providers),providerCatalog,defaultProviderId,const DeepCollectionEquality().hash(roles),instructions,skills,const DeepCollectionEquality().hash(activeSkills),const DeepCollectionEquality().hash(catalogSkills),const DeepCollectionEquality().hash(catalogSkillSummaries),catalogRevision,selectedProjectId,const DeepCollectionEquality().hash(mcpServers),mcpState,lspState,permissionMode,general,webSearch,modelPerformance,runtimeBusy]);

@override
String toString() {
  return 'SettingsPageView(providers: $providers, providerCatalog: $providerCatalog, defaultProviderId: $defaultProviderId, roles: $roles, instructions: $instructions, skills: $skills, activeSkills: $activeSkills, catalogSkills: $catalogSkills, catalogSkillSummaries: $catalogSkillSummaries, catalogRevision: $catalogRevision, selectedProjectId: $selectedProjectId, mcpServers: $mcpServers, mcpState: $mcpState, lspState: $lspState, permissionMode: $permissionMode, general: $general, webSearch: $webSearch, modelPerformance: $modelPerformance, runtimeBusy: $runtimeBusy)';
}


}

/// @nodoc
abstract mixin class $SettingsPageViewCopyWith<$Res>  {
  factory $SettingsPageViewCopyWith(SettingsPageView value, $Res Function(SettingsPageView) _then) = _$SettingsPageViewCopyWithImpl;
@useResult
$Res call({
 List<ProviderSettingsView> providers, ProviderCatalogView providerCatalog, String? defaultProviderId, List<RoleSettingsView> roles, InstructionsSettingsView instructions, SkillsSettingsView skills, List<String> activeSkills, List<String> catalogSkills, List<SkillSummaryView> catalogSkillSummaries, int catalogRevision, String? selectedProjectId, List<McpServerSettingsView> mcpServers, McpStateSnapshot mcpState, LspStateSnapshot lspState, PermissionMode permissionMode, GeneralSettingsView general, WebSearchSettingsView webSearch, ModelPerformanceSnapshotView modelPerformance, bool runtimeBusy
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
@pragma('vm:prefer-inline') @override $Res call({Object? providers = null,Object? providerCatalog = null,Object? defaultProviderId = freezed,Object? roles = null,Object? instructions = null,Object? skills = null,Object? activeSkills = null,Object? catalogSkills = null,Object? catalogSkillSummaries = null,Object? catalogRevision = null,Object? selectedProjectId = freezed,Object? mcpServers = null,Object? mcpState = null,Object? lspState = null,Object? permissionMode = null,Object? general = null,Object? webSearch = null,Object? modelPerformance = null,Object? runtimeBusy = null,}) {
  return _then(SettingsPageView(
providers: null == providers ? _self.providers : providers // ignore: cast_nullable_to_non_nullable
as List<ProviderSettingsView>,providerCatalog: null == providerCatalog ? _self.providerCatalog : providerCatalog // ignore: cast_nullable_to_non_nullable
as ProviderCatalogView,defaultProviderId: freezed == defaultProviderId ? _self.defaultProviderId : defaultProviderId // ignore: cast_nullable_to_non_nullable
as String?,roles: null == roles ? _self.roles : roles // ignore: cast_nullable_to_non_nullable
as List<RoleSettingsView>,instructions: null == instructions ? _self.instructions : instructions // ignore: cast_nullable_to_non_nullable
as InstructionsSettingsView,skills: null == skills ? _self.skills : skills // ignore: cast_nullable_to_non_nullable
as SkillsSettingsView,activeSkills: null == activeSkills ? _self.activeSkills : activeSkills // ignore: cast_nullable_to_non_nullable
as List<String>,catalogSkills: null == catalogSkills ? _self.catalogSkills : catalogSkills // ignore: cast_nullable_to_non_nullable
as List<String>,catalogSkillSummaries: null == catalogSkillSummaries ? _self.catalogSkillSummaries : catalogSkillSummaries // ignore: cast_nullable_to_non_nullable
as List<SkillSummaryView>,catalogRevision: null == catalogRevision ? _self.catalogRevision : catalogRevision // ignore: cast_nullable_to_non_nullable
as int,selectedProjectId: freezed == selectedProjectId ? _self.selectedProjectId : selectedProjectId // ignore: cast_nullable_to_non_nullable
as String?,mcpServers: null == mcpServers ? _self.mcpServers : mcpServers // ignore: cast_nullable_to_non_nullable
as List<McpServerSettingsView>,mcpState: null == mcpState ? _self.mcpState : mcpState // ignore: cast_nullable_to_non_nullable
as McpStateSnapshot,lspState: null == lspState ? _self.lspState : lspState // ignore: cast_nullable_to_non_nullable
as LspStateSnapshot,permissionMode: null == permissionMode ? _self.permissionMode : permissionMode // ignore: cast_nullable_to_non_nullable
as PermissionMode,general: null == general ? _self.general : general // ignore: cast_nullable_to_non_nullable
as GeneralSettingsView,webSearch: null == webSearch ? _self.webSearch : webSearch // ignore: cast_nullable_to_non_nullable
as WebSearchSettingsView,modelPerformance: null == modelPerformance ? _self.modelPerformance : modelPerformance // ignore: cast_nullable_to_non_nullable
as ModelPerformanceSnapshotView,runtimeBusy: null == runtimeBusy ? _self.runtimeBusy : runtimeBusy // ignore: cast_nullable_to_non_nullable
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>(TResult Function( List<ProviderSettingsView> providers,  ProviderCatalogView providerCatalog,  String? defaultProviderId,  List<RoleSettingsView> roles,  InstructionsSettingsView instructions,  SkillsSettingsView skills,  List<String> activeSkills,  List<String> catalogSkills,  List<SkillSummaryView> catalogSkillSummaries,  int catalogRevision,  String? selectedProjectId,  List<McpServerSettingsView> mcpServers,  McpStateSnapshot mcpState,  LspStateSnapshot lspState,  PermissionMode permissionMode,  GeneralSettingsView general,  WebSearchSettingsView webSearch,  ModelPerformanceSnapshotView modelPerformance,  bool runtimeBusy)?  $default,{required TResult orElse(),}) {final _that = this;
switch (_that) {
case _SettingsPageView() when $default != null:
return $default(_that.providers,_that.providerCatalog,_that.defaultProviderId,_that.roles,_that.instructions,_that.skills,_that.activeSkills,_that.catalogSkills,_that.catalogSkillSummaries,_that.catalogRevision,_that.selectedProjectId,_that.mcpServers,_that.mcpState,_that.lspState,_that.permissionMode,_that.general,_that.webSearch,_that.modelPerformance,_that.runtimeBusy);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>(TResult Function( List<ProviderSettingsView> providers,  ProviderCatalogView providerCatalog,  String? defaultProviderId,  List<RoleSettingsView> roles,  InstructionsSettingsView instructions,  SkillsSettingsView skills,  List<String> activeSkills,  List<String> catalogSkills,  List<SkillSummaryView> catalogSkillSummaries,  int catalogRevision,  String? selectedProjectId,  List<McpServerSettingsView> mcpServers,  McpStateSnapshot mcpState,  LspStateSnapshot lspState,  PermissionMode permissionMode,  GeneralSettingsView general,  WebSearchSettingsView webSearch,  ModelPerformanceSnapshotView modelPerformance,  bool runtimeBusy)  $default,) {final _that = this;
switch (_that) {
case _SettingsPageView():
return $default(_that.providers,_that.providerCatalog,_that.defaultProviderId,_that.roles,_that.instructions,_that.skills,_that.activeSkills,_that.catalogSkills,_that.catalogSkillSummaries,_that.catalogRevision,_that.selectedProjectId,_that.mcpServers,_that.mcpState,_that.lspState,_that.permissionMode,_that.general,_that.webSearch,_that.modelPerformance,_that.runtimeBusy);case _:
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>(TResult? Function( List<ProviderSettingsView> providers,  ProviderCatalogView providerCatalog,  String? defaultProviderId,  List<RoleSettingsView> roles,  InstructionsSettingsView instructions,  SkillsSettingsView skills,  List<String> activeSkills,  List<String> catalogSkills,  List<SkillSummaryView> catalogSkillSummaries,  int catalogRevision,  String? selectedProjectId,  List<McpServerSettingsView> mcpServers,  McpStateSnapshot mcpState,  LspStateSnapshot lspState,  PermissionMode permissionMode,  GeneralSettingsView general,  WebSearchSettingsView webSearch,  ModelPerformanceSnapshotView modelPerformance,  bool runtimeBusy)?  $default,) {final _that = this;
switch (_that) {
case _SettingsPageView() when $default != null:
return $default(_that.providers,_that.providerCatalog,_that.defaultProviderId,_that.roles,_that.instructions,_that.skills,_that.activeSkills,_that.catalogSkills,_that.catalogSkillSummaries,_that.catalogRevision,_that.selectedProjectId,_that.mcpServers,_that.mcpState,_that.lspState,_that.permissionMode,_that.general,_that.webSearch,_that.modelPerformance,_that.runtimeBusy);case _:
  return null;

}
}

}

/// @nodoc


class _SettingsPageView implements SettingsPageView {
  const _SettingsPageView({required  List<ProviderSettingsView> providers, required this.providerCatalog, required this.defaultProviderId, required  List<RoleSettingsView> roles, required this.instructions, required this.skills, required  List<String> activeSkills, required  List<String> catalogSkills, required  List<SkillSummaryView> catalogSkillSummaries, required this.catalogRevision, required this.selectedProjectId, required  List<McpServerSettingsView> mcpServers, required this.mcpState, required this.lspState, required this.permissionMode, required this.general, required this.webSearch, required this.modelPerformance, required this.runtimeBusy}): _providers = providers,_roles = roles,_activeSkills = activeSkills,_catalogSkills = catalogSkills,_catalogSkillSummaries = catalogSkillSummaries,_mcpServers = mcpServers;


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

 final  List<String> _catalogSkills;
@override List<String> get catalogSkills {
  if (_catalogSkills is EqualUnmodifiableListView) return _catalogSkills;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_catalogSkills);
}

 final  List<SkillSummaryView> _catalogSkillSummaries;
@override List<SkillSummaryView> get catalogSkillSummaries {
  if (_catalogSkillSummaries is EqualUnmodifiableListView) return _catalogSkillSummaries;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_catalogSkillSummaries);
}

@override final  int catalogRevision;
@override final  String? selectedProjectId;
 final  List<McpServerSettingsView> _mcpServers;
@override List<McpServerSettingsView> get mcpServers {
  if (_mcpServers is EqualUnmodifiableListView) return _mcpServers;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_mcpServers);
}

@override final  McpStateSnapshot mcpState;
@override final  LspStateSnapshot lspState;
@override final  PermissionMode permissionMode;
@override final  GeneralSettingsView general;
@override final  WebSearchSettingsView webSearch;
@override final  ModelPerformanceSnapshotView modelPerformance;
@override final  bool runtimeBusy;

/// Create a copy of SettingsPageView
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
_$SettingsPageViewCopyWith<_SettingsPageView> get copyWith => __$SettingsPageViewCopyWithImpl<_SettingsPageView>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _SettingsPageView&&const DeepCollectionEquality().equals(other._providers, _providers)&&(identical(other.providerCatalog, providerCatalog) || other.providerCatalog == providerCatalog)&&(identical(other.defaultProviderId, defaultProviderId) || other.defaultProviderId == defaultProviderId)&&const DeepCollectionEquality().equals(other._roles, _roles)&&(identical(other.instructions, instructions) || other.instructions == instructions)&&(identical(other.skills, skills) || other.skills == skills)&&const DeepCollectionEquality().equals(other._activeSkills, _activeSkills)&&const DeepCollectionEquality().equals(other._catalogSkills, _catalogSkills)&&const DeepCollectionEquality().equals(other._catalogSkillSummaries, _catalogSkillSummaries)&&(identical(other.catalogRevision, catalogRevision) || other.catalogRevision == catalogRevision)&&(identical(other.selectedProjectId, selectedProjectId) || other.selectedProjectId == selectedProjectId)&&const DeepCollectionEquality().equals(other._mcpServers, _mcpServers)&&(identical(other.mcpState, mcpState) || other.mcpState == mcpState)&&(identical(other.lspState, lspState) || other.lspState == lspState)&&(identical(other.permissionMode, permissionMode) || other.permissionMode == permissionMode)&&(identical(other.general, general) || other.general == general)&&(identical(other.webSearch, webSearch) || other.webSearch == webSearch)&&(identical(other.modelPerformance, modelPerformance) || other.modelPerformance == modelPerformance)&&(identical(other.runtimeBusy, runtimeBusy) || other.runtimeBusy == runtimeBusy));
}


@override
int get hashCode => Object.hashAll([runtimeType,const DeepCollectionEquality().hash(_providers),providerCatalog,defaultProviderId,const DeepCollectionEquality().hash(_roles),instructions,skills,const DeepCollectionEquality().hash(_activeSkills),const DeepCollectionEquality().hash(_catalogSkills),const DeepCollectionEquality().hash(_catalogSkillSummaries),catalogRevision,selectedProjectId,const DeepCollectionEquality().hash(_mcpServers),mcpState,lspState,permissionMode,general,webSearch,modelPerformance,runtimeBusy]);

@override
String toString() {
  return 'SettingsPageView(providers: $providers, providerCatalog: $providerCatalog, defaultProviderId: $defaultProviderId, roles: $roles, instructions: $instructions, skills: $skills, activeSkills: $activeSkills, catalogSkills: $catalogSkills, catalogSkillSummaries: $catalogSkillSummaries, catalogRevision: $catalogRevision, selectedProjectId: $selectedProjectId, mcpServers: $mcpServers, mcpState: $mcpState, lspState: $lspState, permissionMode: $permissionMode, general: $general, webSearch: $webSearch, modelPerformance: $modelPerformance, runtimeBusy: $runtimeBusy)';
}


}

/// @nodoc
abstract mixin class _$SettingsPageViewCopyWith<$Res> implements $SettingsPageViewCopyWith<$Res> {
  factory _$SettingsPageViewCopyWith(_SettingsPageView value, $Res Function(_SettingsPageView) _then) = __$SettingsPageViewCopyWithImpl;
@override @useResult
$Res call({
 List<ProviderSettingsView> providers, ProviderCatalogView providerCatalog, String? defaultProviderId, List<RoleSettingsView> roles, InstructionsSettingsView instructions, SkillsSettingsView skills, List<String> activeSkills, List<String> catalogSkills, List<SkillSummaryView> catalogSkillSummaries, int catalogRevision, String? selectedProjectId, List<McpServerSettingsView> mcpServers, McpStateSnapshot mcpState, LspStateSnapshot lspState, PermissionMode permissionMode, GeneralSettingsView general, WebSearchSettingsView webSearch, ModelPerformanceSnapshotView modelPerformance, bool runtimeBusy
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
@override @pragma('vm:prefer-inline') $Res call({Object? providers = null,Object? providerCatalog = null,Object? defaultProviderId = freezed,Object? roles = null,Object? instructions = null,Object? skills = null,Object? activeSkills = null,Object? catalogSkills = null,Object? catalogSkillSummaries = null,Object? catalogRevision = null,Object? selectedProjectId = freezed,Object? mcpServers = null,Object? mcpState = null,Object? lspState = null,Object? permissionMode = null,Object? general = null,Object? webSearch = null,Object? modelPerformance = null,Object? runtimeBusy = null,}) {
  return _then(_SettingsPageView(
providers: null == providers ? _self._providers : providers // ignore: cast_nullable_to_non_nullable
as List<ProviderSettingsView>,providerCatalog: null == providerCatalog ? _self.providerCatalog : providerCatalog // ignore: cast_nullable_to_non_nullable
as ProviderCatalogView,defaultProviderId: freezed == defaultProviderId ? _self.defaultProviderId : defaultProviderId // ignore: cast_nullable_to_non_nullable
as String?,roles: null == roles ? _self._roles : roles // ignore: cast_nullable_to_non_nullable
as List<RoleSettingsView>,instructions: null == instructions ? _self.instructions : instructions // ignore: cast_nullable_to_non_nullable
as InstructionsSettingsView,skills: null == skills ? _self.skills : skills // ignore: cast_nullable_to_non_nullable
as SkillsSettingsView,activeSkills: null == activeSkills ? _self._activeSkills : activeSkills // ignore: cast_nullable_to_non_nullable
as List<String>,catalogSkills: null == catalogSkills ? _self._catalogSkills : catalogSkills // ignore: cast_nullable_to_non_nullable
as List<String>,catalogSkillSummaries: null == catalogSkillSummaries ? _self._catalogSkillSummaries : catalogSkillSummaries // ignore: cast_nullable_to_non_nullable
as List<SkillSummaryView>,catalogRevision: null == catalogRevision ? _self.catalogRevision : catalogRevision // ignore: cast_nullable_to_non_nullable
as int,selectedProjectId: freezed == selectedProjectId ? _self.selectedProjectId : selectedProjectId // ignore: cast_nullable_to_non_nullable
as String?,mcpServers: null == mcpServers ? _self._mcpServers : mcpServers // ignore: cast_nullable_to_non_nullable
as List<McpServerSettingsView>,mcpState: null == mcpState ? _self.mcpState : mcpState // ignore: cast_nullable_to_non_nullable
as McpStateSnapshot,lspState: null == lspState ? _self.lspState : lspState // ignore: cast_nullable_to_non_nullable
as LspStateSnapshot,permissionMode: null == permissionMode ? _self.permissionMode : permissionMode // ignore: cast_nullable_to_non_nullable
as PermissionMode,general: null == general ? _self.general : general // ignore: cast_nullable_to_non_nullable
as GeneralSettingsView,webSearch: null == webSearch ? _self.webSearch : webSearch // ignore: cast_nullable_to_non_nullable
as WebSearchSettingsView,modelPerformance: null == modelPerformance ? _self.modelPerformance : modelPerformance // ignore: cast_nullable_to_non_nullable
as ModelPerformanceSnapshotView,runtimeBusy: null == runtimeBusy ? _self.runtimeBusy : runtimeBusy // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

/// @nodoc
mixin _$StatusBarView {

 StudioThread get thread; ThreadRuntimeView get runtime; PermissionMode get permissionMode; List<ProviderSettingsView> get providers; List<RoleSettingsView> get roles; bool get isBusy;
/// Create a copy of StatusBarView
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$StatusBarViewCopyWith<StatusBarView> get copyWith => _$StatusBarViewCopyWithImpl<StatusBarView>(this as StatusBarView, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is StatusBarView&&(identical(other.thread, thread) || other.thread == thread)&&(identical(other.runtime, runtime) || other.runtime == runtime)&&(identical(other.permissionMode, permissionMode) || other.permissionMode == permissionMode)&&const DeepCollectionEquality().equals(other.providers, providers)&&const DeepCollectionEquality().equals(other.roles, roles)&&(identical(other.isBusy, isBusy) || other.isBusy == isBusy));
}


@override
int get hashCode => Object.hash(runtimeType,thread,runtime,permissionMode,const DeepCollectionEquality().hash(providers),const DeepCollectionEquality().hash(roles),isBusy);

@override
String toString() {
  return 'StatusBarView(thread: $thread, runtime: $runtime, permissionMode: $permissionMode, providers: $providers, roles: $roles, isBusy: $isBusy)';
}


}

/// @nodoc
abstract mixin class $StatusBarViewCopyWith<$Res>  {
  factory $StatusBarViewCopyWith(StatusBarView value, $Res Function(StatusBarView) _then) = _$StatusBarViewCopyWithImpl;
@useResult
$Res call({
 StudioThread thread, ThreadRuntimeView runtime, PermissionMode permissionMode, List<ProviderSettingsView> providers, List<RoleSettingsView> roles, bool isBusy
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
@pragma('vm:prefer-inline') @override $Res call({Object? thread = null,Object? runtime = null,Object? permissionMode = null,Object? providers = null,Object? roles = null,Object? isBusy = null,}) {
  return _then(StatusBarView(
thread: null == thread ? _self.thread : thread // ignore: cast_nullable_to_non_nullable
as StudioThread,runtime: null == runtime ? _self.runtime : runtime // ignore: cast_nullable_to_non_nullable
as ThreadRuntimeView,permissionMode: null == permissionMode ? _self.permissionMode : permissionMode // ignore: cast_nullable_to_non_nullable
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>(TResult Function( StudioThread thread,  ThreadRuntimeView runtime,  PermissionMode permissionMode,  List<ProviderSettingsView> providers,  List<RoleSettingsView> roles,  bool isBusy)?  $default,{required TResult orElse(),}) {final _that = this;
switch (_that) {
case _StatusBarView() when $default != null:
return $default(_that.thread,_that.runtime,_that.permissionMode,_that.providers,_that.roles,_that.isBusy);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>(TResult Function( StudioThread thread,  ThreadRuntimeView runtime,  PermissionMode permissionMode,  List<ProviderSettingsView> providers,  List<RoleSettingsView> roles,  bool isBusy)  $default,) {final _that = this;
switch (_that) {
case _StatusBarView():
return $default(_that.thread,_that.runtime,_that.permissionMode,_that.providers,_that.roles,_that.isBusy);case _:
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>(TResult? Function( StudioThread thread,  ThreadRuntimeView runtime,  PermissionMode permissionMode,  List<ProviderSettingsView> providers,  List<RoleSettingsView> roles,  bool isBusy)?  $default,) {final _that = this;
switch (_that) {
case _StatusBarView() when $default != null:
return $default(_that.thread,_that.runtime,_that.permissionMode,_that.providers,_that.roles,_that.isBusy);case _:
  return null;

}
}

}

/// @nodoc


class _StatusBarView extends StatusBarView {
  const _StatusBarView({required this.thread, required this.runtime, required this.permissionMode, required  List<ProviderSettingsView> providers, required  List<RoleSettingsView> roles, required this.isBusy}): _providers = providers,_roles = roles,super._();


@override final  StudioThread thread;
@override final  ThreadRuntimeView runtime;
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
  return identical(this, other) || (other.runtimeType == runtimeType&&other is _StatusBarView&&(identical(other.thread, thread) || other.thread == thread)&&(identical(other.runtime, runtime) || other.runtime == runtime)&&(identical(other.permissionMode, permissionMode) || other.permissionMode == permissionMode)&&const DeepCollectionEquality().equals(other._providers, _providers)&&const DeepCollectionEquality().equals(other._roles, _roles)&&(identical(other.isBusy, isBusy) || other.isBusy == isBusy));
}


@override
int get hashCode => Object.hash(runtimeType,thread,runtime,permissionMode,const DeepCollectionEquality().hash(_providers),const DeepCollectionEquality().hash(_roles),isBusy);

@override
String toString() {
  return 'StatusBarView(thread: $thread, runtime: $runtime, permissionMode: $permissionMode, providers: $providers, roles: $roles, isBusy: $isBusy)';
}


}

/// @nodoc
abstract mixin class _$StatusBarViewCopyWith<$Res> implements $StatusBarViewCopyWith<$Res> {
  factory _$StatusBarViewCopyWith(_StatusBarView value, $Res Function(_StatusBarView) _then) = __$StatusBarViewCopyWithImpl;
@override @useResult
$Res call({
 StudioThread thread, ThreadRuntimeView runtime, PermissionMode permissionMode, List<ProviderSettingsView> providers, List<RoleSettingsView> roles, bool isBusy
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
@override @pragma('vm:prefer-inline') $Res call({Object? thread = null,Object? runtime = null,Object? permissionMode = null,Object? providers = null,Object? roles = null,Object? isBusy = null,}) {
  return _then(_StatusBarView(
thread: null == thread ? _self.thread : thread // ignore: cast_nullable_to_non_nullable
as StudioThread,runtime: null == runtime ? _self.runtime : runtime // ignore: cast_nullable_to_non_nullable
as ThreadRuntimeView,permissionMode: null == permissionMode ? _self.permissionMode : permissionMode // ignore: cast_nullable_to_non_nullable
as PermissionMode,providers: null == providers ? _self._providers : providers // ignore: cast_nullable_to_non_nullable
as List<ProviderSettingsView>,roles: null == roles ? _self._roles : roles // ignore: cast_nullable_to_non_nullable
as List<RoleSettingsView>,isBusy: null == isBusy ? _self.isBusy : isBusy // ignore: cast_nullable_to_non_nullable
as bool,
  ));
}


}

// dart format on

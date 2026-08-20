// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'settings.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$LspScopeInput {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is LspScopeInput);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'LspScopeInput()';
}


}

/// @nodoc
class $LspScopeInputCopyWith<$Res>  {
$LspScopeInputCopyWith(LspScopeInput _, $Res Function(LspScopeInput) __);
}


/// Adds pattern-matching-related methods to [LspScopeInput].
extension LspScopeInputPatterns on LspScopeInput {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( LspScopeInput_Server value)?  server,TResult Function( LspScopeInput_Workspace value)?  workspace,TResult Function( LspScopeInput_All value)?  all,required TResult orElse(),}){
final _that = this;
switch (_that) {
case LspScopeInput_Server() when server != null:
return server(_that);case LspScopeInput_Workspace() when workspace != null:
return workspace(_that);case LspScopeInput_All() when all != null:
return all(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( LspScopeInput_Server value)  server,required TResult Function( LspScopeInput_Workspace value)  workspace,required TResult Function( LspScopeInput_All value)  all,}){
final _that = this;
switch (_that) {
case LspScopeInput_Server():
return server(_that);case LspScopeInput_Workspace():
return workspace(_that);case LspScopeInput_All():
return all(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( LspScopeInput_Server value)?  server,TResult? Function( LspScopeInput_Workspace value)?  workspace,TResult? Function( LspScopeInput_All value)?  all,}){
final _that = this;
switch (_that) {
case LspScopeInput_Server() when server != null:
return server(_that);case LspScopeInput_Workspace() when workspace != null:
return workspace(_that);case LspScopeInput_All() when all != null:
return all(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String projectId,  String serverId)?  server,TResult Function( String projectId)?  workspace,TResult Function()?  all,required TResult orElse(),}) {final _that = this;
switch (_that) {
case LspScopeInput_Server() when server != null:
return server(_that.projectId,_that.serverId);case LspScopeInput_Workspace() when workspace != null:
return workspace(_that.projectId);case LspScopeInput_All() when all != null:
return all();case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String projectId,  String serverId)  server,required TResult Function( String projectId)  workspace,required TResult Function()  all,}) {final _that = this;
switch (_that) {
case LspScopeInput_Server():
return server(_that.projectId,_that.serverId);case LspScopeInput_Workspace():
return workspace(_that.projectId);case LspScopeInput_All():
return all();}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String projectId,  String serverId)?  server,TResult? Function( String projectId)?  workspace,TResult? Function()?  all,}) {final _that = this;
switch (_that) {
case LspScopeInput_Server() when server != null:
return server(_that.projectId,_that.serverId);case LspScopeInput_Workspace() when workspace != null:
return workspace(_that.projectId);case LspScopeInput_All() when all != null:
return all();case _:
  return null;

}
}

}

/// @nodoc


class LspScopeInput_Server extends LspScopeInput {
  const LspScopeInput_Server({required this.projectId, required this.serverId}): super._();


 final  String projectId;
 final  String serverId;

/// Create a copy of LspScopeInput
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$LspScopeInput_ServerCopyWith<LspScopeInput_Server> get copyWith => _$LspScopeInput_ServerCopyWithImpl<LspScopeInput_Server>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is LspScopeInput_Server&&(identical(other.projectId, projectId) || other.projectId == projectId)&&(identical(other.serverId, serverId) || other.serverId == serverId));
}


@override
int get hashCode => Object.hash(runtimeType,projectId,serverId);

@override
String toString() {
  return 'LspScopeInput.server(projectId: $projectId, serverId: $serverId)';
}


}

/// @nodoc
abstract mixin class $LspScopeInput_ServerCopyWith<$Res> implements $LspScopeInputCopyWith<$Res> {
  factory $LspScopeInput_ServerCopyWith(LspScopeInput_Server value, $Res Function(LspScopeInput_Server) _then) = _$LspScopeInput_ServerCopyWithImpl;
@useResult
$Res call({
 String projectId, String serverId
});




}
/// @nodoc
class _$LspScopeInput_ServerCopyWithImpl<$Res>
    implements $LspScopeInput_ServerCopyWith<$Res> {
  _$LspScopeInput_ServerCopyWithImpl(this._self, this._then);

  final LspScopeInput_Server _self;
  final $Res Function(LspScopeInput_Server) _then;

/// Create a copy of LspScopeInput
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? projectId = null,Object? serverId = null,}) {
  return _then(LspScopeInput_Server(
projectId: null == projectId ? _self.projectId : projectId // ignore: cast_nullable_to_non_nullable
as String,serverId: null == serverId ? _self.serverId : serverId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class LspScopeInput_Workspace extends LspScopeInput {
  const LspScopeInput_Workspace({required this.projectId}): super._();


 final  String projectId;

/// Create a copy of LspScopeInput
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$LspScopeInput_WorkspaceCopyWith<LspScopeInput_Workspace> get copyWith => _$LspScopeInput_WorkspaceCopyWithImpl<LspScopeInput_Workspace>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is LspScopeInput_Workspace&&(identical(other.projectId, projectId) || other.projectId == projectId));
}


@override
int get hashCode => Object.hash(runtimeType,projectId);

@override
String toString() {
  return 'LspScopeInput.workspace(projectId: $projectId)';
}


}

/// @nodoc
abstract mixin class $LspScopeInput_WorkspaceCopyWith<$Res> implements $LspScopeInputCopyWith<$Res> {
  factory $LspScopeInput_WorkspaceCopyWith(LspScopeInput_Workspace value, $Res Function(LspScopeInput_Workspace) _then) = _$LspScopeInput_WorkspaceCopyWithImpl;
@useResult
$Res call({
 String projectId
});




}
/// @nodoc
class _$LspScopeInput_WorkspaceCopyWithImpl<$Res>
    implements $LspScopeInput_WorkspaceCopyWith<$Res> {
  _$LspScopeInput_WorkspaceCopyWithImpl(this._self, this._then);

  final LspScopeInput_Workspace _self;
  final $Res Function(LspScopeInput_Workspace) _then;

/// Create a copy of LspScopeInput
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? projectId = null,}) {
  return _then(LspScopeInput_Workspace(
projectId: null == projectId ? _self.projectId : projectId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class LspScopeInput_All extends LspScopeInput {
  const LspScopeInput_All(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is LspScopeInput_All);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'LspScopeInput.all()';
}


}




/// @nodoc
mixin _$McpResetInput {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is McpResetInput);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'McpResetInput()';
}


}

/// @nodoc
class $McpResetInputCopyWith<$Res>  {
$McpResetInputCopyWith(McpResetInput _, $Res Function(McpResetInput) __);
}


/// Adds pattern-matching-related methods to [McpResetInput].
extension McpResetInputPatterns on McpResetInput {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( McpResetInput_Server value)?  server,TResult Function( McpResetInput_All value)?  all,required TResult orElse(),}){
final _that = this;
switch (_that) {
case McpResetInput_Server() when server != null:
return server(_that);case McpResetInput_All() when all != null:
return all(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( McpResetInput_Server value)  server,required TResult Function( McpResetInput_All value)  all,}){
final _that = this;
switch (_that) {
case McpResetInput_Server():
return server(_that);case McpResetInput_All():
return all(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( McpResetInput_Server value)?  server,TResult? Function( McpResetInput_All value)?  all,}){
final _that = this;
switch (_that) {
case McpResetInput_Server() when server != null:
return server(_that);case McpResetInput_All() when all != null:
return all(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String serverId)?  server,TResult Function()?  all,required TResult orElse(),}) {final _that = this;
switch (_that) {
case McpResetInput_Server() when server != null:
return server(_that.serverId);case McpResetInput_All() when all != null:
return all();case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String serverId)  server,required TResult Function()  all,}) {final _that = this;
switch (_that) {
case McpResetInput_Server():
return server(_that.serverId);case McpResetInput_All():
return all();}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String serverId)?  server,TResult? Function()?  all,}) {final _that = this;
switch (_that) {
case McpResetInput_Server() when server != null:
return server(_that.serverId);case McpResetInput_All() when all != null:
return all();case _:
  return null;

}
}

}

/// @nodoc


class McpResetInput_Server extends McpResetInput {
  const McpResetInput_Server({required this.serverId}): super._();


 final  String serverId;

/// Create a copy of McpResetInput
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$McpResetInput_ServerCopyWith<McpResetInput_Server> get copyWith => _$McpResetInput_ServerCopyWithImpl<McpResetInput_Server>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is McpResetInput_Server&&(identical(other.serverId, serverId) || other.serverId == serverId));
}


@override
int get hashCode => Object.hash(runtimeType,serverId);

@override
String toString() {
  return 'McpResetInput.server(serverId: $serverId)';
}


}

/// @nodoc
abstract mixin class $McpResetInput_ServerCopyWith<$Res> implements $McpResetInputCopyWith<$Res> {
  factory $McpResetInput_ServerCopyWith(McpResetInput_Server value, $Res Function(McpResetInput_Server) _then) = _$McpResetInput_ServerCopyWithImpl;
@useResult
$Res call({
 String serverId
});




}
/// @nodoc
class _$McpResetInput_ServerCopyWithImpl<$Res>
    implements $McpResetInput_ServerCopyWith<$Res> {
  _$McpResetInput_ServerCopyWithImpl(this._self, this._then);

  final McpResetInput_Server _self;
  final $Res Function(McpResetInput_Server) _then;

/// Create a copy of McpResetInput
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? serverId = null,}) {
  return _then(McpResetInput_Server(
serverId: null == serverId ? _self.serverId : serverId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class McpResetInput_All extends McpResetInput {
  const McpResetInput_All(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is McpResetInput_All);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'McpResetInput.all()';
}


}




/// @nodoc
mixin _$ProviderSecretInput {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ProviderSecretInput);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'ProviderSecretInput()';
}


}

/// @nodoc
class $ProviderSecretInputCopyWith<$Res>  {
$ProviderSecretInputCopyWith(ProviderSecretInput _, $Res Function(ProviderSecretInput) __);
}


/// Adds pattern-matching-related methods to [ProviderSecretInput].
extension ProviderSecretInputPatterns on ProviderSecretInput {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( ProviderSecretInput_Preserve value)?  preserve,TResult Function( ProviderSecretInput_Replace value)?  replace,TResult Function( ProviderSecretInput_Clear value)?  clear,required TResult orElse(),}){
final _that = this;
switch (_that) {
case ProviderSecretInput_Preserve() when preserve != null:
return preserve(_that);case ProviderSecretInput_Replace() when replace != null:
return replace(_that);case ProviderSecretInput_Clear() when clear != null:
return clear(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( ProviderSecretInput_Preserve value)  preserve,required TResult Function( ProviderSecretInput_Replace value)  replace,required TResult Function( ProviderSecretInput_Clear value)  clear,}){
final _that = this;
switch (_that) {
case ProviderSecretInput_Preserve():
return preserve(_that);case ProviderSecretInput_Replace():
return replace(_that);case ProviderSecretInput_Clear():
return clear(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( ProviderSecretInput_Preserve value)?  preserve,TResult? Function( ProviderSecretInput_Replace value)?  replace,TResult? Function( ProviderSecretInput_Clear value)?  clear,}){
final _that = this;
switch (_that) {
case ProviderSecretInput_Preserve() when preserve != null:
return preserve(_that);case ProviderSecretInput_Replace() when replace != null:
return replace(_that);case ProviderSecretInput_Clear() when clear != null:
return clear(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  preserve,TResult Function( String value)?  replace,TResult Function()?  clear,required TResult orElse(),}) {final _that = this;
switch (_that) {
case ProviderSecretInput_Preserve() when preserve != null:
return preserve();case ProviderSecretInput_Replace() when replace != null:
return replace(_that.value);case ProviderSecretInput_Clear() when clear != null:
return clear();case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  preserve,required TResult Function( String value)  replace,required TResult Function()  clear,}) {final _that = this;
switch (_that) {
case ProviderSecretInput_Preserve():
return preserve();case ProviderSecretInput_Replace():
return replace(_that.value);case ProviderSecretInput_Clear():
return clear();}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  preserve,TResult? Function( String value)?  replace,TResult? Function()?  clear,}) {final _that = this;
switch (_that) {
case ProviderSecretInput_Preserve() when preserve != null:
return preserve();case ProviderSecretInput_Replace() when replace != null:
return replace(_that.value);case ProviderSecretInput_Clear() when clear != null:
return clear();case _:
  return null;

}
}

}

/// @nodoc


class ProviderSecretInput_Preserve extends ProviderSecretInput {
  const ProviderSecretInput_Preserve(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ProviderSecretInput_Preserve);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'ProviderSecretInput.preserve()';
}


}




/// @nodoc


class ProviderSecretInput_Replace extends ProviderSecretInput {
  const ProviderSecretInput_Replace({required this.value}): super._();


 final  String value;

/// Create a copy of ProviderSecretInput
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$ProviderSecretInput_ReplaceCopyWith<ProviderSecretInput_Replace> get copyWith => _$ProviderSecretInput_ReplaceCopyWithImpl<ProviderSecretInput_Replace>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ProviderSecretInput_Replace&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode => Object.hash(runtimeType,value);

@override
String toString() {
  return 'ProviderSecretInput.replace(value: $value)';
}


}

/// @nodoc
abstract mixin class $ProviderSecretInput_ReplaceCopyWith<$Res> implements $ProviderSecretInputCopyWith<$Res> {
  factory $ProviderSecretInput_ReplaceCopyWith(ProviderSecretInput_Replace value, $Res Function(ProviderSecretInput_Replace) _then) = _$ProviderSecretInput_ReplaceCopyWithImpl;
@useResult
$Res call({
 String value
});




}
/// @nodoc
class _$ProviderSecretInput_ReplaceCopyWithImpl<$Res>
    implements $ProviderSecretInput_ReplaceCopyWith<$Res> {
  _$ProviderSecretInput_ReplaceCopyWithImpl(this._self, this._then);

  final ProviderSecretInput_Replace _self;
  final $Res Function(ProviderSecretInput_Replace) _then;

/// Create a copy of ProviderSecretInput
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? value = null,}) {
  return _then(ProviderSecretInput_Replace(
value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class ProviderSecretInput_Clear extends ProviderSecretInput {
  const ProviderSecretInput_Clear(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is ProviderSecretInput_Clear);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'ProviderSecretInput.clear()';
}


}




// dart format on

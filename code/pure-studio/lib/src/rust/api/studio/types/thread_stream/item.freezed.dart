// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'item.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeSkillActivationCause {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillActivationCause);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeSkillActivationCause()';
}


}

/// @nodoc
class $BridgeSkillActivationCauseCopyWith<$Res>  {
$BridgeSkillActivationCauseCopyWith(BridgeSkillActivationCause _, $Res Function(BridgeSkillActivationCause) __);
}


/// Adds pattern-matching-related methods to [BridgeSkillActivationCause].
extension BridgeSkillActivationCausePatterns on BridgeSkillActivationCause {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeSkillActivationCause_Tool value)?  tool,TResult Function( BridgeSkillActivationCause_UserGesture value)?  userGesture,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeSkillActivationCause_Tool() when tool != null:
return tool(_that);case BridgeSkillActivationCause_UserGesture() when userGesture != null:
return userGesture(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeSkillActivationCause_Tool value)  tool,required TResult Function( BridgeSkillActivationCause_UserGesture value)  userGesture,}){
final _that = this;
switch (_that) {
case BridgeSkillActivationCause_Tool():
return tool(_that);case BridgeSkillActivationCause_UserGesture():
return userGesture(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeSkillActivationCause_Tool value)?  tool,TResult? Function( BridgeSkillActivationCause_UserGesture value)?  userGesture,}){
final _that = this;
switch (_that) {
case BridgeSkillActivationCause_Tool() when tool != null:
return tool(_that);case BridgeSkillActivationCause_UserGesture() when userGesture != null:
return userGesture(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String toolCallId)?  tool,TResult Function( String invocationId)?  userGesture,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeSkillActivationCause_Tool() when tool != null:
return tool(_that.toolCallId);case BridgeSkillActivationCause_UserGesture() when userGesture != null:
return userGesture(_that.invocationId);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String toolCallId)  tool,required TResult Function( String invocationId)  userGesture,}) {final _that = this;
switch (_that) {
case BridgeSkillActivationCause_Tool():
return tool(_that.toolCallId);case BridgeSkillActivationCause_UserGesture():
return userGesture(_that.invocationId);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String toolCallId)?  tool,TResult? Function( String invocationId)?  userGesture,}) {final _that = this;
switch (_that) {
case BridgeSkillActivationCause_Tool() when tool != null:
return tool(_that.toolCallId);case BridgeSkillActivationCause_UserGesture() when userGesture != null:
return userGesture(_that.invocationId);case _:
  return null;

}
}

}

/// @nodoc


class BridgeSkillActivationCause_Tool extends BridgeSkillActivationCause {
  const BridgeSkillActivationCause_Tool({required this.toolCallId}): super._();


 final  String toolCallId;

/// Create a copy of BridgeSkillActivationCause
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSkillActivationCause_ToolCopyWith<BridgeSkillActivationCause_Tool> get copyWith => _$BridgeSkillActivationCause_ToolCopyWithImpl<BridgeSkillActivationCause_Tool>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillActivationCause_Tool&&(identical(other.toolCallId, toolCallId) || other.toolCallId == toolCallId));
}


@override
int get hashCode => Object.hash(runtimeType,toolCallId);

@override
String toString() {
  return 'BridgeSkillActivationCause.tool(toolCallId: $toolCallId)';
}


}

/// @nodoc
abstract mixin class $BridgeSkillActivationCause_ToolCopyWith<$Res> implements $BridgeSkillActivationCauseCopyWith<$Res> {
  factory $BridgeSkillActivationCause_ToolCopyWith(BridgeSkillActivationCause_Tool value, $Res Function(BridgeSkillActivationCause_Tool) _then) = _$BridgeSkillActivationCause_ToolCopyWithImpl;
@useResult
$Res call({
 String toolCallId
});




}
/// @nodoc
class _$BridgeSkillActivationCause_ToolCopyWithImpl<$Res>
    implements $BridgeSkillActivationCause_ToolCopyWith<$Res> {
  _$BridgeSkillActivationCause_ToolCopyWithImpl(this._self, this._then);

  final BridgeSkillActivationCause_Tool _self;
  final $Res Function(BridgeSkillActivationCause_Tool) _then;

/// Create a copy of BridgeSkillActivationCause
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? toolCallId = null,}) {
  return _then(BridgeSkillActivationCause_Tool(
toolCallId: null == toolCallId ? _self.toolCallId : toolCallId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeSkillActivationCause_UserGesture extends BridgeSkillActivationCause {
  const BridgeSkillActivationCause_UserGesture({required this.invocationId}): super._();


 final  String invocationId;

/// Create a copy of BridgeSkillActivationCause
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSkillActivationCause_UserGestureCopyWith<BridgeSkillActivationCause_UserGesture> get copyWith => _$BridgeSkillActivationCause_UserGestureCopyWithImpl<BridgeSkillActivationCause_UserGesture>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillActivationCause_UserGesture&&(identical(other.invocationId, invocationId) || other.invocationId == invocationId));
}


@override
int get hashCode => Object.hash(runtimeType,invocationId);

@override
String toString() {
  return 'BridgeSkillActivationCause.userGesture(invocationId: $invocationId)';
}


}

/// @nodoc
abstract mixin class $BridgeSkillActivationCause_UserGestureCopyWith<$Res> implements $BridgeSkillActivationCauseCopyWith<$Res> {
  factory $BridgeSkillActivationCause_UserGestureCopyWith(BridgeSkillActivationCause_UserGesture value, $Res Function(BridgeSkillActivationCause_UserGesture) _then) = _$BridgeSkillActivationCause_UserGestureCopyWithImpl;
@useResult
$Res call({
 String invocationId
});




}
/// @nodoc
class _$BridgeSkillActivationCause_UserGestureCopyWithImpl<$Res>
    implements $BridgeSkillActivationCause_UserGestureCopyWith<$Res> {
  _$BridgeSkillActivationCause_UserGestureCopyWithImpl(this._self, this._then);

  final BridgeSkillActivationCause_UserGesture _self;
  final $Res Function(BridgeSkillActivationCause_UserGesture) _then;

/// Create a copy of BridgeSkillActivationCause
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? invocationId = null,}) {
  return _then(BridgeSkillActivationCause_UserGesture(
invocationId: null == invocationId ? _self.invocationId : invocationId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeSkillResourceBase {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillResourceBase);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeSkillResourceBase()';
}


}

/// @nodoc
class $BridgeSkillResourceBaseCopyWith<$Res>  {
$BridgeSkillResourceBaseCopyWith(BridgeSkillResourceBase _, $Res Function(BridgeSkillResourceBase) __);
}


/// Adds pattern-matching-related methods to [BridgeSkillResourceBase].
extension BridgeSkillResourceBasePatterns on BridgeSkillResourceBase {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeSkillResourceBase_Directory value)?  directory,TResult Function( BridgeSkillResourceBase_Url value)?  url,TResult Function( BridgeSkillResourceBase_Opaque value)?  opaque,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeSkillResourceBase_Directory() when directory != null:
return directory(_that);case BridgeSkillResourceBase_Url() when url != null:
return url(_that);case BridgeSkillResourceBase_Opaque() when opaque != null:
return opaque(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeSkillResourceBase_Directory value)  directory,required TResult Function( BridgeSkillResourceBase_Url value)  url,required TResult Function( BridgeSkillResourceBase_Opaque value)  opaque,}){
final _that = this;
switch (_that) {
case BridgeSkillResourceBase_Directory():
return directory(_that);case BridgeSkillResourceBase_Url():
return url(_that);case BridgeSkillResourceBase_Opaque():
return opaque(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeSkillResourceBase_Directory value)?  directory,TResult? Function( BridgeSkillResourceBase_Url value)?  url,TResult? Function( BridgeSkillResourceBase_Opaque value)?  opaque,}){
final _that = this;
switch (_that) {
case BridgeSkillResourceBase_Directory() when directory != null:
return directory(_that);case BridgeSkillResourceBase_Url() when url != null:
return url(_that);case BridgeSkillResourceBase_Opaque() when opaque != null:
return opaque(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String path)?  directory,TResult Function( String url)?  url,TResult Function( String description)?  opaque,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeSkillResourceBase_Directory() when directory != null:
return directory(_that.path);case BridgeSkillResourceBase_Url() when url != null:
return url(_that.url);case BridgeSkillResourceBase_Opaque() when opaque != null:
return opaque(_that.description);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String path)  directory,required TResult Function( String url)  url,required TResult Function( String description)  opaque,}) {final _that = this;
switch (_that) {
case BridgeSkillResourceBase_Directory():
return directory(_that.path);case BridgeSkillResourceBase_Url():
return url(_that.url);case BridgeSkillResourceBase_Opaque():
return opaque(_that.description);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String path)?  directory,TResult? Function( String url)?  url,TResult? Function( String description)?  opaque,}) {final _that = this;
switch (_that) {
case BridgeSkillResourceBase_Directory() when directory != null:
return directory(_that.path);case BridgeSkillResourceBase_Url() when url != null:
return url(_that.url);case BridgeSkillResourceBase_Opaque() when opaque != null:
return opaque(_that.description);case _:
  return null;

}
}

}

/// @nodoc


class BridgeSkillResourceBase_Directory extends BridgeSkillResourceBase {
  const BridgeSkillResourceBase_Directory({required this.path}): super._();


 final  String path;

/// Create a copy of BridgeSkillResourceBase
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSkillResourceBase_DirectoryCopyWith<BridgeSkillResourceBase_Directory> get copyWith => _$BridgeSkillResourceBase_DirectoryCopyWithImpl<BridgeSkillResourceBase_Directory>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillResourceBase_Directory&&(identical(other.path, path) || other.path == path));
}


@override
int get hashCode => Object.hash(runtimeType,path);

@override
String toString() {
  return 'BridgeSkillResourceBase.directory(path: $path)';
}


}

/// @nodoc
abstract mixin class $BridgeSkillResourceBase_DirectoryCopyWith<$Res> implements $BridgeSkillResourceBaseCopyWith<$Res> {
  factory $BridgeSkillResourceBase_DirectoryCopyWith(BridgeSkillResourceBase_Directory value, $Res Function(BridgeSkillResourceBase_Directory) _then) = _$BridgeSkillResourceBase_DirectoryCopyWithImpl;
@useResult
$Res call({
 String path
});




}
/// @nodoc
class _$BridgeSkillResourceBase_DirectoryCopyWithImpl<$Res>
    implements $BridgeSkillResourceBase_DirectoryCopyWith<$Res> {
  _$BridgeSkillResourceBase_DirectoryCopyWithImpl(this._self, this._then);

  final BridgeSkillResourceBase_Directory _self;
  final $Res Function(BridgeSkillResourceBase_Directory) _then;

/// Create a copy of BridgeSkillResourceBase
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? path = null,}) {
  return _then(BridgeSkillResourceBase_Directory(
path: null == path ? _self.path : path // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeSkillResourceBase_Url extends BridgeSkillResourceBase {
  const BridgeSkillResourceBase_Url({required this.url}): super._();


 final  String url;

/// Create a copy of BridgeSkillResourceBase
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSkillResourceBase_UrlCopyWith<BridgeSkillResourceBase_Url> get copyWith => _$BridgeSkillResourceBase_UrlCopyWithImpl<BridgeSkillResourceBase_Url>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillResourceBase_Url&&(identical(other.url, url) || other.url == url));
}


@override
int get hashCode => Object.hash(runtimeType,url);

@override
String toString() {
  return 'BridgeSkillResourceBase.url(url: $url)';
}


}

/// @nodoc
abstract mixin class $BridgeSkillResourceBase_UrlCopyWith<$Res> implements $BridgeSkillResourceBaseCopyWith<$Res> {
  factory $BridgeSkillResourceBase_UrlCopyWith(BridgeSkillResourceBase_Url value, $Res Function(BridgeSkillResourceBase_Url) _then) = _$BridgeSkillResourceBase_UrlCopyWithImpl;
@useResult
$Res call({
 String url
});




}
/// @nodoc
class _$BridgeSkillResourceBase_UrlCopyWithImpl<$Res>
    implements $BridgeSkillResourceBase_UrlCopyWith<$Res> {
  _$BridgeSkillResourceBase_UrlCopyWithImpl(this._self, this._then);

  final BridgeSkillResourceBase_Url _self;
  final $Res Function(BridgeSkillResourceBase_Url) _then;

/// Create a copy of BridgeSkillResourceBase
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? url = null,}) {
  return _then(BridgeSkillResourceBase_Url(
url: null == url ? _self.url : url // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeSkillResourceBase_Opaque extends BridgeSkillResourceBase {
  const BridgeSkillResourceBase_Opaque({required this.description}): super._();


 final  String description;

/// Create a copy of BridgeSkillResourceBase
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSkillResourceBase_OpaqueCopyWith<BridgeSkillResourceBase_Opaque> get copyWith => _$BridgeSkillResourceBase_OpaqueCopyWithImpl<BridgeSkillResourceBase_Opaque>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillResourceBase_Opaque&&(identical(other.description, description) || other.description == description));
}


@override
int get hashCode => Object.hash(runtimeType,description);

@override
String toString() {
  return 'BridgeSkillResourceBase.opaque(description: $description)';
}


}

/// @nodoc
abstract mixin class $BridgeSkillResourceBase_OpaqueCopyWith<$Res> implements $BridgeSkillResourceBaseCopyWith<$Res> {
  factory $BridgeSkillResourceBase_OpaqueCopyWith(BridgeSkillResourceBase_Opaque value, $Res Function(BridgeSkillResourceBase_Opaque) _then) = _$BridgeSkillResourceBase_OpaqueCopyWithImpl;
@useResult
$Res call({
 String description
});




}
/// @nodoc
class _$BridgeSkillResourceBase_OpaqueCopyWithImpl<$Res>
    implements $BridgeSkillResourceBase_OpaqueCopyWith<$Res> {
  _$BridgeSkillResourceBase_OpaqueCopyWithImpl(this._self, this._then);

  final BridgeSkillResourceBase_Opaque _self;
  final $Res Function(BridgeSkillResourceBase_Opaque) _then;

/// Create a copy of BridgeSkillResourceBase
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? description = null,}) {
  return _then(BridgeSkillResourceBase_Opaque(
description: null == description ? _self.description : description // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeThreadAgentState {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadAgentState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadAgentState()';
}


}

/// @nodoc
class $BridgeThreadAgentStateCopyWith<$Res>  {
$BridgeThreadAgentStateCopyWith(BridgeThreadAgentState _, $Res Function(BridgeThreadAgentState) __);
}


/// Adds pattern-matching-related methods to [BridgeThreadAgentState].
extension BridgeThreadAgentStatePatterns on BridgeThreadAgentState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeThreadAgentState_Queued value)?  queued,TResult Function( BridgeThreadAgentState_Running value)?  running,TResult Function( BridgeThreadAgentState_Succeeded value)?  succeeded,TResult Function( BridgeThreadAgentState_Denied value)?  denied,TResult Function( BridgeThreadAgentState_Cancelled value)?  cancelled,TResult Function( BridgeThreadAgentState_Failed value)?  failed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeThreadAgentState_Queued() when queued != null:
return queued(_that);case BridgeThreadAgentState_Running() when running != null:
return running(_that);case BridgeThreadAgentState_Succeeded() when succeeded != null:
return succeeded(_that);case BridgeThreadAgentState_Denied() when denied != null:
return denied(_that);case BridgeThreadAgentState_Cancelled() when cancelled != null:
return cancelled(_that);case BridgeThreadAgentState_Failed() when failed != null:
return failed(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeThreadAgentState_Queued value)  queued,required TResult Function( BridgeThreadAgentState_Running value)  running,required TResult Function( BridgeThreadAgentState_Succeeded value)  succeeded,required TResult Function( BridgeThreadAgentState_Denied value)  denied,required TResult Function( BridgeThreadAgentState_Cancelled value)  cancelled,required TResult Function( BridgeThreadAgentState_Failed value)  failed,}){
final _that = this;
switch (_that) {
case BridgeThreadAgentState_Queued():
return queued(_that);case BridgeThreadAgentState_Running():
return running(_that);case BridgeThreadAgentState_Succeeded():
return succeeded(_that);case BridgeThreadAgentState_Denied():
return denied(_that);case BridgeThreadAgentState_Cancelled():
return cancelled(_that);case BridgeThreadAgentState_Failed():
return failed(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeThreadAgentState_Queued value)?  queued,TResult? Function( BridgeThreadAgentState_Running value)?  running,TResult? Function( BridgeThreadAgentState_Succeeded value)?  succeeded,TResult? Function( BridgeThreadAgentState_Denied value)?  denied,TResult? Function( BridgeThreadAgentState_Cancelled value)?  cancelled,TResult? Function( BridgeThreadAgentState_Failed value)?  failed,}){
final _that = this;
switch (_that) {
case BridgeThreadAgentState_Queued() when queued != null:
return queued(_that);case BridgeThreadAgentState_Running() when running != null:
return running(_that);case BridgeThreadAgentState_Succeeded() when succeeded != null:
return succeeded(_that);case BridgeThreadAgentState_Denied() when denied != null:
return denied(_that);case BridgeThreadAgentState_Cancelled() when cancelled != null:
return cancelled(_that);case BridgeThreadAgentState_Failed() when failed != null:
return failed(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  queued,TResult Function()?  running,TResult Function( PlatformInt64 completedAt,  String summary)?  succeeded,TResult Function( PlatformInt64 deniedAt,  String reason)?  denied,TResult Function( PlatformInt64 cancelledAt,  String reason)?  cancelled,TResult Function( PlatformInt64 failedAt,  String error)?  failed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeThreadAgentState_Queued() when queued != null:
return queued();case BridgeThreadAgentState_Running() when running != null:
return running();case BridgeThreadAgentState_Succeeded() when succeeded != null:
return succeeded(_that.completedAt,_that.summary);case BridgeThreadAgentState_Denied() when denied != null:
return denied(_that.deniedAt,_that.reason);case BridgeThreadAgentState_Cancelled() when cancelled != null:
return cancelled(_that.cancelledAt,_that.reason);case BridgeThreadAgentState_Failed() when failed != null:
return failed(_that.failedAt,_that.error);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  queued,required TResult Function()  running,required TResult Function( PlatformInt64 completedAt,  String summary)  succeeded,required TResult Function( PlatformInt64 deniedAt,  String reason)  denied,required TResult Function( PlatformInt64 cancelledAt,  String reason)  cancelled,required TResult Function( PlatformInt64 failedAt,  String error)  failed,}) {final _that = this;
switch (_that) {
case BridgeThreadAgentState_Queued():
return queued();case BridgeThreadAgentState_Running():
return running();case BridgeThreadAgentState_Succeeded():
return succeeded(_that.completedAt,_that.summary);case BridgeThreadAgentState_Denied():
return denied(_that.deniedAt,_that.reason);case BridgeThreadAgentState_Cancelled():
return cancelled(_that.cancelledAt,_that.reason);case BridgeThreadAgentState_Failed():
return failed(_that.failedAt,_that.error);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  queued,TResult? Function()?  running,TResult? Function( PlatformInt64 completedAt,  String summary)?  succeeded,TResult? Function( PlatformInt64 deniedAt,  String reason)?  denied,TResult? Function( PlatformInt64 cancelledAt,  String reason)?  cancelled,TResult? Function( PlatformInt64 failedAt,  String error)?  failed,}) {final _that = this;
switch (_that) {
case BridgeThreadAgentState_Queued() when queued != null:
return queued();case BridgeThreadAgentState_Running() when running != null:
return running();case BridgeThreadAgentState_Succeeded() when succeeded != null:
return succeeded(_that.completedAt,_that.summary);case BridgeThreadAgentState_Denied() when denied != null:
return denied(_that.deniedAt,_that.reason);case BridgeThreadAgentState_Cancelled() when cancelled != null:
return cancelled(_that.cancelledAt,_that.reason);case BridgeThreadAgentState_Failed() when failed != null:
return failed(_that.failedAt,_that.error);case _:
  return null;

}
}

}

/// @nodoc


class BridgeThreadAgentState_Queued extends BridgeThreadAgentState {
  const BridgeThreadAgentState_Queued(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadAgentState_Queued);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadAgentState.queued()';
}


}




/// @nodoc


class BridgeThreadAgentState_Running extends BridgeThreadAgentState {
  const BridgeThreadAgentState_Running(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadAgentState_Running);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadAgentState.running()';
}


}




/// @nodoc


class BridgeThreadAgentState_Succeeded extends BridgeThreadAgentState {
  const BridgeThreadAgentState_Succeeded({required this.completedAt, required this.summary}): super._();


 final  PlatformInt64 completedAt;
 final  String summary;

/// Create a copy of BridgeThreadAgentState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadAgentState_SucceededCopyWith<BridgeThreadAgentState_Succeeded> get copyWith => _$BridgeThreadAgentState_SucceededCopyWithImpl<BridgeThreadAgentState_Succeeded>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadAgentState_Succeeded&&(identical(other.completedAt, completedAt) || other.completedAt == completedAt)&&(identical(other.summary, summary) || other.summary == summary));
}


@override
int get hashCode => Object.hash(runtimeType,completedAt,summary);

@override
String toString() {
  return 'BridgeThreadAgentState.succeeded(completedAt: $completedAt, summary: $summary)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadAgentState_SucceededCopyWith<$Res> implements $BridgeThreadAgentStateCopyWith<$Res> {
  factory $BridgeThreadAgentState_SucceededCopyWith(BridgeThreadAgentState_Succeeded value, $Res Function(BridgeThreadAgentState_Succeeded) _then) = _$BridgeThreadAgentState_SucceededCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 completedAt, String summary
});




}
/// @nodoc
class _$BridgeThreadAgentState_SucceededCopyWithImpl<$Res>
    implements $BridgeThreadAgentState_SucceededCopyWith<$Res> {
  _$BridgeThreadAgentState_SucceededCopyWithImpl(this._self, this._then);

  final BridgeThreadAgentState_Succeeded _self;
  final $Res Function(BridgeThreadAgentState_Succeeded) _then;

/// Create a copy of BridgeThreadAgentState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? completedAt = null,Object? summary = null,}) {
  return _then(BridgeThreadAgentState_Succeeded(
completedAt: null == completedAt ? _self.completedAt : completedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadAgentState_Denied extends BridgeThreadAgentState {
  const BridgeThreadAgentState_Denied({required this.deniedAt, required this.reason}): super._();


 final  PlatformInt64 deniedAt;
 final  String reason;

/// Create a copy of BridgeThreadAgentState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadAgentState_DeniedCopyWith<BridgeThreadAgentState_Denied> get copyWith => _$BridgeThreadAgentState_DeniedCopyWithImpl<BridgeThreadAgentState_Denied>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadAgentState_Denied&&(identical(other.deniedAt, deniedAt) || other.deniedAt == deniedAt)&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,deniedAt,reason);

@override
String toString() {
  return 'BridgeThreadAgentState.denied(deniedAt: $deniedAt, reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadAgentState_DeniedCopyWith<$Res> implements $BridgeThreadAgentStateCopyWith<$Res> {
  factory $BridgeThreadAgentState_DeniedCopyWith(BridgeThreadAgentState_Denied value, $Res Function(BridgeThreadAgentState_Denied) _then) = _$BridgeThreadAgentState_DeniedCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 deniedAt, String reason
});




}
/// @nodoc
class _$BridgeThreadAgentState_DeniedCopyWithImpl<$Res>
    implements $BridgeThreadAgentState_DeniedCopyWith<$Res> {
  _$BridgeThreadAgentState_DeniedCopyWithImpl(this._self, this._then);

  final BridgeThreadAgentState_Denied _self;
  final $Res Function(BridgeThreadAgentState_Denied) _then;

/// Create a copy of BridgeThreadAgentState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? deniedAt = null,Object? reason = null,}) {
  return _then(BridgeThreadAgentState_Denied(
deniedAt: null == deniedAt ? _self.deniedAt : deniedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadAgentState_Cancelled extends BridgeThreadAgentState {
  const BridgeThreadAgentState_Cancelled({required this.cancelledAt, required this.reason}): super._();


 final  PlatformInt64 cancelledAt;
 final  String reason;

/// Create a copy of BridgeThreadAgentState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadAgentState_CancelledCopyWith<BridgeThreadAgentState_Cancelled> get copyWith => _$BridgeThreadAgentState_CancelledCopyWithImpl<BridgeThreadAgentState_Cancelled>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadAgentState_Cancelled&&(identical(other.cancelledAt, cancelledAt) || other.cancelledAt == cancelledAt)&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,cancelledAt,reason);

@override
String toString() {
  return 'BridgeThreadAgentState.cancelled(cancelledAt: $cancelledAt, reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadAgentState_CancelledCopyWith<$Res> implements $BridgeThreadAgentStateCopyWith<$Res> {
  factory $BridgeThreadAgentState_CancelledCopyWith(BridgeThreadAgentState_Cancelled value, $Res Function(BridgeThreadAgentState_Cancelled) _then) = _$BridgeThreadAgentState_CancelledCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 cancelledAt, String reason
});




}
/// @nodoc
class _$BridgeThreadAgentState_CancelledCopyWithImpl<$Res>
    implements $BridgeThreadAgentState_CancelledCopyWith<$Res> {
  _$BridgeThreadAgentState_CancelledCopyWithImpl(this._self, this._then);

  final BridgeThreadAgentState_Cancelled _self;
  final $Res Function(BridgeThreadAgentState_Cancelled) _then;

/// Create a copy of BridgeThreadAgentState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? cancelledAt = null,Object? reason = null,}) {
  return _then(BridgeThreadAgentState_Cancelled(
cancelledAt: null == cancelledAt ? _self.cancelledAt : cancelledAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadAgentState_Failed extends BridgeThreadAgentState {
  const BridgeThreadAgentState_Failed({required this.failedAt, required this.error}): super._();


 final  PlatformInt64 failedAt;
 final  String error;

/// Create a copy of BridgeThreadAgentState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadAgentState_FailedCopyWith<BridgeThreadAgentState_Failed> get copyWith => _$BridgeThreadAgentState_FailedCopyWithImpl<BridgeThreadAgentState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadAgentState_Failed&&(identical(other.failedAt, failedAt) || other.failedAt == failedAt)&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,failedAt,error);

@override
String toString() {
  return 'BridgeThreadAgentState.failed(failedAt: $failedAt, error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadAgentState_FailedCopyWith<$Res> implements $BridgeThreadAgentStateCopyWith<$Res> {
  factory $BridgeThreadAgentState_FailedCopyWith(BridgeThreadAgentState_Failed value, $Res Function(BridgeThreadAgentState_Failed) _then) = _$BridgeThreadAgentState_FailedCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 failedAt, String error
});




}
/// @nodoc
class _$BridgeThreadAgentState_FailedCopyWithImpl<$Res>
    implements $BridgeThreadAgentState_FailedCopyWith<$Res> {
  _$BridgeThreadAgentState_FailedCopyWithImpl(this._self, this._then);

  final BridgeThreadAgentState_Failed _self;
  final $Res Function(BridgeThreadAgentState_Failed) _then;

/// Create a copy of BridgeThreadAgentState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? failedAt = null,Object? error = null,}) {
  return _then(BridgeThreadAgentState_Failed(
failedAt: null == failedAt ? _self.failedAt : failedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeThreadContentLifecycle {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadContentLifecycle);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadContentLifecycle()';
}


}

/// @nodoc
class $BridgeThreadContentLifecycleCopyWith<$Res>  {
$BridgeThreadContentLifecycleCopyWith(BridgeThreadContentLifecycle _, $Res Function(BridgeThreadContentLifecycle) __);
}


/// Adds pattern-matching-related methods to [BridgeThreadContentLifecycle].
extension BridgeThreadContentLifecyclePatterns on BridgeThreadContentLifecycle {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeThreadContentLifecycle_Streaming value)?  streaming,TResult Function( BridgeThreadContentLifecycle_Completed value)?  completed,TResult Function( BridgeThreadContentLifecycle_Failed value)?  failed,TResult Function( BridgeThreadContentLifecycle_Cancelled value)?  cancelled,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeThreadContentLifecycle_Streaming() when streaming != null:
return streaming(_that);case BridgeThreadContentLifecycle_Completed() when completed != null:
return completed(_that);case BridgeThreadContentLifecycle_Failed() when failed != null:
return failed(_that);case BridgeThreadContentLifecycle_Cancelled() when cancelled != null:
return cancelled(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeThreadContentLifecycle_Streaming value)  streaming,required TResult Function( BridgeThreadContentLifecycle_Completed value)  completed,required TResult Function( BridgeThreadContentLifecycle_Failed value)  failed,required TResult Function( BridgeThreadContentLifecycle_Cancelled value)  cancelled,}){
final _that = this;
switch (_that) {
case BridgeThreadContentLifecycle_Streaming():
return streaming(_that);case BridgeThreadContentLifecycle_Completed():
return completed(_that);case BridgeThreadContentLifecycle_Failed():
return failed(_that);case BridgeThreadContentLifecycle_Cancelled():
return cancelled(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeThreadContentLifecycle_Streaming value)?  streaming,TResult? Function( BridgeThreadContentLifecycle_Completed value)?  completed,TResult? Function( BridgeThreadContentLifecycle_Failed value)?  failed,TResult? Function( BridgeThreadContentLifecycle_Cancelled value)?  cancelled,}){
final _that = this;
switch (_that) {
case BridgeThreadContentLifecycle_Streaming() when streaming != null:
return streaming(_that);case BridgeThreadContentLifecycle_Completed() when completed != null:
return completed(_that);case BridgeThreadContentLifecycle_Failed() when failed != null:
return failed(_that);case BridgeThreadContentLifecycle_Cancelled() when cancelled != null:
return cancelled(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  streaming,TResult Function( PlatformInt64 completedAt)?  completed,TResult Function( PlatformInt64 failedAt,  String error)?  failed,TResult Function( PlatformInt64 cancelledAt,  String reason)?  cancelled,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeThreadContentLifecycle_Streaming() when streaming != null:
return streaming();case BridgeThreadContentLifecycle_Completed() when completed != null:
return completed(_that.completedAt);case BridgeThreadContentLifecycle_Failed() when failed != null:
return failed(_that.failedAt,_that.error);case BridgeThreadContentLifecycle_Cancelled() when cancelled != null:
return cancelled(_that.cancelledAt,_that.reason);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  streaming,required TResult Function( PlatformInt64 completedAt)  completed,required TResult Function( PlatformInt64 failedAt,  String error)  failed,required TResult Function( PlatformInt64 cancelledAt,  String reason)  cancelled,}) {final _that = this;
switch (_that) {
case BridgeThreadContentLifecycle_Streaming():
return streaming();case BridgeThreadContentLifecycle_Completed():
return completed(_that.completedAt);case BridgeThreadContentLifecycle_Failed():
return failed(_that.failedAt,_that.error);case BridgeThreadContentLifecycle_Cancelled():
return cancelled(_that.cancelledAt,_that.reason);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  streaming,TResult? Function( PlatformInt64 completedAt)?  completed,TResult? Function( PlatformInt64 failedAt,  String error)?  failed,TResult? Function( PlatformInt64 cancelledAt,  String reason)?  cancelled,}) {final _that = this;
switch (_that) {
case BridgeThreadContentLifecycle_Streaming() when streaming != null:
return streaming();case BridgeThreadContentLifecycle_Completed() when completed != null:
return completed(_that.completedAt);case BridgeThreadContentLifecycle_Failed() when failed != null:
return failed(_that.failedAt,_that.error);case BridgeThreadContentLifecycle_Cancelled() when cancelled != null:
return cancelled(_that.cancelledAt,_that.reason);case _:
  return null;

}
}

}

/// @nodoc


class BridgeThreadContentLifecycle_Streaming extends BridgeThreadContentLifecycle {
  const BridgeThreadContentLifecycle_Streaming(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadContentLifecycle_Streaming);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadContentLifecycle.streaming()';
}


}




/// @nodoc


class BridgeThreadContentLifecycle_Completed extends BridgeThreadContentLifecycle {
  const BridgeThreadContentLifecycle_Completed({required this.completedAt}): super._();


 final  PlatformInt64 completedAt;

/// Create a copy of BridgeThreadContentLifecycle
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadContentLifecycle_CompletedCopyWith<BridgeThreadContentLifecycle_Completed> get copyWith => _$BridgeThreadContentLifecycle_CompletedCopyWithImpl<BridgeThreadContentLifecycle_Completed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadContentLifecycle_Completed&&(identical(other.completedAt, completedAt) || other.completedAt == completedAt));
}


@override
int get hashCode => Object.hash(runtimeType,completedAt);

@override
String toString() {
  return 'BridgeThreadContentLifecycle.completed(completedAt: $completedAt)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadContentLifecycle_CompletedCopyWith<$Res> implements $BridgeThreadContentLifecycleCopyWith<$Res> {
  factory $BridgeThreadContentLifecycle_CompletedCopyWith(BridgeThreadContentLifecycle_Completed value, $Res Function(BridgeThreadContentLifecycle_Completed) _then) = _$BridgeThreadContentLifecycle_CompletedCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 completedAt
});




}
/// @nodoc
class _$BridgeThreadContentLifecycle_CompletedCopyWithImpl<$Res>
    implements $BridgeThreadContentLifecycle_CompletedCopyWith<$Res> {
  _$BridgeThreadContentLifecycle_CompletedCopyWithImpl(this._self, this._then);

  final BridgeThreadContentLifecycle_Completed _self;
  final $Res Function(BridgeThreadContentLifecycle_Completed) _then;

/// Create a copy of BridgeThreadContentLifecycle
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? completedAt = null,}) {
  return _then(BridgeThreadContentLifecycle_Completed(
completedAt: null == completedAt ? _self.completedAt : completedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,
  ));
}


}

/// @nodoc


class BridgeThreadContentLifecycle_Failed extends BridgeThreadContentLifecycle {
  const BridgeThreadContentLifecycle_Failed({required this.failedAt, required this.error}): super._();


 final  PlatformInt64 failedAt;
 final  String error;

/// Create a copy of BridgeThreadContentLifecycle
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadContentLifecycle_FailedCopyWith<BridgeThreadContentLifecycle_Failed> get copyWith => _$BridgeThreadContentLifecycle_FailedCopyWithImpl<BridgeThreadContentLifecycle_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadContentLifecycle_Failed&&(identical(other.failedAt, failedAt) || other.failedAt == failedAt)&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,failedAt,error);

@override
String toString() {
  return 'BridgeThreadContentLifecycle.failed(failedAt: $failedAt, error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadContentLifecycle_FailedCopyWith<$Res> implements $BridgeThreadContentLifecycleCopyWith<$Res> {
  factory $BridgeThreadContentLifecycle_FailedCopyWith(BridgeThreadContentLifecycle_Failed value, $Res Function(BridgeThreadContentLifecycle_Failed) _then) = _$BridgeThreadContentLifecycle_FailedCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 failedAt, String error
});




}
/// @nodoc
class _$BridgeThreadContentLifecycle_FailedCopyWithImpl<$Res>
    implements $BridgeThreadContentLifecycle_FailedCopyWith<$Res> {
  _$BridgeThreadContentLifecycle_FailedCopyWithImpl(this._self, this._then);

  final BridgeThreadContentLifecycle_Failed _self;
  final $Res Function(BridgeThreadContentLifecycle_Failed) _then;

/// Create a copy of BridgeThreadContentLifecycle
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? failedAt = null,Object? error = null,}) {
  return _then(BridgeThreadContentLifecycle_Failed(
failedAt: null == failedAt ? _self.failedAt : failedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadContentLifecycle_Cancelled extends BridgeThreadContentLifecycle {
  const BridgeThreadContentLifecycle_Cancelled({required this.cancelledAt, required this.reason}): super._();


 final  PlatformInt64 cancelledAt;
 final  String reason;

/// Create a copy of BridgeThreadContentLifecycle
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadContentLifecycle_CancelledCopyWith<BridgeThreadContentLifecycle_Cancelled> get copyWith => _$BridgeThreadContentLifecycle_CancelledCopyWithImpl<BridgeThreadContentLifecycle_Cancelled>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadContentLifecycle_Cancelled&&(identical(other.cancelledAt, cancelledAt) || other.cancelledAt == cancelledAt)&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,cancelledAt,reason);

@override
String toString() {
  return 'BridgeThreadContentLifecycle.cancelled(cancelledAt: $cancelledAt, reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadContentLifecycle_CancelledCopyWith<$Res> implements $BridgeThreadContentLifecycleCopyWith<$Res> {
  factory $BridgeThreadContentLifecycle_CancelledCopyWith(BridgeThreadContentLifecycle_Cancelled value, $Res Function(BridgeThreadContentLifecycle_Cancelled) _then) = _$BridgeThreadContentLifecycle_CancelledCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 cancelledAt, String reason
});




}
/// @nodoc
class _$BridgeThreadContentLifecycle_CancelledCopyWithImpl<$Res>
    implements $BridgeThreadContentLifecycle_CancelledCopyWith<$Res> {
  _$BridgeThreadContentLifecycle_CancelledCopyWithImpl(this._self, this._then);

  final BridgeThreadContentLifecycle_Cancelled _self;
  final $Res Function(BridgeThreadContentLifecycle_Cancelled) _then;

/// Create a copy of BridgeThreadContentLifecycle
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? cancelledAt = null,Object? reason = null,}) {
  return _then(BridgeThreadContentLifecycle_Cancelled(
cancelledAt: null == cancelledAt ? _self.cancelledAt : cancelledAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeThreadInferenceState {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadInferenceState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadInferenceState()';
}


}

/// @nodoc
class $BridgeThreadInferenceStateCopyWith<$Res>  {
$BridgeThreadInferenceStateCopyWith(BridgeThreadInferenceState _, $Res Function(BridgeThreadInferenceState) __);
}


/// Adds pattern-matching-related methods to [BridgeThreadInferenceState].
extension BridgeThreadInferenceStatePatterns on BridgeThreadInferenceState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeThreadInferenceState_Running value)?  running,TResult Function( BridgeThreadInferenceState_Completed value)?  completed,TResult Function( BridgeThreadInferenceState_Failed value)?  failed,TResult Function( BridgeThreadInferenceState_Cancelled value)?  cancelled,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeThreadInferenceState_Running() when running != null:
return running(_that);case BridgeThreadInferenceState_Completed() when completed != null:
return completed(_that);case BridgeThreadInferenceState_Failed() when failed != null:
return failed(_that);case BridgeThreadInferenceState_Cancelled() when cancelled != null:
return cancelled(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeThreadInferenceState_Running value)  running,required TResult Function( BridgeThreadInferenceState_Completed value)  completed,required TResult Function( BridgeThreadInferenceState_Failed value)  failed,required TResult Function( BridgeThreadInferenceState_Cancelled value)  cancelled,}){
final _that = this;
switch (_that) {
case BridgeThreadInferenceState_Running():
return running(_that);case BridgeThreadInferenceState_Completed():
return completed(_that);case BridgeThreadInferenceState_Failed():
return failed(_that);case BridgeThreadInferenceState_Cancelled():
return cancelled(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeThreadInferenceState_Running value)?  running,TResult? Function( BridgeThreadInferenceState_Completed value)?  completed,TResult? Function( BridgeThreadInferenceState_Failed value)?  failed,TResult? Function( BridgeThreadInferenceState_Cancelled value)?  cancelled,}){
final _that = this;
switch (_that) {
case BridgeThreadInferenceState_Running() when running != null:
return running(_that);case BridgeThreadInferenceState_Completed() when completed != null:
return completed(_that);case BridgeThreadInferenceState_Failed() when failed != null:
return failed(_that);case BridgeThreadInferenceState_Cancelled() when cancelled != null:
return cancelled(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  running,TResult Function( PlatformInt64 completedAt,  BridgeTokenUsageSnapshot usage)?  completed,TResult Function( PlatformInt64 failedAt,  String error)?  failed,TResult Function( PlatformInt64 cancelledAt,  String reason)?  cancelled,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeThreadInferenceState_Running() when running != null:
return running();case BridgeThreadInferenceState_Completed() when completed != null:
return completed(_that.completedAt,_that.usage);case BridgeThreadInferenceState_Failed() when failed != null:
return failed(_that.failedAt,_that.error);case BridgeThreadInferenceState_Cancelled() when cancelled != null:
return cancelled(_that.cancelledAt,_that.reason);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  running,required TResult Function( PlatformInt64 completedAt,  BridgeTokenUsageSnapshot usage)  completed,required TResult Function( PlatformInt64 failedAt,  String error)  failed,required TResult Function( PlatformInt64 cancelledAt,  String reason)  cancelled,}) {final _that = this;
switch (_that) {
case BridgeThreadInferenceState_Running():
return running();case BridgeThreadInferenceState_Completed():
return completed(_that.completedAt,_that.usage);case BridgeThreadInferenceState_Failed():
return failed(_that.failedAt,_that.error);case BridgeThreadInferenceState_Cancelled():
return cancelled(_that.cancelledAt,_that.reason);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  running,TResult? Function( PlatformInt64 completedAt,  BridgeTokenUsageSnapshot usage)?  completed,TResult? Function( PlatformInt64 failedAt,  String error)?  failed,TResult? Function( PlatformInt64 cancelledAt,  String reason)?  cancelled,}) {final _that = this;
switch (_that) {
case BridgeThreadInferenceState_Running() when running != null:
return running();case BridgeThreadInferenceState_Completed() when completed != null:
return completed(_that.completedAt,_that.usage);case BridgeThreadInferenceState_Failed() when failed != null:
return failed(_that.failedAt,_that.error);case BridgeThreadInferenceState_Cancelled() when cancelled != null:
return cancelled(_that.cancelledAt,_that.reason);case _:
  return null;

}
}

}

/// @nodoc


class BridgeThreadInferenceState_Running extends BridgeThreadInferenceState {
  const BridgeThreadInferenceState_Running(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadInferenceState_Running);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadInferenceState.running()';
}


}




/// @nodoc


class BridgeThreadInferenceState_Completed extends BridgeThreadInferenceState {
  const BridgeThreadInferenceState_Completed({required this.completedAt, required this.usage}): super._();


 final  PlatformInt64 completedAt;
 final  BridgeTokenUsageSnapshot usage;

/// Create a copy of BridgeThreadInferenceState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadInferenceState_CompletedCopyWith<BridgeThreadInferenceState_Completed> get copyWith => _$BridgeThreadInferenceState_CompletedCopyWithImpl<BridgeThreadInferenceState_Completed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadInferenceState_Completed&&(identical(other.completedAt, completedAt) || other.completedAt == completedAt)&&(identical(other.usage, usage) || other.usage == usage));
}


@override
int get hashCode => Object.hash(runtimeType,completedAt,usage);

@override
String toString() {
  return 'BridgeThreadInferenceState.completed(completedAt: $completedAt, usage: $usage)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadInferenceState_CompletedCopyWith<$Res> implements $BridgeThreadInferenceStateCopyWith<$Res> {
  factory $BridgeThreadInferenceState_CompletedCopyWith(BridgeThreadInferenceState_Completed value, $Res Function(BridgeThreadInferenceState_Completed) _then) = _$BridgeThreadInferenceState_CompletedCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 completedAt, BridgeTokenUsageSnapshot usage
});




}
/// @nodoc
class _$BridgeThreadInferenceState_CompletedCopyWithImpl<$Res>
    implements $BridgeThreadInferenceState_CompletedCopyWith<$Res> {
  _$BridgeThreadInferenceState_CompletedCopyWithImpl(this._self, this._then);

  final BridgeThreadInferenceState_Completed _self;
  final $Res Function(BridgeThreadInferenceState_Completed) _then;

/// Create a copy of BridgeThreadInferenceState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? completedAt = null,Object? usage = null,}) {
  return _then(BridgeThreadInferenceState_Completed(
completedAt: null == completedAt ? _self.completedAt : completedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,usage: null == usage ? _self.usage : usage // ignore: cast_nullable_to_non_nullable
as BridgeTokenUsageSnapshot,
  ));
}


}

/// @nodoc


class BridgeThreadInferenceState_Failed extends BridgeThreadInferenceState {
  const BridgeThreadInferenceState_Failed({required this.failedAt, required this.error}): super._();


 final  PlatformInt64 failedAt;
 final  String error;

/// Create a copy of BridgeThreadInferenceState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadInferenceState_FailedCopyWith<BridgeThreadInferenceState_Failed> get copyWith => _$BridgeThreadInferenceState_FailedCopyWithImpl<BridgeThreadInferenceState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadInferenceState_Failed&&(identical(other.failedAt, failedAt) || other.failedAt == failedAt)&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,failedAt,error);

@override
String toString() {
  return 'BridgeThreadInferenceState.failed(failedAt: $failedAt, error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadInferenceState_FailedCopyWith<$Res> implements $BridgeThreadInferenceStateCopyWith<$Res> {
  factory $BridgeThreadInferenceState_FailedCopyWith(BridgeThreadInferenceState_Failed value, $Res Function(BridgeThreadInferenceState_Failed) _then) = _$BridgeThreadInferenceState_FailedCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 failedAt, String error
});




}
/// @nodoc
class _$BridgeThreadInferenceState_FailedCopyWithImpl<$Res>
    implements $BridgeThreadInferenceState_FailedCopyWith<$Res> {
  _$BridgeThreadInferenceState_FailedCopyWithImpl(this._self, this._then);

  final BridgeThreadInferenceState_Failed _self;
  final $Res Function(BridgeThreadInferenceState_Failed) _then;

/// Create a copy of BridgeThreadInferenceState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? failedAt = null,Object? error = null,}) {
  return _then(BridgeThreadInferenceState_Failed(
failedAt: null == failedAt ? _self.failedAt : failedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadInferenceState_Cancelled extends BridgeThreadInferenceState {
  const BridgeThreadInferenceState_Cancelled({required this.cancelledAt, required this.reason}): super._();


 final  PlatformInt64 cancelledAt;
 final  String reason;

/// Create a copy of BridgeThreadInferenceState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadInferenceState_CancelledCopyWith<BridgeThreadInferenceState_Cancelled> get copyWith => _$BridgeThreadInferenceState_CancelledCopyWithImpl<BridgeThreadInferenceState_Cancelled>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadInferenceState_Cancelled&&(identical(other.cancelledAt, cancelledAt) || other.cancelledAt == cancelledAt)&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,cancelledAt,reason);

@override
String toString() {
  return 'BridgeThreadInferenceState.cancelled(cancelledAt: $cancelledAt, reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadInferenceState_CancelledCopyWith<$Res> implements $BridgeThreadInferenceStateCopyWith<$Res> {
  factory $BridgeThreadInferenceState_CancelledCopyWith(BridgeThreadInferenceState_Cancelled value, $Res Function(BridgeThreadInferenceState_Cancelled) _then) = _$BridgeThreadInferenceState_CancelledCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 cancelledAt, String reason
});




}
/// @nodoc
class _$BridgeThreadInferenceState_CancelledCopyWithImpl<$Res>
    implements $BridgeThreadInferenceState_CancelledCopyWith<$Res> {
  _$BridgeThreadInferenceState_CancelledCopyWithImpl(this._self, this._then);

  final BridgeThreadInferenceState_Cancelled _self;
  final $Res Function(BridgeThreadInferenceState_Cancelled) _then;

/// Create a copy of BridgeThreadInferenceState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? cancelledAt = null,Object? reason = null,}) {
  return _then(BridgeThreadInferenceState_Cancelled(
cancelledAt: null == cancelledAt ? _self.cancelledAt : cancelledAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeThreadItemDeltaState {

 String get delta;
/// Create a copy of BridgeThreadItemDeltaState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemDeltaStateCopyWith<BridgeThreadItemDeltaState> get copyWith => _$BridgeThreadItemDeltaStateCopyWithImpl<BridgeThreadItemDeltaState>(this as BridgeThreadItemDeltaState, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemDeltaState&&(identical(other.delta, delta) || other.delta == delta));
}


@override
int get hashCode => Object.hash(runtimeType,delta);

@override
String toString() {
  return 'BridgeThreadItemDeltaState(delta: $delta)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemDeltaStateCopyWith<$Res>  {
  factory $BridgeThreadItemDeltaStateCopyWith(BridgeThreadItemDeltaState value, $Res Function(BridgeThreadItemDeltaState) _then) = _$BridgeThreadItemDeltaStateCopyWithImpl;
@useResult
$Res call({
 String delta
});




}
/// @nodoc
class _$BridgeThreadItemDeltaStateCopyWithImpl<$Res>
    implements $BridgeThreadItemDeltaStateCopyWith<$Res> {
  _$BridgeThreadItemDeltaStateCopyWithImpl(this._self, this._then);

  final BridgeThreadItemDeltaState _self;
  final $Res Function(BridgeThreadItemDeltaState) _then;

/// Create a copy of BridgeThreadItemDeltaState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? delta = null,}) {
  return _then(_self.copyWith(
delta: null == delta ? _self.delta : delta // ignore: cast_nullable_to_non_nullable
as String,
  ));
}

}


/// Adds pattern-matching-related methods to [BridgeThreadItemDeltaState].
extension BridgeThreadItemDeltaStatePatterns on BridgeThreadItemDeltaState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeThreadItemDeltaState_Text value)?  text,TResult Function( BridgeThreadItemDeltaState_ThinkingSummary value)?  thinkingSummary,TResult Function( BridgeThreadItemDeltaState_ThinkingContent value)?  thinkingContent,TResult Function( BridgeThreadItemDeltaState_Plan value)?  plan,TResult Function( BridgeThreadItemDeltaState_ToolArguments value)?  toolArguments,TResult Function( BridgeThreadItemDeltaState_ToolResult value)?  toolResult,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeThreadItemDeltaState_Text() when text != null:
return text(_that);case BridgeThreadItemDeltaState_ThinkingSummary() when thinkingSummary != null:
return thinkingSummary(_that);case BridgeThreadItemDeltaState_ThinkingContent() when thinkingContent != null:
return thinkingContent(_that);case BridgeThreadItemDeltaState_Plan() when plan != null:
return plan(_that);case BridgeThreadItemDeltaState_ToolArguments() when toolArguments != null:
return toolArguments(_that);case BridgeThreadItemDeltaState_ToolResult() when toolResult != null:
return toolResult(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeThreadItemDeltaState_Text value)  text,required TResult Function( BridgeThreadItemDeltaState_ThinkingSummary value)  thinkingSummary,required TResult Function( BridgeThreadItemDeltaState_ThinkingContent value)  thinkingContent,required TResult Function( BridgeThreadItemDeltaState_Plan value)  plan,required TResult Function( BridgeThreadItemDeltaState_ToolArguments value)  toolArguments,required TResult Function( BridgeThreadItemDeltaState_ToolResult value)  toolResult,}){
final _that = this;
switch (_that) {
case BridgeThreadItemDeltaState_Text():
return text(_that);case BridgeThreadItemDeltaState_ThinkingSummary():
return thinkingSummary(_that);case BridgeThreadItemDeltaState_ThinkingContent():
return thinkingContent(_that);case BridgeThreadItemDeltaState_Plan():
return plan(_that);case BridgeThreadItemDeltaState_ToolArguments():
return toolArguments(_that);case BridgeThreadItemDeltaState_ToolResult():
return toolResult(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeThreadItemDeltaState_Text value)?  text,TResult? Function( BridgeThreadItemDeltaState_ThinkingSummary value)?  thinkingSummary,TResult? Function( BridgeThreadItemDeltaState_ThinkingContent value)?  thinkingContent,TResult? Function( BridgeThreadItemDeltaState_Plan value)?  plan,TResult? Function( BridgeThreadItemDeltaState_ToolArguments value)?  toolArguments,TResult? Function( BridgeThreadItemDeltaState_ToolResult value)?  toolResult,}){
final _that = this;
switch (_that) {
case BridgeThreadItemDeltaState_Text() when text != null:
return text(_that);case BridgeThreadItemDeltaState_ThinkingSummary() when thinkingSummary != null:
return thinkingSummary(_that);case BridgeThreadItemDeltaState_ThinkingContent() when thinkingContent != null:
return thinkingContent(_that);case BridgeThreadItemDeltaState_Plan() when plan != null:
return plan(_that);case BridgeThreadItemDeltaState_ToolArguments() when toolArguments != null:
return toolArguments(_that);case BridgeThreadItemDeltaState_ToolResult() when toolResult != null:
return toolResult(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String delta)?  text,TResult Function( int chunkIndex,  String delta)?  thinkingSummary,TResult Function( int chunkIndex,  String delta)?  thinkingContent,TResult Function( String delta)?  plan,TResult Function( String delta)?  toolArguments,TResult Function( String delta)?  toolResult,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeThreadItemDeltaState_Text() when text != null:
return text(_that.delta);case BridgeThreadItemDeltaState_ThinkingSummary() when thinkingSummary != null:
return thinkingSummary(_that.chunkIndex,_that.delta);case BridgeThreadItemDeltaState_ThinkingContent() when thinkingContent != null:
return thinkingContent(_that.chunkIndex,_that.delta);case BridgeThreadItemDeltaState_Plan() when plan != null:
return plan(_that.delta);case BridgeThreadItemDeltaState_ToolArguments() when toolArguments != null:
return toolArguments(_that.delta);case BridgeThreadItemDeltaState_ToolResult() when toolResult != null:
return toolResult(_that.delta);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String delta)  text,required TResult Function( int chunkIndex,  String delta)  thinkingSummary,required TResult Function( int chunkIndex,  String delta)  thinkingContent,required TResult Function( String delta)  plan,required TResult Function( String delta)  toolArguments,required TResult Function( String delta)  toolResult,}) {final _that = this;
switch (_that) {
case BridgeThreadItemDeltaState_Text():
return text(_that.delta);case BridgeThreadItemDeltaState_ThinkingSummary():
return thinkingSummary(_that.chunkIndex,_that.delta);case BridgeThreadItemDeltaState_ThinkingContent():
return thinkingContent(_that.chunkIndex,_that.delta);case BridgeThreadItemDeltaState_Plan():
return plan(_that.delta);case BridgeThreadItemDeltaState_ToolArguments():
return toolArguments(_that.delta);case BridgeThreadItemDeltaState_ToolResult():
return toolResult(_that.delta);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String delta)?  text,TResult? Function( int chunkIndex,  String delta)?  thinkingSummary,TResult? Function( int chunkIndex,  String delta)?  thinkingContent,TResult? Function( String delta)?  plan,TResult? Function( String delta)?  toolArguments,TResult? Function( String delta)?  toolResult,}) {final _that = this;
switch (_that) {
case BridgeThreadItemDeltaState_Text() when text != null:
return text(_that.delta);case BridgeThreadItemDeltaState_ThinkingSummary() when thinkingSummary != null:
return thinkingSummary(_that.chunkIndex,_that.delta);case BridgeThreadItemDeltaState_ThinkingContent() when thinkingContent != null:
return thinkingContent(_that.chunkIndex,_that.delta);case BridgeThreadItemDeltaState_Plan() when plan != null:
return plan(_that.delta);case BridgeThreadItemDeltaState_ToolArguments() when toolArguments != null:
return toolArguments(_that.delta);case BridgeThreadItemDeltaState_ToolResult() when toolResult != null:
return toolResult(_that.delta);case _:
  return null;

}
}

}

/// @nodoc


class BridgeThreadItemDeltaState_Text extends BridgeThreadItemDeltaState {
  const BridgeThreadItemDeltaState_Text({required this.delta}): super._();


@override final  String delta;

/// Create a copy of BridgeThreadItemDeltaState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemDeltaState_TextCopyWith<BridgeThreadItemDeltaState_Text> get copyWith => _$BridgeThreadItemDeltaState_TextCopyWithImpl<BridgeThreadItemDeltaState_Text>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemDeltaState_Text&&(identical(other.delta, delta) || other.delta == delta));
}


@override
int get hashCode => Object.hash(runtimeType,delta);

@override
String toString() {
  return 'BridgeThreadItemDeltaState.text(delta: $delta)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemDeltaState_TextCopyWith<$Res> implements $BridgeThreadItemDeltaStateCopyWith<$Res> {
  factory $BridgeThreadItemDeltaState_TextCopyWith(BridgeThreadItemDeltaState_Text value, $Res Function(BridgeThreadItemDeltaState_Text) _then) = _$BridgeThreadItemDeltaState_TextCopyWithImpl;
@override @useResult
$Res call({
 String delta
});




}
/// @nodoc
class _$BridgeThreadItemDeltaState_TextCopyWithImpl<$Res>
    implements $BridgeThreadItemDeltaState_TextCopyWith<$Res> {
  _$BridgeThreadItemDeltaState_TextCopyWithImpl(this._self, this._then);

  final BridgeThreadItemDeltaState_Text _self;
  final $Res Function(BridgeThreadItemDeltaState_Text) _then;

/// Create a copy of BridgeThreadItemDeltaState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? delta = null,}) {
  return _then(BridgeThreadItemDeltaState_Text(
delta: null == delta ? _self.delta : delta // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadItemDeltaState_ThinkingSummary extends BridgeThreadItemDeltaState {
  const BridgeThreadItemDeltaState_ThinkingSummary({required this.chunkIndex, required this.delta}): super._();


 final  int chunkIndex;
@override final  String delta;

/// Create a copy of BridgeThreadItemDeltaState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemDeltaState_ThinkingSummaryCopyWith<BridgeThreadItemDeltaState_ThinkingSummary> get copyWith => _$BridgeThreadItemDeltaState_ThinkingSummaryCopyWithImpl<BridgeThreadItemDeltaState_ThinkingSummary>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemDeltaState_ThinkingSummary&&(identical(other.chunkIndex, chunkIndex) || other.chunkIndex == chunkIndex)&&(identical(other.delta, delta) || other.delta == delta));
}


@override
int get hashCode => Object.hash(runtimeType,chunkIndex,delta);

@override
String toString() {
  return 'BridgeThreadItemDeltaState.thinkingSummary(chunkIndex: $chunkIndex, delta: $delta)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemDeltaState_ThinkingSummaryCopyWith<$Res> implements $BridgeThreadItemDeltaStateCopyWith<$Res> {
  factory $BridgeThreadItemDeltaState_ThinkingSummaryCopyWith(BridgeThreadItemDeltaState_ThinkingSummary value, $Res Function(BridgeThreadItemDeltaState_ThinkingSummary) _then) = _$BridgeThreadItemDeltaState_ThinkingSummaryCopyWithImpl;
@override @useResult
$Res call({
 int chunkIndex, String delta
});




}
/// @nodoc
class _$BridgeThreadItemDeltaState_ThinkingSummaryCopyWithImpl<$Res>
    implements $BridgeThreadItemDeltaState_ThinkingSummaryCopyWith<$Res> {
  _$BridgeThreadItemDeltaState_ThinkingSummaryCopyWithImpl(this._self, this._then);

  final BridgeThreadItemDeltaState_ThinkingSummary _self;
  final $Res Function(BridgeThreadItemDeltaState_ThinkingSummary) _then;

/// Create a copy of BridgeThreadItemDeltaState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? chunkIndex = null,Object? delta = null,}) {
  return _then(BridgeThreadItemDeltaState_ThinkingSummary(
chunkIndex: null == chunkIndex ? _self.chunkIndex : chunkIndex // ignore: cast_nullable_to_non_nullable
as int,delta: null == delta ? _self.delta : delta // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadItemDeltaState_ThinkingContent extends BridgeThreadItemDeltaState {
  const BridgeThreadItemDeltaState_ThinkingContent({required this.chunkIndex, required this.delta}): super._();


 final  int chunkIndex;
@override final  String delta;

/// Create a copy of BridgeThreadItemDeltaState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemDeltaState_ThinkingContentCopyWith<BridgeThreadItemDeltaState_ThinkingContent> get copyWith => _$BridgeThreadItemDeltaState_ThinkingContentCopyWithImpl<BridgeThreadItemDeltaState_ThinkingContent>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemDeltaState_ThinkingContent&&(identical(other.chunkIndex, chunkIndex) || other.chunkIndex == chunkIndex)&&(identical(other.delta, delta) || other.delta == delta));
}


@override
int get hashCode => Object.hash(runtimeType,chunkIndex,delta);

@override
String toString() {
  return 'BridgeThreadItemDeltaState.thinkingContent(chunkIndex: $chunkIndex, delta: $delta)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemDeltaState_ThinkingContentCopyWith<$Res> implements $BridgeThreadItemDeltaStateCopyWith<$Res> {
  factory $BridgeThreadItemDeltaState_ThinkingContentCopyWith(BridgeThreadItemDeltaState_ThinkingContent value, $Res Function(BridgeThreadItemDeltaState_ThinkingContent) _then) = _$BridgeThreadItemDeltaState_ThinkingContentCopyWithImpl;
@override @useResult
$Res call({
 int chunkIndex, String delta
});




}
/// @nodoc
class _$BridgeThreadItemDeltaState_ThinkingContentCopyWithImpl<$Res>
    implements $BridgeThreadItemDeltaState_ThinkingContentCopyWith<$Res> {
  _$BridgeThreadItemDeltaState_ThinkingContentCopyWithImpl(this._self, this._then);

  final BridgeThreadItemDeltaState_ThinkingContent _self;
  final $Res Function(BridgeThreadItemDeltaState_ThinkingContent) _then;

/// Create a copy of BridgeThreadItemDeltaState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? chunkIndex = null,Object? delta = null,}) {
  return _then(BridgeThreadItemDeltaState_ThinkingContent(
chunkIndex: null == chunkIndex ? _self.chunkIndex : chunkIndex // ignore: cast_nullable_to_non_nullable
as int,delta: null == delta ? _self.delta : delta // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadItemDeltaState_Plan extends BridgeThreadItemDeltaState {
  const BridgeThreadItemDeltaState_Plan({required this.delta}): super._();


@override final  String delta;

/// Create a copy of BridgeThreadItemDeltaState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemDeltaState_PlanCopyWith<BridgeThreadItemDeltaState_Plan> get copyWith => _$BridgeThreadItemDeltaState_PlanCopyWithImpl<BridgeThreadItemDeltaState_Plan>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemDeltaState_Plan&&(identical(other.delta, delta) || other.delta == delta));
}


@override
int get hashCode => Object.hash(runtimeType,delta);

@override
String toString() {
  return 'BridgeThreadItemDeltaState.plan(delta: $delta)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemDeltaState_PlanCopyWith<$Res> implements $BridgeThreadItemDeltaStateCopyWith<$Res> {
  factory $BridgeThreadItemDeltaState_PlanCopyWith(BridgeThreadItemDeltaState_Plan value, $Res Function(BridgeThreadItemDeltaState_Plan) _then) = _$BridgeThreadItemDeltaState_PlanCopyWithImpl;
@override @useResult
$Res call({
 String delta
});




}
/// @nodoc
class _$BridgeThreadItemDeltaState_PlanCopyWithImpl<$Res>
    implements $BridgeThreadItemDeltaState_PlanCopyWith<$Res> {
  _$BridgeThreadItemDeltaState_PlanCopyWithImpl(this._self, this._then);

  final BridgeThreadItemDeltaState_Plan _self;
  final $Res Function(BridgeThreadItemDeltaState_Plan) _then;

/// Create a copy of BridgeThreadItemDeltaState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? delta = null,}) {
  return _then(BridgeThreadItemDeltaState_Plan(
delta: null == delta ? _self.delta : delta // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadItemDeltaState_ToolArguments extends BridgeThreadItemDeltaState {
  const BridgeThreadItemDeltaState_ToolArguments({required this.delta}): super._();


@override final  String delta;

/// Create a copy of BridgeThreadItemDeltaState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemDeltaState_ToolArgumentsCopyWith<BridgeThreadItemDeltaState_ToolArguments> get copyWith => _$BridgeThreadItemDeltaState_ToolArgumentsCopyWithImpl<BridgeThreadItemDeltaState_ToolArguments>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemDeltaState_ToolArguments&&(identical(other.delta, delta) || other.delta == delta));
}


@override
int get hashCode => Object.hash(runtimeType,delta);

@override
String toString() {
  return 'BridgeThreadItemDeltaState.toolArguments(delta: $delta)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemDeltaState_ToolArgumentsCopyWith<$Res> implements $BridgeThreadItemDeltaStateCopyWith<$Res> {
  factory $BridgeThreadItemDeltaState_ToolArgumentsCopyWith(BridgeThreadItemDeltaState_ToolArguments value, $Res Function(BridgeThreadItemDeltaState_ToolArguments) _then) = _$BridgeThreadItemDeltaState_ToolArgumentsCopyWithImpl;
@override @useResult
$Res call({
 String delta
});




}
/// @nodoc
class _$BridgeThreadItemDeltaState_ToolArgumentsCopyWithImpl<$Res>
    implements $BridgeThreadItemDeltaState_ToolArgumentsCopyWith<$Res> {
  _$BridgeThreadItemDeltaState_ToolArgumentsCopyWithImpl(this._self, this._then);

  final BridgeThreadItemDeltaState_ToolArguments _self;
  final $Res Function(BridgeThreadItemDeltaState_ToolArguments) _then;

/// Create a copy of BridgeThreadItemDeltaState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? delta = null,}) {
  return _then(BridgeThreadItemDeltaState_ToolArguments(
delta: null == delta ? _self.delta : delta // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadItemDeltaState_ToolResult extends BridgeThreadItemDeltaState {
  const BridgeThreadItemDeltaState_ToolResult({required this.delta}): super._();


@override final  String delta;

/// Create a copy of BridgeThreadItemDeltaState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemDeltaState_ToolResultCopyWith<BridgeThreadItemDeltaState_ToolResult> get copyWith => _$BridgeThreadItemDeltaState_ToolResultCopyWithImpl<BridgeThreadItemDeltaState_ToolResult>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemDeltaState_ToolResult&&(identical(other.delta, delta) || other.delta == delta));
}


@override
int get hashCode => Object.hash(runtimeType,delta);

@override
String toString() {
  return 'BridgeThreadItemDeltaState.toolResult(delta: $delta)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemDeltaState_ToolResultCopyWith<$Res> implements $BridgeThreadItemDeltaStateCopyWith<$Res> {
  factory $BridgeThreadItemDeltaState_ToolResultCopyWith(BridgeThreadItemDeltaState_ToolResult value, $Res Function(BridgeThreadItemDeltaState_ToolResult) _then) = _$BridgeThreadItemDeltaState_ToolResultCopyWithImpl;
@override @useResult
$Res call({
 String delta
});




}
/// @nodoc
class _$BridgeThreadItemDeltaState_ToolResultCopyWithImpl<$Res>
    implements $BridgeThreadItemDeltaState_ToolResultCopyWith<$Res> {
  _$BridgeThreadItemDeltaState_ToolResultCopyWithImpl(this._self, this._then);

  final BridgeThreadItemDeltaState_ToolResult _self;
  final $Res Function(BridgeThreadItemDeltaState_ToolResult) _then;

/// Create a copy of BridgeThreadItemDeltaState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? delta = null,}) {
  return _then(BridgeThreadItemDeltaState_ToolResult(
delta: null == delta ? _self.delta : delta // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeThreadItemState {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadItemState()';
}


}

/// @nodoc
class $BridgeThreadItemStateCopyWith<$Res>  {
$BridgeThreadItemStateCopyWith(BridgeThreadItemState _, $Res Function(BridgeThreadItemState) __);
}


/// Adds pattern-matching-related methods to [BridgeThreadItemState].
extension BridgeThreadItemStatePatterns on BridgeThreadItemState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeThreadItemState_Text value)?  text,TResult Function( BridgeThreadItemState_Thinking value)?  thinking,TResult Function( BridgeThreadItemState_Tool value)?  tool,TResult Function( BridgeThreadItemState_Agent value)?  agent,TResult Function( BridgeThreadItemState_Turn value)?  turn,TResult Function( BridgeThreadItemState_Inference value)?  inference,TResult Function( BridgeThreadItemState_Plan value)?  plan,TResult Function( BridgeThreadItemState_Skill value)?  skill,TResult Function( BridgeThreadItemState_File value)?  file,TResult Function( BridgeThreadItemState_ContextCompaction value)?  contextCompaction,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeThreadItemState_Text() when text != null:
return text(_that);case BridgeThreadItemState_Thinking() when thinking != null:
return thinking(_that);case BridgeThreadItemState_Tool() when tool != null:
return tool(_that);case BridgeThreadItemState_Agent() when agent != null:
return agent(_that);case BridgeThreadItemState_Turn() when turn != null:
return turn(_that);case BridgeThreadItemState_Inference() when inference != null:
return inference(_that);case BridgeThreadItemState_Plan() when plan != null:
return plan(_that);case BridgeThreadItemState_Skill() when skill != null:
return skill(_that);case BridgeThreadItemState_File() when file != null:
return file(_that);case BridgeThreadItemState_ContextCompaction() when contextCompaction != null:
return contextCompaction(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeThreadItemState_Text value)  text,required TResult Function( BridgeThreadItemState_Thinking value)  thinking,required TResult Function( BridgeThreadItemState_Tool value)  tool,required TResult Function( BridgeThreadItemState_Agent value)  agent,required TResult Function( BridgeThreadItemState_Turn value)  turn,required TResult Function( BridgeThreadItemState_Inference value)  inference,required TResult Function( BridgeThreadItemState_Plan value)  plan,required TResult Function( BridgeThreadItemState_Skill value)  skill,required TResult Function( BridgeThreadItemState_File value)  file,required TResult Function( BridgeThreadItemState_ContextCompaction value)  contextCompaction,}){
final _that = this;
switch (_that) {
case BridgeThreadItemState_Text():
return text(_that);case BridgeThreadItemState_Thinking():
return thinking(_that);case BridgeThreadItemState_Tool():
return tool(_that);case BridgeThreadItemState_Agent():
return agent(_that);case BridgeThreadItemState_Turn():
return turn(_that);case BridgeThreadItemState_Inference():
return inference(_that);case BridgeThreadItemState_Plan():
return plan(_that);case BridgeThreadItemState_Skill():
return skill(_that);case BridgeThreadItemState_File():
return file(_that);case BridgeThreadItemState_ContextCompaction():
return contextCompaction(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeThreadItemState_Text value)?  text,TResult? Function( BridgeThreadItemState_Thinking value)?  thinking,TResult? Function( BridgeThreadItemState_Tool value)?  tool,TResult? Function( BridgeThreadItemState_Agent value)?  agent,TResult? Function( BridgeThreadItemState_Turn value)?  turn,TResult? Function( BridgeThreadItemState_Inference value)?  inference,TResult? Function( BridgeThreadItemState_Plan value)?  plan,TResult? Function( BridgeThreadItemState_Skill value)?  skill,TResult? Function( BridgeThreadItemState_File value)?  file,TResult? Function( BridgeThreadItemState_ContextCompaction value)?  contextCompaction,}){
final _that = this;
switch (_that) {
case BridgeThreadItemState_Text() when text != null:
return text(_that);case BridgeThreadItemState_Thinking() when thinking != null:
return thinking(_that);case BridgeThreadItemState_Tool() when tool != null:
return tool(_that);case BridgeThreadItemState_Agent() when agent != null:
return agent(_that);case BridgeThreadItemState_Turn() when turn != null:
return turn(_that);case BridgeThreadItemState_Inference() when inference != null:
return inference(_that);case BridgeThreadItemState_Plan() when plan != null:
return plan(_that);case BridgeThreadItemState_Skill() when skill != null:
return skill(_that);case BridgeThreadItemState_File() when file != null:
return file(_that);case BridgeThreadItemState_ContextCompaction() when contextCompaction != null:
return contextCompaction(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeThreadTextChannel channel,  String text,  List<BridgeThreadAttachment> attachments,  BridgeThreadContentLifecycle lifecycle)?  text,TResult Function( List<String> summary,  List<String> content,  BridgeThreadContentLifecycle lifecycle)?  thinking,TResult Function( BridgeThreadToolInvocation invocation,  BridgeThreadToolState state)?  tool,TResult Function( BridgeThreadAgentIdentity identity,  BridgeThreadAgentState state)?  agent,TResult Function( BridgeTurnState state)?  turn,TResult Function( String inferenceId,  String model,  BridgeThreadInferenceState state)?  inference,TResult Function( String content,  BridgeThreadContentLifecycle lifecycle)?  plan,TResult Function( String name,  String source,  String providerId,  BridgeSkillResourceBase resourceBase,  BridgeSkillActivationCause cause,  PlatformInt64 activatedAt)?  skill,TResult Function( String path,  String? mediaType,  PlatformInt64 completedAt)?  file,TResult Function( BigInt beforeTokens,  BigInt afterTokens,  PlatformInt64 compactedAt)?  contextCompaction,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeThreadItemState_Text() when text != null:
return text(_that.channel,_that.text,_that.attachments,_that.lifecycle);case BridgeThreadItemState_Thinking() when thinking != null:
return thinking(_that.summary,_that.content,_that.lifecycle);case BridgeThreadItemState_Tool() when tool != null:
return tool(_that.invocation,_that.state);case BridgeThreadItemState_Agent() when agent != null:
return agent(_that.identity,_that.state);case BridgeThreadItemState_Turn() when turn != null:
return turn(_that.state);case BridgeThreadItemState_Inference() when inference != null:
return inference(_that.inferenceId,_that.model,_that.state);case BridgeThreadItemState_Plan() when plan != null:
return plan(_that.content,_that.lifecycle);case BridgeThreadItemState_Skill() when skill != null:
return skill(_that.name,_that.source,_that.providerId,_that.resourceBase,_that.cause,_that.activatedAt);case BridgeThreadItemState_File() when file != null:
return file(_that.path,_that.mediaType,_that.completedAt);case BridgeThreadItemState_ContextCompaction() when contextCompaction != null:
return contextCompaction(_that.beforeTokens,_that.afterTokens,_that.compactedAt);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeThreadTextChannel channel,  String text,  List<BridgeThreadAttachment> attachments,  BridgeThreadContentLifecycle lifecycle)  text,required TResult Function( List<String> summary,  List<String> content,  BridgeThreadContentLifecycle lifecycle)  thinking,required TResult Function( BridgeThreadToolInvocation invocation,  BridgeThreadToolState state)  tool,required TResult Function( BridgeThreadAgentIdentity identity,  BridgeThreadAgentState state)  agent,required TResult Function( BridgeTurnState state)  turn,required TResult Function( String inferenceId,  String model,  BridgeThreadInferenceState state)  inference,required TResult Function( String content,  BridgeThreadContentLifecycle lifecycle)  plan,required TResult Function( String name,  String source,  String providerId,  BridgeSkillResourceBase resourceBase,  BridgeSkillActivationCause cause,  PlatformInt64 activatedAt)  skill,required TResult Function( String path,  String? mediaType,  PlatformInt64 completedAt)  file,required TResult Function( BigInt beforeTokens,  BigInt afterTokens,  PlatformInt64 compactedAt)  contextCompaction,}) {final _that = this;
switch (_that) {
case BridgeThreadItemState_Text():
return text(_that.channel,_that.text,_that.attachments,_that.lifecycle);case BridgeThreadItemState_Thinking():
return thinking(_that.summary,_that.content,_that.lifecycle);case BridgeThreadItemState_Tool():
return tool(_that.invocation,_that.state);case BridgeThreadItemState_Agent():
return agent(_that.identity,_that.state);case BridgeThreadItemState_Turn():
return turn(_that.state);case BridgeThreadItemState_Inference():
return inference(_that.inferenceId,_that.model,_that.state);case BridgeThreadItemState_Plan():
return plan(_that.content,_that.lifecycle);case BridgeThreadItemState_Skill():
return skill(_that.name,_that.source,_that.providerId,_that.resourceBase,_that.cause,_that.activatedAt);case BridgeThreadItemState_File():
return file(_that.path,_that.mediaType,_that.completedAt);case BridgeThreadItemState_ContextCompaction():
return contextCompaction(_that.beforeTokens,_that.afterTokens,_that.compactedAt);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeThreadTextChannel channel,  String text,  List<BridgeThreadAttachment> attachments,  BridgeThreadContentLifecycle lifecycle)?  text,TResult? Function( List<String> summary,  List<String> content,  BridgeThreadContentLifecycle lifecycle)?  thinking,TResult? Function( BridgeThreadToolInvocation invocation,  BridgeThreadToolState state)?  tool,TResult? Function( BridgeThreadAgentIdentity identity,  BridgeThreadAgentState state)?  agent,TResult? Function( BridgeTurnState state)?  turn,TResult? Function( String inferenceId,  String model,  BridgeThreadInferenceState state)?  inference,TResult? Function( String content,  BridgeThreadContentLifecycle lifecycle)?  plan,TResult? Function( String name,  String source,  String providerId,  BridgeSkillResourceBase resourceBase,  BridgeSkillActivationCause cause,  PlatformInt64 activatedAt)?  skill,TResult? Function( String path,  String? mediaType,  PlatformInt64 completedAt)?  file,TResult? Function( BigInt beforeTokens,  BigInt afterTokens,  PlatformInt64 compactedAt)?  contextCompaction,}) {final _that = this;
switch (_that) {
case BridgeThreadItemState_Text() when text != null:
return text(_that.channel,_that.text,_that.attachments,_that.lifecycle);case BridgeThreadItemState_Thinking() when thinking != null:
return thinking(_that.summary,_that.content,_that.lifecycle);case BridgeThreadItemState_Tool() when tool != null:
return tool(_that.invocation,_that.state);case BridgeThreadItemState_Agent() when agent != null:
return agent(_that.identity,_that.state);case BridgeThreadItemState_Turn() when turn != null:
return turn(_that.state);case BridgeThreadItemState_Inference() when inference != null:
return inference(_that.inferenceId,_that.model,_that.state);case BridgeThreadItemState_Plan() when plan != null:
return plan(_that.content,_that.lifecycle);case BridgeThreadItemState_Skill() when skill != null:
return skill(_that.name,_that.source,_that.providerId,_that.resourceBase,_that.cause,_that.activatedAt);case BridgeThreadItemState_File() when file != null:
return file(_that.path,_that.mediaType,_that.completedAt);case BridgeThreadItemState_ContextCompaction() when contextCompaction != null:
return contextCompaction(_that.beforeTokens,_that.afterTokens,_that.compactedAt);case _:
  return null;

}
}

}

/// @nodoc


class BridgeThreadItemState_Text extends BridgeThreadItemState {
  const BridgeThreadItemState_Text({required this.channel, required this.text, required  List<BridgeThreadAttachment> attachments, required this.lifecycle}): _attachments = attachments,super._();


 final  BridgeThreadTextChannel channel;
 final  String text;
 final  List<BridgeThreadAttachment> _attachments;
 List<BridgeThreadAttachment> get attachments {
  if (_attachments is EqualUnmodifiableListView) return _attachments;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_attachments);
}

 final  BridgeThreadContentLifecycle lifecycle;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemState_TextCopyWith<BridgeThreadItemState_Text> get copyWith => _$BridgeThreadItemState_TextCopyWithImpl<BridgeThreadItemState_Text>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemState_Text&&(identical(other.channel, channel) || other.channel == channel)&&(identical(other.text, text) || other.text == text)&&const DeepCollectionEquality().equals(other._attachments, _attachments)&&(identical(other.lifecycle, lifecycle) || other.lifecycle == lifecycle));
}


@override
int get hashCode => Object.hash(runtimeType,channel,text,const DeepCollectionEquality().hash(_attachments),lifecycle);

@override
String toString() {
  return 'BridgeThreadItemState.text(channel: $channel, text: $text, attachments: $attachments, lifecycle: $lifecycle)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemState_TextCopyWith<$Res> implements $BridgeThreadItemStateCopyWith<$Res> {
  factory $BridgeThreadItemState_TextCopyWith(BridgeThreadItemState_Text value, $Res Function(BridgeThreadItemState_Text) _then) = _$BridgeThreadItemState_TextCopyWithImpl;
@useResult
$Res call({
 BridgeThreadTextChannel channel, String text, List<BridgeThreadAttachment> attachments, BridgeThreadContentLifecycle lifecycle
});


$BridgeThreadContentLifecycleCopyWith<$Res> get lifecycle;

}
/// @nodoc
class _$BridgeThreadItemState_TextCopyWithImpl<$Res>
    implements $BridgeThreadItemState_TextCopyWith<$Res> {
  _$BridgeThreadItemState_TextCopyWithImpl(this._self, this._then);

  final BridgeThreadItemState_Text _self;
  final $Res Function(BridgeThreadItemState_Text) _then;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? channel = null,Object? text = null,Object? attachments = null,Object? lifecycle = null,}) {
  return _then(BridgeThreadItemState_Text(
channel: null == channel ? _self.channel : channel // ignore: cast_nullable_to_non_nullable
as BridgeThreadTextChannel,text: null == text ? _self.text : text // ignore: cast_nullable_to_non_nullable
as String,attachments: null == attachments ? _self._attachments : attachments // ignore: cast_nullable_to_non_nullable
as List<BridgeThreadAttachment>,lifecycle: null == lifecycle ? _self.lifecycle : lifecycle // ignore: cast_nullable_to_non_nullable
as BridgeThreadContentLifecycle,
  ));
}

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeThreadContentLifecycleCopyWith<$Res> get lifecycle {

  return $BridgeThreadContentLifecycleCopyWith<$Res>(_self.lifecycle, (value) {
    return _then(_self.copyWith(lifecycle: value));
  });
}
}

/// @nodoc


class BridgeThreadItemState_Thinking extends BridgeThreadItemState {
  const BridgeThreadItemState_Thinking({required  List<String> summary, required  List<String> content, required this.lifecycle}): _summary = summary,_content = content,super._();


 final  List<String> _summary;
 List<String> get summary {
  if (_summary is EqualUnmodifiableListView) return _summary;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_summary);
}

 final  List<String> _content;
 List<String> get content {
  if (_content is EqualUnmodifiableListView) return _content;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_content);
}

 final  BridgeThreadContentLifecycle lifecycle;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemState_ThinkingCopyWith<BridgeThreadItemState_Thinking> get copyWith => _$BridgeThreadItemState_ThinkingCopyWithImpl<BridgeThreadItemState_Thinking>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemState_Thinking&&const DeepCollectionEquality().equals(other._summary, _summary)&&const DeepCollectionEquality().equals(other._content, _content)&&(identical(other.lifecycle, lifecycle) || other.lifecycle == lifecycle));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_summary),const DeepCollectionEquality().hash(_content),lifecycle);

@override
String toString() {
  return 'BridgeThreadItemState.thinking(summary: $summary, content: $content, lifecycle: $lifecycle)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemState_ThinkingCopyWith<$Res> implements $BridgeThreadItemStateCopyWith<$Res> {
  factory $BridgeThreadItemState_ThinkingCopyWith(BridgeThreadItemState_Thinking value, $Res Function(BridgeThreadItemState_Thinking) _then) = _$BridgeThreadItemState_ThinkingCopyWithImpl;
@useResult
$Res call({
 List<String> summary, List<String> content, BridgeThreadContentLifecycle lifecycle
});


$BridgeThreadContentLifecycleCopyWith<$Res> get lifecycle;

}
/// @nodoc
class _$BridgeThreadItemState_ThinkingCopyWithImpl<$Res>
    implements $BridgeThreadItemState_ThinkingCopyWith<$Res> {
  _$BridgeThreadItemState_ThinkingCopyWithImpl(this._self, this._then);

  final BridgeThreadItemState_Thinking _self;
  final $Res Function(BridgeThreadItemState_Thinking) _then;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? summary = null,Object? content = null,Object? lifecycle = null,}) {
  return _then(BridgeThreadItemState_Thinking(
summary: null == summary ? _self._summary : summary // ignore: cast_nullable_to_non_nullable
as List<String>,content: null == content ? _self._content : content // ignore: cast_nullable_to_non_nullable
as List<String>,lifecycle: null == lifecycle ? _self.lifecycle : lifecycle // ignore: cast_nullable_to_non_nullable
as BridgeThreadContentLifecycle,
  ));
}

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeThreadContentLifecycleCopyWith<$Res> get lifecycle {

  return $BridgeThreadContentLifecycleCopyWith<$Res>(_self.lifecycle, (value) {
    return _then(_self.copyWith(lifecycle: value));
  });
}
}

/// @nodoc


class BridgeThreadItemState_Tool extends BridgeThreadItemState {
  const BridgeThreadItemState_Tool({required this.invocation, required this.state}): super._();


 final  BridgeThreadToolInvocation invocation;
 final  BridgeThreadToolState state;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemState_ToolCopyWith<BridgeThreadItemState_Tool> get copyWith => _$BridgeThreadItemState_ToolCopyWithImpl<BridgeThreadItemState_Tool>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemState_Tool&&(identical(other.invocation, invocation) || other.invocation == invocation)&&(identical(other.state, state) || other.state == state));
}


@override
int get hashCode => Object.hash(runtimeType,invocation,state);

@override
String toString() {
  return 'BridgeThreadItemState.tool(invocation: $invocation, state: $state)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemState_ToolCopyWith<$Res> implements $BridgeThreadItemStateCopyWith<$Res> {
  factory $BridgeThreadItemState_ToolCopyWith(BridgeThreadItemState_Tool value, $Res Function(BridgeThreadItemState_Tool) _then) = _$BridgeThreadItemState_ToolCopyWithImpl;
@useResult
$Res call({
 BridgeThreadToolInvocation invocation, BridgeThreadToolState state
});


$BridgeThreadToolStateCopyWith<$Res> get state;

}
/// @nodoc
class _$BridgeThreadItemState_ToolCopyWithImpl<$Res>
    implements $BridgeThreadItemState_ToolCopyWith<$Res> {
  _$BridgeThreadItemState_ToolCopyWithImpl(this._self, this._then);

  final BridgeThreadItemState_Tool _self;
  final $Res Function(BridgeThreadItemState_Tool) _then;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? invocation = null,Object? state = null,}) {
  return _then(BridgeThreadItemState_Tool(
invocation: null == invocation ? _self.invocation : invocation // ignore: cast_nullable_to_non_nullable
as BridgeThreadToolInvocation,state: null == state ? _self.state : state // ignore: cast_nullable_to_non_nullable
as BridgeThreadToolState,
  ));
}

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeThreadToolStateCopyWith<$Res> get state {

  return $BridgeThreadToolStateCopyWith<$Res>(_self.state, (value) {
    return _then(_self.copyWith(state: value));
  });
}
}

/// @nodoc


class BridgeThreadItemState_Agent extends BridgeThreadItemState {
  const BridgeThreadItemState_Agent({required this.identity, required this.state}): super._();


 final  BridgeThreadAgentIdentity identity;
 final  BridgeThreadAgentState state;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemState_AgentCopyWith<BridgeThreadItemState_Agent> get copyWith => _$BridgeThreadItemState_AgentCopyWithImpl<BridgeThreadItemState_Agent>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemState_Agent&&(identical(other.identity, identity) || other.identity == identity)&&(identical(other.state, state) || other.state == state));
}


@override
int get hashCode => Object.hash(runtimeType,identity,state);

@override
String toString() {
  return 'BridgeThreadItemState.agent(identity: $identity, state: $state)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemState_AgentCopyWith<$Res> implements $BridgeThreadItemStateCopyWith<$Res> {
  factory $BridgeThreadItemState_AgentCopyWith(BridgeThreadItemState_Agent value, $Res Function(BridgeThreadItemState_Agent) _then) = _$BridgeThreadItemState_AgentCopyWithImpl;
@useResult
$Res call({
 BridgeThreadAgentIdentity identity, BridgeThreadAgentState state
});


$BridgeThreadAgentStateCopyWith<$Res> get state;

}
/// @nodoc
class _$BridgeThreadItemState_AgentCopyWithImpl<$Res>
    implements $BridgeThreadItemState_AgentCopyWith<$Res> {
  _$BridgeThreadItemState_AgentCopyWithImpl(this._self, this._then);

  final BridgeThreadItemState_Agent _self;
  final $Res Function(BridgeThreadItemState_Agent) _then;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? identity = null,Object? state = null,}) {
  return _then(BridgeThreadItemState_Agent(
identity: null == identity ? _self.identity : identity // ignore: cast_nullable_to_non_nullable
as BridgeThreadAgentIdentity,state: null == state ? _self.state : state // ignore: cast_nullable_to_non_nullable
as BridgeThreadAgentState,
  ));
}

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeThreadAgentStateCopyWith<$Res> get state {

  return $BridgeThreadAgentStateCopyWith<$Res>(_self.state, (value) {
    return _then(_self.copyWith(state: value));
  });
}
}

/// @nodoc


class BridgeThreadItemState_Turn extends BridgeThreadItemState {
  const BridgeThreadItemState_Turn({required this.state}): super._();


 final  BridgeTurnState state;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemState_TurnCopyWith<BridgeThreadItemState_Turn> get copyWith => _$BridgeThreadItemState_TurnCopyWithImpl<BridgeThreadItemState_Turn>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemState_Turn&&(identical(other.state, state) || other.state == state));
}


@override
int get hashCode => Object.hash(runtimeType,state);

@override
String toString() {
  return 'BridgeThreadItemState.turn(state: $state)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemState_TurnCopyWith<$Res> implements $BridgeThreadItemStateCopyWith<$Res> {
  factory $BridgeThreadItemState_TurnCopyWith(BridgeThreadItemState_Turn value, $Res Function(BridgeThreadItemState_Turn) _then) = _$BridgeThreadItemState_TurnCopyWithImpl;
@useResult
$Res call({
 BridgeTurnState state
});


$BridgeTurnStateCopyWith<$Res> get state;

}
/// @nodoc
class _$BridgeThreadItemState_TurnCopyWithImpl<$Res>
    implements $BridgeThreadItemState_TurnCopyWith<$Res> {
  _$BridgeThreadItemState_TurnCopyWithImpl(this._self, this._then);

  final BridgeThreadItemState_Turn _self;
  final $Res Function(BridgeThreadItemState_Turn) _then;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? state = null,}) {
  return _then(BridgeThreadItemState_Turn(
state: null == state ? _self.state : state // ignore: cast_nullable_to_non_nullable
as BridgeTurnState,
  ));
}

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeTurnStateCopyWith<$Res> get state {

  return $BridgeTurnStateCopyWith<$Res>(_self.state, (value) {
    return _then(_self.copyWith(state: value));
  });
}
}

/// @nodoc


class BridgeThreadItemState_Inference extends BridgeThreadItemState {
  const BridgeThreadItemState_Inference({required this.inferenceId, required this.model, required this.state}): super._();


 final  String inferenceId;
 final  String model;
 final  BridgeThreadInferenceState state;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemState_InferenceCopyWith<BridgeThreadItemState_Inference> get copyWith => _$BridgeThreadItemState_InferenceCopyWithImpl<BridgeThreadItemState_Inference>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemState_Inference&&(identical(other.inferenceId, inferenceId) || other.inferenceId == inferenceId)&&(identical(other.model, model) || other.model == model)&&(identical(other.state, state) || other.state == state));
}


@override
int get hashCode => Object.hash(runtimeType,inferenceId,model,state);

@override
String toString() {
  return 'BridgeThreadItemState.inference(inferenceId: $inferenceId, model: $model, state: $state)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemState_InferenceCopyWith<$Res> implements $BridgeThreadItemStateCopyWith<$Res> {
  factory $BridgeThreadItemState_InferenceCopyWith(BridgeThreadItemState_Inference value, $Res Function(BridgeThreadItemState_Inference) _then) = _$BridgeThreadItemState_InferenceCopyWithImpl;
@useResult
$Res call({
 String inferenceId, String model, BridgeThreadInferenceState state
});


$BridgeThreadInferenceStateCopyWith<$Res> get state;

}
/// @nodoc
class _$BridgeThreadItemState_InferenceCopyWithImpl<$Res>
    implements $BridgeThreadItemState_InferenceCopyWith<$Res> {
  _$BridgeThreadItemState_InferenceCopyWithImpl(this._self, this._then);

  final BridgeThreadItemState_Inference _self;
  final $Res Function(BridgeThreadItemState_Inference) _then;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? inferenceId = null,Object? model = null,Object? state = null,}) {
  return _then(BridgeThreadItemState_Inference(
inferenceId: null == inferenceId ? _self.inferenceId : inferenceId // ignore: cast_nullable_to_non_nullable
as String,model: null == model ? _self.model : model // ignore: cast_nullable_to_non_nullable
as String,state: null == state ? _self.state : state // ignore: cast_nullable_to_non_nullable
as BridgeThreadInferenceState,
  ));
}

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeThreadInferenceStateCopyWith<$Res> get state {

  return $BridgeThreadInferenceStateCopyWith<$Res>(_self.state, (value) {
    return _then(_self.copyWith(state: value));
  });
}
}

/// @nodoc


class BridgeThreadItemState_Plan extends BridgeThreadItemState {
  const BridgeThreadItemState_Plan({required this.content, required this.lifecycle}): super._();


 final  String content;
 final  BridgeThreadContentLifecycle lifecycle;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemState_PlanCopyWith<BridgeThreadItemState_Plan> get copyWith => _$BridgeThreadItemState_PlanCopyWithImpl<BridgeThreadItemState_Plan>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemState_Plan&&(identical(other.content, content) || other.content == content)&&(identical(other.lifecycle, lifecycle) || other.lifecycle == lifecycle));
}


@override
int get hashCode => Object.hash(runtimeType,content,lifecycle);

@override
String toString() {
  return 'BridgeThreadItemState.plan(content: $content, lifecycle: $lifecycle)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemState_PlanCopyWith<$Res> implements $BridgeThreadItemStateCopyWith<$Res> {
  factory $BridgeThreadItemState_PlanCopyWith(BridgeThreadItemState_Plan value, $Res Function(BridgeThreadItemState_Plan) _then) = _$BridgeThreadItemState_PlanCopyWithImpl;
@useResult
$Res call({
 String content, BridgeThreadContentLifecycle lifecycle
});


$BridgeThreadContentLifecycleCopyWith<$Res> get lifecycle;

}
/// @nodoc
class _$BridgeThreadItemState_PlanCopyWithImpl<$Res>
    implements $BridgeThreadItemState_PlanCopyWith<$Res> {
  _$BridgeThreadItemState_PlanCopyWithImpl(this._self, this._then);

  final BridgeThreadItemState_Plan _self;
  final $Res Function(BridgeThreadItemState_Plan) _then;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? content = null,Object? lifecycle = null,}) {
  return _then(BridgeThreadItemState_Plan(
content: null == content ? _self.content : content // ignore: cast_nullable_to_non_nullable
as String,lifecycle: null == lifecycle ? _self.lifecycle : lifecycle // ignore: cast_nullable_to_non_nullable
as BridgeThreadContentLifecycle,
  ));
}

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeThreadContentLifecycleCopyWith<$Res> get lifecycle {

  return $BridgeThreadContentLifecycleCopyWith<$Res>(_self.lifecycle, (value) {
    return _then(_self.copyWith(lifecycle: value));
  });
}
}

/// @nodoc


class BridgeThreadItemState_Skill extends BridgeThreadItemState {
  const BridgeThreadItemState_Skill({required this.name, required this.source, required this.providerId, required this.resourceBase, required this.cause, required this.activatedAt}): super._();


 final  String name;
 final  String source;
 final  String providerId;
 final  BridgeSkillResourceBase resourceBase;
 final  BridgeSkillActivationCause cause;
 final  PlatformInt64 activatedAt;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemState_SkillCopyWith<BridgeThreadItemState_Skill> get copyWith => _$BridgeThreadItemState_SkillCopyWithImpl<BridgeThreadItemState_Skill>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemState_Skill&&(identical(other.name, name) || other.name == name)&&(identical(other.source, source) || other.source == source)&&(identical(other.providerId, providerId) || other.providerId == providerId)&&(identical(other.resourceBase, resourceBase) || other.resourceBase == resourceBase)&&(identical(other.cause, cause) || other.cause == cause)&&(identical(other.activatedAt, activatedAt) || other.activatedAt == activatedAt));
}


@override
int get hashCode => Object.hash(runtimeType,name,source,providerId,resourceBase,cause,activatedAt);

@override
String toString() {
  return 'BridgeThreadItemState.skill(name: $name, source: $source, providerId: $providerId, resourceBase: $resourceBase, cause: $cause, activatedAt: $activatedAt)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemState_SkillCopyWith<$Res> implements $BridgeThreadItemStateCopyWith<$Res> {
  factory $BridgeThreadItemState_SkillCopyWith(BridgeThreadItemState_Skill value, $Res Function(BridgeThreadItemState_Skill) _then) = _$BridgeThreadItemState_SkillCopyWithImpl;
@useResult
$Res call({
 String name, String source, String providerId, BridgeSkillResourceBase resourceBase, BridgeSkillActivationCause cause, PlatformInt64 activatedAt
});


$BridgeSkillResourceBaseCopyWith<$Res> get resourceBase;$BridgeSkillActivationCauseCopyWith<$Res> get cause;

}
/// @nodoc
class _$BridgeThreadItemState_SkillCopyWithImpl<$Res>
    implements $BridgeThreadItemState_SkillCopyWith<$Res> {
  _$BridgeThreadItemState_SkillCopyWithImpl(this._self, this._then);

  final BridgeThreadItemState_Skill _self;
  final $Res Function(BridgeThreadItemState_Skill) _then;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? name = null,Object? source = null,Object? providerId = null,Object? resourceBase = null,Object? cause = null,Object? activatedAt = null,}) {
  return _then(BridgeThreadItemState_Skill(
name: null == name ? _self.name : name // ignore: cast_nullable_to_non_nullable
as String,source: null == source ? _self.source : source // ignore: cast_nullable_to_non_nullable
as String,providerId: null == providerId ? _self.providerId : providerId // ignore: cast_nullable_to_non_nullable
as String,resourceBase: null == resourceBase ? _self.resourceBase : resourceBase // ignore: cast_nullable_to_non_nullable
as BridgeSkillResourceBase,cause: null == cause ? _self.cause : cause // ignore: cast_nullable_to_non_nullable
as BridgeSkillActivationCause,activatedAt: null == activatedAt ? _self.activatedAt : activatedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,
  ));
}

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeSkillResourceBaseCopyWith<$Res> get resourceBase {

  return $BridgeSkillResourceBaseCopyWith<$Res>(_self.resourceBase, (value) {
    return _then(_self.copyWith(resourceBase: value));
  });
}/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeSkillActivationCauseCopyWith<$Res> get cause {

  return $BridgeSkillActivationCauseCopyWith<$Res>(_self.cause, (value) {
    return _then(_self.copyWith(cause: value));
  });
}
}

/// @nodoc


class BridgeThreadItemState_File extends BridgeThreadItemState {
  const BridgeThreadItemState_File({required this.path, this.mediaType, required this.completedAt}): super._();


 final  String path;
 final  String? mediaType;
 final  PlatformInt64 completedAt;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemState_FileCopyWith<BridgeThreadItemState_File> get copyWith => _$BridgeThreadItemState_FileCopyWithImpl<BridgeThreadItemState_File>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemState_File&&(identical(other.path, path) || other.path == path)&&(identical(other.mediaType, mediaType) || other.mediaType == mediaType)&&(identical(other.completedAt, completedAt) || other.completedAt == completedAt));
}


@override
int get hashCode => Object.hash(runtimeType,path,mediaType,completedAt);

@override
String toString() {
  return 'BridgeThreadItemState.file(path: $path, mediaType: $mediaType, completedAt: $completedAt)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemState_FileCopyWith<$Res> implements $BridgeThreadItemStateCopyWith<$Res> {
  factory $BridgeThreadItemState_FileCopyWith(BridgeThreadItemState_File value, $Res Function(BridgeThreadItemState_File) _then) = _$BridgeThreadItemState_FileCopyWithImpl;
@useResult
$Res call({
 String path, String? mediaType, PlatformInt64 completedAt
});




}
/// @nodoc
class _$BridgeThreadItemState_FileCopyWithImpl<$Res>
    implements $BridgeThreadItemState_FileCopyWith<$Res> {
  _$BridgeThreadItemState_FileCopyWithImpl(this._self, this._then);

  final BridgeThreadItemState_File _self;
  final $Res Function(BridgeThreadItemState_File) _then;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? path = null,Object? mediaType = freezed,Object? completedAt = null,}) {
  return _then(BridgeThreadItemState_File(
path: null == path ? _self.path : path // ignore: cast_nullable_to_non_nullable
as String,mediaType: freezed == mediaType ? _self.mediaType : mediaType // ignore: cast_nullable_to_non_nullable
as String?,completedAt: null == completedAt ? _self.completedAt : completedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,
  ));
}


}

/// @nodoc


class BridgeThreadItemState_ContextCompaction extends BridgeThreadItemState {
  const BridgeThreadItemState_ContextCompaction({required this.beforeTokens, required this.afterTokens, required this.compactedAt}): super._();


 final  BigInt beforeTokens;
 final  BigInt afterTokens;
 final  PlatformInt64 compactedAt;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemState_ContextCompactionCopyWith<BridgeThreadItemState_ContextCompaction> get copyWith => _$BridgeThreadItemState_ContextCompactionCopyWithImpl<BridgeThreadItemState_ContextCompaction>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemState_ContextCompaction&&(identical(other.beforeTokens, beforeTokens) || other.beforeTokens == beforeTokens)&&(identical(other.afterTokens, afterTokens) || other.afterTokens == afterTokens)&&(identical(other.compactedAt, compactedAt) || other.compactedAt == compactedAt));
}


@override
int get hashCode => Object.hash(runtimeType,beforeTokens,afterTokens,compactedAt);

@override
String toString() {
  return 'BridgeThreadItemState.contextCompaction(beforeTokens: $beforeTokens, afterTokens: $afterTokens, compactedAt: $compactedAt)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemState_ContextCompactionCopyWith<$Res> implements $BridgeThreadItemStateCopyWith<$Res> {
  factory $BridgeThreadItemState_ContextCompactionCopyWith(BridgeThreadItemState_ContextCompaction value, $Res Function(BridgeThreadItemState_ContextCompaction) _then) = _$BridgeThreadItemState_ContextCompactionCopyWithImpl;
@useResult
$Res call({
 BigInt beforeTokens, BigInt afterTokens, PlatformInt64 compactedAt
});




}
/// @nodoc
class _$BridgeThreadItemState_ContextCompactionCopyWithImpl<$Res>
    implements $BridgeThreadItemState_ContextCompactionCopyWith<$Res> {
  _$BridgeThreadItemState_ContextCompactionCopyWithImpl(this._self, this._then);

  final BridgeThreadItemState_ContextCompaction _self;
  final $Res Function(BridgeThreadItemState_ContextCompaction) _then;

/// Create a copy of BridgeThreadItemState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? beforeTokens = null,Object? afterTokens = null,Object? compactedAt = null,}) {
  return _then(BridgeThreadItemState_ContextCompaction(
beforeTokens: null == beforeTokens ? _self.beforeTokens : beforeTokens // ignore: cast_nullable_to_non_nullable
as BigInt,afterTokens: null == afterTokens ? _self.afterTokens : afterTokens // ignore: cast_nullable_to_non_nullable
as BigInt,compactedAt: null == compactedAt ? _self.compactedAt : compactedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,
  ));
}


}

/// @nodoc
mixin _$BridgeThreadToolState {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadToolState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadToolState()';
}


}

/// @nodoc
class $BridgeThreadToolStateCopyWith<$Res>  {
$BridgeThreadToolStateCopyWith(BridgeThreadToolState _, $Res Function(BridgeThreadToolState) __);
}


/// Adds pattern-matching-related methods to [BridgeThreadToolState].
extension BridgeThreadToolStatePatterns on BridgeThreadToolState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeThreadToolState_Started value)?  started,TResult Function( BridgeThreadToolState_Streaming value)?  streaming,TResult Function( BridgeThreadToolState_AwaitingApproval value)?  awaitingApproval,TResult Function( BridgeThreadToolState_Approved value)?  approved,TResult Function( BridgeThreadToolState_Running value)?  running,TResult Function( BridgeThreadToolState_Succeeded value)?  succeeded,TResult Function( BridgeThreadToolState_Failed value)?  failed,TResult Function( BridgeThreadToolState_Denied value)?  denied,TResult Function( BridgeThreadToolState_Cancelled value)?  cancelled,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeThreadToolState_Started() when started != null:
return started(_that);case BridgeThreadToolState_Streaming() when streaming != null:
return streaming(_that);case BridgeThreadToolState_AwaitingApproval() when awaitingApproval != null:
return awaitingApproval(_that);case BridgeThreadToolState_Approved() when approved != null:
return approved(_that);case BridgeThreadToolState_Running() when running != null:
return running(_that);case BridgeThreadToolState_Succeeded() when succeeded != null:
return succeeded(_that);case BridgeThreadToolState_Failed() when failed != null:
return failed(_that);case BridgeThreadToolState_Denied() when denied != null:
return denied(_that);case BridgeThreadToolState_Cancelled() when cancelled != null:
return cancelled(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeThreadToolState_Started value)  started,required TResult Function( BridgeThreadToolState_Streaming value)  streaming,required TResult Function( BridgeThreadToolState_AwaitingApproval value)  awaitingApproval,required TResult Function( BridgeThreadToolState_Approved value)  approved,required TResult Function( BridgeThreadToolState_Running value)  running,required TResult Function( BridgeThreadToolState_Succeeded value)  succeeded,required TResult Function( BridgeThreadToolState_Failed value)  failed,required TResult Function( BridgeThreadToolState_Denied value)  denied,required TResult Function( BridgeThreadToolState_Cancelled value)  cancelled,}){
final _that = this;
switch (_that) {
case BridgeThreadToolState_Started():
return started(_that);case BridgeThreadToolState_Streaming():
return streaming(_that);case BridgeThreadToolState_AwaitingApproval():
return awaitingApproval(_that);case BridgeThreadToolState_Approved():
return approved(_that);case BridgeThreadToolState_Running():
return running(_that);case BridgeThreadToolState_Succeeded():
return succeeded(_that);case BridgeThreadToolState_Failed():
return failed(_that);case BridgeThreadToolState_Denied():
return denied(_that);case BridgeThreadToolState_Cancelled():
return cancelled(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeThreadToolState_Started value)?  started,TResult? Function( BridgeThreadToolState_Streaming value)?  streaming,TResult? Function( BridgeThreadToolState_AwaitingApproval value)?  awaitingApproval,TResult? Function( BridgeThreadToolState_Approved value)?  approved,TResult? Function( BridgeThreadToolState_Running value)?  running,TResult? Function( BridgeThreadToolState_Succeeded value)?  succeeded,TResult? Function( BridgeThreadToolState_Failed value)?  failed,TResult? Function( BridgeThreadToolState_Denied value)?  denied,TResult? Function( BridgeThreadToolState_Cancelled value)?  cancelled,}){
final _that = this;
switch (_that) {
case BridgeThreadToolState_Started() when started != null:
return started(_that);case BridgeThreadToolState_Streaming() when streaming != null:
return streaming(_that);case BridgeThreadToolState_AwaitingApproval() when awaitingApproval != null:
return awaitingApproval(_that);case BridgeThreadToolState_Approved() when approved != null:
return approved(_that);case BridgeThreadToolState_Running() when running != null:
return running(_that);case BridgeThreadToolState_Succeeded() when succeeded != null:
return succeeded(_that);case BridgeThreadToolState_Failed() when failed != null:
return failed(_that);case BridgeThreadToolState_Denied() when denied != null:
return denied(_that);case BridgeThreadToolState_Cancelled() when cancelled != null:
return cancelled(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  started,TResult Function()?  streaming,TResult Function()?  awaitingApproval,TResult Function()?  approved,TResult Function( String streamedOutput)?  running,TResult Function( PlatformInt64 completedAt,  BridgeThreadToolOutput output)?  succeeded,TResult Function( PlatformInt64 failedAt,  BridgeThreadToolFailure failure,  BridgeThreadToolOutput? output)?  failed,TResult Function( PlatformInt64 deniedAt,  String reason)?  denied,TResult Function( PlatformInt64 cancelledAt,  String reason)?  cancelled,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeThreadToolState_Started() when started != null:
return started();case BridgeThreadToolState_Streaming() when streaming != null:
return streaming();case BridgeThreadToolState_AwaitingApproval() when awaitingApproval != null:
return awaitingApproval();case BridgeThreadToolState_Approved() when approved != null:
return approved();case BridgeThreadToolState_Running() when running != null:
return running(_that.streamedOutput);case BridgeThreadToolState_Succeeded() when succeeded != null:
return succeeded(_that.completedAt,_that.output);case BridgeThreadToolState_Failed() when failed != null:
return failed(_that.failedAt,_that.failure,_that.output);case BridgeThreadToolState_Denied() when denied != null:
return denied(_that.deniedAt,_that.reason);case BridgeThreadToolState_Cancelled() when cancelled != null:
return cancelled(_that.cancelledAt,_that.reason);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  started,required TResult Function()  streaming,required TResult Function()  awaitingApproval,required TResult Function()  approved,required TResult Function( String streamedOutput)  running,required TResult Function( PlatformInt64 completedAt,  BridgeThreadToolOutput output)  succeeded,required TResult Function( PlatformInt64 failedAt,  BridgeThreadToolFailure failure,  BridgeThreadToolOutput? output)  failed,required TResult Function( PlatformInt64 deniedAt,  String reason)  denied,required TResult Function( PlatformInt64 cancelledAt,  String reason)  cancelled,}) {final _that = this;
switch (_that) {
case BridgeThreadToolState_Started():
return started();case BridgeThreadToolState_Streaming():
return streaming();case BridgeThreadToolState_AwaitingApproval():
return awaitingApproval();case BridgeThreadToolState_Approved():
return approved();case BridgeThreadToolState_Running():
return running(_that.streamedOutput);case BridgeThreadToolState_Succeeded():
return succeeded(_that.completedAt,_that.output);case BridgeThreadToolState_Failed():
return failed(_that.failedAt,_that.failure,_that.output);case BridgeThreadToolState_Denied():
return denied(_that.deniedAt,_that.reason);case BridgeThreadToolState_Cancelled():
return cancelled(_that.cancelledAt,_that.reason);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  started,TResult? Function()?  streaming,TResult? Function()?  awaitingApproval,TResult? Function()?  approved,TResult? Function( String streamedOutput)?  running,TResult? Function( PlatformInt64 completedAt,  BridgeThreadToolOutput output)?  succeeded,TResult? Function( PlatformInt64 failedAt,  BridgeThreadToolFailure failure,  BridgeThreadToolOutput? output)?  failed,TResult? Function( PlatformInt64 deniedAt,  String reason)?  denied,TResult? Function( PlatformInt64 cancelledAt,  String reason)?  cancelled,}) {final _that = this;
switch (_that) {
case BridgeThreadToolState_Started() when started != null:
return started();case BridgeThreadToolState_Streaming() when streaming != null:
return streaming();case BridgeThreadToolState_AwaitingApproval() when awaitingApproval != null:
return awaitingApproval();case BridgeThreadToolState_Approved() when approved != null:
return approved();case BridgeThreadToolState_Running() when running != null:
return running(_that.streamedOutput);case BridgeThreadToolState_Succeeded() when succeeded != null:
return succeeded(_that.completedAt,_that.output);case BridgeThreadToolState_Failed() when failed != null:
return failed(_that.failedAt,_that.failure,_that.output);case BridgeThreadToolState_Denied() when denied != null:
return denied(_that.deniedAt,_that.reason);case BridgeThreadToolState_Cancelled() when cancelled != null:
return cancelled(_that.cancelledAt,_that.reason);case _:
  return null;

}
}

}

/// @nodoc


class BridgeThreadToolState_Started extends BridgeThreadToolState {
  const BridgeThreadToolState_Started(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadToolState_Started);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadToolState.started()';
}


}




/// @nodoc


class BridgeThreadToolState_Streaming extends BridgeThreadToolState {
  const BridgeThreadToolState_Streaming(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadToolState_Streaming);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadToolState.streaming()';
}


}




/// @nodoc


class BridgeThreadToolState_AwaitingApproval extends BridgeThreadToolState {
  const BridgeThreadToolState_AwaitingApproval(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadToolState_AwaitingApproval);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadToolState.awaitingApproval()';
}


}




/// @nodoc


class BridgeThreadToolState_Approved extends BridgeThreadToolState {
  const BridgeThreadToolState_Approved(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadToolState_Approved);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadToolState.approved()';
}


}




/// @nodoc


class BridgeThreadToolState_Running extends BridgeThreadToolState {
  const BridgeThreadToolState_Running({required this.streamedOutput}): super._();


 final  String streamedOutput;

/// Create a copy of BridgeThreadToolState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadToolState_RunningCopyWith<BridgeThreadToolState_Running> get copyWith => _$BridgeThreadToolState_RunningCopyWithImpl<BridgeThreadToolState_Running>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadToolState_Running&&(identical(other.streamedOutput, streamedOutput) || other.streamedOutput == streamedOutput));
}


@override
int get hashCode => Object.hash(runtimeType,streamedOutput);

@override
String toString() {
  return 'BridgeThreadToolState.running(streamedOutput: $streamedOutput)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadToolState_RunningCopyWith<$Res> implements $BridgeThreadToolStateCopyWith<$Res> {
  factory $BridgeThreadToolState_RunningCopyWith(BridgeThreadToolState_Running value, $Res Function(BridgeThreadToolState_Running) _then) = _$BridgeThreadToolState_RunningCopyWithImpl;
@useResult
$Res call({
 String streamedOutput
});




}
/// @nodoc
class _$BridgeThreadToolState_RunningCopyWithImpl<$Res>
    implements $BridgeThreadToolState_RunningCopyWith<$Res> {
  _$BridgeThreadToolState_RunningCopyWithImpl(this._self, this._then);

  final BridgeThreadToolState_Running _self;
  final $Res Function(BridgeThreadToolState_Running) _then;

/// Create a copy of BridgeThreadToolState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? streamedOutput = null,}) {
  return _then(BridgeThreadToolState_Running(
streamedOutput: null == streamedOutput ? _self.streamedOutput : streamedOutput // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadToolState_Succeeded extends BridgeThreadToolState {
  const BridgeThreadToolState_Succeeded({required this.completedAt, required this.output}): super._();


 final  PlatformInt64 completedAt;
 final  BridgeThreadToolOutput output;

/// Create a copy of BridgeThreadToolState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadToolState_SucceededCopyWith<BridgeThreadToolState_Succeeded> get copyWith => _$BridgeThreadToolState_SucceededCopyWithImpl<BridgeThreadToolState_Succeeded>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadToolState_Succeeded&&(identical(other.completedAt, completedAt) || other.completedAt == completedAt)&&(identical(other.output, output) || other.output == output));
}


@override
int get hashCode => Object.hash(runtimeType,completedAt,output);

@override
String toString() {
  return 'BridgeThreadToolState.succeeded(completedAt: $completedAt, output: $output)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadToolState_SucceededCopyWith<$Res> implements $BridgeThreadToolStateCopyWith<$Res> {
  factory $BridgeThreadToolState_SucceededCopyWith(BridgeThreadToolState_Succeeded value, $Res Function(BridgeThreadToolState_Succeeded) _then) = _$BridgeThreadToolState_SucceededCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 completedAt, BridgeThreadToolOutput output
});




}
/// @nodoc
class _$BridgeThreadToolState_SucceededCopyWithImpl<$Res>
    implements $BridgeThreadToolState_SucceededCopyWith<$Res> {
  _$BridgeThreadToolState_SucceededCopyWithImpl(this._self, this._then);

  final BridgeThreadToolState_Succeeded _self;
  final $Res Function(BridgeThreadToolState_Succeeded) _then;

/// Create a copy of BridgeThreadToolState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? completedAt = null,Object? output = null,}) {
  return _then(BridgeThreadToolState_Succeeded(
completedAt: null == completedAt ? _self.completedAt : completedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,output: null == output ? _self.output : output // ignore: cast_nullable_to_non_nullable
as BridgeThreadToolOutput,
  ));
}


}

/// @nodoc


class BridgeThreadToolState_Failed extends BridgeThreadToolState {
  const BridgeThreadToolState_Failed({required this.failedAt, required this.failure, this.output}): super._();


 final  PlatformInt64 failedAt;
 final  BridgeThreadToolFailure failure;
 final  BridgeThreadToolOutput? output;

/// Create a copy of BridgeThreadToolState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadToolState_FailedCopyWith<BridgeThreadToolState_Failed> get copyWith => _$BridgeThreadToolState_FailedCopyWithImpl<BridgeThreadToolState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadToolState_Failed&&(identical(other.failedAt, failedAt) || other.failedAt == failedAt)&&(identical(other.failure, failure) || other.failure == failure)&&(identical(other.output, output) || other.output == output));
}


@override
int get hashCode => Object.hash(runtimeType,failedAt,failure,output);

@override
String toString() {
  return 'BridgeThreadToolState.failed(failedAt: $failedAt, failure: $failure, output: $output)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadToolState_FailedCopyWith<$Res> implements $BridgeThreadToolStateCopyWith<$Res> {
  factory $BridgeThreadToolState_FailedCopyWith(BridgeThreadToolState_Failed value, $Res Function(BridgeThreadToolState_Failed) _then) = _$BridgeThreadToolState_FailedCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 failedAt, BridgeThreadToolFailure failure, BridgeThreadToolOutput? output
});




}
/// @nodoc
class _$BridgeThreadToolState_FailedCopyWithImpl<$Res>
    implements $BridgeThreadToolState_FailedCopyWith<$Res> {
  _$BridgeThreadToolState_FailedCopyWithImpl(this._self, this._then);

  final BridgeThreadToolState_Failed _self;
  final $Res Function(BridgeThreadToolState_Failed) _then;

/// Create a copy of BridgeThreadToolState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? failedAt = null,Object? failure = null,Object? output = freezed,}) {
  return _then(BridgeThreadToolState_Failed(
failedAt: null == failedAt ? _self.failedAt : failedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,failure: null == failure ? _self.failure : failure // ignore: cast_nullable_to_non_nullable
as BridgeThreadToolFailure,output: freezed == output ? _self.output : output // ignore: cast_nullable_to_non_nullable
as BridgeThreadToolOutput?,
  ));
}


}

/// @nodoc


class BridgeThreadToolState_Denied extends BridgeThreadToolState {
  const BridgeThreadToolState_Denied({required this.deniedAt, required this.reason}): super._();


 final  PlatformInt64 deniedAt;
 final  String reason;

/// Create a copy of BridgeThreadToolState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadToolState_DeniedCopyWith<BridgeThreadToolState_Denied> get copyWith => _$BridgeThreadToolState_DeniedCopyWithImpl<BridgeThreadToolState_Denied>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadToolState_Denied&&(identical(other.deniedAt, deniedAt) || other.deniedAt == deniedAt)&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,deniedAt,reason);

@override
String toString() {
  return 'BridgeThreadToolState.denied(deniedAt: $deniedAt, reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadToolState_DeniedCopyWith<$Res> implements $BridgeThreadToolStateCopyWith<$Res> {
  factory $BridgeThreadToolState_DeniedCopyWith(BridgeThreadToolState_Denied value, $Res Function(BridgeThreadToolState_Denied) _then) = _$BridgeThreadToolState_DeniedCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 deniedAt, String reason
});




}
/// @nodoc
class _$BridgeThreadToolState_DeniedCopyWithImpl<$Res>
    implements $BridgeThreadToolState_DeniedCopyWith<$Res> {
  _$BridgeThreadToolState_DeniedCopyWithImpl(this._self, this._then);

  final BridgeThreadToolState_Denied _self;
  final $Res Function(BridgeThreadToolState_Denied) _then;

/// Create a copy of BridgeThreadToolState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? deniedAt = null,Object? reason = null,}) {
  return _then(BridgeThreadToolState_Denied(
deniedAt: null == deniedAt ? _self.deniedAt : deniedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadToolState_Cancelled extends BridgeThreadToolState {
  const BridgeThreadToolState_Cancelled({required this.cancelledAt, required this.reason}): super._();


 final  PlatformInt64 cancelledAt;
 final  String reason;

/// Create a copy of BridgeThreadToolState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadToolState_CancelledCopyWith<BridgeThreadToolState_Cancelled> get copyWith => _$BridgeThreadToolState_CancelledCopyWithImpl<BridgeThreadToolState_Cancelled>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadToolState_Cancelled&&(identical(other.cancelledAt, cancelledAt) || other.cancelledAt == cancelledAt)&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,cancelledAt,reason);

@override
String toString() {
  return 'BridgeThreadToolState.cancelled(cancelledAt: $cancelledAt, reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadToolState_CancelledCopyWith<$Res> implements $BridgeThreadToolStateCopyWith<$Res> {
  factory $BridgeThreadToolState_CancelledCopyWith(BridgeThreadToolState_Cancelled value, $Res Function(BridgeThreadToolState_Cancelled) _then) = _$BridgeThreadToolState_CancelledCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 cancelledAt, String reason
});




}
/// @nodoc
class _$BridgeThreadToolState_CancelledCopyWithImpl<$Res>
    implements $BridgeThreadToolState_CancelledCopyWith<$Res> {
  _$BridgeThreadToolState_CancelledCopyWithImpl(this._self, this._then);

  final BridgeThreadToolState_Cancelled _self;
  final $Res Function(BridgeThreadToolState_Cancelled) _then;

/// Create a copy of BridgeThreadToolState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? cancelledAt = null,Object? reason = null,}) {
  return _then(BridgeThreadToolState_Cancelled(
cancelledAt: null == cancelledAt ? _self.cancelledAt : cancelledAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

// dart format on

// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'attachment.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeAttachmentAdmissionContext {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAttachmentAdmissionContext);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeAttachmentAdmissionContext()';
}


}

/// @nodoc
class $BridgeAttachmentAdmissionContextCopyWith<$Res>  {
$BridgeAttachmentAdmissionContextCopyWith(BridgeAttachmentAdmissionContext _, $Res Function(BridgeAttachmentAdmissionContext) __);
}


/// Adds pattern-matching-related methods to [BridgeAttachmentAdmissionContext].
extension BridgeAttachmentAdmissionContextPatterns on BridgeAttachmentAdmissionContext {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeAttachmentAdmissionContext_ExistingThread value)?  existingThread,TResult Function( BridgeAttachmentAdmissionContext_NewThread value)?  newThread,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeAttachmentAdmissionContext_ExistingThread() when existingThread != null:
return existingThread(_that);case BridgeAttachmentAdmissionContext_NewThread() when newThread != null:
return newThread(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeAttachmentAdmissionContext_ExistingThread value)  existingThread,required TResult Function( BridgeAttachmentAdmissionContext_NewThread value)  newThread,}){
final _that = this;
switch (_that) {
case BridgeAttachmentAdmissionContext_ExistingThread():
return existingThread(_that);case BridgeAttachmentAdmissionContext_NewThread():
return newThread(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeAttachmentAdmissionContext_ExistingThread value)?  existingThread,TResult? Function( BridgeAttachmentAdmissionContext_NewThread value)?  newThread,}){
final _that = this;
switch (_that) {
case BridgeAttachmentAdmissionContext_ExistingThread() when existingThread != null:
return existingThread(_that);case BridgeAttachmentAdmissionContext_NewThread() when newThread != null:
return newThread(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String threadId)?  existingThread,TResult Function( BridgeThreadMode mode)?  newThread,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeAttachmentAdmissionContext_ExistingThread() when existingThread != null:
return existingThread(_that.threadId);case BridgeAttachmentAdmissionContext_NewThread() when newThread != null:
return newThread(_that.mode);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String threadId)  existingThread,required TResult Function( BridgeThreadMode mode)  newThread,}) {final _that = this;
switch (_that) {
case BridgeAttachmentAdmissionContext_ExistingThread():
return existingThread(_that.threadId);case BridgeAttachmentAdmissionContext_NewThread():
return newThread(_that.mode);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String threadId)?  existingThread,TResult? Function( BridgeThreadMode mode)?  newThread,}) {final _that = this;
switch (_that) {
case BridgeAttachmentAdmissionContext_ExistingThread() when existingThread != null:
return existingThread(_that.threadId);case BridgeAttachmentAdmissionContext_NewThread() when newThread != null:
return newThread(_that.mode);case _:
  return null;

}
}

}

/// @nodoc


class BridgeAttachmentAdmissionContext_ExistingThread extends BridgeAttachmentAdmissionContext {
  const BridgeAttachmentAdmissionContext_ExistingThread({required this.threadId}): super._();


 final  String threadId;

/// Create a copy of BridgeAttachmentAdmissionContext
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAttachmentAdmissionContext_ExistingThreadCopyWith<BridgeAttachmentAdmissionContext_ExistingThread> get copyWith => _$BridgeAttachmentAdmissionContext_ExistingThreadCopyWithImpl<BridgeAttachmentAdmissionContext_ExistingThread>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAttachmentAdmissionContext_ExistingThread&&(identical(other.threadId, threadId) || other.threadId == threadId));
}


@override
int get hashCode => Object.hash(runtimeType,threadId);

@override
String toString() {
  return 'BridgeAttachmentAdmissionContext.existingThread(threadId: $threadId)';
}


}

/// @nodoc
abstract mixin class $BridgeAttachmentAdmissionContext_ExistingThreadCopyWith<$Res> implements $BridgeAttachmentAdmissionContextCopyWith<$Res> {
  factory $BridgeAttachmentAdmissionContext_ExistingThreadCopyWith(BridgeAttachmentAdmissionContext_ExistingThread value, $Res Function(BridgeAttachmentAdmissionContext_ExistingThread) _then) = _$BridgeAttachmentAdmissionContext_ExistingThreadCopyWithImpl;
@useResult
$Res call({
 String threadId
});




}
/// @nodoc
class _$BridgeAttachmentAdmissionContext_ExistingThreadCopyWithImpl<$Res>
    implements $BridgeAttachmentAdmissionContext_ExistingThreadCopyWith<$Res> {
  _$BridgeAttachmentAdmissionContext_ExistingThreadCopyWithImpl(this._self, this._then);

  final BridgeAttachmentAdmissionContext_ExistingThread _self;
  final $Res Function(BridgeAttachmentAdmissionContext_ExistingThread) _then;

/// Create a copy of BridgeAttachmentAdmissionContext
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? threadId = null,}) {
  return _then(BridgeAttachmentAdmissionContext_ExistingThread(
threadId: null == threadId ? _self.threadId : threadId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeAttachmentAdmissionContext_NewThread extends BridgeAttachmentAdmissionContext {
  const BridgeAttachmentAdmissionContext_NewThread({required this.mode}): super._();


 final  BridgeThreadMode mode;

/// Create a copy of BridgeAttachmentAdmissionContext
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAttachmentAdmissionContext_NewThreadCopyWith<BridgeAttachmentAdmissionContext_NewThread> get copyWith => _$BridgeAttachmentAdmissionContext_NewThreadCopyWithImpl<BridgeAttachmentAdmissionContext_NewThread>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAttachmentAdmissionContext_NewThread&&(identical(other.mode, mode) || other.mode == mode));
}


@override
int get hashCode => Object.hash(runtimeType,mode);

@override
String toString() {
  return 'BridgeAttachmentAdmissionContext.newThread(mode: $mode)';
}


}

/// @nodoc
abstract mixin class $BridgeAttachmentAdmissionContext_NewThreadCopyWith<$Res> implements $BridgeAttachmentAdmissionContextCopyWith<$Res> {
  factory $BridgeAttachmentAdmissionContext_NewThreadCopyWith(BridgeAttachmentAdmissionContext_NewThread value, $Res Function(BridgeAttachmentAdmissionContext_NewThread) _then) = _$BridgeAttachmentAdmissionContext_NewThreadCopyWithImpl;
@useResult
$Res call({
 BridgeThreadMode mode
});




}
/// @nodoc
class _$BridgeAttachmentAdmissionContext_NewThreadCopyWithImpl<$Res>
    implements $BridgeAttachmentAdmissionContext_NewThreadCopyWith<$Res> {
  _$BridgeAttachmentAdmissionContext_NewThreadCopyWithImpl(this._self, this._then);

  final BridgeAttachmentAdmissionContext_NewThread _self;
  final $Res Function(BridgeAttachmentAdmissionContext_NewThread) _then;

/// Create a copy of BridgeAttachmentAdmissionContext
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? mode = null,}) {
  return _then(BridgeAttachmentAdmissionContext_NewThread(
mode: null == mode ? _self.mode : mode // ignore: cast_nullable_to_non_nullable
as BridgeThreadMode,
  ));
}


}

/// @nodoc
mixin _$BridgeAttachmentDraftSource {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAttachmentDraftSource);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeAttachmentDraftSource()';
}


}

/// @nodoc
class $BridgeAttachmentDraftSourceCopyWith<$Res>  {
$BridgeAttachmentDraftSourceCopyWith(BridgeAttachmentDraftSource _, $Res Function(BridgeAttachmentDraftSource) __);
}


/// Adds pattern-matching-related methods to [BridgeAttachmentDraftSource].
extension BridgeAttachmentDraftSourcePatterns on BridgeAttachmentDraftSource {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeAttachmentDraftSource_LocalFile value)?  localFile,TResult Function( BridgeAttachmentDraftSource_RemoteUrl value)?  remoteUrl,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeAttachmentDraftSource_LocalFile() when localFile != null:
return localFile(_that);case BridgeAttachmentDraftSource_RemoteUrl() when remoteUrl != null:
return remoteUrl(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeAttachmentDraftSource_LocalFile value)  localFile,required TResult Function( BridgeAttachmentDraftSource_RemoteUrl value)  remoteUrl,}){
final _that = this;
switch (_that) {
case BridgeAttachmentDraftSource_LocalFile():
return localFile(_that);case BridgeAttachmentDraftSource_RemoteUrl():
return remoteUrl(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeAttachmentDraftSource_LocalFile value)?  localFile,TResult? Function( BridgeAttachmentDraftSource_RemoteUrl value)?  remoteUrl,}){
final _that = this;
switch (_that) {
case BridgeAttachmentDraftSource_LocalFile() when localFile != null:
return localFile(_that);case BridgeAttachmentDraftSource_RemoteUrl() when remoteUrl != null:
return remoteUrl(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String path)?  localFile,TResult Function( String url,  String? filename)?  remoteUrl,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeAttachmentDraftSource_LocalFile() when localFile != null:
return localFile(_that.path);case BridgeAttachmentDraftSource_RemoteUrl() when remoteUrl != null:
return remoteUrl(_that.url,_that.filename);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String path)  localFile,required TResult Function( String url,  String? filename)  remoteUrl,}) {final _that = this;
switch (_that) {
case BridgeAttachmentDraftSource_LocalFile():
return localFile(_that.path);case BridgeAttachmentDraftSource_RemoteUrl():
return remoteUrl(_that.url,_that.filename);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String path)?  localFile,TResult? Function( String url,  String? filename)?  remoteUrl,}) {final _that = this;
switch (_that) {
case BridgeAttachmentDraftSource_LocalFile() when localFile != null:
return localFile(_that.path);case BridgeAttachmentDraftSource_RemoteUrl() when remoteUrl != null:
return remoteUrl(_that.url,_that.filename);case _:
  return null;

}
}

}

/// @nodoc


class BridgeAttachmentDraftSource_LocalFile extends BridgeAttachmentDraftSource {
  const BridgeAttachmentDraftSource_LocalFile({required this.path}): super._();


 final  String path;

/// Create a copy of BridgeAttachmentDraftSource
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAttachmentDraftSource_LocalFileCopyWith<BridgeAttachmentDraftSource_LocalFile> get copyWith => _$BridgeAttachmentDraftSource_LocalFileCopyWithImpl<BridgeAttachmentDraftSource_LocalFile>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAttachmentDraftSource_LocalFile&&(identical(other.path, path) || other.path == path));
}


@override
int get hashCode => Object.hash(runtimeType,path);

@override
String toString() {
  return 'BridgeAttachmentDraftSource.localFile(path: $path)';
}


}

/// @nodoc
abstract mixin class $BridgeAttachmentDraftSource_LocalFileCopyWith<$Res> implements $BridgeAttachmentDraftSourceCopyWith<$Res> {
  factory $BridgeAttachmentDraftSource_LocalFileCopyWith(BridgeAttachmentDraftSource_LocalFile value, $Res Function(BridgeAttachmentDraftSource_LocalFile) _then) = _$BridgeAttachmentDraftSource_LocalFileCopyWithImpl;
@useResult
$Res call({
 String path
});




}
/// @nodoc
class _$BridgeAttachmentDraftSource_LocalFileCopyWithImpl<$Res>
    implements $BridgeAttachmentDraftSource_LocalFileCopyWith<$Res> {
  _$BridgeAttachmentDraftSource_LocalFileCopyWithImpl(this._self, this._then);

  final BridgeAttachmentDraftSource_LocalFile _self;
  final $Res Function(BridgeAttachmentDraftSource_LocalFile) _then;

/// Create a copy of BridgeAttachmentDraftSource
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? path = null,}) {
  return _then(BridgeAttachmentDraftSource_LocalFile(
path: null == path ? _self.path : path // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeAttachmentDraftSource_RemoteUrl extends BridgeAttachmentDraftSource {
  const BridgeAttachmentDraftSource_RemoteUrl({required this.url, this.filename}): super._();


 final  String url;
 final  String? filename;

/// Create a copy of BridgeAttachmentDraftSource
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAttachmentDraftSource_RemoteUrlCopyWith<BridgeAttachmentDraftSource_RemoteUrl> get copyWith => _$BridgeAttachmentDraftSource_RemoteUrlCopyWithImpl<BridgeAttachmentDraftSource_RemoteUrl>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAttachmentDraftSource_RemoteUrl&&(identical(other.url, url) || other.url == url)&&(identical(other.filename, filename) || other.filename == filename));
}


@override
int get hashCode => Object.hash(runtimeType,url,filename);

@override
String toString() {
  return 'BridgeAttachmentDraftSource.remoteUrl(url: $url, filename: $filename)';
}


}

/// @nodoc
abstract mixin class $BridgeAttachmentDraftSource_RemoteUrlCopyWith<$Res> implements $BridgeAttachmentDraftSourceCopyWith<$Res> {
  factory $BridgeAttachmentDraftSource_RemoteUrlCopyWith(BridgeAttachmentDraftSource_RemoteUrl value, $Res Function(BridgeAttachmentDraftSource_RemoteUrl) _then) = _$BridgeAttachmentDraftSource_RemoteUrlCopyWithImpl;
@useResult
$Res call({
 String url, String? filename
});




}
/// @nodoc
class _$BridgeAttachmentDraftSource_RemoteUrlCopyWithImpl<$Res>
    implements $BridgeAttachmentDraftSource_RemoteUrlCopyWith<$Res> {
  _$BridgeAttachmentDraftSource_RemoteUrlCopyWithImpl(this._self, this._then);

  final BridgeAttachmentDraftSource_RemoteUrl _self;
  final $Res Function(BridgeAttachmentDraftSource_RemoteUrl) _then;

/// Create a copy of BridgeAttachmentDraftSource
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? url = null,Object? filename = freezed,}) {
  return _then(BridgeAttachmentDraftSource_RemoteUrl(
url: null == url ? _self.url : url // ignore: cast_nullable_to_non_nullable
as String,filename: freezed == filename ? _self.filename : filename // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

// dart format on

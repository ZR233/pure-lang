// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'response.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeAgentDirectoryState {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentDirectoryState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeAgentDirectoryState()';
}


}

/// @nodoc
class $BridgeAgentDirectoryStateCopyWith<$Res>  {
$BridgeAgentDirectoryStateCopyWith(BridgeAgentDirectoryState _, $Res Function(BridgeAgentDirectoryState) __);
}


/// Adds pattern-matching-related methods to [BridgeAgentDirectoryState].
extension BridgeAgentDirectoryStatePatterns on BridgeAgentDirectoryState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeAgentDirectoryState_Uninitialized value)?  uninitialized,TResult Function( BridgeAgentDirectoryState_Loading value)?  loading,TResult Function( BridgeAgentDirectoryState_Ready value)?  ready,TResult Function( BridgeAgentDirectoryState_Refreshing value)?  refreshing,TResult Function( BridgeAgentDirectoryState_Stale value)?  stale,TResult Function( BridgeAgentDirectoryState_Degraded value)?  degraded,TResult Function( BridgeAgentDirectoryState_Failed value)?  failed,TResult Function( BridgeAgentDirectoryState_Stopped value)?  stopped,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeAgentDirectoryState_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeAgentDirectoryState_Loading() when loading != null:
return loading(_that);case BridgeAgentDirectoryState_Ready() when ready != null:
return ready(_that);case BridgeAgentDirectoryState_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeAgentDirectoryState_Stale() when stale != null:
return stale(_that);case BridgeAgentDirectoryState_Degraded() when degraded != null:
return degraded(_that);case BridgeAgentDirectoryState_Failed() when failed != null:
return failed(_that);case BridgeAgentDirectoryState_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeAgentDirectoryState_Uninitialized value)  uninitialized,required TResult Function( BridgeAgentDirectoryState_Loading value)  loading,required TResult Function( BridgeAgentDirectoryState_Ready value)  ready,required TResult Function( BridgeAgentDirectoryState_Refreshing value)  refreshing,required TResult Function( BridgeAgentDirectoryState_Stale value)  stale,required TResult Function( BridgeAgentDirectoryState_Degraded value)  degraded,required TResult Function( BridgeAgentDirectoryState_Failed value)  failed,required TResult Function( BridgeAgentDirectoryState_Stopped value)  stopped,}){
final _that = this;
switch (_that) {
case BridgeAgentDirectoryState_Uninitialized():
return uninitialized(_that);case BridgeAgentDirectoryState_Loading():
return loading(_that);case BridgeAgentDirectoryState_Ready():
return ready(_that);case BridgeAgentDirectoryState_Refreshing():
return refreshing(_that);case BridgeAgentDirectoryState_Stale():
return stale(_that);case BridgeAgentDirectoryState_Degraded():
return degraded(_that);case BridgeAgentDirectoryState_Failed():
return failed(_that);case BridgeAgentDirectoryState_Stopped():
return stopped(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeAgentDirectoryState_Uninitialized value)?  uninitialized,TResult? Function( BridgeAgentDirectoryState_Loading value)?  loading,TResult? Function( BridgeAgentDirectoryState_Ready value)?  ready,TResult? Function( BridgeAgentDirectoryState_Refreshing value)?  refreshing,TResult? Function( BridgeAgentDirectoryState_Stale value)?  stale,TResult? Function( BridgeAgentDirectoryState_Degraded value)?  degraded,TResult? Function( BridgeAgentDirectoryState_Failed value)?  failed,TResult? Function( BridgeAgentDirectoryState_Stopped value)?  stopped,}){
final _that = this;
switch (_that) {
case BridgeAgentDirectoryState_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeAgentDirectoryState_Loading() when loading != null:
return loading(_that);case BridgeAgentDirectoryState_Ready() when ready != null:
return ready(_that);case BridgeAgentDirectoryState_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeAgentDirectoryState_Stale() when stale != null:
return stale(_that);case BridgeAgentDirectoryState_Degraded() when degraded != null:
return degraded(_that);case BridgeAgentDirectoryState_Failed() when failed != null:
return failed(_that);case BridgeAgentDirectoryState_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeUninitializedResource field0)?  uninitialized,TResult Function( BridgeLoadingResource field0)?  loading,TResult Function( BridgeReadyResource resource,  BridgeAgentDirectoryData value)?  ready,TResult Function( BridgeRefreshingResource resource,  BridgeAgentDirectoryData value)?  refreshing,TResult Function( BridgeStaleResource resource,  BridgeAgentDirectoryData value)?  stale,TResult Function( BridgeDegradedResource resource,  BridgeAgentDirectoryData value)?  degraded,TResult Function( BridgeFailedResource field0)?  failed,TResult Function( BridgeStoppedResource field0)?  stopped,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeAgentDirectoryState_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeAgentDirectoryState_Loading() when loading != null:
return loading(_that.field0);case BridgeAgentDirectoryState_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeAgentDirectoryState_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeAgentDirectoryState_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeAgentDirectoryState_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeAgentDirectoryState_Failed() when failed != null:
return failed(_that.field0);case BridgeAgentDirectoryState_Stopped() when stopped != null:
return stopped(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeUninitializedResource field0)  uninitialized,required TResult Function( BridgeLoadingResource field0)  loading,required TResult Function( BridgeReadyResource resource,  BridgeAgentDirectoryData value)  ready,required TResult Function( BridgeRefreshingResource resource,  BridgeAgentDirectoryData value)  refreshing,required TResult Function( BridgeStaleResource resource,  BridgeAgentDirectoryData value)  stale,required TResult Function( BridgeDegradedResource resource,  BridgeAgentDirectoryData value)  degraded,required TResult Function( BridgeFailedResource field0)  failed,required TResult Function( BridgeStoppedResource field0)  stopped,}) {final _that = this;
switch (_that) {
case BridgeAgentDirectoryState_Uninitialized():
return uninitialized(_that.field0);case BridgeAgentDirectoryState_Loading():
return loading(_that.field0);case BridgeAgentDirectoryState_Ready():
return ready(_that.resource,_that.value);case BridgeAgentDirectoryState_Refreshing():
return refreshing(_that.resource,_that.value);case BridgeAgentDirectoryState_Stale():
return stale(_that.resource,_that.value);case BridgeAgentDirectoryState_Degraded():
return degraded(_that.resource,_that.value);case BridgeAgentDirectoryState_Failed():
return failed(_that.field0);case BridgeAgentDirectoryState_Stopped():
return stopped(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeUninitializedResource field0)?  uninitialized,TResult? Function( BridgeLoadingResource field0)?  loading,TResult? Function( BridgeReadyResource resource,  BridgeAgentDirectoryData value)?  ready,TResult? Function( BridgeRefreshingResource resource,  BridgeAgentDirectoryData value)?  refreshing,TResult? Function( BridgeStaleResource resource,  BridgeAgentDirectoryData value)?  stale,TResult? Function( BridgeDegradedResource resource,  BridgeAgentDirectoryData value)?  degraded,TResult? Function( BridgeFailedResource field0)?  failed,TResult? Function( BridgeStoppedResource field0)?  stopped,}) {final _that = this;
switch (_that) {
case BridgeAgentDirectoryState_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeAgentDirectoryState_Loading() when loading != null:
return loading(_that.field0);case BridgeAgentDirectoryState_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeAgentDirectoryState_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeAgentDirectoryState_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeAgentDirectoryState_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeAgentDirectoryState_Failed() when failed != null:
return failed(_that.field0);case BridgeAgentDirectoryState_Stopped() when stopped != null:
return stopped(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeAgentDirectoryState_Uninitialized extends BridgeAgentDirectoryState {
  const BridgeAgentDirectoryState_Uninitialized(this.field0): super._();


 final  BridgeUninitializedResource field0;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentDirectoryState_UninitializedCopyWith<BridgeAgentDirectoryState_Uninitialized> get copyWith => _$BridgeAgentDirectoryState_UninitializedCopyWithImpl<BridgeAgentDirectoryState_Uninitialized>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentDirectoryState_Uninitialized&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeAgentDirectoryState.uninitialized(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentDirectoryState_UninitializedCopyWith<$Res> implements $BridgeAgentDirectoryStateCopyWith<$Res> {
  factory $BridgeAgentDirectoryState_UninitializedCopyWith(BridgeAgentDirectoryState_Uninitialized value, $Res Function(BridgeAgentDirectoryState_Uninitialized) _then) = _$BridgeAgentDirectoryState_UninitializedCopyWithImpl;
@useResult
$Res call({
 BridgeUninitializedResource field0
});




}
/// @nodoc
class _$BridgeAgentDirectoryState_UninitializedCopyWithImpl<$Res>
    implements $BridgeAgentDirectoryState_UninitializedCopyWith<$Res> {
  _$BridgeAgentDirectoryState_UninitializedCopyWithImpl(this._self, this._then);

  final BridgeAgentDirectoryState_Uninitialized _self;
  final $Res Function(BridgeAgentDirectoryState_Uninitialized) _then;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentDirectoryState_Uninitialized(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeUninitializedResource,
  ));
}


}

/// @nodoc


class BridgeAgentDirectoryState_Loading extends BridgeAgentDirectoryState {
  const BridgeAgentDirectoryState_Loading(this.field0): super._();


 final  BridgeLoadingResource field0;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentDirectoryState_LoadingCopyWith<BridgeAgentDirectoryState_Loading> get copyWith => _$BridgeAgentDirectoryState_LoadingCopyWithImpl<BridgeAgentDirectoryState_Loading>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentDirectoryState_Loading&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeAgentDirectoryState.loading(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentDirectoryState_LoadingCopyWith<$Res> implements $BridgeAgentDirectoryStateCopyWith<$Res> {
  factory $BridgeAgentDirectoryState_LoadingCopyWith(BridgeAgentDirectoryState_Loading value, $Res Function(BridgeAgentDirectoryState_Loading) _then) = _$BridgeAgentDirectoryState_LoadingCopyWithImpl;
@useResult
$Res call({
 BridgeLoadingResource field0
});




}
/// @nodoc
class _$BridgeAgentDirectoryState_LoadingCopyWithImpl<$Res>
    implements $BridgeAgentDirectoryState_LoadingCopyWith<$Res> {
  _$BridgeAgentDirectoryState_LoadingCopyWithImpl(this._self, this._then);

  final BridgeAgentDirectoryState_Loading _self;
  final $Res Function(BridgeAgentDirectoryState_Loading) _then;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentDirectoryState_Loading(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeLoadingResource,
  ));
}


}

/// @nodoc


class BridgeAgentDirectoryState_Ready extends BridgeAgentDirectoryState {
  const BridgeAgentDirectoryState_Ready({required this.resource, required this.value}): super._();


 final  BridgeReadyResource resource;
 final  BridgeAgentDirectoryData value;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentDirectoryState_ReadyCopyWith<BridgeAgentDirectoryState_Ready> get copyWith => _$BridgeAgentDirectoryState_ReadyCopyWithImpl<BridgeAgentDirectoryState_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentDirectoryState_Ready&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeAgentDirectoryState.ready(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentDirectoryState_ReadyCopyWith<$Res> implements $BridgeAgentDirectoryStateCopyWith<$Res> {
  factory $BridgeAgentDirectoryState_ReadyCopyWith(BridgeAgentDirectoryState_Ready value, $Res Function(BridgeAgentDirectoryState_Ready) _then) = _$BridgeAgentDirectoryState_ReadyCopyWithImpl;
@useResult
$Res call({
 BridgeReadyResource resource, BridgeAgentDirectoryData value
});




}
/// @nodoc
class _$BridgeAgentDirectoryState_ReadyCopyWithImpl<$Res>
    implements $BridgeAgentDirectoryState_ReadyCopyWith<$Res> {
  _$BridgeAgentDirectoryState_ReadyCopyWithImpl(this._self, this._then);

  final BridgeAgentDirectoryState_Ready _self;
  final $Res Function(BridgeAgentDirectoryState_Ready) _then;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeAgentDirectoryState_Ready(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeReadyResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeAgentDirectoryData,
  ));
}


}

/// @nodoc


class BridgeAgentDirectoryState_Refreshing extends BridgeAgentDirectoryState {
  const BridgeAgentDirectoryState_Refreshing({required this.resource, required this.value}): super._();


 final  BridgeRefreshingResource resource;
 final  BridgeAgentDirectoryData value;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentDirectoryState_RefreshingCopyWith<BridgeAgentDirectoryState_Refreshing> get copyWith => _$BridgeAgentDirectoryState_RefreshingCopyWithImpl<BridgeAgentDirectoryState_Refreshing>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentDirectoryState_Refreshing&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeAgentDirectoryState.refreshing(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentDirectoryState_RefreshingCopyWith<$Res> implements $BridgeAgentDirectoryStateCopyWith<$Res> {
  factory $BridgeAgentDirectoryState_RefreshingCopyWith(BridgeAgentDirectoryState_Refreshing value, $Res Function(BridgeAgentDirectoryState_Refreshing) _then) = _$BridgeAgentDirectoryState_RefreshingCopyWithImpl;
@useResult
$Res call({
 BridgeRefreshingResource resource, BridgeAgentDirectoryData value
});




}
/// @nodoc
class _$BridgeAgentDirectoryState_RefreshingCopyWithImpl<$Res>
    implements $BridgeAgentDirectoryState_RefreshingCopyWith<$Res> {
  _$BridgeAgentDirectoryState_RefreshingCopyWithImpl(this._self, this._then);

  final BridgeAgentDirectoryState_Refreshing _self;
  final $Res Function(BridgeAgentDirectoryState_Refreshing) _then;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeAgentDirectoryState_Refreshing(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeRefreshingResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeAgentDirectoryData,
  ));
}


}

/// @nodoc


class BridgeAgentDirectoryState_Stale extends BridgeAgentDirectoryState {
  const BridgeAgentDirectoryState_Stale({required this.resource, required this.value}): super._();


 final  BridgeStaleResource resource;
 final  BridgeAgentDirectoryData value;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentDirectoryState_StaleCopyWith<BridgeAgentDirectoryState_Stale> get copyWith => _$BridgeAgentDirectoryState_StaleCopyWithImpl<BridgeAgentDirectoryState_Stale>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentDirectoryState_Stale&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeAgentDirectoryState.stale(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentDirectoryState_StaleCopyWith<$Res> implements $BridgeAgentDirectoryStateCopyWith<$Res> {
  factory $BridgeAgentDirectoryState_StaleCopyWith(BridgeAgentDirectoryState_Stale value, $Res Function(BridgeAgentDirectoryState_Stale) _then) = _$BridgeAgentDirectoryState_StaleCopyWithImpl;
@useResult
$Res call({
 BridgeStaleResource resource, BridgeAgentDirectoryData value
});




}
/// @nodoc
class _$BridgeAgentDirectoryState_StaleCopyWithImpl<$Res>
    implements $BridgeAgentDirectoryState_StaleCopyWith<$Res> {
  _$BridgeAgentDirectoryState_StaleCopyWithImpl(this._self, this._then);

  final BridgeAgentDirectoryState_Stale _self;
  final $Res Function(BridgeAgentDirectoryState_Stale) _then;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeAgentDirectoryState_Stale(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeStaleResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeAgentDirectoryData,
  ));
}


}

/// @nodoc


class BridgeAgentDirectoryState_Degraded extends BridgeAgentDirectoryState {
  const BridgeAgentDirectoryState_Degraded({required this.resource, required this.value}): super._();


 final  BridgeDegradedResource resource;
 final  BridgeAgentDirectoryData value;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentDirectoryState_DegradedCopyWith<BridgeAgentDirectoryState_Degraded> get copyWith => _$BridgeAgentDirectoryState_DegradedCopyWithImpl<BridgeAgentDirectoryState_Degraded>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentDirectoryState_Degraded&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeAgentDirectoryState.degraded(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentDirectoryState_DegradedCopyWith<$Res> implements $BridgeAgentDirectoryStateCopyWith<$Res> {
  factory $BridgeAgentDirectoryState_DegradedCopyWith(BridgeAgentDirectoryState_Degraded value, $Res Function(BridgeAgentDirectoryState_Degraded) _then) = _$BridgeAgentDirectoryState_DegradedCopyWithImpl;
@useResult
$Res call({
 BridgeDegradedResource resource, BridgeAgentDirectoryData value
});




}
/// @nodoc
class _$BridgeAgentDirectoryState_DegradedCopyWithImpl<$Res>
    implements $BridgeAgentDirectoryState_DegradedCopyWith<$Res> {
  _$BridgeAgentDirectoryState_DegradedCopyWithImpl(this._self, this._then);

  final BridgeAgentDirectoryState_Degraded _self;
  final $Res Function(BridgeAgentDirectoryState_Degraded) _then;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeAgentDirectoryState_Degraded(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeDegradedResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeAgentDirectoryData,
  ));
}


}

/// @nodoc


class BridgeAgentDirectoryState_Failed extends BridgeAgentDirectoryState {
  const BridgeAgentDirectoryState_Failed(this.field0): super._();


 final  BridgeFailedResource field0;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentDirectoryState_FailedCopyWith<BridgeAgentDirectoryState_Failed> get copyWith => _$BridgeAgentDirectoryState_FailedCopyWithImpl<BridgeAgentDirectoryState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentDirectoryState_Failed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeAgentDirectoryState.failed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentDirectoryState_FailedCopyWith<$Res> implements $BridgeAgentDirectoryStateCopyWith<$Res> {
  factory $BridgeAgentDirectoryState_FailedCopyWith(BridgeAgentDirectoryState_Failed value, $Res Function(BridgeAgentDirectoryState_Failed) _then) = _$BridgeAgentDirectoryState_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeFailedResource field0
});




}
/// @nodoc
class _$BridgeAgentDirectoryState_FailedCopyWithImpl<$Res>
    implements $BridgeAgentDirectoryState_FailedCopyWith<$Res> {
  _$BridgeAgentDirectoryState_FailedCopyWithImpl(this._self, this._then);

  final BridgeAgentDirectoryState_Failed _self;
  final $Res Function(BridgeAgentDirectoryState_Failed) _then;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentDirectoryState_Failed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeFailedResource,
  ));
}


}

/// @nodoc


class BridgeAgentDirectoryState_Stopped extends BridgeAgentDirectoryState {
  const BridgeAgentDirectoryState_Stopped(this.field0): super._();


 final  BridgeStoppedResource field0;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentDirectoryState_StoppedCopyWith<BridgeAgentDirectoryState_Stopped> get copyWith => _$BridgeAgentDirectoryState_StoppedCopyWithImpl<BridgeAgentDirectoryState_Stopped>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentDirectoryState_Stopped&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeAgentDirectoryState.stopped(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentDirectoryState_StoppedCopyWith<$Res> implements $BridgeAgentDirectoryStateCopyWith<$Res> {
  factory $BridgeAgentDirectoryState_StoppedCopyWith(BridgeAgentDirectoryState_Stopped value, $Res Function(BridgeAgentDirectoryState_Stopped) _then) = _$BridgeAgentDirectoryState_StoppedCopyWithImpl;
@useResult
$Res call({
 BridgeStoppedResource field0
});




}
/// @nodoc
class _$BridgeAgentDirectoryState_StoppedCopyWithImpl<$Res>
    implements $BridgeAgentDirectoryState_StoppedCopyWith<$Res> {
  _$BridgeAgentDirectoryState_StoppedCopyWithImpl(this._self, this._then);

  final BridgeAgentDirectoryState_Stopped _self;
  final $Res Function(BridgeAgentDirectoryState_Stopped) _then;

/// Create a copy of BridgeAgentDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentDirectoryState_Stopped(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeStoppedResource,
  ));
}


}

/// @nodoc
mixin _$BridgeAgentProfilesState {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentProfilesState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeAgentProfilesState()';
}


}

/// @nodoc
class $BridgeAgentProfilesStateCopyWith<$Res>  {
$BridgeAgentProfilesStateCopyWith(BridgeAgentProfilesState _, $Res Function(BridgeAgentProfilesState) __);
}


/// Adds pattern-matching-related methods to [BridgeAgentProfilesState].
extension BridgeAgentProfilesStatePatterns on BridgeAgentProfilesState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeAgentProfilesState_Uninitialized value)?  uninitialized,TResult Function( BridgeAgentProfilesState_Loading value)?  loading,TResult Function( BridgeAgentProfilesState_Ready value)?  ready,TResult Function( BridgeAgentProfilesState_Refreshing value)?  refreshing,TResult Function( BridgeAgentProfilesState_Stale value)?  stale,TResult Function( BridgeAgentProfilesState_Degraded value)?  degraded,TResult Function( BridgeAgentProfilesState_Failed value)?  failed,TResult Function( BridgeAgentProfilesState_Stopped value)?  stopped,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeAgentProfilesState_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeAgentProfilesState_Loading() when loading != null:
return loading(_that);case BridgeAgentProfilesState_Ready() when ready != null:
return ready(_that);case BridgeAgentProfilesState_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeAgentProfilesState_Stale() when stale != null:
return stale(_that);case BridgeAgentProfilesState_Degraded() when degraded != null:
return degraded(_that);case BridgeAgentProfilesState_Failed() when failed != null:
return failed(_that);case BridgeAgentProfilesState_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeAgentProfilesState_Uninitialized value)  uninitialized,required TResult Function( BridgeAgentProfilesState_Loading value)  loading,required TResult Function( BridgeAgentProfilesState_Ready value)  ready,required TResult Function( BridgeAgentProfilesState_Refreshing value)  refreshing,required TResult Function( BridgeAgentProfilesState_Stale value)  stale,required TResult Function( BridgeAgentProfilesState_Degraded value)  degraded,required TResult Function( BridgeAgentProfilesState_Failed value)  failed,required TResult Function( BridgeAgentProfilesState_Stopped value)  stopped,}){
final _that = this;
switch (_that) {
case BridgeAgentProfilesState_Uninitialized():
return uninitialized(_that);case BridgeAgentProfilesState_Loading():
return loading(_that);case BridgeAgentProfilesState_Ready():
return ready(_that);case BridgeAgentProfilesState_Refreshing():
return refreshing(_that);case BridgeAgentProfilesState_Stale():
return stale(_that);case BridgeAgentProfilesState_Degraded():
return degraded(_that);case BridgeAgentProfilesState_Failed():
return failed(_that);case BridgeAgentProfilesState_Stopped():
return stopped(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeAgentProfilesState_Uninitialized value)?  uninitialized,TResult? Function( BridgeAgentProfilesState_Loading value)?  loading,TResult? Function( BridgeAgentProfilesState_Ready value)?  ready,TResult? Function( BridgeAgentProfilesState_Refreshing value)?  refreshing,TResult? Function( BridgeAgentProfilesState_Stale value)?  stale,TResult? Function( BridgeAgentProfilesState_Degraded value)?  degraded,TResult? Function( BridgeAgentProfilesState_Failed value)?  failed,TResult? Function( BridgeAgentProfilesState_Stopped value)?  stopped,}){
final _that = this;
switch (_that) {
case BridgeAgentProfilesState_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeAgentProfilesState_Loading() when loading != null:
return loading(_that);case BridgeAgentProfilesState_Ready() when ready != null:
return ready(_that);case BridgeAgentProfilesState_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeAgentProfilesState_Stale() when stale != null:
return stale(_that);case BridgeAgentProfilesState_Degraded() when degraded != null:
return degraded(_that);case BridgeAgentProfilesState_Failed() when failed != null:
return failed(_that);case BridgeAgentProfilesState_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeUninitializedResource field0)?  uninitialized,TResult Function( BridgeLoadingResource field0)?  loading,TResult Function( BridgeReadyResource resource,  BridgeAgentProfilesData value)?  ready,TResult Function( BridgeRefreshingResource resource,  BridgeAgentProfilesData value)?  refreshing,TResult Function( BridgeStaleResource resource,  BridgeAgentProfilesData value)?  stale,TResult Function( BridgeDegradedResource resource,  BridgeAgentProfilesData value)?  degraded,TResult Function( BridgeFailedResource field0)?  failed,TResult Function( BridgeStoppedResource field0)?  stopped,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeAgentProfilesState_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeAgentProfilesState_Loading() when loading != null:
return loading(_that.field0);case BridgeAgentProfilesState_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeAgentProfilesState_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeAgentProfilesState_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeAgentProfilesState_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeAgentProfilesState_Failed() when failed != null:
return failed(_that.field0);case BridgeAgentProfilesState_Stopped() when stopped != null:
return stopped(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeUninitializedResource field0)  uninitialized,required TResult Function( BridgeLoadingResource field0)  loading,required TResult Function( BridgeReadyResource resource,  BridgeAgentProfilesData value)  ready,required TResult Function( BridgeRefreshingResource resource,  BridgeAgentProfilesData value)  refreshing,required TResult Function( BridgeStaleResource resource,  BridgeAgentProfilesData value)  stale,required TResult Function( BridgeDegradedResource resource,  BridgeAgentProfilesData value)  degraded,required TResult Function( BridgeFailedResource field0)  failed,required TResult Function( BridgeStoppedResource field0)  stopped,}) {final _that = this;
switch (_that) {
case BridgeAgentProfilesState_Uninitialized():
return uninitialized(_that.field0);case BridgeAgentProfilesState_Loading():
return loading(_that.field0);case BridgeAgentProfilesState_Ready():
return ready(_that.resource,_that.value);case BridgeAgentProfilesState_Refreshing():
return refreshing(_that.resource,_that.value);case BridgeAgentProfilesState_Stale():
return stale(_that.resource,_that.value);case BridgeAgentProfilesState_Degraded():
return degraded(_that.resource,_that.value);case BridgeAgentProfilesState_Failed():
return failed(_that.field0);case BridgeAgentProfilesState_Stopped():
return stopped(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeUninitializedResource field0)?  uninitialized,TResult? Function( BridgeLoadingResource field0)?  loading,TResult? Function( BridgeReadyResource resource,  BridgeAgentProfilesData value)?  ready,TResult? Function( BridgeRefreshingResource resource,  BridgeAgentProfilesData value)?  refreshing,TResult? Function( BridgeStaleResource resource,  BridgeAgentProfilesData value)?  stale,TResult? Function( BridgeDegradedResource resource,  BridgeAgentProfilesData value)?  degraded,TResult? Function( BridgeFailedResource field0)?  failed,TResult? Function( BridgeStoppedResource field0)?  stopped,}) {final _that = this;
switch (_that) {
case BridgeAgentProfilesState_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeAgentProfilesState_Loading() when loading != null:
return loading(_that.field0);case BridgeAgentProfilesState_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeAgentProfilesState_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeAgentProfilesState_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeAgentProfilesState_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeAgentProfilesState_Failed() when failed != null:
return failed(_that.field0);case BridgeAgentProfilesState_Stopped() when stopped != null:
return stopped(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeAgentProfilesState_Uninitialized extends BridgeAgentProfilesState {
  const BridgeAgentProfilesState_Uninitialized(this.field0): super._();


 final  BridgeUninitializedResource field0;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentProfilesState_UninitializedCopyWith<BridgeAgentProfilesState_Uninitialized> get copyWith => _$BridgeAgentProfilesState_UninitializedCopyWithImpl<BridgeAgentProfilesState_Uninitialized>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentProfilesState_Uninitialized&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeAgentProfilesState.uninitialized(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentProfilesState_UninitializedCopyWith<$Res> implements $BridgeAgentProfilesStateCopyWith<$Res> {
  factory $BridgeAgentProfilesState_UninitializedCopyWith(BridgeAgentProfilesState_Uninitialized value, $Res Function(BridgeAgentProfilesState_Uninitialized) _then) = _$BridgeAgentProfilesState_UninitializedCopyWithImpl;
@useResult
$Res call({
 BridgeUninitializedResource field0
});




}
/// @nodoc
class _$BridgeAgentProfilesState_UninitializedCopyWithImpl<$Res>
    implements $BridgeAgentProfilesState_UninitializedCopyWith<$Res> {
  _$BridgeAgentProfilesState_UninitializedCopyWithImpl(this._self, this._then);

  final BridgeAgentProfilesState_Uninitialized _self;
  final $Res Function(BridgeAgentProfilesState_Uninitialized) _then;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentProfilesState_Uninitialized(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeUninitializedResource,
  ));
}


}

/// @nodoc


class BridgeAgentProfilesState_Loading extends BridgeAgentProfilesState {
  const BridgeAgentProfilesState_Loading(this.field0): super._();


 final  BridgeLoadingResource field0;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentProfilesState_LoadingCopyWith<BridgeAgentProfilesState_Loading> get copyWith => _$BridgeAgentProfilesState_LoadingCopyWithImpl<BridgeAgentProfilesState_Loading>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentProfilesState_Loading&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeAgentProfilesState.loading(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentProfilesState_LoadingCopyWith<$Res> implements $BridgeAgentProfilesStateCopyWith<$Res> {
  factory $BridgeAgentProfilesState_LoadingCopyWith(BridgeAgentProfilesState_Loading value, $Res Function(BridgeAgentProfilesState_Loading) _then) = _$BridgeAgentProfilesState_LoadingCopyWithImpl;
@useResult
$Res call({
 BridgeLoadingResource field0
});




}
/// @nodoc
class _$BridgeAgentProfilesState_LoadingCopyWithImpl<$Res>
    implements $BridgeAgentProfilesState_LoadingCopyWith<$Res> {
  _$BridgeAgentProfilesState_LoadingCopyWithImpl(this._self, this._then);

  final BridgeAgentProfilesState_Loading _self;
  final $Res Function(BridgeAgentProfilesState_Loading) _then;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentProfilesState_Loading(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeLoadingResource,
  ));
}


}

/// @nodoc


class BridgeAgentProfilesState_Ready extends BridgeAgentProfilesState {
  const BridgeAgentProfilesState_Ready({required this.resource, required this.value}): super._();


 final  BridgeReadyResource resource;
 final  BridgeAgentProfilesData value;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentProfilesState_ReadyCopyWith<BridgeAgentProfilesState_Ready> get copyWith => _$BridgeAgentProfilesState_ReadyCopyWithImpl<BridgeAgentProfilesState_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentProfilesState_Ready&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeAgentProfilesState.ready(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentProfilesState_ReadyCopyWith<$Res> implements $BridgeAgentProfilesStateCopyWith<$Res> {
  factory $BridgeAgentProfilesState_ReadyCopyWith(BridgeAgentProfilesState_Ready value, $Res Function(BridgeAgentProfilesState_Ready) _then) = _$BridgeAgentProfilesState_ReadyCopyWithImpl;
@useResult
$Res call({
 BridgeReadyResource resource, BridgeAgentProfilesData value
});




}
/// @nodoc
class _$BridgeAgentProfilesState_ReadyCopyWithImpl<$Res>
    implements $BridgeAgentProfilesState_ReadyCopyWith<$Res> {
  _$BridgeAgentProfilesState_ReadyCopyWithImpl(this._self, this._then);

  final BridgeAgentProfilesState_Ready _self;
  final $Res Function(BridgeAgentProfilesState_Ready) _then;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeAgentProfilesState_Ready(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeReadyResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeAgentProfilesData,
  ));
}


}

/// @nodoc


class BridgeAgentProfilesState_Refreshing extends BridgeAgentProfilesState {
  const BridgeAgentProfilesState_Refreshing({required this.resource, required this.value}): super._();


 final  BridgeRefreshingResource resource;
 final  BridgeAgentProfilesData value;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentProfilesState_RefreshingCopyWith<BridgeAgentProfilesState_Refreshing> get copyWith => _$BridgeAgentProfilesState_RefreshingCopyWithImpl<BridgeAgentProfilesState_Refreshing>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentProfilesState_Refreshing&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeAgentProfilesState.refreshing(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentProfilesState_RefreshingCopyWith<$Res> implements $BridgeAgentProfilesStateCopyWith<$Res> {
  factory $BridgeAgentProfilesState_RefreshingCopyWith(BridgeAgentProfilesState_Refreshing value, $Res Function(BridgeAgentProfilesState_Refreshing) _then) = _$BridgeAgentProfilesState_RefreshingCopyWithImpl;
@useResult
$Res call({
 BridgeRefreshingResource resource, BridgeAgentProfilesData value
});




}
/// @nodoc
class _$BridgeAgentProfilesState_RefreshingCopyWithImpl<$Res>
    implements $BridgeAgentProfilesState_RefreshingCopyWith<$Res> {
  _$BridgeAgentProfilesState_RefreshingCopyWithImpl(this._self, this._then);

  final BridgeAgentProfilesState_Refreshing _self;
  final $Res Function(BridgeAgentProfilesState_Refreshing) _then;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeAgentProfilesState_Refreshing(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeRefreshingResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeAgentProfilesData,
  ));
}


}

/// @nodoc


class BridgeAgentProfilesState_Stale extends BridgeAgentProfilesState {
  const BridgeAgentProfilesState_Stale({required this.resource, required this.value}): super._();


 final  BridgeStaleResource resource;
 final  BridgeAgentProfilesData value;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentProfilesState_StaleCopyWith<BridgeAgentProfilesState_Stale> get copyWith => _$BridgeAgentProfilesState_StaleCopyWithImpl<BridgeAgentProfilesState_Stale>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentProfilesState_Stale&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeAgentProfilesState.stale(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentProfilesState_StaleCopyWith<$Res> implements $BridgeAgentProfilesStateCopyWith<$Res> {
  factory $BridgeAgentProfilesState_StaleCopyWith(BridgeAgentProfilesState_Stale value, $Res Function(BridgeAgentProfilesState_Stale) _then) = _$BridgeAgentProfilesState_StaleCopyWithImpl;
@useResult
$Res call({
 BridgeStaleResource resource, BridgeAgentProfilesData value
});




}
/// @nodoc
class _$BridgeAgentProfilesState_StaleCopyWithImpl<$Res>
    implements $BridgeAgentProfilesState_StaleCopyWith<$Res> {
  _$BridgeAgentProfilesState_StaleCopyWithImpl(this._self, this._then);

  final BridgeAgentProfilesState_Stale _self;
  final $Res Function(BridgeAgentProfilesState_Stale) _then;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeAgentProfilesState_Stale(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeStaleResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeAgentProfilesData,
  ));
}


}

/// @nodoc


class BridgeAgentProfilesState_Degraded extends BridgeAgentProfilesState {
  const BridgeAgentProfilesState_Degraded({required this.resource, required this.value}): super._();


 final  BridgeDegradedResource resource;
 final  BridgeAgentProfilesData value;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentProfilesState_DegradedCopyWith<BridgeAgentProfilesState_Degraded> get copyWith => _$BridgeAgentProfilesState_DegradedCopyWithImpl<BridgeAgentProfilesState_Degraded>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentProfilesState_Degraded&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeAgentProfilesState.degraded(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentProfilesState_DegradedCopyWith<$Res> implements $BridgeAgentProfilesStateCopyWith<$Res> {
  factory $BridgeAgentProfilesState_DegradedCopyWith(BridgeAgentProfilesState_Degraded value, $Res Function(BridgeAgentProfilesState_Degraded) _then) = _$BridgeAgentProfilesState_DegradedCopyWithImpl;
@useResult
$Res call({
 BridgeDegradedResource resource, BridgeAgentProfilesData value
});




}
/// @nodoc
class _$BridgeAgentProfilesState_DegradedCopyWithImpl<$Res>
    implements $BridgeAgentProfilesState_DegradedCopyWith<$Res> {
  _$BridgeAgentProfilesState_DegradedCopyWithImpl(this._self, this._then);

  final BridgeAgentProfilesState_Degraded _self;
  final $Res Function(BridgeAgentProfilesState_Degraded) _then;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeAgentProfilesState_Degraded(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeDegradedResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeAgentProfilesData,
  ));
}


}

/// @nodoc


class BridgeAgentProfilesState_Failed extends BridgeAgentProfilesState {
  const BridgeAgentProfilesState_Failed(this.field0): super._();


 final  BridgeFailedResource field0;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentProfilesState_FailedCopyWith<BridgeAgentProfilesState_Failed> get copyWith => _$BridgeAgentProfilesState_FailedCopyWithImpl<BridgeAgentProfilesState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentProfilesState_Failed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeAgentProfilesState.failed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentProfilesState_FailedCopyWith<$Res> implements $BridgeAgentProfilesStateCopyWith<$Res> {
  factory $BridgeAgentProfilesState_FailedCopyWith(BridgeAgentProfilesState_Failed value, $Res Function(BridgeAgentProfilesState_Failed) _then) = _$BridgeAgentProfilesState_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeFailedResource field0
});




}
/// @nodoc
class _$BridgeAgentProfilesState_FailedCopyWithImpl<$Res>
    implements $BridgeAgentProfilesState_FailedCopyWith<$Res> {
  _$BridgeAgentProfilesState_FailedCopyWithImpl(this._self, this._then);

  final BridgeAgentProfilesState_Failed _self;
  final $Res Function(BridgeAgentProfilesState_Failed) _then;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentProfilesState_Failed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeFailedResource,
  ));
}


}

/// @nodoc


class BridgeAgentProfilesState_Stopped extends BridgeAgentProfilesState {
  const BridgeAgentProfilesState_Stopped(this.field0): super._();


 final  BridgeStoppedResource field0;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentProfilesState_StoppedCopyWith<BridgeAgentProfilesState_Stopped> get copyWith => _$BridgeAgentProfilesState_StoppedCopyWithImpl<BridgeAgentProfilesState_Stopped>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentProfilesState_Stopped&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeAgentProfilesState.stopped(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentProfilesState_StoppedCopyWith<$Res> implements $BridgeAgentProfilesStateCopyWith<$Res> {
  factory $BridgeAgentProfilesState_StoppedCopyWith(BridgeAgentProfilesState_Stopped value, $Res Function(BridgeAgentProfilesState_Stopped) _then) = _$BridgeAgentProfilesState_StoppedCopyWithImpl;
@useResult
$Res call({
 BridgeStoppedResource field0
});




}
/// @nodoc
class _$BridgeAgentProfilesState_StoppedCopyWithImpl<$Res>
    implements $BridgeAgentProfilesState_StoppedCopyWith<$Res> {
  _$BridgeAgentProfilesState_StoppedCopyWithImpl(this._self, this._then);

  final BridgeAgentProfilesState_Stopped _self;
  final $Res Function(BridgeAgentProfilesState_Stopped) _then;

/// Create a copy of BridgeAgentProfilesState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentProfilesState_Stopped(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeStoppedResource,
  ));
}


}

/// @nodoc
mixin _$BridgeLspStateSnapshot {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspStateSnapshot);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeLspStateSnapshot()';
}


}

/// @nodoc
class $BridgeLspStateSnapshotCopyWith<$Res>  {
$BridgeLspStateSnapshotCopyWith(BridgeLspStateSnapshot _, $Res Function(BridgeLspStateSnapshot) __);
}


/// Adds pattern-matching-related methods to [BridgeLspStateSnapshot].
extension BridgeLspStateSnapshotPatterns on BridgeLspStateSnapshot {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeLspStateSnapshot_Uninitialized value)?  uninitialized,TResult Function( BridgeLspStateSnapshot_Loading value)?  loading,TResult Function( BridgeLspStateSnapshot_Ready value)?  ready,TResult Function( BridgeLspStateSnapshot_Refreshing value)?  refreshing,TResult Function( BridgeLspStateSnapshot_Stale value)?  stale,TResult Function( BridgeLspStateSnapshot_Degraded value)?  degraded,TResult Function( BridgeLspStateSnapshot_Failed value)?  failed,TResult Function( BridgeLspStateSnapshot_Stopped value)?  stopped,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeLspStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeLspStateSnapshot_Loading() when loading != null:
return loading(_that);case BridgeLspStateSnapshot_Ready() when ready != null:
return ready(_that);case BridgeLspStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeLspStateSnapshot_Stale() when stale != null:
return stale(_that);case BridgeLspStateSnapshot_Degraded() when degraded != null:
return degraded(_that);case BridgeLspStateSnapshot_Failed() when failed != null:
return failed(_that);case BridgeLspStateSnapshot_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeLspStateSnapshot_Uninitialized value)  uninitialized,required TResult Function( BridgeLspStateSnapshot_Loading value)  loading,required TResult Function( BridgeLspStateSnapshot_Ready value)  ready,required TResult Function( BridgeLspStateSnapshot_Refreshing value)  refreshing,required TResult Function( BridgeLspStateSnapshot_Stale value)  stale,required TResult Function( BridgeLspStateSnapshot_Degraded value)  degraded,required TResult Function( BridgeLspStateSnapshot_Failed value)  failed,required TResult Function( BridgeLspStateSnapshot_Stopped value)  stopped,}){
final _that = this;
switch (_that) {
case BridgeLspStateSnapshot_Uninitialized():
return uninitialized(_that);case BridgeLspStateSnapshot_Loading():
return loading(_that);case BridgeLspStateSnapshot_Ready():
return ready(_that);case BridgeLspStateSnapshot_Refreshing():
return refreshing(_that);case BridgeLspStateSnapshot_Stale():
return stale(_that);case BridgeLspStateSnapshot_Degraded():
return degraded(_that);case BridgeLspStateSnapshot_Failed():
return failed(_that);case BridgeLspStateSnapshot_Stopped():
return stopped(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeLspStateSnapshot_Uninitialized value)?  uninitialized,TResult? Function( BridgeLspStateSnapshot_Loading value)?  loading,TResult? Function( BridgeLspStateSnapshot_Ready value)?  ready,TResult? Function( BridgeLspStateSnapshot_Refreshing value)?  refreshing,TResult? Function( BridgeLspStateSnapshot_Stale value)?  stale,TResult? Function( BridgeLspStateSnapshot_Degraded value)?  degraded,TResult? Function( BridgeLspStateSnapshot_Failed value)?  failed,TResult? Function( BridgeLspStateSnapshot_Stopped value)?  stopped,}){
final _that = this;
switch (_that) {
case BridgeLspStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeLspStateSnapshot_Loading() when loading != null:
return loading(_that);case BridgeLspStateSnapshot_Ready() when ready != null:
return ready(_that);case BridgeLspStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeLspStateSnapshot_Stale() when stale != null:
return stale(_that);case BridgeLspStateSnapshot_Degraded() when degraded != null:
return degraded(_that);case BridgeLspStateSnapshot_Failed() when failed != null:
return failed(_that);case BridgeLspStateSnapshot_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeUninitializedResource field0)?  uninitialized,TResult Function( BridgeLoadingResource field0)?  loading,TResult Function( BridgeReadyResource resource,  BridgeLspStateData value)?  ready,TResult Function( BridgeRefreshingResource resource,  BridgeLspStateData value)?  refreshing,TResult Function( BridgeStaleResource resource,  BridgeLspStateData value)?  stale,TResult Function( BridgeDegradedResource resource,  BridgeLspStateData value)?  degraded,TResult Function( BridgeFailedResource field0)?  failed,TResult Function( BridgeStoppedResource field0)?  stopped,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeLspStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeLspStateSnapshot_Loading() when loading != null:
return loading(_that.field0);case BridgeLspStateSnapshot_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeLspStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeLspStateSnapshot_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeLspStateSnapshot_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeLspStateSnapshot_Failed() when failed != null:
return failed(_that.field0);case BridgeLspStateSnapshot_Stopped() when stopped != null:
return stopped(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeUninitializedResource field0)  uninitialized,required TResult Function( BridgeLoadingResource field0)  loading,required TResult Function( BridgeReadyResource resource,  BridgeLspStateData value)  ready,required TResult Function( BridgeRefreshingResource resource,  BridgeLspStateData value)  refreshing,required TResult Function( BridgeStaleResource resource,  BridgeLspStateData value)  stale,required TResult Function( BridgeDegradedResource resource,  BridgeLspStateData value)  degraded,required TResult Function( BridgeFailedResource field0)  failed,required TResult Function( BridgeStoppedResource field0)  stopped,}) {final _that = this;
switch (_that) {
case BridgeLspStateSnapshot_Uninitialized():
return uninitialized(_that.field0);case BridgeLspStateSnapshot_Loading():
return loading(_that.field0);case BridgeLspStateSnapshot_Ready():
return ready(_that.resource,_that.value);case BridgeLspStateSnapshot_Refreshing():
return refreshing(_that.resource,_that.value);case BridgeLspStateSnapshot_Stale():
return stale(_that.resource,_that.value);case BridgeLspStateSnapshot_Degraded():
return degraded(_that.resource,_that.value);case BridgeLspStateSnapshot_Failed():
return failed(_that.field0);case BridgeLspStateSnapshot_Stopped():
return stopped(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeUninitializedResource field0)?  uninitialized,TResult? Function( BridgeLoadingResource field0)?  loading,TResult? Function( BridgeReadyResource resource,  BridgeLspStateData value)?  ready,TResult? Function( BridgeRefreshingResource resource,  BridgeLspStateData value)?  refreshing,TResult? Function( BridgeStaleResource resource,  BridgeLspStateData value)?  stale,TResult? Function( BridgeDegradedResource resource,  BridgeLspStateData value)?  degraded,TResult? Function( BridgeFailedResource field0)?  failed,TResult? Function( BridgeStoppedResource field0)?  stopped,}) {final _that = this;
switch (_that) {
case BridgeLspStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeLspStateSnapshot_Loading() when loading != null:
return loading(_that.field0);case BridgeLspStateSnapshot_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeLspStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeLspStateSnapshot_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeLspStateSnapshot_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeLspStateSnapshot_Failed() when failed != null:
return failed(_that.field0);case BridgeLspStateSnapshot_Stopped() when stopped != null:
return stopped(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeLspStateSnapshot_Uninitialized extends BridgeLspStateSnapshot {
  const BridgeLspStateSnapshot_Uninitialized(this.field0): super._();


 final  BridgeUninitializedResource field0;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeLspStateSnapshot_UninitializedCopyWith<BridgeLspStateSnapshot_Uninitialized> get copyWith => _$BridgeLspStateSnapshot_UninitializedCopyWithImpl<BridgeLspStateSnapshot_Uninitialized>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspStateSnapshot_Uninitialized&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeLspStateSnapshot.uninitialized(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeLspStateSnapshot_UninitializedCopyWith<$Res> implements $BridgeLspStateSnapshotCopyWith<$Res> {
  factory $BridgeLspStateSnapshot_UninitializedCopyWith(BridgeLspStateSnapshot_Uninitialized value, $Res Function(BridgeLspStateSnapshot_Uninitialized) _then) = _$BridgeLspStateSnapshot_UninitializedCopyWithImpl;
@useResult
$Res call({
 BridgeUninitializedResource field0
});




}
/// @nodoc
class _$BridgeLspStateSnapshot_UninitializedCopyWithImpl<$Res>
    implements $BridgeLspStateSnapshot_UninitializedCopyWith<$Res> {
  _$BridgeLspStateSnapshot_UninitializedCopyWithImpl(this._self, this._then);

  final BridgeLspStateSnapshot_Uninitialized _self;
  final $Res Function(BridgeLspStateSnapshot_Uninitialized) _then;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeLspStateSnapshot_Uninitialized(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeUninitializedResource,
  ));
}


}

/// @nodoc


class BridgeLspStateSnapshot_Loading extends BridgeLspStateSnapshot {
  const BridgeLspStateSnapshot_Loading(this.field0): super._();


 final  BridgeLoadingResource field0;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeLspStateSnapshot_LoadingCopyWith<BridgeLspStateSnapshot_Loading> get copyWith => _$BridgeLspStateSnapshot_LoadingCopyWithImpl<BridgeLspStateSnapshot_Loading>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspStateSnapshot_Loading&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeLspStateSnapshot.loading(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeLspStateSnapshot_LoadingCopyWith<$Res> implements $BridgeLspStateSnapshotCopyWith<$Res> {
  factory $BridgeLspStateSnapshot_LoadingCopyWith(BridgeLspStateSnapshot_Loading value, $Res Function(BridgeLspStateSnapshot_Loading) _then) = _$BridgeLspStateSnapshot_LoadingCopyWithImpl;
@useResult
$Res call({
 BridgeLoadingResource field0
});




}
/// @nodoc
class _$BridgeLspStateSnapshot_LoadingCopyWithImpl<$Res>
    implements $BridgeLspStateSnapshot_LoadingCopyWith<$Res> {
  _$BridgeLspStateSnapshot_LoadingCopyWithImpl(this._self, this._then);

  final BridgeLspStateSnapshot_Loading _self;
  final $Res Function(BridgeLspStateSnapshot_Loading) _then;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeLspStateSnapshot_Loading(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeLoadingResource,
  ));
}


}

/// @nodoc


class BridgeLspStateSnapshot_Ready extends BridgeLspStateSnapshot {
  const BridgeLspStateSnapshot_Ready({required this.resource, required this.value}): super._();


 final  BridgeReadyResource resource;
 final  BridgeLspStateData value;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeLspStateSnapshot_ReadyCopyWith<BridgeLspStateSnapshot_Ready> get copyWith => _$BridgeLspStateSnapshot_ReadyCopyWithImpl<BridgeLspStateSnapshot_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspStateSnapshot_Ready&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeLspStateSnapshot.ready(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeLspStateSnapshot_ReadyCopyWith<$Res> implements $BridgeLspStateSnapshotCopyWith<$Res> {
  factory $BridgeLspStateSnapshot_ReadyCopyWith(BridgeLspStateSnapshot_Ready value, $Res Function(BridgeLspStateSnapshot_Ready) _then) = _$BridgeLspStateSnapshot_ReadyCopyWithImpl;
@useResult
$Res call({
 BridgeReadyResource resource, BridgeLspStateData value
});




}
/// @nodoc
class _$BridgeLspStateSnapshot_ReadyCopyWithImpl<$Res>
    implements $BridgeLspStateSnapshot_ReadyCopyWith<$Res> {
  _$BridgeLspStateSnapshot_ReadyCopyWithImpl(this._self, this._then);

  final BridgeLspStateSnapshot_Ready _self;
  final $Res Function(BridgeLspStateSnapshot_Ready) _then;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeLspStateSnapshot_Ready(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeReadyResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeLspStateData,
  ));
}


}

/// @nodoc


class BridgeLspStateSnapshot_Refreshing extends BridgeLspStateSnapshot {
  const BridgeLspStateSnapshot_Refreshing({required this.resource, required this.value}): super._();


 final  BridgeRefreshingResource resource;
 final  BridgeLspStateData value;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeLspStateSnapshot_RefreshingCopyWith<BridgeLspStateSnapshot_Refreshing> get copyWith => _$BridgeLspStateSnapshot_RefreshingCopyWithImpl<BridgeLspStateSnapshot_Refreshing>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspStateSnapshot_Refreshing&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeLspStateSnapshot.refreshing(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeLspStateSnapshot_RefreshingCopyWith<$Res> implements $BridgeLspStateSnapshotCopyWith<$Res> {
  factory $BridgeLspStateSnapshot_RefreshingCopyWith(BridgeLspStateSnapshot_Refreshing value, $Res Function(BridgeLspStateSnapshot_Refreshing) _then) = _$BridgeLspStateSnapshot_RefreshingCopyWithImpl;
@useResult
$Res call({
 BridgeRefreshingResource resource, BridgeLspStateData value
});




}
/// @nodoc
class _$BridgeLspStateSnapshot_RefreshingCopyWithImpl<$Res>
    implements $BridgeLspStateSnapshot_RefreshingCopyWith<$Res> {
  _$BridgeLspStateSnapshot_RefreshingCopyWithImpl(this._self, this._then);

  final BridgeLspStateSnapshot_Refreshing _self;
  final $Res Function(BridgeLspStateSnapshot_Refreshing) _then;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeLspStateSnapshot_Refreshing(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeRefreshingResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeLspStateData,
  ));
}


}

/// @nodoc


class BridgeLspStateSnapshot_Stale extends BridgeLspStateSnapshot {
  const BridgeLspStateSnapshot_Stale({required this.resource, required this.value}): super._();


 final  BridgeStaleResource resource;
 final  BridgeLspStateData value;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeLspStateSnapshot_StaleCopyWith<BridgeLspStateSnapshot_Stale> get copyWith => _$BridgeLspStateSnapshot_StaleCopyWithImpl<BridgeLspStateSnapshot_Stale>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspStateSnapshot_Stale&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeLspStateSnapshot.stale(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeLspStateSnapshot_StaleCopyWith<$Res> implements $BridgeLspStateSnapshotCopyWith<$Res> {
  factory $BridgeLspStateSnapshot_StaleCopyWith(BridgeLspStateSnapshot_Stale value, $Res Function(BridgeLspStateSnapshot_Stale) _then) = _$BridgeLspStateSnapshot_StaleCopyWithImpl;
@useResult
$Res call({
 BridgeStaleResource resource, BridgeLspStateData value
});




}
/// @nodoc
class _$BridgeLspStateSnapshot_StaleCopyWithImpl<$Res>
    implements $BridgeLspStateSnapshot_StaleCopyWith<$Res> {
  _$BridgeLspStateSnapshot_StaleCopyWithImpl(this._self, this._then);

  final BridgeLspStateSnapshot_Stale _self;
  final $Res Function(BridgeLspStateSnapshot_Stale) _then;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeLspStateSnapshot_Stale(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeStaleResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeLspStateData,
  ));
}


}

/// @nodoc


class BridgeLspStateSnapshot_Degraded extends BridgeLspStateSnapshot {
  const BridgeLspStateSnapshot_Degraded({required this.resource, required this.value}): super._();


 final  BridgeDegradedResource resource;
 final  BridgeLspStateData value;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeLspStateSnapshot_DegradedCopyWith<BridgeLspStateSnapshot_Degraded> get copyWith => _$BridgeLspStateSnapshot_DegradedCopyWithImpl<BridgeLspStateSnapshot_Degraded>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspStateSnapshot_Degraded&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeLspStateSnapshot.degraded(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeLspStateSnapshot_DegradedCopyWith<$Res> implements $BridgeLspStateSnapshotCopyWith<$Res> {
  factory $BridgeLspStateSnapshot_DegradedCopyWith(BridgeLspStateSnapshot_Degraded value, $Res Function(BridgeLspStateSnapshot_Degraded) _then) = _$BridgeLspStateSnapshot_DegradedCopyWithImpl;
@useResult
$Res call({
 BridgeDegradedResource resource, BridgeLspStateData value
});




}
/// @nodoc
class _$BridgeLspStateSnapshot_DegradedCopyWithImpl<$Res>
    implements $BridgeLspStateSnapshot_DegradedCopyWith<$Res> {
  _$BridgeLspStateSnapshot_DegradedCopyWithImpl(this._self, this._then);

  final BridgeLspStateSnapshot_Degraded _self;
  final $Res Function(BridgeLspStateSnapshot_Degraded) _then;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeLspStateSnapshot_Degraded(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeDegradedResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeLspStateData,
  ));
}


}

/// @nodoc


class BridgeLspStateSnapshot_Failed extends BridgeLspStateSnapshot {
  const BridgeLspStateSnapshot_Failed(this.field0): super._();


 final  BridgeFailedResource field0;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeLspStateSnapshot_FailedCopyWith<BridgeLspStateSnapshot_Failed> get copyWith => _$BridgeLspStateSnapshot_FailedCopyWithImpl<BridgeLspStateSnapshot_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspStateSnapshot_Failed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeLspStateSnapshot.failed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeLspStateSnapshot_FailedCopyWith<$Res> implements $BridgeLspStateSnapshotCopyWith<$Res> {
  factory $BridgeLspStateSnapshot_FailedCopyWith(BridgeLspStateSnapshot_Failed value, $Res Function(BridgeLspStateSnapshot_Failed) _then) = _$BridgeLspStateSnapshot_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeFailedResource field0
});




}
/// @nodoc
class _$BridgeLspStateSnapshot_FailedCopyWithImpl<$Res>
    implements $BridgeLspStateSnapshot_FailedCopyWith<$Res> {
  _$BridgeLspStateSnapshot_FailedCopyWithImpl(this._self, this._then);

  final BridgeLspStateSnapshot_Failed _self;
  final $Res Function(BridgeLspStateSnapshot_Failed) _then;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeLspStateSnapshot_Failed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeFailedResource,
  ));
}


}

/// @nodoc


class BridgeLspStateSnapshot_Stopped extends BridgeLspStateSnapshot {
  const BridgeLspStateSnapshot_Stopped(this.field0): super._();


 final  BridgeStoppedResource field0;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeLspStateSnapshot_StoppedCopyWith<BridgeLspStateSnapshot_Stopped> get copyWith => _$BridgeLspStateSnapshot_StoppedCopyWithImpl<BridgeLspStateSnapshot_Stopped>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspStateSnapshot_Stopped&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeLspStateSnapshot.stopped(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeLspStateSnapshot_StoppedCopyWith<$Res> implements $BridgeLspStateSnapshotCopyWith<$Res> {
  factory $BridgeLspStateSnapshot_StoppedCopyWith(BridgeLspStateSnapshot_Stopped value, $Res Function(BridgeLspStateSnapshot_Stopped) _then) = _$BridgeLspStateSnapshot_StoppedCopyWithImpl;
@useResult
$Res call({
 BridgeStoppedResource field0
});




}
/// @nodoc
class _$BridgeLspStateSnapshot_StoppedCopyWithImpl<$Res>
    implements $BridgeLspStateSnapshot_StoppedCopyWith<$Res> {
  _$BridgeLspStateSnapshot_StoppedCopyWithImpl(this._self, this._then);

  final BridgeLspStateSnapshot_Stopped _self;
  final $Res Function(BridgeLspStateSnapshot_Stopped) _then;

/// Create a copy of BridgeLspStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeLspStateSnapshot_Stopped(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeStoppedResource,
  ));
}


}

/// @nodoc
mixin _$BridgeMcpStateSnapshot {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpStateSnapshot);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeMcpStateSnapshot()';
}


}

/// @nodoc
class $BridgeMcpStateSnapshotCopyWith<$Res>  {
$BridgeMcpStateSnapshotCopyWith(BridgeMcpStateSnapshot _, $Res Function(BridgeMcpStateSnapshot) __);
}


/// Adds pattern-matching-related methods to [BridgeMcpStateSnapshot].
extension BridgeMcpStateSnapshotPatterns on BridgeMcpStateSnapshot {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeMcpStateSnapshot_Uninitialized value)?  uninitialized,TResult Function( BridgeMcpStateSnapshot_Loading value)?  loading,TResult Function( BridgeMcpStateSnapshot_Ready value)?  ready,TResult Function( BridgeMcpStateSnapshot_Refreshing value)?  refreshing,TResult Function( BridgeMcpStateSnapshot_Stale value)?  stale,TResult Function( BridgeMcpStateSnapshot_Degraded value)?  degraded,TResult Function( BridgeMcpStateSnapshot_Failed value)?  failed,TResult Function( BridgeMcpStateSnapshot_Stopped value)?  stopped,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeMcpStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeMcpStateSnapshot_Loading() when loading != null:
return loading(_that);case BridgeMcpStateSnapshot_Ready() when ready != null:
return ready(_that);case BridgeMcpStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeMcpStateSnapshot_Stale() when stale != null:
return stale(_that);case BridgeMcpStateSnapshot_Degraded() when degraded != null:
return degraded(_that);case BridgeMcpStateSnapshot_Failed() when failed != null:
return failed(_that);case BridgeMcpStateSnapshot_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeMcpStateSnapshot_Uninitialized value)  uninitialized,required TResult Function( BridgeMcpStateSnapshot_Loading value)  loading,required TResult Function( BridgeMcpStateSnapshot_Ready value)  ready,required TResult Function( BridgeMcpStateSnapshot_Refreshing value)  refreshing,required TResult Function( BridgeMcpStateSnapshot_Stale value)  stale,required TResult Function( BridgeMcpStateSnapshot_Degraded value)  degraded,required TResult Function( BridgeMcpStateSnapshot_Failed value)  failed,required TResult Function( BridgeMcpStateSnapshot_Stopped value)  stopped,}){
final _that = this;
switch (_that) {
case BridgeMcpStateSnapshot_Uninitialized():
return uninitialized(_that);case BridgeMcpStateSnapshot_Loading():
return loading(_that);case BridgeMcpStateSnapshot_Ready():
return ready(_that);case BridgeMcpStateSnapshot_Refreshing():
return refreshing(_that);case BridgeMcpStateSnapshot_Stale():
return stale(_that);case BridgeMcpStateSnapshot_Degraded():
return degraded(_that);case BridgeMcpStateSnapshot_Failed():
return failed(_that);case BridgeMcpStateSnapshot_Stopped():
return stopped(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeMcpStateSnapshot_Uninitialized value)?  uninitialized,TResult? Function( BridgeMcpStateSnapshot_Loading value)?  loading,TResult? Function( BridgeMcpStateSnapshot_Ready value)?  ready,TResult? Function( BridgeMcpStateSnapshot_Refreshing value)?  refreshing,TResult? Function( BridgeMcpStateSnapshot_Stale value)?  stale,TResult? Function( BridgeMcpStateSnapshot_Degraded value)?  degraded,TResult? Function( BridgeMcpStateSnapshot_Failed value)?  failed,TResult? Function( BridgeMcpStateSnapshot_Stopped value)?  stopped,}){
final _that = this;
switch (_that) {
case BridgeMcpStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeMcpStateSnapshot_Loading() when loading != null:
return loading(_that);case BridgeMcpStateSnapshot_Ready() when ready != null:
return ready(_that);case BridgeMcpStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeMcpStateSnapshot_Stale() when stale != null:
return stale(_that);case BridgeMcpStateSnapshot_Degraded() when degraded != null:
return degraded(_that);case BridgeMcpStateSnapshot_Failed() when failed != null:
return failed(_that);case BridgeMcpStateSnapshot_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeUninitializedResource field0)?  uninitialized,TResult Function( BridgeLoadingResource field0)?  loading,TResult Function( BridgeReadyResource resource,  BridgeMcpStateData value)?  ready,TResult Function( BridgeRefreshingResource resource,  BridgeMcpStateData value)?  refreshing,TResult Function( BridgeStaleResource resource,  BridgeMcpStateData value)?  stale,TResult Function( BridgeDegradedResource resource,  BridgeMcpStateData value)?  degraded,TResult Function( BridgeFailedResource field0)?  failed,TResult Function( BridgeStoppedResource field0)?  stopped,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeMcpStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeMcpStateSnapshot_Loading() when loading != null:
return loading(_that.field0);case BridgeMcpStateSnapshot_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeMcpStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeMcpStateSnapshot_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeMcpStateSnapshot_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeMcpStateSnapshot_Failed() when failed != null:
return failed(_that.field0);case BridgeMcpStateSnapshot_Stopped() when stopped != null:
return stopped(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeUninitializedResource field0)  uninitialized,required TResult Function( BridgeLoadingResource field0)  loading,required TResult Function( BridgeReadyResource resource,  BridgeMcpStateData value)  ready,required TResult Function( BridgeRefreshingResource resource,  BridgeMcpStateData value)  refreshing,required TResult Function( BridgeStaleResource resource,  BridgeMcpStateData value)  stale,required TResult Function( BridgeDegradedResource resource,  BridgeMcpStateData value)  degraded,required TResult Function( BridgeFailedResource field0)  failed,required TResult Function( BridgeStoppedResource field0)  stopped,}) {final _that = this;
switch (_that) {
case BridgeMcpStateSnapshot_Uninitialized():
return uninitialized(_that.field0);case BridgeMcpStateSnapshot_Loading():
return loading(_that.field0);case BridgeMcpStateSnapshot_Ready():
return ready(_that.resource,_that.value);case BridgeMcpStateSnapshot_Refreshing():
return refreshing(_that.resource,_that.value);case BridgeMcpStateSnapshot_Stale():
return stale(_that.resource,_that.value);case BridgeMcpStateSnapshot_Degraded():
return degraded(_that.resource,_that.value);case BridgeMcpStateSnapshot_Failed():
return failed(_that.field0);case BridgeMcpStateSnapshot_Stopped():
return stopped(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeUninitializedResource field0)?  uninitialized,TResult? Function( BridgeLoadingResource field0)?  loading,TResult? Function( BridgeReadyResource resource,  BridgeMcpStateData value)?  ready,TResult? Function( BridgeRefreshingResource resource,  BridgeMcpStateData value)?  refreshing,TResult? Function( BridgeStaleResource resource,  BridgeMcpStateData value)?  stale,TResult? Function( BridgeDegradedResource resource,  BridgeMcpStateData value)?  degraded,TResult? Function( BridgeFailedResource field0)?  failed,TResult? Function( BridgeStoppedResource field0)?  stopped,}) {final _that = this;
switch (_that) {
case BridgeMcpStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeMcpStateSnapshot_Loading() when loading != null:
return loading(_that.field0);case BridgeMcpStateSnapshot_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeMcpStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeMcpStateSnapshot_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeMcpStateSnapshot_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeMcpStateSnapshot_Failed() when failed != null:
return failed(_that.field0);case BridgeMcpStateSnapshot_Stopped() when stopped != null:
return stopped(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeMcpStateSnapshot_Uninitialized extends BridgeMcpStateSnapshot {
  const BridgeMcpStateSnapshot_Uninitialized(this.field0): super._();


 final  BridgeUninitializedResource field0;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMcpStateSnapshot_UninitializedCopyWith<BridgeMcpStateSnapshot_Uninitialized> get copyWith => _$BridgeMcpStateSnapshot_UninitializedCopyWithImpl<BridgeMcpStateSnapshot_Uninitialized>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpStateSnapshot_Uninitialized&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeMcpStateSnapshot.uninitialized(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeMcpStateSnapshot_UninitializedCopyWith<$Res> implements $BridgeMcpStateSnapshotCopyWith<$Res> {
  factory $BridgeMcpStateSnapshot_UninitializedCopyWith(BridgeMcpStateSnapshot_Uninitialized value, $Res Function(BridgeMcpStateSnapshot_Uninitialized) _then) = _$BridgeMcpStateSnapshot_UninitializedCopyWithImpl;
@useResult
$Res call({
 BridgeUninitializedResource field0
});




}
/// @nodoc
class _$BridgeMcpStateSnapshot_UninitializedCopyWithImpl<$Res>
    implements $BridgeMcpStateSnapshot_UninitializedCopyWith<$Res> {
  _$BridgeMcpStateSnapshot_UninitializedCopyWithImpl(this._self, this._then);

  final BridgeMcpStateSnapshot_Uninitialized _self;
  final $Res Function(BridgeMcpStateSnapshot_Uninitialized) _then;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeMcpStateSnapshot_Uninitialized(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeUninitializedResource,
  ));
}


}

/// @nodoc


class BridgeMcpStateSnapshot_Loading extends BridgeMcpStateSnapshot {
  const BridgeMcpStateSnapshot_Loading(this.field0): super._();


 final  BridgeLoadingResource field0;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMcpStateSnapshot_LoadingCopyWith<BridgeMcpStateSnapshot_Loading> get copyWith => _$BridgeMcpStateSnapshot_LoadingCopyWithImpl<BridgeMcpStateSnapshot_Loading>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpStateSnapshot_Loading&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeMcpStateSnapshot.loading(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeMcpStateSnapshot_LoadingCopyWith<$Res> implements $BridgeMcpStateSnapshotCopyWith<$Res> {
  factory $BridgeMcpStateSnapshot_LoadingCopyWith(BridgeMcpStateSnapshot_Loading value, $Res Function(BridgeMcpStateSnapshot_Loading) _then) = _$BridgeMcpStateSnapshot_LoadingCopyWithImpl;
@useResult
$Res call({
 BridgeLoadingResource field0
});




}
/// @nodoc
class _$BridgeMcpStateSnapshot_LoadingCopyWithImpl<$Res>
    implements $BridgeMcpStateSnapshot_LoadingCopyWith<$Res> {
  _$BridgeMcpStateSnapshot_LoadingCopyWithImpl(this._self, this._then);

  final BridgeMcpStateSnapshot_Loading _self;
  final $Res Function(BridgeMcpStateSnapshot_Loading) _then;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeMcpStateSnapshot_Loading(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeLoadingResource,
  ));
}


}

/// @nodoc


class BridgeMcpStateSnapshot_Ready extends BridgeMcpStateSnapshot {
  const BridgeMcpStateSnapshot_Ready({required this.resource, required this.value}): super._();


 final  BridgeReadyResource resource;
 final  BridgeMcpStateData value;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMcpStateSnapshot_ReadyCopyWith<BridgeMcpStateSnapshot_Ready> get copyWith => _$BridgeMcpStateSnapshot_ReadyCopyWithImpl<BridgeMcpStateSnapshot_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpStateSnapshot_Ready&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeMcpStateSnapshot.ready(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeMcpStateSnapshot_ReadyCopyWith<$Res> implements $BridgeMcpStateSnapshotCopyWith<$Res> {
  factory $BridgeMcpStateSnapshot_ReadyCopyWith(BridgeMcpStateSnapshot_Ready value, $Res Function(BridgeMcpStateSnapshot_Ready) _then) = _$BridgeMcpStateSnapshot_ReadyCopyWithImpl;
@useResult
$Res call({
 BridgeReadyResource resource, BridgeMcpStateData value
});




}
/// @nodoc
class _$BridgeMcpStateSnapshot_ReadyCopyWithImpl<$Res>
    implements $BridgeMcpStateSnapshot_ReadyCopyWith<$Res> {
  _$BridgeMcpStateSnapshot_ReadyCopyWithImpl(this._self, this._then);

  final BridgeMcpStateSnapshot_Ready _self;
  final $Res Function(BridgeMcpStateSnapshot_Ready) _then;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeMcpStateSnapshot_Ready(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeReadyResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeMcpStateData,
  ));
}


}

/// @nodoc


class BridgeMcpStateSnapshot_Refreshing extends BridgeMcpStateSnapshot {
  const BridgeMcpStateSnapshot_Refreshing({required this.resource, required this.value}): super._();


 final  BridgeRefreshingResource resource;
 final  BridgeMcpStateData value;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMcpStateSnapshot_RefreshingCopyWith<BridgeMcpStateSnapshot_Refreshing> get copyWith => _$BridgeMcpStateSnapshot_RefreshingCopyWithImpl<BridgeMcpStateSnapshot_Refreshing>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpStateSnapshot_Refreshing&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeMcpStateSnapshot.refreshing(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeMcpStateSnapshot_RefreshingCopyWith<$Res> implements $BridgeMcpStateSnapshotCopyWith<$Res> {
  factory $BridgeMcpStateSnapshot_RefreshingCopyWith(BridgeMcpStateSnapshot_Refreshing value, $Res Function(BridgeMcpStateSnapshot_Refreshing) _then) = _$BridgeMcpStateSnapshot_RefreshingCopyWithImpl;
@useResult
$Res call({
 BridgeRefreshingResource resource, BridgeMcpStateData value
});




}
/// @nodoc
class _$BridgeMcpStateSnapshot_RefreshingCopyWithImpl<$Res>
    implements $BridgeMcpStateSnapshot_RefreshingCopyWith<$Res> {
  _$BridgeMcpStateSnapshot_RefreshingCopyWithImpl(this._self, this._then);

  final BridgeMcpStateSnapshot_Refreshing _self;
  final $Res Function(BridgeMcpStateSnapshot_Refreshing) _then;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeMcpStateSnapshot_Refreshing(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeRefreshingResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeMcpStateData,
  ));
}


}

/// @nodoc


class BridgeMcpStateSnapshot_Stale extends BridgeMcpStateSnapshot {
  const BridgeMcpStateSnapshot_Stale({required this.resource, required this.value}): super._();


 final  BridgeStaleResource resource;
 final  BridgeMcpStateData value;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMcpStateSnapshot_StaleCopyWith<BridgeMcpStateSnapshot_Stale> get copyWith => _$BridgeMcpStateSnapshot_StaleCopyWithImpl<BridgeMcpStateSnapshot_Stale>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpStateSnapshot_Stale&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeMcpStateSnapshot.stale(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeMcpStateSnapshot_StaleCopyWith<$Res> implements $BridgeMcpStateSnapshotCopyWith<$Res> {
  factory $BridgeMcpStateSnapshot_StaleCopyWith(BridgeMcpStateSnapshot_Stale value, $Res Function(BridgeMcpStateSnapshot_Stale) _then) = _$BridgeMcpStateSnapshot_StaleCopyWithImpl;
@useResult
$Res call({
 BridgeStaleResource resource, BridgeMcpStateData value
});




}
/// @nodoc
class _$BridgeMcpStateSnapshot_StaleCopyWithImpl<$Res>
    implements $BridgeMcpStateSnapshot_StaleCopyWith<$Res> {
  _$BridgeMcpStateSnapshot_StaleCopyWithImpl(this._self, this._then);

  final BridgeMcpStateSnapshot_Stale _self;
  final $Res Function(BridgeMcpStateSnapshot_Stale) _then;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeMcpStateSnapshot_Stale(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeStaleResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeMcpStateData,
  ));
}


}

/// @nodoc


class BridgeMcpStateSnapshot_Degraded extends BridgeMcpStateSnapshot {
  const BridgeMcpStateSnapshot_Degraded({required this.resource, required this.value}): super._();


 final  BridgeDegradedResource resource;
 final  BridgeMcpStateData value;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMcpStateSnapshot_DegradedCopyWith<BridgeMcpStateSnapshot_Degraded> get copyWith => _$BridgeMcpStateSnapshot_DegradedCopyWithImpl<BridgeMcpStateSnapshot_Degraded>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpStateSnapshot_Degraded&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeMcpStateSnapshot.degraded(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeMcpStateSnapshot_DegradedCopyWith<$Res> implements $BridgeMcpStateSnapshotCopyWith<$Res> {
  factory $BridgeMcpStateSnapshot_DegradedCopyWith(BridgeMcpStateSnapshot_Degraded value, $Res Function(BridgeMcpStateSnapshot_Degraded) _then) = _$BridgeMcpStateSnapshot_DegradedCopyWithImpl;
@useResult
$Res call({
 BridgeDegradedResource resource, BridgeMcpStateData value
});




}
/// @nodoc
class _$BridgeMcpStateSnapshot_DegradedCopyWithImpl<$Res>
    implements $BridgeMcpStateSnapshot_DegradedCopyWith<$Res> {
  _$BridgeMcpStateSnapshot_DegradedCopyWithImpl(this._self, this._then);

  final BridgeMcpStateSnapshot_Degraded _self;
  final $Res Function(BridgeMcpStateSnapshot_Degraded) _then;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeMcpStateSnapshot_Degraded(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeDegradedResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeMcpStateData,
  ));
}


}

/// @nodoc


class BridgeMcpStateSnapshot_Failed extends BridgeMcpStateSnapshot {
  const BridgeMcpStateSnapshot_Failed(this.field0): super._();


 final  BridgeFailedResource field0;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMcpStateSnapshot_FailedCopyWith<BridgeMcpStateSnapshot_Failed> get copyWith => _$BridgeMcpStateSnapshot_FailedCopyWithImpl<BridgeMcpStateSnapshot_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpStateSnapshot_Failed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeMcpStateSnapshot.failed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeMcpStateSnapshot_FailedCopyWith<$Res> implements $BridgeMcpStateSnapshotCopyWith<$Res> {
  factory $BridgeMcpStateSnapshot_FailedCopyWith(BridgeMcpStateSnapshot_Failed value, $Res Function(BridgeMcpStateSnapshot_Failed) _then) = _$BridgeMcpStateSnapshot_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeFailedResource field0
});




}
/// @nodoc
class _$BridgeMcpStateSnapshot_FailedCopyWithImpl<$Res>
    implements $BridgeMcpStateSnapshot_FailedCopyWith<$Res> {
  _$BridgeMcpStateSnapshot_FailedCopyWithImpl(this._self, this._then);

  final BridgeMcpStateSnapshot_Failed _self;
  final $Res Function(BridgeMcpStateSnapshot_Failed) _then;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeMcpStateSnapshot_Failed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeFailedResource,
  ));
}


}

/// @nodoc


class BridgeMcpStateSnapshot_Stopped extends BridgeMcpStateSnapshot {
  const BridgeMcpStateSnapshot_Stopped(this.field0): super._();


 final  BridgeStoppedResource field0;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMcpStateSnapshot_StoppedCopyWith<BridgeMcpStateSnapshot_Stopped> get copyWith => _$BridgeMcpStateSnapshot_StoppedCopyWithImpl<BridgeMcpStateSnapshot_Stopped>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpStateSnapshot_Stopped&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeMcpStateSnapshot.stopped(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeMcpStateSnapshot_StoppedCopyWith<$Res> implements $BridgeMcpStateSnapshotCopyWith<$Res> {
  factory $BridgeMcpStateSnapshot_StoppedCopyWith(BridgeMcpStateSnapshot_Stopped value, $Res Function(BridgeMcpStateSnapshot_Stopped) _then) = _$BridgeMcpStateSnapshot_StoppedCopyWithImpl;
@useResult
$Res call({
 BridgeStoppedResource field0
});




}
/// @nodoc
class _$BridgeMcpStateSnapshot_StoppedCopyWithImpl<$Res>
    implements $BridgeMcpStateSnapshot_StoppedCopyWith<$Res> {
  _$BridgeMcpStateSnapshot_StoppedCopyWithImpl(this._self, this._then);

  final BridgeMcpStateSnapshot_Stopped _self;
  final $Res Function(BridgeMcpStateSnapshot_Stopped) _then;

/// Create a copy of BridgeMcpStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeMcpStateSnapshot_Stopped(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeStoppedResource,
  ));
}


}

/// @nodoc
mixin _$BridgeModelCatalogStateSnapshot {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeModelCatalogStateSnapshot);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeModelCatalogStateSnapshot()';
}


}

/// @nodoc
class $BridgeModelCatalogStateSnapshotCopyWith<$Res>  {
$BridgeModelCatalogStateSnapshotCopyWith(BridgeModelCatalogStateSnapshot _, $Res Function(BridgeModelCatalogStateSnapshot) __);
}


/// Adds pattern-matching-related methods to [BridgeModelCatalogStateSnapshot].
extension BridgeModelCatalogStateSnapshotPatterns on BridgeModelCatalogStateSnapshot {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeModelCatalogStateSnapshot_Uninitialized value)?  uninitialized,TResult Function( BridgeModelCatalogStateSnapshot_Loading value)?  loading,TResult Function( BridgeModelCatalogStateSnapshot_Ready value)?  ready,TResult Function( BridgeModelCatalogStateSnapshot_Refreshing value)?  refreshing,TResult Function( BridgeModelCatalogStateSnapshot_Stale value)?  stale,TResult Function( BridgeModelCatalogStateSnapshot_Degraded value)?  degraded,TResult Function( BridgeModelCatalogStateSnapshot_Failed value)?  failed,TResult Function( BridgeModelCatalogStateSnapshot_Stopped value)?  stopped,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeModelCatalogStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeModelCatalogStateSnapshot_Loading() when loading != null:
return loading(_that);case BridgeModelCatalogStateSnapshot_Ready() when ready != null:
return ready(_that);case BridgeModelCatalogStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeModelCatalogStateSnapshot_Stale() when stale != null:
return stale(_that);case BridgeModelCatalogStateSnapshot_Degraded() when degraded != null:
return degraded(_that);case BridgeModelCatalogStateSnapshot_Failed() when failed != null:
return failed(_that);case BridgeModelCatalogStateSnapshot_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeModelCatalogStateSnapshot_Uninitialized value)  uninitialized,required TResult Function( BridgeModelCatalogStateSnapshot_Loading value)  loading,required TResult Function( BridgeModelCatalogStateSnapshot_Ready value)  ready,required TResult Function( BridgeModelCatalogStateSnapshot_Refreshing value)  refreshing,required TResult Function( BridgeModelCatalogStateSnapshot_Stale value)  stale,required TResult Function( BridgeModelCatalogStateSnapshot_Degraded value)  degraded,required TResult Function( BridgeModelCatalogStateSnapshot_Failed value)  failed,required TResult Function( BridgeModelCatalogStateSnapshot_Stopped value)  stopped,}){
final _that = this;
switch (_that) {
case BridgeModelCatalogStateSnapshot_Uninitialized():
return uninitialized(_that);case BridgeModelCatalogStateSnapshot_Loading():
return loading(_that);case BridgeModelCatalogStateSnapshot_Ready():
return ready(_that);case BridgeModelCatalogStateSnapshot_Refreshing():
return refreshing(_that);case BridgeModelCatalogStateSnapshot_Stale():
return stale(_that);case BridgeModelCatalogStateSnapshot_Degraded():
return degraded(_that);case BridgeModelCatalogStateSnapshot_Failed():
return failed(_that);case BridgeModelCatalogStateSnapshot_Stopped():
return stopped(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeModelCatalogStateSnapshot_Uninitialized value)?  uninitialized,TResult? Function( BridgeModelCatalogStateSnapshot_Loading value)?  loading,TResult? Function( BridgeModelCatalogStateSnapshot_Ready value)?  ready,TResult? Function( BridgeModelCatalogStateSnapshot_Refreshing value)?  refreshing,TResult? Function( BridgeModelCatalogStateSnapshot_Stale value)?  stale,TResult? Function( BridgeModelCatalogStateSnapshot_Degraded value)?  degraded,TResult? Function( BridgeModelCatalogStateSnapshot_Failed value)?  failed,TResult? Function( BridgeModelCatalogStateSnapshot_Stopped value)?  stopped,}){
final _that = this;
switch (_that) {
case BridgeModelCatalogStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeModelCatalogStateSnapshot_Loading() when loading != null:
return loading(_that);case BridgeModelCatalogStateSnapshot_Ready() when ready != null:
return ready(_that);case BridgeModelCatalogStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeModelCatalogStateSnapshot_Stale() when stale != null:
return stale(_that);case BridgeModelCatalogStateSnapshot_Degraded() when degraded != null:
return degraded(_that);case BridgeModelCatalogStateSnapshot_Failed() when failed != null:
return failed(_that);case BridgeModelCatalogStateSnapshot_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeUninitializedResource field0)?  uninitialized,TResult Function( BridgeLoadingResource field0)?  loading,TResult Function( BridgeReadyResource resource,  BridgeModelCatalogStateData value)?  ready,TResult Function( BridgeRefreshingResource resource,  BridgeModelCatalogStateData value)?  refreshing,TResult Function( BridgeStaleResource resource,  BridgeModelCatalogStateData value)?  stale,TResult Function( BridgeDegradedResource resource,  BridgeModelCatalogStateData value)?  degraded,TResult Function( BridgeFailedResource field0)?  failed,TResult Function( BridgeStoppedResource field0)?  stopped,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeModelCatalogStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeModelCatalogStateSnapshot_Loading() when loading != null:
return loading(_that.field0);case BridgeModelCatalogStateSnapshot_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeModelCatalogStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeModelCatalogStateSnapshot_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeModelCatalogStateSnapshot_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeModelCatalogStateSnapshot_Failed() when failed != null:
return failed(_that.field0);case BridgeModelCatalogStateSnapshot_Stopped() when stopped != null:
return stopped(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeUninitializedResource field0)  uninitialized,required TResult Function( BridgeLoadingResource field0)  loading,required TResult Function( BridgeReadyResource resource,  BridgeModelCatalogStateData value)  ready,required TResult Function( BridgeRefreshingResource resource,  BridgeModelCatalogStateData value)  refreshing,required TResult Function( BridgeStaleResource resource,  BridgeModelCatalogStateData value)  stale,required TResult Function( BridgeDegradedResource resource,  BridgeModelCatalogStateData value)  degraded,required TResult Function( BridgeFailedResource field0)  failed,required TResult Function( BridgeStoppedResource field0)  stopped,}) {final _that = this;
switch (_that) {
case BridgeModelCatalogStateSnapshot_Uninitialized():
return uninitialized(_that.field0);case BridgeModelCatalogStateSnapshot_Loading():
return loading(_that.field0);case BridgeModelCatalogStateSnapshot_Ready():
return ready(_that.resource,_that.value);case BridgeModelCatalogStateSnapshot_Refreshing():
return refreshing(_that.resource,_that.value);case BridgeModelCatalogStateSnapshot_Stale():
return stale(_that.resource,_that.value);case BridgeModelCatalogStateSnapshot_Degraded():
return degraded(_that.resource,_that.value);case BridgeModelCatalogStateSnapshot_Failed():
return failed(_that.field0);case BridgeModelCatalogStateSnapshot_Stopped():
return stopped(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeUninitializedResource field0)?  uninitialized,TResult? Function( BridgeLoadingResource field0)?  loading,TResult? Function( BridgeReadyResource resource,  BridgeModelCatalogStateData value)?  ready,TResult? Function( BridgeRefreshingResource resource,  BridgeModelCatalogStateData value)?  refreshing,TResult? Function( BridgeStaleResource resource,  BridgeModelCatalogStateData value)?  stale,TResult? Function( BridgeDegradedResource resource,  BridgeModelCatalogStateData value)?  degraded,TResult? Function( BridgeFailedResource field0)?  failed,TResult? Function( BridgeStoppedResource field0)?  stopped,}) {final _that = this;
switch (_that) {
case BridgeModelCatalogStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeModelCatalogStateSnapshot_Loading() when loading != null:
return loading(_that.field0);case BridgeModelCatalogStateSnapshot_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeModelCatalogStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeModelCatalogStateSnapshot_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeModelCatalogStateSnapshot_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeModelCatalogStateSnapshot_Failed() when failed != null:
return failed(_that.field0);case BridgeModelCatalogStateSnapshot_Stopped() when stopped != null:
return stopped(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeModelCatalogStateSnapshot_Uninitialized extends BridgeModelCatalogStateSnapshot {
  const BridgeModelCatalogStateSnapshot_Uninitialized(this.field0): super._();


 final  BridgeUninitializedResource field0;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeModelCatalogStateSnapshot_UninitializedCopyWith<BridgeModelCatalogStateSnapshot_Uninitialized> get copyWith => _$BridgeModelCatalogStateSnapshot_UninitializedCopyWithImpl<BridgeModelCatalogStateSnapshot_Uninitialized>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeModelCatalogStateSnapshot_Uninitialized&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeModelCatalogStateSnapshot.uninitialized(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeModelCatalogStateSnapshot_UninitializedCopyWith<$Res> implements $BridgeModelCatalogStateSnapshotCopyWith<$Res> {
  factory $BridgeModelCatalogStateSnapshot_UninitializedCopyWith(BridgeModelCatalogStateSnapshot_Uninitialized value, $Res Function(BridgeModelCatalogStateSnapshot_Uninitialized) _then) = _$BridgeModelCatalogStateSnapshot_UninitializedCopyWithImpl;
@useResult
$Res call({
 BridgeUninitializedResource field0
});




}
/// @nodoc
class _$BridgeModelCatalogStateSnapshot_UninitializedCopyWithImpl<$Res>
    implements $BridgeModelCatalogStateSnapshot_UninitializedCopyWith<$Res> {
  _$BridgeModelCatalogStateSnapshot_UninitializedCopyWithImpl(this._self, this._then);

  final BridgeModelCatalogStateSnapshot_Uninitialized _self;
  final $Res Function(BridgeModelCatalogStateSnapshot_Uninitialized) _then;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeModelCatalogStateSnapshot_Uninitialized(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeUninitializedResource,
  ));
}


}

/// @nodoc


class BridgeModelCatalogStateSnapshot_Loading extends BridgeModelCatalogStateSnapshot {
  const BridgeModelCatalogStateSnapshot_Loading(this.field0): super._();


 final  BridgeLoadingResource field0;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeModelCatalogStateSnapshot_LoadingCopyWith<BridgeModelCatalogStateSnapshot_Loading> get copyWith => _$BridgeModelCatalogStateSnapshot_LoadingCopyWithImpl<BridgeModelCatalogStateSnapshot_Loading>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeModelCatalogStateSnapshot_Loading&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeModelCatalogStateSnapshot.loading(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeModelCatalogStateSnapshot_LoadingCopyWith<$Res> implements $BridgeModelCatalogStateSnapshotCopyWith<$Res> {
  factory $BridgeModelCatalogStateSnapshot_LoadingCopyWith(BridgeModelCatalogStateSnapshot_Loading value, $Res Function(BridgeModelCatalogStateSnapshot_Loading) _then) = _$BridgeModelCatalogStateSnapshot_LoadingCopyWithImpl;
@useResult
$Res call({
 BridgeLoadingResource field0
});




}
/// @nodoc
class _$BridgeModelCatalogStateSnapshot_LoadingCopyWithImpl<$Res>
    implements $BridgeModelCatalogStateSnapshot_LoadingCopyWith<$Res> {
  _$BridgeModelCatalogStateSnapshot_LoadingCopyWithImpl(this._self, this._then);

  final BridgeModelCatalogStateSnapshot_Loading _self;
  final $Res Function(BridgeModelCatalogStateSnapshot_Loading) _then;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeModelCatalogStateSnapshot_Loading(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeLoadingResource,
  ));
}


}

/// @nodoc


class BridgeModelCatalogStateSnapshot_Ready extends BridgeModelCatalogStateSnapshot {
  const BridgeModelCatalogStateSnapshot_Ready({required this.resource, required this.value}): super._();


 final  BridgeReadyResource resource;
 final  BridgeModelCatalogStateData value;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeModelCatalogStateSnapshot_ReadyCopyWith<BridgeModelCatalogStateSnapshot_Ready> get copyWith => _$BridgeModelCatalogStateSnapshot_ReadyCopyWithImpl<BridgeModelCatalogStateSnapshot_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeModelCatalogStateSnapshot_Ready&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeModelCatalogStateSnapshot.ready(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeModelCatalogStateSnapshot_ReadyCopyWith<$Res> implements $BridgeModelCatalogStateSnapshotCopyWith<$Res> {
  factory $BridgeModelCatalogStateSnapshot_ReadyCopyWith(BridgeModelCatalogStateSnapshot_Ready value, $Res Function(BridgeModelCatalogStateSnapshot_Ready) _then) = _$BridgeModelCatalogStateSnapshot_ReadyCopyWithImpl;
@useResult
$Res call({
 BridgeReadyResource resource, BridgeModelCatalogStateData value
});




}
/// @nodoc
class _$BridgeModelCatalogStateSnapshot_ReadyCopyWithImpl<$Res>
    implements $BridgeModelCatalogStateSnapshot_ReadyCopyWith<$Res> {
  _$BridgeModelCatalogStateSnapshot_ReadyCopyWithImpl(this._self, this._then);

  final BridgeModelCatalogStateSnapshot_Ready _self;
  final $Res Function(BridgeModelCatalogStateSnapshot_Ready) _then;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeModelCatalogStateSnapshot_Ready(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeReadyResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeModelCatalogStateData,
  ));
}


}

/// @nodoc


class BridgeModelCatalogStateSnapshot_Refreshing extends BridgeModelCatalogStateSnapshot {
  const BridgeModelCatalogStateSnapshot_Refreshing({required this.resource, required this.value}): super._();


 final  BridgeRefreshingResource resource;
 final  BridgeModelCatalogStateData value;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeModelCatalogStateSnapshot_RefreshingCopyWith<BridgeModelCatalogStateSnapshot_Refreshing> get copyWith => _$BridgeModelCatalogStateSnapshot_RefreshingCopyWithImpl<BridgeModelCatalogStateSnapshot_Refreshing>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeModelCatalogStateSnapshot_Refreshing&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeModelCatalogStateSnapshot.refreshing(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeModelCatalogStateSnapshot_RefreshingCopyWith<$Res> implements $BridgeModelCatalogStateSnapshotCopyWith<$Res> {
  factory $BridgeModelCatalogStateSnapshot_RefreshingCopyWith(BridgeModelCatalogStateSnapshot_Refreshing value, $Res Function(BridgeModelCatalogStateSnapshot_Refreshing) _then) = _$BridgeModelCatalogStateSnapshot_RefreshingCopyWithImpl;
@useResult
$Res call({
 BridgeRefreshingResource resource, BridgeModelCatalogStateData value
});




}
/// @nodoc
class _$BridgeModelCatalogStateSnapshot_RefreshingCopyWithImpl<$Res>
    implements $BridgeModelCatalogStateSnapshot_RefreshingCopyWith<$Res> {
  _$BridgeModelCatalogStateSnapshot_RefreshingCopyWithImpl(this._self, this._then);

  final BridgeModelCatalogStateSnapshot_Refreshing _self;
  final $Res Function(BridgeModelCatalogStateSnapshot_Refreshing) _then;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeModelCatalogStateSnapshot_Refreshing(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeRefreshingResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeModelCatalogStateData,
  ));
}


}

/// @nodoc


class BridgeModelCatalogStateSnapshot_Stale extends BridgeModelCatalogStateSnapshot {
  const BridgeModelCatalogStateSnapshot_Stale({required this.resource, required this.value}): super._();


 final  BridgeStaleResource resource;
 final  BridgeModelCatalogStateData value;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeModelCatalogStateSnapshot_StaleCopyWith<BridgeModelCatalogStateSnapshot_Stale> get copyWith => _$BridgeModelCatalogStateSnapshot_StaleCopyWithImpl<BridgeModelCatalogStateSnapshot_Stale>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeModelCatalogStateSnapshot_Stale&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeModelCatalogStateSnapshot.stale(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeModelCatalogStateSnapshot_StaleCopyWith<$Res> implements $BridgeModelCatalogStateSnapshotCopyWith<$Res> {
  factory $BridgeModelCatalogStateSnapshot_StaleCopyWith(BridgeModelCatalogStateSnapshot_Stale value, $Res Function(BridgeModelCatalogStateSnapshot_Stale) _then) = _$BridgeModelCatalogStateSnapshot_StaleCopyWithImpl;
@useResult
$Res call({
 BridgeStaleResource resource, BridgeModelCatalogStateData value
});




}
/// @nodoc
class _$BridgeModelCatalogStateSnapshot_StaleCopyWithImpl<$Res>
    implements $BridgeModelCatalogStateSnapshot_StaleCopyWith<$Res> {
  _$BridgeModelCatalogStateSnapshot_StaleCopyWithImpl(this._self, this._then);

  final BridgeModelCatalogStateSnapshot_Stale _self;
  final $Res Function(BridgeModelCatalogStateSnapshot_Stale) _then;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeModelCatalogStateSnapshot_Stale(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeStaleResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeModelCatalogStateData,
  ));
}


}

/// @nodoc


class BridgeModelCatalogStateSnapshot_Degraded extends BridgeModelCatalogStateSnapshot {
  const BridgeModelCatalogStateSnapshot_Degraded({required this.resource, required this.value}): super._();


 final  BridgeDegradedResource resource;
 final  BridgeModelCatalogStateData value;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeModelCatalogStateSnapshot_DegradedCopyWith<BridgeModelCatalogStateSnapshot_Degraded> get copyWith => _$BridgeModelCatalogStateSnapshot_DegradedCopyWithImpl<BridgeModelCatalogStateSnapshot_Degraded>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeModelCatalogStateSnapshot_Degraded&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeModelCatalogStateSnapshot.degraded(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeModelCatalogStateSnapshot_DegradedCopyWith<$Res> implements $BridgeModelCatalogStateSnapshotCopyWith<$Res> {
  factory $BridgeModelCatalogStateSnapshot_DegradedCopyWith(BridgeModelCatalogStateSnapshot_Degraded value, $Res Function(BridgeModelCatalogStateSnapshot_Degraded) _then) = _$BridgeModelCatalogStateSnapshot_DegradedCopyWithImpl;
@useResult
$Res call({
 BridgeDegradedResource resource, BridgeModelCatalogStateData value
});




}
/// @nodoc
class _$BridgeModelCatalogStateSnapshot_DegradedCopyWithImpl<$Res>
    implements $BridgeModelCatalogStateSnapshot_DegradedCopyWith<$Res> {
  _$BridgeModelCatalogStateSnapshot_DegradedCopyWithImpl(this._self, this._then);

  final BridgeModelCatalogStateSnapshot_Degraded _self;
  final $Res Function(BridgeModelCatalogStateSnapshot_Degraded) _then;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeModelCatalogStateSnapshot_Degraded(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeDegradedResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeModelCatalogStateData,
  ));
}


}

/// @nodoc


class BridgeModelCatalogStateSnapshot_Failed extends BridgeModelCatalogStateSnapshot {
  const BridgeModelCatalogStateSnapshot_Failed(this.field0): super._();


 final  BridgeFailedResource field0;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeModelCatalogStateSnapshot_FailedCopyWith<BridgeModelCatalogStateSnapshot_Failed> get copyWith => _$BridgeModelCatalogStateSnapshot_FailedCopyWithImpl<BridgeModelCatalogStateSnapshot_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeModelCatalogStateSnapshot_Failed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeModelCatalogStateSnapshot.failed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeModelCatalogStateSnapshot_FailedCopyWith<$Res> implements $BridgeModelCatalogStateSnapshotCopyWith<$Res> {
  factory $BridgeModelCatalogStateSnapshot_FailedCopyWith(BridgeModelCatalogStateSnapshot_Failed value, $Res Function(BridgeModelCatalogStateSnapshot_Failed) _then) = _$BridgeModelCatalogStateSnapshot_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeFailedResource field0
});




}
/// @nodoc
class _$BridgeModelCatalogStateSnapshot_FailedCopyWithImpl<$Res>
    implements $BridgeModelCatalogStateSnapshot_FailedCopyWith<$Res> {
  _$BridgeModelCatalogStateSnapshot_FailedCopyWithImpl(this._self, this._then);

  final BridgeModelCatalogStateSnapshot_Failed _self;
  final $Res Function(BridgeModelCatalogStateSnapshot_Failed) _then;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeModelCatalogStateSnapshot_Failed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeFailedResource,
  ));
}


}

/// @nodoc


class BridgeModelCatalogStateSnapshot_Stopped extends BridgeModelCatalogStateSnapshot {
  const BridgeModelCatalogStateSnapshot_Stopped(this.field0): super._();


 final  BridgeStoppedResource field0;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeModelCatalogStateSnapshot_StoppedCopyWith<BridgeModelCatalogStateSnapshot_Stopped> get copyWith => _$BridgeModelCatalogStateSnapshot_StoppedCopyWithImpl<BridgeModelCatalogStateSnapshot_Stopped>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeModelCatalogStateSnapshot_Stopped&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeModelCatalogStateSnapshot.stopped(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeModelCatalogStateSnapshot_StoppedCopyWith<$Res> implements $BridgeModelCatalogStateSnapshotCopyWith<$Res> {
  factory $BridgeModelCatalogStateSnapshot_StoppedCopyWith(BridgeModelCatalogStateSnapshot_Stopped value, $Res Function(BridgeModelCatalogStateSnapshot_Stopped) _then) = _$BridgeModelCatalogStateSnapshot_StoppedCopyWithImpl;
@useResult
$Res call({
 BridgeStoppedResource field0
});




}
/// @nodoc
class _$BridgeModelCatalogStateSnapshot_StoppedCopyWithImpl<$Res>
    implements $BridgeModelCatalogStateSnapshot_StoppedCopyWith<$Res> {
  _$BridgeModelCatalogStateSnapshot_StoppedCopyWithImpl(this._self, this._then);

  final BridgeModelCatalogStateSnapshot_Stopped _self;
  final $Res Function(BridgeModelCatalogStateSnapshot_Stopped) _then;

/// Create a copy of BridgeModelCatalogStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeModelCatalogStateSnapshot_Stopped(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeStoppedResource,
  ));
}


}

/// @nodoc
mixin _$BridgePersistenceState {

 BigInt get pendingCommits;
/// Create a copy of BridgePersistenceState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgePersistenceStateCopyWith<BridgePersistenceState> get copyWith => _$BridgePersistenceStateCopyWithImpl<BridgePersistenceState>(this as BridgePersistenceState, _$identity);



@override
bool operator ==(Object other) {
  final _this = this as BridgePersistenceState;
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgePersistenceState&&(identical(other.pendingCommits, _this.pendingCommits) || other.pendingCommits == _this.pendingCommits));
}


@override
int get hashCode {
  final _this = this as BridgePersistenceState;
  return Object.hash(runtimeType,_this.pendingCommits);
}

@override
String toString() {
  final _this = this as BridgePersistenceState;
  return 'BridgePersistenceState(pendingCommits: ${_this.pendingCommits})';
}


}

/// @nodoc
abstract mixin class $BridgePersistenceStateCopyWith<$Res>  {
  factory $BridgePersistenceStateCopyWith(BridgePersistenceState value, $Res Function(BridgePersistenceState) _then) = _$BridgePersistenceStateCopyWithImpl;
@useResult
$Res call({
 BigInt pendingCommits
});




}
/// @nodoc
class _$BridgePersistenceStateCopyWithImpl<$Res>
    implements $BridgePersistenceStateCopyWith<$Res> {
  _$BridgePersistenceStateCopyWithImpl(this._self, this._then);

  final BridgePersistenceState _self;
  final $Res Function(BridgePersistenceState) _then;

/// Create a copy of BridgePersistenceState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? pendingCommits = null,}) {
  return _then(_self.copyWith(
pendingCommits: null == pendingCommits ? _self.pendingCommits : pendingCommits // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}

}


/// Adds pattern-matching-related methods to [BridgePersistenceState].
extension BridgePersistenceStatePatterns on BridgePersistenceState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgePersistenceState_Ready value)?  ready,TResult Function( BridgePersistenceState_Flushing value)?  flushing,TResult Function( BridgePersistenceState_Degraded value)?  degraded,TResult Function( BridgePersistenceState_Recovering value)?  recovering,TResult Function( BridgePersistenceState_Blocked value)?  blocked,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgePersistenceState_Ready() when ready != null:
return ready(_that);case BridgePersistenceState_Flushing() when flushing != null:
return flushing(_that);case BridgePersistenceState_Degraded() when degraded != null:
return degraded(_that);case BridgePersistenceState_Recovering() when recovering != null:
return recovering(_that);case BridgePersistenceState_Blocked() when blocked != null:
return blocked(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgePersistenceState_Ready value)  ready,required TResult Function( BridgePersistenceState_Flushing value)  flushing,required TResult Function( BridgePersistenceState_Degraded value)  degraded,required TResult Function( BridgePersistenceState_Recovering value)  recovering,required TResult Function( BridgePersistenceState_Blocked value)  blocked,}){
final _that = this;
switch (_that) {
case BridgePersistenceState_Ready():
return ready(_that);case BridgePersistenceState_Flushing():
return flushing(_that);case BridgePersistenceState_Degraded():
return degraded(_that);case BridgePersistenceState_Recovering():
return recovering(_that);case BridgePersistenceState_Blocked():
return blocked(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgePersistenceState_Ready value)?  ready,TResult? Function( BridgePersistenceState_Flushing value)?  flushing,TResult? Function( BridgePersistenceState_Degraded value)?  degraded,TResult? Function( BridgePersistenceState_Recovering value)?  recovering,TResult? Function( BridgePersistenceState_Blocked value)?  blocked,}){
final _that = this;
switch (_that) {
case BridgePersistenceState_Ready() when ready != null:
return ready(_that);case BridgePersistenceState_Flushing() when flushing != null:
return flushing(_that);case BridgePersistenceState_Degraded() when degraded != null:
return degraded(_that);case BridgePersistenceState_Recovering() when recovering != null:
return recovering(_that);case BridgePersistenceState_Blocked() when blocked != null:
return blocked(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BigInt pendingCommits)?  ready,TResult Function( BigInt pendingCommits,  BigInt? oldestPendingRevision)?  flushing,TResult Function( BigInt pendingCommits,  BigInt? oldestPendingRevision,  PlatformInt64 firstFailedAt,  BridgeStateError error)?  degraded,TResult Function( BigInt pendingCommits,  BigInt? oldestPendingRevision,  PlatformInt64 firstFailedAt)?  recovering,TResult Function( BigInt pendingCommits,  BigInt? oldestPendingRevision,  PlatformInt64 firstFailedAt,  BridgeStateError error)?  blocked,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgePersistenceState_Ready() when ready != null:
return ready(_that.pendingCommits);case BridgePersistenceState_Flushing() when flushing != null:
return flushing(_that.pendingCommits,_that.oldestPendingRevision);case BridgePersistenceState_Degraded() when degraded != null:
return degraded(_that.pendingCommits,_that.oldestPendingRevision,_that.firstFailedAt,_that.error);case BridgePersistenceState_Recovering() when recovering != null:
return recovering(_that.pendingCommits,_that.oldestPendingRevision,_that.firstFailedAt);case BridgePersistenceState_Blocked() when blocked != null:
return blocked(_that.pendingCommits,_that.oldestPendingRevision,_that.firstFailedAt,_that.error);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BigInt pendingCommits)  ready,required TResult Function( BigInt pendingCommits,  BigInt? oldestPendingRevision)  flushing,required TResult Function( BigInt pendingCommits,  BigInt? oldestPendingRevision,  PlatformInt64 firstFailedAt,  BridgeStateError error)  degraded,required TResult Function( BigInt pendingCommits,  BigInt? oldestPendingRevision,  PlatformInt64 firstFailedAt)  recovering,required TResult Function( BigInt pendingCommits,  BigInt? oldestPendingRevision,  PlatformInt64 firstFailedAt,  BridgeStateError error)  blocked,}) {final _that = this;
switch (_that) {
case BridgePersistenceState_Ready():
return ready(_that.pendingCommits);case BridgePersistenceState_Flushing():
return flushing(_that.pendingCommits,_that.oldestPendingRevision);case BridgePersistenceState_Degraded():
return degraded(_that.pendingCommits,_that.oldestPendingRevision,_that.firstFailedAt,_that.error);case BridgePersistenceState_Recovering():
return recovering(_that.pendingCommits,_that.oldestPendingRevision,_that.firstFailedAt);case BridgePersistenceState_Blocked():
return blocked(_that.pendingCommits,_that.oldestPendingRevision,_that.firstFailedAt,_that.error);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BigInt pendingCommits)?  ready,TResult? Function( BigInt pendingCommits,  BigInt? oldestPendingRevision)?  flushing,TResult? Function( BigInt pendingCommits,  BigInt? oldestPendingRevision,  PlatformInt64 firstFailedAt,  BridgeStateError error)?  degraded,TResult? Function( BigInt pendingCommits,  BigInt? oldestPendingRevision,  PlatformInt64 firstFailedAt)?  recovering,TResult? Function( BigInt pendingCommits,  BigInt? oldestPendingRevision,  PlatformInt64 firstFailedAt,  BridgeStateError error)?  blocked,}) {final _that = this;
switch (_that) {
case BridgePersistenceState_Ready() when ready != null:
return ready(_that.pendingCommits);case BridgePersistenceState_Flushing() when flushing != null:
return flushing(_that.pendingCommits,_that.oldestPendingRevision);case BridgePersistenceState_Degraded() when degraded != null:
return degraded(_that.pendingCommits,_that.oldestPendingRevision,_that.firstFailedAt,_that.error);case BridgePersistenceState_Recovering() when recovering != null:
return recovering(_that.pendingCommits,_that.oldestPendingRevision,_that.firstFailedAt);case BridgePersistenceState_Blocked() when blocked != null:
return blocked(_that.pendingCommits,_that.oldestPendingRevision,_that.firstFailedAt,_that.error);case _:
  return null;

}
}

}

/// @nodoc


class BridgePersistenceState_Ready extends BridgePersistenceState {
  const BridgePersistenceState_Ready({required this.pendingCommits}): super._();


@override final  BigInt pendingCommits;

/// Create a copy of BridgePersistenceState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgePersistenceState_ReadyCopyWith<BridgePersistenceState_Ready> get copyWith => _$BridgePersistenceState_ReadyCopyWithImpl<BridgePersistenceState_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgePersistenceState_Ready&&(identical(other.pendingCommits, pendingCommits) || other.pendingCommits == pendingCommits));
}


@override
int get hashCode {
    return Object.hash(runtimeType,pendingCommits);
}

@override
String toString() {
    return 'BridgePersistenceState.ready(pendingCommits: $pendingCommits)';
}


}

/// @nodoc
abstract mixin class $BridgePersistenceState_ReadyCopyWith<$Res> implements $BridgePersistenceStateCopyWith<$Res> {
  factory $BridgePersistenceState_ReadyCopyWith(BridgePersistenceState_Ready value, $Res Function(BridgePersistenceState_Ready) _then) = _$BridgePersistenceState_ReadyCopyWithImpl;
@override @useResult
$Res call({
 BigInt pendingCommits
});




}
/// @nodoc
class _$BridgePersistenceState_ReadyCopyWithImpl<$Res>
    implements $BridgePersistenceState_ReadyCopyWith<$Res> {
  _$BridgePersistenceState_ReadyCopyWithImpl(this._self, this._then);

  final BridgePersistenceState_Ready _self;
  final $Res Function(BridgePersistenceState_Ready) _then;

/// Create a copy of BridgePersistenceState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? pendingCommits = null,}) {
  return _then(BridgePersistenceState_Ready(
pendingCommits: null == pendingCommits ? _self.pendingCommits : pendingCommits // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

/// @nodoc


class BridgePersistenceState_Flushing extends BridgePersistenceState {
  const BridgePersistenceState_Flushing({required this.pendingCommits, this.oldestPendingRevision}): super._();


@override final  BigInt pendingCommits;
 final  BigInt? oldestPendingRevision;

/// Create a copy of BridgePersistenceState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgePersistenceState_FlushingCopyWith<BridgePersistenceState_Flushing> get copyWith => _$BridgePersistenceState_FlushingCopyWithImpl<BridgePersistenceState_Flushing>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgePersistenceState_Flushing&&(identical(other.pendingCommits, pendingCommits) || other.pendingCommits == pendingCommits)&&(identical(other.oldestPendingRevision, oldestPendingRevision) || other.oldestPendingRevision == oldestPendingRevision));
}


@override
int get hashCode {
    return Object.hash(runtimeType,pendingCommits,oldestPendingRevision);
}

@override
String toString() {
    return 'BridgePersistenceState.flushing(pendingCommits: $pendingCommits, oldestPendingRevision: $oldestPendingRevision)';
}


}

/// @nodoc
abstract mixin class $BridgePersistenceState_FlushingCopyWith<$Res> implements $BridgePersistenceStateCopyWith<$Res> {
  factory $BridgePersistenceState_FlushingCopyWith(BridgePersistenceState_Flushing value, $Res Function(BridgePersistenceState_Flushing) _then) = _$BridgePersistenceState_FlushingCopyWithImpl;
@override @useResult
$Res call({
 BigInt pendingCommits, BigInt? oldestPendingRevision
});




}
/// @nodoc
class _$BridgePersistenceState_FlushingCopyWithImpl<$Res>
    implements $BridgePersistenceState_FlushingCopyWith<$Res> {
  _$BridgePersistenceState_FlushingCopyWithImpl(this._self, this._then);

  final BridgePersistenceState_Flushing _self;
  final $Res Function(BridgePersistenceState_Flushing) _then;

/// Create a copy of BridgePersistenceState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? pendingCommits = null,Object? oldestPendingRevision = freezed,}) {
  return _then(BridgePersistenceState_Flushing(
pendingCommits: null == pendingCommits ? _self.pendingCommits : pendingCommits // ignore: cast_nullable_to_non_nullable
as BigInt,oldestPendingRevision: freezed == oldestPendingRevision ? _self.oldestPendingRevision : oldestPendingRevision // ignore: cast_nullable_to_non_nullable
as BigInt?,
  ));
}


}

/// @nodoc


class BridgePersistenceState_Degraded extends BridgePersistenceState {
  const BridgePersistenceState_Degraded({required this.pendingCommits, this.oldestPendingRevision, required this.firstFailedAt, required this.error}): super._();


@override final  BigInt pendingCommits;
 final  BigInt? oldestPendingRevision;
 final  PlatformInt64 firstFailedAt;
 final  BridgeStateError error;

/// Create a copy of BridgePersistenceState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgePersistenceState_DegradedCopyWith<BridgePersistenceState_Degraded> get copyWith => _$BridgePersistenceState_DegradedCopyWithImpl<BridgePersistenceState_Degraded>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgePersistenceState_Degraded&&(identical(other.pendingCommits, pendingCommits) || other.pendingCommits == pendingCommits)&&(identical(other.oldestPendingRevision, oldestPendingRevision) || other.oldestPendingRevision == oldestPendingRevision)&&(identical(other.firstFailedAt, firstFailedAt) || other.firstFailedAt == firstFailedAt)&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode {
    return Object.hash(runtimeType,pendingCommits,oldestPendingRevision,firstFailedAt,error);
}

@override
String toString() {
    return 'BridgePersistenceState.degraded(pendingCommits: $pendingCommits, oldestPendingRevision: $oldestPendingRevision, firstFailedAt: $firstFailedAt, error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgePersistenceState_DegradedCopyWith<$Res> implements $BridgePersistenceStateCopyWith<$Res> {
  factory $BridgePersistenceState_DegradedCopyWith(BridgePersistenceState_Degraded value, $Res Function(BridgePersistenceState_Degraded) _then) = _$BridgePersistenceState_DegradedCopyWithImpl;
@override @useResult
$Res call({
 BigInt pendingCommits, BigInt? oldestPendingRevision, PlatformInt64 firstFailedAt, BridgeStateError error
});




}
/// @nodoc
class _$BridgePersistenceState_DegradedCopyWithImpl<$Res>
    implements $BridgePersistenceState_DegradedCopyWith<$Res> {
  _$BridgePersistenceState_DegradedCopyWithImpl(this._self, this._then);

  final BridgePersistenceState_Degraded _self;
  final $Res Function(BridgePersistenceState_Degraded) _then;

/// Create a copy of BridgePersistenceState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? pendingCommits = null,Object? oldestPendingRevision = freezed,Object? firstFailedAt = null,Object? error = null,}) {
  return _then(BridgePersistenceState_Degraded(
pendingCommits: null == pendingCommits ? _self.pendingCommits : pendingCommits // ignore: cast_nullable_to_non_nullable
as BigInt,oldestPendingRevision: freezed == oldestPendingRevision ? _self.oldestPendingRevision : oldestPendingRevision // ignore: cast_nullable_to_non_nullable
as BigInt?,firstFailedAt: null == firstFailedAt ? _self.firstFailedAt : firstFailedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as BridgeStateError,
  ));
}


}

/// @nodoc


class BridgePersistenceState_Recovering extends BridgePersistenceState {
  const BridgePersistenceState_Recovering({required this.pendingCommits, this.oldestPendingRevision, required this.firstFailedAt}): super._();


@override final  BigInt pendingCommits;
 final  BigInt? oldestPendingRevision;
 final  PlatformInt64 firstFailedAt;

/// Create a copy of BridgePersistenceState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgePersistenceState_RecoveringCopyWith<BridgePersistenceState_Recovering> get copyWith => _$BridgePersistenceState_RecoveringCopyWithImpl<BridgePersistenceState_Recovering>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgePersistenceState_Recovering&&(identical(other.pendingCommits, pendingCommits) || other.pendingCommits == pendingCommits)&&(identical(other.oldestPendingRevision, oldestPendingRevision) || other.oldestPendingRevision == oldestPendingRevision)&&(identical(other.firstFailedAt, firstFailedAt) || other.firstFailedAt == firstFailedAt));
}


@override
int get hashCode {
    return Object.hash(runtimeType,pendingCommits,oldestPendingRevision,firstFailedAt);
}

@override
String toString() {
    return 'BridgePersistenceState.recovering(pendingCommits: $pendingCommits, oldestPendingRevision: $oldestPendingRevision, firstFailedAt: $firstFailedAt)';
}


}

/// @nodoc
abstract mixin class $BridgePersistenceState_RecoveringCopyWith<$Res> implements $BridgePersistenceStateCopyWith<$Res> {
  factory $BridgePersistenceState_RecoveringCopyWith(BridgePersistenceState_Recovering value, $Res Function(BridgePersistenceState_Recovering) _then) = _$BridgePersistenceState_RecoveringCopyWithImpl;
@override @useResult
$Res call({
 BigInt pendingCommits, BigInt? oldestPendingRevision, PlatformInt64 firstFailedAt
});




}
/// @nodoc
class _$BridgePersistenceState_RecoveringCopyWithImpl<$Res>
    implements $BridgePersistenceState_RecoveringCopyWith<$Res> {
  _$BridgePersistenceState_RecoveringCopyWithImpl(this._self, this._then);

  final BridgePersistenceState_Recovering _self;
  final $Res Function(BridgePersistenceState_Recovering) _then;

/// Create a copy of BridgePersistenceState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? pendingCommits = null,Object? oldestPendingRevision = freezed,Object? firstFailedAt = null,}) {
  return _then(BridgePersistenceState_Recovering(
pendingCommits: null == pendingCommits ? _self.pendingCommits : pendingCommits // ignore: cast_nullable_to_non_nullable
as BigInt,oldestPendingRevision: freezed == oldestPendingRevision ? _self.oldestPendingRevision : oldestPendingRevision // ignore: cast_nullable_to_non_nullable
as BigInt?,firstFailedAt: null == firstFailedAt ? _self.firstFailedAt : firstFailedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,
  ));
}


}

/// @nodoc


class BridgePersistenceState_Blocked extends BridgePersistenceState {
  const BridgePersistenceState_Blocked({required this.pendingCommits, this.oldestPendingRevision, required this.firstFailedAt, required this.error}): super._();


@override final  BigInt pendingCommits;
 final  BigInt? oldestPendingRevision;
 final  PlatformInt64 firstFailedAt;
 final  BridgeStateError error;

/// Create a copy of BridgePersistenceState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgePersistenceState_BlockedCopyWith<BridgePersistenceState_Blocked> get copyWith => _$BridgePersistenceState_BlockedCopyWithImpl<BridgePersistenceState_Blocked>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgePersistenceState_Blocked&&(identical(other.pendingCommits, pendingCommits) || other.pendingCommits == pendingCommits)&&(identical(other.oldestPendingRevision, oldestPendingRevision) || other.oldestPendingRevision == oldestPendingRevision)&&(identical(other.firstFailedAt, firstFailedAt) || other.firstFailedAt == firstFailedAt)&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode {
    return Object.hash(runtimeType,pendingCommits,oldestPendingRevision,firstFailedAt,error);
}

@override
String toString() {
    return 'BridgePersistenceState.blocked(pendingCommits: $pendingCommits, oldestPendingRevision: $oldestPendingRevision, firstFailedAt: $firstFailedAt, error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgePersistenceState_BlockedCopyWith<$Res> implements $BridgePersistenceStateCopyWith<$Res> {
  factory $BridgePersistenceState_BlockedCopyWith(BridgePersistenceState_Blocked value, $Res Function(BridgePersistenceState_Blocked) _then) = _$BridgePersistenceState_BlockedCopyWithImpl;
@override @useResult
$Res call({
 BigInt pendingCommits, BigInt? oldestPendingRevision, PlatformInt64 firstFailedAt, BridgeStateError error
});




}
/// @nodoc
class _$BridgePersistenceState_BlockedCopyWithImpl<$Res>
    implements $BridgePersistenceState_BlockedCopyWith<$Res> {
  _$BridgePersistenceState_BlockedCopyWithImpl(this._self, this._then);

  final BridgePersistenceState_Blocked _self;
  final $Res Function(BridgePersistenceState_Blocked) _then;

/// Create a copy of BridgePersistenceState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? pendingCommits = null,Object? oldestPendingRevision = freezed,Object? firstFailedAt = null,Object? error = null,}) {
  return _then(BridgePersistenceState_Blocked(
pendingCommits: null == pendingCommits ? _self.pendingCommits : pendingCommits // ignore: cast_nullable_to_non_nullable
as BigInt,oldestPendingRevision: freezed == oldestPendingRevision ? _self.oldestPendingRevision : oldestPendingRevision // ignore: cast_nullable_to_non_nullable
as BigInt?,firstFailedAt: null == firstFailedAt ? _self.firstFailedAt : firstFailedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as BridgeStateError,
  ));
}


}

/// @nodoc
mixin _$BridgeProjectDirectoryState {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProjectDirectoryState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeProjectDirectoryState()';
}


}

/// @nodoc
class $BridgeProjectDirectoryStateCopyWith<$Res>  {
$BridgeProjectDirectoryStateCopyWith(BridgeProjectDirectoryState _, $Res Function(BridgeProjectDirectoryState) __);
}


/// Adds pattern-matching-related methods to [BridgeProjectDirectoryState].
extension BridgeProjectDirectoryStatePatterns on BridgeProjectDirectoryState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeProjectDirectoryState_Uninitialized value)?  uninitialized,TResult Function( BridgeProjectDirectoryState_Loading value)?  loading,TResult Function( BridgeProjectDirectoryState_Ready value)?  ready,TResult Function( BridgeProjectDirectoryState_Refreshing value)?  refreshing,TResult Function( BridgeProjectDirectoryState_Stale value)?  stale,TResult Function( BridgeProjectDirectoryState_Degraded value)?  degraded,TResult Function( BridgeProjectDirectoryState_Failed value)?  failed,TResult Function( BridgeProjectDirectoryState_Stopped value)?  stopped,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeProjectDirectoryState_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeProjectDirectoryState_Loading() when loading != null:
return loading(_that);case BridgeProjectDirectoryState_Ready() when ready != null:
return ready(_that);case BridgeProjectDirectoryState_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeProjectDirectoryState_Stale() when stale != null:
return stale(_that);case BridgeProjectDirectoryState_Degraded() when degraded != null:
return degraded(_that);case BridgeProjectDirectoryState_Failed() when failed != null:
return failed(_that);case BridgeProjectDirectoryState_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeProjectDirectoryState_Uninitialized value)  uninitialized,required TResult Function( BridgeProjectDirectoryState_Loading value)  loading,required TResult Function( BridgeProjectDirectoryState_Ready value)  ready,required TResult Function( BridgeProjectDirectoryState_Refreshing value)  refreshing,required TResult Function( BridgeProjectDirectoryState_Stale value)  stale,required TResult Function( BridgeProjectDirectoryState_Degraded value)  degraded,required TResult Function( BridgeProjectDirectoryState_Failed value)  failed,required TResult Function( BridgeProjectDirectoryState_Stopped value)  stopped,}){
final _that = this;
switch (_that) {
case BridgeProjectDirectoryState_Uninitialized():
return uninitialized(_that);case BridgeProjectDirectoryState_Loading():
return loading(_that);case BridgeProjectDirectoryState_Ready():
return ready(_that);case BridgeProjectDirectoryState_Refreshing():
return refreshing(_that);case BridgeProjectDirectoryState_Stale():
return stale(_that);case BridgeProjectDirectoryState_Degraded():
return degraded(_that);case BridgeProjectDirectoryState_Failed():
return failed(_that);case BridgeProjectDirectoryState_Stopped():
return stopped(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeProjectDirectoryState_Uninitialized value)?  uninitialized,TResult? Function( BridgeProjectDirectoryState_Loading value)?  loading,TResult? Function( BridgeProjectDirectoryState_Ready value)?  ready,TResult? Function( BridgeProjectDirectoryState_Refreshing value)?  refreshing,TResult? Function( BridgeProjectDirectoryState_Stale value)?  stale,TResult? Function( BridgeProjectDirectoryState_Degraded value)?  degraded,TResult? Function( BridgeProjectDirectoryState_Failed value)?  failed,TResult? Function( BridgeProjectDirectoryState_Stopped value)?  stopped,}){
final _that = this;
switch (_that) {
case BridgeProjectDirectoryState_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeProjectDirectoryState_Loading() when loading != null:
return loading(_that);case BridgeProjectDirectoryState_Ready() when ready != null:
return ready(_that);case BridgeProjectDirectoryState_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeProjectDirectoryState_Stale() when stale != null:
return stale(_that);case BridgeProjectDirectoryState_Degraded() when degraded != null:
return degraded(_that);case BridgeProjectDirectoryState_Failed() when failed != null:
return failed(_that);case BridgeProjectDirectoryState_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeUninitializedResource field0)?  uninitialized,TResult Function( BridgeLoadingResource field0)?  loading,TResult Function( BridgeReadyResource resource,  BridgeProjectDirectoryData value)?  ready,TResult Function( BridgeRefreshingResource resource,  BridgeProjectDirectoryData value)?  refreshing,TResult Function( BridgeStaleResource resource,  BridgeProjectDirectoryData value)?  stale,TResult Function( BridgeDegradedResource resource,  BridgeProjectDirectoryData value)?  degraded,TResult Function( BridgeFailedResource field0)?  failed,TResult Function( BridgeStoppedResource field0)?  stopped,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeProjectDirectoryState_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeProjectDirectoryState_Loading() when loading != null:
return loading(_that.field0);case BridgeProjectDirectoryState_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeProjectDirectoryState_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeProjectDirectoryState_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeProjectDirectoryState_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeProjectDirectoryState_Failed() when failed != null:
return failed(_that.field0);case BridgeProjectDirectoryState_Stopped() when stopped != null:
return stopped(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeUninitializedResource field0)  uninitialized,required TResult Function( BridgeLoadingResource field0)  loading,required TResult Function( BridgeReadyResource resource,  BridgeProjectDirectoryData value)  ready,required TResult Function( BridgeRefreshingResource resource,  BridgeProjectDirectoryData value)  refreshing,required TResult Function( BridgeStaleResource resource,  BridgeProjectDirectoryData value)  stale,required TResult Function( BridgeDegradedResource resource,  BridgeProjectDirectoryData value)  degraded,required TResult Function( BridgeFailedResource field0)  failed,required TResult Function( BridgeStoppedResource field0)  stopped,}) {final _that = this;
switch (_that) {
case BridgeProjectDirectoryState_Uninitialized():
return uninitialized(_that.field0);case BridgeProjectDirectoryState_Loading():
return loading(_that.field0);case BridgeProjectDirectoryState_Ready():
return ready(_that.resource,_that.value);case BridgeProjectDirectoryState_Refreshing():
return refreshing(_that.resource,_that.value);case BridgeProjectDirectoryState_Stale():
return stale(_that.resource,_that.value);case BridgeProjectDirectoryState_Degraded():
return degraded(_that.resource,_that.value);case BridgeProjectDirectoryState_Failed():
return failed(_that.field0);case BridgeProjectDirectoryState_Stopped():
return stopped(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeUninitializedResource field0)?  uninitialized,TResult? Function( BridgeLoadingResource field0)?  loading,TResult? Function( BridgeReadyResource resource,  BridgeProjectDirectoryData value)?  ready,TResult? Function( BridgeRefreshingResource resource,  BridgeProjectDirectoryData value)?  refreshing,TResult? Function( BridgeStaleResource resource,  BridgeProjectDirectoryData value)?  stale,TResult? Function( BridgeDegradedResource resource,  BridgeProjectDirectoryData value)?  degraded,TResult? Function( BridgeFailedResource field0)?  failed,TResult? Function( BridgeStoppedResource field0)?  stopped,}) {final _that = this;
switch (_that) {
case BridgeProjectDirectoryState_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeProjectDirectoryState_Loading() when loading != null:
return loading(_that.field0);case BridgeProjectDirectoryState_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeProjectDirectoryState_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeProjectDirectoryState_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeProjectDirectoryState_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeProjectDirectoryState_Failed() when failed != null:
return failed(_that.field0);case BridgeProjectDirectoryState_Stopped() when stopped != null:
return stopped(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeProjectDirectoryState_Uninitialized extends BridgeProjectDirectoryState {
  const BridgeProjectDirectoryState_Uninitialized(this.field0): super._();


 final  BridgeUninitializedResource field0;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProjectDirectoryState_UninitializedCopyWith<BridgeProjectDirectoryState_Uninitialized> get copyWith => _$BridgeProjectDirectoryState_UninitializedCopyWithImpl<BridgeProjectDirectoryState_Uninitialized>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProjectDirectoryState_Uninitialized&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeProjectDirectoryState.uninitialized(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProjectDirectoryState_UninitializedCopyWith<$Res> implements $BridgeProjectDirectoryStateCopyWith<$Res> {
  factory $BridgeProjectDirectoryState_UninitializedCopyWith(BridgeProjectDirectoryState_Uninitialized value, $Res Function(BridgeProjectDirectoryState_Uninitialized) _then) = _$BridgeProjectDirectoryState_UninitializedCopyWithImpl;
@useResult
$Res call({
 BridgeUninitializedResource field0
});




}
/// @nodoc
class _$BridgeProjectDirectoryState_UninitializedCopyWithImpl<$Res>
    implements $BridgeProjectDirectoryState_UninitializedCopyWith<$Res> {
  _$BridgeProjectDirectoryState_UninitializedCopyWithImpl(this._self, this._then);

  final BridgeProjectDirectoryState_Uninitialized _self;
  final $Res Function(BridgeProjectDirectoryState_Uninitialized) _then;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProjectDirectoryState_Uninitialized(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeUninitializedResource,
  ));
}


}

/// @nodoc


class BridgeProjectDirectoryState_Loading extends BridgeProjectDirectoryState {
  const BridgeProjectDirectoryState_Loading(this.field0): super._();


 final  BridgeLoadingResource field0;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProjectDirectoryState_LoadingCopyWith<BridgeProjectDirectoryState_Loading> get copyWith => _$BridgeProjectDirectoryState_LoadingCopyWithImpl<BridgeProjectDirectoryState_Loading>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProjectDirectoryState_Loading&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeProjectDirectoryState.loading(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProjectDirectoryState_LoadingCopyWith<$Res> implements $BridgeProjectDirectoryStateCopyWith<$Res> {
  factory $BridgeProjectDirectoryState_LoadingCopyWith(BridgeProjectDirectoryState_Loading value, $Res Function(BridgeProjectDirectoryState_Loading) _then) = _$BridgeProjectDirectoryState_LoadingCopyWithImpl;
@useResult
$Res call({
 BridgeLoadingResource field0
});




}
/// @nodoc
class _$BridgeProjectDirectoryState_LoadingCopyWithImpl<$Res>
    implements $BridgeProjectDirectoryState_LoadingCopyWith<$Res> {
  _$BridgeProjectDirectoryState_LoadingCopyWithImpl(this._self, this._then);

  final BridgeProjectDirectoryState_Loading _self;
  final $Res Function(BridgeProjectDirectoryState_Loading) _then;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProjectDirectoryState_Loading(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeLoadingResource,
  ));
}


}

/// @nodoc


class BridgeProjectDirectoryState_Ready extends BridgeProjectDirectoryState {
  const BridgeProjectDirectoryState_Ready({required this.resource, required this.value}): super._();


 final  BridgeReadyResource resource;
 final  BridgeProjectDirectoryData value;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProjectDirectoryState_ReadyCopyWith<BridgeProjectDirectoryState_Ready> get copyWith => _$BridgeProjectDirectoryState_ReadyCopyWithImpl<BridgeProjectDirectoryState_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProjectDirectoryState_Ready&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeProjectDirectoryState.ready(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeProjectDirectoryState_ReadyCopyWith<$Res> implements $BridgeProjectDirectoryStateCopyWith<$Res> {
  factory $BridgeProjectDirectoryState_ReadyCopyWith(BridgeProjectDirectoryState_Ready value, $Res Function(BridgeProjectDirectoryState_Ready) _then) = _$BridgeProjectDirectoryState_ReadyCopyWithImpl;
@useResult
$Res call({
 BridgeReadyResource resource, BridgeProjectDirectoryData value
});




}
/// @nodoc
class _$BridgeProjectDirectoryState_ReadyCopyWithImpl<$Res>
    implements $BridgeProjectDirectoryState_ReadyCopyWith<$Res> {
  _$BridgeProjectDirectoryState_ReadyCopyWithImpl(this._self, this._then);

  final BridgeProjectDirectoryState_Ready _self;
  final $Res Function(BridgeProjectDirectoryState_Ready) _then;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeProjectDirectoryState_Ready(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeReadyResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeProjectDirectoryData,
  ));
}


}

/// @nodoc


class BridgeProjectDirectoryState_Refreshing extends BridgeProjectDirectoryState {
  const BridgeProjectDirectoryState_Refreshing({required this.resource, required this.value}): super._();


 final  BridgeRefreshingResource resource;
 final  BridgeProjectDirectoryData value;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProjectDirectoryState_RefreshingCopyWith<BridgeProjectDirectoryState_Refreshing> get copyWith => _$BridgeProjectDirectoryState_RefreshingCopyWithImpl<BridgeProjectDirectoryState_Refreshing>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProjectDirectoryState_Refreshing&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeProjectDirectoryState.refreshing(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeProjectDirectoryState_RefreshingCopyWith<$Res> implements $BridgeProjectDirectoryStateCopyWith<$Res> {
  factory $BridgeProjectDirectoryState_RefreshingCopyWith(BridgeProjectDirectoryState_Refreshing value, $Res Function(BridgeProjectDirectoryState_Refreshing) _then) = _$BridgeProjectDirectoryState_RefreshingCopyWithImpl;
@useResult
$Res call({
 BridgeRefreshingResource resource, BridgeProjectDirectoryData value
});




}
/// @nodoc
class _$BridgeProjectDirectoryState_RefreshingCopyWithImpl<$Res>
    implements $BridgeProjectDirectoryState_RefreshingCopyWith<$Res> {
  _$BridgeProjectDirectoryState_RefreshingCopyWithImpl(this._self, this._then);

  final BridgeProjectDirectoryState_Refreshing _self;
  final $Res Function(BridgeProjectDirectoryState_Refreshing) _then;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeProjectDirectoryState_Refreshing(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeRefreshingResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeProjectDirectoryData,
  ));
}


}

/// @nodoc


class BridgeProjectDirectoryState_Stale extends BridgeProjectDirectoryState {
  const BridgeProjectDirectoryState_Stale({required this.resource, required this.value}): super._();


 final  BridgeStaleResource resource;
 final  BridgeProjectDirectoryData value;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProjectDirectoryState_StaleCopyWith<BridgeProjectDirectoryState_Stale> get copyWith => _$BridgeProjectDirectoryState_StaleCopyWithImpl<BridgeProjectDirectoryState_Stale>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProjectDirectoryState_Stale&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeProjectDirectoryState.stale(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeProjectDirectoryState_StaleCopyWith<$Res> implements $BridgeProjectDirectoryStateCopyWith<$Res> {
  factory $BridgeProjectDirectoryState_StaleCopyWith(BridgeProjectDirectoryState_Stale value, $Res Function(BridgeProjectDirectoryState_Stale) _then) = _$BridgeProjectDirectoryState_StaleCopyWithImpl;
@useResult
$Res call({
 BridgeStaleResource resource, BridgeProjectDirectoryData value
});




}
/// @nodoc
class _$BridgeProjectDirectoryState_StaleCopyWithImpl<$Res>
    implements $BridgeProjectDirectoryState_StaleCopyWith<$Res> {
  _$BridgeProjectDirectoryState_StaleCopyWithImpl(this._self, this._then);

  final BridgeProjectDirectoryState_Stale _self;
  final $Res Function(BridgeProjectDirectoryState_Stale) _then;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeProjectDirectoryState_Stale(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeStaleResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeProjectDirectoryData,
  ));
}


}

/// @nodoc


class BridgeProjectDirectoryState_Degraded extends BridgeProjectDirectoryState {
  const BridgeProjectDirectoryState_Degraded({required this.resource, required this.value}): super._();


 final  BridgeDegradedResource resource;
 final  BridgeProjectDirectoryData value;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProjectDirectoryState_DegradedCopyWith<BridgeProjectDirectoryState_Degraded> get copyWith => _$BridgeProjectDirectoryState_DegradedCopyWithImpl<BridgeProjectDirectoryState_Degraded>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProjectDirectoryState_Degraded&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeProjectDirectoryState.degraded(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeProjectDirectoryState_DegradedCopyWith<$Res> implements $BridgeProjectDirectoryStateCopyWith<$Res> {
  factory $BridgeProjectDirectoryState_DegradedCopyWith(BridgeProjectDirectoryState_Degraded value, $Res Function(BridgeProjectDirectoryState_Degraded) _then) = _$BridgeProjectDirectoryState_DegradedCopyWithImpl;
@useResult
$Res call({
 BridgeDegradedResource resource, BridgeProjectDirectoryData value
});




}
/// @nodoc
class _$BridgeProjectDirectoryState_DegradedCopyWithImpl<$Res>
    implements $BridgeProjectDirectoryState_DegradedCopyWith<$Res> {
  _$BridgeProjectDirectoryState_DegradedCopyWithImpl(this._self, this._then);

  final BridgeProjectDirectoryState_Degraded _self;
  final $Res Function(BridgeProjectDirectoryState_Degraded) _then;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeProjectDirectoryState_Degraded(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeDegradedResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeProjectDirectoryData,
  ));
}


}

/// @nodoc


class BridgeProjectDirectoryState_Failed extends BridgeProjectDirectoryState {
  const BridgeProjectDirectoryState_Failed(this.field0): super._();


 final  BridgeFailedResource field0;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProjectDirectoryState_FailedCopyWith<BridgeProjectDirectoryState_Failed> get copyWith => _$BridgeProjectDirectoryState_FailedCopyWithImpl<BridgeProjectDirectoryState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProjectDirectoryState_Failed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeProjectDirectoryState.failed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProjectDirectoryState_FailedCopyWith<$Res> implements $BridgeProjectDirectoryStateCopyWith<$Res> {
  factory $BridgeProjectDirectoryState_FailedCopyWith(BridgeProjectDirectoryState_Failed value, $Res Function(BridgeProjectDirectoryState_Failed) _then) = _$BridgeProjectDirectoryState_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeFailedResource field0
});




}
/// @nodoc
class _$BridgeProjectDirectoryState_FailedCopyWithImpl<$Res>
    implements $BridgeProjectDirectoryState_FailedCopyWith<$Res> {
  _$BridgeProjectDirectoryState_FailedCopyWithImpl(this._self, this._then);

  final BridgeProjectDirectoryState_Failed _self;
  final $Res Function(BridgeProjectDirectoryState_Failed) _then;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProjectDirectoryState_Failed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeFailedResource,
  ));
}


}

/// @nodoc


class BridgeProjectDirectoryState_Stopped extends BridgeProjectDirectoryState {
  const BridgeProjectDirectoryState_Stopped(this.field0): super._();


 final  BridgeStoppedResource field0;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProjectDirectoryState_StoppedCopyWith<BridgeProjectDirectoryState_Stopped> get copyWith => _$BridgeProjectDirectoryState_StoppedCopyWithImpl<BridgeProjectDirectoryState_Stopped>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProjectDirectoryState_Stopped&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeProjectDirectoryState.stopped(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProjectDirectoryState_StoppedCopyWith<$Res> implements $BridgeProjectDirectoryStateCopyWith<$Res> {
  factory $BridgeProjectDirectoryState_StoppedCopyWith(BridgeProjectDirectoryState_Stopped value, $Res Function(BridgeProjectDirectoryState_Stopped) _then) = _$BridgeProjectDirectoryState_StoppedCopyWithImpl;
@useResult
$Res call({
 BridgeStoppedResource field0
});




}
/// @nodoc
class _$BridgeProjectDirectoryState_StoppedCopyWithImpl<$Res>
    implements $BridgeProjectDirectoryState_StoppedCopyWith<$Res> {
  _$BridgeProjectDirectoryState_StoppedCopyWithImpl(this._self, this._then);

  final BridgeProjectDirectoryState_Stopped _self;
  final $Res Function(BridgeProjectDirectoryState_Stopped) _then;

/// Create a copy of BridgeProjectDirectoryState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProjectDirectoryState_Stopped(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeStoppedResource,
  ));
}


}

/// @nodoc
mixin _$BridgeProviderUsageData {

 Object get field0;



@override
bool operator ==(Object other) {
  final _this = this as BridgeProviderUsageData;
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageData&&const DeepCollectionEquality().equals(other.field0, _this.field0));
}


@override
int get hashCode {
  final _this = this as BridgeProviderUsageData;
  return Object.hash(runtimeType,const DeepCollectionEquality().hash(_this.field0));
}

@override
String toString() {
  final _this = this as BridgeProviderUsageData;
  return 'BridgeProviderUsageData(field0: ${_this.field0})';
}


}

/// @nodoc
class $BridgeProviderUsageDataCopyWith<$Res>  {
$BridgeProviderUsageDataCopyWith(BridgeProviderUsageData _, $Res Function(BridgeProviderUsageData) __);
}


/// Adds pattern-matching-related methods to [BridgeProviderUsageData].
extension BridgeProviderUsageDataPatterns on BridgeProviderUsageData {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeProviderUsageData_DeepSeekBalance value)?  deepSeekBalance,TResult Function( BridgeProviderUsageData_ZhipuCodingPlan value)?  zhipuCodingPlan,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeProviderUsageData_DeepSeekBalance() when deepSeekBalance != null:
return deepSeekBalance(_that);case BridgeProviderUsageData_ZhipuCodingPlan() when zhipuCodingPlan != null:
return zhipuCodingPlan(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeProviderUsageData_DeepSeekBalance value)  deepSeekBalance,required TResult Function( BridgeProviderUsageData_ZhipuCodingPlan value)  zhipuCodingPlan,}){
final _that = this;
switch (_that) {
case BridgeProviderUsageData_DeepSeekBalance():
return deepSeekBalance(_that);case BridgeProviderUsageData_ZhipuCodingPlan():
return zhipuCodingPlan(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeProviderUsageData_DeepSeekBalance value)?  deepSeekBalance,TResult? Function( BridgeProviderUsageData_ZhipuCodingPlan value)?  zhipuCodingPlan,}){
final _that = this;
switch (_that) {
case BridgeProviderUsageData_DeepSeekBalance() when deepSeekBalance != null:
return deepSeekBalance(_that);case BridgeProviderUsageData_ZhipuCodingPlan() when zhipuCodingPlan != null:
return zhipuCodingPlan(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( DeepSeekBalanceDto field0)?  deepSeekBalance,TResult Function( ZhipuCodingPlanUsageDto field0)?  zhipuCodingPlan,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeProviderUsageData_DeepSeekBalance() when deepSeekBalance != null:
return deepSeekBalance(_that.field0);case BridgeProviderUsageData_ZhipuCodingPlan() when zhipuCodingPlan != null:
return zhipuCodingPlan(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( DeepSeekBalanceDto field0)  deepSeekBalance,required TResult Function( ZhipuCodingPlanUsageDto field0)  zhipuCodingPlan,}) {final _that = this;
switch (_that) {
case BridgeProviderUsageData_DeepSeekBalance():
return deepSeekBalance(_that.field0);case BridgeProviderUsageData_ZhipuCodingPlan():
return zhipuCodingPlan(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( DeepSeekBalanceDto field0)?  deepSeekBalance,TResult? Function( ZhipuCodingPlanUsageDto field0)?  zhipuCodingPlan,}) {final _that = this;
switch (_that) {
case BridgeProviderUsageData_DeepSeekBalance() when deepSeekBalance != null:
return deepSeekBalance(_that.field0);case BridgeProviderUsageData_ZhipuCodingPlan() when zhipuCodingPlan != null:
return zhipuCodingPlan(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeProviderUsageData_DeepSeekBalance extends BridgeProviderUsageData {
  const BridgeProviderUsageData_DeepSeekBalance(this.field0): super._();


@override final  DeepSeekBalanceDto field0;

/// Create a copy of BridgeProviderUsageData
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProviderUsageData_DeepSeekBalanceCopyWith<BridgeProviderUsageData_DeepSeekBalance> get copyWith => _$BridgeProviderUsageData_DeepSeekBalanceCopyWithImpl<BridgeProviderUsageData_DeepSeekBalance>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageData_DeepSeekBalance&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeProviderUsageData.deepSeekBalance(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProviderUsageData_DeepSeekBalanceCopyWith<$Res> implements $BridgeProviderUsageDataCopyWith<$Res> {
  factory $BridgeProviderUsageData_DeepSeekBalanceCopyWith(BridgeProviderUsageData_DeepSeekBalance value, $Res Function(BridgeProviderUsageData_DeepSeekBalance) _then) = _$BridgeProviderUsageData_DeepSeekBalanceCopyWithImpl;
@useResult
$Res call({
 DeepSeekBalanceDto field0
});




}
/// @nodoc
class _$BridgeProviderUsageData_DeepSeekBalanceCopyWithImpl<$Res>
    implements $BridgeProviderUsageData_DeepSeekBalanceCopyWith<$Res> {
  _$BridgeProviderUsageData_DeepSeekBalanceCopyWithImpl(this._self, this._then);

  final BridgeProviderUsageData_DeepSeekBalance _self;
  final $Res Function(BridgeProviderUsageData_DeepSeekBalance) _then;

/// Create a copy of BridgeProviderUsageData
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProviderUsageData_DeepSeekBalance(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as DeepSeekBalanceDto,
  ));
}


}

/// @nodoc


class BridgeProviderUsageData_ZhipuCodingPlan extends BridgeProviderUsageData {
  const BridgeProviderUsageData_ZhipuCodingPlan(this.field0): super._();


@override final  ZhipuCodingPlanUsageDto field0;

/// Create a copy of BridgeProviderUsageData
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProviderUsageData_ZhipuCodingPlanCopyWith<BridgeProviderUsageData_ZhipuCodingPlan> get copyWith => _$BridgeProviderUsageData_ZhipuCodingPlanCopyWithImpl<BridgeProviderUsageData_ZhipuCodingPlan>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageData_ZhipuCodingPlan&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeProviderUsageData.zhipuCodingPlan(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProviderUsageData_ZhipuCodingPlanCopyWith<$Res> implements $BridgeProviderUsageDataCopyWith<$Res> {
  factory $BridgeProviderUsageData_ZhipuCodingPlanCopyWith(BridgeProviderUsageData_ZhipuCodingPlan value, $Res Function(BridgeProviderUsageData_ZhipuCodingPlan) _then) = _$BridgeProviderUsageData_ZhipuCodingPlanCopyWithImpl;
@useResult
$Res call({
 ZhipuCodingPlanUsageDto field0
});




}
/// @nodoc
class _$BridgeProviderUsageData_ZhipuCodingPlanCopyWithImpl<$Res>
    implements $BridgeProviderUsageData_ZhipuCodingPlanCopyWith<$Res> {
  _$BridgeProviderUsageData_ZhipuCodingPlanCopyWithImpl(this._self, this._then);

  final BridgeProviderUsageData_ZhipuCodingPlan _self;
  final $Res Function(BridgeProviderUsageData_ZhipuCodingPlan) _then;

/// Create a copy of BridgeProviderUsageData
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProviderUsageData_ZhipuCodingPlan(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as ZhipuCodingPlanUsageDto,
  ));
}


}

/// @nodoc
mixin _$BridgeProviderUsageState {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeProviderUsageState()';
}


}

/// @nodoc
class $BridgeProviderUsageStateCopyWith<$Res>  {
$BridgeProviderUsageStateCopyWith(BridgeProviderUsageState _, $Res Function(BridgeProviderUsageState) __);
}


/// Adds pattern-matching-related methods to [BridgeProviderUsageState].
extension BridgeProviderUsageStatePatterns on BridgeProviderUsageState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeProviderUsageState_Unsupported value)?  unsupported,TResult Function( BridgeProviderUsageState_MissingCredential value)?  missingCredential,TResult Function( BridgeProviderUsageState_Ready value)?  ready,TResult Function( BridgeProviderUsageState_Failed value)?  failed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeProviderUsageState_Unsupported() when unsupported != null:
return unsupported(_that);case BridgeProviderUsageState_MissingCredential() when missingCredential != null:
return missingCredential(_that);case BridgeProviderUsageState_Ready() when ready != null:
return ready(_that);case BridgeProviderUsageState_Failed() when failed != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeProviderUsageState_Unsupported value)  unsupported,required TResult Function( BridgeProviderUsageState_MissingCredential value)  missingCredential,required TResult Function( BridgeProviderUsageState_Ready value)  ready,required TResult Function( BridgeProviderUsageState_Failed value)  failed,}){
final _that = this;
switch (_that) {
case BridgeProviderUsageState_Unsupported():
return unsupported(_that);case BridgeProviderUsageState_MissingCredential():
return missingCredential(_that);case BridgeProviderUsageState_Ready():
return ready(_that);case BridgeProviderUsageState_Failed():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeProviderUsageState_Unsupported value)?  unsupported,TResult? Function( BridgeProviderUsageState_MissingCredential value)?  missingCredential,TResult? Function( BridgeProviderUsageState_Ready value)?  ready,TResult? Function( BridgeProviderUsageState_Failed value)?  failed,}){
final _that = this;
switch (_that) {
case BridgeProviderUsageState_Unsupported() when unsupported != null:
return unsupported(_that);case BridgeProviderUsageState_MissingCredential() when missingCredential != null:
return missingCredential(_that);case BridgeProviderUsageState_Ready() when ready != null:
return ready(_that);case BridgeProviderUsageState_Failed() when failed != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  unsupported,TResult Function( String message)?  missingCredential,TResult Function( BridgeProviderUsageData data)?  ready,TResult Function( BridgeStateError error)?  failed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeProviderUsageState_Unsupported() when unsupported != null:
return unsupported();case BridgeProviderUsageState_MissingCredential() when missingCredential != null:
return missingCredential(_that.message);case BridgeProviderUsageState_Ready() when ready != null:
return ready(_that.data);case BridgeProviderUsageState_Failed() when failed != null:
return failed(_that.error);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  unsupported,required TResult Function( String message)  missingCredential,required TResult Function( BridgeProviderUsageData data)  ready,required TResult Function( BridgeStateError error)  failed,}) {final _that = this;
switch (_that) {
case BridgeProviderUsageState_Unsupported():
return unsupported();case BridgeProviderUsageState_MissingCredential():
return missingCredential(_that.message);case BridgeProviderUsageState_Ready():
return ready(_that.data);case BridgeProviderUsageState_Failed():
return failed(_that.error);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  unsupported,TResult? Function( String message)?  missingCredential,TResult? Function( BridgeProviderUsageData data)?  ready,TResult? Function( BridgeStateError error)?  failed,}) {final _that = this;
switch (_that) {
case BridgeProviderUsageState_Unsupported() when unsupported != null:
return unsupported();case BridgeProviderUsageState_MissingCredential() when missingCredential != null:
return missingCredential(_that.message);case BridgeProviderUsageState_Ready() when ready != null:
return ready(_that.data);case BridgeProviderUsageState_Failed() when failed != null:
return failed(_that.error);case _:
  return null;

}
}

}

/// @nodoc


class BridgeProviderUsageState_Unsupported extends BridgeProviderUsageState {
  const BridgeProviderUsageState_Unsupported(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageState_Unsupported);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeProviderUsageState.unsupported()';
}


}




/// @nodoc


class BridgeProviderUsageState_MissingCredential extends BridgeProviderUsageState {
  const BridgeProviderUsageState_MissingCredential({required this.message}): super._();


 final  String message;

/// Create a copy of BridgeProviderUsageState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProviderUsageState_MissingCredentialCopyWith<BridgeProviderUsageState_MissingCredential> get copyWith => _$BridgeProviderUsageState_MissingCredentialCopyWithImpl<BridgeProviderUsageState_MissingCredential>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageState_MissingCredential&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode {
    return Object.hash(runtimeType,message);
}

@override
String toString() {
    return 'BridgeProviderUsageState.missingCredential(message: $message)';
}


}

/// @nodoc
abstract mixin class $BridgeProviderUsageState_MissingCredentialCopyWith<$Res> implements $BridgeProviderUsageStateCopyWith<$Res> {
  factory $BridgeProviderUsageState_MissingCredentialCopyWith(BridgeProviderUsageState_MissingCredential value, $Res Function(BridgeProviderUsageState_MissingCredential) _then) = _$BridgeProviderUsageState_MissingCredentialCopyWithImpl;
@useResult
$Res call({
 String message
});




}
/// @nodoc
class _$BridgeProviderUsageState_MissingCredentialCopyWithImpl<$Res>
    implements $BridgeProviderUsageState_MissingCredentialCopyWith<$Res> {
  _$BridgeProviderUsageState_MissingCredentialCopyWithImpl(this._self, this._then);

  final BridgeProviderUsageState_MissingCredential _self;
  final $Res Function(BridgeProviderUsageState_MissingCredential) _then;

/// Create a copy of BridgeProviderUsageState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = null,}) {
  return _then(BridgeProviderUsageState_MissingCredential(
message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeProviderUsageState_Ready extends BridgeProviderUsageState {
  const BridgeProviderUsageState_Ready({required this.data}): super._();


 final  BridgeProviderUsageData data;

/// Create a copy of BridgeProviderUsageState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProviderUsageState_ReadyCopyWith<BridgeProviderUsageState_Ready> get copyWith => _$BridgeProviderUsageState_ReadyCopyWithImpl<BridgeProviderUsageState_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageState_Ready&&(identical(other.data, data) || other.data == data));
}


@override
int get hashCode {
    return Object.hash(runtimeType,data);
}

@override
String toString() {
    return 'BridgeProviderUsageState.ready(data: $data)';
}


}

/// @nodoc
abstract mixin class $BridgeProviderUsageState_ReadyCopyWith<$Res> implements $BridgeProviderUsageStateCopyWith<$Res> {
  factory $BridgeProviderUsageState_ReadyCopyWith(BridgeProviderUsageState_Ready value, $Res Function(BridgeProviderUsageState_Ready) _then) = _$BridgeProviderUsageState_ReadyCopyWithImpl;
@useResult
$Res call({
 BridgeProviderUsageData data
});


$BridgeProviderUsageDataCopyWith<$Res> get data;

}
/// @nodoc
class _$BridgeProviderUsageState_ReadyCopyWithImpl<$Res>
    implements $BridgeProviderUsageState_ReadyCopyWith<$Res> {
  _$BridgeProviderUsageState_ReadyCopyWithImpl(this._self, this._then);

  final BridgeProviderUsageState_Ready _self;
  final $Res Function(BridgeProviderUsageState_Ready) _then;

/// Create a copy of BridgeProviderUsageState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? data = null,}) {
  return _then(BridgeProviderUsageState_Ready(
data: null == data ? _self.data : data // ignore: cast_nullable_to_non_nullable
as BridgeProviderUsageData,
  ));
}

/// Create a copy of BridgeProviderUsageState
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeProviderUsageDataCopyWith<$Res> get data {

  return $BridgeProviderUsageDataCopyWith<$Res>(_self.data, (value) {
    return _then(_self.copyWith(data: value));
  });
}
}

/// @nodoc


class BridgeProviderUsageState_Failed extends BridgeProviderUsageState {
  const BridgeProviderUsageState_Failed({required this.error}): super._();


 final  BridgeStateError error;

/// Create a copy of BridgeProviderUsageState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProviderUsageState_FailedCopyWith<BridgeProviderUsageState_Failed> get copyWith => _$BridgeProviderUsageState_FailedCopyWithImpl<BridgeProviderUsageState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageState_Failed&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode {
    return Object.hash(runtimeType,error);
}

@override
String toString() {
    return 'BridgeProviderUsageState.failed(error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeProviderUsageState_FailedCopyWith<$Res> implements $BridgeProviderUsageStateCopyWith<$Res> {
  factory $BridgeProviderUsageState_FailedCopyWith(BridgeProviderUsageState_Failed value, $Res Function(BridgeProviderUsageState_Failed) _then) = _$BridgeProviderUsageState_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeStateError error
});




}
/// @nodoc
class _$BridgeProviderUsageState_FailedCopyWithImpl<$Res>
    implements $BridgeProviderUsageState_FailedCopyWith<$Res> {
  _$BridgeProviderUsageState_FailedCopyWithImpl(this._self, this._then);

  final BridgeProviderUsageState_Failed _self;
  final $Res Function(BridgeProviderUsageState_Failed) _then;

/// Create a copy of BridgeProviderUsageState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? error = null,}) {
  return _then(BridgeProviderUsageState_Failed(
error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as BridgeStateError,
  ));
}


}

/// @nodoc
mixin _$BridgeProviderUsageStateSnapshot {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageStateSnapshot);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeProviderUsageStateSnapshot()';
}


}

/// @nodoc
class $BridgeProviderUsageStateSnapshotCopyWith<$Res>  {
$BridgeProviderUsageStateSnapshotCopyWith(BridgeProviderUsageStateSnapshot _, $Res Function(BridgeProviderUsageStateSnapshot) __);
}


/// Adds pattern-matching-related methods to [BridgeProviderUsageStateSnapshot].
extension BridgeProviderUsageStateSnapshotPatterns on BridgeProviderUsageStateSnapshot {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeProviderUsageStateSnapshot_Uninitialized value)?  uninitialized,TResult Function( BridgeProviderUsageStateSnapshot_Loading value)?  loading,TResult Function( BridgeProviderUsageStateSnapshot_Ready value)?  ready,TResult Function( BridgeProviderUsageStateSnapshot_Refreshing value)?  refreshing,TResult Function( BridgeProviderUsageStateSnapshot_Stale value)?  stale,TResult Function( BridgeProviderUsageStateSnapshot_Degraded value)?  degraded,TResult Function( BridgeProviderUsageStateSnapshot_Failed value)?  failed,TResult Function( BridgeProviderUsageStateSnapshot_Stopped value)?  stopped,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeProviderUsageStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeProviderUsageStateSnapshot_Loading() when loading != null:
return loading(_that);case BridgeProviderUsageStateSnapshot_Ready() when ready != null:
return ready(_that);case BridgeProviderUsageStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeProviderUsageStateSnapshot_Stale() when stale != null:
return stale(_that);case BridgeProviderUsageStateSnapshot_Degraded() when degraded != null:
return degraded(_that);case BridgeProviderUsageStateSnapshot_Failed() when failed != null:
return failed(_that);case BridgeProviderUsageStateSnapshot_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeProviderUsageStateSnapshot_Uninitialized value)  uninitialized,required TResult Function( BridgeProviderUsageStateSnapshot_Loading value)  loading,required TResult Function( BridgeProviderUsageStateSnapshot_Ready value)  ready,required TResult Function( BridgeProviderUsageStateSnapshot_Refreshing value)  refreshing,required TResult Function( BridgeProviderUsageStateSnapshot_Stale value)  stale,required TResult Function( BridgeProviderUsageStateSnapshot_Degraded value)  degraded,required TResult Function( BridgeProviderUsageStateSnapshot_Failed value)  failed,required TResult Function( BridgeProviderUsageStateSnapshot_Stopped value)  stopped,}){
final _that = this;
switch (_that) {
case BridgeProviderUsageStateSnapshot_Uninitialized():
return uninitialized(_that);case BridgeProviderUsageStateSnapshot_Loading():
return loading(_that);case BridgeProviderUsageStateSnapshot_Ready():
return ready(_that);case BridgeProviderUsageStateSnapshot_Refreshing():
return refreshing(_that);case BridgeProviderUsageStateSnapshot_Stale():
return stale(_that);case BridgeProviderUsageStateSnapshot_Degraded():
return degraded(_that);case BridgeProviderUsageStateSnapshot_Failed():
return failed(_that);case BridgeProviderUsageStateSnapshot_Stopped():
return stopped(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeProviderUsageStateSnapshot_Uninitialized value)?  uninitialized,TResult? Function( BridgeProviderUsageStateSnapshot_Loading value)?  loading,TResult? Function( BridgeProviderUsageStateSnapshot_Ready value)?  ready,TResult? Function( BridgeProviderUsageStateSnapshot_Refreshing value)?  refreshing,TResult? Function( BridgeProviderUsageStateSnapshot_Stale value)?  stale,TResult? Function( BridgeProviderUsageStateSnapshot_Degraded value)?  degraded,TResult? Function( BridgeProviderUsageStateSnapshot_Failed value)?  failed,TResult? Function( BridgeProviderUsageStateSnapshot_Stopped value)?  stopped,}){
final _that = this;
switch (_that) {
case BridgeProviderUsageStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeProviderUsageStateSnapshot_Loading() when loading != null:
return loading(_that);case BridgeProviderUsageStateSnapshot_Ready() when ready != null:
return ready(_that);case BridgeProviderUsageStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeProviderUsageStateSnapshot_Stale() when stale != null:
return stale(_that);case BridgeProviderUsageStateSnapshot_Degraded() when degraded != null:
return degraded(_that);case BridgeProviderUsageStateSnapshot_Failed() when failed != null:
return failed(_that);case BridgeProviderUsageStateSnapshot_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeUninitializedResource field0)?  uninitialized,TResult Function( BridgeLoadingResource field0)?  loading,TResult Function( BridgeReadyResource resource,  BridgeProviderUsageStateData value)?  ready,TResult Function( BridgeRefreshingResource resource,  BridgeProviderUsageStateData value)?  refreshing,TResult Function( BridgeStaleResource resource,  BridgeProviderUsageStateData value)?  stale,TResult Function( BridgeDegradedResource resource,  BridgeProviderUsageStateData value)?  degraded,TResult Function( BridgeFailedResource field0)?  failed,TResult Function( BridgeStoppedResource field0)?  stopped,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeProviderUsageStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeProviderUsageStateSnapshot_Loading() when loading != null:
return loading(_that.field0);case BridgeProviderUsageStateSnapshot_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeProviderUsageStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeProviderUsageStateSnapshot_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeProviderUsageStateSnapshot_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeProviderUsageStateSnapshot_Failed() when failed != null:
return failed(_that.field0);case BridgeProviderUsageStateSnapshot_Stopped() when stopped != null:
return stopped(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeUninitializedResource field0)  uninitialized,required TResult Function( BridgeLoadingResource field0)  loading,required TResult Function( BridgeReadyResource resource,  BridgeProviderUsageStateData value)  ready,required TResult Function( BridgeRefreshingResource resource,  BridgeProviderUsageStateData value)  refreshing,required TResult Function( BridgeStaleResource resource,  BridgeProviderUsageStateData value)  stale,required TResult Function( BridgeDegradedResource resource,  BridgeProviderUsageStateData value)  degraded,required TResult Function( BridgeFailedResource field0)  failed,required TResult Function( BridgeStoppedResource field0)  stopped,}) {final _that = this;
switch (_that) {
case BridgeProviderUsageStateSnapshot_Uninitialized():
return uninitialized(_that.field0);case BridgeProviderUsageStateSnapshot_Loading():
return loading(_that.field0);case BridgeProviderUsageStateSnapshot_Ready():
return ready(_that.resource,_that.value);case BridgeProviderUsageStateSnapshot_Refreshing():
return refreshing(_that.resource,_that.value);case BridgeProviderUsageStateSnapshot_Stale():
return stale(_that.resource,_that.value);case BridgeProviderUsageStateSnapshot_Degraded():
return degraded(_that.resource,_that.value);case BridgeProviderUsageStateSnapshot_Failed():
return failed(_that.field0);case BridgeProviderUsageStateSnapshot_Stopped():
return stopped(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeUninitializedResource field0)?  uninitialized,TResult? Function( BridgeLoadingResource field0)?  loading,TResult? Function( BridgeReadyResource resource,  BridgeProviderUsageStateData value)?  ready,TResult? Function( BridgeRefreshingResource resource,  BridgeProviderUsageStateData value)?  refreshing,TResult? Function( BridgeStaleResource resource,  BridgeProviderUsageStateData value)?  stale,TResult? Function( BridgeDegradedResource resource,  BridgeProviderUsageStateData value)?  degraded,TResult? Function( BridgeFailedResource field0)?  failed,TResult? Function( BridgeStoppedResource field0)?  stopped,}) {final _that = this;
switch (_that) {
case BridgeProviderUsageStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeProviderUsageStateSnapshot_Loading() when loading != null:
return loading(_that.field0);case BridgeProviderUsageStateSnapshot_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeProviderUsageStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeProviderUsageStateSnapshot_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeProviderUsageStateSnapshot_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeProviderUsageStateSnapshot_Failed() when failed != null:
return failed(_that.field0);case BridgeProviderUsageStateSnapshot_Stopped() when stopped != null:
return stopped(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeProviderUsageStateSnapshot_Uninitialized extends BridgeProviderUsageStateSnapshot {
  const BridgeProviderUsageStateSnapshot_Uninitialized(this.field0): super._();


 final  BridgeUninitializedResource field0;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProviderUsageStateSnapshot_UninitializedCopyWith<BridgeProviderUsageStateSnapshot_Uninitialized> get copyWith => _$BridgeProviderUsageStateSnapshot_UninitializedCopyWithImpl<BridgeProviderUsageStateSnapshot_Uninitialized>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageStateSnapshot_Uninitialized&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeProviderUsageStateSnapshot.uninitialized(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProviderUsageStateSnapshot_UninitializedCopyWith<$Res> implements $BridgeProviderUsageStateSnapshotCopyWith<$Res> {
  factory $BridgeProviderUsageStateSnapshot_UninitializedCopyWith(BridgeProviderUsageStateSnapshot_Uninitialized value, $Res Function(BridgeProviderUsageStateSnapshot_Uninitialized) _then) = _$BridgeProviderUsageStateSnapshot_UninitializedCopyWithImpl;
@useResult
$Res call({
 BridgeUninitializedResource field0
});




}
/// @nodoc
class _$BridgeProviderUsageStateSnapshot_UninitializedCopyWithImpl<$Res>
    implements $BridgeProviderUsageStateSnapshot_UninitializedCopyWith<$Res> {
  _$BridgeProviderUsageStateSnapshot_UninitializedCopyWithImpl(this._self, this._then);

  final BridgeProviderUsageStateSnapshot_Uninitialized _self;
  final $Res Function(BridgeProviderUsageStateSnapshot_Uninitialized) _then;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProviderUsageStateSnapshot_Uninitialized(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeUninitializedResource,
  ));
}


}

/// @nodoc


class BridgeProviderUsageStateSnapshot_Loading extends BridgeProviderUsageStateSnapshot {
  const BridgeProviderUsageStateSnapshot_Loading(this.field0): super._();


 final  BridgeLoadingResource field0;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProviderUsageStateSnapshot_LoadingCopyWith<BridgeProviderUsageStateSnapshot_Loading> get copyWith => _$BridgeProviderUsageStateSnapshot_LoadingCopyWithImpl<BridgeProviderUsageStateSnapshot_Loading>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageStateSnapshot_Loading&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeProviderUsageStateSnapshot.loading(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProviderUsageStateSnapshot_LoadingCopyWith<$Res> implements $BridgeProviderUsageStateSnapshotCopyWith<$Res> {
  factory $BridgeProviderUsageStateSnapshot_LoadingCopyWith(BridgeProviderUsageStateSnapshot_Loading value, $Res Function(BridgeProviderUsageStateSnapshot_Loading) _then) = _$BridgeProviderUsageStateSnapshot_LoadingCopyWithImpl;
@useResult
$Res call({
 BridgeLoadingResource field0
});




}
/// @nodoc
class _$BridgeProviderUsageStateSnapshot_LoadingCopyWithImpl<$Res>
    implements $BridgeProviderUsageStateSnapshot_LoadingCopyWith<$Res> {
  _$BridgeProviderUsageStateSnapshot_LoadingCopyWithImpl(this._self, this._then);

  final BridgeProviderUsageStateSnapshot_Loading _self;
  final $Res Function(BridgeProviderUsageStateSnapshot_Loading) _then;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProviderUsageStateSnapshot_Loading(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeLoadingResource,
  ));
}


}

/// @nodoc


class BridgeProviderUsageStateSnapshot_Ready extends BridgeProviderUsageStateSnapshot {
  const BridgeProviderUsageStateSnapshot_Ready({required this.resource, required this.value}): super._();


 final  BridgeReadyResource resource;
 final  BridgeProviderUsageStateData value;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProviderUsageStateSnapshot_ReadyCopyWith<BridgeProviderUsageStateSnapshot_Ready> get copyWith => _$BridgeProviderUsageStateSnapshot_ReadyCopyWithImpl<BridgeProviderUsageStateSnapshot_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageStateSnapshot_Ready&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeProviderUsageStateSnapshot.ready(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeProviderUsageStateSnapshot_ReadyCopyWith<$Res> implements $BridgeProviderUsageStateSnapshotCopyWith<$Res> {
  factory $BridgeProviderUsageStateSnapshot_ReadyCopyWith(BridgeProviderUsageStateSnapshot_Ready value, $Res Function(BridgeProviderUsageStateSnapshot_Ready) _then) = _$BridgeProviderUsageStateSnapshot_ReadyCopyWithImpl;
@useResult
$Res call({
 BridgeReadyResource resource, BridgeProviderUsageStateData value
});




}
/// @nodoc
class _$BridgeProviderUsageStateSnapshot_ReadyCopyWithImpl<$Res>
    implements $BridgeProviderUsageStateSnapshot_ReadyCopyWith<$Res> {
  _$BridgeProviderUsageStateSnapshot_ReadyCopyWithImpl(this._self, this._then);

  final BridgeProviderUsageStateSnapshot_Ready _self;
  final $Res Function(BridgeProviderUsageStateSnapshot_Ready) _then;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeProviderUsageStateSnapshot_Ready(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeReadyResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeProviderUsageStateData,
  ));
}


}

/// @nodoc


class BridgeProviderUsageStateSnapshot_Refreshing extends BridgeProviderUsageStateSnapshot {
  const BridgeProviderUsageStateSnapshot_Refreshing({required this.resource, required this.value}): super._();


 final  BridgeRefreshingResource resource;
 final  BridgeProviderUsageStateData value;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProviderUsageStateSnapshot_RefreshingCopyWith<BridgeProviderUsageStateSnapshot_Refreshing> get copyWith => _$BridgeProviderUsageStateSnapshot_RefreshingCopyWithImpl<BridgeProviderUsageStateSnapshot_Refreshing>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageStateSnapshot_Refreshing&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeProviderUsageStateSnapshot.refreshing(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeProviderUsageStateSnapshot_RefreshingCopyWith<$Res> implements $BridgeProviderUsageStateSnapshotCopyWith<$Res> {
  factory $BridgeProviderUsageStateSnapshot_RefreshingCopyWith(BridgeProviderUsageStateSnapshot_Refreshing value, $Res Function(BridgeProviderUsageStateSnapshot_Refreshing) _then) = _$BridgeProviderUsageStateSnapshot_RefreshingCopyWithImpl;
@useResult
$Res call({
 BridgeRefreshingResource resource, BridgeProviderUsageStateData value
});




}
/// @nodoc
class _$BridgeProviderUsageStateSnapshot_RefreshingCopyWithImpl<$Res>
    implements $BridgeProviderUsageStateSnapshot_RefreshingCopyWith<$Res> {
  _$BridgeProviderUsageStateSnapshot_RefreshingCopyWithImpl(this._self, this._then);

  final BridgeProviderUsageStateSnapshot_Refreshing _self;
  final $Res Function(BridgeProviderUsageStateSnapshot_Refreshing) _then;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeProviderUsageStateSnapshot_Refreshing(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeRefreshingResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeProviderUsageStateData,
  ));
}


}

/// @nodoc


class BridgeProviderUsageStateSnapshot_Stale extends BridgeProviderUsageStateSnapshot {
  const BridgeProviderUsageStateSnapshot_Stale({required this.resource, required this.value}): super._();


 final  BridgeStaleResource resource;
 final  BridgeProviderUsageStateData value;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProviderUsageStateSnapshot_StaleCopyWith<BridgeProviderUsageStateSnapshot_Stale> get copyWith => _$BridgeProviderUsageStateSnapshot_StaleCopyWithImpl<BridgeProviderUsageStateSnapshot_Stale>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageStateSnapshot_Stale&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeProviderUsageStateSnapshot.stale(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeProviderUsageStateSnapshot_StaleCopyWith<$Res> implements $BridgeProviderUsageStateSnapshotCopyWith<$Res> {
  factory $BridgeProviderUsageStateSnapshot_StaleCopyWith(BridgeProviderUsageStateSnapshot_Stale value, $Res Function(BridgeProviderUsageStateSnapshot_Stale) _then) = _$BridgeProviderUsageStateSnapshot_StaleCopyWithImpl;
@useResult
$Res call({
 BridgeStaleResource resource, BridgeProviderUsageStateData value
});




}
/// @nodoc
class _$BridgeProviderUsageStateSnapshot_StaleCopyWithImpl<$Res>
    implements $BridgeProviderUsageStateSnapshot_StaleCopyWith<$Res> {
  _$BridgeProviderUsageStateSnapshot_StaleCopyWithImpl(this._self, this._then);

  final BridgeProviderUsageStateSnapshot_Stale _self;
  final $Res Function(BridgeProviderUsageStateSnapshot_Stale) _then;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeProviderUsageStateSnapshot_Stale(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeStaleResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeProviderUsageStateData,
  ));
}


}

/// @nodoc


class BridgeProviderUsageStateSnapshot_Degraded extends BridgeProviderUsageStateSnapshot {
  const BridgeProviderUsageStateSnapshot_Degraded({required this.resource, required this.value}): super._();


 final  BridgeDegradedResource resource;
 final  BridgeProviderUsageStateData value;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProviderUsageStateSnapshot_DegradedCopyWith<BridgeProviderUsageStateSnapshot_Degraded> get copyWith => _$BridgeProviderUsageStateSnapshot_DegradedCopyWithImpl<BridgeProviderUsageStateSnapshot_Degraded>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageStateSnapshot_Degraded&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeProviderUsageStateSnapshot.degraded(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeProviderUsageStateSnapshot_DegradedCopyWith<$Res> implements $BridgeProviderUsageStateSnapshotCopyWith<$Res> {
  factory $BridgeProviderUsageStateSnapshot_DegradedCopyWith(BridgeProviderUsageStateSnapshot_Degraded value, $Res Function(BridgeProviderUsageStateSnapshot_Degraded) _then) = _$BridgeProviderUsageStateSnapshot_DegradedCopyWithImpl;
@useResult
$Res call({
 BridgeDegradedResource resource, BridgeProviderUsageStateData value
});




}
/// @nodoc
class _$BridgeProviderUsageStateSnapshot_DegradedCopyWithImpl<$Res>
    implements $BridgeProviderUsageStateSnapshot_DegradedCopyWith<$Res> {
  _$BridgeProviderUsageStateSnapshot_DegradedCopyWithImpl(this._self, this._then);

  final BridgeProviderUsageStateSnapshot_Degraded _self;
  final $Res Function(BridgeProviderUsageStateSnapshot_Degraded) _then;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeProviderUsageStateSnapshot_Degraded(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeDegradedResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeProviderUsageStateData,
  ));
}


}

/// @nodoc


class BridgeProviderUsageStateSnapshot_Failed extends BridgeProviderUsageStateSnapshot {
  const BridgeProviderUsageStateSnapshot_Failed(this.field0): super._();


 final  BridgeFailedResource field0;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProviderUsageStateSnapshot_FailedCopyWith<BridgeProviderUsageStateSnapshot_Failed> get copyWith => _$BridgeProviderUsageStateSnapshot_FailedCopyWithImpl<BridgeProviderUsageStateSnapshot_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageStateSnapshot_Failed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeProviderUsageStateSnapshot.failed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProviderUsageStateSnapshot_FailedCopyWith<$Res> implements $BridgeProviderUsageStateSnapshotCopyWith<$Res> {
  factory $BridgeProviderUsageStateSnapshot_FailedCopyWith(BridgeProviderUsageStateSnapshot_Failed value, $Res Function(BridgeProviderUsageStateSnapshot_Failed) _then) = _$BridgeProviderUsageStateSnapshot_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeFailedResource field0
});




}
/// @nodoc
class _$BridgeProviderUsageStateSnapshot_FailedCopyWithImpl<$Res>
    implements $BridgeProviderUsageStateSnapshot_FailedCopyWith<$Res> {
  _$BridgeProviderUsageStateSnapshot_FailedCopyWithImpl(this._self, this._then);

  final BridgeProviderUsageStateSnapshot_Failed _self;
  final $Res Function(BridgeProviderUsageStateSnapshot_Failed) _then;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProviderUsageStateSnapshot_Failed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeFailedResource,
  ));
}


}

/// @nodoc


class BridgeProviderUsageStateSnapshot_Stopped extends BridgeProviderUsageStateSnapshot {
  const BridgeProviderUsageStateSnapshot_Stopped(this.field0): super._();


 final  BridgeStoppedResource field0;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProviderUsageStateSnapshot_StoppedCopyWith<BridgeProviderUsageStateSnapshot_Stopped> get copyWith => _$BridgeProviderUsageStateSnapshot_StoppedCopyWithImpl<BridgeProviderUsageStateSnapshot_Stopped>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProviderUsageStateSnapshot_Stopped&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeProviderUsageStateSnapshot.stopped(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeProviderUsageStateSnapshot_StoppedCopyWith<$Res> implements $BridgeProviderUsageStateSnapshotCopyWith<$Res> {
  factory $BridgeProviderUsageStateSnapshot_StoppedCopyWith(BridgeProviderUsageStateSnapshot_Stopped value, $Res Function(BridgeProviderUsageStateSnapshot_Stopped) _then) = _$BridgeProviderUsageStateSnapshot_StoppedCopyWithImpl;
@useResult
$Res call({
 BridgeStoppedResource field0
});




}
/// @nodoc
class _$BridgeProviderUsageStateSnapshot_StoppedCopyWithImpl<$Res>
    implements $BridgeProviderUsageStateSnapshot_StoppedCopyWith<$Res> {
  _$BridgeProviderUsageStateSnapshot_StoppedCopyWithImpl(this._self, this._then);

  final BridgeProviderUsageStateSnapshot_Stopped _self;
  final $Res Function(BridgeProviderUsageStateSnapshot_Stopped) _then;

/// Create a copy of BridgeProviderUsageStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeProviderUsageStateSnapshot_Stopped(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeStoppedResource,
  ));
}


}

/// @nodoc
mixin _$BridgeRecoveryStateSnapshot {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRecoveryStateSnapshot);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeRecoveryStateSnapshot()';
}


}

/// @nodoc
class $BridgeRecoveryStateSnapshotCopyWith<$Res>  {
$BridgeRecoveryStateSnapshotCopyWith(BridgeRecoveryStateSnapshot _, $Res Function(BridgeRecoveryStateSnapshot) __);
}


/// Adds pattern-matching-related methods to [BridgeRecoveryStateSnapshot].
extension BridgeRecoveryStateSnapshotPatterns on BridgeRecoveryStateSnapshot {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeRecoveryStateSnapshot_Uninitialized value)?  uninitialized,TResult Function( BridgeRecoveryStateSnapshot_Loading value)?  loading,TResult Function( BridgeRecoveryStateSnapshot_Ready value)?  ready,TResult Function( BridgeRecoveryStateSnapshot_Refreshing value)?  refreshing,TResult Function( BridgeRecoveryStateSnapshot_Stale value)?  stale,TResult Function( BridgeRecoveryStateSnapshot_Degraded value)?  degraded,TResult Function( BridgeRecoveryStateSnapshot_Failed value)?  failed,TResult Function( BridgeRecoveryStateSnapshot_Stopped value)?  stopped,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeRecoveryStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeRecoveryStateSnapshot_Loading() when loading != null:
return loading(_that);case BridgeRecoveryStateSnapshot_Ready() when ready != null:
return ready(_that);case BridgeRecoveryStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeRecoveryStateSnapshot_Stale() when stale != null:
return stale(_that);case BridgeRecoveryStateSnapshot_Degraded() when degraded != null:
return degraded(_that);case BridgeRecoveryStateSnapshot_Failed() when failed != null:
return failed(_that);case BridgeRecoveryStateSnapshot_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeRecoveryStateSnapshot_Uninitialized value)  uninitialized,required TResult Function( BridgeRecoveryStateSnapshot_Loading value)  loading,required TResult Function( BridgeRecoveryStateSnapshot_Ready value)  ready,required TResult Function( BridgeRecoveryStateSnapshot_Refreshing value)  refreshing,required TResult Function( BridgeRecoveryStateSnapshot_Stale value)  stale,required TResult Function( BridgeRecoveryStateSnapshot_Degraded value)  degraded,required TResult Function( BridgeRecoveryStateSnapshot_Failed value)  failed,required TResult Function( BridgeRecoveryStateSnapshot_Stopped value)  stopped,}){
final _that = this;
switch (_that) {
case BridgeRecoveryStateSnapshot_Uninitialized():
return uninitialized(_that);case BridgeRecoveryStateSnapshot_Loading():
return loading(_that);case BridgeRecoveryStateSnapshot_Ready():
return ready(_that);case BridgeRecoveryStateSnapshot_Refreshing():
return refreshing(_that);case BridgeRecoveryStateSnapshot_Stale():
return stale(_that);case BridgeRecoveryStateSnapshot_Degraded():
return degraded(_that);case BridgeRecoveryStateSnapshot_Failed():
return failed(_that);case BridgeRecoveryStateSnapshot_Stopped():
return stopped(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeRecoveryStateSnapshot_Uninitialized value)?  uninitialized,TResult? Function( BridgeRecoveryStateSnapshot_Loading value)?  loading,TResult? Function( BridgeRecoveryStateSnapshot_Ready value)?  ready,TResult? Function( BridgeRecoveryStateSnapshot_Refreshing value)?  refreshing,TResult? Function( BridgeRecoveryStateSnapshot_Stale value)?  stale,TResult? Function( BridgeRecoveryStateSnapshot_Degraded value)?  degraded,TResult? Function( BridgeRecoveryStateSnapshot_Failed value)?  failed,TResult? Function( BridgeRecoveryStateSnapshot_Stopped value)?  stopped,}){
final _that = this;
switch (_that) {
case BridgeRecoveryStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeRecoveryStateSnapshot_Loading() when loading != null:
return loading(_that);case BridgeRecoveryStateSnapshot_Ready() when ready != null:
return ready(_that);case BridgeRecoveryStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeRecoveryStateSnapshot_Stale() when stale != null:
return stale(_that);case BridgeRecoveryStateSnapshot_Degraded() when degraded != null:
return degraded(_that);case BridgeRecoveryStateSnapshot_Failed() when failed != null:
return failed(_that);case BridgeRecoveryStateSnapshot_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeUninitializedResource field0)?  uninitialized,TResult Function( BridgeLoadingResource field0)?  loading,TResult Function( BridgeReadyResource resource,  BridgeRecoveryStateData value)?  ready,TResult Function( BridgeRefreshingResource resource,  BridgeRecoveryStateData value)?  refreshing,TResult Function( BridgeStaleResource resource,  BridgeRecoveryStateData value)?  stale,TResult Function( BridgeDegradedResource resource,  BridgeRecoveryStateData value)?  degraded,TResult Function( BridgeFailedResource field0)?  failed,TResult Function( BridgeStoppedResource field0)?  stopped,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeRecoveryStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeRecoveryStateSnapshot_Loading() when loading != null:
return loading(_that.field0);case BridgeRecoveryStateSnapshot_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeRecoveryStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeRecoveryStateSnapshot_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeRecoveryStateSnapshot_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeRecoveryStateSnapshot_Failed() when failed != null:
return failed(_that.field0);case BridgeRecoveryStateSnapshot_Stopped() when stopped != null:
return stopped(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeUninitializedResource field0)  uninitialized,required TResult Function( BridgeLoadingResource field0)  loading,required TResult Function( BridgeReadyResource resource,  BridgeRecoveryStateData value)  ready,required TResult Function( BridgeRefreshingResource resource,  BridgeRecoveryStateData value)  refreshing,required TResult Function( BridgeStaleResource resource,  BridgeRecoveryStateData value)  stale,required TResult Function( BridgeDegradedResource resource,  BridgeRecoveryStateData value)  degraded,required TResult Function( BridgeFailedResource field0)  failed,required TResult Function( BridgeStoppedResource field0)  stopped,}) {final _that = this;
switch (_that) {
case BridgeRecoveryStateSnapshot_Uninitialized():
return uninitialized(_that.field0);case BridgeRecoveryStateSnapshot_Loading():
return loading(_that.field0);case BridgeRecoveryStateSnapshot_Ready():
return ready(_that.resource,_that.value);case BridgeRecoveryStateSnapshot_Refreshing():
return refreshing(_that.resource,_that.value);case BridgeRecoveryStateSnapshot_Stale():
return stale(_that.resource,_that.value);case BridgeRecoveryStateSnapshot_Degraded():
return degraded(_that.resource,_that.value);case BridgeRecoveryStateSnapshot_Failed():
return failed(_that.field0);case BridgeRecoveryStateSnapshot_Stopped():
return stopped(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeUninitializedResource field0)?  uninitialized,TResult? Function( BridgeLoadingResource field0)?  loading,TResult? Function( BridgeReadyResource resource,  BridgeRecoveryStateData value)?  ready,TResult? Function( BridgeRefreshingResource resource,  BridgeRecoveryStateData value)?  refreshing,TResult? Function( BridgeStaleResource resource,  BridgeRecoveryStateData value)?  stale,TResult? Function( BridgeDegradedResource resource,  BridgeRecoveryStateData value)?  degraded,TResult? Function( BridgeFailedResource field0)?  failed,TResult? Function( BridgeStoppedResource field0)?  stopped,}) {final _that = this;
switch (_that) {
case BridgeRecoveryStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeRecoveryStateSnapshot_Loading() when loading != null:
return loading(_that.field0);case BridgeRecoveryStateSnapshot_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeRecoveryStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeRecoveryStateSnapshot_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeRecoveryStateSnapshot_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeRecoveryStateSnapshot_Failed() when failed != null:
return failed(_that.field0);case BridgeRecoveryStateSnapshot_Stopped() when stopped != null:
return stopped(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeRecoveryStateSnapshot_Uninitialized extends BridgeRecoveryStateSnapshot {
  const BridgeRecoveryStateSnapshot_Uninitialized(this.field0): super._();


 final  BridgeUninitializedResource field0;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRecoveryStateSnapshot_UninitializedCopyWith<BridgeRecoveryStateSnapshot_Uninitialized> get copyWith => _$BridgeRecoveryStateSnapshot_UninitializedCopyWithImpl<BridgeRecoveryStateSnapshot_Uninitialized>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRecoveryStateSnapshot_Uninitialized&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeRecoveryStateSnapshot.uninitialized(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeRecoveryStateSnapshot_UninitializedCopyWith<$Res> implements $BridgeRecoveryStateSnapshotCopyWith<$Res> {
  factory $BridgeRecoveryStateSnapshot_UninitializedCopyWith(BridgeRecoveryStateSnapshot_Uninitialized value, $Res Function(BridgeRecoveryStateSnapshot_Uninitialized) _then) = _$BridgeRecoveryStateSnapshot_UninitializedCopyWithImpl;
@useResult
$Res call({
 BridgeUninitializedResource field0
});




}
/// @nodoc
class _$BridgeRecoveryStateSnapshot_UninitializedCopyWithImpl<$Res>
    implements $BridgeRecoveryStateSnapshot_UninitializedCopyWith<$Res> {
  _$BridgeRecoveryStateSnapshot_UninitializedCopyWithImpl(this._self, this._then);

  final BridgeRecoveryStateSnapshot_Uninitialized _self;
  final $Res Function(BridgeRecoveryStateSnapshot_Uninitialized) _then;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeRecoveryStateSnapshot_Uninitialized(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeUninitializedResource,
  ));
}


}

/// @nodoc


class BridgeRecoveryStateSnapshot_Loading extends BridgeRecoveryStateSnapshot {
  const BridgeRecoveryStateSnapshot_Loading(this.field0): super._();


 final  BridgeLoadingResource field0;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRecoveryStateSnapshot_LoadingCopyWith<BridgeRecoveryStateSnapshot_Loading> get copyWith => _$BridgeRecoveryStateSnapshot_LoadingCopyWithImpl<BridgeRecoveryStateSnapshot_Loading>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRecoveryStateSnapshot_Loading&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeRecoveryStateSnapshot.loading(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeRecoveryStateSnapshot_LoadingCopyWith<$Res> implements $BridgeRecoveryStateSnapshotCopyWith<$Res> {
  factory $BridgeRecoveryStateSnapshot_LoadingCopyWith(BridgeRecoveryStateSnapshot_Loading value, $Res Function(BridgeRecoveryStateSnapshot_Loading) _then) = _$BridgeRecoveryStateSnapshot_LoadingCopyWithImpl;
@useResult
$Res call({
 BridgeLoadingResource field0
});




}
/// @nodoc
class _$BridgeRecoveryStateSnapshot_LoadingCopyWithImpl<$Res>
    implements $BridgeRecoveryStateSnapshot_LoadingCopyWith<$Res> {
  _$BridgeRecoveryStateSnapshot_LoadingCopyWithImpl(this._self, this._then);

  final BridgeRecoveryStateSnapshot_Loading _self;
  final $Res Function(BridgeRecoveryStateSnapshot_Loading) _then;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeRecoveryStateSnapshot_Loading(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeLoadingResource,
  ));
}


}

/// @nodoc


class BridgeRecoveryStateSnapshot_Ready extends BridgeRecoveryStateSnapshot {
  const BridgeRecoveryStateSnapshot_Ready({required this.resource, required this.value}): super._();


 final  BridgeReadyResource resource;
 final  BridgeRecoveryStateData value;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRecoveryStateSnapshot_ReadyCopyWith<BridgeRecoveryStateSnapshot_Ready> get copyWith => _$BridgeRecoveryStateSnapshot_ReadyCopyWithImpl<BridgeRecoveryStateSnapshot_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRecoveryStateSnapshot_Ready&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeRecoveryStateSnapshot.ready(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeRecoveryStateSnapshot_ReadyCopyWith<$Res> implements $BridgeRecoveryStateSnapshotCopyWith<$Res> {
  factory $BridgeRecoveryStateSnapshot_ReadyCopyWith(BridgeRecoveryStateSnapshot_Ready value, $Res Function(BridgeRecoveryStateSnapshot_Ready) _then) = _$BridgeRecoveryStateSnapshot_ReadyCopyWithImpl;
@useResult
$Res call({
 BridgeReadyResource resource, BridgeRecoveryStateData value
});




}
/// @nodoc
class _$BridgeRecoveryStateSnapshot_ReadyCopyWithImpl<$Res>
    implements $BridgeRecoveryStateSnapshot_ReadyCopyWith<$Res> {
  _$BridgeRecoveryStateSnapshot_ReadyCopyWithImpl(this._self, this._then);

  final BridgeRecoveryStateSnapshot_Ready _self;
  final $Res Function(BridgeRecoveryStateSnapshot_Ready) _then;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeRecoveryStateSnapshot_Ready(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeReadyResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeRecoveryStateData,
  ));
}


}

/// @nodoc


class BridgeRecoveryStateSnapshot_Refreshing extends BridgeRecoveryStateSnapshot {
  const BridgeRecoveryStateSnapshot_Refreshing({required this.resource, required this.value}): super._();


 final  BridgeRefreshingResource resource;
 final  BridgeRecoveryStateData value;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRecoveryStateSnapshot_RefreshingCopyWith<BridgeRecoveryStateSnapshot_Refreshing> get copyWith => _$BridgeRecoveryStateSnapshot_RefreshingCopyWithImpl<BridgeRecoveryStateSnapshot_Refreshing>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRecoveryStateSnapshot_Refreshing&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeRecoveryStateSnapshot.refreshing(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeRecoveryStateSnapshot_RefreshingCopyWith<$Res> implements $BridgeRecoveryStateSnapshotCopyWith<$Res> {
  factory $BridgeRecoveryStateSnapshot_RefreshingCopyWith(BridgeRecoveryStateSnapshot_Refreshing value, $Res Function(BridgeRecoveryStateSnapshot_Refreshing) _then) = _$BridgeRecoveryStateSnapshot_RefreshingCopyWithImpl;
@useResult
$Res call({
 BridgeRefreshingResource resource, BridgeRecoveryStateData value
});




}
/// @nodoc
class _$BridgeRecoveryStateSnapshot_RefreshingCopyWithImpl<$Res>
    implements $BridgeRecoveryStateSnapshot_RefreshingCopyWith<$Res> {
  _$BridgeRecoveryStateSnapshot_RefreshingCopyWithImpl(this._self, this._then);

  final BridgeRecoveryStateSnapshot_Refreshing _self;
  final $Res Function(BridgeRecoveryStateSnapshot_Refreshing) _then;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeRecoveryStateSnapshot_Refreshing(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeRefreshingResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeRecoveryStateData,
  ));
}


}

/// @nodoc


class BridgeRecoveryStateSnapshot_Stale extends BridgeRecoveryStateSnapshot {
  const BridgeRecoveryStateSnapshot_Stale({required this.resource, required this.value}): super._();


 final  BridgeStaleResource resource;
 final  BridgeRecoveryStateData value;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRecoveryStateSnapshot_StaleCopyWith<BridgeRecoveryStateSnapshot_Stale> get copyWith => _$BridgeRecoveryStateSnapshot_StaleCopyWithImpl<BridgeRecoveryStateSnapshot_Stale>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRecoveryStateSnapshot_Stale&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeRecoveryStateSnapshot.stale(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeRecoveryStateSnapshot_StaleCopyWith<$Res> implements $BridgeRecoveryStateSnapshotCopyWith<$Res> {
  factory $BridgeRecoveryStateSnapshot_StaleCopyWith(BridgeRecoveryStateSnapshot_Stale value, $Res Function(BridgeRecoveryStateSnapshot_Stale) _then) = _$BridgeRecoveryStateSnapshot_StaleCopyWithImpl;
@useResult
$Res call({
 BridgeStaleResource resource, BridgeRecoveryStateData value
});




}
/// @nodoc
class _$BridgeRecoveryStateSnapshot_StaleCopyWithImpl<$Res>
    implements $BridgeRecoveryStateSnapshot_StaleCopyWith<$Res> {
  _$BridgeRecoveryStateSnapshot_StaleCopyWithImpl(this._self, this._then);

  final BridgeRecoveryStateSnapshot_Stale _self;
  final $Res Function(BridgeRecoveryStateSnapshot_Stale) _then;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeRecoveryStateSnapshot_Stale(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeStaleResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeRecoveryStateData,
  ));
}


}

/// @nodoc


class BridgeRecoveryStateSnapshot_Degraded extends BridgeRecoveryStateSnapshot {
  const BridgeRecoveryStateSnapshot_Degraded({required this.resource, required this.value}): super._();


 final  BridgeDegradedResource resource;
 final  BridgeRecoveryStateData value;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRecoveryStateSnapshot_DegradedCopyWith<BridgeRecoveryStateSnapshot_Degraded> get copyWith => _$BridgeRecoveryStateSnapshot_DegradedCopyWithImpl<BridgeRecoveryStateSnapshot_Degraded>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRecoveryStateSnapshot_Degraded&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeRecoveryStateSnapshot.degraded(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeRecoveryStateSnapshot_DegradedCopyWith<$Res> implements $BridgeRecoveryStateSnapshotCopyWith<$Res> {
  factory $BridgeRecoveryStateSnapshot_DegradedCopyWith(BridgeRecoveryStateSnapshot_Degraded value, $Res Function(BridgeRecoveryStateSnapshot_Degraded) _then) = _$BridgeRecoveryStateSnapshot_DegradedCopyWithImpl;
@useResult
$Res call({
 BridgeDegradedResource resource, BridgeRecoveryStateData value
});




}
/// @nodoc
class _$BridgeRecoveryStateSnapshot_DegradedCopyWithImpl<$Res>
    implements $BridgeRecoveryStateSnapshot_DegradedCopyWith<$Res> {
  _$BridgeRecoveryStateSnapshot_DegradedCopyWithImpl(this._self, this._then);

  final BridgeRecoveryStateSnapshot_Degraded _self;
  final $Res Function(BridgeRecoveryStateSnapshot_Degraded) _then;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeRecoveryStateSnapshot_Degraded(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeDegradedResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeRecoveryStateData,
  ));
}


}

/// @nodoc


class BridgeRecoveryStateSnapshot_Failed extends BridgeRecoveryStateSnapshot {
  const BridgeRecoveryStateSnapshot_Failed(this.field0): super._();


 final  BridgeFailedResource field0;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRecoveryStateSnapshot_FailedCopyWith<BridgeRecoveryStateSnapshot_Failed> get copyWith => _$BridgeRecoveryStateSnapshot_FailedCopyWithImpl<BridgeRecoveryStateSnapshot_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRecoveryStateSnapshot_Failed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeRecoveryStateSnapshot.failed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeRecoveryStateSnapshot_FailedCopyWith<$Res> implements $BridgeRecoveryStateSnapshotCopyWith<$Res> {
  factory $BridgeRecoveryStateSnapshot_FailedCopyWith(BridgeRecoveryStateSnapshot_Failed value, $Res Function(BridgeRecoveryStateSnapshot_Failed) _then) = _$BridgeRecoveryStateSnapshot_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeFailedResource field0
});




}
/// @nodoc
class _$BridgeRecoveryStateSnapshot_FailedCopyWithImpl<$Res>
    implements $BridgeRecoveryStateSnapshot_FailedCopyWith<$Res> {
  _$BridgeRecoveryStateSnapshot_FailedCopyWithImpl(this._self, this._then);

  final BridgeRecoveryStateSnapshot_Failed _self;
  final $Res Function(BridgeRecoveryStateSnapshot_Failed) _then;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeRecoveryStateSnapshot_Failed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeFailedResource,
  ));
}


}

/// @nodoc


class BridgeRecoveryStateSnapshot_Stopped extends BridgeRecoveryStateSnapshot {
  const BridgeRecoveryStateSnapshot_Stopped(this.field0): super._();


 final  BridgeStoppedResource field0;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRecoveryStateSnapshot_StoppedCopyWith<BridgeRecoveryStateSnapshot_Stopped> get copyWith => _$BridgeRecoveryStateSnapshot_StoppedCopyWithImpl<BridgeRecoveryStateSnapshot_Stopped>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRecoveryStateSnapshot_Stopped&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeRecoveryStateSnapshot.stopped(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeRecoveryStateSnapshot_StoppedCopyWith<$Res> implements $BridgeRecoveryStateSnapshotCopyWith<$Res> {
  factory $BridgeRecoveryStateSnapshot_StoppedCopyWith(BridgeRecoveryStateSnapshot_Stopped value, $Res Function(BridgeRecoveryStateSnapshot_Stopped) _then) = _$BridgeRecoveryStateSnapshot_StoppedCopyWithImpl;
@useResult
$Res call({
 BridgeStoppedResource field0
});




}
/// @nodoc
class _$BridgeRecoveryStateSnapshot_StoppedCopyWithImpl<$Res>
    implements $BridgeRecoveryStateSnapshot_StoppedCopyWith<$Res> {
  _$BridgeRecoveryStateSnapshot_StoppedCopyWithImpl(this._self, this._then);

  final BridgeRecoveryStateSnapshot_Stopped _self;
  final $Res Function(BridgeRecoveryStateSnapshot_Stopped) _then;

/// Create a copy of BridgeRecoveryStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeRecoveryStateSnapshot_Stopped(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeStoppedResource,
  ));
}


}

/// @nodoc
mixin _$BridgeSettingsConfigStateSnapshot {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSettingsConfigStateSnapshot);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeSettingsConfigStateSnapshot()';
}


}

/// @nodoc
class $BridgeSettingsConfigStateSnapshotCopyWith<$Res>  {
$BridgeSettingsConfigStateSnapshotCopyWith(BridgeSettingsConfigStateSnapshot _, $Res Function(BridgeSettingsConfigStateSnapshot) __);
}


/// Adds pattern-matching-related methods to [BridgeSettingsConfigStateSnapshot].
extension BridgeSettingsConfigStateSnapshotPatterns on BridgeSettingsConfigStateSnapshot {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeSettingsConfigStateSnapshot_Uninitialized value)?  uninitialized,TResult Function( BridgeSettingsConfigStateSnapshot_Loading value)?  loading,TResult Function( BridgeSettingsConfigStateSnapshot_Ready value)?  ready,TResult Function( BridgeSettingsConfigStateSnapshot_Refreshing value)?  refreshing,TResult Function( BridgeSettingsConfigStateSnapshot_Stale value)?  stale,TResult Function( BridgeSettingsConfigStateSnapshot_Degraded value)?  degraded,TResult Function( BridgeSettingsConfigStateSnapshot_Failed value)?  failed,TResult Function( BridgeSettingsConfigStateSnapshot_Stopped value)?  stopped,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeSettingsConfigStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeSettingsConfigStateSnapshot_Loading() when loading != null:
return loading(_that);case BridgeSettingsConfigStateSnapshot_Ready() when ready != null:
return ready(_that);case BridgeSettingsConfigStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeSettingsConfigStateSnapshot_Stale() when stale != null:
return stale(_that);case BridgeSettingsConfigStateSnapshot_Degraded() when degraded != null:
return degraded(_that);case BridgeSettingsConfigStateSnapshot_Failed() when failed != null:
return failed(_that);case BridgeSettingsConfigStateSnapshot_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeSettingsConfigStateSnapshot_Uninitialized value)  uninitialized,required TResult Function( BridgeSettingsConfigStateSnapshot_Loading value)  loading,required TResult Function( BridgeSettingsConfigStateSnapshot_Ready value)  ready,required TResult Function( BridgeSettingsConfigStateSnapshot_Refreshing value)  refreshing,required TResult Function( BridgeSettingsConfigStateSnapshot_Stale value)  stale,required TResult Function( BridgeSettingsConfigStateSnapshot_Degraded value)  degraded,required TResult Function( BridgeSettingsConfigStateSnapshot_Failed value)  failed,required TResult Function( BridgeSettingsConfigStateSnapshot_Stopped value)  stopped,}){
final _that = this;
switch (_that) {
case BridgeSettingsConfigStateSnapshot_Uninitialized():
return uninitialized(_that);case BridgeSettingsConfigStateSnapshot_Loading():
return loading(_that);case BridgeSettingsConfigStateSnapshot_Ready():
return ready(_that);case BridgeSettingsConfigStateSnapshot_Refreshing():
return refreshing(_that);case BridgeSettingsConfigStateSnapshot_Stale():
return stale(_that);case BridgeSettingsConfigStateSnapshot_Degraded():
return degraded(_that);case BridgeSettingsConfigStateSnapshot_Failed():
return failed(_that);case BridgeSettingsConfigStateSnapshot_Stopped():
return stopped(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeSettingsConfigStateSnapshot_Uninitialized value)?  uninitialized,TResult? Function( BridgeSettingsConfigStateSnapshot_Loading value)?  loading,TResult? Function( BridgeSettingsConfigStateSnapshot_Ready value)?  ready,TResult? Function( BridgeSettingsConfigStateSnapshot_Refreshing value)?  refreshing,TResult? Function( BridgeSettingsConfigStateSnapshot_Stale value)?  stale,TResult? Function( BridgeSettingsConfigStateSnapshot_Degraded value)?  degraded,TResult? Function( BridgeSettingsConfigStateSnapshot_Failed value)?  failed,TResult? Function( BridgeSettingsConfigStateSnapshot_Stopped value)?  stopped,}){
final _that = this;
switch (_that) {
case BridgeSettingsConfigStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeSettingsConfigStateSnapshot_Loading() when loading != null:
return loading(_that);case BridgeSettingsConfigStateSnapshot_Ready() when ready != null:
return ready(_that);case BridgeSettingsConfigStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeSettingsConfigStateSnapshot_Stale() when stale != null:
return stale(_that);case BridgeSettingsConfigStateSnapshot_Degraded() when degraded != null:
return degraded(_that);case BridgeSettingsConfigStateSnapshot_Failed() when failed != null:
return failed(_that);case BridgeSettingsConfigStateSnapshot_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeUninitializedResource field0)?  uninitialized,TResult Function( BridgeLoadingResource field0)?  loading,TResult Function( BridgeReadyResource resource,  BridgeSettingsConfigStateData value)?  ready,TResult Function( BridgeRefreshingResource resource,  BridgeSettingsConfigStateData value)?  refreshing,TResult Function( BridgeStaleResource resource,  BridgeSettingsConfigStateData value)?  stale,TResult Function( BridgeDegradedResource resource,  BridgeSettingsConfigStateData value)?  degraded,TResult Function( BridgeFailedResource field0)?  failed,TResult Function( BridgeStoppedResource field0)?  stopped,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeSettingsConfigStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeSettingsConfigStateSnapshot_Loading() when loading != null:
return loading(_that.field0);case BridgeSettingsConfigStateSnapshot_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeSettingsConfigStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeSettingsConfigStateSnapshot_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeSettingsConfigStateSnapshot_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeSettingsConfigStateSnapshot_Failed() when failed != null:
return failed(_that.field0);case BridgeSettingsConfigStateSnapshot_Stopped() when stopped != null:
return stopped(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeUninitializedResource field0)  uninitialized,required TResult Function( BridgeLoadingResource field0)  loading,required TResult Function( BridgeReadyResource resource,  BridgeSettingsConfigStateData value)  ready,required TResult Function( BridgeRefreshingResource resource,  BridgeSettingsConfigStateData value)  refreshing,required TResult Function( BridgeStaleResource resource,  BridgeSettingsConfigStateData value)  stale,required TResult Function( BridgeDegradedResource resource,  BridgeSettingsConfigStateData value)  degraded,required TResult Function( BridgeFailedResource field0)  failed,required TResult Function( BridgeStoppedResource field0)  stopped,}) {final _that = this;
switch (_that) {
case BridgeSettingsConfigStateSnapshot_Uninitialized():
return uninitialized(_that.field0);case BridgeSettingsConfigStateSnapshot_Loading():
return loading(_that.field0);case BridgeSettingsConfigStateSnapshot_Ready():
return ready(_that.resource,_that.value);case BridgeSettingsConfigStateSnapshot_Refreshing():
return refreshing(_that.resource,_that.value);case BridgeSettingsConfigStateSnapshot_Stale():
return stale(_that.resource,_that.value);case BridgeSettingsConfigStateSnapshot_Degraded():
return degraded(_that.resource,_that.value);case BridgeSettingsConfigStateSnapshot_Failed():
return failed(_that.field0);case BridgeSettingsConfigStateSnapshot_Stopped():
return stopped(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeUninitializedResource field0)?  uninitialized,TResult? Function( BridgeLoadingResource field0)?  loading,TResult? Function( BridgeReadyResource resource,  BridgeSettingsConfigStateData value)?  ready,TResult? Function( BridgeRefreshingResource resource,  BridgeSettingsConfigStateData value)?  refreshing,TResult? Function( BridgeStaleResource resource,  BridgeSettingsConfigStateData value)?  stale,TResult? Function( BridgeDegradedResource resource,  BridgeSettingsConfigStateData value)?  degraded,TResult? Function( BridgeFailedResource field0)?  failed,TResult? Function( BridgeStoppedResource field0)?  stopped,}) {final _that = this;
switch (_that) {
case BridgeSettingsConfigStateSnapshot_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeSettingsConfigStateSnapshot_Loading() when loading != null:
return loading(_that.field0);case BridgeSettingsConfigStateSnapshot_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeSettingsConfigStateSnapshot_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeSettingsConfigStateSnapshot_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeSettingsConfigStateSnapshot_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeSettingsConfigStateSnapshot_Failed() when failed != null:
return failed(_that.field0);case BridgeSettingsConfigStateSnapshot_Stopped() when stopped != null:
return stopped(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeSettingsConfigStateSnapshot_Uninitialized extends BridgeSettingsConfigStateSnapshot {
  const BridgeSettingsConfigStateSnapshot_Uninitialized(this.field0): super._();


 final  BridgeUninitializedResource field0;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSettingsConfigStateSnapshot_UninitializedCopyWith<BridgeSettingsConfigStateSnapshot_Uninitialized> get copyWith => _$BridgeSettingsConfigStateSnapshot_UninitializedCopyWithImpl<BridgeSettingsConfigStateSnapshot_Uninitialized>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSettingsConfigStateSnapshot_Uninitialized&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeSettingsConfigStateSnapshot.uninitialized(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeSettingsConfigStateSnapshot_UninitializedCopyWith<$Res> implements $BridgeSettingsConfigStateSnapshotCopyWith<$Res> {
  factory $BridgeSettingsConfigStateSnapshot_UninitializedCopyWith(BridgeSettingsConfigStateSnapshot_Uninitialized value, $Res Function(BridgeSettingsConfigStateSnapshot_Uninitialized) _then) = _$BridgeSettingsConfigStateSnapshot_UninitializedCopyWithImpl;
@useResult
$Res call({
 BridgeUninitializedResource field0
});




}
/// @nodoc
class _$BridgeSettingsConfigStateSnapshot_UninitializedCopyWithImpl<$Res>
    implements $BridgeSettingsConfigStateSnapshot_UninitializedCopyWith<$Res> {
  _$BridgeSettingsConfigStateSnapshot_UninitializedCopyWithImpl(this._self, this._then);

  final BridgeSettingsConfigStateSnapshot_Uninitialized _self;
  final $Res Function(BridgeSettingsConfigStateSnapshot_Uninitialized) _then;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeSettingsConfigStateSnapshot_Uninitialized(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeUninitializedResource,
  ));
}


}

/// @nodoc


class BridgeSettingsConfigStateSnapshot_Loading extends BridgeSettingsConfigStateSnapshot {
  const BridgeSettingsConfigStateSnapshot_Loading(this.field0): super._();


 final  BridgeLoadingResource field0;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSettingsConfigStateSnapshot_LoadingCopyWith<BridgeSettingsConfigStateSnapshot_Loading> get copyWith => _$BridgeSettingsConfigStateSnapshot_LoadingCopyWithImpl<BridgeSettingsConfigStateSnapshot_Loading>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSettingsConfigStateSnapshot_Loading&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeSettingsConfigStateSnapshot.loading(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeSettingsConfigStateSnapshot_LoadingCopyWith<$Res> implements $BridgeSettingsConfigStateSnapshotCopyWith<$Res> {
  factory $BridgeSettingsConfigStateSnapshot_LoadingCopyWith(BridgeSettingsConfigStateSnapshot_Loading value, $Res Function(BridgeSettingsConfigStateSnapshot_Loading) _then) = _$BridgeSettingsConfigStateSnapshot_LoadingCopyWithImpl;
@useResult
$Res call({
 BridgeLoadingResource field0
});




}
/// @nodoc
class _$BridgeSettingsConfigStateSnapshot_LoadingCopyWithImpl<$Res>
    implements $BridgeSettingsConfigStateSnapshot_LoadingCopyWith<$Res> {
  _$BridgeSettingsConfigStateSnapshot_LoadingCopyWithImpl(this._self, this._then);

  final BridgeSettingsConfigStateSnapshot_Loading _self;
  final $Res Function(BridgeSettingsConfigStateSnapshot_Loading) _then;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeSettingsConfigStateSnapshot_Loading(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeLoadingResource,
  ));
}


}

/// @nodoc


class BridgeSettingsConfigStateSnapshot_Ready extends BridgeSettingsConfigStateSnapshot {
  const BridgeSettingsConfigStateSnapshot_Ready({required this.resource, required this.value}): super._();


 final  BridgeReadyResource resource;
 final  BridgeSettingsConfigStateData value;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSettingsConfigStateSnapshot_ReadyCopyWith<BridgeSettingsConfigStateSnapshot_Ready> get copyWith => _$BridgeSettingsConfigStateSnapshot_ReadyCopyWithImpl<BridgeSettingsConfigStateSnapshot_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSettingsConfigStateSnapshot_Ready&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeSettingsConfigStateSnapshot.ready(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeSettingsConfigStateSnapshot_ReadyCopyWith<$Res> implements $BridgeSettingsConfigStateSnapshotCopyWith<$Res> {
  factory $BridgeSettingsConfigStateSnapshot_ReadyCopyWith(BridgeSettingsConfigStateSnapshot_Ready value, $Res Function(BridgeSettingsConfigStateSnapshot_Ready) _then) = _$BridgeSettingsConfigStateSnapshot_ReadyCopyWithImpl;
@useResult
$Res call({
 BridgeReadyResource resource, BridgeSettingsConfigStateData value
});




}
/// @nodoc
class _$BridgeSettingsConfigStateSnapshot_ReadyCopyWithImpl<$Res>
    implements $BridgeSettingsConfigStateSnapshot_ReadyCopyWith<$Res> {
  _$BridgeSettingsConfigStateSnapshot_ReadyCopyWithImpl(this._self, this._then);

  final BridgeSettingsConfigStateSnapshot_Ready _self;
  final $Res Function(BridgeSettingsConfigStateSnapshot_Ready) _then;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeSettingsConfigStateSnapshot_Ready(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeReadyResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeSettingsConfigStateData,
  ));
}


}

/// @nodoc


class BridgeSettingsConfigStateSnapshot_Refreshing extends BridgeSettingsConfigStateSnapshot {
  const BridgeSettingsConfigStateSnapshot_Refreshing({required this.resource, required this.value}): super._();


 final  BridgeRefreshingResource resource;
 final  BridgeSettingsConfigStateData value;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSettingsConfigStateSnapshot_RefreshingCopyWith<BridgeSettingsConfigStateSnapshot_Refreshing> get copyWith => _$BridgeSettingsConfigStateSnapshot_RefreshingCopyWithImpl<BridgeSettingsConfigStateSnapshot_Refreshing>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSettingsConfigStateSnapshot_Refreshing&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeSettingsConfigStateSnapshot.refreshing(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeSettingsConfigStateSnapshot_RefreshingCopyWith<$Res> implements $BridgeSettingsConfigStateSnapshotCopyWith<$Res> {
  factory $BridgeSettingsConfigStateSnapshot_RefreshingCopyWith(BridgeSettingsConfigStateSnapshot_Refreshing value, $Res Function(BridgeSettingsConfigStateSnapshot_Refreshing) _then) = _$BridgeSettingsConfigStateSnapshot_RefreshingCopyWithImpl;
@useResult
$Res call({
 BridgeRefreshingResource resource, BridgeSettingsConfigStateData value
});




}
/// @nodoc
class _$BridgeSettingsConfigStateSnapshot_RefreshingCopyWithImpl<$Res>
    implements $BridgeSettingsConfigStateSnapshot_RefreshingCopyWith<$Res> {
  _$BridgeSettingsConfigStateSnapshot_RefreshingCopyWithImpl(this._self, this._then);

  final BridgeSettingsConfigStateSnapshot_Refreshing _self;
  final $Res Function(BridgeSettingsConfigStateSnapshot_Refreshing) _then;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeSettingsConfigStateSnapshot_Refreshing(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeRefreshingResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeSettingsConfigStateData,
  ));
}


}

/// @nodoc


class BridgeSettingsConfigStateSnapshot_Stale extends BridgeSettingsConfigStateSnapshot {
  const BridgeSettingsConfigStateSnapshot_Stale({required this.resource, required this.value}): super._();


 final  BridgeStaleResource resource;
 final  BridgeSettingsConfigStateData value;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSettingsConfigStateSnapshot_StaleCopyWith<BridgeSettingsConfigStateSnapshot_Stale> get copyWith => _$BridgeSettingsConfigStateSnapshot_StaleCopyWithImpl<BridgeSettingsConfigStateSnapshot_Stale>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSettingsConfigStateSnapshot_Stale&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeSettingsConfigStateSnapshot.stale(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeSettingsConfigStateSnapshot_StaleCopyWith<$Res> implements $BridgeSettingsConfigStateSnapshotCopyWith<$Res> {
  factory $BridgeSettingsConfigStateSnapshot_StaleCopyWith(BridgeSettingsConfigStateSnapshot_Stale value, $Res Function(BridgeSettingsConfigStateSnapshot_Stale) _then) = _$BridgeSettingsConfigStateSnapshot_StaleCopyWithImpl;
@useResult
$Res call({
 BridgeStaleResource resource, BridgeSettingsConfigStateData value
});




}
/// @nodoc
class _$BridgeSettingsConfigStateSnapshot_StaleCopyWithImpl<$Res>
    implements $BridgeSettingsConfigStateSnapshot_StaleCopyWith<$Res> {
  _$BridgeSettingsConfigStateSnapshot_StaleCopyWithImpl(this._self, this._then);

  final BridgeSettingsConfigStateSnapshot_Stale _self;
  final $Res Function(BridgeSettingsConfigStateSnapshot_Stale) _then;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeSettingsConfigStateSnapshot_Stale(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeStaleResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeSettingsConfigStateData,
  ));
}


}

/// @nodoc


class BridgeSettingsConfigStateSnapshot_Degraded extends BridgeSettingsConfigStateSnapshot {
  const BridgeSettingsConfigStateSnapshot_Degraded({required this.resource, required this.value}): super._();


 final  BridgeDegradedResource resource;
 final  BridgeSettingsConfigStateData value;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSettingsConfigStateSnapshot_DegradedCopyWith<BridgeSettingsConfigStateSnapshot_Degraded> get copyWith => _$BridgeSettingsConfigStateSnapshot_DegradedCopyWithImpl<BridgeSettingsConfigStateSnapshot_Degraded>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSettingsConfigStateSnapshot_Degraded&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeSettingsConfigStateSnapshot.degraded(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeSettingsConfigStateSnapshot_DegradedCopyWith<$Res> implements $BridgeSettingsConfigStateSnapshotCopyWith<$Res> {
  factory $BridgeSettingsConfigStateSnapshot_DegradedCopyWith(BridgeSettingsConfigStateSnapshot_Degraded value, $Res Function(BridgeSettingsConfigStateSnapshot_Degraded) _then) = _$BridgeSettingsConfigStateSnapshot_DegradedCopyWithImpl;
@useResult
$Res call({
 BridgeDegradedResource resource, BridgeSettingsConfigStateData value
});




}
/// @nodoc
class _$BridgeSettingsConfigStateSnapshot_DegradedCopyWithImpl<$Res>
    implements $BridgeSettingsConfigStateSnapshot_DegradedCopyWith<$Res> {
  _$BridgeSettingsConfigStateSnapshot_DegradedCopyWithImpl(this._self, this._then);

  final BridgeSettingsConfigStateSnapshot_Degraded _self;
  final $Res Function(BridgeSettingsConfigStateSnapshot_Degraded) _then;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeSettingsConfigStateSnapshot_Degraded(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeDegradedResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeSettingsConfigStateData,
  ));
}


}

/// @nodoc


class BridgeSettingsConfigStateSnapshot_Failed extends BridgeSettingsConfigStateSnapshot {
  const BridgeSettingsConfigStateSnapshot_Failed(this.field0): super._();


 final  BridgeFailedResource field0;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSettingsConfigStateSnapshot_FailedCopyWith<BridgeSettingsConfigStateSnapshot_Failed> get copyWith => _$BridgeSettingsConfigStateSnapshot_FailedCopyWithImpl<BridgeSettingsConfigStateSnapshot_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSettingsConfigStateSnapshot_Failed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeSettingsConfigStateSnapshot.failed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeSettingsConfigStateSnapshot_FailedCopyWith<$Res> implements $BridgeSettingsConfigStateSnapshotCopyWith<$Res> {
  factory $BridgeSettingsConfigStateSnapshot_FailedCopyWith(BridgeSettingsConfigStateSnapshot_Failed value, $Res Function(BridgeSettingsConfigStateSnapshot_Failed) _then) = _$BridgeSettingsConfigStateSnapshot_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeFailedResource field0
});




}
/// @nodoc
class _$BridgeSettingsConfigStateSnapshot_FailedCopyWithImpl<$Res>
    implements $BridgeSettingsConfigStateSnapshot_FailedCopyWith<$Res> {
  _$BridgeSettingsConfigStateSnapshot_FailedCopyWithImpl(this._self, this._then);

  final BridgeSettingsConfigStateSnapshot_Failed _self;
  final $Res Function(BridgeSettingsConfigStateSnapshot_Failed) _then;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeSettingsConfigStateSnapshot_Failed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeFailedResource,
  ));
}


}

/// @nodoc


class BridgeSettingsConfigStateSnapshot_Stopped extends BridgeSettingsConfigStateSnapshot {
  const BridgeSettingsConfigStateSnapshot_Stopped(this.field0): super._();


 final  BridgeStoppedResource field0;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSettingsConfigStateSnapshot_StoppedCopyWith<BridgeSettingsConfigStateSnapshot_Stopped> get copyWith => _$BridgeSettingsConfigStateSnapshot_StoppedCopyWithImpl<BridgeSettingsConfigStateSnapshot_Stopped>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSettingsConfigStateSnapshot_Stopped&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeSettingsConfigStateSnapshot.stopped(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeSettingsConfigStateSnapshot_StoppedCopyWith<$Res> implements $BridgeSettingsConfigStateSnapshotCopyWith<$Res> {
  factory $BridgeSettingsConfigStateSnapshot_StoppedCopyWith(BridgeSettingsConfigStateSnapshot_Stopped value, $Res Function(BridgeSettingsConfigStateSnapshot_Stopped) _then) = _$BridgeSettingsConfigStateSnapshot_StoppedCopyWithImpl;
@useResult
$Res call({
 BridgeStoppedResource field0
});




}
/// @nodoc
class _$BridgeSettingsConfigStateSnapshot_StoppedCopyWithImpl<$Res>
    implements $BridgeSettingsConfigStateSnapshot_StoppedCopyWith<$Res> {
  _$BridgeSettingsConfigStateSnapshot_StoppedCopyWithImpl(this._self, this._then);

  final BridgeSettingsConfigStateSnapshot_Stopped _self;
  final $Res Function(BridgeSettingsConfigStateSnapshot_Stopped) _then;

/// Create a copy of BridgeSettingsConfigStateSnapshot
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeSettingsConfigStateSnapshot_Stopped(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeStoppedResource,
  ));
}


}

/// @nodoc
mixin _$BridgeSkillsResourceState {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillsResourceState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeSkillsResourceState()';
}


}

/// @nodoc
class $BridgeSkillsResourceStateCopyWith<$Res>  {
$BridgeSkillsResourceStateCopyWith(BridgeSkillsResourceState _, $Res Function(BridgeSkillsResourceState) __);
}


/// Adds pattern-matching-related methods to [BridgeSkillsResourceState].
extension BridgeSkillsResourceStatePatterns on BridgeSkillsResourceState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeSkillsResourceState_Uninitialized value)?  uninitialized,TResult Function( BridgeSkillsResourceState_Loading value)?  loading,TResult Function( BridgeSkillsResourceState_Ready value)?  ready,TResult Function( BridgeSkillsResourceState_Refreshing value)?  refreshing,TResult Function( BridgeSkillsResourceState_Stale value)?  stale,TResult Function( BridgeSkillsResourceState_Degraded value)?  degraded,TResult Function( BridgeSkillsResourceState_Failed value)?  failed,TResult Function( BridgeSkillsResourceState_Stopped value)?  stopped,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeSkillsResourceState_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeSkillsResourceState_Loading() when loading != null:
return loading(_that);case BridgeSkillsResourceState_Ready() when ready != null:
return ready(_that);case BridgeSkillsResourceState_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeSkillsResourceState_Stale() when stale != null:
return stale(_that);case BridgeSkillsResourceState_Degraded() when degraded != null:
return degraded(_that);case BridgeSkillsResourceState_Failed() when failed != null:
return failed(_that);case BridgeSkillsResourceState_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeSkillsResourceState_Uninitialized value)  uninitialized,required TResult Function( BridgeSkillsResourceState_Loading value)  loading,required TResult Function( BridgeSkillsResourceState_Ready value)  ready,required TResult Function( BridgeSkillsResourceState_Refreshing value)  refreshing,required TResult Function( BridgeSkillsResourceState_Stale value)  stale,required TResult Function( BridgeSkillsResourceState_Degraded value)  degraded,required TResult Function( BridgeSkillsResourceState_Failed value)  failed,required TResult Function( BridgeSkillsResourceState_Stopped value)  stopped,}){
final _that = this;
switch (_that) {
case BridgeSkillsResourceState_Uninitialized():
return uninitialized(_that);case BridgeSkillsResourceState_Loading():
return loading(_that);case BridgeSkillsResourceState_Ready():
return ready(_that);case BridgeSkillsResourceState_Refreshing():
return refreshing(_that);case BridgeSkillsResourceState_Stale():
return stale(_that);case BridgeSkillsResourceState_Degraded():
return degraded(_that);case BridgeSkillsResourceState_Failed():
return failed(_that);case BridgeSkillsResourceState_Stopped():
return stopped(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeSkillsResourceState_Uninitialized value)?  uninitialized,TResult? Function( BridgeSkillsResourceState_Loading value)?  loading,TResult? Function( BridgeSkillsResourceState_Ready value)?  ready,TResult? Function( BridgeSkillsResourceState_Refreshing value)?  refreshing,TResult? Function( BridgeSkillsResourceState_Stale value)?  stale,TResult? Function( BridgeSkillsResourceState_Degraded value)?  degraded,TResult? Function( BridgeSkillsResourceState_Failed value)?  failed,TResult? Function( BridgeSkillsResourceState_Stopped value)?  stopped,}){
final _that = this;
switch (_that) {
case BridgeSkillsResourceState_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeSkillsResourceState_Loading() when loading != null:
return loading(_that);case BridgeSkillsResourceState_Ready() when ready != null:
return ready(_that);case BridgeSkillsResourceState_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeSkillsResourceState_Stale() when stale != null:
return stale(_that);case BridgeSkillsResourceState_Degraded() when degraded != null:
return degraded(_that);case BridgeSkillsResourceState_Failed() when failed != null:
return failed(_that);case BridgeSkillsResourceState_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeUninitializedResource field0)?  uninitialized,TResult Function( BridgeLoadingResource field0)?  loading,TResult Function( BridgeReadyResource resource,  BridgeSkillsStateData value)?  ready,TResult Function( BridgeRefreshingResource resource,  BridgeSkillsStateData value)?  refreshing,TResult Function( BridgeStaleResource resource,  BridgeSkillsStateData value)?  stale,TResult Function( BridgeDegradedResource resource,  BridgeSkillsStateData value)?  degraded,TResult Function( BridgeFailedResource field0)?  failed,TResult Function( BridgeStoppedResource field0)?  stopped,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeSkillsResourceState_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeSkillsResourceState_Loading() when loading != null:
return loading(_that.field0);case BridgeSkillsResourceState_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeSkillsResourceState_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeSkillsResourceState_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeSkillsResourceState_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeSkillsResourceState_Failed() when failed != null:
return failed(_that.field0);case BridgeSkillsResourceState_Stopped() when stopped != null:
return stopped(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeUninitializedResource field0)  uninitialized,required TResult Function( BridgeLoadingResource field0)  loading,required TResult Function( BridgeReadyResource resource,  BridgeSkillsStateData value)  ready,required TResult Function( BridgeRefreshingResource resource,  BridgeSkillsStateData value)  refreshing,required TResult Function( BridgeStaleResource resource,  BridgeSkillsStateData value)  stale,required TResult Function( BridgeDegradedResource resource,  BridgeSkillsStateData value)  degraded,required TResult Function( BridgeFailedResource field0)  failed,required TResult Function( BridgeStoppedResource field0)  stopped,}) {final _that = this;
switch (_that) {
case BridgeSkillsResourceState_Uninitialized():
return uninitialized(_that.field0);case BridgeSkillsResourceState_Loading():
return loading(_that.field0);case BridgeSkillsResourceState_Ready():
return ready(_that.resource,_that.value);case BridgeSkillsResourceState_Refreshing():
return refreshing(_that.resource,_that.value);case BridgeSkillsResourceState_Stale():
return stale(_that.resource,_that.value);case BridgeSkillsResourceState_Degraded():
return degraded(_that.resource,_that.value);case BridgeSkillsResourceState_Failed():
return failed(_that.field0);case BridgeSkillsResourceState_Stopped():
return stopped(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeUninitializedResource field0)?  uninitialized,TResult? Function( BridgeLoadingResource field0)?  loading,TResult? Function( BridgeReadyResource resource,  BridgeSkillsStateData value)?  ready,TResult? Function( BridgeRefreshingResource resource,  BridgeSkillsStateData value)?  refreshing,TResult? Function( BridgeStaleResource resource,  BridgeSkillsStateData value)?  stale,TResult? Function( BridgeDegradedResource resource,  BridgeSkillsStateData value)?  degraded,TResult? Function( BridgeFailedResource field0)?  failed,TResult? Function( BridgeStoppedResource field0)?  stopped,}) {final _that = this;
switch (_that) {
case BridgeSkillsResourceState_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeSkillsResourceState_Loading() when loading != null:
return loading(_that.field0);case BridgeSkillsResourceState_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeSkillsResourceState_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeSkillsResourceState_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeSkillsResourceState_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeSkillsResourceState_Failed() when failed != null:
return failed(_that.field0);case BridgeSkillsResourceState_Stopped() when stopped != null:
return stopped(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeSkillsResourceState_Uninitialized extends BridgeSkillsResourceState {
  const BridgeSkillsResourceState_Uninitialized(this.field0): super._();


 final  BridgeUninitializedResource field0;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSkillsResourceState_UninitializedCopyWith<BridgeSkillsResourceState_Uninitialized> get copyWith => _$BridgeSkillsResourceState_UninitializedCopyWithImpl<BridgeSkillsResourceState_Uninitialized>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillsResourceState_Uninitialized&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeSkillsResourceState.uninitialized(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeSkillsResourceState_UninitializedCopyWith<$Res> implements $BridgeSkillsResourceStateCopyWith<$Res> {
  factory $BridgeSkillsResourceState_UninitializedCopyWith(BridgeSkillsResourceState_Uninitialized value, $Res Function(BridgeSkillsResourceState_Uninitialized) _then) = _$BridgeSkillsResourceState_UninitializedCopyWithImpl;
@useResult
$Res call({
 BridgeUninitializedResource field0
});




}
/// @nodoc
class _$BridgeSkillsResourceState_UninitializedCopyWithImpl<$Res>
    implements $BridgeSkillsResourceState_UninitializedCopyWith<$Res> {
  _$BridgeSkillsResourceState_UninitializedCopyWithImpl(this._self, this._then);

  final BridgeSkillsResourceState_Uninitialized _self;
  final $Res Function(BridgeSkillsResourceState_Uninitialized) _then;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeSkillsResourceState_Uninitialized(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeUninitializedResource,
  ));
}


}

/// @nodoc


class BridgeSkillsResourceState_Loading extends BridgeSkillsResourceState {
  const BridgeSkillsResourceState_Loading(this.field0): super._();


 final  BridgeLoadingResource field0;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSkillsResourceState_LoadingCopyWith<BridgeSkillsResourceState_Loading> get copyWith => _$BridgeSkillsResourceState_LoadingCopyWithImpl<BridgeSkillsResourceState_Loading>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillsResourceState_Loading&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeSkillsResourceState.loading(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeSkillsResourceState_LoadingCopyWith<$Res> implements $BridgeSkillsResourceStateCopyWith<$Res> {
  factory $BridgeSkillsResourceState_LoadingCopyWith(BridgeSkillsResourceState_Loading value, $Res Function(BridgeSkillsResourceState_Loading) _then) = _$BridgeSkillsResourceState_LoadingCopyWithImpl;
@useResult
$Res call({
 BridgeLoadingResource field0
});




}
/// @nodoc
class _$BridgeSkillsResourceState_LoadingCopyWithImpl<$Res>
    implements $BridgeSkillsResourceState_LoadingCopyWith<$Res> {
  _$BridgeSkillsResourceState_LoadingCopyWithImpl(this._self, this._then);

  final BridgeSkillsResourceState_Loading _self;
  final $Res Function(BridgeSkillsResourceState_Loading) _then;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeSkillsResourceState_Loading(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeLoadingResource,
  ));
}


}

/// @nodoc


class BridgeSkillsResourceState_Ready extends BridgeSkillsResourceState {
  const BridgeSkillsResourceState_Ready({required this.resource, required this.value}): super._();


 final  BridgeReadyResource resource;
 final  BridgeSkillsStateData value;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSkillsResourceState_ReadyCopyWith<BridgeSkillsResourceState_Ready> get copyWith => _$BridgeSkillsResourceState_ReadyCopyWithImpl<BridgeSkillsResourceState_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillsResourceState_Ready&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeSkillsResourceState.ready(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeSkillsResourceState_ReadyCopyWith<$Res> implements $BridgeSkillsResourceStateCopyWith<$Res> {
  factory $BridgeSkillsResourceState_ReadyCopyWith(BridgeSkillsResourceState_Ready value, $Res Function(BridgeSkillsResourceState_Ready) _then) = _$BridgeSkillsResourceState_ReadyCopyWithImpl;
@useResult
$Res call({
 BridgeReadyResource resource, BridgeSkillsStateData value
});




}
/// @nodoc
class _$BridgeSkillsResourceState_ReadyCopyWithImpl<$Res>
    implements $BridgeSkillsResourceState_ReadyCopyWith<$Res> {
  _$BridgeSkillsResourceState_ReadyCopyWithImpl(this._self, this._then);

  final BridgeSkillsResourceState_Ready _self;
  final $Res Function(BridgeSkillsResourceState_Ready) _then;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeSkillsResourceState_Ready(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeReadyResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeSkillsStateData,
  ));
}


}

/// @nodoc


class BridgeSkillsResourceState_Refreshing extends BridgeSkillsResourceState {
  const BridgeSkillsResourceState_Refreshing({required this.resource, required this.value}): super._();


 final  BridgeRefreshingResource resource;
 final  BridgeSkillsStateData value;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSkillsResourceState_RefreshingCopyWith<BridgeSkillsResourceState_Refreshing> get copyWith => _$BridgeSkillsResourceState_RefreshingCopyWithImpl<BridgeSkillsResourceState_Refreshing>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillsResourceState_Refreshing&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeSkillsResourceState.refreshing(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeSkillsResourceState_RefreshingCopyWith<$Res> implements $BridgeSkillsResourceStateCopyWith<$Res> {
  factory $BridgeSkillsResourceState_RefreshingCopyWith(BridgeSkillsResourceState_Refreshing value, $Res Function(BridgeSkillsResourceState_Refreshing) _then) = _$BridgeSkillsResourceState_RefreshingCopyWithImpl;
@useResult
$Res call({
 BridgeRefreshingResource resource, BridgeSkillsStateData value
});




}
/// @nodoc
class _$BridgeSkillsResourceState_RefreshingCopyWithImpl<$Res>
    implements $BridgeSkillsResourceState_RefreshingCopyWith<$Res> {
  _$BridgeSkillsResourceState_RefreshingCopyWithImpl(this._self, this._then);

  final BridgeSkillsResourceState_Refreshing _self;
  final $Res Function(BridgeSkillsResourceState_Refreshing) _then;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeSkillsResourceState_Refreshing(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeRefreshingResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeSkillsStateData,
  ));
}


}

/// @nodoc


class BridgeSkillsResourceState_Stale extends BridgeSkillsResourceState {
  const BridgeSkillsResourceState_Stale({required this.resource, required this.value}): super._();


 final  BridgeStaleResource resource;
 final  BridgeSkillsStateData value;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSkillsResourceState_StaleCopyWith<BridgeSkillsResourceState_Stale> get copyWith => _$BridgeSkillsResourceState_StaleCopyWithImpl<BridgeSkillsResourceState_Stale>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillsResourceState_Stale&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeSkillsResourceState.stale(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeSkillsResourceState_StaleCopyWith<$Res> implements $BridgeSkillsResourceStateCopyWith<$Res> {
  factory $BridgeSkillsResourceState_StaleCopyWith(BridgeSkillsResourceState_Stale value, $Res Function(BridgeSkillsResourceState_Stale) _then) = _$BridgeSkillsResourceState_StaleCopyWithImpl;
@useResult
$Res call({
 BridgeStaleResource resource, BridgeSkillsStateData value
});




}
/// @nodoc
class _$BridgeSkillsResourceState_StaleCopyWithImpl<$Res>
    implements $BridgeSkillsResourceState_StaleCopyWith<$Res> {
  _$BridgeSkillsResourceState_StaleCopyWithImpl(this._self, this._then);

  final BridgeSkillsResourceState_Stale _self;
  final $Res Function(BridgeSkillsResourceState_Stale) _then;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeSkillsResourceState_Stale(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeStaleResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeSkillsStateData,
  ));
}


}

/// @nodoc


class BridgeSkillsResourceState_Degraded extends BridgeSkillsResourceState {
  const BridgeSkillsResourceState_Degraded({required this.resource, required this.value}): super._();


 final  BridgeDegradedResource resource;
 final  BridgeSkillsStateData value;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSkillsResourceState_DegradedCopyWith<BridgeSkillsResourceState_Degraded> get copyWith => _$BridgeSkillsResourceState_DegradedCopyWithImpl<BridgeSkillsResourceState_Degraded>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillsResourceState_Degraded&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeSkillsResourceState.degraded(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeSkillsResourceState_DegradedCopyWith<$Res> implements $BridgeSkillsResourceStateCopyWith<$Res> {
  factory $BridgeSkillsResourceState_DegradedCopyWith(BridgeSkillsResourceState_Degraded value, $Res Function(BridgeSkillsResourceState_Degraded) _then) = _$BridgeSkillsResourceState_DegradedCopyWithImpl;
@useResult
$Res call({
 BridgeDegradedResource resource, BridgeSkillsStateData value
});




}
/// @nodoc
class _$BridgeSkillsResourceState_DegradedCopyWithImpl<$Res>
    implements $BridgeSkillsResourceState_DegradedCopyWith<$Res> {
  _$BridgeSkillsResourceState_DegradedCopyWithImpl(this._self, this._then);

  final BridgeSkillsResourceState_Degraded _self;
  final $Res Function(BridgeSkillsResourceState_Degraded) _then;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeSkillsResourceState_Degraded(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeDegradedResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeSkillsStateData,
  ));
}


}

/// @nodoc


class BridgeSkillsResourceState_Failed extends BridgeSkillsResourceState {
  const BridgeSkillsResourceState_Failed(this.field0): super._();


 final  BridgeFailedResource field0;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSkillsResourceState_FailedCopyWith<BridgeSkillsResourceState_Failed> get copyWith => _$BridgeSkillsResourceState_FailedCopyWithImpl<BridgeSkillsResourceState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillsResourceState_Failed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeSkillsResourceState.failed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeSkillsResourceState_FailedCopyWith<$Res> implements $BridgeSkillsResourceStateCopyWith<$Res> {
  factory $BridgeSkillsResourceState_FailedCopyWith(BridgeSkillsResourceState_Failed value, $Res Function(BridgeSkillsResourceState_Failed) _then) = _$BridgeSkillsResourceState_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeFailedResource field0
});




}
/// @nodoc
class _$BridgeSkillsResourceState_FailedCopyWithImpl<$Res>
    implements $BridgeSkillsResourceState_FailedCopyWith<$Res> {
  _$BridgeSkillsResourceState_FailedCopyWithImpl(this._self, this._then);

  final BridgeSkillsResourceState_Failed _self;
  final $Res Function(BridgeSkillsResourceState_Failed) _then;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeSkillsResourceState_Failed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeFailedResource,
  ));
}


}

/// @nodoc


class BridgeSkillsResourceState_Stopped extends BridgeSkillsResourceState {
  const BridgeSkillsResourceState_Stopped(this.field0): super._();


 final  BridgeStoppedResource field0;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSkillsResourceState_StoppedCopyWith<BridgeSkillsResourceState_Stopped> get copyWith => _$BridgeSkillsResourceState_StoppedCopyWithImpl<BridgeSkillsResourceState_Stopped>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSkillsResourceState_Stopped&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeSkillsResourceState.stopped(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeSkillsResourceState_StoppedCopyWith<$Res> implements $BridgeSkillsResourceStateCopyWith<$Res> {
  factory $BridgeSkillsResourceState_StoppedCopyWith(BridgeSkillsResourceState_Stopped value, $Res Function(BridgeSkillsResourceState_Stopped) _then) = _$BridgeSkillsResourceState_StoppedCopyWithImpl;
@useResult
$Res call({
 BridgeStoppedResource field0
});




}
/// @nodoc
class _$BridgeSkillsResourceState_StoppedCopyWithImpl<$Res>
    implements $BridgeSkillsResourceState_StoppedCopyWith<$Res> {
  _$BridgeSkillsResourceState_StoppedCopyWithImpl(this._self, this._then);

  final BridgeSkillsResourceState_Stopped _self;
  final $Res Function(BridgeSkillsResourceState_Stopped) _then;

/// Create a copy of BridgeSkillsResourceState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeSkillsResourceState_Stopped(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeStoppedResource,
  ));
}


}

/// @nodoc
mixin _$BridgeThreadDirectoryPage {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadDirectoryPage);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeThreadDirectoryPage()';
}


}

/// @nodoc
class $BridgeThreadDirectoryPageCopyWith<$Res>  {
$BridgeThreadDirectoryPageCopyWith(BridgeThreadDirectoryPage _, $Res Function(BridgeThreadDirectoryPage) __);
}


/// Adds pattern-matching-related methods to [BridgeThreadDirectoryPage].
extension BridgeThreadDirectoryPagePatterns on BridgeThreadDirectoryPage {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeThreadDirectoryPage_Uninitialized value)?  uninitialized,TResult Function( BridgeThreadDirectoryPage_Loading value)?  loading,TResult Function( BridgeThreadDirectoryPage_Ready value)?  ready,TResult Function( BridgeThreadDirectoryPage_Refreshing value)?  refreshing,TResult Function( BridgeThreadDirectoryPage_Stale value)?  stale,TResult Function( BridgeThreadDirectoryPage_Degraded value)?  degraded,TResult Function( BridgeThreadDirectoryPage_Failed value)?  failed,TResult Function( BridgeThreadDirectoryPage_Stopped value)?  stopped,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeThreadDirectoryPage_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeThreadDirectoryPage_Loading() when loading != null:
return loading(_that);case BridgeThreadDirectoryPage_Ready() when ready != null:
return ready(_that);case BridgeThreadDirectoryPage_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeThreadDirectoryPage_Stale() when stale != null:
return stale(_that);case BridgeThreadDirectoryPage_Degraded() when degraded != null:
return degraded(_that);case BridgeThreadDirectoryPage_Failed() when failed != null:
return failed(_that);case BridgeThreadDirectoryPage_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeThreadDirectoryPage_Uninitialized value)  uninitialized,required TResult Function( BridgeThreadDirectoryPage_Loading value)  loading,required TResult Function( BridgeThreadDirectoryPage_Ready value)  ready,required TResult Function( BridgeThreadDirectoryPage_Refreshing value)  refreshing,required TResult Function( BridgeThreadDirectoryPage_Stale value)  stale,required TResult Function( BridgeThreadDirectoryPage_Degraded value)  degraded,required TResult Function( BridgeThreadDirectoryPage_Failed value)  failed,required TResult Function( BridgeThreadDirectoryPage_Stopped value)  stopped,}){
final _that = this;
switch (_that) {
case BridgeThreadDirectoryPage_Uninitialized():
return uninitialized(_that);case BridgeThreadDirectoryPage_Loading():
return loading(_that);case BridgeThreadDirectoryPage_Ready():
return ready(_that);case BridgeThreadDirectoryPage_Refreshing():
return refreshing(_that);case BridgeThreadDirectoryPage_Stale():
return stale(_that);case BridgeThreadDirectoryPage_Degraded():
return degraded(_that);case BridgeThreadDirectoryPage_Failed():
return failed(_that);case BridgeThreadDirectoryPage_Stopped():
return stopped(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeThreadDirectoryPage_Uninitialized value)?  uninitialized,TResult? Function( BridgeThreadDirectoryPage_Loading value)?  loading,TResult? Function( BridgeThreadDirectoryPage_Ready value)?  ready,TResult? Function( BridgeThreadDirectoryPage_Refreshing value)?  refreshing,TResult? Function( BridgeThreadDirectoryPage_Stale value)?  stale,TResult? Function( BridgeThreadDirectoryPage_Degraded value)?  degraded,TResult? Function( BridgeThreadDirectoryPage_Failed value)?  failed,TResult? Function( BridgeThreadDirectoryPage_Stopped value)?  stopped,}){
final _that = this;
switch (_that) {
case BridgeThreadDirectoryPage_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeThreadDirectoryPage_Loading() when loading != null:
return loading(_that);case BridgeThreadDirectoryPage_Ready() when ready != null:
return ready(_that);case BridgeThreadDirectoryPage_Refreshing() when refreshing != null:
return refreshing(_that);case BridgeThreadDirectoryPage_Stale() when stale != null:
return stale(_that);case BridgeThreadDirectoryPage_Degraded() when degraded != null:
return degraded(_that);case BridgeThreadDirectoryPage_Failed() when failed != null:
return failed(_that);case BridgeThreadDirectoryPage_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeUninitializedResource field0)?  uninitialized,TResult Function( BridgeLoadingResource field0)?  loading,TResult Function( BridgeReadyResource resource,  BridgeThreadDirectoryPageData value)?  ready,TResult Function( BridgeRefreshingResource resource,  BridgeThreadDirectoryPageData value)?  refreshing,TResult Function( BridgeStaleResource resource,  BridgeThreadDirectoryPageData value)?  stale,TResult Function( BridgeDegradedResource resource,  BridgeThreadDirectoryPageData value)?  degraded,TResult Function( BridgeFailedResource field0)?  failed,TResult Function( BridgeStoppedResource field0)?  stopped,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeThreadDirectoryPage_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeThreadDirectoryPage_Loading() when loading != null:
return loading(_that.field0);case BridgeThreadDirectoryPage_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeThreadDirectoryPage_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeThreadDirectoryPage_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeThreadDirectoryPage_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeThreadDirectoryPage_Failed() when failed != null:
return failed(_that.field0);case BridgeThreadDirectoryPage_Stopped() when stopped != null:
return stopped(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeUninitializedResource field0)  uninitialized,required TResult Function( BridgeLoadingResource field0)  loading,required TResult Function( BridgeReadyResource resource,  BridgeThreadDirectoryPageData value)  ready,required TResult Function( BridgeRefreshingResource resource,  BridgeThreadDirectoryPageData value)  refreshing,required TResult Function( BridgeStaleResource resource,  BridgeThreadDirectoryPageData value)  stale,required TResult Function( BridgeDegradedResource resource,  BridgeThreadDirectoryPageData value)  degraded,required TResult Function( BridgeFailedResource field0)  failed,required TResult Function( BridgeStoppedResource field0)  stopped,}) {final _that = this;
switch (_that) {
case BridgeThreadDirectoryPage_Uninitialized():
return uninitialized(_that.field0);case BridgeThreadDirectoryPage_Loading():
return loading(_that.field0);case BridgeThreadDirectoryPage_Ready():
return ready(_that.resource,_that.value);case BridgeThreadDirectoryPage_Refreshing():
return refreshing(_that.resource,_that.value);case BridgeThreadDirectoryPage_Stale():
return stale(_that.resource,_that.value);case BridgeThreadDirectoryPage_Degraded():
return degraded(_that.resource,_that.value);case BridgeThreadDirectoryPage_Failed():
return failed(_that.field0);case BridgeThreadDirectoryPage_Stopped():
return stopped(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeUninitializedResource field0)?  uninitialized,TResult? Function( BridgeLoadingResource field0)?  loading,TResult? Function( BridgeReadyResource resource,  BridgeThreadDirectoryPageData value)?  ready,TResult? Function( BridgeRefreshingResource resource,  BridgeThreadDirectoryPageData value)?  refreshing,TResult? Function( BridgeStaleResource resource,  BridgeThreadDirectoryPageData value)?  stale,TResult? Function( BridgeDegradedResource resource,  BridgeThreadDirectoryPageData value)?  degraded,TResult? Function( BridgeFailedResource field0)?  failed,TResult? Function( BridgeStoppedResource field0)?  stopped,}) {final _that = this;
switch (_that) {
case BridgeThreadDirectoryPage_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeThreadDirectoryPage_Loading() when loading != null:
return loading(_that.field0);case BridgeThreadDirectoryPage_Ready() when ready != null:
return ready(_that.resource,_that.value);case BridgeThreadDirectoryPage_Refreshing() when refreshing != null:
return refreshing(_that.resource,_that.value);case BridgeThreadDirectoryPage_Stale() when stale != null:
return stale(_that.resource,_that.value);case BridgeThreadDirectoryPage_Degraded() when degraded != null:
return degraded(_that.resource,_that.value);case BridgeThreadDirectoryPage_Failed() when failed != null:
return failed(_that.field0);case BridgeThreadDirectoryPage_Stopped() when stopped != null:
return stopped(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeThreadDirectoryPage_Uninitialized extends BridgeThreadDirectoryPage {
  const BridgeThreadDirectoryPage_Uninitialized(this.field0): super._();


 final  BridgeUninitializedResource field0;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadDirectoryPage_UninitializedCopyWith<BridgeThreadDirectoryPage_Uninitialized> get copyWith => _$BridgeThreadDirectoryPage_UninitializedCopyWithImpl<BridgeThreadDirectoryPage_Uninitialized>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadDirectoryPage_Uninitialized&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeThreadDirectoryPage.uninitialized(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadDirectoryPage_UninitializedCopyWith<$Res> implements $BridgeThreadDirectoryPageCopyWith<$Res> {
  factory $BridgeThreadDirectoryPage_UninitializedCopyWith(BridgeThreadDirectoryPage_Uninitialized value, $Res Function(BridgeThreadDirectoryPage_Uninitialized) _then) = _$BridgeThreadDirectoryPage_UninitializedCopyWithImpl;
@useResult
$Res call({
 BridgeUninitializedResource field0
});




}
/// @nodoc
class _$BridgeThreadDirectoryPage_UninitializedCopyWithImpl<$Res>
    implements $BridgeThreadDirectoryPage_UninitializedCopyWith<$Res> {
  _$BridgeThreadDirectoryPage_UninitializedCopyWithImpl(this._self, this._then);

  final BridgeThreadDirectoryPage_Uninitialized _self;
  final $Res Function(BridgeThreadDirectoryPage_Uninitialized) _then;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeThreadDirectoryPage_Uninitialized(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeUninitializedResource,
  ));
}


}

/// @nodoc


class BridgeThreadDirectoryPage_Loading extends BridgeThreadDirectoryPage {
  const BridgeThreadDirectoryPage_Loading(this.field0): super._();


 final  BridgeLoadingResource field0;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadDirectoryPage_LoadingCopyWith<BridgeThreadDirectoryPage_Loading> get copyWith => _$BridgeThreadDirectoryPage_LoadingCopyWithImpl<BridgeThreadDirectoryPage_Loading>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadDirectoryPage_Loading&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeThreadDirectoryPage.loading(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadDirectoryPage_LoadingCopyWith<$Res> implements $BridgeThreadDirectoryPageCopyWith<$Res> {
  factory $BridgeThreadDirectoryPage_LoadingCopyWith(BridgeThreadDirectoryPage_Loading value, $Res Function(BridgeThreadDirectoryPage_Loading) _then) = _$BridgeThreadDirectoryPage_LoadingCopyWithImpl;
@useResult
$Res call({
 BridgeLoadingResource field0
});




}
/// @nodoc
class _$BridgeThreadDirectoryPage_LoadingCopyWithImpl<$Res>
    implements $BridgeThreadDirectoryPage_LoadingCopyWith<$Res> {
  _$BridgeThreadDirectoryPage_LoadingCopyWithImpl(this._self, this._then);

  final BridgeThreadDirectoryPage_Loading _self;
  final $Res Function(BridgeThreadDirectoryPage_Loading) _then;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeThreadDirectoryPage_Loading(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeLoadingResource,
  ));
}


}

/// @nodoc


class BridgeThreadDirectoryPage_Ready extends BridgeThreadDirectoryPage {
  const BridgeThreadDirectoryPage_Ready({required this.resource, required this.value}): super._();


 final  BridgeReadyResource resource;
 final  BridgeThreadDirectoryPageData value;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadDirectoryPage_ReadyCopyWith<BridgeThreadDirectoryPage_Ready> get copyWith => _$BridgeThreadDirectoryPage_ReadyCopyWithImpl<BridgeThreadDirectoryPage_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadDirectoryPage_Ready&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeThreadDirectoryPage.ready(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadDirectoryPage_ReadyCopyWith<$Res> implements $BridgeThreadDirectoryPageCopyWith<$Res> {
  factory $BridgeThreadDirectoryPage_ReadyCopyWith(BridgeThreadDirectoryPage_Ready value, $Res Function(BridgeThreadDirectoryPage_Ready) _then) = _$BridgeThreadDirectoryPage_ReadyCopyWithImpl;
@useResult
$Res call({
 BridgeReadyResource resource, BridgeThreadDirectoryPageData value
});




}
/// @nodoc
class _$BridgeThreadDirectoryPage_ReadyCopyWithImpl<$Res>
    implements $BridgeThreadDirectoryPage_ReadyCopyWith<$Res> {
  _$BridgeThreadDirectoryPage_ReadyCopyWithImpl(this._self, this._then);

  final BridgeThreadDirectoryPage_Ready _self;
  final $Res Function(BridgeThreadDirectoryPage_Ready) _then;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeThreadDirectoryPage_Ready(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeReadyResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeThreadDirectoryPageData,
  ));
}


}

/// @nodoc


class BridgeThreadDirectoryPage_Refreshing extends BridgeThreadDirectoryPage {
  const BridgeThreadDirectoryPage_Refreshing({required this.resource, required this.value}): super._();


 final  BridgeRefreshingResource resource;
 final  BridgeThreadDirectoryPageData value;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadDirectoryPage_RefreshingCopyWith<BridgeThreadDirectoryPage_Refreshing> get copyWith => _$BridgeThreadDirectoryPage_RefreshingCopyWithImpl<BridgeThreadDirectoryPage_Refreshing>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadDirectoryPage_Refreshing&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeThreadDirectoryPage.refreshing(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadDirectoryPage_RefreshingCopyWith<$Res> implements $BridgeThreadDirectoryPageCopyWith<$Res> {
  factory $BridgeThreadDirectoryPage_RefreshingCopyWith(BridgeThreadDirectoryPage_Refreshing value, $Res Function(BridgeThreadDirectoryPage_Refreshing) _then) = _$BridgeThreadDirectoryPage_RefreshingCopyWithImpl;
@useResult
$Res call({
 BridgeRefreshingResource resource, BridgeThreadDirectoryPageData value
});




}
/// @nodoc
class _$BridgeThreadDirectoryPage_RefreshingCopyWithImpl<$Res>
    implements $BridgeThreadDirectoryPage_RefreshingCopyWith<$Res> {
  _$BridgeThreadDirectoryPage_RefreshingCopyWithImpl(this._self, this._then);

  final BridgeThreadDirectoryPage_Refreshing _self;
  final $Res Function(BridgeThreadDirectoryPage_Refreshing) _then;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeThreadDirectoryPage_Refreshing(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeRefreshingResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeThreadDirectoryPageData,
  ));
}


}

/// @nodoc


class BridgeThreadDirectoryPage_Stale extends BridgeThreadDirectoryPage {
  const BridgeThreadDirectoryPage_Stale({required this.resource, required this.value}): super._();


 final  BridgeStaleResource resource;
 final  BridgeThreadDirectoryPageData value;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadDirectoryPage_StaleCopyWith<BridgeThreadDirectoryPage_Stale> get copyWith => _$BridgeThreadDirectoryPage_StaleCopyWithImpl<BridgeThreadDirectoryPage_Stale>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadDirectoryPage_Stale&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeThreadDirectoryPage.stale(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadDirectoryPage_StaleCopyWith<$Res> implements $BridgeThreadDirectoryPageCopyWith<$Res> {
  factory $BridgeThreadDirectoryPage_StaleCopyWith(BridgeThreadDirectoryPage_Stale value, $Res Function(BridgeThreadDirectoryPage_Stale) _then) = _$BridgeThreadDirectoryPage_StaleCopyWithImpl;
@useResult
$Res call({
 BridgeStaleResource resource, BridgeThreadDirectoryPageData value
});




}
/// @nodoc
class _$BridgeThreadDirectoryPage_StaleCopyWithImpl<$Res>
    implements $BridgeThreadDirectoryPage_StaleCopyWith<$Res> {
  _$BridgeThreadDirectoryPage_StaleCopyWithImpl(this._self, this._then);

  final BridgeThreadDirectoryPage_Stale _self;
  final $Res Function(BridgeThreadDirectoryPage_Stale) _then;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeThreadDirectoryPage_Stale(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeStaleResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeThreadDirectoryPageData,
  ));
}


}

/// @nodoc


class BridgeThreadDirectoryPage_Degraded extends BridgeThreadDirectoryPage {
  const BridgeThreadDirectoryPage_Degraded({required this.resource, required this.value}): super._();


 final  BridgeDegradedResource resource;
 final  BridgeThreadDirectoryPageData value;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadDirectoryPage_DegradedCopyWith<BridgeThreadDirectoryPage_Degraded> get copyWith => _$BridgeThreadDirectoryPage_DegradedCopyWithImpl<BridgeThreadDirectoryPage_Degraded>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadDirectoryPage_Degraded&&(identical(other.resource, resource) || other.resource == resource)&&(identical(other.value, value) || other.value == value));
}


@override
int get hashCode {
    return Object.hash(runtimeType,resource,value);
}

@override
String toString() {
    return 'BridgeThreadDirectoryPage.degraded(resource: $resource, value: $value)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadDirectoryPage_DegradedCopyWith<$Res> implements $BridgeThreadDirectoryPageCopyWith<$Res> {
  factory $BridgeThreadDirectoryPage_DegradedCopyWith(BridgeThreadDirectoryPage_Degraded value, $Res Function(BridgeThreadDirectoryPage_Degraded) _then) = _$BridgeThreadDirectoryPage_DegradedCopyWithImpl;
@useResult
$Res call({
 BridgeDegradedResource resource, BridgeThreadDirectoryPageData value
});




}
/// @nodoc
class _$BridgeThreadDirectoryPage_DegradedCopyWithImpl<$Res>
    implements $BridgeThreadDirectoryPage_DegradedCopyWith<$Res> {
  _$BridgeThreadDirectoryPage_DegradedCopyWithImpl(this._self, this._then);

  final BridgeThreadDirectoryPage_Degraded _self;
  final $Res Function(BridgeThreadDirectoryPage_Degraded) _then;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? resource = null,Object? value = null,}) {
  return _then(BridgeThreadDirectoryPage_Degraded(
resource: null == resource ? _self.resource : resource // ignore: cast_nullable_to_non_nullable
as BridgeDegradedResource,value: null == value ? _self.value : value // ignore: cast_nullable_to_non_nullable
as BridgeThreadDirectoryPageData,
  ));
}


}

/// @nodoc


class BridgeThreadDirectoryPage_Failed extends BridgeThreadDirectoryPage {
  const BridgeThreadDirectoryPage_Failed(this.field0): super._();


 final  BridgeFailedResource field0;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadDirectoryPage_FailedCopyWith<BridgeThreadDirectoryPage_Failed> get copyWith => _$BridgeThreadDirectoryPage_FailedCopyWithImpl<BridgeThreadDirectoryPage_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadDirectoryPage_Failed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeThreadDirectoryPage.failed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadDirectoryPage_FailedCopyWith<$Res> implements $BridgeThreadDirectoryPageCopyWith<$Res> {
  factory $BridgeThreadDirectoryPage_FailedCopyWith(BridgeThreadDirectoryPage_Failed value, $Res Function(BridgeThreadDirectoryPage_Failed) _then) = _$BridgeThreadDirectoryPage_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeFailedResource field0
});




}
/// @nodoc
class _$BridgeThreadDirectoryPage_FailedCopyWithImpl<$Res>
    implements $BridgeThreadDirectoryPage_FailedCopyWith<$Res> {
  _$BridgeThreadDirectoryPage_FailedCopyWithImpl(this._self, this._then);

  final BridgeThreadDirectoryPage_Failed _self;
  final $Res Function(BridgeThreadDirectoryPage_Failed) _then;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeThreadDirectoryPage_Failed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeFailedResource,
  ));
}


}

/// @nodoc


class BridgeThreadDirectoryPage_Stopped extends BridgeThreadDirectoryPage {
  const BridgeThreadDirectoryPage_Stopped(this.field0): super._();


 final  BridgeStoppedResource field0;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadDirectoryPage_StoppedCopyWith<BridgeThreadDirectoryPage_Stopped> get copyWith => _$BridgeThreadDirectoryPage_StoppedCopyWithImpl<BridgeThreadDirectoryPage_Stopped>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadDirectoryPage_Stopped&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode {
    return Object.hash(runtimeType,field0);
}

@override
String toString() {
    return 'BridgeThreadDirectoryPage.stopped(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadDirectoryPage_StoppedCopyWith<$Res> implements $BridgeThreadDirectoryPageCopyWith<$Res> {
  factory $BridgeThreadDirectoryPage_StoppedCopyWith(BridgeThreadDirectoryPage_Stopped value, $Res Function(BridgeThreadDirectoryPage_Stopped) _then) = _$BridgeThreadDirectoryPage_StoppedCopyWithImpl;
@useResult
$Res call({
 BridgeStoppedResource field0
});




}
/// @nodoc
class _$BridgeThreadDirectoryPage_StoppedCopyWithImpl<$Res>
    implements $BridgeThreadDirectoryPage_StoppedCopyWith<$Res> {
  _$BridgeThreadDirectoryPage_StoppedCopyWithImpl(this._self, this._then);

  final BridgeThreadDirectoryPage_Stopped _self;
  final $Res Function(BridgeThreadDirectoryPage_Stopped) _then;

/// Create a copy of BridgeThreadDirectoryPage
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeThreadDirectoryPage_Stopped(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeStoppedResource,
  ));
}


}

/// @nodoc
mixin _$SkillResourceBaseDto {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SkillResourceBaseDto);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'SkillResourceBaseDto()';
}


}

/// @nodoc
class $SkillResourceBaseDtoCopyWith<$Res>  {
$SkillResourceBaseDtoCopyWith(SkillResourceBaseDto _, $Res Function(SkillResourceBaseDto) __);
}


/// Adds pattern-matching-related methods to [SkillResourceBaseDto].
extension SkillResourceBaseDtoPatterns on SkillResourceBaseDto {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( SkillResourceBaseDto_Directory value)?  directory,TResult Function( SkillResourceBaseDto_Url value)?  url,TResult Function( SkillResourceBaseDto_Opaque value)?  opaque,required TResult orElse(),}){
final _that = this;
switch (_that) {
case SkillResourceBaseDto_Directory() when directory != null:
return directory(_that);case SkillResourceBaseDto_Url() when url != null:
return url(_that);case SkillResourceBaseDto_Opaque() when opaque != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( SkillResourceBaseDto_Directory value)  directory,required TResult Function( SkillResourceBaseDto_Url value)  url,required TResult Function( SkillResourceBaseDto_Opaque value)  opaque,}){
final _that = this;
switch (_that) {
case SkillResourceBaseDto_Directory():
return directory(_that);case SkillResourceBaseDto_Url():
return url(_that);case SkillResourceBaseDto_Opaque():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( SkillResourceBaseDto_Directory value)?  directory,TResult? Function( SkillResourceBaseDto_Url value)?  url,TResult? Function( SkillResourceBaseDto_Opaque value)?  opaque,}){
final _that = this;
switch (_that) {
case SkillResourceBaseDto_Directory() when directory != null:
return directory(_that);case SkillResourceBaseDto_Url() when url != null:
return url(_that);case SkillResourceBaseDto_Opaque() when opaque != null:
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
case SkillResourceBaseDto_Directory() when directory != null:
return directory(_that.path);case SkillResourceBaseDto_Url() when url != null:
return url(_that.url);case SkillResourceBaseDto_Opaque() when opaque != null:
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
case SkillResourceBaseDto_Directory():
return directory(_that.path);case SkillResourceBaseDto_Url():
return url(_that.url);case SkillResourceBaseDto_Opaque():
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
case SkillResourceBaseDto_Directory() when directory != null:
return directory(_that.path);case SkillResourceBaseDto_Url() when url != null:
return url(_that.url);case SkillResourceBaseDto_Opaque() when opaque != null:
return opaque(_that.description);case _:
  return null;

}
}

}

/// @nodoc


class SkillResourceBaseDto_Directory extends SkillResourceBaseDto {
  const SkillResourceBaseDto_Directory({required this.path}): super._();


 final  String path;

/// Create a copy of SkillResourceBaseDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SkillResourceBaseDto_DirectoryCopyWith<SkillResourceBaseDto_Directory> get copyWith => _$SkillResourceBaseDto_DirectoryCopyWithImpl<SkillResourceBaseDto_Directory>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SkillResourceBaseDto_Directory&&(identical(other.path, path) || other.path == path));
}


@override
int get hashCode {
    return Object.hash(runtimeType,path);
}

@override
String toString() {
    return 'SkillResourceBaseDto.directory(path: $path)';
}


}

/// @nodoc
abstract mixin class $SkillResourceBaseDto_DirectoryCopyWith<$Res> implements $SkillResourceBaseDtoCopyWith<$Res> {
  factory $SkillResourceBaseDto_DirectoryCopyWith(SkillResourceBaseDto_Directory value, $Res Function(SkillResourceBaseDto_Directory) _then) = _$SkillResourceBaseDto_DirectoryCopyWithImpl;
@useResult
$Res call({
 String path
});




}
/// @nodoc
class _$SkillResourceBaseDto_DirectoryCopyWithImpl<$Res>
    implements $SkillResourceBaseDto_DirectoryCopyWith<$Res> {
  _$SkillResourceBaseDto_DirectoryCopyWithImpl(this._self, this._then);

  final SkillResourceBaseDto_Directory _self;
  final $Res Function(SkillResourceBaseDto_Directory) _then;

/// Create a copy of SkillResourceBaseDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? path = null,}) {
  return _then(SkillResourceBaseDto_Directory(
path: null == path ? _self.path : path // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class SkillResourceBaseDto_Url extends SkillResourceBaseDto {
  const SkillResourceBaseDto_Url({required this.url}): super._();


 final  String url;

/// Create a copy of SkillResourceBaseDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SkillResourceBaseDto_UrlCopyWith<SkillResourceBaseDto_Url> get copyWith => _$SkillResourceBaseDto_UrlCopyWithImpl<SkillResourceBaseDto_Url>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SkillResourceBaseDto_Url&&(identical(other.url, url) || other.url == url));
}


@override
int get hashCode {
    return Object.hash(runtimeType,url);
}

@override
String toString() {
    return 'SkillResourceBaseDto.url(url: $url)';
}


}

/// @nodoc
abstract mixin class $SkillResourceBaseDto_UrlCopyWith<$Res> implements $SkillResourceBaseDtoCopyWith<$Res> {
  factory $SkillResourceBaseDto_UrlCopyWith(SkillResourceBaseDto_Url value, $Res Function(SkillResourceBaseDto_Url) _then) = _$SkillResourceBaseDto_UrlCopyWithImpl;
@useResult
$Res call({
 String url
});




}
/// @nodoc
class _$SkillResourceBaseDto_UrlCopyWithImpl<$Res>
    implements $SkillResourceBaseDto_UrlCopyWith<$Res> {
  _$SkillResourceBaseDto_UrlCopyWithImpl(this._self, this._then);

  final SkillResourceBaseDto_Url _self;
  final $Res Function(SkillResourceBaseDto_Url) _then;

/// Create a copy of SkillResourceBaseDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? url = null,}) {
  return _then(SkillResourceBaseDto_Url(
url: null == url ? _self.url : url // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class SkillResourceBaseDto_Opaque extends SkillResourceBaseDto {
  const SkillResourceBaseDto_Opaque({required this.description}): super._();


 final  String description;

/// Create a copy of SkillResourceBaseDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SkillResourceBaseDto_OpaqueCopyWith<SkillResourceBaseDto_Opaque> get copyWith => _$SkillResourceBaseDto_OpaqueCopyWithImpl<SkillResourceBaseDto_Opaque>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SkillResourceBaseDto_Opaque&&(identical(other.description, description) || other.description == description));
}


@override
int get hashCode {
    return Object.hash(runtimeType,description);
}

@override
String toString() {
    return 'SkillResourceBaseDto.opaque(description: $description)';
}


}

/// @nodoc
abstract mixin class $SkillResourceBaseDto_OpaqueCopyWith<$Res> implements $SkillResourceBaseDtoCopyWith<$Res> {
  factory $SkillResourceBaseDto_OpaqueCopyWith(SkillResourceBaseDto_Opaque value, $Res Function(SkillResourceBaseDto_Opaque) _then) = _$SkillResourceBaseDto_OpaqueCopyWithImpl;
@useResult
$Res call({
 String description
});




}
/// @nodoc
class _$SkillResourceBaseDto_OpaqueCopyWithImpl<$Res>
    implements $SkillResourceBaseDto_OpaqueCopyWith<$Res> {
  _$SkillResourceBaseDto_OpaqueCopyWithImpl(this._self, this._then);

  final SkillResourceBaseDto_Opaque _self;
  final $Res Function(SkillResourceBaseDto_Opaque) _then;

/// Create a copy of SkillResourceBaseDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? description = null,}) {
  return _then(SkillResourceBaseDto_Opaque(
description: null == description ? _self.description : description // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

// dart format on

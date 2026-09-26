// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'thread_activity.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeThreadActivityDetail {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadActivityDetail);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeThreadActivityDetail()';
}


}

/// @nodoc
class $BridgeThreadActivityDetailCopyWith<$Res>  {
$BridgeThreadActivityDetailCopyWith(BridgeThreadActivityDetail _, $Res Function(BridgeThreadActivityDetail) __);
}


/// Adds pattern-matching-related methods to [BridgeThreadActivityDetail].
extension BridgeThreadActivityDetailPatterns on BridgeThreadActivityDetail {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeThreadActivityDetail_Current value)?  current,TResult Function( BridgeThreadActivityDetail_Superseded value)?  superseded,TResult Function( BridgeThreadActivityDetail_Ended value)?  ended,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeThreadActivityDetail_Current() when current != null:
return current(_that);case BridgeThreadActivityDetail_Superseded() when superseded != null:
return superseded(_that);case BridgeThreadActivityDetail_Ended() when ended != null:
return ended(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeThreadActivityDetail_Current value)  current,required TResult Function( BridgeThreadActivityDetail_Superseded value)  superseded,required TResult Function( BridgeThreadActivityDetail_Ended value)  ended,}){
final _that = this;
switch (_that) {
case BridgeThreadActivityDetail_Current():
return current(_that);case BridgeThreadActivityDetail_Superseded():
return superseded(_that);case BridgeThreadActivityDetail_Ended():
return ended(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeThreadActivityDetail_Current value)?  current,TResult? Function( BridgeThreadActivityDetail_Superseded value)?  superseded,TResult? Function( BridgeThreadActivityDetail_Ended value)?  ended,}){
final _that = this;
switch (_that) {
case BridgeThreadActivityDetail_Current() when current != null:
return current(_that);case BridgeThreadActivityDetail_Superseded() when superseded != null:
return superseded(_that);case BridgeThreadActivityDetail_Ended() when ended != null:
return ended(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeThreadActivity activity,  List<BridgeThreadActivityContentPart> reasoning,  List<BridgeThreadActivityContentPart> response,  List<BridgeThreadActivityToolDetail> tools)?  current,TResult Function( BridgeThreadActivity activity,  String requestedActivityId)?  superseded,TResult Function( String threadId,  String activityId)?  ended,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeThreadActivityDetail_Current() when current != null:
return current(_that.activity,_that.reasoning,_that.response,_that.tools);case BridgeThreadActivityDetail_Superseded() when superseded != null:
return superseded(_that.activity,_that.requestedActivityId);case BridgeThreadActivityDetail_Ended() when ended != null:
return ended(_that.threadId,_that.activityId);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeThreadActivity activity,  List<BridgeThreadActivityContentPart> reasoning,  List<BridgeThreadActivityContentPart> response,  List<BridgeThreadActivityToolDetail> tools)  current,required TResult Function( BridgeThreadActivity activity,  String requestedActivityId)  superseded,required TResult Function( String threadId,  String activityId)  ended,}) {final _that = this;
switch (_that) {
case BridgeThreadActivityDetail_Current():
return current(_that.activity,_that.reasoning,_that.response,_that.tools);case BridgeThreadActivityDetail_Superseded():
return superseded(_that.activity,_that.requestedActivityId);case BridgeThreadActivityDetail_Ended():
return ended(_that.threadId,_that.activityId);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeThreadActivity activity,  List<BridgeThreadActivityContentPart> reasoning,  List<BridgeThreadActivityContentPart> response,  List<BridgeThreadActivityToolDetail> tools)?  current,TResult? Function( BridgeThreadActivity activity,  String requestedActivityId)?  superseded,TResult? Function( String threadId,  String activityId)?  ended,}) {final _that = this;
switch (_that) {
case BridgeThreadActivityDetail_Current() when current != null:
return current(_that.activity,_that.reasoning,_that.response,_that.tools);case BridgeThreadActivityDetail_Superseded() when superseded != null:
return superseded(_that.activity,_that.requestedActivityId);case BridgeThreadActivityDetail_Ended() when ended != null:
return ended(_that.threadId,_that.activityId);case _:
  return null;

}
}

}

/// @nodoc


class BridgeThreadActivityDetail_Current extends BridgeThreadActivityDetail {
  const BridgeThreadActivityDetail_Current({required this.activity, required  List<BridgeThreadActivityContentPart> reasoning, required  List<BridgeThreadActivityContentPart> response, required  List<BridgeThreadActivityToolDetail> tools}): _reasoning = reasoning,_response = response,_tools = tools,super._();


 final  BridgeThreadActivity activity;
 final  List<BridgeThreadActivityContentPart> _reasoning;
 List<BridgeThreadActivityContentPart> get reasoning {
  if (_reasoning is EqualUnmodifiableListView) return _reasoning;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_reasoning);
}

 final  List<BridgeThreadActivityContentPart> _response;
 List<BridgeThreadActivityContentPart> get response {
  if (_response is EqualUnmodifiableListView) return _response;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_response);
}

 final  List<BridgeThreadActivityToolDetail> _tools;
 List<BridgeThreadActivityToolDetail> get tools {
  if (_tools is EqualUnmodifiableListView) return _tools;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_tools);
}


/// Create a copy of BridgeThreadActivityDetail
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadActivityDetail_CurrentCopyWith<BridgeThreadActivityDetail_Current> get copyWith => _$BridgeThreadActivityDetail_CurrentCopyWithImpl<BridgeThreadActivityDetail_Current>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadActivityDetail_Current&&(identical(other.activity, activity) || other.activity == activity)&&const DeepCollectionEquality().equals(other.reasoning, _reasoning)&&const DeepCollectionEquality().equals(other.response, _response)&&const DeepCollectionEquality().equals(other.tools, _tools));
}


@override
int get hashCode {
    return Object.hash(runtimeType,activity,const DeepCollectionEquality().hash(_reasoning),const DeepCollectionEquality().hash(_response),const DeepCollectionEquality().hash(_tools));
}

@override
String toString() {
    return 'BridgeThreadActivityDetail.current(activity: $activity, reasoning: $reasoning, response: $response, tools: $tools)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadActivityDetail_CurrentCopyWith<$Res> implements $BridgeThreadActivityDetailCopyWith<$Res> {
  factory $BridgeThreadActivityDetail_CurrentCopyWith(BridgeThreadActivityDetail_Current value, $Res Function(BridgeThreadActivityDetail_Current) _then) = _$BridgeThreadActivityDetail_CurrentCopyWithImpl;
@useResult
$Res call({
 BridgeThreadActivity activity, List<BridgeThreadActivityContentPart> reasoning, List<BridgeThreadActivityContentPart> response, List<BridgeThreadActivityToolDetail> tools
});




}
/// @nodoc
class _$BridgeThreadActivityDetail_CurrentCopyWithImpl<$Res>
    implements $BridgeThreadActivityDetail_CurrentCopyWith<$Res> {
  _$BridgeThreadActivityDetail_CurrentCopyWithImpl(this._self, this._then);

  final BridgeThreadActivityDetail_Current _self;
  final $Res Function(BridgeThreadActivityDetail_Current) _then;

/// Create a copy of BridgeThreadActivityDetail
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? activity = null,Object? reasoning = null,Object? response = null,Object? tools = null,}) {
  return _then(BridgeThreadActivityDetail_Current(
activity: null == activity ? _self.activity : activity // ignore: cast_nullable_to_non_nullable
as BridgeThreadActivity,reasoning: null == reasoning ? _self._reasoning : reasoning // ignore: cast_nullable_to_non_nullable
as List<BridgeThreadActivityContentPart>,response: null == response ? _self._response : response // ignore: cast_nullable_to_non_nullable
as List<BridgeThreadActivityContentPart>,tools: null == tools ? _self._tools : tools // ignore: cast_nullable_to_non_nullable
as List<BridgeThreadActivityToolDetail>,
  ));
}


}

/// @nodoc


class BridgeThreadActivityDetail_Superseded extends BridgeThreadActivityDetail {
  const BridgeThreadActivityDetail_Superseded({required this.activity, required this.requestedActivityId}): super._();


 final  BridgeThreadActivity activity;
 final  String requestedActivityId;

/// Create a copy of BridgeThreadActivityDetail
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadActivityDetail_SupersededCopyWith<BridgeThreadActivityDetail_Superseded> get copyWith => _$BridgeThreadActivityDetail_SupersededCopyWithImpl<BridgeThreadActivityDetail_Superseded>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadActivityDetail_Superseded&&(identical(other.activity, activity) || other.activity == activity)&&(identical(other.requestedActivityId, requestedActivityId) || other.requestedActivityId == requestedActivityId));
}


@override
int get hashCode {
    return Object.hash(runtimeType,activity,requestedActivityId);
}

@override
String toString() {
    return 'BridgeThreadActivityDetail.superseded(activity: $activity, requestedActivityId: $requestedActivityId)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadActivityDetail_SupersededCopyWith<$Res> implements $BridgeThreadActivityDetailCopyWith<$Res> {
  factory $BridgeThreadActivityDetail_SupersededCopyWith(BridgeThreadActivityDetail_Superseded value, $Res Function(BridgeThreadActivityDetail_Superseded) _then) = _$BridgeThreadActivityDetail_SupersededCopyWithImpl;
@useResult
$Res call({
 BridgeThreadActivity activity, String requestedActivityId
});




}
/// @nodoc
class _$BridgeThreadActivityDetail_SupersededCopyWithImpl<$Res>
    implements $BridgeThreadActivityDetail_SupersededCopyWith<$Res> {
  _$BridgeThreadActivityDetail_SupersededCopyWithImpl(this._self, this._then);

  final BridgeThreadActivityDetail_Superseded _self;
  final $Res Function(BridgeThreadActivityDetail_Superseded) _then;

/// Create a copy of BridgeThreadActivityDetail
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? activity = null,Object? requestedActivityId = null,}) {
  return _then(BridgeThreadActivityDetail_Superseded(
activity: null == activity ? _self.activity : activity // ignore: cast_nullable_to_non_nullable
as BridgeThreadActivity,requestedActivityId: null == requestedActivityId ? _self.requestedActivityId : requestedActivityId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadActivityDetail_Ended extends BridgeThreadActivityDetail {
  const BridgeThreadActivityDetail_Ended({required this.threadId, required this.activityId}): super._();


 final  String threadId;
 final  String activityId;

/// Create a copy of BridgeThreadActivityDetail
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadActivityDetail_EndedCopyWith<BridgeThreadActivityDetail_Ended> get copyWith => _$BridgeThreadActivityDetail_EndedCopyWithImpl<BridgeThreadActivityDetail_Ended>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadActivityDetail_Ended&&(identical(other.threadId, threadId) || other.threadId == threadId)&&(identical(other.activityId, activityId) || other.activityId == activityId));
}


@override
int get hashCode {
    return Object.hash(runtimeType,threadId,activityId);
}

@override
String toString() {
    return 'BridgeThreadActivityDetail.ended(threadId: $threadId, activityId: $activityId)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadActivityDetail_EndedCopyWith<$Res> implements $BridgeThreadActivityDetailCopyWith<$Res> {
  factory $BridgeThreadActivityDetail_EndedCopyWith(BridgeThreadActivityDetail_Ended value, $Res Function(BridgeThreadActivityDetail_Ended) _then) = _$BridgeThreadActivityDetail_EndedCopyWithImpl;
@useResult
$Res call({
 String threadId, String activityId
});




}
/// @nodoc
class _$BridgeThreadActivityDetail_EndedCopyWithImpl<$Res>
    implements $BridgeThreadActivityDetail_EndedCopyWith<$Res> {
  _$BridgeThreadActivityDetail_EndedCopyWithImpl(this._self, this._then);

  final BridgeThreadActivityDetail_Ended _self;
  final $Res Function(BridgeThreadActivityDetail_Ended) _then;

/// Create a copy of BridgeThreadActivityDetail
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? threadId = null,Object? activityId = null,}) {
  return _then(BridgeThreadActivityDetail_Ended(
threadId: null == threadId ? _self.threadId : threadId // ignore: cast_nullable_to_non_nullable
as String,activityId: null == activityId ? _self.activityId : activityId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

// dart format on

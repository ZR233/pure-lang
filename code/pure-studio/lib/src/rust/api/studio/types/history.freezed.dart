// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'history.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeTimelineQuery {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTimelineQuery);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeTimelineQuery()';
}


}

/// @nodoc
class $BridgeTimelineQueryCopyWith<$Res>  {
$BridgeTimelineQueryCopyWith(BridgeTimelineQuery _, $Res Function(BridgeTimelineQuery) __);
}


/// Adds pattern-matching-related methods to [BridgeTimelineQuery].
extension BridgeTimelineQueryPatterns on BridgeTimelineQuery {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTimelineQuery_Latest value)?  latest,TResult Function( BridgeTimelineQuery_Before value)?  before,TResult Function( BridgeTimelineQuery_After value)?  after,TResult Function( BridgeTimelineQuery_Around value)?  around,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTimelineQuery_Latest() when latest != null:
return latest(_that);case BridgeTimelineQuery_Before() when before != null:
return before(_that);case BridgeTimelineQuery_After() when after != null:
return after(_that);case BridgeTimelineQuery_Around() when around != null:
return around(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTimelineQuery_Latest value)  latest,required TResult Function( BridgeTimelineQuery_Before value)  before,required TResult Function( BridgeTimelineQuery_After value)  after,required TResult Function( BridgeTimelineQuery_Around value)  around,}){
final _that = this;
switch (_that) {
case BridgeTimelineQuery_Latest():
return latest(_that);case BridgeTimelineQuery_Before():
return before(_that);case BridgeTimelineQuery_After():
return after(_that);case BridgeTimelineQuery_Around():
return around(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTimelineQuery_Latest value)?  latest,TResult? Function( BridgeTimelineQuery_Before value)?  before,TResult? Function( BridgeTimelineQuery_After value)?  after,TResult? Function( BridgeTimelineQuery_Around value)?  around,}){
final _that = this;
switch (_that) {
case BridgeTimelineQuery_Latest() when latest != null:
return latest(_that);case BridgeTimelineQuery_Before() when before != null:
return before(_that);case BridgeTimelineQuery_After() when after != null:
return after(_that);case BridgeTimelineQuery_Around() when around != null:
return around(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  latest,TResult Function( String itemId)?  before,TResult Function( String itemId)?  after,TResult Function( String itemId)?  around,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTimelineQuery_Latest() when latest != null:
return latest();case BridgeTimelineQuery_Before() when before != null:
return before(_that.itemId);case BridgeTimelineQuery_After() when after != null:
return after(_that.itemId);case BridgeTimelineQuery_Around() when around != null:
return around(_that.itemId);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  latest,required TResult Function( String itemId)  before,required TResult Function( String itemId)  after,required TResult Function( String itemId)  around,}) {final _that = this;
switch (_that) {
case BridgeTimelineQuery_Latest():
return latest();case BridgeTimelineQuery_Before():
return before(_that.itemId);case BridgeTimelineQuery_After():
return after(_that.itemId);case BridgeTimelineQuery_Around():
return around(_that.itemId);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  latest,TResult? Function( String itemId)?  before,TResult? Function( String itemId)?  after,TResult? Function( String itemId)?  around,}) {final _that = this;
switch (_that) {
case BridgeTimelineQuery_Latest() when latest != null:
return latest();case BridgeTimelineQuery_Before() when before != null:
return before(_that.itemId);case BridgeTimelineQuery_After() when after != null:
return after(_that.itemId);case BridgeTimelineQuery_Around() when around != null:
return around(_that.itemId);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTimelineQuery_Latest extends BridgeTimelineQuery {
  const BridgeTimelineQuery_Latest(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTimelineQuery_Latest);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeTimelineQuery.latest()';
}


}




/// @nodoc


class BridgeTimelineQuery_Before extends BridgeTimelineQuery {
  const BridgeTimelineQuery_Before({required this.itemId}): super._();


 final  String itemId;

/// Create a copy of BridgeTimelineQuery
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTimelineQuery_BeforeCopyWith<BridgeTimelineQuery_Before> get copyWith => _$BridgeTimelineQuery_BeforeCopyWithImpl<BridgeTimelineQuery_Before>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTimelineQuery_Before&&(identical(other.itemId, itemId) || other.itemId == itemId));
}


@override
int get hashCode {
    return Object.hash(runtimeType,itemId);
}

@override
String toString() {
    return 'BridgeTimelineQuery.before(itemId: $itemId)';
}


}

/// @nodoc
abstract mixin class $BridgeTimelineQuery_BeforeCopyWith<$Res> implements $BridgeTimelineQueryCopyWith<$Res> {
  factory $BridgeTimelineQuery_BeforeCopyWith(BridgeTimelineQuery_Before value, $Res Function(BridgeTimelineQuery_Before) _then) = _$BridgeTimelineQuery_BeforeCopyWithImpl;
@useResult
$Res call({
 String itemId
});




}
/// @nodoc
class _$BridgeTimelineQuery_BeforeCopyWithImpl<$Res>
    implements $BridgeTimelineQuery_BeforeCopyWith<$Res> {
  _$BridgeTimelineQuery_BeforeCopyWithImpl(this._self, this._then);

  final BridgeTimelineQuery_Before _self;
  final $Res Function(BridgeTimelineQuery_Before) _then;

/// Create a copy of BridgeTimelineQuery
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? itemId = null,}) {
  return _then(BridgeTimelineQuery_Before(
itemId: null == itemId ? _self.itemId : itemId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeTimelineQuery_After extends BridgeTimelineQuery {
  const BridgeTimelineQuery_After({required this.itemId}): super._();


 final  String itemId;

/// Create a copy of BridgeTimelineQuery
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTimelineQuery_AfterCopyWith<BridgeTimelineQuery_After> get copyWith => _$BridgeTimelineQuery_AfterCopyWithImpl<BridgeTimelineQuery_After>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTimelineQuery_After&&(identical(other.itemId, itemId) || other.itemId == itemId));
}


@override
int get hashCode {
    return Object.hash(runtimeType,itemId);
}

@override
String toString() {
    return 'BridgeTimelineQuery.after(itemId: $itemId)';
}


}

/// @nodoc
abstract mixin class $BridgeTimelineQuery_AfterCopyWith<$Res> implements $BridgeTimelineQueryCopyWith<$Res> {
  factory $BridgeTimelineQuery_AfterCopyWith(BridgeTimelineQuery_After value, $Res Function(BridgeTimelineQuery_After) _then) = _$BridgeTimelineQuery_AfterCopyWithImpl;
@useResult
$Res call({
 String itemId
});




}
/// @nodoc
class _$BridgeTimelineQuery_AfterCopyWithImpl<$Res>
    implements $BridgeTimelineQuery_AfterCopyWith<$Res> {
  _$BridgeTimelineQuery_AfterCopyWithImpl(this._self, this._then);

  final BridgeTimelineQuery_After _self;
  final $Res Function(BridgeTimelineQuery_After) _then;

/// Create a copy of BridgeTimelineQuery
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? itemId = null,}) {
  return _then(BridgeTimelineQuery_After(
itemId: null == itemId ? _self.itemId : itemId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeTimelineQuery_Around extends BridgeTimelineQuery {
  const BridgeTimelineQuery_Around({required this.itemId}): super._();


 final  String itemId;

/// Create a copy of BridgeTimelineQuery
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTimelineQuery_AroundCopyWith<BridgeTimelineQuery_Around> get copyWith => _$BridgeTimelineQuery_AroundCopyWithImpl<BridgeTimelineQuery_Around>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTimelineQuery_Around&&(identical(other.itemId, itemId) || other.itemId == itemId));
}


@override
int get hashCode {
    return Object.hash(runtimeType,itemId);
}

@override
String toString() {
    return 'BridgeTimelineQuery.around(itemId: $itemId)';
}


}

/// @nodoc
abstract mixin class $BridgeTimelineQuery_AroundCopyWith<$Res> implements $BridgeTimelineQueryCopyWith<$Res> {
  factory $BridgeTimelineQuery_AroundCopyWith(BridgeTimelineQuery_Around value, $Res Function(BridgeTimelineQuery_Around) _then) = _$BridgeTimelineQuery_AroundCopyWithImpl;
@useResult
$Res call({
 String itemId
});




}
/// @nodoc
class _$BridgeTimelineQuery_AroundCopyWithImpl<$Res>
    implements $BridgeTimelineQuery_AroundCopyWith<$Res> {
  _$BridgeTimelineQuery_AroundCopyWithImpl(this._self, this._then);

  final BridgeTimelineQuery_Around _self;
  final $Res Function(BridgeTimelineQuery_Around) _then;

/// Create a copy of BridgeTimelineQuery
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? itemId = null,}) {
  return _then(BridgeTimelineQuery_Around(
itemId: null == itemId ? _self.itemId : itemId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

// dart format on

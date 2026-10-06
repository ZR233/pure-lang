// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'shutdown.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgePendingPersistence {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgePendingPersistence);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgePendingPersistence()';
}


}

/// @nodoc
class $BridgePendingPersistenceCopyWith<$Res>  {
$BridgePendingPersistenceCopyWith(BridgePendingPersistence _, $Res Function(BridgePendingPersistence) __);
}


/// Adds pattern-matching-related methods to [BridgePendingPersistence].
extension BridgePendingPersistencePatterns on BridgePendingPersistence {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgePendingPersistence_Unknown value)?  unknown,TResult Function( BridgePendingPersistence_Pending value)?  pending,TResult Function( BridgePendingPersistence_Drained value)?  drained,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgePendingPersistence_Unknown() when unknown != null:
return unknown(_that);case BridgePendingPersistence_Pending() when pending != null:
return pending(_that);case BridgePendingPersistence_Drained() when drained != null:
return drained(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgePendingPersistence_Unknown value)  unknown,required TResult Function( BridgePendingPersistence_Pending value)  pending,required TResult Function( BridgePendingPersistence_Drained value)  drained,}){
final _that = this;
switch (_that) {
case BridgePendingPersistence_Unknown():
return unknown(_that);case BridgePendingPersistence_Pending():
return pending(_that);case BridgePendingPersistence_Drained():
return drained(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgePendingPersistence_Unknown value)?  unknown,TResult? Function( BridgePendingPersistence_Pending value)?  pending,TResult? Function( BridgePendingPersistence_Drained value)?  drained,}){
final _that = this;
switch (_that) {
case BridgePendingPersistence_Unknown() when unknown != null:
return unknown(_that);case BridgePendingPersistence_Pending() when pending != null:
return pending(_that);case BridgePendingPersistence_Drained() when drained != null:
return drained(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  unknown,TResult Function( BigInt count)?  pending,TResult Function()?  drained,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgePendingPersistence_Unknown() when unknown != null:
return unknown();case BridgePendingPersistence_Pending() when pending != null:
return pending(_that.count);case BridgePendingPersistence_Drained() when drained != null:
return drained();case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  unknown,required TResult Function( BigInt count)  pending,required TResult Function()  drained,}) {final _that = this;
switch (_that) {
case BridgePendingPersistence_Unknown():
return unknown();case BridgePendingPersistence_Pending():
return pending(_that.count);case BridgePendingPersistence_Drained():
return drained();}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  unknown,TResult? Function( BigInt count)?  pending,TResult? Function()?  drained,}) {final _that = this;
switch (_that) {
case BridgePendingPersistence_Unknown() when unknown != null:
return unknown();case BridgePendingPersistence_Pending() when pending != null:
return pending(_that.count);case BridgePendingPersistence_Drained() when drained != null:
return drained();case _:
  return null;

}
}

}

/// @nodoc


class BridgePendingPersistence_Unknown extends BridgePendingPersistence {
  const BridgePendingPersistence_Unknown(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgePendingPersistence_Unknown);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgePendingPersistence.unknown()';
}


}




/// @nodoc


class BridgePendingPersistence_Pending extends BridgePendingPersistence {
  const BridgePendingPersistence_Pending({required this.count}): super._();


 final  BigInt count;

/// Create a copy of BridgePendingPersistence
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgePendingPersistence_PendingCopyWith<BridgePendingPersistence_Pending> get copyWith => _$BridgePendingPersistence_PendingCopyWithImpl<BridgePendingPersistence_Pending>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgePendingPersistence_Pending&&(identical(other.count, count) || other.count == count));
}


@override
int get hashCode {
    return Object.hash(runtimeType,count);
}

@override
String toString() {
    return 'BridgePendingPersistence.pending(count: $count)';
}


}

/// @nodoc
abstract mixin class $BridgePendingPersistence_PendingCopyWith<$Res> implements $BridgePendingPersistenceCopyWith<$Res> {
  factory $BridgePendingPersistence_PendingCopyWith(BridgePendingPersistence_Pending value, $Res Function(BridgePendingPersistence_Pending) _then) = _$BridgePendingPersistence_PendingCopyWithImpl;
@useResult
$Res call({
 BigInt count
});




}
/// @nodoc
class _$BridgePendingPersistence_PendingCopyWithImpl<$Res>
    implements $BridgePendingPersistence_PendingCopyWith<$Res> {
  _$BridgePendingPersistence_PendingCopyWithImpl(this._self, this._then);

  final BridgePendingPersistence_Pending _self;
  final $Res Function(BridgePendingPersistence_Pending) _then;

/// Create a copy of BridgePendingPersistence
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? count = null,}) {
  return _then(BridgePendingPersistence_Pending(
count: null == count ? _self.count : count // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

/// @nodoc


class BridgePendingPersistence_Drained extends BridgePendingPersistence {
  const BridgePendingPersistence_Drained(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgePendingPersistence_Drained);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgePendingPersistence.drained()';
}


}




// dart format on

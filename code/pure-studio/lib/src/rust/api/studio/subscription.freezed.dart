// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'subscription.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeProductStreamEnvelope {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductStreamEnvelope);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeProductStreamEnvelope()';
}


}

/// @nodoc
class $BridgeProductStreamEnvelopeCopyWith<$Res>  {
$BridgeProductStreamEnvelopeCopyWith(BridgeProductStreamEnvelope _, $Res Function(BridgeProductStreamEnvelope) __);
}


/// Adds pattern-matching-related methods to [BridgeProductStreamEnvelope].
extension BridgeProductStreamEnvelopePatterns on BridgeProductStreamEnvelope {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeProductStreamEnvelope_Data value)?  data,TResult Function( BridgeProductStreamEnvelope_Failure value)?  failure,TResult Function( BridgeProductStreamEnvelope_Closed value)?  closed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeProductStreamEnvelope_Data() when data != null:
return data(_that);case BridgeProductStreamEnvelope_Failure() when failure != null:
return failure(_that);case BridgeProductStreamEnvelope_Closed() when closed != null:
return closed(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeProductStreamEnvelope_Data value)  data,required TResult Function( BridgeProductStreamEnvelope_Failure value)  failure,required TResult Function( BridgeProductStreamEnvelope_Closed value)  closed,}){
final _that = this;
switch (_that) {
case BridgeProductStreamEnvelope_Data():
return data(_that);case BridgeProductStreamEnvelope_Failure():
return failure(_that);case BridgeProductStreamEnvelope_Closed():
return closed(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeProductStreamEnvelope_Data value)?  data,TResult? Function( BridgeProductStreamEnvelope_Failure value)?  failure,TResult? Function( BridgeProductStreamEnvelope_Closed value)?  closed,}){
final _that = this;
switch (_that) {
case BridgeProductStreamEnvelope_Data() when data != null:
return data(_that);case BridgeProductStreamEnvelope_Failure() when failure != null:
return failure(_that);case BridgeProductStreamEnvelope_Closed() when closed != null:
return closed(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeProductEventEnvelope event)?  data,TResult Function( BridgeError error)?  failure,TResult Function()?  closed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeProductStreamEnvelope_Data() when data != null:
return data(_that.event);case BridgeProductStreamEnvelope_Failure() when failure != null:
return failure(_that.error);case BridgeProductStreamEnvelope_Closed() when closed != null:
return closed();case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeProductEventEnvelope event)  data,required TResult Function( BridgeError error)  failure,required TResult Function()  closed,}) {final _that = this;
switch (_that) {
case BridgeProductStreamEnvelope_Data():
return data(_that.event);case BridgeProductStreamEnvelope_Failure():
return failure(_that.error);case BridgeProductStreamEnvelope_Closed():
return closed();}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeProductEventEnvelope event)?  data,TResult? Function( BridgeError error)?  failure,TResult? Function()?  closed,}) {final _that = this;
switch (_that) {
case BridgeProductStreamEnvelope_Data() when data != null:
return data(_that.event);case BridgeProductStreamEnvelope_Failure() when failure != null:
return failure(_that.error);case BridgeProductStreamEnvelope_Closed() when closed != null:
return closed();case _:
  return null;

}
}

}

/// @nodoc


class BridgeProductStreamEnvelope_Data extends BridgeProductStreamEnvelope {
  const BridgeProductStreamEnvelope_Data({required this.event}): super._();


 final  BridgeProductEventEnvelope event;

/// Create a copy of BridgeProductStreamEnvelope
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductStreamEnvelope_DataCopyWith<BridgeProductStreamEnvelope_Data> get copyWith => _$BridgeProductStreamEnvelope_DataCopyWithImpl<BridgeProductStreamEnvelope_Data>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductStreamEnvelope_Data&&(identical(other.event, event) || other.event == event));
}


@override
int get hashCode => Object.hash(runtimeType,event);

@override
String toString() {
  return 'BridgeProductStreamEnvelope.data(event: $event)';
}


}

/// @nodoc
abstract mixin class $BridgeProductStreamEnvelope_DataCopyWith<$Res> implements $BridgeProductStreamEnvelopeCopyWith<$Res> {
  factory $BridgeProductStreamEnvelope_DataCopyWith(BridgeProductStreamEnvelope_Data value, $Res Function(BridgeProductStreamEnvelope_Data) _then) = _$BridgeProductStreamEnvelope_DataCopyWithImpl;
@useResult
$Res call({
 BridgeProductEventEnvelope event
});




}
/// @nodoc
class _$BridgeProductStreamEnvelope_DataCopyWithImpl<$Res>
    implements $BridgeProductStreamEnvelope_DataCopyWith<$Res> {
  _$BridgeProductStreamEnvelope_DataCopyWithImpl(this._self, this._then);

  final BridgeProductStreamEnvelope_Data _self;
  final $Res Function(BridgeProductStreamEnvelope_Data) _then;

/// Create a copy of BridgeProductStreamEnvelope
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? event = null,}) {
  return _then(BridgeProductStreamEnvelope_Data(
event: null == event ? _self.event : event // ignore: cast_nullable_to_non_nullable
as BridgeProductEventEnvelope,
  ));
}


}

/// @nodoc


class BridgeProductStreamEnvelope_Failure extends BridgeProductStreamEnvelope {
  const BridgeProductStreamEnvelope_Failure({required this.error}): super._();


 final  BridgeError error;

/// Create a copy of BridgeProductStreamEnvelope
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeProductStreamEnvelope_FailureCopyWith<BridgeProductStreamEnvelope_Failure> get copyWith => _$BridgeProductStreamEnvelope_FailureCopyWithImpl<BridgeProductStreamEnvelope_Failure>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductStreamEnvelope_Failure&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,error);

@override
String toString() {
  return 'BridgeProductStreamEnvelope.failure(error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeProductStreamEnvelope_FailureCopyWith<$Res> implements $BridgeProductStreamEnvelopeCopyWith<$Res> {
  factory $BridgeProductStreamEnvelope_FailureCopyWith(BridgeProductStreamEnvelope_Failure value, $Res Function(BridgeProductStreamEnvelope_Failure) _then) = _$BridgeProductStreamEnvelope_FailureCopyWithImpl;
@useResult
$Res call({
 BridgeError error
});




}
/// @nodoc
class _$BridgeProductStreamEnvelope_FailureCopyWithImpl<$Res>
    implements $BridgeProductStreamEnvelope_FailureCopyWith<$Res> {
  _$BridgeProductStreamEnvelope_FailureCopyWithImpl(this._self, this._then);

  final BridgeProductStreamEnvelope_Failure _self;
  final $Res Function(BridgeProductStreamEnvelope_Failure) _then;

/// Create a copy of BridgeProductStreamEnvelope
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? error = null,}) {
  return _then(BridgeProductStreamEnvelope_Failure(
error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as BridgeError,
  ));
}


}

/// @nodoc


class BridgeProductStreamEnvelope_Closed extends BridgeProductStreamEnvelope {
  const BridgeProductStreamEnvelope_Closed(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeProductStreamEnvelope_Closed);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeProductStreamEnvelope.closed()';
}


}




/// @nodoc
mixin _$BridgeThreadStreamEnvelope {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadStreamEnvelope);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadStreamEnvelope()';
}


}

/// @nodoc
class $BridgeThreadStreamEnvelopeCopyWith<$Res>  {
$BridgeThreadStreamEnvelopeCopyWith(BridgeThreadStreamEnvelope _, $Res Function(BridgeThreadStreamEnvelope) __);
}


/// Adds pattern-matching-related methods to [BridgeThreadStreamEnvelope].
extension BridgeThreadStreamEnvelopePatterns on BridgeThreadStreamEnvelope {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeThreadStreamEnvelope_Data value)?  data,TResult Function( BridgeThreadStreamEnvelope_Failure value)?  failure,TResult Function( BridgeThreadStreamEnvelope_Closed value)?  closed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeThreadStreamEnvelope_Data() when data != null:
return data(_that);case BridgeThreadStreamEnvelope_Failure() when failure != null:
return failure(_that);case BridgeThreadStreamEnvelope_Closed() when closed != null:
return closed(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeThreadStreamEnvelope_Data value)  data,required TResult Function( BridgeThreadStreamEnvelope_Failure value)  failure,required TResult Function( BridgeThreadStreamEnvelope_Closed value)  closed,}){
final _that = this;
switch (_that) {
case BridgeThreadStreamEnvelope_Data():
return data(_that);case BridgeThreadStreamEnvelope_Failure():
return failure(_that);case BridgeThreadStreamEnvelope_Closed():
return closed(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeThreadStreamEnvelope_Data value)?  data,TResult? Function( BridgeThreadStreamEnvelope_Failure value)?  failure,TResult? Function( BridgeThreadStreamEnvelope_Closed value)?  closed,}){
final _that = this;
switch (_that) {
case BridgeThreadStreamEnvelope_Data() when data != null:
return data(_that);case BridgeThreadStreamEnvelope_Failure() when failure != null:
return failure(_that);case BridgeThreadStreamEnvelope_Closed() when closed != null:
return closed(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeThreadSubscriptionUpdate update)?  data,TResult Function( BridgeError error)?  failure,TResult Function()?  closed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeThreadStreamEnvelope_Data() when data != null:
return data(_that.update);case BridgeThreadStreamEnvelope_Failure() when failure != null:
return failure(_that.error);case BridgeThreadStreamEnvelope_Closed() when closed != null:
return closed();case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeThreadSubscriptionUpdate update)  data,required TResult Function( BridgeError error)  failure,required TResult Function()  closed,}) {final _that = this;
switch (_that) {
case BridgeThreadStreamEnvelope_Data():
return data(_that.update);case BridgeThreadStreamEnvelope_Failure():
return failure(_that.error);case BridgeThreadStreamEnvelope_Closed():
return closed();}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeThreadSubscriptionUpdate update)?  data,TResult? Function( BridgeError error)?  failure,TResult? Function()?  closed,}) {final _that = this;
switch (_that) {
case BridgeThreadStreamEnvelope_Data() when data != null:
return data(_that.update);case BridgeThreadStreamEnvelope_Failure() when failure != null:
return failure(_that.error);case BridgeThreadStreamEnvelope_Closed() when closed != null:
return closed();case _:
  return null;

}
}

}

/// @nodoc


class BridgeThreadStreamEnvelope_Data extends BridgeThreadStreamEnvelope {
  const BridgeThreadStreamEnvelope_Data({required this.update}): super._();


 final  BridgeThreadSubscriptionUpdate update;

/// Create a copy of BridgeThreadStreamEnvelope
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadStreamEnvelope_DataCopyWith<BridgeThreadStreamEnvelope_Data> get copyWith => _$BridgeThreadStreamEnvelope_DataCopyWithImpl<BridgeThreadStreamEnvelope_Data>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadStreamEnvelope_Data&&(identical(other.update, update) || other.update == update));
}


@override
int get hashCode => Object.hash(runtimeType,update);

@override
String toString() {
  return 'BridgeThreadStreamEnvelope.data(update: $update)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadStreamEnvelope_DataCopyWith<$Res> implements $BridgeThreadStreamEnvelopeCopyWith<$Res> {
  factory $BridgeThreadStreamEnvelope_DataCopyWith(BridgeThreadStreamEnvelope_Data value, $Res Function(BridgeThreadStreamEnvelope_Data) _then) = _$BridgeThreadStreamEnvelope_DataCopyWithImpl;
@useResult
$Res call({
 BridgeThreadSubscriptionUpdate update
});


$BridgeThreadSubscriptionUpdateCopyWith<$Res> get update;

}
/// @nodoc
class _$BridgeThreadStreamEnvelope_DataCopyWithImpl<$Res>
    implements $BridgeThreadStreamEnvelope_DataCopyWith<$Res> {
  _$BridgeThreadStreamEnvelope_DataCopyWithImpl(this._self, this._then);

  final BridgeThreadStreamEnvelope_Data _self;
  final $Res Function(BridgeThreadStreamEnvelope_Data) _then;

/// Create a copy of BridgeThreadStreamEnvelope
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? update = null,}) {
  return _then(BridgeThreadStreamEnvelope_Data(
update: null == update ? _self.update : update // ignore: cast_nullable_to_non_nullable
as BridgeThreadSubscriptionUpdate,
  ));
}

/// Create a copy of BridgeThreadStreamEnvelope
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeThreadSubscriptionUpdateCopyWith<$Res> get update {

  return $BridgeThreadSubscriptionUpdateCopyWith<$Res>(_self.update, (value) {
    return _then(_self.copyWith(update: value));
  });
}
}

/// @nodoc


class BridgeThreadStreamEnvelope_Failure extends BridgeThreadStreamEnvelope {
  const BridgeThreadStreamEnvelope_Failure({required this.error}): super._();


 final  BridgeError error;

/// Create a copy of BridgeThreadStreamEnvelope
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadStreamEnvelope_FailureCopyWith<BridgeThreadStreamEnvelope_Failure> get copyWith => _$BridgeThreadStreamEnvelope_FailureCopyWithImpl<BridgeThreadStreamEnvelope_Failure>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadStreamEnvelope_Failure&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,error);

@override
String toString() {
  return 'BridgeThreadStreamEnvelope.failure(error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadStreamEnvelope_FailureCopyWith<$Res> implements $BridgeThreadStreamEnvelopeCopyWith<$Res> {
  factory $BridgeThreadStreamEnvelope_FailureCopyWith(BridgeThreadStreamEnvelope_Failure value, $Res Function(BridgeThreadStreamEnvelope_Failure) _then) = _$BridgeThreadStreamEnvelope_FailureCopyWithImpl;
@useResult
$Res call({
 BridgeError error
});




}
/// @nodoc
class _$BridgeThreadStreamEnvelope_FailureCopyWithImpl<$Res>
    implements $BridgeThreadStreamEnvelope_FailureCopyWith<$Res> {
  _$BridgeThreadStreamEnvelope_FailureCopyWithImpl(this._self, this._then);

  final BridgeThreadStreamEnvelope_Failure _self;
  final $Res Function(BridgeThreadStreamEnvelope_Failure) _then;

/// Create a copy of BridgeThreadStreamEnvelope
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? error = null,}) {
  return _then(BridgeThreadStreamEnvelope_Failure(
error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as BridgeError,
  ));
}


}

/// @nodoc


class BridgeThreadStreamEnvelope_Closed extends BridgeThreadStreamEnvelope {
  const BridgeThreadStreamEnvelope_Closed(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadStreamEnvelope_Closed);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadStreamEnvelope.closed()';
}


}




// dart format on

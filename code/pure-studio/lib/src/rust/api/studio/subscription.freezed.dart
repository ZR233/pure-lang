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
mixin _$BridgeSessionStreamEnvelope {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionStreamEnvelope);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeSessionStreamEnvelope()';
}


}

/// @nodoc
class $BridgeSessionStreamEnvelopeCopyWith<$Res>  {
$BridgeSessionStreamEnvelopeCopyWith(BridgeSessionStreamEnvelope _, $Res Function(BridgeSessionStreamEnvelope) __);
}


/// Adds pattern-matching-related methods to [BridgeSessionStreamEnvelope].
extension BridgeSessionStreamEnvelopePatterns on BridgeSessionStreamEnvelope {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeSessionStreamEnvelope_Data value)?  data,TResult Function( BridgeSessionStreamEnvelope_Failure value)?  failure,TResult Function( BridgeSessionStreamEnvelope_Closed value)?  closed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeSessionStreamEnvelope_Data() when data != null:
return data(_that);case BridgeSessionStreamEnvelope_Failure() when failure != null:
return failure(_that);case BridgeSessionStreamEnvelope_Closed() when closed != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeSessionStreamEnvelope_Data value)  data,required TResult Function( BridgeSessionStreamEnvelope_Failure value)  failure,required TResult Function( BridgeSessionStreamEnvelope_Closed value)  closed,}){
final _that = this;
switch (_that) {
case BridgeSessionStreamEnvelope_Data():
return data(_that);case BridgeSessionStreamEnvelope_Failure():
return failure(_that);case BridgeSessionStreamEnvelope_Closed():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeSessionStreamEnvelope_Data value)?  data,TResult? Function( BridgeSessionStreamEnvelope_Failure value)?  failure,TResult? Function( BridgeSessionStreamEnvelope_Closed value)?  closed,}){
final _that = this;
switch (_that) {
case BridgeSessionStreamEnvelope_Data() when data != null:
return data(_that);case BridgeSessionStreamEnvelope_Failure() when failure != null:
return failure(_that);case BridgeSessionStreamEnvelope_Closed() when closed != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeSessionStreamFrame frame)?  data,TResult Function( BridgeError error)?  failure,TResult Function()?  closed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeSessionStreamEnvelope_Data() when data != null:
return data(_that.frame);case BridgeSessionStreamEnvelope_Failure() when failure != null:
return failure(_that.error);case BridgeSessionStreamEnvelope_Closed() when closed != null:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeSessionStreamFrame frame)  data,required TResult Function( BridgeError error)  failure,required TResult Function()  closed,}) {final _that = this;
switch (_that) {
case BridgeSessionStreamEnvelope_Data():
return data(_that.frame);case BridgeSessionStreamEnvelope_Failure():
return failure(_that.error);case BridgeSessionStreamEnvelope_Closed():
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeSessionStreamFrame frame)?  data,TResult? Function( BridgeError error)?  failure,TResult? Function()?  closed,}) {final _that = this;
switch (_that) {
case BridgeSessionStreamEnvelope_Data() when data != null:
return data(_that.frame);case BridgeSessionStreamEnvelope_Failure() when failure != null:
return failure(_that.error);case BridgeSessionStreamEnvelope_Closed() when closed != null:
return closed();case _:
  return null;

}
}

}

/// @nodoc


class BridgeSessionStreamEnvelope_Data extends BridgeSessionStreamEnvelope {
  const BridgeSessionStreamEnvelope_Data({required this.frame}): super._();


 final  BridgeSessionStreamFrame frame;

/// Create a copy of BridgeSessionStreamEnvelope
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionStreamEnvelope_DataCopyWith<BridgeSessionStreamEnvelope_Data> get copyWith => _$BridgeSessionStreamEnvelope_DataCopyWithImpl<BridgeSessionStreamEnvelope_Data>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionStreamEnvelope_Data&&(identical(other.frame, frame) || other.frame == frame));
}


@override
int get hashCode => Object.hash(runtimeType,frame);

@override
String toString() {
  return 'BridgeSessionStreamEnvelope.data(frame: $frame)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionStreamEnvelope_DataCopyWith<$Res> implements $BridgeSessionStreamEnvelopeCopyWith<$Res> {
  factory $BridgeSessionStreamEnvelope_DataCopyWith(BridgeSessionStreamEnvelope_Data value, $Res Function(BridgeSessionStreamEnvelope_Data) _then) = _$BridgeSessionStreamEnvelope_DataCopyWithImpl;
@useResult
$Res call({
 BridgeSessionStreamFrame frame
});


$BridgeSessionStreamFrameCopyWith<$Res> get frame;

}
/// @nodoc
class _$BridgeSessionStreamEnvelope_DataCopyWithImpl<$Res>
    implements $BridgeSessionStreamEnvelope_DataCopyWith<$Res> {
  _$BridgeSessionStreamEnvelope_DataCopyWithImpl(this._self, this._then);

  final BridgeSessionStreamEnvelope_Data _self;
  final $Res Function(BridgeSessionStreamEnvelope_Data) _then;

/// Create a copy of BridgeSessionStreamEnvelope
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? frame = null,}) {
  return _then(BridgeSessionStreamEnvelope_Data(
frame: null == frame ? _self.frame : frame // ignore: cast_nullable_to_non_nullable
as BridgeSessionStreamFrame,
  ));
}

/// Create a copy of BridgeSessionStreamEnvelope
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeSessionStreamFrameCopyWith<$Res> get frame {

  return $BridgeSessionStreamFrameCopyWith<$Res>(_self.frame, (value) {
    return _then(_self.copyWith(frame: value));
  });
}
}

/// @nodoc


class BridgeSessionStreamEnvelope_Failure extends BridgeSessionStreamEnvelope {
  const BridgeSessionStreamEnvelope_Failure({required this.error}): super._();


 final  BridgeError error;

/// Create a copy of BridgeSessionStreamEnvelope
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionStreamEnvelope_FailureCopyWith<BridgeSessionStreamEnvelope_Failure> get copyWith => _$BridgeSessionStreamEnvelope_FailureCopyWithImpl<BridgeSessionStreamEnvelope_Failure>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionStreamEnvelope_Failure&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,error);

@override
String toString() {
  return 'BridgeSessionStreamEnvelope.failure(error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionStreamEnvelope_FailureCopyWith<$Res> implements $BridgeSessionStreamEnvelopeCopyWith<$Res> {
  factory $BridgeSessionStreamEnvelope_FailureCopyWith(BridgeSessionStreamEnvelope_Failure value, $Res Function(BridgeSessionStreamEnvelope_Failure) _then) = _$BridgeSessionStreamEnvelope_FailureCopyWithImpl;
@useResult
$Res call({
 BridgeError error
});




}
/// @nodoc
class _$BridgeSessionStreamEnvelope_FailureCopyWithImpl<$Res>
    implements $BridgeSessionStreamEnvelope_FailureCopyWith<$Res> {
  _$BridgeSessionStreamEnvelope_FailureCopyWithImpl(this._self, this._then);

  final BridgeSessionStreamEnvelope_Failure _self;
  final $Res Function(BridgeSessionStreamEnvelope_Failure) _then;

/// Create a copy of BridgeSessionStreamEnvelope
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? error = null,}) {
  return _then(BridgeSessionStreamEnvelope_Failure(
error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as BridgeError,
  ));
}


}

/// @nodoc


class BridgeSessionStreamEnvelope_Closed extends BridgeSessionStreamEnvelope {
  const BridgeSessionStreamEnvelope_Closed(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionStreamEnvelope_Closed);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeSessionStreamEnvelope.closed()';
}


}




// dart format on

// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'runtime.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeObservedStatePhase {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeObservedStatePhase);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeObservedStatePhase()';
}


}

/// @nodoc
class $BridgeObservedStatePhaseCopyWith<$Res>  {
$BridgeObservedStatePhaseCopyWith(BridgeObservedStatePhase _, $Res Function(BridgeObservedStatePhase) __);
}


/// Adds pattern-matching-related methods to [BridgeObservedStatePhase].
extension BridgeObservedStatePhasePatterns on BridgeObservedStatePhase {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeObservedStatePhase_Uninitialized value)?  uninitialized,TResult Function( BridgeObservedStatePhase_Ready value)?  ready,TResult Function( BridgeObservedStatePhase_Running value)?  running,TResult Function( BridgeObservedStatePhase_Failed value)?  failed,TResult Function( BridgeObservedStatePhase_Stopped value)?  stopped,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeObservedStatePhase_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeObservedStatePhase_Ready() when ready != null:
return ready(_that);case BridgeObservedStatePhase_Running() when running != null:
return running(_that);case BridgeObservedStatePhase_Failed() when failed != null:
return failed(_that);case BridgeObservedStatePhase_Stopped() when stopped != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeObservedStatePhase_Uninitialized value)  uninitialized,required TResult Function( BridgeObservedStatePhase_Ready value)  ready,required TResult Function( BridgeObservedStatePhase_Running value)  running,required TResult Function( BridgeObservedStatePhase_Failed value)  failed,required TResult Function( BridgeObservedStatePhase_Stopped value)  stopped,}){
final _that = this;
switch (_that) {
case BridgeObservedStatePhase_Uninitialized():
return uninitialized(_that);case BridgeObservedStatePhase_Ready():
return ready(_that);case BridgeObservedStatePhase_Running():
return running(_that);case BridgeObservedStatePhase_Failed():
return failed(_that);case BridgeObservedStatePhase_Stopped():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeObservedStatePhase_Uninitialized value)?  uninitialized,TResult? Function( BridgeObservedStatePhase_Ready value)?  ready,TResult? Function( BridgeObservedStatePhase_Running value)?  running,TResult? Function( BridgeObservedStatePhase_Failed value)?  failed,TResult? Function( BridgeObservedStatePhase_Stopped value)?  stopped,}){
final _that = this;
switch (_that) {
case BridgeObservedStatePhase_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeObservedStatePhase_Ready() when ready != null:
return ready(_that);case BridgeObservedStatePhase_Running() when running != null:
return running(_that);case BridgeObservedStatePhase_Failed() when failed != null:
return failed(_that);case BridgeObservedStatePhase_Stopped() when stopped != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  uninitialized,TResult Function()?  ready,TResult Function( BridgeStateOperation operation,  String operationId)?  running,TResult Function( BridgeStateOperation operation,  BridgeStateError error)?  failed,TResult Function()?  stopped,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeObservedStatePhase_Uninitialized() when uninitialized != null:
return uninitialized();case BridgeObservedStatePhase_Ready() when ready != null:
return ready();case BridgeObservedStatePhase_Running() when running != null:
return running(_that.operation,_that.operationId);case BridgeObservedStatePhase_Failed() when failed != null:
return failed(_that.operation,_that.error);case BridgeObservedStatePhase_Stopped() when stopped != null:
return stopped();case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  uninitialized,required TResult Function()  ready,required TResult Function( BridgeStateOperation operation,  String operationId)  running,required TResult Function( BridgeStateOperation operation,  BridgeStateError error)  failed,required TResult Function()  stopped,}) {final _that = this;
switch (_that) {
case BridgeObservedStatePhase_Uninitialized():
return uninitialized();case BridgeObservedStatePhase_Ready():
return ready();case BridgeObservedStatePhase_Running():
return running(_that.operation,_that.operationId);case BridgeObservedStatePhase_Failed():
return failed(_that.operation,_that.error);case BridgeObservedStatePhase_Stopped():
return stopped();}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  uninitialized,TResult? Function()?  ready,TResult? Function( BridgeStateOperation operation,  String operationId)?  running,TResult? Function( BridgeStateOperation operation,  BridgeStateError error)?  failed,TResult? Function()?  stopped,}) {final _that = this;
switch (_that) {
case BridgeObservedStatePhase_Uninitialized() when uninitialized != null:
return uninitialized();case BridgeObservedStatePhase_Ready() when ready != null:
return ready();case BridgeObservedStatePhase_Running() when running != null:
return running(_that.operation,_that.operationId);case BridgeObservedStatePhase_Failed() when failed != null:
return failed(_that.operation,_that.error);case BridgeObservedStatePhase_Stopped() when stopped != null:
return stopped();case _:
  return null;

}
}

}

/// @nodoc


class BridgeObservedStatePhase_Uninitialized extends BridgeObservedStatePhase {
  const BridgeObservedStatePhase_Uninitialized(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeObservedStatePhase_Uninitialized);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeObservedStatePhase.uninitialized()';
}


}




/// @nodoc


class BridgeObservedStatePhase_Ready extends BridgeObservedStatePhase {
  const BridgeObservedStatePhase_Ready(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeObservedStatePhase_Ready);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeObservedStatePhase.ready()';
}


}




/// @nodoc


class BridgeObservedStatePhase_Running extends BridgeObservedStatePhase {
  const BridgeObservedStatePhase_Running({required this.operation, required this.operationId}): super._();


 final  BridgeStateOperation operation;
 final  String operationId;

/// Create a copy of BridgeObservedStatePhase
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeObservedStatePhase_RunningCopyWith<BridgeObservedStatePhase_Running> get copyWith => _$BridgeObservedStatePhase_RunningCopyWithImpl<BridgeObservedStatePhase_Running>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeObservedStatePhase_Running&&(identical(other.operation, operation) || other.operation == operation)&&(identical(other.operationId, operationId) || other.operationId == operationId));
}


@override
int get hashCode => Object.hash(runtimeType,operation,operationId);

@override
String toString() {
  return 'BridgeObservedStatePhase.running(operation: $operation, operationId: $operationId)';
}


}

/// @nodoc
abstract mixin class $BridgeObservedStatePhase_RunningCopyWith<$Res> implements $BridgeObservedStatePhaseCopyWith<$Res> {
  factory $BridgeObservedStatePhase_RunningCopyWith(BridgeObservedStatePhase_Running value, $Res Function(BridgeObservedStatePhase_Running) _then) = _$BridgeObservedStatePhase_RunningCopyWithImpl;
@useResult
$Res call({
 BridgeStateOperation operation, String operationId
});




}
/// @nodoc
class _$BridgeObservedStatePhase_RunningCopyWithImpl<$Res>
    implements $BridgeObservedStatePhase_RunningCopyWith<$Res> {
  _$BridgeObservedStatePhase_RunningCopyWithImpl(this._self, this._then);

  final BridgeObservedStatePhase_Running _self;
  final $Res Function(BridgeObservedStatePhase_Running) _then;

/// Create a copy of BridgeObservedStatePhase
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? operation = null,Object? operationId = null,}) {
  return _then(BridgeObservedStatePhase_Running(
operation: null == operation ? _self.operation : operation // ignore: cast_nullable_to_non_nullable
as BridgeStateOperation,operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeObservedStatePhase_Failed extends BridgeObservedStatePhase {
  const BridgeObservedStatePhase_Failed({required this.operation, required this.error}): super._();


 final  BridgeStateOperation operation;
 final  BridgeStateError error;

/// Create a copy of BridgeObservedStatePhase
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeObservedStatePhase_FailedCopyWith<BridgeObservedStatePhase_Failed> get copyWith => _$BridgeObservedStatePhase_FailedCopyWithImpl<BridgeObservedStatePhase_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeObservedStatePhase_Failed&&(identical(other.operation, operation) || other.operation == operation)&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,operation,error);

@override
String toString() {
  return 'BridgeObservedStatePhase.failed(operation: $operation, error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeObservedStatePhase_FailedCopyWith<$Res> implements $BridgeObservedStatePhaseCopyWith<$Res> {
  factory $BridgeObservedStatePhase_FailedCopyWith(BridgeObservedStatePhase_Failed value, $Res Function(BridgeObservedStatePhase_Failed) _then) = _$BridgeObservedStatePhase_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeStateOperation operation, BridgeStateError error
});




}
/// @nodoc
class _$BridgeObservedStatePhase_FailedCopyWithImpl<$Res>
    implements $BridgeObservedStatePhase_FailedCopyWith<$Res> {
  _$BridgeObservedStatePhase_FailedCopyWithImpl(this._self, this._then);

  final BridgeObservedStatePhase_Failed _self;
  final $Res Function(BridgeObservedStatePhase_Failed) _then;

/// Create a copy of BridgeObservedStatePhase
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? operation = null,Object? error = null,}) {
  return _then(BridgeObservedStatePhase_Failed(
operation: null == operation ? _self.operation : operation // ignore: cast_nullable_to_non_nullable
as BridgeStateOperation,error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as BridgeStateError,
  ));
}


}

/// @nodoc


class BridgeObservedStatePhase_Stopped extends BridgeObservedStatePhase {
  const BridgeObservedStatePhase_Stopped(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeObservedStatePhase_Stopped);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeObservedStatePhase.stopped()';
}


}




// dart format on

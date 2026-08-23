// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'runtime.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeAgentState {

 Object get field0;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentState&&const DeepCollectionEquality().equals(other.field0, field0));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(field0));

@override
String toString() {
  return 'BridgeAgentState(field0: $field0)';
}


}

/// @nodoc
class $BridgeAgentStateCopyWith<$Res>  {
$BridgeAgentStateCopyWith(BridgeAgentState _, $Res Function(BridgeAgentState) __);
}


/// Adds pattern-matching-related methods to [BridgeAgentState].
extension BridgeAgentStatePatterns on BridgeAgentState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeAgentState_Idle value)?  idle,TResult Function( BridgeAgentState_Queued value)?  queued,TResult Function( BridgeAgentState_Running value)?  running,TResult Function( BridgeAgentState_WaitingTool value)?  waitingTool,TResult Function( BridgeAgentState_WaitingInteraction value)?  waitingInteraction,TResult Function( BridgeAgentState_Cancelling value)?  cancelling,TResult Function( BridgeAgentState_Closing value)?  closing,TResult Function( BridgeAgentState_Closed value)?  closed,TResult Function( BridgeAgentState_Faulted value)?  faulted,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeAgentState_Idle() when idle != null:
return idle(_that);case BridgeAgentState_Queued() when queued != null:
return queued(_that);case BridgeAgentState_Running() when running != null:
return running(_that);case BridgeAgentState_WaitingTool() when waitingTool != null:
return waitingTool(_that);case BridgeAgentState_WaitingInteraction() when waitingInteraction != null:
return waitingInteraction(_that);case BridgeAgentState_Cancelling() when cancelling != null:
return cancelling(_that);case BridgeAgentState_Closing() when closing != null:
return closing(_that);case BridgeAgentState_Closed() when closed != null:
return closed(_that);case BridgeAgentState_Faulted() when faulted != null:
return faulted(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeAgentState_Idle value)  idle,required TResult Function( BridgeAgentState_Queued value)  queued,required TResult Function( BridgeAgentState_Running value)  running,required TResult Function( BridgeAgentState_WaitingTool value)  waitingTool,required TResult Function( BridgeAgentState_WaitingInteraction value)  waitingInteraction,required TResult Function( BridgeAgentState_Cancelling value)  cancelling,required TResult Function( BridgeAgentState_Closing value)  closing,required TResult Function( BridgeAgentState_Closed value)  closed,required TResult Function( BridgeAgentState_Faulted value)  faulted,}){
final _that = this;
switch (_that) {
case BridgeAgentState_Idle():
return idle(_that);case BridgeAgentState_Queued():
return queued(_that);case BridgeAgentState_Running():
return running(_that);case BridgeAgentState_WaitingTool():
return waitingTool(_that);case BridgeAgentState_WaitingInteraction():
return waitingInteraction(_that);case BridgeAgentState_Cancelling():
return cancelling(_that);case BridgeAgentState_Closing():
return closing(_that);case BridgeAgentState_Closed():
return closed(_that);case BridgeAgentState_Faulted():
return faulted(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeAgentState_Idle value)?  idle,TResult? Function( BridgeAgentState_Queued value)?  queued,TResult? Function( BridgeAgentState_Running value)?  running,TResult? Function( BridgeAgentState_WaitingTool value)?  waitingTool,TResult? Function( BridgeAgentState_WaitingInteraction value)?  waitingInteraction,TResult? Function( BridgeAgentState_Cancelling value)?  cancelling,TResult? Function( BridgeAgentState_Closing value)?  closing,TResult? Function( BridgeAgentState_Closed value)?  closed,TResult? Function( BridgeAgentState_Faulted value)?  faulted,}){
final _that = this;
switch (_that) {
case BridgeAgentState_Idle() when idle != null:
return idle(_that);case BridgeAgentState_Queued() when queued != null:
return queued(_that);case BridgeAgentState_Running() when running != null:
return running(_that);case BridgeAgentState_WaitingTool() when waitingTool != null:
return waitingTool(_that);case BridgeAgentState_WaitingInteraction() when waitingInteraction != null:
return waitingInteraction(_that);case BridgeAgentState_Cancelling() when cancelling != null:
return cancelling(_that);case BridgeAgentState_Closing() when closing != null:
return closing(_that);case BridgeAgentState_Closed() when closed != null:
return closed(_that);case BridgeAgentState_Faulted() when faulted != null:
return faulted(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeIdleAgent field0)?  idle,TResult Function( BridgeQueuedAgent field0)?  queued,TResult Function( BridgeRunningAgent field0)?  running,TResult Function( BridgeWaitingToolAgent field0)?  waitingTool,TResult Function( BridgeWaitingInteractionAgent field0)?  waitingInteraction,TResult Function( BridgeCancellingAgent field0)?  cancelling,TResult Function( BridgeClosingAgent field0)?  closing,TResult Function( BridgeClosedAgent field0)?  closed,TResult Function( BridgeFaultedAgent field0)?  faulted,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeAgentState_Idle() when idle != null:
return idle(_that.field0);case BridgeAgentState_Queued() when queued != null:
return queued(_that.field0);case BridgeAgentState_Running() when running != null:
return running(_that.field0);case BridgeAgentState_WaitingTool() when waitingTool != null:
return waitingTool(_that.field0);case BridgeAgentState_WaitingInteraction() when waitingInteraction != null:
return waitingInteraction(_that.field0);case BridgeAgentState_Cancelling() when cancelling != null:
return cancelling(_that.field0);case BridgeAgentState_Closing() when closing != null:
return closing(_that.field0);case BridgeAgentState_Closed() when closed != null:
return closed(_that.field0);case BridgeAgentState_Faulted() when faulted != null:
return faulted(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeIdleAgent field0)  idle,required TResult Function( BridgeQueuedAgent field0)  queued,required TResult Function( BridgeRunningAgent field0)  running,required TResult Function( BridgeWaitingToolAgent field0)  waitingTool,required TResult Function( BridgeWaitingInteractionAgent field0)  waitingInteraction,required TResult Function( BridgeCancellingAgent field0)  cancelling,required TResult Function( BridgeClosingAgent field0)  closing,required TResult Function( BridgeClosedAgent field0)  closed,required TResult Function( BridgeFaultedAgent field0)  faulted,}) {final _that = this;
switch (_that) {
case BridgeAgentState_Idle():
return idle(_that.field0);case BridgeAgentState_Queued():
return queued(_that.field0);case BridgeAgentState_Running():
return running(_that.field0);case BridgeAgentState_WaitingTool():
return waitingTool(_that.field0);case BridgeAgentState_WaitingInteraction():
return waitingInteraction(_that.field0);case BridgeAgentState_Cancelling():
return cancelling(_that.field0);case BridgeAgentState_Closing():
return closing(_that.field0);case BridgeAgentState_Closed():
return closed(_that.field0);case BridgeAgentState_Faulted():
return faulted(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeIdleAgent field0)?  idle,TResult? Function( BridgeQueuedAgent field0)?  queued,TResult? Function( BridgeRunningAgent field0)?  running,TResult? Function( BridgeWaitingToolAgent field0)?  waitingTool,TResult? Function( BridgeWaitingInteractionAgent field0)?  waitingInteraction,TResult? Function( BridgeCancellingAgent field0)?  cancelling,TResult? Function( BridgeClosingAgent field0)?  closing,TResult? Function( BridgeClosedAgent field0)?  closed,TResult? Function( BridgeFaultedAgent field0)?  faulted,}) {final _that = this;
switch (_that) {
case BridgeAgentState_Idle() when idle != null:
return idle(_that.field0);case BridgeAgentState_Queued() when queued != null:
return queued(_that.field0);case BridgeAgentState_Running() when running != null:
return running(_that.field0);case BridgeAgentState_WaitingTool() when waitingTool != null:
return waitingTool(_that.field0);case BridgeAgentState_WaitingInteraction() when waitingInteraction != null:
return waitingInteraction(_that.field0);case BridgeAgentState_Cancelling() when cancelling != null:
return cancelling(_that.field0);case BridgeAgentState_Closing() when closing != null:
return closing(_that.field0);case BridgeAgentState_Closed() when closed != null:
return closed(_that.field0);case BridgeAgentState_Faulted() when faulted != null:
return faulted(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeAgentState_Idle extends BridgeAgentState {
  const BridgeAgentState_Idle(this.field0): super._();


@override final  BridgeIdleAgent field0;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentState_IdleCopyWith<BridgeAgentState_Idle> get copyWith => _$BridgeAgentState_IdleCopyWithImpl<BridgeAgentState_Idle>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentState_Idle&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeAgentState.idle(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentState_IdleCopyWith<$Res> implements $BridgeAgentStateCopyWith<$Res> {
  factory $BridgeAgentState_IdleCopyWith(BridgeAgentState_Idle value, $Res Function(BridgeAgentState_Idle) _then) = _$BridgeAgentState_IdleCopyWithImpl;
@useResult
$Res call({
 BridgeIdleAgent field0
});




}
/// @nodoc
class _$BridgeAgentState_IdleCopyWithImpl<$Res>
    implements $BridgeAgentState_IdleCopyWith<$Res> {
  _$BridgeAgentState_IdleCopyWithImpl(this._self, this._then);

  final BridgeAgentState_Idle _self;
  final $Res Function(BridgeAgentState_Idle) _then;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentState_Idle(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeIdleAgent,
  ));
}


}

/// @nodoc


class BridgeAgentState_Queued extends BridgeAgentState {
  const BridgeAgentState_Queued(this.field0): super._();


@override final  BridgeQueuedAgent field0;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentState_QueuedCopyWith<BridgeAgentState_Queued> get copyWith => _$BridgeAgentState_QueuedCopyWithImpl<BridgeAgentState_Queued>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentState_Queued&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeAgentState.queued(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentState_QueuedCopyWith<$Res> implements $BridgeAgentStateCopyWith<$Res> {
  factory $BridgeAgentState_QueuedCopyWith(BridgeAgentState_Queued value, $Res Function(BridgeAgentState_Queued) _then) = _$BridgeAgentState_QueuedCopyWithImpl;
@useResult
$Res call({
 BridgeQueuedAgent field0
});




}
/// @nodoc
class _$BridgeAgentState_QueuedCopyWithImpl<$Res>
    implements $BridgeAgentState_QueuedCopyWith<$Res> {
  _$BridgeAgentState_QueuedCopyWithImpl(this._self, this._then);

  final BridgeAgentState_Queued _self;
  final $Res Function(BridgeAgentState_Queued) _then;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentState_Queued(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeQueuedAgent,
  ));
}


}

/// @nodoc


class BridgeAgentState_Running extends BridgeAgentState {
  const BridgeAgentState_Running(this.field0): super._();


@override final  BridgeRunningAgent field0;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentState_RunningCopyWith<BridgeAgentState_Running> get copyWith => _$BridgeAgentState_RunningCopyWithImpl<BridgeAgentState_Running>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentState_Running&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeAgentState.running(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentState_RunningCopyWith<$Res> implements $BridgeAgentStateCopyWith<$Res> {
  factory $BridgeAgentState_RunningCopyWith(BridgeAgentState_Running value, $Res Function(BridgeAgentState_Running) _then) = _$BridgeAgentState_RunningCopyWithImpl;
@useResult
$Res call({
 BridgeRunningAgent field0
});




}
/// @nodoc
class _$BridgeAgentState_RunningCopyWithImpl<$Res>
    implements $BridgeAgentState_RunningCopyWith<$Res> {
  _$BridgeAgentState_RunningCopyWithImpl(this._self, this._then);

  final BridgeAgentState_Running _self;
  final $Res Function(BridgeAgentState_Running) _then;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentState_Running(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeRunningAgent,
  ));
}


}

/// @nodoc


class BridgeAgentState_WaitingTool extends BridgeAgentState {
  const BridgeAgentState_WaitingTool(this.field0): super._();


@override final  BridgeWaitingToolAgent field0;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentState_WaitingToolCopyWith<BridgeAgentState_WaitingTool> get copyWith => _$BridgeAgentState_WaitingToolCopyWithImpl<BridgeAgentState_WaitingTool>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentState_WaitingTool&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeAgentState.waitingTool(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentState_WaitingToolCopyWith<$Res> implements $BridgeAgentStateCopyWith<$Res> {
  factory $BridgeAgentState_WaitingToolCopyWith(BridgeAgentState_WaitingTool value, $Res Function(BridgeAgentState_WaitingTool) _then) = _$BridgeAgentState_WaitingToolCopyWithImpl;
@useResult
$Res call({
 BridgeWaitingToolAgent field0
});




}
/// @nodoc
class _$BridgeAgentState_WaitingToolCopyWithImpl<$Res>
    implements $BridgeAgentState_WaitingToolCopyWith<$Res> {
  _$BridgeAgentState_WaitingToolCopyWithImpl(this._self, this._then);

  final BridgeAgentState_WaitingTool _self;
  final $Res Function(BridgeAgentState_WaitingTool) _then;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentState_WaitingTool(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeWaitingToolAgent,
  ));
}


}

/// @nodoc


class BridgeAgentState_WaitingInteraction extends BridgeAgentState {
  const BridgeAgentState_WaitingInteraction(this.field0): super._();


@override final  BridgeWaitingInteractionAgent field0;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentState_WaitingInteractionCopyWith<BridgeAgentState_WaitingInteraction> get copyWith => _$BridgeAgentState_WaitingInteractionCopyWithImpl<BridgeAgentState_WaitingInteraction>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentState_WaitingInteraction&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeAgentState.waitingInteraction(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentState_WaitingInteractionCopyWith<$Res> implements $BridgeAgentStateCopyWith<$Res> {
  factory $BridgeAgentState_WaitingInteractionCopyWith(BridgeAgentState_WaitingInteraction value, $Res Function(BridgeAgentState_WaitingInteraction) _then) = _$BridgeAgentState_WaitingInteractionCopyWithImpl;
@useResult
$Res call({
 BridgeWaitingInteractionAgent field0
});




}
/// @nodoc
class _$BridgeAgentState_WaitingInteractionCopyWithImpl<$Res>
    implements $BridgeAgentState_WaitingInteractionCopyWith<$Res> {
  _$BridgeAgentState_WaitingInteractionCopyWithImpl(this._self, this._then);

  final BridgeAgentState_WaitingInteraction _self;
  final $Res Function(BridgeAgentState_WaitingInteraction) _then;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentState_WaitingInteraction(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeWaitingInteractionAgent,
  ));
}


}

/// @nodoc


class BridgeAgentState_Cancelling extends BridgeAgentState {
  const BridgeAgentState_Cancelling(this.field0): super._();


@override final  BridgeCancellingAgent field0;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentState_CancellingCopyWith<BridgeAgentState_Cancelling> get copyWith => _$BridgeAgentState_CancellingCopyWithImpl<BridgeAgentState_Cancelling>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentState_Cancelling&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeAgentState.cancelling(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentState_CancellingCopyWith<$Res> implements $BridgeAgentStateCopyWith<$Res> {
  factory $BridgeAgentState_CancellingCopyWith(BridgeAgentState_Cancelling value, $Res Function(BridgeAgentState_Cancelling) _then) = _$BridgeAgentState_CancellingCopyWithImpl;
@useResult
$Res call({
 BridgeCancellingAgent field0
});




}
/// @nodoc
class _$BridgeAgentState_CancellingCopyWithImpl<$Res>
    implements $BridgeAgentState_CancellingCopyWith<$Res> {
  _$BridgeAgentState_CancellingCopyWithImpl(this._self, this._then);

  final BridgeAgentState_Cancelling _self;
  final $Res Function(BridgeAgentState_Cancelling) _then;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentState_Cancelling(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeCancellingAgent,
  ));
}


}

/// @nodoc


class BridgeAgentState_Closing extends BridgeAgentState {
  const BridgeAgentState_Closing(this.field0): super._();


@override final  BridgeClosingAgent field0;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentState_ClosingCopyWith<BridgeAgentState_Closing> get copyWith => _$BridgeAgentState_ClosingCopyWithImpl<BridgeAgentState_Closing>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentState_Closing&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeAgentState.closing(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentState_ClosingCopyWith<$Res> implements $BridgeAgentStateCopyWith<$Res> {
  factory $BridgeAgentState_ClosingCopyWith(BridgeAgentState_Closing value, $Res Function(BridgeAgentState_Closing) _then) = _$BridgeAgentState_ClosingCopyWithImpl;
@useResult
$Res call({
 BridgeClosingAgent field0
});




}
/// @nodoc
class _$BridgeAgentState_ClosingCopyWithImpl<$Res>
    implements $BridgeAgentState_ClosingCopyWith<$Res> {
  _$BridgeAgentState_ClosingCopyWithImpl(this._self, this._then);

  final BridgeAgentState_Closing _self;
  final $Res Function(BridgeAgentState_Closing) _then;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentState_Closing(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeClosingAgent,
  ));
}


}

/// @nodoc


class BridgeAgentState_Closed extends BridgeAgentState {
  const BridgeAgentState_Closed(this.field0): super._();


@override final  BridgeClosedAgent field0;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentState_ClosedCopyWith<BridgeAgentState_Closed> get copyWith => _$BridgeAgentState_ClosedCopyWithImpl<BridgeAgentState_Closed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentState_Closed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeAgentState.closed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentState_ClosedCopyWith<$Res> implements $BridgeAgentStateCopyWith<$Res> {
  factory $BridgeAgentState_ClosedCopyWith(BridgeAgentState_Closed value, $Res Function(BridgeAgentState_Closed) _then) = _$BridgeAgentState_ClosedCopyWithImpl;
@useResult
$Res call({
 BridgeClosedAgent field0
});




}
/// @nodoc
class _$BridgeAgentState_ClosedCopyWithImpl<$Res>
    implements $BridgeAgentState_ClosedCopyWith<$Res> {
  _$BridgeAgentState_ClosedCopyWithImpl(this._self, this._then);

  final BridgeAgentState_Closed _self;
  final $Res Function(BridgeAgentState_Closed) _then;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentState_Closed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeClosedAgent,
  ));
}


}

/// @nodoc


class BridgeAgentState_Faulted extends BridgeAgentState {
  const BridgeAgentState_Faulted(this.field0): super._();


@override final  BridgeFaultedAgent field0;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeAgentState_FaultedCopyWith<BridgeAgentState_Faulted> get copyWith => _$BridgeAgentState_FaultedCopyWithImpl<BridgeAgentState_Faulted>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeAgentState_Faulted&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeAgentState.faulted(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeAgentState_FaultedCopyWith<$Res> implements $BridgeAgentStateCopyWith<$Res> {
  factory $BridgeAgentState_FaultedCopyWith(BridgeAgentState_Faulted value, $Res Function(BridgeAgentState_Faulted) _then) = _$BridgeAgentState_FaultedCopyWithImpl;
@useResult
$Res call({
 BridgeFaultedAgent field0
});




}
/// @nodoc
class _$BridgeAgentState_FaultedCopyWithImpl<$Res>
    implements $BridgeAgentState_FaultedCopyWith<$Res> {
  _$BridgeAgentState_FaultedCopyWithImpl(this._self, this._then);

  final BridgeAgentState_Faulted _self;
  final $Res Function(BridgeAgentState_Faulted) _then;

/// Create a copy of BridgeAgentState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeAgentState_Faulted(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeFaultedAgent,
  ));
}


}

/// @nodoc
mixin _$BridgeExecutorContinuationState {

 BigInt get revision; int get sliceCount;
/// Create a copy of BridgeExecutorContinuationState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeExecutorContinuationStateCopyWith<BridgeExecutorContinuationState> get copyWith => _$BridgeExecutorContinuationStateCopyWithImpl<BridgeExecutorContinuationState>(this as BridgeExecutorContinuationState, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeExecutorContinuationState&&(identical(other.revision, revision) || other.revision == revision)&&(identical(other.sliceCount, sliceCount) || other.sliceCount == sliceCount));
}


@override
int get hashCode => Object.hash(runtimeType,revision,sliceCount);

@override
String toString() {
  return 'BridgeExecutorContinuationState(revision: $revision, sliceCount: $sliceCount)';
}


}

/// @nodoc
abstract mixin class $BridgeExecutorContinuationStateCopyWith<$Res>  {
  factory $BridgeExecutorContinuationStateCopyWith(BridgeExecutorContinuationState value, $Res Function(BridgeExecutorContinuationState) _then) = _$BridgeExecutorContinuationStateCopyWithImpl;
@useResult
$Res call({
 BigInt revision, int sliceCount
});




}
/// @nodoc
class _$BridgeExecutorContinuationStateCopyWithImpl<$Res>
    implements $BridgeExecutorContinuationStateCopyWith<$Res> {
  _$BridgeExecutorContinuationStateCopyWithImpl(this._self, this._then);

  final BridgeExecutorContinuationState _self;
  final $Res Function(BridgeExecutorContinuationState) _then;

/// Create a copy of BridgeExecutorContinuationState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? revision = null,Object? sliceCount = null,}) {
  return _then(_self.copyWith(
revision: null == revision ? _self.revision : revision // ignore: cast_nullable_to_non_nullable
as BigInt,sliceCount: null == sliceCount ? _self.sliceCount : sliceCount // ignore: cast_nullable_to_non_nullable
as int,
  ));
}

}


/// Adds pattern-matching-related methods to [BridgeExecutorContinuationState].
extension BridgeExecutorContinuationStatePatterns on BridgeExecutorContinuationState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeExecutorContinuationState_Idle value)?  idle,TResult Function( BridgeExecutorContinuationState_Compacting value)?  compacting,TResult Function( BridgeExecutorContinuationState_PendingStart value)?  pendingStart,TResult Function( BridgeExecutorContinuationState_PlannerWakePending value)?  plannerWakePending,TResult Function( BridgeExecutorContinuationState_NeedsAttention value)?  needsAttention,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeExecutorContinuationState_Idle() when idle != null:
return idle(_that);case BridgeExecutorContinuationState_Compacting() when compacting != null:
return compacting(_that);case BridgeExecutorContinuationState_PendingStart() when pendingStart != null:
return pendingStart(_that);case BridgeExecutorContinuationState_PlannerWakePending() when plannerWakePending != null:
return plannerWakePending(_that);case BridgeExecutorContinuationState_NeedsAttention() when needsAttention != null:
return needsAttention(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeExecutorContinuationState_Idle value)  idle,required TResult Function( BridgeExecutorContinuationState_Compacting value)  compacting,required TResult Function( BridgeExecutorContinuationState_PendingStart value)  pendingStart,required TResult Function( BridgeExecutorContinuationState_PlannerWakePending value)  plannerWakePending,required TResult Function( BridgeExecutorContinuationState_NeedsAttention value)  needsAttention,}){
final _that = this;
switch (_that) {
case BridgeExecutorContinuationState_Idle():
return idle(_that);case BridgeExecutorContinuationState_Compacting():
return compacting(_that);case BridgeExecutorContinuationState_PendingStart():
return pendingStart(_that);case BridgeExecutorContinuationState_PlannerWakePending():
return plannerWakePending(_that);case BridgeExecutorContinuationState_NeedsAttention():
return needsAttention(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeExecutorContinuationState_Idle value)?  idle,TResult? Function( BridgeExecutorContinuationState_Compacting value)?  compacting,TResult? Function( BridgeExecutorContinuationState_PendingStart value)?  pendingStart,TResult? Function( BridgeExecutorContinuationState_PlannerWakePending value)?  plannerWakePending,TResult? Function( BridgeExecutorContinuationState_NeedsAttention value)?  needsAttention,}){
final _that = this;
switch (_that) {
case BridgeExecutorContinuationState_Idle() when idle != null:
return idle(_that);case BridgeExecutorContinuationState_Compacting() when compacting != null:
return compacting(_that);case BridgeExecutorContinuationState_PendingStart() when pendingStart != null:
return pendingStart(_that);case BridgeExecutorContinuationState_PlannerWakePending() when plannerWakePending != null:
return plannerWakePending(_that);case BridgeExecutorContinuationState_NeedsAttention() when needsAttention != null:
return needsAttention(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BigInt revision,  int sliceCount)?  idle,TResult Function( BigInt revision,  String sourceTurnId,  int sliceCount)?  compacting,TResult Function( BigInt revision,  String sourceTurnId,  int sliceCount,  BridgeBudgetLimitDto limit)?  pendingStart,TResult Function( BigInt revision,  String sourceTurnId,  int sliceCount)?  plannerWakePending,TResult Function( BigInt revision,  String sourceTurnId,  int sliceCount,  String detail)?  needsAttention,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeExecutorContinuationState_Idle() when idle != null:
return idle(_that.revision,_that.sliceCount);case BridgeExecutorContinuationState_Compacting() when compacting != null:
return compacting(_that.revision,_that.sourceTurnId,_that.sliceCount);case BridgeExecutorContinuationState_PendingStart() when pendingStart != null:
return pendingStart(_that.revision,_that.sourceTurnId,_that.sliceCount,_that.limit);case BridgeExecutorContinuationState_PlannerWakePending() when plannerWakePending != null:
return plannerWakePending(_that.revision,_that.sourceTurnId,_that.sliceCount);case BridgeExecutorContinuationState_NeedsAttention() when needsAttention != null:
return needsAttention(_that.revision,_that.sourceTurnId,_that.sliceCount,_that.detail);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BigInt revision,  int sliceCount)  idle,required TResult Function( BigInt revision,  String sourceTurnId,  int sliceCount)  compacting,required TResult Function( BigInt revision,  String sourceTurnId,  int sliceCount,  BridgeBudgetLimitDto limit)  pendingStart,required TResult Function( BigInt revision,  String sourceTurnId,  int sliceCount)  plannerWakePending,required TResult Function( BigInt revision,  String sourceTurnId,  int sliceCount,  String detail)  needsAttention,}) {final _that = this;
switch (_that) {
case BridgeExecutorContinuationState_Idle():
return idle(_that.revision,_that.sliceCount);case BridgeExecutorContinuationState_Compacting():
return compacting(_that.revision,_that.sourceTurnId,_that.sliceCount);case BridgeExecutorContinuationState_PendingStart():
return pendingStart(_that.revision,_that.sourceTurnId,_that.sliceCount,_that.limit);case BridgeExecutorContinuationState_PlannerWakePending():
return plannerWakePending(_that.revision,_that.sourceTurnId,_that.sliceCount);case BridgeExecutorContinuationState_NeedsAttention():
return needsAttention(_that.revision,_that.sourceTurnId,_that.sliceCount,_that.detail);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BigInt revision,  int sliceCount)?  idle,TResult? Function( BigInt revision,  String sourceTurnId,  int sliceCount)?  compacting,TResult? Function( BigInt revision,  String sourceTurnId,  int sliceCount,  BridgeBudgetLimitDto limit)?  pendingStart,TResult? Function( BigInt revision,  String sourceTurnId,  int sliceCount)?  plannerWakePending,TResult? Function( BigInt revision,  String sourceTurnId,  int sliceCount,  String detail)?  needsAttention,}) {final _that = this;
switch (_that) {
case BridgeExecutorContinuationState_Idle() when idle != null:
return idle(_that.revision,_that.sliceCount);case BridgeExecutorContinuationState_Compacting() when compacting != null:
return compacting(_that.revision,_that.sourceTurnId,_that.sliceCount);case BridgeExecutorContinuationState_PendingStart() when pendingStart != null:
return pendingStart(_that.revision,_that.sourceTurnId,_that.sliceCount,_that.limit);case BridgeExecutorContinuationState_PlannerWakePending() when plannerWakePending != null:
return plannerWakePending(_that.revision,_that.sourceTurnId,_that.sliceCount);case BridgeExecutorContinuationState_NeedsAttention() when needsAttention != null:
return needsAttention(_that.revision,_that.sourceTurnId,_that.sliceCount,_that.detail);case _:
  return null;

}
}

}

/// @nodoc


class BridgeExecutorContinuationState_Idle extends BridgeExecutorContinuationState {
  const BridgeExecutorContinuationState_Idle({required this.revision, required this.sliceCount}): super._();


@override final  BigInt revision;
@override final  int sliceCount;

/// Create a copy of BridgeExecutorContinuationState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeExecutorContinuationState_IdleCopyWith<BridgeExecutorContinuationState_Idle> get copyWith => _$BridgeExecutorContinuationState_IdleCopyWithImpl<BridgeExecutorContinuationState_Idle>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeExecutorContinuationState_Idle&&(identical(other.revision, revision) || other.revision == revision)&&(identical(other.sliceCount, sliceCount) || other.sliceCount == sliceCount));
}


@override
int get hashCode => Object.hash(runtimeType,revision,sliceCount);

@override
String toString() {
  return 'BridgeExecutorContinuationState.idle(revision: $revision, sliceCount: $sliceCount)';
}


}

/// @nodoc
abstract mixin class $BridgeExecutorContinuationState_IdleCopyWith<$Res> implements $BridgeExecutorContinuationStateCopyWith<$Res> {
  factory $BridgeExecutorContinuationState_IdleCopyWith(BridgeExecutorContinuationState_Idle value, $Res Function(BridgeExecutorContinuationState_Idle) _then) = _$BridgeExecutorContinuationState_IdleCopyWithImpl;
@override @useResult
$Res call({
 BigInt revision, int sliceCount
});




}
/// @nodoc
class _$BridgeExecutorContinuationState_IdleCopyWithImpl<$Res>
    implements $BridgeExecutorContinuationState_IdleCopyWith<$Res> {
  _$BridgeExecutorContinuationState_IdleCopyWithImpl(this._self, this._then);

  final BridgeExecutorContinuationState_Idle _self;
  final $Res Function(BridgeExecutorContinuationState_Idle) _then;

/// Create a copy of BridgeExecutorContinuationState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? revision = null,Object? sliceCount = null,}) {
  return _then(BridgeExecutorContinuationState_Idle(
revision: null == revision ? _self.revision : revision // ignore: cast_nullable_to_non_nullable
as BigInt,sliceCount: null == sliceCount ? _self.sliceCount : sliceCount // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc


class BridgeExecutorContinuationState_Compacting extends BridgeExecutorContinuationState {
  const BridgeExecutorContinuationState_Compacting({required this.revision, required this.sourceTurnId, required this.sliceCount}): super._();


@override final  BigInt revision;
 final  String sourceTurnId;
@override final  int sliceCount;

/// Create a copy of BridgeExecutorContinuationState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeExecutorContinuationState_CompactingCopyWith<BridgeExecutorContinuationState_Compacting> get copyWith => _$BridgeExecutorContinuationState_CompactingCopyWithImpl<BridgeExecutorContinuationState_Compacting>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeExecutorContinuationState_Compacting&&(identical(other.revision, revision) || other.revision == revision)&&(identical(other.sourceTurnId, sourceTurnId) || other.sourceTurnId == sourceTurnId)&&(identical(other.sliceCount, sliceCount) || other.sliceCount == sliceCount));
}


@override
int get hashCode => Object.hash(runtimeType,revision,sourceTurnId,sliceCount);

@override
String toString() {
  return 'BridgeExecutorContinuationState.compacting(revision: $revision, sourceTurnId: $sourceTurnId, sliceCount: $sliceCount)';
}


}

/// @nodoc
abstract mixin class $BridgeExecutorContinuationState_CompactingCopyWith<$Res> implements $BridgeExecutorContinuationStateCopyWith<$Res> {
  factory $BridgeExecutorContinuationState_CompactingCopyWith(BridgeExecutorContinuationState_Compacting value, $Res Function(BridgeExecutorContinuationState_Compacting) _then) = _$BridgeExecutorContinuationState_CompactingCopyWithImpl;
@override @useResult
$Res call({
 BigInt revision, String sourceTurnId, int sliceCount
});




}
/// @nodoc
class _$BridgeExecutorContinuationState_CompactingCopyWithImpl<$Res>
    implements $BridgeExecutorContinuationState_CompactingCopyWith<$Res> {
  _$BridgeExecutorContinuationState_CompactingCopyWithImpl(this._self, this._then);

  final BridgeExecutorContinuationState_Compacting _self;
  final $Res Function(BridgeExecutorContinuationState_Compacting) _then;

/// Create a copy of BridgeExecutorContinuationState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? revision = null,Object? sourceTurnId = null,Object? sliceCount = null,}) {
  return _then(BridgeExecutorContinuationState_Compacting(
revision: null == revision ? _self.revision : revision // ignore: cast_nullable_to_non_nullable
as BigInt,sourceTurnId: null == sourceTurnId ? _self.sourceTurnId : sourceTurnId // ignore: cast_nullable_to_non_nullable
as String,sliceCount: null == sliceCount ? _self.sliceCount : sliceCount // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc


class BridgeExecutorContinuationState_PendingStart extends BridgeExecutorContinuationState {
  const BridgeExecutorContinuationState_PendingStart({required this.revision, required this.sourceTurnId, required this.sliceCount, required this.limit}): super._();


@override final  BigInt revision;
 final  String sourceTurnId;
@override final  int sliceCount;
 final  BridgeBudgetLimitDto limit;

/// Create a copy of BridgeExecutorContinuationState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeExecutorContinuationState_PendingStartCopyWith<BridgeExecutorContinuationState_PendingStart> get copyWith => _$BridgeExecutorContinuationState_PendingStartCopyWithImpl<BridgeExecutorContinuationState_PendingStart>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeExecutorContinuationState_PendingStart&&(identical(other.revision, revision) || other.revision == revision)&&(identical(other.sourceTurnId, sourceTurnId) || other.sourceTurnId == sourceTurnId)&&(identical(other.sliceCount, sliceCount) || other.sliceCount == sliceCount)&&(identical(other.limit, limit) || other.limit == limit));
}


@override
int get hashCode => Object.hash(runtimeType,revision,sourceTurnId,sliceCount,limit);

@override
String toString() {
  return 'BridgeExecutorContinuationState.pendingStart(revision: $revision, sourceTurnId: $sourceTurnId, sliceCount: $sliceCount, limit: $limit)';
}


}

/// @nodoc
abstract mixin class $BridgeExecutorContinuationState_PendingStartCopyWith<$Res> implements $BridgeExecutorContinuationStateCopyWith<$Res> {
  factory $BridgeExecutorContinuationState_PendingStartCopyWith(BridgeExecutorContinuationState_PendingStart value, $Res Function(BridgeExecutorContinuationState_PendingStart) _then) = _$BridgeExecutorContinuationState_PendingStartCopyWithImpl;
@override @useResult
$Res call({
 BigInt revision, String sourceTurnId, int sliceCount, BridgeBudgetLimitDto limit
});




}
/// @nodoc
class _$BridgeExecutorContinuationState_PendingStartCopyWithImpl<$Res>
    implements $BridgeExecutorContinuationState_PendingStartCopyWith<$Res> {
  _$BridgeExecutorContinuationState_PendingStartCopyWithImpl(this._self, this._then);

  final BridgeExecutorContinuationState_PendingStart _self;
  final $Res Function(BridgeExecutorContinuationState_PendingStart) _then;

/// Create a copy of BridgeExecutorContinuationState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? revision = null,Object? sourceTurnId = null,Object? sliceCount = null,Object? limit = null,}) {
  return _then(BridgeExecutorContinuationState_PendingStart(
revision: null == revision ? _self.revision : revision // ignore: cast_nullable_to_non_nullable
as BigInt,sourceTurnId: null == sourceTurnId ? _self.sourceTurnId : sourceTurnId // ignore: cast_nullable_to_non_nullable
as String,sliceCount: null == sliceCount ? _self.sliceCount : sliceCount // ignore: cast_nullable_to_non_nullable
as int,limit: null == limit ? _self.limit : limit // ignore: cast_nullable_to_non_nullable
as BridgeBudgetLimitDto,
  ));
}


}

/// @nodoc


class BridgeExecutorContinuationState_PlannerWakePending extends BridgeExecutorContinuationState {
  const BridgeExecutorContinuationState_PlannerWakePending({required this.revision, required this.sourceTurnId, required this.sliceCount}): super._();


@override final  BigInt revision;
 final  String sourceTurnId;
@override final  int sliceCount;

/// Create a copy of BridgeExecutorContinuationState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeExecutorContinuationState_PlannerWakePendingCopyWith<BridgeExecutorContinuationState_PlannerWakePending> get copyWith => _$BridgeExecutorContinuationState_PlannerWakePendingCopyWithImpl<BridgeExecutorContinuationState_PlannerWakePending>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeExecutorContinuationState_PlannerWakePending&&(identical(other.revision, revision) || other.revision == revision)&&(identical(other.sourceTurnId, sourceTurnId) || other.sourceTurnId == sourceTurnId)&&(identical(other.sliceCount, sliceCount) || other.sliceCount == sliceCount));
}


@override
int get hashCode => Object.hash(runtimeType,revision,sourceTurnId,sliceCount);

@override
String toString() {
  return 'BridgeExecutorContinuationState.plannerWakePending(revision: $revision, sourceTurnId: $sourceTurnId, sliceCount: $sliceCount)';
}


}

/// @nodoc
abstract mixin class $BridgeExecutorContinuationState_PlannerWakePendingCopyWith<$Res> implements $BridgeExecutorContinuationStateCopyWith<$Res> {
  factory $BridgeExecutorContinuationState_PlannerWakePendingCopyWith(BridgeExecutorContinuationState_PlannerWakePending value, $Res Function(BridgeExecutorContinuationState_PlannerWakePending) _then) = _$BridgeExecutorContinuationState_PlannerWakePendingCopyWithImpl;
@override @useResult
$Res call({
 BigInt revision, String sourceTurnId, int sliceCount
});




}
/// @nodoc
class _$BridgeExecutorContinuationState_PlannerWakePendingCopyWithImpl<$Res>
    implements $BridgeExecutorContinuationState_PlannerWakePendingCopyWith<$Res> {
  _$BridgeExecutorContinuationState_PlannerWakePendingCopyWithImpl(this._self, this._then);

  final BridgeExecutorContinuationState_PlannerWakePending _self;
  final $Res Function(BridgeExecutorContinuationState_PlannerWakePending) _then;

/// Create a copy of BridgeExecutorContinuationState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? revision = null,Object? sourceTurnId = null,Object? sliceCount = null,}) {
  return _then(BridgeExecutorContinuationState_PlannerWakePending(
revision: null == revision ? _self.revision : revision // ignore: cast_nullable_to_non_nullable
as BigInt,sourceTurnId: null == sourceTurnId ? _self.sourceTurnId : sourceTurnId // ignore: cast_nullable_to_non_nullable
as String,sliceCount: null == sliceCount ? _self.sliceCount : sliceCount // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc


class BridgeExecutorContinuationState_NeedsAttention extends BridgeExecutorContinuationState {
  const BridgeExecutorContinuationState_NeedsAttention({required this.revision, required this.sourceTurnId, required this.sliceCount, required this.detail}): super._();


@override final  BigInt revision;
 final  String sourceTurnId;
@override final  int sliceCount;
 final  String detail;

/// Create a copy of BridgeExecutorContinuationState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeExecutorContinuationState_NeedsAttentionCopyWith<BridgeExecutorContinuationState_NeedsAttention> get copyWith => _$BridgeExecutorContinuationState_NeedsAttentionCopyWithImpl<BridgeExecutorContinuationState_NeedsAttention>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeExecutorContinuationState_NeedsAttention&&(identical(other.revision, revision) || other.revision == revision)&&(identical(other.sourceTurnId, sourceTurnId) || other.sourceTurnId == sourceTurnId)&&(identical(other.sliceCount, sliceCount) || other.sliceCount == sliceCount)&&(identical(other.detail, detail) || other.detail == detail));
}


@override
int get hashCode => Object.hash(runtimeType,revision,sourceTurnId,sliceCount,detail);

@override
String toString() {
  return 'BridgeExecutorContinuationState.needsAttention(revision: $revision, sourceTurnId: $sourceTurnId, sliceCount: $sliceCount, detail: $detail)';
}


}

/// @nodoc
abstract mixin class $BridgeExecutorContinuationState_NeedsAttentionCopyWith<$Res> implements $BridgeExecutorContinuationStateCopyWith<$Res> {
  factory $BridgeExecutorContinuationState_NeedsAttentionCopyWith(BridgeExecutorContinuationState_NeedsAttention value, $Res Function(BridgeExecutorContinuationState_NeedsAttention) _then) = _$BridgeExecutorContinuationState_NeedsAttentionCopyWithImpl;
@override @useResult
$Res call({
 BigInt revision, String sourceTurnId, int sliceCount, String detail
});




}
/// @nodoc
class _$BridgeExecutorContinuationState_NeedsAttentionCopyWithImpl<$Res>
    implements $BridgeExecutorContinuationState_NeedsAttentionCopyWith<$Res> {
  _$BridgeExecutorContinuationState_NeedsAttentionCopyWithImpl(this._self, this._then);

  final BridgeExecutorContinuationState_NeedsAttention _self;
  final $Res Function(BridgeExecutorContinuationState_NeedsAttention) _then;

/// Create a copy of BridgeExecutorContinuationState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? revision = null,Object? sourceTurnId = null,Object? sliceCount = null,Object? detail = null,}) {
  return _then(BridgeExecutorContinuationState_NeedsAttention(
revision: null == revision ? _self.revision : revision // ignore: cast_nullable_to_non_nullable
as BigInt,sourceTurnId: null == sourceTurnId ? _self.sourceTurnId : sourceTurnId // ignore: cast_nullable_to_non_nullable
as String,sliceCount: null == sliceCount ? _self.sliceCount : sliceCount // ignore: cast_nullable_to_non_nullable
as int,detail: null == detail ? _self.detail : detail // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeExecutorTerminalOutcome {

 String get sourceTurnId; String get detail;
/// Create a copy of BridgeExecutorTerminalOutcome
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeExecutorTerminalOutcomeCopyWith<BridgeExecutorTerminalOutcome> get copyWith => _$BridgeExecutorTerminalOutcomeCopyWithImpl<BridgeExecutorTerminalOutcome>(this as BridgeExecutorTerminalOutcome, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeExecutorTerminalOutcome&&(identical(other.sourceTurnId, sourceTurnId) || other.sourceTurnId == sourceTurnId)&&(identical(other.detail, detail) || other.detail == detail));
}


@override
int get hashCode => Object.hash(runtimeType,sourceTurnId,detail);

@override
String toString() {
  return 'BridgeExecutorTerminalOutcome(sourceTurnId: $sourceTurnId, detail: $detail)';
}


}

/// @nodoc
abstract mixin class $BridgeExecutorTerminalOutcomeCopyWith<$Res>  {
  factory $BridgeExecutorTerminalOutcomeCopyWith(BridgeExecutorTerminalOutcome value, $Res Function(BridgeExecutorTerminalOutcome) _then) = _$BridgeExecutorTerminalOutcomeCopyWithImpl;
@useResult
$Res call({
 String sourceTurnId, String detail
});




}
/// @nodoc
class _$BridgeExecutorTerminalOutcomeCopyWithImpl<$Res>
    implements $BridgeExecutorTerminalOutcomeCopyWith<$Res> {
  _$BridgeExecutorTerminalOutcomeCopyWithImpl(this._self, this._then);

  final BridgeExecutorTerminalOutcome _self;
  final $Res Function(BridgeExecutorTerminalOutcome) _then;

/// Create a copy of BridgeExecutorTerminalOutcome
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? sourceTurnId = null,Object? detail = null,}) {
  return _then(_self.copyWith(
sourceTurnId: null == sourceTurnId ? _self.sourceTurnId : sourceTurnId // ignore: cast_nullable_to_non_nullable
as String,detail: null == detail ? _self.detail : detail // ignore: cast_nullable_to_non_nullable
as String,
  ));
}

}


/// Adds pattern-matching-related methods to [BridgeExecutorTerminalOutcome].
extension BridgeExecutorTerminalOutcomePatterns on BridgeExecutorTerminalOutcome {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeExecutorTerminalOutcome_Completed value)?  completed,TResult Function( BridgeExecutorTerminalOutcome_Failed value)?  failed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeExecutorTerminalOutcome_Completed() when completed != null:
return completed(_that);case BridgeExecutorTerminalOutcome_Failed() when failed != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeExecutorTerminalOutcome_Completed value)  completed,required TResult Function( BridgeExecutorTerminalOutcome_Failed value)  failed,}){
final _that = this;
switch (_that) {
case BridgeExecutorTerminalOutcome_Completed():
return completed(_that);case BridgeExecutorTerminalOutcome_Failed():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeExecutorTerminalOutcome_Completed value)?  completed,TResult? Function( BridgeExecutorTerminalOutcome_Failed value)?  failed,}){
final _that = this;
switch (_that) {
case BridgeExecutorTerminalOutcome_Completed() when completed != null:
return completed(_that);case BridgeExecutorTerminalOutcome_Failed() when failed != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String sourceTurnId,  String detail)?  completed,TResult Function( String sourceTurnId,  String detail)?  failed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeExecutorTerminalOutcome_Completed() when completed != null:
return completed(_that.sourceTurnId,_that.detail);case BridgeExecutorTerminalOutcome_Failed() when failed != null:
return failed(_that.sourceTurnId,_that.detail);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String sourceTurnId,  String detail)  completed,required TResult Function( String sourceTurnId,  String detail)  failed,}) {final _that = this;
switch (_that) {
case BridgeExecutorTerminalOutcome_Completed():
return completed(_that.sourceTurnId,_that.detail);case BridgeExecutorTerminalOutcome_Failed():
return failed(_that.sourceTurnId,_that.detail);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String sourceTurnId,  String detail)?  completed,TResult? Function( String sourceTurnId,  String detail)?  failed,}) {final _that = this;
switch (_that) {
case BridgeExecutorTerminalOutcome_Completed() when completed != null:
return completed(_that.sourceTurnId,_that.detail);case BridgeExecutorTerminalOutcome_Failed() when failed != null:
return failed(_that.sourceTurnId,_that.detail);case _:
  return null;

}
}

}

/// @nodoc


class BridgeExecutorTerminalOutcome_Completed extends BridgeExecutorTerminalOutcome {
  const BridgeExecutorTerminalOutcome_Completed({required this.sourceTurnId, required this.detail}): super._();


@override final  String sourceTurnId;
@override final  String detail;

/// Create a copy of BridgeExecutorTerminalOutcome
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeExecutorTerminalOutcome_CompletedCopyWith<BridgeExecutorTerminalOutcome_Completed> get copyWith => _$BridgeExecutorTerminalOutcome_CompletedCopyWithImpl<BridgeExecutorTerminalOutcome_Completed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeExecutorTerminalOutcome_Completed&&(identical(other.sourceTurnId, sourceTurnId) || other.sourceTurnId == sourceTurnId)&&(identical(other.detail, detail) || other.detail == detail));
}


@override
int get hashCode => Object.hash(runtimeType,sourceTurnId,detail);

@override
String toString() {
  return 'BridgeExecutorTerminalOutcome.completed(sourceTurnId: $sourceTurnId, detail: $detail)';
}


}

/// @nodoc
abstract mixin class $BridgeExecutorTerminalOutcome_CompletedCopyWith<$Res> implements $BridgeExecutorTerminalOutcomeCopyWith<$Res> {
  factory $BridgeExecutorTerminalOutcome_CompletedCopyWith(BridgeExecutorTerminalOutcome_Completed value, $Res Function(BridgeExecutorTerminalOutcome_Completed) _then) = _$BridgeExecutorTerminalOutcome_CompletedCopyWithImpl;
@override @useResult
$Res call({
 String sourceTurnId, String detail
});




}
/// @nodoc
class _$BridgeExecutorTerminalOutcome_CompletedCopyWithImpl<$Res>
    implements $BridgeExecutorTerminalOutcome_CompletedCopyWith<$Res> {
  _$BridgeExecutorTerminalOutcome_CompletedCopyWithImpl(this._self, this._then);

  final BridgeExecutorTerminalOutcome_Completed _self;
  final $Res Function(BridgeExecutorTerminalOutcome_Completed) _then;

/// Create a copy of BridgeExecutorTerminalOutcome
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? sourceTurnId = null,Object? detail = null,}) {
  return _then(BridgeExecutorTerminalOutcome_Completed(
sourceTurnId: null == sourceTurnId ? _self.sourceTurnId : sourceTurnId // ignore: cast_nullable_to_non_nullable
as String,detail: null == detail ? _self.detail : detail // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeExecutorTerminalOutcome_Failed extends BridgeExecutorTerminalOutcome {
  const BridgeExecutorTerminalOutcome_Failed({required this.sourceTurnId, required this.detail}): super._();


@override final  String sourceTurnId;
@override final  String detail;

/// Create a copy of BridgeExecutorTerminalOutcome
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeExecutorTerminalOutcome_FailedCopyWith<BridgeExecutorTerminalOutcome_Failed> get copyWith => _$BridgeExecutorTerminalOutcome_FailedCopyWithImpl<BridgeExecutorTerminalOutcome_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeExecutorTerminalOutcome_Failed&&(identical(other.sourceTurnId, sourceTurnId) || other.sourceTurnId == sourceTurnId)&&(identical(other.detail, detail) || other.detail == detail));
}


@override
int get hashCode => Object.hash(runtimeType,sourceTurnId,detail);

@override
String toString() {
  return 'BridgeExecutorTerminalOutcome.failed(sourceTurnId: $sourceTurnId, detail: $detail)';
}


}

/// @nodoc
abstract mixin class $BridgeExecutorTerminalOutcome_FailedCopyWith<$Res> implements $BridgeExecutorTerminalOutcomeCopyWith<$Res> {
  factory $BridgeExecutorTerminalOutcome_FailedCopyWith(BridgeExecutorTerminalOutcome_Failed value, $Res Function(BridgeExecutorTerminalOutcome_Failed) _then) = _$BridgeExecutorTerminalOutcome_FailedCopyWithImpl;
@override @useResult
$Res call({
 String sourceTurnId, String detail
});




}
/// @nodoc
class _$BridgeExecutorTerminalOutcome_FailedCopyWithImpl<$Res>
    implements $BridgeExecutorTerminalOutcome_FailedCopyWith<$Res> {
  _$BridgeExecutorTerminalOutcome_FailedCopyWithImpl(this._self, this._then);

  final BridgeExecutorTerminalOutcome_Failed _self;
  final $Res Function(BridgeExecutorTerminalOutcome_Failed) _then;

/// Create a copy of BridgeExecutorTerminalOutcome
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? sourceTurnId = null,Object? detail = null,}) {
  return _then(BridgeExecutorTerminalOutcome_Failed(
sourceTurnId: null == sourceTurnId ? _self.sourceTurnId : sourceTurnId // ignore: cast_nullable_to_non_nullable
as String,detail: null == detail ? _self.detail : detail // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeIntegratedReviewGateDto {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeIntegratedReviewGateDto);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeIntegratedReviewGateDto()';
}


}

/// @nodoc
class $BridgeIntegratedReviewGateDtoCopyWith<$Res>  {
$BridgeIntegratedReviewGateDtoCopyWith(BridgeIntegratedReviewGateDto _, $Res Function(BridgeIntegratedReviewGateDto) __);
}


/// Adds pattern-matching-related methods to [BridgeIntegratedReviewGateDto].
extension BridgeIntegratedReviewGateDtoPatterns on BridgeIntegratedReviewGateDto {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeIntegratedReviewGateDto_Required value)?  required_,TResult Function( BridgeIntegratedReviewGateDto_SatisfiedByReview value)?  satisfiedByReview,TResult Function( BridgeIntegratedReviewGateDto_NotRequiredNoDelivery value)?  notRequiredNoDelivery,TResult Function( BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent value)?  notRequiredSingleExecutorEquivalent,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeIntegratedReviewGateDto_Required() when required_ != null:
return required_(_that);case BridgeIntegratedReviewGateDto_SatisfiedByReview() when satisfiedByReview != null:
return satisfiedByReview(_that);case BridgeIntegratedReviewGateDto_NotRequiredNoDelivery() when notRequiredNoDelivery != null:
return notRequiredNoDelivery(_that);case BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent() when notRequiredSingleExecutorEquivalent != null:
return notRequiredSingleExecutorEquivalent(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeIntegratedReviewGateDto_Required value)  required_,required TResult Function( BridgeIntegratedReviewGateDto_SatisfiedByReview value)  satisfiedByReview,required TResult Function( BridgeIntegratedReviewGateDto_NotRequiredNoDelivery value)  notRequiredNoDelivery,required TResult Function( BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent value)  notRequiredSingleExecutorEquivalent,}){
final _that = this;
switch (_that) {
case BridgeIntegratedReviewGateDto_Required():
return required_(_that);case BridgeIntegratedReviewGateDto_SatisfiedByReview():
return satisfiedByReview(_that);case BridgeIntegratedReviewGateDto_NotRequiredNoDelivery():
return notRequiredNoDelivery(_that);case BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent():
return notRequiredSingleExecutorEquivalent(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeIntegratedReviewGateDto_Required value)?  required_,TResult? Function( BridgeIntegratedReviewGateDto_SatisfiedByReview value)?  satisfiedByReview,TResult? Function( BridgeIntegratedReviewGateDto_NotRequiredNoDelivery value)?  notRequiredNoDelivery,TResult? Function( BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent value)?  notRequiredSingleExecutorEquivalent,}){
final _that = this;
switch (_that) {
case BridgeIntegratedReviewGateDto_Required() when required_ != null:
return required_(_that);case BridgeIntegratedReviewGateDto_SatisfiedByReview() when satisfiedByReview != null:
return satisfiedByReview(_that);case BridgeIntegratedReviewGateDto_NotRequiredNoDelivery() when notRequiredNoDelivery != null:
return notRequiredNoDelivery(_that);case BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent() when notRequiredSingleExecutorEquivalent != null:
return notRequiredSingleExecutorEquivalent(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String reason)?  required_,TResult Function( String reviewRoundId,  String reviewedHead)?  satisfiedByReview,TResult Function()?  notRequiredNoDelivery,TResult Function( String workUnitId,  int completionRevision,  String mergeRecordId)?  notRequiredSingleExecutorEquivalent,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeIntegratedReviewGateDto_Required() when required_ != null:
return required_(_that.reason);case BridgeIntegratedReviewGateDto_SatisfiedByReview() when satisfiedByReview != null:
return satisfiedByReview(_that.reviewRoundId,_that.reviewedHead);case BridgeIntegratedReviewGateDto_NotRequiredNoDelivery() when notRequiredNoDelivery != null:
return notRequiredNoDelivery();case BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent() when notRequiredSingleExecutorEquivalent != null:
return notRequiredSingleExecutorEquivalent(_that.workUnitId,_that.completionRevision,_that.mergeRecordId);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String reason)  required_,required TResult Function( String reviewRoundId,  String reviewedHead)  satisfiedByReview,required TResult Function()  notRequiredNoDelivery,required TResult Function( String workUnitId,  int completionRevision,  String mergeRecordId)  notRequiredSingleExecutorEquivalent,}) {final _that = this;
switch (_that) {
case BridgeIntegratedReviewGateDto_Required():
return required_(_that.reason);case BridgeIntegratedReviewGateDto_SatisfiedByReview():
return satisfiedByReview(_that.reviewRoundId,_that.reviewedHead);case BridgeIntegratedReviewGateDto_NotRequiredNoDelivery():
return notRequiredNoDelivery();case BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent():
return notRequiredSingleExecutorEquivalent(_that.workUnitId,_that.completionRevision,_that.mergeRecordId);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String reason)?  required_,TResult? Function( String reviewRoundId,  String reviewedHead)?  satisfiedByReview,TResult? Function()?  notRequiredNoDelivery,TResult? Function( String workUnitId,  int completionRevision,  String mergeRecordId)?  notRequiredSingleExecutorEquivalent,}) {final _that = this;
switch (_that) {
case BridgeIntegratedReviewGateDto_Required() when required_ != null:
return required_(_that.reason);case BridgeIntegratedReviewGateDto_SatisfiedByReview() when satisfiedByReview != null:
return satisfiedByReview(_that.reviewRoundId,_that.reviewedHead);case BridgeIntegratedReviewGateDto_NotRequiredNoDelivery() when notRequiredNoDelivery != null:
return notRequiredNoDelivery();case BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent() when notRequiredSingleExecutorEquivalent != null:
return notRequiredSingleExecutorEquivalent(_that.workUnitId,_that.completionRevision,_that.mergeRecordId);case _:
  return null;

}
}

}

/// @nodoc


class BridgeIntegratedReviewGateDto_Required extends BridgeIntegratedReviewGateDto {
  const BridgeIntegratedReviewGateDto_Required({required this.reason}): super._();


 final  String reason;

/// Create a copy of BridgeIntegratedReviewGateDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeIntegratedReviewGateDto_RequiredCopyWith<BridgeIntegratedReviewGateDto_Required> get copyWith => _$BridgeIntegratedReviewGateDto_RequiredCopyWithImpl<BridgeIntegratedReviewGateDto_Required>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeIntegratedReviewGateDto_Required&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,reason);

@override
String toString() {
  return 'BridgeIntegratedReviewGateDto.required_(reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeIntegratedReviewGateDto_RequiredCopyWith<$Res> implements $BridgeIntegratedReviewGateDtoCopyWith<$Res> {
  factory $BridgeIntegratedReviewGateDto_RequiredCopyWith(BridgeIntegratedReviewGateDto_Required value, $Res Function(BridgeIntegratedReviewGateDto_Required) _then) = _$BridgeIntegratedReviewGateDto_RequiredCopyWithImpl;
@useResult
$Res call({
 String reason
});




}
/// @nodoc
class _$BridgeIntegratedReviewGateDto_RequiredCopyWithImpl<$Res>
    implements $BridgeIntegratedReviewGateDto_RequiredCopyWith<$Res> {
  _$BridgeIntegratedReviewGateDto_RequiredCopyWithImpl(this._self, this._then);

  final BridgeIntegratedReviewGateDto_Required _self;
  final $Res Function(BridgeIntegratedReviewGateDto_Required) _then;

/// Create a copy of BridgeIntegratedReviewGateDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reason = null,}) {
  return _then(BridgeIntegratedReviewGateDto_Required(
reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeIntegratedReviewGateDto_SatisfiedByReview extends BridgeIntegratedReviewGateDto {
  const BridgeIntegratedReviewGateDto_SatisfiedByReview({required this.reviewRoundId, required this.reviewedHead}): super._();


 final  String reviewRoundId;
 final  String reviewedHead;

/// Create a copy of BridgeIntegratedReviewGateDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeIntegratedReviewGateDto_SatisfiedByReviewCopyWith<BridgeIntegratedReviewGateDto_SatisfiedByReview> get copyWith => _$BridgeIntegratedReviewGateDto_SatisfiedByReviewCopyWithImpl<BridgeIntegratedReviewGateDto_SatisfiedByReview>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeIntegratedReviewGateDto_SatisfiedByReview&&(identical(other.reviewRoundId, reviewRoundId) || other.reviewRoundId == reviewRoundId)&&(identical(other.reviewedHead, reviewedHead) || other.reviewedHead == reviewedHead));
}


@override
int get hashCode => Object.hash(runtimeType,reviewRoundId,reviewedHead);

@override
String toString() {
  return 'BridgeIntegratedReviewGateDto.satisfiedByReview(reviewRoundId: $reviewRoundId, reviewedHead: $reviewedHead)';
}


}

/// @nodoc
abstract mixin class $BridgeIntegratedReviewGateDto_SatisfiedByReviewCopyWith<$Res> implements $BridgeIntegratedReviewGateDtoCopyWith<$Res> {
  factory $BridgeIntegratedReviewGateDto_SatisfiedByReviewCopyWith(BridgeIntegratedReviewGateDto_SatisfiedByReview value, $Res Function(BridgeIntegratedReviewGateDto_SatisfiedByReview) _then) = _$BridgeIntegratedReviewGateDto_SatisfiedByReviewCopyWithImpl;
@useResult
$Res call({
 String reviewRoundId, String reviewedHead
});




}
/// @nodoc
class _$BridgeIntegratedReviewGateDto_SatisfiedByReviewCopyWithImpl<$Res>
    implements $BridgeIntegratedReviewGateDto_SatisfiedByReviewCopyWith<$Res> {
  _$BridgeIntegratedReviewGateDto_SatisfiedByReviewCopyWithImpl(this._self, this._then);

  final BridgeIntegratedReviewGateDto_SatisfiedByReview _self;
  final $Res Function(BridgeIntegratedReviewGateDto_SatisfiedByReview) _then;

/// Create a copy of BridgeIntegratedReviewGateDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reviewRoundId = null,Object? reviewedHead = null,}) {
  return _then(BridgeIntegratedReviewGateDto_SatisfiedByReview(
reviewRoundId: null == reviewRoundId ? _self.reviewRoundId : reviewRoundId // ignore: cast_nullable_to_non_nullable
as String,reviewedHead: null == reviewedHead ? _self.reviewedHead : reviewedHead // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeIntegratedReviewGateDto_NotRequiredNoDelivery extends BridgeIntegratedReviewGateDto {
  const BridgeIntegratedReviewGateDto_NotRequiredNoDelivery(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeIntegratedReviewGateDto_NotRequiredNoDelivery);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeIntegratedReviewGateDto.notRequiredNoDelivery()';
}


}




/// @nodoc


class BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent extends BridgeIntegratedReviewGateDto {
  const BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent({required this.workUnitId, required this.completionRevision, required this.mergeRecordId}): super._();


 final  String workUnitId;
 final  int completionRevision;
 final  String mergeRecordId;

/// Create a copy of BridgeIntegratedReviewGateDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalentCopyWith<BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent> get copyWith => _$BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalentCopyWithImpl<BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent&&(identical(other.workUnitId, workUnitId) || other.workUnitId == workUnitId)&&(identical(other.completionRevision, completionRevision) || other.completionRevision == completionRevision)&&(identical(other.mergeRecordId, mergeRecordId) || other.mergeRecordId == mergeRecordId));
}


@override
int get hashCode => Object.hash(runtimeType,workUnitId,completionRevision,mergeRecordId);

@override
String toString() {
  return 'BridgeIntegratedReviewGateDto.notRequiredSingleExecutorEquivalent(workUnitId: $workUnitId, completionRevision: $completionRevision, mergeRecordId: $mergeRecordId)';
}


}

/// @nodoc
abstract mixin class $BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalentCopyWith<$Res> implements $BridgeIntegratedReviewGateDtoCopyWith<$Res> {
  factory $BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalentCopyWith(BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent value, $Res Function(BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent) _then) = _$BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalentCopyWithImpl;
@useResult
$Res call({
 String workUnitId, int completionRevision, String mergeRecordId
});




}
/// @nodoc
class _$BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalentCopyWithImpl<$Res>
    implements $BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalentCopyWith<$Res> {
  _$BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalentCopyWithImpl(this._self, this._then);

  final BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent _self;
  final $Res Function(BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent) _then;

/// Create a copy of BridgeIntegratedReviewGateDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? workUnitId = null,Object? completionRevision = null,Object? mergeRecordId = null,}) {
  return _then(BridgeIntegratedReviewGateDto_NotRequiredSingleExecutorEquivalent(
workUnitId: null == workUnitId ? _self.workUnitId : workUnitId // ignore: cast_nullable_to_non_nullable
as String,completionRevision: null == completionRevision ? _self.completionRevision : completionRevision // ignore: cast_nullable_to_non_nullable
as int,mergeRecordId: null == mergeRecordId ? _self.mergeRecordId : mergeRecordId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeLspActivity {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspActivity);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeLspActivity()';
}


}

/// @nodoc
class $BridgeLspActivityCopyWith<$Res>  {
$BridgeLspActivityCopyWith(BridgeLspActivity _, $Res Function(BridgeLspActivity) __);
}


/// Adds pattern-matching-related methods to [BridgeLspActivity].
extension BridgeLspActivityPatterns on BridgeLspActivity {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeLspActivity_Idle value)?  idle,TResult Function( BridgeLspActivity_Busy value)?  busy,TResult Function( BridgeLspActivity_Indexing value)?  indexing,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeLspActivity_Idle() when idle != null:
return idle(_that);case BridgeLspActivity_Busy() when busy != null:
return busy(_that);case BridgeLspActivity_Indexing() when indexing != null:
return indexing(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeLspActivity_Idle value)  idle,required TResult Function( BridgeLspActivity_Busy value)  busy,required TResult Function( BridgeLspActivity_Indexing value)  indexing,}){
final _that = this;
switch (_that) {
case BridgeLspActivity_Idle():
return idle(_that);case BridgeLspActivity_Busy():
return busy(_that);case BridgeLspActivity_Indexing():
return indexing(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeLspActivity_Idle value)?  idle,TResult? Function( BridgeLspActivity_Busy value)?  busy,TResult? Function( BridgeLspActivity_Indexing value)?  indexing,}){
final _that = this;
switch (_that) {
case BridgeLspActivity_Idle() when idle != null:
return idle(_that);case BridgeLspActivity_Busy() when busy != null:
return busy(_that);case BridgeLspActivity_Indexing() when indexing != null:
return indexing(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  idle,TResult Function( String? title,  String? message,  int? percentage)?  busy,TResult Function( String? title,  String? message,  int? percentage)?  indexing,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeLspActivity_Idle() when idle != null:
return idle();case BridgeLspActivity_Busy() when busy != null:
return busy(_that.title,_that.message,_that.percentage);case BridgeLspActivity_Indexing() when indexing != null:
return indexing(_that.title,_that.message,_that.percentage);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  idle,required TResult Function( String? title,  String? message,  int? percentage)  busy,required TResult Function( String? title,  String? message,  int? percentage)  indexing,}) {final _that = this;
switch (_that) {
case BridgeLspActivity_Idle():
return idle();case BridgeLspActivity_Busy():
return busy(_that.title,_that.message,_that.percentage);case BridgeLspActivity_Indexing():
return indexing(_that.title,_that.message,_that.percentage);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  idle,TResult? Function( String? title,  String? message,  int? percentage)?  busy,TResult? Function( String? title,  String? message,  int? percentage)?  indexing,}) {final _that = this;
switch (_that) {
case BridgeLspActivity_Idle() when idle != null:
return idle();case BridgeLspActivity_Busy() when busy != null:
return busy(_that.title,_that.message,_that.percentage);case BridgeLspActivity_Indexing() when indexing != null:
return indexing(_that.title,_that.message,_that.percentage);case _:
  return null;

}
}

}

/// @nodoc


class BridgeLspActivity_Idle extends BridgeLspActivity {
  const BridgeLspActivity_Idle(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspActivity_Idle);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeLspActivity.idle()';
}


}




/// @nodoc


class BridgeLspActivity_Busy extends BridgeLspActivity {
  const BridgeLspActivity_Busy({this.title, this.message, this.percentage}): super._();


 final  String? title;
 final  String? message;
 final  int? percentage;

/// Create a copy of BridgeLspActivity
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeLspActivity_BusyCopyWith<BridgeLspActivity_Busy> get copyWith => _$BridgeLspActivity_BusyCopyWithImpl<BridgeLspActivity_Busy>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspActivity_Busy&&(identical(other.title, title) || other.title == title)&&(identical(other.message, message) || other.message == message)&&(identical(other.percentage, percentage) || other.percentage == percentage));
}


@override
int get hashCode => Object.hash(runtimeType,title,message,percentage);

@override
String toString() {
  return 'BridgeLspActivity.busy(title: $title, message: $message, percentage: $percentage)';
}


}

/// @nodoc
abstract mixin class $BridgeLspActivity_BusyCopyWith<$Res> implements $BridgeLspActivityCopyWith<$Res> {
  factory $BridgeLspActivity_BusyCopyWith(BridgeLspActivity_Busy value, $Res Function(BridgeLspActivity_Busy) _then) = _$BridgeLspActivity_BusyCopyWithImpl;
@useResult
$Res call({
 String? title, String? message, int? percentage
});




}
/// @nodoc
class _$BridgeLspActivity_BusyCopyWithImpl<$Res>
    implements $BridgeLspActivity_BusyCopyWith<$Res> {
  _$BridgeLspActivity_BusyCopyWithImpl(this._self, this._then);

  final BridgeLspActivity_Busy _self;
  final $Res Function(BridgeLspActivity_Busy) _then;

/// Create a copy of BridgeLspActivity
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? title = freezed,Object? message = freezed,Object? percentage = freezed,}) {
  return _then(BridgeLspActivity_Busy(
title: freezed == title ? _self.title : title // ignore: cast_nullable_to_non_nullable
as String?,message: freezed == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String?,percentage: freezed == percentage ? _self.percentage : percentage // ignore: cast_nullable_to_non_nullable
as int?,
  ));
}


}

/// @nodoc


class BridgeLspActivity_Indexing extends BridgeLspActivity {
  const BridgeLspActivity_Indexing({this.title, this.message, this.percentage}): super._();


 final  String? title;
 final  String? message;
 final  int? percentage;

/// Create a copy of BridgeLspActivity
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeLspActivity_IndexingCopyWith<BridgeLspActivity_Indexing> get copyWith => _$BridgeLspActivity_IndexingCopyWithImpl<BridgeLspActivity_Indexing>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspActivity_Indexing&&(identical(other.title, title) || other.title == title)&&(identical(other.message, message) || other.message == message)&&(identical(other.percentage, percentage) || other.percentage == percentage));
}


@override
int get hashCode => Object.hash(runtimeType,title,message,percentage);

@override
String toString() {
  return 'BridgeLspActivity.indexing(title: $title, message: $message, percentage: $percentage)';
}


}

/// @nodoc
abstract mixin class $BridgeLspActivity_IndexingCopyWith<$Res> implements $BridgeLspActivityCopyWith<$Res> {
  factory $BridgeLspActivity_IndexingCopyWith(BridgeLspActivity_Indexing value, $Res Function(BridgeLspActivity_Indexing) _then) = _$BridgeLspActivity_IndexingCopyWithImpl;
@useResult
$Res call({
 String? title, String? message, int? percentage
});




}
/// @nodoc
class _$BridgeLspActivity_IndexingCopyWithImpl<$Res>
    implements $BridgeLspActivity_IndexingCopyWith<$Res> {
  _$BridgeLspActivity_IndexingCopyWithImpl(this._self, this._then);

  final BridgeLspActivity_Indexing _self;
  final $Res Function(BridgeLspActivity_Indexing) _then;

/// Create a copy of BridgeLspActivity
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? title = freezed,Object? message = freezed,Object? percentage = freezed,}) {
  return _then(BridgeLspActivity_Indexing(
title: freezed == title ? _self.title : title // ignore: cast_nullable_to_non_nullable
as String?,message: freezed == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String?,percentage: freezed == percentage ? _self.percentage : percentage // ignore: cast_nullable_to_non_nullable
as int?,
  ));
}


}

/// @nodoc
mixin _$BridgeLspServerState {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspServerState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeLspServerState()';
}


}

/// @nodoc
class $BridgeLspServerStateCopyWith<$Res>  {
$BridgeLspServerStateCopyWith(BridgeLspServerState _, $Res Function(BridgeLspServerState) __);
}


/// Adds pattern-matching-related methods to [BridgeLspServerState].
extension BridgeLspServerStatePatterns on BridgeLspServerState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeLspServerState_Checking value)?  checking,TResult Function( BridgeLspServerState_Available value)?  available,TResult Function( BridgeLspServerState_Unavailable value)?  unavailable,TResult Function( BridgeLspServerState_Disabled value)?  disabled,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeLspServerState_Checking() when checking != null:
return checking(_that);case BridgeLspServerState_Available() when available != null:
return available(_that);case BridgeLspServerState_Unavailable() when unavailable != null:
return unavailable(_that);case BridgeLspServerState_Disabled() when disabled != null:
return disabled(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeLspServerState_Checking value)  checking,required TResult Function( BridgeLspServerState_Available value)  available,required TResult Function( BridgeLspServerState_Unavailable value)  unavailable,required TResult Function( BridgeLspServerState_Disabled value)  disabled,}){
final _that = this;
switch (_that) {
case BridgeLspServerState_Checking():
return checking(_that);case BridgeLspServerState_Available():
return available(_that);case BridgeLspServerState_Unavailable():
return unavailable(_that);case BridgeLspServerState_Disabled():
return disabled(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeLspServerState_Checking value)?  checking,TResult? Function( BridgeLspServerState_Available value)?  available,TResult? Function( BridgeLspServerState_Unavailable value)?  unavailable,TResult? Function( BridgeLspServerState_Disabled value)?  disabled,}){
final _that = this;
switch (_that) {
case BridgeLspServerState_Checking() when checking != null:
return checking(_that);case BridgeLspServerState_Available() when available != null:
return available(_that);case BridgeLspServerState_Unavailable() when unavailable != null:
return unavailable(_that);case BridgeLspServerState_Disabled() when disabled != null:
return disabled(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String message)?  checking,TResult Function( PlatformInt64 checkedAt,  BigInt diagnosticCount,  BridgeLspActivity activity)?  available,TResult Function( PlatformInt64 checkedAt,  BridgeStateError error)?  unavailable,TResult Function( String message)?  disabled,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeLspServerState_Checking() when checking != null:
return checking(_that.message);case BridgeLspServerState_Available() when available != null:
return available(_that.checkedAt,_that.diagnosticCount,_that.activity);case BridgeLspServerState_Unavailable() when unavailable != null:
return unavailable(_that.checkedAt,_that.error);case BridgeLspServerState_Disabled() when disabled != null:
return disabled(_that.message);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String message)  checking,required TResult Function( PlatformInt64 checkedAt,  BigInt diagnosticCount,  BridgeLspActivity activity)  available,required TResult Function( PlatformInt64 checkedAt,  BridgeStateError error)  unavailable,required TResult Function( String message)  disabled,}) {final _that = this;
switch (_that) {
case BridgeLspServerState_Checking():
return checking(_that.message);case BridgeLspServerState_Available():
return available(_that.checkedAt,_that.diagnosticCount,_that.activity);case BridgeLspServerState_Unavailable():
return unavailable(_that.checkedAt,_that.error);case BridgeLspServerState_Disabled():
return disabled(_that.message);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String message)?  checking,TResult? Function( PlatformInt64 checkedAt,  BigInt diagnosticCount,  BridgeLspActivity activity)?  available,TResult? Function( PlatformInt64 checkedAt,  BridgeStateError error)?  unavailable,TResult? Function( String message)?  disabled,}) {final _that = this;
switch (_that) {
case BridgeLspServerState_Checking() when checking != null:
return checking(_that.message);case BridgeLspServerState_Available() when available != null:
return available(_that.checkedAt,_that.diagnosticCount,_that.activity);case BridgeLspServerState_Unavailable() when unavailable != null:
return unavailable(_that.checkedAt,_that.error);case BridgeLspServerState_Disabled() when disabled != null:
return disabled(_that.message);case _:
  return null;

}
}

}

/// @nodoc


class BridgeLspServerState_Checking extends BridgeLspServerState {
  const BridgeLspServerState_Checking({required this.message}): super._();


 final  String message;

/// Create a copy of BridgeLspServerState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeLspServerState_CheckingCopyWith<BridgeLspServerState_Checking> get copyWith => _$BridgeLspServerState_CheckingCopyWithImpl<BridgeLspServerState_Checking>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspServerState_Checking&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode => Object.hash(runtimeType,message);

@override
String toString() {
  return 'BridgeLspServerState.checking(message: $message)';
}


}

/// @nodoc
abstract mixin class $BridgeLspServerState_CheckingCopyWith<$Res> implements $BridgeLspServerStateCopyWith<$Res> {
  factory $BridgeLspServerState_CheckingCopyWith(BridgeLspServerState_Checking value, $Res Function(BridgeLspServerState_Checking) _then) = _$BridgeLspServerState_CheckingCopyWithImpl;
@useResult
$Res call({
 String message
});




}
/// @nodoc
class _$BridgeLspServerState_CheckingCopyWithImpl<$Res>
    implements $BridgeLspServerState_CheckingCopyWith<$Res> {
  _$BridgeLspServerState_CheckingCopyWithImpl(this._self, this._then);

  final BridgeLspServerState_Checking _self;
  final $Res Function(BridgeLspServerState_Checking) _then;

/// Create a copy of BridgeLspServerState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = null,}) {
  return _then(BridgeLspServerState_Checking(
message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeLspServerState_Available extends BridgeLspServerState {
  const BridgeLspServerState_Available({required this.checkedAt, required this.diagnosticCount, required this.activity}): super._();


 final  PlatformInt64 checkedAt;
 final  BigInt diagnosticCount;
 final  BridgeLspActivity activity;

/// Create a copy of BridgeLspServerState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeLspServerState_AvailableCopyWith<BridgeLspServerState_Available> get copyWith => _$BridgeLspServerState_AvailableCopyWithImpl<BridgeLspServerState_Available>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspServerState_Available&&(identical(other.checkedAt, checkedAt) || other.checkedAt == checkedAt)&&(identical(other.diagnosticCount, diagnosticCount) || other.diagnosticCount == diagnosticCount)&&(identical(other.activity, activity) || other.activity == activity));
}


@override
int get hashCode => Object.hash(runtimeType,checkedAt,diagnosticCount,activity);

@override
String toString() {
  return 'BridgeLspServerState.available(checkedAt: $checkedAt, diagnosticCount: $diagnosticCount, activity: $activity)';
}


}

/// @nodoc
abstract mixin class $BridgeLspServerState_AvailableCopyWith<$Res> implements $BridgeLspServerStateCopyWith<$Res> {
  factory $BridgeLspServerState_AvailableCopyWith(BridgeLspServerState_Available value, $Res Function(BridgeLspServerState_Available) _then) = _$BridgeLspServerState_AvailableCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 checkedAt, BigInt diagnosticCount, BridgeLspActivity activity
});


$BridgeLspActivityCopyWith<$Res> get activity;

}
/// @nodoc
class _$BridgeLspServerState_AvailableCopyWithImpl<$Res>
    implements $BridgeLspServerState_AvailableCopyWith<$Res> {
  _$BridgeLspServerState_AvailableCopyWithImpl(this._self, this._then);

  final BridgeLspServerState_Available _self;
  final $Res Function(BridgeLspServerState_Available) _then;

/// Create a copy of BridgeLspServerState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? checkedAt = null,Object? diagnosticCount = null,Object? activity = null,}) {
  return _then(BridgeLspServerState_Available(
checkedAt: null == checkedAt ? _self.checkedAt : checkedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,diagnosticCount: null == diagnosticCount ? _self.diagnosticCount : diagnosticCount // ignore: cast_nullable_to_non_nullable
as BigInt,activity: null == activity ? _self.activity : activity // ignore: cast_nullable_to_non_nullable
as BridgeLspActivity,
  ));
}

/// Create a copy of BridgeLspServerState
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeLspActivityCopyWith<$Res> get activity {

  return $BridgeLspActivityCopyWith<$Res>(_self.activity, (value) {
    return _then(_self.copyWith(activity: value));
  });
}
}

/// @nodoc


class BridgeLspServerState_Unavailable extends BridgeLspServerState {
  const BridgeLspServerState_Unavailable({required this.checkedAt, required this.error}): super._();


 final  PlatformInt64 checkedAt;
 final  BridgeStateError error;

/// Create a copy of BridgeLspServerState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeLspServerState_UnavailableCopyWith<BridgeLspServerState_Unavailable> get copyWith => _$BridgeLspServerState_UnavailableCopyWithImpl<BridgeLspServerState_Unavailable>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspServerState_Unavailable&&(identical(other.checkedAt, checkedAt) || other.checkedAt == checkedAt)&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,checkedAt,error);

@override
String toString() {
  return 'BridgeLspServerState.unavailable(checkedAt: $checkedAt, error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeLspServerState_UnavailableCopyWith<$Res> implements $BridgeLspServerStateCopyWith<$Res> {
  factory $BridgeLspServerState_UnavailableCopyWith(BridgeLspServerState_Unavailable value, $Res Function(BridgeLspServerState_Unavailable) _then) = _$BridgeLspServerState_UnavailableCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 checkedAt, BridgeStateError error
});




}
/// @nodoc
class _$BridgeLspServerState_UnavailableCopyWithImpl<$Res>
    implements $BridgeLspServerState_UnavailableCopyWith<$Res> {
  _$BridgeLspServerState_UnavailableCopyWithImpl(this._self, this._then);

  final BridgeLspServerState_Unavailable _self;
  final $Res Function(BridgeLspServerState_Unavailable) _then;

/// Create a copy of BridgeLspServerState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? checkedAt = null,Object? error = null,}) {
  return _then(BridgeLspServerState_Unavailable(
checkedAt: null == checkedAt ? _self.checkedAt : checkedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as BridgeStateError,
  ));
}


}

/// @nodoc


class BridgeLspServerState_Disabled extends BridgeLspServerState {
  const BridgeLspServerState_Disabled({required this.message}): super._();


 final  String message;

/// Create a copy of BridgeLspServerState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeLspServerState_DisabledCopyWith<BridgeLspServerState_Disabled> get copyWith => _$BridgeLspServerState_DisabledCopyWithImpl<BridgeLspServerState_Disabled>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeLspServerState_Disabled&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode => Object.hash(runtimeType,message);

@override
String toString() {
  return 'BridgeLspServerState.disabled(message: $message)';
}


}

/// @nodoc
abstract mixin class $BridgeLspServerState_DisabledCopyWith<$Res> implements $BridgeLspServerStateCopyWith<$Res> {
  factory $BridgeLspServerState_DisabledCopyWith(BridgeLspServerState_Disabled value, $Res Function(BridgeLspServerState_Disabled) _then) = _$BridgeLspServerState_DisabledCopyWithImpl;
@useResult
$Res call({
 String message
});




}
/// @nodoc
class _$BridgeLspServerState_DisabledCopyWithImpl<$Res>
    implements $BridgeLspServerState_DisabledCopyWith<$Res> {
  _$BridgeLspServerState_DisabledCopyWithImpl(this._self, this._then);

  final BridgeLspServerState_Disabled _self;
  final $Res Function(BridgeLspServerState_Disabled) _then;

/// Create a copy of BridgeLspServerState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = null,}) {
  return _then(BridgeLspServerState_Disabled(
message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeMcpServerState {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpServerState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeMcpServerState()';
}


}

/// @nodoc
class $BridgeMcpServerStateCopyWith<$Res>  {
$BridgeMcpServerStateCopyWith(BridgeMcpServerState _, $Res Function(BridgeMcpServerState) __);
}


/// Adds pattern-matching-related methods to [BridgeMcpServerState].
extension BridgeMcpServerStatePatterns on BridgeMcpServerState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeMcpServerState_Disabled value)?  disabled,TResult Function( BridgeMcpServerState_MissingCredential value)?  missingCredential,TResult Function( BridgeMcpServerState_Checking value)?  checking,TResult Function( BridgeMcpServerState_Available value)?  available,TResult Function( BridgeMcpServerState_Unavailable value)?  unavailable,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeMcpServerState_Disabled() when disabled != null:
return disabled(_that);case BridgeMcpServerState_MissingCredential() when missingCredential != null:
return missingCredential(_that);case BridgeMcpServerState_Checking() when checking != null:
return checking(_that);case BridgeMcpServerState_Available() when available != null:
return available(_that);case BridgeMcpServerState_Unavailable() when unavailable != null:
return unavailable(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeMcpServerState_Disabled value)  disabled,required TResult Function( BridgeMcpServerState_MissingCredential value)  missingCredential,required TResult Function( BridgeMcpServerState_Checking value)  checking,required TResult Function( BridgeMcpServerState_Available value)  available,required TResult Function( BridgeMcpServerState_Unavailable value)  unavailable,}){
final _that = this;
switch (_that) {
case BridgeMcpServerState_Disabled():
return disabled(_that);case BridgeMcpServerState_MissingCredential():
return missingCredential(_that);case BridgeMcpServerState_Checking():
return checking(_that);case BridgeMcpServerState_Available():
return available(_that);case BridgeMcpServerState_Unavailable():
return unavailable(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeMcpServerState_Disabled value)?  disabled,TResult? Function( BridgeMcpServerState_MissingCredential value)?  missingCredential,TResult? Function( BridgeMcpServerState_Checking value)?  checking,TResult? Function( BridgeMcpServerState_Available value)?  available,TResult? Function( BridgeMcpServerState_Unavailable value)?  unavailable,}){
final _that = this;
switch (_that) {
case BridgeMcpServerState_Disabled() when disabled != null:
return disabled(_that);case BridgeMcpServerState_MissingCredential() when missingCredential != null:
return missingCredential(_that);case BridgeMcpServerState_Checking() when checking != null:
return checking(_that);case BridgeMcpServerState_Available() when available != null:
return available(_that);case BridgeMcpServerState_Unavailable() when unavailable != null:
return unavailable(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String message)?  disabled,TResult Function( String message)?  missingCredential,TResult Function( String message)?  checking,TResult Function( PlatformInt64 checkedAt,  BigInt toolCount)?  available,TResult Function( PlatformInt64 checkedAt,  BridgeStateError error)?  unavailable,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeMcpServerState_Disabled() when disabled != null:
return disabled(_that.message);case BridgeMcpServerState_MissingCredential() when missingCredential != null:
return missingCredential(_that.message);case BridgeMcpServerState_Checking() when checking != null:
return checking(_that.message);case BridgeMcpServerState_Available() when available != null:
return available(_that.checkedAt,_that.toolCount);case BridgeMcpServerState_Unavailable() when unavailable != null:
return unavailable(_that.checkedAt,_that.error);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String message)  disabled,required TResult Function( String message)  missingCredential,required TResult Function( String message)  checking,required TResult Function( PlatformInt64 checkedAt,  BigInt toolCount)  available,required TResult Function( PlatformInt64 checkedAt,  BridgeStateError error)  unavailable,}) {final _that = this;
switch (_that) {
case BridgeMcpServerState_Disabled():
return disabled(_that.message);case BridgeMcpServerState_MissingCredential():
return missingCredential(_that.message);case BridgeMcpServerState_Checking():
return checking(_that.message);case BridgeMcpServerState_Available():
return available(_that.checkedAt,_that.toolCount);case BridgeMcpServerState_Unavailable():
return unavailable(_that.checkedAt,_that.error);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String message)?  disabled,TResult? Function( String message)?  missingCredential,TResult? Function( String message)?  checking,TResult? Function( PlatformInt64 checkedAt,  BigInt toolCount)?  available,TResult? Function( PlatformInt64 checkedAt,  BridgeStateError error)?  unavailable,}) {final _that = this;
switch (_that) {
case BridgeMcpServerState_Disabled() when disabled != null:
return disabled(_that.message);case BridgeMcpServerState_MissingCredential() when missingCredential != null:
return missingCredential(_that.message);case BridgeMcpServerState_Checking() when checking != null:
return checking(_that.message);case BridgeMcpServerState_Available() when available != null:
return available(_that.checkedAt,_that.toolCount);case BridgeMcpServerState_Unavailable() when unavailable != null:
return unavailable(_that.checkedAt,_that.error);case _:
  return null;

}
}

}

/// @nodoc


class BridgeMcpServerState_Disabled extends BridgeMcpServerState {
  const BridgeMcpServerState_Disabled({required this.message}): super._();


 final  String message;

/// Create a copy of BridgeMcpServerState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMcpServerState_DisabledCopyWith<BridgeMcpServerState_Disabled> get copyWith => _$BridgeMcpServerState_DisabledCopyWithImpl<BridgeMcpServerState_Disabled>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpServerState_Disabled&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode => Object.hash(runtimeType,message);

@override
String toString() {
  return 'BridgeMcpServerState.disabled(message: $message)';
}


}

/// @nodoc
abstract mixin class $BridgeMcpServerState_DisabledCopyWith<$Res> implements $BridgeMcpServerStateCopyWith<$Res> {
  factory $BridgeMcpServerState_DisabledCopyWith(BridgeMcpServerState_Disabled value, $Res Function(BridgeMcpServerState_Disabled) _then) = _$BridgeMcpServerState_DisabledCopyWithImpl;
@useResult
$Res call({
 String message
});




}
/// @nodoc
class _$BridgeMcpServerState_DisabledCopyWithImpl<$Res>
    implements $BridgeMcpServerState_DisabledCopyWith<$Res> {
  _$BridgeMcpServerState_DisabledCopyWithImpl(this._self, this._then);

  final BridgeMcpServerState_Disabled _self;
  final $Res Function(BridgeMcpServerState_Disabled) _then;

/// Create a copy of BridgeMcpServerState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = null,}) {
  return _then(BridgeMcpServerState_Disabled(
message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeMcpServerState_MissingCredential extends BridgeMcpServerState {
  const BridgeMcpServerState_MissingCredential({required this.message}): super._();


 final  String message;

/// Create a copy of BridgeMcpServerState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMcpServerState_MissingCredentialCopyWith<BridgeMcpServerState_MissingCredential> get copyWith => _$BridgeMcpServerState_MissingCredentialCopyWithImpl<BridgeMcpServerState_MissingCredential>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpServerState_MissingCredential&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode => Object.hash(runtimeType,message);

@override
String toString() {
  return 'BridgeMcpServerState.missingCredential(message: $message)';
}


}

/// @nodoc
abstract mixin class $BridgeMcpServerState_MissingCredentialCopyWith<$Res> implements $BridgeMcpServerStateCopyWith<$Res> {
  factory $BridgeMcpServerState_MissingCredentialCopyWith(BridgeMcpServerState_MissingCredential value, $Res Function(BridgeMcpServerState_MissingCredential) _then) = _$BridgeMcpServerState_MissingCredentialCopyWithImpl;
@useResult
$Res call({
 String message
});




}
/// @nodoc
class _$BridgeMcpServerState_MissingCredentialCopyWithImpl<$Res>
    implements $BridgeMcpServerState_MissingCredentialCopyWith<$Res> {
  _$BridgeMcpServerState_MissingCredentialCopyWithImpl(this._self, this._then);

  final BridgeMcpServerState_MissingCredential _self;
  final $Res Function(BridgeMcpServerState_MissingCredential) _then;

/// Create a copy of BridgeMcpServerState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = null,}) {
  return _then(BridgeMcpServerState_MissingCredential(
message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeMcpServerState_Checking extends BridgeMcpServerState {
  const BridgeMcpServerState_Checking({required this.message}): super._();


 final  String message;

/// Create a copy of BridgeMcpServerState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMcpServerState_CheckingCopyWith<BridgeMcpServerState_Checking> get copyWith => _$BridgeMcpServerState_CheckingCopyWithImpl<BridgeMcpServerState_Checking>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpServerState_Checking&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode => Object.hash(runtimeType,message);

@override
String toString() {
  return 'BridgeMcpServerState.checking(message: $message)';
}


}

/// @nodoc
abstract mixin class $BridgeMcpServerState_CheckingCopyWith<$Res> implements $BridgeMcpServerStateCopyWith<$Res> {
  factory $BridgeMcpServerState_CheckingCopyWith(BridgeMcpServerState_Checking value, $Res Function(BridgeMcpServerState_Checking) _then) = _$BridgeMcpServerState_CheckingCopyWithImpl;
@useResult
$Res call({
 String message
});




}
/// @nodoc
class _$BridgeMcpServerState_CheckingCopyWithImpl<$Res>
    implements $BridgeMcpServerState_CheckingCopyWith<$Res> {
  _$BridgeMcpServerState_CheckingCopyWithImpl(this._self, this._then);

  final BridgeMcpServerState_Checking _self;
  final $Res Function(BridgeMcpServerState_Checking) _then;

/// Create a copy of BridgeMcpServerState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = null,}) {
  return _then(BridgeMcpServerState_Checking(
message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeMcpServerState_Available extends BridgeMcpServerState {
  const BridgeMcpServerState_Available({required this.checkedAt, required this.toolCount}): super._();


 final  PlatformInt64 checkedAt;
 final  BigInt toolCount;

/// Create a copy of BridgeMcpServerState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMcpServerState_AvailableCopyWith<BridgeMcpServerState_Available> get copyWith => _$BridgeMcpServerState_AvailableCopyWithImpl<BridgeMcpServerState_Available>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpServerState_Available&&(identical(other.checkedAt, checkedAt) || other.checkedAt == checkedAt)&&(identical(other.toolCount, toolCount) || other.toolCount == toolCount));
}


@override
int get hashCode => Object.hash(runtimeType,checkedAt,toolCount);

@override
String toString() {
  return 'BridgeMcpServerState.available(checkedAt: $checkedAt, toolCount: $toolCount)';
}


}

/// @nodoc
abstract mixin class $BridgeMcpServerState_AvailableCopyWith<$Res> implements $BridgeMcpServerStateCopyWith<$Res> {
  factory $BridgeMcpServerState_AvailableCopyWith(BridgeMcpServerState_Available value, $Res Function(BridgeMcpServerState_Available) _then) = _$BridgeMcpServerState_AvailableCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 checkedAt, BigInt toolCount
});




}
/// @nodoc
class _$BridgeMcpServerState_AvailableCopyWithImpl<$Res>
    implements $BridgeMcpServerState_AvailableCopyWith<$Res> {
  _$BridgeMcpServerState_AvailableCopyWithImpl(this._self, this._then);

  final BridgeMcpServerState_Available _self;
  final $Res Function(BridgeMcpServerState_Available) _then;

/// Create a copy of BridgeMcpServerState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? checkedAt = null,Object? toolCount = null,}) {
  return _then(BridgeMcpServerState_Available(
checkedAt: null == checkedAt ? _self.checkedAt : checkedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,toolCount: null == toolCount ? _self.toolCount : toolCount // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

/// @nodoc


class BridgeMcpServerState_Unavailable extends BridgeMcpServerState {
  const BridgeMcpServerState_Unavailable({required this.checkedAt, required this.error}): super._();


 final  PlatformInt64 checkedAt;
 final  BridgeStateError error;

/// Create a copy of BridgeMcpServerState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMcpServerState_UnavailableCopyWith<BridgeMcpServerState_Unavailable> get copyWith => _$BridgeMcpServerState_UnavailableCopyWithImpl<BridgeMcpServerState_Unavailable>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMcpServerState_Unavailable&&(identical(other.checkedAt, checkedAt) || other.checkedAt == checkedAt)&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,checkedAt,error);

@override
String toString() {
  return 'BridgeMcpServerState.unavailable(checkedAt: $checkedAt, error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeMcpServerState_UnavailableCopyWith<$Res> implements $BridgeMcpServerStateCopyWith<$Res> {
  factory $BridgeMcpServerState_UnavailableCopyWith(BridgeMcpServerState_Unavailable value, $Res Function(BridgeMcpServerState_Unavailable) _then) = _$BridgeMcpServerState_UnavailableCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 checkedAt, BridgeStateError error
});




}
/// @nodoc
class _$BridgeMcpServerState_UnavailableCopyWithImpl<$Res>
    implements $BridgeMcpServerState_UnavailableCopyWith<$Res> {
  _$BridgeMcpServerState_UnavailableCopyWithImpl(this._self, this._then);

  final BridgeMcpServerState_Unavailable _self;
  final $Res Function(BridgeMcpServerState_Unavailable) _then;

/// Create a copy of BridgeMcpServerState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? checkedAt = null,Object? error = null,}) {
  return _then(BridgeMcpServerState_Unavailable(
checkedAt: null == checkedAt ? _self.checkedAt : checkedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as BridgeStateError,
  ));
}


}

/// @nodoc
mixin _$BridgeMergeCleanupState {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMergeCleanupState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeMergeCleanupState()';
}


}

/// @nodoc
class $BridgeMergeCleanupStateCopyWith<$Res>  {
$BridgeMergeCleanupStateCopyWith(BridgeMergeCleanupState _, $Res Function(BridgeMergeCleanupState) __);
}


/// Adds pattern-matching-related methods to [BridgeMergeCleanupState].
extension BridgeMergeCleanupStatePatterns on BridgeMergeCleanupState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeMergeCleanupState_Pending value)?  pending,TResult Function( BridgeMergeCleanupState_Deferred value)?  deferred_,TResult Function( BridgeMergeCleanupState_Attempting value)?  attempting,TResult Function( BridgeMergeCleanupState_Discarded value)?  discarded,TResult Function( BridgeMergeCleanupState_AlreadyAbsent value)?  alreadyAbsent,TResult Function( BridgeMergeCleanupState_Failed value)?  failed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeMergeCleanupState_Pending() when pending != null:
return pending(_that);case BridgeMergeCleanupState_Deferred() when deferred_ != null:
return deferred_(_that);case BridgeMergeCleanupState_Attempting() when attempting != null:
return attempting(_that);case BridgeMergeCleanupState_Discarded() when discarded != null:
return discarded(_that);case BridgeMergeCleanupState_AlreadyAbsent() when alreadyAbsent != null:
return alreadyAbsent(_that);case BridgeMergeCleanupState_Failed() when failed != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeMergeCleanupState_Pending value)  pending,required TResult Function( BridgeMergeCleanupState_Deferred value)  deferred_,required TResult Function( BridgeMergeCleanupState_Attempting value)  attempting,required TResult Function( BridgeMergeCleanupState_Discarded value)  discarded,required TResult Function( BridgeMergeCleanupState_AlreadyAbsent value)  alreadyAbsent,required TResult Function( BridgeMergeCleanupState_Failed value)  failed,}){
final _that = this;
switch (_that) {
case BridgeMergeCleanupState_Pending():
return pending(_that);case BridgeMergeCleanupState_Deferred():
return deferred_(_that);case BridgeMergeCleanupState_Attempting():
return attempting(_that);case BridgeMergeCleanupState_Discarded():
return discarded(_that);case BridgeMergeCleanupState_AlreadyAbsent():
return alreadyAbsent(_that);case BridgeMergeCleanupState_Failed():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeMergeCleanupState_Pending value)?  pending,TResult? Function( BridgeMergeCleanupState_Deferred value)?  deferred_,TResult? Function( BridgeMergeCleanupState_Attempting value)?  attempting,TResult? Function( BridgeMergeCleanupState_Discarded value)?  discarded,TResult? Function( BridgeMergeCleanupState_AlreadyAbsent value)?  alreadyAbsent,TResult? Function( BridgeMergeCleanupState_Failed value)?  failed,}){
final _that = this;
switch (_that) {
case BridgeMergeCleanupState_Pending() when pending != null:
return pending(_that);case BridgeMergeCleanupState_Deferred() when deferred_ != null:
return deferred_(_that);case BridgeMergeCleanupState_Attempting() when attempting != null:
return attempting(_that);case BridgeMergeCleanupState_Discarded() when discarded != null:
return discarded(_that);case BridgeMergeCleanupState_AlreadyAbsent() when alreadyAbsent != null:
return alreadyAbsent(_that);case BridgeMergeCleanupState_Failed() when failed != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  pending,TResult Function()?  deferred_,TResult Function( String operationId,  PlatformInt64 startedAt)?  attempting,TResult Function( String operationId,  PlatformInt64 completedAt)?  discarded,TResult Function( String operationId,  PlatformInt64 completedAt)?  alreadyAbsent,TResult Function( String operationId,  PlatformInt64 failedAt,  String detail)?  failed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeMergeCleanupState_Pending() when pending != null:
return pending();case BridgeMergeCleanupState_Deferred() when deferred_ != null:
return deferred_();case BridgeMergeCleanupState_Attempting() when attempting != null:
return attempting(_that.operationId,_that.startedAt);case BridgeMergeCleanupState_Discarded() when discarded != null:
return discarded(_that.operationId,_that.completedAt);case BridgeMergeCleanupState_AlreadyAbsent() when alreadyAbsent != null:
return alreadyAbsent(_that.operationId,_that.completedAt);case BridgeMergeCleanupState_Failed() when failed != null:
return failed(_that.operationId,_that.failedAt,_that.detail);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  pending,required TResult Function()  deferred_,required TResult Function( String operationId,  PlatformInt64 startedAt)  attempting,required TResult Function( String operationId,  PlatformInt64 completedAt)  discarded,required TResult Function( String operationId,  PlatformInt64 completedAt)  alreadyAbsent,required TResult Function( String operationId,  PlatformInt64 failedAt,  String detail)  failed,}) {final _that = this;
switch (_that) {
case BridgeMergeCleanupState_Pending():
return pending();case BridgeMergeCleanupState_Deferred():
return deferred_();case BridgeMergeCleanupState_Attempting():
return attempting(_that.operationId,_that.startedAt);case BridgeMergeCleanupState_Discarded():
return discarded(_that.operationId,_that.completedAt);case BridgeMergeCleanupState_AlreadyAbsent():
return alreadyAbsent(_that.operationId,_that.completedAt);case BridgeMergeCleanupState_Failed():
return failed(_that.operationId,_that.failedAt,_that.detail);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  pending,TResult? Function()?  deferred_,TResult? Function( String operationId,  PlatformInt64 startedAt)?  attempting,TResult? Function( String operationId,  PlatformInt64 completedAt)?  discarded,TResult? Function( String operationId,  PlatformInt64 completedAt)?  alreadyAbsent,TResult? Function( String operationId,  PlatformInt64 failedAt,  String detail)?  failed,}) {final _that = this;
switch (_that) {
case BridgeMergeCleanupState_Pending() when pending != null:
return pending();case BridgeMergeCleanupState_Deferred() when deferred_ != null:
return deferred_();case BridgeMergeCleanupState_Attempting() when attempting != null:
return attempting(_that.operationId,_that.startedAt);case BridgeMergeCleanupState_Discarded() when discarded != null:
return discarded(_that.operationId,_that.completedAt);case BridgeMergeCleanupState_AlreadyAbsent() when alreadyAbsent != null:
return alreadyAbsent(_that.operationId,_that.completedAt);case BridgeMergeCleanupState_Failed() when failed != null:
return failed(_that.operationId,_that.failedAt,_that.detail);case _:
  return null;

}
}

}

/// @nodoc


class BridgeMergeCleanupState_Pending extends BridgeMergeCleanupState {
  const BridgeMergeCleanupState_Pending(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMergeCleanupState_Pending);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeMergeCleanupState.pending()';
}


}




/// @nodoc


class BridgeMergeCleanupState_Deferred extends BridgeMergeCleanupState {
  const BridgeMergeCleanupState_Deferred(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMergeCleanupState_Deferred);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeMergeCleanupState.deferred_()';
}


}




/// @nodoc


class BridgeMergeCleanupState_Attempting extends BridgeMergeCleanupState {
  const BridgeMergeCleanupState_Attempting({required this.operationId, required this.startedAt}): super._();


 final  String operationId;
 final  PlatformInt64 startedAt;

/// Create a copy of BridgeMergeCleanupState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMergeCleanupState_AttemptingCopyWith<BridgeMergeCleanupState_Attempting> get copyWith => _$BridgeMergeCleanupState_AttemptingCopyWithImpl<BridgeMergeCleanupState_Attempting>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMergeCleanupState_Attempting&&(identical(other.operationId, operationId) || other.operationId == operationId)&&(identical(other.startedAt, startedAt) || other.startedAt == startedAt));
}


@override
int get hashCode => Object.hash(runtimeType,operationId,startedAt);

@override
String toString() {
  return 'BridgeMergeCleanupState.attempting(operationId: $operationId, startedAt: $startedAt)';
}


}

/// @nodoc
abstract mixin class $BridgeMergeCleanupState_AttemptingCopyWith<$Res> implements $BridgeMergeCleanupStateCopyWith<$Res> {
  factory $BridgeMergeCleanupState_AttemptingCopyWith(BridgeMergeCleanupState_Attempting value, $Res Function(BridgeMergeCleanupState_Attempting) _then) = _$BridgeMergeCleanupState_AttemptingCopyWithImpl;
@useResult
$Res call({
 String operationId, PlatformInt64 startedAt
});




}
/// @nodoc
class _$BridgeMergeCleanupState_AttemptingCopyWithImpl<$Res>
    implements $BridgeMergeCleanupState_AttemptingCopyWith<$Res> {
  _$BridgeMergeCleanupState_AttemptingCopyWithImpl(this._self, this._then);

  final BridgeMergeCleanupState_Attempting _self;
  final $Res Function(BridgeMergeCleanupState_Attempting) _then;

/// Create a copy of BridgeMergeCleanupState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? operationId = null,Object? startedAt = null,}) {
  return _then(BridgeMergeCleanupState_Attempting(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,startedAt: null == startedAt ? _self.startedAt : startedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,
  ));
}


}

/// @nodoc


class BridgeMergeCleanupState_Discarded extends BridgeMergeCleanupState {
  const BridgeMergeCleanupState_Discarded({required this.operationId, required this.completedAt}): super._();


 final  String operationId;
 final  PlatformInt64 completedAt;

/// Create a copy of BridgeMergeCleanupState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMergeCleanupState_DiscardedCopyWith<BridgeMergeCleanupState_Discarded> get copyWith => _$BridgeMergeCleanupState_DiscardedCopyWithImpl<BridgeMergeCleanupState_Discarded>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMergeCleanupState_Discarded&&(identical(other.operationId, operationId) || other.operationId == operationId)&&(identical(other.completedAt, completedAt) || other.completedAt == completedAt));
}


@override
int get hashCode => Object.hash(runtimeType,operationId,completedAt);

@override
String toString() {
  return 'BridgeMergeCleanupState.discarded(operationId: $operationId, completedAt: $completedAt)';
}


}

/// @nodoc
abstract mixin class $BridgeMergeCleanupState_DiscardedCopyWith<$Res> implements $BridgeMergeCleanupStateCopyWith<$Res> {
  factory $BridgeMergeCleanupState_DiscardedCopyWith(BridgeMergeCleanupState_Discarded value, $Res Function(BridgeMergeCleanupState_Discarded) _then) = _$BridgeMergeCleanupState_DiscardedCopyWithImpl;
@useResult
$Res call({
 String operationId, PlatformInt64 completedAt
});




}
/// @nodoc
class _$BridgeMergeCleanupState_DiscardedCopyWithImpl<$Res>
    implements $BridgeMergeCleanupState_DiscardedCopyWith<$Res> {
  _$BridgeMergeCleanupState_DiscardedCopyWithImpl(this._self, this._then);

  final BridgeMergeCleanupState_Discarded _self;
  final $Res Function(BridgeMergeCleanupState_Discarded) _then;

/// Create a copy of BridgeMergeCleanupState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? operationId = null,Object? completedAt = null,}) {
  return _then(BridgeMergeCleanupState_Discarded(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,completedAt: null == completedAt ? _self.completedAt : completedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,
  ));
}


}

/// @nodoc


class BridgeMergeCleanupState_AlreadyAbsent extends BridgeMergeCleanupState {
  const BridgeMergeCleanupState_AlreadyAbsent({required this.operationId, required this.completedAt}): super._();


 final  String operationId;
 final  PlatformInt64 completedAt;

/// Create a copy of BridgeMergeCleanupState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMergeCleanupState_AlreadyAbsentCopyWith<BridgeMergeCleanupState_AlreadyAbsent> get copyWith => _$BridgeMergeCleanupState_AlreadyAbsentCopyWithImpl<BridgeMergeCleanupState_AlreadyAbsent>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMergeCleanupState_AlreadyAbsent&&(identical(other.operationId, operationId) || other.operationId == operationId)&&(identical(other.completedAt, completedAt) || other.completedAt == completedAt));
}


@override
int get hashCode => Object.hash(runtimeType,operationId,completedAt);

@override
String toString() {
  return 'BridgeMergeCleanupState.alreadyAbsent(operationId: $operationId, completedAt: $completedAt)';
}


}

/// @nodoc
abstract mixin class $BridgeMergeCleanupState_AlreadyAbsentCopyWith<$Res> implements $BridgeMergeCleanupStateCopyWith<$Res> {
  factory $BridgeMergeCleanupState_AlreadyAbsentCopyWith(BridgeMergeCleanupState_AlreadyAbsent value, $Res Function(BridgeMergeCleanupState_AlreadyAbsent) _then) = _$BridgeMergeCleanupState_AlreadyAbsentCopyWithImpl;
@useResult
$Res call({
 String operationId, PlatformInt64 completedAt
});




}
/// @nodoc
class _$BridgeMergeCleanupState_AlreadyAbsentCopyWithImpl<$Res>
    implements $BridgeMergeCleanupState_AlreadyAbsentCopyWith<$Res> {
  _$BridgeMergeCleanupState_AlreadyAbsentCopyWithImpl(this._self, this._then);

  final BridgeMergeCleanupState_AlreadyAbsent _self;
  final $Res Function(BridgeMergeCleanupState_AlreadyAbsent) _then;

/// Create a copy of BridgeMergeCleanupState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? operationId = null,Object? completedAt = null,}) {
  return _then(BridgeMergeCleanupState_AlreadyAbsent(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,completedAt: null == completedAt ? _self.completedAt : completedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,
  ));
}


}

/// @nodoc


class BridgeMergeCleanupState_Failed extends BridgeMergeCleanupState {
  const BridgeMergeCleanupState_Failed({required this.operationId, required this.failedAt, required this.detail}): super._();


 final  String operationId;
 final  PlatformInt64 failedAt;
 final  String detail;

/// Create a copy of BridgeMergeCleanupState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeMergeCleanupState_FailedCopyWith<BridgeMergeCleanupState_Failed> get copyWith => _$BridgeMergeCleanupState_FailedCopyWithImpl<BridgeMergeCleanupState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeMergeCleanupState_Failed&&(identical(other.operationId, operationId) || other.operationId == operationId)&&(identical(other.failedAt, failedAt) || other.failedAt == failedAt)&&(identical(other.detail, detail) || other.detail == detail));
}


@override
int get hashCode => Object.hash(runtimeType,operationId,failedAt,detail);

@override
String toString() {
  return 'BridgeMergeCleanupState.failed(operationId: $operationId, failedAt: $failedAt, detail: $detail)';
}


}

/// @nodoc
abstract mixin class $BridgeMergeCleanupState_FailedCopyWith<$Res> implements $BridgeMergeCleanupStateCopyWith<$Res> {
  factory $BridgeMergeCleanupState_FailedCopyWith(BridgeMergeCleanupState_Failed value, $Res Function(BridgeMergeCleanupState_Failed) _then) = _$BridgeMergeCleanupState_FailedCopyWithImpl;
@useResult
$Res call({
 String operationId, PlatformInt64 failedAt, String detail
});




}
/// @nodoc
class _$BridgeMergeCleanupState_FailedCopyWithImpl<$Res>
    implements $BridgeMergeCleanupState_FailedCopyWith<$Res> {
  _$BridgeMergeCleanupState_FailedCopyWithImpl(this._self, this._then);

  final BridgeMergeCleanupState_Failed _self;
  final $Res Function(BridgeMergeCleanupState_Failed) _then;

/// Create a copy of BridgeMergeCleanupState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? operationId = null,Object? failedAt = null,Object? detail = null,}) {
  return _then(BridgeMergeCleanupState_Failed(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,failedAt: null == failedAt ? _self.failedAt : failedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,detail: null == detail ? _self.detail : detail // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeRunningWorkUnitActivity {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRunningWorkUnitActivity);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeRunningWorkUnitActivity()';
}


}

/// @nodoc
class $BridgeRunningWorkUnitActivityCopyWith<$Res>  {
$BridgeRunningWorkUnitActivityCopyWith(BridgeRunningWorkUnitActivity _, $Res Function(BridgeRunningWorkUnitActivity) __);
}


/// Adds pattern-matching-related methods to [BridgeRunningWorkUnitActivity].
extension BridgeRunningWorkUnitActivityPatterns on BridgeRunningWorkUnitActivity {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeRunningWorkUnitActivity_Allocated value)?  allocated,TResult Function( BridgeRunningWorkUnitActivity_Active value)?  active,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeRunningWorkUnitActivity_Allocated() when allocated != null:
return allocated(_that);case BridgeRunningWorkUnitActivity_Active() when active != null:
return active(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeRunningWorkUnitActivity_Allocated value)  allocated,required TResult Function( BridgeRunningWorkUnitActivity_Active value)  active,}){
final _that = this;
switch (_that) {
case BridgeRunningWorkUnitActivity_Allocated():
return allocated(_that);case BridgeRunningWorkUnitActivity_Active():
return active(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeRunningWorkUnitActivity_Allocated value)?  allocated,TResult? Function( BridgeRunningWorkUnitActivity_Active value)?  active,}){
final _that = this;
switch (_that) {
case BridgeRunningWorkUnitActivity_Allocated() when allocated != null:
return allocated(_that);case BridgeRunningWorkUnitActivity_Active() when active != null:
return active(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  allocated,TResult Function( String turnId)?  active,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeRunningWorkUnitActivity_Allocated() when allocated != null:
return allocated();case BridgeRunningWorkUnitActivity_Active() when active != null:
return active(_that.turnId);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  allocated,required TResult Function( String turnId)  active,}) {final _that = this;
switch (_that) {
case BridgeRunningWorkUnitActivity_Allocated():
return allocated();case BridgeRunningWorkUnitActivity_Active():
return active(_that.turnId);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  allocated,TResult? Function( String turnId)?  active,}) {final _that = this;
switch (_that) {
case BridgeRunningWorkUnitActivity_Allocated() when allocated != null:
return allocated();case BridgeRunningWorkUnitActivity_Active() when active != null:
return active(_that.turnId);case _:
  return null;

}
}

}

/// @nodoc


class BridgeRunningWorkUnitActivity_Allocated extends BridgeRunningWorkUnitActivity {
  const BridgeRunningWorkUnitActivity_Allocated(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRunningWorkUnitActivity_Allocated);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeRunningWorkUnitActivity.allocated()';
}


}




/// @nodoc


class BridgeRunningWorkUnitActivity_Active extends BridgeRunningWorkUnitActivity {
  const BridgeRunningWorkUnitActivity_Active({required this.turnId}): super._();


 final  String turnId;

/// Create a copy of BridgeRunningWorkUnitActivity
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRunningWorkUnitActivity_ActiveCopyWith<BridgeRunningWorkUnitActivity_Active> get copyWith => _$BridgeRunningWorkUnitActivity_ActiveCopyWithImpl<BridgeRunningWorkUnitActivity_Active>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRunningWorkUnitActivity_Active&&(identical(other.turnId, turnId) || other.turnId == turnId));
}


@override
int get hashCode => Object.hash(runtimeType,turnId);

@override
String toString() {
  return 'BridgeRunningWorkUnitActivity.active(turnId: $turnId)';
}


}

/// @nodoc
abstract mixin class $BridgeRunningWorkUnitActivity_ActiveCopyWith<$Res> implements $BridgeRunningWorkUnitActivityCopyWith<$Res> {
  factory $BridgeRunningWorkUnitActivity_ActiveCopyWith(BridgeRunningWorkUnitActivity_Active value, $Res Function(BridgeRunningWorkUnitActivity_Active) _then) = _$BridgeRunningWorkUnitActivity_ActiveCopyWithImpl;
@useResult
$Res call({
 String turnId
});




}
/// @nodoc
class _$BridgeRunningWorkUnitActivity_ActiveCopyWithImpl<$Res>
    implements $BridgeRunningWorkUnitActivity_ActiveCopyWith<$Res> {
  _$BridgeRunningWorkUnitActivity_ActiveCopyWithImpl(this._self, this._then);

  final BridgeRunningWorkUnitActivity_Active _self;
  final $Res Function(BridgeRunningWorkUnitActivity_Active) _then;

/// Create a copy of BridgeRunningWorkUnitActivity
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? turnId = null,}) {
  return _then(BridgeRunningWorkUnitActivity_Active(
turnId: null == turnId ? _self.turnId : turnId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeRuntimeState {

 Object get field0;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRuntimeState&&const DeepCollectionEquality().equals(other.field0, field0));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(field0));

@override
String toString() {
  return 'BridgeRuntimeState(field0: $field0)';
}


}

/// @nodoc
class $BridgeRuntimeStateCopyWith<$Res>  {
$BridgeRuntimeStateCopyWith(BridgeRuntimeState _, $Res Function(BridgeRuntimeState) __);
}


/// Adds pattern-matching-related methods to [BridgeRuntimeState].
extension BridgeRuntimeStatePatterns on BridgeRuntimeState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeRuntimeState_Uninitialized value)?  uninitialized,TResult Function( BridgeRuntimeState_Initializing value)?  initializing,TResult Function( BridgeRuntimeState_Ready value)?  ready,TResult Function( BridgeRuntimeState_ShuttingDown value)?  shuttingDown,TResult Function( BridgeRuntimeState_Stopped value)?  stopped,TResult Function( BridgeRuntimeState_Failed value)?  failed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeRuntimeState_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeRuntimeState_Initializing() when initializing != null:
return initializing(_that);case BridgeRuntimeState_Ready() when ready != null:
return ready(_that);case BridgeRuntimeState_ShuttingDown() when shuttingDown != null:
return shuttingDown(_that);case BridgeRuntimeState_Stopped() when stopped != null:
return stopped(_that);case BridgeRuntimeState_Failed() when failed != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeRuntimeState_Uninitialized value)  uninitialized,required TResult Function( BridgeRuntimeState_Initializing value)  initializing,required TResult Function( BridgeRuntimeState_Ready value)  ready,required TResult Function( BridgeRuntimeState_ShuttingDown value)  shuttingDown,required TResult Function( BridgeRuntimeState_Stopped value)  stopped,required TResult Function( BridgeRuntimeState_Failed value)  failed,}){
final _that = this;
switch (_that) {
case BridgeRuntimeState_Uninitialized():
return uninitialized(_that);case BridgeRuntimeState_Initializing():
return initializing(_that);case BridgeRuntimeState_Ready():
return ready(_that);case BridgeRuntimeState_ShuttingDown():
return shuttingDown(_that);case BridgeRuntimeState_Stopped():
return stopped(_that);case BridgeRuntimeState_Failed():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeRuntimeState_Uninitialized value)?  uninitialized,TResult? Function( BridgeRuntimeState_Initializing value)?  initializing,TResult? Function( BridgeRuntimeState_Ready value)?  ready,TResult? Function( BridgeRuntimeState_ShuttingDown value)?  shuttingDown,TResult? Function( BridgeRuntimeState_Stopped value)?  stopped,TResult? Function( BridgeRuntimeState_Failed value)?  failed,}){
final _that = this;
switch (_that) {
case BridgeRuntimeState_Uninitialized() when uninitialized != null:
return uninitialized(_that);case BridgeRuntimeState_Initializing() when initializing != null:
return initializing(_that);case BridgeRuntimeState_Ready() when ready != null:
return ready(_that);case BridgeRuntimeState_ShuttingDown() when shuttingDown != null:
return shuttingDown(_that);case BridgeRuntimeState_Stopped() when stopped != null:
return stopped(_that);case BridgeRuntimeState_Failed() when failed != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeRuntimeTimestamp field0)?  uninitialized,TResult Function( BridgeRuntimeTimestamp field0)?  initializing,TResult Function( BridgeRuntimeTimestamp field0)?  ready,TResult Function( BridgeRuntimeTimestamp field0)?  shuttingDown,TResult Function( BridgeRuntimeTimestamp field0)?  stopped,TResult Function( BridgeFailedRuntimeState field0)?  failed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeRuntimeState_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeRuntimeState_Initializing() when initializing != null:
return initializing(_that.field0);case BridgeRuntimeState_Ready() when ready != null:
return ready(_that.field0);case BridgeRuntimeState_ShuttingDown() when shuttingDown != null:
return shuttingDown(_that.field0);case BridgeRuntimeState_Stopped() when stopped != null:
return stopped(_that.field0);case BridgeRuntimeState_Failed() when failed != null:
return failed(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeRuntimeTimestamp field0)  uninitialized,required TResult Function( BridgeRuntimeTimestamp field0)  initializing,required TResult Function( BridgeRuntimeTimestamp field0)  ready,required TResult Function( BridgeRuntimeTimestamp field0)  shuttingDown,required TResult Function( BridgeRuntimeTimestamp field0)  stopped,required TResult Function( BridgeFailedRuntimeState field0)  failed,}) {final _that = this;
switch (_that) {
case BridgeRuntimeState_Uninitialized():
return uninitialized(_that.field0);case BridgeRuntimeState_Initializing():
return initializing(_that.field0);case BridgeRuntimeState_Ready():
return ready(_that.field0);case BridgeRuntimeState_ShuttingDown():
return shuttingDown(_that.field0);case BridgeRuntimeState_Stopped():
return stopped(_that.field0);case BridgeRuntimeState_Failed():
return failed(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeRuntimeTimestamp field0)?  uninitialized,TResult? Function( BridgeRuntimeTimestamp field0)?  initializing,TResult? Function( BridgeRuntimeTimestamp field0)?  ready,TResult? Function( BridgeRuntimeTimestamp field0)?  shuttingDown,TResult? Function( BridgeRuntimeTimestamp field0)?  stopped,TResult? Function( BridgeFailedRuntimeState field0)?  failed,}) {final _that = this;
switch (_that) {
case BridgeRuntimeState_Uninitialized() when uninitialized != null:
return uninitialized(_that.field0);case BridgeRuntimeState_Initializing() when initializing != null:
return initializing(_that.field0);case BridgeRuntimeState_Ready() when ready != null:
return ready(_that.field0);case BridgeRuntimeState_ShuttingDown() when shuttingDown != null:
return shuttingDown(_that.field0);case BridgeRuntimeState_Stopped() when stopped != null:
return stopped(_that.field0);case BridgeRuntimeState_Failed() when failed != null:
return failed(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeRuntimeState_Uninitialized extends BridgeRuntimeState {
  const BridgeRuntimeState_Uninitialized(this.field0): super._();


@override final  BridgeRuntimeTimestamp field0;

/// Create a copy of BridgeRuntimeState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRuntimeState_UninitializedCopyWith<BridgeRuntimeState_Uninitialized> get copyWith => _$BridgeRuntimeState_UninitializedCopyWithImpl<BridgeRuntimeState_Uninitialized>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRuntimeState_Uninitialized&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeRuntimeState.uninitialized(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeRuntimeState_UninitializedCopyWith<$Res> implements $BridgeRuntimeStateCopyWith<$Res> {
  factory $BridgeRuntimeState_UninitializedCopyWith(BridgeRuntimeState_Uninitialized value, $Res Function(BridgeRuntimeState_Uninitialized) _then) = _$BridgeRuntimeState_UninitializedCopyWithImpl;
@useResult
$Res call({
 BridgeRuntimeTimestamp field0
});




}
/// @nodoc
class _$BridgeRuntimeState_UninitializedCopyWithImpl<$Res>
    implements $BridgeRuntimeState_UninitializedCopyWith<$Res> {
  _$BridgeRuntimeState_UninitializedCopyWithImpl(this._self, this._then);

  final BridgeRuntimeState_Uninitialized _self;
  final $Res Function(BridgeRuntimeState_Uninitialized) _then;

/// Create a copy of BridgeRuntimeState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeRuntimeState_Uninitialized(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeRuntimeTimestamp,
  ));
}


}

/// @nodoc


class BridgeRuntimeState_Initializing extends BridgeRuntimeState {
  const BridgeRuntimeState_Initializing(this.field0): super._();


@override final  BridgeRuntimeTimestamp field0;

/// Create a copy of BridgeRuntimeState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRuntimeState_InitializingCopyWith<BridgeRuntimeState_Initializing> get copyWith => _$BridgeRuntimeState_InitializingCopyWithImpl<BridgeRuntimeState_Initializing>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRuntimeState_Initializing&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeRuntimeState.initializing(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeRuntimeState_InitializingCopyWith<$Res> implements $BridgeRuntimeStateCopyWith<$Res> {
  factory $BridgeRuntimeState_InitializingCopyWith(BridgeRuntimeState_Initializing value, $Res Function(BridgeRuntimeState_Initializing) _then) = _$BridgeRuntimeState_InitializingCopyWithImpl;
@useResult
$Res call({
 BridgeRuntimeTimestamp field0
});




}
/// @nodoc
class _$BridgeRuntimeState_InitializingCopyWithImpl<$Res>
    implements $BridgeRuntimeState_InitializingCopyWith<$Res> {
  _$BridgeRuntimeState_InitializingCopyWithImpl(this._self, this._then);

  final BridgeRuntimeState_Initializing _self;
  final $Res Function(BridgeRuntimeState_Initializing) _then;

/// Create a copy of BridgeRuntimeState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeRuntimeState_Initializing(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeRuntimeTimestamp,
  ));
}


}

/// @nodoc


class BridgeRuntimeState_Ready extends BridgeRuntimeState {
  const BridgeRuntimeState_Ready(this.field0): super._();


@override final  BridgeRuntimeTimestamp field0;

/// Create a copy of BridgeRuntimeState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRuntimeState_ReadyCopyWith<BridgeRuntimeState_Ready> get copyWith => _$BridgeRuntimeState_ReadyCopyWithImpl<BridgeRuntimeState_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRuntimeState_Ready&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeRuntimeState.ready(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeRuntimeState_ReadyCopyWith<$Res> implements $BridgeRuntimeStateCopyWith<$Res> {
  factory $BridgeRuntimeState_ReadyCopyWith(BridgeRuntimeState_Ready value, $Res Function(BridgeRuntimeState_Ready) _then) = _$BridgeRuntimeState_ReadyCopyWithImpl;
@useResult
$Res call({
 BridgeRuntimeTimestamp field0
});




}
/// @nodoc
class _$BridgeRuntimeState_ReadyCopyWithImpl<$Res>
    implements $BridgeRuntimeState_ReadyCopyWith<$Res> {
  _$BridgeRuntimeState_ReadyCopyWithImpl(this._self, this._then);

  final BridgeRuntimeState_Ready _self;
  final $Res Function(BridgeRuntimeState_Ready) _then;

/// Create a copy of BridgeRuntimeState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeRuntimeState_Ready(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeRuntimeTimestamp,
  ));
}


}

/// @nodoc


class BridgeRuntimeState_ShuttingDown extends BridgeRuntimeState {
  const BridgeRuntimeState_ShuttingDown(this.field0): super._();


@override final  BridgeRuntimeTimestamp field0;

/// Create a copy of BridgeRuntimeState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRuntimeState_ShuttingDownCopyWith<BridgeRuntimeState_ShuttingDown> get copyWith => _$BridgeRuntimeState_ShuttingDownCopyWithImpl<BridgeRuntimeState_ShuttingDown>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRuntimeState_ShuttingDown&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeRuntimeState.shuttingDown(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeRuntimeState_ShuttingDownCopyWith<$Res> implements $BridgeRuntimeStateCopyWith<$Res> {
  factory $BridgeRuntimeState_ShuttingDownCopyWith(BridgeRuntimeState_ShuttingDown value, $Res Function(BridgeRuntimeState_ShuttingDown) _then) = _$BridgeRuntimeState_ShuttingDownCopyWithImpl;
@useResult
$Res call({
 BridgeRuntimeTimestamp field0
});




}
/// @nodoc
class _$BridgeRuntimeState_ShuttingDownCopyWithImpl<$Res>
    implements $BridgeRuntimeState_ShuttingDownCopyWith<$Res> {
  _$BridgeRuntimeState_ShuttingDownCopyWithImpl(this._self, this._then);

  final BridgeRuntimeState_ShuttingDown _self;
  final $Res Function(BridgeRuntimeState_ShuttingDown) _then;

/// Create a copy of BridgeRuntimeState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeRuntimeState_ShuttingDown(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeRuntimeTimestamp,
  ));
}


}

/// @nodoc


class BridgeRuntimeState_Stopped extends BridgeRuntimeState {
  const BridgeRuntimeState_Stopped(this.field0): super._();


@override final  BridgeRuntimeTimestamp field0;

/// Create a copy of BridgeRuntimeState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRuntimeState_StoppedCopyWith<BridgeRuntimeState_Stopped> get copyWith => _$BridgeRuntimeState_StoppedCopyWithImpl<BridgeRuntimeState_Stopped>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRuntimeState_Stopped&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeRuntimeState.stopped(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeRuntimeState_StoppedCopyWith<$Res> implements $BridgeRuntimeStateCopyWith<$Res> {
  factory $BridgeRuntimeState_StoppedCopyWith(BridgeRuntimeState_Stopped value, $Res Function(BridgeRuntimeState_Stopped) _then) = _$BridgeRuntimeState_StoppedCopyWithImpl;
@useResult
$Res call({
 BridgeRuntimeTimestamp field0
});




}
/// @nodoc
class _$BridgeRuntimeState_StoppedCopyWithImpl<$Res>
    implements $BridgeRuntimeState_StoppedCopyWith<$Res> {
  _$BridgeRuntimeState_StoppedCopyWithImpl(this._self, this._then);

  final BridgeRuntimeState_Stopped _self;
  final $Res Function(BridgeRuntimeState_Stopped) _then;

/// Create a copy of BridgeRuntimeState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeRuntimeState_Stopped(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeRuntimeTimestamp,
  ));
}


}

/// @nodoc


class BridgeRuntimeState_Failed extends BridgeRuntimeState {
  const BridgeRuntimeState_Failed(this.field0): super._();


@override final  BridgeFailedRuntimeState field0;

/// Create a copy of BridgeRuntimeState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeRuntimeState_FailedCopyWith<BridgeRuntimeState_Failed> get copyWith => _$BridgeRuntimeState_FailedCopyWithImpl<BridgeRuntimeState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeRuntimeState_Failed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeRuntimeState.failed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeRuntimeState_FailedCopyWith<$Res> implements $BridgeRuntimeStateCopyWith<$Res> {
  factory $BridgeRuntimeState_FailedCopyWith(BridgeRuntimeState_Failed value, $Res Function(BridgeRuntimeState_Failed) _then) = _$BridgeRuntimeState_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeFailedRuntimeState field0
});




}
/// @nodoc
class _$BridgeRuntimeState_FailedCopyWithImpl<$Res>
    implements $BridgeRuntimeState_FailedCopyWith<$Res> {
  _$BridgeRuntimeState_FailedCopyWithImpl(this._self, this._then);

  final BridgeRuntimeState_Failed _self;
  final $Res Function(BridgeRuntimeState_Failed) _then;

/// Create a copy of BridgeRuntimeState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeRuntimeState_Failed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeFailedRuntimeState,
  ));
}


}

/// @nodoc
mixin _$BridgeTaskCompletionContent {

 Object get field0;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskCompletionContent&&const DeepCollectionEquality().equals(other.field0, field0));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(field0));

@override
String toString() {
  return 'BridgeTaskCompletionContent(field0: $field0)';
}


}

/// @nodoc
class $BridgeTaskCompletionContentCopyWith<$Res>  {
$BridgeTaskCompletionContentCopyWith(BridgeTaskCompletionContent _, $Res Function(BridgeTaskCompletionContent) __);
}


/// Adds pattern-matching-related methods to [BridgeTaskCompletionContent].
extension BridgeTaskCompletionContentPatterns on BridgeTaskCompletionContent {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTaskCompletionContent_Delivery value)?  delivery,TResult Function( BridgeTaskCompletionContent_NoDelivery value)?  noDelivery,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTaskCompletionContent_Delivery() when delivery != null:
return delivery(_that);case BridgeTaskCompletionContent_NoDelivery() when noDelivery != null:
return noDelivery(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTaskCompletionContent_Delivery value)  delivery,required TResult Function( BridgeTaskCompletionContent_NoDelivery value)  noDelivery,}){
final _that = this;
switch (_that) {
case BridgeTaskCompletionContent_Delivery():
return delivery(_that);case BridgeTaskCompletionContent_NoDelivery():
return noDelivery(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTaskCompletionContent_Delivery value)?  delivery,TResult? Function( BridgeTaskCompletionContent_NoDelivery value)?  noDelivery,}){
final _that = this;
switch (_that) {
case BridgeTaskCompletionContent_Delivery() when delivery != null:
return delivery(_that);case BridgeTaskCompletionContent_NoDelivery() when noDelivery != null:
return noDelivery(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeTaskDeliveryCompletion field0)?  delivery,TResult Function( BridgeTaskNoDeliveryCompletion field0)?  noDelivery,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTaskCompletionContent_Delivery() when delivery != null:
return delivery(_that.field0);case BridgeTaskCompletionContent_NoDelivery() when noDelivery != null:
return noDelivery(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeTaskDeliveryCompletion field0)  delivery,required TResult Function( BridgeTaskNoDeliveryCompletion field0)  noDelivery,}) {final _that = this;
switch (_that) {
case BridgeTaskCompletionContent_Delivery():
return delivery(_that.field0);case BridgeTaskCompletionContent_NoDelivery():
return noDelivery(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeTaskDeliveryCompletion field0)?  delivery,TResult? Function( BridgeTaskNoDeliveryCompletion field0)?  noDelivery,}) {final _that = this;
switch (_that) {
case BridgeTaskCompletionContent_Delivery() when delivery != null:
return delivery(_that.field0);case BridgeTaskCompletionContent_NoDelivery() when noDelivery != null:
return noDelivery(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTaskCompletionContent_Delivery extends BridgeTaskCompletionContent {
  const BridgeTaskCompletionContent_Delivery(this.field0): super._();


@override final  BridgeTaskDeliveryCompletion field0;

/// Create a copy of BridgeTaskCompletionContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskCompletionContent_DeliveryCopyWith<BridgeTaskCompletionContent_Delivery> get copyWith => _$BridgeTaskCompletionContent_DeliveryCopyWithImpl<BridgeTaskCompletionContent_Delivery>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskCompletionContent_Delivery&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskCompletionContent.delivery(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskCompletionContent_DeliveryCopyWith<$Res> implements $BridgeTaskCompletionContentCopyWith<$Res> {
  factory $BridgeTaskCompletionContent_DeliveryCopyWith(BridgeTaskCompletionContent_Delivery value, $Res Function(BridgeTaskCompletionContent_Delivery) _then) = _$BridgeTaskCompletionContent_DeliveryCopyWithImpl;
@useResult
$Res call({
 BridgeTaskDeliveryCompletion field0
});




}
/// @nodoc
class _$BridgeTaskCompletionContent_DeliveryCopyWithImpl<$Res>
    implements $BridgeTaskCompletionContent_DeliveryCopyWith<$Res> {
  _$BridgeTaskCompletionContent_DeliveryCopyWithImpl(this._self, this._then);

  final BridgeTaskCompletionContent_Delivery _self;
  final $Res Function(BridgeTaskCompletionContent_Delivery) _then;

/// Create a copy of BridgeTaskCompletionContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskCompletionContent_Delivery(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskDeliveryCompletion,
  ));
}


}

/// @nodoc


class BridgeTaskCompletionContent_NoDelivery extends BridgeTaskCompletionContent {
  const BridgeTaskCompletionContent_NoDelivery(this.field0): super._();


@override final  BridgeTaskNoDeliveryCompletion field0;

/// Create a copy of BridgeTaskCompletionContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskCompletionContent_NoDeliveryCopyWith<BridgeTaskCompletionContent_NoDelivery> get copyWith => _$BridgeTaskCompletionContent_NoDeliveryCopyWithImpl<BridgeTaskCompletionContent_NoDelivery>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskCompletionContent_NoDelivery&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskCompletionContent.noDelivery(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskCompletionContent_NoDeliveryCopyWith<$Res> implements $BridgeTaskCompletionContentCopyWith<$Res> {
  factory $BridgeTaskCompletionContent_NoDeliveryCopyWith(BridgeTaskCompletionContent_NoDelivery value, $Res Function(BridgeTaskCompletionContent_NoDelivery) _then) = _$BridgeTaskCompletionContent_NoDeliveryCopyWithImpl;
@useResult
$Res call({
 BridgeTaskNoDeliveryCompletion field0
});




}
/// @nodoc
class _$BridgeTaskCompletionContent_NoDeliveryCopyWithImpl<$Res>
    implements $BridgeTaskCompletionContent_NoDeliveryCopyWith<$Res> {
  _$BridgeTaskCompletionContent_NoDeliveryCopyWithImpl(this._self, this._then);

  final BridgeTaskCompletionContent_NoDelivery _self;
  final $Res Function(BridgeTaskCompletionContent_NoDelivery) _then;

/// Create a copy of BridgeTaskCompletionContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskCompletionContent_NoDelivery(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskNoDeliveryCompletion,
  ));
}


}

/// @nodoc
mixin _$BridgeTaskCompletionState {

 Object get field0;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskCompletionState&&const DeepCollectionEquality().equals(other.field0, field0));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(field0));

@override
String toString() {
  return 'BridgeTaskCompletionState(field0: $field0)';
}


}

/// @nodoc
class $BridgeTaskCompletionStateCopyWith<$Res>  {
$BridgeTaskCompletionStateCopyWith(BridgeTaskCompletionState _, $Res Function(BridgeTaskCompletionState) __);
}


/// Adds pattern-matching-related methods to [BridgeTaskCompletionState].
extension BridgeTaskCompletionStatePatterns on BridgeTaskCompletionState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTaskCompletionState_ReadyForReview value)?  readyForReview,TResult Function( BridgeTaskCompletionState_ChangesRequired value)?  changesRequired,TResult Function( BridgeTaskCompletionState_Approved value)?  approved,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTaskCompletionState_ReadyForReview() when readyForReview != null:
return readyForReview(_that);case BridgeTaskCompletionState_ChangesRequired() when changesRequired != null:
return changesRequired(_that);case BridgeTaskCompletionState_Approved() when approved != null:
return approved(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTaskCompletionState_ReadyForReview value)  readyForReview,required TResult Function( BridgeTaskCompletionState_ChangesRequired value)  changesRequired,required TResult Function( BridgeTaskCompletionState_Approved value)  approved,}){
final _that = this;
switch (_that) {
case BridgeTaskCompletionState_ReadyForReview():
return readyForReview(_that);case BridgeTaskCompletionState_ChangesRequired():
return changesRequired(_that);case BridgeTaskCompletionState_Approved():
return approved(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTaskCompletionState_ReadyForReview value)?  readyForReview,TResult? Function( BridgeTaskCompletionState_ChangesRequired value)?  changesRequired,TResult? Function( BridgeTaskCompletionState_Approved value)?  approved,}){
final _that = this;
switch (_that) {
case BridgeTaskCompletionState_ReadyForReview() when readyForReview != null:
return readyForReview(_that);case BridgeTaskCompletionState_ChangesRequired() when changesRequired != null:
return changesRequired(_that);case BridgeTaskCompletionState_Approved() when approved != null:
return approved(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeReadyForReviewCompletion field0)?  readyForReview,TResult Function( BridgeReviewedCompletion field0)?  changesRequired,TResult Function( BridgeReviewedCompletion field0)?  approved,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTaskCompletionState_ReadyForReview() when readyForReview != null:
return readyForReview(_that.field0);case BridgeTaskCompletionState_ChangesRequired() when changesRequired != null:
return changesRequired(_that.field0);case BridgeTaskCompletionState_Approved() when approved != null:
return approved(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeReadyForReviewCompletion field0)  readyForReview,required TResult Function( BridgeReviewedCompletion field0)  changesRequired,required TResult Function( BridgeReviewedCompletion field0)  approved,}) {final _that = this;
switch (_that) {
case BridgeTaskCompletionState_ReadyForReview():
return readyForReview(_that.field0);case BridgeTaskCompletionState_ChangesRequired():
return changesRequired(_that.field0);case BridgeTaskCompletionState_Approved():
return approved(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeReadyForReviewCompletion field0)?  readyForReview,TResult? Function( BridgeReviewedCompletion field0)?  changesRequired,TResult? Function( BridgeReviewedCompletion field0)?  approved,}) {final _that = this;
switch (_that) {
case BridgeTaskCompletionState_ReadyForReview() when readyForReview != null:
return readyForReview(_that.field0);case BridgeTaskCompletionState_ChangesRequired() when changesRequired != null:
return changesRequired(_that.field0);case BridgeTaskCompletionState_Approved() when approved != null:
return approved(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTaskCompletionState_ReadyForReview extends BridgeTaskCompletionState {
  const BridgeTaskCompletionState_ReadyForReview(this.field0): super._();


@override final  BridgeReadyForReviewCompletion field0;

/// Create a copy of BridgeTaskCompletionState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskCompletionState_ReadyForReviewCopyWith<BridgeTaskCompletionState_ReadyForReview> get copyWith => _$BridgeTaskCompletionState_ReadyForReviewCopyWithImpl<BridgeTaskCompletionState_ReadyForReview>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskCompletionState_ReadyForReview&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskCompletionState.readyForReview(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskCompletionState_ReadyForReviewCopyWith<$Res> implements $BridgeTaskCompletionStateCopyWith<$Res> {
  factory $BridgeTaskCompletionState_ReadyForReviewCopyWith(BridgeTaskCompletionState_ReadyForReview value, $Res Function(BridgeTaskCompletionState_ReadyForReview) _then) = _$BridgeTaskCompletionState_ReadyForReviewCopyWithImpl;
@useResult
$Res call({
 BridgeReadyForReviewCompletion field0
});




}
/// @nodoc
class _$BridgeTaskCompletionState_ReadyForReviewCopyWithImpl<$Res>
    implements $BridgeTaskCompletionState_ReadyForReviewCopyWith<$Res> {
  _$BridgeTaskCompletionState_ReadyForReviewCopyWithImpl(this._self, this._then);

  final BridgeTaskCompletionState_ReadyForReview _self;
  final $Res Function(BridgeTaskCompletionState_ReadyForReview) _then;

/// Create a copy of BridgeTaskCompletionState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskCompletionState_ReadyForReview(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeReadyForReviewCompletion,
  ));
}


}

/// @nodoc


class BridgeTaskCompletionState_ChangesRequired extends BridgeTaskCompletionState {
  const BridgeTaskCompletionState_ChangesRequired(this.field0): super._();


@override final  BridgeReviewedCompletion field0;

/// Create a copy of BridgeTaskCompletionState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskCompletionState_ChangesRequiredCopyWith<BridgeTaskCompletionState_ChangesRequired> get copyWith => _$BridgeTaskCompletionState_ChangesRequiredCopyWithImpl<BridgeTaskCompletionState_ChangesRequired>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskCompletionState_ChangesRequired&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskCompletionState.changesRequired(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskCompletionState_ChangesRequiredCopyWith<$Res> implements $BridgeTaskCompletionStateCopyWith<$Res> {
  factory $BridgeTaskCompletionState_ChangesRequiredCopyWith(BridgeTaskCompletionState_ChangesRequired value, $Res Function(BridgeTaskCompletionState_ChangesRequired) _then) = _$BridgeTaskCompletionState_ChangesRequiredCopyWithImpl;
@useResult
$Res call({
 BridgeReviewedCompletion field0
});




}
/// @nodoc
class _$BridgeTaskCompletionState_ChangesRequiredCopyWithImpl<$Res>
    implements $BridgeTaskCompletionState_ChangesRequiredCopyWith<$Res> {
  _$BridgeTaskCompletionState_ChangesRequiredCopyWithImpl(this._self, this._then);

  final BridgeTaskCompletionState_ChangesRequired _self;
  final $Res Function(BridgeTaskCompletionState_ChangesRequired) _then;

/// Create a copy of BridgeTaskCompletionState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskCompletionState_ChangesRequired(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeReviewedCompletion,
  ));
}


}

/// @nodoc


class BridgeTaskCompletionState_Approved extends BridgeTaskCompletionState {
  const BridgeTaskCompletionState_Approved(this.field0): super._();


@override final  BridgeReviewedCompletion field0;

/// Create a copy of BridgeTaskCompletionState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskCompletionState_ApprovedCopyWith<BridgeTaskCompletionState_Approved> get copyWith => _$BridgeTaskCompletionState_ApprovedCopyWithImpl<BridgeTaskCompletionState_Approved>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskCompletionState_Approved&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskCompletionState.approved(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskCompletionState_ApprovedCopyWith<$Res> implements $BridgeTaskCompletionStateCopyWith<$Res> {
  factory $BridgeTaskCompletionState_ApprovedCopyWith(BridgeTaskCompletionState_Approved value, $Res Function(BridgeTaskCompletionState_Approved) _then) = _$BridgeTaskCompletionState_ApprovedCopyWithImpl;
@useResult
$Res call({
 BridgeReviewedCompletion field0
});




}
/// @nodoc
class _$BridgeTaskCompletionState_ApprovedCopyWithImpl<$Res>
    implements $BridgeTaskCompletionState_ApprovedCopyWith<$Res> {
  _$BridgeTaskCompletionState_ApprovedCopyWithImpl(this._self, this._then);

  final BridgeTaskCompletionState_Approved _self;
  final $Res Function(BridgeTaskCompletionState_Approved) _then;

/// Create a copy of BridgeTaskCompletionState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskCompletionState_Approved(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeReviewedCompletion,
  ));
}


}

/// @nodoc
mixin _$BridgeTaskIssueState {

 BridgeTaskFailureDetail get failure;
/// Create a copy of BridgeTaskIssueState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskIssueStateCopyWith<BridgeTaskIssueState> get copyWith => _$BridgeTaskIssueStateCopyWithImpl<BridgeTaskIssueState>(this as BridgeTaskIssueState, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskIssueState&&(identical(other.failure, failure) || other.failure == failure));
}


@override
int get hashCode => Object.hash(runtimeType,failure);

@override
String toString() {
  return 'BridgeTaskIssueState(failure: $failure)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskIssueStateCopyWith<$Res>  {
  factory $BridgeTaskIssueStateCopyWith(BridgeTaskIssueState value, $Res Function(BridgeTaskIssueState) _then) = _$BridgeTaskIssueStateCopyWithImpl;
@useResult
$Res call({
 BridgeTaskFailureDetail failure
});




}
/// @nodoc
class _$BridgeTaskIssueStateCopyWithImpl<$Res>
    implements $BridgeTaskIssueStateCopyWith<$Res> {
  _$BridgeTaskIssueStateCopyWithImpl(this._self, this._then);

  final BridgeTaskIssueState _self;
  final $Res Function(BridgeTaskIssueState) _then;

/// Create a copy of BridgeTaskIssueState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? failure = null,}) {
  return _then(_self.copyWith(
failure: null == failure ? _self.failure : failure // ignore: cast_nullable_to_non_nullable
as BridgeTaskFailureDetail,
  ));
}

}


/// Adds pattern-matching-related methods to [BridgeTaskIssueState].
extension BridgeTaskIssueStatePatterns on BridgeTaskIssueState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTaskIssueState_OpenRecoverable value)?  openRecoverable,TResult Function( BridgeTaskIssueState_OpenFatal value)?  openFatal,TResult Function( BridgeTaskIssueState_Resolved value)?  resolved,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTaskIssueState_OpenRecoverable() when openRecoverable != null:
return openRecoverable(_that);case BridgeTaskIssueState_OpenFatal() when openFatal != null:
return openFatal(_that);case BridgeTaskIssueState_Resolved() when resolved != null:
return resolved(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTaskIssueState_OpenRecoverable value)  openRecoverable,required TResult Function( BridgeTaskIssueState_OpenFatal value)  openFatal,required TResult Function( BridgeTaskIssueState_Resolved value)  resolved,}){
final _that = this;
switch (_that) {
case BridgeTaskIssueState_OpenRecoverable():
return openRecoverable(_that);case BridgeTaskIssueState_OpenFatal():
return openFatal(_that);case BridgeTaskIssueState_Resolved():
return resolved(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTaskIssueState_OpenRecoverable value)?  openRecoverable,TResult? Function( BridgeTaskIssueState_OpenFatal value)?  openFatal,TResult? Function( BridgeTaskIssueState_Resolved value)?  resolved,}){
final _that = this;
switch (_that) {
case BridgeTaskIssueState_OpenRecoverable() when openRecoverable != null:
return openRecoverable(_that);case BridgeTaskIssueState_OpenFatal() when openFatal != null:
return openFatal(_that);case BridgeTaskIssueState_Resolved() when resolved != null:
return resolved(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeTaskFailureDetail failure)?  openRecoverable,TResult Function( BridgeTaskFailureDetail failure)?  openFatal,TResult Function( BridgeTaskFailureDetail failure,  PlatformInt64 resolvedAt)?  resolved,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTaskIssueState_OpenRecoverable() when openRecoverable != null:
return openRecoverable(_that.failure);case BridgeTaskIssueState_OpenFatal() when openFatal != null:
return openFatal(_that.failure);case BridgeTaskIssueState_Resolved() when resolved != null:
return resolved(_that.failure,_that.resolvedAt);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeTaskFailureDetail failure)  openRecoverable,required TResult Function( BridgeTaskFailureDetail failure)  openFatal,required TResult Function( BridgeTaskFailureDetail failure,  PlatformInt64 resolvedAt)  resolved,}) {final _that = this;
switch (_that) {
case BridgeTaskIssueState_OpenRecoverable():
return openRecoverable(_that.failure);case BridgeTaskIssueState_OpenFatal():
return openFatal(_that.failure);case BridgeTaskIssueState_Resolved():
return resolved(_that.failure,_that.resolvedAt);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeTaskFailureDetail failure)?  openRecoverable,TResult? Function( BridgeTaskFailureDetail failure)?  openFatal,TResult? Function( BridgeTaskFailureDetail failure,  PlatformInt64 resolvedAt)?  resolved,}) {final _that = this;
switch (_that) {
case BridgeTaskIssueState_OpenRecoverable() when openRecoverable != null:
return openRecoverable(_that.failure);case BridgeTaskIssueState_OpenFatal() when openFatal != null:
return openFatal(_that.failure);case BridgeTaskIssueState_Resolved() when resolved != null:
return resolved(_that.failure,_that.resolvedAt);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTaskIssueState_OpenRecoverable extends BridgeTaskIssueState {
  const BridgeTaskIssueState_OpenRecoverable({required this.failure}): super._();


@override final  BridgeTaskFailureDetail failure;

/// Create a copy of BridgeTaskIssueState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskIssueState_OpenRecoverableCopyWith<BridgeTaskIssueState_OpenRecoverable> get copyWith => _$BridgeTaskIssueState_OpenRecoverableCopyWithImpl<BridgeTaskIssueState_OpenRecoverable>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskIssueState_OpenRecoverable&&(identical(other.failure, failure) || other.failure == failure));
}


@override
int get hashCode => Object.hash(runtimeType,failure);

@override
String toString() {
  return 'BridgeTaskIssueState.openRecoverable(failure: $failure)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskIssueState_OpenRecoverableCopyWith<$Res> implements $BridgeTaskIssueStateCopyWith<$Res> {
  factory $BridgeTaskIssueState_OpenRecoverableCopyWith(BridgeTaskIssueState_OpenRecoverable value, $Res Function(BridgeTaskIssueState_OpenRecoverable) _then) = _$BridgeTaskIssueState_OpenRecoverableCopyWithImpl;
@override @useResult
$Res call({
 BridgeTaskFailureDetail failure
});




}
/// @nodoc
class _$BridgeTaskIssueState_OpenRecoverableCopyWithImpl<$Res>
    implements $BridgeTaskIssueState_OpenRecoverableCopyWith<$Res> {
  _$BridgeTaskIssueState_OpenRecoverableCopyWithImpl(this._self, this._then);

  final BridgeTaskIssueState_OpenRecoverable _self;
  final $Res Function(BridgeTaskIssueState_OpenRecoverable) _then;

/// Create a copy of BridgeTaskIssueState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? failure = null,}) {
  return _then(BridgeTaskIssueState_OpenRecoverable(
failure: null == failure ? _self.failure : failure // ignore: cast_nullable_to_non_nullable
as BridgeTaskFailureDetail,
  ));
}


}

/// @nodoc


class BridgeTaskIssueState_OpenFatal extends BridgeTaskIssueState {
  const BridgeTaskIssueState_OpenFatal({required this.failure}): super._();


@override final  BridgeTaskFailureDetail failure;

/// Create a copy of BridgeTaskIssueState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskIssueState_OpenFatalCopyWith<BridgeTaskIssueState_OpenFatal> get copyWith => _$BridgeTaskIssueState_OpenFatalCopyWithImpl<BridgeTaskIssueState_OpenFatal>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskIssueState_OpenFatal&&(identical(other.failure, failure) || other.failure == failure));
}


@override
int get hashCode => Object.hash(runtimeType,failure);

@override
String toString() {
  return 'BridgeTaskIssueState.openFatal(failure: $failure)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskIssueState_OpenFatalCopyWith<$Res> implements $BridgeTaskIssueStateCopyWith<$Res> {
  factory $BridgeTaskIssueState_OpenFatalCopyWith(BridgeTaskIssueState_OpenFatal value, $Res Function(BridgeTaskIssueState_OpenFatal) _then) = _$BridgeTaskIssueState_OpenFatalCopyWithImpl;
@override @useResult
$Res call({
 BridgeTaskFailureDetail failure
});




}
/// @nodoc
class _$BridgeTaskIssueState_OpenFatalCopyWithImpl<$Res>
    implements $BridgeTaskIssueState_OpenFatalCopyWith<$Res> {
  _$BridgeTaskIssueState_OpenFatalCopyWithImpl(this._self, this._then);

  final BridgeTaskIssueState_OpenFatal _self;
  final $Res Function(BridgeTaskIssueState_OpenFatal) _then;

/// Create a copy of BridgeTaskIssueState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? failure = null,}) {
  return _then(BridgeTaskIssueState_OpenFatal(
failure: null == failure ? _self.failure : failure // ignore: cast_nullable_to_non_nullable
as BridgeTaskFailureDetail,
  ));
}


}

/// @nodoc


class BridgeTaskIssueState_Resolved extends BridgeTaskIssueState {
  const BridgeTaskIssueState_Resolved({required this.failure, required this.resolvedAt}): super._();


@override final  BridgeTaskFailureDetail failure;
 final  PlatformInt64 resolvedAt;

/// Create a copy of BridgeTaskIssueState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskIssueState_ResolvedCopyWith<BridgeTaskIssueState_Resolved> get copyWith => _$BridgeTaskIssueState_ResolvedCopyWithImpl<BridgeTaskIssueState_Resolved>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskIssueState_Resolved&&(identical(other.failure, failure) || other.failure == failure)&&(identical(other.resolvedAt, resolvedAt) || other.resolvedAt == resolvedAt));
}


@override
int get hashCode => Object.hash(runtimeType,failure,resolvedAt);

@override
String toString() {
  return 'BridgeTaskIssueState.resolved(failure: $failure, resolvedAt: $resolvedAt)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskIssueState_ResolvedCopyWith<$Res> implements $BridgeTaskIssueStateCopyWith<$Res> {
  factory $BridgeTaskIssueState_ResolvedCopyWith(BridgeTaskIssueState_Resolved value, $Res Function(BridgeTaskIssueState_Resolved) _then) = _$BridgeTaskIssueState_ResolvedCopyWithImpl;
@override @useResult
$Res call({
 BridgeTaskFailureDetail failure, PlatformInt64 resolvedAt
});




}
/// @nodoc
class _$BridgeTaskIssueState_ResolvedCopyWithImpl<$Res>
    implements $BridgeTaskIssueState_ResolvedCopyWith<$Res> {
  _$BridgeTaskIssueState_ResolvedCopyWithImpl(this._self, this._then);

  final BridgeTaskIssueState_Resolved _self;
  final $Res Function(BridgeTaskIssueState_Resolved) _then;

/// Create a copy of BridgeTaskIssueState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? failure = null,Object? resolvedAt = null,}) {
  return _then(BridgeTaskIssueState_Resolved(
failure: null == failure ? _self.failure : failure // ignore: cast_nullable_to_non_nullable
as BridgeTaskFailureDetail,resolvedAt: null == resolvedAt ? _self.resolvedAt : resolvedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,
  ));
}


}

/// @nodoc
mixin _$BridgeTaskOutcome {

 String get summary; PlatformInt64 get completedAt;
/// Create a copy of BridgeTaskOutcome
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskOutcomeCopyWith<BridgeTaskOutcome> get copyWith => _$BridgeTaskOutcomeCopyWithImpl<BridgeTaskOutcome>(this as BridgeTaskOutcome, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskOutcome&&(identical(other.summary, summary) || other.summary == summary)&&(identical(other.completedAt, completedAt) || other.completedAt == completedAt));
}


@override
int get hashCode => Object.hash(runtimeType,summary,completedAt);

@override
String toString() {
  return 'BridgeTaskOutcome(summary: $summary, completedAt: $completedAt)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskOutcomeCopyWith<$Res>  {
  factory $BridgeTaskOutcomeCopyWith(BridgeTaskOutcome value, $Res Function(BridgeTaskOutcome) _then) = _$BridgeTaskOutcomeCopyWithImpl;
@useResult
$Res call({
 String summary, PlatformInt64 completedAt
});




}
/// @nodoc
class _$BridgeTaskOutcomeCopyWithImpl<$Res>
    implements $BridgeTaskOutcomeCopyWith<$Res> {
  _$BridgeTaskOutcomeCopyWithImpl(this._self, this._then);

  final BridgeTaskOutcome _self;
  final $Res Function(BridgeTaskOutcome) _then;

/// Create a copy of BridgeTaskOutcome
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? summary = null,Object? completedAt = null,}) {
  return _then(_self.copyWith(
summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as String,completedAt: null == completedAt ? _self.completedAt : completedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,
  ));
}

}


/// Adds pattern-matching-related methods to [BridgeTaskOutcome].
extension BridgeTaskOutcomePatterns on BridgeTaskOutcome {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTaskOutcome_Succeeded value)?  succeeded,TResult Function( BridgeTaskOutcome_Failed value)?  failed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTaskOutcome_Succeeded() when succeeded != null:
return succeeded(_that);case BridgeTaskOutcome_Failed() when failed != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTaskOutcome_Succeeded value)  succeeded,required TResult Function( BridgeTaskOutcome_Failed value)  failed,}){
final _that = this;
switch (_that) {
case BridgeTaskOutcome_Succeeded():
return succeeded(_that);case BridgeTaskOutcome_Failed():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTaskOutcome_Succeeded value)?  succeeded,TResult? Function( BridgeTaskOutcome_Failed value)?  failed,}){
final _that = this;
switch (_that) {
case BridgeTaskOutcome_Succeeded() when succeeded != null:
return succeeded(_that);case BridgeTaskOutcome_Failed() when failed != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String summary,  PlatformInt64 completedAt,  BridgeTaskReviewGate reviewGate)?  succeeded,TResult Function( BridgeTaskFailureKind kind,  String summary,  String evidence,  String cause,  PlatformInt64 completedAt)?  failed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTaskOutcome_Succeeded() when succeeded != null:
return succeeded(_that.summary,_that.completedAt,_that.reviewGate);case BridgeTaskOutcome_Failed() when failed != null:
return failed(_that.kind,_that.summary,_that.evidence,_that.cause,_that.completedAt);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String summary,  PlatformInt64 completedAt,  BridgeTaskReviewGate reviewGate)  succeeded,required TResult Function( BridgeTaskFailureKind kind,  String summary,  String evidence,  String cause,  PlatformInt64 completedAt)  failed,}) {final _that = this;
switch (_that) {
case BridgeTaskOutcome_Succeeded():
return succeeded(_that.summary,_that.completedAt,_that.reviewGate);case BridgeTaskOutcome_Failed():
return failed(_that.kind,_that.summary,_that.evidence,_that.cause,_that.completedAt);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String summary,  PlatformInt64 completedAt,  BridgeTaskReviewGate reviewGate)?  succeeded,TResult? Function( BridgeTaskFailureKind kind,  String summary,  String evidence,  String cause,  PlatformInt64 completedAt)?  failed,}) {final _that = this;
switch (_that) {
case BridgeTaskOutcome_Succeeded() when succeeded != null:
return succeeded(_that.summary,_that.completedAt,_that.reviewGate);case BridgeTaskOutcome_Failed() when failed != null:
return failed(_that.kind,_that.summary,_that.evidence,_that.cause,_that.completedAt);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTaskOutcome_Succeeded extends BridgeTaskOutcome {
  const BridgeTaskOutcome_Succeeded({required this.summary, required this.completedAt, required this.reviewGate}): super._();


@override final  String summary;
@override final  PlatformInt64 completedAt;
 final  BridgeTaskReviewGate reviewGate;

/// Create a copy of BridgeTaskOutcome
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskOutcome_SucceededCopyWith<BridgeTaskOutcome_Succeeded> get copyWith => _$BridgeTaskOutcome_SucceededCopyWithImpl<BridgeTaskOutcome_Succeeded>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskOutcome_Succeeded&&(identical(other.summary, summary) || other.summary == summary)&&(identical(other.completedAt, completedAt) || other.completedAt == completedAt)&&(identical(other.reviewGate, reviewGate) || other.reviewGate == reviewGate));
}


@override
int get hashCode => Object.hash(runtimeType,summary,completedAt,reviewGate);

@override
String toString() {
  return 'BridgeTaskOutcome.succeeded(summary: $summary, completedAt: $completedAt, reviewGate: $reviewGate)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskOutcome_SucceededCopyWith<$Res> implements $BridgeTaskOutcomeCopyWith<$Res> {
  factory $BridgeTaskOutcome_SucceededCopyWith(BridgeTaskOutcome_Succeeded value, $Res Function(BridgeTaskOutcome_Succeeded) _then) = _$BridgeTaskOutcome_SucceededCopyWithImpl;
@override @useResult
$Res call({
 String summary, PlatformInt64 completedAt, BridgeTaskReviewGate reviewGate
});


$BridgeTaskReviewGateCopyWith<$Res> get reviewGate;

}
/// @nodoc
class _$BridgeTaskOutcome_SucceededCopyWithImpl<$Res>
    implements $BridgeTaskOutcome_SucceededCopyWith<$Res> {
  _$BridgeTaskOutcome_SucceededCopyWithImpl(this._self, this._then);

  final BridgeTaskOutcome_Succeeded _self;
  final $Res Function(BridgeTaskOutcome_Succeeded) _then;

/// Create a copy of BridgeTaskOutcome
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? summary = null,Object? completedAt = null,Object? reviewGate = null,}) {
  return _then(BridgeTaskOutcome_Succeeded(
summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as String,completedAt: null == completedAt ? _self.completedAt : completedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,reviewGate: null == reviewGate ? _self.reviewGate : reviewGate // ignore: cast_nullable_to_non_nullable
as BridgeTaskReviewGate,
  ));
}

/// Create a copy of BridgeTaskOutcome
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeTaskReviewGateCopyWith<$Res> get reviewGate {

  return $BridgeTaskReviewGateCopyWith<$Res>(_self.reviewGate, (value) {
    return _then(_self.copyWith(reviewGate: value));
  });
}
}

/// @nodoc


class BridgeTaskOutcome_Failed extends BridgeTaskOutcome {
  const BridgeTaskOutcome_Failed({required this.kind, required this.summary, required this.evidence, required this.cause, required this.completedAt}): super._();


 final  BridgeTaskFailureKind kind;
@override final  String summary;
 final  String evidence;
 final  String cause;
@override final  PlatformInt64 completedAt;

/// Create a copy of BridgeTaskOutcome
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskOutcome_FailedCopyWith<BridgeTaskOutcome_Failed> get copyWith => _$BridgeTaskOutcome_FailedCopyWithImpl<BridgeTaskOutcome_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskOutcome_Failed&&(identical(other.kind, kind) || other.kind == kind)&&(identical(other.summary, summary) || other.summary == summary)&&(identical(other.evidence, evidence) || other.evidence == evidence)&&(identical(other.cause, cause) || other.cause == cause)&&(identical(other.completedAt, completedAt) || other.completedAt == completedAt));
}


@override
int get hashCode => Object.hash(runtimeType,kind,summary,evidence,cause,completedAt);

@override
String toString() {
  return 'BridgeTaskOutcome.failed(kind: $kind, summary: $summary, evidence: $evidence, cause: $cause, completedAt: $completedAt)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskOutcome_FailedCopyWith<$Res> implements $BridgeTaskOutcomeCopyWith<$Res> {
  factory $BridgeTaskOutcome_FailedCopyWith(BridgeTaskOutcome_Failed value, $Res Function(BridgeTaskOutcome_Failed) _then) = _$BridgeTaskOutcome_FailedCopyWithImpl;
@override @useResult
$Res call({
 BridgeTaskFailureKind kind, String summary, String evidence, String cause, PlatformInt64 completedAt
});




}
/// @nodoc
class _$BridgeTaskOutcome_FailedCopyWithImpl<$Res>
    implements $BridgeTaskOutcome_FailedCopyWith<$Res> {
  _$BridgeTaskOutcome_FailedCopyWithImpl(this._self, this._then);

  final BridgeTaskOutcome_Failed _self;
  final $Res Function(BridgeTaskOutcome_Failed) _then;

/// Create a copy of BridgeTaskOutcome
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? kind = null,Object? summary = null,Object? evidence = null,Object? cause = null,Object? completedAt = null,}) {
  return _then(BridgeTaskOutcome_Failed(
kind: null == kind ? _self.kind : kind // ignore: cast_nullable_to_non_nullable
as BridgeTaskFailureKind,summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as String,evidence: null == evidence ? _self.evidence : evidence // ignore: cast_nullable_to_non_nullable
as String,cause: null == cause ? _self.cause : cause // ignore: cast_nullable_to_non_nullable
as String,completedAt: null == completedAt ? _self.completedAt : completedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,
  ));
}


}

/// @nodoc
mixin _$BridgeTaskReviewGate {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewGate);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeTaskReviewGate()';
}


}

/// @nodoc
class $BridgeTaskReviewGateCopyWith<$Res>  {
$BridgeTaskReviewGateCopyWith(BridgeTaskReviewGate _, $Res Function(BridgeTaskReviewGate) __);
}


/// Adds pattern-matching-related methods to [BridgeTaskReviewGate].
extension BridgeTaskReviewGatePatterns on BridgeTaskReviewGate {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTaskReviewGate_NotRequiredNoDelivery value)?  notRequiredNoDelivery,TResult Function( BridgeTaskReviewGate_NotRequiredSingleExecutor value)?  notRequiredSingleExecutor,TResult Function( BridgeTaskReviewGate_IntegratedReview value)?  integratedReview,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTaskReviewGate_NotRequiredNoDelivery() when notRequiredNoDelivery != null:
return notRequiredNoDelivery(_that);case BridgeTaskReviewGate_NotRequiredSingleExecutor() when notRequiredSingleExecutor != null:
return notRequiredSingleExecutor(_that);case BridgeTaskReviewGate_IntegratedReview() when integratedReview != null:
return integratedReview(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTaskReviewGate_NotRequiredNoDelivery value)  notRequiredNoDelivery,required TResult Function( BridgeTaskReviewGate_NotRequiredSingleExecutor value)  notRequiredSingleExecutor,required TResult Function( BridgeTaskReviewGate_IntegratedReview value)  integratedReview,}){
final _that = this;
switch (_that) {
case BridgeTaskReviewGate_NotRequiredNoDelivery():
return notRequiredNoDelivery(_that);case BridgeTaskReviewGate_NotRequiredSingleExecutor():
return notRequiredSingleExecutor(_that);case BridgeTaskReviewGate_IntegratedReview():
return integratedReview(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTaskReviewGate_NotRequiredNoDelivery value)?  notRequiredNoDelivery,TResult? Function( BridgeTaskReviewGate_NotRequiredSingleExecutor value)?  notRequiredSingleExecutor,TResult? Function( BridgeTaskReviewGate_IntegratedReview value)?  integratedReview,}){
final _that = this;
switch (_that) {
case BridgeTaskReviewGate_NotRequiredNoDelivery() when notRequiredNoDelivery != null:
return notRequiredNoDelivery(_that);case BridgeTaskReviewGate_NotRequiredSingleExecutor() when notRequiredSingleExecutor != null:
return notRequiredSingleExecutor(_that);case BridgeTaskReviewGate_IntegratedReview() when integratedReview != null:
return integratedReview(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  notRequiredNoDelivery,TResult Function( String workUnitId)?  notRequiredSingleExecutor,TResult Function( String reviewRoundId)?  integratedReview,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTaskReviewGate_NotRequiredNoDelivery() when notRequiredNoDelivery != null:
return notRequiredNoDelivery();case BridgeTaskReviewGate_NotRequiredSingleExecutor() when notRequiredSingleExecutor != null:
return notRequiredSingleExecutor(_that.workUnitId);case BridgeTaskReviewGate_IntegratedReview() when integratedReview != null:
return integratedReview(_that.reviewRoundId);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  notRequiredNoDelivery,required TResult Function( String workUnitId)  notRequiredSingleExecutor,required TResult Function( String reviewRoundId)  integratedReview,}) {final _that = this;
switch (_that) {
case BridgeTaskReviewGate_NotRequiredNoDelivery():
return notRequiredNoDelivery();case BridgeTaskReviewGate_NotRequiredSingleExecutor():
return notRequiredSingleExecutor(_that.workUnitId);case BridgeTaskReviewGate_IntegratedReview():
return integratedReview(_that.reviewRoundId);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  notRequiredNoDelivery,TResult? Function( String workUnitId)?  notRequiredSingleExecutor,TResult? Function( String reviewRoundId)?  integratedReview,}) {final _that = this;
switch (_that) {
case BridgeTaskReviewGate_NotRequiredNoDelivery() when notRequiredNoDelivery != null:
return notRequiredNoDelivery();case BridgeTaskReviewGate_NotRequiredSingleExecutor() when notRequiredSingleExecutor != null:
return notRequiredSingleExecutor(_that.workUnitId);case BridgeTaskReviewGate_IntegratedReview() when integratedReview != null:
return integratedReview(_that.reviewRoundId);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTaskReviewGate_NotRequiredNoDelivery extends BridgeTaskReviewGate {
  const BridgeTaskReviewGate_NotRequiredNoDelivery(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewGate_NotRequiredNoDelivery);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeTaskReviewGate.notRequiredNoDelivery()';
}


}




/// @nodoc


class BridgeTaskReviewGate_NotRequiredSingleExecutor extends BridgeTaskReviewGate {
  const BridgeTaskReviewGate_NotRequiredSingleExecutor({required this.workUnitId}): super._();


 final  String workUnitId;

/// Create a copy of BridgeTaskReviewGate
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewGate_NotRequiredSingleExecutorCopyWith<BridgeTaskReviewGate_NotRequiredSingleExecutor> get copyWith => _$BridgeTaskReviewGate_NotRequiredSingleExecutorCopyWithImpl<BridgeTaskReviewGate_NotRequiredSingleExecutor>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewGate_NotRequiredSingleExecutor&&(identical(other.workUnitId, workUnitId) || other.workUnitId == workUnitId));
}


@override
int get hashCode => Object.hash(runtimeType,workUnitId);

@override
String toString() {
  return 'BridgeTaskReviewGate.notRequiredSingleExecutor(workUnitId: $workUnitId)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewGate_NotRequiredSingleExecutorCopyWith<$Res> implements $BridgeTaskReviewGateCopyWith<$Res> {
  factory $BridgeTaskReviewGate_NotRequiredSingleExecutorCopyWith(BridgeTaskReviewGate_NotRequiredSingleExecutor value, $Res Function(BridgeTaskReviewGate_NotRequiredSingleExecutor) _then) = _$BridgeTaskReviewGate_NotRequiredSingleExecutorCopyWithImpl;
@useResult
$Res call({
 String workUnitId
});




}
/// @nodoc
class _$BridgeTaskReviewGate_NotRequiredSingleExecutorCopyWithImpl<$Res>
    implements $BridgeTaskReviewGate_NotRequiredSingleExecutorCopyWith<$Res> {
  _$BridgeTaskReviewGate_NotRequiredSingleExecutorCopyWithImpl(this._self, this._then);

  final BridgeTaskReviewGate_NotRequiredSingleExecutor _self;
  final $Res Function(BridgeTaskReviewGate_NotRequiredSingleExecutor) _then;

/// Create a copy of BridgeTaskReviewGate
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? workUnitId = null,}) {
  return _then(BridgeTaskReviewGate_NotRequiredSingleExecutor(
workUnitId: null == workUnitId ? _self.workUnitId : workUnitId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeTaskReviewGate_IntegratedReview extends BridgeTaskReviewGate {
  const BridgeTaskReviewGate_IntegratedReview({required this.reviewRoundId}): super._();


 final  String reviewRoundId;

/// Create a copy of BridgeTaskReviewGate
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewGate_IntegratedReviewCopyWith<BridgeTaskReviewGate_IntegratedReview> get copyWith => _$BridgeTaskReviewGate_IntegratedReviewCopyWithImpl<BridgeTaskReviewGate_IntegratedReview>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewGate_IntegratedReview&&(identical(other.reviewRoundId, reviewRoundId) || other.reviewRoundId == reviewRoundId));
}


@override
int get hashCode => Object.hash(runtimeType,reviewRoundId);

@override
String toString() {
  return 'BridgeTaskReviewGate.integratedReview(reviewRoundId: $reviewRoundId)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewGate_IntegratedReviewCopyWith<$Res> implements $BridgeTaskReviewGateCopyWith<$Res> {
  factory $BridgeTaskReviewGate_IntegratedReviewCopyWith(BridgeTaskReviewGate_IntegratedReview value, $Res Function(BridgeTaskReviewGate_IntegratedReview) _then) = _$BridgeTaskReviewGate_IntegratedReviewCopyWithImpl;
@useResult
$Res call({
 String reviewRoundId
});




}
/// @nodoc
class _$BridgeTaskReviewGate_IntegratedReviewCopyWithImpl<$Res>
    implements $BridgeTaskReviewGate_IntegratedReviewCopyWith<$Res> {
  _$BridgeTaskReviewGate_IntegratedReviewCopyWithImpl(this._self, this._then);

  final BridgeTaskReviewGate_IntegratedReview _self;
  final $Res Function(BridgeTaskReviewGate_IntegratedReview) _then;

/// Create a copy of BridgeTaskReviewGate
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reviewRoundId = null,}) {
  return _then(BridgeTaskReviewGate_IntegratedReview(
reviewRoundId: null == reviewRoundId ? _self.reviewRoundId : reviewRoundId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeTaskReviewState {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeTaskReviewState()';
}


}

/// @nodoc
class $BridgeTaskReviewStateCopyWith<$Res>  {
$BridgeTaskReviewStateCopyWith(BridgeTaskReviewState _, $Res Function(BridgeTaskReviewState) __);
}


/// Adds pattern-matching-related methods to [BridgeTaskReviewState].
extension BridgeTaskReviewStatePatterns on BridgeTaskReviewState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTaskReviewState_PendingDispatch value)?  pendingDispatch,TResult Function( BridgeTaskReviewState_Dispatched value)?  dispatched,TResult Function( BridgeTaskReviewState_Running value)?  running,TResult Function( BridgeTaskReviewState_Passed value)?  passed,TResult Function( BridgeTaskReviewState_ChangesRequired value)?  changesRequired,TResult Function( BridgeTaskReviewState_Blocked value)?  blocked,TResult Function( BridgeTaskReviewState_Failed value)?  failed,TResult Function( BridgeTaskReviewState_Cancelled value)?  cancelled,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTaskReviewState_PendingDispatch() when pendingDispatch != null:
return pendingDispatch(_that);case BridgeTaskReviewState_Dispatched() when dispatched != null:
return dispatched(_that);case BridgeTaskReviewState_Running() when running != null:
return running(_that);case BridgeTaskReviewState_Passed() when passed != null:
return passed(_that);case BridgeTaskReviewState_ChangesRequired() when changesRequired != null:
return changesRequired(_that);case BridgeTaskReviewState_Blocked() when blocked != null:
return blocked(_that);case BridgeTaskReviewState_Failed() when failed != null:
return failed(_that);case BridgeTaskReviewState_Cancelled() when cancelled != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTaskReviewState_PendingDispatch value)  pendingDispatch,required TResult Function( BridgeTaskReviewState_Dispatched value)  dispatched,required TResult Function( BridgeTaskReviewState_Running value)  running,required TResult Function( BridgeTaskReviewState_Passed value)  passed,required TResult Function( BridgeTaskReviewState_ChangesRequired value)  changesRequired,required TResult Function( BridgeTaskReviewState_Blocked value)  blocked,required TResult Function( BridgeTaskReviewState_Failed value)  failed,required TResult Function( BridgeTaskReviewState_Cancelled value)  cancelled,}){
final _that = this;
switch (_that) {
case BridgeTaskReviewState_PendingDispatch():
return pendingDispatch(_that);case BridgeTaskReviewState_Dispatched():
return dispatched(_that);case BridgeTaskReviewState_Running():
return running(_that);case BridgeTaskReviewState_Passed():
return passed(_that);case BridgeTaskReviewState_ChangesRequired():
return changesRequired(_that);case BridgeTaskReviewState_Blocked():
return blocked(_that);case BridgeTaskReviewState_Failed():
return failed(_that);case BridgeTaskReviewState_Cancelled():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTaskReviewState_PendingDispatch value)?  pendingDispatch,TResult? Function( BridgeTaskReviewState_Dispatched value)?  dispatched,TResult? Function( BridgeTaskReviewState_Running value)?  running,TResult? Function( BridgeTaskReviewState_Passed value)?  passed,TResult? Function( BridgeTaskReviewState_ChangesRequired value)?  changesRequired,TResult? Function( BridgeTaskReviewState_Blocked value)?  blocked,TResult? Function( BridgeTaskReviewState_Failed value)?  failed,TResult? Function( BridgeTaskReviewState_Cancelled value)?  cancelled,}){
final _that = this;
switch (_that) {
case BridgeTaskReviewState_PendingDispatch() when pendingDispatch != null:
return pendingDispatch(_that);case BridgeTaskReviewState_Dispatched() when dispatched != null:
return dispatched(_that);case BridgeTaskReviewState_Running() when running != null:
return running(_that);case BridgeTaskReviewState_Passed() when passed != null:
return passed(_that);case BridgeTaskReviewState_ChangesRequired() when changesRequired != null:
return changesRequired(_that);case BridgeTaskReviewState_Blocked() when blocked != null:
return blocked(_that);case BridgeTaskReviewState_Failed() when failed != null:
return failed(_that);case BridgeTaskReviewState_Cancelled() when cancelled != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  pendingDispatch,TResult Function( String reviewerAgentId)?  dispatched,TResult Function( String reviewerAgentId)?  running,TResult Function( String reviewerAgentId,  String summary)?  passed,TResult Function( String reviewerAgentId,  String summary)?  changesRequired,TResult Function( String reviewerAgentId,  String summary)?  blocked,TResult Function( String? reviewerAgentId,  String error,  String summary)?  failed,TResult Function( String? reviewerAgentId,  String reason,  String summary)?  cancelled,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTaskReviewState_PendingDispatch() when pendingDispatch != null:
return pendingDispatch();case BridgeTaskReviewState_Dispatched() when dispatched != null:
return dispatched(_that.reviewerAgentId);case BridgeTaskReviewState_Running() when running != null:
return running(_that.reviewerAgentId);case BridgeTaskReviewState_Passed() when passed != null:
return passed(_that.reviewerAgentId,_that.summary);case BridgeTaskReviewState_ChangesRequired() when changesRequired != null:
return changesRequired(_that.reviewerAgentId,_that.summary);case BridgeTaskReviewState_Blocked() when blocked != null:
return blocked(_that.reviewerAgentId,_that.summary);case BridgeTaskReviewState_Failed() when failed != null:
return failed(_that.reviewerAgentId,_that.error,_that.summary);case BridgeTaskReviewState_Cancelled() when cancelled != null:
return cancelled(_that.reviewerAgentId,_that.reason,_that.summary);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  pendingDispatch,required TResult Function( String reviewerAgentId)  dispatched,required TResult Function( String reviewerAgentId)  running,required TResult Function( String reviewerAgentId,  String summary)  passed,required TResult Function( String reviewerAgentId,  String summary)  changesRequired,required TResult Function( String reviewerAgentId,  String summary)  blocked,required TResult Function( String? reviewerAgentId,  String error,  String summary)  failed,required TResult Function( String? reviewerAgentId,  String reason,  String summary)  cancelled,}) {final _that = this;
switch (_that) {
case BridgeTaskReviewState_PendingDispatch():
return pendingDispatch();case BridgeTaskReviewState_Dispatched():
return dispatched(_that.reviewerAgentId);case BridgeTaskReviewState_Running():
return running(_that.reviewerAgentId);case BridgeTaskReviewState_Passed():
return passed(_that.reviewerAgentId,_that.summary);case BridgeTaskReviewState_ChangesRequired():
return changesRequired(_that.reviewerAgentId,_that.summary);case BridgeTaskReviewState_Blocked():
return blocked(_that.reviewerAgentId,_that.summary);case BridgeTaskReviewState_Failed():
return failed(_that.reviewerAgentId,_that.error,_that.summary);case BridgeTaskReviewState_Cancelled():
return cancelled(_that.reviewerAgentId,_that.reason,_that.summary);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  pendingDispatch,TResult? Function( String reviewerAgentId)?  dispatched,TResult? Function( String reviewerAgentId)?  running,TResult? Function( String reviewerAgentId,  String summary)?  passed,TResult? Function( String reviewerAgentId,  String summary)?  changesRequired,TResult? Function( String reviewerAgentId,  String summary)?  blocked,TResult? Function( String? reviewerAgentId,  String error,  String summary)?  failed,TResult? Function( String? reviewerAgentId,  String reason,  String summary)?  cancelled,}) {final _that = this;
switch (_that) {
case BridgeTaskReviewState_PendingDispatch() when pendingDispatch != null:
return pendingDispatch();case BridgeTaskReviewState_Dispatched() when dispatched != null:
return dispatched(_that.reviewerAgentId);case BridgeTaskReviewState_Running() when running != null:
return running(_that.reviewerAgentId);case BridgeTaskReviewState_Passed() when passed != null:
return passed(_that.reviewerAgentId,_that.summary);case BridgeTaskReviewState_ChangesRequired() when changesRequired != null:
return changesRequired(_that.reviewerAgentId,_that.summary);case BridgeTaskReviewState_Blocked() when blocked != null:
return blocked(_that.reviewerAgentId,_that.summary);case BridgeTaskReviewState_Failed() when failed != null:
return failed(_that.reviewerAgentId,_that.error,_that.summary);case BridgeTaskReviewState_Cancelled() when cancelled != null:
return cancelled(_that.reviewerAgentId,_that.reason,_that.summary);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTaskReviewState_PendingDispatch extends BridgeTaskReviewState {
  const BridgeTaskReviewState_PendingDispatch(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewState_PendingDispatch);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeTaskReviewState.pendingDispatch()';
}


}




/// @nodoc


class BridgeTaskReviewState_Dispatched extends BridgeTaskReviewState {
  const BridgeTaskReviewState_Dispatched({required this.reviewerAgentId}): super._();


 final  String reviewerAgentId;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewState_DispatchedCopyWith<BridgeTaskReviewState_Dispatched> get copyWith => _$BridgeTaskReviewState_DispatchedCopyWithImpl<BridgeTaskReviewState_Dispatched>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewState_Dispatched&&(identical(other.reviewerAgentId, reviewerAgentId) || other.reviewerAgentId == reviewerAgentId));
}


@override
int get hashCode => Object.hash(runtimeType,reviewerAgentId);

@override
String toString() {
  return 'BridgeTaskReviewState.dispatched(reviewerAgentId: $reviewerAgentId)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewState_DispatchedCopyWith<$Res> implements $BridgeTaskReviewStateCopyWith<$Res> {
  factory $BridgeTaskReviewState_DispatchedCopyWith(BridgeTaskReviewState_Dispatched value, $Res Function(BridgeTaskReviewState_Dispatched) _then) = _$BridgeTaskReviewState_DispatchedCopyWithImpl;
@useResult
$Res call({
 String reviewerAgentId
});




}
/// @nodoc
class _$BridgeTaskReviewState_DispatchedCopyWithImpl<$Res>
    implements $BridgeTaskReviewState_DispatchedCopyWith<$Res> {
  _$BridgeTaskReviewState_DispatchedCopyWithImpl(this._self, this._then);

  final BridgeTaskReviewState_Dispatched _self;
  final $Res Function(BridgeTaskReviewState_Dispatched) _then;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reviewerAgentId = null,}) {
  return _then(BridgeTaskReviewState_Dispatched(
reviewerAgentId: null == reviewerAgentId ? _self.reviewerAgentId : reviewerAgentId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeTaskReviewState_Running extends BridgeTaskReviewState {
  const BridgeTaskReviewState_Running({required this.reviewerAgentId}): super._();


 final  String reviewerAgentId;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewState_RunningCopyWith<BridgeTaskReviewState_Running> get copyWith => _$BridgeTaskReviewState_RunningCopyWithImpl<BridgeTaskReviewState_Running>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewState_Running&&(identical(other.reviewerAgentId, reviewerAgentId) || other.reviewerAgentId == reviewerAgentId));
}


@override
int get hashCode => Object.hash(runtimeType,reviewerAgentId);

@override
String toString() {
  return 'BridgeTaskReviewState.running(reviewerAgentId: $reviewerAgentId)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewState_RunningCopyWith<$Res> implements $BridgeTaskReviewStateCopyWith<$Res> {
  factory $BridgeTaskReviewState_RunningCopyWith(BridgeTaskReviewState_Running value, $Res Function(BridgeTaskReviewState_Running) _then) = _$BridgeTaskReviewState_RunningCopyWithImpl;
@useResult
$Res call({
 String reviewerAgentId
});




}
/// @nodoc
class _$BridgeTaskReviewState_RunningCopyWithImpl<$Res>
    implements $BridgeTaskReviewState_RunningCopyWith<$Res> {
  _$BridgeTaskReviewState_RunningCopyWithImpl(this._self, this._then);

  final BridgeTaskReviewState_Running _self;
  final $Res Function(BridgeTaskReviewState_Running) _then;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reviewerAgentId = null,}) {
  return _then(BridgeTaskReviewState_Running(
reviewerAgentId: null == reviewerAgentId ? _self.reviewerAgentId : reviewerAgentId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeTaskReviewState_Passed extends BridgeTaskReviewState {
  const BridgeTaskReviewState_Passed({required this.reviewerAgentId, required this.summary}): super._();


 final  String reviewerAgentId;
 final  String summary;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewState_PassedCopyWith<BridgeTaskReviewState_Passed> get copyWith => _$BridgeTaskReviewState_PassedCopyWithImpl<BridgeTaskReviewState_Passed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewState_Passed&&(identical(other.reviewerAgentId, reviewerAgentId) || other.reviewerAgentId == reviewerAgentId)&&(identical(other.summary, summary) || other.summary == summary));
}


@override
int get hashCode => Object.hash(runtimeType,reviewerAgentId,summary);

@override
String toString() {
  return 'BridgeTaskReviewState.passed(reviewerAgentId: $reviewerAgentId, summary: $summary)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewState_PassedCopyWith<$Res> implements $BridgeTaskReviewStateCopyWith<$Res> {
  factory $BridgeTaskReviewState_PassedCopyWith(BridgeTaskReviewState_Passed value, $Res Function(BridgeTaskReviewState_Passed) _then) = _$BridgeTaskReviewState_PassedCopyWithImpl;
@useResult
$Res call({
 String reviewerAgentId, String summary
});




}
/// @nodoc
class _$BridgeTaskReviewState_PassedCopyWithImpl<$Res>
    implements $BridgeTaskReviewState_PassedCopyWith<$Res> {
  _$BridgeTaskReviewState_PassedCopyWithImpl(this._self, this._then);

  final BridgeTaskReviewState_Passed _self;
  final $Res Function(BridgeTaskReviewState_Passed) _then;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reviewerAgentId = null,Object? summary = null,}) {
  return _then(BridgeTaskReviewState_Passed(
reviewerAgentId: null == reviewerAgentId ? _self.reviewerAgentId : reviewerAgentId // ignore: cast_nullable_to_non_nullable
as String,summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeTaskReviewState_ChangesRequired extends BridgeTaskReviewState {
  const BridgeTaskReviewState_ChangesRequired({required this.reviewerAgentId, required this.summary}): super._();


 final  String reviewerAgentId;
 final  String summary;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewState_ChangesRequiredCopyWith<BridgeTaskReviewState_ChangesRequired> get copyWith => _$BridgeTaskReviewState_ChangesRequiredCopyWithImpl<BridgeTaskReviewState_ChangesRequired>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewState_ChangesRequired&&(identical(other.reviewerAgentId, reviewerAgentId) || other.reviewerAgentId == reviewerAgentId)&&(identical(other.summary, summary) || other.summary == summary));
}


@override
int get hashCode => Object.hash(runtimeType,reviewerAgentId,summary);

@override
String toString() {
  return 'BridgeTaskReviewState.changesRequired(reviewerAgentId: $reviewerAgentId, summary: $summary)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewState_ChangesRequiredCopyWith<$Res> implements $BridgeTaskReviewStateCopyWith<$Res> {
  factory $BridgeTaskReviewState_ChangesRequiredCopyWith(BridgeTaskReviewState_ChangesRequired value, $Res Function(BridgeTaskReviewState_ChangesRequired) _then) = _$BridgeTaskReviewState_ChangesRequiredCopyWithImpl;
@useResult
$Res call({
 String reviewerAgentId, String summary
});




}
/// @nodoc
class _$BridgeTaskReviewState_ChangesRequiredCopyWithImpl<$Res>
    implements $BridgeTaskReviewState_ChangesRequiredCopyWith<$Res> {
  _$BridgeTaskReviewState_ChangesRequiredCopyWithImpl(this._self, this._then);

  final BridgeTaskReviewState_ChangesRequired _self;
  final $Res Function(BridgeTaskReviewState_ChangesRequired) _then;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reviewerAgentId = null,Object? summary = null,}) {
  return _then(BridgeTaskReviewState_ChangesRequired(
reviewerAgentId: null == reviewerAgentId ? _self.reviewerAgentId : reviewerAgentId // ignore: cast_nullable_to_non_nullable
as String,summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeTaskReviewState_Blocked extends BridgeTaskReviewState {
  const BridgeTaskReviewState_Blocked({required this.reviewerAgentId, required this.summary}): super._();


 final  String reviewerAgentId;
 final  String summary;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewState_BlockedCopyWith<BridgeTaskReviewState_Blocked> get copyWith => _$BridgeTaskReviewState_BlockedCopyWithImpl<BridgeTaskReviewState_Blocked>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewState_Blocked&&(identical(other.reviewerAgentId, reviewerAgentId) || other.reviewerAgentId == reviewerAgentId)&&(identical(other.summary, summary) || other.summary == summary));
}


@override
int get hashCode => Object.hash(runtimeType,reviewerAgentId,summary);

@override
String toString() {
  return 'BridgeTaskReviewState.blocked(reviewerAgentId: $reviewerAgentId, summary: $summary)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewState_BlockedCopyWith<$Res> implements $BridgeTaskReviewStateCopyWith<$Res> {
  factory $BridgeTaskReviewState_BlockedCopyWith(BridgeTaskReviewState_Blocked value, $Res Function(BridgeTaskReviewState_Blocked) _then) = _$BridgeTaskReviewState_BlockedCopyWithImpl;
@useResult
$Res call({
 String reviewerAgentId, String summary
});




}
/// @nodoc
class _$BridgeTaskReviewState_BlockedCopyWithImpl<$Res>
    implements $BridgeTaskReviewState_BlockedCopyWith<$Res> {
  _$BridgeTaskReviewState_BlockedCopyWithImpl(this._self, this._then);

  final BridgeTaskReviewState_Blocked _self;
  final $Res Function(BridgeTaskReviewState_Blocked) _then;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reviewerAgentId = null,Object? summary = null,}) {
  return _then(BridgeTaskReviewState_Blocked(
reviewerAgentId: null == reviewerAgentId ? _self.reviewerAgentId : reviewerAgentId // ignore: cast_nullable_to_non_nullable
as String,summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeTaskReviewState_Failed extends BridgeTaskReviewState {
  const BridgeTaskReviewState_Failed({this.reviewerAgentId, required this.error, required this.summary}): super._();


 final  String? reviewerAgentId;
 final  String error;
 final  String summary;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewState_FailedCopyWith<BridgeTaskReviewState_Failed> get copyWith => _$BridgeTaskReviewState_FailedCopyWithImpl<BridgeTaskReviewState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewState_Failed&&(identical(other.reviewerAgentId, reviewerAgentId) || other.reviewerAgentId == reviewerAgentId)&&(identical(other.error, error) || other.error == error)&&(identical(other.summary, summary) || other.summary == summary));
}


@override
int get hashCode => Object.hash(runtimeType,reviewerAgentId,error,summary);

@override
String toString() {
  return 'BridgeTaskReviewState.failed(reviewerAgentId: $reviewerAgentId, error: $error, summary: $summary)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewState_FailedCopyWith<$Res> implements $BridgeTaskReviewStateCopyWith<$Res> {
  factory $BridgeTaskReviewState_FailedCopyWith(BridgeTaskReviewState_Failed value, $Res Function(BridgeTaskReviewState_Failed) _then) = _$BridgeTaskReviewState_FailedCopyWithImpl;
@useResult
$Res call({
 String? reviewerAgentId, String error, String summary
});




}
/// @nodoc
class _$BridgeTaskReviewState_FailedCopyWithImpl<$Res>
    implements $BridgeTaskReviewState_FailedCopyWith<$Res> {
  _$BridgeTaskReviewState_FailedCopyWithImpl(this._self, this._then);

  final BridgeTaskReviewState_Failed _self;
  final $Res Function(BridgeTaskReviewState_Failed) _then;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reviewerAgentId = freezed,Object? error = null,Object? summary = null,}) {
  return _then(BridgeTaskReviewState_Failed(
reviewerAgentId: freezed == reviewerAgentId ? _self.reviewerAgentId : reviewerAgentId // ignore: cast_nullable_to_non_nullable
as String?,error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as String,summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeTaskReviewState_Cancelled extends BridgeTaskReviewState {
  const BridgeTaskReviewState_Cancelled({this.reviewerAgentId, required this.reason, required this.summary}): super._();


 final  String? reviewerAgentId;
 final  String reason;
 final  String summary;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewState_CancelledCopyWith<BridgeTaskReviewState_Cancelled> get copyWith => _$BridgeTaskReviewState_CancelledCopyWithImpl<BridgeTaskReviewState_Cancelled>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewState_Cancelled&&(identical(other.reviewerAgentId, reviewerAgentId) || other.reviewerAgentId == reviewerAgentId)&&(identical(other.reason, reason) || other.reason == reason)&&(identical(other.summary, summary) || other.summary == summary));
}


@override
int get hashCode => Object.hash(runtimeType,reviewerAgentId,reason,summary);

@override
String toString() {
  return 'BridgeTaskReviewState.cancelled(reviewerAgentId: $reviewerAgentId, reason: $reason, summary: $summary)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewState_CancelledCopyWith<$Res> implements $BridgeTaskReviewStateCopyWith<$Res> {
  factory $BridgeTaskReviewState_CancelledCopyWith(BridgeTaskReviewState_Cancelled value, $Res Function(BridgeTaskReviewState_Cancelled) _then) = _$BridgeTaskReviewState_CancelledCopyWithImpl;
@useResult
$Res call({
 String? reviewerAgentId, String reason, String summary
});




}
/// @nodoc
class _$BridgeTaskReviewState_CancelledCopyWithImpl<$Res>
    implements $BridgeTaskReviewState_CancelledCopyWith<$Res> {
  _$BridgeTaskReviewState_CancelledCopyWithImpl(this._self, this._then);

  final BridgeTaskReviewState_Cancelled _self;
  final $Res Function(BridgeTaskReviewState_Cancelled) _then;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reviewerAgentId = freezed,Object? reason = null,Object? summary = null,}) {
  return _then(BridgeTaskReviewState_Cancelled(
reviewerAgentId: freezed == reviewerAgentId ? _self.reviewerAgentId : reviewerAgentId // ignore: cast_nullable_to_non_nullable
as String?,reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeTaskState {

 Object get field0;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState&&const DeepCollectionEquality().equals(other.field0, field0));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(field0));

@override
String toString() {
  return 'BridgeTaskState(field0: $field0)';
}


}

/// @nodoc
class $BridgeTaskStateCopyWith<$Res>  {
$BridgeTaskStateCopyWith(BridgeTaskState _, $Res Function(BridgeTaskState) __);
}


/// Adds pattern-matching-related methods to [BridgeTaskState].
extension BridgeTaskStatePatterns on BridgeTaskState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTaskState_Planning value)?  planning,TResult Function( BridgeTaskState_PendingConfirmation value)?  pendingConfirmation,TResult Function( BridgeTaskState_EditingDocuments value)?  editingDocuments,TResult Function( BridgeTaskState_Working value)?  working,TResult Function( BridgeTaskState_Reviewing value)?  reviewing,TResult Function( BridgeTaskState_Completed value)?  completed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTaskState_Planning() when planning != null:
return planning(_that);case BridgeTaskState_PendingConfirmation() when pendingConfirmation != null:
return pendingConfirmation(_that);case BridgeTaskState_EditingDocuments() when editingDocuments != null:
return editingDocuments(_that);case BridgeTaskState_Working() when working != null:
return working(_that);case BridgeTaskState_Reviewing() when reviewing != null:
return reviewing(_that);case BridgeTaskState_Completed() when completed != null:
return completed(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTaskState_Planning value)  planning,required TResult Function( BridgeTaskState_PendingConfirmation value)  pendingConfirmation,required TResult Function( BridgeTaskState_EditingDocuments value)  editingDocuments,required TResult Function( BridgeTaskState_Working value)  working,required TResult Function( BridgeTaskState_Reviewing value)  reviewing,required TResult Function( BridgeTaskState_Completed value)  completed,}){
final _that = this;
switch (_that) {
case BridgeTaskState_Planning():
return planning(_that);case BridgeTaskState_PendingConfirmation():
return pendingConfirmation(_that);case BridgeTaskState_EditingDocuments():
return editingDocuments(_that);case BridgeTaskState_Working():
return working(_that);case BridgeTaskState_Reviewing():
return reviewing(_that);case BridgeTaskState_Completed():
return completed(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTaskState_Planning value)?  planning,TResult? Function( BridgeTaskState_PendingConfirmation value)?  pendingConfirmation,TResult? Function( BridgeTaskState_EditingDocuments value)?  editingDocuments,TResult? Function( BridgeTaskState_Working value)?  working,TResult? Function( BridgeTaskState_Reviewing value)?  reviewing,TResult? Function( BridgeTaskState_Completed value)?  completed,}){
final _that = this;
switch (_that) {
case BridgeTaskState_Planning() when planning != null:
return planning(_that);case BridgeTaskState_PendingConfirmation() when pendingConfirmation != null:
return pendingConfirmation(_that);case BridgeTaskState_EditingDocuments() when editingDocuments != null:
return editingDocuments(_that);case BridgeTaskState_Working() when working != null:
return working(_that);case BridgeTaskState_Reviewing() when reviewing != null:
return reviewing(_that);case BridgeTaskState_Completed() when completed != null:
return completed(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgePlanningTaskState field0)?  planning,TResult Function( BridgePendingConfirmationTaskState field0)?  pendingConfirmation,TResult Function( BridgeEditingDocumentsTaskState field0)?  editingDocuments,TResult Function( BridgeWorkingTaskState field0)?  working,TResult Function( BridgeReviewingTaskState field0)?  reviewing,TResult Function( BridgeCompletedTaskState field0)?  completed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTaskState_Planning() when planning != null:
return planning(_that.field0);case BridgeTaskState_PendingConfirmation() when pendingConfirmation != null:
return pendingConfirmation(_that.field0);case BridgeTaskState_EditingDocuments() when editingDocuments != null:
return editingDocuments(_that.field0);case BridgeTaskState_Working() when working != null:
return working(_that.field0);case BridgeTaskState_Reviewing() when reviewing != null:
return reviewing(_that.field0);case BridgeTaskState_Completed() when completed != null:
return completed(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgePlanningTaskState field0)  planning,required TResult Function( BridgePendingConfirmationTaskState field0)  pendingConfirmation,required TResult Function( BridgeEditingDocumentsTaskState field0)  editingDocuments,required TResult Function( BridgeWorkingTaskState field0)  working,required TResult Function( BridgeReviewingTaskState field0)  reviewing,required TResult Function( BridgeCompletedTaskState field0)  completed,}) {final _that = this;
switch (_that) {
case BridgeTaskState_Planning():
return planning(_that.field0);case BridgeTaskState_PendingConfirmation():
return pendingConfirmation(_that.field0);case BridgeTaskState_EditingDocuments():
return editingDocuments(_that.field0);case BridgeTaskState_Working():
return working(_that.field0);case BridgeTaskState_Reviewing():
return reviewing(_that.field0);case BridgeTaskState_Completed():
return completed(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgePlanningTaskState field0)?  planning,TResult? Function( BridgePendingConfirmationTaskState field0)?  pendingConfirmation,TResult? Function( BridgeEditingDocumentsTaskState field0)?  editingDocuments,TResult? Function( BridgeWorkingTaskState field0)?  working,TResult? Function( BridgeReviewingTaskState field0)?  reviewing,TResult? Function( BridgeCompletedTaskState field0)?  completed,}) {final _that = this;
switch (_that) {
case BridgeTaskState_Planning() when planning != null:
return planning(_that.field0);case BridgeTaskState_PendingConfirmation() when pendingConfirmation != null:
return pendingConfirmation(_that.field0);case BridgeTaskState_EditingDocuments() when editingDocuments != null:
return editingDocuments(_that.field0);case BridgeTaskState_Working() when working != null:
return working(_that.field0);case BridgeTaskState_Reviewing() when reviewing != null:
return reviewing(_that.field0);case BridgeTaskState_Completed() when completed != null:
return completed(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTaskState_Planning extends BridgeTaskState {
  const BridgeTaskState_Planning(this.field0): super._();


@override final  BridgePlanningTaskState field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskState_PlanningCopyWith<BridgeTaskState_Planning> get copyWith => _$BridgeTaskState_PlanningCopyWithImpl<BridgeTaskState_Planning>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState_Planning&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState.planning(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskState_PlanningCopyWith<$Res> implements $BridgeTaskStateCopyWith<$Res> {
  factory $BridgeTaskState_PlanningCopyWith(BridgeTaskState_Planning value, $Res Function(BridgeTaskState_Planning) _then) = _$BridgeTaskState_PlanningCopyWithImpl;
@useResult
$Res call({
 BridgePlanningTaskState field0
});




}
/// @nodoc
class _$BridgeTaskState_PlanningCopyWithImpl<$Res>
    implements $BridgeTaskState_PlanningCopyWith<$Res> {
  _$BridgeTaskState_PlanningCopyWithImpl(this._self, this._then);

  final BridgeTaskState_Planning _self;
  final $Res Function(BridgeTaskState_Planning) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_Planning(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgePlanningTaskState,
  ));
}


}

/// @nodoc


class BridgeTaskState_PendingConfirmation extends BridgeTaskState {
  const BridgeTaskState_PendingConfirmation(this.field0): super._();


@override final  BridgePendingConfirmationTaskState field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskState_PendingConfirmationCopyWith<BridgeTaskState_PendingConfirmation> get copyWith => _$BridgeTaskState_PendingConfirmationCopyWithImpl<BridgeTaskState_PendingConfirmation>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState_PendingConfirmation&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState.pendingConfirmation(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskState_PendingConfirmationCopyWith<$Res> implements $BridgeTaskStateCopyWith<$Res> {
  factory $BridgeTaskState_PendingConfirmationCopyWith(BridgeTaskState_PendingConfirmation value, $Res Function(BridgeTaskState_PendingConfirmation) _then) = _$BridgeTaskState_PendingConfirmationCopyWithImpl;
@useResult
$Res call({
 BridgePendingConfirmationTaskState field0
});




}
/// @nodoc
class _$BridgeTaskState_PendingConfirmationCopyWithImpl<$Res>
    implements $BridgeTaskState_PendingConfirmationCopyWith<$Res> {
  _$BridgeTaskState_PendingConfirmationCopyWithImpl(this._self, this._then);

  final BridgeTaskState_PendingConfirmation _self;
  final $Res Function(BridgeTaskState_PendingConfirmation) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_PendingConfirmation(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgePendingConfirmationTaskState,
  ));
}


}

/// @nodoc


class BridgeTaskState_EditingDocuments extends BridgeTaskState {
  const BridgeTaskState_EditingDocuments(this.field0): super._();


@override final  BridgeEditingDocumentsTaskState field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskState_EditingDocumentsCopyWith<BridgeTaskState_EditingDocuments> get copyWith => _$BridgeTaskState_EditingDocumentsCopyWithImpl<BridgeTaskState_EditingDocuments>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState_EditingDocuments&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState.editingDocuments(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskState_EditingDocumentsCopyWith<$Res> implements $BridgeTaskStateCopyWith<$Res> {
  factory $BridgeTaskState_EditingDocumentsCopyWith(BridgeTaskState_EditingDocuments value, $Res Function(BridgeTaskState_EditingDocuments) _then) = _$BridgeTaskState_EditingDocumentsCopyWithImpl;
@useResult
$Res call({
 BridgeEditingDocumentsTaskState field0
});




}
/// @nodoc
class _$BridgeTaskState_EditingDocumentsCopyWithImpl<$Res>
    implements $BridgeTaskState_EditingDocumentsCopyWith<$Res> {
  _$BridgeTaskState_EditingDocumentsCopyWithImpl(this._self, this._then);

  final BridgeTaskState_EditingDocuments _self;
  final $Res Function(BridgeTaskState_EditingDocuments) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_EditingDocuments(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeEditingDocumentsTaskState,
  ));
}


}

/// @nodoc


class BridgeTaskState_Working extends BridgeTaskState {
  const BridgeTaskState_Working(this.field0): super._();


@override final  BridgeWorkingTaskState field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskState_WorkingCopyWith<BridgeTaskState_Working> get copyWith => _$BridgeTaskState_WorkingCopyWithImpl<BridgeTaskState_Working>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState_Working&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState.working(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskState_WorkingCopyWith<$Res> implements $BridgeTaskStateCopyWith<$Res> {
  factory $BridgeTaskState_WorkingCopyWith(BridgeTaskState_Working value, $Res Function(BridgeTaskState_Working) _then) = _$BridgeTaskState_WorkingCopyWithImpl;
@useResult
$Res call({
 BridgeWorkingTaskState field0
});




}
/// @nodoc
class _$BridgeTaskState_WorkingCopyWithImpl<$Res>
    implements $BridgeTaskState_WorkingCopyWith<$Res> {
  _$BridgeTaskState_WorkingCopyWithImpl(this._self, this._then);

  final BridgeTaskState_Working _self;
  final $Res Function(BridgeTaskState_Working) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_Working(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeWorkingTaskState,
  ));
}


}

/// @nodoc


class BridgeTaskState_Reviewing extends BridgeTaskState {
  const BridgeTaskState_Reviewing(this.field0): super._();


@override final  BridgeReviewingTaskState field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskState_ReviewingCopyWith<BridgeTaskState_Reviewing> get copyWith => _$BridgeTaskState_ReviewingCopyWithImpl<BridgeTaskState_Reviewing>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState_Reviewing&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState.reviewing(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskState_ReviewingCopyWith<$Res> implements $BridgeTaskStateCopyWith<$Res> {
  factory $BridgeTaskState_ReviewingCopyWith(BridgeTaskState_Reviewing value, $Res Function(BridgeTaskState_Reviewing) _then) = _$BridgeTaskState_ReviewingCopyWithImpl;
@useResult
$Res call({
 BridgeReviewingTaskState field0
});




}
/// @nodoc
class _$BridgeTaskState_ReviewingCopyWithImpl<$Res>
    implements $BridgeTaskState_ReviewingCopyWith<$Res> {
  _$BridgeTaskState_ReviewingCopyWithImpl(this._self, this._then);

  final BridgeTaskState_Reviewing _self;
  final $Res Function(BridgeTaskState_Reviewing) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_Reviewing(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeReviewingTaskState,
  ));
}


}

/// @nodoc


class BridgeTaskState_Completed extends BridgeTaskState {
  const BridgeTaskState_Completed(this.field0): super._();


@override final  BridgeCompletedTaskState field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskState_CompletedCopyWith<BridgeTaskState_Completed> get copyWith => _$BridgeTaskState_CompletedCopyWithImpl<BridgeTaskState_Completed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState_Completed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState.completed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskState_CompletedCopyWith<$Res> implements $BridgeTaskStateCopyWith<$Res> {
  factory $BridgeTaskState_CompletedCopyWith(BridgeTaskState_Completed value, $Res Function(BridgeTaskState_Completed) _then) = _$BridgeTaskState_CompletedCopyWithImpl;
@useResult
$Res call({
 BridgeCompletedTaskState field0
});




}
/// @nodoc
class _$BridgeTaskState_CompletedCopyWithImpl<$Res>
    implements $BridgeTaskState_CompletedCopyWith<$Res> {
  _$BridgeTaskState_CompletedCopyWithImpl(this._self, this._then);

  final BridgeTaskState_Completed _self;
  final $Res Function(BridgeTaskState_Completed) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_Completed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeCompletedTaskState,
  ));
}


}

/// @nodoc
mixin _$BridgeTaskWorkUnitState {

 Object get field0;



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState&&const DeepCollectionEquality().equals(other.field0, field0));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(field0));

@override
String toString() {
  return 'BridgeTaskWorkUnitState(field0: $field0)';
}


}

/// @nodoc
class $BridgeTaskWorkUnitStateCopyWith<$Res>  {
$BridgeTaskWorkUnitStateCopyWith(BridgeTaskWorkUnitState _, $Res Function(BridgeTaskWorkUnitState) __);
}


/// Adds pattern-matching-related methods to [BridgeTaskWorkUnitState].
extension BridgeTaskWorkUnitStatePatterns on BridgeTaskWorkUnitState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTaskWorkUnitState_Pending value)?  pending,TResult Function( BridgeTaskWorkUnitState_Running value)?  running,TResult Function( BridgeTaskWorkUnitState_WaitingReview value)?  waitingReview,TResult Function( BridgeTaskWorkUnitState_ReviewPassed value)?  reviewPassed,TResult Function( BridgeTaskWorkUnitState_ChangesRequired value)?  changesRequired,TResult Function( BridgeTaskWorkUnitState_Paused value)?  paused,TResult Function( BridgeTaskWorkUnitState_Completed value)?  completed,TResult Function( BridgeTaskWorkUnitState_Failed value)?  failed,TResult Function( BridgeTaskWorkUnitState_Cancelled value)?  cancelled,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTaskWorkUnitState_Pending() when pending != null:
return pending(_that);case BridgeTaskWorkUnitState_Running() when running != null:
return running(_that);case BridgeTaskWorkUnitState_WaitingReview() when waitingReview != null:
return waitingReview(_that);case BridgeTaskWorkUnitState_ReviewPassed() when reviewPassed != null:
return reviewPassed(_that);case BridgeTaskWorkUnitState_ChangesRequired() when changesRequired != null:
return changesRequired(_that);case BridgeTaskWorkUnitState_Paused() when paused != null:
return paused(_that);case BridgeTaskWorkUnitState_Completed() when completed != null:
return completed(_that);case BridgeTaskWorkUnitState_Failed() when failed != null:
return failed(_that);case BridgeTaskWorkUnitState_Cancelled() when cancelled != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTaskWorkUnitState_Pending value)  pending,required TResult Function( BridgeTaskWorkUnitState_Running value)  running,required TResult Function( BridgeTaskWorkUnitState_WaitingReview value)  waitingReview,required TResult Function( BridgeTaskWorkUnitState_ReviewPassed value)  reviewPassed,required TResult Function( BridgeTaskWorkUnitState_ChangesRequired value)  changesRequired,required TResult Function( BridgeTaskWorkUnitState_Paused value)  paused,required TResult Function( BridgeTaskWorkUnitState_Completed value)  completed,required TResult Function( BridgeTaskWorkUnitState_Failed value)  failed,required TResult Function( BridgeTaskWorkUnitState_Cancelled value)  cancelled,}){
final _that = this;
switch (_that) {
case BridgeTaskWorkUnitState_Pending():
return pending(_that);case BridgeTaskWorkUnitState_Running():
return running(_that);case BridgeTaskWorkUnitState_WaitingReview():
return waitingReview(_that);case BridgeTaskWorkUnitState_ReviewPassed():
return reviewPassed(_that);case BridgeTaskWorkUnitState_ChangesRequired():
return changesRequired(_that);case BridgeTaskWorkUnitState_Paused():
return paused(_that);case BridgeTaskWorkUnitState_Completed():
return completed(_that);case BridgeTaskWorkUnitState_Failed():
return failed(_that);case BridgeTaskWorkUnitState_Cancelled():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTaskWorkUnitState_Pending value)?  pending,TResult? Function( BridgeTaskWorkUnitState_Running value)?  running,TResult? Function( BridgeTaskWorkUnitState_WaitingReview value)?  waitingReview,TResult? Function( BridgeTaskWorkUnitState_ReviewPassed value)?  reviewPassed,TResult? Function( BridgeTaskWorkUnitState_ChangesRequired value)?  changesRequired,TResult? Function( BridgeTaskWorkUnitState_Paused value)?  paused,TResult? Function( BridgeTaskWorkUnitState_Completed value)?  completed,TResult? Function( BridgeTaskWorkUnitState_Failed value)?  failed,TResult? Function( BridgeTaskWorkUnitState_Cancelled value)?  cancelled,}){
final _that = this;
switch (_that) {
case BridgeTaskWorkUnitState_Pending() when pending != null:
return pending(_that);case BridgeTaskWorkUnitState_Running() when running != null:
return running(_that);case BridgeTaskWorkUnitState_WaitingReview() when waitingReview != null:
return waitingReview(_that);case BridgeTaskWorkUnitState_ReviewPassed() when reviewPassed != null:
return reviewPassed(_that);case BridgeTaskWorkUnitState_ChangesRequired() when changesRequired != null:
return changesRequired(_that);case BridgeTaskWorkUnitState_Paused() when paused != null:
return paused(_that);case BridgeTaskWorkUnitState_Completed() when completed != null:
return completed(_that);case BridgeTaskWorkUnitState_Failed() when failed != null:
return failed(_that);case BridgeTaskWorkUnitState_Cancelled() when cancelled != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgePendingWorkUnit field0)?  pending,TResult Function( BridgeRunningWorkUnit field0)?  running,TResult Function( BridgeWaitingReviewWorkUnit field0)?  waitingReview,TResult Function( BridgeReviewPassedWorkUnit field0)?  reviewPassed,TResult Function( BridgeChangesRequiredWorkUnit field0)?  changesRequired,TResult Function( BridgePausedWorkUnit field0)?  paused,TResult Function( BridgeCompletedWorkUnit field0)?  completed,TResult Function( BridgeFailedWorkUnit field0)?  failed,TResult Function( BridgeCancelledWorkUnit field0)?  cancelled,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTaskWorkUnitState_Pending() when pending != null:
return pending(_that.field0);case BridgeTaskWorkUnitState_Running() when running != null:
return running(_that.field0);case BridgeTaskWorkUnitState_WaitingReview() when waitingReview != null:
return waitingReview(_that.field0);case BridgeTaskWorkUnitState_ReviewPassed() when reviewPassed != null:
return reviewPassed(_that.field0);case BridgeTaskWorkUnitState_ChangesRequired() when changesRequired != null:
return changesRequired(_that.field0);case BridgeTaskWorkUnitState_Paused() when paused != null:
return paused(_that.field0);case BridgeTaskWorkUnitState_Completed() when completed != null:
return completed(_that.field0);case BridgeTaskWorkUnitState_Failed() when failed != null:
return failed(_that.field0);case BridgeTaskWorkUnitState_Cancelled() when cancelled != null:
return cancelled(_that.field0);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgePendingWorkUnit field0)  pending,required TResult Function( BridgeRunningWorkUnit field0)  running,required TResult Function( BridgeWaitingReviewWorkUnit field0)  waitingReview,required TResult Function( BridgeReviewPassedWorkUnit field0)  reviewPassed,required TResult Function( BridgeChangesRequiredWorkUnit field0)  changesRequired,required TResult Function( BridgePausedWorkUnit field0)  paused,required TResult Function( BridgeCompletedWorkUnit field0)  completed,required TResult Function( BridgeFailedWorkUnit field0)  failed,required TResult Function( BridgeCancelledWorkUnit field0)  cancelled,}) {final _that = this;
switch (_that) {
case BridgeTaskWorkUnitState_Pending():
return pending(_that.field0);case BridgeTaskWorkUnitState_Running():
return running(_that.field0);case BridgeTaskWorkUnitState_WaitingReview():
return waitingReview(_that.field0);case BridgeTaskWorkUnitState_ReviewPassed():
return reviewPassed(_that.field0);case BridgeTaskWorkUnitState_ChangesRequired():
return changesRequired(_that.field0);case BridgeTaskWorkUnitState_Paused():
return paused(_that.field0);case BridgeTaskWorkUnitState_Completed():
return completed(_that.field0);case BridgeTaskWorkUnitState_Failed():
return failed(_that.field0);case BridgeTaskWorkUnitState_Cancelled():
return cancelled(_that.field0);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgePendingWorkUnit field0)?  pending,TResult? Function( BridgeRunningWorkUnit field0)?  running,TResult? Function( BridgeWaitingReviewWorkUnit field0)?  waitingReview,TResult? Function( BridgeReviewPassedWorkUnit field0)?  reviewPassed,TResult? Function( BridgeChangesRequiredWorkUnit field0)?  changesRequired,TResult? Function( BridgePausedWorkUnit field0)?  paused,TResult? Function( BridgeCompletedWorkUnit field0)?  completed,TResult? Function( BridgeFailedWorkUnit field0)?  failed,TResult? Function( BridgeCancelledWorkUnit field0)?  cancelled,}) {final _that = this;
switch (_that) {
case BridgeTaskWorkUnitState_Pending() when pending != null:
return pending(_that.field0);case BridgeTaskWorkUnitState_Running() when running != null:
return running(_that.field0);case BridgeTaskWorkUnitState_WaitingReview() when waitingReview != null:
return waitingReview(_that.field0);case BridgeTaskWorkUnitState_ReviewPassed() when reviewPassed != null:
return reviewPassed(_that.field0);case BridgeTaskWorkUnitState_ChangesRequired() when changesRequired != null:
return changesRequired(_that.field0);case BridgeTaskWorkUnitState_Paused() when paused != null:
return paused(_that.field0);case BridgeTaskWorkUnitState_Completed() when completed != null:
return completed(_that.field0);case BridgeTaskWorkUnitState_Failed() when failed != null:
return failed(_that.field0);case BridgeTaskWorkUnitState_Cancelled() when cancelled != null:
return cancelled(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTaskWorkUnitState_Pending extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_Pending(this.field0): super._();


@override final  BridgePendingWorkUnit field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_PendingCopyWith<BridgeTaskWorkUnitState_Pending> get copyWith => _$BridgeTaskWorkUnitState_PendingCopyWithImpl<BridgeTaskWorkUnitState_Pending>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_Pending&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.pending(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_PendingCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_PendingCopyWith(BridgeTaskWorkUnitState_Pending value, $Res Function(BridgeTaskWorkUnitState_Pending) _then) = _$BridgeTaskWorkUnitState_PendingCopyWithImpl;
@useResult
$Res call({
 BridgePendingWorkUnit field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_PendingCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_PendingCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_PendingCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_Pending _self;
  final $Res Function(BridgeTaskWorkUnitState_Pending) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_Pending(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgePendingWorkUnit,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_Running extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_Running(this.field0): super._();


@override final  BridgeRunningWorkUnit field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_RunningCopyWith<BridgeTaskWorkUnitState_Running> get copyWith => _$BridgeTaskWorkUnitState_RunningCopyWithImpl<BridgeTaskWorkUnitState_Running>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_Running&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.running(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_RunningCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_RunningCopyWith(BridgeTaskWorkUnitState_Running value, $Res Function(BridgeTaskWorkUnitState_Running) _then) = _$BridgeTaskWorkUnitState_RunningCopyWithImpl;
@useResult
$Res call({
 BridgeRunningWorkUnit field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_RunningCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_RunningCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_RunningCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_Running _self;
  final $Res Function(BridgeTaskWorkUnitState_Running) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_Running(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeRunningWorkUnit,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_WaitingReview extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_WaitingReview(this.field0): super._();


@override final  BridgeWaitingReviewWorkUnit field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_WaitingReviewCopyWith<BridgeTaskWorkUnitState_WaitingReview> get copyWith => _$BridgeTaskWorkUnitState_WaitingReviewCopyWithImpl<BridgeTaskWorkUnitState_WaitingReview>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_WaitingReview&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.waitingReview(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_WaitingReviewCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_WaitingReviewCopyWith(BridgeTaskWorkUnitState_WaitingReview value, $Res Function(BridgeTaskWorkUnitState_WaitingReview) _then) = _$BridgeTaskWorkUnitState_WaitingReviewCopyWithImpl;
@useResult
$Res call({
 BridgeWaitingReviewWorkUnit field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_WaitingReviewCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_WaitingReviewCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_WaitingReviewCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_WaitingReview _self;
  final $Res Function(BridgeTaskWorkUnitState_WaitingReview) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_WaitingReview(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeWaitingReviewWorkUnit,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_ReviewPassed extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_ReviewPassed(this.field0): super._();


@override final  BridgeReviewPassedWorkUnit field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_ReviewPassedCopyWith<BridgeTaskWorkUnitState_ReviewPassed> get copyWith => _$BridgeTaskWorkUnitState_ReviewPassedCopyWithImpl<BridgeTaskWorkUnitState_ReviewPassed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_ReviewPassed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.reviewPassed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_ReviewPassedCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_ReviewPassedCopyWith(BridgeTaskWorkUnitState_ReviewPassed value, $Res Function(BridgeTaskWorkUnitState_ReviewPassed) _then) = _$BridgeTaskWorkUnitState_ReviewPassedCopyWithImpl;
@useResult
$Res call({
 BridgeReviewPassedWorkUnit field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_ReviewPassedCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_ReviewPassedCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_ReviewPassedCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_ReviewPassed _self;
  final $Res Function(BridgeTaskWorkUnitState_ReviewPassed) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_ReviewPassed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeReviewPassedWorkUnit,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_ChangesRequired extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_ChangesRequired(this.field0): super._();


@override final  BridgeChangesRequiredWorkUnit field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_ChangesRequiredCopyWith<BridgeTaskWorkUnitState_ChangesRequired> get copyWith => _$BridgeTaskWorkUnitState_ChangesRequiredCopyWithImpl<BridgeTaskWorkUnitState_ChangesRequired>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_ChangesRequired&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.changesRequired(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_ChangesRequiredCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_ChangesRequiredCopyWith(BridgeTaskWorkUnitState_ChangesRequired value, $Res Function(BridgeTaskWorkUnitState_ChangesRequired) _then) = _$BridgeTaskWorkUnitState_ChangesRequiredCopyWithImpl;
@useResult
$Res call({
 BridgeChangesRequiredWorkUnit field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_ChangesRequiredCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_ChangesRequiredCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_ChangesRequiredCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_ChangesRequired _self;
  final $Res Function(BridgeTaskWorkUnitState_ChangesRequired) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_ChangesRequired(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeChangesRequiredWorkUnit,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_Paused extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_Paused(this.field0): super._();


@override final  BridgePausedWorkUnit field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_PausedCopyWith<BridgeTaskWorkUnitState_Paused> get copyWith => _$BridgeTaskWorkUnitState_PausedCopyWithImpl<BridgeTaskWorkUnitState_Paused>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_Paused&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.paused(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_PausedCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_PausedCopyWith(BridgeTaskWorkUnitState_Paused value, $Res Function(BridgeTaskWorkUnitState_Paused) _then) = _$BridgeTaskWorkUnitState_PausedCopyWithImpl;
@useResult
$Res call({
 BridgePausedWorkUnit field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_PausedCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_PausedCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_PausedCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_Paused _self;
  final $Res Function(BridgeTaskWorkUnitState_Paused) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_Paused(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgePausedWorkUnit,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_Completed extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_Completed(this.field0): super._();


@override final  BridgeCompletedWorkUnit field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_CompletedCopyWith<BridgeTaskWorkUnitState_Completed> get copyWith => _$BridgeTaskWorkUnitState_CompletedCopyWithImpl<BridgeTaskWorkUnitState_Completed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_Completed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.completed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_CompletedCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_CompletedCopyWith(BridgeTaskWorkUnitState_Completed value, $Res Function(BridgeTaskWorkUnitState_Completed) _then) = _$BridgeTaskWorkUnitState_CompletedCopyWithImpl;
@useResult
$Res call({
 BridgeCompletedWorkUnit field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_CompletedCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_CompletedCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_CompletedCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_Completed _self;
  final $Res Function(BridgeTaskWorkUnitState_Completed) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_Completed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeCompletedWorkUnit,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_Failed extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_Failed(this.field0): super._();


@override final  BridgeFailedWorkUnit field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_FailedCopyWith<BridgeTaskWorkUnitState_Failed> get copyWith => _$BridgeTaskWorkUnitState_FailedCopyWithImpl<BridgeTaskWorkUnitState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_Failed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.failed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_FailedCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_FailedCopyWith(BridgeTaskWorkUnitState_Failed value, $Res Function(BridgeTaskWorkUnitState_Failed) _then) = _$BridgeTaskWorkUnitState_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeFailedWorkUnit field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_FailedCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_FailedCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_FailedCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_Failed _self;
  final $Res Function(BridgeTaskWorkUnitState_Failed) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_Failed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeFailedWorkUnit,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_Cancelled extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_Cancelled(this.field0): super._();


@override final  BridgeCancelledWorkUnit field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_CancelledCopyWith<BridgeTaskWorkUnitState_Cancelled> get copyWith => _$BridgeTaskWorkUnitState_CancelledCopyWithImpl<BridgeTaskWorkUnitState_Cancelled>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_Cancelled&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.cancelled(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_CancelledCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_CancelledCopyWith(BridgeTaskWorkUnitState_Cancelled value, $Res Function(BridgeTaskWorkUnitState_Cancelled) _then) = _$BridgeTaskWorkUnitState_CancelledCopyWithImpl;
@useResult
$Res call({
 BridgeCancelledWorkUnit field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_CancelledCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_CancelledCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_CancelledCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_Cancelled _self;
  final $Res Function(BridgeTaskWorkUnitState_Cancelled) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_Cancelled(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeCancelledWorkUnit,
  ));
}


}

/// @nodoc
mixin _$BridgeWaitingReviewPhase {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeWaitingReviewPhase);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeWaitingReviewPhase()';
}


}

/// @nodoc
class $BridgeWaitingReviewPhaseCopyWith<$Res>  {
$BridgeWaitingReviewPhaseCopyWith(BridgeWaitingReviewPhase _, $Res Function(BridgeWaitingReviewPhase) __);
}


/// Adds pattern-matching-related methods to [BridgeWaitingReviewPhase].
extension BridgeWaitingReviewPhasePatterns on BridgeWaitingReviewPhase {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeWaitingReviewPhase_AwaitingReport value)?  awaitingReport,TResult Function( BridgeWaitingReviewPhase_Ready value)?  ready,TResult Function( BridgeWaitingReviewPhase_Reviewing value)?  reviewing,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeWaitingReviewPhase_AwaitingReport() when awaitingReport != null:
return awaitingReport(_that);case BridgeWaitingReviewPhase_Ready() when ready != null:
return ready(_that);case BridgeWaitingReviewPhase_Reviewing() when reviewing != null:
return reviewing(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeWaitingReviewPhase_AwaitingReport value)  awaitingReport,required TResult Function( BridgeWaitingReviewPhase_Ready value)  ready,required TResult Function( BridgeWaitingReviewPhase_Reviewing value)  reviewing,}){
final _that = this;
switch (_that) {
case BridgeWaitingReviewPhase_AwaitingReport():
return awaitingReport(_that);case BridgeWaitingReviewPhase_Ready():
return ready(_that);case BridgeWaitingReviewPhase_Reviewing():
return reviewing(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeWaitingReviewPhase_AwaitingReport value)?  awaitingReport,TResult? Function( BridgeWaitingReviewPhase_Ready value)?  ready,TResult? Function( BridgeWaitingReviewPhase_Reviewing value)?  reviewing,}){
final _that = this;
switch (_that) {
case BridgeWaitingReviewPhase_AwaitingReport() when awaitingReport != null:
return awaitingReport(_that);case BridgeWaitingReviewPhase_Ready() when ready != null:
return ready(_that);case BridgeWaitingReviewPhase_Reviewing() when reviewing != null:
return reviewing(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeExecutorTerminalOutcome outcome,  BridgeExecutorContinuationState continuation)?  awaitingReport,TResult Function( String completionId,  int completionRevision,  String verificationSummary)?  ready,TResult Function( String completionId,  int completionRevision,  String reviewRoundId,  String verificationSummary)?  reviewing,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeWaitingReviewPhase_AwaitingReport() when awaitingReport != null:
return awaitingReport(_that.outcome,_that.continuation);case BridgeWaitingReviewPhase_Ready() when ready != null:
return ready(_that.completionId,_that.completionRevision,_that.verificationSummary);case BridgeWaitingReviewPhase_Reviewing() when reviewing != null:
return reviewing(_that.completionId,_that.completionRevision,_that.reviewRoundId,_that.verificationSummary);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeExecutorTerminalOutcome outcome,  BridgeExecutorContinuationState continuation)  awaitingReport,required TResult Function( String completionId,  int completionRevision,  String verificationSummary)  ready,required TResult Function( String completionId,  int completionRevision,  String reviewRoundId,  String verificationSummary)  reviewing,}) {final _that = this;
switch (_that) {
case BridgeWaitingReviewPhase_AwaitingReport():
return awaitingReport(_that.outcome,_that.continuation);case BridgeWaitingReviewPhase_Ready():
return ready(_that.completionId,_that.completionRevision,_that.verificationSummary);case BridgeWaitingReviewPhase_Reviewing():
return reviewing(_that.completionId,_that.completionRevision,_that.reviewRoundId,_that.verificationSummary);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeExecutorTerminalOutcome outcome,  BridgeExecutorContinuationState continuation)?  awaitingReport,TResult? Function( String completionId,  int completionRevision,  String verificationSummary)?  ready,TResult? Function( String completionId,  int completionRevision,  String reviewRoundId,  String verificationSummary)?  reviewing,}) {final _that = this;
switch (_that) {
case BridgeWaitingReviewPhase_AwaitingReport() when awaitingReport != null:
return awaitingReport(_that.outcome,_that.continuation);case BridgeWaitingReviewPhase_Ready() when ready != null:
return ready(_that.completionId,_that.completionRevision,_that.verificationSummary);case BridgeWaitingReviewPhase_Reviewing() when reviewing != null:
return reviewing(_that.completionId,_that.completionRevision,_that.reviewRoundId,_that.verificationSummary);case _:
  return null;

}
}

}

/// @nodoc


class BridgeWaitingReviewPhase_AwaitingReport extends BridgeWaitingReviewPhase {
  const BridgeWaitingReviewPhase_AwaitingReport({required this.outcome, required this.continuation}): super._();


 final  BridgeExecutorTerminalOutcome outcome;
 final  BridgeExecutorContinuationState continuation;

/// Create a copy of BridgeWaitingReviewPhase
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeWaitingReviewPhase_AwaitingReportCopyWith<BridgeWaitingReviewPhase_AwaitingReport> get copyWith => _$BridgeWaitingReviewPhase_AwaitingReportCopyWithImpl<BridgeWaitingReviewPhase_AwaitingReport>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeWaitingReviewPhase_AwaitingReport&&(identical(other.outcome, outcome) || other.outcome == outcome)&&(identical(other.continuation, continuation) || other.continuation == continuation));
}


@override
int get hashCode => Object.hash(runtimeType,outcome,continuation);

@override
String toString() {
  return 'BridgeWaitingReviewPhase.awaitingReport(outcome: $outcome, continuation: $continuation)';
}


}

/// @nodoc
abstract mixin class $BridgeWaitingReviewPhase_AwaitingReportCopyWith<$Res> implements $BridgeWaitingReviewPhaseCopyWith<$Res> {
  factory $BridgeWaitingReviewPhase_AwaitingReportCopyWith(BridgeWaitingReviewPhase_AwaitingReport value, $Res Function(BridgeWaitingReviewPhase_AwaitingReport) _then) = _$BridgeWaitingReviewPhase_AwaitingReportCopyWithImpl;
@useResult
$Res call({
 BridgeExecutorTerminalOutcome outcome, BridgeExecutorContinuationState continuation
});


$BridgeExecutorTerminalOutcomeCopyWith<$Res> get outcome;$BridgeExecutorContinuationStateCopyWith<$Res> get continuation;

}
/// @nodoc
class _$BridgeWaitingReviewPhase_AwaitingReportCopyWithImpl<$Res>
    implements $BridgeWaitingReviewPhase_AwaitingReportCopyWith<$Res> {
  _$BridgeWaitingReviewPhase_AwaitingReportCopyWithImpl(this._self, this._then);

  final BridgeWaitingReviewPhase_AwaitingReport _self;
  final $Res Function(BridgeWaitingReviewPhase_AwaitingReport) _then;

/// Create a copy of BridgeWaitingReviewPhase
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? outcome = null,Object? continuation = null,}) {
  return _then(BridgeWaitingReviewPhase_AwaitingReport(
outcome: null == outcome ? _self.outcome : outcome // ignore: cast_nullable_to_non_nullable
as BridgeExecutorTerminalOutcome,continuation: null == continuation ? _self.continuation : continuation // ignore: cast_nullable_to_non_nullable
as BridgeExecutorContinuationState,
  ));
}

/// Create a copy of BridgeWaitingReviewPhase
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeExecutorTerminalOutcomeCopyWith<$Res> get outcome {

  return $BridgeExecutorTerminalOutcomeCopyWith<$Res>(_self.outcome, (value) {
    return _then(_self.copyWith(outcome: value));
  });
}/// Create a copy of BridgeWaitingReviewPhase
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeExecutorContinuationStateCopyWith<$Res> get continuation {

  return $BridgeExecutorContinuationStateCopyWith<$Res>(_self.continuation, (value) {
    return _then(_self.copyWith(continuation: value));
  });
}
}

/// @nodoc


class BridgeWaitingReviewPhase_Ready extends BridgeWaitingReviewPhase {
  const BridgeWaitingReviewPhase_Ready({required this.completionId, required this.completionRevision, required this.verificationSummary}): super._();


 final  String completionId;
 final  int completionRevision;
 final  String verificationSummary;

/// Create a copy of BridgeWaitingReviewPhase
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeWaitingReviewPhase_ReadyCopyWith<BridgeWaitingReviewPhase_Ready> get copyWith => _$BridgeWaitingReviewPhase_ReadyCopyWithImpl<BridgeWaitingReviewPhase_Ready>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeWaitingReviewPhase_Ready&&(identical(other.completionId, completionId) || other.completionId == completionId)&&(identical(other.completionRevision, completionRevision) || other.completionRevision == completionRevision)&&(identical(other.verificationSummary, verificationSummary) || other.verificationSummary == verificationSummary));
}


@override
int get hashCode => Object.hash(runtimeType,completionId,completionRevision,verificationSummary);

@override
String toString() {
  return 'BridgeWaitingReviewPhase.ready(completionId: $completionId, completionRevision: $completionRevision, verificationSummary: $verificationSummary)';
}


}

/// @nodoc
abstract mixin class $BridgeWaitingReviewPhase_ReadyCopyWith<$Res> implements $BridgeWaitingReviewPhaseCopyWith<$Res> {
  factory $BridgeWaitingReviewPhase_ReadyCopyWith(BridgeWaitingReviewPhase_Ready value, $Res Function(BridgeWaitingReviewPhase_Ready) _then) = _$BridgeWaitingReviewPhase_ReadyCopyWithImpl;
@useResult
$Res call({
 String completionId, int completionRevision, String verificationSummary
});




}
/// @nodoc
class _$BridgeWaitingReviewPhase_ReadyCopyWithImpl<$Res>
    implements $BridgeWaitingReviewPhase_ReadyCopyWith<$Res> {
  _$BridgeWaitingReviewPhase_ReadyCopyWithImpl(this._self, this._then);

  final BridgeWaitingReviewPhase_Ready _self;
  final $Res Function(BridgeWaitingReviewPhase_Ready) _then;

/// Create a copy of BridgeWaitingReviewPhase
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? completionId = null,Object? completionRevision = null,Object? verificationSummary = null,}) {
  return _then(BridgeWaitingReviewPhase_Ready(
completionId: null == completionId ? _self.completionId : completionId // ignore: cast_nullable_to_non_nullable
as String,completionRevision: null == completionRevision ? _self.completionRevision : completionRevision // ignore: cast_nullable_to_non_nullable
as int,verificationSummary: null == verificationSummary ? _self.verificationSummary : verificationSummary // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeWaitingReviewPhase_Reviewing extends BridgeWaitingReviewPhase {
  const BridgeWaitingReviewPhase_Reviewing({required this.completionId, required this.completionRevision, required this.reviewRoundId, required this.verificationSummary}): super._();


 final  String completionId;
 final  int completionRevision;
 final  String reviewRoundId;
 final  String verificationSummary;

/// Create a copy of BridgeWaitingReviewPhase
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeWaitingReviewPhase_ReviewingCopyWith<BridgeWaitingReviewPhase_Reviewing> get copyWith => _$BridgeWaitingReviewPhase_ReviewingCopyWithImpl<BridgeWaitingReviewPhase_Reviewing>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeWaitingReviewPhase_Reviewing&&(identical(other.completionId, completionId) || other.completionId == completionId)&&(identical(other.completionRevision, completionRevision) || other.completionRevision == completionRevision)&&(identical(other.reviewRoundId, reviewRoundId) || other.reviewRoundId == reviewRoundId)&&(identical(other.verificationSummary, verificationSummary) || other.verificationSummary == verificationSummary));
}


@override
int get hashCode => Object.hash(runtimeType,completionId,completionRevision,reviewRoundId,verificationSummary);

@override
String toString() {
  return 'BridgeWaitingReviewPhase.reviewing(completionId: $completionId, completionRevision: $completionRevision, reviewRoundId: $reviewRoundId, verificationSummary: $verificationSummary)';
}


}

/// @nodoc
abstract mixin class $BridgeWaitingReviewPhase_ReviewingCopyWith<$Res> implements $BridgeWaitingReviewPhaseCopyWith<$Res> {
  factory $BridgeWaitingReviewPhase_ReviewingCopyWith(BridgeWaitingReviewPhase_Reviewing value, $Res Function(BridgeWaitingReviewPhase_Reviewing) _then) = _$BridgeWaitingReviewPhase_ReviewingCopyWithImpl;
@useResult
$Res call({
 String completionId, int completionRevision, String reviewRoundId, String verificationSummary
});




}
/// @nodoc
class _$BridgeWaitingReviewPhase_ReviewingCopyWithImpl<$Res>
    implements $BridgeWaitingReviewPhase_ReviewingCopyWith<$Res> {
  _$BridgeWaitingReviewPhase_ReviewingCopyWithImpl(this._self, this._then);

  final BridgeWaitingReviewPhase_Reviewing _self;
  final $Res Function(BridgeWaitingReviewPhase_Reviewing) _then;

/// Create a copy of BridgeWaitingReviewPhase
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? completionId = null,Object? completionRevision = null,Object? reviewRoundId = null,Object? verificationSummary = null,}) {
  return _then(BridgeWaitingReviewPhase_Reviewing(
completionId: null == completionId ? _self.completionId : completionId // ignore: cast_nullable_to_non_nullable
as String,completionRevision: null == completionRevision ? _self.completionRevision : completionRevision // ignore: cast_nullable_to_non_nullable
as int,reviewRoundId: null == reviewRoundId ? _self.reviewRoundId : reviewRoundId // ignore: cast_nullable_to_non_nullable
as String,verificationSummary: null == verificationSummary ? _self.verificationSummary : verificationSummary // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeWorkUnitCompletionOutcome {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeWorkUnitCompletionOutcome);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeWorkUnitCompletionOutcome()';
}


}

/// @nodoc
class $BridgeWorkUnitCompletionOutcomeCopyWith<$Res>  {
$BridgeWorkUnitCompletionOutcomeCopyWith(BridgeWorkUnitCompletionOutcome _, $Res Function(BridgeWorkUnitCompletionOutcome) __);
}


/// Adds pattern-matching-related methods to [BridgeWorkUnitCompletionOutcome].
extension BridgeWorkUnitCompletionOutcomePatterns on BridgeWorkUnitCompletionOutcome {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeWorkUnitCompletionOutcome_Merged value)?  merged,TResult Function( BridgeWorkUnitCompletionOutcome_NoDelivery value)?  noDelivery,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeWorkUnitCompletionOutcome_Merged() when merged != null:
return merged(_that);case BridgeWorkUnitCompletionOutcome_NoDelivery() when noDelivery != null:
return noDelivery(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeWorkUnitCompletionOutcome_Merged value)  merged,required TResult Function( BridgeWorkUnitCompletionOutcome_NoDelivery value)  noDelivery,}){
final _that = this;
switch (_that) {
case BridgeWorkUnitCompletionOutcome_Merged():
return merged(_that);case BridgeWorkUnitCompletionOutcome_NoDelivery():
return noDelivery(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeWorkUnitCompletionOutcome_Merged value)?  merged,TResult? Function( BridgeWorkUnitCompletionOutcome_NoDelivery value)?  noDelivery,}){
final _that = this;
switch (_that) {
case BridgeWorkUnitCompletionOutcome_Merged() when merged != null:
return merged(_that);case BridgeWorkUnitCompletionOutcome_NoDelivery() when noDelivery != null:
return noDelivery(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String mergeRecordId)?  merged,TResult Function( String completionId)?  noDelivery,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeWorkUnitCompletionOutcome_Merged() when merged != null:
return merged(_that.mergeRecordId);case BridgeWorkUnitCompletionOutcome_NoDelivery() when noDelivery != null:
return noDelivery(_that.completionId);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String mergeRecordId)  merged,required TResult Function( String completionId)  noDelivery,}) {final _that = this;
switch (_that) {
case BridgeWorkUnitCompletionOutcome_Merged():
return merged(_that.mergeRecordId);case BridgeWorkUnitCompletionOutcome_NoDelivery():
return noDelivery(_that.completionId);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String mergeRecordId)?  merged,TResult? Function( String completionId)?  noDelivery,}) {final _that = this;
switch (_that) {
case BridgeWorkUnitCompletionOutcome_Merged() when merged != null:
return merged(_that.mergeRecordId);case BridgeWorkUnitCompletionOutcome_NoDelivery() when noDelivery != null:
return noDelivery(_that.completionId);case _:
  return null;

}
}

}

/// @nodoc


class BridgeWorkUnitCompletionOutcome_Merged extends BridgeWorkUnitCompletionOutcome {
  const BridgeWorkUnitCompletionOutcome_Merged({required this.mergeRecordId}): super._();


 final  String mergeRecordId;

/// Create a copy of BridgeWorkUnitCompletionOutcome
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeWorkUnitCompletionOutcome_MergedCopyWith<BridgeWorkUnitCompletionOutcome_Merged> get copyWith => _$BridgeWorkUnitCompletionOutcome_MergedCopyWithImpl<BridgeWorkUnitCompletionOutcome_Merged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeWorkUnitCompletionOutcome_Merged&&(identical(other.mergeRecordId, mergeRecordId) || other.mergeRecordId == mergeRecordId));
}


@override
int get hashCode => Object.hash(runtimeType,mergeRecordId);

@override
String toString() {
  return 'BridgeWorkUnitCompletionOutcome.merged(mergeRecordId: $mergeRecordId)';
}


}

/// @nodoc
abstract mixin class $BridgeWorkUnitCompletionOutcome_MergedCopyWith<$Res> implements $BridgeWorkUnitCompletionOutcomeCopyWith<$Res> {
  factory $BridgeWorkUnitCompletionOutcome_MergedCopyWith(BridgeWorkUnitCompletionOutcome_Merged value, $Res Function(BridgeWorkUnitCompletionOutcome_Merged) _then) = _$BridgeWorkUnitCompletionOutcome_MergedCopyWithImpl;
@useResult
$Res call({
 String mergeRecordId
});




}
/// @nodoc
class _$BridgeWorkUnitCompletionOutcome_MergedCopyWithImpl<$Res>
    implements $BridgeWorkUnitCompletionOutcome_MergedCopyWith<$Res> {
  _$BridgeWorkUnitCompletionOutcome_MergedCopyWithImpl(this._self, this._then);

  final BridgeWorkUnitCompletionOutcome_Merged _self;
  final $Res Function(BridgeWorkUnitCompletionOutcome_Merged) _then;

/// Create a copy of BridgeWorkUnitCompletionOutcome
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? mergeRecordId = null,}) {
  return _then(BridgeWorkUnitCompletionOutcome_Merged(
mergeRecordId: null == mergeRecordId ? _self.mergeRecordId : mergeRecordId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeWorkUnitCompletionOutcome_NoDelivery extends BridgeWorkUnitCompletionOutcome {
  const BridgeWorkUnitCompletionOutcome_NoDelivery({required this.completionId}): super._();


 final  String completionId;

/// Create a copy of BridgeWorkUnitCompletionOutcome
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeWorkUnitCompletionOutcome_NoDeliveryCopyWith<BridgeWorkUnitCompletionOutcome_NoDelivery> get copyWith => _$BridgeWorkUnitCompletionOutcome_NoDeliveryCopyWithImpl<BridgeWorkUnitCompletionOutcome_NoDelivery>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeWorkUnitCompletionOutcome_NoDelivery&&(identical(other.completionId, completionId) || other.completionId == completionId));
}


@override
int get hashCode => Object.hash(runtimeType,completionId);

@override
String toString() {
  return 'BridgeWorkUnitCompletionOutcome.noDelivery(completionId: $completionId)';
}


}

/// @nodoc
abstract mixin class $BridgeWorkUnitCompletionOutcome_NoDeliveryCopyWith<$Res> implements $BridgeWorkUnitCompletionOutcomeCopyWith<$Res> {
  factory $BridgeWorkUnitCompletionOutcome_NoDeliveryCopyWith(BridgeWorkUnitCompletionOutcome_NoDelivery value, $Res Function(BridgeWorkUnitCompletionOutcome_NoDelivery) _then) = _$BridgeWorkUnitCompletionOutcome_NoDeliveryCopyWithImpl;
@useResult
$Res call({
 String completionId
});




}
/// @nodoc
class _$BridgeWorkUnitCompletionOutcome_NoDeliveryCopyWithImpl<$Res>
    implements $BridgeWorkUnitCompletionOutcome_NoDeliveryCopyWith<$Res> {
  _$BridgeWorkUnitCompletionOutcome_NoDeliveryCopyWithImpl(this._self, this._then);

  final BridgeWorkUnitCompletionOutcome_NoDelivery _self;
  final $Res Function(BridgeWorkUnitCompletionOutcome_NoDelivery) _then;

/// Create a copy of BridgeWorkUnitCompletionOutcome
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? completionId = null,}) {
  return _then(BridgeWorkUnitCompletionOutcome_NoDelivery(
completionId: null == completionId ? _self.completionId : completionId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeWorkUnitFailure {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeWorkUnitFailure);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeWorkUnitFailure()';
}


}

/// @nodoc
class $BridgeWorkUnitFailureCopyWith<$Res>  {
$BridgeWorkUnitFailureCopyWith(BridgeWorkUnitFailure _, $Res Function(BridgeWorkUnitFailure) __);
}


/// Adds pattern-matching-related methods to [BridgeWorkUnitFailure].
extension BridgeWorkUnitFailurePatterns on BridgeWorkUnitFailure {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeWorkUnitFailure_Spawn value)?  spawn,TResult Function( BridgeWorkUnitFailure_Execution value)?  execution,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeWorkUnitFailure_Spawn() when spawn != null:
return spawn(_that);case BridgeWorkUnitFailure_Execution() when execution != null:
return execution(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeWorkUnitFailure_Spawn value)  spawn,required TResult Function( BridgeWorkUnitFailure_Execution value)  execution,}){
final _that = this;
switch (_that) {
case BridgeWorkUnitFailure_Spawn():
return spawn(_that);case BridgeWorkUnitFailure_Execution():
return execution(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeWorkUnitFailure_Spawn value)?  spawn,TResult? Function( BridgeWorkUnitFailure_Execution value)?  execution,}){
final _that = this;
switch (_that) {
case BridgeWorkUnitFailure_Spawn() when spawn != null:
return spawn(_that);case BridgeWorkUnitFailure_Execution() when execution != null:
return execution(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeTaskSpawnFailure failure)?  spawn,TResult Function( String operationId,  String detail)?  execution,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeWorkUnitFailure_Spawn() when spawn != null:
return spawn(_that.failure);case BridgeWorkUnitFailure_Execution() when execution != null:
return execution(_that.operationId,_that.detail);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeTaskSpawnFailure failure)  spawn,required TResult Function( String operationId,  String detail)  execution,}) {final _that = this;
switch (_that) {
case BridgeWorkUnitFailure_Spawn():
return spawn(_that.failure);case BridgeWorkUnitFailure_Execution():
return execution(_that.operationId,_that.detail);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeTaskSpawnFailure failure)?  spawn,TResult? Function( String operationId,  String detail)?  execution,}) {final _that = this;
switch (_that) {
case BridgeWorkUnitFailure_Spawn() when spawn != null:
return spawn(_that.failure);case BridgeWorkUnitFailure_Execution() when execution != null:
return execution(_that.operationId,_that.detail);case _:
  return null;

}
}

}

/// @nodoc


class BridgeWorkUnitFailure_Spawn extends BridgeWorkUnitFailure {
  const BridgeWorkUnitFailure_Spawn({required this.failure}): super._();


 final  BridgeTaskSpawnFailure failure;

/// Create a copy of BridgeWorkUnitFailure
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeWorkUnitFailure_SpawnCopyWith<BridgeWorkUnitFailure_Spawn> get copyWith => _$BridgeWorkUnitFailure_SpawnCopyWithImpl<BridgeWorkUnitFailure_Spawn>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeWorkUnitFailure_Spawn&&(identical(other.failure, failure) || other.failure == failure));
}


@override
int get hashCode => Object.hash(runtimeType,failure);

@override
String toString() {
  return 'BridgeWorkUnitFailure.spawn(failure: $failure)';
}


}

/// @nodoc
abstract mixin class $BridgeWorkUnitFailure_SpawnCopyWith<$Res> implements $BridgeWorkUnitFailureCopyWith<$Res> {
  factory $BridgeWorkUnitFailure_SpawnCopyWith(BridgeWorkUnitFailure_Spawn value, $Res Function(BridgeWorkUnitFailure_Spawn) _then) = _$BridgeWorkUnitFailure_SpawnCopyWithImpl;
@useResult
$Res call({
 BridgeTaskSpawnFailure failure
});




}
/// @nodoc
class _$BridgeWorkUnitFailure_SpawnCopyWithImpl<$Res>
    implements $BridgeWorkUnitFailure_SpawnCopyWith<$Res> {
  _$BridgeWorkUnitFailure_SpawnCopyWithImpl(this._self, this._then);

  final BridgeWorkUnitFailure_Spawn _self;
  final $Res Function(BridgeWorkUnitFailure_Spawn) _then;

/// Create a copy of BridgeWorkUnitFailure
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? failure = null,}) {
  return _then(BridgeWorkUnitFailure_Spawn(
failure: null == failure ? _self.failure : failure // ignore: cast_nullable_to_non_nullable
as BridgeTaskSpawnFailure,
  ));
}


}

/// @nodoc


class BridgeWorkUnitFailure_Execution extends BridgeWorkUnitFailure {
  const BridgeWorkUnitFailure_Execution({required this.operationId, required this.detail}): super._();


 final  String operationId;
 final  String detail;

/// Create a copy of BridgeWorkUnitFailure
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeWorkUnitFailure_ExecutionCopyWith<BridgeWorkUnitFailure_Execution> get copyWith => _$BridgeWorkUnitFailure_ExecutionCopyWithImpl<BridgeWorkUnitFailure_Execution>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeWorkUnitFailure_Execution&&(identical(other.operationId, operationId) || other.operationId == operationId)&&(identical(other.detail, detail) || other.detail == detail));
}


@override
int get hashCode => Object.hash(runtimeType,operationId,detail);

@override
String toString() {
  return 'BridgeWorkUnitFailure.execution(operationId: $operationId, detail: $detail)';
}


}

/// @nodoc
abstract mixin class $BridgeWorkUnitFailure_ExecutionCopyWith<$Res> implements $BridgeWorkUnitFailureCopyWith<$Res> {
  factory $BridgeWorkUnitFailure_ExecutionCopyWith(BridgeWorkUnitFailure_Execution value, $Res Function(BridgeWorkUnitFailure_Execution) _then) = _$BridgeWorkUnitFailure_ExecutionCopyWithImpl;
@useResult
$Res call({
 String operationId, String detail
});




}
/// @nodoc
class _$BridgeWorkUnitFailure_ExecutionCopyWithImpl<$Res>
    implements $BridgeWorkUnitFailure_ExecutionCopyWith<$Res> {
  _$BridgeWorkUnitFailure_ExecutionCopyWithImpl(this._self, this._then);

  final BridgeWorkUnitFailure_Execution _self;
  final $Res Function(BridgeWorkUnitFailure_Execution) _then;

/// Create a copy of BridgeWorkUnitFailure
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? operationId = null,Object? detail = null,}) {
  return _then(BridgeWorkUnitFailure_Execution(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,detail: null == detail ? _self.detail : detail // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeWorkUnitPauseReason {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeWorkUnitPauseReason);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeWorkUnitPauseReason()';
}


}

/// @nodoc
class $BridgeWorkUnitPauseReasonCopyWith<$Res>  {
$BridgeWorkUnitPauseReasonCopyWith(BridgeWorkUnitPauseReason _, $Res Function(BridgeWorkUnitPauseReason) __);
}


/// Adds pattern-matching-related methods to [BridgeWorkUnitPauseReason].
extension BridgeWorkUnitPauseReasonPatterns on BridgeWorkUnitPauseReason {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeWorkUnitPauseReason_Budget value)?  budget,TResult Function( BridgeWorkUnitPauseReason_Operational value)?  operational,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeWorkUnitPauseReason_Budget() when budget != null:
return budget(_that);case BridgeWorkUnitPauseReason_Operational() when operational != null:
return operational(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeWorkUnitPauseReason_Budget value)  budget,required TResult Function( BridgeWorkUnitPauseReason_Operational value)  operational,}){
final _that = this;
switch (_that) {
case BridgeWorkUnitPauseReason_Budget():
return budget(_that);case BridgeWorkUnitPauseReason_Operational():
return operational(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeWorkUnitPauseReason_Budget value)?  budget,TResult? Function( BridgeWorkUnitPauseReason_Operational value)?  operational,}){
final _that = this;
switch (_that) {
case BridgeWorkUnitPauseReason_Budget() when budget != null:
return budget(_that);case BridgeWorkUnitPauseReason_Operational() when operational != null:
return operational(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeBudgetLimitDto limit)?  budget,TResult Function( String operationId,  String detail)?  operational,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeWorkUnitPauseReason_Budget() when budget != null:
return budget(_that.limit);case BridgeWorkUnitPauseReason_Operational() when operational != null:
return operational(_that.operationId,_that.detail);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeBudgetLimitDto limit)  budget,required TResult Function( String operationId,  String detail)  operational,}) {final _that = this;
switch (_that) {
case BridgeWorkUnitPauseReason_Budget():
return budget(_that.limit);case BridgeWorkUnitPauseReason_Operational():
return operational(_that.operationId,_that.detail);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeBudgetLimitDto limit)?  budget,TResult? Function( String operationId,  String detail)?  operational,}) {final _that = this;
switch (_that) {
case BridgeWorkUnitPauseReason_Budget() when budget != null:
return budget(_that.limit);case BridgeWorkUnitPauseReason_Operational() when operational != null:
return operational(_that.operationId,_that.detail);case _:
  return null;

}
}

}

/// @nodoc


class BridgeWorkUnitPauseReason_Budget extends BridgeWorkUnitPauseReason {
  const BridgeWorkUnitPauseReason_Budget({required this.limit}): super._();


 final  BridgeBudgetLimitDto limit;

/// Create a copy of BridgeWorkUnitPauseReason
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeWorkUnitPauseReason_BudgetCopyWith<BridgeWorkUnitPauseReason_Budget> get copyWith => _$BridgeWorkUnitPauseReason_BudgetCopyWithImpl<BridgeWorkUnitPauseReason_Budget>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeWorkUnitPauseReason_Budget&&(identical(other.limit, limit) || other.limit == limit));
}


@override
int get hashCode => Object.hash(runtimeType,limit);

@override
String toString() {
  return 'BridgeWorkUnitPauseReason.budget(limit: $limit)';
}


}

/// @nodoc
abstract mixin class $BridgeWorkUnitPauseReason_BudgetCopyWith<$Res> implements $BridgeWorkUnitPauseReasonCopyWith<$Res> {
  factory $BridgeWorkUnitPauseReason_BudgetCopyWith(BridgeWorkUnitPauseReason_Budget value, $Res Function(BridgeWorkUnitPauseReason_Budget) _then) = _$BridgeWorkUnitPauseReason_BudgetCopyWithImpl;
@useResult
$Res call({
 BridgeBudgetLimitDto limit
});




}
/// @nodoc
class _$BridgeWorkUnitPauseReason_BudgetCopyWithImpl<$Res>
    implements $BridgeWorkUnitPauseReason_BudgetCopyWith<$Res> {
  _$BridgeWorkUnitPauseReason_BudgetCopyWithImpl(this._self, this._then);

  final BridgeWorkUnitPauseReason_Budget _self;
  final $Res Function(BridgeWorkUnitPauseReason_Budget) _then;

/// Create a copy of BridgeWorkUnitPauseReason
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? limit = null,}) {
  return _then(BridgeWorkUnitPauseReason_Budget(
limit: null == limit ? _self.limit : limit // ignore: cast_nullable_to_non_nullable
as BridgeBudgetLimitDto,
  ));
}


}

/// @nodoc


class BridgeWorkUnitPauseReason_Operational extends BridgeWorkUnitPauseReason {
  const BridgeWorkUnitPauseReason_Operational({required this.operationId, required this.detail}): super._();


 final  String operationId;
 final  String detail;

/// Create a copy of BridgeWorkUnitPauseReason
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeWorkUnitPauseReason_OperationalCopyWith<BridgeWorkUnitPauseReason_Operational> get copyWith => _$BridgeWorkUnitPauseReason_OperationalCopyWithImpl<BridgeWorkUnitPauseReason_Operational>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeWorkUnitPauseReason_Operational&&(identical(other.operationId, operationId) || other.operationId == operationId)&&(identical(other.detail, detail) || other.detail == detail));
}


@override
int get hashCode => Object.hash(runtimeType,operationId,detail);

@override
String toString() {
  return 'BridgeWorkUnitPauseReason.operational(operationId: $operationId, detail: $detail)';
}


}

/// @nodoc
abstract mixin class $BridgeWorkUnitPauseReason_OperationalCopyWith<$Res> implements $BridgeWorkUnitPauseReasonCopyWith<$Res> {
  factory $BridgeWorkUnitPauseReason_OperationalCopyWith(BridgeWorkUnitPauseReason_Operational value, $Res Function(BridgeWorkUnitPauseReason_Operational) _then) = _$BridgeWorkUnitPauseReason_OperationalCopyWithImpl;
@useResult
$Res call({
 String operationId, String detail
});




}
/// @nodoc
class _$BridgeWorkUnitPauseReason_OperationalCopyWithImpl<$Res>
    implements $BridgeWorkUnitPauseReason_OperationalCopyWith<$Res> {
  _$BridgeWorkUnitPauseReason_OperationalCopyWithImpl(this._self, this._then);

  final BridgeWorkUnitPauseReason_Operational _self;
  final $Res Function(BridgeWorkUnitPauseReason_Operational) _then;

/// Create a copy of BridgeWorkUnitPauseReason
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? operationId = null,Object? detail = null,}) {
  return _then(BridgeWorkUnitPauseReason_Operational(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,detail: null == detail ? _self.detail : detail // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

// dart format on

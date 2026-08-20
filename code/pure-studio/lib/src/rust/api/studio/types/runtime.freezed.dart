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

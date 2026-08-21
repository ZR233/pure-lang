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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTaskReviewState_Pending value)?  pending,TResult Function( BridgeTaskReviewState_Pass value)?  pass,TResult Function( BridgeTaskReviewState_ChangesRequired value)?  changesRequired,TResult Function( BridgeTaskReviewState_Blocked value)?  blocked,TResult Function( BridgeTaskReviewState_Failed value)?  failed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTaskReviewState_Pending() when pending != null:
return pending(_that);case BridgeTaskReviewState_Pass() when pass != null:
return pass(_that);case BridgeTaskReviewState_ChangesRequired() when changesRequired != null:
return changesRequired(_that);case BridgeTaskReviewState_Blocked() when blocked != null:
return blocked(_that);case BridgeTaskReviewState_Failed() when failed != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTaskReviewState_Pending value)  pending,required TResult Function( BridgeTaskReviewState_Pass value)  pass,required TResult Function( BridgeTaskReviewState_ChangesRequired value)  changesRequired,required TResult Function( BridgeTaskReviewState_Blocked value)  blocked,required TResult Function( BridgeTaskReviewState_Failed value)  failed,}){
final _that = this;
switch (_that) {
case BridgeTaskReviewState_Pending():
return pending(_that);case BridgeTaskReviewState_Pass():
return pass(_that);case BridgeTaskReviewState_ChangesRequired():
return changesRequired(_that);case BridgeTaskReviewState_Blocked():
return blocked(_that);case BridgeTaskReviewState_Failed():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTaskReviewState_Pending value)?  pending,TResult? Function( BridgeTaskReviewState_Pass value)?  pass,TResult? Function( BridgeTaskReviewState_ChangesRequired value)?  changesRequired,TResult? Function( BridgeTaskReviewState_Blocked value)?  blocked,TResult? Function( BridgeTaskReviewState_Failed value)?  failed,}){
final _that = this;
switch (_that) {
case BridgeTaskReviewState_Pending() when pending != null:
return pending(_that);case BridgeTaskReviewState_Pass() when pass != null:
return pass(_that);case BridgeTaskReviewState_ChangesRequired() when changesRequired != null:
return changesRequired(_that);case BridgeTaskReviewState_Blocked() when blocked != null:
return blocked(_that);case BridgeTaskReviewState_Failed() when failed != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgePendingReviewerState reviewer)?  pending,TResult Function( String summary)?  pass,TResult Function( String summary)?  changesRequired,TResult Function( String summary)?  blocked,TResult Function( BridgeFailedReviewerState reviewer,  String error,  String summary)?  failed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTaskReviewState_Pending() when pending != null:
return pending(_that.reviewer);case BridgeTaskReviewState_Pass() when pass != null:
return pass(_that.summary);case BridgeTaskReviewState_ChangesRequired() when changesRequired != null:
return changesRequired(_that.summary);case BridgeTaskReviewState_Blocked() when blocked != null:
return blocked(_that.summary);case BridgeTaskReviewState_Failed() when failed != null:
return failed(_that.reviewer,_that.error,_that.summary);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgePendingReviewerState reviewer)  pending,required TResult Function( String summary)  pass,required TResult Function( String summary)  changesRequired,required TResult Function( String summary)  blocked,required TResult Function( BridgeFailedReviewerState reviewer,  String error,  String summary)  failed,}) {final _that = this;
switch (_that) {
case BridgeTaskReviewState_Pending():
return pending(_that.reviewer);case BridgeTaskReviewState_Pass():
return pass(_that.summary);case BridgeTaskReviewState_ChangesRequired():
return changesRequired(_that.summary);case BridgeTaskReviewState_Blocked():
return blocked(_that.summary);case BridgeTaskReviewState_Failed():
return failed(_that.reviewer,_that.error,_that.summary);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgePendingReviewerState reviewer)?  pending,TResult? Function( String summary)?  pass,TResult? Function( String summary)?  changesRequired,TResult? Function( String summary)?  blocked,TResult? Function( BridgeFailedReviewerState reviewer,  String error,  String summary)?  failed,}) {final _that = this;
switch (_that) {
case BridgeTaskReviewState_Pending() when pending != null:
return pending(_that.reviewer);case BridgeTaskReviewState_Pass() when pass != null:
return pass(_that.summary);case BridgeTaskReviewState_ChangesRequired() when changesRequired != null:
return changesRequired(_that.summary);case BridgeTaskReviewState_Blocked() when blocked != null:
return blocked(_that.summary);case BridgeTaskReviewState_Failed() when failed != null:
return failed(_that.reviewer,_that.error,_that.summary);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTaskReviewState_Pending extends BridgeTaskReviewState {
  const BridgeTaskReviewState_Pending({required this.reviewer}): super._();


 final  BridgePendingReviewerState reviewer;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewState_PendingCopyWith<BridgeTaskReviewState_Pending> get copyWith => _$BridgeTaskReviewState_PendingCopyWithImpl<BridgeTaskReviewState_Pending>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewState_Pending&&(identical(other.reviewer, reviewer) || other.reviewer == reviewer));
}


@override
int get hashCode => Object.hash(runtimeType,reviewer);

@override
String toString() {
  return 'BridgeTaskReviewState.pending(reviewer: $reviewer)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewState_PendingCopyWith<$Res> implements $BridgeTaskReviewStateCopyWith<$Res> {
  factory $BridgeTaskReviewState_PendingCopyWith(BridgeTaskReviewState_Pending value, $Res Function(BridgeTaskReviewState_Pending) _then) = _$BridgeTaskReviewState_PendingCopyWithImpl;
@useResult
$Res call({
 BridgePendingReviewerState reviewer
});




}
/// @nodoc
class _$BridgeTaskReviewState_PendingCopyWithImpl<$Res>
    implements $BridgeTaskReviewState_PendingCopyWith<$Res> {
  _$BridgeTaskReviewState_PendingCopyWithImpl(this._self, this._then);

  final BridgeTaskReviewState_Pending _self;
  final $Res Function(BridgeTaskReviewState_Pending) _then;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reviewer = null,}) {
  return _then(BridgeTaskReviewState_Pending(
reviewer: null == reviewer ? _self.reviewer : reviewer // ignore: cast_nullable_to_non_nullable
as BridgePendingReviewerState,
  ));
}


}

/// @nodoc


class BridgeTaskReviewState_Pass extends BridgeTaskReviewState {
  const BridgeTaskReviewState_Pass({required this.summary}): super._();


 final  String summary;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewState_PassCopyWith<BridgeTaskReviewState_Pass> get copyWith => _$BridgeTaskReviewState_PassCopyWithImpl<BridgeTaskReviewState_Pass>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewState_Pass&&(identical(other.summary, summary) || other.summary == summary));
}


@override
int get hashCode => Object.hash(runtimeType,summary);

@override
String toString() {
  return 'BridgeTaskReviewState.pass(summary: $summary)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewState_PassCopyWith<$Res> implements $BridgeTaskReviewStateCopyWith<$Res> {
  factory $BridgeTaskReviewState_PassCopyWith(BridgeTaskReviewState_Pass value, $Res Function(BridgeTaskReviewState_Pass) _then) = _$BridgeTaskReviewState_PassCopyWithImpl;
@useResult
$Res call({
 String summary
});




}
/// @nodoc
class _$BridgeTaskReviewState_PassCopyWithImpl<$Res>
    implements $BridgeTaskReviewState_PassCopyWith<$Res> {
  _$BridgeTaskReviewState_PassCopyWithImpl(this._self, this._then);

  final BridgeTaskReviewState_Pass _self;
  final $Res Function(BridgeTaskReviewState_Pass) _then;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? summary = null,}) {
  return _then(BridgeTaskReviewState_Pass(
summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeTaskReviewState_ChangesRequired extends BridgeTaskReviewState {
  const BridgeTaskReviewState_ChangesRequired({required this.summary}): super._();


 final  String summary;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewState_ChangesRequiredCopyWith<BridgeTaskReviewState_ChangesRequired> get copyWith => _$BridgeTaskReviewState_ChangesRequiredCopyWithImpl<BridgeTaskReviewState_ChangesRequired>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewState_ChangesRequired&&(identical(other.summary, summary) || other.summary == summary));
}


@override
int get hashCode => Object.hash(runtimeType,summary);

@override
String toString() {
  return 'BridgeTaskReviewState.changesRequired(summary: $summary)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewState_ChangesRequiredCopyWith<$Res> implements $BridgeTaskReviewStateCopyWith<$Res> {
  factory $BridgeTaskReviewState_ChangesRequiredCopyWith(BridgeTaskReviewState_ChangesRequired value, $Res Function(BridgeTaskReviewState_ChangesRequired) _then) = _$BridgeTaskReviewState_ChangesRequiredCopyWithImpl;
@useResult
$Res call({
 String summary
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
@pragma('vm:prefer-inline') $Res call({Object? summary = null,}) {
  return _then(BridgeTaskReviewState_ChangesRequired(
summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeTaskReviewState_Blocked extends BridgeTaskReviewState {
  const BridgeTaskReviewState_Blocked({required this.summary}): super._();


 final  String summary;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewState_BlockedCopyWith<BridgeTaskReviewState_Blocked> get copyWith => _$BridgeTaskReviewState_BlockedCopyWithImpl<BridgeTaskReviewState_Blocked>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewState_Blocked&&(identical(other.summary, summary) || other.summary == summary));
}


@override
int get hashCode => Object.hash(runtimeType,summary);

@override
String toString() {
  return 'BridgeTaskReviewState.blocked(summary: $summary)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewState_BlockedCopyWith<$Res> implements $BridgeTaskReviewStateCopyWith<$Res> {
  factory $BridgeTaskReviewState_BlockedCopyWith(BridgeTaskReviewState_Blocked value, $Res Function(BridgeTaskReviewState_Blocked) _then) = _$BridgeTaskReviewState_BlockedCopyWithImpl;
@useResult
$Res call({
 String summary
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
@pragma('vm:prefer-inline') $Res call({Object? summary = null,}) {
  return _then(BridgeTaskReviewState_Blocked(
summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeTaskReviewState_Failed extends BridgeTaskReviewState {
  const BridgeTaskReviewState_Failed({required this.reviewer, required this.error, required this.summary}): super._();


 final  BridgeFailedReviewerState reviewer;
 final  String error;
 final  String summary;

/// Create a copy of BridgeTaskReviewState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewState_FailedCopyWith<BridgeTaskReviewState_Failed> get copyWith => _$BridgeTaskReviewState_FailedCopyWithImpl<BridgeTaskReviewState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewState_Failed&&(identical(other.reviewer, reviewer) || other.reviewer == reviewer)&&(identical(other.error, error) || other.error == error)&&(identical(other.summary, summary) || other.summary == summary));
}


@override
int get hashCode => Object.hash(runtimeType,reviewer,error,summary);

@override
String toString() {
  return 'BridgeTaskReviewState.failed(reviewer: $reviewer, error: $error, summary: $summary)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewState_FailedCopyWith<$Res> implements $BridgeTaskReviewStateCopyWith<$Res> {
  factory $BridgeTaskReviewState_FailedCopyWith(BridgeTaskReviewState_Failed value, $Res Function(BridgeTaskReviewState_Failed) _then) = _$BridgeTaskReviewState_FailedCopyWithImpl;
@useResult
$Res call({
 BridgeFailedReviewerState reviewer, String error, String summary
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
@pragma('vm:prefer-inline') $Res call({Object? reviewer = null,Object? error = null,Object? summary = null,}) {
  return _then(BridgeTaskReviewState_Failed(
reviewer: null == reviewer ? _self.reviewer : reviewer // ignore: cast_nullable_to_non_nullable
as BridgeFailedReviewerState,error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as String,summary: null == summary ? _self.summary : summary // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeTaskReviewTarget {

 String get reviewedHead;
/// Create a copy of BridgeTaskReviewTarget
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewTargetCopyWith<BridgeTaskReviewTarget> get copyWith => _$BridgeTaskReviewTargetCopyWithImpl<BridgeTaskReviewTarget>(this as BridgeTaskReviewTarget, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewTarget&&(identical(other.reviewedHead, reviewedHead) || other.reviewedHead == reviewedHead));
}


@override
int get hashCode => Object.hash(runtimeType,reviewedHead);

@override
String toString() {
  return 'BridgeTaskReviewTarget(reviewedHead: $reviewedHead)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewTargetCopyWith<$Res>  {
  factory $BridgeTaskReviewTargetCopyWith(BridgeTaskReviewTarget value, $Res Function(BridgeTaskReviewTarget) _then) = _$BridgeTaskReviewTargetCopyWithImpl;
@useResult
$Res call({
 String reviewedHead
});




}
/// @nodoc
class _$BridgeTaskReviewTargetCopyWithImpl<$Res>
    implements $BridgeTaskReviewTargetCopyWith<$Res> {
  _$BridgeTaskReviewTargetCopyWithImpl(this._self, this._then);

  final BridgeTaskReviewTarget _self;
  final $Res Function(BridgeTaskReviewTarget) _then;

/// Create a copy of BridgeTaskReviewTarget
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? reviewedHead = null,}) {
  return _then(_self.copyWith(
reviewedHead: null == reviewedHead ? _self.reviewedHead : reviewedHead // ignore: cast_nullable_to_non_nullable
as String,
  ));
}

}


/// Adds pattern-matching-related methods to [BridgeTaskReviewTarget].
extension BridgeTaskReviewTargetPatterns on BridgeTaskReviewTarget {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTaskReviewTarget_Delivery value)?  delivery,TResult Function( BridgeTaskReviewTarget_Integration value)?  integration,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTaskReviewTarget_Delivery() when delivery != null:
return delivery(_that);case BridgeTaskReviewTarget_Integration() when integration != null:
return integration(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTaskReviewTarget_Delivery value)  delivery,required TResult Function( BridgeTaskReviewTarget_Integration value)  integration,}){
final _that = this;
switch (_that) {
case BridgeTaskReviewTarget_Delivery():
return delivery(_that);case BridgeTaskReviewTarget_Integration():
return integration(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTaskReviewTarget_Delivery value)?  delivery,TResult? Function( BridgeTaskReviewTarget_Integration value)?  integration,}){
final _that = this;
switch (_that) {
case BridgeTaskReviewTarget_Delivery() when delivery != null:
return delivery(_that);case BridgeTaskReviewTarget_Integration() when integration != null:
return integration(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String workUnitId,  String completionId,  int completionRevision,  String reviewedHead)?  delivery,TResult Function( String reviewedHead)?  integration,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTaskReviewTarget_Delivery() when delivery != null:
return delivery(_that.workUnitId,_that.completionId,_that.completionRevision,_that.reviewedHead);case BridgeTaskReviewTarget_Integration() when integration != null:
return integration(_that.reviewedHead);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String workUnitId,  String completionId,  int completionRevision,  String reviewedHead)  delivery,required TResult Function( String reviewedHead)  integration,}) {final _that = this;
switch (_that) {
case BridgeTaskReviewTarget_Delivery():
return delivery(_that.workUnitId,_that.completionId,_that.completionRevision,_that.reviewedHead);case BridgeTaskReviewTarget_Integration():
return integration(_that.reviewedHead);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String workUnitId,  String completionId,  int completionRevision,  String reviewedHead)?  delivery,TResult? Function( String reviewedHead)?  integration,}) {final _that = this;
switch (_that) {
case BridgeTaskReviewTarget_Delivery() when delivery != null:
return delivery(_that.workUnitId,_that.completionId,_that.completionRevision,_that.reviewedHead);case BridgeTaskReviewTarget_Integration() when integration != null:
return integration(_that.reviewedHead);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTaskReviewTarget_Delivery extends BridgeTaskReviewTarget {
  const BridgeTaskReviewTarget_Delivery({required this.workUnitId, required this.completionId, required this.completionRevision, required this.reviewedHead}): super._();


 final  String workUnitId;
 final  String completionId;
 final  int completionRevision;
@override final  String reviewedHead;

/// Create a copy of BridgeTaskReviewTarget
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewTarget_DeliveryCopyWith<BridgeTaskReviewTarget_Delivery> get copyWith => _$BridgeTaskReviewTarget_DeliveryCopyWithImpl<BridgeTaskReviewTarget_Delivery>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewTarget_Delivery&&(identical(other.workUnitId, workUnitId) || other.workUnitId == workUnitId)&&(identical(other.completionId, completionId) || other.completionId == completionId)&&(identical(other.completionRevision, completionRevision) || other.completionRevision == completionRevision)&&(identical(other.reviewedHead, reviewedHead) || other.reviewedHead == reviewedHead));
}


@override
int get hashCode => Object.hash(runtimeType,workUnitId,completionId,completionRevision,reviewedHead);

@override
String toString() {
  return 'BridgeTaskReviewTarget.delivery(workUnitId: $workUnitId, completionId: $completionId, completionRevision: $completionRevision, reviewedHead: $reviewedHead)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewTarget_DeliveryCopyWith<$Res> implements $BridgeTaskReviewTargetCopyWith<$Res> {
  factory $BridgeTaskReviewTarget_DeliveryCopyWith(BridgeTaskReviewTarget_Delivery value, $Res Function(BridgeTaskReviewTarget_Delivery) _then) = _$BridgeTaskReviewTarget_DeliveryCopyWithImpl;
@override @useResult
$Res call({
 String workUnitId, String completionId, int completionRevision, String reviewedHead
});




}
/// @nodoc
class _$BridgeTaskReviewTarget_DeliveryCopyWithImpl<$Res>
    implements $BridgeTaskReviewTarget_DeliveryCopyWith<$Res> {
  _$BridgeTaskReviewTarget_DeliveryCopyWithImpl(this._self, this._then);

  final BridgeTaskReviewTarget_Delivery _self;
  final $Res Function(BridgeTaskReviewTarget_Delivery) _then;

/// Create a copy of BridgeTaskReviewTarget
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? workUnitId = null,Object? completionId = null,Object? completionRevision = null,Object? reviewedHead = null,}) {
  return _then(BridgeTaskReviewTarget_Delivery(
workUnitId: null == workUnitId ? _self.workUnitId : workUnitId // ignore: cast_nullable_to_non_nullable
as String,completionId: null == completionId ? _self.completionId : completionId // ignore: cast_nullable_to_non_nullable
as String,completionRevision: null == completionRevision ? _self.completionRevision : completionRevision // ignore: cast_nullable_to_non_nullable
as int,reviewedHead: null == reviewedHead ? _self.reviewedHead : reviewedHead // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeTaskReviewTarget_Integration extends BridgeTaskReviewTarget {
  const BridgeTaskReviewTarget_Integration({required this.reviewedHead}): super._();


@override final  String reviewedHead;

/// Create a copy of BridgeTaskReviewTarget
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskReviewTarget_IntegrationCopyWith<BridgeTaskReviewTarget_Integration> get copyWith => _$BridgeTaskReviewTarget_IntegrationCopyWithImpl<BridgeTaskReviewTarget_Integration>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskReviewTarget_Integration&&(identical(other.reviewedHead, reviewedHead) || other.reviewedHead == reviewedHead));
}


@override
int get hashCode => Object.hash(runtimeType,reviewedHead);

@override
String toString() {
  return 'BridgeTaskReviewTarget.integration(reviewedHead: $reviewedHead)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskReviewTarget_IntegrationCopyWith<$Res> implements $BridgeTaskReviewTargetCopyWith<$Res> {
  factory $BridgeTaskReviewTarget_IntegrationCopyWith(BridgeTaskReviewTarget_Integration value, $Res Function(BridgeTaskReviewTarget_Integration) _then) = _$BridgeTaskReviewTarget_IntegrationCopyWithImpl;
@override @useResult
$Res call({
 String reviewedHead
});




}
/// @nodoc
class _$BridgeTaskReviewTarget_IntegrationCopyWithImpl<$Res>
    implements $BridgeTaskReviewTarget_IntegrationCopyWith<$Res> {
  _$BridgeTaskReviewTarget_IntegrationCopyWithImpl(this._self, this._then);

  final BridgeTaskReviewTarget_Integration _self;
  final $Res Function(BridgeTaskReviewTarget_Integration) _then;

/// Create a copy of BridgeTaskReviewTarget
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? reviewedHead = null,}) {
  return _then(BridgeTaskReviewTarget_Integration(
reviewedHead: null == reviewedHead ? _self.reviewedHead : reviewedHead // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeTaskState {

 BridgeTaskStateData get field0;
/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskStateCopyWith<BridgeTaskState> get copyWith => _$BridgeTaskStateCopyWithImpl<BridgeTaskState>(this as BridgeTaskState, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskStateCopyWith<$Res>  {
  factory $BridgeTaskStateCopyWith(BridgeTaskState value, $Res Function(BridgeTaskState) _then) = _$BridgeTaskStateCopyWithImpl;
@useResult
$Res call({
 BridgeTaskStateData field0
});




}
/// @nodoc
class _$BridgeTaskStateCopyWithImpl<$Res>
    implements $BridgeTaskStateCopyWith<$Res> {
  _$BridgeTaskStateCopyWithImpl(this._self, this._then);

  final BridgeTaskState _self;
  final $Res Function(BridgeTaskState) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? field0 = null,}) {
  return _then(_self.copyWith(
field0: null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskStateData,
  ));
}

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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTaskState_DesignUpdating value)?  designUpdating,TResult Function( BridgeTaskState_Implementing value)?  implementing,TResult Function( BridgeTaskState_Merging value)?  merging,TResult Function( BridgeTaskState_Reviewing value)?  reviewing,TResult Function( BridgeTaskState_Reworking value)?  reworking,TResult Function( BridgeTaskState_Stopping value)?  stopping,TResult Function( BridgeTaskState_Blocked value)?  blocked,TResult Function( BridgeTaskState_Completed value)?  completed,TResult Function( BridgeTaskState_Failed value)?  failed,TResult Function( BridgeTaskState_Cancelled value)?  cancelled,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTaskState_DesignUpdating() when designUpdating != null:
return designUpdating(_that);case BridgeTaskState_Implementing() when implementing != null:
return implementing(_that);case BridgeTaskState_Merging() when merging != null:
return merging(_that);case BridgeTaskState_Reviewing() when reviewing != null:
return reviewing(_that);case BridgeTaskState_Reworking() when reworking != null:
return reworking(_that);case BridgeTaskState_Stopping() when stopping != null:
return stopping(_that);case BridgeTaskState_Blocked() when blocked != null:
return blocked(_that);case BridgeTaskState_Completed() when completed != null:
return completed(_that);case BridgeTaskState_Failed() when failed != null:
return failed(_that);case BridgeTaskState_Cancelled() when cancelled != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTaskState_DesignUpdating value)  designUpdating,required TResult Function( BridgeTaskState_Implementing value)  implementing,required TResult Function( BridgeTaskState_Merging value)  merging,required TResult Function( BridgeTaskState_Reviewing value)  reviewing,required TResult Function( BridgeTaskState_Reworking value)  reworking,required TResult Function( BridgeTaskState_Stopping value)  stopping,required TResult Function( BridgeTaskState_Blocked value)  blocked,required TResult Function( BridgeTaskState_Completed value)  completed,required TResult Function( BridgeTaskState_Failed value)  failed,required TResult Function( BridgeTaskState_Cancelled value)  cancelled,}){
final _that = this;
switch (_that) {
case BridgeTaskState_DesignUpdating():
return designUpdating(_that);case BridgeTaskState_Implementing():
return implementing(_that);case BridgeTaskState_Merging():
return merging(_that);case BridgeTaskState_Reviewing():
return reviewing(_that);case BridgeTaskState_Reworking():
return reworking(_that);case BridgeTaskState_Stopping():
return stopping(_that);case BridgeTaskState_Blocked():
return blocked(_that);case BridgeTaskState_Completed():
return completed(_that);case BridgeTaskState_Failed():
return failed(_that);case BridgeTaskState_Cancelled():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTaskState_DesignUpdating value)?  designUpdating,TResult? Function( BridgeTaskState_Implementing value)?  implementing,TResult? Function( BridgeTaskState_Merging value)?  merging,TResult? Function( BridgeTaskState_Reviewing value)?  reviewing,TResult? Function( BridgeTaskState_Reworking value)?  reworking,TResult? Function( BridgeTaskState_Stopping value)?  stopping,TResult? Function( BridgeTaskState_Blocked value)?  blocked,TResult? Function( BridgeTaskState_Completed value)?  completed,TResult? Function( BridgeTaskState_Failed value)?  failed,TResult? Function( BridgeTaskState_Cancelled value)?  cancelled,}){
final _that = this;
switch (_that) {
case BridgeTaskState_DesignUpdating() when designUpdating != null:
return designUpdating(_that);case BridgeTaskState_Implementing() when implementing != null:
return implementing(_that);case BridgeTaskState_Merging() when merging != null:
return merging(_that);case BridgeTaskState_Reviewing() when reviewing != null:
return reviewing(_that);case BridgeTaskState_Reworking() when reworking != null:
return reworking(_that);case BridgeTaskState_Stopping() when stopping != null:
return stopping(_that);case BridgeTaskState_Blocked() when blocked != null:
return blocked(_that);case BridgeTaskState_Completed() when completed != null:
return completed(_that);case BridgeTaskState_Failed() when failed != null:
return failed(_that);case BridgeTaskState_Cancelled() when cancelled != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeTaskStateData field0)?  designUpdating,TResult Function( BridgeTaskStateData field0)?  implementing,TResult Function( BridgeTaskStateData field0)?  merging,TResult Function( BridgeTaskStateData field0)?  reviewing,TResult Function( BridgeTaskStateData field0)?  reworking,TResult Function( BridgeTaskStateData field0)?  stopping,TResult Function( BridgeTaskStateData field0)?  blocked,TResult Function( BridgeTaskStateData field0)?  completed,TResult Function( BridgeTaskStateData field0)?  failed,TResult Function( BridgeTaskStateData field0)?  cancelled,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTaskState_DesignUpdating() when designUpdating != null:
return designUpdating(_that.field0);case BridgeTaskState_Implementing() when implementing != null:
return implementing(_that.field0);case BridgeTaskState_Merging() when merging != null:
return merging(_that.field0);case BridgeTaskState_Reviewing() when reviewing != null:
return reviewing(_that.field0);case BridgeTaskState_Reworking() when reworking != null:
return reworking(_that.field0);case BridgeTaskState_Stopping() when stopping != null:
return stopping(_that.field0);case BridgeTaskState_Blocked() when blocked != null:
return blocked(_that.field0);case BridgeTaskState_Completed() when completed != null:
return completed(_that.field0);case BridgeTaskState_Failed() when failed != null:
return failed(_that.field0);case BridgeTaskState_Cancelled() when cancelled != null:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeTaskStateData field0)  designUpdating,required TResult Function( BridgeTaskStateData field0)  implementing,required TResult Function( BridgeTaskStateData field0)  merging,required TResult Function( BridgeTaskStateData field0)  reviewing,required TResult Function( BridgeTaskStateData field0)  reworking,required TResult Function( BridgeTaskStateData field0)  stopping,required TResult Function( BridgeTaskStateData field0)  blocked,required TResult Function( BridgeTaskStateData field0)  completed,required TResult Function( BridgeTaskStateData field0)  failed,required TResult Function( BridgeTaskStateData field0)  cancelled,}) {final _that = this;
switch (_that) {
case BridgeTaskState_DesignUpdating():
return designUpdating(_that.field0);case BridgeTaskState_Implementing():
return implementing(_that.field0);case BridgeTaskState_Merging():
return merging(_that.field0);case BridgeTaskState_Reviewing():
return reviewing(_that.field0);case BridgeTaskState_Reworking():
return reworking(_that.field0);case BridgeTaskState_Stopping():
return stopping(_that.field0);case BridgeTaskState_Blocked():
return blocked(_that.field0);case BridgeTaskState_Completed():
return completed(_that.field0);case BridgeTaskState_Failed():
return failed(_that.field0);case BridgeTaskState_Cancelled():
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeTaskStateData field0)?  designUpdating,TResult? Function( BridgeTaskStateData field0)?  implementing,TResult? Function( BridgeTaskStateData field0)?  merging,TResult? Function( BridgeTaskStateData field0)?  reviewing,TResult? Function( BridgeTaskStateData field0)?  reworking,TResult? Function( BridgeTaskStateData field0)?  stopping,TResult? Function( BridgeTaskStateData field0)?  blocked,TResult? Function( BridgeTaskStateData field0)?  completed,TResult? Function( BridgeTaskStateData field0)?  failed,TResult? Function( BridgeTaskStateData field0)?  cancelled,}) {final _that = this;
switch (_that) {
case BridgeTaskState_DesignUpdating() when designUpdating != null:
return designUpdating(_that.field0);case BridgeTaskState_Implementing() when implementing != null:
return implementing(_that.field0);case BridgeTaskState_Merging() when merging != null:
return merging(_that.field0);case BridgeTaskState_Reviewing() when reviewing != null:
return reviewing(_that.field0);case BridgeTaskState_Reworking() when reworking != null:
return reworking(_that.field0);case BridgeTaskState_Stopping() when stopping != null:
return stopping(_that.field0);case BridgeTaskState_Blocked() when blocked != null:
return blocked(_that.field0);case BridgeTaskState_Completed() when completed != null:
return completed(_that.field0);case BridgeTaskState_Failed() when failed != null:
return failed(_that.field0);case BridgeTaskState_Cancelled() when cancelled != null:
return cancelled(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTaskState_DesignUpdating extends BridgeTaskState {
  const BridgeTaskState_DesignUpdating(this.field0): super._();


@override final  BridgeTaskStateData field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskState_DesignUpdatingCopyWith<BridgeTaskState_DesignUpdating> get copyWith => _$BridgeTaskState_DesignUpdatingCopyWithImpl<BridgeTaskState_DesignUpdating>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState_DesignUpdating&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState.designUpdating(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskState_DesignUpdatingCopyWith<$Res> implements $BridgeTaskStateCopyWith<$Res> {
  factory $BridgeTaskState_DesignUpdatingCopyWith(BridgeTaskState_DesignUpdating value, $Res Function(BridgeTaskState_DesignUpdating) _then) = _$BridgeTaskState_DesignUpdatingCopyWithImpl;
@override @useResult
$Res call({
 BridgeTaskStateData field0
});




}
/// @nodoc
class _$BridgeTaskState_DesignUpdatingCopyWithImpl<$Res>
    implements $BridgeTaskState_DesignUpdatingCopyWith<$Res> {
  _$BridgeTaskState_DesignUpdatingCopyWithImpl(this._self, this._then);

  final BridgeTaskState_DesignUpdating _self;
  final $Res Function(BridgeTaskState_DesignUpdating) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_DesignUpdating(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskStateData,
  ));
}


}

/// @nodoc


class BridgeTaskState_Implementing extends BridgeTaskState {
  const BridgeTaskState_Implementing(this.field0): super._();


@override final  BridgeTaskStateData field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskState_ImplementingCopyWith<BridgeTaskState_Implementing> get copyWith => _$BridgeTaskState_ImplementingCopyWithImpl<BridgeTaskState_Implementing>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState_Implementing&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState.implementing(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskState_ImplementingCopyWith<$Res> implements $BridgeTaskStateCopyWith<$Res> {
  factory $BridgeTaskState_ImplementingCopyWith(BridgeTaskState_Implementing value, $Res Function(BridgeTaskState_Implementing) _then) = _$BridgeTaskState_ImplementingCopyWithImpl;
@override @useResult
$Res call({
 BridgeTaskStateData field0
});




}
/// @nodoc
class _$BridgeTaskState_ImplementingCopyWithImpl<$Res>
    implements $BridgeTaskState_ImplementingCopyWith<$Res> {
  _$BridgeTaskState_ImplementingCopyWithImpl(this._self, this._then);

  final BridgeTaskState_Implementing _self;
  final $Res Function(BridgeTaskState_Implementing) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_Implementing(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskStateData,
  ));
}


}

/// @nodoc


class BridgeTaskState_Merging extends BridgeTaskState {
  const BridgeTaskState_Merging(this.field0): super._();


@override final  BridgeTaskStateData field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskState_MergingCopyWith<BridgeTaskState_Merging> get copyWith => _$BridgeTaskState_MergingCopyWithImpl<BridgeTaskState_Merging>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState_Merging&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState.merging(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskState_MergingCopyWith<$Res> implements $BridgeTaskStateCopyWith<$Res> {
  factory $BridgeTaskState_MergingCopyWith(BridgeTaskState_Merging value, $Res Function(BridgeTaskState_Merging) _then) = _$BridgeTaskState_MergingCopyWithImpl;
@override @useResult
$Res call({
 BridgeTaskStateData field0
});




}
/// @nodoc
class _$BridgeTaskState_MergingCopyWithImpl<$Res>
    implements $BridgeTaskState_MergingCopyWith<$Res> {
  _$BridgeTaskState_MergingCopyWithImpl(this._self, this._then);

  final BridgeTaskState_Merging _self;
  final $Res Function(BridgeTaskState_Merging) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_Merging(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskStateData,
  ));
}


}

/// @nodoc


class BridgeTaskState_Reviewing extends BridgeTaskState {
  const BridgeTaskState_Reviewing(this.field0): super._();


@override final  BridgeTaskStateData field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
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
@override @useResult
$Res call({
 BridgeTaskStateData field0
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
@override @pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_Reviewing(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskStateData,
  ));
}


}

/// @nodoc


class BridgeTaskState_Reworking extends BridgeTaskState {
  const BridgeTaskState_Reworking(this.field0): super._();


@override final  BridgeTaskStateData field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskState_ReworkingCopyWith<BridgeTaskState_Reworking> get copyWith => _$BridgeTaskState_ReworkingCopyWithImpl<BridgeTaskState_Reworking>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState_Reworking&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState.reworking(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskState_ReworkingCopyWith<$Res> implements $BridgeTaskStateCopyWith<$Res> {
  factory $BridgeTaskState_ReworkingCopyWith(BridgeTaskState_Reworking value, $Res Function(BridgeTaskState_Reworking) _then) = _$BridgeTaskState_ReworkingCopyWithImpl;
@override @useResult
$Res call({
 BridgeTaskStateData field0
});




}
/// @nodoc
class _$BridgeTaskState_ReworkingCopyWithImpl<$Res>
    implements $BridgeTaskState_ReworkingCopyWith<$Res> {
  _$BridgeTaskState_ReworkingCopyWithImpl(this._self, this._then);

  final BridgeTaskState_Reworking _self;
  final $Res Function(BridgeTaskState_Reworking) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_Reworking(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskStateData,
  ));
}


}

/// @nodoc


class BridgeTaskState_Stopping extends BridgeTaskState {
  const BridgeTaskState_Stopping(this.field0): super._();


@override final  BridgeTaskStateData field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskState_StoppingCopyWith<BridgeTaskState_Stopping> get copyWith => _$BridgeTaskState_StoppingCopyWithImpl<BridgeTaskState_Stopping>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState_Stopping&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState.stopping(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskState_StoppingCopyWith<$Res> implements $BridgeTaskStateCopyWith<$Res> {
  factory $BridgeTaskState_StoppingCopyWith(BridgeTaskState_Stopping value, $Res Function(BridgeTaskState_Stopping) _then) = _$BridgeTaskState_StoppingCopyWithImpl;
@override @useResult
$Res call({
 BridgeTaskStateData field0
});




}
/// @nodoc
class _$BridgeTaskState_StoppingCopyWithImpl<$Res>
    implements $BridgeTaskState_StoppingCopyWith<$Res> {
  _$BridgeTaskState_StoppingCopyWithImpl(this._self, this._then);

  final BridgeTaskState_Stopping _self;
  final $Res Function(BridgeTaskState_Stopping) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_Stopping(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskStateData,
  ));
}


}

/// @nodoc


class BridgeTaskState_Blocked extends BridgeTaskState {
  const BridgeTaskState_Blocked(this.field0): super._();


@override final  BridgeTaskStateData field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskState_BlockedCopyWith<BridgeTaskState_Blocked> get copyWith => _$BridgeTaskState_BlockedCopyWithImpl<BridgeTaskState_Blocked>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState_Blocked&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState.blocked(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskState_BlockedCopyWith<$Res> implements $BridgeTaskStateCopyWith<$Res> {
  factory $BridgeTaskState_BlockedCopyWith(BridgeTaskState_Blocked value, $Res Function(BridgeTaskState_Blocked) _then) = _$BridgeTaskState_BlockedCopyWithImpl;
@override @useResult
$Res call({
 BridgeTaskStateData field0
});




}
/// @nodoc
class _$BridgeTaskState_BlockedCopyWithImpl<$Res>
    implements $BridgeTaskState_BlockedCopyWith<$Res> {
  _$BridgeTaskState_BlockedCopyWithImpl(this._self, this._then);

  final BridgeTaskState_Blocked _self;
  final $Res Function(BridgeTaskState_Blocked) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_Blocked(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskStateData,
  ));
}


}

/// @nodoc


class BridgeTaskState_Completed extends BridgeTaskState {
  const BridgeTaskState_Completed(this.field0): super._();


@override final  BridgeTaskStateData field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
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
@override @useResult
$Res call({
 BridgeTaskStateData field0
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
@override @pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_Completed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskStateData,
  ));
}


}

/// @nodoc


class BridgeTaskState_Failed extends BridgeTaskState {
  const BridgeTaskState_Failed(this.field0): super._();


@override final  BridgeTaskStateData field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskState_FailedCopyWith<BridgeTaskState_Failed> get copyWith => _$BridgeTaskState_FailedCopyWithImpl<BridgeTaskState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState_Failed&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState.failed(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskState_FailedCopyWith<$Res> implements $BridgeTaskStateCopyWith<$Res> {
  factory $BridgeTaskState_FailedCopyWith(BridgeTaskState_Failed value, $Res Function(BridgeTaskState_Failed) _then) = _$BridgeTaskState_FailedCopyWithImpl;
@override @useResult
$Res call({
 BridgeTaskStateData field0
});




}
/// @nodoc
class _$BridgeTaskState_FailedCopyWithImpl<$Res>
    implements $BridgeTaskState_FailedCopyWith<$Res> {
  _$BridgeTaskState_FailedCopyWithImpl(this._self, this._then);

  final BridgeTaskState_Failed _self;
  final $Res Function(BridgeTaskState_Failed) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_Failed(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskStateData,
  ));
}


}

/// @nodoc


class BridgeTaskState_Cancelled extends BridgeTaskState {
  const BridgeTaskState_Cancelled(this.field0): super._();


@override final  BridgeTaskStateData field0;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskState_CancelledCopyWith<BridgeTaskState_Cancelled> get copyWith => _$BridgeTaskState_CancelledCopyWithImpl<BridgeTaskState_Cancelled>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskState_Cancelled&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskState.cancelled(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskState_CancelledCopyWith<$Res> implements $BridgeTaskStateCopyWith<$Res> {
  factory $BridgeTaskState_CancelledCopyWith(BridgeTaskState_Cancelled value, $Res Function(BridgeTaskState_Cancelled) _then) = _$BridgeTaskState_CancelledCopyWithImpl;
@override @useResult
$Res call({
 BridgeTaskStateData field0
});




}
/// @nodoc
class _$BridgeTaskState_CancelledCopyWithImpl<$Res>
    implements $BridgeTaskState_CancelledCopyWith<$Res> {
  _$BridgeTaskState_CancelledCopyWithImpl(this._self, this._then);

  final BridgeTaskState_Cancelled _self;
  final $Res Function(BridgeTaskState_Cancelled) _then;

/// Create a copy of BridgeTaskState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskState_Cancelled(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskStateData,
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTaskWorkUnitState_Pending value)?  pending,TResult Function( BridgeTaskWorkUnitState_Running value)?  running,TResult Function( BridgeTaskWorkUnitState_AwaitingCompletion value)?  awaitingCompletion,TResult Function( BridgeTaskWorkUnitState_ReadyForReview value)?  readyForReview,TResult Function( BridgeTaskWorkUnitState_Reviewing value)?  reviewing,TResult Function( BridgeTaskWorkUnitState_ChangesRequested value)?  changesRequested,TResult Function( BridgeTaskWorkUnitState_Approved value)?  approved,TResult Function( BridgeTaskWorkUnitState_Merged value)?  merged,TResult Function( BridgeTaskWorkUnitState_NoDelivery value)?  noDelivery,TResult Function( BridgeTaskWorkUnitState_NeedsAttention value)?  needsAttention,TResult Function( BridgeTaskWorkUnitState_Failed value)?  failed,TResult Function( BridgeTaskWorkUnitState_Cancelled value)?  cancelled,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTaskWorkUnitState_Pending() when pending != null:
return pending(_that);case BridgeTaskWorkUnitState_Running() when running != null:
return running(_that);case BridgeTaskWorkUnitState_AwaitingCompletion() when awaitingCompletion != null:
return awaitingCompletion(_that);case BridgeTaskWorkUnitState_ReadyForReview() when readyForReview != null:
return readyForReview(_that);case BridgeTaskWorkUnitState_Reviewing() when reviewing != null:
return reviewing(_that);case BridgeTaskWorkUnitState_ChangesRequested() when changesRequested != null:
return changesRequested(_that);case BridgeTaskWorkUnitState_Approved() when approved != null:
return approved(_that);case BridgeTaskWorkUnitState_Merged() when merged != null:
return merged(_that);case BridgeTaskWorkUnitState_NoDelivery() when noDelivery != null:
return noDelivery(_that);case BridgeTaskWorkUnitState_NeedsAttention() when needsAttention != null:
return needsAttention(_that);case BridgeTaskWorkUnitState_Failed() when failed != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTaskWorkUnitState_Pending value)  pending,required TResult Function( BridgeTaskWorkUnitState_Running value)  running,required TResult Function( BridgeTaskWorkUnitState_AwaitingCompletion value)  awaitingCompletion,required TResult Function( BridgeTaskWorkUnitState_ReadyForReview value)  readyForReview,required TResult Function( BridgeTaskWorkUnitState_Reviewing value)  reviewing,required TResult Function( BridgeTaskWorkUnitState_ChangesRequested value)  changesRequested,required TResult Function( BridgeTaskWorkUnitState_Approved value)  approved,required TResult Function( BridgeTaskWorkUnitState_Merged value)  merged,required TResult Function( BridgeTaskWorkUnitState_NoDelivery value)  noDelivery,required TResult Function( BridgeTaskWorkUnitState_NeedsAttention value)  needsAttention,required TResult Function( BridgeTaskWorkUnitState_Failed value)  failed,required TResult Function( BridgeTaskWorkUnitState_Cancelled value)  cancelled,}){
final _that = this;
switch (_that) {
case BridgeTaskWorkUnitState_Pending():
return pending(_that);case BridgeTaskWorkUnitState_Running():
return running(_that);case BridgeTaskWorkUnitState_AwaitingCompletion():
return awaitingCompletion(_that);case BridgeTaskWorkUnitState_ReadyForReview():
return readyForReview(_that);case BridgeTaskWorkUnitState_Reviewing():
return reviewing(_that);case BridgeTaskWorkUnitState_ChangesRequested():
return changesRequested(_that);case BridgeTaskWorkUnitState_Approved():
return approved(_that);case BridgeTaskWorkUnitState_Merged():
return merged(_that);case BridgeTaskWorkUnitState_NoDelivery():
return noDelivery(_that);case BridgeTaskWorkUnitState_NeedsAttention():
return needsAttention(_that);case BridgeTaskWorkUnitState_Failed():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTaskWorkUnitState_Pending value)?  pending,TResult? Function( BridgeTaskWorkUnitState_Running value)?  running,TResult? Function( BridgeTaskWorkUnitState_AwaitingCompletion value)?  awaitingCompletion,TResult? Function( BridgeTaskWorkUnitState_ReadyForReview value)?  readyForReview,TResult? Function( BridgeTaskWorkUnitState_Reviewing value)?  reviewing,TResult? Function( BridgeTaskWorkUnitState_ChangesRequested value)?  changesRequested,TResult? Function( BridgeTaskWorkUnitState_Approved value)?  approved,TResult? Function( BridgeTaskWorkUnitState_Merged value)?  merged,TResult? Function( BridgeTaskWorkUnitState_NoDelivery value)?  noDelivery,TResult? Function( BridgeTaskWorkUnitState_NeedsAttention value)?  needsAttention,TResult? Function( BridgeTaskWorkUnitState_Failed value)?  failed,TResult? Function( BridgeTaskWorkUnitState_Cancelled value)?  cancelled,}){
final _that = this;
switch (_that) {
case BridgeTaskWorkUnitState_Pending() when pending != null:
return pending(_that);case BridgeTaskWorkUnitState_Running() when running != null:
return running(_that);case BridgeTaskWorkUnitState_AwaitingCompletion() when awaitingCompletion != null:
return awaitingCompletion(_that);case BridgeTaskWorkUnitState_ReadyForReview() when readyForReview != null:
return readyForReview(_that);case BridgeTaskWorkUnitState_Reviewing() when reviewing != null:
return reviewing(_that);case BridgeTaskWorkUnitState_ChangesRequested() when changesRequested != null:
return changesRequested(_that);case BridgeTaskWorkUnitState_Approved() when approved != null:
return approved(_that);case BridgeTaskWorkUnitState_Merged() when merged != null:
return merged(_that);case BridgeTaskWorkUnitState_NoDelivery() when noDelivery != null:
return noDelivery(_that);case BridgeTaskWorkUnitState_NeedsAttention() when needsAttention != null:
return needsAttention(_that);case BridgeTaskWorkUnitState_Failed() when failed != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeTaskWorkUnitProgress field0)?  pending,TResult Function( BridgeRunningWorkUnit field0)?  running,TResult Function( BridgeAwaitingWorkUnit field0)?  awaitingCompletion,TResult Function( BridgeTaskWorkUnitProgress field0)?  readyForReview,TResult Function( BridgeTaskWorkUnitProgress field0)?  reviewing,TResult Function( BridgeTaskWorkUnitProgress field0)?  changesRequested,TResult Function( BridgeTaskWorkUnitProgress field0)?  approved,TResult Function( BridgeTaskWorkUnitProgress field0)?  merged,TResult Function( BridgeTaskWorkUnitProgress field0)?  noDelivery,TResult Function( BridgeTaskWorkUnitProgress field0)?  needsAttention,TResult Function( BridgeTaskWorkUnitProgress field0)?  failed,TResult Function( BridgeTaskWorkUnitProgress field0)?  cancelled,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTaskWorkUnitState_Pending() when pending != null:
return pending(_that.field0);case BridgeTaskWorkUnitState_Running() when running != null:
return running(_that.field0);case BridgeTaskWorkUnitState_AwaitingCompletion() when awaitingCompletion != null:
return awaitingCompletion(_that.field0);case BridgeTaskWorkUnitState_ReadyForReview() when readyForReview != null:
return readyForReview(_that.field0);case BridgeTaskWorkUnitState_Reviewing() when reviewing != null:
return reviewing(_that.field0);case BridgeTaskWorkUnitState_ChangesRequested() when changesRequested != null:
return changesRequested(_that.field0);case BridgeTaskWorkUnitState_Approved() when approved != null:
return approved(_that.field0);case BridgeTaskWorkUnitState_Merged() when merged != null:
return merged(_that.field0);case BridgeTaskWorkUnitState_NoDelivery() when noDelivery != null:
return noDelivery(_that.field0);case BridgeTaskWorkUnitState_NeedsAttention() when needsAttention != null:
return needsAttention(_that.field0);case BridgeTaskWorkUnitState_Failed() when failed != null:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeTaskWorkUnitProgress field0)  pending,required TResult Function( BridgeRunningWorkUnit field0)  running,required TResult Function( BridgeAwaitingWorkUnit field0)  awaitingCompletion,required TResult Function( BridgeTaskWorkUnitProgress field0)  readyForReview,required TResult Function( BridgeTaskWorkUnitProgress field0)  reviewing,required TResult Function( BridgeTaskWorkUnitProgress field0)  changesRequested,required TResult Function( BridgeTaskWorkUnitProgress field0)  approved,required TResult Function( BridgeTaskWorkUnitProgress field0)  merged,required TResult Function( BridgeTaskWorkUnitProgress field0)  noDelivery,required TResult Function( BridgeTaskWorkUnitProgress field0)  needsAttention,required TResult Function( BridgeTaskWorkUnitProgress field0)  failed,required TResult Function( BridgeTaskWorkUnitProgress field0)  cancelled,}) {final _that = this;
switch (_that) {
case BridgeTaskWorkUnitState_Pending():
return pending(_that.field0);case BridgeTaskWorkUnitState_Running():
return running(_that.field0);case BridgeTaskWorkUnitState_AwaitingCompletion():
return awaitingCompletion(_that.field0);case BridgeTaskWorkUnitState_ReadyForReview():
return readyForReview(_that.field0);case BridgeTaskWorkUnitState_Reviewing():
return reviewing(_that.field0);case BridgeTaskWorkUnitState_ChangesRequested():
return changesRequested(_that.field0);case BridgeTaskWorkUnitState_Approved():
return approved(_that.field0);case BridgeTaskWorkUnitState_Merged():
return merged(_that.field0);case BridgeTaskWorkUnitState_NoDelivery():
return noDelivery(_that.field0);case BridgeTaskWorkUnitState_NeedsAttention():
return needsAttention(_that.field0);case BridgeTaskWorkUnitState_Failed():
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeTaskWorkUnitProgress field0)?  pending,TResult? Function( BridgeRunningWorkUnit field0)?  running,TResult? Function( BridgeAwaitingWorkUnit field0)?  awaitingCompletion,TResult? Function( BridgeTaskWorkUnitProgress field0)?  readyForReview,TResult? Function( BridgeTaskWorkUnitProgress field0)?  reviewing,TResult? Function( BridgeTaskWorkUnitProgress field0)?  changesRequested,TResult? Function( BridgeTaskWorkUnitProgress field0)?  approved,TResult? Function( BridgeTaskWorkUnitProgress field0)?  merged,TResult? Function( BridgeTaskWorkUnitProgress field0)?  noDelivery,TResult? Function( BridgeTaskWorkUnitProgress field0)?  needsAttention,TResult? Function( BridgeTaskWorkUnitProgress field0)?  failed,TResult? Function( BridgeTaskWorkUnitProgress field0)?  cancelled,}) {final _that = this;
switch (_that) {
case BridgeTaskWorkUnitState_Pending() when pending != null:
return pending(_that.field0);case BridgeTaskWorkUnitState_Running() when running != null:
return running(_that.field0);case BridgeTaskWorkUnitState_AwaitingCompletion() when awaitingCompletion != null:
return awaitingCompletion(_that.field0);case BridgeTaskWorkUnitState_ReadyForReview() when readyForReview != null:
return readyForReview(_that.field0);case BridgeTaskWorkUnitState_Reviewing() when reviewing != null:
return reviewing(_that.field0);case BridgeTaskWorkUnitState_ChangesRequested() when changesRequested != null:
return changesRequested(_that.field0);case BridgeTaskWorkUnitState_Approved() when approved != null:
return approved(_that.field0);case BridgeTaskWorkUnitState_Merged() when merged != null:
return merged(_that.field0);case BridgeTaskWorkUnitState_NoDelivery() when noDelivery != null:
return noDelivery(_that.field0);case BridgeTaskWorkUnitState_NeedsAttention() when needsAttention != null:
return needsAttention(_that.field0);case BridgeTaskWorkUnitState_Failed() when failed != null:
return failed(_that.field0);case BridgeTaskWorkUnitState_Cancelled() when cancelled != null:
return cancelled(_that.field0);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTaskWorkUnitState_Pending extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_Pending(this.field0): super._();


@override final  BridgeTaskWorkUnitProgress field0;

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
 BridgeTaskWorkUnitProgress field0
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
as BridgeTaskWorkUnitProgress,
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


class BridgeTaskWorkUnitState_AwaitingCompletion extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_AwaitingCompletion(this.field0): super._();


@override final  BridgeAwaitingWorkUnit field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_AwaitingCompletionCopyWith<BridgeTaskWorkUnitState_AwaitingCompletion> get copyWith => _$BridgeTaskWorkUnitState_AwaitingCompletionCopyWithImpl<BridgeTaskWorkUnitState_AwaitingCompletion>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_AwaitingCompletion&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.awaitingCompletion(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_AwaitingCompletionCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_AwaitingCompletionCopyWith(BridgeTaskWorkUnitState_AwaitingCompletion value, $Res Function(BridgeTaskWorkUnitState_AwaitingCompletion) _then) = _$BridgeTaskWorkUnitState_AwaitingCompletionCopyWithImpl;
@useResult
$Res call({
 BridgeAwaitingWorkUnit field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_AwaitingCompletionCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_AwaitingCompletionCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_AwaitingCompletionCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_AwaitingCompletion _self;
  final $Res Function(BridgeTaskWorkUnitState_AwaitingCompletion) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_AwaitingCompletion(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeAwaitingWorkUnit,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_ReadyForReview extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_ReadyForReview(this.field0): super._();


@override final  BridgeTaskWorkUnitProgress field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_ReadyForReviewCopyWith<BridgeTaskWorkUnitState_ReadyForReview> get copyWith => _$BridgeTaskWorkUnitState_ReadyForReviewCopyWithImpl<BridgeTaskWorkUnitState_ReadyForReview>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_ReadyForReview&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.readyForReview(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_ReadyForReviewCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_ReadyForReviewCopyWith(BridgeTaskWorkUnitState_ReadyForReview value, $Res Function(BridgeTaskWorkUnitState_ReadyForReview) _then) = _$BridgeTaskWorkUnitState_ReadyForReviewCopyWithImpl;
@useResult
$Res call({
 BridgeTaskWorkUnitProgress field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_ReadyForReviewCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_ReadyForReviewCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_ReadyForReviewCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_ReadyForReview _self;
  final $Res Function(BridgeTaskWorkUnitState_ReadyForReview) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_ReadyForReview(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskWorkUnitProgress,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_Reviewing extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_Reviewing(this.field0): super._();


@override final  BridgeTaskWorkUnitProgress field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_ReviewingCopyWith<BridgeTaskWorkUnitState_Reviewing> get copyWith => _$BridgeTaskWorkUnitState_ReviewingCopyWithImpl<BridgeTaskWorkUnitState_Reviewing>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_Reviewing&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.reviewing(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_ReviewingCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_ReviewingCopyWith(BridgeTaskWorkUnitState_Reviewing value, $Res Function(BridgeTaskWorkUnitState_Reviewing) _then) = _$BridgeTaskWorkUnitState_ReviewingCopyWithImpl;
@useResult
$Res call({
 BridgeTaskWorkUnitProgress field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_ReviewingCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_ReviewingCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_ReviewingCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_Reviewing _self;
  final $Res Function(BridgeTaskWorkUnitState_Reviewing) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_Reviewing(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskWorkUnitProgress,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_ChangesRequested extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_ChangesRequested(this.field0): super._();


@override final  BridgeTaskWorkUnitProgress field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_ChangesRequestedCopyWith<BridgeTaskWorkUnitState_ChangesRequested> get copyWith => _$BridgeTaskWorkUnitState_ChangesRequestedCopyWithImpl<BridgeTaskWorkUnitState_ChangesRequested>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_ChangesRequested&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.changesRequested(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_ChangesRequestedCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_ChangesRequestedCopyWith(BridgeTaskWorkUnitState_ChangesRequested value, $Res Function(BridgeTaskWorkUnitState_ChangesRequested) _then) = _$BridgeTaskWorkUnitState_ChangesRequestedCopyWithImpl;
@useResult
$Res call({
 BridgeTaskWorkUnitProgress field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_ChangesRequestedCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_ChangesRequestedCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_ChangesRequestedCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_ChangesRequested _self;
  final $Res Function(BridgeTaskWorkUnitState_ChangesRequested) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_ChangesRequested(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskWorkUnitProgress,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_Approved extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_Approved(this.field0): super._();


@override final  BridgeTaskWorkUnitProgress field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_ApprovedCopyWith<BridgeTaskWorkUnitState_Approved> get copyWith => _$BridgeTaskWorkUnitState_ApprovedCopyWithImpl<BridgeTaskWorkUnitState_Approved>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_Approved&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.approved(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_ApprovedCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_ApprovedCopyWith(BridgeTaskWorkUnitState_Approved value, $Res Function(BridgeTaskWorkUnitState_Approved) _then) = _$BridgeTaskWorkUnitState_ApprovedCopyWithImpl;
@useResult
$Res call({
 BridgeTaskWorkUnitProgress field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_ApprovedCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_ApprovedCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_ApprovedCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_Approved _self;
  final $Res Function(BridgeTaskWorkUnitState_Approved) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_Approved(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskWorkUnitProgress,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_Merged extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_Merged(this.field0): super._();


@override final  BridgeTaskWorkUnitProgress field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_MergedCopyWith<BridgeTaskWorkUnitState_Merged> get copyWith => _$BridgeTaskWorkUnitState_MergedCopyWithImpl<BridgeTaskWorkUnitState_Merged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_Merged&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.merged(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_MergedCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_MergedCopyWith(BridgeTaskWorkUnitState_Merged value, $Res Function(BridgeTaskWorkUnitState_Merged) _then) = _$BridgeTaskWorkUnitState_MergedCopyWithImpl;
@useResult
$Res call({
 BridgeTaskWorkUnitProgress field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_MergedCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_MergedCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_MergedCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_Merged _self;
  final $Res Function(BridgeTaskWorkUnitState_Merged) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_Merged(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskWorkUnitProgress,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_NoDelivery extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_NoDelivery(this.field0): super._();


@override final  BridgeTaskWorkUnitProgress field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_NoDeliveryCopyWith<BridgeTaskWorkUnitState_NoDelivery> get copyWith => _$BridgeTaskWorkUnitState_NoDeliveryCopyWithImpl<BridgeTaskWorkUnitState_NoDelivery>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_NoDelivery&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.noDelivery(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_NoDeliveryCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_NoDeliveryCopyWith(BridgeTaskWorkUnitState_NoDelivery value, $Res Function(BridgeTaskWorkUnitState_NoDelivery) _then) = _$BridgeTaskWorkUnitState_NoDeliveryCopyWithImpl;
@useResult
$Res call({
 BridgeTaskWorkUnitProgress field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_NoDeliveryCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_NoDeliveryCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_NoDeliveryCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_NoDelivery _self;
  final $Res Function(BridgeTaskWorkUnitState_NoDelivery) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_NoDelivery(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskWorkUnitProgress,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_NeedsAttention extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_NeedsAttention(this.field0): super._();


@override final  BridgeTaskWorkUnitProgress field0;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTaskWorkUnitState_NeedsAttentionCopyWith<BridgeTaskWorkUnitState_NeedsAttention> get copyWith => _$BridgeTaskWorkUnitState_NeedsAttentionCopyWithImpl<BridgeTaskWorkUnitState_NeedsAttention>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTaskWorkUnitState_NeedsAttention&&(identical(other.field0, field0) || other.field0 == field0));
}


@override
int get hashCode => Object.hash(runtimeType,field0);

@override
String toString() {
  return 'BridgeTaskWorkUnitState.needsAttention(field0: $field0)';
}


}

/// @nodoc
abstract mixin class $BridgeTaskWorkUnitState_NeedsAttentionCopyWith<$Res> implements $BridgeTaskWorkUnitStateCopyWith<$Res> {
  factory $BridgeTaskWorkUnitState_NeedsAttentionCopyWith(BridgeTaskWorkUnitState_NeedsAttention value, $Res Function(BridgeTaskWorkUnitState_NeedsAttention) _then) = _$BridgeTaskWorkUnitState_NeedsAttentionCopyWithImpl;
@useResult
$Res call({
 BridgeTaskWorkUnitProgress field0
});




}
/// @nodoc
class _$BridgeTaskWorkUnitState_NeedsAttentionCopyWithImpl<$Res>
    implements $BridgeTaskWorkUnitState_NeedsAttentionCopyWith<$Res> {
  _$BridgeTaskWorkUnitState_NeedsAttentionCopyWithImpl(this._self, this._then);

  final BridgeTaskWorkUnitState_NeedsAttention _self;
  final $Res Function(BridgeTaskWorkUnitState_NeedsAttention) _then;

/// Create a copy of BridgeTaskWorkUnitState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? field0 = null,}) {
  return _then(BridgeTaskWorkUnitState_NeedsAttention(
null == field0 ? _self.field0 : field0 // ignore: cast_nullable_to_non_nullable
as BridgeTaskWorkUnitProgress,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_Failed extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_Failed(this.field0): super._();


@override final  BridgeTaskWorkUnitProgress field0;

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
 BridgeTaskWorkUnitProgress field0
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
as BridgeTaskWorkUnitProgress,
  ));
}


}

/// @nodoc


class BridgeTaskWorkUnitState_Cancelled extends BridgeTaskWorkUnitState {
  const BridgeTaskWorkUnitState_Cancelled(this.field0): super._();


@override final  BridgeTaskWorkUnitProgress field0;

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
 BridgeTaskWorkUnitProgress field0
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
as BridgeTaskWorkUnitProgress,
  ));
}


}

// dart format on

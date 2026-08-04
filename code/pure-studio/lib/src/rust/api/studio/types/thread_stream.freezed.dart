// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'thread_stream.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeInteractionPayload {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionPayload);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeInteractionPayload()';
}


}

/// @nodoc
class $BridgeInteractionPayloadCopyWith<$Res>  {
$BridgeInteractionPayloadCopyWith(BridgeInteractionPayload _, $Res Function(BridgeInteractionPayload) __);
}


/// Adds pattern-matching-related methods to [BridgeInteractionPayload].
extension BridgeInteractionPayloadPatterns on BridgeInteractionPayload {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeInteractionPayload_UserInput value)?  userInput,TResult Function( BridgeInteractionPayload_ToolApproval value)?  toolApproval,TResult Function( BridgeInteractionPayload_PlanConfirmation value)?  planConfirmation,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeInteractionPayload_UserInput() when userInput != null:
return userInput(_that);case BridgeInteractionPayload_ToolApproval() when toolApproval != null:
return toolApproval(_that);case BridgeInteractionPayload_PlanConfirmation() when planConfirmation != null:
return planConfirmation(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeInteractionPayload_UserInput value)  userInput,required TResult Function( BridgeInteractionPayload_ToolApproval value)  toolApproval,required TResult Function( BridgeInteractionPayload_PlanConfirmation value)  planConfirmation,}){
final _that = this;
switch (_that) {
case BridgeInteractionPayload_UserInput():
return userInput(_that);case BridgeInteractionPayload_ToolApproval():
return toolApproval(_that);case BridgeInteractionPayload_PlanConfirmation():
return planConfirmation(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeInteractionPayload_UserInput value)?  userInput,TResult? Function( BridgeInteractionPayload_ToolApproval value)?  toolApproval,TResult? Function( BridgeInteractionPayload_PlanConfirmation value)?  planConfirmation,}){
final _that = this;
switch (_that) {
case BridgeInteractionPayload_UserInput() when userInput != null:
return userInput(_that);case BridgeInteractionPayload_ToolApproval() when toolApproval != null:
return toolApproval(_that);case BridgeInteractionPayload_PlanConfirmation() when planConfirmation != null:
return planConfirmation(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( List<BridgeUserQuestion> questions)?  userInput,TResult Function( String name,  String argumentsJson,  String? workingDirectory,  String? parentAgentId)?  toolApproval,TResult Function( String planId,  String content)?  planConfirmation,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeInteractionPayload_UserInput() when userInput != null:
return userInput(_that.questions);case BridgeInteractionPayload_ToolApproval() when toolApproval != null:
return toolApproval(_that.name,_that.argumentsJson,_that.workingDirectory,_that.parentAgentId);case BridgeInteractionPayload_PlanConfirmation() when planConfirmation != null:
return planConfirmation(_that.planId,_that.content);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( List<BridgeUserQuestion> questions)  userInput,required TResult Function( String name,  String argumentsJson,  String? workingDirectory,  String? parentAgentId)  toolApproval,required TResult Function( String planId,  String content)  planConfirmation,}) {final _that = this;
switch (_that) {
case BridgeInteractionPayload_UserInput():
return userInput(_that.questions);case BridgeInteractionPayload_ToolApproval():
return toolApproval(_that.name,_that.argumentsJson,_that.workingDirectory,_that.parentAgentId);case BridgeInteractionPayload_PlanConfirmation():
return planConfirmation(_that.planId,_that.content);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( List<BridgeUserQuestion> questions)?  userInput,TResult? Function( String name,  String argumentsJson,  String? workingDirectory,  String? parentAgentId)?  toolApproval,TResult? Function( String planId,  String content)?  planConfirmation,}) {final _that = this;
switch (_that) {
case BridgeInteractionPayload_UserInput() when userInput != null:
return userInput(_that.questions);case BridgeInteractionPayload_ToolApproval() when toolApproval != null:
return toolApproval(_that.name,_that.argumentsJson,_that.workingDirectory,_that.parentAgentId);case BridgeInteractionPayload_PlanConfirmation() when planConfirmation != null:
return planConfirmation(_that.planId,_that.content);case _:
  return null;

}
}

}

/// @nodoc


class BridgeInteractionPayload_UserInput extends BridgeInteractionPayload {
  const BridgeInteractionPayload_UserInput({required final  List<BridgeUserQuestion> questions}): _questions = questions,super._();


 final  List<BridgeUserQuestion> _questions;
 List<BridgeUserQuestion> get questions {
  if (_questions is EqualUnmodifiableListView) return _questions;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_questions);
}


/// Create a copy of BridgeInteractionPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeInteractionPayload_UserInputCopyWith<BridgeInteractionPayload_UserInput> get copyWith => _$BridgeInteractionPayload_UserInputCopyWithImpl<BridgeInteractionPayload_UserInput>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionPayload_UserInput&&const DeepCollectionEquality().equals(other._questions, _questions));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_questions));

@override
String toString() {
  return 'BridgeInteractionPayload.userInput(questions: $questions)';
}


}

/// @nodoc
abstract mixin class $BridgeInteractionPayload_UserInputCopyWith<$Res> implements $BridgeInteractionPayloadCopyWith<$Res> {
  factory $BridgeInteractionPayload_UserInputCopyWith(BridgeInteractionPayload_UserInput value, $Res Function(BridgeInteractionPayload_UserInput) _then) = _$BridgeInteractionPayload_UserInputCopyWithImpl;
@useResult
$Res call({
 List<BridgeUserQuestion> questions
});




}
/// @nodoc
class _$BridgeInteractionPayload_UserInputCopyWithImpl<$Res>
    implements $BridgeInteractionPayload_UserInputCopyWith<$Res> {
  _$BridgeInteractionPayload_UserInputCopyWithImpl(this._self, this._then);

  final BridgeInteractionPayload_UserInput _self;
  final $Res Function(BridgeInteractionPayload_UserInput) _then;

/// Create a copy of BridgeInteractionPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? questions = null,}) {
  return _then(BridgeInteractionPayload_UserInput(
questions: null == questions ? _self._questions : questions // ignore: cast_nullable_to_non_nullable
as List<BridgeUserQuestion>,
  ));
}


}

/// @nodoc


class BridgeInteractionPayload_ToolApproval extends BridgeInteractionPayload {
  const BridgeInteractionPayload_ToolApproval({required this.name, required this.argumentsJson, this.workingDirectory, this.parentAgentId}): super._();


 final  String name;
 final  String argumentsJson;
 final  String? workingDirectory;
 final  String? parentAgentId;

/// Create a copy of BridgeInteractionPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeInteractionPayload_ToolApprovalCopyWith<BridgeInteractionPayload_ToolApproval> get copyWith => _$BridgeInteractionPayload_ToolApprovalCopyWithImpl<BridgeInteractionPayload_ToolApproval>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionPayload_ToolApproval&&(identical(other.name, name) || other.name == name)&&(identical(other.argumentsJson, argumentsJson) || other.argumentsJson == argumentsJson)&&(identical(other.workingDirectory, workingDirectory) || other.workingDirectory == workingDirectory)&&(identical(other.parentAgentId, parentAgentId) || other.parentAgentId == parentAgentId));
}


@override
int get hashCode => Object.hash(runtimeType,name,argumentsJson,workingDirectory,parentAgentId);

@override
String toString() {
  return 'BridgeInteractionPayload.toolApproval(name: $name, argumentsJson: $argumentsJson, workingDirectory: $workingDirectory, parentAgentId: $parentAgentId)';
}


}

/// @nodoc
abstract mixin class $BridgeInteractionPayload_ToolApprovalCopyWith<$Res> implements $BridgeInteractionPayloadCopyWith<$Res> {
  factory $BridgeInteractionPayload_ToolApprovalCopyWith(BridgeInteractionPayload_ToolApproval value, $Res Function(BridgeInteractionPayload_ToolApproval) _then) = _$BridgeInteractionPayload_ToolApprovalCopyWithImpl;
@useResult
$Res call({
 String name, String argumentsJson, String? workingDirectory, String? parentAgentId
});




}
/// @nodoc
class _$BridgeInteractionPayload_ToolApprovalCopyWithImpl<$Res>
    implements $BridgeInteractionPayload_ToolApprovalCopyWith<$Res> {
  _$BridgeInteractionPayload_ToolApprovalCopyWithImpl(this._self, this._then);

  final BridgeInteractionPayload_ToolApproval _self;
  final $Res Function(BridgeInteractionPayload_ToolApproval) _then;

/// Create a copy of BridgeInteractionPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? name = null,Object? argumentsJson = null,Object? workingDirectory = freezed,Object? parentAgentId = freezed,}) {
  return _then(BridgeInteractionPayload_ToolApproval(
name: null == name ? _self.name : name // ignore: cast_nullable_to_non_nullable
as String,argumentsJson: null == argumentsJson ? _self.argumentsJson : argumentsJson // ignore: cast_nullable_to_non_nullable
as String,workingDirectory: freezed == workingDirectory ? _self.workingDirectory : workingDirectory // ignore: cast_nullable_to_non_nullable
as String?,parentAgentId: freezed == parentAgentId ? _self.parentAgentId : parentAgentId // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc


class BridgeInteractionPayload_PlanConfirmation extends BridgeInteractionPayload {
  const BridgeInteractionPayload_PlanConfirmation({required this.planId, required this.content}): super._();


 final  String planId;
 final  String content;

/// Create a copy of BridgeInteractionPayload
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeInteractionPayload_PlanConfirmationCopyWith<BridgeInteractionPayload_PlanConfirmation> get copyWith => _$BridgeInteractionPayload_PlanConfirmationCopyWithImpl<BridgeInteractionPayload_PlanConfirmation>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionPayload_PlanConfirmation&&(identical(other.planId, planId) || other.planId == planId)&&(identical(other.content, content) || other.content == content));
}


@override
int get hashCode => Object.hash(runtimeType,planId,content);

@override
String toString() {
  return 'BridgeInteractionPayload.planConfirmation(planId: $planId, content: $content)';
}


}

/// @nodoc
abstract mixin class $BridgeInteractionPayload_PlanConfirmationCopyWith<$Res> implements $BridgeInteractionPayloadCopyWith<$Res> {
  factory $BridgeInteractionPayload_PlanConfirmationCopyWith(BridgeInteractionPayload_PlanConfirmation value, $Res Function(BridgeInteractionPayload_PlanConfirmation) _then) = _$BridgeInteractionPayload_PlanConfirmationCopyWithImpl;
@useResult
$Res call({
 String planId, String content
});




}
/// @nodoc
class _$BridgeInteractionPayload_PlanConfirmationCopyWithImpl<$Res>
    implements $BridgeInteractionPayload_PlanConfirmationCopyWith<$Res> {
  _$BridgeInteractionPayload_PlanConfirmationCopyWithImpl(this._self, this._then);

  final BridgeInteractionPayload_PlanConfirmation _self;
  final $Res Function(BridgeInteractionPayload_PlanConfirmation) _then;

/// Create a copy of BridgeInteractionPayload
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? planId = null,Object? content = null,}) {
  return _then(BridgeInteractionPayload_PlanConfirmation(
planId: null == planId ? _self.planId : planId // ignore: cast_nullable_to_non_nullable
as String,content: null == content ? _self.content : content // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeInteractionResolution {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionResolution);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeInteractionResolution()';
}


}

/// @nodoc
class $BridgeInteractionResolutionCopyWith<$Res>  {
$BridgeInteractionResolutionCopyWith(BridgeInteractionResolution _, $Res Function(BridgeInteractionResolution) __);
}


/// Adds pattern-matching-related methods to [BridgeInteractionResolution].
extension BridgeInteractionResolutionPatterns on BridgeInteractionResolution {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeInteractionResolution_UserInput value)?  userInput,TResult Function( BridgeInteractionResolution_ToolApproval value)?  toolApproval,TResult Function( BridgeInteractionResolution_PlanConfirmation value)?  planConfirmation,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeInteractionResolution_UserInput() when userInput != null:
return userInput(_that);case BridgeInteractionResolution_ToolApproval() when toolApproval != null:
return toolApproval(_that);case BridgeInteractionResolution_PlanConfirmation() when planConfirmation != null:
return planConfirmation(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeInteractionResolution_UserInput value)  userInput,required TResult Function( BridgeInteractionResolution_ToolApproval value)  toolApproval,required TResult Function( BridgeInteractionResolution_PlanConfirmation value)  planConfirmation,}){
final _that = this;
switch (_that) {
case BridgeInteractionResolution_UserInput():
return userInput(_that);case BridgeInteractionResolution_ToolApproval():
return toolApproval(_that);case BridgeInteractionResolution_PlanConfirmation():
return planConfirmation(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeInteractionResolution_UserInput value)?  userInput,TResult? Function( BridgeInteractionResolution_ToolApproval value)?  toolApproval,TResult? Function( BridgeInteractionResolution_PlanConfirmation value)?  planConfirmation,}){
final _that = this;
switch (_that) {
case BridgeInteractionResolution_UserInput() when userInput != null:
return userInput(_that);case BridgeInteractionResolution_ToolApproval() when toolApproval != null:
return toolApproval(_that);case BridgeInteractionResolution_PlanConfirmation() when planConfirmation != null:
return planConfirmation(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( List<BridgeUserInputAnswer> answers)?  userInput,TResult Function( BridgeToolApprovalResolution decision,  String? reason)?  toolApproval,TResult Function( BridgePlanConfirmationResolution decision,  String? content,  String? reason)?  planConfirmation,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeInteractionResolution_UserInput() when userInput != null:
return userInput(_that.answers);case BridgeInteractionResolution_ToolApproval() when toolApproval != null:
return toolApproval(_that.decision,_that.reason);case BridgeInteractionResolution_PlanConfirmation() when planConfirmation != null:
return planConfirmation(_that.decision,_that.content,_that.reason);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( List<BridgeUserInputAnswer> answers)  userInput,required TResult Function( BridgeToolApprovalResolution decision,  String? reason)  toolApproval,required TResult Function( BridgePlanConfirmationResolution decision,  String? content,  String? reason)  planConfirmation,}) {final _that = this;
switch (_that) {
case BridgeInteractionResolution_UserInput():
return userInput(_that.answers);case BridgeInteractionResolution_ToolApproval():
return toolApproval(_that.decision,_that.reason);case BridgeInteractionResolution_PlanConfirmation():
return planConfirmation(_that.decision,_that.content,_that.reason);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( List<BridgeUserInputAnswer> answers)?  userInput,TResult? Function( BridgeToolApprovalResolution decision,  String? reason)?  toolApproval,TResult? Function( BridgePlanConfirmationResolution decision,  String? content,  String? reason)?  planConfirmation,}) {final _that = this;
switch (_that) {
case BridgeInteractionResolution_UserInput() when userInput != null:
return userInput(_that.answers);case BridgeInteractionResolution_ToolApproval() when toolApproval != null:
return toolApproval(_that.decision,_that.reason);case BridgeInteractionResolution_PlanConfirmation() when planConfirmation != null:
return planConfirmation(_that.decision,_that.content,_that.reason);case _:
  return null;

}
}

}

/// @nodoc


class BridgeInteractionResolution_UserInput extends BridgeInteractionResolution {
  const BridgeInteractionResolution_UserInput({required final  List<BridgeUserInputAnswer> answers}): _answers = answers,super._();


 final  List<BridgeUserInputAnswer> _answers;
 List<BridgeUserInputAnswer> get answers {
  if (_answers is EqualUnmodifiableListView) return _answers;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_answers);
}


/// Create a copy of BridgeInteractionResolution
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeInteractionResolution_UserInputCopyWith<BridgeInteractionResolution_UserInput> get copyWith => _$BridgeInteractionResolution_UserInputCopyWithImpl<BridgeInteractionResolution_UserInput>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionResolution_UserInput&&const DeepCollectionEquality().equals(other._answers, _answers));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_answers));

@override
String toString() {
  return 'BridgeInteractionResolution.userInput(answers: $answers)';
}


}

/// @nodoc
abstract mixin class $BridgeInteractionResolution_UserInputCopyWith<$Res> implements $BridgeInteractionResolutionCopyWith<$Res> {
  factory $BridgeInteractionResolution_UserInputCopyWith(BridgeInteractionResolution_UserInput value, $Res Function(BridgeInteractionResolution_UserInput) _then) = _$BridgeInteractionResolution_UserInputCopyWithImpl;
@useResult
$Res call({
 List<BridgeUserInputAnswer> answers
});




}
/// @nodoc
class _$BridgeInteractionResolution_UserInputCopyWithImpl<$Res>
    implements $BridgeInteractionResolution_UserInputCopyWith<$Res> {
  _$BridgeInteractionResolution_UserInputCopyWithImpl(this._self, this._then);

  final BridgeInteractionResolution_UserInput _self;
  final $Res Function(BridgeInteractionResolution_UserInput) _then;

/// Create a copy of BridgeInteractionResolution
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? answers = null,}) {
  return _then(BridgeInteractionResolution_UserInput(
answers: null == answers ? _self._answers : answers // ignore: cast_nullable_to_non_nullable
as List<BridgeUserInputAnswer>,
  ));
}


}

/// @nodoc


class BridgeInteractionResolution_ToolApproval extends BridgeInteractionResolution {
  const BridgeInteractionResolution_ToolApproval({required this.decision, this.reason}): super._();


 final  BridgeToolApprovalResolution decision;
 final  String? reason;

/// Create a copy of BridgeInteractionResolution
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeInteractionResolution_ToolApprovalCopyWith<BridgeInteractionResolution_ToolApproval> get copyWith => _$BridgeInteractionResolution_ToolApprovalCopyWithImpl<BridgeInteractionResolution_ToolApproval>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionResolution_ToolApproval&&(identical(other.decision, decision) || other.decision == decision)&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,decision,reason);

@override
String toString() {
  return 'BridgeInteractionResolution.toolApproval(decision: $decision, reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeInteractionResolution_ToolApprovalCopyWith<$Res> implements $BridgeInteractionResolutionCopyWith<$Res> {
  factory $BridgeInteractionResolution_ToolApprovalCopyWith(BridgeInteractionResolution_ToolApproval value, $Res Function(BridgeInteractionResolution_ToolApproval) _then) = _$BridgeInteractionResolution_ToolApprovalCopyWithImpl;
@useResult
$Res call({
 BridgeToolApprovalResolution decision, String? reason
});




}
/// @nodoc
class _$BridgeInteractionResolution_ToolApprovalCopyWithImpl<$Res>
    implements $BridgeInteractionResolution_ToolApprovalCopyWith<$Res> {
  _$BridgeInteractionResolution_ToolApprovalCopyWithImpl(this._self, this._then);

  final BridgeInteractionResolution_ToolApproval _self;
  final $Res Function(BridgeInteractionResolution_ToolApproval) _then;

/// Create a copy of BridgeInteractionResolution
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? decision = null,Object? reason = freezed,}) {
  return _then(BridgeInteractionResolution_ToolApproval(
decision: null == decision ? _self.decision : decision // ignore: cast_nullable_to_non_nullable
as BridgeToolApprovalResolution,reason: freezed == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc


class BridgeInteractionResolution_PlanConfirmation extends BridgeInteractionResolution {
  const BridgeInteractionResolution_PlanConfirmation({required this.decision, this.content, this.reason}): super._();


 final  BridgePlanConfirmationResolution decision;
 final  String? content;
 final  String? reason;

/// Create a copy of BridgeInteractionResolution
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeInteractionResolution_PlanConfirmationCopyWith<BridgeInteractionResolution_PlanConfirmation> get copyWith => _$BridgeInteractionResolution_PlanConfirmationCopyWithImpl<BridgeInteractionResolution_PlanConfirmation>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionResolution_PlanConfirmation&&(identical(other.decision, decision) || other.decision == decision)&&(identical(other.content, content) || other.content == content)&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,decision,content,reason);

@override
String toString() {
  return 'BridgeInteractionResolution.planConfirmation(decision: $decision, content: $content, reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeInteractionResolution_PlanConfirmationCopyWith<$Res> implements $BridgeInteractionResolutionCopyWith<$Res> {
  factory $BridgeInteractionResolution_PlanConfirmationCopyWith(BridgeInteractionResolution_PlanConfirmation value, $Res Function(BridgeInteractionResolution_PlanConfirmation) _then) = _$BridgeInteractionResolution_PlanConfirmationCopyWithImpl;
@useResult
$Res call({
 BridgePlanConfirmationResolution decision, String? content, String? reason
});




}
/// @nodoc
class _$BridgeInteractionResolution_PlanConfirmationCopyWithImpl<$Res>
    implements $BridgeInteractionResolution_PlanConfirmationCopyWith<$Res> {
  _$BridgeInteractionResolution_PlanConfirmationCopyWithImpl(this._self, this._then);

  final BridgeInteractionResolution_PlanConfirmation _self;
  final $Res Function(BridgeInteractionResolution_PlanConfirmation) _then;

/// Create a copy of BridgeInteractionResolution
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? decision = null,Object? content = freezed,Object? reason = freezed,}) {
  return _then(BridgeInteractionResolution_PlanConfirmation(
decision: null == decision ? _self.decision : decision // ignore: cast_nullable_to_non_nullable
as BridgePlanConfirmationResolution,content: freezed == content ? _self.content : content // ignore: cast_nullable_to_non_nullable
as String?,reason: freezed == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc
mixin _$BridgeThreadItemContent {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemContent);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadItemContent()';
}


}

/// @nodoc
class $BridgeThreadItemContentCopyWith<$Res>  {
$BridgeThreadItemContentCopyWith(BridgeThreadItemContent _, $Res Function(BridgeThreadItemContent) __);
}


/// Adds pattern-matching-related methods to [BridgeThreadItemContent].
extension BridgeThreadItemContentPatterns on BridgeThreadItemContent {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeThreadItemContent_UserMessage value)?  userMessage,TResult Function( BridgeThreadItemContent_AgentMessage value)?  agentMessage,TResult Function( BridgeThreadItemContent_Reasoning value)?  reasoning,TResult Function( BridgeThreadItemContent_Plan value)?  plan,TResult Function( BridgeThreadItemContent_ToolCall value)?  toolCall,TResult Function( BridgeThreadItemContent_File value)?  file,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeThreadItemContent_UserMessage() when userMessage != null:
return userMessage(_that);case BridgeThreadItemContent_AgentMessage() when agentMessage != null:
return agentMessage(_that);case BridgeThreadItemContent_Reasoning() when reasoning != null:
return reasoning(_that);case BridgeThreadItemContent_Plan() when plan != null:
return plan(_that);case BridgeThreadItemContent_ToolCall() when toolCall != null:
return toolCall(_that);case BridgeThreadItemContent_File() when file != null:
return file(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeThreadItemContent_UserMessage value)  userMessage,required TResult Function( BridgeThreadItemContent_AgentMessage value)  agentMessage,required TResult Function( BridgeThreadItemContent_Reasoning value)  reasoning,required TResult Function( BridgeThreadItemContent_Plan value)  plan,required TResult Function( BridgeThreadItemContent_ToolCall value)  toolCall,required TResult Function( BridgeThreadItemContent_File value)  file,}){
final _that = this;
switch (_that) {
case BridgeThreadItemContent_UserMessage():
return userMessage(_that);case BridgeThreadItemContent_AgentMessage():
return agentMessage(_that);case BridgeThreadItemContent_Reasoning():
return reasoning(_that);case BridgeThreadItemContent_Plan():
return plan(_that);case BridgeThreadItemContent_ToolCall():
return toolCall(_that);case BridgeThreadItemContent_File():
return file(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeThreadItemContent_UserMessage value)?  userMessage,TResult? Function( BridgeThreadItemContent_AgentMessage value)?  agentMessage,TResult? Function( BridgeThreadItemContent_Reasoning value)?  reasoning,TResult? Function( BridgeThreadItemContent_Plan value)?  plan,TResult? Function( BridgeThreadItemContent_ToolCall value)?  toolCall,TResult? Function( BridgeThreadItemContent_File value)?  file,}){
final _that = this;
switch (_that) {
case BridgeThreadItemContent_UserMessage() when userMessage != null:
return userMessage(_that);case BridgeThreadItemContent_AgentMessage() when agentMessage != null:
return agentMessage(_that);case BridgeThreadItemContent_Reasoning() when reasoning != null:
return reasoning(_that);case BridgeThreadItemContent_Plan() when plan != null:
return plan(_that);case BridgeThreadItemContent_ToolCall() when toolCall != null:
return toolCall(_that);case BridgeThreadItemContent_File() when file != null:
return file(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String text,  List<BridgeThreadAttachment> attachments)?  userMessage,TResult Function( BridgeAgentMessageChannel channel,  String text)?  agentMessage,TResult Function( List<String> summary,  List<String> content)?  reasoning,TResult Function( String content)?  plan,TResult Function( BridgeThreadToolCall tool)?  toolCall,TResult Function( String path,  String? mediaType)?  file,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeThreadItemContent_UserMessage() when userMessage != null:
return userMessage(_that.text,_that.attachments);case BridgeThreadItemContent_AgentMessage() when agentMessage != null:
return agentMessage(_that.channel,_that.text);case BridgeThreadItemContent_Reasoning() when reasoning != null:
return reasoning(_that.summary,_that.content);case BridgeThreadItemContent_Plan() when plan != null:
return plan(_that.content);case BridgeThreadItemContent_ToolCall() when toolCall != null:
return toolCall(_that.tool);case BridgeThreadItemContent_File() when file != null:
return file(_that.path,_that.mediaType);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String text,  List<BridgeThreadAttachment> attachments)  userMessage,required TResult Function( BridgeAgentMessageChannel channel,  String text)  agentMessage,required TResult Function( List<String> summary,  List<String> content)  reasoning,required TResult Function( String content)  plan,required TResult Function( BridgeThreadToolCall tool)  toolCall,required TResult Function( String path,  String? mediaType)  file,}) {final _that = this;
switch (_that) {
case BridgeThreadItemContent_UserMessage():
return userMessage(_that.text,_that.attachments);case BridgeThreadItemContent_AgentMessage():
return agentMessage(_that.channel,_that.text);case BridgeThreadItemContent_Reasoning():
return reasoning(_that.summary,_that.content);case BridgeThreadItemContent_Plan():
return plan(_that.content);case BridgeThreadItemContent_ToolCall():
return toolCall(_that.tool);case BridgeThreadItemContent_File():
return file(_that.path,_that.mediaType);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String text,  List<BridgeThreadAttachment> attachments)?  userMessage,TResult? Function( BridgeAgentMessageChannel channel,  String text)?  agentMessage,TResult? Function( List<String> summary,  List<String> content)?  reasoning,TResult? Function( String content)?  plan,TResult? Function( BridgeThreadToolCall tool)?  toolCall,TResult? Function( String path,  String? mediaType)?  file,}) {final _that = this;
switch (_that) {
case BridgeThreadItemContent_UserMessage() when userMessage != null:
return userMessage(_that.text,_that.attachments);case BridgeThreadItemContent_AgentMessage() when agentMessage != null:
return agentMessage(_that.channel,_that.text);case BridgeThreadItemContent_Reasoning() when reasoning != null:
return reasoning(_that.summary,_that.content);case BridgeThreadItemContent_Plan() when plan != null:
return plan(_that.content);case BridgeThreadItemContent_ToolCall() when toolCall != null:
return toolCall(_that.tool);case BridgeThreadItemContent_File() when file != null:
return file(_that.path,_that.mediaType);case _:
  return null;

}
}

}

/// @nodoc


class BridgeThreadItemContent_UserMessage extends BridgeThreadItemContent {
  const BridgeThreadItemContent_UserMessage({required this.text, required final  List<BridgeThreadAttachment> attachments}): _attachments = attachments,super._();


 final  String text;
 final  List<BridgeThreadAttachment> _attachments;
 List<BridgeThreadAttachment> get attachments {
  if (_attachments is EqualUnmodifiableListView) return _attachments;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_attachments);
}


/// Create a copy of BridgeThreadItemContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemContent_UserMessageCopyWith<BridgeThreadItemContent_UserMessage> get copyWith => _$BridgeThreadItemContent_UserMessageCopyWithImpl<BridgeThreadItemContent_UserMessage>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemContent_UserMessage&&(identical(other.text, text) || other.text == text)&&const DeepCollectionEquality().equals(other._attachments, _attachments));
}


@override
int get hashCode => Object.hash(runtimeType,text,const DeepCollectionEquality().hash(_attachments));

@override
String toString() {
  return 'BridgeThreadItemContent.userMessage(text: $text, attachments: $attachments)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemContent_UserMessageCopyWith<$Res> implements $BridgeThreadItemContentCopyWith<$Res> {
  factory $BridgeThreadItemContent_UserMessageCopyWith(BridgeThreadItemContent_UserMessage value, $Res Function(BridgeThreadItemContent_UserMessage) _then) = _$BridgeThreadItemContent_UserMessageCopyWithImpl;
@useResult
$Res call({
 String text, List<BridgeThreadAttachment> attachments
});




}
/// @nodoc
class _$BridgeThreadItemContent_UserMessageCopyWithImpl<$Res>
    implements $BridgeThreadItemContent_UserMessageCopyWith<$Res> {
  _$BridgeThreadItemContent_UserMessageCopyWithImpl(this._self, this._then);

  final BridgeThreadItemContent_UserMessage _self;
  final $Res Function(BridgeThreadItemContent_UserMessage) _then;

/// Create a copy of BridgeThreadItemContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? text = null,Object? attachments = null,}) {
  return _then(BridgeThreadItemContent_UserMessage(
text: null == text ? _self.text : text // ignore: cast_nullable_to_non_nullable
as String,attachments: null == attachments ? _self._attachments : attachments // ignore: cast_nullable_to_non_nullable
as List<BridgeThreadAttachment>,
  ));
}


}

/// @nodoc


class BridgeThreadItemContent_AgentMessage extends BridgeThreadItemContent {
  const BridgeThreadItemContent_AgentMessage({required this.channel, required this.text}): super._();


 final  BridgeAgentMessageChannel channel;
 final  String text;

/// Create a copy of BridgeThreadItemContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemContent_AgentMessageCopyWith<BridgeThreadItemContent_AgentMessage> get copyWith => _$BridgeThreadItemContent_AgentMessageCopyWithImpl<BridgeThreadItemContent_AgentMessage>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemContent_AgentMessage&&(identical(other.channel, channel) || other.channel == channel)&&(identical(other.text, text) || other.text == text));
}


@override
int get hashCode => Object.hash(runtimeType,channel,text);

@override
String toString() {
  return 'BridgeThreadItemContent.agentMessage(channel: $channel, text: $text)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemContent_AgentMessageCopyWith<$Res> implements $BridgeThreadItemContentCopyWith<$Res> {
  factory $BridgeThreadItemContent_AgentMessageCopyWith(BridgeThreadItemContent_AgentMessage value, $Res Function(BridgeThreadItemContent_AgentMessage) _then) = _$BridgeThreadItemContent_AgentMessageCopyWithImpl;
@useResult
$Res call({
 BridgeAgentMessageChannel channel, String text
});




}
/// @nodoc
class _$BridgeThreadItemContent_AgentMessageCopyWithImpl<$Res>
    implements $BridgeThreadItemContent_AgentMessageCopyWith<$Res> {
  _$BridgeThreadItemContent_AgentMessageCopyWithImpl(this._self, this._then);

  final BridgeThreadItemContent_AgentMessage _self;
  final $Res Function(BridgeThreadItemContent_AgentMessage) _then;

/// Create a copy of BridgeThreadItemContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? channel = null,Object? text = null,}) {
  return _then(BridgeThreadItemContent_AgentMessage(
channel: null == channel ? _self.channel : channel // ignore: cast_nullable_to_non_nullable
as BridgeAgentMessageChannel,text: null == text ? _self.text : text // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadItemContent_Reasoning extends BridgeThreadItemContent {
  const BridgeThreadItemContent_Reasoning({required final  List<String> summary, required final  List<String> content}): _summary = summary,_content = content,super._();


 final  List<String> _summary;
 List<String> get summary {
  if (_summary is EqualUnmodifiableListView) return _summary;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_summary);
}

 final  List<String> _content;
 List<String> get content {
  if (_content is EqualUnmodifiableListView) return _content;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_content);
}


/// Create a copy of BridgeThreadItemContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemContent_ReasoningCopyWith<BridgeThreadItemContent_Reasoning> get copyWith => _$BridgeThreadItemContent_ReasoningCopyWithImpl<BridgeThreadItemContent_Reasoning>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemContent_Reasoning&&const DeepCollectionEquality().equals(other._summary, _summary)&&const DeepCollectionEquality().equals(other._content, _content));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_summary),const DeepCollectionEquality().hash(_content));

@override
String toString() {
  return 'BridgeThreadItemContent.reasoning(summary: $summary, content: $content)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemContent_ReasoningCopyWith<$Res> implements $BridgeThreadItemContentCopyWith<$Res> {
  factory $BridgeThreadItemContent_ReasoningCopyWith(BridgeThreadItemContent_Reasoning value, $Res Function(BridgeThreadItemContent_Reasoning) _then) = _$BridgeThreadItemContent_ReasoningCopyWithImpl;
@useResult
$Res call({
 List<String> summary, List<String> content
});




}
/// @nodoc
class _$BridgeThreadItemContent_ReasoningCopyWithImpl<$Res>
    implements $BridgeThreadItemContent_ReasoningCopyWith<$Res> {
  _$BridgeThreadItemContent_ReasoningCopyWithImpl(this._self, this._then);

  final BridgeThreadItemContent_Reasoning _self;
  final $Res Function(BridgeThreadItemContent_Reasoning) _then;

/// Create a copy of BridgeThreadItemContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? summary = null,Object? content = null,}) {
  return _then(BridgeThreadItemContent_Reasoning(
summary: null == summary ? _self._summary : summary // ignore: cast_nullable_to_non_nullable
as List<String>,content: null == content ? _self._content : content // ignore: cast_nullable_to_non_nullable
as List<String>,
  ));
}


}

/// @nodoc


class BridgeThreadItemContent_Plan extends BridgeThreadItemContent {
  const BridgeThreadItemContent_Plan({required this.content}): super._();


 final  String content;

/// Create a copy of BridgeThreadItemContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemContent_PlanCopyWith<BridgeThreadItemContent_Plan> get copyWith => _$BridgeThreadItemContent_PlanCopyWithImpl<BridgeThreadItemContent_Plan>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemContent_Plan&&(identical(other.content, content) || other.content == content));
}


@override
int get hashCode => Object.hash(runtimeType,content);

@override
String toString() {
  return 'BridgeThreadItemContent.plan(content: $content)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemContent_PlanCopyWith<$Res> implements $BridgeThreadItemContentCopyWith<$Res> {
  factory $BridgeThreadItemContent_PlanCopyWith(BridgeThreadItemContent_Plan value, $Res Function(BridgeThreadItemContent_Plan) _then) = _$BridgeThreadItemContent_PlanCopyWithImpl;
@useResult
$Res call({
 String content
});




}
/// @nodoc
class _$BridgeThreadItemContent_PlanCopyWithImpl<$Res>
    implements $BridgeThreadItemContent_PlanCopyWith<$Res> {
  _$BridgeThreadItemContent_PlanCopyWithImpl(this._self, this._then);

  final BridgeThreadItemContent_Plan _self;
  final $Res Function(BridgeThreadItemContent_Plan) _then;

/// Create a copy of BridgeThreadItemContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? content = null,}) {
  return _then(BridgeThreadItemContent_Plan(
content: null == content ? _self.content : content // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeThreadItemContent_ToolCall extends BridgeThreadItemContent {
  const BridgeThreadItemContent_ToolCall({required this.tool}): super._();


 final  BridgeThreadToolCall tool;

/// Create a copy of BridgeThreadItemContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemContent_ToolCallCopyWith<BridgeThreadItemContent_ToolCall> get copyWith => _$BridgeThreadItemContent_ToolCallCopyWithImpl<BridgeThreadItemContent_ToolCall>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemContent_ToolCall&&(identical(other.tool, tool) || other.tool == tool));
}


@override
int get hashCode => Object.hash(runtimeType,tool);

@override
String toString() {
  return 'BridgeThreadItemContent.toolCall(tool: $tool)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemContent_ToolCallCopyWith<$Res> implements $BridgeThreadItemContentCopyWith<$Res> {
  factory $BridgeThreadItemContent_ToolCallCopyWith(BridgeThreadItemContent_ToolCall value, $Res Function(BridgeThreadItemContent_ToolCall) _then) = _$BridgeThreadItemContent_ToolCallCopyWithImpl;
@useResult
$Res call({
 BridgeThreadToolCall tool
});




}
/// @nodoc
class _$BridgeThreadItemContent_ToolCallCopyWithImpl<$Res>
    implements $BridgeThreadItemContent_ToolCallCopyWith<$Res> {
  _$BridgeThreadItemContent_ToolCallCopyWithImpl(this._self, this._then);

  final BridgeThreadItemContent_ToolCall _self;
  final $Res Function(BridgeThreadItemContent_ToolCall) _then;

/// Create a copy of BridgeThreadItemContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? tool = null,}) {
  return _then(BridgeThreadItemContent_ToolCall(
tool: null == tool ? _self.tool : tool // ignore: cast_nullable_to_non_nullable
as BridgeThreadToolCall,
  ));
}


}

/// @nodoc


class BridgeThreadItemContent_File extends BridgeThreadItemContent {
  const BridgeThreadItemContent_File({required this.path, this.mediaType}): super._();


 final  String path;
 final  String? mediaType;

/// Create a copy of BridgeThreadItemContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadItemContent_FileCopyWith<BridgeThreadItemContent_File> get copyWith => _$BridgeThreadItemContent_FileCopyWithImpl<BridgeThreadItemContent_File>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadItemContent_File&&(identical(other.path, path) || other.path == path)&&(identical(other.mediaType, mediaType) || other.mediaType == mediaType));
}


@override
int get hashCode => Object.hash(runtimeType,path,mediaType);

@override
String toString() {
  return 'BridgeThreadItemContent.file(path: $path, mediaType: $mediaType)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadItemContent_FileCopyWith<$Res> implements $BridgeThreadItemContentCopyWith<$Res> {
  factory $BridgeThreadItemContent_FileCopyWith(BridgeThreadItemContent_File value, $Res Function(BridgeThreadItemContent_File) _then) = _$BridgeThreadItemContent_FileCopyWithImpl;
@useResult
$Res call({
 String path, String? mediaType
});




}
/// @nodoc
class _$BridgeThreadItemContent_FileCopyWithImpl<$Res>
    implements $BridgeThreadItemContent_FileCopyWith<$Res> {
  _$BridgeThreadItemContent_FileCopyWithImpl(this._self, this._then);

  final BridgeThreadItemContent_File _self;
  final $Res Function(BridgeThreadItemContent_File) _then;

/// Create a copy of BridgeThreadItemContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? path = null,Object? mediaType = freezed,}) {
  return _then(BridgeThreadItemContent_File(
path: null == path ? _self.path : path // ignore: cast_nullable_to_non_nullable
as String,mediaType: freezed == mediaType ? _self.mediaType : mediaType // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc
mixin _$BridgeThreadNotification {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadNotification);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadNotification()';
}


}

/// @nodoc
class $BridgeThreadNotificationCopyWith<$Res>  {
$BridgeThreadNotificationCopyWith(BridgeThreadNotification _, $Res Function(BridgeThreadNotification) __);
}


/// Adds pattern-matching-related methods to [BridgeThreadNotification].
extension BridgeThreadNotificationPatterns on BridgeThreadNotification {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeThreadNotification_TurnStarted value)?  turnStarted,TResult Function( BridgeThreadNotification_TurnUpdated value)?  turnUpdated,TResult Function( BridgeThreadNotification_TurnCompleted value)?  turnCompleted,TResult Function( BridgeThreadNotification_ItemStarted value)?  itemStarted,TResult Function( BridgeThreadNotification_ItemDelta value)?  itemDelta,TResult Function( BridgeThreadNotification_ItemCompleted value)?  itemCompleted,TResult Function( BridgeThreadNotification_InteractionChanged value)?  interactionChanged,TResult Function( BridgeThreadNotification_ThreadRuntimeUpdated value)?  threadRuntimeUpdated,TResult Function( BridgeThreadNotification_Lagged value)?  lagged,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeThreadNotification_TurnStarted() when turnStarted != null:
return turnStarted(_that);case BridgeThreadNotification_TurnUpdated() when turnUpdated != null:
return turnUpdated(_that);case BridgeThreadNotification_TurnCompleted() when turnCompleted != null:
return turnCompleted(_that);case BridgeThreadNotification_ItemStarted() when itemStarted != null:
return itemStarted(_that);case BridgeThreadNotification_ItemDelta() when itemDelta != null:
return itemDelta(_that);case BridgeThreadNotification_ItemCompleted() when itemCompleted != null:
return itemCompleted(_that);case BridgeThreadNotification_InteractionChanged() when interactionChanged != null:
return interactionChanged(_that);case BridgeThreadNotification_ThreadRuntimeUpdated() when threadRuntimeUpdated != null:
return threadRuntimeUpdated(_that);case BridgeThreadNotification_Lagged() when lagged != null:
return lagged(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeThreadNotification_TurnStarted value)  turnStarted,required TResult Function( BridgeThreadNotification_TurnUpdated value)  turnUpdated,required TResult Function( BridgeThreadNotification_TurnCompleted value)  turnCompleted,required TResult Function( BridgeThreadNotification_ItemStarted value)  itemStarted,required TResult Function( BridgeThreadNotification_ItemDelta value)  itemDelta,required TResult Function( BridgeThreadNotification_ItemCompleted value)  itemCompleted,required TResult Function( BridgeThreadNotification_InteractionChanged value)  interactionChanged,required TResult Function( BridgeThreadNotification_ThreadRuntimeUpdated value)  threadRuntimeUpdated,required TResult Function( BridgeThreadNotification_Lagged value)  lagged,}){
final _that = this;
switch (_that) {
case BridgeThreadNotification_TurnStarted():
return turnStarted(_that);case BridgeThreadNotification_TurnUpdated():
return turnUpdated(_that);case BridgeThreadNotification_TurnCompleted():
return turnCompleted(_that);case BridgeThreadNotification_ItemStarted():
return itemStarted(_that);case BridgeThreadNotification_ItemDelta():
return itemDelta(_that);case BridgeThreadNotification_ItemCompleted():
return itemCompleted(_that);case BridgeThreadNotification_InteractionChanged():
return interactionChanged(_that);case BridgeThreadNotification_ThreadRuntimeUpdated():
return threadRuntimeUpdated(_that);case BridgeThreadNotification_Lagged():
return lagged(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeThreadNotification_TurnStarted value)?  turnStarted,TResult? Function( BridgeThreadNotification_TurnUpdated value)?  turnUpdated,TResult? Function( BridgeThreadNotification_TurnCompleted value)?  turnCompleted,TResult? Function( BridgeThreadNotification_ItemStarted value)?  itemStarted,TResult? Function( BridgeThreadNotification_ItemDelta value)?  itemDelta,TResult? Function( BridgeThreadNotification_ItemCompleted value)?  itemCompleted,TResult? Function( BridgeThreadNotification_InteractionChanged value)?  interactionChanged,TResult? Function( BridgeThreadNotification_ThreadRuntimeUpdated value)?  threadRuntimeUpdated,TResult? Function( BridgeThreadNotification_Lagged value)?  lagged,}){
final _that = this;
switch (_that) {
case BridgeThreadNotification_TurnStarted() when turnStarted != null:
return turnStarted(_that);case BridgeThreadNotification_TurnUpdated() when turnUpdated != null:
return turnUpdated(_that);case BridgeThreadNotification_TurnCompleted() when turnCompleted != null:
return turnCompleted(_that);case BridgeThreadNotification_ItemStarted() when itemStarted != null:
return itemStarted(_that);case BridgeThreadNotification_ItemDelta() when itemDelta != null:
return itemDelta(_that);case BridgeThreadNotification_ItemCompleted() when itemCompleted != null:
return itemCompleted(_that);case BridgeThreadNotification_InteractionChanged() when interactionChanged != null:
return interactionChanged(_that);case BridgeThreadNotification_ThreadRuntimeUpdated() when threadRuntimeUpdated != null:
return threadRuntimeUpdated(_that);case BridgeThreadNotification_Lagged() when lagged != null:
return lagged(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeTurn turn)?  turnStarted,TResult Function( BridgeTurn turn)?  turnUpdated,TResult Function( BridgeTurn turn)?  turnCompleted,TResult Function( BridgeThreadItem item)?  itemStarted,TResult Function( BridgeThreadItemDelta delta)?  itemDelta,TResult Function( BridgeThreadItem item)?  itemCompleted,TResult Function( BridgeInteractionRequest interaction)?  interactionChanged,TResult Function( BridgeThreadRuntimeSnapshot runtime)?  threadRuntimeUpdated,TResult Function( BigInt dropped)?  lagged,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeThreadNotification_TurnStarted() when turnStarted != null:
return turnStarted(_that.turn);case BridgeThreadNotification_TurnUpdated() when turnUpdated != null:
return turnUpdated(_that.turn);case BridgeThreadNotification_TurnCompleted() when turnCompleted != null:
return turnCompleted(_that.turn);case BridgeThreadNotification_ItemStarted() when itemStarted != null:
return itemStarted(_that.item);case BridgeThreadNotification_ItemDelta() when itemDelta != null:
return itemDelta(_that.delta);case BridgeThreadNotification_ItemCompleted() when itemCompleted != null:
return itemCompleted(_that.item);case BridgeThreadNotification_InteractionChanged() when interactionChanged != null:
return interactionChanged(_that.interaction);case BridgeThreadNotification_ThreadRuntimeUpdated() when threadRuntimeUpdated != null:
return threadRuntimeUpdated(_that.runtime);case BridgeThreadNotification_Lagged() when lagged != null:
return lagged(_that.dropped);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeTurn turn)  turnStarted,required TResult Function( BridgeTurn turn)  turnUpdated,required TResult Function( BridgeTurn turn)  turnCompleted,required TResult Function( BridgeThreadItem item)  itemStarted,required TResult Function( BridgeThreadItemDelta delta)  itemDelta,required TResult Function( BridgeThreadItem item)  itemCompleted,required TResult Function( BridgeInteractionRequest interaction)  interactionChanged,required TResult Function( BridgeThreadRuntimeSnapshot runtime)  threadRuntimeUpdated,required TResult Function( BigInt dropped)  lagged,}) {final _that = this;
switch (_that) {
case BridgeThreadNotification_TurnStarted():
return turnStarted(_that.turn);case BridgeThreadNotification_TurnUpdated():
return turnUpdated(_that.turn);case BridgeThreadNotification_TurnCompleted():
return turnCompleted(_that.turn);case BridgeThreadNotification_ItemStarted():
return itemStarted(_that.item);case BridgeThreadNotification_ItemDelta():
return itemDelta(_that.delta);case BridgeThreadNotification_ItemCompleted():
return itemCompleted(_that.item);case BridgeThreadNotification_InteractionChanged():
return interactionChanged(_that.interaction);case BridgeThreadNotification_ThreadRuntimeUpdated():
return threadRuntimeUpdated(_that.runtime);case BridgeThreadNotification_Lagged():
return lagged(_that.dropped);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeTurn turn)?  turnStarted,TResult? Function( BridgeTurn turn)?  turnUpdated,TResult? Function( BridgeTurn turn)?  turnCompleted,TResult? Function( BridgeThreadItem item)?  itemStarted,TResult? Function( BridgeThreadItemDelta delta)?  itemDelta,TResult? Function( BridgeThreadItem item)?  itemCompleted,TResult? Function( BridgeInteractionRequest interaction)?  interactionChanged,TResult? Function( BridgeThreadRuntimeSnapshot runtime)?  threadRuntimeUpdated,TResult? Function( BigInt dropped)?  lagged,}) {final _that = this;
switch (_that) {
case BridgeThreadNotification_TurnStarted() when turnStarted != null:
return turnStarted(_that.turn);case BridgeThreadNotification_TurnUpdated() when turnUpdated != null:
return turnUpdated(_that.turn);case BridgeThreadNotification_TurnCompleted() when turnCompleted != null:
return turnCompleted(_that.turn);case BridgeThreadNotification_ItemStarted() when itemStarted != null:
return itemStarted(_that.item);case BridgeThreadNotification_ItemDelta() when itemDelta != null:
return itemDelta(_that.delta);case BridgeThreadNotification_ItemCompleted() when itemCompleted != null:
return itemCompleted(_that.item);case BridgeThreadNotification_InteractionChanged() when interactionChanged != null:
return interactionChanged(_that.interaction);case BridgeThreadNotification_ThreadRuntimeUpdated() when threadRuntimeUpdated != null:
return threadRuntimeUpdated(_that.runtime);case BridgeThreadNotification_Lagged() when lagged != null:
return lagged(_that.dropped);case _:
  return null;

}
}

}

/// @nodoc


class BridgeThreadNotification_TurnStarted extends BridgeThreadNotification {
  const BridgeThreadNotification_TurnStarted({required this.turn}): super._();


 final  BridgeTurn turn;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadNotification_TurnStartedCopyWith<BridgeThreadNotification_TurnStarted> get copyWith => _$BridgeThreadNotification_TurnStartedCopyWithImpl<BridgeThreadNotification_TurnStarted>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadNotification_TurnStarted&&(identical(other.turn, turn) || other.turn == turn));
}


@override
int get hashCode => Object.hash(runtimeType,turn);

@override
String toString() {
  return 'BridgeThreadNotification.turnStarted(turn: $turn)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadNotification_TurnStartedCopyWith<$Res> implements $BridgeThreadNotificationCopyWith<$Res> {
  factory $BridgeThreadNotification_TurnStartedCopyWith(BridgeThreadNotification_TurnStarted value, $Res Function(BridgeThreadNotification_TurnStarted) _then) = _$BridgeThreadNotification_TurnStartedCopyWithImpl;
@useResult
$Res call({
 BridgeTurn turn
});




}
/// @nodoc
class _$BridgeThreadNotification_TurnStartedCopyWithImpl<$Res>
    implements $BridgeThreadNotification_TurnStartedCopyWith<$Res> {
  _$BridgeThreadNotification_TurnStartedCopyWithImpl(this._self, this._then);

  final BridgeThreadNotification_TurnStarted _self;
  final $Res Function(BridgeThreadNotification_TurnStarted) _then;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? turn = null,}) {
  return _then(BridgeThreadNotification_TurnStarted(
turn: null == turn ? _self.turn : turn // ignore: cast_nullable_to_non_nullable
as BridgeTurn,
  ));
}


}

/// @nodoc


class BridgeThreadNotification_TurnUpdated extends BridgeThreadNotification {
  const BridgeThreadNotification_TurnUpdated({required this.turn}): super._();


 final  BridgeTurn turn;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadNotification_TurnUpdatedCopyWith<BridgeThreadNotification_TurnUpdated> get copyWith => _$BridgeThreadNotification_TurnUpdatedCopyWithImpl<BridgeThreadNotification_TurnUpdated>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadNotification_TurnUpdated&&(identical(other.turn, turn) || other.turn == turn));
}


@override
int get hashCode => Object.hash(runtimeType,turn);

@override
String toString() {
  return 'BridgeThreadNotification.turnUpdated(turn: $turn)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadNotification_TurnUpdatedCopyWith<$Res> implements $BridgeThreadNotificationCopyWith<$Res> {
  factory $BridgeThreadNotification_TurnUpdatedCopyWith(BridgeThreadNotification_TurnUpdated value, $Res Function(BridgeThreadNotification_TurnUpdated) _then) = _$BridgeThreadNotification_TurnUpdatedCopyWithImpl;
@useResult
$Res call({
 BridgeTurn turn
});




}
/// @nodoc
class _$BridgeThreadNotification_TurnUpdatedCopyWithImpl<$Res>
    implements $BridgeThreadNotification_TurnUpdatedCopyWith<$Res> {
  _$BridgeThreadNotification_TurnUpdatedCopyWithImpl(this._self, this._then);

  final BridgeThreadNotification_TurnUpdated _self;
  final $Res Function(BridgeThreadNotification_TurnUpdated) _then;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? turn = null,}) {
  return _then(BridgeThreadNotification_TurnUpdated(
turn: null == turn ? _self.turn : turn // ignore: cast_nullable_to_non_nullable
as BridgeTurn,
  ));
}


}

/// @nodoc


class BridgeThreadNotification_TurnCompleted extends BridgeThreadNotification {
  const BridgeThreadNotification_TurnCompleted({required this.turn}): super._();


 final  BridgeTurn turn;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadNotification_TurnCompletedCopyWith<BridgeThreadNotification_TurnCompleted> get copyWith => _$BridgeThreadNotification_TurnCompletedCopyWithImpl<BridgeThreadNotification_TurnCompleted>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadNotification_TurnCompleted&&(identical(other.turn, turn) || other.turn == turn));
}


@override
int get hashCode => Object.hash(runtimeType,turn);

@override
String toString() {
  return 'BridgeThreadNotification.turnCompleted(turn: $turn)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadNotification_TurnCompletedCopyWith<$Res> implements $BridgeThreadNotificationCopyWith<$Res> {
  factory $BridgeThreadNotification_TurnCompletedCopyWith(BridgeThreadNotification_TurnCompleted value, $Res Function(BridgeThreadNotification_TurnCompleted) _then) = _$BridgeThreadNotification_TurnCompletedCopyWithImpl;
@useResult
$Res call({
 BridgeTurn turn
});




}
/// @nodoc
class _$BridgeThreadNotification_TurnCompletedCopyWithImpl<$Res>
    implements $BridgeThreadNotification_TurnCompletedCopyWith<$Res> {
  _$BridgeThreadNotification_TurnCompletedCopyWithImpl(this._self, this._then);

  final BridgeThreadNotification_TurnCompleted _self;
  final $Res Function(BridgeThreadNotification_TurnCompleted) _then;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? turn = null,}) {
  return _then(BridgeThreadNotification_TurnCompleted(
turn: null == turn ? _self.turn : turn // ignore: cast_nullable_to_non_nullable
as BridgeTurn,
  ));
}


}

/// @nodoc


class BridgeThreadNotification_ItemStarted extends BridgeThreadNotification {
  const BridgeThreadNotification_ItemStarted({required this.item}): super._();


 final  BridgeThreadItem item;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadNotification_ItemStartedCopyWith<BridgeThreadNotification_ItemStarted> get copyWith => _$BridgeThreadNotification_ItemStartedCopyWithImpl<BridgeThreadNotification_ItemStarted>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadNotification_ItemStarted&&(identical(other.item, item) || other.item == item));
}


@override
int get hashCode => Object.hash(runtimeType,item);

@override
String toString() {
  return 'BridgeThreadNotification.itemStarted(item: $item)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadNotification_ItemStartedCopyWith<$Res> implements $BridgeThreadNotificationCopyWith<$Res> {
  factory $BridgeThreadNotification_ItemStartedCopyWith(BridgeThreadNotification_ItemStarted value, $Res Function(BridgeThreadNotification_ItemStarted) _then) = _$BridgeThreadNotification_ItemStartedCopyWithImpl;
@useResult
$Res call({
 BridgeThreadItem item
});




}
/// @nodoc
class _$BridgeThreadNotification_ItemStartedCopyWithImpl<$Res>
    implements $BridgeThreadNotification_ItemStartedCopyWith<$Res> {
  _$BridgeThreadNotification_ItemStartedCopyWithImpl(this._self, this._then);

  final BridgeThreadNotification_ItemStarted _self;
  final $Res Function(BridgeThreadNotification_ItemStarted) _then;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? item = null,}) {
  return _then(BridgeThreadNotification_ItemStarted(
item: null == item ? _self.item : item // ignore: cast_nullable_to_non_nullable
as BridgeThreadItem,
  ));
}


}

/// @nodoc


class BridgeThreadNotification_ItemDelta extends BridgeThreadNotification {
  const BridgeThreadNotification_ItemDelta({required this.delta}): super._();


 final  BridgeThreadItemDelta delta;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadNotification_ItemDeltaCopyWith<BridgeThreadNotification_ItemDelta> get copyWith => _$BridgeThreadNotification_ItemDeltaCopyWithImpl<BridgeThreadNotification_ItemDelta>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadNotification_ItemDelta&&(identical(other.delta, delta) || other.delta == delta));
}


@override
int get hashCode => Object.hash(runtimeType,delta);

@override
String toString() {
  return 'BridgeThreadNotification.itemDelta(delta: $delta)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadNotification_ItemDeltaCopyWith<$Res> implements $BridgeThreadNotificationCopyWith<$Res> {
  factory $BridgeThreadNotification_ItemDeltaCopyWith(BridgeThreadNotification_ItemDelta value, $Res Function(BridgeThreadNotification_ItemDelta) _then) = _$BridgeThreadNotification_ItemDeltaCopyWithImpl;
@useResult
$Res call({
 BridgeThreadItemDelta delta
});




}
/// @nodoc
class _$BridgeThreadNotification_ItemDeltaCopyWithImpl<$Res>
    implements $BridgeThreadNotification_ItemDeltaCopyWith<$Res> {
  _$BridgeThreadNotification_ItemDeltaCopyWithImpl(this._self, this._then);

  final BridgeThreadNotification_ItemDelta _self;
  final $Res Function(BridgeThreadNotification_ItemDelta) _then;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? delta = null,}) {
  return _then(BridgeThreadNotification_ItemDelta(
delta: null == delta ? _self.delta : delta // ignore: cast_nullable_to_non_nullable
as BridgeThreadItemDelta,
  ));
}


}

/// @nodoc


class BridgeThreadNotification_ItemCompleted extends BridgeThreadNotification {
  const BridgeThreadNotification_ItemCompleted({required this.item}): super._();


 final  BridgeThreadItem item;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadNotification_ItemCompletedCopyWith<BridgeThreadNotification_ItemCompleted> get copyWith => _$BridgeThreadNotification_ItemCompletedCopyWithImpl<BridgeThreadNotification_ItemCompleted>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadNotification_ItemCompleted&&(identical(other.item, item) || other.item == item));
}


@override
int get hashCode => Object.hash(runtimeType,item);

@override
String toString() {
  return 'BridgeThreadNotification.itemCompleted(item: $item)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadNotification_ItemCompletedCopyWith<$Res> implements $BridgeThreadNotificationCopyWith<$Res> {
  factory $BridgeThreadNotification_ItemCompletedCopyWith(BridgeThreadNotification_ItemCompleted value, $Res Function(BridgeThreadNotification_ItemCompleted) _then) = _$BridgeThreadNotification_ItemCompletedCopyWithImpl;
@useResult
$Res call({
 BridgeThreadItem item
});




}
/// @nodoc
class _$BridgeThreadNotification_ItemCompletedCopyWithImpl<$Res>
    implements $BridgeThreadNotification_ItemCompletedCopyWith<$Res> {
  _$BridgeThreadNotification_ItemCompletedCopyWithImpl(this._self, this._then);

  final BridgeThreadNotification_ItemCompleted _self;
  final $Res Function(BridgeThreadNotification_ItemCompleted) _then;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? item = null,}) {
  return _then(BridgeThreadNotification_ItemCompleted(
item: null == item ? _self.item : item // ignore: cast_nullable_to_non_nullable
as BridgeThreadItem,
  ));
}


}

/// @nodoc


class BridgeThreadNotification_InteractionChanged extends BridgeThreadNotification {
  const BridgeThreadNotification_InteractionChanged({required this.interaction}): super._();


 final  BridgeInteractionRequest interaction;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadNotification_InteractionChangedCopyWith<BridgeThreadNotification_InteractionChanged> get copyWith => _$BridgeThreadNotification_InteractionChangedCopyWithImpl<BridgeThreadNotification_InteractionChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadNotification_InteractionChanged&&(identical(other.interaction, interaction) || other.interaction == interaction));
}


@override
int get hashCode => Object.hash(runtimeType,interaction);

@override
String toString() {
  return 'BridgeThreadNotification.interactionChanged(interaction: $interaction)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadNotification_InteractionChangedCopyWith<$Res> implements $BridgeThreadNotificationCopyWith<$Res> {
  factory $BridgeThreadNotification_InteractionChangedCopyWith(BridgeThreadNotification_InteractionChanged value, $Res Function(BridgeThreadNotification_InteractionChanged) _then) = _$BridgeThreadNotification_InteractionChangedCopyWithImpl;
@useResult
$Res call({
 BridgeInteractionRequest interaction
});




}
/// @nodoc
class _$BridgeThreadNotification_InteractionChangedCopyWithImpl<$Res>
    implements $BridgeThreadNotification_InteractionChangedCopyWith<$Res> {
  _$BridgeThreadNotification_InteractionChangedCopyWithImpl(this._self, this._then);

  final BridgeThreadNotification_InteractionChanged _self;
  final $Res Function(BridgeThreadNotification_InteractionChanged) _then;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? interaction = null,}) {
  return _then(BridgeThreadNotification_InteractionChanged(
interaction: null == interaction ? _self.interaction : interaction // ignore: cast_nullable_to_non_nullable
as BridgeInteractionRequest,
  ));
}


}

/// @nodoc


class BridgeThreadNotification_ThreadRuntimeUpdated extends BridgeThreadNotification {
  const BridgeThreadNotification_ThreadRuntimeUpdated({required this.runtime}): super._();


 final  BridgeThreadRuntimeSnapshot runtime;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadNotification_ThreadRuntimeUpdatedCopyWith<BridgeThreadNotification_ThreadRuntimeUpdated> get copyWith => _$BridgeThreadNotification_ThreadRuntimeUpdatedCopyWithImpl<BridgeThreadNotification_ThreadRuntimeUpdated>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadNotification_ThreadRuntimeUpdated&&(identical(other.runtime, runtime) || other.runtime == runtime));
}


@override
int get hashCode => Object.hash(runtimeType,runtime);

@override
String toString() {
  return 'BridgeThreadNotification.threadRuntimeUpdated(runtime: $runtime)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadNotification_ThreadRuntimeUpdatedCopyWith<$Res> implements $BridgeThreadNotificationCopyWith<$Res> {
  factory $BridgeThreadNotification_ThreadRuntimeUpdatedCopyWith(BridgeThreadNotification_ThreadRuntimeUpdated value, $Res Function(BridgeThreadNotification_ThreadRuntimeUpdated) _then) = _$BridgeThreadNotification_ThreadRuntimeUpdatedCopyWithImpl;
@useResult
$Res call({
 BridgeThreadRuntimeSnapshot runtime
});




}
/// @nodoc
class _$BridgeThreadNotification_ThreadRuntimeUpdatedCopyWithImpl<$Res>
    implements $BridgeThreadNotification_ThreadRuntimeUpdatedCopyWith<$Res> {
  _$BridgeThreadNotification_ThreadRuntimeUpdatedCopyWithImpl(this._self, this._then);

  final BridgeThreadNotification_ThreadRuntimeUpdated _self;
  final $Res Function(BridgeThreadNotification_ThreadRuntimeUpdated) _then;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? runtime = null,}) {
  return _then(BridgeThreadNotification_ThreadRuntimeUpdated(
runtime: null == runtime ? _self.runtime : runtime // ignore: cast_nullable_to_non_nullable
as BridgeThreadRuntimeSnapshot,
  ));
}


}

/// @nodoc


class BridgeThreadNotification_Lagged extends BridgeThreadNotification {
  const BridgeThreadNotification_Lagged({required this.dropped}): super._();


 final  BigInt dropped;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadNotification_LaggedCopyWith<BridgeThreadNotification_Lagged> get copyWith => _$BridgeThreadNotification_LaggedCopyWithImpl<BridgeThreadNotification_Lagged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadNotification_Lagged&&(identical(other.dropped, dropped) || other.dropped == dropped));
}


@override
int get hashCode => Object.hash(runtimeType,dropped);

@override
String toString() {
  return 'BridgeThreadNotification.lagged(dropped: $dropped)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadNotification_LaggedCopyWith<$Res> implements $BridgeThreadNotificationCopyWith<$Res> {
  factory $BridgeThreadNotification_LaggedCopyWith(BridgeThreadNotification_Lagged value, $Res Function(BridgeThreadNotification_Lagged) _then) = _$BridgeThreadNotification_LaggedCopyWithImpl;
@useResult
$Res call({
 BigInt dropped
});




}
/// @nodoc
class _$BridgeThreadNotification_LaggedCopyWithImpl<$Res>
    implements $BridgeThreadNotification_LaggedCopyWith<$Res> {
  _$BridgeThreadNotification_LaggedCopyWithImpl(this._self, this._then);

  final BridgeThreadNotification_Lagged _self;
  final $Res Function(BridgeThreadNotification_Lagged) _then;

/// Create a copy of BridgeThreadNotification
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? dropped = null,}) {
  return _then(BridgeThreadNotification_Lagged(
dropped: null == dropped ? _self.dropped : dropped // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

/// @nodoc
mixin _$BridgeThreadSubscriptionUpdate {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadSubscriptionUpdate);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeThreadSubscriptionUpdate()';
}


}

/// @nodoc
class $BridgeThreadSubscriptionUpdateCopyWith<$Res>  {
$BridgeThreadSubscriptionUpdateCopyWith(BridgeThreadSubscriptionUpdate _, $Res Function(BridgeThreadSubscriptionUpdate) __);
}


/// Adds pattern-matching-related methods to [BridgeThreadSubscriptionUpdate].
extension BridgeThreadSubscriptionUpdatePatterns on BridgeThreadSubscriptionUpdate {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeThreadSubscriptionUpdate_Snapshot value)?  snapshot,TResult Function( BridgeThreadSubscriptionUpdate_Notification value)?  notification,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeThreadSubscriptionUpdate_Snapshot() when snapshot != null:
return snapshot(_that);case BridgeThreadSubscriptionUpdate_Notification() when notification != null:
return notification(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeThreadSubscriptionUpdate_Snapshot value)  snapshot,required TResult Function( BridgeThreadSubscriptionUpdate_Notification value)  notification,}){
final _that = this;
switch (_that) {
case BridgeThreadSubscriptionUpdate_Snapshot():
return snapshot(_that);case BridgeThreadSubscriptionUpdate_Notification():
return notification(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeThreadSubscriptionUpdate_Snapshot value)?  snapshot,TResult? Function( BridgeThreadSubscriptionUpdate_Notification value)?  notification,}){
final _that = this;
switch (_that) {
case BridgeThreadSubscriptionUpdate_Snapshot() when snapshot != null:
return snapshot(_that);case BridgeThreadSubscriptionUpdate_Notification() when notification != null:
return notification(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeThreadSnapshot snapshot)?  snapshot,TResult Function( BridgeThreadNotificationEnvelope notification)?  notification,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeThreadSubscriptionUpdate_Snapshot() when snapshot != null:
return snapshot(_that.snapshot);case BridgeThreadSubscriptionUpdate_Notification() when notification != null:
return notification(_that.notification);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeThreadSnapshot snapshot)  snapshot,required TResult Function( BridgeThreadNotificationEnvelope notification)  notification,}) {final _that = this;
switch (_that) {
case BridgeThreadSubscriptionUpdate_Snapshot():
return snapshot(_that.snapshot);case BridgeThreadSubscriptionUpdate_Notification():
return notification(_that.notification);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeThreadSnapshot snapshot)?  snapshot,TResult? Function( BridgeThreadNotificationEnvelope notification)?  notification,}) {final _that = this;
switch (_that) {
case BridgeThreadSubscriptionUpdate_Snapshot() when snapshot != null:
return snapshot(_that.snapshot);case BridgeThreadSubscriptionUpdate_Notification() when notification != null:
return notification(_that.notification);case _:
  return null;

}
}

}

/// @nodoc


class BridgeThreadSubscriptionUpdate_Snapshot extends BridgeThreadSubscriptionUpdate {
  const BridgeThreadSubscriptionUpdate_Snapshot({required this.snapshot}): super._();


 final  BridgeThreadSnapshot snapshot;

/// Create a copy of BridgeThreadSubscriptionUpdate
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadSubscriptionUpdate_SnapshotCopyWith<BridgeThreadSubscriptionUpdate_Snapshot> get copyWith => _$BridgeThreadSubscriptionUpdate_SnapshotCopyWithImpl<BridgeThreadSubscriptionUpdate_Snapshot>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadSubscriptionUpdate_Snapshot&&(identical(other.snapshot, snapshot) || other.snapshot == snapshot));
}


@override
int get hashCode => Object.hash(runtimeType,snapshot);

@override
String toString() {
  return 'BridgeThreadSubscriptionUpdate.snapshot(snapshot: $snapshot)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadSubscriptionUpdate_SnapshotCopyWith<$Res> implements $BridgeThreadSubscriptionUpdateCopyWith<$Res> {
  factory $BridgeThreadSubscriptionUpdate_SnapshotCopyWith(BridgeThreadSubscriptionUpdate_Snapshot value, $Res Function(BridgeThreadSubscriptionUpdate_Snapshot) _then) = _$BridgeThreadSubscriptionUpdate_SnapshotCopyWithImpl;
@useResult
$Res call({
 BridgeThreadSnapshot snapshot
});




}
/// @nodoc
class _$BridgeThreadSubscriptionUpdate_SnapshotCopyWithImpl<$Res>
    implements $BridgeThreadSubscriptionUpdate_SnapshotCopyWith<$Res> {
  _$BridgeThreadSubscriptionUpdate_SnapshotCopyWithImpl(this._self, this._then);

  final BridgeThreadSubscriptionUpdate_Snapshot _self;
  final $Res Function(BridgeThreadSubscriptionUpdate_Snapshot) _then;

/// Create a copy of BridgeThreadSubscriptionUpdate
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? snapshot = null,}) {
  return _then(BridgeThreadSubscriptionUpdate_Snapshot(
snapshot: null == snapshot ? _self.snapshot : snapshot // ignore: cast_nullable_to_non_nullable
as BridgeThreadSnapshot,
  ));
}


}

/// @nodoc


class BridgeThreadSubscriptionUpdate_Notification extends BridgeThreadSubscriptionUpdate {
  const BridgeThreadSubscriptionUpdate_Notification({required this.notification}): super._();


 final  BridgeThreadNotificationEnvelope notification;

/// Create a copy of BridgeThreadSubscriptionUpdate
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeThreadSubscriptionUpdate_NotificationCopyWith<BridgeThreadSubscriptionUpdate_Notification> get copyWith => _$BridgeThreadSubscriptionUpdate_NotificationCopyWithImpl<BridgeThreadSubscriptionUpdate_Notification>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeThreadSubscriptionUpdate_Notification&&(identical(other.notification, notification) || other.notification == notification));
}


@override
int get hashCode => Object.hash(runtimeType,notification);

@override
String toString() {
  return 'BridgeThreadSubscriptionUpdate.notification(notification: $notification)';
}


}

/// @nodoc
abstract mixin class $BridgeThreadSubscriptionUpdate_NotificationCopyWith<$Res> implements $BridgeThreadSubscriptionUpdateCopyWith<$Res> {
  factory $BridgeThreadSubscriptionUpdate_NotificationCopyWith(BridgeThreadSubscriptionUpdate_Notification value, $Res Function(BridgeThreadSubscriptionUpdate_Notification) _then) = _$BridgeThreadSubscriptionUpdate_NotificationCopyWithImpl;
@useResult
$Res call({
 BridgeThreadNotificationEnvelope notification
});




}
/// @nodoc
class _$BridgeThreadSubscriptionUpdate_NotificationCopyWithImpl<$Res>
    implements $BridgeThreadSubscriptionUpdate_NotificationCopyWith<$Res> {
  _$BridgeThreadSubscriptionUpdate_NotificationCopyWithImpl(this._self, this._then);

  final BridgeThreadSubscriptionUpdate_Notification _self;
  final $Res Function(BridgeThreadSubscriptionUpdate_Notification) _then;

/// Create a copy of BridgeThreadSubscriptionUpdate
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? notification = null,}) {
  return _then(BridgeThreadSubscriptionUpdate_Notification(
notification: null == notification ? _self.notification : notification // ignore: cast_nullable_to_non_nullable
as BridgeThreadNotificationEnvelope,
  ));
}


}

/// @nodoc
mixin _$BridgeTurnState {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeTurnState()';
}


}

/// @nodoc
class $BridgeTurnStateCopyWith<$Res>  {
$BridgeTurnStateCopyWith(BridgeTurnState _, $Res Function(BridgeTurnState) __);
}


/// Adds pattern-matching-related methods to [BridgeTurnState].
extension BridgeTurnStatePatterns on BridgeTurnState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTurnState_Queued value)?  queued,TResult Function( BridgeTurnState_InProgress value)?  inProgress,TResult Function( BridgeTurnState_Completed value)?  completed,TResult Function( BridgeTurnState_Failed value)?  failed,TResult Function( BridgeTurnState_Interrupted value)?  interrupted,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTurnState_Queued() when queued != null:
return queued(_that);case BridgeTurnState_InProgress() when inProgress != null:
return inProgress(_that);case BridgeTurnState_Completed() when completed != null:
return completed(_that);case BridgeTurnState_Failed() when failed != null:
return failed(_that);case BridgeTurnState_Interrupted() when interrupted != null:
return interrupted(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTurnState_Queued value)  queued,required TResult Function( BridgeTurnState_InProgress value)  inProgress,required TResult Function( BridgeTurnState_Completed value)  completed,required TResult Function( BridgeTurnState_Failed value)  failed,required TResult Function( BridgeTurnState_Interrupted value)  interrupted,}){
final _that = this;
switch (_that) {
case BridgeTurnState_Queued():
return queued(_that);case BridgeTurnState_InProgress():
return inProgress(_that);case BridgeTurnState_Completed():
return completed(_that);case BridgeTurnState_Failed():
return failed(_that);case BridgeTurnState_Interrupted():
return interrupted(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTurnState_Queued value)?  queued,TResult? Function( BridgeTurnState_InProgress value)?  inProgress,TResult? Function( BridgeTurnState_Completed value)?  completed,TResult? Function( BridgeTurnState_Failed value)?  failed,TResult? Function( BridgeTurnState_Interrupted value)?  interrupted,}){
final _that = this;
switch (_that) {
case BridgeTurnState_Queued() when queued != null:
return queued(_that);case BridgeTurnState_InProgress() when inProgress != null:
return inProgress(_that);case BridgeTurnState_Completed() when completed != null:
return completed(_that);case BridgeTurnState_Failed() when failed != null:
return failed(_that);case BridgeTurnState_Interrupted() when interrupted != null:
return interrupted(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  queued,TResult Function( BridgeTurnPhase phase)?  inProgress,TResult Function()?  completed,TResult Function( String reason)?  failed,TResult Function( String reason)?  interrupted,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTurnState_Queued() when queued != null:
return queued();case BridgeTurnState_InProgress() when inProgress != null:
return inProgress(_that.phase);case BridgeTurnState_Completed() when completed != null:
return completed();case BridgeTurnState_Failed() when failed != null:
return failed(_that.reason);case BridgeTurnState_Interrupted() when interrupted != null:
return interrupted(_that.reason);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  queued,required TResult Function( BridgeTurnPhase phase)  inProgress,required TResult Function()  completed,required TResult Function( String reason)  failed,required TResult Function( String reason)  interrupted,}) {final _that = this;
switch (_that) {
case BridgeTurnState_Queued():
return queued();case BridgeTurnState_InProgress():
return inProgress(_that.phase);case BridgeTurnState_Completed():
return completed();case BridgeTurnState_Failed():
return failed(_that.reason);case BridgeTurnState_Interrupted():
return interrupted(_that.reason);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  queued,TResult? Function( BridgeTurnPhase phase)?  inProgress,TResult? Function()?  completed,TResult? Function( String reason)?  failed,TResult? Function( String reason)?  interrupted,}) {final _that = this;
switch (_that) {
case BridgeTurnState_Queued() when queued != null:
return queued();case BridgeTurnState_InProgress() when inProgress != null:
return inProgress(_that.phase);case BridgeTurnState_Completed() when completed != null:
return completed();case BridgeTurnState_Failed() when failed != null:
return failed(_that.reason);case BridgeTurnState_Interrupted() when interrupted != null:
return interrupted(_that.reason);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTurnState_Queued extends BridgeTurnState {
  const BridgeTurnState_Queued(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnState_Queued);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeTurnState.queued()';
}


}




/// @nodoc


class BridgeTurnState_InProgress extends BridgeTurnState {
  const BridgeTurnState_InProgress({required this.phase}): super._();


 final  BridgeTurnPhase phase;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTurnState_InProgressCopyWith<BridgeTurnState_InProgress> get copyWith => _$BridgeTurnState_InProgressCopyWithImpl<BridgeTurnState_InProgress>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnState_InProgress&&(identical(other.phase, phase) || other.phase == phase));
}


@override
int get hashCode => Object.hash(runtimeType,phase);

@override
String toString() {
  return 'BridgeTurnState.inProgress(phase: $phase)';
}


}

/// @nodoc
abstract mixin class $BridgeTurnState_InProgressCopyWith<$Res> implements $BridgeTurnStateCopyWith<$Res> {
  factory $BridgeTurnState_InProgressCopyWith(BridgeTurnState_InProgress value, $Res Function(BridgeTurnState_InProgress) _then) = _$BridgeTurnState_InProgressCopyWithImpl;
@useResult
$Res call({
 BridgeTurnPhase phase
});




}
/// @nodoc
class _$BridgeTurnState_InProgressCopyWithImpl<$Res>
    implements $BridgeTurnState_InProgressCopyWith<$Res> {
  _$BridgeTurnState_InProgressCopyWithImpl(this._self, this._then);

  final BridgeTurnState_InProgress _self;
  final $Res Function(BridgeTurnState_InProgress) _then;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? phase = null,}) {
  return _then(BridgeTurnState_InProgress(
phase: null == phase ? _self.phase : phase // ignore: cast_nullable_to_non_nullable
as BridgeTurnPhase,
  ));
}


}

/// @nodoc


class BridgeTurnState_Completed extends BridgeTurnState {
  const BridgeTurnState_Completed(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnState_Completed);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeTurnState.completed()';
}


}




/// @nodoc


class BridgeTurnState_Failed extends BridgeTurnState {
  const BridgeTurnState_Failed({required this.reason}): super._();


 final  String reason;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTurnState_FailedCopyWith<BridgeTurnState_Failed> get copyWith => _$BridgeTurnState_FailedCopyWithImpl<BridgeTurnState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnState_Failed&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,reason);

@override
String toString() {
  return 'BridgeTurnState.failed(reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeTurnState_FailedCopyWith<$Res> implements $BridgeTurnStateCopyWith<$Res> {
  factory $BridgeTurnState_FailedCopyWith(BridgeTurnState_Failed value, $Res Function(BridgeTurnState_Failed) _then) = _$BridgeTurnState_FailedCopyWithImpl;
@useResult
$Res call({
 String reason
});




}
/// @nodoc
class _$BridgeTurnState_FailedCopyWithImpl<$Res>
    implements $BridgeTurnState_FailedCopyWith<$Res> {
  _$BridgeTurnState_FailedCopyWithImpl(this._self, this._then);

  final BridgeTurnState_Failed _self;
  final $Res Function(BridgeTurnState_Failed) _then;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reason = null,}) {
  return _then(BridgeTurnState_Failed(
reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeTurnState_Interrupted extends BridgeTurnState {
  const BridgeTurnState_Interrupted({required this.reason}): super._();


 final  String reason;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTurnState_InterruptedCopyWith<BridgeTurnState_Interrupted> get copyWith => _$BridgeTurnState_InterruptedCopyWithImpl<BridgeTurnState_Interrupted>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnState_Interrupted&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,reason);

@override
String toString() {
  return 'BridgeTurnState.interrupted(reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeTurnState_InterruptedCopyWith<$Res> implements $BridgeTurnStateCopyWith<$Res> {
  factory $BridgeTurnState_InterruptedCopyWith(BridgeTurnState_Interrupted value, $Res Function(BridgeTurnState_Interrupted) _then) = _$BridgeTurnState_InterruptedCopyWithImpl;
@useResult
$Res call({
 String reason
});




}
/// @nodoc
class _$BridgeTurnState_InterruptedCopyWithImpl<$Res>
    implements $BridgeTurnState_InterruptedCopyWith<$Res> {
  _$BridgeTurnState_InterruptedCopyWithImpl(this._self, this._then);

  final BridgeTurnState_Interrupted _self;
  final $Res Function(BridgeTurnState_Interrupted) _then;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reason = null,}) {
  return _then(BridgeTurnState_Interrupted(
reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

// dart format on

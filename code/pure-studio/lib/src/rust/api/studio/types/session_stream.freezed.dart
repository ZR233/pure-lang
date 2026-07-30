// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'session_stream.dart';

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
mixin _$BridgeSessionEventKind {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeSessionEventKind()';
}


}

/// @nodoc
class $BridgeSessionEventKindCopyWith<$Res>  {
$BridgeSessionEventKindCopyWith(BridgeSessionEventKind _, $Res Function(BridgeSessionEventKind) __);
}


/// Adds pattern-matching-related methods to [BridgeSessionEventKind].
extension BridgeSessionEventKindPatterns on BridgeSessionEventKind {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeSessionEventKind_TurnChanged value)?  turnChanged,TResult Function( BridgeSessionEventKind_MessageChanged value)?  messageChanged,TResult Function( BridgeSessionEventKind_MessageRemoved value)?  messageRemoved,TResult Function( BridgeSessionEventKind_PartChanged value)?  partChanged,TResult Function( BridgeSessionEventKind_PartRemoved value)?  partRemoved,TResult Function( BridgeSessionEventKind_PartDelta value)?  partDelta,TResult Function( BridgeSessionEventKind_InteractionChanged value)?  interactionChanged,TResult Function( BridgeSessionEventKind_AgentChanged value)?  agentChanged,TResult Function( BridgeSessionEventKind_TimelineEventAppended value)?  timelineEventAppended,TResult Function( BridgeSessionEventKind_RuntimeChanged value)?  runtimeChanged,TResult Function( BridgeSessionEventKind_SkillActivated value)?  skillActivated,TResult Function( BridgeSessionEventKind_PlanChanged value)?  planChanged,TResult Function( BridgeSessionEventKind_ContextCompacted value)?  contextCompacted,TResult Function( BridgeSessionEventKind_ErrorOccurred value)?  errorOccurred,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeSessionEventKind_TurnChanged() when turnChanged != null:
return turnChanged(_that);case BridgeSessionEventKind_MessageChanged() when messageChanged != null:
return messageChanged(_that);case BridgeSessionEventKind_MessageRemoved() when messageRemoved != null:
return messageRemoved(_that);case BridgeSessionEventKind_PartChanged() when partChanged != null:
return partChanged(_that);case BridgeSessionEventKind_PartRemoved() when partRemoved != null:
return partRemoved(_that);case BridgeSessionEventKind_PartDelta() when partDelta != null:
return partDelta(_that);case BridgeSessionEventKind_InteractionChanged() when interactionChanged != null:
return interactionChanged(_that);case BridgeSessionEventKind_AgentChanged() when agentChanged != null:
return agentChanged(_that);case BridgeSessionEventKind_TimelineEventAppended() when timelineEventAppended != null:
return timelineEventAppended(_that);case BridgeSessionEventKind_RuntimeChanged() when runtimeChanged != null:
return runtimeChanged(_that);case BridgeSessionEventKind_SkillActivated() when skillActivated != null:
return skillActivated(_that);case BridgeSessionEventKind_PlanChanged() when planChanged != null:
return planChanged(_that);case BridgeSessionEventKind_ContextCompacted() when contextCompacted != null:
return contextCompacted(_that);case BridgeSessionEventKind_ErrorOccurred() when errorOccurred != null:
return errorOccurred(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeSessionEventKind_TurnChanged value)  turnChanged,required TResult Function( BridgeSessionEventKind_MessageChanged value)  messageChanged,required TResult Function( BridgeSessionEventKind_MessageRemoved value)  messageRemoved,required TResult Function( BridgeSessionEventKind_PartChanged value)  partChanged,required TResult Function( BridgeSessionEventKind_PartRemoved value)  partRemoved,required TResult Function( BridgeSessionEventKind_PartDelta value)  partDelta,required TResult Function( BridgeSessionEventKind_InteractionChanged value)  interactionChanged,required TResult Function( BridgeSessionEventKind_AgentChanged value)  agentChanged,required TResult Function( BridgeSessionEventKind_TimelineEventAppended value)  timelineEventAppended,required TResult Function( BridgeSessionEventKind_RuntimeChanged value)  runtimeChanged,required TResult Function( BridgeSessionEventKind_SkillActivated value)  skillActivated,required TResult Function( BridgeSessionEventKind_PlanChanged value)  planChanged,required TResult Function( BridgeSessionEventKind_ContextCompacted value)  contextCompacted,required TResult Function( BridgeSessionEventKind_ErrorOccurred value)  errorOccurred,}){
final _that = this;
switch (_that) {
case BridgeSessionEventKind_TurnChanged():
return turnChanged(_that);case BridgeSessionEventKind_MessageChanged():
return messageChanged(_that);case BridgeSessionEventKind_MessageRemoved():
return messageRemoved(_that);case BridgeSessionEventKind_PartChanged():
return partChanged(_that);case BridgeSessionEventKind_PartRemoved():
return partRemoved(_that);case BridgeSessionEventKind_PartDelta():
return partDelta(_that);case BridgeSessionEventKind_InteractionChanged():
return interactionChanged(_that);case BridgeSessionEventKind_AgentChanged():
return agentChanged(_that);case BridgeSessionEventKind_TimelineEventAppended():
return timelineEventAppended(_that);case BridgeSessionEventKind_RuntimeChanged():
return runtimeChanged(_that);case BridgeSessionEventKind_SkillActivated():
return skillActivated(_that);case BridgeSessionEventKind_PlanChanged():
return planChanged(_that);case BridgeSessionEventKind_ContextCompacted():
return contextCompacted(_that);case BridgeSessionEventKind_ErrorOccurred():
return errorOccurred(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeSessionEventKind_TurnChanged value)?  turnChanged,TResult? Function( BridgeSessionEventKind_MessageChanged value)?  messageChanged,TResult? Function( BridgeSessionEventKind_MessageRemoved value)?  messageRemoved,TResult? Function( BridgeSessionEventKind_PartChanged value)?  partChanged,TResult? Function( BridgeSessionEventKind_PartRemoved value)?  partRemoved,TResult? Function( BridgeSessionEventKind_PartDelta value)?  partDelta,TResult? Function( BridgeSessionEventKind_InteractionChanged value)?  interactionChanged,TResult? Function( BridgeSessionEventKind_AgentChanged value)?  agentChanged,TResult? Function( BridgeSessionEventKind_TimelineEventAppended value)?  timelineEventAppended,TResult? Function( BridgeSessionEventKind_RuntimeChanged value)?  runtimeChanged,TResult? Function( BridgeSessionEventKind_SkillActivated value)?  skillActivated,TResult? Function( BridgeSessionEventKind_PlanChanged value)?  planChanged,TResult? Function( BridgeSessionEventKind_ContextCompacted value)?  contextCompacted,TResult? Function( BridgeSessionEventKind_ErrorOccurred value)?  errorOccurred,}){
final _that = this;
switch (_that) {
case BridgeSessionEventKind_TurnChanged() when turnChanged != null:
return turnChanged(_that);case BridgeSessionEventKind_MessageChanged() when messageChanged != null:
return messageChanged(_that);case BridgeSessionEventKind_MessageRemoved() when messageRemoved != null:
return messageRemoved(_that);case BridgeSessionEventKind_PartChanged() when partChanged != null:
return partChanged(_that);case BridgeSessionEventKind_PartRemoved() when partRemoved != null:
return partRemoved(_that);case BridgeSessionEventKind_PartDelta() when partDelta != null:
return partDelta(_that);case BridgeSessionEventKind_InteractionChanged() when interactionChanged != null:
return interactionChanged(_that);case BridgeSessionEventKind_AgentChanged() when agentChanged != null:
return agentChanged(_that);case BridgeSessionEventKind_TimelineEventAppended() when timelineEventAppended != null:
return timelineEventAppended(_that);case BridgeSessionEventKind_RuntimeChanged() when runtimeChanged != null:
return runtimeChanged(_that);case BridgeSessionEventKind_SkillActivated() when skillActivated != null:
return skillActivated(_that);case BridgeSessionEventKind_PlanChanged() when planChanged != null:
return planChanged(_that);case BridgeSessionEventKind_ContextCompacted() when contextCompacted != null:
return contextCompacted(_that);case BridgeSessionEventKind_ErrorOccurred() when errorOccurred != null:
return errorOccurred(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeSessionTurn turn)?  turnChanged,TResult Function( BridgeSessionMessage message)?  messageChanged,TResult Function( String messageId)?  messageRemoved,TResult Function( BridgeSessionPart part_)?  partChanged,TResult Function( String messageId,  String partId)?  partRemoved,TResult Function( BridgeSessionPartDelta delta)?  partDelta,TResult Function( BridgeInteractionRequest interaction)?  interactionChanged,TResult Function( BridgeSessionAgentSnapshot agent)?  agentChanged,TResult Function( BridgeSessionTimelineEvent event)?  timelineEventAppended,TResult Function( BridgeSessionRuntimeSnapshot runtime)?  runtimeChanged,TResult Function( BridgeSkillActivation activation)?  skillActivated,TResult Function( BridgePlanLifecycleEvent event)?  planChanged,TResult Function( BridgeSessionContextCompaction compaction)?  contextCompacted,TResult Function( String message,  BridgeErrorSeverity severity)?  errorOccurred,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeSessionEventKind_TurnChanged() when turnChanged != null:
return turnChanged(_that.turn);case BridgeSessionEventKind_MessageChanged() when messageChanged != null:
return messageChanged(_that.message);case BridgeSessionEventKind_MessageRemoved() when messageRemoved != null:
return messageRemoved(_that.messageId);case BridgeSessionEventKind_PartChanged() when partChanged != null:
return partChanged(_that.part_);case BridgeSessionEventKind_PartRemoved() when partRemoved != null:
return partRemoved(_that.messageId,_that.partId);case BridgeSessionEventKind_PartDelta() when partDelta != null:
return partDelta(_that.delta);case BridgeSessionEventKind_InteractionChanged() when interactionChanged != null:
return interactionChanged(_that.interaction);case BridgeSessionEventKind_AgentChanged() when agentChanged != null:
return agentChanged(_that.agent);case BridgeSessionEventKind_TimelineEventAppended() when timelineEventAppended != null:
return timelineEventAppended(_that.event);case BridgeSessionEventKind_RuntimeChanged() when runtimeChanged != null:
return runtimeChanged(_that.runtime);case BridgeSessionEventKind_SkillActivated() when skillActivated != null:
return skillActivated(_that.activation);case BridgeSessionEventKind_PlanChanged() when planChanged != null:
return planChanged(_that.event);case BridgeSessionEventKind_ContextCompacted() when contextCompacted != null:
return contextCompacted(_that.compaction);case BridgeSessionEventKind_ErrorOccurred() when errorOccurred != null:
return errorOccurred(_that.message,_that.severity);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeSessionTurn turn)  turnChanged,required TResult Function( BridgeSessionMessage message)  messageChanged,required TResult Function( String messageId)  messageRemoved,required TResult Function( BridgeSessionPart part_)  partChanged,required TResult Function( String messageId,  String partId)  partRemoved,required TResult Function( BridgeSessionPartDelta delta)  partDelta,required TResult Function( BridgeInteractionRequest interaction)  interactionChanged,required TResult Function( BridgeSessionAgentSnapshot agent)  agentChanged,required TResult Function( BridgeSessionTimelineEvent event)  timelineEventAppended,required TResult Function( BridgeSessionRuntimeSnapshot runtime)  runtimeChanged,required TResult Function( BridgeSkillActivation activation)  skillActivated,required TResult Function( BridgePlanLifecycleEvent event)  planChanged,required TResult Function( BridgeSessionContextCompaction compaction)  contextCompacted,required TResult Function( String message,  BridgeErrorSeverity severity)  errorOccurred,}) {final _that = this;
switch (_that) {
case BridgeSessionEventKind_TurnChanged():
return turnChanged(_that.turn);case BridgeSessionEventKind_MessageChanged():
return messageChanged(_that.message);case BridgeSessionEventKind_MessageRemoved():
return messageRemoved(_that.messageId);case BridgeSessionEventKind_PartChanged():
return partChanged(_that.part_);case BridgeSessionEventKind_PartRemoved():
return partRemoved(_that.messageId,_that.partId);case BridgeSessionEventKind_PartDelta():
return partDelta(_that.delta);case BridgeSessionEventKind_InteractionChanged():
return interactionChanged(_that.interaction);case BridgeSessionEventKind_AgentChanged():
return agentChanged(_that.agent);case BridgeSessionEventKind_TimelineEventAppended():
return timelineEventAppended(_that.event);case BridgeSessionEventKind_RuntimeChanged():
return runtimeChanged(_that.runtime);case BridgeSessionEventKind_SkillActivated():
return skillActivated(_that.activation);case BridgeSessionEventKind_PlanChanged():
return planChanged(_that.event);case BridgeSessionEventKind_ContextCompacted():
return contextCompacted(_that.compaction);case BridgeSessionEventKind_ErrorOccurred():
return errorOccurred(_that.message,_that.severity);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeSessionTurn turn)?  turnChanged,TResult? Function( BridgeSessionMessage message)?  messageChanged,TResult? Function( String messageId)?  messageRemoved,TResult? Function( BridgeSessionPart part_)?  partChanged,TResult? Function( String messageId,  String partId)?  partRemoved,TResult? Function( BridgeSessionPartDelta delta)?  partDelta,TResult? Function( BridgeInteractionRequest interaction)?  interactionChanged,TResult? Function( BridgeSessionAgentSnapshot agent)?  agentChanged,TResult? Function( BridgeSessionTimelineEvent event)?  timelineEventAppended,TResult? Function( BridgeSessionRuntimeSnapshot runtime)?  runtimeChanged,TResult? Function( BridgeSkillActivation activation)?  skillActivated,TResult? Function( BridgePlanLifecycleEvent event)?  planChanged,TResult? Function( BridgeSessionContextCompaction compaction)?  contextCompacted,TResult? Function( String message,  BridgeErrorSeverity severity)?  errorOccurred,}) {final _that = this;
switch (_that) {
case BridgeSessionEventKind_TurnChanged() when turnChanged != null:
return turnChanged(_that.turn);case BridgeSessionEventKind_MessageChanged() when messageChanged != null:
return messageChanged(_that.message);case BridgeSessionEventKind_MessageRemoved() when messageRemoved != null:
return messageRemoved(_that.messageId);case BridgeSessionEventKind_PartChanged() when partChanged != null:
return partChanged(_that.part_);case BridgeSessionEventKind_PartRemoved() when partRemoved != null:
return partRemoved(_that.messageId,_that.partId);case BridgeSessionEventKind_PartDelta() when partDelta != null:
return partDelta(_that.delta);case BridgeSessionEventKind_InteractionChanged() when interactionChanged != null:
return interactionChanged(_that.interaction);case BridgeSessionEventKind_AgentChanged() when agentChanged != null:
return agentChanged(_that.agent);case BridgeSessionEventKind_TimelineEventAppended() when timelineEventAppended != null:
return timelineEventAppended(_that.event);case BridgeSessionEventKind_RuntimeChanged() when runtimeChanged != null:
return runtimeChanged(_that.runtime);case BridgeSessionEventKind_SkillActivated() when skillActivated != null:
return skillActivated(_that.activation);case BridgeSessionEventKind_PlanChanged() when planChanged != null:
return planChanged(_that.event);case BridgeSessionEventKind_ContextCompacted() when contextCompacted != null:
return contextCompacted(_that.compaction);case BridgeSessionEventKind_ErrorOccurred() when errorOccurred != null:
return errorOccurred(_that.message,_that.severity);case _:
  return null;

}
}

}

/// @nodoc


class BridgeSessionEventKind_TurnChanged extends BridgeSessionEventKind {
  const BridgeSessionEventKind_TurnChanged({required this.turn}): super._();
  

 final  BridgeSessionTurn turn;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventKind_TurnChangedCopyWith<BridgeSessionEventKind_TurnChanged> get copyWith => _$BridgeSessionEventKind_TurnChangedCopyWithImpl<BridgeSessionEventKind_TurnChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind_TurnChanged&&(identical(other.turn, turn) || other.turn == turn));
}


@override
int get hashCode => Object.hash(runtimeType,turn);

@override
String toString() {
  return 'BridgeSessionEventKind.turnChanged(turn: $turn)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventKind_TurnChangedCopyWith<$Res> implements $BridgeSessionEventKindCopyWith<$Res> {
  factory $BridgeSessionEventKind_TurnChangedCopyWith(BridgeSessionEventKind_TurnChanged value, $Res Function(BridgeSessionEventKind_TurnChanged) _then) = _$BridgeSessionEventKind_TurnChangedCopyWithImpl;
@useResult
$Res call({
 BridgeSessionTurn turn
});




}
/// @nodoc
class _$BridgeSessionEventKind_TurnChangedCopyWithImpl<$Res>
    implements $BridgeSessionEventKind_TurnChangedCopyWith<$Res> {
  _$BridgeSessionEventKind_TurnChangedCopyWithImpl(this._self, this._then);

  final BridgeSessionEventKind_TurnChanged _self;
  final $Res Function(BridgeSessionEventKind_TurnChanged) _then;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? turn = null,}) {
  return _then(BridgeSessionEventKind_TurnChanged(
turn: null == turn ? _self.turn : turn // ignore: cast_nullable_to_non_nullable
as BridgeSessionTurn,
  ));
}


}

/// @nodoc


class BridgeSessionEventKind_MessageChanged extends BridgeSessionEventKind {
  const BridgeSessionEventKind_MessageChanged({required this.message}): super._();
  

 final  BridgeSessionMessage message;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventKind_MessageChangedCopyWith<BridgeSessionEventKind_MessageChanged> get copyWith => _$BridgeSessionEventKind_MessageChangedCopyWithImpl<BridgeSessionEventKind_MessageChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind_MessageChanged&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode => Object.hash(runtimeType,message);

@override
String toString() {
  return 'BridgeSessionEventKind.messageChanged(message: $message)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventKind_MessageChangedCopyWith<$Res> implements $BridgeSessionEventKindCopyWith<$Res> {
  factory $BridgeSessionEventKind_MessageChangedCopyWith(BridgeSessionEventKind_MessageChanged value, $Res Function(BridgeSessionEventKind_MessageChanged) _then) = _$BridgeSessionEventKind_MessageChangedCopyWithImpl;
@useResult
$Res call({
 BridgeSessionMessage message
});




}
/// @nodoc
class _$BridgeSessionEventKind_MessageChangedCopyWithImpl<$Res>
    implements $BridgeSessionEventKind_MessageChangedCopyWith<$Res> {
  _$BridgeSessionEventKind_MessageChangedCopyWithImpl(this._self, this._then);

  final BridgeSessionEventKind_MessageChanged _self;
  final $Res Function(BridgeSessionEventKind_MessageChanged) _then;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = null,}) {
  return _then(BridgeSessionEventKind_MessageChanged(
message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as BridgeSessionMessage,
  ));
}


}

/// @nodoc


class BridgeSessionEventKind_MessageRemoved extends BridgeSessionEventKind {
  const BridgeSessionEventKind_MessageRemoved({required this.messageId}): super._();
  

 final  String messageId;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventKind_MessageRemovedCopyWith<BridgeSessionEventKind_MessageRemoved> get copyWith => _$BridgeSessionEventKind_MessageRemovedCopyWithImpl<BridgeSessionEventKind_MessageRemoved>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind_MessageRemoved&&(identical(other.messageId, messageId) || other.messageId == messageId));
}


@override
int get hashCode => Object.hash(runtimeType,messageId);

@override
String toString() {
  return 'BridgeSessionEventKind.messageRemoved(messageId: $messageId)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventKind_MessageRemovedCopyWith<$Res> implements $BridgeSessionEventKindCopyWith<$Res> {
  factory $BridgeSessionEventKind_MessageRemovedCopyWith(BridgeSessionEventKind_MessageRemoved value, $Res Function(BridgeSessionEventKind_MessageRemoved) _then) = _$BridgeSessionEventKind_MessageRemovedCopyWithImpl;
@useResult
$Res call({
 String messageId
});




}
/// @nodoc
class _$BridgeSessionEventKind_MessageRemovedCopyWithImpl<$Res>
    implements $BridgeSessionEventKind_MessageRemovedCopyWith<$Res> {
  _$BridgeSessionEventKind_MessageRemovedCopyWithImpl(this._self, this._then);

  final BridgeSessionEventKind_MessageRemoved _self;
  final $Res Function(BridgeSessionEventKind_MessageRemoved) _then;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? messageId = null,}) {
  return _then(BridgeSessionEventKind_MessageRemoved(
messageId: null == messageId ? _self.messageId : messageId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeSessionEventKind_PartChanged extends BridgeSessionEventKind {
  const BridgeSessionEventKind_PartChanged({required this.part_}): super._();
  

 final  BridgeSessionPart part_;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventKind_PartChangedCopyWith<BridgeSessionEventKind_PartChanged> get copyWith => _$BridgeSessionEventKind_PartChangedCopyWithImpl<BridgeSessionEventKind_PartChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind_PartChanged&&(identical(other.part_, part_) || other.part_ == part_));
}


@override
int get hashCode => Object.hash(runtimeType,part_);

@override
String toString() {
  return 'BridgeSessionEventKind.partChanged(part_: $part_)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventKind_PartChangedCopyWith<$Res> implements $BridgeSessionEventKindCopyWith<$Res> {
  factory $BridgeSessionEventKind_PartChangedCopyWith(BridgeSessionEventKind_PartChanged value, $Res Function(BridgeSessionEventKind_PartChanged) _then) = _$BridgeSessionEventKind_PartChangedCopyWithImpl;
@useResult
$Res call({
 BridgeSessionPart part_
});




}
/// @nodoc
class _$BridgeSessionEventKind_PartChangedCopyWithImpl<$Res>
    implements $BridgeSessionEventKind_PartChangedCopyWith<$Res> {
  _$BridgeSessionEventKind_PartChangedCopyWithImpl(this._self, this._then);

  final BridgeSessionEventKind_PartChanged _self;
  final $Res Function(BridgeSessionEventKind_PartChanged) _then;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? part_ = null,}) {
  return _then(BridgeSessionEventKind_PartChanged(
part_: null == part_ ? _self.part_ : part_ // ignore: cast_nullable_to_non_nullable
as BridgeSessionPart,
  ));
}


}

/// @nodoc


class BridgeSessionEventKind_PartRemoved extends BridgeSessionEventKind {
  const BridgeSessionEventKind_PartRemoved({required this.messageId, required this.partId}): super._();
  

 final  String messageId;
 final  String partId;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventKind_PartRemovedCopyWith<BridgeSessionEventKind_PartRemoved> get copyWith => _$BridgeSessionEventKind_PartRemovedCopyWithImpl<BridgeSessionEventKind_PartRemoved>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind_PartRemoved&&(identical(other.messageId, messageId) || other.messageId == messageId)&&(identical(other.partId, partId) || other.partId == partId));
}


@override
int get hashCode => Object.hash(runtimeType,messageId,partId);

@override
String toString() {
  return 'BridgeSessionEventKind.partRemoved(messageId: $messageId, partId: $partId)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventKind_PartRemovedCopyWith<$Res> implements $BridgeSessionEventKindCopyWith<$Res> {
  factory $BridgeSessionEventKind_PartRemovedCopyWith(BridgeSessionEventKind_PartRemoved value, $Res Function(BridgeSessionEventKind_PartRemoved) _then) = _$BridgeSessionEventKind_PartRemovedCopyWithImpl;
@useResult
$Res call({
 String messageId, String partId
});




}
/// @nodoc
class _$BridgeSessionEventKind_PartRemovedCopyWithImpl<$Res>
    implements $BridgeSessionEventKind_PartRemovedCopyWith<$Res> {
  _$BridgeSessionEventKind_PartRemovedCopyWithImpl(this._self, this._then);

  final BridgeSessionEventKind_PartRemoved _self;
  final $Res Function(BridgeSessionEventKind_PartRemoved) _then;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? messageId = null,Object? partId = null,}) {
  return _then(BridgeSessionEventKind_PartRemoved(
messageId: null == messageId ? _self.messageId : messageId // ignore: cast_nullable_to_non_nullable
as String,partId: null == partId ? _self.partId : partId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeSessionEventKind_PartDelta extends BridgeSessionEventKind {
  const BridgeSessionEventKind_PartDelta({required this.delta}): super._();
  

 final  BridgeSessionPartDelta delta;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventKind_PartDeltaCopyWith<BridgeSessionEventKind_PartDelta> get copyWith => _$BridgeSessionEventKind_PartDeltaCopyWithImpl<BridgeSessionEventKind_PartDelta>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind_PartDelta&&(identical(other.delta, delta) || other.delta == delta));
}


@override
int get hashCode => Object.hash(runtimeType,delta);

@override
String toString() {
  return 'BridgeSessionEventKind.partDelta(delta: $delta)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventKind_PartDeltaCopyWith<$Res> implements $BridgeSessionEventKindCopyWith<$Res> {
  factory $BridgeSessionEventKind_PartDeltaCopyWith(BridgeSessionEventKind_PartDelta value, $Res Function(BridgeSessionEventKind_PartDelta) _then) = _$BridgeSessionEventKind_PartDeltaCopyWithImpl;
@useResult
$Res call({
 BridgeSessionPartDelta delta
});




}
/// @nodoc
class _$BridgeSessionEventKind_PartDeltaCopyWithImpl<$Res>
    implements $BridgeSessionEventKind_PartDeltaCopyWith<$Res> {
  _$BridgeSessionEventKind_PartDeltaCopyWithImpl(this._self, this._then);

  final BridgeSessionEventKind_PartDelta _self;
  final $Res Function(BridgeSessionEventKind_PartDelta) _then;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? delta = null,}) {
  return _then(BridgeSessionEventKind_PartDelta(
delta: null == delta ? _self.delta : delta // ignore: cast_nullable_to_non_nullable
as BridgeSessionPartDelta,
  ));
}


}

/// @nodoc


class BridgeSessionEventKind_InteractionChanged extends BridgeSessionEventKind {
  const BridgeSessionEventKind_InteractionChanged({required this.interaction}): super._();
  

 final  BridgeInteractionRequest interaction;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventKind_InteractionChangedCopyWith<BridgeSessionEventKind_InteractionChanged> get copyWith => _$BridgeSessionEventKind_InteractionChangedCopyWithImpl<BridgeSessionEventKind_InteractionChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind_InteractionChanged&&(identical(other.interaction, interaction) || other.interaction == interaction));
}


@override
int get hashCode => Object.hash(runtimeType,interaction);

@override
String toString() {
  return 'BridgeSessionEventKind.interactionChanged(interaction: $interaction)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventKind_InteractionChangedCopyWith<$Res> implements $BridgeSessionEventKindCopyWith<$Res> {
  factory $BridgeSessionEventKind_InteractionChangedCopyWith(BridgeSessionEventKind_InteractionChanged value, $Res Function(BridgeSessionEventKind_InteractionChanged) _then) = _$BridgeSessionEventKind_InteractionChangedCopyWithImpl;
@useResult
$Res call({
 BridgeInteractionRequest interaction
});




}
/// @nodoc
class _$BridgeSessionEventKind_InteractionChangedCopyWithImpl<$Res>
    implements $BridgeSessionEventKind_InteractionChangedCopyWith<$Res> {
  _$BridgeSessionEventKind_InteractionChangedCopyWithImpl(this._self, this._then);

  final BridgeSessionEventKind_InteractionChanged _self;
  final $Res Function(BridgeSessionEventKind_InteractionChanged) _then;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? interaction = null,}) {
  return _then(BridgeSessionEventKind_InteractionChanged(
interaction: null == interaction ? _self.interaction : interaction // ignore: cast_nullable_to_non_nullable
as BridgeInteractionRequest,
  ));
}


}

/// @nodoc


class BridgeSessionEventKind_AgentChanged extends BridgeSessionEventKind {
  const BridgeSessionEventKind_AgentChanged({required this.agent}): super._();
  

 final  BridgeSessionAgentSnapshot agent;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventKind_AgentChangedCopyWith<BridgeSessionEventKind_AgentChanged> get copyWith => _$BridgeSessionEventKind_AgentChangedCopyWithImpl<BridgeSessionEventKind_AgentChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind_AgentChanged&&(identical(other.agent, agent) || other.agent == agent));
}


@override
int get hashCode => Object.hash(runtimeType,agent);

@override
String toString() {
  return 'BridgeSessionEventKind.agentChanged(agent: $agent)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventKind_AgentChangedCopyWith<$Res> implements $BridgeSessionEventKindCopyWith<$Res> {
  factory $BridgeSessionEventKind_AgentChangedCopyWith(BridgeSessionEventKind_AgentChanged value, $Res Function(BridgeSessionEventKind_AgentChanged) _then) = _$BridgeSessionEventKind_AgentChangedCopyWithImpl;
@useResult
$Res call({
 BridgeSessionAgentSnapshot agent
});




}
/// @nodoc
class _$BridgeSessionEventKind_AgentChangedCopyWithImpl<$Res>
    implements $BridgeSessionEventKind_AgentChangedCopyWith<$Res> {
  _$BridgeSessionEventKind_AgentChangedCopyWithImpl(this._self, this._then);

  final BridgeSessionEventKind_AgentChanged _self;
  final $Res Function(BridgeSessionEventKind_AgentChanged) _then;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? agent = null,}) {
  return _then(BridgeSessionEventKind_AgentChanged(
agent: null == agent ? _self.agent : agent // ignore: cast_nullable_to_non_nullable
as BridgeSessionAgentSnapshot,
  ));
}


}

/// @nodoc


class BridgeSessionEventKind_TimelineEventAppended extends BridgeSessionEventKind {
  const BridgeSessionEventKind_TimelineEventAppended({required this.event}): super._();
  

 final  BridgeSessionTimelineEvent event;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventKind_TimelineEventAppendedCopyWith<BridgeSessionEventKind_TimelineEventAppended> get copyWith => _$BridgeSessionEventKind_TimelineEventAppendedCopyWithImpl<BridgeSessionEventKind_TimelineEventAppended>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind_TimelineEventAppended&&(identical(other.event, event) || other.event == event));
}


@override
int get hashCode => Object.hash(runtimeType,event);

@override
String toString() {
  return 'BridgeSessionEventKind.timelineEventAppended(event: $event)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventKind_TimelineEventAppendedCopyWith<$Res> implements $BridgeSessionEventKindCopyWith<$Res> {
  factory $BridgeSessionEventKind_TimelineEventAppendedCopyWith(BridgeSessionEventKind_TimelineEventAppended value, $Res Function(BridgeSessionEventKind_TimelineEventAppended) _then) = _$BridgeSessionEventKind_TimelineEventAppendedCopyWithImpl;
@useResult
$Res call({
 BridgeSessionTimelineEvent event
});




}
/// @nodoc
class _$BridgeSessionEventKind_TimelineEventAppendedCopyWithImpl<$Res>
    implements $BridgeSessionEventKind_TimelineEventAppendedCopyWith<$Res> {
  _$BridgeSessionEventKind_TimelineEventAppendedCopyWithImpl(this._self, this._then);

  final BridgeSessionEventKind_TimelineEventAppended _self;
  final $Res Function(BridgeSessionEventKind_TimelineEventAppended) _then;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? event = null,}) {
  return _then(BridgeSessionEventKind_TimelineEventAppended(
event: null == event ? _self.event : event // ignore: cast_nullable_to_non_nullable
as BridgeSessionTimelineEvent,
  ));
}


}

/// @nodoc


class BridgeSessionEventKind_RuntimeChanged extends BridgeSessionEventKind {
  const BridgeSessionEventKind_RuntimeChanged({required this.runtime}): super._();
  

 final  BridgeSessionRuntimeSnapshot runtime;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventKind_RuntimeChangedCopyWith<BridgeSessionEventKind_RuntimeChanged> get copyWith => _$BridgeSessionEventKind_RuntimeChangedCopyWithImpl<BridgeSessionEventKind_RuntimeChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind_RuntimeChanged&&(identical(other.runtime, runtime) || other.runtime == runtime));
}


@override
int get hashCode => Object.hash(runtimeType,runtime);

@override
String toString() {
  return 'BridgeSessionEventKind.runtimeChanged(runtime: $runtime)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventKind_RuntimeChangedCopyWith<$Res> implements $BridgeSessionEventKindCopyWith<$Res> {
  factory $BridgeSessionEventKind_RuntimeChangedCopyWith(BridgeSessionEventKind_RuntimeChanged value, $Res Function(BridgeSessionEventKind_RuntimeChanged) _then) = _$BridgeSessionEventKind_RuntimeChangedCopyWithImpl;
@useResult
$Res call({
 BridgeSessionRuntimeSnapshot runtime
});




}
/// @nodoc
class _$BridgeSessionEventKind_RuntimeChangedCopyWithImpl<$Res>
    implements $BridgeSessionEventKind_RuntimeChangedCopyWith<$Res> {
  _$BridgeSessionEventKind_RuntimeChangedCopyWithImpl(this._self, this._then);

  final BridgeSessionEventKind_RuntimeChanged _self;
  final $Res Function(BridgeSessionEventKind_RuntimeChanged) _then;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? runtime = null,}) {
  return _then(BridgeSessionEventKind_RuntimeChanged(
runtime: null == runtime ? _self.runtime : runtime // ignore: cast_nullable_to_non_nullable
as BridgeSessionRuntimeSnapshot,
  ));
}


}

/// @nodoc


class BridgeSessionEventKind_SkillActivated extends BridgeSessionEventKind {
  const BridgeSessionEventKind_SkillActivated({required this.activation}): super._();
  

 final  BridgeSkillActivation activation;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventKind_SkillActivatedCopyWith<BridgeSessionEventKind_SkillActivated> get copyWith => _$BridgeSessionEventKind_SkillActivatedCopyWithImpl<BridgeSessionEventKind_SkillActivated>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind_SkillActivated&&(identical(other.activation, activation) || other.activation == activation));
}


@override
int get hashCode => Object.hash(runtimeType,activation);

@override
String toString() {
  return 'BridgeSessionEventKind.skillActivated(activation: $activation)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventKind_SkillActivatedCopyWith<$Res> implements $BridgeSessionEventKindCopyWith<$Res> {
  factory $BridgeSessionEventKind_SkillActivatedCopyWith(BridgeSessionEventKind_SkillActivated value, $Res Function(BridgeSessionEventKind_SkillActivated) _then) = _$BridgeSessionEventKind_SkillActivatedCopyWithImpl;
@useResult
$Res call({
 BridgeSkillActivation activation
});




}
/// @nodoc
class _$BridgeSessionEventKind_SkillActivatedCopyWithImpl<$Res>
    implements $BridgeSessionEventKind_SkillActivatedCopyWith<$Res> {
  _$BridgeSessionEventKind_SkillActivatedCopyWithImpl(this._self, this._then);

  final BridgeSessionEventKind_SkillActivated _self;
  final $Res Function(BridgeSessionEventKind_SkillActivated) _then;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? activation = null,}) {
  return _then(BridgeSessionEventKind_SkillActivated(
activation: null == activation ? _self.activation : activation // ignore: cast_nullable_to_non_nullable
as BridgeSkillActivation,
  ));
}


}

/// @nodoc


class BridgeSessionEventKind_PlanChanged extends BridgeSessionEventKind {
  const BridgeSessionEventKind_PlanChanged({required this.event}): super._();
  

 final  BridgePlanLifecycleEvent event;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventKind_PlanChangedCopyWith<BridgeSessionEventKind_PlanChanged> get copyWith => _$BridgeSessionEventKind_PlanChangedCopyWithImpl<BridgeSessionEventKind_PlanChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind_PlanChanged&&(identical(other.event, event) || other.event == event));
}


@override
int get hashCode => Object.hash(runtimeType,event);

@override
String toString() {
  return 'BridgeSessionEventKind.planChanged(event: $event)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventKind_PlanChangedCopyWith<$Res> implements $BridgeSessionEventKindCopyWith<$Res> {
  factory $BridgeSessionEventKind_PlanChangedCopyWith(BridgeSessionEventKind_PlanChanged value, $Res Function(BridgeSessionEventKind_PlanChanged) _then) = _$BridgeSessionEventKind_PlanChangedCopyWithImpl;
@useResult
$Res call({
 BridgePlanLifecycleEvent event
});




}
/// @nodoc
class _$BridgeSessionEventKind_PlanChangedCopyWithImpl<$Res>
    implements $BridgeSessionEventKind_PlanChangedCopyWith<$Res> {
  _$BridgeSessionEventKind_PlanChangedCopyWithImpl(this._self, this._then);

  final BridgeSessionEventKind_PlanChanged _self;
  final $Res Function(BridgeSessionEventKind_PlanChanged) _then;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? event = null,}) {
  return _then(BridgeSessionEventKind_PlanChanged(
event: null == event ? _self.event : event // ignore: cast_nullable_to_non_nullable
as BridgePlanLifecycleEvent,
  ));
}


}

/// @nodoc


class BridgeSessionEventKind_ContextCompacted extends BridgeSessionEventKind {
  const BridgeSessionEventKind_ContextCompacted({required this.compaction}): super._();
  

 final  BridgeSessionContextCompaction compaction;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventKind_ContextCompactedCopyWith<BridgeSessionEventKind_ContextCompacted> get copyWith => _$BridgeSessionEventKind_ContextCompactedCopyWithImpl<BridgeSessionEventKind_ContextCompacted>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind_ContextCompacted&&(identical(other.compaction, compaction) || other.compaction == compaction));
}


@override
int get hashCode => Object.hash(runtimeType,compaction);

@override
String toString() {
  return 'BridgeSessionEventKind.contextCompacted(compaction: $compaction)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventKind_ContextCompactedCopyWith<$Res> implements $BridgeSessionEventKindCopyWith<$Res> {
  factory $BridgeSessionEventKind_ContextCompactedCopyWith(BridgeSessionEventKind_ContextCompacted value, $Res Function(BridgeSessionEventKind_ContextCompacted) _then) = _$BridgeSessionEventKind_ContextCompactedCopyWithImpl;
@useResult
$Res call({
 BridgeSessionContextCompaction compaction
});




}
/// @nodoc
class _$BridgeSessionEventKind_ContextCompactedCopyWithImpl<$Res>
    implements $BridgeSessionEventKind_ContextCompactedCopyWith<$Res> {
  _$BridgeSessionEventKind_ContextCompactedCopyWithImpl(this._self, this._then);

  final BridgeSessionEventKind_ContextCompacted _self;
  final $Res Function(BridgeSessionEventKind_ContextCompacted) _then;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? compaction = null,}) {
  return _then(BridgeSessionEventKind_ContextCompacted(
compaction: null == compaction ? _self.compaction : compaction // ignore: cast_nullable_to_non_nullable
as BridgeSessionContextCompaction,
  ));
}


}

/// @nodoc


class BridgeSessionEventKind_ErrorOccurred extends BridgeSessionEventKind {
  const BridgeSessionEventKind_ErrorOccurred({required this.message, required this.severity}): super._();
  

 final  String message;
 final  BridgeErrorSeverity severity;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventKind_ErrorOccurredCopyWith<BridgeSessionEventKind_ErrorOccurred> get copyWith => _$BridgeSessionEventKind_ErrorOccurredCopyWithImpl<BridgeSessionEventKind_ErrorOccurred>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventKind_ErrorOccurred&&(identical(other.message, message) || other.message == message)&&(identical(other.severity, severity) || other.severity == severity));
}


@override
int get hashCode => Object.hash(runtimeType,message,severity);

@override
String toString() {
  return 'BridgeSessionEventKind.errorOccurred(message: $message, severity: $severity)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventKind_ErrorOccurredCopyWith<$Res> implements $BridgeSessionEventKindCopyWith<$Res> {
  factory $BridgeSessionEventKind_ErrorOccurredCopyWith(BridgeSessionEventKind_ErrorOccurred value, $Res Function(BridgeSessionEventKind_ErrorOccurred) _then) = _$BridgeSessionEventKind_ErrorOccurredCopyWithImpl;
@useResult
$Res call({
 String message, BridgeErrorSeverity severity
});




}
/// @nodoc
class _$BridgeSessionEventKind_ErrorOccurredCopyWithImpl<$Res>
    implements $BridgeSessionEventKind_ErrorOccurredCopyWith<$Res> {
  _$BridgeSessionEventKind_ErrorOccurredCopyWithImpl(this._self, this._then);

  final BridgeSessionEventKind_ErrorOccurred _self;
  final $Res Function(BridgeSessionEventKind_ErrorOccurred) _then;

/// Create a copy of BridgeSessionEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = null,Object? severity = null,}) {
  return _then(BridgeSessionEventKind_ErrorOccurred(
message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String,severity: null == severity ? _self.severity : severity // ignore: cast_nullable_to_non_nullable
as BridgeErrorSeverity,
  ));
}


}

/// @nodoc
mixin _$BridgeSessionEventPosition {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventPosition);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeSessionEventPosition()';
}


}

/// @nodoc
class $BridgeSessionEventPositionCopyWith<$Res>  {
$BridgeSessionEventPositionCopyWith(BridgeSessionEventPosition _, $Res Function(BridgeSessionEventPosition) __);
}


/// Adds pattern-matching-related methods to [BridgeSessionEventPosition].
extension BridgeSessionEventPositionPatterns on BridgeSessionEventPosition {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeSessionEventPosition_Durable value)?  durable,TResult Function( BridgeSessionEventPosition_Transient value)?  transient,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeSessionEventPosition_Durable() when durable != null:
return durable(_that);case BridgeSessionEventPosition_Transient() when transient != null:
return transient(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeSessionEventPosition_Durable value)  durable,required TResult Function( BridgeSessionEventPosition_Transient value)  transient,}){
final _that = this;
switch (_that) {
case BridgeSessionEventPosition_Durable():
return durable(_that);case BridgeSessionEventPosition_Transient():
return transient(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeSessionEventPosition_Durable value)?  durable,TResult? Function( BridgeSessionEventPosition_Transient value)?  transient,}){
final _that = this;
switch (_that) {
case BridgeSessionEventPosition_Durable() when durable != null:
return durable(_that);case BridgeSessionEventPosition_Transient() when transient != null:
return transient(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BigInt sequence)?  durable,TResult Function( BigInt revision)?  transient,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeSessionEventPosition_Durable() when durable != null:
return durable(_that.sequence);case BridgeSessionEventPosition_Transient() when transient != null:
return transient(_that.revision);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BigInt sequence)  durable,required TResult Function( BigInt revision)  transient,}) {final _that = this;
switch (_that) {
case BridgeSessionEventPosition_Durable():
return durable(_that.sequence);case BridgeSessionEventPosition_Transient():
return transient(_that.revision);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BigInt sequence)?  durable,TResult? Function( BigInt revision)?  transient,}) {final _that = this;
switch (_that) {
case BridgeSessionEventPosition_Durable() when durable != null:
return durable(_that.sequence);case BridgeSessionEventPosition_Transient() when transient != null:
return transient(_that.revision);case _:
  return null;

}
}

}

/// @nodoc


class BridgeSessionEventPosition_Durable extends BridgeSessionEventPosition {
  const BridgeSessionEventPosition_Durable({required this.sequence}): super._();
  

 final  BigInt sequence;

/// Create a copy of BridgeSessionEventPosition
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventPosition_DurableCopyWith<BridgeSessionEventPosition_Durable> get copyWith => _$BridgeSessionEventPosition_DurableCopyWithImpl<BridgeSessionEventPosition_Durable>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventPosition_Durable&&(identical(other.sequence, sequence) || other.sequence == sequence));
}


@override
int get hashCode => Object.hash(runtimeType,sequence);

@override
String toString() {
  return 'BridgeSessionEventPosition.durable(sequence: $sequence)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventPosition_DurableCopyWith<$Res> implements $BridgeSessionEventPositionCopyWith<$Res> {
  factory $BridgeSessionEventPosition_DurableCopyWith(BridgeSessionEventPosition_Durable value, $Res Function(BridgeSessionEventPosition_Durable) _then) = _$BridgeSessionEventPosition_DurableCopyWithImpl;
@useResult
$Res call({
 BigInt sequence
});




}
/// @nodoc
class _$BridgeSessionEventPosition_DurableCopyWithImpl<$Res>
    implements $BridgeSessionEventPosition_DurableCopyWith<$Res> {
  _$BridgeSessionEventPosition_DurableCopyWithImpl(this._self, this._then);

  final BridgeSessionEventPosition_Durable _self;
  final $Res Function(BridgeSessionEventPosition_Durable) _then;

/// Create a copy of BridgeSessionEventPosition
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? sequence = null,}) {
  return _then(BridgeSessionEventPosition_Durable(
sequence: null == sequence ? _self.sequence : sequence // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

/// @nodoc


class BridgeSessionEventPosition_Transient extends BridgeSessionEventPosition {
  const BridgeSessionEventPosition_Transient({required this.revision}): super._();
  

 final  BigInt revision;

/// Create a copy of BridgeSessionEventPosition
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionEventPosition_TransientCopyWith<BridgeSessionEventPosition_Transient> get copyWith => _$BridgeSessionEventPosition_TransientCopyWithImpl<BridgeSessionEventPosition_Transient>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionEventPosition_Transient&&(identical(other.revision, revision) || other.revision == revision));
}


@override
int get hashCode => Object.hash(runtimeType,revision);

@override
String toString() {
  return 'BridgeSessionEventPosition.transient(revision: $revision)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionEventPosition_TransientCopyWith<$Res> implements $BridgeSessionEventPositionCopyWith<$Res> {
  factory $BridgeSessionEventPosition_TransientCopyWith(BridgeSessionEventPosition_Transient value, $Res Function(BridgeSessionEventPosition_Transient) _then) = _$BridgeSessionEventPosition_TransientCopyWithImpl;
@useResult
$Res call({
 BigInt revision
});




}
/// @nodoc
class _$BridgeSessionEventPosition_TransientCopyWithImpl<$Res>
    implements $BridgeSessionEventPosition_TransientCopyWith<$Res> {
  _$BridgeSessionEventPosition_TransientCopyWithImpl(this._self, this._then);

  final BridgeSessionEventPosition_Transient _self;
  final $Res Function(BridgeSessionEventPosition_Transient) _then;

/// Create a copy of BridgeSessionEventPosition
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? revision = null,}) {
  return _then(BridgeSessionEventPosition_Transient(
revision: null == revision ? _self.revision : revision // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

/// @nodoc
mixin _$BridgeSessionPartContent {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionPartContent);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeSessionPartContent()';
}


}

/// @nodoc
class $BridgeSessionPartContentCopyWith<$Res>  {
$BridgeSessionPartContentCopyWith(BridgeSessionPartContent _, $Res Function(BridgeSessionPartContent) __);
}


/// Adds pattern-matching-related methods to [BridgeSessionPartContent].
extension BridgeSessionPartContentPatterns on BridgeSessionPartContent {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeSessionPartContent_Text value)?  text,TResult Function( BridgeSessionPartContent_Reasoning value)?  reasoning,TResult Function( BridgeSessionPartContent_Tool value)?  tool,TResult Function( BridgeSessionPartContent_Agent value)?  agent,TResult Function( BridgeSessionPartContent_Turn value)?  turn,TResult Function( BridgeSessionPartContent_Inference value)?  inference,TResult Function( BridgeSessionPartContent_Plan value)?  plan,TResult Function( BridgeSessionPartContent_File value)?  file,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeSessionPartContent_Text() when text != null:
return text(_that);case BridgeSessionPartContent_Reasoning() when reasoning != null:
return reasoning(_that);case BridgeSessionPartContent_Tool() when tool != null:
return tool(_that);case BridgeSessionPartContent_Agent() when agent != null:
return agent(_that);case BridgeSessionPartContent_Turn() when turn != null:
return turn(_that);case BridgeSessionPartContent_Inference() when inference != null:
return inference(_that);case BridgeSessionPartContent_Plan() when plan != null:
return plan(_that);case BridgeSessionPartContent_File() when file != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeSessionPartContent_Text value)  text,required TResult Function( BridgeSessionPartContent_Reasoning value)  reasoning,required TResult Function( BridgeSessionPartContent_Tool value)  tool,required TResult Function( BridgeSessionPartContent_Agent value)  agent,required TResult Function( BridgeSessionPartContent_Turn value)  turn,required TResult Function( BridgeSessionPartContent_Inference value)  inference,required TResult Function( BridgeSessionPartContent_Plan value)  plan,required TResult Function( BridgeSessionPartContent_File value)  file,}){
final _that = this;
switch (_that) {
case BridgeSessionPartContent_Text():
return text(_that);case BridgeSessionPartContent_Reasoning():
return reasoning(_that);case BridgeSessionPartContent_Tool():
return tool(_that);case BridgeSessionPartContent_Agent():
return agent(_that);case BridgeSessionPartContent_Turn():
return turn(_that);case BridgeSessionPartContent_Inference():
return inference(_that);case BridgeSessionPartContent_Plan():
return plan(_that);case BridgeSessionPartContent_File():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeSessionPartContent_Text value)?  text,TResult? Function( BridgeSessionPartContent_Reasoning value)?  reasoning,TResult? Function( BridgeSessionPartContent_Tool value)?  tool,TResult? Function( BridgeSessionPartContent_Agent value)?  agent,TResult? Function( BridgeSessionPartContent_Turn value)?  turn,TResult? Function( BridgeSessionPartContent_Inference value)?  inference,TResult? Function( BridgeSessionPartContent_Plan value)?  plan,TResult? Function( BridgeSessionPartContent_File value)?  file,}){
final _that = this;
switch (_that) {
case BridgeSessionPartContent_Text() when text != null:
return text(_that);case BridgeSessionPartContent_Reasoning() when reasoning != null:
return reasoning(_that);case BridgeSessionPartContent_Tool() when tool != null:
return tool(_that);case BridgeSessionPartContent_Agent() when agent != null:
return agent(_that);case BridgeSessionPartContent_Turn() when turn != null:
return turn(_that);case BridgeSessionPartContent_Inference() when inference != null:
return inference(_that);case BridgeSessionPartContent_Plan() when plan != null:
return plan(_that);case BridgeSessionPartContent_File() when file != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeSessionTextChannel channel,  String text,  List<BridgeSessionAttachment> attachments)?  text,TResult Function( List<String> summary,  List<String> content)?  reasoning,TResult Function( BridgeSessionToolPart tool)?  tool,TResult Function( BridgeSessionAgentPart agent)?  agent,TResult Function()?  turn,TResult Function( String inferenceId,  String model)?  inference,TResult Function( String content)?  plan,TResult Function( String path,  String? mediaType)?  file,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeSessionPartContent_Text() when text != null:
return text(_that.channel,_that.text,_that.attachments);case BridgeSessionPartContent_Reasoning() when reasoning != null:
return reasoning(_that.summary,_that.content);case BridgeSessionPartContent_Tool() when tool != null:
return tool(_that.tool);case BridgeSessionPartContent_Agent() when agent != null:
return agent(_that.agent);case BridgeSessionPartContent_Turn() when turn != null:
return turn();case BridgeSessionPartContent_Inference() when inference != null:
return inference(_that.inferenceId,_that.model);case BridgeSessionPartContent_Plan() when plan != null:
return plan(_that.content);case BridgeSessionPartContent_File() when file != null:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeSessionTextChannel channel,  String text,  List<BridgeSessionAttachment> attachments)  text,required TResult Function( List<String> summary,  List<String> content)  reasoning,required TResult Function( BridgeSessionToolPart tool)  tool,required TResult Function( BridgeSessionAgentPart agent)  agent,required TResult Function()  turn,required TResult Function( String inferenceId,  String model)  inference,required TResult Function( String content)  plan,required TResult Function( String path,  String? mediaType)  file,}) {final _that = this;
switch (_that) {
case BridgeSessionPartContent_Text():
return text(_that.channel,_that.text,_that.attachments);case BridgeSessionPartContent_Reasoning():
return reasoning(_that.summary,_that.content);case BridgeSessionPartContent_Tool():
return tool(_that.tool);case BridgeSessionPartContent_Agent():
return agent(_that.agent);case BridgeSessionPartContent_Turn():
return turn();case BridgeSessionPartContent_Inference():
return inference(_that.inferenceId,_that.model);case BridgeSessionPartContent_Plan():
return plan(_that.content);case BridgeSessionPartContent_File():
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeSessionTextChannel channel,  String text,  List<BridgeSessionAttachment> attachments)?  text,TResult? Function( List<String> summary,  List<String> content)?  reasoning,TResult? Function( BridgeSessionToolPart tool)?  tool,TResult? Function( BridgeSessionAgentPart agent)?  agent,TResult? Function()?  turn,TResult? Function( String inferenceId,  String model)?  inference,TResult? Function( String content)?  plan,TResult? Function( String path,  String? mediaType)?  file,}) {final _that = this;
switch (_that) {
case BridgeSessionPartContent_Text() when text != null:
return text(_that.channel,_that.text,_that.attachments);case BridgeSessionPartContent_Reasoning() when reasoning != null:
return reasoning(_that.summary,_that.content);case BridgeSessionPartContent_Tool() when tool != null:
return tool(_that.tool);case BridgeSessionPartContent_Agent() when agent != null:
return agent(_that.agent);case BridgeSessionPartContent_Turn() when turn != null:
return turn();case BridgeSessionPartContent_Inference() when inference != null:
return inference(_that.inferenceId,_that.model);case BridgeSessionPartContent_Plan() when plan != null:
return plan(_that.content);case BridgeSessionPartContent_File() when file != null:
return file(_that.path,_that.mediaType);case _:
  return null;

}
}

}

/// @nodoc


class BridgeSessionPartContent_Text extends BridgeSessionPartContent {
  const BridgeSessionPartContent_Text({required this.channel, required this.text, required final  List<BridgeSessionAttachment> attachments}): _attachments = attachments,super._();
  

 final  BridgeSessionTextChannel channel;
 final  String text;
 final  List<BridgeSessionAttachment> _attachments;
 List<BridgeSessionAttachment> get attachments {
  if (_attachments is EqualUnmodifiableListView) return _attachments;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_attachments);
}


/// Create a copy of BridgeSessionPartContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionPartContent_TextCopyWith<BridgeSessionPartContent_Text> get copyWith => _$BridgeSessionPartContent_TextCopyWithImpl<BridgeSessionPartContent_Text>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionPartContent_Text&&(identical(other.channel, channel) || other.channel == channel)&&(identical(other.text, text) || other.text == text)&&const DeepCollectionEquality().equals(other._attachments, _attachments));
}


@override
int get hashCode => Object.hash(runtimeType,channel,text,const DeepCollectionEquality().hash(_attachments));

@override
String toString() {
  return 'BridgeSessionPartContent.text(channel: $channel, text: $text, attachments: $attachments)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionPartContent_TextCopyWith<$Res> implements $BridgeSessionPartContentCopyWith<$Res> {
  factory $BridgeSessionPartContent_TextCopyWith(BridgeSessionPartContent_Text value, $Res Function(BridgeSessionPartContent_Text) _then) = _$BridgeSessionPartContent_TextCopyWithImpl;
@useResult
$Res call({
 BridgeSessionTextChannel channel, String text, List<BridgeSessionAttachment> attachments
});




}
/// @nodoc
class _$BridgeSessionPartContent_TextCopyWithImpl<$Res>
    implements $BridgeSessionPartContent_TextCopyWith<$Res> {
  _$BridgeSessionPartContent_TextCopyWithImpl(this._self, this._then);

  final BridgeSessionPartContent_Text _self;
  final $Res Function(BridgeSessionPartContent_Text) _then;

/// Create a copy of BridgeSessionPartContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? channel = null,Object? text = null,Object? attachments = null,}) {
  return _then(BridgeSessionPartContent_Text(
channel: null == channel ? _self.channel : channel // ignore: cast_nullable_to_non_nullable
as BridgeSessionTextChannel,text: null == text ? _self.text : text // ignore: cast_nullable_to_non_nullable
as String,attachments: null == attachments ? _self._attachments : attachments // ignore: cast_nullable_to_non_nullable
as List<BridgeSessionAttachment>,
  ));
}


}

/// @nodoc


class BridgeSessionPartContent_Reasoning extends BridgeSessionPartContent {
  const BridgeSessionPartContent_Reasoning({required final  List<String> summary, required final  List<String> content}): _summary = summary,_content = content,super._();
  

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


/// Create a copy of BridgeSessionPartContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionPartContent_ReasoningCopyWith<BridgeSessionPartContent_Reasoning> get copyWith => _$BridgeSessionPartContent_ReasoningCopyWithImpl<BridgeSessionPartContent_Reasoning>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionPartContent_Reasoning&&const DeepCollectionEquality().equals(other._summary, _summary)&&const DeepCollectionEquality().equals(other._content, _content));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_summary),const DeepCollectionEquality().hash(_content));

@override
String toString() {
  return 'BridgeSessionPartContent.reasoning(summary: $summary, content: $content)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionPartContent_ReasoningCopyWith<$Res> implements $BridgeSessionPartContentCopyWith<$Res> {
  factory $BridgeSessionPartContent_ReasoningCopyWith(BridgeSessionPartContent_Reasoning value, $Res Function(BridgeSessionPartContent_Reasoning) _then) = _$BridgeSessionPartContent_ReasoningCopyWithImpl;
@useResult
$Res call({
 List<String> summary, List<String> content
});




}
/// @nodoc
class _$BridgeSessionPartContent_ReasoningCopyWithImpl<$Res>
    implements $BridgeSessionPartContent_ReasoningCopyWith<$Res> {
  _$BridgeSessionPartContent_ReasoningCopyWithImpl(this._self, this._then);

  final BridgeSessionPartContent_Reasoning _self;
  final $Res Function(BridgeSessionPartContent_Reasoning) _then;

/// Create a copy of BridgeSessionPartContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? summary = null,Object? content = null,}) {
  return _then(BridgeSessionPartContent_Reasoning(
summary: null == summary ? _self._summary : summary // ignore: cast_nullable_to_non_nullable
as List<String>,content: null == content ? _self._content : content // ignore: cast_nullable_to_non_nullable
as List<String>,
  ));
}


}

/// @nodoc


class BridgeSessionPartContent_Tool extends BridgeSessionPartContent {
  const BridgeSessionPartContent_Tool({required this.tool}): super._();
  

 final  BridgeSessionToolPart tool;

/// Create a copy of BridgeSessionPartContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionPartContent_ToolCopyWith<BridgeSessionPartContent_Tool> get copyWith => _$BridgeSessionPartContent_ToolCopyWithImpl<BridgeSessionPartContent_Tool>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionPartContent_Tool&&(identical(other.tool, tool) || other.tool == tool));
}


@override
int get hashCode => Object.hash(runtimeType,tool);

@override
String toString() {
  return 'BridgeSessionPartContent.tool(tool: $tool)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionPartContent_ToolCopyWith<$Res> implements $BridgeSessionPartContentCopyWith<$Res> {
  factory $BridgeSessionPartContent_ToolCopyWith(BridgeSessionPartContent_Tool value, $Res Function(BridgeSessionPartContent_Tool) _then) = _$BridgeSessionPartContent_ToolCopyWithImpl;
@useResult
$Res call({
 BridgeSessionToolPart tool
});




}
/// @nodoc
class _$BridgeSessionPartContent_ToolCopyWithImpl<$Res>
    implements $BridgeSessionPartContent_ToolCopyWith<$Res> {
  _$BridgeSessionPartContent_ToolCopyWithImpl(this._self, this._then);

  final BridgeSessionPartContent_Tool _self;
  final $Res Function(BridgeSessionPartContent_Tool) _then;

/// Create a copy of BridgeSessionPartContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? tool = null,}) {
  return _then(BridgeSessionPartContent_Tool(
tool: null == tool ? _self.tool : tool // ignore: cast_nullable_to_non_nullable
as BridgeSessionToolPart,
  ));
}


}

/// @nodoc


class BridgeSessionPartContent_Agent extends BridgeSessionPartContent {
  const BridgeSessionPartContent_Agent({required this.agent}): super._();
  

 final  BridgeSessionAgentPart agent;

/// Create a copy of BridgeSessionPartContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionPartContent_AgentCopyWith<BridgeSessionPartContent_Agent> get copyWith => _$BridgeSessionPartContent_AgentCopyWithImpl<BridgeSessionPartContent_Agent>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionPartContent_Agent&&(identical(other.agent, agent) || other.agent == agent));
}


@override
int get hashCode => Object.hash(runtimeType,agent);

@override
String toString() {
  return 'BridgeSessionPartContent.agent(agent: $agent)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionPartContent_AgentCopyWith<$Res> implements $BridgeSessionPartContentCopyWith<$Res> {
  factory $BridgeSessionPartContent_AgentCopyWith(BridgeSessionPartContent_Agent value, $Res Function(BridgeSessionPartContent_Agent) _then) = _$BridgeSessionPartContent_AgentCopyWithImpl;
@useResult
$Res call({
 BridgeSessionAgentPart agent
});




}
/// @nodoc
class _$BridgeSessionPartContent_AgentCopyWithImpl<$Res>
    implements $BridgeSessionPartContent_AgentCopyWith<$Res> {
  _$BridgeSessionPartContent_AgentCopyWithImpl(this._self, this._then);

  final BridgeSessionPartContent_Agent _self;
  final $Res Function(BridgeSessionPartContent_Agent) _then;

/// Create a copy of BridgeSessionPartContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? agent = null,}) {
  return _then(BridgeSessionPartContent_Agent(
agent: null == agent ? _self.agent : agent // ignore: cast_nullable_to_non_nullable
as BridgeSessionAgentPart,
  ));
}


}

/// @nodoc


class BridgeSessionPartContent_Turn extends BridgeSessionPartContent {
  const BridgeSessionPartContent_Turn(): super._();
  






@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionPartContent_Turn);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeSessionPartContent.turn()';
}


}




/// @nodoc


class BridgeSessionPartContent_Inference extends BridgeSessionPartContent {
  const BridgeSessionPartContent_Inference({required this.inferenceId, required this.model}): super._();
  

 final  String inferenceId;
 final  String model;

/// Create a copy of BridgeSessionPartContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionPartContent_InferenceCopyWith<BridgeSessionPartContent_Inference> get copyWith => _$BridgeSessionPartContent_InferenceCopyWithImpl<BridgeSessionPartContent_Inference>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionPartContent_Inference&&(identical(other.inferenceId, inferenceId) || other.inferenceId == inferenceId)&&(identical(other.model, model) || other.model == model));
}


@override
int get hashCode => Object.hash(runtimeType,inferenceId,model);

@override
String toString() {
  return 'BridgeSessionPartContent.inference(inferenceId: $inferenceId, model: $model)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionPartContent_InferenceCopyWith<$Res> implements $BridgeSessionPartContentCopyWith<$Res> {
  factory $BridgeSessionPartContent_InferenceCopyWith(BridgeSessionPartContent_Inference value, $Res Function(BridgeSessionPartContent_Inference) _then) = _$BridgeSessionPartContent_InferenceCopyWithImpl;
@useResult
$Res call({
 String inferenceId, String model
});




}
/// @nodoc
class _$BridgeSessionPartContent_InferenceCopyWithImpl<$Res>
    implements $BridgeSessionPartContent_InferenceCopyWith<$Res> {
  _$BridgeSessionPartContent_InferenceCopyWithImpl(this._self, this._then);

  final BridgeSessionPartContent_Inference _self;
  final $Res Function(BridgeSessionPartContent_Inference) _then;

/// Create a copy of BridgeSessionPartContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? inferenceId = null,Object? model = null,}) {
  return _then(BridgeSessionPartContent_Inference(
inferenceId: null == inferenceId ? _self.inferenceId : inferenceId // ignore: cast_nullable_to_non_nullable
as String,model: null == model ? _self.model : model // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeSessionPartContent_Plan extends BridgeSessionPartContent {
  const BridgeSessionPartContent_Plan({required this.content}): super._();
  

 final  String content;

/// Create a copy of BridgeSessionPartContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionPartContent_PlanCopyWith<BridgeSessionPartContent_Plan> get copyWith => _$BridgeSessionPartContent_PlanCopyWithImpl<BridgeSessionPartContent_Plan>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionPartContent_Plan&&(identical(other.content, content) || other.content == content));
}


@override
int get hashCode => Object.hash(runtimeType,content);

@override
String toString() {
  return 'BridgeSessionPartContent.plan(content: $content)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionPartContent_PlanCopyWith<$Res> implements $BridgeSessionPartContentCopyWith<$Res> {
  factory $BridgeSessionPartContent_PlanCopyWith(BridgeSessionPartContent_Plan value, $Res Function(BridgeSessionPartContent_Plan) _then) = _$BridgeSessionPartContent_PlanCopyWithImpl;
@useResult
$Res call({
 String content
});




}
/// @nodoc
class _$BridgeSessionPartContent_PlanCopyWithImpl<$Res>
    implements $BridgeSessionPartContent_PlanCopyWith<$Res> {
  _$BridgeSessionPartContent_PlanCopyWithImpl(this._self, this._then);

  final BridgeSessionPartContent_Plan _self;
  final $Res Function(BridgeSessionPartContent_Plan) _then;

/// Create a copy of BridgeSessionPartContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? content = null,}) {
  return _then(BridgeSessionPartContent_Plan(
content: null == content ? _self.content : content // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeSessionPartContent_File extends BridgeSessionPartContent {
  const BridgeSessionPartContent_File({required this.path, this.mediaType}): super._();
  

 final  String path;
 final  String? mediaType;

/// Create a copy of BridgeSessionPartContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionPartContent_FileCopyWith<BridgeSessionPartContent_File> get copyWith => _$BridgeSessionPartContent_FileCopyWithImpl<BridgeSessionPartContent_File>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionPartContent_File&&(identical(other.path, path) || other.path == path)&&(identical(other.mediaType, mediaType) || other.mediaType == mediaType));
}


@override
int get hashCode => Object.hash(runtimeType,path,mediaType);

@override
String toString() {
  return 'BridgeSessionPartContent.file(path: $path, mediaType: $mediaType)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionPartContent_FileCopyWith<$Res> implements $BridgeSessionPartContentCopyWith<$Res> {
  factory $BridgeSessionPartContent_FileCopyWith(BridgeSessionPartContent_File value, $Res Function(BridgeSessionPartContent_File) _then) = _$BridgeSessionPartContent_FileCopyWithImpl;
@useResult
$Res call({
 String path, String? mediaType
});




}
/// @nodoc
class _$BridgeSessionPartContent_FileCopyWithImpl<$Res>
    implements $BridgeSessionPartContent_FileCopyWith<$Res> {
  _$BridgeSessionPartContent_FileCopyWithImpl(this._self, this._then);

  final BridgeSessionPartContent_File _self;
  final $Res Function(BridgeSessionPartContent_File) _then;

/// Create a copy of BridgeSessionPartContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? path = null,Object? mediaType = freezed,}) {
  return _then(BridgeSessionPartContent_File(
path: null == path ? _self.path : path // ignore: cast_nullable_to_non_nullable
as String,mediaType: freezed == mediaType ? _self.mediaType : mediaType // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc
mixin _$BridgeSessionResyncReason {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionResyncReason);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeSessionResyncReason()';
}


}

/// @nodoc
class $BridgeSessionResyncReasonCopyWith<$Res>  {
$BridgeSessionResyncReasonCopyWith(BridgeSessionResyncReason _, $Res Function(BridgeSessionResyncReason) __);
}


/// Adds pattern-matching-related methods to [BridgeSessionResyncReason].
extension BridgeSessionResyncReasonPatterns on BridgeSessionResyncReason {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeSessionResyncReason_Lagged value)?  lagged,TResult Function( BridgeSessionResyncReason_CursorExpired value)?  cursorExpired,TResult Function( BridgeSessionResyncReason_ReplayLimitExceeded value)?  replayLimitExceeded,TResult Function( BridgeSessionResyncReason_RevisionGap value)?  revisionGap,TResult Function( BridgeSessionResyncReason_ProjectionInvariant value)?  projectionInvariant,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeSessionResyncReason_Lagged() when lagged != null:
return lagged(_that);case BridgeSessionResyncReason_CursorExpired() when cursorExpired != null:
return cursorExpired(_that);case BridgeSessionResyncReason_ReplayLimitExceeded() when replayLimitExceeded != null:
return replayLimitExceeded(_that);case BridgeSessionResyncReason_RevisionGap() when revisionGap != null:
return revisionGap(_that);case BridgeSessionResyncReason_ProjectionInvariant() when projectionInvariant != null:
return projectionInvariant(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeSessionResyncReason_Lagged value)  lagged,required TResult Function( BridgeSessionResyncReason_CursorExpired value)  cursorExpired,required TResult Function( BridgeSessionResyncReason_ReplayLimitExceeded value)  replayLimitExceeded,required TResult Function( BridgeSessionResyncReason_RevisionGap value)  revisionGap,required TResult Function( BridgeSessionResyncReason_ProjectionInvariant value)  projectionInvariant,}){
final _that = this;
switch (_that) {
case BridgeSessionResyncReason_Lagged():
return lagged(_that);case BridgeSessionResyncReason_CursorExpired():
return cursorExpired(_that);case BridgeSessionResyncReason_ReplayLimitExceeded():
return replayLimitExceeded(_that);case BridgeSessionResyncReason_RevisionGap():
return revisionGap(_that);case BridgeSessionResyncReason_ProjectionInvariant():
return projectionInvariant(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeSessionResyncReason_Lagged value)?  lagged,TResult? Function( BridgeSessionResyncReason_CursorExpired value)?  cursorExpired,TResult? Function( BridgeSessionResyncReason_ReplayLimitExceeded value)?  replayLimitExceeded,TResult? Function( BridgeSessionResyncReason_RevisionGap value)?  revisionGap,TResult? Function( BridgeSessionResyncReason_ProjectionInvariant value)?  projectionInvariant,}){
final _that = this;
switch (_that) {
case BridgeSessionResyncReason_Lagged() when lagged != null:
return lagged(_that);case BridgeSessionResyncReason_CursorExpired() when cursorExpired != null:
return cursorExpired(_that);case BridgeSessionResyncReason_ReplayLimitExceeded() when replayLimitExceeded != null:
return replayLimitExceeded(_that);case BridgeSessionResyncReason_RevisionGap() when revisionGap != null:
return revisionGap(_that);case BridgeSessionResyncReason_ProjectionInvariant() when projectionInvariant != null:
return projectionInvariant(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BigInt events)?  lagged,TResult Function( BigInt requested,  BigInt oldestAvailable)?  cursorExpired,TResult Function( BigInt available,  BigInt limit)?  replayLimitExceeded,TResult Function( String partId,  BigInt expected,  BigInt actual)?  revisionGap,TResult Function( String message)?  projectionInvariant,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeSessionResyncReason_Lagged() when lagged != null:
return lagged(_that.events);case BridgeSessionResyncReason_CursorExpired() when cursorExpired != null:
return cursorExpired(_that.requested,_that.oldestAvailable);case BridgeSessionResyncReason_ReplayLimitExceeded() when replayLimitExceeded != null:
return replayLimitExceeded(_that.available,_that.limit);case BridgeSessionResyncReason_RevisionGap() when revisionGap != null:
return revisionGap(_that.partId,_that.expected,_that.actual);case BridgeSessionResyncReason_ProjectionInvariant() when projectionInvariant != null:
return projectionInvariant(_that.message);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BigInt events)  lagged,required TResult Function( BigInt requested,  BigInt oldestAvailable)  cursorExpired,required TResult Function( BigInt available,  BigInt limit)  replayLimitExceeded,required TResult Function( String partId,  BigInt expected,  BigInt actual)  revisionGap,required TResult Function( String message)  projectionInvariant,}) {final _that = this;
switch (_that) {
case BridgeSessionResyncReason_Lagged():
return lagged(_that.events);case BridgeSessionResyncReason_CursorExpired():
return cursorExpired(_that.requested,_that.oldestAvailable);case BridgeSessionResyncReason_ReplayLimitExceeded():
return replayLimitExceeded(_that.available,_that.limit);case BridgeSessionResyncReason_RevisionGap():
return revisionGap(_that.partId,_that.expected,_that.actual);case BridgeSessionResyncReason_ProjectionInvariant():
return projectionInvariant(_that.message);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BigInt events)?  lagged,TResult? Function( BigInt requested,  BigInt oldestAvailable)?  cursorExpired,TResult? Function( BigInt available,  BigInt limit)?  replayLimitExceeded,TResult? Function( String partId,  BigInt expected,  BigInt actual)?  revisionGap,TResult? Function( String message)?  projectionInvariant,}) {final _that = this;
switch (_that) {
case BridgeSessionResyncReason_Lagged() when lagged != null:
return lagged(_that.events);case BridgeSessionResyncReason_CursorExpired() when cursorExpired != null:
return cursorExpired(_that.requested,_that.oldestAvailable);case BridgeSessionResyncReason_ReplayLimitExceeded() when replayLimitExceeded != null:
return replayLimitExceeded(_that.available,_that.limit);case BridgeSessionResyncReason_RevisionGap() when revisionGap != null:
return revisionGap(_that.partId,_that.expected,_that.actual);case BridgeSessionResyncReason_ProjectionInvariant() when projectionInvariant != null:
return projectionInvariant(_that.message);case _:
  return null;

}
}

}

/// @nodoc


class BridgeSessionResyncReason_Lagged extends BridgeSessionResyncReason {
  const BridgeSessionResyncReason_Lagged({required this.events}): super._();
  

 final  BigInt events;

/// Create a copy of BridgeSessionResyncReason
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionResyncReason_LaggedCopyWith<BridgeSessionResyncReason_Lagged> get copyWith => _$BridgeSessionResyncReason_LaggedCopyWithImpl<BridgeSessionResyncReason_Lagged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionResyncReason_Lagged&&(identical(other.events, events) || other.events == events));
}


@override
int get hashCode => Object.hash(runtimeType,events);

@override
String toString() {
  return 'BridgeSessionResyncReason.lagged(events: $events)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionResyncReason_LaggedCopyWith<$Res> implements $BridgeSessionResyncReasonCopyWith<$Res> {
  factory $BridgeSessionResyncReason_LaggedCopyWith(BridgeSessionResyncReason_Lagged value, $Res Function(BridgeSessionResyncReason_Lagged) _then) = _$BridgeSessionResyncReason_LaggedCopyWithImpl;
@useResult
$Res call({
 BigInt events
});




}
/// @nodoc
class _$BridgeSessionResyncReason_LaggedCopyWithImpl<$Res>
    implements $BridgeSessionResyncReason_LaggedCopyWith<$Res> {
  _$BridgeSessionResyncReason_LaggedCopyWithImpl(this._self, this._then);

  final BridgeSessionResyncReason_Lagged _self;
  final $Res Function(BridgeSessionResyncReason_Lagged) _then;

/// Create a copy of BridgeSessionResyncReason
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? events = null,}) {
  return _then(BridgeSessionResyncReason_Lagged(
events: null == events ? _self.events : events // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

/// @nodoc


class BridgeSessionResyncReason_CursorExpired extends BridgeSessionResyncReason {
  const BridgeSessionResyncReason_CursorExpired({required this.requested, required this.oldestAvailable}): super._();
  

 final  BigInt requested;
 final  BigInt oldestAvailable;

/// Create a copy of BridgeSessionResyncReason
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionResyncReason_CursorExpiredCopyWith<BridgeSessionResyncReason_CursorExpired> get copyWith => _$BridgeSessionResyncReason_CursorExpiredCopyWithImpl<BridgeSessionResyncReason_CursorExpired>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionResyncReason_CursorExpired&&(identical(other.requested, requested) || other.requested == requested)&&(identical(other.oldestAvailable, oldestAvailable) || other.oldestAvailable == oldestAvailable));
}


@override
int get hashCode => Object.hash(runtimeType,requested,oldestAvailable);

@override
String toString() {
  return 'BridgeSessionResyncReason.cursorExpired(requested: $requested, oldestAvailable: $oldestAvailable)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionResyncReason_CursorExpiredCopyWith<$Res> implements $BridgeSessionResyncReasonCopyWith<$Res> {
  factory $BridgeSessionResyncReason_CursorExpiredCopyWith(BridgeSessionResyncReason_CursorExpired value, $Res Function(BridgeSessionResyncReason_CursorExpired) _then) = _$BridgeSessionResyncReason_CursorExpiredCopyWithImpl;
@useResult
$Res call({
 BigInt requested, BigInt oldestAvailable
});




}
/// @nodoc
class _$BridgeSessionResyncReason_CursorExpiredCopyWithImpl<$Res>
    implements $BridgeSessionResyncReason_CursorExpiredCopyWith<$Res> {
  _$BridgeSessionResyncReason_CursorExpiredCopyWithImpl(this._self, this._then);

  final BridgeSessionResyncReason_CursorExpired _self;
  final $Res Function(BridgeSessionResyncReason_CursorExpired) _then;

/// Create a copy of BridgeSessionResyncReason
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? requested = null,Object? oldestAvailable = null,}) {
  return _then(BridgeSessionResyncReason_CursorExpired(
requested: null == requested ? _self.requested : requested // ignore: cast_nullable_to_non_nullable
as BigInt,oldestAvailable: null == oldestAvailable ? _self.oldestAvailable : oldestAvailable // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

/// @nodoc


class BridgeSessionResyncReason_ReplayLimitExceeded extends BridgeSessionResyncReason {
  const BridgeSessionResyncReason_ReplayLimitExceeded({required this.available, required this.limit}): super._();
  

 final  BigInt available;
 final  BigInt limit;

/// Create a copy of BridgeSessionResyncReason
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionResyncReason_ReplayLimitExceededCopyWith<BridgeSessionResyncReason_ReplayLimitExceeded> get copyWith => _$BridgeSessionResyncReason_ReplayLimitExceededCopyWithImpl<BridgeSessionResyncReason_ReplayLimitExceeded>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionResyncReason_ReplayLimitExceeded&&(identical(other.available, available) || other.available == available)&&(identical(other.limit, limit) || other.limit == limit));
}


@override
int get hashCode => Object.hash(runtimeType,available,limit);

@override
String toString() {
  return 'BridgeSessionResyncReason.replayLimitExceeded(available: $available, limit: $limit)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionResyncReason_ReplayLimitExceededCopyWith<$Res> implements $BridgeSessionResyncReasonCopyWith<$Res> {
  factory $BridgeSessionResyncReason_ReplayLimitExceededCopyWith(BridgeSessionResyncReason_ReplayLimitExceeded value, $Res Function(BridgeSessionResyncReason_ReplayLimitExceeded) _then) = _$BridgeSessionResyncReason_ReplayLimitExceededCopyWithImpl;
@useResult
$Res call({
 BigInt available, BigInt limit
});




}
/// @nodoc
class _$BridgeSessionResyncReason_ReplayLimitExceededCopyWithImpl<$Res>
    implements $BridgeSessionResyncReason_ReplayLimitExceededCopyWith<$Res> {
  _$BridgeSessionResyncReason_ReplayLimitExceededCopyWithImpl(this._self, this._then);

  final BridgeSessionResyncReason_ReplayLimitExceeded _self;
  final $Res Function(BridgeSessionResyncReason_ReplayLimitExceeded) _then;

/// Create a copy of BridgeSessionResyncReason
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? available = null,Object? limit = null,}) {
  return _then(BridgeSessionResyncReason_ReplayLimitExceeded(
available: null == available ? _self.available : available // ignore: cast_nullable_to_non_nullable
as BigInt,limit: null == limit ? _self.limit : limit // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

/// @nodoc


class BridgeSessionResyncReason_RevisionGap extends BridgeSessionResyncReason {
  const BridgeSessionResyncReason_RevisionGap({required this.partId, required this.expected, required this.actual}): super._();
  

 final  String partId;
 final  BigInt expected;
 final  BigInt actual;

/// Create a copy of BridgeSessionResyncReason
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionResyncReason_RevisionGapCopyWith<BridgeSessionResyncReason_RevisionGap> get copyWith => _$BridgeSessionResyncReason_RevisionGapCopyWithImpl<BridgeSessionResyncReason_RevisionGap>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionResyncReason_RevisionGap&&(identical(other.partId, partId) || other.partId == partId)&&(identical(other.expected, expected) || other.expected == expected)&&(identical(other.actual, actual) || other.actual == actual));
}


@override
int get hashCode => Object.hash(runtimeType,partId,expected,actual);

@override
String toString() {
  return 'BridgeSessionResyncReason.revisionGap(partId: $partId, expected: $expected, actual: $actual)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionResyncReason_RevisionGapCopyWith<$Res> implements $BridgeSessionResyncReasonCopyWith<$Res> {
  factory $BridgeSessionResyncReason_RevisionGapCopyWith(BridgeSessionResyncReason_RevisionGap value, $Res Function(BridgeSessionResyncReason_RevisionGap) _then) = _$BridgeSessionResyncReason_RevisionGapCopyWithImpl;
@useResult
$Res call({
 String partId, BigInt expected, BigInt actual
});




}
/// @nodoc
class _$BridgeSessionResyncReason_RevisionGapCopyWithImpl<$Res>
    implements $BridgeSessionResyncReason_RevisionGapCopyWith<$Res> {
  _$BridgeSessionResyncReason_RevisionGapCopyWithImpl(this._self, this._then);

  final BridgeSessionResyncReason_RevisionGap _self;
  final $Res Function(BridgeSessionResyncReason_RevisionGap) _then;

/// Create a copy of BridgeSessionResyncReason
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? partId = null,Object? expected = null,Object? actual = null,}) {
  return _then(BridgeSessionResyncReason_RevisionGap(
partId: null == partId ? _self.partId : partId // ignore: cast_nullable_to_non_nullable
as String,expected: null == expected ? _self.expected : expected // ignore: cast_nullable_to_non_nullable
as BigInt,actual: null == actual ? _self.actual : actual // ignore: cast_nullable_to_non_nullable
as BigInt,
  ));
}


}

/// @nodoc


class BridgeSessionResyncReason_ProjectionInvariant extends BridgeSessionResyncReason {
  const BridgeSessionResyncReason_ProjectionInvariant({required this.message}): super._();
  

 final  String message;

/// Create a copy of BridgeSessionResyncReason
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionResyncReason_ProjectionInvariantCopyWith<BridgeSessionResyncReason_ProjectionInvariant> get copyWith => _$BridgeSessionResyncReason_ProjectionInvariantCopyWithImpl<BridgeSessionResyncReason_ProjectionInvariant>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionResyncReason_ProjectionInvariant&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode => Object.hash(runtimeType,message);

@override
String toString() {
  return 'BridgeSessionResyncReason.projectionInvariant(message: $message)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionResyncReason_ProjectionInvariantCopyWith<$Res> implements $BridgeSessionResyncReasonCopyWith<$Res> {
  factory $BridgeSessionResyncReason_ProjectionInvariantCopyWith(BridgeSessionResyncReason_ProjectionInvariant value, $Res Function(BridgeSessionResyncReason_ProjectionInvariant) _then) = _$BridgeSessionResyncReason_ProjectionInvariantCopyWithImpl;
@useResult
$Res call({
 String message
});




}
/// @nodoc
class _$BridgeSessionResyncReason_ProjectionInvariantCopyWithImpl<$Res>
    implements $BridgeSessionResyncReason_ProjectionInvariantCopyWith<$Res> {
  _$BridgeSessionResyncReason_ProjectionInvariantCopyWithImpl(this._self, this._then);

  final BridgeSessionResyncReason_ProjectionInvariant _self;
  final $Res Function(BridgeSessionResyncReason_ProjectionInvariant) _then;

/// Create a copy of BridgeSessionResyncReason
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = null,}) {
  return _then(BridgeSessionResyncReason_ProjectionInvariant(
message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeSessionStreamFrame {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionStreamFrame);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeSessionStreamFrame()';
}


}

/// @nodoc
class $BridgeSessionStreamFrameCopyWith<$Res>  {
$BridgeSessionStreamFrameCopyWith(BridgeSessionStreamFrame _, $Res Function(BridgeSessionStreamFrame) __);
}


/// Adds pattern-matching-related methods to [BridgeSessionStreamFrame].
extension BridgeSessionStreamFramePatterns on BridgeSessionStreamFrame {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeSessionStreamFrame_Snapshot value)?  snapshot,TResult Function( BridgeSessionStreamFrame_Event value)?  event,TResult Function( BridgeSessionStreamFrame_ResyncRequired value)?  resyncRequired,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeSessionStreamFrame_Snapshot() when snapshot != null:
return snapshot(_that);case BridgeSessionStreamFrame_Event() when event != null:
return event(_that);case BridgeSessionStreamFrame_ResyncRequired() when resyncRequired != null:
return resyncRequired(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeSessionStreamFrame_Snapshot value)  snapshot,required TResult Function( BridgeSessionStreamFrame_Event value)  event,required TResult Function( BridgeSessionStreamFrame_ResyncRequired value)  resyncRequired,}){
final _that = this;
switch (_that) {
case BridgeSessionStreamFrame_Snapshot():
return snapshot(_that);case BridgeSessionStreamFrame_Event():
return event(_that);case BridgeSessionStreamFrame_ResyncRequired():
return resyncRequired(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeSessionStreamFrame_Snapshot value)?  snapshot,TResult? Function( BridgeSessionStreamFrame_Event value)?  event,TResult? Function( BridgeSessionStreamFrame_ResyncRequired value)?  resyncRequired,}){
final _that = this;
switch (_that) {
case BridgeSessionStreamFrame_Snapshot() when snapshot != null:
return snapshot(_that);case BridgeSessionStreamFrame_Event() when event != null:
return event(_that);case BridgeSessionStreamFrame_ResyncRequired() when resyncRequired != null:
return resyncRequired(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeSessionViewSnapshot snapshot)?  snapshot,TResult Function( BridgeSessionEventEnvelope event)?  event,TResult Function( BridgeSessionResyncReason reason)?  resyncRequired,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeSessionStreamFrame_Snapshot() when snapshot != null:
return snapshot(_that.snapshot);case BridgeSessionStreamFrame_Event() when event != null:
return event(_that.event);case BridgeSessionStreamFrame_ResyncRequired() when resyncRequired != null:
return resyncRequired(_that.reason);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeSessionViewSnapshot snapshot)  snapshot,required TResult Function( BridgeSessionEventEnvelope event)  event,required TResult Function( BridgeSessionResyncReason reason)  resyncRequired,}) {final _that = this;
switch (_that) {
case BridgeSessionStreamFrame_Snapshot():
return snapshot(_that.snapshot);case BridgeSessionStreamFrame_Event():
return event(_that.event);case BridgeSessionStreamFrame_ResyncRequired():
return resyncRequired(_that.reason);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeSessionViewSnapshot snapshot)?  snapshot,TResult? Function( BridgeSessionEventEnvelope event)?  event,TResult? Function( BridgeSessionResyncReason reason)?  resyncRequired,}) {final _that = this;
switch (_that) {
case BridgeSessionStreamFrame_Snapshot() when snapshot != null:
return snapshot(_that.snapshot);case BridgeSessionStreamFrame_Event() when event != null:
return event(_that.event);case BridgeSessionStreamFrame_ResyncRequired() when resyncRequired != null:
return resyncRequired(_that.reason);case _:
  return null;

}
}

}

/// @nodoc


class BridgeSessionStreamFrame_Snapshot extends BridgeSessionStreamFrame {
  const BridgeSessionStreamFrame_Snapshot({required this.snapshot}): super._();
  

 final  BridgeSessionViewSnapshot snapshot;

/// Create a copy of BridgeSessionStreamFrame
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionStreamFrame_SnapshotCopyWith<BridgeSessionStreamFrame_Snapshot> get copyWith => _$BridgeSessionStreamFrame_SnapshotCopyWithImpl<BridgeSessionStreamFrame_Snapshot>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionStreamFrame_Snapshot&&(identical(other.snapshot, snapshot) || other.snapshot == snapshot));
}


@override
int get hashCode => Object.hash(runtimeType,snapshot);

@override
String toString() {
  return 'BridgeSessionStreamFrame.snapshot(snapshot: $snapshot)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionStreamFrame_SnapshotCopyWith<$Res> implements $BridgeSessionStreamFrameCopyWith<$Res> {
  factory $BridgeSessionStreamFrame_SnapshotCopyWith(BridgeSessionStreamFrame_Snapshot value, $Res Function(BridgeSessionStreamFrame_Snapshot) _then) = _$BridgeSessionStreamFrame_SnapshotCopyWithImpl;
@useResult
$Res call({
 BridgeSessionViewSnapshot snapshot
});




}
/// @nodoc
class _$BridgeSessionStreamFrame_SnapshotCopyWithImpl<$Res>
    implements $BridgeSessionStreamFrame_SnapshotCopyWith<$Res> {
  _$BridgeSessionStreamFrame_SnapshotCopyWithImpl(this._self, this._then);

  final BridgeSessionStreamFrame_Snapshot _self;
  final $Res Function(BridgeSessionStreamFrame_Snapshot) _then;

/// Create a copy of BridgeSessionStreamFrame
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? snapshot = null,}) {
  return _then(BridgeSessionStreamFrame_Snapshot(
snapshot: null == snapshot ? _self.snapshot : snapshot // ignore: cast_nullable_to_non_nullable
as BridgeSessionViewSnapshot,
  ));
}


}

/// @nodoc


class BridgeSessionStreamFrame_Event extends BridgeSessionStreamFrame {
  const BridgeSessionStreamFrame_Event({required this.event}): super._();
  

 final  BridgeSessionEventEnvelope event;

/// Create a copy of BridgeSessionStreamFrame
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionStreamFrame_EventCopyWith<BridgeSessionStreamFrame_Event> get copyWith => _$BridgeSessionStreamFrame_EventCopyWithImpl<BridgeSessionStreamFrame_Event>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionStreamFrame_Event&&(identical(other.event, event) || other.event == event));
}


@override
int get hashCode => Object.hash(runtimeType,event);

@override
String toString() {
  return 'BridgeSessionStreamFrame.event(event: $event)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionStreamFrame_EventCopyWith<$Res> implements $BridgeSessionStreamFrameCopyWith<$Res> {
  factory $BridgeSessionStreamFrame_EventCopyWith(BridgeSessionStreamFrame_Event value, $Res Function(BridgeSessionStreamFrame_Event) _then) = _$BridgeSessionStreamFrame_EventCopyWithImpl;
@useResult
$Res call({
 BridgeSessionEventEnvelope event
});




}
/// @nodoc
class _$BridgeSessionStreamFrame_EventCopyWithImpl<$Res>
    implements $BridgeSessionStreamFrame_EventCopyWith<$Res> {
  _$BridgeSessionStreamFrame_EventCopyWithImpl(this._self, this._then);

  final BridgeSessionStreamFrame_Event _self;
  final $Res Function(BridgeSessionStreamFrame_Event) _then;

/// Create a copy of BridgeSessionStreamFrame
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? event = null,}) {
  return _then(BridgeSessionStreamFrame_Event(
event: null == event ? _self.event : event // ignore: cast_nullable_to_non_nullable
as BridgeSessionEventEnvelope,
  ));
}


}

/// @nodoc


class BridgeSessionStreamFrame_ResyncRequired extends BridgeSessionStreamFrame {
  const BridgeSessionStreamFrame_ResyncRequired({required this.reason}): super._();
  

 final  BridgeSessionResyncReason reason;

/// Create a copy of BridgeSessionStreamFrame
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionStreamFrame_ResyncRequiredCopyWith<BridgeSessionStreamFrame_ResyncRequired> get copyWith => _$BridgeSessionStreamFrame_ResyncRequiredCopyWithImpl<BridgeSessionStreamFrame_ResyncRequired>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionStreamFrame_ResyncRequired&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,reason);

@override
String toString() {
  return 'BridgeSessionStreamFrame.resyncRequired(reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionStreamFrame_ResyncRequiredCopyWith<$Res> implements $BridgeSessionStreamFrameCopyWith<$Res> {
  factory $BridgeSessionStreamFrame_ResyncRequiredCopyWith(BridgeSessionStreamFrame_ResyncRequired value, $Res Function(BridgeSessionStreamFrame_ResyncRequired) _then) = _$BridgeSessionStreamFrame_ResyncRequiredCopyWithImpl;
@useResult
$Res call({
 BridgeSessionResyncReason reason
});


$BridgeSessionResyncReasonCopyWith<$Res> get reason;

}
/// @nodoc
class _$BridgeSessionStreamFrame_ResyncRequiredCopyWithImpl<$Res>
    implements $BridgeSessionStreamFrame_ResyncRequiredCopyWith<$Res> {
  _$BridgeSessionStreamFrame_ResyncRequiredCopyWithImpl(this._self, this._then);

  final BridgeSessionStreamFrame_ResyncRequired _self;
  final $Res Function(BridgeSessionStreamFrame_ResyncRequired) _then;

/// Create a copy of BridgeSessionStreamFrame
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reason = null,}) {
  return _then(BridgeSessionStreamFrame_ResyncRequired(
reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as BridgeSessionResyncReason,
  ));
}

/// Create a copy of BridgeSessionStreamFrame
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeSessionResyncReasonCopyWith<$Res> get reason {
  
  return $BridgeSessionResyncReasonCopyWith<$Res>(_self.reason, (value) {
    return _then(_self.copyWith(reason: value));
  });
}
}

/// @nodoc
mixin _$BridgeSessionTimelineEventKind {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionTimelineEventKind);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeSessionTimelineEventKind()';
}


}

/// @nodoc
class $BridgeSessionTimelineEventKindCopyWith<$Res>  {
$BridgeSessionTimelineEventKindCopyWith(BridgeSessionTimelineEventKind _, $Res Function(BridgeSessionTimelineEventKind) __);
}


/// Adds pattern-matching-related methods to [BridgeSessionTimelineEventKind].
extension BridgeSessionTimelineEventKindPatterns on BridgeSessionTimelineEventKind {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeSessionTimelineEventKind_SubAgentActivity value)?  subAgentActivity,TResult Function( BridgeSessionTimelineEventKind_TodoListChanged value)?  todoListChanged,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeSessionTimelineEventKind_SubAgentActivity() when subAgentActivity != null:
return subAgentActivity(_that);case BridgeSessionTimelineEventKind_TodoListChanged() when todoListChanged != null:
return todoListChanged(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeSessionTimelineEventKind_SubAgentActivity value)  subAgentActivity,required TResult Function( BridgeSessionTimelineEventKind_TodoListChanged value)  todoListChanged,}){
final _that = this;
switch (_that) {
case BridgeSessionTimelineEventKind_SubAgentActivity():
return subAgentActivity(_that);case BridgeSessionTimelineEventKind_TodoListChanged():
return todoListChanged(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeSessionTimelineEventKind_SubAgentActivity value)?  subAgentActivity,TResult? Function( BridgeSessionTimelineEventKind_TodoListChanged value)?  todoListChanged,}){
final _that = this;
switch (_that) {
case BridgeSessionTimelineEventKind_SubAgentActivity() when subAgentActivity != null:
return subAgentActivity(_that);case BridgeSessionTimelineEventKind_TodoListChanged() when todoListChanged != null:
return todoListChanged(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String callId,  String? agentId,  String? path,  String? parentPath,  BridgeSubAgentActivityKind kind,  BridgeAgentStatus? status,  String? message,  bool? timedOut,  String? error)?  subAgentActivity,TResult Function( BridgeTodoListSnapshot snapshot)?  todoListChanged,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeSessionTimelineEventKind_SubAgentActivity() when subAgentActivity != null:
return subAgentActivity(_that.callId,_that.agentId,_that.path,_that.parentPath,_that.kind,_that.status,_that.message,_that.timedOut,_that.error);case BridgeSessionTimelineEventKind_TodoListChanged() when todoListChanged != null:
return todoListChanged(_that.snapshot);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String callId,  String? agentId,  String? path,  String? parentPath,  BridgeSubAgentActivityKind kind,  BridgeAgentStatus? status,  String? message,  bool? timedOut,  String? error)  subAgentActivity,required TResult Function( BridgeTodoListSnapshot snapshot)  todoListChanged,}) {final _that = this;
switch (_that) {
case BridgeSessionTimelineEventKind_SubAgentActivity():
return subAgentActivity(_that.callId,_that.agentId,_that.path,_that.parentPath,_that.kind,_that.status,_that.message,_that.timedOut,_that.error);case BridgeSessionTimelineEventKind_TodoListChanged():
return todoListChanged(_that.snapshot);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String callId,  String? agentId,  String? path,  String? parentPath,  BridgeSubAgentActivityKind kind,  BridgeAgentStatus? status,  String? message,  bool? timedOut,  String? error)?  subAgentActivity,TResult? Function( BridgeTodoListSnapshot snapshot)?  todoListChanged,}) {final _that = this;
switch (_that) {
case BridgeSessionTimelineEventKind_SubAgentActivity() when subAgentActivity != null:
return subAgentActivity(_that.callId,_that.agentId,_that.path,_that.parentPath,_that.kind,_that.status,_that.message,_that.timedOut,_that.error);case BridgeSessionTimelineEventKind_TodoListChanged() when todoListChanged != null:
return todoListChanged(_that.snapshot);case _:
  return null;

}
}

}

/// @nodoc


class BridgeSessionTimelineEventKind_SubAgentActivity extends BridgeSessionTimelineEventKind {
  const BridgeSessionTimelineEventKind_SubAgentActivity({required this.callId, this.agentId, this.path, this.parentPath, required this.kind, this.status, this.message, this.timedOut, this.error}): super._();
  

 final  String callId;
 final  String? agentId;
 final  String? path;
 final  String? parentPath;
 final  BridgeSubAgentActivityKind kind;
 final  BridgeAgentStatus? status;
 final  String? message;
 final  bool? timedOut;
 final  String? error;

/// Create a copy of BridgeSessionTimelineEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionTimelineEventKind_SubAgentActivityCopyWith<BridgeSessionTimelineEventKind_SubAgentActivity> get copyWith => _$BridgeSessionTimelineEventKind_SubAgentActivityCopyWithImpl<BridgeSessionTimelineEventKind_SubAgentActivity>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionTimelineEventKind_SubAgentActivity&&(identical(other.callId, callId) || other.callId == callId)&&(identical(other.agentId, agentId) || other.agentId == agentId)&&(identical(other.path, path) || other.path == path)&&(identical(other.parentPath, parentPath) || other.parentPath == parentPath)&&(identical(other.kind, kind) || other.kind == kind)&&(identical(other.status, status) || other.status == status)&&(identical(other.message, message) || other.message == message)&&(identical(other.timedOut, timedOut) || other.timedOut == timedOut)&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode => Object.hash(runtimeType,callId,agentId,path,parentPath,kind,status,message,timedOut,error);

@override
String toString() {
  return 'BridgeSessionTimelineEventKind.subAgentActivity(callId: $callId, agentId: $agentId, path: $path, parentPath: $parentPath, kind: $kind, status: $status, message: $message, timedOut: $timedOut, error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionTimelineEventKind_SubAgentActivityCopyWith<$Res> implements $BridgeSessionTimelineEventKindCopyWith<$Res> {
  factory $BridgeSessionTimelineEventKind_SubAgentActivityCopyWith(BridgeSessionTimelineEventKind_SubAgentActivity value, $Res Function(BridgeSessionTimelineEventKind_SubAgentActivity) _then) = _$BridgeSessionTimelineEventKind_SubAgentActivityCopyWithImpl;
@useResult
$Res call({
 String callId, String? agentId, String? path, String? parentPath, BridgeSubAgentActivityKind kind, BridgeAgentStatus? status, String? message, bool? timedOut, String? error
});




}
/// @nodoc
class _$BridgeSessionTimelineEventKind_SubAgentActivityCopyWithImpl<$Res>
    implements $BridgeSessionTimelineEventKind_SubAgentActivityCopyWith<$Res> {
  _$BridgeSessionTimelineEventKind_SubAgentActivityCopyWithImpl(this._self, this._then);

  final BridgeSessionTimelineEventKind_SubAgentActivity _self;
  final $Res Function(BridgeSessionTimelineEventKind_SubAgentActivity) _then;

/// Create a copy of BridgeSessionTimelineEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? callId = null,Object? agentId = freezed,Object? path = freezed,Object? parentPath = freezed,Object? kind = null,Object? status = freezed,Object? message = freezed,Object? timedOut = freezed,Object? error = freezed,}) {
  return _then(BridgeSessionTimelineEventKind_SubAgentActivity(
callId: null == callId ? _self.callId : callId // ignore: cast_nullable_to_non_nullable
as String,agentId: freezed == agentId ? _self.agentId : agentId // ignore: cast_nullable_to_non_nullable
as String?,path: freezed == path ? _self.path : path // ignore: cast_nullable_to_non_nullable
as String?,parentPath: freezed == parentPath ? _self.parentPath : parentPath // ignore: cast_nullable_to_non_nullable
as String?,kind: null == kind ? _self.kind : kind // ignore: cast_nullable_to_non_nullable
as BridgeSubAgentActivityKind,status: freezed == status ? _self.status : status // ignore: cast_nullable_to_non_nullable
as BridgeAgentStatus?,message: freezed == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String?,timedOut: freezed == timedOut ? _self.timedOut : timedOut // ignore: cast_nullable_to_non_nullable
as bool?,error: freezed == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc


class BridgeSessionTimelineEventKind_TodoListChanged extends BridgeSessionTimelineEventKind {
  const BridgeSessionTimelineEventKind_TodoListChanged({required this.snapshot}): super._();
  

 final  BridgeTodoListSnapshot snapshot;

/// Create a copy of BridgeSessionTimelineEventKind
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionTimelineEventKind_TodoListChangedCopyWith<BridgeSessionTimelineEventKind_TodoListChanged> get copyWith => _$BridgeSessionTimelineEventKind_TodoListChangedCopyWithImpl<BridgeSessionTimelineEventKind_TodoListChanged>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionTimelineEventKind_TodoListChanged&&(identical(other.snapshot, snapshot) || other.snapshot == snapshot));
}


@override
int get hashCode => Object.hash(runtimeType,snapshot);

@override
String toString() {
  return 'BridgeSessionTimelineEventKind.todoListChanged(snapshot: $snapshot)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionTimelineEventKind_TodoListChangedCopyWith<$Res> implements $BridgeSessionTimelineEventKindCopyWith<$Res> {
  factory $BridgeSessionTimelineEventKind_TodoListChangedCopyWith(BridgeSessionTimelineEventKind_TodoListChanged value, $Res Function(BridgeSessionTimelineEventKind_TodoListChanged) _then) = _$BridgeSessionTimelineEventKind_TodoListChangedCopyWithImpl;
@useResult
$Res call({
 BridgeTodoListSnapshot snapshot
});




}
/// @nodoc
class _$BridgeSessionTimelineEventKind_TodoListChangedCopyWithImpl<$Res>
    implements $BridgeSessionTimelineEventKind_TodoListChangedCopyWith<$Res> {
  _$BridgeSessionTimelineEventKind_TodoListChangedCopyWithImpl(this._self, this._then);

  final BridgeSessionTimelineEventKind_TodoListChanged _self;
  final $Res Function(BridgeSessionTimelineEventKind_TodoListChanged) _then;

/// Create a copy of BridgeSessionTimelineEventKind
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? snapshot = null,}) {
  return _then(BridgeSessionTimelineEventKind_TodoListChanged(
snapshot: null == snapshot ? _self.snapshot : snapshot // ignore: cast_nullable_to_non_nullable
as BridgeTodoListSnapshot,
  ));
}


}

/// @nodoc
mixin _$BridgeSessionTurnState {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionTurnState);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeSessionTurnState()';
}


}

/// @nodoc
class $BridgeSessionTurnStateCopyWith<$Res>  {
$BridgeSessionTurnStateCopyWith(BridgeSessionTurnState _, $Res Function(BridgeSessionTurnState) __);
}


/// Adds pattern-matching-related methods to [BridgeSessionTurnState].
extension BridgeSessionTurnStatePatterns on BridgeSessionTurnState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeSessionTurnState_Queued value)?  queued,TResult Function( BridgeSessionTurnState_InProgress value)?  inProgress,TResult Function( BridgeSessionTurnState_Completed value)?  completed,TResult Function( BridgeSessionTurnState_Failed value)?  failed,TResult Function( BridgeSessionTurnState_Cancelled value)?  cancelled,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeSessionTurnState_Queued() when queued != null:
return queued(_that);case BridgeSessionTurnState_InProgress() when inProgress != null:
return inProgress(_that);case BridgeSessionTurnState_Completed() when completed != null:
return completed(_that);case BridgeSessionTurnState_Failed() when failed != null:
return failed(_that);case BridgeSessionTurnState_Cancelled() when cancelled != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeSessionTurnState_Queued value)  queued,required TResult Function( BridgeSessionTurnState_InProgress value)  inProgress,required TResult Function( BridgeSessionTurnState_Completed value)  completed,required TResult Function( BridgeSessionTurnState_Failed value)  failed,required TResult Function( BridgeSessionTurnState_Cancelled value)  cancelled,}){
final _that = this;
switch (_that) {
case BridgeSessionTurnState_Queued():
return queued(_that);case BridgeSessionTurnState_InProgress():
return inProgress(_that);case BridgeSessionTurnState_Completed():
return completed(_that);case BridgeSessionTurnState_Failed():
return failed(_that);case BridgeSessionTurnState_Cancelled():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeSessionTurnState_Queued value)?  queued,TResult? Function( BridgeSessionTurnState_InProgress value)?  inProgress,TResult? Function( BridgeSessionTurnState_Completed value)?  completed,TResult? Function( BridgeSessionTurnState_Failed value)?  failed,TResult? Function( BridgeSessionTurnState_Cancelled value)?  cancelled,}){
final _that = this;
switch (_that) {
case BridgeSessionTurnState_Queued() when queued != null:
return queued(_that);case BridgeSessionTurnState_InProgress() when inProgress != null:
return inProgress(_that);case BridgeSessionTurnState_Completed() when completed != null:
return completed(_that);case BridgeSessionTurnState_Failed() when failed != null:
return failed(_that);case BridgeSessionTurnState_Cancelled() when cancelled != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  queued,TResult Function( BridgeSessionTurnActivity activity)?  inProgress,TResult Function()?  completed,TResult Function( String reason)?  failed,TResult Function( String reason)?  cancelled,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeSessionTurnState_Queued() when queued != null:
return queued();case BridgeSessionTurnState_InProgress() when inProgress != null:
return inProgress(_that.activity);case BridgeSessionTurnState_Completed() when completed != null:
return completed();case BridgeSessionTurnState_Failed() when failed != null:
return failed(_that.reason);case BridgeSessionTurnState_Cancelled() when cancelled != null:
return cancelled(_that.reason);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  queued,required TResult Function( BridgeSessionTurnActivity activity)  inProgress,required TResult Function()  completed,required TResult Function( String reason)  failed,required TResult Function( String reason)  cancelled,}) {final _that = this;
switch (_that) {
case BridgeSessionTurnState_Queued():
return queued();case BridgeSessionTurnState_InProgress():
return inProgress(_that.activity);case BridgeSessionTurnState_Completed():
return completed();case BridgeSessionTurnState_Failed():
return failed(_that.reason);case BridgeSessionTurnState_Cancelled():
return cancelled(_that.reason);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  queued,TResult? Function( BridgeSessionTurnActivity activity)?  inProgress,TResult? Function()?  completed,TResult? Function( String reason)?  failed,TResult? Function( String reason)?  cancelled,}) {final _that = this;
switch (_that) {
case BridgeSessionTurnState_Queued() when queued != null:
return queued();case BridgeSessionTurnState_InProgress() when inProgress != null:
return inProgress(_that.activity);case BridgeSessionTurnState_Completed() when completed != null:
return completed();case BridgeSessionTurnState_Failed() when failed != null:
return failed(_that.reason);case BridgeSessionTurnState_Cancelled() when cancelled != null:
return cancelled(_that.reason);case _:
  return null;

}
}

}

/// @nodoc


class BridgeSessionTurnState_Queued extends BridgeSessionTurnState {
  const BridgeSessionTurnState_Queued(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionTurnState_Queued);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeSessionTurnState.queued()';
}


}




/// @nodoc


class BridgeSessionTurnState_InProgress extends BridgeSessionTurnState {
  const BridgeSessionTurnState_InProgress({required this.activity}): super._();


 final  BridgeSessionTurnActivity activity;

/// Create a copy of BridgeSessionTurnState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionTurnState_InProgressCopyWith<BridgeSessionTurnState_InProgress> get copyWith => _$BridgeSessionTurnState_InProgressCopyWithImpl<BridgeSessionTurnState_InProgress>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionTurnState_InProgress&&(identical(other.activity, activity) || other.activity == activity));
}


@override
int get hashCode => Object.hash(runtimeType,activity);

@override
String toString() {
  return 'BridgeSessionTurnState.inProgress(activity: $activity)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionTurnState_InProgressCopyWith<$Res> implements $BridgeSessionTurnStateCopyWith<$Res> {
  factory $BridgeSessionTurnState_InProgressCopyWith(BridgeSessionTurnState_InProgress value, $Res Function(BridgeSessionTurnState_InProgress) _then) = _$BridgeSessionTurnState_InProgressCopyWithImpl;
@useResult
$Res call({
 BridgeSessionTurnActivity activity
});




}
/// @nodoc
class _$BridgeSessionTurnState_InProgressCopyWithImpl<$Res>
    implements $BridgeSessionTurnState_InProgressCopyWith<$Res> {
  _$BridgeSessionTurnState_InProgressCopyWithImpl(this._self, this._then);

  final BridgeSessionTurnState_InProgress _self;
  final $Res Function(BridgeSessionTurnState_InProgress) _then;

/// Create a copy of BridgeSessionTurnState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? activity = null,}) {
  return _then(BridgeSessionTurnState_InProgress(
activity: null == activity ? _self.activity : activity // ignore: cast_nullable_to_non_nullable
as BridgeSessionTurnActivity,
  ));
}


}

/// @nodoc


class BridgeSessionTurnState_Completed extends BridgeSessionTurnState {
  const BridgeSessionTurnState_Completed(): super._();







@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionTurnState_Completed);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeSessionTurnState.completed()';
}


}




/// @nodoc


class BridgeSessionTurnState_Failed extends BridgeSessionTurnState {
  const BridgeSessionTurnState_Failed({required this.reason}): super._();


 final  String reason;

/// Create a copy of BridgeSessionTurnState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionTurnState_FailedCopyWith<BridgeSessionTurnState_Failed> get copyWith => _$BridgeSessionTurnState_FailedCopyWithImpl<BridgeSessionTurnState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionTurnState_Failed&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,reason);

@override
String toString() {
  return 'BridgeSessionTurnState.failed(reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionTurnState_FailedCopyWith<$Res> implements $BridgeSessionTurnStateCopyWith<$Res> {
  factory $BridgeSessionTurnState_FailedCopyWith(BridgeSessionTurnState_Failed value, $Res Function(BridgeSessionTurnState_Failed) _then) = _$BridgeSessionTurnState_FailedCopyWithImpl;
@useResult
$Res call({
 String reason
});




}
/// @nodoc
class _$BridgeSessionTurnState_FailedCopyWithImpl<$Res>
    implements $BridgeSessionTurnState_FailedCopyWith<$Res> {
  _$BridgeSessionTurnState_FailedCopyWithImpl(this._self, this._then);

  final BridgeSessionTurnState_Failed _self;
  final $Res Function(BridgeSessionTurnState_Failed) _then;

/// Create a copy of BridgeSessionTurnState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reason = null,}) {
  return _then(BridgeSessionTurnState_Failed(
reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeSessionTurnState_Cancelled extends BridgeSessionTurnState {
  const BridgeSessionTurnState_Cancelled({required this.reason}): super._();


 final  String reason;

/// Create a copy of BridgeSessionTurnState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeSessionTurnState_CancelledCopyWith<BridgeSessionTurnState_Cancelled> get copyWith => _$BridgeSessionTurnState_CancelledCopyWithImpl<BridgeSessionTurnState_Cancelled>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeSessionTurnState_Cancelled&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode => Object.hash(runtimeType,reason);

@override
String toString() {
  return 'BridgeSessionTurnState.cancelled(reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeSessionTurnState_CancelledCopyWith<$Res> implements $BridgeSessionTurnStateCopyWith<$Res> {
  factory $BridgeSessionTurnState_CancelledCopyWith(BridgeSessionTurnState_Cancelled value, $Res Function(BridgeSessionTurnState_Cancelled) _then) = _$BridgeSessionTurnState_CancelledCopyWithImpl;
@useResult
$Res call({
 String reason
});




}
/// @nodoc
class _$BridgeSessionTurnState_CancelledCopyWithImpl<$Res>
    implements $BridgeSessionTurnState_CancelledCopyWith<$Res> {
  _$BridgeSessionTurnState_CancelledCopyWithImpl(this._self, this._then);

  final BridgeSessionTurnState_Cancelled _self;
  final $Res Function(BridgeSessionTurnState_Cancelled) _then;

/// Create a copy of BridgeSessionTurnState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? reason = null,}) {
  return _then(BridgeSessionTurnState_Cancelled(
reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

// dart format on

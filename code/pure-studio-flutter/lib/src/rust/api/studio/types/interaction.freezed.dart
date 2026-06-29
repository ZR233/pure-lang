// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'interaction.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeInteractionPayloadDto {





@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionPayloadDto);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
  return 'BridgeInteractionPayloadDto()';
}


}

/// @nodoc
class $BridgeInteractionPayloadDtoCopyWith<$Res>  {
$BridgeInteractionPayloadDtoCopyWith(BridgeInteractionPayloadDto _, $Res Function(BridgeInteractionPayloadDto) __);
}


/// Adds pattern-matching-related methods to [BridgeInteractionPayloadDto].
extension BridgeInteractionPayloadDtoPatterns on BridgeInteractionPayloadDto {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeInteractionPayloadDto_UserInput value)?  userInput,TResult Function( BridgeInteractionPayloadDto_ToolApproval value)?  toolApproval,TResult Function( BridgeInteractionPayloadDto_PlanConfirmation value)?  planConfirmation,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeInteractionPayloadDto_UserInput() when userInput != null:
return userInput(_that);case BridgeInteractionPayloadDto_ToolApproval() when toolApproval != null:
return toolApproval(_that);case BridgeInteractionPayloadDto_PlanConfirmation() when planConfirmation != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeInteractionPayloadDto_UserInput value)  userInput,required TResult Function( BridgeInteractionPayloadDto_ToolApproval value)  toolApproval,required TResult Function( BridgeInteractionPayloadDto_PlanConfirmation value)  planConfirmation,}){
final _that = this;
switch (_that) {
case BridgeInteractionPayloadDto_UserInput():
return userInput(_that);case BridgeInteractionPayloadDto_ToolApproval():
return toolApproval(_that);case BridgeInteractionPayloadDto_PlanConfirmation():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeInteractionPayloadDto_UserInput value)?  userInput,TResult? Function( BridgeInteractionPayloadDto_ToolApproval value)?  toolApproval,TResult? Function( BridgeInteractionPayloadDto_PlanConfirmation value)?  planConfirmation,}){
final _that = this;
switch (_that) {
case BridgeInteractionPayloadDto_UserInput() when userInput != null:
return userInput(_that);case BridgeInteractionPayloadDto_ToolApproval() when toolApproval != null:
return toolApproval(_that);case BridgeInteractionPayloadDto_PlanConfirmation() when planConfirmation != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( List<BridgeUserQuestionDto> questions)?  userInput,TResult Function( String name,  String argumentsJson,  String? workingDirectory,  String? parentAgentId)?  toolApproval,TResult Function( String planId,  String content)?  planConfirmation,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeInteractionPayloadDto_UserInput() when userInput != null:
return userInput(_that.questions);case BridgeInteractionPayloadDto_ToolApproval() when toolApproval != null:
return toolApproval(_that.name,_that.argumentsJson,_that.workingDirectory,_that.parentAgentId);case BridgeInteractionPayloadDto_PlanConfirmation() when planConfirmation != null:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( List<BridgeUserQuestionDto> questions)  userInput,required TResult Function( String name,  String argumentsJson,  String? workingDirectory,  String? parentAgentId)  toolApproval,required TResult Function( String planId,  String content)  planConfirmation,}) {final _that = this;
switch (_that) {
case BridgeInteractionPayloadDto_UserInput():
return userInput(_that.questions);case BridgeInteractionPayloadDto_ToolApproval():
return toolApproval(_that.name,_that.argumentsJson,_that.workingDirectory,_that.parentAgentId);case BridgeInteractionPayloadDto_PlanConfirmation():
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( List<BridgeUserQuestionDto> questions)?  userInput,TResult? Function( String name,  String argumentsJson,  String? workingDirectory,  String? parentAgentId)?  toolApproval,TResult? Function( String planId,  String content)?  planConfirmation,}) {final _that = this;
switch (_that) {
case BridgeInteractionPayloadDto_UserInput() when userInput != null:
return userInput(_that.questions);case BridgeInteractionPayloadDto_ToolApproval() when toolApproval != null:
return toolApproval(_that.name,_that.argumentsJson,_that.workingDirectory,_that.parentAgentId);case BridgeInteractionPayloadDto_PlanConfirmation() when planConfirmation != null:
return planConfirmation(_that.planId,_that.content);case _:
  return null;

}
}

}

/// @nodoc


class BridgeInteractionPayloadDto_UserInput extends BridgeInteractionPayloadDto {
  const BridgeInteractionPayloadDto_UserInput({required final  List<BridgeUserQuestionDto> questions}): _questions = questions,super._();
  

 final  List<BridgeUserQuestionDto> _questions;
 List<BridgeUserQuestionDto> get questions {
  if (_questions is EqualUnmodifiableListView) return _questions;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_questions);
}


/// Create a copy of BridgeInteractionPayloadDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeInteractionPayloadDto_UserInputCopyWith<BridgeInteractionPayloadDto_UserInput> get copyWith => _$BridgeInteractionPayloadDto_UserInputCopyWithImpl<BridgeInteractionPayloadDto_UserInput>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionPayloadDto_UserInput&&const DeepCollectionEquality().equals(other._questions, _questions));
}


@override
int get hashCode => Object.hash(runtimeType,const DeepCollectionEquality().hash(_questions));

@override
String toString() {
  return 'BridgeInteractionPayloadDto.userInput(questions: $questions)';
}


}

/// @nodoc
abstract mixin class $BridgeInteractionPayloadDto_UserInputCopyWith<$Res> implements $BridgeInteractionPayloadDtoCopyWith<$Res> {
  factory $BridgeInteractionPayloadDto_UserInputCopyWith(BridgeInteractionPayloadDto_UserInput value, $Res Function(BridgeInteractionPayloadDto_UserInput) _then) = _$BridgeInteractionPayloadDto_UserInputCopyWithImpl;
@useResult
$Res call({
 List<BridgeUserQuestionDto> questions
});




}
/// @nodoc
class _$BridgeInteractionPayloadDto_UserInputCopyWithImpl<$Res>
    implements $BridgeInteractionPayloadDto_UserInputCopyWith<$Res> {
  _$BridgeInteractionPayloadDto_UserInputCopyWithImpl(this._self, this._then);

  final BridgeInteractionPayloadDto_UserInput _self;
  final $Res Function(BridgeInteractionPayloadDto_UserInput) _then;

/// Create a copy of BridgeInteractionPayloadDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? questions = null,}) {
  return _then(BridgeInteractionPayloadDto_UserInput(
questions: null == questions ? _self._questions : questions // ignore: cast_nullable_to_non_nullable
as List<BridgeUserQuestionDto>,
  ));
}


}

/// @nodoc


class BridgeInteractionPayloadDto_ToolApproval extends BridgeInteractionPayloadDto {
  const BridgeInteractionPayloadDto_ToolApproval({required this.name, required this.argumentsJson, this.workingDirectory, this.parentAgentId}): super._();
  

 final  String name;
 final  String argumentsJson;
 final  String? workingDirectory;
 final  String? parentAgentId;

/// Create a copy of BridgeInteractionPayloadDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeInteractionPayloadDto_ToolApprovalCopyWith<BridgeInteractionPayloadDto_ToolApproval> get copyWith => _$BridgeInteractionPayloadDto_ToolApprovalCopyWithImpl<BridgeInteractionPayloadDto_ToolApproval>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionPayloadDto_ToolApproval&&(identical(other.name, name) || other.name == name)&&(identical(other.argumentsJson, argumentsJson) || other.argumentsJson == argumentsJson)&&(identical(other.workingDirectory, workingDirectory) || other.workingDirectory == workingDirectory)&&(identical(other.parentAgentId, parentAgentId) || other.parentAgentId == parentAgentId));
}


@override
int get hashCode => Object.hash(runtimeType,name,argumentsJson,workingDirectory,parentAgentId);

@override
String toString() {
  return 'BridgeInteractionPayloadDto.toolApproval(name: $name, argumentsJson: $argumentsJson, workingDirectory: $workingDirectory, parentAgentId: $parentAgentId)';
}


}

/// @nodoc
abstract mixin class $BridgeInteractionPayloadDto_ToolApprovalCopyWith<$Res> implements $BridgeInteractionPayloadDtoCopyWith<$Res> {
  factory $BridgeInteractionPayloadDto_ToolApprovalCopyWith(BridgeInteractionPayloadDto_ToolApproval value, $Res Function(BridgeInteractionPayloadDto_ToolApproval) _then) = _$BridgeInteractionPayloadDto_ToolApprovalCopyWithImpl;
@useResult
$Res call({
 String name, String argumentsJson, String? workingDirectory, String? parentAgentId
});




}
/// @nodoc
class _$BridgeInteractionPayloadDto_ToolApprovalCopyWithImpl<$Res>
    implements $BridgeInteractionPayloadDto_ToolApprovalCopyWith<$Res> {
  _$BridgeInteractionPayloadDto_ToolApprovalCopyWithImpl(this._self, this._then);

  final BridgeInteractionPayloadDto_ToolApproval _self;
  final $Res Function(BridgeInteractionPayloadDto_ToolApproval) _then;

/// Create a copy of BridgeInteractionPayloadDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? name = null,Object? argumentsJson = null,Object? workingDirectory = freezed,Object? parentAgentId = freezed,}) {
  return _then(BridgeInteractionPayloadDto_ToolApproval(
name: null == name ? _self.name : name // ignore: cast_nullable_to_non_nullable
as String,argumentsJson: null == argumentsJson ? _self.argumentsJson : argumentsJson // ignore: cast_nullable_to_non_nullable
as String,workingDirectory: freezed == workingDirectory ? _self.workingDirectory : workingDirectory // ignore: cast_nullable_to_non_nullable
as String?,parentAgentId: freezed == parentAgentId ? _self.parentAgentId : parentAgentId // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc


class BridgeInteractionPayloadDto_PlanConfirmation extends BridgeInteractionPayloadDto {
  const BridgeInteractionPayloadDto_PlanConfirmation({required this.planId, required this.content}): super._();
  

 final  String planId;
 final  String content;

/// Create a copy of BridgeInteractionPayloadDto
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeInteractionPayloadDto_PlanConfirmationCopyWith<BridgeInteractionPayloadDto_PlanConfirmation> get copyWith => _$BridgeInteractionPayloadDto_PlanConfirmationCopyWithImpl<BridgeInteractionPayloadDto_PlanConfirmation>(this, _$identity);



@override
bool operator ==(Object other) {
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionPayloadDto_PlanConfirmation&&(identical(other.planId, planId) || other.planId == planId)&&(identical(other.content, content) || other.content == content));
}


@override
int get hashCode => Object.hash(runtimeType,planId,content);

@override
String toString() {
  return 'BridgeInteractionPayloadDto.planConfirmation(planId: $planId, content: $content)';
}


}

/// @nodoc
abstract mixin class $BridgeInteractionPayloadDto_PlanConfirmationCopyWith<$Res> implements $BridgeInteractionPayloadDtoCopyWith<$Res> {
  factory $BridgeInteractionPayloadDto_PlanConfirmationCopyWith(BridgeInteractionPayloadDto_PlanConfirmation value, $Res Function(BridgeInteractionPayloadDto_PlanConfirmation) _then) = _$BridgeInteractionPayloadDto_PlanConfirmationCopyWithImpl;
@useResult
$Res call({
 String planId, String content
});




}
/// @nodoc
class _$BridgeInteractionPayloadDto_PlanConfirmationCopyWithImpl<$Res>
    implements $BridgeInteractionPayloadDto_PlanConfirmationCopyWith<$Res> {
  _$BridgeInteractionPayloadDto_PlanConfirmationCopyWithImpl(this._self, this._then);

  final BridgeInteractionPayloadDto_PlanConfirmation _self;
  final $Res Function(BridgeInteractionPayloadDto_PlanConfirmation) _then;

/// Create a copy of BridgeInteractionPayloadDto
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? planId = null,Object? content = null,}) {
  return _then(BridgeInteractionPayloadDto_PlanConfirmation(
planId: null == planId ? _self.planId : planId // ignore: cast_nullable_to_non_nullable
as String,content: null == content ? _self.content : content // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

// dart format on

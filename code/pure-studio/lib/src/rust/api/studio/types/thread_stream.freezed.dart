// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'thread_stream.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeInteractionContent {

 Object get state;



@override
bool operator ==(Object other) {
  final _this = this as BridgeInteractionContent;
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionContent&&const DeepCollectionEquality().equals(other.state, _this.state));
}


@override
int get hashCode {
  final _this = this as BridgeInteractionContent;
  return Object.hash(runtimeType,const DeepCollectionEquality().hash(_this.state));
}

@override
String toString() {
  final _this = this as BridgeInteractionContent;
  return 'BridgeInteractionContent(state: ${_this.state})';
}


}

/// @nodoc
class $BridgeInteractionContentCopyWith<$Res>  {
$BridgeInteractionContentCopyWith(BridgeInteractionContent _, $Res Function(BridgeInteractionContent) __);
}


/// Adds pattern-matching-related methods to [BridgeInteractionContent].
extension BridgeInteractionContentPatterns on BridgeInteractionContent {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeInteractionContent_UserInput value)?  userInput,TResult Function( BridgeInteractionContent_ToolApproval value)?  toolApproval,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeInteractionContent_UserInput() when userInput != null:
return userInput(_that);case BridgeInteractionContent_ToolApproval() when toolApproval != null:
return toolApproval(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeInteractionContent_UserInput value)  userInput,required TResult Function( BridgeInteractionContent_ToolApproval value)  toolApproval,}){
final _that = this;
switch (_that) {
case BridgeInteractionContent_UserInput():
return userInput(_that);case BridgeInteractionContent_ToolApproval():
return toolApproval(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeInteractionContent_UserInput value)?  userInput,TResult? Function( BridgeInteractionContent_ToolApproval value)?  toolApproval,}){
final _that = this;
switch (_that) {
case BridgeInteractionContent_UserInput() when userInput != null:
return userInput(_that);case BridgeInteractionContent_ToolApproval() when toolApproval != null:
return toolApproval(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( List<BridgeUserQuestion> questions,  BridgeUserInputInteractionState state)?  userInput,TResult Function( String name,  String argumentsJson,  String? workingDirectory,  String? parentAgentId,  BridgeToolApprovalInteractionState state)?  toolApproval,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeInteractionContent_UserInput() when userInput != null:
return userInput(_that.questions,_that.state);case BridgeInteractionContent_ToolApproval() when toolApproval != null:
return toolApproval(_that.name,_that.argumentsJson,_that.workingDirectory,_that.parentAgentId,_that.state);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( List<BridgeUserQuestion> questions,  BridgeUserInputInteractionState state)  userInput,required TResult Function( String name,  String argumentsJson,  String? workingDirectory,  String? parentAgentId,  BridgeToolApprovalInteractionState state)  toolApproval,}) {final _that = this;
switch (_that) {
case BridgeInteractionContent_UserInput():
return userInput(_that.questions,_that.state);case BridgeInteractionContent_ToolApproval():
return toolApproval(_that.name,_that.argumentsJson,_that.workingDirectory,_that.parentAgentId,_that.state);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( List<BridgeUserQuestion> questions,  BridgeUserInputInteractionState state)?  userInput,TResult? Function( String name,  String argumentsJson,  String? workingDirectory,  String? parentAgentId,  BridgeToolApprovalInteractionState state)?  toolApproval,}) {final _that = this;
switch (_that) {
case BridgeInteractionContent_UserInput() when userInput != null:
return userInput(_that.questions,_that.state);case BridgeInteractionContent_ToolApproval() when toolApproval != null:
return toolApproval(_that.name,_that.argumentsJson,_that.workingDirectory,_that.parentAgentId,_that.state);case _:
  return null;

}
}

}

/// @nodoc


class BridgeInteractionContent_UserInput extends BridgeInteractionContent {
  const BridgeInteractionContent_UserInput({required  List<BridgeUserQuestion> questions, required this.state}): _questions = questions,super._();


 final  List<BridgeUserQuestion> _questions;
 List<BridgeUserQuestion> get questions {
  if (_questions is EqualUnmodifiableListView) return _questions;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_questions);
}

@override final  BridgeUserInputInteractionState state;

/// Create a copy of BridgeInteractionContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeInteractionContent_UserInputCopyWith<BridgeInteractionContent_UserInput> get copyWith => _$BridgeInteractionContent_UserInputCopyWithImpl<BridgeInteractionContent_UserInput>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionContent_UserInput&&const DeepCollectionEquality().equals(other.questions, _questions)&&(identical(other.state, state) || other.state == state));
}


@override
int get hashCode {
    return Object.hash(runtimeType,const DeepCollectionEquality().hash(_questions),state);
}

@override
String toString() {
    return 'BridgeInteractionContent.userInput(questions: $questions, state: $state)';
}


}

/// @nodoc
abstract mixin class $BridgeInteractionContent_UserInputCopyWith<$Res> implements $BridgeInteractionContentCopyWith<$Res> {
  factory $BridgeInteractionContent_UserInputCopyWith(BridgeInteractionContent_UserInput value, $Res Function(BridgeInteractionContent_UserInput) _then) = _$BridgeInteractionContent_UserInputCopyWithImpl;
@useResult
$Res call({
 List<BridgeUserQuestion> questions, BridgeUserInputInteractionState state
});


$BridgeUserInputInteractionStateCopyWith<$Res> get state;

}
/// @nodoc
class _$BridgeInteractionContent_UserInputCopyWithImpl<$Res>
    implements $BridgeInteractionContent_UserInputCopyWith<$Res> {
  _$BridgeInteractionContent_UserInputCopyWithImpl(this._self, this._then);

  final BridgeInteractionContent_UserInput _self;
  final $Res Function(BridgeInteractionContent_UserInput) _then;

/// Create a copy of BridgeInteractionContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? questions = null,Object? state = null,}) {
  return _then(BridgeInteractionContent_UserInput(
questions: null == questions ? _self._questions : questions // ignore: cast_nullable_to_non_nullable
as List<BridgeUserQuestion>,state: null == state ? _self.state : state // ignore: cast_nullable_to_non_nullable
as BridgeUserInputInteractionState,
  ));
}

/// Create a copy of BridgeInteractionContent
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeUserInputInteractionStateCopyWith<$Res> get state {

  return $BridgeUserInputInteractionStateCopyWith<$Res>(_self.state, (value) {
    return _then(_self.copyWith(state: value));
  });
}
}

/// @nodoc


class BridgeInteractionContent_ToolApproval extends BridgeInteractionContent {
  const BridgeInteractionContent_ToolApproval({required this.name, required this.argumentsJson, this.workingDirectory, this.parentAgentId, required this.state}): super._();


 final  String name;
 final  String argumentsJson;
 final  String? workingDirectory;
 final  String? parentAgentId;
@override final  BridgeToolApprovalInteractionState state;

/// Create a copy of BridgeInteractionContent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeInteractionContent_ToolApprovalCopyWith<BridgeInteractionContent_ToolApproval> get copyWith => _$BridgeInteractionContent_ToolApprovalCopyWithImpl<BridgeInteractionContent_ToolApproval>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionContent_ToolApproval&&(identical(other.name, name) || other.name == name)&&(identical(other.argumentsJson, argumentsJson) || other.argumentsJson == argumentsJson)&&(identical(other.workingDirectory, workingDirectory) || other.workingDirectory == workingDirectory)&&(identical(other.parentAgentId, parentAgentId) || other.parentAgentId == parentAgentId)&&(identical(other.state, state) || other.state == state));
}


@override
int get hashCode {
    return Object.hash(runtimeType,name,argumentsJson,workingDirectory,parentAgentId,state);
}

@override
String toString() {
    return 'BridgeInteractionContent.toolApproval(name: $name, argumentsJson: $argumentsJson, workingDirectory: $workingDirectory, parentAgentId: $parentAgentId, state: $state)';
}


}

/// @nodoc
abstract mixin class $BridgeInteractionContent_ToolApprovalCopyWith<$Res> implements $BridgeInteractionContentCopyWith<$Res> {
  factory $BridgeInteractionContent_ToolApprovalCopyWith(BridgeInteractionContent_ToolApproval value, $Res Function(BridgeInteractionContent_ToolApproval) _then) = _$BridgeInteractionContent_ToolApprovalCopyWithImpl;
@useResult
$Res call({
 String name, String argumentsJson, String? workingDirectory, String? parentAgentId, BridgeToolApprovalInteractionState state
});


$BridgeToolApprovalInteractionStateCopyWith<$Res> get state;

}
/// @nodoc
class _$BridgeInteractionContent_ToolApprovalCopyWithImpl<$Res>
    implements $BridgeInteractionContent_ToolApprovalCopyWith<$Res> {
  _$BridgeInteractionContent_ToolApprovalCopyWithImpl(this._self, this._then);

  final BridgeInteractionContent_ToolApproval _self;
  final $Res Function(BridgeInteractionContent_ToolApproval) _then;

/// Create a copy of BridgeInteractionContent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? name = null,Object? argumentsJson = null,Object? workingDirectory = freezed,Object? parentAgentId = freezed,Object? state = null,}) {
  return _then(BridgeInteractionContent_ToolApproval(
name: null == name ? _self.name : name // ignore: cast_nullable_to_non_nullable
as String,argumentsJson: null == argumentsJson ? _self.argumentsJson : argumentsJson // ignore: cast_nullable_to_non_nullable
as String,workingDirectory: freezed == workingDirectory ? _self.workingDirectory : workingDirectory // ignore: cast_nullable_to_non_nullable
as String?,parentAgentId: freezed == parentAgentId ? _self.parentAgentId : parentAgentId // ignore: cast_nullable_to_non_nullable
as String?,state: null == state ? _self.state : state // ignore: cast_nullable_to_non_nullable
as BridgeToolApprovalInteractionState,
  ));
}

/// Create a copy of BridgeInteractionContent
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeToolApprovalInteractionStateCopyWith<$Res> get state {

  return $BridgeToolApprovalInteractionStateCopyWith<$Res>(_self.state, (value) {
    return _then(_self.copyWith(state: value));
  });
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeInteractionResolution_UserInput value)?  userInput,TResult Function( BridgeInteractionResolution_ToolApproval value)?  toolApproval,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeInteractionResolution_UserInput() when userInput != null:
return userInput(_that);case BridgeInteractionResolution_ToolApproval() when toolApproval != null:
return toolApproval(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeInteractionResolution_UserInput value)  userInput,required TResult Function( BridgeInteractionResolution_ToolApproval value)  toolApproval,}){
final _that = this;
switch (_that) {
case BridgeInteractionResolution_UserInput():
return userInput(_that);case BridgeInteractionResolution_ToolApproval():
return toolApproval(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeInteractionResolution_UserInput value)?  userInput,TResult? Function( BridgeInteractionResolution_ToolApproval value)?  toolApproval,}){
final _that = this;
switch (_that) {
case BridgeInteractionResolution_UserInput() when userInput != null:
return userInput(_that);case BridgeInteractionResolution_ToolApproval() when toolApproval != null:
return toolApproval(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( List<BridgeUserInputAnswer> answers)?  userInput,TResult Function( BridgeToolApprovalResolution decision,  String? reason)?  toolApproval,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeInteractionResolution_UserInput() when userInput != null:
return userInput(_that.answers);case BridgeInteractionResolution_ToolApproval() when toolApproval != null:
return toolApproval(_that.decision,_that.reason);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( List<BridgeUserInputAnswer> answers)  userInput,required TResult Function( BridgeToolApprovalResolution decision,  String? reason)  toolApproval,}) {final _that = this;
switch (_that) {
case BridgeInteractionResolution_UserInput():
return userInput(_that.answers);case BridgeInteractionResolution_ToolApproval():
return toolApproval(_that.decision,_that.reason);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( List<BridgeUserInputAnswer> answers)?  userInput,TResult? Function( BridgeToolApprovalResolution decision,  String? reason)?  toolApproval,}) {final _that = this;
switch (_that) {
case BridgeInteractionResolution_UserInput() when userInput != null:
return userInput(_that.answers);case BridgeInteractionResolution_ToolApproval() when toolApproval != null:
return toolApproval(_that.decision,_that.reason);case _:
  return null;

}
}

}

/// @nodoc


class BridgeInteractionResolution_UserInput extends BridgeInteractionResolution {
  const BridgeInteractionResolution_UserInput({required  List<BridgeUserInputAnswer> answers}): _answers = answers,super._();


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
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeInteractionResolution_UserInput&&const DeepCollectionEquality().equals(other.answers, _answers));
}


@override
int get hashCode {
    return Object.hash(runtimeType,const DeepCollectionEquality().hash(_answers));
}

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
int get hashCode {
    return Object.hash(runtimeType,decision,reason);
}

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
int get hashCode {
    return Object.hash(runtimeType,turn);
}

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
int get hashCode {
    return Object.hash(runtimeType,turn);
}

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
int get hashCode {
    return Object.hash(runtimeType,turn);
}

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
int get hashCode {
    return Object.hash(runtimeType,item);
}

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
int get hashCode {
    return Object.hash(runtimeType,delta);
}

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
int get hashCode {
    return Object.hash(runtimeType,item);
}

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
int get hashCode {
    return Object.hash(runtimeType,interaction);
}

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
int get hashCode {
    return Object.hash(runtimeType,runtime);
}

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
int get hashCode {
    return Object.hash(runtimeType,dropped);
}

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
int get hashCode {
    return Object.hash(runtimeType,snapshot);
}

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
int get hashCode {
    return Object.hash(runtimeType,notification);
}

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
mixin _$BridgeToolApprovalInteractionState {

 String get operationId;
/// Create a copy of BridgeToolApprovalInteractionState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeToolApprovalInteractionStateCopyWith<BridgeToolApprovalInteractionState> get copyWith => _$BridgeToolApprovalInteractionStateCopyWithImpl<BridgeToolApprovalInteractionState>(this as BridgeToolApprovalInteractionState, _$identity);



@override
bool operator ==(Object other) {
  final _this = this as BridgeToolApprovalInteractionState;
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeToolApprovalInteractionState&&(identical(other.operationId, _this.operationId) || other.operationId == _this.operationId));
}


@override
int get hashCode {
  final _this = this as BridgeToolApprovalInteractionState;
  return Object.hash(runtimeType,_this.operationId);
}

@override
String toString() {
  final _this = this as BridgeToolApprovalInteractionState;
  return 'BridgeToolApprovalInteractionState(operationId: ${_this.operationId})';
}


}

/// @nodoc
abstract mixin class $BridgeToolApprovalInteractionStateCopyWith<$Res>  {
  factory $BridgeToolApprovalInteractionStateCopyWith(BridgeToolApprovalInteractionState value, $Res Function(BridgeToolApprovalInteractionState) _then) = _$BridgeToolApprovalInteractionStateCopyWithImpl;
@useResult
$Res call({
 String operationId
});




}
/// @nodoc
class _$BridgeToolApprovalInteractionStateCopyWithImpl<$Res>
    implements $BridgeToolApprovalInteractionStateCopyWith<$Res> {
  _$BridgeToolApprovalInteractionStateCopyWithImpl(this._self, this._then);

  final BridgeToolApprovalInteractionState _self;
  final $Res Function(BridgeToolApprovalInteractionState) _then;

/// Create a copy of BridgeToolApprovalInteractionState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? operationId = null,}) {
  return _then(_self.copyWith(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}

}


/// Adds pattern-matching-related methods to [BridgeToolApprovalInteractionState].
extension BridgeToolApprovalInteractionStatePatterns on BridgeToolApprovalInteractionState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeToolApprovalInteractionState_Pending value)?  pending,TResult Function( BridgeToolApprovalInteractionState_Resolved value)?  resolved,TResult Function( BridgeToolApprovalInteractionState_Cancelled value)?  cancelled,TResult Function( BridgeToolApprovalInteractionState_Expired value)?  expired,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeToolApprovalInteractionState_Pending() when pending != null:
return pending(_that);case BridgeToolApprovalInteractionState_Resolved() when resolved != null:
return resolved(_that);case BridgeToolApprovalInteractionState_Cancelled() when cancelled != null:
return cancelled(_that);case BridgeToolApprovalInteractionState_Expired() when expired != null:
return expired(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeToolApprovalInteractionState_Pending value)  pending,required TResult Function( BridgeToolApprovalInteractionState_Resolved value)  resolved,required TResult Function( BridgeToolApprovalInteractionState_Cancelled value)  cancelled,required TResult Function( BridgeToolApprovalInteractionState_Expired value)  expired,}){
final _that = this;
switch (_that) {
case BridgeToolApprovalInteractionState_Pending():
return pending(_that);case BridgeToolApprovalInteractionState_Resolved():
return resolved(_that);case BridgeToolApprovalInteractionState_Cancelled():
return cancelled(_that);case BridgeToolApprovalInteractionState_Expired():
return expired(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeToolApprovalInteractionState_Pending value)?  pending,TResult? Function( BridgeToolApprovalInteractionState_Resolved value)?  resolved,TResult? Function( BridgeToolApprovalInteractionState_Cancelled value)?  cancelled,TResult? Function( BridgeToolApprovalInteractionState_Expired value)?  expired,}){
final _that = this;
switch (_that) {
case BridgeToolApprovalInteractionState_Pending() when pending != null:
return pending(_that);case BridgeToolApprovalInteractionState_Resolved() when resolved != null:
return resolved(_that);case BridgeToolApprovalInteractionState_Cancelled() when cancelled != null:
return cancelled(_that);case BridgeToolApprovalInteractionState_Expired() when expired != null:
return expired(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String operationId)?  pending,TResult Function( String operationId,  PlatformInt64 resolvedAt,  BridgeToolApprovalResolution decision,  String? reason)?  resolved,TResult Function( String operationId,  PlatformInt64 cancelledAt,  String reason)?  cancelled,TResult Function( String operationId,  PlatformInt64 expiredAt)?  expired,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeToolApprovalInteractionState_Pending() when pending != null:
return pending(_that.operationId);case BridgeToolApprovalInteractionState_Resolved() when resolved != null:
return resolved(_that.operationId,_that.resolvedAt,_that.decision,_that.reason);case BridgeToolApprovalInteractionState_Cancelled() when cancelled != null:
return cancelled(_that.operationId,_that.cancelledAt,_that.reason);case BridgeToolApprovalInteractionState_Expired() when expired != null:
return expired(_that.operationId,_that.expiredAt);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String operationId)  pending,required TResult Function( String operationId,  PlatformInt64 resolvedAt,  BridgeToolApprovalResolution decision,  String? reason)  resolved,required TResult Function( String operationId,  PlatformInt64 cancelledAt,  String reason)  cancelled,required TResult Function( String operationId,  PlatformInt64 expiredAt)  expired,}) {final _that = this;
switch (_that) {
case BridgeToolApprovalInteractionState_Pending():
return pending(_that.operationId);case BridgeToolApprovalInteractionState_Resolved():
return resolved(_that.operationId,_that.resolvedAt,_that.decision,_that.reason);case BridgeToolApprovalInteractionState_Cancelled():
return cancelled(_that.operationId,_that.cancelledAt,_that.reason);case BridgeToolApprovalInteractionState_Expired():
return expired(_that.operationId,_that.expiredAt);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String operationId)?  pending,TResult? Function( String operationId,  PlatformInt64 resolvedAt,  BridgeToolApprovalResolution decision,  String? reason)?  resolved,TResult? Function( String operationId,  PlatformInt64 cancelledAt,  String reason)?  cancelled,TResult? Function( String operationId,  PlatformInt64 expiredAt)?  expired,}) {final _that = this;
switch (_that) {
case BridgeToolApprovalInteractionState_Pending() when pending != null:
return pending(_that.operationId);case BridgeToolApprovalInteractionState_Resolved() when resolved != null:
return resolved(_that.operationId,_that.resolvedAt,_that.decision,_that.reason);case BridgeToolApprovalInteractionState_Cancelled() when cancelled != null:
return cancelled(_that.operationId,_that.cancelledAt,_that.reason);case BridgeToolApprovalInteractionState_Expired() when expired != null:
return expired(_that.operationId,_that.expiredAt);case _:
  return null;

}
}

}

/// @nodoc


class BridgeToolApprovalInteractionState_Pending extends BridgeToolApprovalInteractionState {
  const BridgeToolApprovalInteractionState_Pending({required this.operationId}): super._();


@override final  String operationId;

/// Create a copy of BridgeToolApprovalInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeToolApprovalInteractionState_PendingCopyWith<BridgeToolApprovalInteractionState_Pending> get copyWith => _$BridgeToolApprovalInteractionState_PendingCopyWithImpl<BridgeToolApprovalInteractionState_Pending>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeToolApprovalInteractionState_Pending&&(identical(other.operationId, operationId) || other.operationId == operationId));
}


@override
int get hashCode {
    return Object.hash(runtimeType,operationId);
}

@override
String toString() {
    return 'BridgeToolApprovalInteractionState.pending(operationId: $operationId)';
}


}

/// @nodoc
abstract mixin class $BridgeToolApprovalInteractionState_PendingCopyWith<$Res> implements $BridgeToolApprovalInteractionStateCopyWith<$Res> {
  factory $BridgeToolApprovalInteractionState_PendingCopyWith(BridgeToolApprovalInteractionState_Pending value, $Res Function(BridgeToolApprovalInteractionState_Pending) _then) = _$BridgeToolApprovalInteractionState_PendingCopyWithImpl;
@override @useResult
$Res call({
 String operationId
});




}
/// @nodoc
class _$BridgeToolApprovalInteractionState_PendingCopyWithImpl<$Res>
    implements $BridgeToolApprovalInteractionState_PendingCopyWith<$Res> {
  _$BridgeToolApprovalInteractionState_PendingCopyWithImpl(this._self, this._then);

  final BridgeToolApprovalInteractionState_Pending _self;
  final $Res Function(BridgeToolApprovalInteractionState_Pending) _then;

/// Create a copy of BridgeToolApprovalInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? operationId = null,}) {
  return _then(BridgeToolApprovalInteractionState_Pending(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeToolApprovalInteractionState_Resolved extends BridgeToolApprovalInteractionState {
  const BridgeToolApprovalInteractionState_Resolved({required this.operationId, required this.resolvedAt, required this.decision, this.reason}): super._();


@override final  String operationId;
 final  PlatformInt64 resolvedAt;
 final  BridgeToolApprovalResolution decision;
 final  String? reason;

/// Create a copy of BridgeToolApprovalInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeToolApprovalInteractionState_ResolvedCopyWith<BridgeToolApprovalInteractionState_Resolved> get copyWith => _$BridgeToolApprovalInteractionState_ResolvedCopyWithImpl<BridgeToolApprovalInteractionState_Resolved>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeToolApprovalInteractionState_Resolved&&(identical(other.operationId, operationId) || other.operationId == operationId)&&(identical(other.resolvedAt, resolvedAt) || other.resolvedAt == resolvedAt)&&(identical(other.decision, decision) || other.decision == decision)&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode {
    return Object.hash(runtimeType,operationId,resolvedAt,decision,reason);
}

@override
String toString() {
    return 'BridgeToolApprovalInteractionState.resolved(operationId: $operationId, resolvedAt: $resolvedAt, decision: $decision, reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeToolApprovalInteractionState_ResolvedCopyWith<$Res> implements $BridgeToolApprovalInteractionStateCopyWith<$Res> {
  factory $BridgeToolApprovalInteractionState_ResolvedCopyWith(BridgeToolApprovalInteractionState_Resolved value, $Res Function(BridgeToolApprovalInteractionState_Resolved) _then) = _$BridgeToolApprovalInteractionState_ResolvedCopyWithImpl;
@override @useResult
$Res call({
 String operationId, PlatformInt64 resolvedAt, BridgeToolApprovalResolution decision, String? reason
});




}
/// @nodoc
class _$BridgeToolApprovalInteractionState_ResolvedCopyWithImpl<$Res>
    implements $BridgeToolApprovalInteractionState_ResolvedCopyWith<$Res> {
  _$BridgeToolApprovalInteractionState_ResolvedCopyWithImpl(this._self, this._then);

  final BridgeToolApprovalInteractionState_Resolved _self;
  final $Res Function(BridgeToolApprovalInteractionState_Resolved) _then;

/// Create a copy of BridgeToolApprovalInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? operationId = null,Object? resolvedAt = null,Object? decision = null,Object? reason = freezed,}) {
  return _then(BridgeToolApprovalInteractionState_Resolved(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,resolvedAt: null == resolvedAt ? _self.resolvedAt : resolvedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,decision: null == decision ? _self.decision : decision // ignore: cast_nullable_to_non_nullable
as BridgeToolApprovalResolution,reason: freezed == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

/// @nodoc


class BridgeToolApprovalInteractionState_Cancelled extends BridgeToolApprovalInteractionState {
  const BridgeToolApprovalInteractionState_Cancelled({required this.operationId, required this.cancelledAt, required this.reason}): super._();


@override final  String operationId;
 final  PlatformInt64 cancelledAt;
 final  String reason;

/// Create a copy of BridgeToolApprovalInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeToolApprovalInteractionState_CancelledCopyWith<BridgeToolApprovalInteractionState_Cancelled> get copyWith => _$BridgeToolApprovalInteractionState_CancelledCopyWithImpl<BridgeToolApprovalInteractionState_Cancelled>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeToolApprovalInteractionState_Cancelled&&(identical(other.operationId, operationId) || other.operationId == operationId)&&(identical(other.cancelledAt, cancelledAt) || other.cancelledAt == cancelledAt)&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode {
    return Object.hash(runtimeType,operationId,cancelledAt,reason);
}

@override
String toString() {
    return 'BridgeToolApprovalInteractionState.cancelled(operationId: $operationId, cancelledAt: $cancelledAt, reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeToolApprovalInteractionState_CancelledCopyWith<$Res> implements $BridgeToolApprovalInteractionStateCopyWith<$Res> {
  factory $BridgeToolApprovalInteractionState_CancelledCopyWith(BridgeToolApprovalInteractionState_Cancelled value, $Res Function(BridgeToolApprovalInteractionState_Cancelled) _then) = _$BridgeToolApprovalInteractionState_CancelledCopyWithImpl;
@override @useResult
$Res call({
 String operationId, PlatformInt64 cancelledAt, String reason
});




}
/// @nodoc
class _$BridgeToolApprovalInteractionState_CancelledCopyWithImpl<$Res>
    implements $BridgeToolApprovalInteractionState_CancelledCopyWith<$Res> {
  _$BridgeToolApprovalInteractionState_CancelledCopyWithImpl(this._self, this._then);

  final BridgeToolApprovalInteractionState_Cancelled _self;
  final $Res Function(BridgeToolApprovalInteractionState_Cancelled) _then;

/// Create a copy of BridgeToolApprovalInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? operationId = null,Object? cancelledAt = null,Object? reason = null,}) {
  return _then(BridgeToolApprovalInteractionState_Cancelled(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,cancelledAt: null == cancelledAt ? _self.cancelledAt : cancelledAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeToolApprovalInteractionState_Expired extends BridgeToolApprovalInteractionState {
  const BridgeToolApprovalInteractionState_Expired({required this.operationId, required this.expiredAt}): super._();


@override final  String operationId;
 final  PlatformInt64 expiredAt;

/// Create a copy of BridgeToolApprovalInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeToolApprovalInteractionState_ExpiredCopyWith<BridgeToolApprovalInteractionState_Expired> get copyWith => _$BridgeToolApprovalInteractionState_ExpiredCopyWithImpl<BridgeToolApprovalInteractionState_Expired>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeToolApprovalInteractionState_Expired&&(identical(other.operationId, operationId) || other.operationId == operationId)&&(identical(other.expiredAt, expiredAt) || other.expiredAt == expiredAt));
}


@override
int get hashCode {
    return Object.hash(runtimeType,operationId,expiredAt);
}

@override
String toString() {
    return 'BridgeToolApprovalInteractionState.expired(operationId: $operationId, expiredAt: $expiredAt)';
}


}

/// @nodoc
abstract mixin class $BridgeToolApprovalInteractionState_ExpiredCopyWith<$Res> implements $BridgeToolApprovalInteractionStateCopyWith<$Res> {
  factory $BridgeToolApprovalInteractionState_ExpiredCopyWith(BridgeToolApprovalInteractionState_Expired value, $Res Function(BridgeToolApprovalInteractionState_Expired) _then) = _$BridgeToolApprovalInteractionState_ExpiredCopyWithImpl;
@override @useResult
$Res call({
 String operationId, PlatformInt64 expiredAt
});




}
/// @nodoc
class _$BridgeToolApprovalInteractionState_ExpiredCopyWithImpl<$Res>
    implements $BridgeToolApprovalInteractionState_ExpiredCopyWith<$Res> {
  _$BridgeToolApprovalInteractionState_ExpiredCopyWithImpl(this._self, this._then);

  final BridgeToolApprovalInteractionState_Expired _self;
  final $Res Function(BridgeToolApprovalInteractionState_Expired) _then;

/// Create a copy of BridgeToolApprovalInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? operationId = null,Object? expiredAt = null,}) {
  return _then(BridgeToolApprovalInteractionState_Expired(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,expiredAt: null == expiredAt ? _self.expiredAt : expiredAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,
  ));
}


}

/// @nodoc
mixin _$BridgeTurnCancellationCause {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnCancellationCause);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeTurnCancellationCause()';
}


}

/// @nodoc
class $BridgeTurnCancellationCauseCopyWith<$Res>  {
$BridgeTurnCancellationCauseCopyWith(BridgeTurnCancellationCause _, $Res Function(BridgeTurnCancellationCause) __);
}


/// Adds pattern-matching-related methods to [BridgeTurnCancellationCause].
extension BridgeTurnCancellationCausePatterns on BridgeTurnCancellationCause {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTurnCancellationCause_UserRequested value)?  userRequested,TResult Function( BridgeTurnCancellationCause_RuntimeShutdown value)?  runtimeShutdown,TResult Function( BridgeTurnCancellationCause_AgentClosed value)?  agentClosed,TResult Function( BridgeTurnCancellationCause_Recovery value)?  recovery,TResult Function( BridgeTurnCancellationCause_Coalesced value)?  coalesced,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTurnCancellationCause_UserRequested() when userRequested != null:
return userRequested(_that);case BridgeTurnCancellationCause_RuntimeShutdown() when runtimeShutdown != null:
return runtimeShutdown(_that);case BridgeTurnCancellationCause_AgentClosed() when agentClosed != null:
return agentClosed(_that);case BridgeTurnCancellationCause_Recovery() when recovery != null:
return recovery(_that);case BridgeTurnCancellationCause_Coalesced() when coalesced != null:
return coalesced(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTurnCancellationCause_UserRequested value)  userRequested,required TResult Function( BridgeTurnCancellationCause_RuntimeShutdown value)  runtimeShutdown,required TResult Function( BridgeTurnCancellationCause_AgentClosed value)  agentClosed,required TResult Function( BridgeTurnCancellationCause_Recovery value)  recovery,required TResult Function( BridgeTurnCancellationCause_Coalesced value)  coalesced,}){
final _that = this;
switch (_that) {
case BridgeTurnCancellationCause_UserRequested():
return userRequested(_that);case BridgeTurnCancellationCause_RuntimeShutdown():
return runtimeShutdown(_that);case BridgeTurnCancellationCause_AgentClosed():
return agentClosed(_that);case BridgeTurnCancellationCause_Recovery():
return recovery(_that);case BridgeTurnCancellationCause_Coalesced():
return coalesced(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTurnCancellationCause_UserRequested value)?  userRequested,TResult? Function( BridgeTurnCancellationCause_RuntimeShutdown value)?  runtimeShutdown,TResult? Function( BridgeTurnCancellationCause_AgentClosed value)?  agentClosed,TResult? Function( BridgeTurnCancellationCause_Recovery value)?  recovery,TResult? Function( BridgeTurnCancellationCause_Coalesced value)?  coalesced,}){
final _that = this;
switch (_that) {
case BridgeTurnCancellationCause_UserRequested() when userRequested != null:
return userRequested(_that);case BridgeTurnCancellationCause_RuntimeShutdown() when runtimeShutdown != null:
return runtimeShutdown(_that);case BridgeTurnCancellationCause_AgentClosed() when agentClosed != null:
return agentClosed(_that);case BridgeTurnCancellationCause_Recovery() when recovery != null:
return recovery(_that);case BridgeTurnCancellationCause_Coalesced() when coalesced != null:
return coalesced(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  userRequested,TResult Function()?  runtimeShutdown,TResult Function()?  agentClosed,TResult Function()?  recovery,TResult Function( String targetTurnId)?  coalesced,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTurnCancellationCause_UserRequested() when userRequested != null:
return userRequested();case BridgeTurnCancellationCause_RuntimeShutdown() when runtimeShutdown != null:
return runtimeShutdown();case BridgeTurnCancellationCause_AgentClosed() when agentClosed != null:
return agentClosed();case BridgeTurnCancellationCause_Recovery() when recovery != null:
return recovery();case BridgeTurnCancellationCause_Coalesced() when coalesced != null:
return coalesced(_that.targetTurnId);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  userRequested,required TResult Function()  runtimeShutdown,required TResult Function()  agentClosed,required TResult Function()  recovery,required TResult Function( String targetTurnId)  coalesced,}) {final _that = this;
switch (_that) {
case BridgeTurnCancellationCause_UserRequested():
return userRequested();case BridgeTurnCancellationCause_RuntimeShutdown():
return runtimeShutdown();case BridgeTurnCancellationCause_AgentClosed():
return agentClosed();case BridgeTurnCancellationCause_Recovery():
return recovery();case BridgeTurnCancellationCause_Coalesced():
return coalesced(_that.targetTurnId);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  userRequested,TResult? Function()?  runtimeShutdown,TResult? Function()?  agentClosed,TResult? Function()?  recovery,TResult? Function( String targetTurnId)?  coalesced,}) {final _that = this;
switch (_that) {
case BridgeTurnCancellationCause_UserRequested() when userRequested != null:
return userRequested();case BridgeTurnCancellationCause_RuntimeShutdown() when runtimeShutdown != null:
return runtimeShutdown();case BridgeTurnCancellationCause_AgentClosed() when agentClosed != null:
return agentClosed();case BridgeTurnCancellationCause_Recovery() when recovery != null:
return recovery();case BridgeTurnCancellationCause_Coalesced() when coalesced != null:
return coalesced(_that.targetTurnId);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTurnCancellationCause_UserRequested extends BridgeTurnCancellationCause {
  const BridgeTurnCancellationCause_UserRequested(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnCancellationCause_UserRequested);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeTurnCancellationCause.userRequested()';
}


}




/// @nodoc


class BridgeTurnCancellationCause_RuntimeShutdown extends BridgeTurnCancellationCause {
  const BridgeTurnCancellationCause_RuntimeShutdown(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnCancellationCause_RuntimeShutdown);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeTurnCancellationCause.runtimeShutdown()';
}


}




/// @nodoc


class BridgeTurnCancellationCause_AgentClosed extends BridgeTurnCancellationCause {
  const BridgeTurnCancellationCause_AgentClosed(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnCancellationCause_AgentClosed);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeTurnCancellationCause.agentClosed()';
}


}




/// @nodoc


class BridgeTurnCancellationCause_Recovery extends BridgeTurnCancellationCause {
  const BridgeTurnCancellationCause_Recovery(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnCancellationCause_Recovery);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeTurnCancellationCause.recovery()';
}


}




/// @nodoc


class BridgeTurnCancellationCause_Coalesced extends BridgeTurnCancellationCause {
  const BridgeTurnCancellationCause_Coalesced({required this.targetTurnId}): super._();


 final  String targetTurnId;

/// Create a copy of BridgeTurnCancellationCause
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTurnCancellationCause_CoalescedCopyWith<BridgeTurnCancellationCause_Coalesced> get copyWith => _$BridgeTurnCancellationCause_CoalescedCopyWithImpl<BridgeTurnCancellationCause_Coalesced>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnCancellationCause_Coalesced&&(identical(other.targetTurnId, targetTurnId) || other.targetTurnId == targetTurnId));
}


@override
int get hashCode {
    return Object.hash(runtimeType,targetTurnId);
}

@override
String toString() {
    return 'BridgeTurnCancellationCause.coalesced(targetTurnId: $targetTurnId)';
}


}

/// @nodoc
abstract mixin class $BridgeTurnCancellationCause_CoalescedCopyWith<$Res> implements $BridgeTurnCancellationCauseCopyWith<$Res> {
  factory $BridgeTurnCancellationCause_CoalescedCopyWith(BridgeTurnCancellationCause_Coalesced value, $Res Function(BridgeTurnCancellationCause_Coalesced) _then) = _$BridgeTurnCancellationCause_CoalescedCopyWithImpl;
@useResult
$Res call({
 String targetTurnId
});




}
/// @nodoc
class _$BridgeTurnCancellationCause_CoalescedCopyWithImpl<$Res>
    implements $BridgeTurnCancellationCause_CoalescedCopyWith<$Res> {
  _$BridgeTurnCancellationCause_CoalescedCopyWithImpl(this._self, this._then);

  final BridgeTurnCancellationCause_Coalesced _self;
  final $Res Function(BridgeTurnCancellationCause_Coalesced) _then;

/// Create a copy of BridgeTurnCancellationCause
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? targetTurnId = null,}) {
  return _then(BridgeTurnCancellationCause_Coalesced(
targetTurnId: null == targetTurnId ? _self.targetTurnId : targetTurnId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeTurnRolloverOutcome {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnRolloverOutcome);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeTurnRolloverOutcome()';
}


}

/// @nodoc
class $BridgeTurnRolloverOutcomeCopyWith<$Res>  {
$BridgeTurnRolloverOutcomeCopyWith(BridgeTurnRolloverOutcome _, $Res Function(BridgeTurnRolloverOutcome) __);
}


/// Adds pattern-matching-related methods to [BridgeTurnRolloverOutcome].
extension BridgeTurnRolloverOutcomePatterns on BridgeTurnRolloverOutcome {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTurnRolloverOutcome_NotAttempted value)?  notAttempted,TResult Function( BridgeTurnRolloverOutcome_Succeeded value)?  succeeded,TResult Function( BridgeTurnRolloverOutcome_Failed value)?  failed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTurnRolloverOutcome_NotAttempted() when notAttempted != null:
return notAttempted(_that);case BridgeTurnRolloverOutcome_Succeeded() when succeeded != null:
return succeeded(_that);case BridgeTurnRolloverOutcome_Failed() when failed != null:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTurnRolloverOutcome_NotAttempted value)  notAttempted,required TResult Function( BridgeTurnRolloverOutcome_Succeeded value)  succeeded,required TResult Function( BridgeTurnRolloverOutcome_Failed value)  failed,}){
final _that = this;
switch (_that) {
case BridgeTurnRolloverOutcome_NotAttempted():
return notAttempted(_that);case BridgeTurnRolloverOutcome_Succeeded():
return succeeded(_that);case BridgeTurnRolloverOutcome_Failed():
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTurnRolloverOutcome_NotAttempted value)?  notAttempted,TResult? Function( BridgeTurnRolloverOutcome_Succeeded value)?  succeeded,TResult? Function( BridgeTurnRolloverOutcome_Failed value)?  failed,}){
final _that = this;
switch (_that) {
case BridgeTurnRolloverOutcome_NotAttempted() when notAttempted != null:
return notAttempted(_that);case BridgeTurnRolloverOutcome_Succeeded() when succeeded != null:
return succeeded(_that);case BridgeTurnRolloverOutcome_Failed() when failed != null:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  notAttempted,TResult Function()?  succeeded,TResult Function( String error)?  failed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTurnRolloverOutcome_NotAttempted() when notAttempted != null:
return notAttempted();case BridgeTurnRolloverOutcome_Succeeded() when succeeded != null:
return succeeded();case BridgeTurnRolloverOutcome_Failed() when failed != null:
return failed(_that.error);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  notAttempted,required TResult Function()  succeeded,required TResult Function( String error)  failed,}) {final _that = this;
switch (_that) {
case BridgeTurnRolloverOutcome_NotAttempted():
return notAttempted();case BridgeTurnRolloverOutcome_Succeeded():
return succeeded();case BridgeTurnRolloverOutcome_Failed():
return failed(_that.error);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  notAttempted,TResult? Function()?  succeeded,TResult? Function( String error)?  failed,}) {final _that = this;
switch (_that) {
case BridgeTurnRolloverOutcome_NotAttempted() when notAttempted != null:
return notAttempted();case BridgeTurnRolloverOutcome_Succeeded() when succeeded != null:
return succeeded();case BridgeTurnRolloverOutcome_Failed() when failed != null:
return failed(_that.error);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTurnRolloverOutcome_NotAttempted extends BridgeTurnRolloverOutcome {
  const BridgeTurnRolloverOutcome_NotAttempted(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnRolloverOutcome_NotAttempted);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeTurnRolloverOutcome.notAttempted()';
}


}




/// @nodoc


class BridgeTurnRolloverOutcome_Succeeded extends BridgeTurnRolloverOutcome {
  const BridgeTurnRolloverOutcome_Succeeded(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnRolloverOutcome_Succeeded);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeTurnRolloverOutcome.succeeded()';
}


}




/// @nodoc


class BridgeTurnRolloverOutcome_Failed extends BridgeTurnRolloverOutcome {
  const BridgeTurnRolloverOutcome_Failed({required this.error}): super._();


 final  String error;

/// Create a copy of BridgeTurnRolloverOutcome
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTurnRolloverOutcome_FailedCopyWith<BridgeTurnRolloverOutcome_Failed> get copyWith => _$BridgeTurnRolloverOutcome_FailedCopyWithImpl<BridgeTurnRolloverOutcome_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnRolloverOutcome_Failed&&(identical(other.error, error) || other.error == error));
}


@override
int get hashCode {
    return Object.hash(runtimeType,error);
}

@override
String toString() {
    return 'BridgeTurnRolloverOutcome.failed(error: $error)';
}


}

/// @nodoc
abstract mixin class $BridgeTurnRolloverOutcome_FailedCopyWith<$Res> implements $BridgeTurnRolloverOutcomeCopyWith<$Res> {
  factory $BridgeTurnRolloverOutcome_FailedCopyWith(BridgeTurnRolloverOutcome_Failed value, $Res Function(BridgeTurnRolloverOutcome_Failed) _then) = _$BridgeTurnRolloverOutcome_FailedCopyWithImpl;
@useResult
$Res call({
 String error
});




}
/// @nodoc
class _$BridgeTurnRolloverOutcome_FailedCopyWithImpl<$Res>
    implements $BridgeTurnRolloverOutcome_FailedCopyWith<$Res> {
  _$BridgeTurnRolloverOutcome_FailedCopyWithImpl(this._self, this._then);

  final BridgeTurnRolloverOutcome_Failed _self;
  final $Res Function(BridgeTurnRolloverOutcome_Failed) _then;

/// Create a copy of BridgeTurnRolloverOutcome
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? error = null,}) {
  return _then(BridgeTurnRolloverOutcome_Failed(
error: null == error ? _self.error : error // ignore: cast_nullable_to_non_nullable
as String,
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeTurnState_Queued value)?  queued,TResult Function( BridgeTurnState_Running value)?  running,TResult Function( BridgeTurnState_Completed value)?  completed,TResult Function( BridgeTurnState_Cancelled value)?  cancelled,TResult Function( BridgeTurnState_Failed value)?  failed,TResult Function( BridgeTurnState_BudgetLimited value)?  budgetLimited,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeTurnState_Queued() when queued != null:
return queued(_that);case BridgeTurnState_Running() when running != null:
return running(_that);case BridgeTurnState_Completed() when completed != null:
return completed(_that);case BridgeTurnState_Cancelled() when cancelled != null:
return cancelled(_that);case BridgeTurnState_Failed() when failed != null:
return failed(_that);case BridgeTurnState_BudgetLimited() when budgetLimited != null:
return budgetLimited(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeTurnState_Queued value)  queued,required TResult Function( BridgeTurnState_Running value)  running,required TResult Function( BridgeTurnState_Completed value)  completed,required TResult Function( BridgeTurnState_Cancelled value)  cancelled,required TResult Function( BridgeTurnState_Failed value)  failed,required TResult Function( BridgeTurnState_BudgetLimited value)  budgetLimited,}){
final _that = this;
switch (_that) {
case BridgeTurnState_Queued():
return queued(_that);case BridgeTurnState_Running():
return running(_that);case BridgeTurnState_Completed():
return completed(_that);case BridgeTurnState_Cancelled():
return cancelled(_that);case BridgeTurnState_Failed():
return failed(_that);case BridgeTurnState_BudgetLimited():
return budgetLimited(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeTurnState_Queued value)?  queued,TResult? Function( BridgeTurnState_Running value)?  running,TResult? Function( BridgeTurnState_Completed value)?  completed,TResult? Function( BridgeTurnState_Cancelled value)?  cancelled,TResult? Function( BridgeTurnState_Failed value)?  failed,TResult? Function( BridgeTurnState_BudgetLimited value)?  budgetLimited,}){
final _that = this;
switch (_that) {
case BridgeTurnState_Queued() when queued != null:
return queued(_that);case BridgeTurnState_Running() when running != null:
return running(_that);case BridgeTurnState_Completed() when completed != null:
return completed(_that);case BridgeTurnState_Cancelled() when cancelled != null:
return cancelled(_that);case BridgeTurnState_Failed() when failed != null:
return failed(_that);case BridgeTurnState_BudgetLimited() when budgetLimited != null:
return budgetLimited(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( PlatformInt64 queuedAt)?  queued,TResult Function( PlatformInt64 startedAt,  BridgeTurnPhase phase)?  running,TResult Function( PlatformInt64? startedAt,  PlatformInt64 completedAt,  BridgeTurnCompletion completion)?  completed,TResult Function( PlatformInt64? startedAt,  PlatformInt64 requestedAt,  PlatformInt64 completedAt,  BridgeTurnCancellationCause cause)?  cancelled,TResult Function( PlatformInt64? startedAt,  PlatformInt64 completedAt,  BridgeTurnFailureDto failure)?  failed,TResult Function( PlatformInt64? startedAt,  PlatformInt64 completedAt,  BridgeTurnBudgetLimit limit,  BridgeTurnRolloverOutcome rollover)?  budgetLimited,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeTurnState_Queued() when queued != null:
return queued(_that.queuedAt);case BridgeTurnState_Running() when running != null:
return running(_that.startedAt,_that.phase);case BridgeTurnState_Completed() when completed != null:
return completed(_that.startedAt,_that.completedAt,_that.completion);case BridgeTurnState_Cancelled() when cancelled != null:
return cancelled(_that.startedAt,_that.requestedAt,_that.completedAt,_that.cause);case BridgeTurnState_Failed() when failed != null:
return failed(_that.startedAt,_that.completedAt,_that.failure);case BridgeTurnState_BudgetLimited() when budgetLimited != null:
return budgetLimited(_that.startedAt,_that.completedAt,_that.limit,_that.rollover);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( PlatformInt64 queuedAt)  queued,required TResult Function( PlatformInt64 startedAt,  BridgeTurnPhase phase)  running,required TResult Function( PlatformInt64? startedAt,  PlatformInt64 completedAt,  BridgeTurnCompletion completion)  completed,required TResult Function( PlatformInt64? startedAt,  PlatformInt64 requestedAt,  PlatformInt64 completedAt,  BridgeTurnCancellationCause cause)  cancelled,required TResult Function( PlatformInt64? startedAt,  PlatformInt64 completedAt,  BridgeTurnFailureDto failure)  failed,required TResult Function( PlatformInt64? startedAt,  PlatformInt64 completedAt,  BridgeTurnBudgetLimit limit,  BridgeTurnRolloverOutcome rollover)  budgetLimited,}) {final _that = this;
switch (_that) {
case BridgeTurnState_Queued():
return queued(_that.queuedAt);case BridgeTurnState_Running():
return running(_that.startedAt,_that.phase);case BridgeTurnState_Completed():
return completed(_that.startedAt,_that.completedAt,_that.completion);case BridgeTurnState_Cancelled():
return cancelled(_that.startedAt,_that.requestedAt,_that.completedAt,_that.cause);case BridgeTurnState_Failed():
return failed(_that.startedAt,_that.completedAt,_that.failure);case BridgeTurnState_BudgetLimited():
return budgetLimited(_that.startedAt,_that.completedAt,_that.limit,_that.rollover);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( PlatformInt64 queuedAt)?  queued,TResult? Function( PlatformInt64 startedAt,  BridgeTurnPhase phase)?  running,TResult? Function( PlatformInt64? startedAt,  PlatformInt64 completedAt,  BridgeTurnCompletion completion)?  completed,TResult? Function( PlatformInt64? startedAt,  PlatformInt64 requestedAt,  PlatformInt64 completedAt,  BridgeTurnCancellationCause cause)?  cancelled,TResult? Function( PlatformInt64? startedAt,  PlatformInt64 completedAt,  BridgeTurnFailureDto failure)?  failed,TResult? Function( PlatformInt64? startedAt,  PlatformInt64 completedAt,  BridgeTurnBudgetLimit limit,  BridgeTurnRolloverOutcome rollover)?  budgetLimited,}) {final _that = this;
switch (_that) {
case BridgeTurnState_Queued() when queued != null:
return queued(_that.queuedAt);case BridgeTurnState_Running() when running != null:
return running(_that.startedAt,_that.phase);case BridgeTurnState_Completed() when completed != null:
return completed(_that.startedAt,_that.completedAt,_that.completion);case BridgeTurnState_Cancelled() when cancelled != null:
return cancelled(_that.startedAt,_that.requestedAt,_that.completedAt,_that.cause);case BridgeTurnState_Failed() when failed != null:
return failed(_that.startedAt,_that.completedAt,_that.failure);case BridgeTurnState_BudgetLimited() when budgetLimited != null:
return budgetLimited(_that.startedAt,_that.completedAt,_that.limit,_that.rollover);case _:
  return null;

}
}

}

/// @nodoc


class BridgeTurnState_Queued extends BridgeTurnState {
  const BridgeTurnState_Queued({required this.queuedAt}): super._();


 final  PlatformInt64 queuedAt;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTurnState_QueuedCopyWith<BridgeTurnState_Queued> get copyWith => _$BridgeTurnState_QueuedCopyWithImpl<BridgeTurnState_Queued>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnState_Queued&&(identical(other.queuedAt, queuedAt) || other.queuedAt == queuedAt));
}


@override
int get hashCode {
    return Object.hash(runtimeType,queuedAt);
}

@override
String toString() {
    return 'BridgeTurnState.queued(queuedAt: $queuedAt)';
}


}

/// @nodoc
abstract mixin class $BridgeTurnState_QueuedCopyWith<$Res> implements $BridgeTurnStateCopyWith<$Res> {
  factory $BridgeTurnState_QueuedCopyWith(BridgeTurnState_Queued value, $Res Function(BridgeTurnState_Queued) _then) = _$BridgeTurnState_QueuedCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 queuedAt
});




}
/// @nodoc
class _$BridgeTurnState_QueuedCopyWithImpl<$Res>
    implements $BridgeTurnState_QueuedCopyWith<$Res> {
  _$BridgeTurnState_QueuedCopyWithImpl(this._self, this._then);

  final BridgeTurnState_Queued _self;
  final $Res Function(BridgeTurnState_Queued) _then;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? queuedAt = null,}) {
  return _then(BridgeTurnState_Queued(
queuedAt: null == queuedAt ? _self.queuedAt : queuedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,
  ));
}


}

/// @nodoc


class BridgeTurnState_Running extends BridgeTurnState {
  const BridgeTurnState_Running({required this.startedAt, required this.phase}): super._();


 final  PlatformInt64 startedAt;
 final  BridgeTurnPhase phase;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTurnState_RunningCopyWith<BridgeTurnState_Running> get copyWith => _$BridgeTurnState_RunningCopyWithImpl<BridgeTurnState_Running>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnState_Running&&(identical(other.startedAt, startedAt) || other.startedAt == startedAt)&&(identical(other.phase, phase) || other.phase == phase));
}


@override
int get hashCode {
    return Object.hash(runtimeType,startedAt,phase);
}

@override
String toString() {
    return 'BridgeTurnState.running(startedAt: $startedAt, phase: $phase)';
}


}

/// @nodoc
abstract mixin class $BridgeTurnState_RunningCopyWith<$Res> implements $BridgeTurnStateCopyWith<$Res> {
  factory $BridgeTurnState_RunningCopyWith(BridgeTurnState_Running value, $Res Function(BridgeTurnState_Running) _then) = _$BridgeTurnState_RunningCopyWithImpl;
@useResult
$Res call({
 PlatformInt64 startedAt, BridgeTurnPhase phase
});




}
/// @nodoc
class _$BridgeTurnState_RunningCopyWithImpl<$Res>
    implements $BridgeTurnState_RunningCopyWith<$Res> {
  _$BridgeTurnState_RunningCopyWithImpl(this._self, this._then);

  final BridgeTurnState_Running _self;
  final $Res Function(BridgeTurnState_Running) _then;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? startedAt = null,Object? phase = null,}) {
  return _then(BridgeTurnState_Running(
startedAt: null == startedAt ? _self.startedAt : startedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,phase: null == phase ? _self.phase : phase // ignore: cast_nullable_to_non_nullable
as BridgeTurnPhase,
  ));
}


}

/// @nodoc


class BridgeTurnState_Completed extends BridgeTurnState {
  const BridgeTurnState_Completed({this.startedAt, required this.completedAt, required this.completion}): super._();


 final  PlatformInt64? startedAt;
 final  PlatformInt64 completedAt;
 final  BridgeTurnCompletion completion;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTurnState_CompletedCopyWith<BridgeTurnState_Completed> get copyWith => _$BridgeTurnState_CompletedCopyWithImpl<BridgeTurnState_Completed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnState_Completed&&(identical(other.startedAt, startedAt) || other.startedAt == startedAt)&&(identical(other.completedAt, completedAt) || other.completedAt == completedAt)&&(identical(other.completion, completion) || other.completion == completion));
}


@override
int get hashCode {
    return Object.hash(runtimeType,startedAt,completedAt,completion);
}

@override
String toString() {
    return 'BridgeTurnState.completed(startedAt: $startedAt, completedAt: $completedAt, completion: $completion)';
}


}

/// @nodoc
abstract mixin class $BridgeTurnState_CompletedCopyWith<$Res> implements $BridgeTurnStateCopyWith<$Res> {
  factory $BridgeTurnState_CompletedCopyWith(BridgeTurnState_Completed value, $Res Function(BridgeTurnState_Completed) _then) = _$BridgeTurnState_CompletedCopyWithImpl;
@useResult
$Res call({
 PlatformInt64? startedAt, PlatformInt64 completedAt, BridgeTurnCompletion completion
});




}
/// @nodoc
class _$BridgeTurnState_CompletedCopyWithImpl<$Res>
    implements $BridgeTurnState_CompletedCopyWith<$Res> {
  _$BridgeTurnState_CompletedCopyWithImpl(this._self, this._then);

  final BridgeTurnState_Completed _self;
  final $Res Function(BridgeTurnState_Completed) _then;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? startedAt = freezed,Object? completedAt = null,Object? completion = null,}) {
  return _then(BridgeTurnState_Completed(
startedAt: freezed == startedAt ? _self.startedAt : startedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64?,completedAt: null == completedAt ? _self.completedAt : completedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,completion: null == completion ? _self.completion : completion // ignore: cast_nullable_to_non_nullable
as BridgeTurnCompletion,
  ));
}


}

/// @nodoc


class BridgeTurnState_Cancelled extends BridgeTurnState {
  const BridgeTurnState_Cancelled({this.startedAt, required this.requestedAt, required this.completedAt, required this.cause}): super._();


 final  PlatformInt64? startedAt;
 final  PlatformInt64 requestedAt;
 final  PlatformInt64 completedAt;
 final  BridgeTurnCancellationCause cause;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTurnState_CancelledCopyWith<BridgeTurnState_Cancelled> get copyWith => _$BridgeTurnState_CancelledCopyWithImpl<BridgeTurnState_Cancelled>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnState_Cancelled&&(identical(other.startedAt, startedAt) || other.startedAt == startedAt)&&(identical(other.requestedAt, requestedAt) || other.requestedAt == requestedAt)&&(identical(other.completedAt, completedAt) || other.completedAt == completedAt)&&(identical(other.cause, cause) || other.cause == cause));
}


@override
int get hashCode {
    return Object.hash(runtimeType,startedAt,requestedAt,completedAt,cause);
}

@override
String toString() {
    return 'BridgeTurnState.cancelled(startedAt: $startedAt, requestedAt: $requestedAt, completedAt: $completedAt, cause: $cause)';
}


}

/// @nodoc
abstract mixin class $BridgeTurnState_CancelledCopyWith<$Res> implements $BridgeTurnStateCopyWith<$Res> {
  factory $BridgeTurnState_CancelledCopyWith(BridgeTurnState_Cancelled value, $Res Function(BridgeTurnState_Cancelled) _then) = _$BridgeTurnState_CancelledCopyWithImpl;
@useResult
$Res call({
 PlatformInt64? startedAt, PlatformInt64 requestedAt, PlatformInt64 completedAt, BridgeTurnCancellationCause cause
});


$BridgeTurnCancellationCauseCopyWith<$Res> get cause;

}
/// @nodoc
class _$BridgeTurnState_CancelledCopyWithImpl<$Res>
    implements $BridgeTurnState_CancelledCopyWith<$Res> {
  _$BridgeTurnState_CancelledCopyWithImpl(this._self, this._then);

  final BridgeTurnState_Cancelled _self;
  final $Res Function(BridgeTurnState_Cancelled) _then;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? startedAt = freezed,Object? requestedAt = null,Object? completedAt = null,Object? cause = null,}) {
  return _then(BridgeTurnState_Cancelled(
startedAt: freezed == startedAt ? _self.startedAt : startedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64?,requestedAt: null == requestedAt ? _self.requestedAt : requestedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,completedAt: null == completedAt ? _self.completedAt : completedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,cause: null == cause ? _self.cause : cause // ignore: cast_nullable_to_non_nullable
as BridgeTurnCancellationCause,
  ));
}

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeTurnCancellationCauseCopyWith<$Res> get cause {

  return $BridgeTurnCancellationCauseCopyWith<$Res>(_self.cause, (value) {
    return _then(_self.copyWith(cause: value));
  });
}
}

/// @nodoc


class BridgeTurnState_Failed extends BridgeTurnState {
  const BridgeTurnState_Failed({this.startedAt, required this.completedAt, required this.failure}): super._();


 final  PlatformInt64? startedAt;
 final  PlatformInt64 completedAt;
 final  BridgeTurnFailureDto failure;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTurnState_FailedCopyWith<BridgeTurnState_Failed> get copyWith => _$BridgeTurnState_FailedCopyWithImpl<BridgeTurnState_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnState_Failed&&(identical(other.startedAt, startedAt) || other.startedAt == startedAt)&&(identical(other.completedAt, completedAt) || other.completedAt == completedAt)&&(identical(other.failure, failure) || other.failure == failure));
}


@override
int get hashCode {
    return Object.hash(runtimeType,startedAt,completedAt,failure);
}

@override
String toString() {
    return 'BridgeTurnState.failed(startedAt: $startedAt, completedAt: $completedAt, failure: $failure)';
}


}

/// @nodoc
abstract mixin class $BridgeTurnState_FailedCopyWith<$Res> implements $BridgeTurnStateCopyWith<$Res> {
  factory $BridgeTurnState_FailedCopyWith(BridgeTurnState_Failed value, $Res Function(BridgeTurnState_Failed) _then) = _$BridgeTurnState_FailedCopyWithImpl;
@useResult
$Res call({
 PlatformInt64? startedAt, PlatformInt64 completedAt, BridgeTurnFailureDto failure
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
@pragma('vm:prefer-inline') $Res call({Object? startedAt = freezed,Object? completedAt = null,Object? failure = null,}) {
  return _then(BridgeTurnState_Failed(
startedAt: freezed == startedAt ? _self.startedAt : startedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64?,completedAt: null == completedAt ? _self.completedAt : completedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,failure: null == failure ? _self.failure : failure // ignore: cast_nullable_to_non_nullable
as BridgeTurnFailureDto,
  ));
}


}

/// @nodoc


class BridgeTurnState_BudgetLimited extends BridgeTurnState {
  const BridgeTurnState_BudgetLimited({this.startedAt, required this.completedAt, required this.limit, required this.rollover}): super._();


 final  PlatformInt64? startedAt;
 final  PlatformInt64 completedAt;
 final  BridgeTurnBudgetLimit limit;
 final  BridgeTurnRolloverOutcome rollover;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeTurnState_BudgetLimitedCopyWith<BridgeTurnState_BudgetLimited> get copyWith => _$BridgeTurnState_BudgetLimitedCopyWithImpl<BridgeTurnState_BudgetLimited>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeTurnState_BudgetLimited&&(identical(other.startedAt, startedAt) || other.startedAt == startedAt)&&(identical(other.completedAt, completedAt) || other.completedAt == completedAt)&&(identical(other.limit, limit) || other.limit == limit)&&(identical(other.rollover, rollover) || other.rollover == rollover));
}


@override
int get hashCode {
    return Object.hash(runtimeType,startedAt,completedAt,limit,rollover);
}

@override
String toString() {
    return 'BridgeTurnState.budgetLimited(startedAt: $startedAt, completedAt: $completedAt, limit: $limit, rollover: $rollover)';
}


}

/// @nodoc
abstract mixin class $BridgeTurnState_BudgetLimitedCopyWith<$Res> implements $BridgeTurnStateCopyWith<$Res> {
  factory $BridgeTurnState_BudgetLimitedCopyWith(BridgeTurnState_BudgetLimited value, $Res Function(BridgeTurnState_BudgetLimited) _then) = _$BridgeTurnState_BudgetLimitedCopyWithImpl;
@useResult
$Res call({
 PlatformInt64? startedAt, PlatformInt64 completedAt, BridgeTurnBudgetLimit limit, BridgeTurnRolloverOutcome rollover
});


$BridgeTurnRolloverOutcomeCopyWith<$Res> get rollover;

}
/// @nodoc
class _$BridgeTurnState_BudgetLimitedCopyWithImpl<$Res>
    implements $BridgeTurnState_BudgetLimitedCopyWith<$Res> {
  _$BridgeTurnState_BudgetLimitedCopyWithImpl(this._self, this._then);

  final BridgeTurnState_BudgetLimited _self;
  final $Res Function(BridgeTurnState_BudgetLimited) _then;

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? startedAt = freezed,Object? completedAt = null,Object? limit = null,Object? rollover = null,}) {
  return _then(BridgeTurnState_BudgetLimited(
startedAt: freezed == startedAt ? _self.startedAt : startedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64?,completedAt: null == completedAt ? _self.completedAt : completedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,limit: null == limit ? _self.limit : limit // ignore: cast_nullable_to_non_nullable
as BridgeTurnBudgetLimit,rollover: null == rollover ? _self.rollover : rollover // ignore: cast_nullable_to_non_nullable
as BridgeTurnRolloverOutcome,
  ));
}

/// Create a copy of BridgeTurnState
/// with the given fields replaced by the non-null parameter values.
@override
@pragma('vm:prefer-inline')
$BridgeTurnRolloverOutcomeCopyWith<$Res> get rollover {

  return $BridgeTurnRolloverOutcomeCopyWith<$Res>(_self.rollover, (value) {
    return _then(_self.copyWith(rollover: value));
  });
}
}

/// @nodoc
mixin _$BridgeUserInputInteractionState {

 String get operationId;
/// Create a copy of BridgeUserInputInteractionState
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUserInputInteractionStateCopyWith<BridgeUserInputInteractionState> get copyWith => _$BridgeUserInputInteractionStateCopyWithImpl<BridgeUserInputInteractionState>(this as BridgeUserInputInteractionState, _$identity);



@override
bool operator ==(Object other) {
  final _this = this as BridgeUserInputInteractionState;
  return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUserInputInteractionState&&(identical(other.operationId, _this.operationId) || other.operationId == _this.operationId));
}


@override
int get hashCode {
  final _this = this as BridgeUserInputInteractionState;
  return Object.hash(runtimeType,_this.operationId);
}

@override
String toString() {
  final _this = this as BridgeUserInputInteractionState;
  return 'BridgeUserInputInteractionState(operationId: ${_this.operationId})';
}


}

/// @nodoc
abstract mixin class $BridgeUserInputInteractionStateCopyWith<$Res>  {
  factory $BridgeUserInputInteractionStateCopyWith(BridgeUserInputInteractionState value, $Res Function(BridgeUserInputInteractionState) _then) = _$BridgeUserInputInteractionStateCopyWithImpl;
@useResult
$Res call({
 String operationId
});




}
/// @nodoc
class _$BridgeUserInputInteractionStateCopyWithImpl<$Res>
    implements $BridgeUserInputInteractionStateCopyWith<$Res> {
  _$BridgeUserInputInteractionStateCopyWithImpl(this._self, this._then);

  final BridgeUserInputInteractionState _self;
  final $Res Function(BridgeUserInputInteractionState) _then;

/// Create a copy of BridgeUserInputInteractionState
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') @override $Res call({Object? operationId = null,}) {
  return _then(_self.copyWith(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}

}


/// Adds pattern-matching-related methods to [BridgeUserInputInteractionState].
extension BridgeUserInputInteractionStatePatterns on BridgeUserInputInteractionState {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeUserInputInteractionState_Pending value)?  pending,TResult Function( BridgeUserInputInteractionState_Resolved value)?  resolved,TResult Function( BridgeUserInputInteractionState_Cancelled value)?  cancelled,TResult Function( BridgeUserInputInteractionState_Expired value)?  expired,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeUserInputInteractionState_Pending() when pending != null:
return pending(_that);case BridgeUserInputInteractionState_Resolved() when resolved != null:
return resolved(_that);case BridgeUserInputInteractionState_Cancelled() when cancelled != null:
return cancelled(_that);case BridgeUserInputInteractionState_Expired() when expired != null:
return expired(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeUserInputInteractionState_Pending value)  pending,required TResult Function( BridgeUserInputInteractionState_Resolved value)  resolved,required TResult Function( BridgeUserInputInteractionState_Cancelled value)  cancelled,required TResult Function( BridgeUserInputInteractionState_Expired value)  expired,}){
final _that = this;
switch (_that) {
case BridgeUserInputInteractionState_Pending():
return pending(_that);case BridgeUserInputInteractionState_Resolved():
return resolved(_that);case BridgeUserInputInteractionState_Cancelled():
return cancelled(_that);case BridgeUserInputInteractionState_Expired():
return expired(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeUserInputInteractionState_Pending value)?  pending,TResult? Function( BridgeUserInputInteractionState_Resolved value)?  resolved,TResult? Function( BridgeUserInputInteractionState_Cancelled value)?  cancelled,TResult? Function( BridgeUserInputInteractionState_Expired value)?  expired,}){
final _that = this;
switch (_that) {
case BridgeUserInputInteractionState_Pending() when pending != null:
return pending(_that);case BridgeUserInputInteractionState_Resolved() when resolved != null:
return resolved(_that);case BridgeUserInputInteractionState_Cancelled() when cancelled != null:
return cancelled(_that);case BridgeUserInputInteractionState_Expired() when expired != null:
return expired(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( String operationId)?  pending,TResult Function( String operationId,  PlatformInt64 resolvedAt,  List<BridgeUserInputAnswer> answers)?  resolved,TResult Function( String operationId,  PlatformInt64 cancelledAt,  String reason)?  cancelled,TResult Function( String operationId,  PlatformInt64 expiredAt)?  expired,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeUserInputInteractionState_Pending() when pending != null:
return pending(_that.operationId);case BridgeUserInputInteractionState_Resolved() when resolved != null:
return resolved(_that.operationId,_that.resolvedAt,_that.answers);case BridgeUserInputInteractionState_Cancelled() when cancelled != null:
return cancelled(_that.operationId,_that.cancelledAt,_that.reason);case BridgeUserInputInteractionState_Expired() when expired != null:
return expired(_that.operationId,_that.expiredAt);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( String operationId)  pending,required TResult Function( String operationId,  PlatformInt64 resolvedAt,  List<BridgeUserInputAnswer> answers)  resolved,required TResult Function( String operationId,  PlatformInt64 cancelledAt,  String reason)  cancelled,required TResult Function( String operationId,  PlatformInt64 expiredAt)  expired,}) {final _that = this;
switch (_that) {
case BridgeUserInputInteractionState_Pending():
return pending(_that.operationId);case BridgeUserInputInteractionState_Resolved():
return resolved(_that.operationId,_that.resolvedAt,_that.answers);case BridgeUserInputInteractionState_Cancelled():
return cancelled(_that.operationId,_that.cancelledAt,_that.reason);case BridgeUserInputInteractionState_Expired():
return expired(_that.operationId,_that.expiredAt);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( String operationId)?  pending,TResult? Function( String operationId,  PlatformInt64 resolvedAt,  List<BridgeUserInputAnswer> answers)?  resolved,TResult? Function( String operationId,  PlatformInt64 cancelledAt,  String reason)?  cancelled,TResult? Function( String operationId,  PlatformInt64 expiredAt)?  expired,}) {final _that = this;
switch (_that) {
case BridgeUserInputInteractionState_Pending() when pending != null:
return pending(_that.operationId);case BridgeUserInputInteractionState_Resolved() when resolved != null:
return resolved(_that.operationId,_that.resolvedAt,_that.answers);case BridgeUserInputInteractionState_Cancelled() when cancelled != null:
return cancelled(_that.operationId,_that.cancelledAt,_that.reason);case BridgeUserInputInteractionState_Expired() when expired != null:
return expired(_that.operationId,_that.expiredAt);case _:
  return null;

}
}

}

/// @nodoc


class BridgeUserInputInteractionState_Pending extends BridgeUserInputInteractionState {
  const BridgeUserInputInteractionState_Pending({required this.operationId}): super._();


@override final  String operationId;

/// Create a copy of BridgeUserInputInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUserInputInteractionState_PendingCopyWith<BridgeUserInputInteractionState_Pending> get copyWith => _$BridgeUserInputInteractionState_PendingCopyWithImpl<BridgeUserInputInteractionState_Pending>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUserInputInteractionState_Pending&&(identical(other.operationId, operationId) || other.operationId == operationId));
}


@override
int get hashCode {
    return Object.hash(runtimeType,operationId);
}

@override
String toString() {
    return 'BridgeUserInputInteractionState.pending(operationId: $operationId)';
}


}

/// @nodoc
abstract mixin class $BridgeUserInputInteractionState_PendingCopyWith<$Res> implements $BridgeUserInputInteractionStateCopyWith<$Res> {
  factory $BridgeUserInputInteractionState_PendingCopyWith(BridgeUserInputInteractionState_Pending value, $Res Function(BridgeUserInputInteractionState_Pending) _then) = _$BridgeUserInputInteractionState_PendingCopyWithImpl;
@override @useResult
$Res call({
 String operationId
});




}
/// @nodoc
class _$BridgeUserInputInteractionState_PendingCopyWithImpl<$Res>
    implements $BridgeUserInputInteractionState_PendingCopyWith<$Res> {
  _$BridgeUserInputInteractionState_PendingCopyWithImpl(this._self, this._then);

  final BridgeUserInputInteractionState_Pending _self;
  final $Res Function(BridgeUserInputInteractionState_Pending) _then;

/// Create a copy of BridgeUserInputInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? operationId = null,}) {
  return _then(BridgeUserInputInteractionState_Pending(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeUserInputInteractionState_Resolved extends BridgeUserInputInteractionState {
  const BridgeUserInputInteractionState_Resolved({required this.operationId, required this.resolvedAt, required  List<BridgeUserInputAnswer> answers}): _answers = answers,super._();


@override final  String operationId;
 final  PlatformInt64 resolvedAt;
 final  List<BridgeUserInputAnswer> _answers;
 List<BridgeUserInputAnswer> get answers {
  if (_answers is EqualUnmodifiableListView) return _answers;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_answers);
}


/// Create a copy of BridgeUserInputInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUserInputInteractionState_ResolvedCopyWith<BridgeUserInputInteractionState_Resolved> get copyWith => _$BridgeUserInputInteractionState_ResolvedCopyWithImpl<BridgeUserInputInteractionState_Resolved>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUserInputInteractionState_Resolved&&(identical(other.operationId, operationId) || other.operationId == operationId)&&(identical(other.resolvedAt, resolvedAt) || other.resolvedAt == resolvedAt)&&const DeepCollectionEquality().equals(other.answers, _answers));
}


@override
int get hashCode {
    return Object.hash(runtimeType,operationId,resolvedAt,const DeepCollectionEquality().hash(_answers));
}

@override
String toString() {
    return 'BridgeUserInputInteractionState.resolved(operationId: $operationId, resolvedAt: $resolvedAt, answers: $answers)';
}


}

/// @nodoc
abstract mixin class $BridgeUserInputInteractionState_ResolvedCopyWith<$Res> implements $BridgeUserInputInteractionStateCopyWith<$Res> {
  factory $BridgeUserInputInteractionState_ResolvedCopyWith(BridgeUserInputInteractionState_Resolved value, $Res Function(BridgeUserInputInteractionState_Resolved) _then) = _$BridgeUserInputInteractionState_ResolvedCopyWithImpl;
@override @useResult
$Res call({
 String operationId, PlatformInt64 resolvedAt, List<BridgeUserInputAnswer> answers
});




}
/// @nodoc
class _$BridgeUserInputInteractionState_ResolvedCopyWithImpl<$Res>
    implements $BridgeUserInputInteractionState_ResolvedCopyWith<$Res> {
  _$BridgeUserInputInteractionState_ResolvedCopyWithImpl(this._self, this._then);

  final BridgeUserInputInteractionState_Resolved _self;
  final $Res Function(BridgeUserInputInteractionState_Resolved) _then;

/// Create a copy of BridgeUserInputInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? operationId = null,Object? resolvedAt = null,Object? answers = null,}) {
  return _then(BridgeUserInputInteractionState_Resolved(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,resolvedAt: null == resolvedAt ? _self.resolvedAt : resolvedAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,answers: null == answers ? _self._answers : answers // ignore: cast_nullable_to_non_nullable
as List<BridgeUserInputAnswer>,
  ));
}


}

/// @nodoc


class BridgeUserInputInteractionState_Cancelled extends BridgeUserInputInteractionState {
  const BridgeUserInputInteractionState_Cancelled({required this.operationId, required this.cancelledAt, required this.reason}): super._();


@override final  String operationId;
 final  PlatformInt64 cancelledAt;
 final  String reason;

/// Create a copy of BridgeUserInputInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUserInputInteractionState_CancelledCopyWith<BridgeUserInputInteractionState_Cancelled> get copyWith => _$BridgeUserInputInteractionState_CancelledCopyWithImpl<BridgeUserInputInteractionState_Cancelled>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUserInputInteractionState_Cancelled&&(identical(other.operationId, operationId) || other.operationId == operationId)&&(identical(other.cancelledAt, cancelledAt) || other.cancelledAt == cancelledAt)&&(identical(other.reason, reason) || other.reason == reason));
}


@override
int get hashCode {
    return Object.hash(runtimeType,operationId,cancelledAt,reason);
}

@override
String toString() {
    return 'BridgeUserInputInteractionState.cancelled(operationId: $operationId, cancelledAt: $cancelledAt, reason: $reason)';
}


}

/// @nodoc
abstract mixin class $BridgeUserInputInteractionState_CancelledCopyWith<$Res> implements $BridgeUserInputInteractionStateCopyWith<$Res> {
  factory $BridgeUserInputInteractionState_CancelledCopyWith(BridgeUserInputInteractionState_Cancelled value, $Res Function(BridgeUserInputInteractionState_Cancelled) _then) = _$BridgeUserInputInteractionState_CancelledCopyWithImpl;
@override @useResult
$Res call({
 String operationId, PlatformInt64 cancelledAt, String reason
});




}
/// @nodoc
class _$BridgeUserInputInteractionState_CancelledCopyWithImpl<$Res>
    implements $BridgeUserInputInteractionState_CancelledCopyWith<$Res> {
  _$BridgeUserInputInteractionState_CancelledCopyWithImpl(this._self, this._then);

  final BridgeUserInputInteractionState_Cancelled _self;
  final $Res Function(BridgeUserInputInteractionState_Cancelled) _then;

/// Create a copy of BridgeUserInputInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? operationId = null,Object? cancelledAt = null,Object? reason = null,}) {
  return _then(BridgeUserInputInteractionState_Cancelled(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,cancelledAt: null == cancelledAt ? _self.cancelledAt : cancelledAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,reason: null == reason ? _self.reason : reason // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeUserInputInteractionState_Expired extends BridgeUserInputInteractionState {
  const BridgeUserInputInteractionState_Expired({required this.operationId, required this.expiredAt}): super._();


@override final  String operationId;
 final  PlatformInt64 expiredAt;

/// Create a copy of BridgeUserInputInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeUserInputInteractionState_ExpiredCopyWith<BridgeUserInputInteractionState_Expired> get copyWith => _$BridgeUserInputInteractionState_ExpiredCopyWithImpl<BridgeUserInputInteractionState_Expired>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeUserInputInteractionState_Expired&&(identical(other.operationId, operationId) || other.operationId == operationId)&&(identical(other.expiredAt, expiredAt) || other.expiredAt == expiredAt));
}


@override
int get hashCode {
    return Object.hash(runtimeType,operationId,expiredAt);
}

@override
String toString() {
    return 'BridgeUserInputInteractionState.expired(operationId: $operationId, expiredAt: $expiredAt)';
}


}

/// @nodoc
abstract mixin class $BridgeUserInputInteractionState_ExpiredCopyWith<$Res> implements $BridgeUserInputInteractionStateCopyWith<$Res> {
  factory $BridgeUserInputInteractionState_ExpiredCopyWith(BridgeUserInputInteractionState_Expired value, $Res Function(BridgeUserInputInteractionState_Expired) _then) = _$BridgeUserInputInteractionState_ExpiredCopyWithImpl;
@override @useResult
$Res call({
 String operationId, PlatformInt64 expiredAt
});




}
/// @nodoc
class _$BridgeUserInputInteractionState_ExpiredCopyWithImpl<$Res>
    implements $BridgeUserInputInteractionState_ExpiredCopyWith<$Res> {
  _$BridgeUserInputInteractionState_ExpiredCopyWithImpl(this._self, this._then);

  final BridgeUserInputInteractionState_Expired _self;
  final $Res Function(BridgeUserInputInteractionState_Expired) _then;

/// Create a copy of BridgeUserInputInteractionState
/// with the given fields replaced by the non-null parameter values.
@override @pragma('vm:prefer-inline') $Res call({Object? operationId = null,Object? expiredAt = null,}) {
  return _then(BridgeUserInputInteractionState_Expired(
operationId: null == operationId ? _self.operationId : operationId // ignore: cast_nullable_to_non_nullable
as String,expiredAt: null == expiredAt ? _self.expiredAt : expiredAt // ignore: cast_nullable_to_non_nullable
as PlatformInt64,
  ));
}


}

// dart format on

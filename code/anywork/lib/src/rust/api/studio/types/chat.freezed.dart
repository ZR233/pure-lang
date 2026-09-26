// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'chat.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$BridgeChatFocus {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeChatFocus);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeChatFocus()';
}


}

/// @nodoc
class $BridgeChatFocusCopyWith<$Res>  {
$BridgeChatFocusCopyWith(BridgeChatFocus _, $Res Function(BridgeChatFocus) __);
}


/// Adds pattern-matching-related methods to [BridgeChatFocus].
extension BridgeChatFocusPatterns on BridgeChatFocus {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeChatFocus_Latest value)?  latest,TResult Function( BridgeChatFocus_Around value)?  around,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeChatFocus_Latest() when latest != null:
return latest(_that);case BridgeChatFocus_Around() when around != null:
return around(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeChatFocus_Latest value)  latest,required TResult Function( BridgeChatFocus_Around value)  around,}){
final _that = this;
switch (_that) {
case BridgeChatFocus_Latest():
return latest(_that);case BridgeChatFocus_Around():
return around(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeChatFocus_Latest value)?  latest,TResult? Function( BridgeChatFocus_Around value)?  around,}){
final _that = this;
switch (_that) {
case BridgeChatFocus_Latest() when latest != null:
return latest(_that);case BridgeChatFocus_Around() when around != null:
return around(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  latest,TResult Function( String itemId)?  around,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeChatFocus_Latest() when latest != null:
return latest();case BridgeChatFocus_Around() when around != null:
return around(_that.itemId);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  latest,required TResult Function( String itemId)  around,}) {final _that = this;
switch (_that) {
case BridgeChatFocus_Latest():
return latest();case BridgeChatFocus_Around():
return around(_that.itemId);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  latest,TResult? Function( String itemId)?  around,}) {final _that = this;
switch (_that) {
case BridgeChatFocus_Latest() when latest != null:
return latest();case BridgeChatFocus_Around() when around != null:
return around(_that.itemId);case _:
  return null;

}
}

}

/// @nodoc


class BridgeChatFocus_Latest extends BridgeChatFocus {
  const BridgeChatFocus_Latest(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeChatFocus_Latest);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeChatFocus.latest()';
}


}




/// @nodoc


class BridgeChatFocus_Around extends BridgeChatFocus {
  const BridgeChatFocus_Around({required this.itemId}): super._();


 final  String itemId;

/// Create a copy of BridgeChatFocus
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeChatFocus_AroundCopyWith<BridgeChatFocus_Around> get copyWith => _$BridgeChatFocus_AroundCopyWithImpl<BridgeChatFocus_Around>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeChatFocus_Around&&(identical(other.itemId, itemId) || other.itemId == itemId));
}


@override
int get hashCode {
    return Object.hash(runtimeType,itemId);
}

@override
String toString() {
    return 'BridgeChatFocus.around(itemId: $itemId)';
}


}

/// @nodoc
abstract mixin class $BridgeChatFocus_AroundCopyWith<$Res> implements $BridgeChatFocusCopyWith<$Res> {
  factory $BridgeChatFocus_AroundCopyWith(BridgeChatFocus_Around value, $Res Function(BridgeChatFocus_Around) _then) = _$BridgeChatFocus_AroundCopyWithImpl;
@useResult
$Res call({
 String itemId
});




}
/// @nodoc
class _$BridgeChatFocus_AroundCopyWithImpl<$Res>
    implements $BridgeChatFocus_AroundCopyWith<$Res> {
  _$BridgeChatFocus_AroundCopyWithImpl(this._self, this._then);

  final BridgeChatFocus_Around _self;
  final $Res Function(BridgeChatFocus_Around) _then;

/// Create a copy of BridgeChatFocus
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? itemId = null,}) {
  return _then(BridgeChatFocus_Around(
itemId: null == itemId ? _self.itemId : itemId // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc
mixin _$BridgeChatUpdate {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeChatUpdate);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeChatUpdate()';
}


}

/// @nodoc
class $BridgeChatUpdateCopyWith<$Res>  {
$BridgeChatUpdateCopyWith(BridgeChatUpdate _, $Res Function(BridgeChatUpdate) __);
}


/// Adds pattern-matching-related methods to [BridgeChatUpdate].
extension BridgeChatUpdatePatterns on BridgeChatUpdate {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeChatUpdate_Reset value)?  reset,TResult Function( BridgeChatUpdate_Patch value)?  patch,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeChatUpdate_Reset() when reset != null:
return reset(_that);case BridgeChatUpdate_Patch() when patch != null:
return patch(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeChatUpdate_Reset value)  reset,required TResult Function( BridgeChatUpdate_Patch value)  patch,}){
final _that = this;
switch (_that) {
case BridgeChatUpdate_Reset():
return reset(_that);case BridgeChatUpdate_Patch():
return patch(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeChatUpdate_Reset value)?  reset,TResult? Function( BridgeChatUpdate_Patch value)?  patch,}){
final _that = this;
switch (_that) {
case BridgeChatUpdate_Reset() when reset != null:
return reset(_that);case BridgeChatUpdate_Patch() when patch != null:
return patch(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BridgeChatSnapshot snapshot)?  reset,TResult Function( BigInt from,  BigInt to,  List<BridgeViewChange> changes,  bool hasNewer,  BridgeUpdatePriority priority)?  patch,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeChatUpdate_Reset() when reset != null:
return reset(_that.snapshot);case BridgeChatUpdate_Patch() when patch != null:
return patch(_that.from,_that.to,_that.changes,_that.hasNewer,_that.priority);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BridgeChatSnapshot snapshot)  reset,required TResult Function( BigInt from,  BigInt to,  List<BridgeViewChange> changes,  bool hasNewer,  BridgeUpdatePriority priority)  patch,}) {final _that = this;
switch (_that) {
case BridgeChatUpdate_Reset():
return reset(_that.snapshot);case BridgeChatUpdate_Patch():
return patch(_that.from,_that.to,_that.changes,_that.hasNewer,_that.priority);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BridgeChatSnapshot snapshot)?  reset,TResult? Function( BigInt from,  BigInt to,  List<BridgeViewChange> changes,  bool hasNewer,  BridgeUpdatePriority priority)?  patch,}) {final _that = this;
switch (_that) {
case BridgeChatUpdate_Reset() when reset != null:
return reset(_that.snapshot);case BridgeChatUpdate_Patch() when patch != null:
return patch(_that.from,_that.to,_that.changes,_that.hasNewer,_that.priority);case _:
  return null;

}
}

}

/// @nodoc


class BridgeChatUpdate_Reset extends BridgeChatUpdate {
  const BridgeChatUpdate_Reset({required this.snapshot}): super._();


 final  BridgeChatSnapshot snapshot;

/// Create a copy of BridgeChatUpdate
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeChatUpdate_ResetCopyWith<BridgeChatUpdate_Reset> get copyWith => _$BridgeChatUpdate_ResetCopyWithImpl<BridgeChatUpdate_Reset>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeChatUpdate_Reset&&(identical(other.snapshot, snapshot) || other.snapshot == snapshot));
}


@override
int get hashCode {
    return Object.hash(runtimeType,snapshot);
}

@override
String toString() {
    return 'BridgeChatUpdate.reset(snapshot: $snapshot)';
}


}

/// @nodoc
abstract mixin class $BridgeChatUpdate_ResetCopyWith<$Res> implements $BridgeChatUpdateCopyWith<$Res> {
  factory $BridgeChatUpdate_ResetCopyWith(BridgeChatUpdate_Reset value, $Res Function(BridgeChatUpdate_Reset) _then) = _$BridgeChatUpdate_ResetCopyWithImpl;
@useResult
$Res call({
 BridgeChatSnapshot snapshot
});




}
/// @nodoc
class _$BridgeChatUpdate_ResetCopyWithImpl<$Res>
    implements $BridgeChatUpdate_ResetCopyWith<$Res> {
  _$BridgeChatUpdate_ResetCopyWithImpl(this._self, this._then);

  final BridgeChatUpdate_Reset _self;
  final $Res Function(BridgeChatUpdate_Reset) _then;

/// Create a copy of BridgeChatUpdate
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? snapshot = null,}) {
  return _then(BridgeChatUpdate_Reset(
snapshot: null == snapshot ? _self.snapshot : snapshot // ignore: cast_nullable_to_non_nullable
as BridgeChatSnapshot,
  ));
}


}

/// @nodoc


class BridgeChatUpdate_Patch extends BridgeChatUpdate {
  const BridgeChatUpdate_Patch({required this.from, required this.to, required  List<BridgeViewChange> changes, required this.hasNewer, required this.priority}): _changes = changes,super._();


 final  BigInt from;
 final  BigInt to;
 final  List<BridgeViewChange> _changes;
 List<BridgeViewChange> get changes {
  if (_changes is EqualUnmodifiableListView) return _changes;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_changes);
}

 final  bool hasNewer;
 final  BridgeUpdatePriority priority;

/// Create a copy of BridgeChatUpdate
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeChatUpdate_PatchCopyWith<BridgeChatUpdate_Patch> get copyWith => _$BridgeChatUpdate_PatchCopyWithImpl<BridgeChatUpdate_Patch>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeChatUpdate_Patch&&(identical(other.from, from) || other.from == from)&&(identical(other.to, to) || other.to == to)&&const DeepCollectionEquality().equals(other.changes, _changes)&&(identical(other.hasNewer, hasNewer) || other.hasNewer == hasNewer)&&(identical(other.priority, priority) || other.priority == priority));
}


@override
int get hashCode {
    return Object.hash(runtimeType,from,to,const DeepCollectionEquality().hash(_changes),hasNewer,priority);
}

@override
String toString() {
    return 'BridgeChatUpdate.patch(from: $from, to: $to, changes: $changes, hasNewer: $hasNewer, priority: $priority)';
}


}

/// @nodoc
abstract mixin class $BridgeChatUpdate_PatchCopyWith<$Res> implements $BridgeChatUpdateCopyWith<$Res> {
  factory $BridgeChatUpdate_PatchCopyWith(BridgeChatUpdate_Patch value, $Res Function(BridgeChatUpdate_Patch) _then) = _$BridgeChatUpdate_PatchCopyWithImpl;
@useResult
$Res call({
 BigInt from, BigInt to, List<BridgeViewChange> changes, bool hasNewer, BridgeUpdatePriority priority
});




}
/// @nodoc
class _$BridgeChatUpdate_PatchCopyWithImpl<$Res>
    implements $BridgeChatUpdate_PatchCopyWith<$Res> {
  _$BridgeChatUpdate_PatchCopyWithImpl(this._self, this._then);

  final BridgeChatUpdate_Patch _self;
  final $Res Function(BridgeChatUpdate_Patch) _then;

/// Create a copy of BridgeChatUpdate
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? from = null,Object? to = null,Object? changes = null,Object? hasNewer = null,Object? priority = null,}) {
  return _then(BridgeChatUpdate_Patch(
from: null == from ? _self.from : from // ignore: cast_nullable_to_non_nullable
as BigInt,to: null == to ? _self.to : to // ignore: cast_nullable_to_non_nullable
as BigInt,changes: null == changes ? _self._changes : changes // ignore: cast_nullable_to_non_nullable
as List<BridgeViewChange>,hasNewer: null == hasNewer ? _self.hasNewer : hasNewer // ignore: cast_nullable_to_non_nullable
as bool,priority: null == priority ? _self.priority : priority // ignore: cast_nullable_to_non_nullable
as BridgeUpdatePriority,
  ));
}


}

/// @nodoc
mixin _$BridgeContentField {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeContentField);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeContentField()';
}


}

/// @nodoc
class $BridgeContentFieldCopyWith<$Res>  {
$BridgeContentFieldCopyWith(BridgeContentField _, $Res Function(BridgeContentField) __);
}


/// Adds pattern-matching-related methods to [BridgeContentField].
extension BridgeContentFieldPatterns on BridgeContentField {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeContentField_Text value)?  text,TResult Function( BridgeContentField_ThinkingSummary value)?  thinkingSummary,TResult Function( BridgeContentField_ThinkingContent value)?  thinkingContent,TResult Function( BridgeContentField_ToolArguments value)?  toolArguments,TResult Function( BridgeContentField_ToolResult value)?  toolResult,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeContentField_Text() when text != null:
return text(_that);case BridgeContentField_ThinkingSummary() when thinkingSummary != null:
return thinkingSummary(_that);case BridgeContentField_ThinkingContent() when thinkingContent != null:
return thinkingContent(_that);case BridgeContentField_ToolArguments() when toolArguments != null:
return toolArguments(_that);case BridgeContentField_ToolResult() when toolResult != null:
return toolResult(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeContentField_Text value)  text,required TResult Function( BridgeContentField_ThinkingSummary value)  thinkingSummary,required TResult Function( BridgeContentField_ThinkingContent value)  thinkingContent,required TResult Function( BridgeContentField_ToolArguments value)  toolArguments,required TResult Function( BridgeContentField_ToolResult value)  toolResult,}){
final _that = this;
switch (_that) {
case BridgeContentField_Text():
return text(_that);case BridgeContentField_ThinkingSummary():
return thinkingSummary(_that);case BridgeContentField_ThinkingContent():
return thinkingContent(_that);case BridgeContentField_ToolArguments():
return toolArguments(_that);case BridgeContentField_ToolResult():
return toolResult(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeContentField_Text value)?  text,TResult? Function( BridgeContentField_ThinkingSummary value)?  thinkingSummary,TResult? Function( BridgeContentField_ThinkingContent value)?  thinkingContent,TResult? Function( BridgeContentField_ToolArguments value)?  toolArguments,TResult? Function( BridgeContentField_ToolResult value)?  toolResult,}){
final _that = this;
switch (_that) {
case BridgeContentField_Text() when text != null:
return text(_that);case BridgeContentField_ThinkingSummary() when thinkingSummary != null:
return thinkingSummary(_that);case BridgeContentField_ThinkingContent() when thinkingContent != null:
return thinkingContent(_that);case BridgeContentField_ToolArguments() when toolArguments != null:
return toolArguments(_that);case BridgeContentField_ToolResult() when toolResult != null:
return toolResult(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  text,TResult Function( int chunkIndex)?  thinkingSummary,TResult Function( int chunkIndex)?  thinkingContent,TResult Function()?  toolArguments,TResult Function()?  toolResult,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeContentField_Text() when text != null:
return text();case BridgeContentField_ThinkingSummary() when thinkingSummary != null:
return thinkingSummary(_that.chunkIndex);case BridgeContentField_ThinkingContent() when thinkingContent != null:
return thinkingContent(_that.chunkIndex);case BridgeContentField_ToolArguments() when toolArguments != null:
return toolArguments();case BridgeContentField_ToolResult() when toolResult != null:
return toolResult();case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  text,required TResult Function( int chunkIndex)  thinkingSummary,required TResult Function( int chunkIndex)  thinkingContent,required TResult Function()  toolArguments,required TResult Function()  toolResult,}) {final _that = this;
switch (_that) {
case BridgeContentField_Text():
return text();case BridgeContentField_ThinkingSummary():
return thinkingSummary(_that.chunkIndex);case BridgeContentField_ThinkingContent():
return thinkingContent(_that.chunkIndex);case BridgeContentField_ToolArguments():
return toolArguments();case BridgeContentField_ToolResult():
return toolResult();}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  text,TResult? Function( int chunkIndex)?  thinkingSummary,TResult? Function( int chunkIndex)?  thinkingContent,TResult? Function()?  toolArguments,TResult? Function()?  toolResult,}) {final _that = this;
switch (_that) {
case BridgeContentField_Text() when text != null:
return text();case BridgeContentField_ThinkingSummary() when thinkingSummary != null:
return thinkingSummary(_that.chunkIndex);case BridgeContentField_ThinkingContent() when thinkingContent != null:
return thinkingContent(_that.chunkIndex);case BridgeContentField_ToolArguments() when toolArguments != null:
return toolArguments();case BridgeContentField_ToolResult() when toolResult != null:
return toolResult();case _:
  return null;

}
}

}

/// @nodoc


class BridgeContentField_Text extends BridgeContentField {
  const BridgeContentField_Text(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeContentField_Text);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeContentField.text()';
}


}




/// @nodoc


class BridgeContentField_ThinkingSummary extends BridgeContentField {
  const BridgeContentField_ThinkingSummary({required this.chunkIndex}): super._();


 final  int chunkIndex;

/// Create a copy of BridgeContentField
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeContentField_ThinkingSummaryCopyWith<BridgeContentField_ThinkingSummary> get copyWith => _$BridgeContentField_ThinkingSummaryCopyWithImpl<BridgeContentField_ThinkingSummary>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeContentField_ThinkingSummary&&(identical(other.chunkIndex, chunkIndex) || other.chunkIndex == chunkIndex));
}


@override
int get hashCode {
    return Object.hash(runtimeType,chunkIndex);
}

@override
String toString() {
    return 'BridgeContentField.thinkingSummary(chunkIndex: $chunkIndex)';
}


}

/// @nodoc
abstract mixin class $BridgeContentField_ThinkingSummaryCopyWith<$Res> implements $BridgeContentFieldCopyWith<$Res> {
  factory $BridgeContentField_ThinkingSummaryCopyWith(BridgeContentField_ThinkingSummary value, $Res Function(BridgeContentField_ThinkingSummary) _then) = _$BridgeContentField_ThinkingSummaryCopyWithImpl;
@useResult
$Res call({
 int chunkIndex
});




}
/// @nodoc
class _$BridgeContentField_ThinkingSummaryCopyWithImpl<$Res>
    implements $BridgeContentField_ThinkingSummaryCopyWith<$Res> {
  _$BridgeContentField_ThinkingSummaryCopyWithImpl(this._self, this._then);

  final BridgeContentField_ThinkingSummary _self;
  final $Res Function(BridgeContentField_ThinkingSummary) _then;

/// Create a copy of BridgeContentField
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? chunkIndex = null,}) {
  return _then(BridgeContentField_ThinkingSummary(
chunkIndex: null == chunkIndex ? _self.chunkIndex : chunkIndex // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc


class BridgeContentField_ThinkingContent extends BridgeContentField {
  const BridgeContentField_ThinkingContent({required this.chunkIndex}): super._();


 final  int chunkIndex;

/// Create a copy of BridgeContentField
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeContentField_ThinkingContentCopyWith<BridgeContentField_ThinkingContent> get copyWith => _$BridgeContentField_ThinkingContentCopyWithImpl<BridgeContentField_ThinkingContent>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeContentField_ThinkingContent&&(identical(other.chunkIndex, chunkIndex) || other.chunkIndex == chunkIndex));
}


@override
int get hashCode {
    return Object.hash(runtimeType,chunkIndex);
}

@override
String toString() {
    return 'BridgeContentField.thinkingContent(chunkIndex: $chunkIndex)';
}


}

/// @nodoc
abstract mixin class $BridgeContentField_ThinkingContentCopyWith<$Res> implements $BridgeContentFieldCopyWith<$Res> {
  factory $BridgeContentField_ThinkingContentCopyWith(BridgeContentField_ThinkingContent value, $Res Function(BridgeContentField_ThinkingContent) _then) = _$BridgeContentField_ThinkingContentCopyWithImpl;
@useResult
$Res call({
 int chunkIndex
});




}
/// @nodoc
class _$BridgeContentField_ThinkingContentCopyWithImpl<$Res>
    implements $BridgeContentField_ThinkingContentCopyWith<$Res> {
  _$BridgeContentField_ThinkingContentCopyWithImpl(this._self, this._then);

  final BridgeContentField_ThinkingContent _self;
  final $Res Function(BridgeContentField_ThinkingContent) _then;

/// Create a copy of BridgeContentField
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? chunkIndex = null,}) {
  return _then(BridgeContentField_ThinkingContent(
chunkIndex: null == chunkIndex ? _self.chunkIndex : chunkIndex // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc


class BridgeContentField_ToolArguments extends BridgeContentField {
  const BridgeContentField_ToolArguments(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeContentField_ToolArguments);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeContentField.toolArguments()';
}


}




/// @nodoc


class BridgeContentField_ToolResult extends BridgeContentField {
  const BridgeContentField_ToolResult(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeContentField_ToolResult);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeContentField.toolResult()';
}


}




/// @nodoc
mixin _$BridgeFieldChange {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeFieldChange);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeFieldChange()';
}


}

/// @nodoc
class $BridgeFieldChangeCopyWith<$Res>  {
$BridgeFieldChangeCopyWith(BridgeFieldChange _, $Res Function(BridgeFieldChange) __);
}


/// Adds pattern-matching-related methods to [BridgeFieldChange].
extension BridgeFieldChangePatterns on BridgeFieldChange {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeFieldChange_Unchanged value)?  unchanged,TResult Function( BridgeFieldChange_Append value)?  append,TResult Function( BridgeFieldChange_Replace value)?  replace,TResult Function( BridgeFieldChange_Remove value)?  remove,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeFieldChange_Unchanged() when unchanged != null:
return unchanged(_that);case BridgeFieldChange_Append() when append != null:
return append(_that);case BridgeFieldChange_Replace() when replace != null:
return replace(_that);case BridgeFieldChange_Remove() when remove != null:
return remove(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeFieldChange_Unchanged value)  unchanged,required TResult Function( BridgeFieldChange_Append value)  append,required TResult Function( BridgeFieldChange_Replace value)  replace,required TResult Function( BridgeFieldChange_Remove value)  remove,}){
final _that = this;
switch (_that) {
case BridgeFieldChange_Unchanged():
return unchanged(_that);case BridgeFieldChange_Append():
return append(_that);case BridgeFieldChange_Replace():
return replace(_that);case BridgeFieldChange_Remove():
return remove(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeFieldChange_Unchanged value)?  unchanged,TResult? Function( BridgeFieldChange_Append value)?  append,TResult? Function( BridgeFieldChange_Replace value)?  replace,TResult? Function( BridgeFieldChange_Remove value)?  remove,}){
final _that = this;
switch (_that) {
case BridgeFieldChange_Unchanged() when unchanged != null:
return unchanged(_that);case BridgeFieldChange_Append() when append != null:
return append(_that);case BridgeFieldChange_Replace() when replace != null:
return replace(_that);case BridgeFieldChange_Remove() when remove != null:
return remove(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  unchanged,TResult Function( String text)?  append,TResult Function( String text)?  replace,TResult Function()?  remove,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeFieldChange_Unchanged() when unchanged != null:
return unchanged();case BridgeFieldChange_Append() when append != null:
return append(_that.text);case BridgeFieldChange_Replace() when replace != null:
return replace(_that.text);case BridgeFieldChange_Remove() when remove != null:
return remove();case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  unchanged,required TResult Function( String text)  append,required TResult Function( String text)  replace,required TResult Function()  remove,}) {final _that = this;
switch (_that) {
case BridgeFieldChange_Unchanged():
return unchanged();case BridgeFieldChange_Append():
return append(_that.text);case BridgeFieldChange_Replace():
return replace(_that.text);case BridgeFieldChange_Remove():
return remove();}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  unchanged,TResult? Function( String text)?  append,TResult? Function( String text)?  replace,TResult? Function()?  remove,}) {final _that = this;
switch (_that) {
case BridgeFieldChange_Unchanged() when unchanged != null:
return unchanged();case BridgeFieldChange_Append() when append != null:
return append(_that.text);case BridgeFieldChange_Replace() when replace != null:
return replace(_that.text);case BridgeFieldChange_Remove() when remove != null:
return remove();case _:
  return null;

}
}

}

/// @nodoc


class BridgeFieldChange_Unchanged extends BridgeFieldChange {
  const BridgeFieldChange_Unchanged(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeFieldChange_Unchanged);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeFieldChange.unchanged()';
}


}




/// @nodoc


class BridgeFieldChange_Append extends BridgeFieldChange {
  const BridgeFieldChange_Append({required this.text}): super._();


 final  String text;

/// Create a copy of BridgeFieldChange
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeFieldChange_AppendCopyWith<BridgeFieldChange_Append> get copyWith => _$BridgeFieldChange_AppendCopyWithImpl<BridgeFieldChange_Append>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeFieldChange_Append&&(identical(other.text, text) || other.text == text));
}


@override
int get hashCode {
    return Object.hash(runtimeType,text);
}

@override
String toString() {
    return 'BridgeFieldChange.append(text: $text)';
}


}

/// @nodoc
abstract mixin class $BridgeFieldChange_AppendCopyWith<$Res> implements $BridgeFieldChangeCopyWith<$Res> {
  factory $BridgeFieldChange_AppendCopyWith(BridgeFieldChange_Append value, $Res Function(BridgeFieldChange_Append) _then) = _$BridgeFieldChange_AppendCopyWithImpl;
@useResult
$Res call({
 String text
});




}
/// @nodoc
class _$BridgeFieldChange_AppendCopyWithImpl<$Res>
    implements $BridgeFieldChange_AppendCopyWith<$Res> {
  _$BridgeFieldChange_AppendCopyWithImpl(this._self, this._then);

  final BridgeFieldChange_Append _self;
  final $Res Function(BridgeFieldChange_Append) _then;

/// Create a copy of BridgeFieldChange
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? text = null,}) {
  return _then(BridgeFieldChange_Append(
text: null == text ? _self.text : text // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeFieldChange_Replace extends BridgeFieldChange {
  const BridgeFieldChange_Replace({required this.text}): super._();


 final  String text;

/// Create a copy of BridgeFieldChange
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeFieldChange_ReplaceCopyWith<BridgeFieldChange_Replace> get copyWith => _$BridgeFieldChange_ReplaceCopyWithImpl<BridgeFieldChange_Replace>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeFieldChange_Replace&&(identical(other.text, text) || other.text == text));
}


@override
int get hashCode {
    return Object.hash(runtimeType,text);
}

@override
String toString() {
    return 'BridgeFieldChange.replace(text: $text)';
}


}

/// @nodoc
abstract mixin class $BridgeFieldChange_ReplaceCopyWith<$Res> implements $BridgeFieldChangeCopyWith<$Res> {
  factory $BridgeFieldChange_ReplaceCopyWith(BridgeFieldChange_Replace value, $Res Function(BridgeFieldChange_Replace) _then) = _$BridgeFieldChange_ReplaceCopyWithImpl;
@useResult
$Res call({
 String text
});




}
/// @nodoc
class _$BridgeFieldChange_ReplaceCopyWithImpl<$Res>
    implements $BridgeFieldChange_ReplaceCopyWith<$Res> {
  _$BridgeFieldChange_ReplaceCopyWithImpl(this._self, this._then);

  final BridgeFieldChange_Replace _self;
  final $Res Function(BridgeFieldChange_Replace) _then;

/// Create a copy of BridgeFieldChange
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? text = null,}) {
  return _then(BridgeFieldChange_Replace(
text: null == text ? _self.text : text // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class BridgeFieldChange_Remove extends BridgeFieldChange {
  const BridgeFieldChange_Remove(): super._();







@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeFieldChange_Remove);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeFieldChange.remove()';
}


}




/// @nodoc
mixin _$BridgeViewChange {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeViewChange);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'BridgeViewChange()';
}


}

/// @nodoc
class $BridgeViewChangeCopyWith<$Res>  {
$BridgeViewChangeCopyWith(BridgeViewChange _, $Res Function(BridgeViewChange) __);
}


/// Adds pattern-matching-related methods to [BridgeViewChange].
extension BridgeViewChangePatterns on BridgeViewChange {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( BridgeViewChange_Splice value)?  splice,TResult Function( BridgeViewChange_UpdateItem value)?  updateItem,required TResult orElse(),}){
final _that = this;
switch (_that) {
case BridgeViewChange_Splice() when splice != null:
return splice(_that);case BridgeViewChange_UpdateItem() when updateItem != null:
return updateItem(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( BridgeViewChange_Splice value)  splice,required TResult Function( BridgeViewChange_UpdateItem value)  updateItem,}){
final _that = this;
switch (_that) {
case BridgeViewChange_Splice():
return splice(_that);case BridgeViewChange_UpdateItem():
return updateItem(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( BridgeViewChange_Splice value)?  splice,TResult? Function( BridgeViewChange_UpdateItem value)?  updateItem,}){
final _that = this;
switch (_that) {
case BridgeViewChange_Splice() when splice != null:
return splice(_that);case BridgeViewChange_UpdateItem() when updateItem != null:
return updateItem(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( BigInt index,  BigInt remove,  List<BridgeChatItem> items)?  splice,TResult Function( String itemId,  BigInt expectedRevision,  BigInt revision,  BigInt omittedBytes,  bool saved,  List<BridgeFieldUpdate> fields)?  updateItem,required TResult orElse(),}) {final _that = this;
switch (_that) {
case BridgeViewChange_Splice() when splice != null:
return splice(_that.index,_that.remove,_that.items);case BridgeViewChange_UpdateItem() when updateItem != null:
return updateItem(_that.itemId,_that.expectedRevision,_that.revision,_that.omittedBytes,_that.saved,_that.fields);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( BigInt index,  BigInt remove,  List<BridgeChatItem> items)  splice,required TResult Function( String itemId,  BigInt expectedRevision,  BigInt revision,  BigInt omittedBytes,  bool saved,  List<BridgeFieldUpdate> fields)  updateItem,}) {final _that = this;
switch (_that) {
case BridgeViewChange_Splice():
return splice(_that.index,_that.remove,_that.items);case BridgeViewChange_UpdateItem():
return updateItem(_that.itemId,_that.expectedRevision,_that.revision,_that.omittedBytes,_that.saved,_that.fields);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( BigInt index,  BigInt remove,  List<BridgeChatItem> items)?  splice,TResult? Function( String itemId,  BigInt expectedRevision,  BigInt revision,  BigInt omittedBytes,  bool saved,  List<BridgeFieldUpdate> fields)?  updateItem,}) {final _that = this;
switch (_that) {
case BridgeViewChange_Splice() when splice != null:
return splice(_that.index,_that.remove,_that.items);case BridgeViewChange_UpdateItem() when updateItem != null:
return updateItem(_that.itemId,_that.expectedRevision,_that.revision,_that.omittedBytes,_that.saved,_that.fields);case _:
  return null;

}
}

}

/// @nodoc


class BridgeViewChange_Splice extends BridgeViewChange {
  const BridgeViewChange_Splice({required this.index, required this.remove, required  List<BridgeChatItem> items}): _items = items,super._();


 final  BigInt index;
 final  BigInt remove;
 final  List<BridgeChatItem> _items;
 List<BridgeChatItem> get items {
  if (_items is EqualUnmodifiableListView) return _items;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_items);
}


/// Create a copy of BridgeViewChange
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeViewChange_SpliceCopyWith<BridgeViewChange_Splice> get copyWith => _$BridgeViewChange_SpliceCopyWithImpl<BridgeViewChange_Splice>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeViewChange_Splice&&(identical(other.index, index) || other.index == index)&&(identical(other.remove, remove) || other.remove == remove)&&const DeepCollectionEquality().equals(other.items, _items));
}


@override
int get hashCode {
    return Object.hash(runtimeType,index,remove,const DeepCollectionEquality().hash(_items));
}

@override
String toString() {
    return 'BridgeViewChange.splice(index: $index, remove: $remove, items: $items)';
}


}

/// @nodoc
abstract mixin class $BridgeViewChange_SpliceCopyWith<$Res> implements $BridgeViewChangeCopyWith<$Res> {
  factory $BridgeViewChange_SpliceCopyWith(BridgeViewChange_Splice value, $Res Function(BridgeViewChange_Splice) _then) = _$BridgeViewChange_SpliceCopyWithImpl;
@useResult
$Res call({
 BigInt index, BigInt remove, List<BridgeChatItem> items
});




}
/// @nodoc
class _$BridgeViewChange_SpliceCopyWithImpl<$Res>
    implements $BridgeViewChange_SpliceCopyWith<$Res> {
  _$BridgeViewChange_SpliceCopyWithImpl(this._self, this._then);

  final BridgeViewChange_Splice _self;
  final $Res Function(BridgeViewChange_Splice) _then;

/// Create a copy of BridgeViewChange
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? index = null,Object? remove = null,Object? items = null,}) {
  return _then(BridgeViewChange_Splice(
index: null == index ? _self.index : index // ignore: cast_nullable_to_non_nullable
as BigInt,remove: null == remove ? _self.remove : remove // ignore: cast_nullable_to_non_nullable
as BigInt,items: null == items ? _self._items : items // ignore: cast_nullable_to_non_nullable
as List<BridgeChatItem>,
  ));
}


}

/// @nodoc


class BridgeViewChange_UpdateItem extends BridgeViewChange {
  const BridgeViewChange_UpdateItem({required this.itemId, required this.expectedRevision, required this.revision, required this.omittedBytes, required this.saved, required  List<BridgeFieldUpdate> fields}): _fields = fields,super._();


 final  String itemId;
 final  BigInt expectedRevision;
 final  BigInt revision;
 final  BigInt omittedBytes;
 final  bool saved;
 final  List<BridgeFieldUpdate> _fields;
 List<BridgeFieldUpdate> get fields {
  if (_fields is EqualUnmodifiableListView) return _fields;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_fields);
}


/// Create a copy of BridgeViewChange
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$BridgeViewChange_UpdateItemCopyWith<BridgeViewChange_UpdateItem> get copyWith => _$BridgeViewChange_UpdateItemCopyWithImpl<BridgeViewChange_UpdateItem>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is BridgeViewChange_UpdateItem&&(identical(other.itemId, itemId) || other.itemId == itemId)&&(identical(other.expectedRevision, expectedRevision) || other.expectedRevision == expectedRevision)&&(identical(other.revision, revision) || other.revision == revision)&&(identical(other.omittedBytes, omittedBytes) || other.omittedBytes == omittedBytes)&&(identical(other.saved, saved) || other.saved == saved)&&const DeepCollectionEquality().equals(other.fields, _fields));
}


@override
int get hashCode {
    return Object.hash(runtimeType,itemId,expectedRevision,revision,omittedBytes,saved,const DeepCollectionEquality().hash(_fields));
}

@override
String toString() {
    return 'BridgeViewChange.updateItem(itemId: $itemId, expectedRevision: $expectedRevision, revision: $revision, omittedBytes: $omittedBytes, saved: $saved, fields: $fields)';
}


}

/// @nodoc
abstract mixin class $BridgeViewChange_UpdateItemCopyWith<$Res> implements $BridgeViewChangeCopyWith<$Res> {
  factory $BridgeViewChange_UpdateItemCopyWith(BridgeViewChange_UpdateItem value, $Res Function(BridgeViewChange_UpdateItem) _then) = _$BridgeViewChange_UpdateItemCopyWithImpl;
@useResult
$Res call({
 String itemId, BigInt expectedRevision, BigInt revision, BigInt omittedBytes, bool saved, List<BridgeFieldUpdate> fields
});




}
/// @nodoc
class _$BridgeViewChange_UpdateItemCopyWithImpl<$Res>
    implements $BridgeViewChange_UpdateItemCopyWith<$Res> {
  _$BridgeViewChange_UpdateItemCopyWithImpl(this._self, this._then);

  final BridgeViewChange_UpdateItem _self;
  final $Res Function(BridgeViewChange_UpdateItem) _then;

/// Create a copy of BridgeViewChange
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? itemId = null,Object? expectedRevision = null,Object? revision = null,Object? omittedBytes = null,Object? saved = null,Object? fields = null,}) {
  return _then(BridgeViewChange_UpdateItem(
itemId: null == itemId ? _self.itemId : itemId // ignore: cast_nullable_to_non_nullable
as String,expectedRevision: null == expectedRevision ? _self.expectedRevision : expectedRevision // ignore: cast_nullable_to_non_nullable
as BigInt,revision: null == revision ? _self.revision : revision // ignore: cast_nullable_to_non_nullable
as BigInt,omittedBytes: null == omittedBytes ? _self.omittedBytes : omittedBytes // ignore: cast_nullable_to_non_nullable
as BigInt,saved: null == saved ? _self.saved : saved // ignore: cast_nullable_to_non_nullable
as bool,fields: null == fields ? _self._fields : fields // ignore: cast_nullable_to_non_nullable
as List<BridgeFieldUpdate>,
  ));
}


}

// dart format on

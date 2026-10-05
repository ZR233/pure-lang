import 'package:flutter/material.dart';

import 'studio_menu.dart';

/// One selectable choice of a [StudioFormSelectField].
///
/// [value] may be null when the field's type argument is nullable, keeping an
/// explicit "no value" choice selectable instead of canceling the field.
class StudioFormSelectItem<T> {
  const StudioFormSelectItem({
    required this.value,
    required this.child,
    this.enabled = true,
    this.itemKey,
  });

  /// The value reported through the field when this choice is activated.
  final T value;

  /// Row content inside the menu.
  final Widget child;

  /// Whether the choice can be activated.
  final bool enabled;

  /// Stable interaction key for the row.
  final Key? itemKey;
}

/// Anchored select field for settings forms.
///
/// The field keeps [FormField] semantics — validation, saved values and
/// nullable values — while presenting choices through the shared [StudioMenu]
/// anchoring contract: the menu aligns to the field's real bounds, flips or
/// clamps to the window, scrolls long content, and closes on outside taps,
/// Escape, or layout changes.
///
/// Draft fields start from [initialValue] and only change through user
/// selection; rebuilding with a new key restarts the draft as before. Fields
/// driven by canonical state pass [value] instead and follow it on every
/// rebuild.
class StudioFormSelectField<T> extends FormField<T> {
  StudioFormSelectField({
    required this.items,
    Object? value = _noExternalValue,
    this.hint,
    this.onChanged,
    super.key,
    T? initialValue,
    super.enabled,
    super.autovalidateMode,
    super.onSaved,
    super.validator,
    super.restorationId,
    this.decoration = const InputDecoration(),
    this.isExpanded = false,
  }) : _externalValue = value,
       super(
         initialValue: identical(value, _noExternalValue)
             ? initialValue
             : value as T?,
         builder: (FormFieldState<T> field) {
           final state = field as _StudioFormSelectFieldState<T>;
           return state._build(field);
         },
       );

  /// Sentinel distinguishing an omitted controlled value from an explicit
  /// null value.
  static const _NoExternalValue _noExternalValue = _NoExternalValue();

  /// Controlled value marker; [_noExternalValue] when [value] was omitted.
  final Object? _externalValue;

  /// Current choices; called on every rebuild so open menus refresh.
  final List<StudioFormSelectItem<T>> items;

  /// Shown inside the field when the current value matches no item.
  final Widget? hint;

  /// Reported with the activated choice's value; a null callback disables the
  /// field.
  final ValueChanged<T?>? onChanged;

  /// Field decoration; keeps label, hint, dense and disabled styling.
  final InputDecoration decoration;

  /// Whether the selected text ellipsizes instead of overflowing the field.
  final bool isExpanded;

  bool get _isControlled => !identical(_externalValue, _noExternalValue);

  T? get _controlledValue => _isControlled ? _externalValue as T? : null;

  @override
  FormFieldState<T> createState() => _StudioFormSelectFieldState<T>();
}

class _NoExternalValue {
  const _NoExternalValue();
}

class _StudioFormSelectFieldState<T> extends FormFieldState<T> {
  final FocusNode _triggerFocusNode = FocusNode(
    debugLabel: 'studio-form-select-trigger',
  );

  @override
  StudioFormSelectField<T> get widget =>
      super.widget as StudioFormSelectField<T>;

  @override
  void didUpdateWidget(StudioFormSelectField<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    // Controlled fields follow the canonical value on every rebuild; draft
    // fields keep user edits until their owning key remounts them.
    if (widget._isControlled && widget._controlledValue != value) {
      didChange(widget._controlledValue);
    }
  }

  @override
  void dispose() {
    _triggerFocusNode.dispose();
    super.dispose();
  }

  bool get _isEnabled =>
      widget.enabled && widget.onChanged != null && widget.items.isNotEmpty;

  StudioFormSelectItem<T>? get _selectedItem {
    final value = this.value;
    for (final item in widget.items) {
      if (item.value == value) {
        return item;
      }
    }
    return null;
  }

  Widget _build(FormFieldState<T> field) {
    final selected = _selectedItem;
    final isEmpty = selected == null && widget.hint == null;
    Widget fieldChild;
    if (selected != null) {
      fieldChild = selected.child;
    } else {
      final hint = widget.hint;
      fieldChild = hint ?? const SizedBox.shrink();
    }
    if (widget.isExpanded) {
      fieldChild = Row(children: [Expanded(child: fieldChild)]);
    }
    return StudioMenu.custom(
      childFocusNode: _triggerFocusNode,
      itemBuilder: (context) => [
        for (final item in widget.items)
          StudioMenuItem<T>(
            value: item.value,
            enabled: item.enabled,
            selected: identical(item, selected),
            itemKey: item.itemKey,
            child: item.child,
          ),
      ],
      enabled: _isEnabled,
      onSelected: (value) {
        if (widget._isControlled) {
          // Controlled fields only report; the display follows the canonical
          // value on the next rebuild so a failed save never shows a local
          // optimistic choice.
          widget.onChanged?.call(value);
        } else {
          didChange(value);
          widget.onChanged?.call(value);
        }
      },
      triggerBuilder: (context, controller, canOpen) => InkWell(
        focusNode: _triggerFocusNode,
        onTap: _isEnabled && canOpen ? controller.toggle : null,
        borderRadius: BorderRadius.circular(4),
        child: InputDecorator(
          isEmpty: isEmpty,
          isFocused: controller.isOpen,
          decoration: widget.decoration.copyWith(
            enabled: _isEnabled,
            errorText: field.errorText,
            suffixIcon:
                widget.decoration.suffixIcon ??
                const Icon(Icons.arrow_drop_down),
          ),
          child: DefaultTextStyle(
            style: Theme.of(context).textTheme.bodyMedium ?? const TextStyle(),
            child: fieldChild,
          ),
        ),
      ),
    );
  }
}

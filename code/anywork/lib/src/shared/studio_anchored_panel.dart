import 'dart:async';

import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'studio_anchored_surface.dart';

/// Anchored free-content panel for readouts and detail views.
///
/// The panel rides on the single Raw-driven anchored surface: the framework
/// primitive owns the overlay lifecycle and the shared layout layer derives
/// constraints and position from the real anchor bounds and the actual
/// content size, so the panel stays next to its trigger, flips above only
/// when the content does not fit below and never opens off-screen. Long
/// content bounds to the available side and scrolls with a visible
/// scrollbar.
///
/// The interaction contract follows the legacy status details: hovering the
/// trigger shows the panel transiently (hidden after [hideDelay] once the
/// pointer and focus have left), clicking toggles it, and a
/// [pinOpenOnTap] trigger keeps an open panel open under the pointer instead
/// of toggling it closed. Clicks focus the trigger first so Escape can close
/// the panel; the focus change from that click never re-shows a panel the
/// same click closed, and after a click closes the panel the hover under the
/// pointer does not immediately reopen it. Hover entry never steals focus
/// from other widgets, and keyboard entry follows the trigger's own focus.
class StudioAnchoredPanel extends StatefulWidget {
  const StudioAnchoredPanel({
    required this.child,
    required this.panelBuilder,
    this.width = 300,
    this.enabled = true,
    this.showOnFocus = false,
    this.pinOpenOnTap = false,
    this.panelIgnoresPointer = false,
    this.semanticsLabel,
    this.semanticsValue,
    this.onHoverChange,
    this.onFocusChange,
    this.hideDelay = const Duration(milliseconds: 120),
    super.key,
  });

  /// Trigger content; owns its own visuals and hover highlighting.
  final Widget child;

  /// Builds the panel content; called on every rebuild while open.
  final WidgetBuilder panelBuilder;

  /// Preferred panel width; clamped to the measured safe area on every open
  /// and relayout.
  final double width;

  /// Whether hover, focus and taps can open the panel.
  final bool enabled;

  /// Whether keyboard focus on the trigger opens the panel.
  final bool showOnFocus;

  /// Whether tapping the trigger while the panel is open under the pointer
  /// keeps it open instead of toggling it closed.
  final bool pinOpenOnTap;

  /// Whether the panel content ignores business interactions (buttons and
  /// other activators inside the content cannot be triggered). Scrolling
  /// stays available through the scrollbar thumb and the mouse wheel, so
  /// bounded read-only content remains fully readable.
  final bool panelIgnoresPointer;

  /// Semantics label for the trigger; when present the trigger is exposed as
  /// a focusable button with keyboard activation.
  final String? semanticsLabel;

  /// Semantics value describing the trigger's current state.
  final String? semanticsValue;

  /// Notified when the pointer starts or stops hovering the trigger.
  final ValueChanged<bool>? onHoverChange;

  /// Notified when the trigger gains or loses keyboard focus.
  final ValueChanged<bool>? onFocusChange;

  /// Delay before an unhovered, unfocused panel closes.
  final Duration hideDelay;

  @override
  State<StudioAnchoredPanel> createState() => _StudioAnchoredPanelState();
}

class _StudioAnchoredPanelState extends State<StudioAnchoredPanel> {
  final GlobalKey<StudioAnchoredSurfaceState> _surfaceKey = GlobalKey(
    debugLabel: 'studio-panel-surface',
  );
  final FocusNode _focusNode = FocusNode(debugLabel: 'studio-anchored-panel');
  final ScrollController _scrollController = ScrollController(
    debugLabel: 'studio-panel-content',
  );
  Timer? _hideTimer;
  bool _focused = false;
  bool _hovering = false;
  bool _pointerDownOnTrigger = false;

  /// Set when a trigger click closed the panel; hover under the pointer must
  /// not reopen it until the pointer actually leaves the trigger once.
  bool _hoverReopenSuppressed = false;

  StudioAnchoredSurfaceState? get _surface => _surfaceKey.currentState;

  bool get _isOpen => _surface?.isOpen ?? false;

  @override
  void dispose() {
    _hideTimer?.cancel();
    _focusNode.dispose();
    _scrollController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return StudioAnchoredSurface(
      key: _surfaceKey,
      enabled: widget.enabled,
      fixedWidth: widget.width,
      focusTriggerNode: widget.semanticsLabel == null ? null : _focusNode,
      onOpen: _handleOpened,
      onClose: _handleClosed,
      triggerBuilder: _buildTrigger,
      contentBuilder: _buildPanel,
    );
  }

  Widget _buildTrigger(BuildContext context) {
    Widget content = Listener(
      behavior: HitTestBehavior.translucent,
      onPointerDown: (_) {
        _pointerDownOnTrigger = true;
        _handlePointerDown();
      },
      onPointerUp: (_) => _pointerDownOnTrigger = false,
      onPointerCancel: (_) => _pointerDownOnTrigger = false,
      onPointerHover: (_) => _showTransient(),
      child: MouseRegion(
        onEnter: (_) => _handleHover(true),
        onExit: (_) => _handleHover(false),
        child: widget.child,
      ),
    );
    final semanticsLabel = widget.semanticsLabel;
    if (semanticsLabel != null) {
      content = Semantics(
        container: true,
        label: semanticsLabel,
        value: widget.semanticsValue,
        button: true,
        focusable: true,
        focused: _focused,
        onTap: _toggleByKeyboard,
        child: content,
      );
    }
    return FocusableActionDetector(
      focusNode: _focusNode,
      onFocusChange: _handleFocusChange,
      shortcuts: const {
        SingleActivator(LogicalKeyboardKey.enter): ActivateIntent(),
        SingleActivator(LogicalKeyboardKey.space): ActivateIntent(),
        SingleActivator(LogicalKeyboardKey.escape): _DismissPanelIntent(),
      },
      actions: {
        ActivateIntent: CallbackAction<ActivateIntent>(
          onInvoke: (_) {
            _toggleByKeyboard();
            return null;
          },
        ),
        _DismissPanelIntent: CallbackAction<_DismissPanelIntent>(
          onInvoke: (_) {
            _hide(fromKeyboard: true);
            return null;
          },
        ),
      },
      child: content,
    );
  }

  Widget _buildPanel(BuildContext context) {
    final scrollable = SingleChildScrollView(
      controller: _scrollController,
      child: widget.panelBuilder(context),
    );
    if (widget.panelIgnoresPointer) {
      // Business interactions inside read-only content are disabled, but the
      // scrollbar thumb and the mouse wheel keep the bounded content
      // scrollable; the translucent listener receives wheel signals even
      // though the content below ignores pointers.
      return Scrollbar(
        controller: _scrollController,
        thumbVisibility: true,
        child: Listener(
          behavior: HitTestBehavior.translucent,
          onPointerSignal: _handlePointerSignal,
          child: IgnorePointer(child: scrollable),
        ),
      );
    }
    return Scrollbar(
      controller: _scrollController,
      thumbVisibility: true,
      child: MouseRegion(
        onEnter: (_) => _cancelHide(),
        onExit: (_) => _scheduleHide(),
        child: scrollable,
      ),
    );
  }

  void _handlePointerSignal(PointerSignalEvent event) {
    if (event is! PointerScrollEvent ||
        !_scrollController.hasClients ||
        _scrollController.position.maxScrollExtent <= 0) {
      return;
    }
    final position = _scrollController.position;
    final target = (position.pixels + event.scrollDelta.dy).clamp(
      position.minScrollExtent,
      position.maxScrollExtent,
    );
    _scrollController.jumpTo(target);
  }

  void _handleHover(bool hovering) {
    _hovering = hovering;
    widget.onHoverChange?.call(hovering);
    if (hovering) {
      _showTransient();
    } else {
      // Leaving the trigger re-enables hover shows for the next entry.
      _hoverReopenSuppressed = false;
      _scheduleHide();
    }
  }

  void _handlePointerDown() {
    // Clicking the trigger is an explicit interaction: focus it so the panel
    // can be dismissed with the keyboard right away, no matter where focus
    // was before.
    if (_focusNode.canRequestFocus) {
      _focusNode.requestFocus();
    }
    if (_isOpen && widget.pinOpenOnTap && _hovering) {
      // Legacy pin behavior: an open panel under the pointer stays open, and
      // the click focus keeps it from hiding on later hover exits.
      _cancelHide();
      return;
    }
    _toggle();
  }

  void _toggle() {
    if (_isOpen) {
      _hide();
    } else {
      _show();
    }
  }

  void _toggleByKeyboard() => _toggle();

  void _handleFocusChange(bool focused) {
    setState(() => _focused = focused);
    widget.onFocusChange?.call(focused);
    if (focused) {
      _cancelHide();
      // The focus granted by the current click must not re-show the panel
      // the same click is toggling; only genuine keyboard focus shows.
      if (widget.showOnFocus && !_pointerDownOnTrigger) {
        _showTransient();
      }
    } else {
      _scheduleHide();
    }
  }

  /// Shows the panel transiently: hover-held, never stealing focus, and
  /// hidden again after [StudioAnchoredPanel.hideDelay] once the pointer and
  /// focus have left.
  void _showTransient() {
    _cancelHide();
    if (_hoverReopenSuppressed || _isOpen) {
      return;
    }
    _show();
  }

  void _show() {
    _cancelHide();
    if (!widget.enabled || _isOpen) {
      return;
    }
    // Hover entry never takes focus; focus stays wherever the user was
    // working (for example a composer input being typed into).
    _surface?.open(focusTrigger: false);
  }

  void _handleOpened() {
    // Presentational open hook; the anchoring contract lives in the shared
    // surface.
  }

  void _handleClosed() {
    _cancelHide();
  }

  void _scheduleHide() {
    if (_focused) {
      _cancelHide();
      return;
    }
    _cancelHide();
    _hideTimer = Timer(widget.hideDelay, _hide);
  }

  void _cancelHide() {
    _hideTimer?.cancel();
    _hideTimer = null;
  }

  void _hide({bool fromKeyboard = false}) {
    _cancelHide();
    if (!_isOpen) {
      return;
    }
    if (_pointerDownOnTrigger || (fromKeyboard && _hovering)) {
      // A click closed the panel (or Escape did while the pointer rests on
      // the trigger); hover events from the same visit must not reopen it
      // until the pointer leaves once.
      _hoverReopenSuppressed = true;
    }
    // Keyboard dismissals restore focus to the trigger (which already holds
    // it for detector-driven Escapes); hover-timeout closes are native: no
    // focus is moved and nothing is armed.
    _surface?.close(
      reason: fromKeyboard
          ? StudioSurfaceCloseReason.keyboardDismiss
          : StudioSurfaceCloseReason.native,
    );
  }
}

class _DismissPanelIntent extends Intent {
  const _DismissPanelIntent();
}

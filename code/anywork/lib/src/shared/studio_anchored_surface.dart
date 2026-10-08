import 'package:flutter/material.dart';

import 'studio_anchor_space.dart';

export 'studio_anchor_space.dart' show StudioAnchorPlacement;

/// Why an anchored surface closed.
///
/// [selection] and [keyboardDismiss] restore focus to the real trigger and
/// arm the one-shot auto-open suppression for exactly that restore;
/// [outside] leaves focus with the newly tapped target; [native] covers the
/// framework-initiated closes (view-size changes, ancestor scrolls, an
/// unmeasurable trigger and disposal). Suppressions armed for a possible
/// native restore expire at the end of the frame in which they were armed,
/// so an unconsumed token can never swallow a later genuine keyboard entry.
enum StudioSurfaceCloseReason { selection, keyboardDismiss, outside, native }

/// The single Raw-driven anchored popup surface for Studio.
///
/// [RawMenuAnchor] owns the whole overlay lifecycle: the surface is attached
/// and detached with the anchor, closes on ancestor scrolls and view-size
/// changes, and reports the real anchor rectangle and overlay size every
/// frame. [StudioAnchorLayout] turns those into constraints and a position
/// derived from the actual laid-out child size in the same layout pass, so
/// the surface never opens from stale measurements, never estimates item
/// heights and never maintains a second overlay of its own.
///
/// The surface only anchors and shows content. Opening is refused while
/// [enabled] or [valid] is false, while the trigger has no measurable
/// on-screen bounds or while the effective surface size under the caller's
/// caps would be zero; an open surface renders nothing and closes safely
/// when any of these turns false while it stays open. Keyboard opens
/// focus the real trigger node; closing removes the overlay [FocusScope], and
/// the framework then restores focus to that trigger. Focus-driven triggers
/// that auto-open on keyboard focus consume [consumeAutoOpenSuppression] for
/// that one restore so the menu cannot bounce open again, while outside taps
/// still pass focus to the newly tapped target. Hover-driven opens pass
/// `focusTrigger: false` and leave focus untouched.
class StudioAnchoredSurface extends StatefulWidget {
  const StudioAnchoredSurface({
    required this.triggerBuilder,
    required this.contentBuilder,
    this.enabled = true,
    this.valid = true,
    this.userConstraints,
    this.fixedWidth,
    this.placement = StudioAnchorPlacement.vertical,
    this.focusTriggerNode,
    this.onOpen,
    this.onClose,
    super.key,
  });

  /// Builds the real trigger; the surface attaches its anchor key and
  /// keyboard bindings around it.
  final WidgetBuilder triggerBuilder;

  /// Builds the popup content shown inside the shared surface chrome; called
  /// on every rebuild so open surfaces refresh with the owning widget.
  final WidgetBuilder contentBuilder;

  /// Whether the surface may open.
  final bool enabled;

  /// Whether the currently open surface still shows valid content.
  ///
  /// Owners flip this to false while open when the surface must not stay
  /// open (the trigger became disabled, the row set lost its real
  /// identities). The invalid frame renders nothing and the surface closes
  /// after that frame, bounded to the session that went invalid: a user
  /// reopen that restores validity first is never closed by the stale
  /// request. Purely presentational refreshes (labels, selected hints,
  /// canonical data) keep this true and keep refreshing the open surface.
  final bool valid;

  /// Optional caller caps for the surface, further clamped to the measured
  /// space on every open and relayout.
  final BoxConstraints? userConstraints;

  /// Preferred surface width for fixed-width panels; menus leave this null
  /// and size to their widest row.
  final double? fixedWidth;

  /// Existing menus open vertically. Sidebar readouts can explicitly stay
  /// beside their trigger to leave adjacent rows selectable.
  final StudioAnchorPlacement placement;

  /// Focus node of the trigger's real focusable widget; keyboard opens
  /// request focus here, and closing the surface restores focus to it.
  final FocusNode? focusTriggerNode;

  /// Called after the surface opens.
  final VoidCallback? onOpen;

  /// Called after the surface closes.
  final VoidCallback? onClose;

  /// Vertical gap kept between the trigger and the surface in both
  /// directions.
  static const Offset gap = Offset(6, 6);

  @override
  State<StudioAnchoredSurface> createState() => StudioAnchoredSurfaceState();
}

class StudioAnchoredSurfaceState extends State<StudioAnchoredSurface> {
  final GlobalKey _anchorKey = GlobalKey(
    debugLabel: 'studio-anchored-surface-anchor',
  );
  final MenuController _controller = MenuController();
  final FocusScopeNode _scopeNode = FocusScopeNode(
    debugLabel: 'studio-anchored-surface-scope',
  );
  bool _openRequestedWithFocus = false;
  bool _suppressAutoOpenOnFocusGain = false;
  int _suppressionArmSeq = 0;
  StudioSurfaceCloseReason? _pendingCloseReason;
  int _closeSessionToken = 0;
  bool _sessionActive = false;
  bool _triggerFocusedInSession = false;

  FocusNode? get _triggerNode => widget.focusTriggerNode;

  @override
  void initState() {
    super.initState();
    _triggerNode?.addListener(_handleTriggerFocus);
  }

  @override
  void didUpdateWidget(StudioAnchoredSurface oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.focusTriggerNode != widget.focusTriggerNode) {
      oldWidget.focusTriggerNode?.removeListener(_handleTriggerFocus);
      _triggerNode?.addListener(_handleTriggerFocus);
    }
  }

  @override
  void dispose() {
    _suppressionArmSeq++;
    _closeSessionToken++;
    _triggerNode?.removeListener(_handleTriggerFocus);
    _scopeNode.dispose();
    super.dispose();
  }

  /// Whether the surface is currently open.
  bool get isOpen => _controller.isOpen;

  /// Opens the surface after the framework reports real anchor bounds.
  ///
  /// A no-op while open, disabled or invalid; the open request is refused
  /// entirely when the trigger is not measurably on screen with a usable
  /// side under the caller's own caps.
  /// [focusTrigger] decides whether the real trigger node takes focus for
  /// keyboard entry; hover-driven callers pass false and focus is left alone.
  void open({bool focusTrigger = true}) {
    if (!widget.enabled || !widget.valid || isOpen) {
      return;
    }
    // A new session invalidates any leftover suppression token and any
    // pending invalid-close request from a previous session.
    _suppressionArmSeq++;
    _closeSessionToken++;
    _suppressAutoOpenOnFocusGain = false;
    // Seed the session from the live focus state: click and keyboard entry
    // already focused the trigger by the time open runs, hover entry did not
    // touch it. Later gains while the session is active extend the record.
    _sessionActive = true;
    _triggerFocusedInSession = _triggerNode?.hasFocus ?? false;
    _openRequestedWithFocus = focusTrigger;
    _controller.open();
  }

  /// Closes the surface.
  ///
  /// [reason] decides the focus contract of this close; see
  /// [StudioSurfaceCloseReason]. Closing an already closed surface is a
  /// no-op.
  void close({
    StudioSurfaceCloseReason reason = StudioSurfaceCloseReason.native,
  }) {
    if (!isOpen) {
      _pendingCloseReason = null;
      return;
    }
    _pendingCloseReason = reason;
    _controller.close();
  }

  /// Toggles the surface open or closed.
  void toggle({bool focusTrigger = true}) {
    if (isOpen) {
      close();
    } else {
      open(focusTrigger: focusTrigger);
    }
  }

  /// Consumes the one-shot auto-open suppression armed when closing restored
  /// focus to the trigger.
  ///
  /// Triggers that auto-open on keyboard focus consult this in their focus
  /// listener: the restore after a keyboard dismiss or selection would
  /// otherwise look like a fresh keyboard entry and immediately re-open the
  /// surface. Returns true at most once per restore; every later focus gain
  /// keeps the trigger's own behavior.
  bool consumeAutoOpenSuppression() {
    if (!_suppressAutoOpenOnFocusGain) {
      return false;
    }
    _suppressionArmSeq++;
    _suppressAutoOpenOnFocusGain = false;
    return true;
  }

  void _handleOpenRequest(Offset? position, VoidCallback showOverlay) {
    if (!widget.enabled ||
        !widget.valid ||
        !StudioAnchorLayout.canAnchorSurface(
          context,
          anchorKey: _anchorKey,
          userConstraints: widget.userConstraints,
          fixedWidth: widget.fixedWidth,
          placement: widget.placement,
        )) {
      // Refuse instead of showing a surface that would cover its own trigger
      // or hang off-screen; the controller stays closed, no session starts,
      // and any pending invalid-close request is spent.
      _closeSessionToken++;
      _openRequestedWithFocus = false;
      _sessionActive = false;
      return;
    }
    final focusTrigger = _openRequestedWithFocus;
    _openRequestedWithFocus = false;
    showOverlay();
    if (focusTrigger) {
      // Mirrors RawMenuAnchor's own open: focus the trigger once the overlay
      // is up, so focus history restores to it when the scope goes away.
      final node = _triggerNode;
      if (node != null && node.canRequestFocus) {
        node.requestFocus();
      }
    }
  }

  void _handleCloseRequest(VoidCallback hideOverlay) {
    // The session ended: pending invalid-close requests must never fire
    // into whatever session opens next.
    _closeSessionToken++;
    _handleCloseReason(_pendingCloseReason ?? StudioSurfaceCloseReason.native);
    _pendingCloseReason = null;
    hideOverlay();
    _sessionActive = false;
  }

  /// Applies the focus contract for one close.
  ///
  /// [StudioSurfaceCloseReason.selection] and
  /// [StudioSurfaceCloseReason.keyboardDismiss] explicitly move focus back to
  /// the real trigger (before the overlay is hidden, so the scope removal has
  /// nothing left to restore) and arm the matching one-shot suppression —
  /// but only for sessions that entered through the trigger; hover sessions
  /// keep focus wherever it was. [StudioSurfaceCloseReason.outside] and
  /// native closes never steal focus from the new target, and only arm the
  /// suppression when the overlay scope still holds focus, because then the
  /// scope removal itself would restore focus to the trigger and read as
  /// keyboard entry.
  void _handleCloseReason(StudioSurfaceCloseReason reason) {
    final node = _triggerNode;
    if (node == null || !node.canRequestFocus || node.hasFocus) {
      return;
    }
    if (!_triggerFocusedInSession) {
      // Hover sessions never routed focus through the trigger.
      return;
    }
    switch (reason) {
      case StudioSurfaceCloseReason.selection:
      case StudioSurfaceCloseReason.keyboardDismiss:
        _armAutoOpenSuppression();
        node.requestFocus();
      case StudioSurfaceCloseReason.outside:
      case StudioSurfaceCloseReason.native:
        if (_scopeNode.hasFocus) {
          _armAutoOpenSuppression();
        }
    }
  }

  /// Arms the one-shot suppression and schedules it to expire at the end of
  /// the current frame.
  ///
  /// The matching focus change is flushed within the frame, so a token that
  /// no focus listener consumed (for example an outside tap whose target took
  /// focus first) is provably gone before the next keyboard entry. This is a
  /// frame-bounded lifetime, not a wall-clock timeout.
  void _armAutoOpenSuppression() {
    _suppressAutoOpenOnFocusGain = true;
    final seq = ++_suppressionArmSeq;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (seq == _suppressionArmSeq && _suppressAutoOpenOnFocusGain) {
        _suppressAutoOpenOnFocusGain = false;
      }
    });
  }

  /// Records that the trigger held focus at some point during the session.
  ///
  /// Only gains count: focus moving off the trigger into the items is the
  /// normal keyboard flow and must not clear the record. Events after the
  /// session ended (the close restore itself) are ignored, and the next
  /// [open] re-seeds the flag from live state.
  void _handleTriggerFocus() {
    if (_sessionActive && (_triggerNode?.hasFocus ?? false)) {
      _triggerFocusedInSession = true;
    }
  }

  /// Closes the surface after the current frame if the session that went
  /// invalid is still the open one and still invalid.
  ///
  /// The session token spends on every session boundary — open, refused
  /// open, any close and disposal — so a stale callback can never close a
  /// fresh session. The recheck reads only live state (the State's own
  /// context and the current widget), never a context captured inside an
  /// already-unmounted overlay build. Invalid frames render nothing, so no
  /// expired surface is shown while the close is pending.
  void _scheduleInvalidClose() {
    final token = _closeSessionToken;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (token != _closeSessionToken || !mounted || !isOpen) {
        return;
      }
      if (_isSessionInvalidNow()) {
        close();
      }
    });
  }

  /// Whether the currently open session must not stay open: the owner
  /// disabled the surface or declared its content invalid, or the trigger
  /// lost its usable anchoring space measured with the same caller caps
  /// ([StudioAnchoredSurface.userConstraints] and
  /// [StudioAnchoredSurface.fixedWidth]) the live layout applies.
  bool _isSessionInvalidNow() {
    if (!widget.enabled || !widget.valid) {
      return true;
    }
    return !StudioAnchorLayout.canAnchorSurface(
      context,
      anchorKey: _anchorKey,
      gap: StudioAnchoredSurface.gap,
      userConstraints: widget.userConstraints,
      fixedWidth: widget.fixedWidth,
      placement: widget.placement,
    );
  }

  @override
  Widget build(BuildContext context) {
    return RawMenuAnchor(
      controller: _controller,
      onOpenRequested: _handleOpenRequest,
      onCloseRequested: _handleCloseRequest,
      onOpen: widget.onOpen,
      onClose: widget.onClose,
      overlayBuilder: _buildOverlay,
      builder: (context, controller, child) =>
          KeyedSubtree(key: _anchorKey, child: widget.triggerBuilder(context)),
    );
  }

  Widget _buildOverlay(BuildContext context, RawMenuOverlayInfo info) {
    if (!widget.enabled || !widget.valid) {
      // The owner disabled the surface or declared the open content
      // invalid; render nothing and close after this frame unless the
      // session recovered first.
      _scheduleInvalidClose();
      return const SizedBox.shrink();
    }
    final layout = StudioAnchorLayout(
      info.anchorRect,
      info.overlaySize,
      MediaQuery.paddingOf(context),
      MediaQuery.viewInsetsOf(context),
      Directionality.of(context),
      userConstraints: widget.userConstraints,
      fixedWidth: widget.fixedWidth,
      placement: widget.placement,
      gap: StudioAnchoredSurface.gap,
    );
    if (!layout.anchorVisible || !layout.hasUsableSpace) {
      // The trigger left the visible region or the effective surface size
      // under the caller's caps collapsed to zero: render nothing and close
      // safely after the current frame (layout cannot close the overlay);
      // the post-frame recheck keeps a recovered session open.
      _scheduleInvalidClose();
      return const SizedBox.shrink();
    }
    return CustomSingleChildLayout(
      delegate: layout,
      child: _buildChrome(context, info),
    );
  }

  Widget _buildChrome(BuildContext context, RawMenuOverlayInfo info) {
    final theme = Theme.of(context);
    final style = MenuTheme.of(context).style;
    const states = <WidgetState>{};

    final backgroundColor =
        style?.backgroundColor?.resolve(states) ??
        theme.colorScheme.surfaceContainer;
    final surfaceTintColor =
        style?.surfaceTintColor?.resolve(states) ?? Colors.transparent;
    final shadowColor =
        style?.shadowColor?.resolve(states) ?? theme.colorScheme.shadow;
    final elevation = style?.elevation?.resolve(states) ?? 3.0;
    final side = style?.side?.resolve(states);
    final shape =
        (style?.shape?.resolve(states) ??
                const RoundedRectangleBorder(
                  borderRadius: BorderRadius.all(Radius.circular(4)),
                ))
            .copyWith(side: side);
    final padding =
        style?.padding?.resolve(states) ??
        const EdgeInsets.symmetric(vertical: 8);

    return TapRegion(
      groupId: info.tapRegionGroupId,
      onTapOutside: (_) => close(reason: StudioSurfaceCloseReason.outside),
      child: FocusScope(
        node: _scopeNode,
        skipTraversal: true,
        child: Actions(
          // The shared surface keeps only the universal close bookkeeping:
          // Escape dismisses an open surface wherever focus sits inside it.
          // Directional navigation and traversal grouping belong to the
          // owner's content (menus install their own row navigation), so
          // free-content surfaces keep their native keyboard semantics.
          actions: <Type, Action<Intent>>{
            DismissIntent: _DismissSurfaceAction(
              onDismiss: () =>
                  close(reason: StudioSurfaceCloseReason.keyboardDismiss),
            ),
          },
          child: Material(
            type: MaterialType.canvas,
            color: backgroundColor,
            surfaceTintColor: surfaceTintColor,
            shadowColor: shadowColor,
            elevation: elevation,
            shape: shape,
            clipBehavior: Clip.antiAlias,
            child: Padding(
              padding: padding,
              child: widget.contentBuilder(context),
            ),
          ),
        ),
      ),
    );
  }
}

/// Dismisses an anchored surface with an explicit close reason.
///
/// [DismissMenuAction] would call the raw controller without a reason, so
/// keyboard dismissals would be indistinguishable from native closes; this
/// action marks the close as keyboard-driven while behaving the same.
class _DismissSurfaceAction extends DismissAction {
  _DismissSurfaceAction({required this.onDismiss});

  final VoidCallback onDismiss;

  @override
  void invoke(DismissIntent intent) => onDismiss();

  @override
  bool isEnabled(DismissIntent intent) => true;
}

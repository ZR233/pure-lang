import 'dart:math' as math;

import 'package:flutter/material.dart';

/// Shared layout rules for an anchored popup surface around its real trigger.
///
/// The delegate runs inside the framework-owned overlay: [RawMenuAnchor]
/// supplies the real anchor rectangle, overlay size and text direction every
/// frame, and the delegate combines them with the safe-area padding, view
/// insets, a fixed [reservedMargin] from the window edge and the trigger
/// [gap] to decide the surface constraints and its final position from the
/// **actual** laid-out child size. There is no second overlay lifecycle, no
/// pre-measured style state and no estimated item heights: the size used for
/// positioning is the size the surface really paints with, in the same
/// layout pass.
class StudioAnchorLayout extends SingleChildLayoutDelegate {
  /// Creates a layout for one anchored surface.
  ///
  /// [anchorRect] and [overlaySize] come from [RawMenuOverlayInfo]; [padding]
  /// and [viewInsets] must be read from the overlay context so the safe
  /// rectangle describes the surface's real coordinate space.
  StudioAnchorLayout(
    this._anchorRect,
    this._overlaySize,
    this._padding,
    this._viewInsets,
    this._textDirection, {
    this.userConstraints,
    this.fixedWidth,
    this.gap = const Offset(0, 6),
  }) : safeRect = _reservedMargin.deflateRect(
         _padding.deflateRect(
           _viewInsets.deflateRect(Offset.zero & _overlaySize),
         ),
       );

  /// Fixed margin every anchored surface keeps from the safe-area edges on
  /// every side, in both text directions.
  static const double reservedMargin = 8.0;

  static const EdgeInsets _reservedMargin = EdgeInsets.all(reservedMargin);

  final Rect _anchorRect;
  final Size _overlaySize;
  final EdgeInsets _padding;
  final EdgeInsets _viewInsets;
  final TextDirection _textDirection;

  /// Optional caller caps applied on top of the measured space.
  final BoxConstraints? userConstraints;

  /// Preferred width for panels with a fixed content width; menus leave this
  /// null and size to their widest row.
  final double? fixedWidth;

  /// Space kept free between the trigger and the surface on both sides.
  final Offset gap;

  /// The overlay area the surface may occupy: safe padding and view insets
  /// excluded first, then [reservedMargin] on every side.
  final Rect safeRect;

  /// Usable height below the trigger (gap excluded).
  double get below => safeRect.bottom - (_anchorRect.bottom + gap.dy);

  /// Usable height above the trigger (gap excluded).
  double get above => (_anchorRect.top - gap.dy) - safeRect.top;

  /// Whether the anchor currently intersects the safe area at all; surfaces
  /// must not stay open for a trigger that left the visible region.
  bool get anchorVisible => _anchorRect.overlaps(safeRect);

  /// Whether the measured space still leaves a non-zero effective surface
  /// size after the caller's own caps ([userConstraints], [fixedWidth]).
  ///
  /// The raw side heights alone are not enough: a caller cap of zero width
  /// or height collapses the surface to nothing, and such a surface must
  /// neither open nor stay open. The probe ([canAnchorSurface]) and the
  /// live layout ([hasUsableSpace]) share this single definition, so an
  /// open request and an open overlay never disagree about the space.
  static bool hasUsableSpaceIn({
    required double safeWidth,
    required double below,
    required double above,
    BoxConstraints? userConstraints,
    double? fixedWidth,
  }) {
    if (safeWidth <= 0) {
      return false;
    }
    final effectiveWidth = fixedWidth != null
        ? math.min(fixedWidth, safeWidth)
        : math.min(userConstraints?.maxWidth ?? double.infinity, safeWidth);
    final effectiveHeight = math.min(
      userConstraints?.maxHeight ?? double.infinity,
      math.max(below, above),
    );
    return effectiveWidth > 0 && effectiveHeight > 0;
  }

  /// Whether the effective surface size for this layout is still non-zero.
  bool get hasUsableSpace => StudioAnchorLayout.hasUsableSpaceIn(
    safeWidth: safeRect.width,
    below: below,
    above: above,
    userConstraints: userConstraints,
    fixedWidth: fixedWidth,
  );

  @override
  BoxConstraints getConstraintsForChild(BoxConstraints constraints) {
    final availableWidth = math.max(0.0, safeRect.width);
    final maxSide = math.max(below, above);
    final maxHeight = math.max(
      0.0,
      math.min(userConstraints?.maxHeight ?? double.infinity, maxSide),
    );
    final width = fixedWidth;
    if (width != null) {
      final pinned = math.min(width, availableWidth);
      return BoxConstraints(
        minWidth: pinned,
        maxWidth: pinned,
        minHeight: 0,
        maxHeight: maxHeight,
      );
    }
    final maxWidth = math.min(
      userConstraints?.maxWidth ?? double.infinity,
      availableWidth,
    );
    return BoxConstraints(
      minWidth: math.min(userConstraints?.minWidth ?? 0.0, maxWidth),
      maxWidth: maxWidth,
      minHeight: 0,
      maxHeight: maxHeight,
    );
  }

  @override
  Offset getPositionForChild(Size size, Size childSize) {
    // Align to the leading text-direction edge of the trigger, clamped to the
    // safe rectangle so both LTR and RTL keep the window margin.
    final rightBound = math.max(
      safeRect.left,
      safeRect.right - childSize.width,
    );
    final double x =
        (_textDirection == TextDirection.rtl
                ? _anchorRect.right - childSize.width
                : _anchorRect.left)
            .clamp(safeRect.left, rightBound);

    // Open below the trigger first; flip above only when the real child size
    // does not fit below. The height constraint already caps the child to the
    // larger side, so one of the two always fits; the clamp is defensive.
    final downY = _anchorRect.bottom + gap.dy;
    if (downY + childSize.height <= safeRect.bottom) {
      return Offset(x, downY);
    }
    final upY = _anchorRect.top - gap.dy - childSize.height;
    if (upY >= safeRect.top) {
      return Offset(x, upY);
    }
    return Offset(
      x,
      below >= above
          ? downY
          : upY.clamp(
              safeRect.top,
              math.max(safeRect.top, safeRect.bottom - childSize.height),
            ),
    );
  }

  @override
  bool shouldRelayout(StudioAnchorLayout oldDelegate) {
    return _anchorRect != oldDelegate._anchorRect ||
        _overlaySize != oldDelegate._overlaySize ||
        _padding != oldDelegate._padding ||
        _viewInsets != oldDelegate._viewInsets ||
        _textDirection != oldDelegate._textDirection ||
        userConstraints != oldDelegate.userConstraints ||
        fixedWidth != oldDelegate.fixedWidth ||
        gap != oldDelegate.gap;
  }

  /// Whether [anchorKey] currently resolves to a laid-out trigger with a
  /// measurable on-screen rectangle and a non-zero effective surface size
  /// under the same caller caps the live layout would apply.
  ///
  /// Callers use this to refuse opening instead of showing a surface that
  /// would cover its own trigger or hang off-screen. The walk mirrors what
  /// [RawMenuAnchor] reports in [RawMenuOverlayInfo]: the anchor render box
  /// mapped into the nearest [Overlay] through its paint transform.
  static bool canAnchorSurface(
    BuildContext context, {
    required GlobalKey anchorKey,
    Offset gap = const Offset(0, 6),
    BoxConstraints? userConstraints,
    double? fixedWidth,
  }) {
    if (!context.mounted) {
      return false;
    }
    final anchorObject = anchorKey.currentContext?.findRenderObject();
    if (anchorObject is! RenderBox ||
        !anchorObject.attached ||
        !anchorObject.hasSize ||
        anchorObject.size.isEmpty) {
      return false;
    }
    final overlay = Overlay.maybeOf(context);
    final overlayObject = overlay?.context.findRenderObject();
    if (overlayObject is! RenderBox ||
        !overlayObject.attached ||
        !overlayObject.hasSize) {
      return false;
    }
    final anchorRect = MatrixUtils.transformRect(
      anchorObject.getTransformTo(overlayObject),
      Offset.zero & anchorObject.size,
    );
    final mediaQuery = MediaQuery.maybeOf(context);
    final allowedRect = _reservedMargin.deflateRect(
      (mediaQuery?.padding ?? EdgeInsets.zero).deflateRect(
        (mediaQuery?.viewInsets ?? EdgeInsets.zero).deflateRect(
          Offset.zero & overlayObject.size,
        ),
      ),
    );
    final below = allowedRect.bottom - (anchorRect.bottom + gap.dy);
    final above = (anchorRect.top - gap.dy) - allowedRect.top;
    if (!hasUsableSpaceIn(
      safeWidth: allowedRect.width,
      below: below,
      above: above,
      userConstraints: userConstraints,
      fixedWidth: fixedWidth,
    )) {
      return false;
    }
    return anchorRect.overlaps(allowedRect);
  }
}

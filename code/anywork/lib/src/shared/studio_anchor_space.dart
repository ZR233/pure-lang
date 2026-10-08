import 'dart:math' as math;

import 'package:flutter/material.dart';

/// Placement policy around the real trigger; all policies share the same
/// framework-owned overlay and safe-area measurements.
enum StudioAnchorPlacement { vertical, beside }

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
    this.placement = StudioAnchorPlacement.vertical,
    this.gap = const Offset(6, 6),
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

  /// Menus keep their vertical placement; sidebar readouts stay beside the row
  /// so that the surface cannot cover another row in the same column.
  final StudioAnchorPlacement placement;

  /// Space kept free between the trigger and the surface on both sides.
  final Offset gap;

  /// The overlay area the surface may occupy: safe padding and view insets
  /// excluded first, then [reservedMargin] on every side.
  final Rect safeRect;

  /// Usable height below the trigger (gap excluded).
  double get below => safeRect.bottom - (_anchorRect.bottom + gap.dy);

  /// Usable height above the trigger (gap excluded).
  double get above => (_anchorRect.top - gap.dy) - safeRect.top;

  double get _left => (_anchorRect.left - gap.dx) - safeRect.left;

  double get _right => safeRect.right - (_anchorRect.right + gap.dx);

  double get _availableWidth => math.max(
    0.0,
    placement == StudioAnchorPlacement.beside
        ? math.max(_left, _right)
        : safeRect.width,
  );

  double get _availableHeight => math.max(
    0.0,
    placement == StudioAnchorPlacement.beside
        ? safeRect.height
        : math.max(below, above),
  );

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
  bool get hasUsableSpace {
    // A beside readout may refuse an unreadably narrow side, but never falls
    // back to covering rows below its trigger. Existing vertical menus retain
    // their original cap/clamp semantics.
    if (placement == StudioAnchorPlacement.beside) {
      final effectiveWidth = math.min(
        fixedWidth ?? userConstraints?.maxWidth ?? double.infinity,
        _availableWidth,
      );
      if (effectiveWidth < (userConstraints?.minWidth ?? 0.0)) {
        return false;
      }
    }
    return StudioAnchorLayout.hasUsableSpaceIn(
      safeWidth: _availableWidth,
      below: _availableHeight,
      above: _availableHeight,
      userConstraints: userConstraints,
      fixedWidth: fixedWidth,
    );
  }

  @override
  BoxConstraints getConstraintsForChild(BoxConstraints constraints) {
    final availableWidth = _availableWidth;
    final maxSide = _availableHeight;
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
    if (placement == StudioAnchorPlacement.beside) {
      // Prefer the text-direction trailing side when the actual laid-out
      // width fits, otherwise flip. Constraints used the larger side, so the
      // card can shrink to that side without overlapping the trigger column.
      final preferRight = _textDirection == TextDirection.ltr;
      final preferredSpace = preferRight ? _right : _left;
      final oppositeSpace = preferRight ? _left : _right;
      final useRight = preferredSpace >= childSize.width
          ? preferRight
          : oppositeSpace >= childSize.width
          ? !preferRight
          : _right >= _left;
      final x = useRight
          ? _anchorRect.right + gap.dx
          : _anchorRect.left - gap.dx - childSize.width;
      return Offset(
        x.clamp(
          safeRect.left,
          math.max(safeRect.left, safeRect.right - childSize.width),
        ),
        _anchorRect.top.clamp(
          safeRect.top,
          math.max(safeRect.top, safeRect.bottom - childSize.height),
        ),
      );
    }
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
        placement != oldDelegate.placement ||
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
    Offset gap = const Offset(6, 6),
    BoxConstraints? userConstraints,
    double? fixedWidth,
    StudioAnchorPlacement placement = StudioAnchorPlacement.vertical,
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
    final layout = StudioAnchorLayout(
      anchorRect,
      overlayObject.size,
      mediaQuery?.padding ?? EdgeInsets.zero,
      mediaQuery?.viewInsets ?? EdgeInsets.zero,
      Directionality.of(context),
      userConstraints: userConstraints,
      fixedWidth: fixedWidth,
      placement: placement,
      gap: gap,
    );
    return layout.anchorVisible && layout.hasUsableSpace;
  }
}

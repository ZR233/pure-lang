part of 'timeline_view.dart';

/// Content growth is a layout correction, not a new scrolling activity.
/// Keeping it here avoids post-frame jumpTo calls that cancel pointer gestures.
class _TimelineScrollController extends ScrollController {
  _TimelineScrollController(this.followingBottom, this.restoreTarget)
    : super(keepScrollOffset: false);

  final bool Function() followingBottom;
  final double? Function() restoreTarget;

  @override
  ScrollPosition createScrollPosition(
    ScrollPhysics physics,
    ScrollContext context,
    ScrollPosition? oldPosition,
  ) => _TimelineScrollPosition(
    physics: physics,
    context: context,
    oldPosition: oldPosition,
    followingBottom: followingBottom,
    restoreTarget: restoreTarget,
  );
}

class _TimelineScrollPosition extends ScrollPositionWithSingleContext {
  _TimelineScrollPosition({
    required super.physics,
    required super.context,
    required super.oldPosition,
    required this.followingBottom,
    required this.restoreTarget,
  }) : super(keepScrollOffset: false);

  final bool Function() followingBottom;
  final double? Function() restoreTarget;

  @override
  bool applyContentDimensions(double minScrollExtent, double maxScrollExtent) {
    // Resolve both initial placement and preview -> full-body restoration
    // against this layout's bounds, before an idle activity can start a bounce
    // using a stale, temporarily out-of-range coordinate.
    final target = followingBottom() ? maxScrollExtent : restoreTarget();
    if (target != null) {
      final clamped = target.clamp(minScrollExtent, maxScrollExtent).toDouble();
      if (pixels != clamped) {
        correctPixels(clamped);
        return false;
      }
    }
    return super.applyContentDimensions(minScrollExtent, maxScrollExtent);
  }

  @override
  bool correctForNewDimensions(
    ScrollMetrics oldPosition,
    ScrollMetrics newPosition,
  ) {
    // applyContentDimensions has already resolved explicit layout intent. Do
    // not let RangeMaintainingScrollPhysics reinterpret that coordinate using
    // the old window's bounds. Manual scrolling retains Flutter's physics.
    if (followingBottom() || restoreTarget() != null) return true;
    return super.correctForNewDimensions(oldPosition, newPosition);
  }
}

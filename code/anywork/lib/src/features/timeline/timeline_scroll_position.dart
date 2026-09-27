part of 'timeline_view.dart';

/// Content growth is a layout correction, not a new scrolling activity.
/// Keeping it here avoids post-frame jumpTo calls that cancel pointer gestures.
class _TimelineScrollController extends ScrollController {
  _TimelineScrollController(this.followingBottom)
    : super(keepScrollOffset: false);

  final bool Function() followingBottom;

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
  );
}

class _TimelineScrollPosition extends ScrollPositionWithSingleContext {
  _TimelineScrollPosition({
    required super.physics,
    required super.context,
    required super.oldPosition,
    required this.followingBottom,
  }) : super(keepScrollOffset: false);

  final bool Function() followingBottom;

  @override
  bool applyContentDimensions(double minScrollExtent, double maxScrollExtent) {
    // Follow the tail during layout, including corrections to estimated heights.
    // Explicit history restoration uses the indexed list and painted row geometry.
    final target = followingBottom() ? maxScrollExtent : null;
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
    if (followingBottom()) return true;
    return super.correctForNewDimensions(oldPosition, newPosition);
  }
}

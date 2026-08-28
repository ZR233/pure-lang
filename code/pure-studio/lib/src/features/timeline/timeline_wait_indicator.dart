part of 'timeline_view.dart';

/// Timeline 专用的小型“活时间线”脉冲。
///
/// 三颗陶土色节点沿时间线依次呼吸，只强调当前活动的等待，避免通用大 spinner。
/// 动画由受控 [AnimationController] 驱动：`MediaQuery.disableAnimationsOf` 或
/// `TickerMode` 禁用时不启动循环 ticker，改为绘制同尺寸的静态节点。整个组件
/// 包在 [ExcludeSemantics] 中，装饰性节点不参与朗读；稳定、可参数化的 key 由
/// 调用方通过 [key] 提供。
class TimelineWaitIndicator extends StatefulWidget {
  const TimelineWaitIndicator({this.active = true, super.key});

  /// 是否运行循环动画；为 false 时仅呈现静态节点。
  final bool active;

  @override
  State<TimelineWaitIndicator> createState() => _TimelineWaitIndicatorState();
}

class _TimelineWaitIndicatorState extends State<TimelineWaitIndicator>
    with SingleTickerProviderStateMixin {
  /// 单次完整呼吸周期（三颗节点依次错峰），落在 0.9–1.2 秒的节奏区间。
  static const _cycle = Duration(milliseconds: 1050);

  /// 每一颗节点相对前一颗的相位偏移。
  static const _stagger = 0.33;

  static const _dotSize = 6.0;
  static const _gap = 3.0;

  late final AnimationController _controller;

  @override
  void initState() {
    super.initState();
    _controller = AnimationController(vsync: this, duration: _cycle);
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _syncAnimation();
  }

  @override
  void didUpdateWidget(covariant TimelineWaitIndicator oldWidget) {
    super.didUpdateWidget(oldWidget);
    _syncAnimation();
  }

  bool get _reducedMotion =>
      MediaQuery.disableAnimationsOf(context) ||
      !TickerMode.valuesOf(context).enabled;

  void _syncAnimation() {
    if (widget.active && !_reducedMotion) {
      if (!_controller.isAnimating) {
        _controller.repeat();
      }
    } else {
      _controller.stop();
      _controller.value = 0;
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final color = StudioColors.clay;
    final reducedMotion = _reducedMotion;
    return ExcludeSemantics(
      child: SizedBox(
        width: _dotSize * 3 + _gap * 2,
        height: _dotSize,
        child: AnimatedBuilder(
          animation: _controller,
          builder: (context, _) {
            final shouldAnimate = !reducedMotion && widget.active;
            if (!shouldAnimate) {
              return _staticDots(color);
            }
            final t = _controller.value;
            return Row(
              crossAxisAlignment: CrossAxisAlignment.center,
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                for (var i = 0; i < 3; i++) ...[
                  if (i > 0) const SizedBox(width: _gap),
                  _breathingDot(color, i, t),
                ],
              ],
            );
          },
        ),
      ),
    );
  }

  Widget _staticDots(Color color) {
    final size = _dotSize;
    return Row(
      crossAxisAlignment: CrossAxisAlignment.center,
      mainAxisAlignment: MainAxisAlignment.center,
      children: [
        for (var i = 0; i < 3; i++) ...[
          if (i > 0) const SizedBox(width: _gap),
          Container(
            width: size,
            height: size,
            decoration: BoxDecoration(color: color, shape: BoxShape.circle),
          ),
        ],
      ],
    );
  }

  Widget _breathingDot(Color color, int index, double t) {
    final phase = ((t - _stagger * index) % 1.0).abs();
    final breathing = math.sin(math.pi * phase).clamp(0.0, 1.0);
    final scale = (0.72 + 0.28 * breathing).toDouble();
    final opacity = (0.35 + 0.65 * breathing).clamp(0.0, 1.0).toDouble();
    return Transform.scale(
      alignment: Alignment.center,
      scale: scale,
      child: Opacity(
        opacity: opacity,
        child: Container(
          width: _dotSize,
          height: _dotSize,
          decoration: BoxDecoration(color: color, shape: BoxShape.circle),
        ),
      ),
    );
  }
}

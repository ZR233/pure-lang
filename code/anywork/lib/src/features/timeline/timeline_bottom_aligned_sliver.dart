part of 'timeline_view.dart';

/// Align a short lazy list to the bottom without adding scrollable padding.
/// The single forward list supplies the full extent once it fits the viewport.
class _BottomAlignedSliver extends SingleChildRenderObjectWidget {
  const _BottomAlignedSliver({required this.onSlackChanged, super.child});

  /// 布局期上报当前前导留白 `L`（内容比视口长时为 0）。
  ///
  /// 调用方用它把「视觉位置」换算成「内容自身偏移」，从而让锚点记录不受贴底留白影响。
  /// 上报发生在布局期，只写调用方的普通字段，不触发 rebuild，也不改变几何。
  final ValueChanged<double> onSlackChanged;

  @override
  _RenderBottomAlignedSliver createRenderObject(BuildContext context) {
    return _RenderBottomAlignedSliver(onSlackChanged);
  }

  @override
  void updateRenderObject(
    BuildContext context,
    _RenderBottomAlignedSliver renderObject,
  ) {
    renderObject.onSlackChanged = onSlackChanged;
  }
}

class _RenderBottomAlignedSliver extends RenderSliverEdgeInsetsPadding {
  _RenderBottomAlignedSliver(this._onSlackChanged);

  ValueChanged<double> _onSlackChanged;

  ValueChanged<double> get onSlackChanged => _onSlackChanged;

  /// 布局期留白接收者。
  ///
  /// 更换接收者时立刻补报一次当前留白：留白只在布局期变化，新所有者若只等下一次
  /// 变化，就会一直拿着过期的值。补报同样只写调用方的普通字段，不触发 rebuild、
  /// 不改变几何（幂等，重复补报无副作用）。
  set onSlackChanged(ValueChanged<double> value) {
    if (identical(value, _onSlackChanged)) {
      return;
    }
    _onSlackChanged = value;
    value(_slack);
  }

  double _slack = 0;

  /// 自身不额外加内边距：贴底只通过 `paintOrigin` 表达，避免把留白并进 `scrollExtent`。
  @override
  EdgeInsets? get resolvedPadding => EdgeInsets.zero;

  @override
  void performLayout() {
    super.performLayout();
    final geometry = this.geometry;
    if (geometry == null || geometry.scrollOffsetCorrection != null) {
      return;
    }
    final slack = math.max(
      0.0,
      constraints.viewportMainAxisExtent -
          constraints.precedingScrollExtent -
          geometry.scrollExtent,
    );
    if (slack > 0) {
      // 只挪绘制原点：`scrollExtent`（进而是 `maxScrollExtent`）保持等于内容高度。
      this.geometry = geometry.copyWith(
        paintOrigin: geometry.paintOrigin + slack,
      );
    }
    if ((slack - _slack).abs() > 0.01) {
      _slack = slack;
      _onSlackChanged(slack);
    }
  }
}

part of 'timeline_view.dart';

/// 把「视口剩余空间」压到子 sliver 内容前方（不改动滚动范围）的 sliver。
///
/// 它只放在 `CustomScrollView.center` 的第一个正向位置（也就是 center 本身），
/// 并且包住正向区里的全部内容（消息行 + 收束区）。
///
/// 几何（设 `V = viewportMainAxisExtent`、`P = precedingScrollExtent`、
/// `G = 子 sliver 的 scrollExtent`，`L = max(0, V - P - G)`）：
///
/// - `scrollExtent = G`，**不把 `L` 写进滚动范围**：内容比视口短时
///   `maxScrollExtent = max(0, G - V) = 0`，留白不可能被滚动出来；
/// - 整段内容通过 `paintOrigin = L` 下移，因此内容比视口短时它整体贴住视口底部；
/// - 内容比视口长时 `L = 0`，本 sliver 与「直接放子 sliver」完全等价，
///   `center`/item 锚点语义不变；
/// - `L` 由本帧的 `G` 直接算出并立刻参与本帧几何，不存在「先画在顶部、下一帧
///   再靠 setState/paint 纠正」的反馈环，因此首帧就是最终位置。
///
/// 贴底只在 [alignShortContentToBottom] 成立时生效，也就是反向区为空（`center`
/// 是第一行）的时候。此时 `minScrollExtent == maxScrollExtent == 0`，唯一合法的
/// 滚动位置是 0，加 `paintOrigin` 不可能与反向区/负向偏移互相错位。一旦用户上翻
/// 历史把 `center` 拆到中间，反向区非空，本 sliver 就退化成普通 sliver，几何与
/// 改动前逐帧一致（`L = 0`）：反向区由视口的负向通道按 `V - centerOffset` 定位，
/// 不会与这里的位移打架。
class _BottomAlignedSliver extends SingleChildRenderObjectWidget {
  const _BottomAlignedSliver({
    required this.alignShortContentToBottom,
    required this.onSlackChanged,
    super.child,
    super.key,
  });

  /// 内容比视口短时是否把剩余空间压到内容前方（贴底）。
  ///
  /// 只在正向区独占整个滚动内容（反向区为空）时为 true；详见类文档。
  final bool alignShortContentToBottom;

  /// 布局期上报当前前导留白 `L`（内容比视口长时为 0）。
  ///
  /// 调用方用它把「视觉位置」换算成「内容自身偏移」，从而让锚点记录不受贴底留白影响。
  /// 上报发生在布局期，只写调用方的普通字段，不触发 rebuild，也不改变几何。
  final ValueChanged<double> onSlackChanged;

  @override
  _RenderBottomAlignedSliver createRenderObject(BuildContext context) {
    return _RenderBottomAlignedSliver(
      alignShortContentToBottom,
      onSlackChanged,
    );
  }

  @override
  void updateRenderObject(
    BuildContext context,
    _RenderBottomAlignedSliver renderObject,
  ) {
    renderObject.alignShortContentToBottom = alignShortContentToBottom;
    renderObject.onSlackChanged = onSlackChanged;
  }
}

class _RenderBottomAlignedSliver extends RenderSliverEdgeInsetsPadding {
  /// 参数顺序与 [_BottomAlignedSliver] 一致：贴底开关、留白接收者。
  _RenderBottomAlignedSliver(
    this._alignShortContentToBottom,
    this._onSlackChanged,
  );

  bool _alignShortContentToBottom;

  /// 内容比视口短时是否把剩余空间压到内容前方（贴底）。
  ///
  /// 这个属性参与几何（`paintOrigin`），所以改变时必须重新布局：约束不变、只有该
  /// 参数改变时不能沿用上一帧的布局结果。
  bool get alignShortContentToBottom => _alignShortContentToBottom;

  set alignShortContentToBottom(bool value) {
    if (_alignShortContentToBottom == value) {
      return;
    }
    _alignShortContentToBottom = value;
    markNeedsLayout();
  }

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
    final slack = !alignShortContentToBottom
        ? 0.0
        : math.max(
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

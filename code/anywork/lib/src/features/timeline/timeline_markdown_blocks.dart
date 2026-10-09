part of 'timeline_view.dart';

enum _MarkdownSurface { assistant, user, panel, reasoning, error }

// 原始字符串（r''）把 \r \n \\ \[ \] 作为**转义**交给 RegExp，因此这里匹配真实 CR/LF、
// 反斜杠与 markdown 符号（反引号、* _ [ ] ( ) # > ~ ! |），而不是字母 r/n；含换行的
// 正文因此走 Markdown 分支。非原始字符串里 \r/\n 会变成真实控制字符、\[ 还会成为非法
// 转义，所以这里必须保持 raw。
final RegExp _streamingMarkdownSyntax = RegExp(r'[\r\n\\`*_\[\]()#>~!|]');

class _AgentMarkdown extends ConsumerWidget {
  const _AgentMarkdown({
    required this.id,
    required this.status,
    required this.text,
    required this.surface,
  });

  final String id;
  final String status;
  final String text;
  final _MarkdownSurface surface;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final plainText =
        !_streamingMarkdownSyntax.hasMatch(text) &&
        !text.contains('://') &&
        !text.contains('www.');
    if (plainText) {
      return _PlainBodyText(
        key: ValueKey('plain-$id'),
        id: id,
        status: status,
        text: text,
        surface: surface,
      );
    }
    final repaired = repairAgentMarkdownForDisplay(text);
    final imageAlts = repaired.contains('![')
        ? _markdownImageAlts(repaired)
        : const <String, String>{};
    final scheme = Theme.of(context).colorScheme;
    final onUserSurface = surface == _MarkdownSurface.user;
    return GptMarkdownTheme(
      // VS Code 预览的标题阶梯：全部 w600、line-height 1.25，字级按正文字号
      // 相对递进（2/1.5/1.25/1/0.875/0.85em）。只能在 GptMarkdownThemeData
      // 上按级提供；用局部 InheritedWidget 覆盖，不改动全局主题。
      gptThemeData: GptMarkdownTheme.of(context).copyWith(
        h1: _timelineHeadingStyle(context, surface, 2.0),
        h2: _timelineHeadingStyle(context, surface, 1.5),
        h3: _timelineHeadingStyle(context, surface, 1.25),
        h4: _timelineHeadingStyle(context, surface, 1.0),
        h5: _timelineHeadingStyle(context, surface, 0.875),
        h6: _timelineHeadingStyle(context, surface, 0.85),
      ),
      child: GptMarkdown(
        repaired,
        key: ValueKey('gpt-markdown-$id-$status'),
        style: _markdownBodyStyle(context, surface),
        styleSheet: GptMarkdownStyleSheet(
          blockQuote: BlockQuoteStyle(
            textStyle: TextStyle(
              color: context.colors.onSurfaceVariant,
              height: 1.57,
            ),
          ),
          heading: HeadingStyle(
            // 仅做随表面的语义色覆盖；节奏与 H1/H2 分隔线由 headingBuilder 提供。
            textStyle: switch (surface) {
              _MarkdownSurface.reasoning => TextStyle(
                color: context.colors.onSurfaceVariant,
              ),
              _MarkdownSurface.error => TextStyle(color: scheme.error),
              _ => null,
            },
          ),
          list: ListStyle(
            // 有序/无序列表统一标记距离；标记色弱化，正文保持主导。
            indent: 8,
            gapAfterMarker: 8,
            bulletColor: scheme.onSurfaceVariant,
            markerTextStyle: TextStyle(
              fontWeight: FontWeight.w500,
              color: scheme.onSurfaceVariant,
            ),
          ),
          table: TableStyle(
            // VS Code 表格：无表头背景、无外圆角，浅色细网格做清楚分隔。
            borderColor: scheme.outlineVariant,
            headerBackground: Colors.transparent,
            headerTextStyle: const TextStyle(fontWeight: FontWeight.w600),
            cellPadding: const EdgeInsets.symmetric(
              horizontal: 10,
              vertical: 5,
            ),
          ),
        ),
        headingBuilder: (context, level, content, style) {
          final bodySize = _markdownBodyStyle(context, surface)?.fontSize ?? 14;
          // VS Code 预览节奏：块间空行承担约一段距（≈1.15em），标题只补
          // margin-top 24px（相对正文 ≈1.7em）；H3-H6 底距交还空行节奏。
          // H1/H2 自带 0.3em 底距与 1px 轻分隔线（padding 在线之上）。
          // 选择顺序：标题整树包 _HeadingSelectionUnit 作为块级选择单元，
          // 外层选择区域见块级边界而非窄标题文本盒，几何排序不再因文本
          // 盒 top 偏移把标题排到后续正文之后（复制错序）。
          final topPadding = bodySize * 24 / 14;
          final Widget heading;
          if (level > 2) {
            heading = LayoutBuilder(
              builder: (context, constraints) {
                // 有界阅读列内标题占满列宽，块级选择边界覆盖整行；
                // 无界约束保持内容宽度，避免 infinity 布局错误。
                return constraints.hasBoundedWidth
                    ? SizedBox(width: double.infinity, child: content)
                    : content;
              },
            );
          } else {
            final headingSize = bodySize * (level == 1 ? 2.0 : 1.5);
            heading = LayoutBuilder(
              builder: (context, constraints) {
                final rule = DecoratedBox(
                  decoration: BoxDecoration(
                    border: Border(
                      bottom: BorderSide(
                        color: Theme.of(context).colorScheme.outlineVariant,
                        width: 1,
                      ),
                    ),
                  ),
                  child: Padding(
                    padding: EdgeInsets.only(bottom: headingSize * 0.3),
                    child: content,
                  ),
                );
                // 有界阅读列内底线占满列宽、内容保持起始对齐；
                // 无界约束保持内容宽度，避免 infinity 布局错误。
                return constraints.hasBoundedWidth
                    ? SizedBox(width: double.infinity, child: rule)
                    : rule;
              },
            );
          }
          return _HeadingSelectionUnit(
            child: Padding(
              padding: EdgeInsets.only(top: topPadding),
              child: heading,
            ),
          );
        },
        onLinkTap: (url, _) {
          unawaited(_openTimelineWebLink(context, ref, url));
        },
        inlineLinkBuilder: (link) => safeExternalWebUrl(link.url) == null
            ? TextSpan(children: link.labelSpans, style: link.style)
            : link.defaultSpan(),
        inlineCodeStyle: InlineCodeStyle(
          // 包注册字体（family+package 由 InlineCodeStyle.applyTo 拼成
          // packages/gpt_markdown/JetBrainsMono）；不传 fallback 以继承
          // app 正文字体链，中文代码字形回落 Noto Sans SC 而非衬线。
          fontFamily: kGptMarkdownMonoFontFamily,
          fontFamilyPackage: kGptMarkdownFontPackage,
          fontSizeFactor: 1.0,
          color: scheme.onSurface,
          backgroundColor: onUserSurface
              ? scheme.surfaceContainerLowest
              : scheme.surfaceContainerLow,
          // 只用轻底色区分，去掉描边和加粗，避免打断中文阅读基线。
          borderWidth: 0,
          borderRadius: Radius.circular(3),
          padding: const EdgeInsets.symmetric(horizontal: 5, vertical: 1.5),
        ),
        // VS Code 预览引用：常规左侧线 + 低对比背景，无垂直外边距
        // （块间空行已承担节奏），内边距 0 16 0 10、小圆角。
        blockQuoteBuilder: (context, content, style) => LayoutBuilder(
          builder: (context, constraints) {
            final card = DecoratedBox(
              key: const ValueKey('studio-markdown-quote'),
              decoration: BoxDecoration(
                color: scheme.surfaceContainerLowest,
                border: BorderDirectional(
                  start: BorderSide(color: scheme.outlineVariant, width: 5),
                ),
                borderRadius: BorderRadius.circular(2),
              ),
              child: Padding(
                padding: const EdgeInsetsDirectional.fromSTEB(10, 0, 16, 0),
                child: content,
              ),
            );
            // 宽度有界时占满可用宽度，与其他块级内容右边缘对齐；
            // 无界约束（如横向滚动内）保持内容宽度，避免 infinity 布局错误。
            return constraints.hasBoundedWidth
                ? SizedBox(width: double.infinity, child: card)
                : card;
          },
        ),
        imageBuilder: (context, url, _, _) =>
            _studioMarkdownImage(context, url, imageAlts[url] ?? '', surface),
        codeBuilder: (context, name, code, closed) {
          final scheme = Theme.of(context).colorScheme;
          final textTheme = Theme.of(context).textTheme;
          // VS Code 预览代码区：低对比背景 + 1px 边框，不显示语言栏；
          // 垂直节奏交还块间空行。
          final codeBackground = onUserSurface
              ? scheme.surfaceContainerHigh
              : scheme.surfaceContainerLow;
          final bodyStyle = surface == _MarkdownSurface.reasoning
              ? textTheme.bodySmall
              : textTheme.bodyMedium;
          return StudioCodeBlock(
            text: code,
            margin: EdgeInsets.zero,
            padding: const EdgeInsets.all(16),
            backgroundColor: codeBackground,
            borderColor: scheme.outlineVariant,
            borderRadius: const Radius.circular(3),
            textStyle: bodyStyle?.copyWith(
              color: surface == _MarkdownSurface.reasoning
                  ? context.colors.onSurfaceVariant
                  : surface == _MarkdownSurface.error
                  ? scheme.error
                  : scheme.onSurface,
              // 包注册字体；不传 fallback，中文继承 app 正文字体链。
              fontFamily: _monoFontFamily,
              fontSize: bodyStyle.fontSize ?? 14,
              height: 1.36,
            ),
          );
        },
      ),
    );
  }

  /// VS Code 预览标题：相对所在 surface 正文字号的递进字级（em），
  /// 全部 w600、line-height 1.25；从全局正文字体派生保持家族继承。
  TextStyle? _timelineHeadingStyle(
    BuildContext context,
    _MarkdownSurface surface,
    double em,
  ) {
    final bodySize = _markdownBodyStyle(context, surface)?.fontSize ?? 14;
    return Theme.of(context).textTheme.bodyMedium?.copyWith(
      fontSize: bodySize * em,
      fontWeight: FontWeight.w600,
      height: 1.25,
    );
  }
}

/// gpt_markdown 包注册的等宽字体在宿主中的完整 family 名
/// （见 InlineCodeStyle.applyTo 的 `packages/<package>/<family>` 拼法）。
const String _monoFontFamily =
    'packages/$kGptMarkdownFontPackage/$kGptMarkdownMonoFontFamily';

/// 标题的块级选择单元：内层标题文本注册到局部
/// [StaticSelectionContainerDelegate]，外层选择区域只见 [SelectionContainer]
/// 的 RenderBox 边界（paintBounds，覆盖标题完整 padding 与底线），而不是
/// 窄标题文本盒——几何选择排序因此把标题当作与后续正文平级的块单元，
/// 不会因文本盒 top 偏移把标题排到正文之后（复制错序）。标题仍参与外层
/// 连续选择：委托嵌套是 SDK 官方用法（SelectableRegion 自身即用
/// SelectionContainer 嵌套委托），不通过独立文本编辑控件隔离标题，也不
/// 改变全局选择排序。
class _HeadingSelectionUnit extends StatefulWidget {
  const _HeadingSelectionUnit({required this.child});

  final Widget child;

  @override
  State<_HeadingSelectionUnit> createState() => _HeadingSelectionUnitState();
}

class _HeadingSelectionUnitState extends State<_HeadingSelectionUnit> {
  // State 拥有 delegate 生命周期（create once / dispose），与 SDK
  // SelectionListenerContainer 同构：SelectionContainer 自身 dispose 时
  // 只清理引用，不负责销毁 delegate。
  final StaticSelectionContainerDelegate _delegate =
      StaticSelectionContainerDelegate();

  @override
  void dispose() {
    _delegate.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return SelectionContainer(delegate: _delegate, child: widget.child);
  }
}

/// 纯文本正文的**分块**渲染（只对超长、纯 LTR 的普通正文生效）。
///
/// 背景与归因：`main26-ui19-large/stress-body-report.json` 记录了 640K 单段纯文本正文
/// 出现 155ms 帧。把这些帧归因于"纯文本分支用单个 [Text]、每来一个增量都对整段做一次
/// 段落布局"是**可证推断**（该正文不含 markdown 语法、确实走单个 [Text]，段落布局成本随
/// 正文长度增长），不是已做 profile 的结论。
///
/// 这里把已稳定的行按真实换行边界切成有界块，每块复用同一个 [Text] 实例——实例不变时
/// Flutter 跳过重建与重排——只对仍在增长的尾块重新布局。收益是**有条件的**：
///
/// - 逐帧追加（流式）：单次 build 新封存 ≤ `_sealBatchLimit` × `_sealMeasureCharacters`，
///   正常增量下每帧只封存一块，因此每帧布局成本 ≈ 尾块，与整段长度无关。
/// - 一次性换入超大正文（例如重开历史）：该帧仍会包含一个未及封存的大块尾部，其布局与
///   整段同阶（与优化前同级，无回归）；后续各帧继续按批封存。
/// - **整体退回单个 [Text]**：方向不是 LTR，或当前正文（未封存后缀）含 RTL/bidi 控制
///   字符——此时已封存前缀也会被清掉，不留"已分块前缀 + 风险正文"。
/// - **只是不再新增块**：测量窗内量不出安全断点（单个超长无空白 run、CJK 无空格断行、
///   缩进续行等）时保留已封存前缀 + 单个尾块 [Text]；尾块起点仍是真实行起点，所以这样
///   是安全的，只是收益到此为止——这种情况不等于整体回退，不伪称"O(尾块)"覆盖所有正文。
///
/// 正确性约束：
///
/// - 断点取自 [TextPainter] 在**有界测量窗**内、用**同一 style/宽度/TextScaler/方向/
///   locale** 量出的真实行起点，且要求上一行以空白结束、下一行以非空白开始；各块单独
///   换行与整段一致（贪心断行只取决于断点之前的正文），块内文本按顺序拼接仍等于原文
///   ——不插换行、不丢空白、不截尾。
/// - 复用前对**每一块**做逐字精确比较，不做首尾采样、也不用哈希近似：同 identity 的
///   Replace 可以只改中间而首尾不变，只有整个已封存前缀逐字一致才允许复用。
/// - 复用前还对**本次 canonical 正文**做一次风险判定：已封存前缀由逐字校验证明未变（且
///   当初是在无风险判定下封存的），未封存后缀整段扫描，因此不会漏掉短于阈值的尾巴或
///   Replace 进尾巴的风险字符。
/// - 跨多个 [Text]（各自一个 RenderParagraph）的 `SelectionArea` 复制内容按 SDK 契约
///   直接顺序拼接各 Selectable 的 plainText、不插分隔（`widgets/selectable_region.dart`
///   的 `getSelectedContent`），因此应与整段一致；该契约与真机的选择手柄/复制表现仍需实机
///   确认，这里不当作已验证结论。
/// - 缓存与正文同源、与 [id] 绑定：换会话/换条目、正文被替换、宽度/文字缩放/方向/locale
///   变化时整体失效重建，既不产生第二份正文事实源，也不会无界增长。
class _PlainBodyText extends StatefulWidget {
  const _PlainBodyText({
    required this.id,
    required this.status,
    required this.text,
    required this.surface,
    super.key,
  });

  final String id;
  final String status;
  final String text;
  final _MarkdownSurface surface;

  @override
  State<_PlainBodyText> createState() => _PlainBodyTextState();
}

class _PlainBodyTextState extends State<_PlainBodyText> {
  /// 只有超过这么多字符的正文才切块；普通消息保持单个 [Text]。
  static const _sealMinimumCharacters = 4096;

  /// 单个块的期望上限（字符）：断点在不超过它的前提下尽可能靠近。
  static const _sealTargetCharacters = 4096;

  /// 测量窗上限（目标块长 + 尾部上下文）：保证每次 [TextPainter] 测量有界，且断点所在
  /// 行的换行不会因为人为截断而改变。
  static const _sealMeasureCharacters = 6144;

  /// 单次 build 最多新封存的块数，把一次 build 的封存工作量收成有界。
  // Keep each build bounded while allowing the sealed prefix to catch up with
  // a streaming response.  Each pass lays out only a 6 KiB window.
  static const _sealBatchLimit = 2;

  final List<_PlainBodyChunk> _sealed = <_PlainBodyChunk>[];
  int _sealedEnd = 0;
  double? _width;
  TextScaler? _textScaler;
  TextDirection? _textDirection;
  TextStyle? _style;
  Locale? _locale;

  /// 最近一次"未封存后缀是否含 RTL/bidi"判定的结果；只在同一 String 实例且起点相同时复用，
  /// 因此任何内容变化都会重新整段扫描（不采样）。
  String? _riskText;
  int _riskStart = 0;
  bool _riskVerdict = false;

  /// 上一次已验证的 canonical 正文。流式响应通常只追加尾部，可据此跳过已封存
  /// 前缀的重复逐字比较；发生中间替换时仍回退到完整校验。
  String? _validatedText;
  String? _validatedId;

  /// 内容子树的稳定身份。
  ///
  /// 流式结束只是外层「禁用选择」的包装被去掉；用同一个 [GlobalKey] 让内容子树原样
  /// 搬移而不是整棵重建，否则终态那一帧会把整段正文重新布局一次，抵消流式期间的收益。
  late final GlobalKey _contentKey = GlobalKey();

  void _resetSealed() {
    _sealed.clear();
    _sealedEnd = 0;
    _validatedText = null;
  }

  /// 精确校验已封存前缀：逐块与当前正文对应位置比较，全部一致才允许复用。
  ///
  /// 只比对首尾块是采样，无法排除"同 identity 的 Replace 只改中间"；这里对每一块做连续
  /// 字符串比较（memcmp 级，不触发布局，也不用哈希近似），任一字符不同即整体重建。代价是
  /// 每帧一次 O(已封存长度) 的字符串比较，用来换取"显示的正文一定是最新正文"。
  bool _sealedPrefixMatches(String text) {
    if (_sealedEnd > text.length) return false;
    final validatedText = _validatedText;
    if (validatedText != null &&
        (identical(validatedText, text) || text.startsWith(validatedText))) {
      _validatedText = text;
      return true;
    }
    var offset = 0;
    for (final chunk in _sealed) {
      if (!text.startsWith(chunk.text, offset)) return false;
      offset += chunk.text.length;
    }
    final matches = offset == _sealedEnd;
    if (matches) _validatedText = text;
    return matches;
  }

  @override
  Widget build(BuildContext context) {
    final bodyStyle = _markdownBodyStyle(context, widget.surface);
    final effectiveStyle = DefaultTextStyle.of(context).style.merge(bodyStyle);
    return LayoutBuilder(
      builder: (context, constraints) {
        if (_validatedId != widget.id) {
          _resetSealed();
          _riskText = null;
          _validatedId = widget.id;
        }
        final textScaler = MediaQuery.textScalerOf(context);
        final textDirection = Directionality.of(context);
        final locale = Localizations.maybeLocaleOf(context);
        final width = constraints.maxWidth;
        // TextScaler 可以是非线性的，不能用 scale(1) 之类的单一因子代表它；框架约定
        // [TextScaler.==] 用于判断文本控件是否需要重建，这里按实际 TextScaler 值失效。
        if (_width != width ||
            _textScaler != textScaler ||
            _textDirection != textDirection ||
            _style != effectiveStyle ||
            _locale != locale) {
          _width = width;
          _textScaler = textScaler;
          _textDirection = textDirection;
          _style = effectiveStyle;
          _locale = locale;
          _resetSealed();
        }
        if (!_sealedPrefixMatches(widget.text)) {
          _resetSealed();
        }
        _validatedText ??= widget.text;
        // 复用前对**本次 canonical 正文**做风险判定，而不是只看测量窗：已封存前缀由上面的
        // 逐字校验证明未变（且它是在"当时无风险"的判定下才封存的），未封存后缀在这里整段
        // 扫描——因此不会漏掉短于阈值的尾巴，也不会漏掉 Replace 到尾巴里的风险字符。
        // 方向不是 LTR 或后缀命中 RTL/bidi 时，已封存前缀同样要清掉，整体退回单个 [Text]。
        final chunkable =
            textDirection == TextDirection.ltr &&
            !_suffixHasBidiRisk(widget.text, _sealedEnd);
        if (!chunkable) {
          if (_sealed.isNotEmpty) {
            _resetSealed();
          }
        } else {
          _seal(
            width: width,
            style: effectiveStyle,
            textScaler: textScaler,
            textDirection: textDirection,
            locale: locale,
          );
        }
        final open = widget.text.substring(_sealedEnd);
        final children = <Widget>[
          for (final chunk in _sealed) chunk.widget,
          if (open.isNotEmpty)
            Text(
              open,
              key: ValueKey('plain-open-${widget.id}'),
              style: bodyStyle,
            ),
        ];
        if (children.isEmpty) {
          return const SizedBox.shrink();
        }
        final content = children.length == 1
            ? children.single
            : Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: children,
              );
        final body = KeyedSubtree(key: _contentKey, child: content);
        return widget.status == 'streaming'
            ? SelectionContainer.disabled(child: body)
            : body;
      },
    );
  }

  /// 在有界测量窗内把待渲染区封存成有界块；单次 build 最多封存 [_sealBatchLimit] 块。
  void _seal({
    required double width,
    required TextStyle style,
    required TextScaler textScaler,
    required TextDirection textDirection,
    required Locale? locale,
  }) {
    if (!width.isFinite || width <= 0) return;
    for (var attempt = 0; attempt < _sealBatchLimit; attempt += 1) {
      final open = widget.text.substring(_sealedEnd);
      if (open.length <= _sealMinimumCharacters) return;
      // 只测量有界的候选前缀；未截断的尾部继续留在尾块里。
      final measureLength = open.length < _sealMeasureCharacters
          ? open.length
          : _sealMeasureCharacters;
      final window = open.substring(0, measureLength);
      final boundary = _sealBoundary(
        window,
        open: open,
        width: width,
        style: style,
        textScaler: textScaler,
        textDirection: textDirection,
        locale: locale,
      );
      // 量不出新的安全边界时不再新增块：保留已封存前缀 + 单个尾块 Text。这不是整体回退，
      // 也仍然安全——前缀块与尾块都以真实行起点开始，单独换行与整段一致。
      if (boundary <= 0) return;
      final chunkText = open.substring(0, boundary);
      _sealed.add(
        _PlainBodyChunk(
          text: chunkText,
          widget: Text(
            chunkText,
            key: ValueKey('plain-chunk-${widget.id}-${_sealed.length}'),
            style: _markdownBodyStyle(context, widget.surface),
          ),
        ),
      );
      _sealedEnd += boundary;
    }
  }

  /// [window] 内最后一个可安全切分的位置（真实换行点），量不出时返回 0。
  ///
  /// 只测量 [window]（有界前缀），但"空白之后"的判定用完整 [open]：截断不会改变断点所在
  /// 行的换行（贪心断行只取决于断点之前的正文），尾部上下文也保留在尾块里。
  ///
  /// 行首用 [TextPainter.getPositionForOffset] 用同一 style/宽度/TextScaler/方向/locale
  /// 量出，因此切在这里各块单独换行与整段一致。优先取不超过 [_sealTargetCharacters] 的
  /// 最后一个安全边界；窗口内没有这样的边界时退到最后一个安全边界（仍然有界）。
  int _sealBoundary(
    String window, {
    required String open,
    required double width,
    required TextStyle style,
    required TextScaler textScaler,
    required TextDirection textDirection,
    required Locale? locale,
  }) {
    final painter = TextPainter(
      text: TextSpan(text: window, style: style),
      textDirection: textDirection,
      textScaler: textScaler,
      textAlign: TextAlign.start,
      locale: locale,
    )..layout(maxWidth: width);
    try {
      final metrics = painter.computeLineMetrics();
      if (metrics.length <= 1) return 0;
      // 只有 LTR 会走到这里，行首取左边缘。
      final starts = <int>[0];
      for (final metric in metrics) {
        final offset = painter
            .getPositionForOffset(Offset(0, metric.baseline))
            .offset;
        if (offset <= 0 || offset >= window.length) continue;
        if (starts.last != offset) starts.add(offset);
      }
      // 断点只在"上一行以空白结束、下一行以非空白开始"的真实行起点上取：上一块以行尾
      // 空白结尾（不可见），下一块以非空白开头。硬换行、缩进续行、超长无空白 run 都
      // 量不出这种断点，于是返回 0 退回单个 Text，绝不从中间切开一行。
      var withinTarget = 0;
      var lastSafe = 0;
      for (final raw in starts) {
        // 行首偏移可能正好落在被断行吞掉的空白上，先归一到空白之后。
        var start = raw;
        while (start < open.length && open.codeUnitAt(start) == 0x20) {
          start += 1;
        }
        // 断点必须落在测量窗内，块长才确实有界（窗口外的正文留给尾块）。
        if (start < _sealMinimumCharacters || start >= window.length) continue;
        if (open.codeUnitAt(start - 1) != 0x20 ||
            open.codeUnitAt(start) == 0x20) {
          continue;
        }
        lastSafe = start;
        if (start <= _sealTargetCharacters) withinTarget = start;
      }
      return withinTarget > 0 ? withinTarget : lastSafe;
    } finally {
      painter.dispose();
    }
  }

  /// [text] 自 [start] 起的后缀里是否含 RTL 或 bidi 控制字符，带同一实例/同起点的结果缓存。
  ///
  /// 已封存前缀不在这里重复扫描：它由 [_sealedPrefixMatches] 逐字证明未变，并且是在
  /// "当时无风险"的判定下才封存的；因此"前缀安全 + 后缀安全"覆盖整段当前正文。
  bool _suffixHasBidiRisk(String text, int start) {
    if (identical(_riskText, text) && _riskStart == start) {
      return _riskVerdict;
    }
    final previousText = _riskText;
    final appendOnly =
        previousText != null &&
        _riskStart == start &&
        text.startsWith(previousText);
    final verdict = appendOnly
        ? _riskVerdict || _hasBidiRisk(text, previousText.length, text.length)
        : _hasBidiRisk(text, start, text.length);
    _riskText = text;
    _riskStart = start;
    _riskVerdict = verdict;
    return verdict;
  }

  /// [text] 的 [start], [end) 区间内是否含 RTL 或 bidi 控制字符。
  ///
  /// 保守判定：切块会引入多个 paragraph，RTL/bidi 段落状态不能证明与整段一致，因此一旦
  /// 命中就整体退回单个 [Text]（普通 LTR 正文、CJK 与补充平面非 RTL 字符不受影响）。
  /// 流式下扫描的是未封存后缀（有界）；只有整体退回时才会每帧扫描整段，属明确代价，
  /// 不做采样、不做哈希近似。
  static bool _hasBidiRisk(String text, int start, int end) {
    if (start > 0 &&
        start < end &&
        _isHighSurrogate(text.codeUnitAt(start - 1)) &&
        _isLowSurrogate(text.codeUnitAt(start))) {
      // 起点落在代理对中间时回退一格重新成对判定，避免漏判补充平面的 RTL 字符。
      start -= 1;
    }
    for (var index = start; index < end; index += 1) {
      final unit = text.codeUnitAt(index);
      if (_isHighSurrogate(unit)) {
        if (index + 1 >= end || !_isLowSurrogate(text.codeUnitAt(index + 1))) {
          return true;
        }
        final low = text.codeUnitAt(index + 1);
        final codePoint = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
        if ((codePoint >= 0x10800 && codePoint <= 0x10FFF) ||
            (codePoint >= 0x1E800 && codePoint <= 0x1EFFF)) {
          return true;
        }
        index += 1;
        continue;
      }
      if ((unit >= 0x0590 && unit <= 0x08FF) ||
          (unit >= 0xFB1D && unit <= 0xFDFF) ||
          (unit >= 0xFE70 && unit <= 0xFEFF) ||
          (unit >= 0x200E && unit <= 0x200F) ||
          (unit >= 0x202A && unit <= 0x202E) ||
          (unit >= 0x2066 && unit <= 0x2069)) {
        return true;
      }
    }
    return false;
  }

  static bool _isHighSurrogate(int unit) => unit >= 0xD800 && unit <= 0xDBFF;

  static bool _isLowSurrogate(int unit) => unit >= 0xDC00 && unit <= 0xDFFF;
}

/// 一个已封存、不再变化的纯文本块：文本 + 复用的 [Text] widget 实例。
class _PlainBodyChunk {
  const _PlainBodyChunk({required this.text, required this.widget});

  final String text;
  final Widget widget;
}

Future<void> _openTimelineWebLink(
  BuildContext context,
  WidgetRef ref,
  String rawUrl,
) async {
  final url = safeExternalWebUrl(rawUrl);
  if (url == null) {
    return;
  }
  try {
    await ref.read(externalUrlLauncherProvider)(url);
  } catch (_) {
    if (!context.mounted) {
      return;
    }
    ScaffoldMessenger.of(context).showSnackBar(
      SnackBar(content: Text(context.l10n.timelineExternalLinkOpenFailed)),
    );
  }
}

TextStyle? _markdownBodyStyle(BuildContext context, _MarkdownSurface surface) {
  final theme = Theme.of(context);
  // VS Code 预览正文行高 22px/14px ≈ 1.57，各阅读表面保持一致节奏。
  if (surface == _MarkdownSurface.reasoning) {
    return theme.textTheme.bodySmall?.copyWith(
      color: context.colors.onSurfaceVariant,
      height: 1.57,
    );
  }
  if (surface == _MarkdownSurface.error) {
    return theme.textTheme.bodyMedium?.copyWith(
      color: theme.colorScheme.error,
      height: 1.57,
    );
  }
  return theme.textTheme.bodyMedium?.copyWith(
    color: theme.colorScheme.onSurface,
    height: 1.57,
  );
}

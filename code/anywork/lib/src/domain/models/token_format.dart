/// 上下文与词元数量的十进制紧凑显示。
///
/// 规则见 `design/19-studio-ui.md` §19.8：小于 1,000 显示为整数；千级使用小写 `k`，
/// 百万及以上使用小写 `m`；四舍五入最多保留两位小数并去除尾零，舍入到单位边界时提升
/// 单位（例如 `999_999` → `1m`）。该规则只作用于展示，不改变底层精确数值，也不套用于
/// 金额、字节、百分比和调用次数。缺失（`null` 或负值）数据保持未知占位而不伪装为零。
String formatTokenCount(int? value) {
  if (value == null || value < 0) {
    return tokenCountUnknown;
  }
  if (value < 1000) {
    return value.toString();
  }
  if (value < 1000000) {
    // value/1000 保留到百分位：value/10 四舍五入。
    final hundredths = _roundedDivide(value, 10);
    if (hundredths >= 100000) {
      // 舍入到 1000k 边界时升级为 m（本分支内只有 `1m` 可达）。
      return _formatScaled(hundredths ~/ 1000, 'm');
    }
    return _formatScaled(hundredths, 'k');
  }
  // value/1_000_000 保留到百分位：value/10000 四舍五入。
  return _formatScaled(_roundedDivide(value, 10000), 'm');
}

/// 词元数量未知时的占位符，与状态页既有的未知占位保持一致。
const String tokenCountUnknown = '—';

/// `(value / divisor)` 四舍五入到整数，用商余数避免 `value * 2` 之类的溢出。
int _roundedDivide(int value, int divisor) {
  final quotient = value ~/ divisor;
  final remainder = value % divisor;
  return quotient + (remainder * 2 >= divisor ? 1 : 0);
}

String _formatScaled(int hundredths, String unit) {
  final whole = hundredths ~/ 100;
  final fraction = hundredths % 100;
  if (fraction == 0) {
    return '$whole$unit';
  }
  final digits = fraction
      .toString()
      .padLeft(2, '0')
      .replaceFirst(RegExp(r'0+$'), '');
  return '$whole.$digits$unit';
}

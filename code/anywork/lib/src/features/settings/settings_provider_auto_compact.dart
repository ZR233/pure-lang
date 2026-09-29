import 'dart:math' as math;

import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_readouts.dart';

/// 供应商编辑页与详情页共享的模型上下文压缩阈值控件。
///
/// 编辑模式提供输入框与滑块的双向同步与“恢复默认”操作；只读模式只展示模型默认、用户
/// 覆盖、实际生效值与上下文安全上限。所有词元数值统一经 [formatTokenCount] 展示，输入框
/// 仍携带精确正整数 tokens。覆盖值等于模型默认时按“恢复默认”处理，与服务端归一化一致。
///
/// 输入框使用表单 `validator`：无效（空/零/非数）输入会显示错误并由 `Form.validate`
/// 阻止保存；恢复默认与滑块可随时把无效输入改回有效值，不会困住草稿。
class ProviderModelAutoCompactControl extends StatefulWidget {
  const ProviderModelAutoCompactControl({
    super.key,
    required this.limit,
    required this.providerId,
    required this.modelSlug,
    this.enabled = true,
    this.onOverrideChanged,
  });

  final ProviderModelAutoCompactView limit;
  final String providerId;
  final String modelSlug;
  final bool enabled;

  /// 非空时提供编辑；`null` 表示只读展示（详情页）。
  final ValueChanged<int?>? onOverrideChanged;

  @override
  State<ProviderModelAutoCompactControl> createState() =>
      _ProviderModelAutoCompactControlState();
}

class _ProviderModelAutoCompactControlState
    extends State<ProviderModelAutoCompactControl> {
  /// 递增后重建编辑区，强制清空无效输入并回到默认值。
  int _editorEpoch = 0;

  void _restoreDefault() {
    if (!widget.enabled) {
      return;
    }
    setState(() => _editorEpoch += 1);
    widget.onOverrideChanged!(null);
  }

  @override
  Widget build(BuildContext context) {
    final limit = widget.limit;
    final editable = widget.onOverrideChanged != null;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  context.l10n.settingsModelAutoCompactTitle,
                  style: context.text.labelLarge?.copyWith(
                    color: context.colors.onSurface,
                  ),
                ),
              ),
              if (editable)
                TextButton.icon(
                  key: StudioDriverKeys.providerModelAutoCompactReset(
                    widget.providerId,
                    widget.modelSlug,
                  ),
                  icon: const Icon(Icons.restart_alt, size: 16),
                  label: Text(context.l10n.settingsModelAutoCompactRestore),
                  onPressed: widget.enabled ? _restoreDefault : null,
                ),
            ],
          ),
          _AutoCompactReadout(limit: limit),
          if (editable) ...[
            const SizedBox(height: 8),
            _CompactLimitEditor(
              key: ValueKey(_editorEpoch),
              providerId: widget.providerId,
              modelSlug: widget.modelSlug,
              limit: limit,
              enabled: widget.enabled,
              onChanged: widget.onOverrideChanged!,
            ),
          ],
        ],
      ),
    );
  }
}

class _AutoCompactReadout extends StatelessWidget {
  const _AutoCompactReadout({required this.limit});

  final ProviderModelAutoCompactView limit;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 6),
      child: Wrap(
        spacing: 18,
        runSpacing: 6,
        children: [
          SettingsMetric(
            context.l10n.settingsModelAutoCompactDefaultLabel,
            formatTokenCount(limit.defaultLimit),
          ),
          SettingsMetric(
            context.l10n.settingsModelAutoCompactOverrideLabel,
            limit.hasOverride
                ? formatTokenCount(limit.overrideLimit)
                : context.l10n.settingsModelAutoCompactUsingDefault,
          ),
          SettingsMetric(
            context.l10n.settingsModelAutoCompactEffectiveLabel,
            formatTokenCount(limit.effectiveLimit),
          ),
          SettingsMetric(
            context.l10n.settingsModelAutoCompactSafeLabel,
            formatTokenCount(limit.safeLimit),
          ),
        ],
      ),
    );
  }
}

class _CompactLimitEditor extends StatefulWidget {
  const _CompactLimitEditor({
    super.key,
    required this.providerId,
    required this.modelSlug,
    required this.limit,
    required this.enabled,
    required this.onChanged,
  });

  final String providerId;
  final String modelSlug;
  final ProviderModelAutoCompactView limit;
  final bool enabled;
  final ValueChanged<int?> onChanged;

  @override
  State<_CompactLimitEditor> createState() => _CompactLimitEditorState();
}

class _CompactLimitEditorState extends State<_CompactLimitEditor> {
  late final TextEditingController _controller;
  late int _shownValue;

  int get _current => widget.limit.overrideLimit ?? widget.limit.defaultLimit;

  @override
  void initState() {
    super.initState();
    _shownValue = _current;
    _controller = TextEditingController(text: '$_current');
  }

  @override
  void didUpdateWidget(covariant _CompactLimitEditor oldWidget) {
    super.didUpdateWidget(oldWidget);
    // 仅当外部值（滑块/恢复默认/切换模型）确实变化时同步输入框，避免覆盖正在键入的文本。
    final next = _current;
    if (next != _shownValue) {
      _shownValue = next;
      _controller.text = '$next';
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _commit(int chosen) {
    final text = '$chosen';
    setState(() {
      _shownValue = chosen;
      if (_controller.text != text) {
        _controller.text = text;
      }
    });
    widget.onChanged(chosen == widget.limit.defaultLimit ? null : chosen);
  }

  void _onFieldChanged(String text) {
    final parsed = int.tryParse(text.trim());
    if (parsed == null || parsed <= 0) {
      // 无效输入不写入模型；重建以清除上一次有效值的紧凑后缀，输入框自身按
      // validator 显示错误并阻止保存。
      setState(() {});
      return;
    }
    _commit(parsed);
  }

  void _onSlider(double value) {
    final parsed = value.round();
    if (parsed <= 0) {
      return;
    }
    _commit(parsed);
  }

  @override
  Widget build(BuildContext context) {
    final limit = widget.limit;
    final safeLimit = limit.safeLimit;
    final hardMax = math.max(safeLimit ?? limit.defaultLimit, _current);
    final sliderMax = math.max(hardMax, 1).toDouble();
    final sliderValue = _current.clamp(1, hardMax < 1 ? 1 : hardMax).toDouble();
    final parsed = int.tryParse(_controller.text.trim());
    final valid = parsed != null && parsed > 0;
    final compact = valid ? formatTokenCount(parsed) : '';
    final aboveSafe = safeLimit != null && _current > safeLimit;
    final note = safeLimit == null
        ? context.l10n.settingsModelAutoCompactUnknownCapacity
        : aboveSafe
        ? context.l10n.settingsModelAutoCompactAboveSafe(
            formatTokenCount(safeLimit),
          )
        : context.l10n.settingsModelAutoCompactHelp;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Expanded(
              child: Slider(
                key: StudioDriverKeys.providerModelAutoCompactSlider(
                  widget.providerId,
                  widget.modelSlug,
                ),
                min: 1,
                max: sliderMax,
                value: sliderValue,
                label: compact,
                onChanged: widget.enabled ? _onSlider : null,
              ),
            ),
            const SizedBox(width: 12),
            SizedBox(
              width: 168,
              child: TextFormField(
                key: StudioDriverKeys.providerModelAutoCompactInput(
                  widget.providerId,
                  widget.modelSlug,
                ),
                controller: _controller,
                enabled: widget.enabled,
                autovalidateMode: AutovalidateMode.always,
                keyboardType: TextInputType.number,
                decoration: InputDecoration(
                  labelText: context.l10n.settingsModelAutoCompactInputLabel,
                  // 无效输入不显示上一个有效值的紧凑后缀，避免误导。
                  suffixText: valid ? compact : null,
                  errorMaxLines: 2,
                ),
                validator: (text) => _isPositiveInteger(text)
                    ? null
                    : context.l10n.settingsModelAutoCompactInvalid,
                onChanged: _onFieldChanged,
              ),
            ),
          ],
        ),
        const SizedBox(height: 6),
        Text(
          note,
          style: context.text.bodySmall?.copyWith(
            color: context.colors.onSurfaceVariant,
          ),
        ),
      ],
    );
  }
}

bool _isPositiveInteger(String? text) {
  final parsed = int.tryParse(text?.trim() ?? '');
  return parsed != null && parsed > 0;
}

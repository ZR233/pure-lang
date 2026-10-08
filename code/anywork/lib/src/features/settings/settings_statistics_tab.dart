import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../app/theme/studio_tokens.dart';
import '../../data/repositories/studio_repository.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_form_select.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';
import 'settings_scope.dart';

class StatisticsTab extends ConsumerStatefulWidget {
  const StatisticsTab({required this.tabIndex, super.key});

  /// 本页在设置壳中的 tab 索引，用于「仅在可见时持有租约」。
  final int tabIndex;

  @override
  ConsumerState<StatisticsTab> createState() => _StatisticsTabState();
}

class _StatisticsTabState extends ConsumerState<StatisticsTab> {
  String? _filter;
  bool _mismatchesOnly = false;

  /// 最近一次可用的 canonical 统计快照；后续刷新保留 last-known，不整页闪回。
  ModelPerformanceSnapshotView? _lastSnapshot;

  @override
  Widget build(BuildContext context) {
    // 只在「统计」tab 可见时租用模型性能 topic；隐藏页不因 keep-alive 持租约。
    final visible = ref.watch(settingsVisibleTabProvider) == widget.tabIndex;
    if (visible) {
      ref.watch(settingsStatisticsScopeProvider);
    }
    final asyncSnapshot = ref.watch(settingsStatisticsProvider);
    final latest = asyncSnapshot.value;
    if (latest != null) _lastSnapshot = latest;
    final snapshot =
        latest ?? _lastSnapshot ?? const ModelPerformanceSnapshotView();
    final filter = _filter;
    // 事件更新后仅在被过滤项消失时清理筛选，绝不重置滚动或筛选本身。
    if (filter != null &&
        !snapshot.history.any((item) => item.filterKey == filter) &&
        !snapshot.summaries.any((item) => item.filterKey == filter)) {
      _filter = null;
    }
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 760;
        final history = [
          for (final item in snapshot.history)
            if ((_filter == null || item.filterKey == _filter) &&
                (!_mismatchesOnly ||
                    item.modelMatchState == ModelMatchState.mismatched))
              item,
        ];
        final hasProjectionIssue =
            snapshot.statisticsPending ||
            snapshot.statisticsGap ||
            snapshot.readFailed;
        return SettingsPageLayout(
          maxWidth: 1120,
          header: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              SettingsHeader(
                title: context.l10n.settingsStatisticsTitle,
                subtitle: context.l10n.settingsStatisticsSubtitle,
              ),
              // pending/gap/readFailed 在固定页头轻量显示，不在正文顶部插拔大块。
              if (visible && hasProjectionIssue)
                Padding(
                  padding: const EdgeInsets.only(top: 10),
                  child: _StatisticsStatusLine(snapshot: snapshot),
                ),
              if (visible)
                SettingsTopicStatus(topic: const ModelPerformanceTopic()),
            ],
          ),
          child: CustomScrollView(
            key: StudioDriverKeys.statisticsHistory,
            slivers: [
              SliverToBoxAdapter(
                child: _SummarySection(
                  compact: compact,
                  summaries: snapshot.summaries,
                ),
              ),
              SliverPadding(
                padding: const EdgeInsets.symmetric(vertical: 20),
                sliver: SliverToBoxAdapter(
                  child: _HistoryHeader(
                    history: snapshot.history,
                    summaries: snapshot.summaries,
                    value: _filter,
                    onChanged: (value) => setState(() => _filter = value),
                    mismatchesOnly: _mismatchesOnly,
                    onMismatchesOnlyChanged: (value) =>
                        setState(() => _mismatchesOnly = value),
                  ),
                ),
              ),
              if (history.isEmpty) ...[
                if (_filter != null ||
                    _mismatchesOnly ||
                    snapshot.history.isNotEmpty ||
                    !hasProjectionIssue)
                  SliverToBoxAdapter(
                    child: _EmptyState(
                      label:
                          snapshot.history.isEmpty &&
                              _filter == null &&
                              !_mismatchesOnly
                          ? context.l10n.settingsStatisticsEmpty
                          : _mismatchesOnly
                          ? context.l10n.settingsStatisticsMismatchEmpty
                          : context.l10n.settingsStatisticsFilteredEmpty,
                    ),
                  ),
              ] else ...[
                if (!compact) SliverToBoxAdapter(child: _WideHistoryHeader()),
                SliverList.builder(
                  itemCount: history.length,
                  itemBuilder: (context, index) {
                    final sample = history[index];
                    final key = StudioDriverKeys.statisticsHistoryRow(
                      sample.providerInstanceId,
                      sample.model,
                      sample.reasoningEffort,
                      index,
                    );
                    return compact
                        ? _CompactHistoryCard(key: key, sample: sample)
                        : _WideHistoryRow(key: key, sample: sample);
                  },
                ),
              ],
              const SliverToBoxAdapter(child: SizedBox(height: 24)),
            ],
          ),
        );
      },
    );
  }
}

/// 统计投影的轻量状态行：pending/gap/readFailed 只在固定页头占一行，避免正文顶部
/// 反复插拔大块导致布局跳动。
class _StatisticsStatusLine extends StatelessWidget {
  const _StatisticsStatusLine({required this.snapshot});

  final ModelPerformanceSnapshotView snapshot;

  @override
  Widget build(BuildContext context) {
    final items = <Widget>[
      if (snapshot.readFailed)
        _StatisticsStatusItem(
          key: const ValueKey('statistics-status-read-failed'),
          icon: Icons.error_outline,
          label: context.l10n.settingsStatisticsReadFailedTitle,
          color: context.colors.error,
        ),
      if (snapshot.statisticsGap)
        _StatisticsStatusItem(
          key: const ValueKey('statistics-status-gap'),
          icon: Icons.warning_amber_rounded,
          label: context.l10n.settingsStatisticsGapTitle,
          color: context.statusColors.warning,
        ),
      if (snapshot.statisticsPending)
        _StatisticsStatusItem(
          key: const ValueKey('statistics-status-pending'),
          icon: Icons.schedule_rounded,
          label: context.l10n.settingsStatisticsPendingTitle,
          color: context.colors.onSurfaceVariant,
        ),
    ];
    if (items.isEmpty) return const SizedBox.shrink();
    return Wrap(spacing: 16, runSpacing: 4, children: items);
  }
}

class _StatisticsStatusItem extends StatelessWidget {
  const _StatisticsStatusItem({
    required this.icon,
    required this.label,
    required this.color,
    super.key,
  });

  final IconData icon;
  final String label;
  final Color color;

  @override
  Widget build(BuildContext context) {
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Icon(icon, size: 15, color: color),
        const SizedBox(width: 6),
        Text(label, style: context.text.bodySmall?.copyWith(color: color)),
      ],
    );
  }
}

class _SummarySection extends StatelessWidget {
  const _SummarySection({required this.compact, required this.summaries});

  final bool compact;
  final List<ModelPerformanceSummaryView> summaries;

  @override
  Widget build(BuildContext context) {
    return SettingsSectionPanel(
      key: StudioDriverKeys.statisticsSummary,
      title: context.l10n.settingsStatisticsSummaryTitle,
      children: [
        if (summaries.isEmpty)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 16),
            child: Text(
              context.l10n.settingsStatisticsSummaryEmpty,
              style: Theme.of(context).textTheme.bodyMedium
                  ?.copyWith(color: context.colors.onSurfaceVariant),
            ),
          )
        else if (compact)
          for (final summary in summaries) _CompactSummaryCard(summary: summary)
        else
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: DataTable(
              columns: [
                DataColumn(label: Text(context.l10n.statisticsModel)),
                DataColumn(label: Text(context.l10n.statisticsReasoningEffort)),
                DataColumn(label: Text(context.l10n.statisticsSpeed)),
                DataColumn(label: Text(context.l10n.statisticsSamples)),
                DataColumn(label: Text(context.l10n.statisticsOutputTokens)),
                DataColumn(label: Text(context.l10n.statisticsAverageTtft)),
                DataColumn(label: Text(context.l10n.statisticsAverageResponse)),
              ],
              rows: [
                for (final summary in summaries)
                  DataRow(
                    cells: [
                      DataCell(
                        _ModelLabel(
                          key: StudioDriverKeys.statisticsSummaryRow(
                            summary.providerInstanceId,
                            summary.model,
                            summary.reasoningEffort,
                          ),
                          summary: summary,
                        ),
                      ),
                      DataCell(
                        Text(
                          _formatReasoningEffort(
                            context,
                            summary.reasoningEffort,
                          ),
                        ),
                      ),
                      DataCell(
                        Text(
                          context.tokenThroughputLabel(summary.tokensPerSecond),
                        ),
                      ),
                      DataCell(Text('${summary.sampleCount}')),
                      DataCell(
                        Text(formatTokenCount(summary.completionTokens)),
                      ),
                      DataCell(Text(_formatMillis(summary.averageTtftMillis))),
                      DataCell(
                        Text(_formatMillis(summary.averageResponseMillis)),
                      ),
                    ],
                  ),
              ],
            ),
          ),
      ],
    );
  }
}

class _CompactSummaryCard extends StatelessWidget {
  const _CompactSummaryCard({required this.summary});

  final ModelPerformanceSummaryView summary;

  @override
  Widget build(BuildContext context) => SettingsResourceRow(
    key: StudioDriverKeys.statisticsSummaryRow(
      summary.providerInstanceId,
      summary.model,
      summary.reasoningEffort,
    ),
    title: _identityLabel(context, summary.model),
    subtitle:
        '${_identityLabel(context, summary.providerDisplayName)} · ${_identityLabel(context, summary.providerInstanceId)}',
    children: [
      Wrap(
        spacing: 16,
        runSpacing: 8,
        children: [
          SettingsMetric(
            context.l10n.statisticsReasoningEffort,
            _formatReasoningEffort(context, summary.reasoningEffort),
          ),
          SettingsMetric(
            context.l10n.statisticsSpeed,
            context.tokenThroughputLabel(summary.tokensPerSecond),
          ),
          SettingsMetric(
            context.l10n.statisticsSamples,
            '${summary.sampleCount}',
          ),
          SettingsMetric(
            context.l10n.statisticsOutputTokens,
            formatTokenCount(summary.completionTokens),
          ),
          SettingsMetric(
            context.l10n.statisticsAverageTtft,
            _formatMillis(summary.averageTtftMillis),
          ),
          SettingsMetric(
            context.l10n.statisticsAverageResponse,
            _formatMillis(summary.averageResponseMillis),
          ),
        ],
      ),
      const Divider(height: 24),
    ],
  );
}

class _ModelLabel extends StatelessWidget {
  const _ModelLabel({required this.summary, super.key});

  final ModelPerformanceSummaryView summary;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          _identityLabel(context, summary.model),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
        Text(
          '${_identityLabel(context, summary.providerDisplayName)} · ${_identityLabel(context, summary.providerInstanceId)}',
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: Theme.of(context).textTheme.labelSmall
              ?.copyWith(color: context.colors.onSurfaceVariant),
        ),
      ],
    );
  }
}

class _HistoryHeader extends StatelessWidget {
  const _HistoryHeader({
    required this.history,
    required this.summaries,
    required this.value,
    required this.onChanged,
    required this.mismatchesOnly,
    required this.onMismatchesOnlyChanged,
  });

  final List<ModelPerformanceSampleView> history;
  final List<ModelPerformanceSummaryView> summaries;
  final String? value;
  final ValueChanged<String?> onChanged;
  final bool mismatchesOnly;
  final ValueChanged<bool> onMismatchesOnlyChanged;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final seen = <String>{};
        final filterWidth = constraints.maxWidth < 240
            ? constraints.maxWidth
            : 240.0;
        final title = Text(
          context.l10n.settingsStatisticsHistoryTitle,
          style: Theme.of(context).textTheme.titleMedium
              ?.copyWith(fontWeight: FontWeight.w600),
        );
        final filter = SizedBox(
          key: StudioDriverKeys.statisticsFilter,
          width: filterWidth,
          child: StudioFormSelectField<String?>(
            // Caller-owned canonical selection, not a draft edit: the field
            // follows [value] without a value-driven key, so choosing a
            // filter never remounts the field's state and its real trigger
            // focus node survives the parent rebuild (an explicit null —
            // "all models" — stays a legitimate controlled value).
            value: value,
            isExpanded: true,
            decoration: const InputDecoration(isDense: true),
            items: [
              StudioFormSelectItem<String?>(
                value: null,
                child: Text(context.l10n.settingsStatisticsAllModels),
              ),
              for (final sample in history)
                if (seen.add(sample.filterKey))
                  StudioFormSelectItem<String?>(
                    value: sample.filterKey,
                    child: Text(
                      _formatPerformanceIdentity(
                        context,
                        sample.providerDisplayName,
                        sample.providerInstanceId,
                        sample.model,
                        sample.reasoningEffort,
                      ),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
              for (final summary in summaries)
                if (seen.add(summary.filterKey))
                  StudioFormSelectItem<String?>(
                    value: summary.filterKey,
                    child: Text(
                      _formatPerformanceIdentity(
                        context,
                        summary.providerDisplayName,
                        summary.providerInstanceId,
                        summary.model,
                        summary.reasoningEffort,
                      ),
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
            ],
            onChanged: onChanged,
          ),
        );
        final mismatchFilter = InkWell(
          onTap: () => onMismatchesOnlyChanged(!mismatchesOnly),
          borderRadius: BorderRadius.circular(4),
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: 4),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Checkbox(
                  key: StudioDriverKeys.statisticsMismatchOnly,
                  value: mismatchesOnly,
                  onChanged: (value) => onMismatchesOnlyChanged(value ?? false),
                  visualDensity: VisualDensity.compact,
                ),
                Flexible(
                  child: Text(
                    context.l10n.settingsStatisticsMismatchesOnly,
                    maxLines: 2,
                  ),
                ),
              ],
            ),
          ),
        );
        if (constraints.maxWidth < 376) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              title,
              const SizedBox(height: 12),
              filter,
              const SizedBox(height: 8),
              mismatchFilter,
            ],
          );
        }
        return Row(
          children: [
            Expanded(child: title),
            mismatchFilter,
            const SizedBox(width: 12),
            filter,
          ],
        );
      },
    );
  }
}

class _WideHistoryHeader extends StatelessWidget {
  @override
  Widget build(BuildContext context) {
    return _WideCells(
      emphasized: true,
      children: [
        Text(context.l10n.statisticsCompletedAt),
        Text(context.l10n.statisticsHistoryModelHeader),
        Text(context.l10n.statisticsReasoningEffort),
        Text(context.l10n.statisticsOutputTokens),
        const Text('TTFT'),
        Text(context.l10n.statisticsDecode),
        Text(context.l10n.statisticsTotalResponse),
        Text(context.l10n.statisticsSpeed),
      ],
    );
  }
}

class _WideHistoryRow extends StatelessWidget {
  const _WideHistoryRow({required this.sample, super.key});

  final ModelPerformanceSampleView sample;

  @override
  Widget build(BuildContext context) {
    return _WideCells(
      children: [
        Text(_formatCompletedAt(context, sample.completedAt)),
        _ModelIdentity(sample: sample),
        Text(_formatReasoningEffort(context, sample.reasoningEffort)),
        Text(formatTokenCount(sample.completionTokens)),
        Text(_formatSampleMillis(context, sample.ttftMillis)),
        Text(_formatSampleMillis(context, sample.decodeMillis)),
        Text(_formatSampleMillis(context, sample.totalResponseMillis)),
        Text(_formatSampleSpeed(context, sample.tokensPerSecond)),
      ],
    );
  }
}

class _WideCells extends StatelessWidget {
  const _WideCells({required this.children, this.emphasized = false});

  final List<Widget> children;
  final bool emphasized;

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(
        color: emphasized
            ? context.colors.surfaceContainer
            : Colors.transparent,
        border: Border(
          bottom: BorderSide(color: context.colors.outlineVariant),
        ),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
        child: Row(
          children: [
            for (var index = 0; index < children.length; index++)
              Expanded(
                flex: index == 1 ? 2 : 1,
                child: DefaultTextStyle(
                  style: emphasized
                      ? Theme.of(context).textTheme.labelMedium!
                            .copyWith(fontWeight: FontWeight.w600)
                      : Theme.of(context).textTheme.bodySmall!,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  child: children[index],
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _CompactHistoryCard extends StatelessWidget {
  const _CompactHistoryCard({required this.sample, super.key});

  final ModelPerformanceSampleView sample;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 18),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _ModelIdentity(sample: sample),
          const SizedBox(height: 4),
          Text(
            _formatCompletedAt(context, sample.completedAt),
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: context.text.bodySmall?.copyWith(
              color: context.colors.onSurfaceVariant,
            ),
          ),
          const SizedBox(height: 12),
          Wrap(
            spacing: 16,
            runSpacing: 8,
            children: [
              SettingsMetric(
                context.l10n.statisticsReasoningEffort,
                _formatReasoningEffort(context, sample.reasoningEffort),
              ),
              SettingsMetric(
                context.l10n.statisticsSpeed,
                _formatSampleSpeed(context, sample.tokensPerSecond),
              ),
              SettingsMetric(
                context.l10n.statisticsOutputTokens,
                formatTokenCount(sample.completionTokens),
              ),
              SettingsMetric(
                'TTFT',
                _formatSampleMillis(context, sample.ttftMillis),
              ),
              SettingsMetric(
                context.l10n.statisticsDecode,
                _formatSampleMillis(context, sample.decodeMillis),
              ),
              SettingsMetric(
                context.l10n.statisticsTotalResponse,
                _formatSampleMillis(context, sample.totalResponseMillis),
              ),
            ],
          ),
          const Divider(height: 24),
        ],
      ),
    );
  }
}

/// 历史行模型身份：第一行为请求（配置）模型，缺失时回退发送模型或历史模型；
/// 仅在配置与发送模型都存在且不同时展示映射行，发送与响应模型都存在且不同时
/// 展示橙色上游响应行并跟随不匹配徽标；未报告/未采集仅保留中性文字提示。
class _ModelIdentity extends StatelessWidget {
  const _ModelIdentity({required this.sample});

  final ModelPerformanceSampleView sample;

  @override
  Widget build(BuildContext context) {
    final configured = sample.configuredModel;
    final sent = sample.sentModel;
    final reported = sample.reportedModel;
    final hasMapping = configured != null && sent != null && configured != sent;
    final responseDiffers =
        sent != null && reported != null && reported != sent;
    final mismatched = sample.modelMatchState == ModelMatchState.mismatched;
    final neutralStatus = switch (sample.modelMatchState) {
      ModelMatchState.unreported => context.l10n.statisticsModelUnreported,
      ModelMatchState.legacyUnknown =>
        context.l10n.statisticsModelLegacyUnknown,
      _ => null,
    };
    final detailStyle = context.text.labelSmall?.copyWith(
      color: context.colors.onSurfaceVariant,
    );
    final warning = Theme.of(context)
        .extension<StudioSemanticColors>()!
        .warning;
    final description = _modelIdentityDescription(context, sample);
    return Tooltip(
      message: description,
      child: Semantics(
        container: true,
        label: description,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              _identityLabel(context, configured ?? sample.displayModel),
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: context.text.bodySmall?.copyWith(
                fontWeight: FontWeight.w600,
              ),
            ),
            if (hasMapping)
              Padding(
                padding: const EdgeInsets.only(left: 12, top: 2),
                child: Text(
                  '↳ ${_identityLabel(context, sent)}',
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: detailStyle,
                ),
              ),
            if (responseDiffers)
              Padding(
                padding: EdgeInsets.only(left: hasMapping ? 24 : 12, top: 2),
                child: Wrap(
                  spacing: 6,
                  runSpacing: 2,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    Text(
                      '↳ ${context.l10n.statisticsUpstreamReportedModel}: '
                      '${_identityLabel(context, reported)}',
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: detailStyle?.copyWith(color: warning),
                    ),
                    if (mismatched)
                      _ModelMismatchBadge(
                        label: context.l10n.statisticsModelMismatchBadge,
                      ),
                  ],
                ),
              ),
            if (mismatched && !responseDiffers)
              Padding(
                padding: const EdgeInsets.only(left: 12, top: 2),
                child: _ModelMismatchBadge(
                  label: context.l10n.statisticsModelMismatchBadge,
                ),
              ),
            if (neutralStatus != null)
              Padding(
                padding: const EdgeInsets.only(left: 12, top: 2),
                child: Text(
                  neutralStatus,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: detailStyle,
                ),
              ),
          ],
        ),
      ),
    );
  }
}

/// 「模型不匹配」徽标：浅橙底、橙字、橙色细边框的小圆角标签。
class _ModelMismatchBadge extends StatelessWidget {
  const _ModelMismatchBadge({required this.label});

  final String label;

  @override
  Widget build(BuildContext context) {
    final semantic = Theme.of(context).extension<StudioSemanticColors>()!;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: semantic.warningContainer,
        borderRadius: BorderRadius.circular(StudioRadii.xs),
        border: Border.all(color: semantic.warning.withValues(alpha: 0.6)),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
        child: Text(
          label,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: context.text.labelSmall?.copyWith(
            fontSize: 10,
            color: semantic.warning,
          ),
        ),
      ),
    );
  }
}

class _EmptyState extends StatelessWidget {
  const _EmptyState({required this.label});

  final String label;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 48),
      child: Center(
        child: Text(
          label,
          style: TextStyle(color: context.colors.onSurfaceVariant),
        ),
      ),
    );
  }
}

String _formatMillis(double millis) {
  if (millis < 1_000) return '${millis.round()} ms';
  return '${(millis / 1_000).toStringAsFixed(1)} s';
}

String _formatSampleMillis(BuildContext context, int? millis) {
  return millis == null
      ? context.l10n.statisticsMetricNotCollected
      : _formatMillis(millis.toDouble());
}

String _formatSampleSpeed(BuildContext context, double? tokensPerSecond) {
  return tokensPerSecond == null
      ? context.l10n.statisticsMetricNotCollected
      : context.tokenThroughputLabel(tokensPerSecond);
}

String _identityLabel(BuildContext context, String value) {
  return value.isEmpty ? context.l10n.statisticsMetricNotCollected : value;
}

String _formatReasoningEffort(BuildContext context, String? effort) {
  return effort ?? context.l10n.statisticsReasoningEffortUnspecified;
}

String _modelIdentityDescription(
  BuildContext context,
  ModelPerformanceSampleView sample,
) {
  final status = switch (sample.modelMatchState) {
    ModelMatchState.matched => context.l10n.statisticsModelMatched,
    ModelMatchState.mismatched => context.l10n.statisticsModelMismatched,
    ModelMatchState.unreported => context.l10n.statisticsModelUnreported,
    ModelMatchState.legacyUnknown => context.l10n.statisticsModelLegacyUnknown,
  };
  final providerName = _identityLabel(context, sample.providerDisplayName);
  final instanceId = _identityLabel(context, sample.providerInstanceId);
  final provider = sample.providerDisplayName == sample.providerInstanceId
      ? providerName
      : '$providerName · $instanceId';
  final unavailable = context.l10n.statisticsModelUnavailable;
  return [
    provider,
    '${context.l10n.statisticsConfiguredModel}: ${sample.configuredModel == null ? unavailable : _identityLabel(context, sample.configuredModel!)}',
    '${context.l10n.statisticsSentModel}: ${sample.sentModel == null ? unavailable : _identityLabel(context, sample.sentModel!)}',
    '${context.l10n.statisticsReportedModel}: ${sample.reportedModel == null ? unavailable : _identityLabel(context, sample.reportedModel!)}',
    status,
  ].join('\n');
}

String _formatPerformanceIdentity(
  BuildContext context,
  String providerDisplayName,
  String providerInstanceId,
  String model,
  String? reasoningEffort,
) {
  return [
    _identityLabel(context, providerDisplayName),
    _identityLabel(context, providerInstanceId),
    _identityLabel(context, model),
    _formatReasoningEffort(context, reasoningEffort),
  ].join(' · ');
}

String _formatCompletedAt(BuildContext context, DateTime value) {
  final local = MaterialLocalizations.of(context);
  return '${local.formatShortDate(value)} ${TimeOfDay.fromDateTime(value).format(context)}';
}

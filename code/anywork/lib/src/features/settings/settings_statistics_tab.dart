import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_badges.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';

class StatisticsTab extends StatefulWidget {
  const StatisticsTab({required this.snapshot, super.key});

  final ModelPerformanceSnapshotView snapshot;

  @override
  State<StatisticsTab> createState() => _StatisticsTabState();
}

class _StatisticsTabState extends State<StatisticsTab> {
  String? _filter;
  bool _mismatchesOnly = false;

  @override
  void didUpdateWidget(covariant StatisticsTab oldWidget) {
    super.didUpdateWidget(oldWidget);
    final filter = _filter;
    if (filter != null &&
        !widget.snapshot.summaries.any((item) => item.filterKey == filter)) {
      _filter = null;
    }
  }

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final compact = constraints.maxWidth < 760;
        final history = [
          for (final item in widget.snapshot.history)
            if ((_filter == null || item.filterKey == _filter) &&
                (!_mismatchesOnly ||
                    item.modelMatchState == ModelMatchState.mismatched))
              item,
        ];
        return SettingsPageLayout(
          maxWidth: 1120,
          header: SettingsHeader(
            title: context.l10n.settingsStatisticsTitle,
            subtitle: context.l10n.settingsStatisticsSubtitle,
          ),
          child: CustomScrollView(
            key: StudioDriverKeys.statisticsHistory,
            slivers: [
              SliverToBoxAdapter(
                child: _SummarySection(
                  compact: compact,
                  summaries: widget.snapshot.summaries,
                ),
              ),
              SliverPadding(
                padding: const EdgeInsets.symmetric(vertical: 20),
                sliver: SliverToBoxAdapter(
                  child: _HistoryHeader(
                    summaries: widget.snapshot.summaries,
                    value: _filter,
                    onChanged: (value) => setState(() => _filter = value),
                    mismatchesOnly: _mismatchesOnly,
                    onMismatchesOnlyChanged: (value) =>
                        setState(() => _mismatchesOnly = value),
                  ),
                ),
              ),
              if (history.isEmpty)
                SliverToBoxAdapter(
                  child: _EmptyState(
                    label: _mismatchesOnly
                        ? context.l10n.settingsStatisticsMismatchEmpty
                        : context.l10n.settingsStatisticsEmpty,
                  ),
                )
              else ...[
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
              context.l10n.settingsStatisticsEmpty,
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
                      DataCell(Text('${summary.completionTokens}')),
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
    title: summary.model,
    subtitle: '${summary.providerDisplayName} · ${summary.providerInstanceId}',
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
            '${summary.completionTokens}',
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
        Text(summary.model, maxLines: 1, overflow: TextOverflow.ellipsis),
        Text(
          '${summary.providerDisplayName} · ${summary.providerInstanceId}',
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
    required this.summaries,
    required this.value,
    required this.onChanged,
    required this.mismatchesOnly,
    required this.onMismatchesOnlyChanged,
  });

  final List<ModelPerformanceSummaryView> summaries;
  final String? value;
  final ValueChanged<String?> onChanged;
  final bool mismatchesOnly;
  final ValueChanged<bool> onMismatchesOnlyChanged;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final filterWidth = constraints.maxWidth < 240
            ? constraints.maxWidth
            : 240.0;
        final title = Text(
          context.l10n.settingsStatisticsHistoryTitle,
          style: Theme.of(context).textTheme.titleMedium
              ?.copyWith(fontWeight: FontWeight.w600),
        );
        final filter = SizedBox(
          width: filterWidth,
          child: DropdownButtonFormField<String?>(
            key: StudioDriverKeys.statisticsFilter,
            initialValue: value,
            isExpanded: true,
            decoration: const InputDecoration(isDense: true),
            items: [
              DropdownMenuItem<String?>(
                child: Text(context.l10n.settingsStatisticsAllModels),
              ),
              for (final summary in summaries)
                DropdownMenuItem<String?>(
                  value: summary.filterKey,
                  child: Text(
                    _formatPerformanceIdentity(context, summary),
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
        Text(context.l10n.statisticsModel),
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
        Text('${sample.completionTokens}'),
        Text(_formatMillis(sample.ttftMillis.toDouble())),
        Text(_formatMillis(sample.decodeMillis.toDouble())),
        Text(_formatMillis(sample.totalResponseMillis.toDouble())),
        Text(context.tokenThroughputLabel(sample.tokensPerSecond)),
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
    final description = _modelIdentityDescription(context, sample);
    final configuredDiffers =
        sample.configuredModel != null &&
        sample.configuredModel != sample.displayModel;
    return Tooltip(
      message: description,
      child: Semantics(
        container: true,
        label: description,
        child: SettingsResourceRow(
          title: sample.displayModel,
          subtitle:
              '${sample.providerDisplayName} · ${sample.providerInstanceId} · ${_formatCompletedAt(context, sample.completedAt)}',
          status: _ModelStatusChip(sample: sample),
          children: [
            Wrap(
              spacing: 16,
              runSpacing: 8,
              children: [
                if (configuredDiffers)
                  SettingsMetric(
                    context.l10n.statisticsConfiguredModel,
                    sample.configuredModel!,
                  ),
                SettingsMetric(
                  context.l10n.statisticsReportedModel,
                  sample.reportedModel ??
                      context.l10n.statisticsModelUnavailable,
                ),
                SettingsMetric(
                  context.l10n.statisticsReasoningEffort,
                  _formatReasoningEffort(context, sample.reasoningEffort),
                ),
                SettingsMetric(
                  context.l10n.statisticsSpeed,
                  context.tokenThroughputLabel(sample.tokensPerSecond),
                ),
                SettingsMetric(
                  context.l10n.statisticsOutputTokens,
                  '${sample.completionTokens}',
                ),
                SettingsMetric(
                  'TTFT',
                  _formatMillis(sample.ttftMillis.toDouble()),
                ),
                SettingsMetric(
                  context.l10n.statisticsDecode,
                  _formatMillis(sample.decodeMillis.toDouble()),
                ),
                SettingsMetric(
                  context.l10n.statisticsTotalResponse,
                  _formatMillis(sample.totalResponseMillis.toDouble()),
                ),
              ],
            ),
            const Divider(height: 24),
          ],
        ),
      ),
    );
  }
}

class _ModelIdentity extends StatelessWidget {
  const _ModelIdentity({required this.sample});

  final ModelPerformanceSampleView sample;

  @override
  Widget build(BuildContext context) {
    final configuredDiffers =
        sample.configuredModel != null &&
        sample.configuredModel != sample.displayModel;
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
              sample.displayModel,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
            if (sample.modelMatchState != ModelMatchState.matched) ...[
              const SizedBox(height: 4),
              _ModelStatusChip(sample: sample),
            ],
            if (configuredDiffers)
              Text(
                '${context.l10n.statisticsConfiguredModel}: ${sample.configuredModel}',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: context.text.labelSmall?.copyWith(
                  color: context.colors.onSurfaceVariant,
                ),
              ),
            Text(
              '${sample.providerDisplayName} · ${sample.providerInstanceId}',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: context.text.labelSmall?.copyWith(
                color: context.colors.onSurfaceVariant,
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _ModelStatusChip extends StatelessWidget {
  const _ModelStatusChip({required this.sample});

  final ModelPerformanceSampleView sample;

  @override
  Widget build(BuildContext context) {
    final (label, icon, tone) = switch (sample.modelMatchState) {
      ModelMatchState.matched => (
        context.l10n.statisticsModelMatched,
        Icons.check_rounded,
        StudioTone.success,
      ),
      ModelMatchState.mismatched => (
        context.l10n.statisticsModelMismatched,
        Icons.warning_amber_rounded,
        StudioTone.warning,
      ),
      ModelMatchState.unreported => (
        context.l10n.statisticsModelUnreported,
        Icons.help_outline_rounded,
        StudioTone.neutral,
      ),
      ModelMatchState.legacyUnknown => (
        context.l10n.statisticsModelLegacyUnknown,
        Icons.history_rounded,
        StudioTone.neutral,
      ),
    };
    return StudioCompactChip(
      label: label,
      icon: icon,
      tone: tone,
      maxWidth: 100,
      tooltip: _modelIdentityDescription(context, sample),
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
  final unavailable = context.l10n.statisticsModelUnavailable;
  return [
    '${context.l10n.statisticsConfiguredModel}: ${sample.configuredModel ?? unavailable}',
    '${context.l10n.statisticsSentModel}: ${sample.sentModel ?? unavailable}',
    '${context.l10n.statisticsReportedModel}: ${sample.reportedModel ?? unavailable}',
    status,
  ].join('\n');
}

String _formatPerformanceIdentity(
  BuildContext context,
  ModelPerformanceSummaryView summary,
) {
  return [
    summary.providerDisplayName,
    summary.providerInstanceId,
    summary.model,
    _formatReasoningEffort(context, summary.reasoningEffort),
  ].join(' · ');
}

String _formatCompletedAt(BuildContext context, DateTime value) {
  final local = MaterialLocalizations.of(context);
  return '${local.formatShortDate(value)} ${TimeOfDay.fromDateTime(value).format(context)}';
}

import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_chrome.dart';
import 'settings_common.dart';
import 'settings_provider_drafts.dart';

class ProviderUsagePanel extends StatelessWidget {
  const ProviderUsagePanel({
    super.key,
    required this.provider,
    required this.usage,
    required this.loading,
    required this.error,
    required this.onRefresh,
  });

  final ProviderSettingsView provider;
  final ProviderUsageView? usage;
  final bool loading;
  final String? error;
  final VoidCallback onRefresh;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final usage = this.usage;
    return StudioPanel(
      backgroundColor: context.studioPaper2,
      borderColor: context.studioLine,
      radius: StudioRadii.sm,
      padding: const EdgeInsets.all(13),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(
                Icons.account_balance_wallet_outlined,
                size: 17,
                color: colors.onSurfaceVariant,
              ),
              const SizedBox(width: 7),
              Expanded(
                child: Text(
                  context.l10n.settingsUsageTitle,
                  style: Theme.of(context).textTheme.titleSmall?.copyWith(
                    color: context.studioInk,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              Text(
                usageUpdatedLabel(context, usage?.updatedAt),
                style: Theme.of(context).textTheme.labelSmall
                    ?.copyWith(color: colors.onSurfaceVariant),
              ),
              const SizedBox(width: 4),
              IconButton(
                tooltip: context.l10n.settingsRefreshUsage,
                icon: loading
                    ? const SizedBox(
                        width: 18,
                        height: 18,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.refresh, size: 18),
                onPressed: loading ? null : onRefresh,
              ),
            ],
          ),
          if (error != null && error!.isNotEmpty) ...[
            const SizedBox(height: 8),
            ProviderUsageMessage(
              icon: Icons.error_outline,
              message: error!,
              tone: UsageTone.failed,
            ),
          ],
          const SizedBox(height: 8),
          if (usage == null)
            ProviderUsageMessage(
              icon: loading
                  ? Icons.hourglass_empty
                  : Icons.account_balance_wallet_outlined,
              message: loading
                  ? context.l10n.settingsUsageChecking
                  : context.l10n.settingsUsageNotLoaded,
              tone: loading ? UsageTone.neutral : UsageTone.muted,
            )
          else
            switch (usage.state) {
              ReadyProviderUsageView(
                data: DeepSeekBalanceProviderUsageView(:final balance),
              ) =>
                _DeepSeekUsage(usage: balance),
              ReadyProviderUsageView(
                data: ZhipuCodingPlanProviderUsageView(:final codingPlan),
              ) =>
                _ZhipuCodingPlanUsage(usage: codingPlan),
              UnsupportedProviderUsageView() ||
              MissingCredentialProviderUsageView() ||
              FailedProviderUsageView() => ProviderUsageMessage(
                icon: usage.state is UnsupportedProviderUsageView
                    ? Icons.info_outline
                    : Icons.error_outline,
                message: providerUsageMessage(context, provider, usage),
                tone: switch (usage.state) {
                  FailedProviderUsageView() => UsageTone.failed,
                  MissingCredentialProviderUsageView() => UsageTone.warning,
                  UnsupportedProviderUsageView() => UsageTone.muted,
                  ReadyProviderUsageView() => UsageTone.neutral,
                },
              ),
            },
        ],
      ),
    );
  }
}

class _DeepSeekUsage extends StatelessWidget {
  const _DeepSeekUsage({required this.usage});

  final DeepSeekBalanceUsageView usage;

  @override
  Widget build(BuildContext context) {
    final primary =
        usage.balances
            .where((item) => item.currency.toUpperCase() == 'CNY')
            .firstOrNull ??
        usage.balances.firstOrNull;
    if (primary == null) {
      return ProviderUsageMessage(
        icon: Icons.info_outline,
        message: context.l10n.settingsUsageUnavailable,
        tone: UsageTone.muted,
      );
    }
    final colors = Theme.of(context).colorScheme;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                usage.isAvailable
                    ? context.l10n.settingsUsageAvailableBalance
                    : context.l10n.settingsUsageBalanceUnavailable,
                style: Theme.of(context).textTheme.labelMedium
                    ?.copyWith(color: colors.onSurfaceVariant),
              ),
            ),
            Text(
              '${primary.currency} ${primary.totalBalance}',
              style: Theme.of(context).textTheme.titleMedium,
            ),
          ],
        ),
        const SizedBox(height: 8),
        Wrap(
          spacing: 8,
          runSpacing: 6,
          children: [
            SettingsInfoPill(
              icon: Icons.card_giftcard_outlined,
              label: context.l10n.settingsUsageGranted(primary.grantedBalance),
            ),
            SettingsInfoPill(
              icon: Icons.payments_outlined,
              label: context.l10n.settingsUsageToppedUp(
                primary.toppedUpBalance,
              ),
            ),
            for (final item in usage.balances.where(
              (item) => item.currency != primary.currency,
            ))
              SettingsInfoPill(
                icon: Icons.account_balance_wallet_outlined,
                label: '${item.currency} ${item.totalBalance}',
              ),
          ],
        ),
      ],
    );
  }
}

class _ZhipuCodingPlanUsage extends StatelessWidget {
  const _ZhipuCodingPlanUsage({required this.usage});

  final ZhipuCodingPlanUsageView usage;

  @override
  Widget build(BuildContext context) {
    final ordered = [
      findQuotaLimit(usage.limits, 'fiveHour'),
      findQuotaLimit(usage.limits, 'weekly'),
      findQuotaLimit(usage.limits, 'mcpMonthly'),
      ...usage.limits.where((limit) => limit.window == 'other'),
    ].whereType<ZhipuQuotaLimitView>().toList();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        if (usage.level != null && usage.level!.isNotEmpty) ...[
          SettingsInfoPill(
            icon: Icons.workspace_premium_outlined,
            label: usage.level!,
          ),
          const SizedBox(height: 8),
        ],
        if (ordered.isEmpty)
          ProviderUsageMessage(
            icon: Icons.info_outline,
            message: context.l10n.settingsUsageUnavailable,
            tone: UsageTone.muted,
          )
        else
          LayoutBuilder(
            builder: (context, constraints) {
              final twoColumns = constraints.maxWidth >= 650;
              return Wrap(
                spacing: 10,
                runSpacing: 10,
                children: [
                  for (final limit in ordered)
                    SizedBox(
                      width: twoColumns
                          ? (constraints.maxWidth - 10) / 2
                          : constraints.maxWidth,
                      child: _QuotaCard(limit: limit),
                    ),
                ],
              );
            },
          ),
      ],
    );
  }
}

class _QuotaCard extends StatelessWidget {
  const _QuotaCard({required this.limit});

  final ZhipuQuotaLimitView limit;

  @override
  Widget build(BuildContext context) {
    final percent = quotaRemainingPercent(limit);
    return DecoratedBox(
      decoration: BoxDecoration(
        color: context.colors.surfaceContainerLowest,
        border: Border.all(color: context.studioLine),
        borderRadius: BorderRadius.circular(StudioRadii.sm),
      ),
      child: Padding(
        padding: const EdgeInsets.all(11),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(
                  child: Text(
                    quotaTitle(context, limit),
                    style: Theme.of(context).textTheme.labelLarge
                        ?.copyWith(color: context.studioInk),
                  ),
                ),
                Text(
                  quotaResetLabel(context, limit.nextResetAt),
                  style: Theme.of(context).textTheme.labelSmall
                      ?.copyWith(color: context.studioInkSoft),
                ),
              ],
            ),
            const SizedBox(height: 7),
            Text(
              formatPercent(percent),
              style: Theme.of(context).textTheme.titleMedium?.copyWith(
                color: context.studioInk,
                fontWeight: FontWeight.w600,
              ),
            ),
            Text(
              quotaDetail(context, limit),
              style: Theme.of(context).textTheme.bodySmall
                  ?.copyWith(color: context.studioInkSoft),
            ),
            const SizedBox(height: 8),
            ClipRRect(
              borderRadius: BorderRadius.circular(999),
              child: SizedBox(
                height: 6,
                child: LinearProgressIndicator(
                  value: percent / 100,
                  backgroundColor: context.studioPaper3,
                  color: percent >= 65 ? StudioColors.sage : StudioColors.clay,
                ),
              ),
            ),
            if (limit.usageDetails.isNotEmpty) ...[
              const SizedBox(height: 8),
              Wrap(
                spacing: 6,
                runSpacing: 4,
                children: [
                  for (final detail in limit.usageDetails)
                    Tooltip(
                      message: detail.name,
                      child: StudioPill(
                        label: '${detail.name} ${formatToolUsage(detail)}',
                        backgroundColor: context.studioPaper2,
                        borderColor: context.studioLine,
                      ),
                    ),
                ],
              ),
            ],
          ],
        ),
      ),
    );
  }
}

class ProviderUsageMessage extends StatelessWidget {
  const ProviderUsageMessage({
    super.key,
    required this.icon,
    required this.message,
    required this.tone,
  });

  final IconData icon;
  final String message;
  final UsageTone tone;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final color = switch (tone) {
      UsageTone.failed => colors.error,
      UsageTone.warning => colors.tertiary,
      UsageTone.neutral || UsageTone.muted => colors.onSurfaceVariant,
    };
    return StudioInlineMessage(icon: icon, message: message, color: color);
  }
}

enum UsageTone { failed, warning, neutral, muted }

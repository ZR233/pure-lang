import 'package:flutter/material.dart';

import '../../app/theme/studio_tokens.dart';
import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';
import '../../shared/studio_chrome.dart';
import '../../shared/studio_driver_keys.dart';
import 'settings_common.dart';
import 'settings_provider_drafts.dart';
import 'settings_provider_usage.dart';

class ProviderList extends StatelessWidget {
  const ProviderList({
    super.key,
    required this.providers,
    required this.defaultProviderId,
    required this.filtering,
    required this.usageByProvider,
    required this.loadingProviderIds,
    required this.usageErrorsByProviderId,
    required this.onQueryChanged,
    required this.onAdd,
    required this.onSelect,
    required this.onSetDefault,
    required this.onRefreshAll,
    required this.onRefreshProvider,
    required this.onEdit,
    required this.onDelete,
  });

  final List<ProviderSettingsView> providers;
  final String? defaultProviderId;
  final bool filtering;
  final Map<String, ProviderUsageView> usageByProvider;
  final Set<String> loadingProviderIds;
  final Map<String, String> usageErrorsByProviderId;
  final ValueChanged<String> onQueryChanged;
  final VoidCallback onAdd;
  final ValueChanged<ProviderSettingsView> onSelect;
  final ValueChanged<ProviderSettingsView> onSetDefault;
  final Future<void> Function({String? providerId}) onRefreshAll;
  final ValueChanged<ProviderSettingsView> onRefreshProvider;
  final ValueChanged<ProviderSettingsView> onEdit;
  final ValueChanged<ProviderSettingsView>? onDelete;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        SettingsHeader(
          title: context.l10n.settingsProvidersTitle,
          subtitle: context.l10n.settingsProvidersSubtitle,
          trailing: Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              OutlinedButton.icon(
                key: StudioDriverKeys.providerUsageCheck,
                icon: const Icon(Icons.refresh),
                label: Text(context.l10n.settingsRefreshUsage),
                onPressed: () => onRefreshAll(),
              ),
              FilledButton.icon(
                icon: const Icon(Icons.add),
                label: Text(context.l10n.settingsAddProvider),
                onPressed: onAdd,
              ),
            ],
          ),
        ),
        const SizedBox(height: 14),
        SettingsSearchField(
          hintText: context.l10n.settingsSearchProviders,
          onChanged: onQueryChanged,
        ),
        const SizedBox(height: 14),
        Expanded(
          child: providers.isEmpty
              ? Center(
                  child: StudioEmptyState(
                    icon: Icons.cloud_off_outlined,
                    title: filtering
                        ? context.l10n.settingsNoProvidersMatchTitle
                        : context.l10n.settingsNoProvidersTitle,
                    message: filtering
                        ? context.l10n.settingsNoProvidersMatchMessage
                        : context.l10n.settingsNoProvidersMessage,
                  ),
                )
              : SingleChildScrollView(
                  child: SettingsGroup(
                    children: [
                      for (final provider in providers)
                        ProviderListRow(
                          provider: provider,
                          isDefault: provider.id == defaultProviderId,
                          usage: usageByProvider[provider.id],
                          usageLoading: loadingProviderIds.contains(
                            provider.id,
                          ),
                          usageError:
                              usageErrorsByProviderId[provider.id] ??
                              usageErrorsByProviderId['*'],
                          onOpen: () => onSelect(provider),
                          onSetDefault: () => onSetDefault(provider),
                          onRefreshUsage: () => onRefreshProvider(provider),
                          onEdit: () => onEdit(provider),
                          onDelete: onDelete == null
                              ? null
                              : () => onDelete!(provider),
                        ),
                    ],
                  ),
                ),
        ),
      ],
    );
  }
}

class ProviderListRow extends StatelessWidget {
  const ProviderListRow({
    super.key,
    required this.provider,
    required this.isDefault,
    required this.usage,
    required this.usageLoading,
    required this.usageError,
    required this.onOpen,
    required this.onSetDefault,
    required this.onRefreshUsage,
    required this.onEdit,
    required this.onDelete,
  });

  final ProviderSettingsView provider;
  final bool isDefault;
  final ProviderUsageView? usage;
  final bool usageLoading;
  final String? usageError;
  final VoidCallback onOpen;
  final VoidCallback onSetDefault;
  final VoidCallback onRefreshUsage;
  final VoidCallback onEdit;
  final VoidCallback? onDelete;

  @override
  Widget build(BuildContext context) {
    return Material(
      key: StudioDriverKeys.providerRow(provider.id),
      color: Colors.transparent,
      child: InkWell(
        onTap: onOpen,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              _ProviderLogo(provider: provider, active: isDefault),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    _ProviderRowTitle(provider: provider, isDefault: isDefault),
                    const SizedBox(height: 7),
                    Wrap(
                      spacing: 6,
                      runSpacing: 6,
                      children: [
                        SettingsMiniMeta(
                          icon: Icons.key_outlined,
                          label: provider.id,
                        ),
                        SettingsProviderStatusChip(provider: provider),
                      ],
                    ),
                    if (provider.allModels.isNotEmpty) ...[
                      const SizedBox(height: 7),
                      Wrap(
                        spacing: 6,
                        runSpacing: 6,
                        children: [
                          for (final model in provider.allModels.take(4))
                            StudioPill(
                              label: model.slug,
                              backgroundColor: context.studioPaper2,
                              borderColor: context.studioLine,
                            ),
                          if (provider.allModels.length > 4)
                            StudioPill(
                              label: '+${provider.allModels.length - 4}',
                            ),
                        ],
                      ),
                    ],
                    const SizedBox(height: 9),
                    ProviderListUsage(
                      provider: provider,
                      usage: usage,
                      loading: usageLoading,
                      error: usageError,
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 8),
              _ProviderRowMenu(
                isDefault: isDefault,
                onSetDefault: onSetDefault,
                onRefreshUsage: onRefreshUsage,
                onEdit: onEdit,
                onDelete: onDelete,
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _ProviderLogo extends StatelessWidget {
  const _ProviderLogo({required this.provider, required this.active});

  final ProviderSettingsView provider;
  final bool active;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return SizedBox.square(
      dimension: 40,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: active ? StudioColors.clay : context.studioPaper2,
          borderRadius: BorderRadius.circular(StudioRadii.sm),
          border: Border.all(color: colors.outlineVariant),
        ),
        child: Center(
          child: Text(
            providerInitials(provider.name),
            style: Theme.of(context).textTheme.titleSmall?.copyWith(
              color: active ? Colors.white : context.studioInk,
              fontWeight: FontWeight.w800,
            ),
          ),
        ),
      ),
    );
  }
}

class _ProviderRowTitle extends StatelessWidget {
  const _ProviderRowTitle({required this.provider, required this.isDefault});

  final ProviderSettingsView provider;
  final bool isDefault;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Flexible(
              child: Text(
                provider.name,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.titleSmall?.copyWith(
                  color: context.studioInk,
                  fontWeight: FontWeight.w700,
                ),
              ),
            ),
            if (isDefault) ...[
              const SizedBox(width: 8),
              StudioPill(
                label: context.l10n.settingsDefaultBadge,
                backgroundColor: StudioColors.claySoft,
                foregroundColor: StudioColors.clayDeep,
                borderColor: StudioColors.claySoft,
              ),
            ],
          ],
        ),
        if (provider.subtitle.isNotEmpty) ...[
          const SizedBox(height: 2),
          Text(
            provider.subtitle,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: Theme.of(context).textTheme.bodySmall?.copyWith(
              color: Theme.of(context).colorScheme.onSurfaceVariant,
            ),
          ),
        ],
      ],
    );
  }
}

class ProviderListUsage extends StatelessWidget {
  const ProviderListUsage({
    super.key,
    required this.provider,
    required this.usage,
    required this.loading,
    required this.error,
  });

  final ProviderSettingsView provider;
  final ProviderUsageView? usage;
  final bool loading;
  final String? error;

  @override
  Widget build(BuildContext context) {
    final usage = this.usage;
    if (error?.isNotEmpty ?? false) {
      return ProviderUsageMessage(
        icon: Icons.error_outline,
        message: error!,
        tone: UsageTone.failed,
      );
    }
    if (loading) {
      return ProviderUsageMessage(
        icon: Icons.hourglass_empty,
        message: context.l10n.settingsUsageCheckingShort,
        tone: UsageTone.neutral,
      );
    }
    if (usage == null || usage.state is! ReadyProviderUsageView) {
      final message = usage == null
          ? providerUsageSummary(context, provider, null, loading)
          : providerUsageSummary(context, provider, usage, loading);
      final tone = switch (usage?.state) {
        FailedProviderUsageView() => UsageTone.failed,
        MissingCredentialProviderUsageView() => UsageTone.warning,
        UnsupportedProviderUsageView() ||
        ReadyProviderUsageView() ||
        null => UsageTone.muted,
      };
      return ProviderUsageMessage(
        icon: usage?.state is FailedProviderUsageView
            ? Icons.error_outline
            : Icons.info_outline,
        message: message,
        tone: tone,
      );
    }
    if (usage.state case ReadyProviderUsageView(
      data: ZhipuCodingPlanProviderUsageView(:final codingPlan),
    )) {
      return _ProviderQuotaList(limits: codingPlan.limits);
    }
    return ProviderUsageMessage(
      icon: Icons.account_balance_wallet_outlined,
      message: providerUsageSummary(context, provider, usage, loading),
      tone: UsageTone.muted,
    );
  }
}

class _ProviderQuotaList extends StatelessWidget {
  const _ProviderQuotaList({required this.limits});

  final List<ZhipuQuotaLimitView> limits;

  @override
  Widget build(BuildContext context) {
    final ordered = [
      findQuotaLimit(limits, 'fiveHour'),
      findQuotaLimit(limits, 'weekly'),
      findQuotaLimit(limits, 'mcpMonthly'),
    ].whereType<ZhipuQuotaLimitView>().toList();
    if (ordered.isEmpty) {
      return ProviderUsageMessage(
        icon: Icons.info_outline,
        message: context.l10n.settingsUsageUnavailable,
        tone: UsageTone.muted,
      );
    }
    return Column(
      children: [
        for (var index = 0; index < ordered.length; index++) ...[
          _ProviderQuotaRow(limit: ordered[index]),
          if (index < ordered.length - 1) const SizedBox(height: 8),
        ],
      ],
    );
  }
}

class _ProviderQuotaRow extends StatelessWidget {
  const _ProviderQuotaRow({required this.limit});

  final ZhipuQuotaLimitView limit;

  @override
  Widget build(BuildContext context) {
    final percent = quotaRemainingPercent(limit);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                quotaTitle(context, limit),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: context.text.labelMedium?.copyWith(
                  color: context.studioInk,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ),
            const SizedBox(width: 10),
            Text(
              context.l10n.settingsUsagePercentRemaining(
                formatPercent(percent),
              ),
              style: context.text.labelMedium?.copyWith(
                color: context.studioInk,
                fontWeight: FontWeight.w600,
              ),
            ),
            if (quotaResetLabel(context, limit.nextResetAt).isNotEmpty) ...[
              const SizedBox(width: 10),
              Text(
                quotaResetLabel(context, limit.nextResetAt),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: context.text.labelSmall?.copyWith(
                  color: context.studioInkSoft,
                ),
              ),
            ],
          ],
        ),
        const SizedBox(height: 5),
        ClipRRect(
          borderRadius: BorderRadius.circular(StudioRadii.xs),
          child: SizedBox(
            height: 5,
            child: LinearProgressIndicator(
              value: percent / 100,
              backgroundColor: context.studioPaper3,
              color: percent >= 65 ? StudioColors.sage : StudioColors.clay,
            ),
          ),
        ),
      ],
    );
  }
}

enum _ProviderRowAction { setDefault, refresh, edit, delete }

class _ProviderRowMenu extends StatelessWidget {
  const _ProviderRowMenu({
    required this.isDefault,
    required this.onSetDefault,
    required this.onRefreshUsage,
    required this.onEdit,
    required this.onDelete,
  });

  final bool isDefault;
  final VoidCallback onSetDefault;
  final VoidCallback onRefreshUsage;
  final VoidCallback onEdit;
  final VoidCallback? onDelete;

  @override
  Widget build(BuildContext context) {
    return PopupMenuButton<_ProviderRowAction>(
      tooltip: context.l10n.settingsProviderActions,
      icon: const Icon(Icons.more_horiz),
      iconSize: 20,
      constraints: const BoxConstraints(minWidth: 40, minHeight: 40),
      onSelected: (action) {
        switch (action) {
          case _ProviderRowAction.setDefault:
            onSetDefault();
          case _ProviderRowAction.refresh:
            onRefreshUsage();
          case _ProviderRowAction.edit:
            onEdit();
          case _ProviderRowAction.delete:
            onDelete?.call();
        }
      },
      itemBuilder: (context) => [
        PopupMenuItem(
          value: _ProviderRowAction.setDefault,
          enabled: !isDefault,
          child: _ProviderMenuLabel(
            icon: isDefault
                ? Icons.check_circle_outline
                : Icons.radio_button_unchecked,
            label: isDefault
                ? context.l10n.settingsDefaultProvider
                : context.l10n.settingsSetAsDefaultProvider,
          ),
        ),
        PopupMenuItem(
          value: _ProviderRowAction.refresh,
          child: _ProviderMenuLabel(
            icon: Icons.refresh,
            label: context.l10n.settingsRefreshUsage,
          ),
        ),
        PopupMenuItem(
          value: _ProviderRowAction.edit,
          child: _ProviderMenuLabel(
            icon: Icons.edit_outlined,
            label: context.l10n.settingsEditProvider,
          ),
        ),
        if (onDelete != null)
          PopupMenuItem(
            value: _ProviderRowAction.delete,
            child: _ProviderMenuLabel(
              icon: Icons.delete_outline,
              label: context.l10n.settingsDeleteProvider,
            ),
          ),
      ],
    );
  }
}

class _ProviderMenuLabel extends StatelessWidget {
  const _ProviderMenuLabel({required this.icon, required this.label});

  final IconData icon;
  final String label;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [Icon(icon, size: 18), const SizedBox(width: 10), Text(label)],
    );
  }
}

import 'package:flutter/material.dart';

import '../../domain/models/studio_models.dart';
import '../../l10n/studio_l10n.dart';

enum ProviderDraftMode { create, edit }

class ProviderDraft {
  const ProviderDraft({
    required this.mode,
    required this.originalId,
    required this.provider,
  });

  factory ProviderDraft.create(ProviderSettingsView provider) {
    return ProviderDraft(
      mode: ProviderDraftMode.create,
      originalId: provider.id,
      provider: provider,
    );
  }

  factory ProviderDraft.edit(ProviderSettingsView provider) {
    return ProviderDraft(
      mode: ProviderDraftMode.edit,
      originalId: provider.id,
      provider: provider,
    );
  }

  final ProviderDraftMode mode;
  final String originalId;
  final ProviderSettingsView provider;

  ProviderDraft copyWith({ProviderSettingsView? provider}) {
    return ProviderDraft(
      mode: mode,
      originalId: originalId,
      provider: provider ?? this.provider,
    );
  }
}

abstract final class ProviderDraftFactory {
  static ProviderDraft? create({
    required ProviderCatalogView catalog,
    required String templateId,
    required String providerId,
  }) {
    final provider = _providerFromPreset(
      catalog: catalog,
      templateId: templateId,
      providerId: providerId,
    );
    return provider == null ? null : ProviderDraft.create(provider);
  }

  static ProviderDraft? changeTemplate({
    required ProviderDraft draft,
    required ProviderCatalogView catalog,
    required String templateId,
    required String providerId,
  }) {
    final provider = templateId.isEmpty
        ? _customProvider(draft.provider, providerId)
        : _providerFromPreset(
            catalog: catalog,
            templateId: templateId,
            providerId: providerId,
          );
    return provider == null ? null : draft.copyWith(provider: provider);
  }

  static ProviderSettingsView? _providerFromPreset({
    required ProviderCatalogView catalog,
    required String templateId,
    required String providerId,
  }) {
    final preset = catalog.preset(templateId);
    if (preset == null) {
      return null;
    }
    return preset.createProvider(
      providerId,
      catalog.modelsFor(preset.modelCatalogId),
    );
  }

  static ProviderSettingsView _customProvider(
    ProviderSettingsView current,
    String providerId,
  ) {
    return current.copyWith(
      id: providerId,
      templateKind: '',
      catalogId: '',
      defaultModels: const [],
      models: current.customModels,
      defaultModel: current.customModels.firstOrNull?.slug ?? '',
      credentialLabel: 'API Key',
      credentialEnv: '',
      capabilitySource: 'explicit',
      hostedWebSearch: false,
      standaloneWebSearch: '',
      promptCacheDialect: 'none',
      responsesProgrammaticToolCalling: false,
      iconKey: null,
    );
  }
}

String providerInitials(String value) {
  final words = value.trim().split(RegExp(r'\s+'));
  if (words.isEmpty || words.first.isEmpty) {
    return '?';
  }
  if (words.length == 1) {
    final word = words.first;
    return word.substring(0, word.length < 2 ? word.length : 2).toUpperCase();
  }
  return words
      .take(2)
      .map((word) => word.isEmpty ? '' : word.substring(0, 1))
      .join()
      .toUpperCase();
}

String providerModelPriceLabel(ProviderModelView model) {
  if (model.currency.isEmpty ||
      model.inputPricePerMTok == null ||
      model.outputPricePerMTok == null) {
    return '';
  }
  return '${model.currency} ${_trimNumber(model.inputPricePerMTok!)}/${_trimNumber(model.outputPricePerMTok!)}';
}

String _trimNumber(double value) {
  if (value.truncateToDouble() == value) {
    return value.toStringAsFixed(0);
  }
  return value
      .toStringAsFixed(3)
      .replaceFirst(RegExp(r'0+$'), '')
      .replaceFirst(RegExp(r'\.$'), '');
}

String providerUsageSummary(
  BuildContext context,
  ProviderSettingsView provider,
  ProviderUsageView? usage,
  bool loading,
) {
  if (loading && usage == null) {
    return context.l10n.settingsUsageCheckingShort;
  }
  if (usage == null) {
    return context.l10n.settingsUsageNotLoaded;
  }
  return switch (usage.state) {
    UnsupportedProviderUsageView() => context.l10n.settingsUsageUnsupported,
    MissingCredentialProviderUsageView() =>
      context.l10n.settingsUsageMissingKey,
    FailedProviderUsageView() => context.l10n.settingsUsageFailed,
    ReadyProviderUsageView(:final data) => _readyProviderUsageSummary(data),
  };
}

String _readyProviderUsageSummary(ProviderUsageDataView data) {
  if (data case DeepSeekBalanceProviderUsageView(:final balance)) {
    final primary =
        balance.balances
            .where((item) => item.currency.toUpperCase() == 'CNY')
            .firstOrNull ??
        balance.balances.firstOrNull;
    return primary == null
        ? 'Usage unavailable'
        : '${primary.currency} ${primary.totalBalance}';
  }
  if (data case ZhipuCodingPlanProviderUsageView(:final codingPlan)) {
    final fiveHour = findQuotaLimit(codingPlan.limits, 'fiveHour');
    final weekly = findQuotaLimit(codingPlan.limits, 'weekly');
    if (fiveHour != null && weekly != null) {
      return '5h ${formatPercent(quotaRemainingPercent(fiveHour))} · 7d ${formatPercent(quotaRemainingPercent(weekly))}';
    }
  }
  return 'Usage unavailable';
}

String providerUsageMessage(
  BuildContext context,
  ProviderSettingsView provider,
  ProviderUsageView usage,
) {
  return switch (usage.state) {
    MissingCredentialProviderUsageView(:final message) =>
      message.isEmpty ? context.l10n.settingsUsageApiKeyMissing : message,
    FailedProviderUsageView(:final message) =>
      message.isEmpty ? context.l10n.settingsUsageQueryFailed : message,
    UnsupportedProviderUsageView() =>
      context.l10n.settingsUsageUnsupportedForProvider(provider.name),
    ReadyProviderUsageView() => context.l10n.settingsUsageUnavailable,
  };
}

String usageUpdatedLabel(BuildContext context, int? seconds) {
  if (seconds == null || seconds <= 0) {
    return context.l10n.settingsUsageNotChecked;
  }
  return context.l10n.settingsUsageUpdated(_formatUnixShort(seconds));
}

ZhipuQuotaLimitView? findQuotaLimit(
  List<ZhipuQuotaLimitView> limits,
  String window,
) {
  return limits.where((limit) => limit.window == window).firstOrNull;
}

double quotaRemainingPercent(ZhipuQuotaLimitView limit) {
  final remaining = limit.remaining;
  final total = limit.total;
  if (remaining != null && total != null && total > 0) {
    return _clampPercent((remaining / total) * 100);
  }
  return _clampPercent(100 - limit.percentage);
}

String quotaTitle(BuildContext context, ZhipuQuotaLimitView limit) {
  return switch (limit.window) {
    'fiveHour' => context.l10n.settingsUsageFiveHourQuota,
    'weekly' => context.l10n.settingsUsageWeeklyQuota,
    'mcpMonthly' => context.l10n.settingsUsageMcpQuota,
    _ => limit.label.isEmpty ? context.l10n.settingsUsageQuota : limit.label,
  };
}

String quotaDetail(BuildContext context, ZhipuQuotaLimitView limit) {
  final remaining = limit.remaining;
  final currentValue = limit.currentValue;
  final total = limit.total;
  if (remaining != null && total != null) {
    return context.l10n.settingsUsageQuotaRemaining(
      _formatCompactNumber(remaining),
      _formatCompactNumber(total),
    );
  }
  if (currentValue != null && total != null) {
    return context.l10n.settingsUsageQuotaUsed(
      _formatCompactNumber(currentValue),
      _formatCompactNumber(total),
    );
  }
  return context.l10n.settingsUsagePercentRemaining(
    formatPercent(quotaRemainingPercent(limit)),
  );
}

String quotaResetLabel(BuildContext context, int? seconds) {
  if (seconds == null || seconds <= 0) {
    return '';
  }
  return context.l10n.settingsUsageReset(_formatUnixShort(seconds));
}

String formatToolUsage(ZhipuToolUsageDetailView detail) {
  final currentValue = detail.currentValue;
  final total = detail.total;
  if (currentValue != null && total != null) {
    return '${_formatCompactNumber((total - currentValue).clamp(0, total))}/${_formatCompactNumber(total)}';
  }
  if (currentValue != null) {
    return _formatCompactNumber(currentValue);
  }
  if (detail.percentage != null) {
    return formatPercent(_clampPercent(100 - detail.percentage!));
  }
  return '';
}

String _formatUnixShort(int seconds) {
  final date = DateTime.fromMillisecondsSinceEpoch(seconds * 1000);
  final hour = date.hour.toString().padLeft(2, '0');
  final minute = date.minute.toString().padLeft(2, '0');
  return '${date.month}/${date.day} $hour:$minute';
}

String _formatCompactNumber(num value) {
  final number = value.toDouble();
  final abs = number.abs();
  if (abs >= 1000000) {
    return '${_trimNumber(number / 1000000)}M';
  }
  if (abs >= 1000) {
    return '${_trimNumber(number / 1000)}K';
  }
  return _trimNumber(number);
}

String formatPercent(double value) => '${_trimNumber(value)}%';

double _clampPercent(double value) {
  if (!value.isFinite) {
    return 0;
  }
  if (value < 0) {
    return 0;
  }
  if (value > 100) {
    return 100;
  }
  return value;
}

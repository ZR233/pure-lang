import {
  AlertCircle,
  Activity,
  Cpu,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  Server,
  Trash2,
  Wallet,
} from "lucide-react";
import type { TFunction } from "i18next";
import { useMemo, useState, type Dispatch, type SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import type {
  ModelRecord,
  ProviderKind,
  ProviderRecord,
  ProviderSettingsSaveSnapshot,
  ProviderTemplateRecord,
  ProviderUsageRecord,
  ZhipuQuotaLimit,
} from "../types";
import {
  applyProviderTemplate,
  cloneProvider,
  createProviderFromTemplate,
  normalizeProvider,
  replaceProvider,
  suggestProviderId,
} from "../lib/provider-settings";
import { allModels, initials, translateStatus } from "../lib/utils";
import { ProviderEditPage } from "./ProviderEditPage";

type ProviderSettingsProps = {
  providers: ProviderRecord[];
  templates: ProviderTemplateRecord[];
  providerUsages: ProviderUsageRecord[];
  providerUsagesLoading: boolean;
  providerUsageError: string | null;
  selectedProviderId: string | null;
  providerSearch: string;
  setProviderSearch: Dispatch<SetStateAction<string>>;
  onSaveProviderSettings: (snapshot: ProviderSettingsSaveSnapshot) => Promise<boolean>;
  onRefreshProviderUsages: () => void;
};

type ProviderDraft = {
  mode: "create" | "edit";
  originalId: string | null;
  provider: ProviderRecord;
  autoId: string;
  autoName: string;
};

export function ProviderSettings({
  providers,
  templates,
  providerUsages,
  providerUsagesLoading,
  providerUsageError,
  selectedProviderId,
  providerSearch,
  setProviderSearch,
  onSaveProviderSettings,
  onRefreshProviderUsages,
}: ProviderSettingsProps) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<ProviderDraft | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const providerUsageById = useMemo(
    () => new Map(providerUsages.map((usage) => [usage.providerId, usage])),
    [providerUsages],
  );
  const filteredProviders = providers.filter((provider) => {
    const query = providerSearch.trim().toLowerCase();
    if (!query) {
      return true;
    }
    const modelText = allModels(provider)
      .map((model) => `${model.slug} ${model.displayName}`)
      .join(" ")
      .toLowerCase();
    return (
      provider.name.toLowerCase().includes(query) ||
      provider.id.toLowerCase().includes(query) ||
      modelText.includes(query)
    );
  });
  async function saveSnapshot(snapshot: ProviderSettingsSaveSnapshot) {
    if (isSaving) {
      return false;
    }
    setIsSaving(true);
    try {
      return await onSaveProviderSettings(snapshot);
    } finally {
      setIsSaving(false);
    }
  }

  function startAddProvider() {
    const template = templates[0];
    if (!template) {
      return;
    }
    const id = suggestProviderId(providers, template.id);
    setDraft({
      mode: "create",
      originalId: null,
      provider: createProviderFromTemplate(template, id),
      autoId: id,
      autoName: template.name,
    });
  }

  function startEditProvider(provider: ProviderRecord) {
    setDraft({
      mode: "edit",
      originalId: provider.id,
      provider: cloneProvider(provider),
      autoId: provider.id,
      autoName: provider.name,
    });
  }

  function updateDraftProvider(updater: (provider: ProviderRecord) => ProviderRecord) {
    setDraft((current) =>
      current
        ? {
            ...current,
            provider: normalizeProvider(updater(current.provider)),
          }
        : current,
    );
  }

  function changeDraftTemplate(kind: ProviderKind) {
    const template = templates.find((item) => item.id === kind);
    if (!template) {
      return;
    }
    setDraft((current) => {
      if (!current) {
        return current;
      }
      const shouldUseTemplateId =
        current.mode === "create" && current.provider.id === current.autoId;
      const shouldUseTemplateName =
        current.mode === "create" && current.provider.name === current.autoName;
      const nextId = shouldUseTemplateId
        ? suggestProviderId(providers, template.id)
        : current.provider.id;
      const nextName = shouldUseTemplateName ? template.name : current.provider.name;
      return {
        ...current,
        provider: applyProviderTemplate(current.provider, template, {
          id: nextId,
          name: nextName,
        }),
        autoId: current.mode === "create" ? nextId : current.autoId,
        autoName: current.mode === "create" ? template.name : current.autoName,
      };
    });
  }

  function addCustomModel() {
    updateDraftProvider((provider) => {
      const existing = new Set(allModels(provider).map((model) => model.slug));
      let slug = "custom-model";
      for (let index = 2; existing.has(slug); index += 1) {
        slug = `custom-model-${index}`;
      }
      return {
        ...provider,
        customModels: [
          ...provider.customModels,
          {
            slug,
            displayName: t("model.defaultCustomModelName"),
            reasoningEfforts: ["high"],
          },
        ],
        defaultModel: provider.defaultModel || slug,
      };
    });
  }

  function updateCustomModel(index: number, patch: Partial<ModelRecord>) {
    updateDraftProvider((provider) => ({
      ...provider,
      customModels: provider.customModels.map((model, modelIndex) =>
        modelIndex === index ? { ...model, ...patch } : model,
      ),
    }));
  }

  function removeCustomModel(index: number) {
    updateDraftProvider((provider) => ({
      ...provider,
      customModels: provider.customModels.filter((_, modelIndex) => modelIndex !== index),
    }));
  }

  async function saveDraft() {
    if (!draft) {
      return;
    }
    const nextProvider = normalizeProvider(draft.provider);
    const nextProviders =
      draft.mode === "create"
        ? [...providers, nextProvider]
        : replaceProvider(providers, draft.originalId ?? nextProvider.id, nextProvider);
    const nextSelectedProviderId =
      draft.mode === "create" || selectedProviderId === draft.originalId
        ? nextProvider.id
        : selectedProviderId;
    const saved = await saveSnapshot({
      providers: nextProviders,
      selectedProviderId: nextSelectedProviderId,
    });
    if (saved) {
      setDraft(null);
    }
  }

  async function selectProvider(providerId: string) {
    if (providerId === selectedProviderId) {
      return;
    }
    await saveSnapshot({ selectedProviderId: providerId });
  }

  async function removeProvider(providerId: string) {
    if (providers.length <= 1) {
      return;
    }
    const remainingProviders = providers.filter((provider) => provider.id !== providerId);
    const nextSelectedProviderId =
      selectedProviderId === providerId ? (remainingProviders[0]?.id ?? null) : selectedProviderId;
    const saved = await saveSnapshot({
      providers: remainingProviders,
      selectedProviderId: nextSelectedProviderId,
    });
    if (saved && draft?.originalId === providerId) {
      setDraft(null);
    }
  }

  return (
    <div>
      {draft ? (
        <ProviderEditPage
          mode={draft.mode}
          provider={draft.provider}
          templates={templates}
          isSaving={isSaving}
          onCancel={() => setDraft(null)}
          onSave={() => void saveDraft()}
          onChangeTemplate={changeDraftTemplate}
          onUpdateProvider={updateDraftProvider}
          onAddCustomModel={addCustomModel}
          onUpdateCustomModel={updateCustomModel}
          onRemoveCustomModel={removeCustomModel}
        />
      ) : (
        <section className="space-y-4">
          <div className="flex items-start justify-between gap-4">
            <div>
              <h2 className="text-lg font-semibold text-foreground">{t("settings.providerRoute")}</h2>
              <p className="text-sm text-muted-foreground">{t("settings.providerRouteDesc")}</p>
            </div>
            <div className="flex items-center gap-2">
              <div className="relative">
                <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
                <Input
                  className="pl-9"
                  value={providerSearch}
                  onChange={(event) => setProviderSearch(event.target.value)}
                  placeholder={t("provider.searchPlaceholder")}
                />
              </div>
              <div className="flex items-center gap-1">
                <Button
                  variant="outline"
                  size="sm"
                  disabled={providerUsagesLoading}
                  onClick={onRefreshProviderUsages}
                  title={t("provider.refreshUsageTooltip")}
                >
                  <RefreshCw size={16} className={providerUsagesLoading ? "animate-spin" : ""} />
                </Button>
                <Button variant="outline" size="sm" onClick={startAddProvider}>
                  <Plus size={16} className="mr-1" />
                  {t("provider.addProvider")}
                </Button>
              </div>
            </div>
          </div>

          {providerUsageError ? (
            <div className="flex items-center gap-2 rounded-lg border border-border bg-muted/50 p-3 text-sm text-destructive">
              <AlertCircle size={16} />
              <span>{providerUsageError}</span>
            </div>
          ) : null}

          <div className="grid gap-4">
            {filteredProviders.length === 0 ? (
              <p className="text-sm text-muted-foreground text-center py-8">
                {providerSearch.trim()
                  ? t("provider.noMatches")
                  : t("provider.noProviders")}
              </p>
            ) : (
              filteredProviders.map((provider) => {
                const usage = providerUsageById.get(provider.id);
                const defaultModel = allModels(provider).find(
                  (model) => model.slug === provider.defaultModel,
                );
                const previewModels = allModels(provider).slice(0, 4);
                const remainingModels = Math.max(0, allModels(provider).length - 4);
                const initialsText = initials(provider.name || provider.id);
                const status = provider.status;

                return (
                  <Card
                    key={provider.id}
                    className={`hover:border-primary/30 transition-colors cursor-pointer ${
                      provider.id === selectedProviderId ? "border-primary/50" : ""
                    }`}
                    onClick={() => void selectProvider(provider.id)}
                  >
                    <div className="p-4 space-y-3">
                      <div className="flex items-start justify-between gap-4">
                        <div className="flex items-center gap-3 min-w-0">
                          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-muted text-sm font-medium text-muted-foreground">
                            {initialsText}
                          </div>
                          <div className="min-w-0">
                            <div className="flex items-center gap-2">
                              <strong className="text-sm text-foreground truncate">
                                {provider.name || provider.id}
                              </strong>
                              <Badge
                                variant={status === "healthy" ? "default" : "secondary"}
                                className="shrink-0"
                              >
                                {translateStatus(status, t)}
                              </Badge>
                            </div>
                            <span className="text-xs text-muted-foreground">{provider.id}</span>
                          </div>
                        </div>
                        <div className="flex items-center gap-1 shrink-0">
                          <Button
                            variant="ghost"
                            size="icon"
                            disabled={isSaving}
                            onClick={(e) => {
                              e.stopPropagation();
                              startEditProvider(provider);
                            }}
                            title={t("provider.editTooltip")}
                          >
                            <Pencil size={16} />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            disabled={isSaving || providers.length <= 1}
                            onClick={(e) => {
                              e.stopPropagation();
                              void removeProvider(provider.id);
                            }}
                            title={t("provider.deleteTooltip")}
                          >
                            <Trash2 size={16} />
                          </Button>
                        </div>
                      </div>

                      <div className="flex items-center gap-6 text-xs text-muted-foreground">
                        <span className="flex items-center gap-1">
                          <Cpu size={14} />
                          <small>{t("provider.defaultModel")}</small>
                          <strong className="text-foreground">
                            {defaultModel?.displayName ?? provider.defaultModel}
                          </strong>
                        </span>
                        <span className="flex items-center gap-1">
                          <Server size={14} />
                          <small>{t("model.title")}</small>
                          <strong className="text-foreground">
                            {t("provider.models", { count: provider.modelCount })}
                          </strong>
                        </span>
                        <span className={`flex items-center gap-1 ${providerUsageStatusClass(usage, providerUsagesLoading)}`}>
                          <Wallet size={14} />
                          <small>{t("provider.usage")}</small>
                          <strong className="text-foreground">
                            {providerUsageSummary(provider, usage, providerUsagesLoading, t)}
                          </strong>
                        </span>
                      </div>

                      <ProviderUsagePanel
                        usage={usage}
                        loading={providerUsagesLoading}
                        t={t}
                      />

                      <Separator />

                      <div className="flex items-center gap-2 flex-wrap">
                        <span className="inline-flex items-center rounded-full border border-border px-2 py-0.5 text-xs font-medium">
                          {t("provider.models", { count: provider.modelCount })}
                        </span>
                        {previewModels.map((model) => (
                          <span
                            key={model.slug}
                            className={`inline-flex items-center rounded-full border px-2 py-0.5 text-xs font-medium ${
                              model.slug === provider.defaultModel
                                ? "border-primary/50 bg-primary/5 text-primary"
                                : "border-border text-muted-foreground"
                            }`}
                          >
                            {model.displayName || model.slug}
                          </span>
                        ))}
                        {remainingModels > 0 ? (
                          <span className="inline-flex items-center rounded-full border border-border px-2 py-0.5 text-xs font-medium text-muted-foreground">
                            +{remainingModels}
                          </span>
                        ) : null}
                      </div>
                    </div>
                  </Card>
                );
              })
            )}
          </div>
        </section>
      )}
    </div>
  );
}

type ProviderUsagePanelProps = {
  usage: ProviderUsageRecord | undefined;
  loading: boolean;
  t: TFunction;
};

function ProviderUsagePanel({ usage, loading, t }: ProviderUsagePanelProps) {
  if (loading && !usage) {
    return (
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <Activity size={16} />
        <span>{t("provider.usageChecking")}</span>
      </div>
    );
  }

  if (!usage) {
    return (
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <Activity size={16} />
        <span>{t("provider.usageNotLoaded")}</span>
      </div>
    );
  }

  switch (usage.status) {
    case "unsupported":
      return (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Activity size={16} />
          <span>{t("provider.usageUnsupported")}</span>
        </div>
      );
    case "missingCredential":
      return (
        <div className="flex items-center gap-2 text-xs text-amber-600">
          <AlertCircle size={16} />
          <span>{t("provider.usageMissingCredential")}</span>
        </div>
      );
    case "failed":
      return (
        <div className="flex items-center gap-2 text-xs text-destructive">
          <AlertCircle size={16} />
          <span>{usage.message || t("provider.usageFailed")}</span>
        </div>
      );
    case "ready":
      if (usage.usageKind === "deepseekBalance" && usage.balance) {
        return <DeepSeekUsagePanel usage={usage} loading={loading} t={t} />;
      }
      if (usage.usageKind === "zhipuCodingPlan" && usage.codingPlan) {
        return <ZhipuUsagePanel usage={usage} loading={loading} t={t} />;
      }
      return (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Activity size={16} />
          <span>{t("provider.usageUnavailable")}</span>
        </div>
      );
  }
}

function DeepSeekUsagePanel({
  usage,
  loading,
  t,
}: {
  usage: ProviderUsageRecord;
  loading: boolean;
  t: TFunction;
}) {
  const balance = usage.balance;
  const primary = balance ? primaryBalance(balance.balances) : null;
  return (
    <div className={`rounded-lg bg-muted/50 p-3 space-y-2 ${loading ? "opacity-60" : ""}`}>
      <div className="flex items-center justify-between">
        <span className={`flex items-center gap-1 text-xs ${balance?.isAvailable ? "text-green-600" : "text-amber-600"}`}>
          <Wallet size={14} />
          {balance?.isAvailable ? t("provider.balanceAvailable") : t("provider.balanceUnavailable")}
        </span>
        <strong className="text-sm">{primary ? formatMoney(primary.totalBalance, primary.currency) : t("provider.usageUnavailable")}</strong>
      </div>
      <div className="grid gap-2">
        {(balance?.balances ?? []).map((item) => (
          <div key={item.currency} className="text-xs">
            <span className="text-muted-foreground">{item.currency}</span>
            <strong className="ml-2">{formatMoney(item.totalBalance, item.currency)}</strong>
            <em className="block text-muted-foreground">
              {t("provider.balanceGranted")}: {item.grantedBalance || "0"} ·{" "}
              {t("provider.balanceToppedUp")}: {item.toppedUpBalance || "0"}
            </em>
          </div>
        ))}
      </div>
    </div>
  );
}

function ZhipuUsagePanel({
  usage,
  loading,
  t,
}: {
  usage: ProviderUsageRecord;
  loading: boolean;
  t: TFunction;
}) {
  const codingPlan = usage.codingPlan;
  const fiveHour = findLimit(codingPlan?.limits ?? [], "fiveHour");
  const weekly = findLimit(codingPlan?.limits ?? [], "weekly");
  const mcp = findLimit(codingPlan?.limits ?? [], "mcpMonthly");
  const quotaRows = [
    [t("provider.quotaFiveHour"), fiveHour] as const,
    [t("provider.quotaWeekly"), weekly] as const,
    [t("provider.quotaMcp"), mcp] as const,
  ].filter((item): item is readonly [string, ZhipuQuotaLimit] => Boolean(item[1]));

  return (
    <div className={`rounded-lg bg-muted/50 p-3 space-y-2 ${loading ? "opacity-60" : ""}`}>
      {quotaRows.length > 0 ? (
        <div className="grid gap-3">
          {quotaRows.map(([label, limit]) => (
            <QuotaRow key={limit.window} label={label} limit={limit} t={t} />
          ))}
        </div>
      ) : (
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <Activity size={16} />
          <span>{t("provider.usageUnavailable")}</span>
        </div>
      )}
      {mcp?.usageDetails.length ? (
        <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs">
          {mcp.usageDetails.map((detail) => (
            <span key={detail.name}>
              <small className="text-muted-foreground">{detail.name}</small>
              <strong className="ml-1">{formatToolUsage(detail)}</strong>
            </span>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function QuotaRow({
  label,
  limit,
  t,
}: {
  label: string;
  limit: ZhipuQuotaLimit;
  t: TFunction;
}) {
  const remainingPercent = quotaRemainingPercent(limit);
  const resetText = formatResetAt(limit.nextResetAt);
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between text-xs">
        <strong>{label}</strong>
        <span className="text-muted-foreground">
          {t("provider.remainingPercent", { percent: formatPercent(remainingPercent) })}
        </span>
      </div>
      <div className="h-1.5 rounded-full bg-muted">
        <span
          className="block h-full rounded-full bg-primary/60 transition-all"
          style={{ width: `${remainingPercent}%` }}
        />
      </div>
      <div className="flex items-center justify-between text-xs text-muted-foreground">
        <span>{quotaAmountText(limit)}</span>
        {resetText ? <span>{t("provider.resetAt", { time: resetText })}</span> : null}
      </div>
    </div>
  );
}

function providerUsageSummary(
  provider: ProviderRecord,
  usage: ProviderUsageRecord | undefined,
  loading: boolean,
  t: TFunction,
) {
  if (loading && !usage) {
    return t("provider.usageChecking");
  }
  if (!usage) {
    return t("provider.usageNotLoaded");
  }
  switch (usage.status) {
    case "unsupported":
      return t("provider.usageUnsupported");
    case "missingCredential":
      return t("provider.usageMissingCredentialShort");
    case "failed":
      return t("provider.usageFailed");
    case "ready":
      if (usage.usageKind === "deepseekBalance" && usage.balance) {
        const primary = primaryBalance(usage.balance.balances);
        return primary ? formatMoney(primary.totalBalance, primary.currency) : t("provider.usageUnavailable");
      }
      if (provider.templateKind === "zhipu-coding-plan" && usage.codingPlan) {
        const fiveHour = findLimit(usage.codingPlan.limits, "fiveHour");
        const weekly = findLimit(usage.codingPlan.limits, "weekly");
        if (fiveHour && weekly) {
          return `${t("provider.quotaFiveHour")} ${formatPercent(quotaRemainingPercent(fiveHour))} · ${t(
            "provider.quotaWeekly",
          )} ${formatPercent(quotaRemainingPercent(weekly))}`;
        }
      }
      return t("provider.usageUnavailable");
  }
}

function providerUsageStatusClass(usage: ProviderUsageRecord | undefined, loading: boolean) {
  if (loading && !usage) {
    return "checking";
  }
  if (!usage) {
    return "muted";
  }
  switch (usage.status) {
    case "ready":
      return "ready";
    case "failed":
      return "danger";
    case "missingCredential":
      return "warning";
    case "unsupported":
      return "muted";
  }
}

function findLimit(limits: ZhipuQuotaLimit[], window: ZhipuQuotaLimit["window"]) {
  return limits.find((limit) => limit.window === window);
}

function primaryBalance(balances: { currency: string; totalBalance: string }[]) {
  return balances.find((item) => item.currency.toUpperCase() === "CNY") ?? balances[0] ?? null;
}

function quotaRemainingPercent(limit: ZhipuQuotaLimit) {
  if (typeof limit.remaining === "number" && typeof limit.total === "number" && limit.total > 0) {
    return clampPercent((limit.remaining / limit.total) * 100);
  }
  return clampPercent(100 - limit.percentage);
}

function quotaAmountText(limit: ZhipuQuotaLimit) {
  if (typeof limit.remaining === "number" && typeof limit.total === "number") {
    return `${formatNumber(limit.remaining)} / ${formatNumber(limit.total)}`;
  }
  if (typeof limit.remaining === "number") {
    return formatNumber(limit.remaining);
  }
  if (typeof limit.currentValue === "number" && typeof limit.total === "number") {
    return `${formatNumber(Math.max(0, limit.total - limit.currentValue))} / ${formatNumber(limit.total)}`;
  }
  return `${formatPercent(quotaRemainingPercent(limit))}`;
}

function formatToolUsage(detail: {
  currentValue?: number | null;
  total?: number | null;
  percentage?: number | null;
}) {
  if (typeof detail.currentValue === "number" && typeof detail.total === "number") {
    const remaining = Math.max(0, detail.total - detail.currentValue);
    return `${formatNumber(remaining)} / ${formatNumber(detail.total)}`;
  }
  if (typeof detail.percentage === "number") {
    return formatPercent(clampPercent(100 - detail.percentage));
  }
  return "--";
}

function formatMoney(amount: string, currency: string) {
  return `${currency} ${amount}`;
}

function formatNumber(value: number) {
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value);
}

function formatPercent(value: number) {
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value)}%`;
}

function formatResetAt(value: number | null | undefined) {
  if (!value) {
    return null;
  }
  return new Date(value * 1000).toLocaleString(undefined, {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function clampPercent(value: number) {
  if (!Number.isFinite(value)) {
    return 0;
  }
  return Math.max(0, Math.min(100, value));
}

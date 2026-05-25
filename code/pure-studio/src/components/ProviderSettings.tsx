import {
  CheckCircle2,
  Cpu,
  KeyRound,
  Link2,
  Pencil,
  Plus,
  Search,
  Server,
  Trash2,
} from "lucide-react";
import { useState, type Dispatch, type SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import type { ModelRecord, ProviderKind, ProviderRecord, ProviderTemplateRecord } from "../types";
import { cloneModel } from "../lib/provider-mapper";
import { allModels, initials, providerStatusClass, translateStatus, translateUpdatedAt } from "../lib/utils";
import { ProviderEditPage } from "./ProviderEditPage";

type ProviderSettingsProps = {
  providers: ProviderRecord[];
  templates: ProviderTemplateRecord[];
  selectedProviderId: string | null;
  providerSearch: string;
  setProviders: Dispatch<SetStateAction<ProviderRecord[]>>;
  setSelectedProviderId: Dispatch<SetStateAction<string | null>>;
  setProviderSearch: Dispatch<SetStateAction<string>>;
};

function normalizeProvider(provider: ProviderRecord): ProviderRecord {
  const models = allModels(provider);
  const defaultModel = models.some((model) => model.slug === provider.defaultModel)
    ? provider.defaultModel
    : (models[0]?.slug ?? "");
  return {
    ...provider,
    subtitle: `${provider.name || provider.id} Platform`,
    status: provider.bearerToken.trim() ? "Healthy" : "Needs setup",
    defaultModel,
    models,
    modelCount: models.length.toString(),
  };
}

function templateProvider(template: ProviderTemplateRecord, id: string): ProviderRecord {
  const provider: ProviderRecord = {
    id,
    templateKind: template.id,
    name: template.name,
    subtitle: `${template.name} Platform`,
    status: "Needs setup",
    baseUrl: template.baseUrl,
    bearerToken: "",
    defaultModel: template.defaultModel,
    modelCount: template.defaultModels.length.toString(),
    updatedAt: "Draft",
    wireApi: template.wireApi,
    models: template.defaultModels,
    defaultModels: template.defaultModels,
    customModels: [],
  };
  return normalizeProvider(provider);
}

function suggestProviderId(providers: ProviderRecord[], kind: ProviderKind) {
  if (!providers.some((provider) => provider.id === kind)) {
    return kind;
  }
  for (let index = 2; ; index += 1) {
    const candidate = `${kind}-${index}`;
    if (!providers.some((provider) => provider.id === candidate)) {
      return candidate;
    }
  }
}

export function ProviderSettings({
  providers,
  templates,
  selectedProviderId,
  providerSearch,
  setProviders,
  setSelectedProviderId,
  setProviderSearch,
}: ProviderSettingsProps) {
  const { t } = useTranslation();
  const [editingProviderId, setEditingProviderId] = useState<string | null>(null);
  const filteredProviders = providers.filter((provider) => {
    const query = providerSearch.trim().toLowerCase();
    if (!query) {
      return true;
    }
    return (
      provider.name.toLowerCase().includes(query) ||
      provider.id.toLowerCase().includes(query) ||
      provider.baseUrl.toLowerCase().includes(query)
    );
  });
  const selectedProvider =
    providers.find((provider) => provider.id === selectedProviderId) ?? providers[0] ?? null;
  const editingProvider =
    providers.find((provider) => provider.id === editingProviderId) ?? null;

  function updateEditingProvider(updater: (provider: ProviderRecord) => ProviderRecord) {
    if (!editingProvider) {
      return;
    }
    const previousId = editingProvider.id;
    const nextProvider = normalizeProvider(updater(editingProvider));
    setProviders((current) =>
      current.map((provider) => (provider.id === previousId ? nextProvider : provider)),
    );
    setEditingProviderId(nextProvider.id);
    setSelectedProviderId((current) => (current === previousId ? nextProvider.id : current));
  }

  function addProvider(kind: ProviderKind) {
    const template = templates.find((item) => item.id === kind);
    if (!template) {
      return;
    }
    const nextProvider = templateProvider(template, suggestProviderId(providers, kind));
    setProviders((current) => [...current, nextProvider]);
    setSelectedProviderId((current) => current ?? nextProvider.id);
    setEditingProviderId(nextProvider.id);
  }

  function changeEditingTemplate(kind: ProviderKind) {
    const template = templates.find((item) => item.id === kind);
    if (!template) {
      return;
    }
    updateEditingProvider((provider) => ({
      ...provider,
      templateKind: kind,
      name: provider.name || template.name,
      baseUrl: template.baseUrl,
      wireApi: template.wireApi,
      defaultModel: template.defaultModel,
      defaultModels: template.defaultModels.map(cloneModel),
    }));
  }

  function addCustomModel() {
    updateEditingProvider((provider) => {
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
            displayName: "Custom Model",
            reasoningEfforts: ["high"],
          },
        ],
        defaultModel: provider.defaultModel || slug,
      };
    });
  }

  function updateCustomModel(index: number, patch: Partial<ModelRecord>) {
    updateEditingProvider((provider) => ({
      ...provider,
      customModels: provider.customModels.map((model, modelIndex) =>
        modelIndex === index ? { ...model, ...patch } : model,
      ),
    }));
  }

  function removeCustomModel(index: number) {
    updateEditingProvider((provider) => ({
      ...provider,
      customModels: provider.customModels.filter((_, modelIndex) => modelIndex !== index),
    }));
  }

  function removeProvider(providerId: string) {
    if (providers.length <= 1) {
      return;
    }
    const remainingProviders = providers.filter((provider) => provider.id !== providerId);
    setProviders(remainingProviders);
    setSelectedProviderId((current) =>
      current === providerId ? (remainingProviders[0]?.id ?? null) : current,
    );
    if (editingProviderId === providerId) {
      setEditingProviderId(null);
    }
  }

  return (
    <div className="provider-settings">
      {editingProvider ? (
        <ProviderEditPage
          provider={editingProvider}
          templates={templates}
          onBack={() => setEditingProviderId(null)}
          onChangeTemplate={changeEditingTemplate}
          onUpdateProvider={updateEditingProvider}
          onAddCustomModel={addCustomModel}
          onUpdateCustomModel={updateCustomModel}
          onRemoveCustomModel={removeCustomModel}
        />
      ) : (
        <section className="provider-directory">
          <div className="provider-console-head">
            <div>
              <h2>{t("settings.providerRoute")}</h2>
              <p>{t("settings.providerRouteDesc")}</p>
            </div>
            <div className="provider-console-tools">
              <label className="search-box">
                <Search size={16} />
                <input
                  value={providerSearch}
                  onChange={(event) => setProviderSearch(event.target.value)}
                  placeholder={t("provider.searchPlaceholder")}
                />
              </label>
              <div className="provider-add-actions">
                {templates.map((template) => (
                  <button key={template.id} onClick={() => addProvider(template.id)}>
                    <Plus size={16} />
                    {t("provider.addProvider", { name: template.name })}
                  </button>
                ))}
              </div>
            </div>
          </div>

          <div className="provider-card-list">
            {filteredProviders.length === 0 ? (
              <div className="provider-empty-state">
                <Server size={28} />
                <strong>{t("provider.noMatchingProviders")}</strong>
                <span>{t("provider.clearSearchHint")}</span>
              </div>
            ) : (
              filteredProviders.map((provider) => {
                const isActive = provider.id === selectedProvider?.id;
                const models = allModels(provider);
                const templateName =
                  templates.find((template) => template.id === provider.templateKind)?.name ??
                  provider.templateKind;
                const defaultModel =
                  models.find((model) => model.slug === provider.defaultModel)?.displayName ||
                  provider.defaultModel ||
                  t("provider.notSelected");
                const previewModels = models.slice(0, 4);
                const remainingModels = Math.max(0, models.length - previewModels.length);

                return (
                  <article
                    key={provider.id}
                    className={`provider-card ${isActive ? "active" : ""}`}
                  >
                    <div className="provider-card-shell">
                      <button
                        className="provider-card-select"
                        onClick={() => setSelectedProviderId(provider.id)}
                      >
                        <span className={`provider-badge provider-${provider.templateKind}`}>
                          {initials(provider.name) || "P"}
                        </span>
                        <span className="provider-title-block">
                          <span className="provider-title-line">
                            <strong>{provider.name || provider.id}</strong>
                            <span className={`provider-state ${providerStatusClass(provider)}`}>
                              <CheckCircle2 size={14} />
                              {translateStatus(provider.status, t)}
                            </span>
                            {isActive ? <span className="default-route">{t("provider.defaultRoute")}</span> : null}
                          </span>
                          <span className="provider-url">
                            <Link2 size={14} />
                            {provider.baseUrl || t("provider.defaultBaseUrl")}
                          </span>
                        </span>
                      </button>

                      <div className="provider-card-actions">
                        <button
                          className="icon-button provider-icon-action"
                          onClick={() => setEditingProviderId(provider.id)}
                          title={t("provider.editTooltip")}
                        >
                          <Pencil size={16} />
                        </button>
                        <button
                          className="icon-button provider-icon-action danger"
                          disabled={providers.length <= 1}
                          onClick={() => removeProvider(provider.id)}
                          title={t("provider.deleteTooltip")}
                        >
                          <Trash2 size={16} />
                        </button>
                      </div>
                    </div>

                    <div className="provider-card-meta">
                      <span>
                        <Server size={14} />
                        <small>{t("provider.key")}</small>
                        <strong>{provider.id}</strong>
                      </span>
                      <span>
                        <Cpu size={14} />
                        <small>{t("provider.defaultModel")}</small>
                        <strong>{defaultModel}</strong>
                      </span>
                      <span>
                        <KeyRound size={14} />
                        <small>{t("provider.apiKey")}</small>
                        <strong>{provider.bearerToken ? t("provider.saved") : t("provider.notConfigured")}</strong>
                      </span>
                      <span>
                        <small>{t("provider.template")}</small>
                        <strong>{templateName}</strong>
                      </span>
                      <span>
                        <small>{t("provider.protocolType")}</small>
                        <strong>{provider.wireApi}</strong>
                      </span>
                      <span>
                        <small>{t("provider.updated")}</small>
                        <strong>{translateUpdatedAt(provider.updatedAt, t)}</strong>
                      </span>
                    </div>

                    <div className="provider-model-strip">
                      <span className="model-count-pill">{t("provider.models", { count: provider.modelCount })}</span>
                      {previewModels.map((model) => (
                        <span
                          key={model.slug}
                          className={
                            model.slug === provider.defaultModel ? "model-chip active" : "model-chip"
                          }
                        >
                          {model.displayName || model.slug}
                        </span>
                      ))}
                      {remainingModels > 0 ? (
                        <span className="model-chip muted-chip">+{remainingModels}</span>
                      ) : null}
                    </div>
                  </article>
                );
              })
            )}
          </div>
        </section>
      )}

    </div>
  );
}

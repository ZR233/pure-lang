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
import type { ModelRecord, ProviderKind, ProviderRecord, ProviderTemplateRecord } from "../types";
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

function initials(name: string) {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase() ?? "")
    .join("");
}

function allModels(provider: ProviderRecord) {
  return [...provider.defaultModels, ...provider.customModels];
}

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

function cloneModel(model: ModelRecord): ModelRecord {
  return {
    slug: model.slug,
    displayName: model.displayName,
    description: model.description ?? null,
    contextWindow: model.contextWindow ?? null,
    maxContextWindow: model.maxContextWindow ?? null,
    autoCompactTokenLimit: model.autoCompactTokenLimit ?? null,
    defaultTemperature: model.defaultTemperature ?? null,
    maxOutputTokens: model.maxOutputTokens ?? null,
    reasoningEfforts: [...model.reasoningEfforts],
    capabilities: [...(model.capabilities ?? [])],
    inputModalities: [...(model.inputModalities ?? [])],
    truncationMode: model.truncationMode,
    truncationLimit: model.truncationLimit,
  };
}

function providerStatusClass(provider: ProviderRecord) {
  return provider.status.toLowerCase().includes("healthy") ? "ready" : "needs-setup";
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
              <h2>Provider 路由</h2>
              <p>配置 ~/.pure/config.toml 中的模型接入点。</p>
            </div>
            <div className="provider-console-tools">
              <label className="search-box">
                <Search size={16} />
                <input
                  value={providerSearch}
                  onChange={(event) => setProviderSearch(event.target.value)}
                  placeholder="搜索 provider、key 或 base URL"
                />
              </label>
              <div className="provider-add-actions">
                {templates.map((template) => (
                  <button key={template.id} onClick={() => addProvider(template.id)}>
                    <Plus size={16} />
                    {template.name}
                  </button>
                ))}
              </div>
            </div>
          </div>

          <div className="provider-card-list">
            {filteredProviders.length === 0 ? (
              <div className="provider-empty-state">
                <Server size={28} />
                <strong>没有匹配的 provider</strong>
                <span>清空搜索条件后可查看完整列表。</span>
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
                  "未选择";
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
                              {provider.status}
                            </span>
                            {isActive ? <span className="default-route">默认路由</span> : null}
                          </span>
                          <span className="provider-url">
                            <Link2 size={14} />
                            {provider.baseUrl || "(default base URL)"}
                          </span>
                        </span>
                      </button>

                      <div className="provider-card-actions">
                        <button
                          className="icon-button provider-icon-action"
                          onClick={() => setEditingProviderId(provider.id)}
                          title="编辑 provider"
                        >
                          <Pencil size={16} />
                        </button>
                        <button
                          className="icon-button provider-icon-action danger"
                          disabled={providers.length <= 1}
                          onClick={() => removeProvider(provider.id)}
                          title="删除 provider"
                        >
                          <Trash2 size={16} />
                        </button>
                      </div>
                    </div>

                    <div className="provider-card-meta">
                      <span>
                        <Server size={14} />
                        <small>Key</small>
                        <strong>{provider.id}</strong>
                      </span>
                      <span>
                        <Cpu size={14} />
                        <small>Default model</small>
                        <strong>{defaultModel}</strong>
                      </span>
                      <span>
                        <KeyRound size={14} />
                        <small>API Key</small>
                        <strong>{provider.bearerToken ? "已保存" : "未配置"}</strong>
                      </span>
                      <span>
                        <small>Template</small>
                        <strong>{templateName}</strong>
                      </span>
                      <span>
                        <small>协议类型</small>
                        <strong>{provider.wireApi}</strong>
                      </span>
                      <span>
                        <small>Updated</small>
                        <strong>{provider.updatedAt}</strong>
                      </span>
                    </div>

                    <div className="provider-model-strip">
                      <span className="model-count-pill">{provider.modelCount} models</span>
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

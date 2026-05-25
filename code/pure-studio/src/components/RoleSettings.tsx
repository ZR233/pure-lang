import type { Dispatch, SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import type { ModelRecord, ProviderRecord, RoleKey, RoleRecord } from "../types";

type RoleSettingsProps = {
  roles: RoleRecord[];
  providers: ProviderRecord[];
  setRoles: Dispatch<SetStateAction<RoleRecord[]>>;
};

const ROLE_I18N_KEYS: Record<RoleKey, { label: string; hint: string }> = {
  explorer: { label: "roles.explorer", hint: "roles.explorerHint" },
  planner: { label: "roles.planner", hint: "roles.plannerHint" },
  executor: { label: "roles.executor", hint: "roles.executorHint" },
  reviewer: { label: "roles.reviewer", hint: "roles.reviewerHint" },
};

const ROLE_ORDER: RoleKey[] = ["explorer", "planner", "executor", "reviewer"];

function allModels(provider: ProviderRecord) {
  return provider.models.length > 0
    ? provider.models
    : [...provider.defaultModels, ...provider.customModels];
}

function modelForProvider(provider: ProviderRecord, modelSlug: string): ModelRecord | null {
  return allModels(provider).find((model) => model.slug === modelSlug) ?? null;
}

function providerDefaultModel(provider: ProviderRecord) {
  const models = allModels(provider);
  return models.some((model) => model.slug === provider.defaultModel)
    ? provider.defaultModel
    : (models[0]?.slug ?? "");
}

function effortForModel(provider: ProviderRecord, modelSlug: string, preferredEffort?: string) {
  const model = modelForProvider(provider, modelSlug);
  if (!model) {
    return "";
  }
  if (preferredEffort && model.reasoningEfforts.includes(preferredEffort)) {
    return preferredEffort;
  }
  return model.reasoningEfforts[0] ?? "";
}

export function normalizeRolesForProviders(
  roles: RoleRecord[],
  providers: ProviderRecord[],
): RoleRecord[] {
  const fallbackProvider = providers[0] ?? null;
  if (!fallbackProvider) {
    return ROLE_ORDER.map((key) => ({
      key,
      displayName: key,
      provider: "",
      model: "",
      effort: "",
    }));
  }

  return ROLE_ORDER.map((key) => {
    const role = roles.find((item) => item.key === key);
    const provider = providers.find((item) => item.id === role?.provider) ?? fallbackProvider;
    const model = modelForProvider(provider, role?.model ?? "")
      ? (role?.model ?? "")
      : providerDefaultModel(provider);
    return {
      key,
      displayName: role?.displayName || key,
      provider: provider.id,
      model,
      effort: effortForModel(provider, model, role?.effort),
    };
  });
}

export function RoleSettings({ roles, providers, setRoles }: RoleSettingsProps) {
  const { t } = useTranslation();
  const normalizedRoles = normalizeRolesForProviders(roles, providers);

  function replaceRole(nextRole: RoleRecord) {
    setRoles((current) => {
      const normalized = normalizeRolesForProviders(current, providers);
      return normalized.map((role) => (role.key === nextRole.key ? nextRole : role));
    });
  }

  function changeProvider(role: RoleRecord, providerId: string) {
    const provider = providers.find((item) => item.id === providerId);
    if (!provider) {
      return;
    }
    const model = providerDefaultModel(provider);
    replaceRole({
      ...role,
      provider: provider.id,
      model,
      effort: effortForModel(provider, model),
    });
  }

  function changeModel(role: RoleRecord, modelSlug: string) {
    const provider = providers.find((item) => item.id === role.provider);
    if (!provider) {
      return;
    }
    replaceRole({
      ...role,
      model: modelSlug,
      effort: effortForModel(provider, modelSlug),
    });
  }

  function changeEffort(role: RoleRecord, effort: string) {
    replaceRole({ ...role, effort });
  }

  return (
    <section className="role-settings">
      <div className="role-settings-head">
        <div>
          <h2>{t("settings.roleRoute")}</h2>
          <p>{t("settings.roleRouteDesc")}</p>
        </div>
      </div>

      <div className="role-card-list">
        {normalizedRoles.map((role) => {
          const provider = providers.find((item) => item.id === role.provider) ?? providers[0];
          const models = provider ? allModels(provider) : [];
          const selectedModel = models.find((model) => model.slug === role.model);
          const efforts = selectedModel?.reasoningEfforts ?? [];
          const roleI18n = ROLE_I18N_KEYS[role.key];

          return (
            <article className="role-card" key={role.key}>
              <div className="role-card-title">
                <div>
                  <h3>{t(roleI18n.label)}</h3>
                  <p>{t(roleI18n.hint)}</p>
                </div>
                <span>{role.key}</span>
              </div>

              <div className="role-form-grid">
                <label>
                  <span>Provider</span>
                  <select
                    value={role.provider}
                    onChange={(event) => changeProvider(role, event.target.value)}
                  >
                    {providers.map((providerOption) => (
                      <option key={providerOption.id} value={providerOption.id}>
                        {providerOption.name || providerOption.id} ({providerOption.id})
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  <span>Model</span>
                  <select
                    value={role.model}
                    onChange={(event) => changeModel(role, event.target.value)}
                  >
                    {models.map((model) => (
                      <option key={model.slug} value={model.slug}>
                        {model.displayName || model.slug}
                      </option>
                    ))}
                  </select>
                </label>
                <label>
                  <span>Effort</span>
                  <select
                    value={role.effort}
                    onChange={(event) => changeEffort(role, event.target.value)}
                  >
                    {efforts.map((effort) => (
                      <option key={effort} value={effort}>
                        {effort}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}

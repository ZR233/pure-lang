import type { Dispatch, SetStateAction } from "react";
import type { ModelRecord, ProviderRecord, RoleKey, RoleRecord } from "../types";

type RoleSettingsProps = {
  roles: RoleRecord[];
  providers: ProviderRecord[];
  setRoles: Dispatch<SetStateAction<RoleRecord[]>>;
};

const ROLE_LABELS: Record<RoleKey, string> = {
  explorer: "探索者",
  planner: "计划者",
  executor: "执行者",
  reviewer: "审查者",
};

const ROLE_HINTS: Record<RoleKey, string> = {
  explorer: "代码、文档和上下文探索",
  planner: "默认聊天和计划生成",
  executor: "subagent 默认执行角色",
  reviewer: "代码审查和结果检查",
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
      displayName: ROLE_LABELS[key],
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
      displayName: role?.displayName || ROLE_LABELS[key],
      provider: provider.id,
      model,
      effort: effortForModel(provider, model, role?.effort),
    };
  });
}

export function RoleSettings({ roles, providers, setRoles }: RoleSettingsProps) {
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
          <h2>角色路由</h2>
          <p>固定四个模型角色，聊天使用计划者，subagent 默认使用执行者。</p>
        </div>
      </div>

      <div className="role-card-list">
        {normalizedRoles.map((role) => {
          const provider = providers.find((item) => item.id === role.provider) ?? providers[0];
          const models = provider ? allModels(provider) : [];
          const selectedModel = models.find((model) => model.slug === role.model);
          const efforts = selectedModel?.reasoningEfforts ?? [];

          return (
            <article className="role-card" key={role.key}>
              <div className="role-card-title">
                <div>
                  <h3>{role.displayName}</h3>
                  <p>{ROLE_HINTS[role.key]}</p>
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

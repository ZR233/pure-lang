import type { Dispatch, SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import { Card } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ModelRecord, ProviderRecord, RoleKey, RoleRecord } from "../types";
import { allModels } from "../lib/utils";

type RoleSettingsProps = {
  roles: RoleRecord[];
  providers: ProviderRecord[];
  setRoles: Dispatch<SetStateAction<RoleRecord[]>>;
  onSaveRoles: (roles: RoleRecord[]) => Promise<boolean>;
};

const ROLE_I18N_KEYS: Record<RoleKey, { label: string; hint: string }> = {
  explorer: { label: "roles.explorer", hint: "roles.explorerHint" },
  planner: { label: "roles.planner", hint: "roles.plannerHint" },
  executor: { label: "roles.executor", hint: "roles.executorHint" },
  reviewer: { label: "roles.reviewer", hint: "roles.reviewerHint" },
};

const ROLE_ORDER: RoleKey[] = ["explorer", "planner", "executor", "reviewer"];

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

export function RoleSettings({ roles, providers, setRoles, onSaveRoles }: RoleSettingsProps) {
  const { t } = useTranslation();
  const normalizedRoles = normalizeRolesForProviders(roles, providers);

  function replaceRole(nextRole: RoleRecord) {
    const nextRoles = normalizedRoles.map((role) =>
      role.key === nextRole.key ? nextRole : role,
    );
    setRoles(nextRoles);
    void onSaveRoles(nextRoles);
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
    <section className="space-y-4">
      <div>
        <h2 className="text-lg font-semibold text-foreground">{t("settings.roleRoute")}</h2>
        <p className="text-sm text-muted-foreground">{t("settings.roleRouteDesc")}</p>
      </div>

      <div className="grid grid-cols-2 gap-3.5 content-start">
        {normalizedRoles.map((role) => {
          const provider = providers.find((item) => item.id === role.provider) ?? providers[0];
          const models = provider ? allModels(provider) : [];
          const selectedModel = models.find((model) => model.slug === role.model);
          const efforts = selectedModel?.reasoningEfforts ?? [];
          const roleI18n = ROLE_I18N_KEYS[role.key];

          return (
            <Card className="grid gap-4 p-4" key={role.key}>
              <div className="flex items-start justify-between gap-2">
                <div>
                  <h3 className="text-sm font-semibold text-foreground">{t(roleI18n.label)}</h3>
                  <p className="text-xs text-muted-foreground">{t(roleI18n.hint)}</p>
                </div>
                <span className="text-xs text-muted-foreground">{role.key}</span>
              </div>

              <div className="grid gap-3">
                <div className="space-y-1.5">
                  <Label>{t("roleRoute.provider")}</Label>
                  <Select
                    value={role.provider}
                    onValueChange={(value) => changeProvider(role, value)}
                  >
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {providers.map((providerOption) => (
                        <SelectItem key={providerOption.id} value={providerOption.id}>
                          {providerOption.name || providerOption.id} ({providerOption.id})
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="space-y-1.5">
                  <Label>{t("roleRoute.model")}</Label>
                  <Select
                    value={role.model}
                    onValueChange={(value) => changeModel(role, value)}
                  >
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {models.map((model) => (
                        <SelectItem key={model.slug} value={model.slug}>
                          {model.displayName || model.slug}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="space-y-1.5">
                  <Label>{t("roleRoute.effort")}</Label>
                  <Select
                    value={role.effort}
                    onValueChange={(value) => changeEffort(role, value)}
                  >
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {efforts.map((effort) => (
                        <SelectItem key={effort} value={effort}>
                          {effort}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              </div>
            </Card>
          );
        })}
      </div>
    </section>
  );
}

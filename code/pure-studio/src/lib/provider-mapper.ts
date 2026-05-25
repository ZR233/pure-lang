import type { ModelRecord, ProviderInput, ProviderRecord, RoleInput, RoleRecord } from "../types";
import { previewTemplates } from "./templates";

export function cloneModel(model: ModelRecord): ModelRecord {
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

export function makeProvider(input: ProviderInput): ProviderRecord {
  const template = previewTemplates.find((item) => item.id === input.templateKind);
  const defaultModels = template?.defaultModels.map(cloneModel) ?? [];
  const customModels = input.customModels.map(cloneModel);
  const models = [...defaultModels, ...customModels];
  const defaultModel = models.some((model) => model.slug === input.defaultModel)
    ? input.defaultModel
    : (models[0]?.slug ?? "");
  return {
    id: input.id,
    templateKind: input.templateKind,
    name: input.name,
    subtitle: `${input.name || input.id} Platform`,
    status: input.bearerToken.trim() ? "Healthy" : "Needs setup",
    baseUrl: input.baseUrl,
    bearerToken: input.bearerToken,
    defaultModel,
    modelCount: models.length.toString(),
    updatedAt: "Preview",
    wireApi: input.wireApi,
    models,
    defaultModels,
    customModels,
  };
}

export function makeRole(input: RoleInput, fallbackRoles: RoleRecord[]): RoleRecord {
  const displayName =
    fallbackRoles.find((role) => role.key === input.key)?.displayName ?? input.key;
  return {
    key: input.key,
    displayName,
    provider: input.provider,
    model: input.model,
    effort: input.effort,
  };
}

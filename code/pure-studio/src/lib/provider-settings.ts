import type { ProviderKind, ProviderRecord, ProviderTemplateRecord } from "../types";
import { cloneModel } from "./provider-mapper";

function combinedModels(provider: ProviderRecord) {
  return [...provider.defaultModels, ...provider.customModels];
}

export function normalizeProvider(provider: ProviderRecord): ProviderRecord {
  const defaultModels = provider.defaultModels.map(cloneModel);
  const customModels = provider.customModels.map(cloneModel);
  const models = combinedModels({ ...provider, defaultModels, customModels });
  const defaultModel = models.some((model) => model.slug === provider.defaultModel)
    ? provider.defaultModel
    : (models[0]?.slug ?? "");
  const hasBearerToken = Boolean(provider.bearerToken.trim() || provider.hasBearerToken);
  return {
    ...provider,
    subtitle: `${provider.name || provider.id} Platform`,
    status: hasBearerToken ? "Healthy" : "Needs setup",
    hasBearerToken,
    defaultModel,
    models,
    defaultModels,
    customModels,
    modelCount: models.length.toString(),
  };
}

export function cloneProvider(provider: ProviderRecord): ProviderRecord {
  return normalizeProvider({
    ...provider,
    models: provider.models.map(cloneModel),
    defaultModels: provider.defaultModels.map(cloneModel),
    customModels: provider.customModels.map(cloneModel),
  });
}

export function suggestProviderId(providers: ProviderRecord[], kind: ProviderKind) {
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

export function createProviderFromTemplate(
  template: ProviderTemplateRecord,
  id: string,
): ProviderRecord {
  return normalizeProvider({
    id,
    templateKind: template.id,
    name: template.name,
    subtitle: `${template.name} Platform`,
    status: "Needs setup",
    baseUrl: template.baseUrl,
    bearerToken: "",
    hasBearerToken: false,
    defaultModel: template.defaultModel,
    modelCount: template.defaultModels.length.toString(),
    updatedAt: "Draft",
    wireApi: template.wireApi,
    models: template.defaultModels.map(cloneModel),
    defaultModels: template.defaultModels.map(cloneModel),
    customModels: [],
  });
}

export function applyProviderTemplate(
  provider: ProviderRecord,
  template: ProviderTemplateRecord,
  override?: { id?: string; name?: string },
): ProviderRecord {
  return normalizeProvider({
    ...provider,
    id: override?.id ?? provider.id,
    templateKind: template.id,
    name: override?.name ?? provider.name,
    baseUrl: template.baseUrl,
    wireApi: template.wireApi,
    defaultModel: template.defaultModel,
    defaultModels: template.defaultModels.map(cloneModel),
    models: template.defaultModels.map(cloneModel),
  });
}

export function replaceProvider(
  providers: ProviderRecord[],
  originalId: string,
  nextProvider: ProviderRecord,
) {
  return providers.map((provider) => (provider.id === originalId ? nextProvider : provider));
}

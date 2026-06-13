import type { ModelCapabilities, ModelRecord, ProviderInput, ProviderRecord, RoleInput, RoleRecord } from "../types";
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
    currency: model.currency ?? null,
    inputPricePerMTok: model.inputPricePerMTok ?? null,
    outputPricePerMTok: model.outputPricePerMTok ?? null,
    cacheReadPricePerMTok: model.cacheReadPricePerMTok ?? null,
    reasoningEfforts: [...model.reasoningEfforts],
    capabilities: cloneCapabilities(model.capabilities),
    truncationMode: model.truncationMode,
    truncationLimit: model.truncationLimit,
    baseInstructions: model.baseInstructions ?? "",
  };
}

export function defaultTextCapabilities(): ModelCapabilities {
  return {
    streaming: true,
    temperature: false,
    reasoning: true,
    webSearch: false,
    input: ["text"],
    output: ["text"],
    tools: {
      functionCalling: true,
      parallelToolCalls: true,
      customTools: false,
      freeformTools: false,
    },
    interleaved: { field: "reasoning_content" },
  };
}

function cloneCapabilities(capabilities: ModelCapabilities | undefined): ModelCapabilities {
  const source = capabilities ?? defaultTextCapabilities();
  return {
    streaming: source.streaming,
    temperature: source.temperature,
    reasoning: source.reasoning,
    webSearch: source.webSearch,
    input: [...source.input],
    output: [...source.output],
    tools: { ...source.tools },
    interleaved: source.interleaved ? { ...source.interleaved } : null,
  };
}

export function makeProvider(input: ProviderInput): ProviderRecord {
  const template = previewTemplates.find((item) => item.id === input.templateKind);
  const defaultModels = template?.defaultModels.map(cloneModel) ?? [];
  const customModels = input.customModels.map((model) =>
    cloneModel({
      slug: model.slug,
      displayName: model.displayName,
      reasoningEfforts: [...model.reasoningEfforts],
      capabilities: defaultTextCapabilities(),
      baseInstructions: model.baseInstructions ?? "",
    }),
  );
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
    providerKind: input.providerKind,
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

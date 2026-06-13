import type { ModelCapabilities, ProviderTemplateRecord } from "../types";

function textCapabilities(): ModelCapabilities {
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

function openAiCapabilities(): ModelCapabilities {
  return {
    streaming: true,
    temperature: false,
    reasoning: true,
    webSearch: true,
    input: ["text", "image"],
    output: ["text"],
    tools: {
      functionCalling: true,
      parallelToolCalls: true,
      customTools: true,
      freeformTools: true,
    },
    interleaved: null,
  };
}

function zhipuTextModel(
  slug: string,
  displayName: string,
  description: string,
  contextWindow: number,
  maxOutputTokens: number,
) {
  return {
    slug,
    displayName,
    description,
    contextWindow,
    maxContextWindow: contextWindow,
    maxOutputTokens,
    reasoningEfforts: ["enabled", "none"],
    capabilities: textCapabilities(),
    truncationMode: "tokens",
    truncationLimit: 10_000,
    baseInstructions: "",
  };
}

function openAiModel(
  slug: string,
  displayName: string,
  description: string,
  contextWindow: number,
  maxContextWindow: number,
  truncationMode: "tokens" | "bytes",
) {
  return {
    slug,
    displayName,
    description,
    contextWindow,
    maxContextWindow,
    maxOutputTokens: null,
    reasoningEfforts: ["medium", "low", "high", "xhigh"],
    capabilities: openAiCapabilities(),
    truncationMode,
    truncationLimit: 10_000,
    baseInstructions: "",
  };
}

const zhipuDefaultModels = [
  zhipuTextModel(
    "glm-5.2",
    "GLM-5.2",
    "Zhipu latest flagship model with stronger coding and long-horizon agent work.",
    1_000_000,
    128_000,
  ),
  zhipuTextModel(
    "glm-5",
    "GLM-5",
    "Zhipu high-intelligence foundation model for coding and agentic planning.",
    200_000,
    128_000,
  ),
  zhipuTextModel(
    "glm-5-turbo",
    "GLM-5-Turbo",
    "Zhipu GLM-5 Turbo model optimized for long task continuity.",
    200_000,
    128_000,
  ),
  zhipuTextModel(
    "glm-4.7",
    "GLM-4.7",
    "Zhipu high-intelligence model for dialogue, reasoning, agents, and coding.",
    200_000,
    128_000,
  ),
  zhipuTextModel(
    "glm-4.7-flashx",
    "GLM-4.7-FlashX",
    "Zhipu lightweight high-speed model for general text tasks.",
    200_000,
    128_000,
  ),
  zhipuTextModel(
    "glm-4.7-flash",
    "GLM-4.7-Flash",
    "Zhipu free GLM-4.7 base model.",
    200_000,
    128_000,
  ),
];

export const previewTemplates: ProviderTemplateRecord[] = [
  {
    id: "deepseek",
    name: "DeepSeek",
    baseUrl: "https://api.deepseek.com",
    defaultModel: "deepseek-v4-flash",
    providerKind: "deep_seek",
    defaultModels: [
      {
        slug: "deepseek-v4-flash",
        displayName: "DeepSeek V4 Flash",
        description: "DeepSeek fast reasoning model with thinking mode.",
        contextWindow: 1_000_000,
        maxContextWindow: 1_000_000,
        maxOutputTokens: 384_000,
        currency: "CNY",
        inputPricePerMTok: 1,
        outputPricePerMTok: 2,
        cacheReadPricePerMTok: 0.02,
        reasoningEfforts: ["high", "max"],
        capabilities: textCapabilities(),
        truncationMode: "tokens",
        truncationLimit: 10_000,
        baseInstructions: "",
      },
      {
        slug: "deepseek-v4-pro",
        displayName: "DeepSeek V4 Pro",
        description: "DeepSeek flagship reasoning model with thinking mode.",
        contextWindow: 1_000_000,
        maxContextWindow: 1_000_000,
        maxOutputTokens: 384_000,
        currency: "CNY",
        inputPricePerMTok: 3,
        outputPricePerMTok: 6,
        cacheReadPricePerMTok: 0.025,
        reasoningEfforts: ["high", "max"],
        capabilities: textCapabilities(),
        truncationMode: "tokens",
        truncationLimit: 10_000,
        baseInstructions: "",
      },
    ],
  },
  {
    id: "openai",
    name: "OpenAI",
    baseUrl: "https://api.openai.com/v1",
    defaultModel: "gpt-5.5",
    providerKind: "open_ai",
    defaultModels: [
      openAiModel(
        "gpt-5.5",
        "GPT-5.5",
        "Frontier model for complex coding, research, and real-world work.",
        272_000,
        272_000,
        "tokens",
      ),
      openAiModel(
        "gpt-5.4",
        "gpt-5.4",
        "Strong model for everyday coding.",
        272_000,
        1_000_000,
        "tokens",
      ),
      openAiModel(
        "gpt-5.4-mini",
        "GPT-5.4-Mini",
        "Small, fast, and cost-efficient model for simpler coding tasks.",
        272_000,
        272_000,
        "tokens",
      ),
    ],
  },
  {
    id: "zhipu",
    name: "Zhipu",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    defaultModel: "glm-5.2",
    providerKind: "zhipu",
    defaultModels: zhipuDefaultModels,
  },
  {
    id: "zhipu-coding-plan",
    name: "Zhipu Coding Plan",
    baseUrl: "https://open.bigmodel.cn/api/coding/paas/v4",
    defaultModel: "glm-5.2",
    providerKind: "zhipu",
    defaultModels: zhipuDefaultModels,
  },
];

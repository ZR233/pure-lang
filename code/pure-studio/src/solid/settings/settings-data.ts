import type {
  InstructionsInput,
  InstructionsRecord,
  KeyValuePair,
  McpServerInput,
  McpServerRecord,
  McpTransport,
  ModelRecord,
  ProviderRecord,
  RoleKey,
  RoleRecord,
} from "../../types";
import { allModels } from "../../lib/utils";

export type SettingsTab = "providers" | "instructions" | "skills" | "roles" | "mcp" | "security" | "general";

export const settingsTabs: SettingsTab[] = ["providers", "instructions", "skills", "roles", "mcp", "security", "general"];

export type InstructionsDraft = {
  baseOverride: string;
  developer: string;
  user: string;
  projectDocMaxBytes: string;
  fallbackFilenames: string;
};

export type McpDraftServer = McpServerInput & Pick<
  McpServerRecord,
  "availabilityKind" | "availabilityMessage" | "lastCheckedAt" | "toolCount"
>;

const roleOrder: RoleKey[] = ["explorer", "planner", "executor", "reviewer"];

export function modelForProvider(provider: ProviderRecord, modelSlug: string): ModelRecord | null {
  return allModels(provider).find((model) => model.slug === modelSlug) ?? null;
}

export function providerDefaultModel(provider: ProviderRecord) {
  const models = allModels(provider);
  return models.some((model) => model.slug === provider.defaultModel)
    ? provider.defaultModel
    : (models[0]?.slug ?? "");
}

export function effortForModel(provider: ProviderRecord, modelSlug: string, preferredEffort?: string) {
  const model = modelForProvider(provider, modelSlug);
  if (!model) return "";
  if (preferredEffort && model.reasoningEfforts.includes(preferredEffort)) return preferredEffort;
  return model.reasoningEfforts[0] ?? "";
}

export function normalizeRolesForProviders(roles: RoleRecord[], providers: ProviderRecord[]): RoleRecord[] {
  const fallbackProvider = providers[0] ?? null;
  if (!fallbackProvider) {
    return roleOrder.map((key) => ({
      key,
      displayName: key,
      provider: "",
      model: "",
      effort: "",
    }));
  }

  return roleOrder.map((key) => {
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

export function instructionsDraft(instructions: InstructionsRecord): InstructionsDraft {
  return {
    baseOverride: instructions.baseOverride,
    developer: instructions.developer,
    user: instructions.user,
    projectDocMaxBytes: String(instructions.projectDocMaxBytes),
    fallbackFilenames: instructions.projectDocFallbackFilenames.join(", "),
  };
}

export function instructionsInput(draft: InstructionsDraft): InstructionsInput {
  const projectDocMaxBytes = Number.parseInt(draft.projectDocMaxBytes, 10);
  return {
    baseOverride: draft.baseOverride,
    developer: draft.developer,
    user: draft.user,
    projectDocMaxBytes: Number.isFinite(projectDocMaxBytes) ? Math.max(0, projectDocMaxBytes) : 65536,
    projectDocFallbackFilenames: draft.fallbackFilenames
      .split(",")
      .map((name) => name.trim())
      .filter(Boolean),
  };
}

export function mcpDraftServer(server: McpServerRecord): McpDraftServer {
  return {
    id: server.id,
    enabled: server.enabled,
    transport: server.transport,
    command: server.command ?? "",
    args: [...server.args],
    env: server.env.map((entry) => ({ ...entry })),
    cwd: server.cwd ?? "",
    url: server.url ?? "",
    bearerTokenEnvVar: server.bearerTokenEnvVar ?? "",
    headers: server.headers.map((entry) => ({ ...entry })),
    sourceKind: server.sourceKind,
    sourceLabel: server.sourceLabel,
    sourceDetail: server.sourceDetail,
    statusKind: server.statusKind,
    statusMessage: server.statusMessage,
    mutationPolicy: server.mutationPolicy,
    availabilityKind: server.availabilityKind,
    availabilityMessage: server.availabilityMessage,
    lastCheckedAt: server.lastCheckedAt,
    toolCount: server.toolCount,
  };
}

export function normalizeMcpServerInput(server: McpDraftServer): McpServerInput {
  return {
    id: server.id.trim(),
    enabled: server.enabled,
    transport: server.transport,
    command: optionalText(server.command),
    args: server.args.map((arg) => arg.trim()).filter(Boolean),
    env: cleanKeyValues(server.env),
    cwd: optionalText(server.cwd),
    url: optionalText(server.url),
    bearerTokenEnvVar: optionalText(server.bearerTokenEnvVar),
    headers: cleanKeyValues(server.headers),
    sourceKind: server.sourceKind,
    sourceLabel: server.sourceLabel,
    sourceDetail: server.sourceDetail,
    statusKind: server.statusKind,
    statusMessage: server.statusMessage,
    mutationPolicy: server.mutationPolicy,
  };
}

export function emptyMcpDraftServer(id: string): McpDraftServer {
  return {
    id,
    enabled: true,
    transport: "stdio",
    command: "",
    args: [],
    env: [],
    cwd: null,
    url: null,
    bearerTokenEnvVar: null,
    headers: [],
    availabilityKind: "checking",
    availabilityMessage: null,
    lastCheckedAt: null,
    toolCount: null,
  };
}

export function uniqueMcpServerId(servers: McpDraftServer[]) {
  const existing = new Set(servers.map((server) => server.id));
  let index = 1;
  while (existing.has(`server-${index}`)) index += 1;
  return `server-${index}`;
}

export function isLockedMcpServer(server: Pick<McpDraftServer, "sourceKind" | "mutationPolicy">) {
  return server.sourceKind === "builtIn" || server.mutationPolicy === "lockedIdentity";
}

export function searchableMcpServerText(server: McpDraftServer) {
  return [
    server.id,
    server.transport,
    server.command ?? "",
    server.url ?? "",
    server.bearerTokenEnvVar ?? "",
    server.sourceKind ?? "",
    server.sourceLabel ?? "",
    server.sourceDetail ?? "",
    server.statusKind ?? "",
    server.statusMessage ?? "",
    server.availabilityKind ?? "",
    server.availabilityMessage ?? "",
    server.lastCheckedAt?.toString() ?? "",
    server.toolCount?.toString() ?? "",
    ...server.args,
    ...server.env.flatMap((entry) => [entry.key, entry.value]),
    ...server.headers.flatMap((entry) => [entry.key, entry.value]),
  ]
    .join(" ")
    .toLowerCase();
}

export function endpointSummary(server: { transport: McpTransport; command?: string | null; url?: string | null }) {
  return server.transport === "stdio" ? server.command ?? "" : server.url ?? "";
}

function cleanKeyValues(values: KeyValuePair[]) {
  return values
    .map((entry) => ({ key: entry.key.trim(), value: entry.value }))
    .filter((entry) => entry.key);
}

function optionalText(value: string | null | undefined) {
  const trimmed = value?.trim() ?? "";
  return trimmed ? trimmed : null;
}

import type { InstructionsInput, McpServerInput, ProviderSettingsInput } from "../types";

export function renderPreviewToml(
  input: ProviderSettingsInput,
  mcpServers: McpServerInput[] = [],
  instructions?: InstructionsInput,
) {
  const userMcpServers = mcpServers.filter((server) => server.sourceKind !== "builtIn");
  return [
    "schema_version = 3",
    "",
    "[runtime]",
    'permission_mode = "request-approval"',
    'active_skills = ["rust", "git", "doc"]',
    'active_mcp_servers = ["github", "filesystem"]',
    "",
    ...instructionsToml(instructions),
    ...userMcpServers.flatMap((server) => mcpServerToml(server)),
    ...input.roles.flatMap((role) => [
      `[roles.${role.key}]`,
      `provider = "${role.provider}"`,
      `model = "${role.model}"`,
      `effort = "${role.effort}"`,
      "",
    ]),
    ...input.providers.flatMap((provider) => [
      `[providers.${provider.id}]`,
      `provider_kind = "${provider.providerKind}"`,
      `name = "${provider.name}"`,
      `base_url = "${provider.baseUrl}"`,
      `bearer_token = "${provider.bearerToken}"`,
      `default_model = "${provider.defaultModel}"`,
      "",
    ]),
  ].join("\n");
}

function instructionsToml(instructions?: InstructionsInput) {
  if (!instructions) return [];
  const lines = ["[instructions]"];
  if (instructions.baseOverride.trim()) {
    lines.push(`base_override = ${tomlString(instructions.baseOverride)}`);
  }
  if (instructions.developer.trim()) {
    lines.push(`developer = ${tomlString(instructions.developer)}`);
  }
  if (instructions.user.trim()) {
    lines.push(`user = ${tomlString(instructions.user)}`);
  }
  lines.push(`project_doc_max_bytes = ${instructions.projectDocMaxBytes}`);
  if (instructions.projectDocFallbackFilenames.length > 0) {
    lines.push(
      `project_doc_fallback_filenames = [${instructions.projectDocFallbackFilenames.map(tomlString).join(", ")}]`,
    );
  }
  return [...lines, ""];
}

function mcpServerToml(server: McpServerInput) {
  const lines = [
    `[mcp_servers.${server.id}]`,
    `enabled = ${server.enabled}`,
    `transport = "${server.transport}"`,
  ];
  if (server.transport === "stdio") {
    if (server.command) lines.push(`command = ${tomlString(server.command)}`);
    lines.push(`args = [${server.args.map(tomlString).join(", ")}]`);
    if (server.cwd) lines.push(`cwd = ${tomlString(server.cwd)}`);
    if (server.env.length) lines.push(`env = { ${keyValueInline(server.env)} }`);
  } else {
    if (server.url) lines.push(`url = ${tomlString(server.url)}`);
    if (server.bearerTokenEnvVar) {
      lines.push(`bearer_token_env_var = ${tomlString(server.bearerTokenEnvVar)}`);
    }
    if (server.headers.length) lines.push(`headers = { ${keyValueInline(server.headers)} }`);
  }
  return [...lines, ""];
}

function keyValueInline(values: { key: string; value: string }[]) {
  return values
    .filter((entry) => entry.key.trim())
    .map((entry) => `${entry.key.trim()} = ${tomlString(entry.value)}`)
    .join(", ");
}

function tomlString(value: string) {
  return JSON.stringify(value);
}

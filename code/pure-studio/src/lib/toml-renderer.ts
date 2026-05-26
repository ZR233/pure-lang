import type { ProviderSettingsInput } from "../types";

export function renderPreviewToml(input: ProviderSettingsInput) {
  return [
    "schema_version = 1",
    "",
    "[runtime]",
    'active_skills = ["rust", "git", "doc"]',
    'active_mcp_servers = ["github", "filesystem"]',
    "",
    ...input.roles.flatMap((role) => [
      `[roles.${role.key}]`,
      `provider = "${role.provider}"`,
      `model = "${role.model}"`,
      `effort = "${role.effort}"`,
      "",
    ]),
    ...input.providers.flatMap((provider) => [
      `[providers.${provider.id}]`,
      `name = "${provider.name}"`,
      `base_url = "${provider.baseUrl}"`,
      `bearer_token = "${provider.bearerToken}"`,
      `default_model = "${provider.defaultModel}"`,
      `wire_api = "${provider.wireApi}"`,
      "",
    ]),
  ].join("\n");
}

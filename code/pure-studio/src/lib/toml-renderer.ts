import type { ProviderSettingsInput } from "../types";

export function renderPreviewToml(input: ProviderSettingsInput) {
  return [
    "schema_version = 3",
    "",
    "[runtime]",
    'permission_mode = "request-approval"',
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
      `provider_kind = "${provider.providerKind}"`,
      `name = "${provider.name}"`,
      `base_url = "${provider.baseUrl}"`,
      `bearer_token = "${provider.bearerToken}"`,
      `default_model = "${provider.defaultModel}"`,
      "",
    ]),
  ].join("\n");
}

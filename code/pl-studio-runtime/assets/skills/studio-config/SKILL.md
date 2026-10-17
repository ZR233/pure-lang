---
name: studio-config
description: Use when asked where anywork stores its configuration or how to configure providers, model routes, permissions, skills, MCP, LSP, UI, or web search. Covers config file locations, TOML schema, credential handling, and safe manual editing.
metadata:
  category: guides
---

# anywork Configuration

Use this skill when the user asks where anywork keeps its settings or wants to configure something outside the Settings page.

## Where Configuration Lives

anywork reads a single user config file:

```text
~/.anywork/config.toml          Windows: %USERPROFILE%\.anywork\config.toml
```

The home directory can be overridden, in resolution order:

1. `--studio-home <absolute path>` launch argument
2. `ANYWORK_HOME` environment variable (absolute, non-empty)
3. default `<user home>/.anywork`

Product metadata lives in `<home>/studio/v2/studio.sqlite`; global directories and product settings live in `<home>/v2/`. Call records use `<home>/v2/calls/calls.sqlite` and rolling logs under `logs/`. Each Thread keeps its history and current checkpoint in `<home>/v2/sessions/<thread-key>/history.sqlite`; `<thread-key>` is derived from the Thread identity. Old-version directories remain isolated. Never edit these files by hand.

## Format Rules

- TOML with snake_case keys; the current runtime accepts `schema_version = 21`. Changing the version number alone does not migrate a configuration.
- A missing file means in-memory defaults shown in Settings; nothing is written until you save.
- Known version upgrades preserve user settings and credential associations through migration. Startup migrates schema 18 to 19 by removing only `planner` from `disabled_system_agents`, then migrates 19 to 20 by copying the old `planner` route to `mode.simple` and `mode.task` before deleting it from child routes. Migration runs before recovery; a reset is never a substitute for a supported migration.
- Unrecoverable global data corruption, unsupported versions and invalid references trigger one complete startup backup of `config.toml`, `agents/`, `v2/` and `studio/v2/` under `<home>/startup-backups/<unique-id>/original/`, followed by initialization with current defaults. The durable `startup-recovery.toml` journal resumes an interrupted recovery. The UI reports the backup location. Permission, capacity, file-lock, credential-service and internal failures stop initialization without resetting. System credentials, old-version directories, remote connection configuration and physical project/worktree directories are preserved. Individual session read failures do not reset global data.
- Explicit reload while Studio is running remains strict: it reports the invalid file instead of replacing it.
- Saving from Settings writes atomically. External file edits are not picked up automatically; use explicit reload after editing a valid current-schema file.
- Provider save replies confirm the credential/configuration transaction, not MCP readiness or usage API completion. All presets use the same save path; MCP follows canonical configuration in the background and reports its own status.

## Common Sections

- `[models.providers.<id>]` — provider endpoint, preset, credential reference, model catalog.
- `[mode_model_routes."<mode-id>"]` — new/root Thread default route per full Mode ID. `mode.simple` and `mode.task` are required; custom Modes inherit `mode.simple` until explicitly saved.
- `[models.routes.<role>]` — child model route per role: `explorer`, `executor`, `worktree_executor`, `reviewer`. All four must resolve. Root Threads keep their route in the Thread state checkpoint instead of a global `planner` route.
- `[runtime]` — `permission_mode` (`request-approval` | `auto-review` | `full-access`), tool capabilities, active skills and MCP servers.
- `[skills]` — enable/disable, auto-learn, project/user/external skill directories, disabled skills. The default writable project directory is `.agents/skills`; explicit `project_dir` values remain unchanged. In addition to configured `user_dir`, 糊来帮 always discovers the read-only user compatibility directory at `$HOME/.agents/skills` on Linux and `%USERPROFILE%\.agents\skills` on Windows.
- `[mcp]` — custom servers under `[mcp.servers.<id>]` plus builtin server states.
- `[lsp.servers.<id>]` — command-based LSP servers outside the bundled catalog.
- `[ui]` — `follow_active_turn`, `compact_timeline`. Studio always uses its fixed light theme. The obsolete `follow_system_theme` key is ignored when reading current-schema files and omitted on the next normal save; no configuration reset is needed.
- `[web_search]` — OpenAI standalone `/alpha/search` configuration: mode, context size, allowed domains, location.
- `[deepseek_web_search]` — Two independent switches for the DeepSeek Search & Fetch tools: `enabled` (web search) and `fetch_enabled` (page fetch), both defaulting to `true`. The section, a missing key, and a missing section all mean enabled. It defines no result count, content toggle, OpenAI-specific mode, domain, location, or context option; those request details are fixed deployment constants in code.
- `[instructions]` — base override, developer/user instructions, project doc limits.

A few sections are omitted when left at defaults (`runtime`, `instructions`, `lsp`, `ui`); a default Studio save still writes `[skills]` with `project_dir = ".agents/skills"` and `user_dir = "~/.anywork/skills"`, `[web_search]` with `mode = "cached"`, and `[deepseek_web_search]` with `enabled = true` and `fetch_enabled = true`, so do not delete them assuming they are unused.

Web search is not a single selection. Both services are resolved independently of the session, route, or current model: OpenAI standalone search uses the provider's `gpt-6-sol` when its catalog has it (otherwise the provider's first valid model), DeepSeek search calls the official `POST {root}/v1/tools/search` and `POST {root}/v1/tools/fetch` endpoints without resolving any model, and providers are ordered by stable ID. Provider availability depends only on capability plus a usable key; whether the current model supports function calling only decides whether that thread registers the additive tools. At runtime the OpenAI `web_search`, DeepSeek `deepseek_web_search` (`queries`, 1–4 merged queries) and `deepseek_web_fetch` (`url`), and any generic MCP tools (including Zhipu, discovered and registered through generic MCP when a valid key is available) coexist as additive tools; there is no `selected` backend and no exclusive route that hides the other tools. Only the two DeepSeek tool descriptions mention per-request billing and the fallback condition, and searches are neither ordered nor automatically fallen back across providers. `enabled` and `fetch_enabled` gate the tools independently, and each switch only affects its own tool. A DeepSeek provider instance that overrides the preset's canonical base URL stops inheriting the DeepSeek standalone capability unless it explicitly declares the `deep_seek_tools_api` dialect; the pre-21 token `deep_seek_anthropic_messages` is rewritten by the configuration migration and is rejected if written by hand. This revocation is DeepSeek-only and does not change the existing OpenAI `/alpha/search` standalone inheritance for non-canonical OpenAI endpoints.

## Model compaction thresholds

Each provider catalog may store `auto_compact_overrides` keyed by exact model slug. A missing key means no user override: use the model's explicit `auto_compact_token_limit` preset, otherwise floor 90% of its resolved context capacity (`context_window`, falling back to `max_context_window`). Unknown capacity disables automatic compaction; never substitute zero or a guessed window. User overrides must be positive and the effective value cannot exceed the 90% safety limit. Restore default by removing only the corresponding override key; do not copy computed defaults into the configuration.

Edit a threshold directly in the provider detail card's model list: valid input saves automatically, and restore default clears the override. Other provider connection, credential and model-declaration edits still use their editor's Save/Cancel draft. A successful configuration reply does not change an in-flight model request; new bindings use it at their next safe refresh boundary.

## Credentials

Tokens never live in `config.toml`. Saving from Settings clears any inline token and stores it in the system credential store (service `anywork`, account `provider:<id>`). To use an environment variable instead, set `bearer_token_env`; when both exist, the stored credential takes precedence.

The four built-in Zhipu MCP servers only borrow a usable key from `preset = "zhipu-coding-plan"`. With multiple instances, the first usable key in stable provider-ID order wins. Ordinary `zhipu` presets and matching endpoint hosts are not credential fallbacks. A missing/blank key does not connect, discover or register callable tools; an explicit builtin disable remains disabled. They use the same generic MCP runtime and published-generation leases as user MCP servers, with no provider-specific Thread.

## Model catalog sources

A provider catalog either bundles a built-in model list or declares only user models. `additional_models` entries are user-authored and the only entries a user may add or delete; a `Bundled` catalog without an observation still resolves to its built-in list.

Online discovery is declared per catalog, never per adapter: a catalog that supports it records the discovery transport and any excluded non-chat slugs. Once during startup the runtime queries every instance whose catalog declares discovery, one independent task per instance with its own query identity, timeout and cache file. A successful response replaces that instance's observed list as a whole, and a valid empty list is an accepted replacement. A failed query only records that instance's typed error and falls back to its built-in list; other instances and the first screen are unaffected, and no global revision or reset is triggered.

The effective model list is the observed online list union user models when an observation exists, otherwise the built-in list union user models; a user model with the same slug wins and hides the other entry. Snapshots mark every entry's origin (`bundled`, `online`, `manual`) so the UI groups them instead of inferring from slug differences.

There is no manual or periodic refresh: the directory is synchronized once per launch, its status is read-only, and restarting the app is the only way to resynchronize. Deleting a user model requires an expected settings revision, is rejected for non-user entries, and also removes that slug's connection and compaction overrides. Model prices are inherited from the built-in catalog by exact, case-sensitive slug only; a slug without a price stays unpriced rather than zero, and a catalog change never recomputes recorded costs.

The Providers tab reads the catalog snapshot produced by that startup synchronization; it does not poll instances or retrigger queries when the tab becomes visible. The read-only `usageQueryRevision` is a non-secret, runtime-owned query identity, not a TOML setting or secret fingerprint.

## Minimal Working Example

Every provider needs `name`, `base_url`, and a `catalog` section; every role needs a route. `effort` is optional and must match the model's supported effort candidates.

```toml
schema_version = 21

[deepseek_web_search]
enabled = true
fetch_enabled = true

[models.providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
bearer_token_env = "DEEPSEEK_API_KEY"

[models.providers.deepseek.catalog]
source = "bundled"
catalog = "deepseek"

[models.routes.explorer]
provider = "deepseek"
model = "deepseek-flash"
effort = "high"

[mode_model_routes."mode.simple"]
provider = "deepseek"
model = "deepseek-flash"
effort = "high"

[mode_model_routes."mode.task"]
provider = "deepseek"
model = "deepseek-flash"
effort = "high"

[models.routes.executor]
provider = "deepseek"
model = "deepseek-flash"
effort = "high"

[models.routes.worktree_executor]
provider = "deepseek"
model = "deepseek-flash"
effort = "high"

[models.routes.reviewer]
provider = "deepseek"
model = "deepseek-flash"
effort = "high"
```

## Safe Editing Workflow

1. Prefer the Settings page; it validates and persists correctly.
2. For manual edits, copy `config.toml` aside first.
3. Keep `schema_version`, both built-in Mode routes, and all four child role routes valid.
4. Reference tokens via `bearer_token_env`; never paste raw tokens.
5. Add providers from Settings or a preset (`deepseek`, `openai`, `zhipu`, ...) rather than hand-writing catalog metadata.
6. After external edits, use explicit reload and confirm the canonical settings reflect the change. If validation fails, preserve the original settings and report the error; do not restart as a repair step.

---
name: studio-config
description: Use when asked where Pure Studio stores its configuration or how to configure providers, model routes, permissions, skills, MCP, LSP, UI, or web search. Covers config file locations, TOML schema, credential handling, and safe manual editing.
metadata:
  category: guides
---

# Pure Studio Configuration

Use this skill when the user asks where Pure Studio keeps its settings or wants to configure something outside the Settings page.

## Where Configuration Lives

Pure Studio reads a single user config file:

```text
~/.pure/config.toml          Windows: %USERPROFILE%\.pure\config.toml
```

The home directory can be overridden, in resolution order:

1. `--studio-home <absolute path>` launch argument
2. `PURE_STUDIO_HOME` environment variable (absolute, non-empty)
3. default `<user home>/.pure`

Product state (projects, threads, tasks) lives in `<home>/studio/studio.sqlite`; never edit it by hand.

## Format Rules

- TOML with snake_case keys; only `schema_version = 17` is accepted after startup migration.
- A missing file means in-memory defaults shown in Settings; nothing is written until you save.
- At startup, schemas 15 and 16 are typed-migrated to 17 after a byte-for-byte `config.toml.migrated.<timestamp>.bak` backup. Other unparsable, invalid, inline-credential, or unknown-schema files use the rejected-backup recovery path. A failed backup or replacement keeps startup closed and preserves the original file.
- Explicit reload while Studio is running remains strict: it reports the invalid file instead of replacing it.
- Saving from Settings writes atomically. External file edits are not picked up automatically; restart Studio after editing config.toml by hand.

## Common Sections

- `[models.providers.<id>]` — provider endpoint, preset, credential reference, model catalog.
- `[models.routes.<role>]` — model route per role: `explorer`, `planner`, `executor`, `worktree_executor`, `reviewer`. All five must resolve.
- `[runtime]` — `permission_mode` (`request-approval` | `auto-review` | `full-access`), tool capabilities, active skills and MCP servers.
- `[skills]` — enable/disable, auto-learn, project/user/external skill directories, disabled skills. In addition to configured `user_dir`, Pure always discovers the read-only user compatibility directory at `$HOME/.agents/skills` on Linux and `%USERPROFILE%\.agents\skills` on Windows.
- `[mcp]` — custom servers under `[mcp.servers.<id>]` plus builtin server states.
- `[lsp.servers.<id>]` — command-based LSP servers outside the bundled catalog.
- `[ui]` — `follow_system_theme`, `follow_active_turn`, `compact_timeline`.
- `[web_search]` — search mode, context size, allowed domains, location.
- `[deepseek_web_search]` — DeepSeek native hosted search toggle. The section and `enabled` field both default to `true`; it deliberately has no OpenAI-specific mode, domain, location, or context options.
- `[instructions]` — base override, developer/user instructions, project doc limits.

A few sections are omitted when left at defaults (`runtime`, `instructions`, `lsp`, `ui`); a default Studio save still writes `[skills]` with `user_dir = "~/.pure/skills"`, `[web_search]` with `mode = "cached"`, and `[deepseek_web_search]` with `enabled = true`, so do not delete them assuming they are unused.

When the current route is an eligible credentialed DeepSeek Responses model, DeepSeek native search takes priority. Disabling it falls back to the separately configured OpenAI search when available. Provider instances that override a preset's canonical base URL do not inherit hosted search capability unless they explicitly declare the matching hosted dialect.

## Credentials

Tokens never live in `config.toml`. Saving from Settings clears any inline token and stores it in the system credential store (service `pure-studio`, account `provider:<id>`). To use an environment variable instead, set `bearer_token_env`; when both exist, the stored credential takes precedence.

## Minimal Working Example

Every provider needs `name`, `base_url`, and a `catalog` section; every role needs a route. `effort` is optional and must match the model's supported effort candidates.

```toml
schema_version = 17

[deepseek_web_search]
enabled = true

[models.providers.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
bearer_token_env = "DEEPSEEK_API_KEY"

[models.providers.deepseek.catalog]
source = "bundled"
catalog = "deepseek"

[models.routes.explorer]
provider = "deepseek"
model = "deepseek-v4-flash"

[models.routes.planner]
provider = "deepseek"
model = "deepseek-v4-flash"
effort = "high"

[models.routes.executor]
provider = "deepseek"
model = "deepseek-v4-flash"

[models.routes.worktree_executor]
provider = "deepseek"
model = "deepseek-v4-flash"

[models.routes.reviewer]
provider = "deepseek"
model = "deepseek-v4-flash"
```

## Safe Editing Workflow

1. Prefer the Settings page; it validates and persists correctly.
2. For manual edits, copy `config.toml` aside first.
3. Keep `schema_version` and all five role routes valid.
4. Reference tokens via `bearer_token_env`; never paste raw tokens.
5. Add providers from Settings or a preset (`deepseek`, `openai`, `zhipu`, ...) rather than hand-writing catalog metadata.
6. After external edits, restart Studio and confirm the change took effect.

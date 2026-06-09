import {
  Globe2,
  Pencil,
  Plus,
  Power,
  Save,
  Search,
  Server,
  Terminal,
  Trash2,
  X,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { KeyValuePair, McpServerInput, McpServerRecord, McpTransport } from "../types";

type McpSettingsProps = {
  servers: McpServerRecord[];
  onSaveMcpSettings: (servers: McpServerInput[]) => Promise<boolean>;
};

type DraftServer = McpServerInput;

const transports: McpTransport[] = ["stdio", "streamableHttp"];

export function McpSettings({ servers, onSaveMcpSettings }: McpSettingsProps) {
  const { t } = useTranslation();
  const [drafts, setDrafts] = useState<DraftServer[]>(() => servers.map(serverInput));
  const [editingId, setEditingId] = useState<string | null>(servers[0]?.id ?? null);
  const [search, setSearch] = useState("");
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setDrafts(servers.map(serverInput));
    setEditingId((current) =>
      current && servers.some((server) => server.id === current) ? current : servers[0]?.id ?? null,
    );
  }, [servers]);

  const filteredDrafts = useMemo(() => {
    const query = search.trim().toLowerCase();
    if (!query) return drafts;
    return drafts.filter((server) => searchableServerText(server).includes(query));
  }, [drafts, search]);

  const editingServer = drafts.find((server) => server.id === editingId) ?? null;

  async function save(nextDrafts: DraftServer[]) {
    setSaving(true);
    try {
      const saved = await onSaveMcpSettings(nextDrafts.map(normalizeServerInput));
      if (saved) {
        setDrafts(nextDrafts);
      }
      return saved;
    } finally {
      setSaving(false);
    }
  }

  function updateEditing(updater: (server: DraftServer) => DraftServer) {
    if (!editingId) return;
    setDrafts((current) =>
      current.map((server) => (server.id === editingId ? updater(server) : server)),
    );
  }

  function addServer() {
    const id = uniqueServerId(drafts);
    const nextServer: DraftServer = {
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
    };
    setDrafts((current) => [...current, nextServer]);
    setEditingId(id);
  }

  async function toggleServer(server: DraftServer) {
    const nextDrafts = drafts.map((draft) =>
      draft.id === server.id ? { ...draft, enabled: !draft.enabled } : draft,
    );
    await save(nextDrafts);
  }

  async function deleteServer(server: DraftServer) {
    const nextDrafts = drafts.filter((draft) => draft.id !== server.id);
    if (editingId === server.id) {
      setEditingId(nextDrafts[0]?.id ?? null);
    }
    await save(nextDrafts);
  }

  async function saveEditing() {
    await save(drafts);
  }

  return (
    <section className="mcp-settings">
      <div className="skills-console-head">
        <div>
          <h2>{t("settings.mcp.title")}</h2>
          <p>{t("settings.mcp.description")}</p>
        </div>
        <div className="skills-console-tools">
          <label className="search-box">
            <Search size={16} />
            <input
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t("settings.mcp.searchPlaceholder")}
            />
          </label>
          <button type="button" onClick={addServer}>
            <Plus size={16} />
            {t("settings.mcp.addServer")}
          </button>
        </div>
      </div>

      <div className="mcp-layout">
        <div className="mcp-list">
          {filteredDrafts.length === 0 ? (
            <div className="skills-empty-state">
              <Server size={28} />
              <strong>{search.trim() ? t("settings.mcp.noMatches") : t("settings.mcp.empty")}</strong>
            </div>
          ) : (
            filteredDrafts.map((server) => {
              const active = server.id === editingId;
              const TransportIcon = server.transport === "stdio" ? Terminal : Globe2;
              return (
                <article
                  className={`mcp-row${active ? " active" : ""}`}
                  key={server.id}
                >
                  <button
                    type="button"
                    className="mcp-row-main"
                    onClick={() => setEditingId(server.id)}
                  >
                    <span className="mcp-row-icon">
                      <TransportIcon size={18} />
                    </span>
                    <span>
                      <strong>{server.id}</strong>
                      <small>{endpointSummary(server) || t("settings.mcp.noEndpoint")}</small>
                    </span>
                  </button>
                  <span className={`mcp-status${server.enabled ? " enabled" : ""}`}>
                    {server.enabled ? t("settings.mcp.enabled") : t("settings.mcp.disabled")}
                  </span>
                  <div className="mcp-row-actions">
                    <button
                      type="button"
                      title={t("settings.mcp.toggle")}
                      onClick={() => void toggleServer(server)}
                      disabled={saving}
                    >
                      <Power size={16} />
                    </button>
                    <button
                      type="button"
                      title={t("actions.edit")}
                      onClick={() => setEditingId(server.id)}
                    >
                      <Pencil size={16} />
                    </button>
                    <button
                      type="button"
                      title={t("actions.delete")}
                      onClick={() => void deleteServer(server)}
                      disabled={saving}
                    >
                      <Trash2 size={16} />
                    </button>
                  </div>
                </article>
              );
            })
          )}
        </div>

        {editingServer ? (
          <form
            className="mcp-editor"
            onSubmit={(event) => {
              event.preventDefault();
              void saveEditing();
            }}
          >
            <div className="mcp-editor-head">
              <div>
                <h3>{editingServer.id || t("settings.mcp.newServer")}</h3>
                <p>{t("settings.mcp.editorSubtitle")}</p>
              </div>
              <button type="submit" disabled={saving}>
                <Save size={16} />
                {saving ? t("actions.saving") : t("actions.save")}
              </button>
            </div>

            <label className="settings-field">
              <span>{t("settings.mcp.serverId")}</span>
              <input
                value={editingServer.id}
                onChange={(event) => {
                  const previousId = editingServer.id;
                  const nextId = event.target.value;
                  setDrafts((current) =>
                    current.map((server) =>
                      server.id === previousId ? { ...server, id: nextId } : server,
                    ),
                  );
                  setEditingId(nextId);
                }}
              />
            </label>

            <div className="mcp-segmented" role="radiogroup" aria-label={t("settings.mcp.transport")}>
              {transports.map((transport) => {
                const active = editingServer.transport === transport;
                const Icon = transport === "stdio" ? Terminal : Globe2;
                return (
                  <button
                    type="button"
                    key={transport}
                    className={active ? "active" : ""}
                    role="radio"
                    aria-checked={active}
                    onClick={() => updateEditing((server) => ({ ...server, transport }))}
                  >
                    <Icon size={16} />
                    {t(`settings.mcp.transportOptions.${transport}`)}
                  </button>
                );
              })}
            </div>

            {editingServer.transport === "stdio" ? (
              <StdioFields server={editingServer} updateServer={updateEditing} />
            ) : (
              <HttpFields server={editingServer} updateServer={updateEditing} />
            )}
          </form>
        ) : null}
      </div>
    </section>
  );
}

function StdioFields({
  server,
  updateServer,
}: {
  server: DraftServer;
  updateServer: (updater: (server: DraftServer) => DraftServer) => void;
}) {
  const { t } = useTranslation();
  return (
    <>
      <label className="settings-field">
        <span>{t("settings.mcp.command")}</span>
        <input
          value={server.command ?? ""}
          onChange={(event) => updateServer((current) => ({ ...current, command: event.target.value }))}
          placeholder="npx"
        />
      </label>
      <StringListEditor
        label={t("settings.mcp.args")}
        values={server.args}
        placeholder="-y"
        onChange={(args) => updateServer((current) => ({ ...current, args }))}
      />
      <label className="settings-field">
        <span>{t("settings.mcp.cwd")}</span>
        <input
          value={server.cwd ?? ""}
          onChange={(event) => updateServer((current) => ({ ...current, cwd: event.target.value }))}
          placeholder="D:/workspace"
        />
      </label>
      <KeyValueEditor
        label={t("settings.mcp.env")}
        values={server.env}
        onChange={(env) => updateServer((current) => ({ ...current, env }))}
      />
    </>
  );
}

function HttpFields({
  server,
  updateServer,
}: {
  server: DraftServer;
  updateServer: (updater: (server: DraftServer) => DraftServer) => void;
}) {
  const { t } = useTranslation();
  return (
    <>
      <label className="settings-field">
        <span>{t("settings.mcp.url")}</span>
        <input
          value={server.url ?? ""}
          onChange={(event) => updateServer((current) => ({ ...current, url: event.target.value }))}
          placeholder="https://example.com/mcp"
        />
      </label>
      <label className="settings-field">
        <span>{t("settings.mcp.bearerTokenEnvVar")}</span>
        <input
          value={server.bearerTokenEnvVar ?? ""}
          onChange={(event) =>
            updateServer((current) => ({ ...current, bearerTokenEnvVar: event.target.value }))
          }
          placeholder="MCP_API_TOKEN"
        />
      </label>
      <KeyValueEditor
        label={t("settings.mcp.headers")}
        values={server.headers}
        onChange={(headers) => updateServer((current) => ({ ...current, headers }))}
      />
    </>
  );
}

function StringListEditor({
  label,
  values,
  placeholder,
  onChange,
}: {
  label: string;
  values: string[];
  placeholder: string;
  onChange: (values: string[]) => void;
}) {
  const { t } = useTranslation();
  const rows = values.length ? values : [""];
  return (
    <div className="mcp-field-group">
      <span>{label}</span>
      {rows.map((value, index) => (
        <div className="mcp-inline-row" key={`${index}-${value}`}>
          <input
            value={value}
            placeholder={placeholder}
            onChange={(event) => {
              const next = [...rows];
              next[index] = event.target.value;
              onChange(next);
            }}
          />
          <button
            type="button"
            title={t("actions.delete")}
            onClick={() => onChange(rows.filter((_, rowIndex) => rowIndex !== index))}
          >
            <X size={16} />
          </button>
        </div>
      ))}
      <button type="button" className="mcp-add-row" onClick={() => onChange([...values, ""])}>
        <Plus size={16} />
        {t("settings.mcp.addRow")}
      </button>
    </div>
  );
}

function KeyValueEditor({
  label,
  values,
  onChange,
}: {
  label: string;
  values: KeyValuePair[];
  onChange: (values: KeyValuePair[]) => void;
}) {
  const { t } = useTranslation();
  const rows = values.length ? values : [{ key: "", value: "" }];
  return (
    <div className="mcp-field-group">
      <span>{label}</span>
      {rows.map((entry, index) => (
        <div className="mcp-key-value-row" key={`${index}-${entry.key}`}>
          <input
            value={entry.key}
            placeholder={t("settings.mcp.key")}
            onChange={(event) => {
              const next = rows.map((row) => ({ ...row }));
              next[index].key = event.target.value;
              onChange(next);
            }}
          />
          <input
            value={entry.value}
            placeholder={t("settings.mcp.value")}
            onChange={(event) => {
              const next = rows.map((row) => ({ ...row }));
              next[index].value = event.target.value;
              onChange(next);
            }}
          />
          <button
            type="button"
            title={t("actions.delete")}
            onClick={() => onChange(rows.filter((_, rowIndex) => rowIndex !== index))}
          >
            <X size={16} />
          </button>
        </div>
      ))}
      <button type="button" className="mcp-add-row" onClick={() => onChange([...values, { key: "", value: "" }])}>
        <Plus size={16} />
        {t("settings.mcp.addRow")}
      </button>
    </div>
  );
}

function serverInput(server: McpServerRecord): DraftServer {
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
  };
}

function normalizeServerInput(server: DraftServer): McpServerInput {
  return {
    ...server,
    id: server.id.trim(),
    command: optionalText(server.command),
    args: server.args.map((arg) => arg.trim()).filter(Boolean),
    env: server.env
      .map((entry) => ({ key: entry.key.trim(), value: entry.value }))
      .filter((entry) => entry.key || entry.value.trim()),
    cwd: optionalText(server.cwd),
    url: optionalText(server.url),
    bearerTokenEnvVar: optionalText(server.bearerTokenEnvVar),
    headers: server.headers
      .map((entry) => ({ key: entry.key.trim(), value: entry.value }))
      .filter((entry) => entry.key || entry.value.trim()),
  };
}

function optionalText(value: string | null | undefined) {
  const trimmed = value?.trim() ?? "";
  return trimmed ? trimmed : null;
}

function searchableServerText(server: DraftServer) {
  return [
    server.id,
    server.transport,
    server.command ?? "",
    server.url ?? "",
    server.bearerTokenEnvVar ?? "",
    ...server.args,
    ...server.env.flatMap((entry) => [entry.key, entry.value]),
    ...server.headers.flatMap((entry) => [entry.key, entry.value]),
  ]
    .join(" ")
    .toLowerCase();
}

function endpointSummary(server: DraftServer) {
  return server.transport === "stdio" ? server.command ?? "" : server.url ?? "";
}

function uniqueServerId(servers: DraftServer[]) {
  const existing = new Set(servers.map((server) => server.id));
  let index = 1;
  while (existing.has(`server-${index}`)) {
    index += 1;
  }
  return `server-${index}`;
}

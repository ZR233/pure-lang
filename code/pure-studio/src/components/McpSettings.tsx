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
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import type {
  KeyValuePair,
  McpAvailabilityKind,
  McpServerInput,
  McpServerRecord,
  McpTransport,
} from "../types";

type McpSettingsProps = {
  servers: McpServerRecord[];
  onSaveMcpSettings: (servers: McpServerInput[]) => Promise<boolean>;
};

type DraftServer = McpServerInput & Pick<
  McpServerRecord,
  "availabilityKind" | "availabilityMessage" | "lastCheckedAt" | "toolCount"
>;

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
      availabilityKind: "checking",
      availabilityMessage: null,
      lastCheckedAt: null,
      toolCount: null,
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
    if (isLockedServer(server)) return;
    const nextDrafts = drafts.filter((draft) => draft.id !== server.id);
    if (editingId === server.id) {
      setEditingId(nextDrafts[0]?.id ?? null);
    }
    await save(nextDrafts);
  }

  async function saveEditing() {
    if (editingServer && isLockedServer(editingServer)) return;
    await save(drafts);
  }

  return (
    <section className="space-y-4">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h2 className="text-lg font-semibold text-foreground">{t("settings.mcp.title")}</h2>
          <p className="text-sm text-muted-foreground">{t("settings.mcp.description")}</p>
        </div>
        <div className="flex items-center gap-2">
          <div className="relative">
            <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
            <Input
              className="pl-9"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t("settings.mcp.searchPlaceholder")}
            />
          </div>
          <Button variant="outline" size="sm" type="button" onClick={addServer}>
            <Plus size={16} className="mr-1" />
            {t("settings.mcp.addServer")}
          </Button>
        </div>
      </div>

      <div className="grid grid-cols-[1fr_1fr] gap-4">
        <div className="space-y-2">
          {filteredDrafts.length === 0 ? (
            <div className="flex flex-col items-center gap-2 py-16 text-center">
              <Server size={28} className="text-muted-foreground" />
              <strong className="text-sm text-muted-foreground">
                {search.trim() ? t("settings.mcp.noMatches") : t("settings.mcp.empty")}
              </strong>
            </div>
          ) : (
            filteredDrafts.map((server) => {
              const active = server.id === editingId;
              const TransportIcon = server.transport === "stdio" ? Terminal : Globe2;
              const locked = isLockedServer(server);
              const statusKind = serverStatusKind(server);
              const availabilityKind = serverAvailabilityKind(server);
              return (
                <Card
                  key={server.id}
                  className={`p-3 transition-colors cursor-pointer ${
                    active ? "border-primary/50" : "hover:border-primary/30"
                  }`}
                  onClick={() => setEditingId(server.id)}
                >
                  <div className="flex items-center justify-between gap-2">
                    <div className="flex items-center gap-2 min-w-0">
                      <TransportIcon size={18} className="shrink-0 text-muted-foreground" />
                      <div className="min-w-0">
                        <strong className="text-sm text-foreground block truncate">{server.id}</strong>
                        <small className="text-xs text-muted-foreground block truncate">
                          {endpointSummary(server) || t("settings.mcp.noEndpoint")}
                        </small>
                      </div>
                    </div>
                  </div>
                  <div className="flex items-center gap-2 mt-2">
                    <Badge variant={statusKind === "disabled" ? "secondary" : "default"}>
                      {t(`settings.mcp.status.${statusKind}`)}
                    </Badge>
                    <Badge variant="outline">
                      {t(`settings.mcp.availability.${availabilityKind}`)}
                    </Badge>
                    {server.sourceKind === "builtIn" ? (
                      <Badge variant="secondary">{t("settings.mcp.builtInSource")}</Badge>
                    ) : null}
                  </div>
                  <div className="flex items-center gap-1 mt-2">
                    <Button
                      variant="ghost"
                      size="icon"
                      title={t("settings.mcp.toggle")}
                      onClick={(e) => {
                        e.stopPropagation();
                        void toggleServer(server);
                      }}
                      disabled={saving}
                    >
                      <Power size={16} />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      title={t("actions.edit")}
                      onClick={(e) => {
                        e.stopPropagation();
                        setEditingId(server.id);
                      }}
                    >
                      <Pencil size={16} />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      title={t("actions.delete")}
                      onClick={(e) => {
                        e.stopPropagation();
                        void deleteServer(server);
                      }}
                      disabled={saving || locked}
                    >
                      <Trash2 size={16} />
                    </Button>
                  </div>
                </Card>
              );
            })
          )}
        </div>

        {editingServer ? (
          <form
            className="space-y-4"
            onSubmit={(event) => {
              event.preventDefault();
              void saveEditing();
            }}
          >
            <div className="flex items-start justify-between gap-4">
              <div>
                <h3 className="text-base font-semibold text-foreground">
                  {editingServer.id || t("settings.mcp.newServer")}
                </h3>
                <p className="text-sm text-muted-foreground">{t("settings.mcp.editorSubtitle")}</p>
              </div>
              <Button type="submit" disabled={saving || isLockedServer(editingServer)}>
                <Save size={16} className="mr-1" />
                {saving ? t("actions.saving") : t("actions.save")}
              </Button>
            </div>

            {editingServer.sourceKind === "builtIn" ? (
              <div className="rounded-lg border border-border bg-muted/50 p-3 space-y-1">
                <strong className="text-xs">{t("settings.mcp.builtInSource")}</strong>
                <span className="block text-xs text-muted-foreground">
                  {editingServer.sourceDetail ?? t("settings.mcp.builtInDetail")}
                </span>
                {editingServer.statusMessage ? (
                  <small className="block text-xs text-muted-foreground">{editingServer.statusMessage}</small>
                ) : null}
              </div>
            ) : null}

            <div className="rounded-lg border border-border bg-muted/30 p-3 space-y-1">
              <strong className="text-xs">{t("settings.mcp.availabilityTitle")}</strong>
              <span className="block text-xs text-muted-foreground">
                {t(`settings.mcp.availability.${serverAvailabilityKind(editingServer)}`)}
                {editingServer.toolCount != null
                  ? ` · ${t("settings.mcp.toolCount", { count: editingServer.toolCount })}`
                  : ""}
                {editingServer.lastCheckedAt
                  ? ` · ${t("settings.mcp.lastCheckedAt", {
                      time: formatCheckedAt(editingServer.lastCheckedAt),
                    })}`
                  : ""}
              </span>
              {editingServer.availabilityMessage ? (
                <small className="block text-xs text-muted-foreground">{editingServer.availabilityMessage}</small>
              ) : null}
            </div>

            <div className="space-y-2">
              <Label>{t("settings.mcp.serverId")}</Label>
              <Input
                value={editingServer.id}
                disabled={isLockedServer(editingServer)}
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
            </div>

            <div className="flex rounded-lg border border-border p-1" role="radiogroup" aria-label={t("settings.mcp.transport")}>
              {transports.map((transport) => {
                const active = editingServer.transport === transport;
                const Icon = transport === "stdio" ? Terminal : Globe2;
                return (
                  <button
                    type="button"
                    key={transport}
                    className={`flex items-center gap-1.5 px-3 py-1.5 text-sm rounded-md transition-colors ${
                      active
                        ? "bg-primary text-primary-foreground"
                        : "text-muted-foreground hover:text-foreground"
                    }`}
                    role="radio"
                    aria-checked={active}
                    disabled={isLockedServer(editingServer)}
                    onClick={() => updateEditing((server) => ({ ...server, transport }))}
                  >
                    <Icon size={16} />
                    {t(`settings.mcp.transportOptions.${transport}`)}
                  </button>
                );
              })}
            </div>

            {editingServer.transport === "stdio" ? (
              <StdioFields
                server={editingServer}
                updateServer={updateEditing}
                locked={isLockedServer(editingServer)}
              />
            ) : (
              <HttpFields
                server={editingServer}
                updateServer={updateEditing}
                locked={isLockedServer(editingServer)}
              />
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
  locked,
}: {
  server: DraftServer;
  updateServer: (updater: (server: DraftServer) => DraftServer) => void;
  locked: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <Label>{t("settings.mcp.command")}</Label>
        <Input
          value={server.command ?? ""}
          disabled={locked}
          onChange={(event) => updateServer((current) => ({ ...current, command: event.target.value }))}
          placeholder="npx"
        />
      </div>
      <StringListEditor
        label={t("settings.mcp.args")}
        values={server.args}
        placeholder="-y"
        locked={locked}
        onChange={(args) => updateServer((current) => ({ ...current, args }))}
      />
      <div className="space-y-2">
        <Label>{t("settings.mcp.cwd")}</Label>
        <Input
          value={server.cwd ?? ""}
          disabled={locked}
          onChange={(event) => updateServer((current) => ({ ...current, cwd: event.target.value }))}
          placeholder="D:/workspace"
        />
      </div>
      <KeyValueEditor
        label={t("settings.mcp.env")}
        values={server.env}
        locked={locked}
        onChange={(env) => updateServer((current) => ({ ...current, env }))}
      />
    </div>
  );
}

function HttpFields({
  server,
  updateServer,
  locked,
}: {
  server: DraftServer;
  updateServer: (updater: (server: DraftServer) => DraftServer) => void;
  locked: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <Label>{t("settings.mcp.url")}</Label>
        <Input
          value={server.url ?? ""}
          disabled={locked}
          onChange={(event) => updateServer((current) => ({ ...current, url: event.target.value }))}
          placeholder="https://example.com/mcp"
        />
      </div>
      <div className="space-y-2">
        <Label>{t("settings.mcp.bearerTokenEnvVar")}</Label>
        <Input
          value={server.bearerTokenEnvVar ?? ""}
          disabled={locked}
          onChange={(event) =>
            updateServer((current) => ({ ...current, bearerTokenEnvVar: event.target.value }))
          }
          placeholder="MCP_API_TOKEN"
        />
      </div>
      <KeyValueEditor
        label={t("settings.mcp.headers")}
        values={server.headers}
        locked={locked}
        onChange={(headers) => updateServer((current) => ({ ...current, headers }))}
      />
    </div>
  );
}

function StringListEditor({
  label,
  values,
  placeholder,
  locked,
  onChange,
}: {
  label: string;
  values: string[];
  placeholder: string;
  locked: boolean;
  onChange: (values: string[]) => void;
}) {
  const { t } = useTranslation();
  const rows = values.length ? values : [""];
  return (
    <div className="space-y-2">
      <Label>{label}</Label>
      {rows.map((value, index) => (
        <div className="flex items-center gap-2" key={index}>
          <Input
            value={value}
            placeholder={placeholder}
            disabled={locked}
            onChange={(event) => {
              const next = [...rows];
              next[index] = event.target.value;
              onChange(next);
            }}
          />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            title={t("actions.delete")}
            disabled={locked}
            onClick={() => onChange(rows.filter((_, rowIndex) => rowIndex !== index))}
          >
            <X size={16} />
          </Button>
        </div>
      ))}
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={locked}
        onClick={() => onChange([...values, ""])}
      >
        <Plus size={16} className="mr-1" />
        {t("settings.mcp.addRow")}
      </Button>
    </div>
  );
}

function KeyValueEditor({
  label,
  values,
  locked,
  onChange,
}: {
  label: string;
  values: KeyValuePair[];
  locked: boolean;
  onChange: (values: KeyValuePair[]) => void;
}) {
  const { t } = useTranslation();
  const rows = values.length ? values : [{ key: "", value: "" }];
  return (
    <div className="space-y-2">
      <Label>{label}</Label>
      {rows.map((entry, index) => (
        <div className="flex items-center gap-2" key={index}>
          <Input
            value={entry.key}
            placeholder={t("settings.mcp.key")}
            disabled={locked}
            onChange={(event) => {
              const next = rows.map((row) => ({ ...row }));
              next[index].key = event.target.value;
              onChange(next);
            }}
          />
          <Input
            value={entry.value}
            placeholder={t("settings.mcp.value")}
            disabled={locked}
            onChange={(event) => {
              const next = rows.map((row) => ({ ...row }));
              next[index].value = event.target.value;
              onChange(next);
            }}
          />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            title={t("actions.delete")}
            disabled={locked}
            onClick={() => onChange(rows.filter((_, rowIndex) => rowIndex !== index))}
          >
            <X size={16} />
          </Button>
        </div>
      ))}
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={locked}
        onClick={() => onChange([...values, { key: "", value: "" }])}
      >
        <Plus size={16} className="mr-1" />
        {t("settings.mcp.addRow")}
      </Button>
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

function normalizeServerInput(server: DraftServer): McpServerInput {
  return {
    id: server.id.trim(),
    enabled: server.enabled,
    transport: server.transport,
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
    sourceKind: server.sourceKind,
    sourceLabel: server.sourceLabel,
    sourceDetail: server.sourceDetail,
    statusKind: server.statusKind,
    statusMessage: server.statusMessage,
    mutationPolicy: server.mutationPolicy,
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

function endpointSummary(server: DraftServer) {
  return server.transport === "stdio" ? server.command ?? "" : server.url ?? "";
}

function isLockedServer(server: DraftServer) {
  return server.sourceKind === "builtIn" || server.mutationPolicy === "lockedIdentity";
}

function serverStatusKind(server: DraftServer) {
  return server.statusKind ?? (server.enabled ? "enabled" : "disabled");
}

function serverAvailabilityKind(server: DraftServer): McpAvailabilityKind {
  return server.availabilityKind ?? (server.enabled ? "checking" : "disabled");
}

function formatCheckedAt(value: number) {
  return new Date(value * 1000).toLocaleString();
}

function uniqueServerId(servers: DraftServer[]) {
  const existing = new Set(servers.map((server) => server.id));
  let index = 1;
  while (existing.has(`server-${index}`)) {
    index += 1;
  }
  return `server-${index}`;
}

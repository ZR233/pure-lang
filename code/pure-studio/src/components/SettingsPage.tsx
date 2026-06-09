import { ArrowLeft } from "lucide-react";
import type { Dispatch, SetStateAction } from "react";
import { useTranslation } from "react-i18next";
import type { SettingsTab } from "../state/studio-state";
import type {
  ProviderRecord,
  ProviderSettingsSaveSnapshot,
  ProviderTemplateRecord,
  ProviderUsageRecord,
  McpServerInput,
  McpServerRecord,
  PermissionMode,
  RoleRecord,
} from "../types";
import { McpSettings } from "./McpSettings";
import { ProviderSettings } from "./ProviderSettings";
import { RoleSettings } from "./RoleSettings";
import { SecuritySettings } from "./SecuritySettings";
import { SkillsSettings } from "./SkillsSettings";

type SettingsPageProps = {
  activeSettingsTab: SettingsTab;
  providers: ProviderRecord[];
  mcpServers: McpServerRecord[];
  providerTemplates: ProviderTemplateRecord[];
  providerUsages: ProviderUsageRecord[];
  providerUsagesLoading: boolean;
  providerUsageError: string | null;
  roles: RoleRecord[];
  selectedProjectId: string | null;
  selectedProviderId: string | null;
  providerSearch: string;
  configExists: boolean;
  permissionMode: PermissionMode;
  setRoles: Dispatch<SetStateAction<RoleRecord[]>>;
  setProviderSearch: Dispatch<SetStateAction<string>>;
  onClose: () => void;
  onSetActiveTab: (tab: SettingsTab) => void;
  onSaveProviderSettings: (snapshot?: ProviderSettingsSaveSnapshot) => Promise<boolean>;
  onSavePermissionMode: (mode: PermissionMode) => Promise<void>;
  onSaveMcpSettings: (servers: McpServerInput[]) => Promise<boolean>;
  onRefreshProviderUsages: () => void;
};

const SETTINGS_TABS: SettingsTab[] = ["providers", "skills", "roles", "mcp", "security", "general"];

export function SettingsPage({
  activeSettingsTab,
  providers,
  mcpServers,
  providerTemplates,
  providerUsages,
  providerUsagesLoading,
  providerUsageError,
  roles,
  selectedProjectId,
  selectedProviderId,
  providerSearch,
  configExists,
  permissionMode,
  onClose,
  onSetActiveTab,
  onSaveProviderSettings,
  onSavePermissionMode,
  onSaveMcpSettings,
  onRefreshProviderUsages,
  setRoles,
  setProviderSearch,
}: SettingsPageProps) {
  const { t } = useTranslation();

  return (
    <section className="settings-page">
      <header className="settings-header">
        <button className="back-button" onClick={onClose}>
          <ArrowLeft size={18} />
        </button>
        <div>
          <h1>{t("settings.title")}</h1>
          <p>{configExists ? "~/.pure/config.toml" : t("settings.defaultConfigDraft")}</p>
        </div>
      </header>

      <nav className="settings-tabs">
        {SETTINGS_TABS.map((tab) => (
          <button
            key={tab}
            className={tab === activeSettingsTab ? "active" : ""}
            onClick={() => onSetActiveTab(tab)}
          >
            {t(`settings.tabs.${tab}`)}
          </button>
        ))}
      </nav>

      {activeSettingsTab === "providers" ? (
        <ProviderSettings
          providers={providers}
          templates={providerTemplates}
          providerUsages={providerUsages}
          providerUsagesLoading={providerUsagesLoading}
          providerUsageError={providerUsageError}
          selectedProviderId={selectedProviderId}
          providerSearch={providerSearch}
          setProviderSearch={setProviderSearch}
          onSaveProviderSettings={onSaveProviderSettings}
          onRefreshProviderUsages={onRefreshProviderUsages}
        />
      ) : activeSettingsTab === "roles" ? (
        <RoleSettings
          providers={providers}
          roles={roles}
          setRoles={setRoles}
          onSaveRoles={(nextRoles) => onSaveProviderSettings({ roles: nextRoles })}
        />
      ) : activeSettingsTab === "skills" ? (
        <SkillsSettings selectedProjectId={selectedProjectId} />
      ) : activeSettingsTab === "mcp" ? (
        <McpSettings servers={mcpServers} onSaveMcpSettings={onSaveMcpSettings} />
      ) : activeSettingsTab === "security" ? (
        <SecuritySettings
          permissionMode={permissionMode}
          onSavePermissionMode={onSavePermissionMode}
        />
      ) : (
        <div className="settings-placeholder">
          <h2>{t("settings.comingSoon")}</h2>
          <p>{t("settings.comingSoonDesc")}</p>
        </div>
      )}
    </section>
  );
}

import { ArrowLeft, RefreshCw, Save } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ProviderRecord, ProviderTemplateRecord, RoleRecord } from "../types";
import { ProviderSettings } from "./ProviderSettings";
import { RoleSettings, normalizeRolesForProviders } from "./RoleSettings";

type SettingsTab = "providers" | "models" | "roles" | "security" | "general";

type SettingsPageProps = {
  activeSettingsTab: SettingsTab;
  providers: ProviderRecord[];
  providerTemplates: ProviderTemplateRecord[];
  roles: RoleRecord[];
  selectedProviderId: string | null;
  providerSearch: string;
  configExists: boolean;
  configToml: string;
  setProviders: React.Dispatch<React.SetStateAction<ProviderRecord[]>>;
  setRoles: React.Dispatch<React.SetStateAction<RoleRecord[]>>;
  setSelectedProviderId: React.Dispatch<React.SetStateAction<string | null>>;
  setProviderSearch: React.Dispatch<React.SetStateAction<string>>;
  setConfigToml: React.Dispatch<React.SetStateAction<string>>;
  onClose: () => void;
  onSetActiveTab: (tab: SettingsTab) => void;
  onSaveProviderSettings: () => void;
  onSaveConfig: () => void;
  onReloadConfig: () => void;
};

const SETTINGS_TABS: SettingsTab[] = ["providers", "models", "roles", "security", "general"];

export function SettingsPage({
  activeSettingsTab,
  providers,
  providerTemplates,
  roles,
  selectedProviderId,
  providerSearch,
  configExists,
  onClose,
  onSetActiveTab,
  onSaveProviderSettings,
  onSaveConfig,
  onReloadConfig,
  setProviders,
  setRoles,
  setSelectedProviderId,
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
        <div className="settings-actions">
          <button onClick={onReloadConfig}>
            <RefreshCw size={16} />
            {t("actions.reload")}
          </button>
          <button
            className="primary"
            onClick={() =>
              activeSettingsTab === "providers" || activeSettingsTab === "roles"
                ? onSaveProviderSettings()
                : onSaveConfig()
            }
          >
            <Save size={16} />
            {t("actions.save")}
          </button>
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
          selectedProviderId={selectedProviderId}
          providerSearch={providerSearch}
          setProviders={setProviders}
          setSelectedProviderId={setSelectedProviderId}
          setProviderSearch={setProviderSearch}
        />
      ) : activeSettingsTab === "roles" ? (
        <RoleSettings providers={providers} roles={roles} setRoles={setRoles} />
      ) : (
        <div className="settings-placeholder">
          <h2>{t("settings.comingSoon")}</h2>
          <p>{t("settings.comingSoonDesc")}</p>
        </div>
      )}
    </section>
  );
}
